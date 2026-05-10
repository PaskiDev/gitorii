// Extended Git operations for Torii
use git2::BranchType;
use crate::error::Result;
use crate::core::GitRepo;
use chrono::NaiveDateTime;

impl GitRepo {
    /// Show commit history
    pub fn log(
        &self,
        count: Option<usize>,
        oneline: bool,
        graph: bool,
        author: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        grep: Option<&str>,
        stat: bool,
    ) -> Result<()> {
        if graph {
            return self.log_graph(count.unwrap_or(50), true);
        }
        let mut revwalk = self.repository().revwalk()?;
        revwalk.push_head()?;

        let max_count = count.unwrap_or(10);
        let mut shown = 0;

        // Parse date filters
        let since_ts: Option<i64> = since.and_then(|s| {
            NaiveDateTime::parse_from_str(&format!("{} 00:00:00", s), "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| dt.and_utc().timestamp())
        });
        let until_ts: Option<i64> = until.and_then(|s| {
            NaiveDateTime::parse_from_str(&format!("{} 23:59:59", s), "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| dt.and_utc().timestamp())
        });

        println!("📜 Commit History:");
        println!();

        for oid in revwalk {
            if shown >= max_count {
                break;
            }

            let oid = oid?;
            let commit = self.repository().find_commit(oid)?;
            let ts = commit.time().seconds();

            // Author filter
            if let Some(filter) = author {
                let name = commit.author().name().unwrap_or("").to_lowercase();
                let email = commit.author().email().unwrap_or("").to_lowercase();
                let f = filter.to_lowercase();
                if !name.contains(&f) && !email.contains(&f) {
                    continue;
                }
            }

            // Date filters
            if let Some(s) = since_ts {
                if ts < s { continue; }
            }
            if let Some(u) = until_ts {
                if ts > u { continue; }
            }

            // Grep filter
            if let Some(pattern) = grep {
                let msg = commit.message().unwrap_or("");
                if !msg.to_lowercase().contains(&pattern.to_lowercase()) {
                    continue;
                }
            }

            if oneline {
                let short_id = &oid.to_string()[..7];
                let message = commit.message().unwrap_or("<no message>").lines().next().unwrap_or("");
                println!("  {} {}", short_id, message);
            } else {
                println!("  commit {}", oid);
                if let Some(author_name) = commit.author().name() {
                    println!("  Author: {}", author_name);
                }
                println!("  Date:   {}", chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "<unknown>".to_string()));
                println!();
                if let Some(msg) = commit.message() {
                    for line in msg.lines() {
                        println!("      {}", line);
                    }
                }
                println!();

                // Stat: show changed files count
                if stat {
                    if let Ok(parent) = commit.parent(0) {
                        let old_tree = parent.tree().ok();
                        let new_tree = commit.tree().ok();
                        if let (Some(old), Some(new)) = (old_tree, new_tree) {
                            let diff = self.repository().diff_tree_to_tree(Some(&old), Some(&new), None);
                            if let Ok(diff) = diff {
                                let stats = diff.stats()?;
                                println!("  {} files changed, {} insertions(+), {} deletions(-)",
                                    stats.files_changed(),
                                    stats.insertions(),
                                    stats.deletions()
                                );
                                println!();
                            }
                        }
                    }
                }
            }

            shown += 1;
        }

        Ok(())
    }

    /// Render commit history as a graph (`torii log --graph`).
    /// Style picked from TORII_GRAPH_STYLE env var (ascii|curves|heavy);
    /// defaults to curves.
    fn log_graph(&self, limit: usize, include_all: bool) -> Result<()> {
        let style = std::env::var("TORII_GRAPH_STYLE")
            .ok()
            .map(|s| crate::graph::GraphStyle::from_str(&s))
            .unwrap_or_default();
        let rendered = crate::graph::render_repo_with(self.repository(), limit, include_all, style)
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let extra_pad = style.expanded_extra_lines();
        println!("📜 Commit Graph:");
        println!();
        for (commit, row) in &rendered {
            if !row.transition_line.is_empty() {
                println!("  {}", row.transition_line);
            }
            let refs_str = if commit.refs.is_empty() {
                String::new()
            } else {
                let badges: Vec<String> = commit
                    .refs
                    .iter()
                    .map(|r| crate::graph::format_ref_badge(r))
                    .collect();
                format!("{} ", badges.join(" "))
            };
            let summary = if commit.summary.chars().count() > 80 {
                let cut: String = commit.summary.chars().take(79).collect();
                format!("{}…", cut)
            } else {
                commit.summary.clone()
            };

            if extra_pad > 0 {
                // Expanded: commit row carries node + hash + badges; second
                // row indents the message under the lanes.
                println!(
                    "  {} {} {}",
                    row.commit_line, commit.short_id, refs_str.trim_end()
                );
                let pad_row = crate::graph::padding_row(&row.commit_line, style);
                println!("  {} {}", pad_row, summary);
                for _ in 1..extra_pad {
                    println!("  {}", pad_row);
                }
            } else {
                println!(
                    "  {} {} {}{}",
                    row.commit_line, commit.short_id, refs_str, summary
                );
            }
        }
        Ok(())
    }

    /// Show reflog (HEAD movement history)
    pub fn show_reflog(&self, count: usize) -> Result<()> {
        let reflog = self.repo.reflog("HEAD")
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("📋 Reflog (HEAD movements):");
        println!();

        for (i, entry) in reflog.iter().enumerate() {
            if i >= count {
                break;
            }
            let oid_short = entry.id_new().to_string();
            let oid_short = &oid_short[..7.min(oid_short.len())];
            let message = entry.message().unwrap_or("");
            println!("  {} {}", oid_short, message);
        }

        println!();
        println!("💡 Restore a state: torii save --reset <commit-hash> --reset-mode soft");

        Ok(())
    }

    /// Rebase with a pre-written todo file (no editor required)
    pub fn rebase_with_todo(&self, base: &str, todo_file: &std::path::Path) -> Result<()> {
        #[cfg(unix)]
        {
            let repo_path = self.repo.path().parent().unwrap().to_path_buf();
            let todo_abs = todo_file.canonicalize().map_err(|_| {
                crate::error::ToriiError::InvalidConfig(
                    format!("Todo file not found: {}", todo_file.display())
                )
            })?;
            println!("🔄 Rebasing from {} using todo file: {}", base, todo_abs.display());
            let (todo_for_git, reword_map) = preprocess_reword_todo(&todo_abs)?;
            let editor = format!("cp {}", todo_for_git.display());
            let mut cmd = std::process::Command::new("git");
            cmd.args(["rebase", "-i", base])
                .env("GIT_SEQUENCE_EDITOR", &editor)
                .current_dir(&repo_path);
            install_message_editor(&mut cmd, &reword_map, &repo_path)?;
            let status = cmd.status()?;
            report_rebase_outcome(&repo_path, status);
        }
        #[cfg(not(unix))]
        {
            let _ = (base, todo_file);
            return Err(crate::error::ToriiError::InvalidConfig(
                "Interactive rebase with todo file requires a Unix shell. Not supported on this platform.".to_string()
            ));
        }
        Ok(())
    }

    /// Interactive rebase
    pub fn rebase_interactive(&self, base: &str) -> Result<()> {
        #[cfg(unix)]
        {
            let repo_path = self.repo.path().parent().unwrap().to_path_buf();
            println!("🔄 Starting interactive rebase onto {}...", base);
            let status = std::process::Command::new("git")
                .args(["rebase", "-i", base])
                .current_dir(&repo_path)
                .status()?;
            report_rebase_outcome(&repo_path, status);
        }
        #[cfg(not(unix))]
        {
            let _ = base;
            return Err(crate::error::ToriiError::InvalidConfig(
                "Interactive rebase requires a Unix terminal. Not supported on this platform.".to_string()
            ));
        }
        Ok(())
    }

    /// Interactive rebase from the root commit (`git rebase -i --root`)
    pub fn rebase_root_interactive(&self) -> Result<()> {
        #[cfg(unix)]
        {
            let repo_path = self.repo.path().parent().unwrap().to_path_buf();
            println!("🔄 Starting interactive rebase from root...");
            let status = std::process::Command::new("git")
                .args(["rebase", "-i", "--root"])
                .current_dir(&repo_path)
                .status()?;
            report_rebase_outcome(&repo_path, status);
        }
        #[cfg(not(unix))]
        {
            return Err(crate::error::ToriiError::InvalidConfig(
                "Interactive rebase requires a Unix terminal. Not supported on this platform.".to_string()
            ));
        }
        Ok(())
    }

    /// Rebase from the root commit using a pre-written todo file
    pub fn rebase_root_with_todo(&self, todo_file: &std::path::Path) -> Result<()> {
        #[cfg(unix)]
        {
            let repo_path = self.repo.path().parent().unwrap().to_path_buf();
            let todo_abs = todo_file.canonicalize().map_err(|_| {
                crate::error::ToriiError::InvalidConfig(
                    format!("Todo file not found: {}", todo_file.display())
                )
            })?;
            println!("🔄 Rebasing from root using todo file: {}", todo_abs.display());
            let (todo_for_git, reword_map) = preprocess_reword_todo(&todo_abs)?;
            let editor = format!("cp {}", todo_for_git.display());
            let mut cmd = std::process::Command::new("git");
            cmd.args(["rebase", "-i", "--root"])
                .env("GIT_SEQUENCE_EDITOR", &editor)
                .current_dir(&repo_path);
            install_message_editor(&mut cmd, &reword_map, &repo_path)?;
            let status = cmd.status()?;
            report_rebase_outcome(&repo_path, status);
        }
        #[cfg(not(unix))]
        {
            let _ = todo_file;
            return Err(crate::error::ToriiError::InvalidConfig(
                "Interactive rebase with todo file requires a Unix shell. Not supported on this platform.".to_string()
            ));
        }
        Ok(())
    }

    /// Continue an in-progress rebase
    pub fn rebase_continue(&self) -> Result<()> {
        // Two rebase formats coexist:
        //   - libgit2-initiated rebases → use Repository::open_rebase
        //   - `git rebase -i` (CLI, used for interactive/edit/reword) → not
        //     openable via libgit2; delegate to `git rebase --continue`
        if self.has_cli_rebase_in_progress() {
            return self.delegate_rebase_subcommand("--continue", "continued");
        }
        let mut rebase = self.repo.open_rebase(None)
            .map_err(|_| crate::error::ToriiError::InvalidConfig(
                "No rebase in progress".to_string()
            ))?;

        let sig = self.repo.signature()
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        // Commit the currently resolved step
        rebase.commit(None, &sig, None)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        // Apply remaining steps
        while let Some(op) = rebase.next() {
            let _op = op.map_err(|e| crate::error::ToriiError::Git(e))?;
            rebase.commit(None, &sig, None)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
        }

        rebase.finish(Some(&sig))
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("✅ Rebase continued");
        Ok(())
    }

    /// Abort the current rebase
    pub fn rebase_abort(&self) -> Result<()> {
        if self.has_cli_rebase_in_progress() {
            return self.delegate_rebase_subcommand("--abort", "aborted");
        }
        let mut rebase = self.repo.open_rebase(None)
            .map_err(|_| crate::error::ToriiError::InvalidConfig(
                "No rebase in progress".to_string()
            ))?;

        rebase.abort()
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("✅ Rebase aborted");
        Ok(())
    }

    /// Skip current patch in rebase
    pub fn rebase_skip(&self) -> Result<()> {
        if self.has_cli_rebase_in_progress() {
            return self.delegate_rebase_subcommand("--skip", "skipped");
        }
        let mut rebase = self.repo.open_rebase(None)
            .map_err(|_| crate::error::ToriiError::InvalidConfig(
                "No rebase in progress".to_string()
            ))?;

        let sig = self.repo.signature()
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        // Advance past current step without committing
        rebase.next()
            .ok_or_else(|| crate::error::ToriiError::InvalidConfig("No current step to skip".to_string()))?
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        // Continue remaining steps
        while let Some(op) = rebase.next() {
            let _op = op.map_err(|e| crate::error::ToriiError::Git(e))?;
            rebase.commit(None, &sig, None)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
        }

        rebase.finish(Some(&sig))
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("✅ Patch skipped");
        Ok(())
    }

    /// True if a `git rebase -i` style rebase is paused on disk
    /// (libgit2's open_rebase does not see these — it only handles rebases
    /// created via the libgit2 API).
    fn has_cli_rebase_in_progress(&self) -> bool {
        let git_dir = self.repo.path();
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
    }

    /// Run `git rebase <flag>` for CLI-managed rebases.
    fn delegate_rebase_subcommand(&self, flag: &str, verb: &str) -> Result<()> {
        let repo_path = self.repo.path().parent().unwrap().to_path_buf();
        let status = std::process::Command::new("git")
            .args(["rebase", flag])
            .current_dir(&repo_path)
            .status()
            .map_err(|e| crate::error::ToriiError::InvalidConfig(format!("spawn git: {}", e)))?;
        report_rebase_outcome(&repo_path, status);
        // If the rebase finished cleanly (no in-progress dir), echo the verb.
        let still_active = repo_path.join(".git").join("rebase-merge").exists()
            || repo_path.join(".git").join("rebase-apply").exists();
        if !still_active && status.success() {
            println!("✅ Rebase {}", verb);
        }
        Ok(())
    }

    /// Show changes
    pub fn diff(&self, staged: bool, last: bool) -> Result<()> {
        if last {
            // Show diff of last commit
            let head = self.repository().head()?.peel_to_commit()?;
            let tree = head.tree()?;
            
            let parent_tree = if head.parent_count() > 0 {
                Some(head.parent(0)?.tree()?)
            } else {
                None
            };
            
            let diff = self.repository().diff_tree_to_tree(
                parent_tree.as_ref(),
                Some(&tree),
                None,
            )?;
            
            self.print_diff(&diff)?;
        } else if staged {
            // Show staged changes
            let head = self.repository().head()?.peel_to_tree()?;
            let diff = self.repository().diff_tree_to_index(Some(&head), None, None)?;
            self.print_diff(&diff)?;
        } else {
            // Show unstaged changes
            let diff = self.repository().diff_index_to_workdir(None, None)?;
            self.print_diff(&diff)?;
        }
        
        Ok(())
    }

    fn print_diff(&self, diff: &git2::Diff) -> Result<()> {
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            let content = std::str::from_utf8(line.content()).unwrap_or("<binary>");
            
            match origin {
                '+' => print!("\x1b[32m+{}\x1b[0m", content),
                '-' => print!("\x1b[31m-{}\x1b[0m", content),
                _ => print!(" {}", content),
            }
            true
        })?;
        
