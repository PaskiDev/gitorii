//! `torii package` — package registry.

use crate::package::{
    filter_by_version as pkg_filter_by_version, filter_older_than as pkg_filter_older_than,
    get_package_client, PackageListFilters,
};
use crate::pr::detect_platform_from_remote_named;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum PackageCommands {
    /// List packages in the project registry
    List {
        /// Filter by package type (e.g. "generic")
        #[arg(long = "type")]
        package_type: Option<String>,
        /// Substring search on package name
        #[arg(long)]
        name: Option<String>,
        /// Max entries to return (1..=100)
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List files inside a package
    Files { id: String },
    /// Delete one package (`<id>`) or many (use `--version` / `--older-than`).
    Delete {
        /// Explicit package id. Mutually exclusive with the filter flags.
        id: Option<String>,
        /// Delete every package matching this version
        #[arg(long, conflicts_with = "id")]
        version: Option<String>,
        /// Delete only packages older than this duration (e.g. `90d`, `7d`)
        #[arg(long, conflicts_with = "id")]
        older_than: Option<String>,
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub(crate) fn run(action: &PackageCommands, remote: &String) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
        .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
    let client = get_package_client(&platform)?;
    match action {
        PackageCommands::List {
            package_type,
            name,
            limit,
        } => {
            let filters = PackageListFilters {
                package_type: package_type.clone(),
                name_search: name.clone(),
                per_page: *limit,
            };
            let packages = client.list(&owner, &repo_name, &filters)?;
            if packages.is_empty() {
                println!("No packages found.");
            } else {
                println!(
                    "{:<8} {:<20} {:<16} {:<10} {}",
                    "ID", "NAME", "VERSION", "TYPE", "CREATED"
                );
                for p in &packages {
                    let created = p.created_at.get(..10).unwrap_or(&p.created_at);
                    println!(
                        "{:<8} {:<20} {:<16} {:<10} {}",
                        p.id, p.name, p.version, p.package_type, created
                    );
                }
            }
        }
        PackageCommands::Files { id } => {
            let files = client.list_files(&owner, &repo_name, id)?;
            if files.is_empty() {
                println!("No files in package {}.", id);
            } else {
                println!("{:<10} {:<40} {}", "FILE-ID", "NAME", "SIZE");
                for f in &files {
                    let size_mb = f.size_bytes as f64 / 1_048_576.0;
                    println!("{:<10} {:<40} {:.2} MB", f.id, f.file_name, size_mb);
                }
            }
        }
        PackageCommands::Delete {
            id,
            version,
            older_than,
            yes,
        } => {
            if let Some(pid) = id {
                if !*yes {
                    print!("Delete package {}? [y/N] ", pid);
                    use std::io::Write;
                    std::io::stdout().flush()?;
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("❌ Cancelled.");
                        return Ok(());
                    }
                }
                client.delete(&owner, &repo_name, pid)?;
                println!("✅ Deleted package {}", pid);
                return Ok(());
            }
            if version.is_none() && older_than.is_none() {
                anyhow::bail!(
                                "`package delete` needs either an explicit id or at least one of --version / --older-than"
                            );
            }
            // List ALL packages (no API-side version filter needed,
            // we narrow client-side for predictable semantics).
            let filters = PackageListFilters {
                per_page: 100,
                ..PackageListFilters::default()
            };
            let mut targets = client.list(&owner, &repo_name, &filters)?;
            if let Some(v) = version {
                targets = pkg_filter_by_version(targets, v);
            }
            if let Some(d) = older_than.as_deref() {
                let mins = crate::duration::parse_duration(d)? as i64;
                let days = std::cmp::max(1, mins / (60 * 24));
                targets = pkg_filter_older_than(targets, days);
            }
            if targets.is_empty() {
                println!("No packages matched the filter.");
                return Ok(());
            }
            if !*yes {
                println!("Will delete {} package(s):", targets.len());
                for p in targets.iter().take(10) {
                    println!(
                        "  {} {} {} {} {}",
                        p.id,
                        p.name,
                        p.version,
                        p.package_type,
                        &p.created_at[..p.created_at.len().min(10)]
                    );
                }
                if targets.len() > 10 {
                    println!("  ... and {} more", targets.len() - 10);
                }
                print!("Proceed? [y/N] ");
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Cancelled.");
                    return Ok(());
                }
            }
            let mut ok = 0usize;
            let mut failed: Vec<(String, String)> = Vec::new();
            for p in &targets {
                match client.delete(&owner, &repo_name, &p.id) {
                    Ok(_) => {
                        ok += 1;
                        println!("  ✅ {} {} {}", p.id, p.name, p.version);
                    }
                    Err(e) => {
                        failed.push((p.id.clone(), e.to_string()));
                        println!("  ❌ {}: {}", p.id, e);
                    }
                }
            }
            println!("Done: {} deleted, {} failed.", ok, failed.len());
            if !failed.is_empty() {
                anyhow::bail!("{} package(s) could not be deleted", failed.len());
            }
        }
    }
    Ok(())
}
