//! `torii remote` — platform-side repository management.

use crate::core::GitRepo;
use crate::remote::{get_platform_client, RepoFeatures, RepoSettings, Visibility};
use crate::ssh::SshHelper;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum RemoteCommands {
    /// Create a new remote repository on one or more platforms
    #[command(after_help = "Examples:
  torii remote create github myrepo                       User repo (your account)
  torii remote create github acme/widget                  Org repo: acme/widget
  torii remote create gitlab syrakon/svitrio-turso        GitLab group repo
  torii remote create gitlab engineering/web/api          GitLab subgroup repo
  torii remote create github,gitlab acme/myrepo --push    Same owner on both
  torii remote create github acme/myrepo --private --push

`<NAME>` accepts either `repo` (creates in your personal namespace) or
`owner/repo` (creates in the named org / group / subgroup). The
`--namespace <owner>` flag is the equivalent if you prefer keeping
NAME bare.")]
    Create {
        /// Platform (or comma-separated list): github, gitlab, codeberg, bitbucket, gitea, forgejo
        #[arg(value_delimiter = ',')]
        platforms: String,
        /// Repository name. Supports `repo` (personal) or `owner/repo`
        /// (organization / GitLab group / subgroup path). Slashes select
        /// the namespace.
        name: String,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(long)]
        public: bool,
        #[arg(long)]
        private: bool,
        #[arg(long)]
        push: bool,
        /// Override namespace explicitly. Equivalent to passing
        /// `<namespace>/<name>` as NAME. Useful when the repo name itself
        /// contains a slash you don't want parsed as a namespace.
        #[arg(long, value_name = "OWNER")]
        namespace: Option<String>,
    },
    /// Delete a remote repository on one or more platforms
    Delete {
        /// Platform (or comma-separated list)
        platforms: String,
        owner: String,
        repo: String,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    Visibility {
        platform: String,
        owner: String,
        repo: String,
        #[arg(long, conflicts_with = "private")]
        public: bool,
        #[arg(long, conflicts_with = "public")]
        private: bool,
    },
    Configure {
        platform: String,
        owner: String,
        repo: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        homepage: Option<String>,
        #[arg(long)]
        default_branch: Option<String>,
        #[arg(long)]
        enable_issues: bool,
        #[arg(long, conflicts_with = "enable_issues")]
        disable_issues: bool,
        #[arg(long)]
        enable_wiki: bool,
        #[arg(long, conflicts_with = "enable_wiki")]
        disable_wiki: bool,
        #[arg(long)]
        enable_projects: bool,
        #[arg(long, conflicts_with = "enable_projects")]
        disable_projects: bool,
    },
    Info {
        platform: String,
        owner: String,
        repo: String,
    },
    List {
        platform: String,
    },
    /// List remotes configured in the current repository
    Local,

    /// Link an existing remote repo to local (writes origin without touching the platform)
    #[command(after_help = "Examples:
  torii remote link github user/repo            Link via SSH (default)
  torii remote link gitlab user/repo --https    Link via HTTPS
  torii remote link --url git@host:owner/repo.git
  torii remote link my-fork github user/repo    Use a remote name other than 'origin'")]
    Link {
        /// Optional remote name (default: origin)
        #[arg(long, default_value = "origin")]
        name: String,

        /// Platform shortcut: github, gitlab, codeberg, bitbucket, gitea, forgejo, sourcehut
        platform: Option<String>,

        /// owner/repo on the platform
        repo: Option<String>,

        /// Use HTTPS instead of SSH
        #[arg(long)]
        https: bool,

        /// Provide a full URL directly (bypasses platform/repo)
        #[arg(long, value_name = "URL")]
        url: Option<String>,

        /// Replace existing remote with the same name
        #[arg(long)]
        force: bool,
    },

    /// Remove a local remote alias from .git/config — does NOT touch the
    /// platform. Inverse of `link`.
    #[command(after_help = "Examples:
  torii remote unlink origin           Drop the default origin alias
  torii remote unlink upstream         Drop a custom-named remote
  torii remote unlink old --yes        Skip confirmation prompt")]
    Unlink {
        /// Name of the local remote alias to remove (e.g. origin, upstream)
        name: String,

        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List refs the remote currently advertises (≡ `git ls-remote`).
    /// Hits the network — uses your configured auth.
    #[command(after_help = "Examples:
  torii remote refs origin              List all refs on origin
  torii remote refs origin --heads      Branch heads only
  torii remote refs origin --tags       Tags only
  torii remote refs https://...         Ad-hoc URL (no need to add as remote first)")]
    Refs {
        /// Local remote alias OR a full URL.
        target: String,
        /// Only print branch heads (`refs/heads/*`).
        #[arg(long)]
        heads: bool,
        /// Only print tag refs (`refs/tags/*`).
        #[arg(long)]
        tags: bool,
    },
}

pub(crate) fn run(action: &RemoteCommands) -> Result<()> {
    match action {
        RemoteCommands::Create {
            platforms,
            name,
            description,
            public,
            private: _,
            push,
            namespace,
        } => {
            let platforms: Vec<String> = platforms
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if platforms.is_empty() {
                anyhow::bail!("At least one platform is required");
            }
            let visibility = if *public {
                Visibility::Public
            } else {
                Visibility::Private
            };
            let multi = platforms.len() > 1;

            // Resolve namespace + repo name. Precedence:
            //   --namespace <owner> wins (NAME stays bare).
            //   else, last `/` in NAME splits owner/repo (GitLab
            //   subgroups stay in the owner segment, e.g.
            //   `engineering/web/api` → owner=`engineering/web`,
            //   repo=`api`).
            let (resolved_ns, resolved_name): (Option<String>, String) = match namespace {
                Some(ns) => (Some(ns.clone()), name.clone()),
                None => match name.rsplit_once('/') {
                    Some((owner, repo)) => (Some(owner.to_string()), repo.to_string()),
                    None => (None, name.clone()),
                },
            };

            let mut created: Vec<(String, crate::remote::RemoteRepo)> = Vec::new();
            for platform in &platforms {
                print!("🚀 {} - ", platform);
                match get_platform_client(platform) {
                    Ok(client) => match client.create_repo(
                        &resolved_name,
                        description.as_deref(),
                        visibility.clone(),
                        resolved_ns.as_deref(),
                    ) {
                        Ok(repo) => {
                            println!("✅ Created");
                            println!("   URL: {}", repo.url);
                            println!("   SSH: {}", repo.ssh_url);
                            created.push((platform.clone(), repo));
                        }
                        Err(e) => println!("❌ Failed: {}", e),
                    },
                    Err(e) => println!("❌ Platform error: {}", e),
                }
            }

            if multi {
                println!(
                    "\n📊 Created on {}/{} platforms",
                    created.len(),
                    platforms.len()
                );
            }

            if *push && !created.is_empty() {
                println!("\n📤 Linking remotes and pushing...");
                let git_repo = GitRepo::open(".")?;
                for (idx, (platform, repo)) in created.iter().enumerate() {
                    let remote_name = if !multi || idx == 0 {
                        "origin".to_string()
                    } else {
                        platform.clone()
                    };
                    let _ = git_repo.remote_add(&remote_name, &repo.ssh_url);
                }
                git_repo.push(false)?;
                println!("✅ Pushed");
            }
        }
        RemoteCommands::Delete {
            platforms,
            owner,
            repo,
            yes,
        } => {
            let platforms: Vec<String> = platforms
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if platforms.is_empty() {
                anyhow::bail!("At least one platform is required");
            }
            if !yes {
                println!("⚠️  Are you sure you want to delete {}/{} on {} platform(s)? This cannot be undone!", owner, repo, platforms.len());
                println!("   Run with --yes to confirm");
                return Ok(());
            }

            for platform in &platforms {
                print!("🗑️  {} - ", platform);
                match get_platform_client(platform) {
                    Ok(client) => match client.delete_repo(owner, repo) {
                        Ok(_) => println!("✅ Deleted"),
                        Err(e) => println!("❌ Failed: {}", e),
                    },
                    Err(e) => println!("❌ Platform error: {}", e),
                }
            }
            return Ok(());
        }
        RemoteCommands::Visibility {
            platform,
            owner,
            repo,
            public,
            private,
        } => {
            let client = get_platform_client(platform)?;

            let visibility = if *public {
                Visibility::Public
            } else if *private {
                Visibility::Private
            } else {
                println!("❌ Specify --public or --private");
                return Ok(());
            };

            println!(
                "🔒 Changing visibility of {}/{} to {:?}...",
                owner, repo, visibility
            );
            client.set_visibility(owner, repo, visibility)?;
            println!("✅ Visibility updated");
        }
        RemoteCommands::Configure {
            platform,
            owner,
            repo,
            description,
            homepage,
            default_branch,
            enable_issues,
            disable_issues,
            enable_wiki,
            disable_wiki,
            enable_projects,
            disable_projects,
        } => {
            let client = get_platform_client(platform)?;

            // Build settings
            let settings = RepoSettings {
                description: description.clone(),
                homepage: homepage.clone(),
                default_branch: default_branch.clone(),
                ..Default::default()
            };

            // Build features
            let mut features = RepoFeatures::default();
            if *enable_issues {
                features.issues = Some(true);
            }
            if *disable_issues {
                features.issues = Some(false);
            }
            if *enable_wiki {
                features.wiki = Some(true);
            }
            if *disable_wiki {
                features.wiki = Some(false);
            }
            if *enable_projects {
                features.projects = Some(true);
            }
            if *disable_projects {
                features.projects = Some(false);
            }

            println!("⚙️  Configuring repository {}/{}...", owner, repo);

            // Update settings if any
            if settings.description.is_some()
                || settings.homepage.is_some()
                || settings.default_branch.is_some()
            {
                client.update_repo(owner, repo, settings)?;
            }

            // Update features if any
            if features.issues.is_some() || features.wiki.is_some() || features.projects.is_some() {
                client.configure_features(owner, repo, features)?;
            }

            println!("✅ Repository configured");
        }
        RemoteCommands::Info {
            platform,
            owner,
            repo,
        } => {
            let client = get_platform_client(platform)?;
            println!("📊 Fetching repository information...");
            let repo_info = client.get_repo(owner, repo)?;

            println!("\n📦 Repository: {}", repo_info.name);
            if let Some(desc) = &repo_info.description {
                println!("   Description: {}", desc);
            }
            println!("   Visibility: {:?}", repo_info.visibility);
            println!("   Default Branch: {}", repo_info.default_branch);
            println!("   URL: {}", repo_info.url);
            println!("   SSH: {}", repo_info.ssh_url);
        }
        RemoteCommands::Local => {
            let repo = GitRepo::open(".")?;
            let remotes = repo.remotes()?;
            if remotes.is_empty() {
                println!("No remotes configured");
            } else {
                for (name, url) in &remotes {
                    println!("  {}  {}", name, url.as_deref().unwrap_or("(no url)"));
                }
            }
        }
        RemoteCommands::Link {
            name,
            platform,
            repo,
            https,
            url,
            force,
        } => {
            let resolved_url = if let Some(u) = url {
                u.clone()
            } else {
                let plat = platform.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Provide --url <URL> or <platform> <owner>/<repo>")
                })?;
                let owner_repo = repo
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Missing <owner>/<repo>"))?;
                let (ssh_host, https_host) = match plat {
                                "github"    => ("github.com", "github.com"),
                                "gitlab"    => ("gitlab.com", "gitlab.com"),
                                "codeberg"  => ("codeberg.org", "codeberg.org"),
                                "bitbucket" => ("bitbucket.org", "bitbucket.org"),
                                "gitea"     => ("gitea.com", "gitea.com"),
                                "forgejo"   => ("codeberg.org", "codeberg.org"),
                                "sourcehut" => ("git.sr.ht", "git.sr.ht"),
                                _ => anyhow::bail!(
                                    "Unknown platform '{}'. Supported: github, gitlab, codeberg, bitbucket, gitea, forgejo, sourcehut",
                                    plat
                                ),
                            };
                let use_ssh = if *https {
                    false
                } else {
                    SshHelper::has_ssh_keys()
                };
                if use_ssh {
                    format!("git@{}:{}.git", ssh_host, owner_repo)
                } else {
                    format!("https://{}/{}.git", https_host, owner_repo)
                }
            };

            let git_repo = GitRepo::open(".")?;
            if git_repo.remote_exists(name) {
                if !*force {
                    anyhow::bail!(
                                    "Remote '{}' already exists. Use --force to overwrite, or 'torii remote local' to inspect.",
                                    name
                                );
                }
                git_repo.remote_set_url(name, &resolved_url)?;
                println!("🔗 Updated remote '{}' → {}", name, resolved_url);
            } else {
                git_repo.remote_add(name, &resolved_url)?;
                println!("🔗 Linked remote '{}' → {}", name, resolved_url);
            }
        }
        RemoteCommands::Unlink { name, yes } => {
            let git_repo = GitRepo::open(".")?;
            let url = git_repo
                .remote_url(name)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "No local remote named '{}'. Run `torii remote local` to list.",
                        name
                    )
                })?
                .unwrap_or_else(|| "(no url)".to_string());

            if !*yes {
                use std::io::{BufRead, Write};
                println!("⚠️  Drop local alias '{}' → {}?", name, url);
                println!("   (Does NOT touch the remote on the platform.)");
                print!("   Confirm [y/N]: ");
                std::io::stdout().flush().ok();
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                let ans = line.trim().to_ascii_lowercase();
                if !matches!(ans.as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            git_repo
                .remote_delete(name)
                .map_err(|e| anyhow::anyhow!("delete remote '{}': {}", name, e))?;
            println!("🔗 Unlinked local remote '{}' (platform untouched)", name);
        }
        RemoteCommands::List { platform } => {
            let client = get_platform_client(platform)?;
            println!("📋 Fetching repositories from {}...", platform);
            let repos = client.list_repos()?;

            if repos.is_empty() {
                println!("No repositories found");
            } else {
                println!("\n📦 Repositories ({}):\n", repos.len());
                for repo in repos {
                    println!("  • {} - {:?}", repo.name, repo.visibility);
                    if let Some(desc) = &repo.description {
                        println!("    {}", desc);
                    }
                }
            }
        }
        RemoteCommands::Refs {
            target,
            heads,
            tags,
        } => {
            // Resolve target — local remote alias or URL.
            let repo = git2::Repository::open(".")?;
            let mut remote = match repo.find_remote(target) {
                Ok(r) => r,
                Err(_) => repo.remote_anonymous(target)?,
            };
            // Connect read-only with default auth callbacks.
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.credentials(|url, user, allowed| {
                // SSH agent first (most common); fall back to
                // userpass-plaintext nothing — let libgit2 fail
                // cleanly so the user knows to set up auth.
                if allowed.contains(git2::CredentialType::SSH_KEY) {
                    git2::Cred::ssh_key_from_agent(user.unwrap_or("git"))
                } else {
                    Err(git2::Error::from_str(&format!(
                        "no credentials available for {url}"
                    )))
                }
            });
            remote.connect_auth(git2::Direction::Fetch, Some(callbacks), None)?;
            let list = remote.list()?;
            for head in list {
                let name = head.name();
                let keep = match (*heads, *tags) {
                    (true, false) => name.starts_with("refs/heads/"),
                    (false, true) => name.starts_with("refs/tags/"),
                    _ => true,
                };
                if keep {
                    println!("{}\t{}", head.oid(), name);
                }
            }
        }
    }
    Ok(())
}