        Ok(())
    }

    /// List local branches
    pub fn list_branches(&self) -> Result<Vec<String>> {
        let branches = self.repository().branches(Some(BranchType::Local))?;
        let mut branch_names = Vec::new();

        for branch in branches {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                branch_names.push(name.to_string());
            }
        }

        Ok(branch_names)
    }

    /// List remote branches
    pub fn list_remote_branches(&self) -> Result<Vec<String>> {
        let branches = self.repository().branches(Some(BranchType::Remote))?;
        let mut branch_names = Vec::new();

        for branch in branches {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                // Skip HEAD symrefs (e.g. origin/HEAD)
                if !name.ends_with("/HEAD") {
                    branch_names.push(name.to_string());
                }
            }
        }

        Ok(branch_names)
    }

    /// Create a new branch
    /// Create an orphan branch — sets HEAD to refs/heads/<name> without any parent.
    /// The next commit will be a new root. Index and working tree are left intact
    /// so the user can stage what they want before the first commit.
    pub fn create_orphan_branch(&self, name: &str) -> Result<()> {
        let refname = format!("refs/heads/{}", name);
        // Reject if branch already exists
        if self.repository().find_reference(&refname).is_ok() {
            return Err(crate::error::ToriiError::InvalidConfig(
                format!("Branch '{}' already exists", name)
            ));
        }
        // Point HEAD at the (yet-unborn) ref. libgit2 allows symbolic HEAD
        // pointing to a non-existent branch — the first commit will create it.
        self.repository().set_head(&refname)
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        Ok(())
    }

    pub fn create_branch(&self, name: &str) -> Result<()> {
        let head = self.repository().head()?.peel_to_commit()?;
        self.repository().branch(name, &head, false)?;
        Ok(())
    }

    /// Delete a branch
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        let mut branch = self.repository().find_branch(name, BranchType::Local)?;
        branch.delete()?;
        Ok(())
    }

    /// Switch to a branch
    pub fn switch_branch(&self, name: &str) -> Result<()> {
        let obj = self.repository().revparse_single(&format!("refs/heads/{}", name))?;
        self.repository().checkout_tree(&obj, None)?;
        self.repository().set_head(&format!("refs/heads/{}", name))?;
        Ok(())
    }

    /// Checkout a remote branch — creates local tracking branch then switches to it
    pub fn checkout_remote_branch(&self, remote_name: &str) -> Result<()> {
        // remote_name is e.g. "origin/feature-x", local name is "feature-x"
        let local_name = remote_name
            .splitn(2, '/')
            .nth(1)
            .unwrap_or(remote_name);
        let repo = self.repository();
        // Create local branch tracking the remote if it doesn't exist
        if repo.find_branch(local_name, BranchType::Local).is_err() {
            let obj = repo.revparse_single(&format!("refs/remotes/{}", remote_name))?;
            let commit = obj.peel_to_commit()?;
            let mut branch = repo.branch(local_name, &commit, false)?;
            // Set upstream tracking
            branch.set_upstream(Some(remote_name))?;
        }
        self.switch_branch(local_name)
    }

    /// Clone a repository
    pub fn clone_repo(url: &str, directory: Option<&str>) -> Result<()> {
        let target = if let Some(dir) = directory {
            dir.to_string()
        } else {
            url.split('/')
                .last()
                .unwrap_or("repo")
                .trim_end_matches(".git")
                .to_string()
        };

        let cfg = crate::config::ToriiConfig::load_global().unwrap_or_default();

        let mut callbacks = git2::RemoteCallbacks::new();
        let url_owned = url.to_string();
        callbacks.credentials(move |_url, username_from_url, allowed_types| {
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                let username = username_from_url.unwrap_or("git");
                let home = dirs::home_dir().unwrap_or_default();
                let ed25519 = home.join(".ssh").join("id_ed25519");
                let rsa = home.join(".ssh").join("id_rsa");
                if ed25519.exists() {
                    return git2::Cred::ssh_key(username, None, &ed25519, None);
                } else if rsa.exists() {
                    return git2::Cred::ssh_key(username, None, &rsa, None);
                } else {
                    return git2::Cred::ssh_key_from_agent(username);
                }
            }
            if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                // Pick token based on hostname
                let token = if url_owned.contains("github.com") {
                    cfg.auth.github_token.clone()
                } else if url_owned.contains("gitlab.com") {
                    cfg.auth.gitlab_token.clone()
                } else if url_owned.contains("codeberg.org") {
                    cfg.auth.codeberg_token.clone()
                } else if url_owned.contains("bitbucket.org") {
                    cfg.auth.github_token.clone() // fallback
                } else {
                    cfg.auth.gitea_token.clone()
                };
                if let Some(token) = token {
                    return git2::Cred::userpass_plaintext("oauth2", &token);
                }
            }
            git2::Cred::default()
        });

        // ── Progress reporting ─────────────────────────────────────────────
        // Without these callbacks a clone of a huge repo (servo, chromium)
        // looked frozen — no output for minutes while tens of MB streamed.
        // Now we print pack progress (objects/bytes) and pass through
        // server sideband messages ("Compressing objects: N% …").
        use std::cell::RefCell;
        use std::io::Write;
        use std::time::Instant;

        let last_print = RefCell::new(Instant::now());
        callbacks.transfer_progress(move |stats| {
            // Throttle: redraw at most ~10 fps. Cheap and avoids flooding.
            let mut last = last_print.borrow_mut();
            let done = stats.received_objects() == stats.total_objects()
                && stats.indexed_objects() == stats.total_objects();
            if !done && last.elapsed().as_millis() < 100 {
                return true;
            }
            *last = Instant::now();

            let total = stats.total_objects().max(1);
            let recv = stats.received_objects();
            let idx = stats.indexed_objects();
            let pct = recv * 100 / total;
            let mb = stats.received_bytes() as f64 / (1024.0 * 1024.0);
            // \r overwrites the line in place; final newline emitted on done.
            print!(
                "\r📥 {pct}%  {recv}/{total} objects  {idx} indexed  {mb:.1} MB",
            );
            std::io::stdout().flush().ok();
            if done {
                println!();
            }
            true
        });
        callbacks.sideband_progress(|line| {
            // Server-side messages like "remote: Counting objects: …".
            // libgit2 hands them already prefixed with carriage returns,
            // so just write verbatim.
            std::io::stderr().write_all(line).ok();
            true
        });

        let mut fetch_opts = git2::FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);
        // Allow shallow clones via env var (no CLI flag yet, but unblocks
        // huge repos like servo/chromium today). 0 / unset = full history.
        if let Ok(d) = std::env::var("TORII_CLONE_DEPTH") {
            if let Ok(depth) = d.parse::<i32>() {
                if depth > 0 {
                    fetch_opts.depth(depth);
                }
            }
        }

        println!("🔄 Cloning {url} → {target}");
        git2::build::RepoBuilder::new()
            .fetch_options(fetch_opts)
            .clone(url, std::path::Path::new(&target))?;

        Ok(())
    }

    /// Rename a branch
    pub fn rename_branch(&self, old_name: &str, new_name: &str) -> Result<()> {
        let mut branch = self.repo.find_branch(old_name, git2::BranchType::Local)
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        branch.rename(new_name, false)
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        Ok(())
    }

    /// Rewrite commit history with new dates
    pub fn rewrite_history(&self, start_date: &str, end_date: &str) -> Result<()> {
        println!("🔄 Rewriting commit history...");

        let start_ts = NaiveDateTime::parse_from_str(&format!("{} 00:00", start_date), "%Y-%m-%d %H:%M")
            .map_err(|e| crate::error::ToriiError::InvalidConfig(format!("Invalid start date: {}", e)))?
            .and_utc().timestamp();
        let end_ts = NaiveDateTime::parse_from_str(&format!("{} 23:59", end_date), "%Y-%m-%d %H:%M")
            .map_err(|e| crate::error::ToriiError::InvalidConfig(format!("Invalid end date: {}", e)))?
            .and_utc().timestamp();

        let mut revwalk = self.repo.revwalk()
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        revwalk.push_head().map_err(|e| crate::error::ToriiError::Git(e))?;
        revwalk.set_sorting(git2::Sort::REVERSE | git2::Sort::TIME)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        let oids: Vec<git2::Oid> = revwalk
            .filter_map(|r| r.ok())
            .collect();

        let total = oids.len();
        if total == 0 { return Ok(()); }

        let interval = (end_ts - start_ts) / (total as i64 - 1).max(1);

        // Walk oldest→newest, rewrite each commit with new timestamp
        let mut old_to_new: std::collections::HashMap<git2::Oid, git2::Oid> = std::collections::HashMap::new();

        for (i, oid) in oids.iter().enumerate() {
            let commit = self.repo.find_commit(*oid)
                .map_err(|e| crate::error::ToriiError::Git(e))?;

            let new_ts = start_ts + (i as i64 * interval);
            let new_time = git2::Time::new(new_ts, 0);

            let author = commit.author();
            let committer = commit.committer();
            let new_author = git2::Signature::new(
                author.name().unwrap_or(""),
                author.email().unwrap_or(""),
                &new_time,
            ).map_err(|e| crate::error::ToriiError::Git(e))?;
            let new_committer = git2::Signature::new(
                committer.name().unwrap_or(""),
                committer.email().unwrap_or(""),
                &new_time,
            ).map_err(|e| crate::error::ToriiError::Git(e))?;

            let tree = commit.tree().map_err(|e| crate::error::ToriiError::Git(e))?;
            let parents: Vec<git2::Commit> = commit.parent_ids()
                .filter_map(|pid| old_to_new.get(&pid).and_then(|new_pid| self.repo.find_commit(*new_pid).ok())
                    .or_else(|| self.repo.find_commit(pid).ok()))
                .collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

            let new_oid = self.repo.commit(
                None,
                &new_author,
                &new_committer,
                commit.message().unwrap_or(""),
                &tree,
                &parent_refs,
            ).map_err(|e| crate::error::ToriiError::Git(e))?;

            old_to_new.insert(*oid, new_oid);
        }

        // Update HEAD to point to the new tip
        if let Some(new_tip) = oids.last().and_then(|oid| old_to_new.get(oid)) {
            let head = self.repo.head().map_err(|e| crate::error::ToriiError::Git(e))?;
            if let Some(branch_name) = head.shorthand() {
                let refname = format!("refs/heads/{}", branch_name);
                self.repo.reference(&refname, *new_tip, true, "history rewrite")
                    .map_err(|e| crate::error::ToriiError::Git(e))?;
            }
        }

        println!("✅ Rewrote {} commits", total);
        println!("💡 Run 'torii sync --force' to update remote");
        Ok(())
    }

    /// Remove a file from the entire git history
    pub fn remove_file_from_history(&self, file_path: &str) -> Result<()> {
        println!("🗑️  Removing '{}' from entire history...", file_path);

        let mut revwalk = self.repo.revwalk()
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        revwalk.push_glob("refs/heads/*")
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        revwalk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        let oids: Vec<git2::Oid> = revwalk.filter_map(|r| r.ok()).collect();
        let mut old_to_new: std::collections::HashMap<git2::Oid, git2::Oid> = std::collections::HashMap::new();
        let mut modified = 0usize;

        for oid in &oids {
            let commit = self.repo.find_commit(*oid)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            let tree = commit.tree().map_err(|e| crate::error::ToriiError::Git(e))?;

            // Build new tree without the target file
            let new_tree_oid = remove_path_from_tree(&self.repo, &tree, file_path)?;

            let parents: Vec<git2::Commit> = commit.parent_ids()
                .filter_map(|pid| {
                    old_to_new.get(&pid)
                        .and_then(|new_pid| self.repo.find_commit(*new_pid).ok())
                        .or_else(|| self.repo.find_commit(pid).ok())
                })
                .collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

            let new_tree = self.repo.find_tree(new_tree_oid)
                .map_err(|e| crate::error::ToriiError::Git(e))?;

            if new_tree_oid != tree.id() {
                modified += 1;
            }

            let new_oid = self.repo.commit(
                None,
                &commit.author(),
                &commit.committer(),
                commit.message().unwrap_or(""),
                &new_tree,
                &parent_refs,
            ).map_err(|e| crate::error::ToriiError::Git(e))?;

            old_to_new.insert(*oid, new_oid);
        }

        // Update all branch refs
        let branches: Vec<(String, git2::Oid)> = self.repo.branches(Some(git2::BranchType::Local))
            .map_err(|e| crate::error::ToriiError::Git(e))?
            .filter_map(|b| b.ok())
            .filter_map(|(branch, _)| {
                let name = branch.name().ok()??.to_string();
                let oid = branch.get().target()?;
                Some((name, oid))
            })
            .collect();

        for (name, old_oid) in branches {
            if let Some(new_oid) = old_to_new.get(&old_oid) {
                let refname = format!("refs/heads/{}", name);
                let _ = self.repo.reference(&refname, *new_oid, true, "remove file from history");
            }
        }

        // Sync working tree + index with the new HEAD so subsequent operations
        // (notably `save --amend`) don't see the pre-rewrite state cached in the index.
        if let Ok(head) = self.repo.head() {
            if let Ok(commit) = head.peel_to_commit() {
                let mut checkout = git2::build::CheckoutBuilder::default();
                checkout.force();
                let _ = self.repo.checkout_tree(commit.as_object(), Some(&mut checkout));
                let mut index = self.repo.index().map_err(|e| crate::error::ToriiError::Git(e))?;
                let _ = index.read_tree(&commit.tree().map_err(|e| crate::error::ToriiError::Git(e))?);
                let _ = index.write();
            }
        }

        println!("✅ '{}' removed from {} commits", file_path, modified);
        println!("💡 Run 'torii history clean' then 'torii sync --force' to update remote");
        Ok(())
    }

    /// Clean up repository (expire reflogs, remove stale backup refs)
    pub fn clean_history(&self) -> Result<()> {
        println!("🧹 Cleaning repository...");

        // Remove filter-branch backup refs
        let orig_refs = self.repo.path().join("refs").join("original");
        if orig_refs.exists() {
            let _ = std::fs::remove_dir_all(&orig_refs);
        }

        // Expire reflogs by deleting reflog files (git2 has no expire API)
        let logs_dir = self.repo.path().join("logs");
        if logs_dir.exists() {
            let _ = remove_dir_contents(&logs_dir);
        }

        println!("✅ Repository cleaned");
        Ok(())
    }

    /// Verify remote repository status
    pub fn verify_remote(&self) -> Result<()> {
        println!("🔍 Verifying remote status...\n");

        let local_oid = self.repo.head()
            .map_err(|e| crate::error::ToriiError::Git(e))?
            .target()
            .ok_or_else(|| crate::error::ToriiError::InvalidConfig("No HEAD".to_string()))?;

        let local_hash = local_oid.to_string();

        // Find remote tracking ref
        let branch = self.get_current_branch()?;
        let remote_ref = format!("refs/remotes/origin/{}", branch);
        let remote_hash = self.repo.find_reference(&remote_ref)
            .ok()
            .and_then(|r| r.target())
            .map(|oid| oid.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        println!("Local HEAD:  {}", &local_hash[..7.min(local_hash.len())]);
        println!("Remote HEAD: {}", &remote_hash[..7.min(remote_hash.len())]);

        if local_hash == remote_hash {
            println!("\n✅ Local and remote are in sync");
        } else {
            println!("\n⚠️  Local and remote have diverged");
            println!("💡 Use 'torii sync --force' to push local changes");
        }

        Ok(())
    }

    /// Fetch from remote without merging
    pub fn fetch(&self) -> Result<()> {
        println!("🔄 Fetching from remote...");

        let mut remote = self.repo.find_remote("origin")
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let remote_url = remote.url().unwrap_or("").to_string();
        let callbacks = GitRepo::auth_callbacks_for(&remote_url);
        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        remote.fetch(&[] as &[&str], Some(&mut fetch_options), None)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("✅ Fetched from remote");
        Ok(())
    }

    /// Revert a specific commit
    pub fn revert_commit(&self, commit_hash: &str) -> Result<()> {
        println!("🔄 Reverting commit {}...", commit_hash);

        let commit = self.repo.revparse_single(commit_hash)
            .map_err(|e| crate::error::ToriiError::Git(e))?
            .peel_to_commit()
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        self.repo.revert(&commit, None)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        // Commit the revert
        let sig = self.repo.signature()
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let mut index = self.repo.index()
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let tree_oid = index.write_tree()
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let tree = self.repo.find_tree(tree_oid)
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let head = self.repo.head()
            .map_err(|e| crate::error::ToriiError::Git(e))?
            .peel_to_commit()
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let msg = format!("Revert \"{}\"", commit.summary().unwrap_or(commit_hash));
        self.repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &[&head])
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("✅ Reverted commit {}", &commit_hash[..7.min(commit_hash.len())]);
        Ok(())
    }

    /// Reset to a specific commit
    pub fn reset_commit(&self, commit_hash: &str, mode: &str) -> Result<()> {
        println!("🔄 Resetting to commit {} (mode: {})...", commit_hash, mode);

        let obj = self.repo.revparse_single(commit_hash)
            .map_err(|e| crate::error::ToriiError::Git(e))?;
        let commit = obj.peel_to_commit()
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        let reset_type = match mode {
            "soft" => git2::ResetType::Soft,
            "hard" => git2::ResetType::Hard,
            _ => git2::ResetType::Mixed,
        };

        self.repo.reset(commit.as_object(), reset_type, None)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        let short = commit.id().to_string();
        println!("✅ Reset to {}", &short[..7]);
        Ok(())
    }

    /// Merge a branch into current branch
    pub fn merge_branch(&self, branch_name: &str) -> Result<()> {
        let branch_ref = format!("refs/heads/{}", branch_name);
        let annotated = self.repo.find_reference(&branch_ref)
            .map_err(|e| crate::error::ToriiError::Git(e))
            .and_then(|r| self.repo.reference_to_annotated_commit(&r)
                .map_err(|e| crate::error::ToriiError::Git(e)))?;

        let (analysis, _) = self.repo.merge_analysis(&[&annotated])
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        if analysis.is_up_to_date() {
            println!("Already up to date.");
            return Ok(());
        }

        if analysis.is_fast_forward() {
            let refname = format!("refs/heads/{}", self.get_current_branch()?);
            let mut reference = self.repo.find_reference(&refname)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            reference.set_target(annotated.id(), "Fast-forward")
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            self.repo.set_head(&refname)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            self.repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            println!("✅ Fast-forward merged {}", branch_name);
        } else {
            // Normal merge commit
            self.repo.merge(&[&annotated], None, None)
                .map_err(|e| crate::error::ToriiError::Git(e))?;

            let mut index = self.repo.index()
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            if index.has_conflicts() {
                println!("⚠️  Merge conflicts detected. Resolve them and run: torii save -m \"merge\"");
                return Ok(());
            }

            let tree_oid = index.write_tree()
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            let tree = self.repo.find_tree(tree_oid)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            let sig = self.repo.signature()
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            let head = self.repo.head()
                .map_err(|e| crate::error::ToriiError::Git(e))?
                .peel_to_commit()
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            let branch_commit = self.repo.find_reference(&branch_ref)
                .map_err(|e| crate::error::ToriiError::Git(e))?
                .peel_to_commit()
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            let msg = format!("Merge branch '{}'", branch_name);
            self.repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &[&head, &branch_commit])
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            self.repo.cleanup_state()
                .map_err(|e| crate::error::ToriiError::Git(e))?;

            println!("✅ Merged {}", branch_name);
        }

        Ok(())
    }

    /// Rebase current branch onto another branch
    pub fn rebase_branch(&self, branch_name: &str) -> Result<()> {
        // git2's Rebase API is available — use it for non-interactive rebase
        let branch_ref = format!("refs/heads/{}", branch_name);
        let upstream = self.repo.find_reference(&branch_ref)
            .map_err(|e| crate::error::ToriiError::Git(e))
            .and_then(|r| self.repo.reference_to_annotated_commit(&r)
                .map_err(|e| crate::error::ToriiError::Git(e)))?;

        let mut rebase = self.repo.rebase(None, Some(&upstream), None, None)
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        let sig = self.repo.signature()
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        while let Some(op) = rebase.next() {
            op.map_err(|e| crate::error::ToriiError::Git(e))?;
            let index = self.repo.index()
                .map_err(|e| crate::error::ToriiError::Git(e))?;
            if index.has_conflicts() {
                println!("⚠️  Rebase conflict. Resolve conflicts and run: torii history rebase --continue");
                return Ok(());
            }
            rebase.commit(None, &sig, None)
                .map_err(|e| crate::error::ToriiError::Git(e))?;
        }

        rebase.finish(Some(&sig))
            .map_err(|e| crate::error::ToriiError::Git(e))?;

        println!("✅ Rebased onto {}", branch_name);
        Ok(())
    }

    /// List all tracked files in the index
    #[allow(dead_code)]
    pub fn ls(&self, path_filter: Option<&str>) -> Result<()> {
        let mut index = self.repo.index()?;
        index.read(true)?;

        let entries: Vec<_> = index.iter()
            .filter(|e| {
                let path = String::from_utf8_lossy(&e.path).to_string();
                match path_filter {
                    Some(filter) => path.starts_with(filter),
                    None => true,
                }
            })
            .collect();

        if entries.is_empty() {
            println!("No tracked files.");
            return Ok(());
        }

        for entry in &entries {
            let path = String::from_utf8_lossy(&entry.path);
            println!("{}", path);
        }

        println!();
        println!("{} tracked file(s)", entries.len());

        Ok(())
    }

    /// Show details of a commit, tag, or file at a given ref
    pub fn show(&self, object: Option<&str>) -> Result<()> {
        // Use the ref or default to HEAD
        let target = object.unwrap_or("HEAD");

        // Try to resolve as commit first
        let resolved = self.repo.revparse_single(target);

        match resolved {
            Ok(obj) => {
                match obj.kind() {
                    Some(git2::ObjectType::Commit) => {
                        let commit = obj.peel_to_commit()?;
                        let sig = commit.author();
                        let time = commit.time();
                        let timestamp = chrono::DateTime::from_timestamp(time.seconds(), 0)
                            .unwrap_or_default();

                        println!("commit {}", commit.id());
                        println!("Author: {} <{}>", sig.name().unwrap_or(""), sig.email().unwrap_or(""));
                        println!("Date:   {}", timestamp.format("%Y-%m-%d %H:%M:%S"));
                        println!();
                        println!("    {}", commit.message().unwrap_or("").trim());
                        println!();

                        // Show diff vs parent via git2
                        let commit_tree = commit.tree().ok();
                        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
                        if let Some(new_tree) = commit_tree {
                            let diff = self.repo.diff_tree_to_tree(
                                parent_tree.as_ref(),
                                Some(&new_tree),
                                None,
                            )?;
                            diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
                                let origin = line.origin();
                                let content = std::str::from_utf8(line.content()).unwrap_or("");
                                match origin {
                                    '+' => print!("\x1b[32m+{}\x1b[0m", content),
                                    '-' => print!("\x1b[31m-{}\x1b[0m", content),
                                    'H' | 'F' => print!("{}", content),
                                    _ => print!(" {}", content),
                                }
                                true
                            })?;
                        }
                    }
                    Some(git2::ObjectType::Tag) => {
                        let tag = obj.peel_to_tag()?;
                        println!("tag {}", tag.name().unwrap_or(""));
                        if let Some(tagger) = tag.tagger() {
                            println!("Tagger: {} <{}>", tagger.name().unwrap_or(""), tagger.email().unwrap_or(""));
                        }
                        println!();
                        println!("{}", tag.message().unwrap_or("").trim());
                    }
                    Some(git2::ObjectType::Blob) => {
                        let blob = obj.peel_to_blob()?;
                        let content = std::str::from_utf8(blob.content())
                            .unwrap_or("<binary>");
                        print!("{}", content);
                    }
                    _ => {
                        println!("{}", obj.id());
                    }
                }
            }
            Err(_) => {
                return Err(crate::error::ToriiError::InvalidConfig(
                    format!("Unknown ref or object: '{}'", target)
                ).into());
            }
        }

        Ok(())
    }
}

