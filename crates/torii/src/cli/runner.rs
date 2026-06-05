//! `torii runner` — CI runner management (platform API + local docker).

use crate::pr::detect_platform_from_remote_named;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum RunnerCommands {
    /// List the project's runners
    List,
    /// Show one runner's details
    Show { id: String },
    /// Delete a runner (the agent on the host still needs uninstalling)
    Remove {
        id: String,
        /// Skip confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Reset the runner's authentication token (GitLab only). Prints
    /// the new token — paste it into the runner's `config.toml`.
    ResetToken { id: String },
    /// Pause a runner (GitLab only). The runner stays connected but
    /// stops picking up jobs until you `resume` it.
    Pause { id: String },
    /// Resume a previously paused runner (GitLab only).
    Resume { id: String },

    /// Register a self-hosted runner against the active platform.
    ///
    /// Fetches a registration token from the platform's API and wraps
    /// the platform's native register CLI:
    ///   - GitLab: `gitlab-runner register` (binary must be on PATH).
    ///   - GitHub: `./config.sh` inside the runner directory (use
    ///     `--runner-dir` to point at the unpacked actions-runner).
    ///
    /// The actual agent install (downloading the binary, setting up
    /// the systemd service, etc.) is platform/distro-specific and is
    /// NOT done by torii — install the runner first via your package
    /// manager or the platform's docs, then run this to register it.
    Register {
        /// Human-readable description for the runner (GitLab) / name (GitHub).
        #[arg(long)]
        description: Option<String>,
        /// Comma-separated tag list (GitLab) / labels (GitHub).
        #[arg(long)]
        tags: Option<String>,
        /// Executor for GitLab runners: shell, docker, kubernetes, …
        /// Defaults to `shell`. Ignored on GitHub (Actions runners use
        /// a single execution model).
        #[arg(long, default_value = "shell")]
        executor: String,
        /// Docker image when `--executor docker` is used.
        #[arg(long, default_value = "alpine:latest")]
        docker_image: String,
        /// For GitHub: directory where `./config.sh` lives (the
        /// unpacked `actions-runner-*` tarball). Defaults to
        /// `./actions-runner` in the current dir.
        #[arg(long)]
        runner_dir: Option<String>,
        /// Skip the confirmation prompt that shows the resolved
        /// command before running it. Useful for scripts.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Scaffold a runner config (`gitlab-runner` only for now).
    /// Writes a starter `~/.gitlab-runner/config.toml` if absent so
    /// `torii runner register` has a place to land its block. Does
    /// NOT install the binary.
    Init,

    /// Spawn a Dockerized runner against the current platform.
    ///
    /// Pulls the upstream image, runs it as a detached container
    /// with the Docker socket mounted (so the runner can launch its
    /// own job containers), then runs `gitlab-runner register`
    /// inside to attach it to the project.
    ///
    /// Container is named `torii-runner-<slug>` so the rest of
    /// `torii runner status / start / stop / logs / destroy` can
    /// list and drive it without any local state file. GitLab only
    /// for now — GitHub Actions self-hosted runners use a different
    /// container shape (ephemeral tokens, JIT config) we haven't
    /// wired yet.
    #[command(after_help = "Examples:
  torii runner spawn                                  GitLab project, docker executor
  torii runner spawn --name void-torii                Custom container name suffix
  torii runner spawn --executor shell                 Use the shell executor inside the container
  torii runner spawn --image rust:1.94                Run jobs inside this Docker image
  torii runner spawn --tags torii,docker              Tag list passed to register
  torii runner spawn --remote github-paskidev         (rejected — GitHub spawn not implemented)")]
    Spawn {
        /// Human-readable suffix appended to the container name. Final
        /// name is `torii-runner-<name>`. Defaults to a slug derived
        /// from the owner/repo.
        #[arg(long)]
        name: Option<String>,
        /// Image used for the runner container itself. Defaults to
        /// the upstream `gitlab/gitlab-runner:latest`.
        #[arg(long, default_value = "gitlab/gitlab-runner:latest")]
        runner_image: String,
        /// Executor passed to `gitlab-runner register`.
        #[arg(long, default_value = "docker")]
        executor: String,
        /// Image used for the jobs the runner picks up (only meaningful
        /// when `--executor docker`).
        #[arg(long, default_value = "alpine:latest")]
        image: String,
        /// Tag list for the runner. Comma-separated.
        #[arg(long)]
        tags: Option<String>,
        /// Description shown in the platform's runner list.
        #[arg(long)]
        description: Option<String>,
        /// Skip confirmation prompt for the resolved docker commands.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List dockerized runners managed by torii (`torii-runner-*`
    /// containers) and their status.
    Status,

    /// Start a stopped torii-managed runner container.
    Start { name: String },

    /// Stop a running torii-managed runner container.
    Stop { name: String },

    /// Tail the logs of a torii-managed runner container.
    Logs {
        name: String,
        /// Follow the log stream (`docker logs -f`). Press Ctrl-C to
        /// detach; the container keeps running.
        #[arg(short = 'f', long)]
        follow: bool,
        /// Show only the last N lines of historical logs.
        #[arg(long)]
        tail: Option<usize>,
    },

    /// Remove a torii-managed runner container completely. Stops it
    /// first if needed. Doesn't touch the platform-side runner
    /// registration — use `torii runner remove <id>` for that.
    Destroy {
        name: String,
        /// Skip confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Run a single CI job locally, without pushing anything.
    ///
    /// Wraps the platform's local-exec tool:
    ///   - GitLab: `gitlab-runner exec <executor> <job>` (deprecated
    ///     in GitLab Runner 17.x but still functional). If you have
    ///     `gitlab-ci-local` on PATH and pass `--use-gitlab-ci-local`,
    ///     torii calls that instead.
    ///   - GitHub: `act -j <job>`. Install `act` from
    ///     <https://github.com/nektos/act>.
    ///
    /// Useful for iterating on a `.gitlab-ci.yml` / GitHub workflow
    /// without burning CI minutes or polluting pipelines.
    #[command(after_help = "Examples:
  torii runner exec build                              GitLab `build` job, docker executor
  torii runner exec test --executor shell              Shell executor instead
  torii runner exec deploy --env STAGE=prod --env KEY=… Pass env vars
  torii runner exec test --ci-file .gitlab-ci.alt.yml   Different CI file
  torii runner exec unit --remote github-paskidev      GitHub workflow via `act`")]
    Exec {
        /// Name of the job to run. Must exist in the resolved CI
        /// file.
        job: String,
        /// CI config file. Defaults to `.gitlab-ci.yml` (GitLab) or
        /// the autodetected GitHub workflow (act picks one).
        #[arg(long)]
        ci_file: Option<String>,
        /// Executor passed to `gitlab-runner exec`. Ignored on
        /// GitHub.
        #[arg(long, default_value = "docker")]
        executor: String,
        /// Pass an environment variable to the job. Repeatable:
        /// `--env KEY=VAL --env OTHER=VAL`.
        #[arg(long = "env", value_name = "KEY=VAL")]
        env: Vec<String>,
        /// Prefer `gitlab-ci-local` over the deprecated
        /// `gitlab-runner exec`. The binary must be on PATH.
        #[arg(long)]
        use_gitlab_ci_local: bool,
    },
}

pub(crate) fn run(action: &RunnerCommands, remote: &String) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
        .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
    let client = crate::runner::get_runner_client(&platform)?;
    match action {
        RunnerCommands::List => {
            let runners = client.list(&owner, &repo_name)?;
            if runners.is_empty() {
                println!(
                    "No runners found on {} for {}/{}.",
                    platform, owner, repo_name
                );
            } else {
                println!(
                    "{:<8} {:<10} {:<28} {:<8} {:<12} TAGS",
                    "ID", "STATUS", "DESCRIPTION", "OS", "TYPE"
                );
                for r in &runners {
                    let icon = match r.status.as_str() {
                        "online" | "active" => "🟢",
                        "offline" | "stale" => "⚪",
                        "paused" => "⏸",
                        _ => "·",
                    };
                    let tags = if r.tags.is_empty() {
                        "—".into()
                    } else {
                        r.tags.join(",")
                    };
                    let trim = |s: &str, n: usize| -> String {
                        if s.chars().count() <= n {
                            s.to_string()
                        } else {
                            format!(
                                "{}…",
                                s.chars().take(n.saturating_sub(1)).collect::<String>()
                            )
                        }
                    };
                    println!(
                        "{:<8} {} {:<8} {:<28} {:<8} {:<12} {}",
                        r.id,
                        icon,
                        r.status,
                        trim(&r.description, 27),
                        trim(&r.os, 7),
                        trim(&r.runner_type, 11),
                        tags
                    );
                }
            }
        }
        RunnerCommands::Show { id } => {
            let r = client.show(&owner, &repo_name, id)?;
            println!("Runner #{}", r.id);
            println!("  description: {}", r.description);
            println!(
                "  status:      {}{}",
                r.status,
                if r.paused { " (paused)" } else { "" }
            );
            println!("  type:        {}", r.runner_type);
            println!("  os:          {}", r.os);
            if !r.ip_address.is_empty() {
                println!("  ip:          {}", r.ip_address);
            }
            if !r.version.is_empty() {
                println!("  version:     {}", r.version);
            }
            println!(
                "  tags:        {}",
                if r.tags.is_empty() {
                    "—".to_string()
                } else {
                    r.tags.join(", ")
                }
            );
            if !r.web_url.is_empty() {
                println!("  url:         {}", r.web_url);
            }
        }
        RunnerCommands::Remove { id, yes } => {
            if !*yes {
                print!("Delete runner {} on {}? The agent on the host still needs uninstalling separately. [y/N] ", id, platform);
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Cancelled.");
                    return Ok(());
                }
            }
            client.remove(&owner, &repo_name, id)?;
            println!("✅ Removed runner {}", id);
        }
        RunnerCommands::ResetToken { id } => {
            let new_token = client.reset_token(&owner, &repo_name, id)?;
            println!("✅ New auth token for runner {}:\n", id);
            println!("    {}\n", new_token);
            println!("Paste it into the runner's config.toml under `token = \"…\"` and restart the agent.");
        }
        RunnerCommands::Pause { id } => {
            client.pause(&owner, &repo_name, id)?;
            println!("⏸  Paused runner {}", id);
        }
        RunnerCommands::Resume { id } => {
            client.resume(&owner, &repo_name, id)?;
            println!("▶  Resumed runner {}", id);
        }
        RunnerCommands::Register {
            description,
            tags,
            executor,
            docker_image,
            runner_dir,
            yes,
        } => {
            let reg = client.registration_token(&owner, &repo_name)?;
            run_runner_register(
                &platform,
                &reg,
                description.as_deref(),
                tags.as_deref(),
                executor,
                docker_image,
                runner_dir.as_deref(),
                *yes,
            )?;
        }
        RunnerCommands::Init => {
            run_runner_init(&platform)?;
        }
        RunnerCommands::Spawn {
            name,
            runner_image,
            executor,
            image,
            tags,
            description,
            yes,
        } => {
            let reg = client.registration_token(&owner, &repo_name)?;
            run_runner_spawn(
                &platform,
                &owner,
                &repo_name,
                &reg,
                name.as_deref(),
                runner_image,
                executor,
                image,
                tags.as_deref(),
                description.as_deref(),
                *yes,
            )?;
        }
        RunnerCommands::Status => {
            run_runner_status()?;
        }
        RunnerCommands::Start { name } => {
            run_runner_docker(&["start", &container_name(name)], "start")?;
        }
        RunnerCommands::Stop { name } => {
            run_runner_docker(&["stop", &container_name(name)], "stop")?;
        }
        RunnerCommands::Logs { name, follow, tail } => {
            let mut args: Vec<String> = vec!["logs".to_string()];
            if *follow {
                args.push("-f".to_string());
            }
            if let Some(n) = tail {
                args.push(format!("--tail={}", n));
            }
            args.push(container_name(name));
            let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            run_runner_docker_inherit(&argv)?;
        }
        RunnerCommands::Exec {
            job,
            ci_file,
            executor,
            env,
            use_gitlab_ci_local,
        } => {
            run_runner_exec(
                &platform,
                job,
                ci_file.as_deref(),
                executor,
                env,
                *use_gitlab_ci_local,
            )?;
        }
        RunnerCommands::Destroy { name, yes } => {
            let full = container_name(name);
            if !*yes {
                print!("Destroy container `{}` (still leaves the runner registered on the platform)? [y/N] ", full);
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Cancelled.");
                    return Ok(());
                }
            }
            // `rm -f` stops + removes in one go.
            run_runner_docker(&["rm", "-f", &full], "rm")?;
        }
    }
    Ok(())
}

/// `torii runner register` — wrap the platform's native register CLI.
/// We never copy the platform's logic; we just hand it the token and
/// a tidy argv. If the binary is missing we tell the user where to
/// install it from, instead of trying to ship our own.
fn run_runner_register(
    platform: &str,
    reg: &crate::runner::RegistrationToken,
    description: Option<&str>,
    tags: Option<&str>,
    executor: &str,
    docker_image: &str,
    runner_dir: Option<&str>,
    yes: bool,
) -> Result<()> {
    use std::process::Command;

    let (bin, args, cwd) = match platform {
        "gitlab" => {
            let bin = which_binary("gitlab-runner").ok_or_else(|| {
                anyhow::anyhow!(
                    "`gitlab-runner` not found on PATH. Install it first: \
                 https://docs.gitlab.com/runner/install/. Then re-run \
                 `torii runner register`."
                )
            })?;
            let mut argv: Vec<String> = vec![
                "register".to_string(),
                "--non-interactive".to_string(),
                "--url".to_string(),
                reg.register_url.clone(),
                "--registration-token".to_string(),
                reg.token.clone(),
                "--executor".to_string(),
                executor.to_string(),
            ];
            if executor == "docker" {
                argv.push("--docker-image".to_string());
                argv.push(docker_image.to_string());
            }
            if let Some(d) = description {
                argv.push("--description".to_string());
                argv.push(d.to_string());
            }
            if let Some(t) = tags {
                argv.push("--tag-list".to_string());
                argv.push(t.to_string());
            }
            (bin, argv, None::<String>)
        }
        "github" => {
            let dir = runner_dir
                .map(String::from)
                .unwrap_or_else(|| "./actions-runner".to_string());
            let config_sh = std::path::Path::new(&dir).join("config.sh");
            if !config_sh.exists() {
                anyhow::bail!(
                    "GitHub Actions runner not found at `{}`. Download it from \
                     https://github.com/{}/{}/settings/actions/runners/new and \
                     unpack it there, then re-run with `--runner-dir <path>`.",
                    config_sh.display(),
                    // can't access owner/repo here; the registration_token
                    // URL already includes them.
                    "OWNER",
                    "REPO"
                );
            }
            let mut argv: Vec<String> = vec![
                "--unattended".to_string(),
                "--url".to_string(),
                reg.register_url.clone(),
                "--token".to_string(),
                reg.token.clone(),
                "--replace".to_string(),
            ];
            if let Some(d) = description {
                argv.push("--name".to_string());
                argv.push(d.to_string());
            }
            if let Some(t) = tags {
                argv.push("--labels".to_string());
                argv.push(t.to_string());
            }
            (config_sh.display().to_string(), argv, Some(dir))
        }
        other => {
            anyhow::bail!(
                "`torii runner register` is GitHub + GitLab only. `{}` not implemented yet.",
                other
            );
        }
    };

    // Show the resolved command so the user can audit it before we
    // launch a subprocess that mutates the host's runner state.
    let pretty_args = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{}\"", a)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("🛠  Will run:");
    if let Some(d) = &cwd {
        println!("     (in {}) {} {}", d, bin, pretty_args);
    } else {
        println!("     {} {}", bin, pretty_args);
    }
    if let Some(exp) = reg.expires_in_seconds {
        println!("     (token expires in ~{}s)", exp);
    }

    if !yes {
        use std::io::Write;
        print!("Proceed? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Cancelled.");
            return Ok(());
        }
    }

    let mut cmd = Command::new(&bin);
    cmd.args(&args);
    if let Some(d) = &cwd {
        cmd.current_dir(d);
    }
    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("failed to exec `{}`: {}", bin, e))?;
    if !status.success() {
        anyhow::bail!(
            "Runner register exited with {}. Token was already consumed; \
             generate a fresh one before retrying.",
            status
        );
    }
    println!("✅ Runner registered.");
    Ok(())
}

