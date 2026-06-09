use crate::error::{Result, ToriiError};
use git2::{IndexAddOption, Repository, Signature, StatusOptions};
use std::path::{Path, PathBuf};

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
            .map_err(|_| ToriiError::RepositoryNotFound(path_ref.display().to_string()))?;
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
        // .git/ always has a parent (the work tree) for non-bare repos.
        let repo_path = self
            .repo
            .path()
            .parent()
            .ok_or_else(|| {
                crate::error::ToriiError::RepoState(
                    "git directory has no parent (bare repo?)".to_string(),
                )
            })?
            .to_path_buf();
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
                if !buf.ends_with('\n') {
                    buf.push('\n');
                }
            }
        }

        if local_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&local_path) {
                buf.push_str("# ─── from .toriignore.local ───\n");
                buf.push_str(&content);
            }
        }

        std::fs::write(&exclude_path, buf)
            .map_err(|e| ToriiError::Fs(format!("write {}: {}", exclude_path.display(), e)))?;
        Ok(())
    }

    /// Add all changes to staging, respecting .toriignore.
    ///
    /// 0.7.7: `.torii/` is treated as reserved internal state and is
    /// never staged by `-a`, the same way `git add .` skips `.git/`.
    /// Before 0.7.7 snapshots lived in `.torii/snapshots/` inside the
    /// working tree, and a follow-up `torii save -am` silently
    /// absorbed the entire snapshot (one case in the wild was 681 MB
    /// pushed to origin before the receiving end aborted with a zlib
    /// stream error). Fix #1 in 0.7.7 moved snapshots out of the
    /// working tree; this skip is the defense-in-depth so anything
    /// else under `.torii/` (config.json, mirrors.json) is also kept
    /// out of `-a`. To stage a path under `.torii/` deliberately,
    /// pass it explicitly: `torii save .torii/config.json -m "..."`.
    pub fn add_all(&self) -> Result<()> {
        self.sync_toriignore()?;

        let mut index = self.repo.index()?;
        let mut skipped_torii = false;
        let cb = &mut |path: &Path, _matched: &[u8]| -> i32 {
            let s = path.to_string_lossy();
            if s == ".torii" || s.starts_with(".torii/") || s.starts_with(".torii\\") {
                skipped_torii = true;
                1 // skip
            } else {
                0 // add
            }
        };
        index.add_all(
            ["*"].iter(),
            IndexAddOption::DEFAULT,
            Some(cb as &mut git2::IndexMatchedPath<'_>),
        )?;
        index.write()?;
        if skipped_torii {
            eprintln!(
                "ℹ Skipped `.torii/` from staging (reserved for torii internal state). \
                       Pass paths explicitly if you really want to stage something inside it."
            );
        }
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

        commit_inner(&self.repo, Some("HEAD"), &sig, message, &tree, &parents)?;

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
        let head_oid = head_ref
            .target()
            .ok_or_else(|| ToriiError::RepoState("HEAD has no target".to_string()))?;
        let head_commit = self.repo.find_commit(head_oid)?;

        let parents: Vec<_> = head_commit.parents().collect();
        let parent_refs: Vec<_> = parents.iter().collect();

        let new_oid = commit_inner(&self.repo, None, &sig, message, &tree, &parent_refs)?;

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
    pub(crate) fn auth_callbacks_for<'a>(url: &str) -> git2::RemoteCallbacks<'a> {
        // Token lookups inside the credentials callback now go through
        // crate::auth::resolve_token; no global config load needed here.
        let url_owned = url.to_string();
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(move |cb_url, username_from_url, allowed_types| {
            let effective_url = if url_owned.is_empty() {
                cb_url
            } else {
                &url_owned
            };
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
    ///     Reused by clone / fetch / pull so every long-running op gives the
    ///     same visual feedback. Throttled to ~10 fps.
    pub(crate) fn attach_fetch_progress<'a>(callbacks: &mut git2::RemoteCallbacks<'a>) {
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
                let pct = (idx_deltas * 100).checked_div(total_deltas).unwrap_or(100);
                print!(
                    "\r🧩 Resolving deltas {pct}%  {idx_deltas}/{total_deltas}                       "
                );
            } else {
                let pct = (recv * 100).checked_div(total).unwrap_or(0);
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
    ///     Throttled to ~10 fps.
    pub(crate) fn attach_push_progress<'a>(callbacks: &mut git2::RemoteCallbacks<'a>) {
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

            let pct = (current * 100).checked_div(total).unwrap_or(0);
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
        if fetch_head_path
            .metadata()
            .map(|m| m.len() == 0)
            .unwrap_or(true)
        {
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
            self.repo
                .checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            return Ok(());
        }

        Err(ToriiError::RepoState(format!(
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
                 (Common with very large pushes over SSH; try HTTPS with a token.)",
            )));
        }

        // Push tags via git2 — enumerate local tags and push each one
        self.push_all_tags("origin", force)?;

        Ok(())
    }

    /// Push local tags to a remote, but **only the ones that aren't already
    /// in sync** with what the remote advertises.
    ///
    /// 0.7.8: pre-fix this function pushed every local tag on every
    /// `torii sync --push`. GitLab fires its `workflow:rules` on each tag
    /// ref the remote sees in a push event — even when the tag OID is
    /// identical to what was already there — so every release retriggered
    /// CI pipelines for every historical tag (v0.7.0, v0.7.1, v0.7.2, …).
    /// In the gitorii repo this was producing 4+ stale pipelines per
    /// release that all eventually got canceled, plus wasted runner time
    /// while they were queued.
    ///
    /// The fix: do an ls-remote (libgit2's `Remote::list` after
    /// `connect_auth`), compare each local tag's OID against the remote's,
    /// and push *only* the ones that differ (or don't exist remotely).
    /// One extra network round-trip in exchange for not retriggering N
    /// pipelines per release. With `force=true` the comparison still
    /// holds but the refspec gets a `+` prefix so rewritten tag OIDs
    /// (e.g. after `torii history reauthor --since ...`) still go through.
    pub fn push_all_tags(&self, remote_name: &str, force: bool) -> Result<()> {
        let local_tags = self.repo.tag_names(None)?;
        if local_tags.is_empty() {
            return Ok(());
        }

        // Build the local-OID map first so we don't keep the repo borrowed
        // while we open the remote connection.
        let local: std::collections::HashMap<String, git2::Oid> = local_tags
            .iter()
            .flatten()
            .filter_map(|t| {
                let refname = format!("refs/tags/{}", t);
                self.repo
                    .refname_to_id(&refname)
                    .ok()
                    .map(|oid| (t.to_string(), oid))
            })
            .collect();

        let mut remote = self.repo.find_remote(remote_name)?;
        let remote_url = remote.url().unwrap_or("").to_string();

        // ls-remote equivalent: connect, list, disconnect. We pass a fresh
        // set of auth callbacks since `connect_auth` consumes them.
        let remote_tags: std::collections::HashMap<String, git2::Oid> = {
            let callbacks = Self::auth_callbacks_for(&remote_url);
            remote.connect_auth(git2::Direction::Fetch, Some(callbacks), None)?;
            let list = remote.list()?;
            let map = list
                .iter()
                .filter_map(|h| {
                    let name = h.name();
                    name.strip_prefix("refs/tags/")
                        // Drop the peeled-tag suffix `^{}` libgit2 sometimes
                        // surfaces in remote listings for annotated tags —
                        // we only care about the tag object itself, which
                        // is the entry without the suffix.
                        .filter(|n| !n.ends_with("^{}"))
                        .map(|n| (n.to_string(), h.oid()))
                })
                .collect::<std::collections::HashMap<_, _>>();
            remote.disconnect()?;
            map
        };

        // Only the tags whose OID differs (or are missing remotely) get a
        // refspec. Tags that match are left alone — GitLab won't see a
        // push event for them, so its workflow:rules won't fire.
        let refspecs: Vec<String> = local
            .iter()
            .filter(|(name, oid)| remote_tags.get(*name) != Some(oid))
            .map(|(t, _)| {
                let r = format!("refs/tags/{}:refs/tags/{}", t, t);
                if force {
                    format!("+{}", r)
                } else {
                    r
                }
            })
            .collect();

        if refspecs.is_empty() {
            return Ok(());
        }

        let refspec_refs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
        let callbacks = Self::auth_callbacks_for(&remote_url);
        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(callbacks);
        remote
            .push(&refspec_refs, Some(&mut push_options))
            .map_err(ToriiError::Git)?;
        Ok(())
    }

    /// Get current branch name
    pub fn get_current_branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        let branch_name = head
            .shorthand()
            .ok_or_else(|| ToriiError::Git(git2::Error::from_str("Could not get branch name")))?;
        Ok(branch_name.to_string())
    }

    /// Get the underlying libgit2 repository handle. Crate-internal on
    /// purpose: the public API must not leak `git2` types (the future
    /// `VcsEngine` trait depends on that boundary).
    pub(crate) fn repository(&self) -> &Repository {
        &self.repo
    }

    /// Absolute path of the work tree, when the repository isn't bare.
    pub fn workdir(&self) -> Option<&Path> {
        self.repo.workdir()
    }

    /// Remote aliases configured in the repo, in config order, with their
    /// fetch URLs (`None` when a remote has no URL).
    pub fn remotes(&self) -> Result<Vec<(String, Option<String>)>> {
        let names = self.repo.remotes()?;
        let mut out = Vec::new();
        for name in names.iter().flatten() {
            let url = self
                .repo
                .find_remote(name)
                .ok()
                .and_then(|r| r.url().map(String::from));
            out.push((name.to_string(), url));
        }
        Ok(out)
    }

    /// Whether a remote alias with this name exists.
    pub fn remote_exists(&self, name: &str) -> bool {
        self.repo.find_remote(name).is_ok()
    }

    /// Fetch URL of a remote, when configured. Errors if the remote
    /// doesn't exist.
    pub fn remote_url(&self, name: &str) -> Result<Option<String>> {
        let remote = self.repo.find_remote(name)?;
        Ok(remote.url().map(String::from))
    }

    /// Register a new remote alias.
    pub fn remote_add(&self, name: &str, url: &str) -> Result<()> {
        self.repo.remote(name, url)?;
        Ok(())
    }

    /// Overwrite the URL of an existing remote alias.
    pub fn remote_set_url(&self, name: &str, url: &str) -> Result<()> {
        self.repo.remote_set_url(name, url)?;
        Ok(())
    }

    /// Drop a remote alias (does not touch the platform side).
    pub fn remote_delete(&self, name: &str) -> Result<()> {
        self.repo.remote_delete(name)?;
        Ok(())
    }

    /// Collect repository status — branch, HEAD summary, remote tracking
    /// and per-file changes. Pure data: rendering lives with the callers
    /// (CLI, TUI, IDE).
    pub fn status(&self) -> Result<RepoStatus> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        let statuses = self.repo.statuses(Some(&mut opts))?;

        let branch = self.get_current_branch()?;

        let head = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .map(|commit| HeadCommitInfo {
                short_id: format!("{:.7}", commit.id()),
                summary: commit
                    .message()
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string(),
                seconds_since_epoch: commit.time().seconds(),
            });

        let remote = self.repo.find_remote("origin").ok().and_then(|remote| {
            let url = remote.url()?;
            let name = url
                .split('/')
                .next_back()
                .unwrap_or("origin")
                .trim_end_matches(".git")
                .to_string();
            let ahead_behind =
                self.repo
                    .head()
                    .ok()
                    .and_then(|h| h.target())
                    .and_then(|local_oid| {
                        let remote_ref = self
                            .repo
                            .find_reference(&format!("refs/remotes/origin/{}", branch))
                            .ok()?;
                        let remote_oid = remote_ref.target()?;
                        self.repo.graph_ahead_behind(local_oid, remote_oid).ok()
                    });
            Some(RemoteStatusInfo { name, ahead_behind })
        });

        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry.path().unwrap_or("unknown").to_string();

            if status.is_index_new() || status.is_index_modified() || status.is_index_deleted() {
                let kind = if status.is_index_new() {
                    ChangeKind::Added
                } else if status.is_index_modified() {
                    ChangeKind::Modified
                } else {
                    ChangeKind::Deleted
                };
                staged.push(StatusEntry {
                    kind,
                    path: path.clone(),
                });
            }

            if status.is_wt_modified() || status.is_wt_deleted() {
                let kind = if status.is_wt_modified() {
                    ChangeKind::Modified
                } else {
                    ChangeKind::Deleted
                };
                unstaged.push(StatusEntry {
                    kind,
                    path: path.clone(),
                });
            }

            if status.is_wt_new() {
                untracked.push(path);
            }
        }

        Ok(RepoStatus {
            branch,
            head,
            remote,
            staged,
            unstaged,
            untracked,
        })
    }

    /// Get git signature using the unified resolver.
    fn get_signature(&self) -> Result<Signature<'static>> {
        resolve_signature(&self.repo)
    }
}