fn remove_path_from_tree(repo: &git2::Repository, tree: &git2::Tree, path: &str) -> crate::error::Result<git2::Oid> {
    let mut builder = repo.treebuilder(Some(tree))
        .map_err(|e| crate::error::ToriiError::Git(e))?;

    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() == 1 {
        // Leaf — just remove it
        let _ = builder.remove(parts[0]);
    } else {
        let dir = parts[0];
        let rest = parts[1];
        if let Ok(entry) = tree.get_name(dir).ok_or(git2::Error::from_str("not found")) {
            if let Ok(sub_tree) = repo.find_tree(entry.id()) {
                let new_sub_oid = remove_path_from_tree(repo, &sub_tree, rest)?;
                let new_sub = repo.find_tree(new_sub_oid)
                    .map_err(|e| crate::error::ToriiError::Git(e))?;
                if new_sub.is_empty() {
                    let _ = builder.remove(dir);
                } else {
                    builder.insert(dir, new_sub_oid, 0o040000)
                        .map_err(|e| crate::error::ToriiError::Git(e))?;
                }
            }
        }
    }

    builder.write().map_err(|e| crate::error::ToriiError::Git(e))
}

fn remove_dir_contents(dir: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            remove_dir_contents(&path)?;
            let _ = std::fs::remove_dir(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

// ============================================================================
// Rebase helpers — todo-file preprocessing + outcome detection
// ============================================================================

/// Read a torii todo file and emit two artefacts:
///   1. A new todo file safe to feed to `git rebase -i` (every `reword` line
///      becomes `pick`; we'll edit the message ourselves via GIT_EDITOR).
///   2. A map { full_sha → new_message } harvested from `reword <sha> <msg>`
///      entries. Lines without an inline message keep the original commit
///      subject so behaviour matches a no-op `pick`.
///
/// torii-only extension to git's todo grammar:
///   reword <sha> <new commit message on rest of line>
///
/// Standard git todo lines (`pick`, `edit`, `squash`, `fixup`, `drop`,
/// `exec ...`, etc.) pass through unchanged.
#[cfg(unix)]
fn preprocess_reword_todo(
    src: &std::path::Path,
) -> Result<(std::path::PathBuf, std::collections::HashMap<String, String>)> {
    let raw = std::fs::read_to_string(src).map_err(|e| {
        crate::error::ToriiError::InvalidConfig(format!("read todo {}: {}", src.display(), e))
    })?;

    let mut out_lines: Vec<String> = Vec::with_capacity(raw.lines().count());
    let mut reword_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out_lines.push(line.to_string());
            continue;
        }
        let mut parts = trimmed.splitn(3, ' ');
        let cmd = parts.next().unwrap_or("");
        if cmd != "reword" && cmd != "r" {
            out_lines.push(line.to_string());
            continue;
        }
        let sha = match parts.next() {
            Some(s) => s.to_string(),
            None => {
                out_lines.push(line.to_string());
                continue;
            }
        };
        let inline_msg = parts.next().unwrap_or("").trim().to_string();
        if !inline_msg.is_empty() {
            reword_map.insert(sha.clone(), inline_msg);
            // Keep `reword` so git invokes GIT_EDITOR; our shim replaces msg.
            out_lines.push(format!("reword {}", sha));
        } else {
            // No inline message → behave like a noop reword. Use `pick` so git
            // doesn't pause asking for a message we can't supply.
            out_lines.push(format!("pick {}", sha));
        }
    }

    let dest = std::env::temp_dir().join(format!(
        "torii-rebase-todo-{}.txt",
        std::process::id()
    ));
    std::fs::write(&dest, out_lines.join("\n") + "\n").map_err(|e| {
        crate::error::ToriiError::InvalidConfig(format!("write todo: {}", e))
    })?;

    Ok((dest, reword_map))
}

