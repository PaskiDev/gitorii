//! Issue view state + loaders.

use super::*;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IssueEntry {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub url: String,
    pub labels: Vec<String>,
    pub comments: u64,
    pub created_at: String,
    pub body: Option<String>,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueConfirm {
    None,
    Close,
    CreateTitle,
    CreateDesc,
    Comment,
    SwitchPlatform,
}

#[derive(Debug, Clone)]
pub struct IssuePlatformEntry {
    pub platform: String,
    pub owner: String,
    pub repo: String,
    pub label: String,
}

pub struct IssueState {
    pub issues: Vec<IssueEntry>,
    pub idx: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: IssueConfirm,
    pub platform: String,
    pub owner: String,
    pub repo_name: String,
    pub available_platforms: Vec<IssuePlatformEntry>,
    pub platform_idx: usize,
    pub create_title: String,
    pub create_desc: String,
    pub create_input: String,
    pub comment_input: String,
}

impl Default for IssueState {
    fn default() -> Self {
        Self {
            issues: vec![],
            idx: 0,
            loading: false,
            error: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: IssueConfirm::None,
            platform: String::new(),
            owner: String::new(),
            repo_name: String::new(),
            available_platforms: vec![],
            platform_idx: 0,
            create_title: String::new(),
            create_desc: String::new(),
            create_input: String::new(),
            comment_input: String::new(),
        }
    }
}

// ── Workspace state ───────────────────────────────────────────────────────────

impl App {
    pub fn load_issues(&mut self) {
        use crate::issue::get_issue_client;
        use crate::pr::detect_platform_from_remote;

        self.issue_view.issues.clear();
        self.issue_view.error = None;
        self.issue_view.loading = true;
        self.issue_rx = None;

        let (platform, owner, repo_name) = if !self.issue_view.platform.is_empty()
            && !self.issue_view.owner.is_empty()
            && !self.issue_view.repo_name.is_empty()
        {
            (
                self.issue_view.platform.clone(),
                self.issue_view.owner.clone(),
                self.issue_view.repo_name.clone(),
            )
        } else {
            match detect_platform_from_remote(&self.repo_path) {
                Some(t) => t,
                None => {
                    self.issue_view.loading = false;
                    self.issue_view.error =
                        Some("no github / gitlab / codeberg remote detected".to_string());
                    return;
                }
            }
        };
        self.issue_view.platform = platform.clone();
        self.issue_view.owner = owner.clone();
        self.issue_view.repo_name = repo_name.clone();

        let client = match get_issue_client(&platform) {
            Err(e) => {
                self.issue_view.loading = false;
                self.issue_view.error = Some(e.to_string());
                return;
            }
            Ok(c) => c,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.issue_rx = Some(rx);
        let platform_for_rows = platform.clone();

        std::thread::spawn(move || {
            let result = client.list(&owner, &repo_name, "open").map(|issues| {
                issues
                    .into_iter()
                    .map(|i| IssueEntry {
                        number: i.number,
                        title: i.title,
                        state: i.state,
                        author: i.author,
                        url: i.url,
                        labels: i.labels,
                        comments: i.comments,
                        created_at: i.created_at,
                        body: i.body,
                        platform: platform_for_rows.clone(),
                    })
                    .collect()
            });
            let _ = tx.send(result);
        });
    }

    pub fn load_issue_platforms(&mut self) {
        use crate::pr::detect_platform_from_remote_named;
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else {
            return;
        };
        let Ok(remotes) = repo.remotes() else {
            return;
        };
        let mut seen = std::collections::HashSet::new();
        self.issue_view.available_platforms = remotes
            .iter()
            .flatten()
            .filter_map(|name| {
                let (platform, owner, repo_name) =
                    detect_platform_from_remote_named(&self.repo_path, name)?;
                let key = format!("{}/{}/{}", platform, owner, repo_name);
                if !seen.insert(key) {
                    return None;
                }
                Some(IssuePlatformEntry {
                    label: format!("{} — {}/{}", platform, owner, repo_name),
                    platform,
                    owner,
                    repo: repo_name,
                })
            })
            .collect();
        self.issue_view.platform_idx = self
            .issue_view
            .available_platforms
            .iter()
            .position(|p| {
                p.platform == self.issue_view.platform
                    && p.owner == self.issue_view.owner
                    && p.repo == self.issue_view.repo_name
            })
            .unwrap_or(0);
        if self.issue_view.available_platforms.is_empty() && !self.issue_view.platform.is_empty() {
            self.issue_view
                .available_platforms
                .push(IssuePlatformEntry {
                    label: format!(
                        "{} — {}/{}",
                        self.issue_view.platform, self.issue_view.owner, self.issue_view.repo_name
                    ),
                    platform: self.issue_view.platform.clone(),
                    owner: self.issue_view.owner.clone(),
                    repo: self.issue_view.repo_name.clone(),
                });
        }
    }
}
