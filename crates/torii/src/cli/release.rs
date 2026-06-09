//! `torii release` — release pages.

use crate::pr::detect_platform_from_remote_named;
use crate::release::get_release_client;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum ReleaseCommands {
    /// List recent releases
    List {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show one release's full details (description, web URL, etc.)
    Show { tag: String },
    /// Edit release metadata. Pass `--name` and/or `--notes`.
    Edit {
        tag: String,
        /// New release name/title.
        #[arg(long)]
        name: Option<String>,
        /// Path to a markdown file with the new description. Use `-` for stdin.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Delete the release entity (leaves the tag intact).
    Delete {
        tag: String,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub(crate) fn run(action: &ReleaseCommands, remote: &String) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
        .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
    let client = get_release_client(&platform)?;
    match action {
        ReleaseCommands::List { limit } => {
            let rels = client.list(&owner, &repo_name, *limit)?;
            if rels.is_empty() {
                println!("No releases found.");
            } else {
                println!("{:<14} {:<32} CREATED", "TAG", "NAME");
                for r in &rels {
                    let created = r.created_at.get(..10).unwrap_or(&r.created_at);
                    println!("{:<14} {:<32} {}", r.tag, r.name, created);
                }
            }
        }
        ReleaseCommands::Show { tag } => {
            let r = client.get(&owner, &repo_name, tag)?;
            println!("Tag:         {}", r.tag);
            println!("Name:        {}", r.name);
            println!("Created:     {}", r.created_at);
            if !r.web_url.is_empty() {
                println!("URL:         {}", r.web_url);
            }
            if let Some(id) = &r.id {
                println!("ID:          {}", id);
            }
            println!("\n--- Description ---\n{}", r.description);
        }
        ReleaseCommands::Edit { tag, name, notes } => {
            // Resolve `--notes` source: file path, `-` for stdin, or absent.
            let body =
                match notes.as_deref() {
                    Some("-") => {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf)?;
                        Some(buf)
                    }
                    Some(path) => Some(std::fs::read_to_string(path).map_err(|e| {
                        anyhow::anyhow!("Failed to read notes file {}: {}", path, e)
                    })?),
                    None => None,
                };
            client.edit(&owner, &repo_name, tag, name.as_deref(), body.as_deref())?;
            println!("✅ Edited release {}", tag);
        }
        ReleaseCommands::Delete { tag, yes } => {
            if !*yes {
                print!(
                    "Delete release {} (tag stays, only the release entity is removed)? [y/N] ",
                    tag
                );
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Cancelled.");
                    return Ok(());
                }
            }
            client.delete(&owner, &repo_name, tag)?;
            println!("✅ Deleted release {}", tag);
        }
    }
    Ok(())
}
