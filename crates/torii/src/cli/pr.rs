//! `torii pr` — pull / merge requests.

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum PrCommands {
    /// List pull requests
    List {
        /// State: open, closed, merged, all (default: open)
        #[arg(long, default_value = "open")]
        state: String,
    },
    /// Create a pull request
    Create {
        /// PR title
        #[arg(short, long)]
        title: String,
        /// Base branch (default: main)
        #[arg(short, long, default_value = "main")]
        base: String,
        /// Head branch (default: current branch)
        #[arg(long)]
        head: Option<String>,
        /// PR description
        #[arg(short, long)]
        description: Option<String>,
        /// Mark as draft
        #[arg(long)]
        draft: bool,
    },
    /// Merge a pull request
    Merge {
        /// PR number
        number: u64,
        /// Merge method: merge, squash, rebase (default: merge)
        #[arg(long, default_value = "merge")]
        method: String,
    },
    /// Close a pull request
    Close {
        /// PR number
        number: u64,
    },
    /// Checkout the branch of a pull request
    Checkout {
        /// PR number
        number: u64,
    },
    /// Open a pull request in the browser
    Open {
        /// PR number
        number: u64,
    },
}

pub(crate) fn run(action: &PrCommands) -> Result<()> {
    use crate::pr::{detect_platform_from_remote, get_pr_client, CreatePrOptions, MergeMethod};
    let repo_path = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .to_string_lossy()
        .to_string();
    let (platform, owner, repo_name) =
        detect_platform_from_remote(&repo_path).ok_or_else(|| {
            crate::error::ToriiError::InvalidConfig(
                "Could not detect platform from remote. Is 'origin' set to a GitHub/GitLab URL?"
                    .to_string(),
            )
        })?;
    let client = get_pr_client(&platform)?;
    match action {
        PrCommands::List { state } => {
            let prs = client.list(&owner, &repo_name, state)?;
            if prs.is_empty() {
                println!("No {} pull requests.", state);
            } else {
                for pr in &prs {
                    let draft = if pr.draft { " [draft]" } else { "" };
                    let merge = match pr.mergeable {
                        Some(true) => " ✓",
                        Some(false) => " ✗",
                        None => "",
                    };
                    println!("#{:<5} {}{}{}", pr.number, pr.title, draft, merge);
                    println!(
                        "       {} → {}  by {}  {}",
                        pr.head, pr.base, pr.author, pr.created_at
                    );
                    println!("       {}", pr.url);
                    println!();
                }
            }
        }
        PrCommands::Create {
            title,
            base,
            head,
            description,
            draft,
        } => {
            let head_branch = if let Some(h) = head {
                h.clone()
            } else {
                let repo = git2::Repository::discover(&repo_path)
                    .map_err(crate::error::ToriiError::Git)?;
                repo.head()
                    .ok()
                    .and_then(|h| h.shorthand().map(|s| s.to_string()))
                    .unwrap_or_else(|| "HEAD".to_string())
            };
            let opts = CreatePrOptions {
                title: title.clone(),
                body: description.clone(),
                head: head_branch,
                base: base.clone(),
                draft: *draft,
            };
            let pr = client.create(&owner, &repo_name, opts)?;
            println!("Created PR #{}: {}", pr.number, pr.title);
            println!("{}", pr.url);
        }
        PrCommands::Merge { number, method } => {
            let merge_method = match method.as_str() {
                "squash" => MergeMethod::Squash,
                "rebase" => MergeMethod::Rebase,
                _ => MergeMethod::Merge,
            };
            client.merge(&owner, &repo_name, *number, merge_method)?;
            println!("Merged PR #{}", number);
        }
        PrCommands::Close { number } => {
            client.close(&owner, &repo_name, *number)?;
            println!("Closed PR #{}", number);
        }
        PrCommands::Checkout { number } => {
            let pr = client.get(&owner, &repo_name, *number)?;
            let branch = client.checkout_branch(&pr);
            let status = std::process::Command::new("torii")
                .args(["branch", &branch])
                .status();
            match status {
                Ok(s) if s.success() => println!("Checked out branch: {}", branch),
                _ => eprintln!("Failed to checkout branch: {}", branch),
            }
        }
        PrCommands::Open { number } => {
            let pr = client.get(&owner, &repo_name, *number)?;
            let _ = std::process::Command::new("xdg-open")
                .arg(&pr.url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            println!("Opening: {}", pr.url);
        }
    }
    Ok(())
}