/// `torii runner exec` — run a CI job locally without push. Detects
/// platform from the remote and wraps the right local-exec binary;
/// inherits stdio so the user sees the build log live.
fn run_runner_exec(
    platform: &str,
    job: &str,
    ci_file: Option<&str>,
    executor: &str,
    env: &[String],
    use_gitlab_ci_local: bool,
) -> Result<()> {
    match platform {
        "gitlab" => {
            // Two paths, picked by `--use-gitlab-ci-local`. The
            // gitlab-ci-local project is the modern replacement now
            // that `gitlab-runner exec` is deprecated (17.x).
            if use_gitlab_ci_local {
                which_binary("gitlab-ci-local").ok_or_else(|| {
                    anyhow::anyhow!(
                        "`gitlab-ci-local` not on PATH. Install from \
                     https://github.com/firecow/gitlab-ci-local (npm i -g gitlab-ci-local)."
                    )
                })?;
                let mut argv: Vec<String> = vec![job.to_string()];
                if let Some(f) = ci_file {
                    argv.push("--file".to_string());
                    argv.push(f.to_string());
                }
                for kv in env {
                    argv.push("--variable".to_string());
                    argv.push(kv.clone());
                }
                let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                println!("🛠  gitlab-ci-local {}", argv_refs.join(" "));
                let status = std::process::Command::new("gitlab-ci-local")
                    .args(&argv_refs)
                    .status()
                    .map_err(|e| anyhow::anyhow!("spawn gitlab-ci-local: {}", e))?;
                if !status.success() {
                    anyhow::bail!("gitlab-ci-local exited {}", status);
                }
                Ok(())
            } else {
                which_binary("gitlab-runner").ok_or_else(|| {
                    anyhow::anyhow!(
                        "`gitlab-runner` not on PATH. Install from \
                     https://docs.gitlab.com/runner/install/, or pass \
                     `--use-gitlab-ci-local` if you have that installed instead."
                    )
                })?;
                let mut argv: Vec<String> =
                    vec!["exec".to_string(), executor.to_string(), job.to_string()];
                if let Some(f) = ci_file {
                    argv.push("--cicd-config-file".to_string());
                    argv.push(f.to_string());
                }
                for kv in env {
                    argv.push("--env".to_string());
                    argv.push(kv.clone());
                }
                let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                eprintln!(
                    "ℹ  `gitlab-runner exec` is deprecated in GitLab Runner 17.x. \
                     Consider `gitlab-ci-local` (`--use-gitlab-ci-local`)."
                );
                println!("🛠  gitlab-runner {}", argv_refs.join(" "));
                let status = std::process::Command::new("gitlab-runner")
                    .args(&argv_refs)
                    .status()
                    .map_err(|e| anyhow::anyhow!("spawn gitlab-runner: {}", e))?;
                if !status.success() {
                    anyhow::bail!("gitlab-runner exec exited {}", status);
                }
                Ok(())
            }
        }
        "github" => {
            which_binary("act").ok_or_else(|| {
                anyhow::anyhow!("`act` not on PATH. Install from https://github.com/nektos/act.")
            })?;
            let mut argv: Vec<String> = vec!["-j".to_string(), job.to_string()];
            if let Some(f) = ci_file {
                argv.push("-W".to_string());
                argv.push(f.to_string());
            }
            for kv in env {
                argv.push("--env".to_string());
                argv.push(kv.clone());
            }
            let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            println!("🛠  act {}", argv_refs.join(" "));
            let status = std::process::Command::new("act")
                .args(&argv_refs)
                .status()
                .map_err(|e| anyhow::anyhow!("spawn act: {}", e))?;
            if !status.success() {
                anyhow::bail!("act exited {}", status);
            }
            Ok(())
        }
        other => anyhow::bail!(
            "`torii runner exec` doesn't know how to drive a local executor for `{}`. \
             Supported: gitlab (gitlab-runner / gitlab-ci-local), github (act).",
            other
        ),
    }
}

