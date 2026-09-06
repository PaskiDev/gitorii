//! Clone view state + loaders/actions.
//!
//! The view is a browser over the repositories an account can reach,
//! with a URL box under it for everything the browser cannot name: a
//! host torii has no listing for, someone else's repository, a path on
//! this machine. Both ends produce the same thing — a URL and a
//! destination — and hand it to the same worker.

use super::*;

/// One repository as the list shows it.
pub struct CloneRepo {
    /// `owner/repo`, read off the web URL rather than the API's `name`.
    /// GitLab's `name` is a display name and may hold spaces, and every
    /// platform's is the bare repository without its owner, which is
    /// not enough to tell two `api` repositories apart.
    pub slug: String,
    pub description: Option<String>,
    pub private: bool,
    pub ssh_url: String,
    pub https_url: String,
}

impl CloneRepo {
    /// The URL to clone from, for the protocol currently chosen.
    /// Falls back to the other one when the platform only returned one.
    pub fn url_for(&self, ssh: bool) -> &str {
        let (first, second) = if ssh {
            (&self.ssh_url, &self.https_url)
        } else {
            (&self.https_url, &self.ssh_url)
        };
        if first.is_empty() {
            second
        } else {
            first
        }
    }

    /// The directory name a clone of this repository gets by default.
    pub fn dir_name(&self) -> &str {
        self.slug.rsplit('/').next().unwrap_or(&self.slug)
    }
}

