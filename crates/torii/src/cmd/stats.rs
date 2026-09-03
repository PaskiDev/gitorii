//! What a repository looks like from a distance: how much work lands in it,
//! who does it, what it is made of, and where the churn is.
//!
//! Split in three on purpose, because they cost three different amounts:
//!
//! - [`shape`] reads refs and the working tree. Cheap, always available.
//! - [`history`] walks the commits, capped at [`HISTORY_CAP`]. A large repo
//!   pays a noticeable but bounded price.
//! - [`churn`] diffs each commit against its parent, capped at [`CHURN_CAP`].
//!   That is the expensive one, so it runs off the UI thread.

use git2::Repository;
use std::collections::HashMap;
use std::path::Path;

/// Commits walked for activity and authorship. Beyond this the numbers stop
/// being about "lately" anyway, and the wait stops being worth it.
pub const HISTORY_CAP: usize = 5_000;
/// Commits diffed for churn. Each one costs a tree comparison.
pub const CHURN_CAP: usize = 400;
/// Weeks in the activity sparkline.
pub const WEEKS: usize = 12;

/// The cheap half: refs, working tree, and what the files are.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub branch: String,
    pub local_branches: usize,
    pub remote_branches: usize,
    pub tags: usize,
    pub remotes: Vec<String>,
    pub files: usize,
    pub bytes: u64,
    pub dirty: usize,
    /// Extension → number of tracked files, largest first.
    pub languages: Vec<(String, usize)>,
}

/// The walked half: when the work happened and who did it.
#[derive(Debug, Clone, Default)]
pub struct History {
    pub commits: usize,
    /// Whether the walk stopped at [`HISTORY_CAP`] rather than at the root.
    pub capped: bool,
    /// Commits per week, oldest first, ending with the current week.
    pub weeks: [usize; WEEKS],
    /// Author → commits, largest first.
    pub authors: Vec<(String, usize)>,
    /// Unix seconds of the oldest and newest commit walked.
    pub first: Option<i64>,
    pub last: Option<i64>,
}

/// The expensive half: which files keep being touched.
#[derive(Debug, Clone, Default)]
pub struct Churn {
    /// Path → times it appears in a commit's diff, largest first.
    pub hot: Vec<(String, usize)>,
    /// How many commits were actually diffed.
    pub commits: usize,
}

/// Refs, working tree and file mix. No history walk.
pub fn shape(repo_path: &Path) -> Shape {
    let mut out = Shape::default();
    let Ok(repo) = Repository::discover(repo_path) else {
        return out;
    };

    out.branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(str::to_string))
        .unwrap_or_else(|| "detached".into());

    if let Ok(branches) = repo.branches(None) {
        for entry in branches.flatten() {
            match entry.1 {
                git2::BranchType::Local => out.local_branches += 1,
                git2::BranchType::Remote => out.remote_branches += 1,
            }
        }
    }
    out.tags = repo.tag_names(None).map(|t| t.len()).unwrap_or(0);
    out.remotes = repo
        .remotes()
        .map(|r| r.iter().flatten().map(str::to_string).collect())
        .unwrap_or_default();

    // Tracked files come from the index: walking the working tree would count
    // build output and everything else git was told to ignore.
    if let Ok(index) = repo.index() {
        let mut by_ext: HashMap<String, usize> = HashMap::new();
        let workdir = repo.workdir().map(|w| w.to_path_buf());
        for entry in index.iter() {
            out.files += 1;
            let Ok(path) = String::from_utf8(entry.path.clone()) else {
                continue;
            };
            let ext = Path::new(&path)
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_else(|| "—".into());
            *by_ext.entry(ext).or_default() += 1;
            // `entry.file_size` is what git recorded; for a file that is gone
            // or unstaged it can be stale, so the disk wins when it answers.
            let size = workdir
                .as_ref()
                .and_then(|w| std::fs::metadata(w.join(&path)).ok())
                .map(|m| m.len())
                .unwrap_or(entry.file_size as u64);
            out.bytes += size;
        }
        let mut langs: Vec<(String, usize)> = by_ext.into_iter().collect();
        langs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out.languages = langs;
    }

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).include_ignored(false);
    out.dirty = repo.statuses(Some(&mut opts)).map(|s| s.len()).unwrap_or(0);

    out
}

