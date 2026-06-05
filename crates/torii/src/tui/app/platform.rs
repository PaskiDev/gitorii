//! Platform view (CI) state + loaders/actions.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformSubTab {
    Pipelines,
    Jobs,
    Releases,
    Packages,
    Runners,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformFocus {
    /// Browsing the active sub-tab list.
    List,
    /// Drill-down inside Jobs (entered from a pipeline). The list now
    /// shows jobs of `active_pipeline_id`; Esc returns to Pipelines.
    JobsOfPipeline,
    /// Drill-down inside a single job's log/trace.
    JobLog,
    /// Remote-selector popup is open over the view.
    RemotePopup,
    /// 0.7.26: contextual-actions dropdown (cancel/retry/pause/etc.)
    /// opened with `o`. Replaces the individual c/x/a/t/d keybinds
    /// from 0.7.24 — those collided across sub-tabs (c=cancel in
    /// Pipelines vs c=pause in Runners) and weren't discoverable.
    OpsDropdown,
    /// 0.7.26: filter dropdown (status + branch) opened with `f`.
    FilterDropdown,
}

pub struct PlatformState {
    pub sub_tab: PlatformSubTab,
    pub focus: PlatformFocus,

    /// Remote name currently consulted (e.g. "origin", "github", "upstream").
    /// Always one concrete remote in 0.7.12; `--remote all` is CLI-only.
    pub remote: String,
    /// Auto-discovered remote list (populated on view enter).
    pub remotes: Vec<String>,
    pub remote_popup_idx: usize,

    /// Resolved platform/owner/repo for `remote`. Updated when remote changes.
    pub platform: String,
    pub owner: String,
    pub repo_name: String,

    pub pipelines: Vec<crate::pipeline::Pipeline>,
    pub pipelines_idx: usize,
    pub jobs: Vec<crate::pipeline::Job>,
    pub jobs_idx: usize,
    pub releases: Vec<crate::release::Release>,
    pub releases_idx: usize,
    pub packages: Vec<crate::package::Package>,
    pub packages_idx: usize,
    pub runners: Vec<crate::runner::Runner>,
    pub runners_idx: usize,

    /// Set when we drilled from a pipeline row into its jobs.
    pub active_pipeline_id: Option<u64>,
    /// Job trace text + scroll, when focus == JobLog.
    pub job_log: Option<String>,
    pub job_log_scroll: u16,

    pub loading: bool,
    pub error: Option<String>,

    /// 0.7.24 — gate for contextual actions. While true, ops keys are
    /// ignored so the user can't fire five retries by mashing Enter.
    /// 0.7.27: the result/feedback no longer lives here — it goes to the
    /// app-wide `status_msg` (the single source of "what just happened")
    /// and to the event log, like every other view does.
    pub action_in_flight: bool,

    /// 0.7.24 — auto-refresh of the active list while in List focus.
    /// Off by default; the user toggles with `p`. Interval is 10s by
    /// default, tunable later via settings.
    pub auto_refresh: bool,
    pub last_poll_at: Option<std::time::Instant>,

    /// 0.7.24 — live tail of the job log. Enabled automatically when
    /// drilling into a running/pending job; stops when the job reaches
    /// a terminal status. `job_log_user_scrolled` blocks auto-bottom
    /// so the user can read past lines without being yanked forward.
    pub job_log_live: bool,
    pub job_log_last_poll_at: Option<std::time::Instant>,
    pub job_log_user_scrolled: bool,
    /// Status of the job the log belongs to (for the "● live" indicator
    /// and for deciding when to stop polling).
    pub job_log_status: String,

    /// 0.7.24 — list filters. `filter_status` is set from a dropdown
    /// (None / running / failed / success / pending) and passed to the
    /// platform API. `filter_branch` toggles client-side filtering by
    /// the local repo's current branch.
    pub filter_status: Option<String>,
    pub filter_branch_only: bool,

