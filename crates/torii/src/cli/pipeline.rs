//! `torii pipeline` / `torii job` — CI pipelines and jobs.

use crate::pipeline::{filter_older_than, get_pipeline_client, ListFilters};
use crate::pr::detect_platform_from_remote_named;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum PipelineCommands {
    /// List recent pipelines on the auto-detected platform
    List {
        /// Filter by normalized status: success|failed|running|canceled|pending
        #[arg(long)]
        status: Option<String>,
        /// Max entries to return (clamped to [1, 100]).
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Cancel a running pipeline by id
    Cancel { id: String },
    /// Retry a pipeline (re-run failed/canceled jobs) by id
    Retry { id: String },
    /// Delete one pipeline (`<id>`) or many (use `--status` / `--older-than`).
    /// Requires either an explicit id or at least one filter — never both.
    Delete {
        /// Explicit pipeline id. Mutually exclusive with the filter flags.
        id: Option<String>,
        /// Delete every pipeline matching this normalized status. Required
        /// (alongside or instead of `--older-than`) when no id is given.
        #[arg(long, conflicts_with = "id")]
        status: Option<String>,
        /// Delete only pipelines older than this duration (e.g. `7d`,
        /// `12h`, `30m`). Combine with `--status` to narrow further.
        #[arg(long, conflicts_with = "id")]
        older_than: Option<String>,
        /// Skip the confirmation prompt. Required for non-interactive use.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum JobCommands {
    /// List jobs in a pipeline / workflow run
    List {
        /// Pipeline (GitLab) / workflow-run (GitHub) id to list jobs from.
        #[arg(long)]
        pipeline: String,
        /// Optional status filter: success|failed|running|canceled|pending|other
        #[arg(long)]
        status: Option<String>,
    },
    /// Print a job's log/trace. The killer feature — replaces
    /// "open browser, find job, click logs, scroll".
    Log {
        id: String,
        /// Show only the last N lines of the log (good for failure
        /// post-mortems, since the actual error usually lives at the tail).
        #[arg(long)]
        tail: Option<usize>,
    },
    /// Re-run a single job. GitLab only — GitHub Actions doesn't support
    /// per-job retry; use `torii pipeline retry <run-id>` there.
    Retry { id: String },
    /// Cancel a running job. GitLab only — GitHub Actions cancels at the
    /// workflow-run level; use `torii pipeline cancel <run-id>`.
    Cancel { id: String },
    /// Download a job's artifacts archive to a local path. GitLab only —
    /// GitHub artifacts are scoped to the workflow run, not the job.
    Artifacts {
        id: String,
        /// Output path for the artifacts zip. Defaults to `./<job-id>-artifacts.zip`.
        #[arg(short = 'o', long)]
        output: Option<String>,
    },
    /// Erase a job's log + artifacts but keep the job entry in the UI.
    /// GitLab only.
    Erase {
        id: String,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub(crate) fn run(action: &PipelineCommands, remote: &String) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name, api_base_url) =
        crate::pr::detect_platform_full(&repo_path, remote).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not detect platform from remote `{}`. \
                         Check the remote exists (`torii remote local`) and points to a known \
                         platform, or add one via `torii platforms add`.",
                remote
            )
        })?;
    let client =
        crate::pipeline::get_pipeline_client_with_base_url(&platform, Some(&api_base_url))?;
    match action {
        PipelineCommands::List { status, limit } => {
            let filters = ListFilters {
                status: status.clone(),
                per_page: *limit,
            };
            let pipelines = client.list(&owner, &repo_name, &filters)?;
            if pipelines.is_empty() {
                println!("No pipelines found.");
            } else {
                println!(
                    "{:<12} {:<12} {:<24} {:<10} CREATED",
                    "ID", "STATUS", "BRANCH", "SHA"
                );
                for p in &pipelines {
                    let icon = match p.status.as_str() {
                        "success" => "✅",
                        "failed" => "❌",
                        "running" => "🟡",
                        "canceled" => "⚪",
                        "pending" => "⏳",
                        _ => "·",
                    };
                    let sha_short = p.sha.chars().take(8).collect::<String>();
                    let created = p.created_at.get(..10).unwrap_or(&p.created_at);
                    println!(
                        "{:<12} {} {:<10} {:<24} {:<10} {}",
                        p.id, icon, p.raw_status, p.branch, sha_short, created
                    );
                }
            }
        }
        PipelineCommands::Cancel { id } => {
            client.cancel(&owner, &repo_name, id)?;
            println!("✅ Cancelled pipeline {}", id);
        }
        PipelineCommands::Retry { id } => {
            client.retry(&owner, &repo_name, id)?;
            println!("✅ Retried pipeline {}", id);
        }
        PipelineCommands::Delete {
            id,
            status,
            older_than,
            yes,
        } => {
            // Two modes:
            //   1. Explicit id → delete that one.
            //   2. Filter-driven → list everything matching
            //      --status, narrow further by --older-than,
            //      confirm count, delete each. Reports per-id
            //      success/failure so a single 403 doesn't abort
            //      the rest.
            if let Some(pid) = id {
                if !*yes {
                    print!("Delete pipeline {}? [y/N] ", pid);
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
                println!("✅ Deleted pipeline {}", pid);
                return Ok(());
            }
            if status.is_none() && older_than.is_none() {
                anyhow::bail!(
                    "`pipeline delete` needs either an explicit id or \
                                 at least one of --status / --older-than"
                );
            }
            // List up to 100 matching, then narrow by date.
            let filters = ListFilters {
                status: status.clone(),
                per_page: 100,
            };
            let mut targets = client.list(&owner, &repo_name, &filters)?;
            if let Some(d) = older_than.as_deref() {
                let mins = crate::duration::parse_duration(d)? as i64;
                let days = std::cmp::max(1, mins / (60 * 24));
                targets = filter_older_than(targets, days);
            }
            if targets.is_empty() {
                println!("No pipelines matched the filter.");
                return Ok(());
            }
            if !*yes {
                println!("Will delete {} pipeline(s):", targets.len());
                for p in targets.iter().take(10) {
                    println!(
                        "  {} [{}] {} {}",
                        p.id,
                        p.raw_status,
                        p.branch,
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
                        println!("  ✅ {}", p.id);
                    }
                    Err(e) => {
                        failed.push((p.id.clone(), e.to_string()));
                        println!("  ❌ {}: {}", p.id, e);
                    }
                }
            }
            println!("Done: {} deleted, {} failed.", ok, failed.len());
            if !failed.is_empty() {
                anyhow::bail!("{} pipeline(s) could not be deleted", failed.len());
            }
        }
    }
    Ok(())
}