/// What kind of change a status entry represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// A single staged or unstaged path in the status report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusEntry {
    pub kind: ChangeKind,
    pub path: String,
}

/// HEAD commit summary for the status report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeadCommitInfo {
    pub short_id: String,
    /// First line of the commit message.
    pub summary: String,
    /// Commit time as seconds since the Unix epoch.
    pub seconds_since_epoch: i64,
}

/// Tracking state against `origin/<branch>`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoteStatusInfo {
    /// Remote repo name derived from the URL (without `.git`).
    pub name: String,
    /// `(ahead, behind)` vs the upstream branch, when it exists.
    pub ahead_behind: Option<(usize, usize)>,
}

/// Repository state decoupled from presentation — built by
/// [`GitRepo::status`], rendered by the CLI / TUI / IDE layers.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoStatus {
    pub branch: String,
    pub head: Option<HeadCommitInfo>,
    pub remote: Option<RemoteStatusInfo>,
    pub staged: Vec<StatusEntry>,
    pub unstaged: Vec<StatusEntry>,
    pub untracked: Vec<String>,
}

impl RepoStatus {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.untracked.is_empty()
    }
}

/// Resolve a commit signature for the current user, in this order:
///
///   1. Torii **local** config (`.torii/config.toml [user]` under the
///      repo's work tree) — set via `torii config set user.name X --local`.
///      Per-repo identity wins so users can keep a personal global and a
///      work-tree-specific override (e.g. `paski@paski.dev` globally,
///      `paski@employer.com` for the work repo).
///   2. Torii **global** config (`~/.config/torii/config.toml [user]`)
///      — set via `torii config set user.name X` without `--local`.
///   3. Git's own config chain (`.git/config` → `~/.gitconfig` →
///      `/etc/gitconfig`). Kept so users who already had a working
///      `git config` setup don't have to duplicate it.
///   4. Hard error. **No more silent fallback to "Torii User"** — that
///      placeholder was the root cause of an earlier author-fallback
///      bug. Bogus commits are worse than a clear error that prompts
///      the user to fix it.
///
/// Returns the signature ready to pass to `repo.commit(..)`.
///
/// Returns `Signature<'static>` deliberately: callers often need to
/// hold the signature past a subsequent `&mut repo` operation
/// (`stash_save2`, `commit` after another index op), and the
/// `'static` lifetime decouples it from the borrow used here. Possible
/// because `Signature::now` produces an owned signature.
// Porcelain tier (see `commit_inner`): takes a raw `git2::Repository` for
// the TUI's sake. Domain callers should go through `GitRepo` methods.
#[doc(hidden)]
pub fn resolve_signature(repo: &git2::Repository) -> Result<Signature<'static>> {
    // `load_local(repo)` already merges global underneath local — the
    // `--local` override naturally wins. Fall back to global-only if
    // the repo is bare (no workdir to host a `.torii/config.toml`).
    //
    // Pre-0.7.14: only `load_global()` was consulted here, so `--local`
    // edits to user.name/user.email were silently ignored. That's the
    // bug fixed by switching to `load_local()`.
    let tc = repo
        .workdir()
        .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
        .unwrap_or_else(|| crate::config::ToriiConfig::load_global().unwrap_or_default());

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