/// Final docker container name for a torii-managed runner. Single
/// source of truth so `spawn` and the other commands agree on the
/// name shape.
fn container_name(suffix: &str) -> String {
    let trimmed = suffix.trim();
    if trimmed.is_empty() {
        return "torii-runner".to_string();
    }
    if trimmed.starts_with("torii-runner-") || trimmed == "torii-runner" {
        return trimmed.to_string();
    }
    format!("torii-runner-{}", trimmed)
}

/// `torii runner spawn` — bring a Dockerized GitLab Runner up against
/// the current project. Two-phase: `docker run` to start the agent,
/// then `docker exec gitlab-runner register` inside to attach it.
/// Mirrors the manual flow the user has been doing by hand.
fn run_runner_spawn(
    platform: &str,
    owner: &str,
    repo: &str,
    reg: &crate::runner::RegistrationToken,
    name: Option<&str>,
    runner_image: &str,
    executor: &str,
    job_image: &str,
    tags: Option<&str>,
    description: Option<&str>,
    yes: bool,
) -> Result<()> {
    if platform != "gitlab" {
        anyhow::bail!(
            "`torii runner spawn` is GitLab-only for now. GitHub Actions self-hosted \
             runners use a different container shape (ephemeral tokens, JIT config) — \
             use `torii runner register --runner-dir <path>` against an unpacked \
             actions-runner tarball instead."
        );
    }

    which_binary("docker").ok_or_else(|| {
        anyhow::anyhow!(
            "`docker` not found on PATH. Install Docker (or Podman with a `docker` shim) \
         and re-run."
        )
    })?;

    let slug = name
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", owner.replace('/', "-"), repo.replace('/', "-")));
    let cname = container_name(&slug);

    // Phase 1 — the docker run command. Mount the host socket so the
    // runner (executor=docker) can launch sibling job containers.
    // `--restart=unless-stopped` matches the convention from
    // gitlab-runner Docker docs.
    let run_args: Vec<String> = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        cname.clone(),
        "--restart".to_string(),
        "unless-stopped".to_string(),
        "-v".to_string(),
        "/var/run/docker.sock:/var/run/docker.sock".to_string(),
        "-v".to_string(),
        format!("{}-config:/etc/gitlab-runner", cname),
        runner_image.to_string(),
    ];

    // Phase 2 — register inside.
    let mut register_args: Vec<String> = vec![
        "exec".to_string(),
        cname.clone(),
        "gitlab-runner".to_string(),
        "register".to_string(),
        "--non-interactive".to_string(),
        "--url".to_string(),
        reg.register_url.clone(),
        "--registration-token".to_string(),
        reg.token.clone(),
        "--executor".to_string(),
        executor.to_string(),
    ];
    if executor == "docker" {
        register_args.push("--docker-image".to_string());
        register_args.push(job_image.to_string());
    }
    if let Some(d) = description {
        register_args.push("--description".to_string());
        register_args.push(d.to_string());
    } else {
        register_args.push("--description".to_string());
        register_args.push(format!("torii-spawned {}", cname));
    }
    if let Some(t) = tags {
        register_args.push("--tag-list".to_string());
        register_args.push(t.to_string());
    }

    let pretty_run = run_args.join(" ");
    let pretty_reg = register_args
        .iter()
        .map(|a| {
            if a == &reg.token {
                "<TOKEN>".to_string()
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("🛠  Will run:");
    println!("     docker {}", pretty_run);
    println!("     docker {}", pretty_reg);

    if !yes {
        use std::io::Write;
        print!("Proceed? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Cancelled.");
            return Ok(());
        }
    }

    // Spawn phase. We don't pipe stdout because the user just wants
    // the container id back, same as bare `docker run -d` prints.
    let run_argv: Vec<&str> = run_args.iter().map(|s| s.as_str()).collect();
    run_runner_docker_inherit(&run_argv)?;

    // Brief pause: the freshly-started container may still be coming
    // up when we run exec; gitlab-runner takes a beat to settle. A
    // tight loop polling `docker exec` would be cleaner but a single
    // 1s sleep is enough for the common case and keeps the code
    // trivial.
    std::thread::sleep(std::time::Duration::from_secs(1));

    let reg_argv: Vec<&str> = register_args.iter().map(|s| s.as_str()).collect();
    run_runner_docker_inherit(&reg_argv)?;

    println!("✓ Runner container `{}` up and registered.", cname);
    println!("  · `torii runner status`           list torii-managed runners");
    println!("  · `torii runner logs {} -f`       follow output", cname);
    println!(
        "  · `torii runner stop {}`          pause without unregistering",
        cname
    );
    println!(
        "  · `torii runner destroy {}`       remove the container",
        cname
    );
    Ok(())
}

/// `torii runner status` — surface every container whose name starts
/// with `torii-runner-`, with its docker state column.
fn run_runner_status() -> Result<()> {
    which_binary("docker").ok_or_else(|| {
        anyhow::anyhow!(
            "`docker` not found on PATH. (`runner status` only knows about Dockerized \
         runners spawned by `torii runner spawn`.)"
        )
    })?;

    let out = std::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=torii-runner-",
            "--format",
            "{{.Names}}\t{{.State}}\t{{.Status}}\t{{.Image}}",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("run docker ps: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("docker ps failed: {}", err.trim());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        println!("No torii-managed runner containers found.");
        println!("Spawn one with: `torii runner spawn`");
        return Ok(());
    }

    println!("{:<32} {:<10} {:<24} IMAGE", "NAME", "STATE", "STATUS");
    for line in lines {
        let cols: Vec<&str> = line.split('\t').collect();
        let (name, state, status, image) = (
            cols.first().copied().unwrap_or(""),
            cols.get(1).copied().unwrap_or(""),
            cols.get(2).copied().unwrap_or(""),
            cols.get(3).copied().unwrap_or(""),
        );
        let icon = match state {
            "running" => "🟢",
            "exited" => "⚪",
            "paused" => "⏸",
            "restarting" => "🔄",
            _ => "·",
        };
        println!(
            "{:<32} {} {:<8} {:<24} {}",
            name, icon, state, status, image
        );
    }
    Ok(())
}

/// Run `docker <args>` capturing stdout/stderr, surface stderr on
/// failure with the original status code. Used for the short ops
/// (start/stop) where there's no useful output to stream.
fn run_runner_docker(args: &[&str], label: &str) -> Result<()> {
    which_binary("docker").ok_or_else(|| anyhow::anyhow!("`docker` not found on PATH."))?;
    let out = std::process::Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn docker {}: {}", label, e))?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout);
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            println!("{}", trimmed);
        }
        println!("✓ {} ok", label);
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("docker {} failed: {}", label, err.trim());
    }
}