    /// 0.7.26 — index of the currently highlighted dropdown row when
    /// `focus == OpsDropdown` or `FilterDropdown`. Reset to 0 on open.
    pub dropdown_idx: usize,
}

impl Default for PlatformState {
    fn default() -> Self {
        Self {
            sub_tab: PlatformSubTab::Pipelines,
            focus: PlatformFocus::List,
            remote: "origin".to_string(),
            remotes: vec![],
            remote_popup_idx: 0,
            platform: String::new(),
            owner: String::new(),
            repo_name: String::new(),
            pipelines: vec![],
            pipelines_idx: 0,
            jobs: vec![],
            jobs_idx: 0,
            releases: vec![],
            releases_idx: 0,
            packages: vec![],
            packages_idx: 0,
            runners: vec![],
            runners_idx: 0,
            active_pipeline_id: None,
            job_log: None,
            job_log_scroll: 0,
            loading: false,
            error: None,
            action_in_flight: false,
            auto_refresh: false,
            last_poll_at: None,
            job_log_live: false,
            job_log_last_poll_at: None,
            job_log_user_scrolled: false,
            job_log_status: String::new(),
            filter_status: None,
            filter_branch_only: false,
            dropdown_idx: 0,
        }
    }
}

impl App {
    pub fn load_platform_enter(&mut self) {
        self.platform_view.remotes = discover_remotes(&self.repo_path);
        // If `remote` isn't in the discovered list, fall back to the
        // first remote that points to a supported platform.
        if !self
            .platform_view
            .remotes
            .contains(&self.platform_view.remote)
        {
            let pick = self
                .platform_view
                .remotes
                .first()
                .cloned()
                .unwrap_or_else(|| "origin".to_string());
            self.platform_view.remote = pick;
        }
        self.load_platform_active_sub_tab();
    }

    pub fn load_platform_active_sub_tab(&mut self) {
        match self.platform_view.sub_tab {
            PlatformSubTab::Pipelines => self.load_platform_pipelines(),
            PlatformSubTab::Jobs => {
                if let Some(pid) = self.platform_view.active_pipeline_id {
                    self.load_platform_jobs_for_pipeline(pid.to_string());
                } else {
                    // No drill-down context: fall back to Pipelines.
                    self.platform_view.sub_tab = PlatformSubTab::Pipelines;
                    self.load_platform_pipelines();
                }
            }
            PlatformSubTab::Releases => self.load_platform_releases(),
            PlatformSubTab::Packages => self.load_platform_packages(),
            PlatformSubTab::Runners => self.load_platform_runners(),
        }
    }

    pub fn load_platform_runners(&mut self) {
        self.platform_view.runners.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_runners_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::runner::get_runner_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_runners_rx = Some(rx);

        std::thread::spawn(move || {
            // 0.8.1 — the list shown in the TUI is the *combined*
            // view: every runner the platform knows about (online)
            // *plus* every torii-spawned Docker container on this
            // host (local). The local ones get `runner_type =
            // "local-docker"` so the renderer can distinguish them.
            // Local entries are listed first so the "what's running
            // on my machine" answer is at the top.
            let online = client.list(&owner, &repo);
            let mut combined: Vec<crate::runner::Runner> = list_local_runner_containers();
            match online {
                Ok(mut items) => {
                    combined.append(&mut items);
                    let _ = tx.send(Ok(combined));
                }
                Err(e) => {
                    // Even when the platform call fails we surface
                    // the local containers — the user still wants to
                    // see what's running locally.
                    if combined.is_empty() {
                        let _ = tx.send(Err(e));
                    } else {
                        let _ = tx.send(Ok(combined));
                    }
                }
            }
        });
    }

    // ── 0.7.25 runner actions (pause/resume/remove/reset-token) ─────────────

