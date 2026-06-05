//! PR/MR view state + loaders.

use super::*;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PrEntry {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub head: String,
    pub base: String,
    pub author: String,
    pub url: String,
    pub draft: bool,
    pub mergeable: Option<bool>,
    pub created_at: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrStateFilter {
    Open,
    Closed,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrConfirm {
    None,
    Merge,
    Close,
    CreateTitle,
    CreateHead,
    CreateBase,
    CreateDesc,
    CreatePlatforms,
    EditTitle,
    EditDesc,
    EditBase,
    SwitchPlatform,
}

#[derive(Debug, Clone)]
pub struct PrPlatformEntry {
    pub platform: String, // "github" / "gitlab"
    pub owner: String,
    pub repo: String,
    pub label: String, // display: "github — paskidev/gitorii"
}

pub struct PrState {
    pub prs: Vec<PrEntry>,
    pub idx: usize,
    pub filter: PrStateFilter,
    pub loading: bool,
    pub error: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: PrConfirm,
    pub merge_method: usize, // 0=merge, 1=squash, 2=rebase
    pub platform: String,
    pub owner: String,
    pub repo_name: String,
    // create flow
    pub create_title: String,
    pub create_head: String,
    pub create_base: String,
    pub create_desc: String,
    pub create_draft: bool,
    pub create_input: String,
    // edit flow
    pub edit_input: String,
    pub edit_desc: String,
    // branch dropdown (edit base)
    pub branches: Vec<String>,
    pub branch_idx: usize,
    // platform switcher
    pub available_platforms: Vec<PrPlatformEntry>,
    pub platform_idx: usize,
    // create — platform multi-select
    pub create_platform_idx: usize,
    pub create_platform_selected: Vec<bool>,
}

impl Default for PrState {
    fn default() -> Self {
        Self {
            prs: vec![],
            idx: 0,
            filter: PrStateFilter::Open,
            loading: false,
            error: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: PrConfirm::None,
            merge_method: 0,
            platform: String::new(),
            owner: String::new(),
            repo_name: String::new(),
            create_title: String::new(),
            create_head: String::new(),
            create_base: String::new(),
            create_desc: String::new(),
            create_draft: false,
            create_input: String::new(),
            edit_input: String::new(),
            edit_desc: String::new(),
            branches: vec![],
            branch_idx: 0,
            available_platforms: vec![],
            platform_idx: 0,
            create_platform_idx: 0,
            create_platform_selected: vec![],
        }
    }
}

// ── Issue state ───────────────────────────────────────────────────────────────

impl App {
    pub fn load_prs(&mut self) {
        use crate::pr::{detect_platform_from_remote, get_pr_client};

        self.pr_view.prs.clear();
        self.pr_view.error = None;
        self.pr_view.loading = true;
        self.pr_rx = None;

        let Some((platform, owner, repo_name)) = detect_platform_from_remote(&self.repo_path)
        else {
            self.pr_view.loading = false;
            self.pr_view.error = Some("no github / gitlab / codeberg remote detected".to_string());
            return;
        };
        self.pr_view.platform = platform.clone();
        self.pr_view.owner = owner.clone();
        self.pr_view.repo_name = repo_name.clone();

        let state = match self.pr_view.filter {
            PrStateFilter::Open => "open".to_string(),
            PrStateFilter::Closed => "closed".to_string(),
            PrStateFilter::All => "all".to_string(),
        };

        let client = match get_pr_client(&platform) {
            Err(e) => {
                self.pr_view.loading = false;
                self.pr_view.error = Some(e.to_string());
                return;
            }
            Ok(c) => c,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.pr_rx = Some(rx);

        std::thread::spawn(move || {
            let result = client.list(&owner, &repo_name, &state).map(|prs| {
                prs.into_iter()
                    .map(|p| PrEntry {
                        number: p.number,
                        title: p.title,
                        state: p.state,
                        head: p.head,
                        base: p.base,
                        author: p.author,
                        url: p.url,
                        draft: p.draft,
                        mergeable: p.mergeable,
                        created_at: p.created_at,
                        body: p.body,
                    })
                    .collect()
            });
            let _ = tx.send(result);
        });
    }

    pub fn load_pr_platforms(&mut self) {
        use crate::pr::detect_platform_from_remote;
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else {
            return;
        };
        let Ok(remotes) = repo.remotes() else { return };
        let mut seen = std::collections::HashSet::new();
        self.pr_view.available_platforms = remotes
            .iter()
            .filter_map(|name| {
                let name = name?;
                let remote = repo.find_remote(name).ok()?;
                let url = remote.url()?.to_string();
                let platform = if url.contains("github.com") {
                    "github"
                } else if url.contains("gitlab.com") {
                    "gitlab"
                } else {
                    return None;
                };
                // parse owner/repo from url
                let path = if url.contains('@') {
                    url.splitn(2, ':').nth(1)?
                } else {
                    url.trim_start_matches("https://")
                        .trim_start_matches("http://")
                        .splitn(2, '/')
                        .nth(1)?
                };
                let path = path.trim_end_matches(".git");
                let mut parts = path.splitn(2, '/');
                let owner = parts.next()?.to_string();
                let repo_name = parts.next()?.to_string();
                let key = format!("{}/{}/{}", platform, owner, repo_name);
                if !seen.insert(key) {
                    return None;
                }
                Some(PrPlatformEntry {
                    label: format!("{} — {}/{}", platform, owner, repo_name),
                    platform: platform.to_string(),
                    owner,
                    repo: repo_name,
                })
            })
            .collect();
        // set platform_idx to current active platform
        let current = &self.pr_view.platform;
        let current_owner = &self.pr_view.owner;
        self.pr_view.platform_idx = self
            .pr_view
            .available_platforms
            .iter()
            .position(|p| &p.platform == current && &p.owner == current_owner)
            .unwrap_or(0);
        // also try detect_platform_from_remote as fallback if list empty
        if self.pr_view.available_platforms.is_empty() {
            if let Some((platform, owner, repo_name)) = detect_platform_from_remote(&self.repo_path)
            {
                self.pr_view.available_platforms.push(PrPlatformEntry {
                    label: format!("{} — {}/{}", platform, owner, repo_name),
                    platform,
                    owner,
                    repo: repo_name,
                });
            }
        }
    }

    pub fn load_pr_branches(&mut self) {
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else {
            return;
        };
        let Ok(branches) = repo.branches(None) else {
            return;
        };
        self.pr_view.branches = branches
            .filter_map(|b| b.ok())
            .filter_map(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
            .collect();
        self.pr_view.branches.sort();
    }

    pub fn pr_move_up(&mut self) {
        if self.pr_view.idx > 0 {
            self.pr_view.idx -= 1;
        }
    }

    pub fn pr_move_down(&mut self) {
        if self.pr_view.idx + 1 < self.pr_view.prs.len() {
            self.pr_view.idx += 1;
        }
    }
}
