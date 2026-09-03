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

/// How a commit was signed, read from the object header — no gpg call, so
/// this is free. It says a signature is *present*, never that it is valid:
/// verifying is a subprocess per commit, and the log view already does that
/// on demand for one commit at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigKind {
    Pgp,
    Ssh,
    /// A signature in a format neither preamble matches.
    Other,
}

impl SigKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pgp => "pgp",
            Self::Ssh => "ssh",
            Self::Other => "signed",
        }
    }
}

/// Everything the repository knows about one person.
#[derive(Debug, Clone, Default)]
pub struct Person {
    /// The name on most of their commits.
    pub name: String,
    /// Every other name they have committed under.
    pub also_known_as: Vec<String>,
    /// Their address, and any others that resolve to the same identity.
    pub email: String,
    pub other_emails: Vec<String>,
    pub commits: usize,
    /// Commits carrying a signature header.
    pub signed: usize,
    /// The signature formats seen, in the order first met.
    pub sig_kinds: Vec<SigKind>,
    pub first: Option<i64>,
    pub last: Option<i64>,
    /// Commits where this person is the author but someone else committed —
    /// a patch applied by a maintainer, a rebase, a cherry-pick.
    pub committed_by_other: usize,
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

/// Who commits here, with everything the repository records about them.
///
/// Identity is the email address, lower-cased: it is what git actually keys
/// on, and one person's name is spelled several ways over a long history. The
/// other spellings are kept and shown rather than discarded.
pub fn people(repo_path: &Path) -> Vec<Person> {
    let Ok(repo) = Repository::discover(repo_path) else {
        return Vec::new();
    };
    let Ok(mut walk) = repo.revwalk() else {
        return Vec::new();
    };
    if walk.push_head().is_err() {
        return Vec::new();
    }

    // email → person, plus a count of each spelling of the name so the most
    // used one can lead.
    let mut by_email: HashMap<String, Person> = HashMap::new();
    let mut names: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for oid in walk.take(HISTORY_CAP).flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let author = commit.author();
        let email = author.email().unwrap_or("(none)").to_ascii_lowercase();
        let name = author.name().unwrap_or("(unknown)").to_string();

        let person = by_email.entry(email.clone()).or_insert_with(|| Person {
            email: email.clone(),
            ..Default::default()
        });
        person.commits += 1;
        *names
            .entry(email.clone())
            .or_default()
            .entry(name)
            .or_default() += 1;

        let when = commit.time().seconds();
        person.first = Some(person.first.map_or(when, |f: i64| f.min(when)));
        person.last = Some(person.last.map_or(when, |l: i64| l.max(when)));

        if commit
            .committer()
            .email()
            .unwrap_or("")
            .to_ascii_lowercase()
            != email
        {
            person.committed_by_other += 1;
        }

        if let Some(kind) = signature_kind(&commit) {
            person.signed += 1;
            if !person.sig_kinds.contains(&kind) {
                person.sig_kinds.push(kind);
            }
        }
    }

    let mut people: Vec<Person> = by_email
        .into_values()
        .map(|mut p| {
            if let Some(spellings) = names.get(&p.email) {
                let mut sorted: Vec<(&String, &usize)> = spellings.iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                if let Some((main, _)) = sorted.first() {
                    p.name = (*main).clone();
                }
                p.also_known_as = sorted.iter().skip(1).map(|(n, _)| (*n).clone()).collect();
            }
            p
        })
        .collect();

    // Same name, different addresses: show them together rather than as two
    // strangers, since that is what a machine change or a work address is.
    people.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.email.cmp(&b.email)));
    let mut merged: Vec<Person> = Vec::new();
    for person in people {
        match merged.iter_mut().find(|p| p.name == person.name) {
            Some(existing) => {
                existing.commits += person.commits;
                existing.signed += person.signed;
                existing.committed_by_other += person.committed_by_other;
                existing.other_emails.push(person.email.clone());
                existing.other_emails.extend(person.other_emails);
                for kind in person.sig_kinds {
                    if !existing.sig_kinds.contains(&kind) {
                        existing.sig_kinds.push(kind);
                    }
                }
                existing.first = match (existing.first, person.first) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                existing.last = match (existing.last, person.last) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
            }
            None => merged.push(person),
        }
    }
    merged.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.name.cmp(&b.name)));
    merged
}

