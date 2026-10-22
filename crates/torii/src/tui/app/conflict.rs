//! Conflict prompt: a merge / rebase / cherry-pick run from the TUI that
//! stops on conflicts asks which side wins — everywhere or file by file —
//! and carries the operation on, as the CLI does.
//!
//! The operation itself runs as a `torii` child with no terminal, so it
//! leaves the conflicts in place; the TUI then reads them from the index.

use super::*;
use crate::cli::conflict_picker::PickerState;
use crate::vcs::conflict::{self, ConflictFile, OpKind, Resolution};

/// An operation that may stop on conflicts.
#[derive(Debug, Clone, PartialEq)]
pub enum Integrate {
    Merge(String),
    Rebase(String),
    CherryPick(String),
}

impl Integrate {
    pub fn op(&self) -> OpKind {
        match self {
            Integrate::Merge(_) => OpKind::Merge,
            Integrate::Rebase(_) => OpKind::Rebase,
            Integrate::CherryPick(_) => OpKind::CherryPick,
        }
    }

    pub fn target(&self) -> &str {
        match self {
            Integrate::Merge(t) | Integrate::Rebase(t) | Integrate::CherryPick(t) => t,
        }
    }

    fn start_args(&self) -> Vec<String> {
        let t = self.target().to_string();
        match self {
            Integrate::Merge(_) => vec!["sync".into(), t, "--merge".into()],
            Integrate::Rebase(_) => vec!["history".into(), "rebase".into(), t],
            Integrate::CherryPick(_) => vec!["cherry-pick".into(), t],
        }
    }

    /// Finishes the operation once no file is left in conflict. A rebase
    /// may stop again at a later commit.
    fn continue_args(&self) -> Vec<String> {
        match self {
            Integrate::Merge(t) => vec!["save".into(), "-m".into(), format!("Merge branch '{t}'")],
            Integrate::Rebase(_) => {
                vec!["history".into(), "rebase".into(), "--continue".into()]
            }
            Integrate::CherryPick(_) => vec!["cherry-pick".into(), "--continue".into()],
        }
    }

    /// What to type after resolving by hand.
    fn continue_hint(&self) -> &'static str {
        match self {
            Integrate::Merge(_) => "torii save -m \"merge\"",
            Integrate::Rebase(_) => "torii history rebase --continue",
            Integrate::CherryPick(_) => "torii cherry-pick --continue",
        }
    }

    fn done_msg(&self) -> String {
        match self {
            Integrate::Merge(t) => format!("merged: {t}"),
            Integrate::Rebase(t) => format!("rebased onto: {t}"),
            Integrate::CherryPick(t) => format!("cherry-picked: {t}"),
        }
    }

    fn failed_msg(&self) -> String {
        match self {
            Integrate::Merge(t) => format!("merge failed: {t}"),
            Integrate::Rebase(t) => format!("rebase failed onto: {t}"),
            Integrate::CherryPick(t) => format!("cherry-pick failed: {t}"),
        }
    }
}

/// Open while conflicts wait for an answer. `picker` is `Some` once the
/// user asked to choose file by file.
pub struct ConflictPrompt {
    pub job: Integrate,
    /// The user's side: the branch they were on when the operation started.
    pub local_label: String,
    pub files: Vec<ConflictFile>,
    pub picker: Option<PickerState>,
}