/// Install a GIT_EDITOR shim so that `reword` lines from the torii todo file
/// take effect.
///
/// Strategy: write a tiny shell script that, given the COMMIT_EDITMSG file
/// path, reads the *original* subject line from the file (git puts the
/// pre-reword message there), looks it up in our `subject → new_msg` map,
/// and if matched replaces the file content. Otherwise leaves untouched.
///
/// Matching by subject (rather than SHA) is robust to rebase rewriting
/// parent SHAs as it goes — git's HEAD changes during the rebase, but the
/// subject of the message-being-edited is the original.
#[cfg(unix)]
fn install_message_editor(
    cmd: &mut std::process::Command,
    reword_map: &std::collections::HashMap<String, String>,
    repo_path: &std::path::Path,
) -> Result<()> {
    if reword_map.is_empty() {
        return Ok(());
    }

    // Resolve each todo-file sha to the original commit subject. The map is
    // (subject → new message). Tabs + newlines in the new message are encoded.
    let mut subject_map: Vec<(String, String)> = Vec::with_capacity(reword_map.len());
    for (sha, new_msg) in reword_map {
        let subj = std::process::Command::new("git")
            .args(["log", "-1", "--format=%s", sha])
            .current_dir(repo_path)
            .output();
        let subject = match subj {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            }
            _ => continue, // unknown sha → skip silently (rebase may still proceed)
        };
        if subject.is_empty() {
            continue;
        }
        subject_map.push((subject, new_msg.clone()));
    }
    if subject_map.is_empty() {
        return Ok(());
    }

    let map_path = std::env::temp_dir().join(format!(
        "torii-rebase-rewords-{}.tsv",
        std::process::id()
    ));
    let mut map_text = String::new();
    for (subj, msg) in &subject_map {
        let safe_subj = subj.replace('\\', "\\\\").replace('\t', " ");
        let safe_msg = msg.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "    ");
        map_text.push_str(&format!("{}\t{}\n", safe_subj, safe_msg));
    }
    std::fs::write(&map_path, &map_text).map_err(|e| {
        crate::error::ToriiError::InvalidConfig(format!("write reword map: {}", e))
    })?;

    let shim_path = std::env::temp_dir().join(format!(
        "torii-rebase-editor-{}.sh",
        std::process::id()
    ));
    let shim_body = format!(
        r#"#!/bin/sh
# torii-generated rebase message editor.
# Usage: GIT_EDITOR <commit-msg-file>
set -e
MSG_FILE="$1"
[ -z "$MSG_FILE" ] && exit 0
MAP="{map}"
[ ! -f "$MAP" ] && exit 0
# Read the first non-comment line of the message — that's the subject.
SUBJ=$(awk '/^#/ {{ next }} /./ {{ print; exit }}' "$MSG_FILE")
[ -z "$SUBJ" ] && exit 0
while IFS=$(printf '\t') read -r KEY MSG; do
    if [ "$KEY" = "$SUBJ" ]; then
        printf '%b\n' "$(printf '%s' "$MSG" | sed 's/\\n/\n/g; s/\\\\/\\/g')" > "$MSG_FILE"
        exit 0
    fi
done < "$MAP"
exit 0
"#,
        map = map_path.display(),
    );
    std::fs::write(&shim_path, shim_body).map_err(|e| {
        crate::error::ToriiError::InvalidConfig(format!("write shim: {}", e))
    })?;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&shim_path)
        .map_err(|e| crate::error::ToriiError::InvalidConfig(format!("shim perms: {}", e)))?
        .permissions();
    perms.set_mode(0o755);
    let _ = std::fs::set_permissions(&shim_path, perms);

    cmd.env("GIT_EDITOR", &shim_path);
    Ok(())
}

