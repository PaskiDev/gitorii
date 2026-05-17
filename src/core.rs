use git2::{Repository, Signature, IndexAddOption, StatusOptions};
use std::path::{Path, PathBuf};
use crate::error::{Result, ToriiError};

pub struct GitRepo {
    pub(crate) repo: Repository,
}

impl GitRepo {
    /// Initialize a new git repository.
    ///
    /// Sets the initial branch from `git.default_branch` in the global torii
    /// config (default `main`) instead of libgit2's hard-coded `master`.
    pub fn init<P: AsRef<Path>>(path: P) -> Result<Self> {
        let initial = crate::config::ToriiConfig::load_global()
            .map(|c| c.git.default_branch)
            .unwrap_or_else(|_| "main".to_string());
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head(&initial);
        let repo = Repository::init_opts(path, &opts)?;
        Ok(Self { repo })
    }

    /// Open an existing repository
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let repo = Repository::discover(path_ref)
            .map_err(|_| ToriiError::RepositoryNotFound(
                path_ref.display().to_string()
            ))?;
        let git_repo = Self { repo };
        // Sync .toriignore on every open so all git operations respect it
        git_repo.sync_toriignore()?;
        Ok(git_repo)
    }

    /// Sync .toriignore (+ .toriignore.local) → .git/info/exclude so git
    /// itself respects the patterns. Always force-excludes `.toriignore.local`
    /// itself — local rules are machine-private and must never be committed.
    /// Called automatically on open and before staging.
    pub fn sync_toriignore(&self) -> Result<()> {
        let repo_path = self.repo.path().parent().unwrap().to_path_buf();
        let public_path = repo_path.join(".toriignore");
        let local_path = repo_path.join(".toriignore.local");
        let exclude_path = self.repo.path().join("info").join("exclude");

        let mut buf = String::from(
            "# Synced from .toriignore by torii — do not edit manually\n\
             # Local-only rules — never commit\n\
             .toriignore.local\n",
        );

        if public_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&public_path) {
                buf.push_str(&content);
                if !buf.ends_with('\n') { buf.push('\n'); }
            }
        }

        if local_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&local_path) {
                buf.push_str("# ─── from .toriignore.local ───\n");
                buf.push_str(&content);
            }
        }

        let _ = std::fs::write(&exclude_path, buf);
        Ok(())
    }

    /// Add all changes to staging, respecting .toriignore
    pub fn add_all(&self) -> Result<()> {
        self.sync_toriignore()?;

        let mut index = self.repo.index()?;
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.write()?;
        Ok(())
    }

    /// Add specific files to staging
    pub fn add<P: AsRef<Path>>(&self, paths: &[P]) -> Result<()> {
        let mut index = self.repo.index()?;
        for path in paths {
            index.add_path(path.as_ref())?;
        }
        index.write()?;
        Ok(())
    }

    /// Unstage paths — equivalent to `git reset HEAD -- <paths>` (or `git rm --cached`
    /// for files that were never committed). Keeps files on disk.
    pub fn unstage<P: AsRef<Path>>(&self, paths: &[P]) -> Result<()> {
        match self.repo.head() {
            Ok(head) => {
                let head_obj = head.peel(git2::ObjectType::Commit)?;
                let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_ref()).collect();
                self.repo.reset_default(Some(&head_obj), path_refs.iter())?;
            }
            Err(_) => {
                // No HEAD yet (root commit not made) — drop entries from index directly
                let mut index = self.repo.index()?;
                for path in paths {
                    let _ = index.remove_path(path.as_ref());
                }
                index.write()?;
            }
        }
        Ok(())
    }

    /// Unstage all paths currently in the index.
    pub fn unstage_all(&self) -> Result<()> {
        let index = self.repo.index()?;
        let paths: Vec<PathBuf> = index
            .iter()
            .filter_map(|e| std::str::from_utf8(&e.path).ok().map(PathBuf::from))
            .collect();
        if paths.is_empty() {
            return Ok(());
        }
        self.unstage(&paths)
    }

    /// Commit changes
    pub fn commit(&self, message: &str) -> Result<()> {
        let sig = self.get_signature()?;
        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;

        // Root commit (empty repo) has no parent
        let parent_commit = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };

        let parents: Vec<&git2::Commit> = parent_commit.iter().collect();

        self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &parents,
        )?;

        Ok(())
    }

    /// Amend the previous commit
    pub fn commit_amend(&self, message: &str) -> Result<()> {
        let sig = self.get_signature()?;
        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;

        // Resolve HEAD via the branch ref directly to dodge stale internal state
        // after operations like history rewrite.
        let head_ref = self.repo.head()?;
        let head_oid = head_ref.target()
            .ok_or_else(|| ToriiError::InvalidConfig("HEAD has no target".to_string()))?;
        let head_commit = self.repo.find_commit(head_oid)?;

        let parents: Vec<_> = head_commit.parents().collect();
        let parent_refs: Vec<_> = parents.iter().collect();

        let new_oid = self.repo.commit(
            None,
            &sig,
            &sig,
            message,
            &tree,
            &parent_refs,
        )?;

        // Move HEAD (or the underlying branch ref) to the new commit explicitly,
        // bypassing libgit2's "first parent" check that fails when HEAD was
        // rewritten just before this call.
        if head_ref.is_branch() {
            if let Some(refname) = head_ref.name() {
                self.repo.reference(refname, new_oid, true, "amend")?;
            }
        } else {
            self.repo.set_head_detached(new_oid)?;
        }

        Ok(())
    }
    
    /// Build auth callbacks for SSH and HTTPS token auth.
    /// Pass the remote URL so the correct token is selected per host.
    pub fn auth_callbacks_for<'a>(url: &str) -> git2::RemoteCallbacks<'a> {
        let cfg = crate::config::ToriiConfig::load_global().unwrap_or_default();
        let url_owned = url.to_string();
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(move |cb_url, username_from_url, allowed_types| {
            let effective_url = if url_owned.is_empty() { cb_url } else { &url_owned };
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
                // Route through the unified token resolver so local
                // overrides and env vars are honoured here too.
                let provider = if effective_url.contains("github.com") {
                    "github"
                } else if effective_url.contains("gitlab.com") {
                    "gitlab"
                } else if effective_url.contains("codeberg.org") {
                    "codeberg"
                } else {
                    "gitea"
                };
                if let Some(token) = crate::auth::resolve_token(provider, ".").value {
                    return git2::Cred::userpass_plaintext("oauth2", &token);
                }
            }
            git2::Cred::default()
        });
        callbacks
    }

    /// Attach progress reporters to an existing `RemoteCallbacks`. Covers:
    ///   - transfer_progress: pack receive + indexing + delta resolution
    ///   - sideband_progress: server messages like "Counting objects: …"
    /// Reused by clone / fetch / pull so every long-running op gives the
    /// same visual feedback. Throttled to ~10 fps.
    pub fn attach_fetch_progress<'a>(callbacks: &mut git2::RemoteCallbacks<'a>) {
        use std::cell::RefCell;
        use std::io::Write;
        use std::time::Instant;

        let last_print = RefCell::new(Instant::now());
        callbacks.transfer_progress(move |stats| {
            let mut last = last_print.borrow_mut();
            let total = stats.total_objects();
            let recv = stats.received_objects();
            let idx = stats.indexed_objects();
            let total_deltas = stats.total_deltas();
            let idx_deltas = stats.indexed_deltas();
            let receiving_done = total > 0 && recv == total && idx == total;
            let deltas_done = total_deltas == 0 || idx_deltas == total_deltas;
            let done = receiving_done && deltas_done;

            if !done && last.elapsed().as_millis() < 100 {
                return true;
            }
            *last = Instant::now();

            let mb = stats.received_bytes() as f64 / (1024.0 * 1024.0);
            // Two phases: receiving objects, then resolving deltas.
            // libgit2 reports both via the same callback, so emit whichever
            // is currently advancing.
            if total_deltas > 0 && recv == total {
                let pct = if total_deltas > 0 { idx_deltas * 100 / total_deltas } else { 100 };
                print!(
                    "\r🧩 Resolving deltas {pct}%  {idx_deltas}/{total_deltas}                       "
                );
            } else {
                let pct = if total > 0 { recv * 100 / total } else { 0 };
                print!(
                    "\r📥 {pct}%  {recv}/{total} objects  {idx} indexed  {mb:.1} MB       ",
                );
            }
            std::io::stdout().flush().ok();
            if done {
                println!();
            }
            true
        });
        callbacks.sideband_progress(|line| {
            std::io::stderr().write_all(line).ok();
            true
        });
    }

    /// Attach progress reporters for push (different libgit2 callback set).
    ///   - push_transfer_progress: pack upload
    ///   - sideband_progress: server messages
    /// Throttled to ~10 fps.
    pub fn attach_push_progress<'a>(callbacks: &mut git2::RemoteCallbacks<'a>) {
        use std::cell::RefCell;
        use std::io::Write;
        use std::time::Instant;

        let last_print = RefCell::new(Instant::now());
        callbacks.push_transfer_progress(move |current, total, bytes| {
            let mut last = last_print.borrow_mut();
            let done = total > 0 && current == total;
            if !done && last.elapsed().as_millis() < 100 {
                return;
            }
            *last = Instant::now();

            let pct = if total > 0 { current * 100 / total } else { 0 };
            let mb = bytes as f64 / (1024.0 * 1024.0);
            print!("\r📤 {pct}%  {current}/{total} objects  {mb:.1} MB       ");
            std::io::stdout().flush().ok();
            if done {
                println!();
            }
        });
        callbacks.sideband_progress(|line| {
            std::io::stderr().write_all(line).ok();
            true
        });
    }

    /// Pull from remote (fetch + fast-forward merge of current branch)
    pub fn pull(&self) -> Result<()> {
        let branch = self.get_current_branch()?;
        let mut remote = self.repo.find_remote("origin")?;

        let remote_url = remote.url().unwrap_or("").to_string();
        let mut callbacks = Self::auth_callbacks_for(&remote_url);
        Self::attach_fetch_progress(&mut callbacks);

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        remote.fetch(&[&branch], Some(&mut fetch_options), None)?;

        // Empty / freshly-created remotes leave FETCH_HEAD as a 0-byte file
        // and libgit2 then refuses to parse it as a reference. Treat that
        // as "nothing to pull" — same outcome as up-to-date.
        let fetch_head_path = self.repo.path().join("FETCH_HEAD");
        if fetch_head_path.metadata().map(|m| m.len() == 0).unwrap_or(true) {
            return Ok(());
        }
        let fetch_head = self.repo.find_reference("FETCH_HEAD")?;
        let fetch_commit = self.repo.reference_to_annotated_commit(&fetch_head)?;

        let analysis = self.repo.merge_analysis(&[&fetch_commit])?;

        if analysis.0.is_up_to_date() {
            return Ok(());
        }
        if analysis.0.is_fast_forward() {
            let refname = format!("refs/heads/{}", branch);
            let mut reference = self.repo.find_reference(&refname)?;
            reference.set_target(fetch_commit.id(), "Fast-forward")?;
            self.repo.set_head(&refname)?;
            self.repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            return Ok(());
        }

        Err(ToriiError::InvalidConfig(format!(
            "Pull not fast-forward on '{}'. Local and remote diverged. Use 'torii sync {} --merge' or 'torii sync {} --rebase' to integrate.",
            branch, branch, branch
        )))
    }

    /// Push to remote
    pub fn push(&self, force: bool) -> Result<()> {
        let mut remote = self.repo.find_remote("origin")?;
        let branch = self.get_current_branch()?;

        let refspec = if force {
            format!("+refs/heads/{}:refs/heads/{}", branch, branch)
        } else {
            format!("refs/heads/{}:refs/heads/{}", branch, branch)
        };

        let remote_url = remote.url().unwrap_or("").to_string();
        let mut callbacks = Self::auth_callbacks_for(&remote_url);

        // Capture per-ref rejections AND track that the callback was actually
        // fired. libgit2's `remote.push()` can return Ok in three failure
        // modes our previous fix didn't cover:
        //   1. Server-side rejection — caught by push_update_reference (msg)
        //   2. Connection dropped mid-pack on huge pushes — push_update_reference
        //      *never fires*, so we must also assert it was called at all
        //   3. SSH transport silently no-ops on auth fail — same: callback skip
        // We now treat "callback never fired" as failure too, since a real
        // accepted push always invokes the callback exactly once per refspec.
        let rejections: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let acknowledged: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let rejections_cb = rejections.clone();
        let acknowledged_cb = acknowledged.clone();
        callbacks.push_update_reference(move |refname, status| {
            acknowledged_cb.lock().unwrap().push(refname.to_string());
            if let Some(msg) = status {
                rejections_cb
                    .lock()
                    .unwrap()
                    .push((refname.to_string(), msg.to_string()));
            }
            Ok(())
        });

        // Live pack-upload progress + server sideband. Same look as fetch.
        Self::attach_push_progress(&mut callbacks);

        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(callbacks);

        // Push branch
        remote.push(&[&refspec], Some(&mut push_options))?;

        // Surface server-side rejections that libgit2 swallows silently.
        let rejected = rejections.lock().unwrap();
        if !rejected.is_empty() {
            let detail = rejected
                .iter()
                .map(|(r, m)| format!("{} → {}", r, m))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ToriiError::Git(git2::Error::from_str(&format!(
                "push rejected by remote: {}",
                detail
            ))));
        }

        // No callback at all = transport silently dropped the push. Caught
        // in the wild pushing 3GB to GitLab over SSH where libgit2 returned
        // Ok with zero refs ever acknowledged by the server.
        let acks = acknowledged.lock().unwrap();
        if acks.is_empty() {
            return Err(ToriiError::Git(git2::Error::from_str(
                "push completed without server acknowledging any refs — \
                 transport may have failed silently. Check network / auth and retry. \
                 (Common with very large pushes over SSH; try HTTPS with a token.)"
            )));
        }

        // Push tags via git2 — enumerate local tags and push each one
        self.push_all_tags_via_git2("origin", force)?;

        Ok(())
    }

    /// Push all local tags to a remote using git2 (no subprocess needed)
    pub fn push_all_tags_via_git2(&self, remote_name: &str, force: bool) -> Result<()> {
        let tags = self.repo.tag_names(None)?;
        if tags.is_empty() {
            return Ok(());
        }
        let mut remote = self.repo.find_remote(remote_name)?;
        let remote_url = remote.url().unwrap_or("").to_string();
        let refspecs: Vec<String> = tags.iter()
            .flatten()
            .map(|t| {
                let r = format!("refs/tags/{}:refs/tags/{}", t, t);
                if force { format!("+{}", r) } else { r }
            })
            .collect();
        let refspec_refs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
        if !refspec_refs.is_empty() {
            let callbacks = Self::auth_callbacks_for(&remote_url);
            let mut push_options = git2::PushOptions::new();
            push_options.remote_callbacks(callbacks);
            if let Err(e) = remote.push(&refspec_refs, Some(&mut push_options)) {
                eprintln!("⚠️  Tag push failed: {}", e);
            }
        }
        Ok(())
    }

    /// Get current branch name
    pub fn get_current_branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        let branch_name = head.shorthand()
            .ok_or_else(|| ToriiError::Git(git2::Error::from_str("Could not get branch name")))?;
        Ok(branch_name.to_string())
    }

    /// Get the repository reference
    pub fn repository(&self) -> &Repository {
        &self.repo
    }

    /// Show repository status with context and suggestions
    pub fn status(&self) -> Result<()> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        let statuses = self.repo.statuses(Some(&mut opts))?;

        // Header
        println!("📊 Repository Status\n");
        
        // Branch and commit info
        let branch = self.get_current_branch()?;
        println!("Branch: {}", branch);
        
        // Get latest commit info
        if let Ok(head) = self.repo.head() {
            if let Ok(commit) = head.peel_to_commit() {
                let msg = commit.message().unwrap_or("").lines().next().unwrap_or("");
                let time = commit.time();
                let timestamp = chrono::DateTime::from_timestamp(time.seconds(), 0)
                    .unwrap_or_default();
                let now = chrono::Utc::now();
                let duration = now.signed_duration_since(timestamp);
                
                let time_ago = if duration.num_days() > 0 {
                    format!("{} days ago", duration.num_days())
                } else if duration.num_hours() > 0 {
                    format!("{} hours ago", duration.num_hours())
                } else if duration.num_minutes() > 0 {
                    format!("{} minutes ago", duration.num_minutes())
                } else {
                    "just now".to_string()
                };
                
                let short_id = format!("{:.7}", commit.id());
                println!("Commit: {} - \"{}\" ({})", short_id, msg, time_ago);
            }
        }
        
        // Remote status
        if let Ok(remote) = self.repo.find_remote("origin") {
            if let Some(url) = remote.url() {
                let remote_name = url.split('/').last().unwrap_or("origin");
                print!("Remote: {}", remote_name.trim_end_matches(".git"));
                
                // Check if ahead/behind
                if let Ok(head) = self.repo.head() {
                    if let Ok(local_oid) = head.target().ok_or("No target") {
                        let remote_branch = format!("refs/remotes/origin/{}", branch);
                        if let Ok(remote_ref) = self.repo.find_reference(&remote_branch) {
                            if let Ok(remote_oid) = remote_ref.target().ok_or("No target") {
                                if let Ok((ahead, behind)) = self.repo.graph_ahead_behind(local_oid, remote_oid) {
                                    if ahead > 0 || behind > 0 {
                                        print!(" (");
                                        if ahead > 0 {
                                            print!("{} ahead", ahead);
                                        }
                                        if ahead > 0 && behind > 0 {
                                            print!(", ");
                                        }
                                        if behind > 0 {
                                            print!("{} behind", behind);
                                        }
                                        print!(")");
                                    } else {
                                        print!(" (up to date)");
                                    }
                                }
                            }
                        }
                    }
                }
                println!();
            }
        }
        
        println!();

        // Categorize changes
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry.path().unwrap_or("unknown").to_string();

            if status.is_index_new() || status.is_index_modified() || status.is_index_deleted() {
                let prefix = if status.is_index_new() {
                    "A "
                } else if status.is_index_modified() {
                    "M "
                } else {
                    "D "
                };
                staged.push(format!("{} {}", prefix, path));
            }
            
            if status.is_wt_modified() || status.is_wt_deleted() {
                let prefix = if status.is_wt_modified() {
                    "M "
                } else {
                    "D "
                };
                unstaged.push(format!("{} {}", prefix, path));
            }
            
            if status.is_wt_new() {
                untracked.push(format!("?? {}", path));
            }
        }

        // Show status
        let is_clean = staged.is_empty() && unstaged.is_empty() && untracked.is_empty();

        if is_clean {
            println!("✨ Working tree clean");
        } else {
            if !staged.is_empty() {
                println!("✅ Changes staged for commit:");
                for file in &staged {
                    println!("  {}", file);
                }
                println!();
            }

            if !unstaged.is_empty() {
                println!("📝 Changes not staged:");
                for file in &unstaged {
                    println!("  {}", file);
                }
                println!();
            }

            if !untracked.is_empty() {
                println!("📦 Untracked files:");
                for file in &untracked {
                    println!("  {}", file);
                }
                println!();
            }
        }

        // Next steps suggestions
        println!("💡 Next steps:");
        if is_clean {
            println!("  • Start new work: torii branch feature-name -c");
            println!("  • Update from remote: torii sync");
            println!("  • Create snapshot: torii snapshot create");
        } else if !staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
            println!("  • Commit staged changes: torii save -m \"message\"");
            println!("  • See staged changes: torii diff --staged");
        } else if !unstaged.is_empty() || !untracked.is_empty() {
            println!("  • Save all changes: torii save -am \"message\"");
            println!("  • See changes: torii diff");
            if !staged.is_empty() {
                println!("  • Commit only staged: torii save -m \"message\"");
            }
        }

        Ok(())
    }

    /// Get git signature using the unified resolver.
    fn get_signature(&self) -> Result<Signature<'static>> {
        resolve_signature(&self.repo)
    }
}