/// Which kind of signature a commit carries, if any. Reads the object header,
/// so it costs nothing and proves nothing about validity.
pub fn signature_kind(commit: &git2::Commit<'_>) -> Option<SigKind> {
    let raw = commit.header_field_bytes("gpgsig").ok()?;
    let text = String::from_utf8_lossy(&raw);
    if text.contains("BEGIN SSH SIGNATURE") {
        Some(SigKind::Ssh)
    } else if text.contains("BEGIN PGP SIGNATURE") {
        Some(SigKind::Pgp)
    } else if text.trim().is_empty() {
        None
    } else {
        Some(SigKind::Other)
    }
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
    fn people_are_keyed_by_email_and_keep_every_spelling_of_their_name() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        let mut parent: Option<git2::Oid> = None;
        // The same address under two spellings, then someone else.
        for (i, (name, email)) in [
            ("Pasqual Peñalver", "public@paski.dev"),
            ("paski", "public@paski.dev"),
            ("Pasqual Peñalver", "public@paski.dev"),
            ("Other Dev", "other@example.com"),
        ]
        .iter()
        .enumerate()
        {
            let sig =
                git2::Signature::new(name, email, &git2::Time::new(1_700_000_000, 0)).unwrap();
            std::fs::write(tmp.path().join(format!("f{i}")), "x").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new(&format!("f{i}"))).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let parents: Vec<git2::Commit> = parent
                .iter()
                .map(|o| repo.find_commit(*o).unwrap())
                .collect();
            let refs: Vec<&git2::Commit> = parents.iter().collect();
            parent = Some(
                repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &refs)
                    .unwrap(),
            );
        }

        let people = people(tmp.path());
        assert_eq!(people.len(), 2, "{people:?}");

        let first = &people[0];
        assert_eq!(first.name, "Pasqual Peñalver", "the usual spelling leads");
        assert_eq!(first.email, "public@paski.dev");
        assert_eq!(first.commits, 3);
        assert_eq!(
            first.also_known_as,
            vec!["paski".to_string()],
            "the other spelling is kept, not thrown away"
        );
        assert_eq!(people[1].email, "other@example.com");
    }

    /// An address is matched case-insensitively, the way git does.
    #[test]
    fn the_same_address_in_capitals_is_the_same_person() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        let mut parent: Option<git2::Oid> = None;
        for (i, email) in ["Dev@Example.COM", "dev@example.com"].iter().enumerate() {
            let sig =
                git2::Signature::new("Dev", email, &git2::Time::new(1_700_000_000, 0)).unwrap();
            std::fs::write(tmp.path().join(format!("f{i}")), "x").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new(&format!("f{i}"))).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let parents: Vec<git2::Commit> = parent
                .iter()
                .map(|o| repo.find_commit(*o).unwrap())
                .collect();
            let refs: Vec<&git2::Commit> = parents.iter().collect();
            parent = Some(
                repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &refs)
                    .unwrap(),
            );
        }

        let people = people(tmp.path());
        assert_eq!(people.len(), 1, "{people:?}");
        assert_eq!(people[0].commits, 2);
    }

    /// The signature is read from the object header: present or not, never
    /// "valid" — proving that needs gpg, and this must stay free.
    #[test]
    fn a_signature_is_recognised_by_its_preamble() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        let sig =
            git2::Signature::new("Dev", "dev@example.com", &git2::Time::new(1_700_000_000, 0))
                .unwrap();
        std::fs::write(tmp.path().join("f"), "x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("f")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();

        let content = repo
            .commit_create_buffer(&sig, &sig, "signed", &tree, &[])
            .unwrap();
        let armor =
            "-----BEGIN PGP SIGNATURE-----\n\nnot a real one\n-----END PGP SIGNATURE-----\n";
        let oid = repo
            .commit_signed(content.as_str().unwrap(), armor, None)
            .unwrap();
        repo.reference("refs/heads/main", oid, true, "test")
            .unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let commit = repo.find_commit(oid).unwrap();
        assert_eq!(signature_kind(&commit), Some(SigKind::Pgp));

        let people = people(tmp.path());
        assert_eq!(people[0].signed, 1);
        assert_eq!(people[0].sig_kinds, vec![SigKind::Pgp]);
    }

    #[test]
    fn an_unsigned_commit_reports_no_signature() {
        let tmp = tempfile::tempdir().unwrap();
        repo_with_history(tmp.path());
        let people = people(tmp.path());
        assert!(people.iter().all(|p| p.signed == 0), "{people:?}");
        assert!(people.iter().all(|p| p.sig_kinds.is_empty()));
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