fn run_torii(repo_path: &str, args: &[String]) -> bool {
    std::process::Command::new(crate::tui::torii_exe())
        .args(args)
        .current_dir(repo_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl App {
    /// Run `job`; on conflicts, open the prompt instead of reporting.
    pub fn integrate(&mut self, job: Integrate) {
        let local_label = self.branch.clone();
        let ok = run_torii(&self.repo_path, &job.start_args());
        self.after_integrate(job, local_label, ok);
    }

    fn after_integrate(&mut self, job: Integrate, local_label: String, ok: bool) {
        let files = Repository::discover(&self.repo_path)
            .ok()
            .and_then(|r| conflict::list_conflicts(&r).ok())
            .unwrap_or_default();
        if !files.is_empty() {
            self.conflict = Some(ConflictPrompt {
                job,
                local_label,
                files,
                picker: None,
            });
        } else if ok {
            self.log_event(job.done_msg(), EventKind::Success);
        } else {
            self.log_event(job.failed_msg(), EventKind::Error);
        }
        self.refresh().ok();
    }

    /// Apply the answer to the open prompt and, when nothing is left in
    /// conflict, carry the operation on.
    pub fn conflict_resolve(&mut self, res: Resolution) {
        let Some(p) = self.conflict.take() else {
            return;
        };
        let left = if res == Resolution::Manual {
            Ok(p.files.len())
        } else {
            Repository::discover(&self.repo_path)
                .map_err(anyhow::Error::from)
                .and_then(|r| conflict::resolve_files(&r, p.job.op(), &res))
        };
        match left {
            Err(e) => {
                self.log_event(format!("resolve failed: {e}"), EventKind::Error);
                self.refresh().ok();
            }
            Ok(0) => {
                let ok = run_torii(&self.repo_path, &p.job.continue_args());
                self.after_integrate(p.job, p.local_label, ok);
            }
            Ok(n) => {
                self.log_event(
                    format!(
                        "{n} file(s) left in conflict — resolve them, then run: {}",
                        p.job.continue_hint()
                    ),
                    EventKind::Info,
                );
                self.refresh().ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::conflict::Side;
    use std::path::Path;

    fn commit(repo: &Repository, files: &[(&str, &str)], parents: &[git2::Oid]) -> git2::Oid {
        let dir = repo.workdir().unwrap();
        let mut index = repo.index().unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
            index.add_path(Path::new(name)).unwrap();
        }
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        let parents: Vec<_> = parents
            .iter()
            .map(|p| repo.find_commit(*p).unwrap())
            .collect();
        let refs: Vec<_> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &refs)
            .unwrap()
    }

    /// `a` and `b` conflict between HEAD (ours) and `feat` (theirs), with the
    /// merge left in progress as `torii sync feat --merge` leaves it.
    fn merge_in_conflict() -> (tempfile::TempDir, App) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        let base = commit(&repo, &[("a", "base\n"), ("b", "base\n")], &[]);
        let ours = commit(&repo, &[("a", "ours\n"), ("b", "ours\n")], &[base]);
        repo.set_head_detached(base).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        let theirs = commit(&repo, &[("a", "theirs\n"), ("b", "theirs\n")], &[base]);
        repo.branch("feat", &repo.find_commit(theirs).unwrap(), true)
            .unwrap();
        repo.set_head_detached(ours).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        let ann = repo.find_annotated_commit(theirs).unwrap();
        repo.merge(&[&ann], None, None).unwrap();

        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        let files = conflict::list_conflicts(&repo).unwrap();
        assert_eq!(files.len(), 2);
        app.conflict = Some(ConflictPrompt {
            job: Integrate::Merge("feat".into()),
            local_label: "main".into(),
            files,
            picker: None,
        });
        (tmp, app)
    }

    fn read(tmp: &tempfile::TempDir, name: &str) -> String {
        std::fs::read_to_string(tmp.path().join(name)).unwrap()
    }

    #[test]
    fn per_file_resolves_the_chosen_and_leaves_the_rest() {
        let (tmp, mut app) = merge_in_conflict();
        app.conflict_resolve(Resolution::PerFile(vec![("a".into(), Side::Local)]));

        assert!(app.conflict.is_none(), "the prompt closes once answered");
        assert_eq!(read(&tmp, "a"), "ours\n");
        assert!(read(&tmp, "b").contains("<<<<<<<"), "b was not chosen");
        let left = conflict::list_conflicts(&Repository::open(tmp.path()).unwrap()).unwrap();
        assert_eq!(left.len(), 1);
        let last = &app.event_log.last().unwrap().message;
        assert!(
            last.starts_with("1 file(s) left in conflict") && last.contains("torii save"),
            "{last}"
        );
    }

    #[test]
    fn manual_touches_nothing_and_says_how_to_go_on() {
        let (tmp, mut app) = merge_in_conflict();
        app.conflict_resolve(Resolution::Manual);

        assert!(app.conflict.is_none());
        assert!(read(&tmp, "a").contains("<<<<<<<"));
        assert!(read(&tmp, "b").contains("<<<<<<<"));
        let last = &app.event_log.last().unwrap().message;
        assert!(last.starts_with("2 file(s) left in conflict"), "{last}");
    }

    #[test]
    fn continue_finishes_each_operation_its_own_way() {
        let m = Integrate::Merge("feat".into());
        assert_eq!(m.start_args(), ["sync", "feat", "--merge"]);
        assert_eq!(m.continue_args(), ["save", "-m", "Merge branch 'feat'"]);
        assert_eq!(
            Integrate::Rebase("main".into()).continue_args(),
            ["history", "rebase", "--continue"]
        );
        assert_eq!(
            Integrate::CherryPick("abc1234".into()).continue_args(),
            ["cherry-pick", "--continue"]
        );
    }
}
