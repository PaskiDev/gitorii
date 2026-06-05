//! `torii workspace` — multi-repo workspaces.

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum WorkspaceCommands {
    /// Add a repository to a workspace
    Add {
        /// Workspace name
        workspace: String,
        /// Repository path
        path: String,
    },
    /// Remove a repository from a workspace
    Remove {
        /// Workspace name
        workspace: String,
        /// Repository path
        path: String,
    },
    /// Delete a workspace entirely
    Delete {
        /// Workspace name
        workspace: String,
    },
    /// List all workspaces and their repos
    List,
    /// Show git status across all repos in a workspace
    Status {
        /// Workspace name
        workspace: String,
    },
    /// Commit changes across all repos in a workspace
    Save {
        /// Workspace name
        workspace: String,
        /// Commit message
        #[arg(short, long)]
        message: String,
        /// Stage all changes before committing
        #[arg(short, long)]
        all: bool,
    },
    /// Pull and push all repos in a workspace
    Sync {
        /// Workspace name
        workspace: String,
        /// Force push
        #[arg(long)]
        force: bool,
    },
}

pub(crate) fn run(action: &WorkspaceCommands) -> Result<()> {
    use crate::workspace::workspace::{SaveOutcome, WorkspaceManager};
    match action {
        WorkspaceCommands::Add { workspace, path } => {
            let expanded = WorkspaceManager::add(workspace, path)?;
            println!(
                "✅ Added {} to workspace '{}'",
                expanded.display(),
                workspace
            );
        }
        WorkspaceCommands::Remove { workspace, path } => {
            WorkspaceManager::remove(workspace, path)?;
            println!("✅ Removed {} from workspace '{}'", path, workspace);
        }
        WorkspaceCommands::Delete { workspace } => {
            WorkspaceManager::delete(workspace)?;
            println!("✅ Deleted workspace '{}'", workspace);
        }
        WorkspaceCommands::List => {
            let workspaces = WorkspaceManager::list()?;
            if workspaces.is_empty() {
                println!("No workspaces configured.");
                println!("Add one: torii workspace add <name> <path>");
            }
            for ws in &workspaces {
                println!("📦 {}", ws.name);
                for repo in &ws.repos {
                    let icon = if repo.exists { "  ✓" } else { "  ✗" };
                    println!("{} {}", icon, repo.path);
                }
                println!();
            }
        }
        WorkspaceCommands::Status { workspace } => {
            let statuses = WorkspaceManager::status(workspace)?;
            println!("📦 {}", workspace);
            println!();
            for s in &statuses {
                if let Some(err) = &s.error {
                    println!("  {:<20} ❌ {}", s.name, err);
                    continue;
                }
                let changes = s.staged + s.unstaged + s.untracked;
                if changes == 0 {
                    println!("  {:<20} ✅ clean        ({})", s.name, s.branch);
                } else {
                    let mut parts = vec![];
                    if s.staged > 0 {
                        parts.push(format!("{} staged", s.staged));
                    }
                    if s.unstaged > 0 {
                        parts.push(format!("{} modified", s.unstaged));
                    }
                    if s.untracked > 0 {
                        parts.push(format!("{} untracked", s.untracked));
                    }
                    println!("  {:<20} 📝 {}  ({})", s.name, parts.join(", "), s.branch);
                }
                if s.ahead > 0 || s.behind > 0 {
                    println!("  {:<20}    ↑{} ahead, ↓{} behind", "", s.ahead, s.behind);
                }
            }
            println!();
        }
        WorkspaceCommands::Save {
            workspace,
            message,
            all,
        } => {
            println!("📦 {} — saving", workspace);
            println!();
            let results = WorkspaceManager::save(workspace, message, *all, |r| match &r.outcome {
                SaveOutcome::Saved => println!("  {} ✅ saved", r.name),
                SaveOutcome::NoChanges => println!("  {} — no changes", r.name),
                SaveOutcome::Failed(e) => println!("  {} ❌ {}", r.name, e),
            })?;
            let committed = results
                .iter()
                .filter(|r| matches!(r.outcome, SaveOutcome::Saved))
                .count();
            let skipped = results
                .iter()
                .filter(|r| matches!(r.outcome, SaveOutcome::NoChanges))
                .count();
            println!();
            println!("{} committed, {} skipped", committed, skipped);
        }
        WorkspaceCommands::Sync { workspace, force } => {
            println!("📦 {} — syncing", workspace);
            println!();
            let results = WorkspaceManager::sync(workspace, *force, |r| match &r.error {
                None => println!("  {} ✅ synced", r.name),
                Some(e) => println!("  {} ❌ {}", r.name, e),
            })?;
            let ok = results.iter().filter(|r| r.error.is_none()).count();
            let failed = results.iter().filter(|r| r.error.is_some()).count();
            println!();
            println!("{} synced, {} failed", ok, failed);
        }
    }
    Ok(())
}
