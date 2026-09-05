//! Log view state + ops.

use super::*;

pub struct CommitFileEntry {
    pub path: String,
    pub status: char, // 'A' added, 'M' modified, 'D' deleted, 'R' renamed
}

/// The prefix that turns the log search into an identity filter. The
/// parser, the `A` key and the heading all have to agree on it, so it
/// is one constant rather than three string literals.
pub const AUTHOR_PREFIX: &str = "author:";

/// The identity term of a search query, if it carries one. `None` means
/// a plain search, which keeps matching message / author / email / hash.
///
/// The prefix is matched case-insensitively and `get` does the bounds
/// check, so a query that opens with a multi-byte character is a miss
/// rather than a panic on a char boundary.
pub fn author_query(query: &str) -> Option<&str> {
    let q = query.trim_start();
    let head = q.get(..AUTHOR_PREFIX.len())?;
    if !head.eq_ignore_ascii_case(AUTHOR_PREFIX) {
        return None;
    }
    Some(q[AUTHOR_PREFIX.len()..].trim())
}

/// One row of the author dropdown.
///
/// `identity: None` is the "all authors" row that lifts the filter. It
/// is a row rather than a separate key because the list is the whole
/// interface: everything the menu can do is on it, including undoing
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorRow {
    pub identity: Option<String>,
    pub count: usize,
}

pub struct LogState {
    pub idx: usize,
    pub scroll: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
    pub page_size: usize,
    pub all_loaded: bool,
    pub commit_files: Vec<CommitFileEntry>,
    pub last_files_idx: Option<usize>,
    pub ops_mode: bool,
    pub ops_idx: usize,

    /// The author dropdown (`A`). `authors_idx` indexes the rows
    /// `log_author_rows` builds, which is also what the renderer walks,
    /// so the highlight and the identity Enter applies cannot drift.
    pub authors_mode: bool,
    pub authors_idx: usize,

    /// Commits picked with Space, as full OIDs, in the order they were
    /// picked. Empty means no selection and the ops menu keeps its
    /// whole-history list.
    pub selected: Vec<String>,
    /// The `Name <email>` every selected commit shares. Set by the first
    /// pick and cleared with the selection; while it is `Some`, a commit
    /// authored by anyone else cannot join.
    ///
    /// This is the lock the selection exists for. `reauthor` matches on
    /// an identity, so a set spanning two of them has no single `--old`
    /// to run with, and the operation people actually want — "these
    /// commits, this identity" — is only well defined on a set that
    /// agrees.
    pub selection_identity: Option<String>,

    /// Per-commit graph rows aligned with `App.commits`. Always populated in
    /// the Log view to differentiate it visually from the History view.
    pub graph_rows: Vec<crate::graph::GraphRow>,

    /// 0.7.36 — toggle for the GPG signature column. Off by default
    /// because the verify loop spawns one `gpg --verify` per commit
    /// and that's not free. `G` flips it.
    pub show_signatures: bool,
    /// Cached `signature_letter` results keyed by full commit OID.
    /// Populated lazily when the column toggles on; cleared on log
    /// refresh so a newly-signed commit doesn't stay stale.
    pub signature_cache: std::collections::HashMap<String, char>,
    /// 0.7.36 — armor overlay (`S` over the selected commit). `None`
    /// = closed; `Some(Loading)` = the worker is computing; `Some(Done)`
    /// / `Some(Error)` = the modal stays open until any key closes it.
    pub signature_overlay: Option<SignatureOverlay>,
}

#[derive(Debug, Clone)]
pub enum SignatureOverlay {
    Loading {
        oid: String,
    },
    Done {
        oid: String,
        armor: String,
        verdict: String,
        verdict_color: SignatureVerdictColor,
    },
    Error {
        oid: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignatureVerdictColor {
    Good,
    Unknown,
    Bad,
    Other,
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            idx: 0,
            scroll: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
            page_size: 50,
            all_loaded: false,
            commit_files: vec![],
            last_files_idx: None,
            ops_mode: false,
            ops_idx: 0,
            authors_mode: false,
            authors_idx: 0,
            selected: vec![],
            selection_identity: None,
            graph_rows: vec![],
            show_signatures: false,
            signature_cache: std::collections::HashMap::new(),
            signature_overlay: None,
        }
    }
}

// ── Branch state ─────────────────────────────────────────────────────────────

