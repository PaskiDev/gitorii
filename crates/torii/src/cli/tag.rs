//! `torii tag` — tag and release management.

use crate::core::GitRepo;
use crate::versioning::AutoTagger;
use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum TagCommands {
    /// Create a new tag (or auto-bump the next release tag with --release)
    Create {
        /// Tag name (omit when using --release)
        name: Option<String>,

        /// Tag message (creates annotated tag)
        #[arg(short, long)]
        message: Option<String>,

        /// Auto-bump the next version from conventional commits since last tag
        #[arg(long)]
        release: bool,

        /// Force a specific bump (used with --release): major, minor, patch
        #[arg(long, requires = "release")]
        bump: Option<String>,

        /// Preview the next version without creating the tag (used with --release)
        #[arg(long, requires = "release")]
        dry_run: bool,
    },

    /// List all tags
    List,

    /// Delete a tag
    Delete {
        /// Tag name to delete
        name: String,
    },

    /// Push tags to remote
    Push {
        /// Specific tag to push (all if not specified)
        name: Option<String>,

        /// Force-push tags even when the remote ref already exists at a
        /// different commit (rewrites remote tag history).
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Show tag details
    Show {
        /// Tag name
        name: String,
    },
}

pub(crate) fn run(action: &TagCommands) -> Result<()> {
    let repo = GitRepo::open(".")?;
    match action {
        TagCommands::Create {
            name,
            message,
            release,
            bump,
            dry_run,
        } => {
            if *release {
                let tagger = AutoTagger::new(repo);
                let current = tagger.get_latest_version()?;

                let next = if let Some(bump_str) = bump {
                    use crate::versioning::semver::VersionBump;
                    let b = match bump_str.as_str() {
                        "major" => VersionBump::Major,
                        "minor" => VersionBump::Minor,
                        "patch" => VersionBump::Patch,
                        _ => anyhow::bail!("Invalid bump: use major, minor or patch"),
                    };
                    let base = current
                        .clone()
                        .unwrap_or_else(crate::versioning::semver::Version::initial);
                    base.bump(b)
                } else {
                    tagger.calculate_next_version_from_log()?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "No releasable commits found since last tag (need feat: or fix:)"
                        )
                    })?
                };

                println!(
                    "📦 Current version: {}",
                    current
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "none".to_string())
                );
                println!("🚀 Next version:    v{}", next);

                if *dry_run {
                    println!("   (dry run — no tag created)");
                } else {
                    tagger.create_tag(&next, &format!("Release v{}", next))?;
                    println!("💡 Push with: torii sync --push");
                }
            } else {
                let tag_name = name.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Tag name required (or use --release to auto-bump)")
                })?;
                repo.create_tag(tag_name, message.as_deref())?;
                println!("✅ Tag created: {}", tag_name);
            }
        }
        TagCommands::List => {
            repo.list_tags()?;
        }
        TagCommands::Delete { name } => {
            repo.delete_tag(name)?;
            println!("✅ Tag deleted: {}", name);
        }
        TagCommands::Push { name, force } => {
            repo.push_tags(name.as_deref(), *force)?;
            let force_note = if *force { " (force)" } else { "" };
            if let Some(tag) = name {
                println!("✅ Pushed tag: {}{}", tag, force_note);
            } else {
                println!("✅ Pushed all tags{}", force_note);
            }
        }
        TagCommands::Show { name } => {
            repo.show_tag(name)?;
        }
    }
    Ok(())
}