/// Create a commit, signing it with GPG when the active torii config
/// has `git.sign_commits = true`. Returns the new commit's OID.
///
/// When signing is off this is a thin wrapper around
/// [`git2::Repository::commit`] — same behaviour, same ref update.
/// When signing is on it uses libgit2's `commit_create_buffer` +
/// `commit_signed` pair and manually updates the named ref (libgit2
/// doesn't do ref-bookkeeping for signed commits, see
/// <https://libgit2.org/libgit2/#HEAD/group/commit/git_commit_signed>).
///
/// This is the **fix for the GPG-sign no-op bug** in 0.7.13 and
/// earlier: those versions accepted `git.sign_commits = true` in the
/// config layer but never honoured it at commit time. From 0.7.14
/// onwards the flag actually drives this branch.
// Porcelain tier: still consumed by the TUI in the `gitorii` crate, which
// predates the lib/bin split and works on raw `git2` handles. Hidden from
// the documented domain API — slated for migration behind `GitRepo`.
#[doc(hidden)]
pub fn commit_inner(
    repo: &git2::Repository,
    update_ref: Option<&str>,
    sig: &Signature,
    message: &str,
    tree: &git2::Tree,
    parents: &[&git2::Commit],
) -> Result<git2::Oid> {
    // Convenience wrapper for the common case where author == committer.
    commit_inner_split(repo, update_ref, sig, sig, message, tree, parents)
}