pub(crate) fn run_job(action: &JobCommands, remote: &String) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
        .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
    let client = get_pipeline_client(&platform)?;
    match action {
        JobCommands::List { pipeline, status } => {
            let jobs = client.list_jobs(&owner, &repo_name, pipeline, status.as_deref())?;
            if jobs.is_empty() {
                println!("No jobs found for pipeline {}.", pipeline);
            } else {
                println!(
                    "{:<14} {:<10} {:<24} {:<12} DURATION",
                    "ID", "STATUS", "NAME", "STAGE"
                );
                for j in &jobs {
                    let icon = match j.status.as_str() {
                        "success" => "✅",
                        "failed" => "❌",
                        "running" => "🟡",
                        "canceled" => "⚪",
                        "pending" => "⏳",
                        _ => "·",
                    };
                    let dur = j
                        .duration_seconds
                        .map(|d| format!("{}s", d as i64))
                        .unwrap_or_else(|| "—".into());
                    println!(
                        "{:<14} {} {:<8} {:<24} {:<12} {}",
                        j.id, icon, j.raw_status, j.name, j.stage, dur
                    );
                }
            }
        }
        JobCommands::Log { id, tail } => {
            let log = client.job_log(&owner, &repo_name, id)?;
            if let Some(n) = tail {
                let lines: Vec<&str> = log.lines().collect();
                let start = lines.len().saturating_sub(*n);
                for line in &lines[start..] {
                    println!("{}", line);
                }
            } else {
                println!("{}", log);
            }
        }
        JobCommands::Retry { id } => {
            client.job_retry(&owner, &repo_name, id)?;
            println!("✅ Retried job {}", id);
        }
        JobCommands::Cancel { id } => {
            client.job_cancel(&owner, &repo_name, id)?;
            println!("✅ Cancelled job {}", id);
        }
        JobCommands::Artifacts { id, output } => {
            let default_path = format!("./{}-artifacts.zip", id);
            let out = output.clone().unwrap_or(default_path);
            let path = std::path::Path::new(&out);
            client.job_artifacts_download(&owner, &repo_name, id, path)?;
            println!("✅ Downloaded job {} artifacts → {}", id, out);
        }
        JobCommands::Erase { id, yes } => {
            if !*yes {
                print!(
                    "Erase log + artifacts of job {}? (job metadata kept) [y/N] ",
                    id
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
            client.job_erase(&owner, &repo_name, id)?;
            println!("✅ Erased job {} log + artifacts", id);
        }
    }
    Ok(())
}
