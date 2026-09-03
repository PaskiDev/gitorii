//! Stats view state: the numbers, and how they are fetched.
//!
//! The three halves of `cmd::stats` cost three different amounts, so they are
//! fetched differently. Shape and history are read when the view opens, which
//! is a bounded wait. Churn diffs hundreds of commits, so it goes to a worker
//! thread and lands later — the view says "measuring…" until it does, rather
//! than freezing the whole TUI on a large repo.

use super::*;
use crate::stats::{self, Churn, History, Person, Shape};

/// Which repository the numbers are about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatsMode {
    #[default]
    Repo,
    Workspace,
    /// Everyone who has committed, with every address and signature the repo
    /// records for them.
    People,
}

/// One line of the workspace table.
#[derive(Clone)]
pub struct RepoRow {
    pub name: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub dirty: usize,
    pub files: usize,
    pub bytes: u64,
}

/// The churn worker's thread, joined when it is dropped.
///
/// A detached thread outlives the `App` that started it, and this one opens a
/// libgit2 repository of its own. Several of them still walking commits while
/// the test binary tears down is what took the whole process out with a
/// SIGSEGV, so the handle is kept and the thread is joined with the state that
/// owns it.
#[derive(Default)]
struct ChurnWorker(Option<std::thread::JoinHandle<()>>);

impl Drop for ChurnWorker {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // The channel is unbounded, so the worker never blocks on its
            // send: this waits for `churn` alone, which stops after
            // `CHURN_CAP` commits. A panic inside the worker is already
            // reported by the default hook, so the result has nothing to add.
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
pub struct StatsState {
    pub mode: StatsMode,
    pub shape: Shape,
    pub history: History,
    /// `None` until the worker answers.
    pub churn: Option<Churn>,
    pub churn_rx: Option<std::sync::mpsc::Receiver<Churn>>,
    /// The thread behind `churn_rx`, held so it is joined and not detached.
    churn_worker: ChurnWorker,
    pub rows: Vec<RepoRow>,
    pub people: Vec<Person>,
    /// Row of the people table.
    pub people_idx: usize,
    /// Which repo path the numbers belong to, so a switch of repo is noticed.
    pub loaded_for: Option<String>,
}

impl Clone for StatsState {
    /// The receiver is not cloneable and a clone would not be listening to
    /// anything anyway; a cloned state simply has no worker in flight. The
    /// thread stays with the original, which is the half that can still read
    /// its answer.
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            shape: self.shape.clone(),
            history: self.history.clone(),
            churn: self.churn.clone(),
            churn_rx: None,
            churn_worker: ChurnWorker(None),
            rows: self.rows.clone(),
            people: self.people.clone(),
            people_idx: self.people_idx,
            loaded_for: self.loaded_for.clone(),
        }
    }
}

impl App {
    /// Read the cheap halves now and start the expensive one.
    pub fn load_stats(&mut self) {
        let path = std::path::PathBuf::from(&self.repo_path);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Walking the sidebar calls `go_to` on every step, so this is entered
        // once per keypress. Re-reading the cheap halves is what keeps the
        // numbers fresh; starting a second churn worker for a repository
        // already being measured only piles up threads behind the first.
        let measuring_this_repo = self.stats_view.churn_worker.0.is_some()
            && self.stats_view.loaded_for.as_deref() == Some(self.repo_path.as_str());

        self.stats_view.shape = stats::shape(&path);
        self.stats_view.history = stats::history(&path, now);
        self.stats_view.loaded_for = Some(self.repo_path.clone());

        if !measuring_this_repo {
            self.stats_view.churn = None;
            // Churn on a worker: on a big repo this is hundreds of tree diffs,
            // and the UI must keep drawing while they happen. Storing the
            // handle drops the previous worker, joining it — on the path that
            // gets here it has already answered, and on a change of repository
            // waiting for the old one beats leaving it running.
            let (tx, rx) = std::sync::mpsc::channel();
            let handle = std::thread::spawn(move || {
                let _ = tx.send(stats::churn(&path));
            });
            self.stats_view.churn_rx = Some(rx);
            self.stats_view.churn_worker = ChurnWorker(Some(handle));
        }

        self.stats_view.people = stats::people(std::path::Path::new(&self.repo_path));
        let last = self.stats_view.people.len().saturating_sub(1);
        self.stats_view.people_idx = self.stats_view.people_idx.min(last);

        self.load_workspace_rows();
    }

