//! `torii issue` — issue management.

use crate::issue::{get_issue_client, CreateIssueOptions};
use crate::pr::detect_platform_from_remote;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum IssueCommands {
    /// List issues
    List {
        #[arg(long, default_value = "open")]
        state: String,
    },
    /// Create an issue
    Create {
        #[arg(short, long)]
        title: String,
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// Close an issue
    Close { number: u64 },
    /// Add a comment to an issue
    Comment {
        number: u64,
        #[arg(short, long)]
        message: String,
    },
}

pub(crate) fn run(action: &IssueCommands) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name) = detect_platform_from_remote(&repo_path)
        .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote origin"))?;
    let client = get_issue_client(&platform)?;
    match action {
        IssueCommands::List { state } => {
            let issues = client.list(&owner, &repo_name, &state)?;
            if issues.is_empty() {
                println!("No {} issues.", state);
            } else {
                for i in &issues {
                    let labels = if i.labels.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", i.labels.join(", "))
                    };
                    let comments = if i.comments > 0 {
                        format!(" 💬{}", i.comments)
                    } else {
                        String::new()
                    };
                    println!("#{:<6} {}{}{}", i.number, i.title, labels, comments);
                    println!(
                        "       {} → {}  by {}  {}",
                        i.state,
                        i.url,
                        i.author,
                        &i.created_at[..10]
                    );
                }
            }
        }
        IssueCommands::Create { title, description } => {
            let opts = CreateIssueOptions {
                title: title.clone(),
                body: description.clone(),
            };
            let issue = client.create(&owner, &repo_name, opts)?;
            println!("Created issue #{}: {}", issue.number, issue.title);
            println!("{}", issue.url);
        }
        IssueCommands::Close { number } => {
            client.close(&owner, &repo_name, *number)?;
            println!("✅ Closed issue #{}", number);
        }
        IssueCommands::Comment { number, message } => {
            client.comment(&owner, &repo_name, *number, message)?;
            println!("✅ Comment added to issue #{}", number);
        }
    }
    Ok(())
}