/// Walk the history for activity and authorship. `now` is passed in so the
/// week buckets can be tested without waiting a week.
pub fn history(repo_path: &Path, now: i64) -> History {
    let mut out = History::default();
    let Ok(repo) = Repository::discover(repo_path) else {
        return out;
    };
    let Ok(mut walk) = repo.revwalk() else {
        return out;
    };
    if walk.push_head().is_err() {
        return out; // an unborn HEAD has no history, and that is not an error
    }

    const WEEK: i64 = 7 * 24 * 60 * 60;
    let mut by_author: HashMap<String, usize> = HashMap::new();

    for oid in walk.take(HISTORY_CAP + 1).flatten() {
        if out.commits == HISTORY_CAP {
            out.capped = true;
            break;
        }
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        out.commits += 1;

        let when = commit.time().seconds();
        out.first = Some(out.first.map_or(when, |f: i64| f.min(when)));
        out.last = Some(out.last.map_or(when, |l: i64| l.max(when)));

        // Bucket by whole weeks back from now; the last bucket is this week.
        let age_weeks = ((now - when).max(0) / WEEK) as usize;
        if age_weeks < WEEKS {
            out.weeks[WEEKS - 1 - age_weeks] += 1;
        }

        let name = commit
            .author()
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| "(unknown)".into());
        *by_author.entry(name).or_default() += 1;
    }

    let mut authors: Vec<(String, usize)> = by_author.into_iter().collect();
    authors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.authors = authors;
    out
}

/// Diff the recent commits to find what keeps being touched. Expensive: this
/// is the part that belongs on a worker thread.
pub fn churn(repo_path: &Path) -> Churn {
    let mut out = Churn::default();
    let Ok(repo) = Repository::discover(repo_path) else {
        return out;
    };
    let Ok(mut walk) = repo.revwalk() else {
        return out;
    };
    if walk.push_head().is_err() {
        return out;
    }

    let mut touches: HashMap<String, usize> = HashMap::new();
    for oid in walk.take(CHURN_CAP).flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        // A merge's diff against its first parent double-counts everything the
        // branch already did, so merges are skipped.
        if commit.parent_count() != 1 {
            continue;
        }
        let (Ok(tree), Ok(parent)) = (commit.tree(), commit.parent(0)) else {
            continue;
        };
        let Ok(parent_tree) = parent.tree() else {
            continue;
        };
        let Ok(diff) = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None) else {
            continue;
        };
        out.commits += 1;
        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                *touches
                    .entry(path.to_string_lossy().into_owned())
                    .or_default() += 1;
            }
        }
    }

    let mut hot: Vec<(String, usize)> = touches.into_iter().collect();
    hot.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    hot.truncate(12);
    out.hot = hot;
    out
}

/// Draw counts as a sparkline. Empty input is an empty line, and a flat run of
/// zeros stays at the lowest block rather than dividing by zero.
pub fn sparkline(values: &[usize]) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|v| match (v * (BLOCKS.len() - 1)).checked_div(max) {
            Some(idx) => BLOCKS[idx],
            // A flat run of zeros has no maximum to scale against; it sits on
            // the floor rather than dividing by it.
            None => BLOCKS[0],
        })
        .collect()
}