/// Inspect the rebase state directory after `git rebase` returned. If a rebase
/// is still in progress (because of `edit`, conflicts, or `break`), surface a
/// clear next-step message instead of the misleading "Rebase complete".
fn report_rebase_outcome(repo_path: &std::path::Path, status: std::process::ExitStatus) {
    let merge_dir = repo_path.join(".git").join("rebase-merge");
    let apply_dir = repo_path.join(".git").join("rebase-apply");
    let in_progress = merge_dir.exists() || apply_dir.exists();

    if in_progress {
        let stopped_sha = std::fs::read_to_string(merge_dir.join("stopped-sha"))
            .or_else(|_| std::fs::read_to_string(apply_dir.join("stopped-sha")))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        eprintln!();
        if stopped_sha.is_empty() {
            eprintln!("⏸️  Rebase paused.");
        } else {
            eprintln!("⏸️  Rebase paused at {}.", &stopped_sha[..stopped_sha.len().min(7)]);
        }
        eprintln!("    Edit files / amend / cherry-pick as needed, then:");
        eprintln!("      torii history rebase --continue");
        eprintln!("    Or abort with:");
        eprintln!("      torii history rebase --abort");
        return;
    }

    if !status.success() {
        eprintln!("⚠️  Rebase ended with conflicts or was aborted.");
        return;
    }
    println!("✅ Rebase complete");
}