    /// The workspace table. Each repo costs an index read, which is cheap
    /// enough to do inline; the history walk is deliberately not done per repo.
    fn load_workspace_rows(&mut self) {
        self.stats_view.rows.clear();
        let Some(name) = self.active_workspace.clone() else {
            return;
        };
        self.load_workspaces();
        let Some(ws) = self
            .workspace_view
            .workspaces
            .iter()
            .find(|w| w.name == name)
            .cloned()
        else {
            return;
        };
        for repo in &ws.repos {
            let shape = stats::shape(std::path::Path::new(&repo.path));
            self.stats_view.rows.push(RepoRow {
                name: std::path::Path::new(&repo.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| repo.path.clone()),
                branch: repo.branch.clone(),
                ahead: repo.ahead,
                behind: repo.behind,
                dirty: if repo.dirty { shape.dirty.max(1) } else { 0 },
                files: shape.files,
                bytes: shape.bytes,
            });
        }
    }

    /// Take the churn result if the worker has finished. Called from the tick
    /// loop, like the OAuth worker.
    pub fn poll_stats_worker(&mut self) {
        let Some(rx) = &self.stats_view.churn_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(churn) => {
                self.stats_view.churn = Some(churn);
                self.stats_view.churn_rx = None;
                // It has sent its answer, so it is on its way out: joining it
                // here costs nothing and leaves no thread behind.
                self.stats_view.churn_worker = ChurnWorker(None);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // The worker died without answering: stop waiting on it, and
                // show nothing rather than "measuring…" for ever.
                self.stats_view.churn = Some(Churn::default());
                self.stats_view.churn_rx = None;
                self.stats_view.churn_worker = ChurnWorker(None);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    pub fn stats_toggle_mode(&mut self) {
        self.stats_view.mode = match self.stats_view.mode {
            StatsMode::Repo => StatsMode::Workspace,
            StatsMode::Workspace => StatsMode::People,
            StatsMode::People => StatsMode::Repo,
        };
    }

    pub fn stats_move(&mut self, delta: isize) {
        let last = self.stats_view.people.len().saturating_sub(1) as isize;
        let idx = self.stats_view.people_idx as isize + delta;
        self.stats_view.people_idx = idx.clamp(0, last.max(0)) as usize;
    }

    pub fn stats_selected_person(&self) -> Option<&Person> {
        self.stats_view.people.get(self.stats_view.people_idx)
    }

    /// Totals across the workspace, for the line under the table.
    pub fn stats_workspace_totals(&self) -> (usize, u64, usize, usize, usize) {
        let rows = &self.stats_view.rows;
        (
            rows.iter().map(|r| r.files).sum(),
            rows.iter().map(|r| r.bytes).sum(),
            rows.iter().filter(|r| r.dirty > 0).count(),
            rows.iter().map(|r| r.ahead).sum(),
            rows.iter().map(|r| r.behind).sum(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walking the sidebar calls `go_to` on every step, and Stats sits at
    /// index 1, so `load_stats` used to start another churn worker per
    /// keypress — each with a libgit2 repository of its own. One repository
    /// being measured means one worker measuring it.
    #[test]
    fn a_second_load_does_not_stack_another_worker() {
        let mut app = App::test_blank();

        app.load_stats();
        assert!(
            app.stats_view.churn_worker.0.is_some(),
            "the first load starts the worker"
        );

        // Nothing has polled the receiver, so the first worker still owns the
        // answer and the second load has no reason to start a rival.
        app.load_stats();
        assert!(
            app.stats_view.churn_rx.is_some(),
            "the first worker is still the one being waited on"
        );
    }

    /// Dropping the state joins the thread. If it did not, the worker would
    /// outlive the `App` and keep walking commits through a libgit2 handle
    /// nobody owns any more.
    #[test]
    fn dropping_the_state_leaves_no_worker_running() {
        let mut app = App::test_blank();
        app.load_stats();
        drop(app);
        // Reaching here at all is the assertion: `drop` returns only once the
        // worker has been joined.
    }
}