/// A byte count a human can read at a glance.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEEK: i64 = 7 * 24 * 60 * 60;

    /// A repo with commits at known times and two authors.
    fn repo_with_history(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let mut parents_oid: Option<git2::Oid> = None;
        // now, one week ago, five weeks ago — the buckets these land in are
        // what the sparkline is made of.
        let now = 1_700_000_000;
        for (i, (name, ago)) in [
            ("Alice", 0),
            ("Alice", WEEK),
            ("Bob", 5 * WEEK),
            ("Alice", 40 * WEEK), // older than the window
        ]
        .iter()
        .enumerate()
        {
            let when = git2::Time::new(now - ago, 0);
            let sig = git2::Signature::new(name, "a@b", &when).unwrap();
            std::fs::write(dir.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new(&format!("f{i}.txt"))).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let parents: Vec<git2::Commit> = parents_oid
                .iter()
                .map(|o| repo.find_commit(*o).unwrap())
                .collect();
            let refs: Vec<&git2::Commit> = parents.iter().collect();
            let oid = repo
                .commit(Some("HEAD"), &sig, &sig, "c", &tree, &refs)
                .unwrap();
            parents_oid = Some(oid);
        }
        repo
    }

    #[test]
    fn the_shape_counts_what_git_tracks() {
        let tmp = tempfile::tempdir().unwrap();
        repo_with_history(tmp.path());
        std::fs::write(tmp.path().join("ignored.tmp"), "junk").unwrap();

        let shape = shape(tmp.path());
        assert_eq!(shape.files, 4, "four tracked files, not the untracked one");
        assert!(shape.bytes > 0);
        assert_eq!(shape.local_branches, 1);
        assert!(shape.dirty >= 1, "the untracked file shows as dirty");
        assert_eq!(
            shape.languages.first().map(|(e, n)| (e.as_str(), *n)),
            Some(("txt", 4))
        );
    }

    #[test]
    fn the_history_buckets_commits_by_week_and_ranks_authors() {
        let tmp = tempfile::tempdir().unwrap();
        repo_with_history(tmp.path());

        let h = history(tmp.path(), 1_700_000_000);
        assert_eq!(h.commits, 4);
        assert!(!h.capped);

        // This week and last week hold one each; five weeks back holds one;
        // the fortieth is outside the window and is counted nowhere.
        assert_eq!(h.weeks[WEEKS - 1], 1, "this week: {:?}", h.weeks);
        assert_eq!(h.weeks[WEEKS - 2], 1, "last week: {:?}", h.weeks);
        assert_eq!(h.weeks[WEEKS - 6], 1, "five weeks back: {:?}", h.weeks);
        assert_eq!(h.weeks.iter().sum::<usize>(), 3, "{:?}", h.weeks);

        assert_eq!(h.authors[0], ("Alice".to_string(), 3));
        assert_eq!(h.authors[1], ("Bob".to_string(), 1));
        assert!(h.first.unwrap() < h.last.unwrap());
    }

    #[test]
    fn churn_counts_the_files_a_commit_touched() {
        let tmp = tempfile::tempdir().unwrap();
        repo_with_history(tmp.path());

        let c = churn(tmp.path());
        assert_eq!(
            c.commits, 3,
            "the root commit has no parent to diff against"
        );
        // Each commit adds one file, so every file is touched once.
        assert!(c.hot.iter().all(|(_, n)| *n == 1), "{:?}", c.hot);
        assert_eq!(c.hot.len(), 3);
    }

    #[test]
    fn a_repo_without_commits_reports_zeroes_rather_than_failing() {
        let tmp = tempfile::tempdir().unwrap();
        Repository::init(tmp.path()).unwrap();

        let h = history(tmp.path(), 1_700_000_000);
        assert_eq!(h.commits, 0);
        assert_eq!(churn(tmp.path()).commits, 0);
        assert_eq!(shape(tmp.path()).files, 0);
    }

    #[test]
    fn the_sparkline_scales_to_its_own_maximum() {
        assert_eq!(sparkline(&[0, 1, 2, 3, 4, 5, 6, 7]), "▁▂▃▄▅▆▇█");
        assert_eq!(
            sparkline(&[0, 0, 0]),
            "▁▁▁",
            "a flat run must not divide by zero"
        );
        assert_eq!(sparkline(&[]), "");
        assert_eq!(sparkline(&[5, 5]), "██");
    }

    #[test]
    fn bytes_read_the_way_a_person_says_them() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(12 * 1024 * 1024), "12.0 MB");
    }
}