    pub(crate) fn spawn_runner_action<F>(&mut self, op: F)
    where
        F: FnOnce(
                &str,
                &str,
                Box<dyn crate::runner::RunnerClient>,
            ) -> std::result::Result<String, String>
            + Send
            + 'static,
    {
        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.set_status("✗ no remote/platform resolved");
            return;
        };
        let client = match crate::runner::get_runner_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("✗ {}", e));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_action_rx = Some(rx);
        self.platform_view.action_in_flight = true;
        std::thread::spawn(move || {
            let _ = tx.send(op(&owner, &repo, client));
        });
    }

    pub fn action_runner_pause(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client
                .pause(owner, repo, &id)
                .map(|_| format!("✓ runner #{} paused", id_disp))
                .map_err(|e| format!("✗ pause runner #{}: {}", id_disp, e))
        });
    }

    pub fn action_runner_resume(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client
                .resume(owner, repo, &id)
                .map(|_| format!("✓ runner #{} resumed", id_disp))
                .map_err(|e| format!("✗ resume runner #{}: {}", id_disp, e))
        });
    }

    pub fn action_runner_remove(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client
                .remove(owner, repo, &id)
                .map(|_| format!("✓ runner #{} removed", id_disp))
                .map_err(|e| format!("✗ remove runner #{}: {}", id_disp, e))
        });
    }

    pub fn action_runner_reset_token(&mut self, id: String) {
        // Reset-token is special: it returns a credential we have to
        // surface back to the user. The action_msg lane is a one-line
        // status bar — wrong place for a long secret. Instead we
        // route the new value through the event log (toggle with `e`).
        // The action_msg just announces success and points there.
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client
                .reset_token(owner, repo, &id)
                .map(|new_token| {
                    // Embed the token in the success message itself
                    // with a recognisable prefix; the main loop's
                    // post-action hook pulls it back out and pushes
                    // it to the event log.
                    format!("✓ runner #{} token reset|token={}", id_disp, new_token)
                })
                .map_err(|e| format!("✗ reset runner #{} token: {}", id_disp, e))
        });
    }

    pub(crate) fn resolve_platform_target(&mut self) -> Option<(String, String, String)> {
        let res = crate::pr::detect_platform_from_remote_named(
            &self.repo_path,
            &self.platform_view.remote,
        );
        match res {
            Some((p, o, r)) => {
                self.platform_view.platform = p.clone();
                self.platform_view.owner = o.clone();
                self.platform_view.repo_name = r.clone();
                Some((p, o, r))
            }
            None => {
                self.platform_view.error = Some(format!(
                    "remote '{}' is not a github/gitlab URL",
                    self.platform_view.remote,
                ));
                None
            }
        }
    }

    pub fn load_platform_pipelines(&mut self) {
        self.platform_view.pipelines.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_pipelines_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_pipelines_rx = Some(rx);

        let status = self.platform_view.filter_status.clone();
        let branch_filter = if self.platform_view.filter_branch_only {
            Some(self.branch.clone())
        } else {
            None
        };

        std::thread::spawn(move || {
            let filters = crate::pipeline::ListFilters {
                status,
                per_page: 50,
            };
            let result = client.list(&owner, &repo, &filters);
            // Branch filter is client-side because not every platform
            // exposes a ref filter cleanly (Bitbucket, sourcehut).
            let result = match (result, branch_filter) {
                (Ok(mut items), Some(b)) => {
                    items.retain(|p| p.branch == b);
                    Ok(items)
                }
                (other, _) => other,
            };
            let _ = tx.send(result);
        });
    }

    pub fn load_platform_jobs_for_pipeline(&mut self, pipeline_id: String) {
        self.platform_view.jobs.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_jobs_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_jobs_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.list_jobs(&owner, &repo, &pipeline_id, None));
        });
    }

    pub fn load_platform_releases(&mut self) {
        self.platform_view.releases.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_releases_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::release::get_release_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_releases_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.list(&owner, &repo, 50));
        });
    }

    pub fn load_platform_packages(&mut self) {
        self.platform_view.packages.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_packages_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        // Packages are GitLab-only in 0.7.12 (GitHub Packages API requires
        // package_type+username scoping that doesn't map cleanly to the
        // CI surface — the CLI returns an error pointing at `release` for
        // GitHub, we mirror that here so the view doesn't appear broken).
        if platform != "gitlab" {
            self.platform_view.loading = false;
            self.platform_view.error = Some(
                "Packages are GitLab-only here. For GitHub, see Releases (assets).".to_string(),
            );
            return;
        }

        let client = match crate::package::get_package_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_packages_rx = Some(rx);

        std::thread::spawn(move || {
            let filters = crate::package::PackageListFilters::default();
            let _ = tx.send(client.list(&owner, &repo, &filters));
        });
    }

    // ── 0.7.24: contextual actions on pipelines/jobs ────────────────────────
    //
    // All four spawn a background thread that calls the platform API and
    // pipes a `Result<message, error>` back through `platform_action_rx`.
    // The main loop pumps the receiver into `platform_view.action_msg` and
    // triggers a list reload so the new status shows up. `action_in_flight`
    // gates the keybinds in events.rs so the user can't fire 5 retries by
    // mashing the key.
    pub(crate) fn spawn_platform_action<F>(&mut self, op: F)
    where
        F: FnOnce(
                &str,
                &str,
                Box<dyn crate::pipeline::PipelineClient>,
            ) -> std::result::Result<String, String>
            + Send
            + 'static,
    {
        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.set_status("✗ no remote/platform resolved");
            return;
        };
        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("✗ {}", e));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_action_rx = Some(rx);
        self.platform_view.action_in_flight = true;
        std::thread::spawn(move || {
            let _ = tx.send(op(&owner, &repo, client));
        });
    }

    pub fn action_pipeline_cancel(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client
                .cancel(owner, repo, &id)
                .map(|_| format!("✓ pipeline #{} canceled", id_disp))
                .map_err(|e| format!("✗ cancel pipeline #{}: {}", id_disp, e))
        });
    }

    pub fn action_pipeline_retry(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client
                .retry(owner, repo, &id)
                .map(|_| format!("✓ pipeline #{} retried", id_disp))
                .map_err(|e| format!("✗ retry pipeline #{}: {}", id_disp, e))
        });
    }

    pub fn action_job_cancel(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client
                .job_cancel(owner, repo, &id)
                .map(|_| format!("✓ job #{} canceled", id_disp))
                .map_err(|e| format!("✗ cancel job #{}: {}", id_disp, e))
        });
    }

    pub fn action_job_retry(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client
                .job_retry(owner, repo, &id)
                .map(|_| format!("✓ job #{} retried", id_disp))
                .map_err(|e| format!("✗ retry job #{}: {}", id_disp, e))
        });
    }

    pub fn action_job_artifacts(&mut self, id: String) {
        let id_disp = id.clone();
        // Write to <repo_path>/artifacts/job-<id>.zip — same convention as
        // the CLI's `torii job artifacts <id>` (matches user muscle memory
        // if they've used the non-TUI command before).
        let out_dir = std::path::PathBuf::from(&self.repo_path).join("artifacts");
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            self.set_status(format!("✗ mkdir {}: {}", out_dir.display(), e));
            return;
        }
        let out_path = out_dir.join(format!("job-{}.zip", id));
        let out_disp = out_path.display().to_string();
        self.spawn_platform_action(move |owner, repo, client| {
            client
                .job_artifacts_download(owner, repo, &id, &out_path)
                .map(|_| format!("✓ job #{} artifacts → {}", id_disp, out_disp))
                .map_err(|e| format!("✗ artifacts job #{}: {}", id_disp, e))
        });
    }

    pub fn load_platform_job_log(&mut self, job_id: String) {
        self.platform_view.job_log = None;
        self.platform_view.job_log_scroll = 0;
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_job_log_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_job_log_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.job_log(&owner, &repo, &job_id));
        });
    }
}
