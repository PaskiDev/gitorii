//! `torii snapshot` — WIP snapshots and stash.

use crate::snapshot::SnapshotManager;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum SnapshotCommands {
    /// Create a new snapshot
    Create {
        /// Optional snapshot name/description
        #[arg(short, long)]
        name: Option<String>,
    },

    /// List all snapshots
    List,

    /// Restore from a snapshot
    Restore {
        /// Snapshot ID to restore
        id: String,
    },

    /// Delete a snapshot
    Delete {
        /// Snapshot ID to delete
        id: String,
    },

    /// Auto-snapshot configuration
    Config {
        /// Enable auto-snapshots
        #[arg(long)]
        enable: bool,

        /// Snapshot interval (e.g., 1h, 30m)
        #[arg(long)]
        interval: Option<String>,
    },

    /// Save work temporarily (like git stash)
    Stash {
        /// Name for the stash
        #[arg(short, long)]
        name: Option<String>,

        /// Include untracked files
        #[arg(short = 'u', long)]
        include_untracked: bool,
    },

    /// Restore stashed work
    Unstash {
        /// Stash ID to restore (latest if not specified)
        id: Option<String>,

        /// Keep the stash after restoring
        #[arg(short, long)]
        keep: bool,
    },

    /// `git stash apply` alias — restore without dropping the stash.
    /// Equivalent to `torii snapshot unstash --keep [<id>]`.
    Apply {
        /// Snapshot/stash ID (latest if not specified).
        id: Option<String>,
    },

    /// `git stash pop` alias — restore and drop the stash.
    /// Equivalent to `torii snapshot unstash [<id>]`.
    Pop {
        /// Snapshot/stash ID (latest if not specified).
        id: Option<String>,
    },

    /// `git stash drop` alias — delete a specific snapshot.
    /// Equivalent to `torii snapshot delete <id>`.
    Drop {
        /// Snapshot/stash ID to drop.
        id: String,
    },

    /// Delete every snapshot/stash in this repo. Asks for confirmation
    /// unless `--yes` is given.
    Clear {
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show what's inside a snapshot — branch, commit, timestamp,
    /// and a list of files captured.
    Show {
        /// Snapshot/stash ID.
        id: String,
    },

    /// Undo last operation
    Undo,
}

pub(crate) fn run(action: &SnapshotCommands) -> Result<()> {
    let snapshot_mgr = SnapshotManager::new(".")?;
    match action {
        SnapshotCommands::Create { name } => {
            let snapshot_id = snapshot_mgr.create_snapshot(name.as_deref())?;
            println!("✅ Snapshot created: {}", snapshot_id);
        }
        SnapshotCommands::List => {
            snapshot_mgr.list_snapshots()?;
        }
        SnapshotCommands::Restore { id } => {
            snapshot_mgr.restore_snapshot(id)?;
            println!("✅ Restored snapshot: {}", id);
        }
        SnapshotCommands::Delete { id } => {
            snapshot_mgr.delete_snapshot(id)?;
            println!("✅ Deleted snapshot: {}", id);
        }
        SnapshotCommands::Config { enable, interval } => {
            let interval_minutes = interval.as_ref().and_then(|s| s.parse::<u32>().ok());
            snapshot_mgr.configure_auto_snapshot(*enable, interval_minutes)?;
            println!("✅ Auto-snapshot configuration updated");
        }
        SnapshotCommands::Stash {
            name,
            include_untracked,
        } => {
            snapshot_mgr.stash(name.as_deref(), *include_untracked)?;
        }
        SnapshotCommands::Unstash { id, keep } => {
            snapshot_mgr.unstash(id.as_deref(), *keep)?;
        }
        SnapshotCommands::Apply { id } => {
            snapshot_mgr.unstash(id.as_deref(), true)?;
        }
        SnapshotCommands::Pop { id } => {
            snapshot_mgr.unstash(id.as_deref(), false)?;
        }
        SnapshotCommands::Drop { id } => {
            snapshot_mgr.delete_snapshot(id)?;
            println!("✅ Dropped snapshot: {}", id);
        }
        SnapshotCommands::Clear { yes } => {
            if !*yes {
                use std::io::{self, BufRead, IsTerminal, Write};
                if !io::stdin().is_terminal() {
                    anyhow::bail!("Refusing to clear without --yes when there's no tty to prompt.");
                }
                print!("⚠  Delete ALL snapshots in this repo? [y/N] ");
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin().lock().read_line(&mut line)?;
                if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let count = snapshot_mgr.clear_all()?;
            println!("🧹 Cleared {count} snapshot(s).");
        }
        SnapshotCommands::Show { id } => {
            snapshot_mgr.show(id)?;
        }
        SnapshotCommands::Undo => {
            snapshot_mgr.undo()?;
        }
    }
    Ok(())
}