/// Resolve a commit signature for the current user, in this order:
///
///   1. Torii config (`~/.config/torii/config.toml [user]` —
///      `user.name`/`user.email`). This is the source of truth for any
///      identity the user set via `torii config set …`.
///   2. Git's own config chain (`.git/config` → `~/.gitconfig` →
///      `/etc/gitconfig`). Kept so users who already had a working
///      `git config` setup don't have to duplicate it.
///   3. Hard error. **No more silent fallback to "Torii User"** — that
///      placeholder was the root cause of
///      `docs/BUG_COMMIT_AUTHOR_FALLBACK.md`. Bogus commits are worse
///      than a clear error that prompts the user to fix it.
///
/// Returns the signature ready to pass to `repo.commit(..)`.
///
/// Returns `Signature<'static>` deliberately: callers often need to
/// hold the signature past a subsequent `&mut repo` operation
/// (`stash_save2`, `commit` after another index op), and the
/// `'static` lifetime decouples it from the borrow used here. Possible
/// because `Signature::now` produces an owned signature.
pub fn resolve_signature(repo: &git2::Repository) -> Result<Signature<'static>> {
    let tc = crate::config::ToriiConfig::load_global().unwrap_or_default();

    // Filter out empty strings at every level. `torii config set user.name ""`
    // counts as "not configured" — the previous behaviour passed the empty
    // string straight to libgit2, which then rejected the commit with a
    // generic "Signature cannot have an empty name or email" instead of our
    // torii-flavoured fix-it hint.
    let name = tc
        .user
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            repo.config()
                .ok()
                .and_then(|c| c.get_string("user.name").ok())
                .filter(|s| !s.trim().is_empty())
        })
        .ok_or_else(|| {
            crate::error::ToriiError::InvalidConfig(
                "user.name not configured. Set it with:\n  \
                 torii config set user.name \"Your Name\""
                    .to_string(),
            )
        })?;

    let email = tc
        .user
        .email
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            repo.config()
                .ok()
                .and_then(|c| c.get_string("user.email").ok())
                .filter(|s| !s.trim().is_empty())
        })
        .ok_or_else(|| {
            crate::error::ToriiError::InvalidConfig(
                "user.email not configured. Set it with:\n  \
                 torii config set user.email \"you@example.com\""
                    .to_string(),
            )
        })?;

    Ok(Signature::now(&name, &email)?)
}