impl App {
    /// 0.7.36 — populate the per-commit signature cache for every
    /// commit currently in `self.commits`. Blocks (sequential
    /// `gpg --verify` calls) but the typical log slice is ≤ 50
    /// commits and each verify is fast on a local keyring. Called
    /// when the user toggles the column on so the column isn't
    /// stuck showing `…` forever.
    pub fn refresh_signature_cache(&mut self) {
        let repo = match git2::Repository::open(&self.repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        for c in &self.commits {
            if self.log.signature_cache.contains_key(&c.full_hash) {
                continue;
            }
            let Ok(oid) = git2::Oid::from_str(&c.full_hash) else {
                continue;
            };
            let letter = crate::vcs::core_extensions::signature_letter(&repo, oid);
            let ch = letter.chars().next().unwrap_or('?');
            self.log.signature_cache.insert(c.full_hash.clone(), ch);
        }
    }

    /// 0.7.36 — start an armor overlay worker for the given commit
    /// OID. Mirrors the OAuth pattern: status starts at `Loading`,
    /// the worker emits `Done` (armor + verdict) or `Error` once it
    /// finishes. Reused by the Log view's `S` keybind.
    pub fn start_signature_overlay(&mut self, oid_str: String) {
        use crate::tui::app::{SignatureOverlay, SignatureVerdictColor};
        self.log.signature_overlay = Some(SignatureOverlay::Loading {
            oid: oid_str.clone(),
        });
        let (tx, rx) = std::sync::mpsc::channel();
        self.log_signature_rx = Some(rx);

        let repo_path = self.repo_path.clone();
        std::thread::spawn(move || {
            let send_err = |msg: String| {
                let _ = tx.send(SignatureOverlay::Error {
                    oid: oid_str.clone(),
                    message: msg,
                });
            };

            let repo = match git2::Repository::open(&repo_path) {
                Ok(r) => r,
                Err(e) => {
                    send_err(format!("open repo: {}", e));
                    return;
                }
            };
            let oid = match git2::Oid::from_str(&oid_str) {
                Ok(o) => o,
                Err(e) => {
                    send_err(format!("bad oid: {}", e));
                    return;
                }
            };
            let (sig_buf, payload_buf) = match repo.extract_signature(&oid, None) {
                Ok(pair) => pair,
                Err(_) => {
                    send_err("commit has no GPG signature".to_string());
                    return;
                }
            };
            let sig_bytes: &[u8] = &sig_buf;
            let payload: Vec<u8> = payload_buf.to_vec();
            let armor = match std::str::from_utf8(sig_bytes) {
                Ok(s) => s.to_string(),
                Err(e) => {
                    send_err(format!("armor utf-8: {}", e));
                    return;
                }
            };

            let program = repo
                .workdir()
                .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
                .and_then(|c| c.git.gpg_program);

            let (verdict, color) = match crate::gpg::verify(&armor, &payload, program.as_deref()) {
                Ok(crate::gpg::VerifyStatus::Good { signer }) => (
                    format!("✓ Good signature from {}", signer),
                    SignatureVerdictColor::Good,
                ),
                Ok(crate::gpg::VerifyStatus::UnknownKey { key_id }) => (
                    format!(
                        "? Unknown signer key {} — import to verify",
                        key_id.as_deref().unwrap_or("?")
                    ),
                    SignatureVerdictColor::Unknown,
                ),
                Ok(crate::gpg::VerifyStatus::Bad) => (
                    "✗ BAD signature — payload does not match".to_string(),
                    SignatureVerdictColor::Bad,
                ),
                Ok(crate::gpg::VerifyStatus::Other(msg)) => (msg, SignatureVerdictColor::Other),
                Err(e) => (format!("verify error: {}", e), SignatureVerdictColor::Other),
            };

            let _ = tx.send(SignatureOverlay::Done {
                oid: oid_str,
                armor,
                verdict,
                verdict_color: color,
            });
        });
    }

    pub fn go_to_diff_from_log(&mut self) {
        self.prev_view = Some(self.view.clone());
        self.load_commit_diff_from_log();
        self.view = View::Diff;
        self.status_msg = None;
    }

    /// Recompute graph rows from `self.commits`. Cheap (≤ a few hundred
    /// commits in TUI). No-op if commits empty.
    pub fn recompute_graph_rows(&mut self) {
        use crate::graph::{render_with, GraphCommit};
        if self.commits.is_empty() {
            self.log.graph_rows.clear();
            return;
        }
        let repo = match git2::Repository::open(&self.repo_path) {
            Ok(r) => r,
            Err(_) => {
                self.log.graph_rows.clear();
                return;
            }
        };
        let input: Vec<GraphCommit> = self
            .commits
            .iter()
            .map(|c| {
                let parents = git2::Oid::from_str(&c.full_hash)
                    .ok()
                    .and_then(|oid| repo.find_commit(oid).ok())
                    .map(|commit| commit.parent_ids().map(|p| p.to_string()).collect())
                    .unwrap_or_default();
                GraphCommit {
                    id: c.full_hash.clone(),
                    parents,
                }
            })
            .collect();
        // BubblesX is a CLI-only expanded layout (one extra padding line per
        // commit) that breaks the TUI's 1:1 commit→ListItem indexing. Degrade
        // to its compact sibling for TUI rendering.
        let style = match self.settings.graph_style {
            crate::graph::GraphStyle::BubblesX => crate::graph::GraphStyle::Bubbles,
            other => other,
        };
        self.log.graph_rows = render_with(&input, style);
    }

    // ── Dashboard helpers ────────────────────────────────────────────────────

    /// `Name <email>` of a commit in the list, the form `--old` wants.
    pub fn commit_identity(&self, idx: usize) -> Option<String> {
        self.commits
            .get(idx)
            .map(|c| format!("{} <{}>", c.author, c.author_email))
    }

    /// Space over a commit. Returns the reason it was refused, if it was.
    ///
    /// The first pick sets the identity the rest of the selection is held
    /// to; unpicking the last one releases it, so the next Space starts a
    /// fresh set rather than staying bound to whoever came first.
    pub fn log_toggle_selection(&mut self, idx: usize) -> Option<String> {
        let identity = self.commit_identity(idx)?;
        let (oid, short) = match self.commits.get(idx) {
            Some(c) => (c.full_hash.clone(), c.hash.clone()),
            None => return None,
        };

        if let Some(pos) = self.log.selected.iter().position(|s| *s == oid) {
            self.log.selected.remove(pos);
            if self.log.selected.is_empty() {
                self.log.selection_identity = None;
            }
            return None;
        }

        match self.log.selection_identity.clone() {
            Some(locked) if locked != identity => Some(format!(
                "{} is authored by {}, not {} — not added",
                short, identity, locked
            )),
            _ => {
                self.log.selection_identity = Some(identity);
                self.log.selected.push(oid);
                None
            }
        }
    }

    pub fn log_clear_selection(&mut self) {
        self.log.selected.clear();
        self.log.selection_identity = None;
    }

    /// Whether a commit can still join the selection — false only for the
    /// ones the lock shuts out, which is what greys them in the list.
    pub fn log_selectable(&self, idx: usize) -> bool {
        match &self.log.selection_identity {
            None => true,
            Some(locked) => self.commit_identity(idx).as_ref() == Some(locked),
        }
    }

    pub fn log_move_up(&mut self) {
        if self.log.filtered.is_empty() {
            if self.log.idx > 0 {
                self.log.idx -= 1;
            }
        } else {
            let pos = self
                .log
                .filtered
                .iter()
                .position(|&i| i == self.log.idx)
                .unwrap_or(0);
            if pos > 0 {
                self.log.idx = self.log.filtered[pos - 1];
            }
        }
        self.sync_log_scroll();
        self.log_load_commit_files();
    }

    pub fn log_move_down(&mut self) {
        if self.log.filtered.is_empty() {
            if self.log.idx + 1 < self.commits.len() {
                self.log.idx += 1;
            } else {
                self.log_load_more();
            }
        } else {
            let pos = self
                .log
                .filtered
                .iter()
                .position(|&i| i == self.log.idx)
                .unwrap_or(0);
            if pos + 1 < self.log.filtered.len() {
                self.log.idx = self.log.filtered[pos + 1];
            } else {
                // The bottom of a filtered list is not the end of the
                // history — the filter only ever saw the page that had
                // been fetched. Page the way an unfiltered list does;
                // `refresh` re-runs the filter over what it loaded, so
                // the matches in the new page appear below the caret.
                self.log_load_more();
            }
        }
        self.sync_log_scroll();
        self.log_load_commit_files();
    }

    pub fn log_load_commit_files(&mut self) {
        if self.log.last_files_idx == Some(self.log.idx) {
            return;
        }
        self.log.last_files_idx = Some(self.log.idx);
        self.log.commit_files.clear();
        let Some(commit) = self.commits.get(self.log.idx) else {
            return;
        };
        let hash = commit.full_hash.clone();
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else {
            return;
        };
        let Ok(oid) = git2::Oid::from_str(&hash) else {
            return;
        };
        let Ok(commit) = repo.find_commit(oid) else {
            return;
        };
        let Ok(tree) = commit.tree() else { return };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) else {
            return;
        };
        let _ = diff.foreach(
            &mut |delta, _| {
                let status = match delta.status() {
                    git2::Delta::Added => 'A',
                    git2::Delta::Deleted => 'D',
                    git2::Delta::Modified => 'M',
                    git2::Delta::Renamed => 'R',
                    _ => 'M',
                };
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .to_string();
                self.log.commit_files.push(CommitFileEntry { path, status });
                true
            },
            None,
            None,
            None,
        );
    }