/// Like [`commit_inner`] but takes the author and committer separately.
/// Used by history-rewriting ops (reauthor, rewrite, remove-file) that
/// preserve the original author while changing the committer.
pub(crate) fn commit_inner_split(
    repo: &git2::Repository,
    update_ref: Option<&str>,
    author: &Signature,
    committer: &Signature,
    message: &str,
    tree: &git2::Tree,
    parents: &[&git2::Commit],
) -> Result<git2::Oid> {
    // Cheap config read — load_local already merges global underneath
    // local, so a per-repo override of `git.sign_commits` works as
    // expected.
    let tc = repo
        .workdir()
        .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
        .unwrap_or_else(|| crate::config::ToriiConfig::load_global().unwrap_or_default());

    // 0.7.35 — per-invocation override set by `torii save -S` /
    // `--no-sign`. The CLI handler sets this env var around the
    // commit call so we don't have to thread a `force_sign: Option<bool>`
    // through every commit path (initial commit, amend, history
    // rewrite, the TUI's save action, …). Returned to its prior
    // state by the CLI guard after the commit.
    let should_sign = match std::env::var("TORII_SIGN_OVERRIDE").ok().as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => tc.git.sign_commits,
    };

    if !should_sign {
        return Ok(repo.commit(update_ref, author, committer, message, tree, parents)?);
    }

    // Signing path: need a key id.
    let key = tc
        .git
        .gpg_key
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            ToriiError::InvalidConfig(
                "git.sign_commits = true but git.gpg_key is not set. Configure with:\n  \
             torii config set git.gpg_key <YOUR-KEY-ID>"
                    .to_string(),
            )
        })?;

    // Build the commit object as bytes, sign it, then write the signed
    // commit object out.
    let buffer = repo.commit_create_buffer(author, committer, message, tree, parents)?;
    let buffer_str = std::str::from_utf8(&buffer).map_err(|e| {
        ToriiError::RepoState(format!(
            "commit buffer not valid UTF-8 (cannot GPG-sign): {}",
            e
        ))
    })?;
    // 0.7.35 — honour `git.gpg_program` so users on systems where gpg
    // is shipped as gpg2 (or under a custom path) can point at it.
    let signature = crate::gpg::sign_blob(&buffer, key, tc.git.gpg_program.as_deref())?;
    let new_oid = repo.commit_signed(buffer_str, &signature, Some("gpgsig"))?;

    // libgit2's commit_signed leaves ref updates to the caller. Move
    // the requested ref (typically HEAD, which resolves through the
    // symbolic chain to the current branch).
    if let Some(name) = update_ref {
        // Resolve "HEAD" to the underlying branch ref so we update the
        // branch, not the symbolic HEAD itself. For non-HEAD names we
        // assume the caller passed a direct ref path.
        let target_ref = if name == "HEAD" {
            match repo.head() {
                Ok(h) => h
                    .name()
                    .map(String::from)
                    .unwrap_or_else(|| "refs/heads/main".to_string()),
                // Unborn HEAD: first commit. Use the default branch
                // name from torii config so this matches `torii init`.
                Err(_) => format!("refs/heads/{}", tc.git.default_branch),
            }
        } else {
            name.to_string()
        };
        repo.reference(&target_ref, new_oid, true, "torii signed commit")?;
    }

    Ok(new_oid)
}