/// One row of the platform dropdown.
pub struct ClonePlatformRow {
    pub name: &'static str,
    pub has_token: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CloneFocus {
    /// Browsing the repository list.
    List,
    /// The platform dropdown is open over it.
    Platforms,
    /// Typing in the filter box.
    Filter,
    /// Typing a URL by hand.
    Url,
    /// Editing the destination path.
    Dest,
}

/// What the clone worker is doing, as it does it. The modal stays open
/// on every ending so the outcome is read rather than glimpsed.
#[derive(Debug, Clone)]
pub enum CloneStatus {
    /// Handed to libgit2; nothing measurable has come back yet.
    Starting,
    Working(crate::core_extensions::CloneProgress),
    /// Cloned into `path`.
    Done(String),
    /// Stopped on request. Nothing is left on disk: libgit2 removes the
    /// tree it made, and the clone never starts onto a path that was
    /// already there.
    Cancelled,
    Error(String),
}

/// An in-flight (or just-finished) clone.
#[derive(Debug, Clone)]
pub struct CloneRun {
    pub url: String,
    pub dest: String,
    pub status: CloneStatus,
    /// Raised when the user asks the clone to stop. The worker's
    /// progress sink reads it on its next report and tells libgit2 to
    /// give up, so a stop lands within one report rather than at once —
    /// which is why the modal says it is stopping instead of vanishing.
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CloneRun {
    pub fn cancel_asked(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub struct CloneState {
    pub focus: CloneFocus,

    /// Which host the list is of, and where the dropdown's cursor sits.
    pub platform: String,
    pub platform_idx: usize,

    pub repos: Vec<CloneRepo>,
    pub idx: usize,
    /// Indices into `repos` that match `filter`, recomputed when it
    /// changes. Empty `filter` means every repository, and `filtered`
    /// is then the whole range rather than a special case.
    pub filtered: Vec<usize>,
    pub filter: String,

    pub loading: bool,
    pub error: Option<String>,

    /// A URL typed or pasted by hand. Cloning from here ignores the
    /// list entirely.
    pub url_input: String,

    /// Where the clone will land. Derived from `clone.base_dir` and the
    /// selected repository until the user edits it, after which it is
    /// left alone — a field that overwrites what you typed every time
    /// the cursor moves is not an editable field.
    pub dest_input: String,
    pub dest_edited: bool,

    /// SSH when true, HTTPS otherwise. Seeded the way the CLI seeds it.
    pub ssh: bool,

    /// `clone.base_dir`, read once when the view is entered. The
    /// destination is re-derived on every keystroke and every move of
    /// the caret; re-reading the config file that often would be a file
    /// read per keypress for a value that cannot change while the view
    /// is open.
    pub base_dir: String,

    /// The clone that is running, or the one that just finished.
    pub run: Option<CloneRun>,
}

impl Default for CloneState {
    fn default() -> Self {
        Self {
            focus: CloneFocus::List,
            platform: "github".to_string(),
            platform_idx: 0,
            repos: Vec::new(),
            idx: 0,
            filtered: Vec::new(),
            filter: String::new(),
            loading: false,
            error: None,
            url_input: String::new(),
            dest_input: String::new(),
            dest_edited: false,
            ssh: true,
            base_dir: ".".to_string(),
            run: None,
        }
    }
}

impl CloneState {
    /// The repository under the caret, honouring the filter.
    pub fn selected(&self) -> Option<&CloneRepo> {
        self.repos.get(*self.filtered.get(self.idx)?)
    }

    pub fn visible_len(&self) -> usize {
        self.filtered.len()
    }
}

/// The git hosts the view offers, in a fixed order so the menu does not
/// reshuffle under the cursor.
///
/// `cargo` is in `auth::PROVIDERS` but is a package registry, not a
/// place repositories are cloned from, so it is not here. Nothing here
/// predicts *which* hosts can list repositories: that lives in the core
/// clients, and a second copy of it in the UI would be the one that
/// goes stale. A host with no listing answers with its own "not yet
/// wired" message, which the view shows over the URL box.
pub const CLONE_PLATFORMS: &[&str] = &[
    "github",
    "gitlab",
    "codeberg",
    "forgejo",
    "gitea",
    "bitbucket",
    "sourcehut",
    "azure",
    "radicle",
];

impl App {
    /// Entering the view: pick a host, then go and ask it.
    pub fn load_clone_enter(&mut self) {
        // A clone that finished is not news the next time the view is
        // opened; one still running is, so it stays.
        if matches!(
            self.clone_view.run.as_ref().map(|r| &r.status),
            Some(CloneStatus::Done(_)) | Some(CloneStatus::Error(_)) | Some(CloneStatus::Cancelled)
        ) {
            self.clone_view.run = None;
        }

        self.clone_view.focus = CloneFocus::List;
        let cfg = crate::config::ToriiConfig::load_global().unwrap_or_default();
        self.clone_view.ssh = clone_prefers_ssh(&cfg);
        self.clone_view.base_dir = cfg.clone.base_dir.clone();

        // Land on a host this machine can actually ask, rather than on
        // whichever one happens to be first.
        if !self.clone_platform_has_token(&self.clone_view.platform) {
            if let Some(p) = CLONE_PLATFORMS
                .iter()
                .find(|p| self.clone_platform_has_token(p))
            {
                self.clone_view.platform = (*p).to_string();
            }
        }
        self.clone_view.platform_idx = CLONE_PLATFORMS
            .iter()
            .position(|p| *p == self.clone_view.platform)
            .unwrap_or(0);

        if self.clone_view.repos.is_empty() && self.clone_repos_rx.is_none() {
            self.load_clone_repos();
        } else {
            self.clone_sync_dest();
        }
    }

    /// The message a host with no token gets instead of a request. The
    /// request would only come back with the same thing, one round trip
    /// later, and this way the answer names the command that fixes it.
    fn clone_no_token_error(platform: &str) -> String {
        format!(
            "no {0} token here — run: torii auth set {0} <token>",
            platform
        )
    }

    pub fn clone_platform_has_token(&self, platform: &str) -> bool {
        // The local node is the auth. Asking for a token would skip
        // the listing that `rad ls` already has.
        if platform == "radicle" {
            return true;
        }
        crate::auth::resolve_token(platform, &self.repo_path)
            .value
            .is_some()
    }

    /// The dropdown's rows: every host, and whether there is a token for
    /// it here. A host with no token is still listed and still
    /// selectable — the error it gives says what to run, which is more
    /// use than a row that cannot be reached.
    pub fn clone_platform_rows(&self) -> Vec<ClonePlatformRow> {
        CLONE_PLATFORMS
            .iter()
            .map(|name| ClonePlatformRow {
                name,
                has_token: self.clone_platform_has_token(name),
            })
            .collect()
    }

    /// Ask the current platform for the account's repositories.
    ///
    /// The client is built inside the worker rather than out here,
    /// because it is not `Send` and cannot cross the thread boundary.
    pub fn load_clone_repos(&mut self) {
        self.clone_view.repos.clear();
        self.clone_view.filtered.clear();
        self.clone_view.idx = 0;
        self.clone_view.error = None;
        self.clone_view.loading = true;

        let platform = self.clone_view.platform.clone();
        if !self.clone_platform_has_token(&platform) {
            self.clone_view.loading = false;
            self.clone_view.error = Some(Self::clone_no_token_error(&platform));
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();
        self.clone_repos_rx = Some(rx);

        std::thread::spawn(move || {
            let result = crate::remote::get_platform_client(&platform)
                .and_then(|client| client.list_repos())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Take what the worker found and put it in the list.
    pub fn clone_apply_repos(&mut self, repos: Vec<crate::remote::RemoteRepo>) {
        self.clone_view.repos = repos
            .into_iter()
            .map(|r| CloneRepo {
                slug: clone_slug(&r.url, &r.name),
                description: r.description,
                private: !matches!(r.visibility, crate::remote::Visibility::Public),
                ssh_url: r.ssh_url,
                https_url: r.clone_url,
            })
            .collect();
        // Alphabetical: the API's order is its own business, and a list
        // you have to scan is easier to scan sorted.
        self.clone_view
            .repos
            .sort_by(|a, b| a.slug.to_lowercase().cmp(&b.slug.to_lowercase()));
        self.clone_view.idx = 0;
        self.clone_refilter();
    }

    /// Recompute which rows the filter lets through, keeping the caret
    /// in range.
    pub fn clone_refilter(&mut self) {
        let needle = self.clone_view.filter.to_lowercase();
        self.clone_view.filtered = self
            .clone_view
            .repos
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                if needle.is_empty() {
                    return true;
                }
                r.slug.to_lowercase().contains(&needle)
                    || r.description
                        .as_deref()
                        .map(|d| d.to_lowercase().contains(&needle))
                        .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();
        if self.clone_view.idx >= self.clone_view.filtered.len() {
            self.clone_view.idx = self.clone_view.filtered.len().saturating_sub(1);
        }
        self.clone_sync_dest();
    }

    pub fn clone_move_up(&mut self) {
        if self.clone_view.idx > 0 {
            self.clone_view.idx -= 1;
            self.clone_sync_dest();
        }
    }

    pub fn clone_move_down(&mut self) {
        if self.clone_view.idx + 1 < self.clone_view.visible_len() {
            self.clone_view.idx += 1;
            self.clone_sync_dest();
        }
    }

    /// Re-derive the destination from `clone.base_dir` and whatever is
    /// about to be cloned — unless the user has taken the field over.
    ///
    /// "Whatever is about to be cloned" is the URL box when it has
    /// something in it and the selected repository otherwise, which is
    /// the same order `clone_target` resolves in. The two agreeing is
    /// the point: the path on screen is the path the clone will use.
    pub fn clone_sync_dest(&mut self) {
        if self.clone_view.dest_edited {
            return;
        }
        let typed = self.clone_view.url_input.trim();
        let name = if !typed.is_empty() {
            clone_dir_from_url(typed).to_string()
        } else {
            match self.clone_view.selected() {
                Some(r) => r.dir_name().to_string(),
                None => return,
            }
        };
        self.clone_view.dest_input = self.clone_dest_for(&name);
    }

    /// `<clone.base_dir>/<name>`, with `~` expanded. A `.` base gives
    /// `./name`, which is what `git clone` would have done.
    pub fn clone_dest_for(&self, name: &str) -> String {
        let base = self.clone_view.base_dir.trim();
        let base = if base.is_empty() { "." } else { base };
        crate::worktree::expand_tilde(std::path::Path::new(base))
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    /// What `Enter` is about to do: the URL to clone and where to put
    /// it, or nothing when neither the list nor the box has an answer.
    pub fn clone_target(&self) -> Option<(String, String)> {
        let typed = self.clone_view.url_input.trim();
        let url = if !typed.is_empty() {
            typed.to_string()
        } else {
            self.clone_view
                .selected()?
                .url_for(self.clone_view.ssh)
                .to_string()
        };
        if url.is_empty() {
            return None;
        }
        let dest = self.clone_view.dest_input.trim();
        let dest = if dest.is_empty() {
            self.clone_dest_for(clone_dir_from_url(&url))
        } else {
            dest.to_string()
        };
        Some((url, dest))
    }

    /// Hand the clone to a worker and watch it from the modal.
    ///
    /// Refuses rather than starts when a clone is already running: two
    /// at once share one progress line and one destination field, and
    /// the second would silently take both over.
    pub fn clone_start(&mut self) {
        if self.clone_run_in_flight() {
            self.set_status("✗ a clone is already running");
            return;
        }
        let Some((url, dest)) = self.clone_target() else {
            self.set_status("✗ nothing selected and no URL typed");
            return;
        };
        if std::path::Path::new(&dest).exists() {
            self.set_status(format!("✗ {} already exists", dest));
            self.clone_view.error = Some(format!(
                "{} already exists — edit the destination with [d]",
                dest
            ));
            return;
        }

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.clone_view.error = None;
        self.clone_view.run = Some(CloneRun {
            url: url.clone(),
            dest: dest.clone(),
            status: CloneStatus::Starting,
            cancel: std::sync::Arc::clone(&cancel),
        });
        self.log_event(format!("clone {} → {}", url, dest), EventKind::Info);

        let (tx, rx) = std::sync::mpsc::channel();
        self.clone_run_rx = Some(rx);

        std::thread::spawn(move || {
            use std::sync::atomic::Ordering;

            let result = if crate::radicle::is_radicle_url(&url) {
                crate::radicle::rad_clone_cancelable(
                    &url,
                    Some(dest.as_str()),
                    Some(cancel.as_ref()),
                )
            } else {
                // libgit2 reports far faster than a terminal can be redrawn.
                // The floor is the same 100 ms the CLI's printing sink uses,
                // and the last report of each phase is let through whatever
                // the clock says, so the bar always reaches its end.
                let mut last = std::time::Instant::now();
                let sink_tx = tx.clone();
                let sink_cancel = std::sync::Arc::clone(&cancel);
                crate::core::GitRepo::clone_repo_with_progress(
                    &url,
                    Some(dest.as_str()),
                    &mut |p| {
                        // The stop is read every time, throttle or no
                        // throttle: a request to stop should not have to
                        // wait for the next report the user can see.
                        if sink_cancel.load(Ordering::Relaxed) {
                            return false;
                        }
                        if !p.done() && last.elapsed().as_millis() < 100 {
                            return true;
                        }
                        last = std::time::Instant::now();
                        let _ = sink_tx.send(CloneStatus::Working(p));
                        true
                    },
                )
            };
            // A clone that finished is finished, even if the stop was
            // asked for while the last objects were landing: the
            // repository is on disk either way, and calling that
            // "stopped" would send the user looking for something that
            // is already there.
            let _ = tx.send(match result {
                Ok(()) => CloneStatus::Done(dest),
                Err(_) if cancel.load(Ordering::Relaxed) => CloneStatus::Cancelled,
                Err(e) => CloneStatus::Error(e.to_string()),
            });
        });
    }

    /// Ask a running clone to stop. Returns whether there was one.
    ///
    /// The worker sees this on its next progress report, so the modal
    /// stays up saying so rather than closing on a clone that is still
    /// pulling objects.
    pub fn clone_cancel(&mut self) -> bool {
        if !self.clone_run_in_flight() {
            return false;
        }
        // The flag comes out of the borrow before anything else on
        // `self` is touched: the status line and the event log below
        // both want it mutably, and the run is only being read for this.
        let Some(cancel) = self
            .clone_view
            .run
            .as_ref()
            .map(|r| std::sync::Arc::clone(&r.cancel))
        else {
            return false;
        };
        // `swap` is the whole handshake: it raises the flag and says
        // whether it was already up, so a second Esc gets an answer
        // rather than looking like a key that did nothing.
        if cancel.swap(true, std::sync::atomic::Ordering::Relaxed) {
            self.set_status("stopping — waiting for the transfer to answer");
            return true;
        }
        self.log_event("clone: stop asked", EventKind::Info);
        self.set_status("stopping the clone…");
        true
    }

    pub fn clone_run_in_flight(&self) -> bool {
        matches!(
            self.clone_view.run.as_ref().map(|r| &r.status),
            Some(CloneStatus::Starting) | Some(CloneStatus::Working(_))
        )
    }
}

/// SSH or HTTPS, decided the way `torii clone` decides it: the config
/// says so, or the presence of a key does.
pub fn clone_prefers_ssh(cfg: &crate::config::ToriiConfig) -> bool {
    if cfg.mirror.default_protocol == "https" {
        false
    } else {
        crate::ssh::SshHelper::has_ssh_keys()
    }
}

/// The directory name a URL clones into, matching git's own rule.
pub fn clone_dir_from_url(url: &str) -> &str {
    url.trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
}

/// `owner/repo` from a repository's web URL, falling back to the API's
/// own name when the URL is missing or shaped unexpectedly.
fn clone_slug(url: &str, name: &str) -> String {
    // Everything after the scheme, or the whole string when there is
    // none — an scp-like `git@host:owner/repo` has no `://` to cut at.
    let path = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .trim_end_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // [host, owner, …, repo] — everything after the host, so a GitLab
    // subgroup keeps its path instead of losing its middle.
    if segments.len() >= 3 {
        return segments[1..].join("/");
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(slug: &str) -> CloneRepo {
        CloneRepo {
            slug: slug.into(),
            description: None,
            private: false,
            ssh_url: format!("git@host:{}.git", slug),
            https_url: format!("https://host/{}.git", slug),
        }
    }

    /// The path on screen is the path the clone will use — until the
    /// user says otherwise, and then it stays said.
    #[test]
    fn the_destination_follows_the_caret_until_it_is_edited() {
        let mut app = App::test_blank();
        app.clone_view.repos = vec![repo("o/one"), repo("o/two")];
        app.clone_refilter();
        assert_eq!(app.clone_view.dest_input, "./one");

        app.clone_move_down();
        assert_eq!(app.clone_view.dest_input, "./two");

        app.clone_view.dest_input = "/srv/mine".into();
        app.clone_view.dest_edited = true;
        app.clone_move_up();
        assert_eq!(
            app.clone_view.dest_input, "/srv/mine",
            "the caret wrote over a field the user had taken"
        );
    }

    #[test]
    fn a_typed_url_wins_over_the_selection_and_the_destination_follows() {
        let mut app = App::test_blank();
        app.clone_view.repos = vec![repo("o/one")];
        app.clone_refilter();
        app.clone_view.ssh = false;

        let (url, dest) = app.clone_target().expect("a selected repo is a target");
        assert_eq!(url, "https://host/o/one.git");
        assert_eq!(dest, "./one");

        app.clone_view.url_input = "https://elsewhere/x/other.git".into();
        app.clone_sync_dest();
        let (url, dest) = app.clone_target().expect("a typed URL is a target");
        assert_eq!(url, "https://elsewhere/x/other.git");
        assert_eq!(dest, "./other");
    }

    #[test]
    fn the_protocol_switch_picks_the_other_url() {
        let mut app = App::test_blank();
        app.clone_view.repos = vec![repo("o/one")];
        app.clone_refilter();

        app.clone_view.ssh = true;
        assert_eq!(app.clone_target().unwrap().0, "git@host:o/one.git");
        app.clone_view.ssh = false;
        assert_eq!(app.clone_target().unwrap().0, "https://host/o/one.git");
    }

    #[test]
    fn the_filter_reads_the_slug_and_the_description() {
        let mut app = App::test_blank();
        let mut described = repo("o/three");
        described.description = Some("the parser".into());
        app.clone_view.repos = vec![repo("o/one"), repo("o/two"), described];

        app.clone_view.filter = "two".into();
        app.clone_refilter();
        assert_eq!(app.clone_view.filtered, vec![1]);

        // The description is searched too, so a repository can be found
        // by what it does and not only by what it is called.
        app.clone_view.filter = "parser".into();
        app.clone_refilter();
        assert_eq!(app.clone_view.filtered, vec![2]);

        // A filter that matches nothing leaves nothing selected rather
        // than a caret pointing past the end.
        app.clone_view.filter = "nothing".into();
        app.clone_refilter();
        assert!(app.clone_view.filtered.is_empty());
        assert!(app.clone_view.selected().is_none());
    }

    /// Esc over a running clone raises the flag its worker reads; over
    /// anything else it is not a stop at all, and says so, so the Esc
    /// chain can go on to the next thing it means.
    #[test]
    fn a_stop_is_only_asked_of_a_clone_that_is_running() {
        let mut app = App::test_blank();
        assert!(!app.clone_cancel(), "there was nothing to stop");

        app.clone_view.run = Some(CloneRun {
            url: "https://host/o/r.git".into(),
            dest: "./r".into(),
            status: CloneStatus::Starting,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        assert!(app.clone_cancel());
        assert!(app.clone_view.run.as_ref().unwrap().cancel_asked());

        // Asking twice is not an error, and must not unset it.
        assert!(app.clone_cancel());
        assert!(app.clone_view.run.as_ref().unwrap().cancel_asked());

        // A run that has already ended is not one to stop, whatever the
        // flag on it still says.
        app.clone_view.run.as_mut().unwrap().status = CloneStatus::Cancelled;
        assert!(!app.clone_cancel());
    }

    /// A destination that exists is refused before the worker starts,
    /// not by libgit2 once it is halfway in.
    #[test]
    fn an_existing_destination_is_refused() {
        let mut app = App::test_blank();
        app.clone_view.repos = vec![repo("o/one")];
        app.clone_refilter();
        // The repository the test itself is running in will do.
        app.clone_view.dest_input = ".".into();
        app.clone_view.dest_edited = true;

        app.clone_start();
        assert!(
            app.clone_view.run.is_none(),
            "a clone was started onto a path that already exists"
        );
        assert!(app.clone_view.error.is_some());
    }

    #[test]
    fn a_slug_is_everything_after_the_host() {
        assert_eq!(
            clone_slug("https://github.com/torvalds/linux", "linux"),
            "torvalds/linux"
        );
        // A GitLab subgroup is part of the path, not noise to drop.
        assert_eq!(
            clone_slug("https://gitlab.com/group/sub/api", "api"),
            "group/sub/api"
        );
        // Nothing usable in the URL — the API's own name stands in.
        assert_eq!(clone_slug("", "lonely"), "lonely");
        assert_eq!(clone_slug("https://codeberg.org/", "lonely"), "lonely");
    }

    #[test]
    fn the_directory_comes_off_the_end_of_the_url() {
        assert_eq!(clone_dir_from_url("https://github.com/u/repo.git"), "repo");
        assert_eq!(clone_dir_from_url("https://github.com/u/repo"), "repo");
        assert_eq!(clone_dir_from_url("git@github.com:u/repo.git"), "repo");
        assert_eq!(clone_dir_from_url("/tmp/source.git"), "source");
        assert_eq!(clone_dir_from_url("https://github.com/u/repo/"), "repo");
        assert_eq!(
            clone_dir_from_url("rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"),
            "z3gqcJUoA1n9HaHKufZs5FCSGazv5"
        );
    }

    #[test]
    fn a_radicle_slug_is_the_project_name() {
        assert_eq!(
            clone_slug("rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5", "heartwood"),
            "heartwood"
        );
    }

    #[test]
    fn radicle_needs_no_token_and_is_on_the_menu() {
        assert!(CLONE_PLATFORMS.contains(&"radicle"));
        let app = App::test_blank();
        assert!(app.clone_platform_has_token("radicle"));
    }

    #[test]
    fn a_repo_falls_back_to_the_protocol_it_has() {
        let only_https = CloneRepo {
            slug: "u/r".into(),
            description: None,
            private: false,
            ssh_url: String::new(),
            https_url: "https://host/u/r.git".into(),
        };
        // Asked for SSH, but the platform only gave one URL.
        assert_eq!(only_https.url_for(true), "https://host/u/r.git");
        assert_eq!(only_https.url_for(false), "https://host/u/r.git");
        assert_eq!(only_https.dir_name(), "r");
    }
}