    pub fn log_load_more(&mut self) {
        if !self.log.all_loaded {
            self.log.page_size += 50;
            let _ = self.refresh();
        }
    }

    pub fn log_update_filter(&mut self) {
        // `author:<who>` narrows to an identity; anything else is the
        // plain substring search. The author term is matched against the
        // whole `Name <email>`, which is what lets a bare name, a bare
        // address and a pasted identity all land on the same commits.
        let author_term = author_query(&self.log.search_query).map(str::to_lowercase);
        if let Some(term) = author_term {
            self.log.filtered = self
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    // A bare `author:` is a query still being typed, not
                    // one that matches nothing — keep the list whole
                    // under the cursor.
                    term.is_empty()
                        || format!("{} <{}>", c.author, c.author_email)
                            .to_lowercase()
                            .contains(&term)
                })
                .map(|(i, _)| i)
                .collect();
            self.snap_to_first_match();
            return;
        }
        let q = self.log.search_query.to_lowercase();
        if q.is_empty() {
            self.log.filtered.clear();
            return;
        }
        self.log.filtered = self
            .commits
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.message.to_lowercase().contains(&q)
                    || c.author.to_lowercase().contains(&q)
                    || c.author_email.to_lowercase().contains(&q)
                    || c.hash.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.snap_to_first_match();
    }

    /// The rows of the author dropdown: every identity among the loaded
    /// commits, most commits first, under an "all authors" row.
    ///
    /// Ordering is `git shortlog -sn`'s, and ties break alphabetically so
    /// the list does not reshuffle under the cursor between two authors
    /// with the same count. The counts are of *loaded* commits, which is
    /// all the filter can see either — the heading says as much while
    /// history remains.
    pub fn log_author_rows(&self) -> Vec<AuthorRow> {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for i in 0..self.commits.len() {
            if let Some(id) = self.commit_identity(i) {
                *counts.entry(id).or_insert(0) += 1;
            }
        }
        let mut authors: Vec<AuthorRow> = counts
            .into_iter()
            .map(|(identity, count)| AuthorRow {
                identity: Some(identity),
                count,
            })
            .collect();
        authors.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.identity.cmp(&b.identity))
        });

        let mut rows = vec![AuthorRow {
            identity: None,
            count: self.commits.len(),
        }];
        rows.extend(authors);
        rows
    }

    /// `A`: open the dropdown over the log, with the cursor already on
    /// the author the view is about — whoever is filtered in, or failing
    /// that whoever wrote the commit under the caret. That makes the
    /// common ask (`A`, Enter) two keys and no reading.
    ///
    /// Returns false when there is nothing to choose from, so the key
    /// stays quiet on an empty log instead of opening an empty box.
    pub fn log_open_author_menu(&mut self) -> bool {
        let rows = self.log_author_rows();
        if rows.len() < 2 {
            return false;
        }
        let wanted = author_query(&self.log.search_query)
            .map(str::to_string)
            .or_else(|| self.commit_identity(self.log.idx));
        self.log.authors_idx = wanted
            .and_then(|id| {
                rows.iter()
                    .position(|r| r.identity.as_deref() == Some(id.as_str()))
            })
            .unwrap_or(0);
        self.log.authors_mode = true;
        true
    }

    /// Apply the row the cursor is on. `None` is the "all authors" row,
    /// which clears the query the same way an emptied search box does.
    pub fn log_apply_author_row(&mut self, row: &AuthorRow) -> String {
        match &row.identity {
            None => {
                self.log.search_query.clear();
                self.log.filtered.clear();
                "author filter cleared".to_string()
            }
            Some(identity) => {
                self.log.search_query = format!("{}{}", AUTHOR_PREFIX, identity);
                self.log_update_filter();
                format!(
                    "{} — {} of {} loaded commits",
                    identity,
                    self.log.filtered.len(),
                    self.commits.len()
                )
            }
        }
    }

    /// Keep the caret on a row the filter still shows.
    fn snap_to_first_match(&mut self) {
        if !self.log.filtered.contains(&self.log.idx) {
            if let Some(&first) = self.log.filtered.first() {
                self.log.idx = first;
                self.sync_log_scroll();
            }
        }
    }

    pub(crate) fn sync_log_scroll(&mut self) {
        let page = 20usize;
        if self.log.idx < self.log.scroll {
            self.log.scroll = self.log.idx;
        } else if self.log.idx >= self.log.scroll + page {
            self.log.scroll = self.log.idx + 1 - page;
        }
    }

    // ── Branch helpers ───────────────────────────────────────────────────────
}