#[cfg(test)]
mod add_all_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn add_all_skips_dot_torii_directory() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        // Bare-minimum init: a regular non-bare repo with no initial
        // commit (add_all only writes the index, doesn't require HEAD).
        let _ = git2::Repository::init(repo_path).unwrap();
        let gitorii = GitRepo::open(repo_path).unwrap();

        // Real change that SHOULD be staged.
        fs::write(repo_path.join("README.md"), "hello").unwrap();
        // Bogus .torii/ content that must NEVER be staged by -a.
        fs::create_dir_all(repo_path.join(".torii/snapshots/x")).unwrap();
        fs::write(
            repo_path.join(".torii/snapshots/x/big.bin"),
            vec![0u8; 1024],
        )
        .unwrap();
        fs::write(repo_path.join(".torii/config.json"), "{}").unwrap();

        gitorii.add_all().unwrap();

        let index = gitorii.repo.index().unwrap();
        let staged: Vec<String> = index
            .iter()
            .map(|e| String::from_utf8_lossy(&e.path).into_owned())
            .collect();

        assert!(
            staged.iter().any(|p| p == "README.md"),
            "README.md should be staged, got: {:?}",
            staged
        );
        assert!(
            !staged.iter().any(|p| p.starts_with(".torii")),
            ".torii/* must not be staged by add_all, got: {:?}",
            staged
        );
    }
}