/// Same as `run_runner_docker` but inherits stdio. Used for `spawn`
/// (the docker run prints the container id) and `logs` (where the
/// user wants the stream live).
fn run_runner_docker_inherit(args: &[&str]) -> Result<()> {
    which_binary("docker").ok_or_else(|| anyhow::anyhow!("`docker` not found on PATH."))?;
    let status = std::process::Command::new("docker")
        .args(args)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn docker: {}", e))?;
    if !status.success() {
        anyhow::bail!(
            "docker {} exited {}",
            args.first().copied().unwrap_or("?"),
            status
        );
    }
    Ok(())
}

/// `torii runner init` — scaffold a starter config so `register` has
/// somewhere to land its block. Today only GitLab benefits (the
/// `gitlab-runner` binary expects a config.toml on first register).
/// GitHub's `./config.sh` writes its own files in the runner dir.
fn run_runner_init(platform: &str) -> Result<()> {
    match platform {
        "gitlab" => {
            let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no HOME"))?;
            let dir = home.join(".gitlab-runner");
            let path = dir.join("config.toml");
            if path.exists() {
                println!("ℹ  {} already exists; nothing to do.", path.display());
                return Ok(());
            }
            std::fs::create_dir_all(&dir)
                .map_err(|e| anyhow::anyhow!("mkdir {}: {}", dir.display(), e))?;
            // Minimal config that `gitlab-runner register` will append
            // its `[[runners]]` block into. The `concurrent` value can
            // be tuned later via `gitlab-runner` itself.
            let body = "concurrent = 1\ncheck_interval = 0\n\n[session_server]\n  session_timeout = 1800\n";
            std::fs::write(&path, body)
                .map_err(|e| anyhow::anyhow!("write {}: {}", path.display(), e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            println!("✅ Wrote starter {}.", path.display());
            println!("   Next: `torii runner register` to attach this host to the project.");
            Ok(())
        }
        "github" => {
            println!("ℹ  GitHub Actions runners don't need a scaffold — the runner tarball");
            println!("   carries its own `./config.sh`. Download it from");
            println!("   https://github.com/<owner>/<repo>/settings/actions/runners/new ,");
            println!("   unpack it, and run `torii runner register --runner-dir <path>`.");
            Ok(())
        }
        other => {
            anyhow::bail!(
                "`torii runner init` is GitHub + GitLab only. `{}` not implemented yet.",
                other
            );
        }
    }
}

/// Look up an executable by name on PATH. Returns the full path so
/// `Command::new(<that>)` is unambiguous, or None when missing.
fn which_binary(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}
