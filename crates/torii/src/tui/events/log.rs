//! Log view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

/// Ask for the argument a log operation needs before running it.
///
/// The comment above the ops menu claimed this already happened. It did
/// not: rebase, blame, remove-file and rewrite-dates returned their
/// `Action` straight from the menu, and every one of those handlers
/// begins by reading `history_view.input`, which nothing had filled in.
/// An empty input made the handler return without running anything and
/// without logging anything, so picking "rewrite dates" from the log
/// looked like a dead key.
///
/// The prompts themselves were never dead — they are written against
/// `View::History`, which renders the same rows as `View::Log` (history
/// is the log with the reflog state attached; they even share a sidebar
/// slot). Nothing ever switched to it, so the whole prompt layer sat
/// unreachable. Switching here is what wakes it up: `ui.rs` draws the
/// prompt line for `View::History`, and the event dispatcher already
/// routes keystrokes to `handle_history` while a text `confirm` is
/// pending. The action handlers switch back to `View::Log` when they
/// are done.
fn open_history_prompt(app: &mut App, confirm: HistoryConfirm) {
    app.history_view.input.clear();
    app.history_view.input2.clear();
    app.history_view.identity_error = None;
    app.history_view.confirm = confirm;
    app.view = View::History;
}

/// The ops menu while commits are picked — `views::log::log_ops_selection`
/// on the other side of the same index.
///
/// Reauthor does not ask for the old identity here. Every picked commit
/// already carries it, and it is the half that has to be *exact* for the
/// rewrite to match anything: retyping it is work the selection has
/// already done, with a chance of getting it wrong. Only the replacement
/// is typed, and `tui::mod` passes the picked OIDs as `--only` so the
/// rewrite stops at them.
fn selection_op(app: &mut App, idx: usize) -> Option<Action> {
    match idx {
        0 => {
            app.history_view.input = app.log.selection_identity.clone().unwrap_or_default();
            app.history_view.input2.clear();
            app.history_view.identity_error = None;
            app.history_view.confirm = HistoryConfirm::ReauthorNew;
            app.view = View::History;
        }
        1 => app.log_clear_selection(),
        _ => {}
    }
    None
}

pub(super) fn handle_log(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // A pending history prompt owns the keyboard until it is answered or
    // cancelled. The dispatcher sends `View::History` here when the
    // sidebar is unfocused ("fused into Log") and to `handle_history`
    // when it is focused, so the delegation has to live on this side to
    // hold for both. Guarded on `confirm`, not on the view, so a log with
    // no prompt open keeps every one of its own keybinds.
    if app.history_view.confirm != HistoryConfirm::None {
        return super::history::handle_history(key, app);
    }
    if app.log.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.log.search_mode = false;
                app.log.search_query.clear();
                app.log.filtered.clear();
            }
            (_, KeyCode::Enter) => {
                app.log.search_mode = false;
            }
            (_, KeyCode::Backspace) => {
                app.log.search_query.pop();
                app.log_update_filter();
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.log.search_query.push(c);
                app.log_update_filter();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if app.log.ops_mode {
        let ops_len = crate::tui::views::log::log_ops_for(app).len();
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.log.ops_idx > 0 => {
                app.log.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.log.ops_idx + 1 < ops_len => {
                app.log.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.log.ops_idx;
                app.log.ops_mode = false;
                if !app.log.selected.is_empty() {
                    return selection_op(app, idx);
                }
                // 0.8.3 — extended ops menu. Indices map onto the
                // `log_ops()` list; the renderer reads the same list
                // so the order can't drift. Ops that need no argument
                // (scan / clean) come back as Action variants and run
                // from the main loop's handler. Ops that need one go
                // through `open_history_prompt` to ask for it first.
                // Show-signature opens the armor overlay directly
                // because the worker is already a method on App.
                match idx {
                    0 => return Some(Action::OpenDiffFromLog),
                    1 => return Some(Action::LogCopyHash),
                    2 => {
                        app.log.search_mode = true;
                        app.log.search_query.clear();
                        app.log.filtered.clear();
                    }
                    3 => return Some(Action::HistoryCherryPick),
                    4 => open_history_prompt(app, HistoryConfirm::Rebase),
                    5 => {
                        if let Some(c) = app.commits.get(app.log.idx) {
                            let oid = c.full_hash.clone();
                            app.start_signature_overlay(oid);
                        }
                    }
                    6 => open_history_prompt(app, HistoryConfirm::Blame),
                    7 => open_history_prompt(app, HistoryConfirm::RemoveFile),
                    8 => open_history_prompt(app, HistoryConfirm::RewriteStart),
                    9 => open_history_prompt(app, HistoryConfirm::ReauthorOld),
                    10 => return Some(Action::HistoryScan),
                    11 => return Some(Action::HistoryClean),
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.log.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    // 0.7.36 — armor overlay grabs every key so the user can dismiss
    // it without accidentally firing log-navigation keybinds.
    if app.log.signature_overlay.is_some() {
        // Esc / any non-modifier key closes the modal.
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {
                app.log.signature_overlay = None;
                app.log_signature_rx = None;
            }
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            app.log_move_up();
            app.log.ops_mode = false;
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            app.log_move_down();
            app.log.ops_mode = false;
        }
        (_, KeyCode::Char('o')) => {
            app.log.ops_mode = true;
            app.log.ops_idx = 0;
        }
        // Space picks the commit under the caret. The first pick locks
        // the set to that author identity; a commit by anyone else is
        // refused out loud rather than quietly ignored, because the
        // greyed row and the dead keypress otherwise look like a bug.
        (_, KeyCode::Char(' ')) => {
            let idx = app.log.idx;
            if let Some(why) = app.log_toggle_selection(idx) {
                app.log_event(why, EventKind::Info);
            }
        }
        (_, KeyCode::Char('/')) => {
            app.log.search_mode = true;
            app.log.search_query.clear();
            app.log.filtered.clear();
        }
        // `A` narrows the log to the author of the commit under the
        // caret. It writes the same `author:<who>` query `/` accepts,
        // so the two are one filter with two ways in, and it uses the
        // full `Name <email>` because a bare name does not separate two
        // people who share one — or one person's two addresses.
        // Pressing it again on the same author lifts the filter.
        (KeyModifiers::SHIFT, KeyCode::Char('A')) | (_, KeyCode::Char('A')) => {
            let idx = app.log.idx;
            if let Some(msg) = app.log_filter_by_author(idx) {
                app.log_event(msg, EventKind::Info);
            }
        }
        // 0.7.36 — toggle the GPG signature column. Populates the
        // verdict cache the first time it turns on (cheap because we
        // only run gpg --verify on the commits already loaded into
        // the log slice, typically ≤ page_size).
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (_, KeyCode::Char('G')) => {
            app.log.show_signatures = !app.log.show_signatures;
            if app.log.show_signatures {
                app.refresh_signature_cache();
            }
        }
        // 0.7.36 — open the armor overlay for the currently selected
        // commit. Spawns a worker so the modal can render `loading…`
        // while gpg runs.
        (KeyModifiers::SHIFT, KeyCode::Char('S')) | (_, KeyCode::Char('S')) => {
            if let Some(c) = app.commits.get(app.log.idx) {
                let oid = c.full_hash.clone();
                app.start_signature_overlay(oid);
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod ops_prompt_tests {
    use super::*;

    fn press(code: KeyCode) -> event::KeyEvent {
        event::KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Capitals and `<>` arrive with SHIFT held; lowercase in the same
    /// string does not. Mirroring that is the whole point of the test
    /// that uses it.
    fn shift(c: char) -> event::KeyEvent {
        let mods = if c.is_uppercase() || "<>@!#$%^&*()_+~".contains(c) {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        event::KeyEvent::new(KeyCode::Char(c), mods)
    }

    fn log_with_ops_menu_on(idx: usize) -> App {
        let mut app = App::test_blank();
        app.view = View::Log;
        app.log.ops_mode = true;
        app.log.ops_idx = idx;
        app
    }

    fn entry(hash: &str, author: &str, email: &str) -> CommitEntry {
        CommitEntry {
            hash: hash.into(),
            full_hash: format!("{hash}{}", "0".repeat(40 - hash.len())),
            message: "a commit".into(),
            author: author.into(),
            author_email: email.into(),
            time: "now".into(),
        }
    }

    /// Three commits by two people: the first two agree, the third does
    /// not. Enough to lock a selection and to be shut out by one.
    fn log_with_two_authors() -> App {
        let mut app = App::test_blank();
        app.view = View::Log;
        app.commits = vec![
            entry("aaaaaaa", "paskidev", "old@proton.me"),
            entry("bbbbbbb", "paskidev", "old@proton.me"),
            entry("ccccccc", "Someone Else", "else@example.test"),
        ];
        app
    }

    /// The first pick locks the set to that identity; a commit by anyone
    /// else is refused, and refused *out loud* — a greyed row plus a
    /// keypress that does nothing is indistinguishable from a bug.
    #[test]
    fn the_selection_locks_onto_one_identity() {
        let mut app = log_with_two_authors();

        handle_log(press(KeyCode::Char(' ')), &mut app);
        assert_eq!(app.log.selected.len(), 1);
        assert_eq!(
            app.log.selection_identity.as_deref(),
            Some("paskidev <old@proton.me>")
        );

        app.log.idx = 1;
        handle_log(press(KeyCode::Char(' ')), &mut app);
        assert_eq!(app.log.selected.len(), 2, "same identity, joins the set");

        app.log.idx = 2;
        let events_before = app.event_log.len();
        handle_log(press(KeyCode::Char(' ')), &mut app);
        assert_eq!(app.log.selected.len(), 2, "a different author was let in");
        assert!(!app.log_selectable(2), "and it is greyed in the list");
        assert!(
            app.event_log.len() > events_before,
            "the refusal has to say something"
        );
    }

    /// Unpicking the last one releases the lock — otherwise the next
    /// Space is still bound to whoever happened to be picked first.
    #[test]
    fn emptying_the_selection_releases_the_identity() {
        let mut app = log_with_two_authors();
        handle_log(press(KeyCode::Char(' ')), &mut app);
        handle_log(press(KeyCode::Char(' ')), &mut app);

        assert!(app.log.selected.is_empty());
        assert!(app.log.selection_identity.is_none());

        app.log.idx = 2;
        handle_log(press(KeyCode::Char(' ')), &mut app);
        assert_eq!(
            app.log.selection_identity.as_deref(),
            Some("Someone Else <else@example.test>")
        );
    }

    /// With commits picked the dropdown is the short menu, and its
    /// reauthor entry skips the old-identity prompt: the selection
    /// already carries it, exactly as it has to be spelled.
    #[test]
    fn the_picked_menu_prefills_the_old_identity() {
        let mut app = log_with_two_authors();
        handle_log(press(KeyCode::Char(' ')), &mut app);
        handle_log(press(KeyCode::Char('o')), &mut app);

        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(action.is_none(), "nothing runs before the new identity");
        assert_eq!(app.history_view.confirm, HistoryConfirm::ReauthorNew);
        assert_eq!(app.view, View::History);
        assert_eq!(app.history_view.input, "paskidev <old@proton.me>");
        assert!(app.history_view.input2.is_empty());
    }

    /// Second entry of the short menu, and the way back to the long one.
    #[test]
    fn the_picked_menu_can_drop_the_picks() {
        let mut app = log_with_two_authors();
        handle_log(press(KeyCode::Char(' ')), &mut app);
        handle_log(press(KeyCode::Char('o')), &mut app);
        handle_log(press(KeyCode::Down), &mut app);
        handle_log(press(KeyCode::Enter), &mut app);

        assert!(app.log.selected.is_empty());
        assert!(app.log.selection_identity.is_none());
    }

    /// The short menu is two entries long, so the whole-history indices
    /// must not be reachable while it is up — index 9 there is reauthor
    /// over *every* commit that matches.
    #[test]
    fn the_picked_menu_cannot_reach_the_whole_history_ops() {
        let mut app = log_with_two_authors();
        handle_log(press(KeyCode::Char(' ')), &mut app);
        handle_log(press(KeyCode::Char('o')), &mut app);

        for _ in 0..12 {
            handle_log(press(KeyCode::Down), &mut app);
        }
        assert_eq!(app.log.ops_idx, 1, "cursor ran past the short menu");
    }

    /// The bug this pins: "rewrite dates" returned its action straight
    /// from the menu, with both date fields still empty. The handler does
    /// nothing when they are, and logs nothing either, so the entry read
    /// as a dead key — the operation was unreachable from the only view
    /// that offers it.
    #[test]
    fn rewrite_dates_asks_for_the_dates_before_it_runs_anything() {
        let mut app = log_with_ops_menu_on(8);
        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(
            action.is_none(),
            "nothing may run until both dates have been given"
        );
        assert_eq!(app.history_view.confirm, HistoryConfirm::RewriteStart);
        assert_eq!(
            app.view,
            View::History,
            "the prompt line is only drawn for the history view"
        );
        assert!(!app.log.ops_mode, "the menu closes behind the prompt");
    }

    /// Keystrokes have to reach the prompt *through* `handle_log`: with
    /// the sidebar unfocused the dispatcher routes `View::History` here,
    /// so without the delegation the log's own keybinds would swallow
    /// every character of the date.
    #[test]
    fn both_dates_are_typed_through_the_log_handler() {
        let mut app = log_with_ops_menu_on(8);
        handle_log(press(KeyCode::Enter), &mut app);

        for c in "2026-01-01".chars() {
            handle_log(press(KeyCode::Char(c)), &mut app);
        }
        handle_log(press(KeyCode::Enter), &mut app);
        assert_eq!(app.history_view.confirm, HistoryConfirm::RewriteEnd);

        for c in "2026-09-04".chars() {
            handle_log(press(KeyCode::Char(c)), &mut app);
        }
        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(
            matches!(action, Some(Action::HistoryRewrite)),
            "the second date confirms the rewrite"
        );
        assert_eq!(app.history_view.input, "2026-01-01");
        assert_eq!(app.history_view.input2, "2026-09-04");
    }

    /// The other three arguments-taking operations went the same way.
    #[test]
    fn the_other_prompting_operations_are_reachable_too() {
        for (idx, expected) in [
            (4, HistoryConfirm::Rebase),
            (6, HistoryConfirm::Blame),
            (7, HistoryConfirm::RemoveFile),
        ] {
            let mut app = log_with_ops_menu_on(idx);
            let action = handle_log(press(KeyCode::Enter), &mut app);
            assert!(action.is_none(), "op {idx} ran before it had its argument");
            assert_eq!(app.history_view.confirm, expected, "op {idx}");
        }
    }

    /// Reauthor had no entry anywhere in the TUI — the ops menu went
    /// straight from "rewrite dates" to "scan history", so the only way
    /// to change an identity was the CLI.
    #[test]
    fn reauthor_asks_for_both_identities_before_it_runs_anything() {
        let mut app = log_with_ops_menu_on(9);
        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(action.is_none(), "nothing may run until both are given");
        assert_eq!(app.history_view.confirm, HistoryConfirm::ReauthorOld);
        assert_eq!(app.view, View::History);

        for c in "Old Name <old@example.com>".chars() {
            handle_log(shift(c), &mut app);
        }
        handle_log(press(KeyCode::Enter), &mut app);
        assert_eq!(app.history_view.confirm, HistoryConfirm::ReauthorNew);

        for c in "New Name <new@example.com>".chars() {
            handle_log(shift(c), &mut app);
        }
        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(matches!(action, Some(Action::HistoryReauthor)));
        assert_eq!(app.history_view.input, "Old Name <old@example.com>");
        assert_eq!(app.history_view.input2, "New Name <new@example.com>");
        assert!(app.history_view.identity_error.is_none());
    }

    /// A bare email parses on the CLI, where `--since` and `--dry-run`
    /// are there to bound what it sweeps up. From the menu it is a
    /// blind match over the whole history, so the prompt holds until
    /// both halves are there — and says so, instead of eating Enter.
    #[test]
    fn half_an_identity_does_not_advance_the_reauthor_prompt() {
        let mut app = log_with_ops_menu_on(9);
        handle_log(press(KeyCode::Enter), &mut app);

        for c in "old@example.com".chars() {
            handle_log(press(KeyCode::Char(c)), &mut app);
        }
        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(action.is_none());
        assert_eq!(
            app.history_view.confirm,
            HistoryConfirm::ReauthorOld,
            "half an identity moved on to the second prompt"
        );
        let why = app
            .history_view
            .identity_error
            .as_deref()
            .expect("the refusal has to say why");
        assert!(why.contains("Name <email>"), "unhelpful refusal: {why}");

        // Typing again is the fix, so the complaint goes with the edit.
        handle_log(press(KeyCode::Backspace), &mut app);
        assert!(app.history_view.identity_error.is_none());
    }

    /// Same rule on the second prompt: a name with no email would be
    /// rejected by the CLI parser after the fact, with the TUI already
    /// back on the log and the message buried in the event feed.
    #[test]
    fn the_new_identity_is_held_to_the_same_form() {
        let mut app = log_with_ops_menu_on(9);
        handle_log(press(KeyCode::Enter), &mut app);
        for c in "Old Name <old@example.com>".chars() {
            handle_log(shift(c), &mut app);
        }
        handle_log(press(KeyCode::Enter), &mut app);

        for c in "New Name".chars() {
            handle_log(shift(c), &mut app);
        }
        let action = handle_log(press(KeyCode::Enter), &mut app);

        assert!(action.is_none(), "reauthor ran on half an identity");
        assert_eq!(app.history_view.confirm, HistoryConfirm::ReauthorNew);
        assert!(app.history_view.identity_error.is_some());
    }

    /// An identity is capitals and `<>`, which reach the handler with
    /// SHIFT held. The older prompts match on `KeyModifiers::NONE` alone
    /// and would drop every one of those characters, turning
    /// `Pasqual Peñalver <a@b.c>` into `asqual eñalver a@b.c`.
    #[test]
    fn shifted_characters_reach_the_identity_prompts() {
        let mut app = log_with_ops_menu_on(9);
        handle_log(press(KeyCode::Enter), &mut app);

        for c in "Pasqual Peñalver <public@paski.dev>".chars() {
            handle_log(shift(c), &mut app);
        }

        assert_eq!(
            app.history_view.input,
            "Pasqual Peñalver <public@paski.dev>"
        );
    }

    /// Inserting an entry mid-list silently repoints every index after
    /// it. These are the two that moved.
    #[test]
    fn the_argument_free_operations_kept_their_indices() {
        let mut app = log_with_ops_menu_on(10);
        assert!(matches!(
            handle_log(press(KeyCode::Enter), &mut app),
            Some(Action::HistoryScan)
        ));

        let mut app = log_with_ops_menu_on(11);
        assert!(matches!(
            handle_log(press(KeyCode::Enter), &mut app),
            Some(Action::HistoryClean)
        ));
    }

    /// With no prompt pending the log keeps its own keys — the delegation
    /// is guarded on `confirm`, not on the view.
    #[test]
    fn a_log_with_no_prompt_open_still_handles_its_own_keys() {
        let mut app = App::test_blank();
        app.view = View::Log;
        handle_log(press(KeyCode::Char('/')), &mut app);
        assert!(app.log.search_mode, "`/` still opens the log's search");
    }

    /// `A` writes the same query `/` accepts, in the identity form —
    /// anything shorter would fold two people who share a name.
    #[test]
    fn a_narrows_the_log_to_the_author_under_the_caret() {
        let mut app = log_with_two_authors();
        app.log.idx = 2;

        handle_log(press(KeyCode::Char('A')), &mut app);

        assert_eq!(
            app.log.search_query,
            "author:Someone Else <else@example.test>"
        );
        assert_eq!(app.log.filtered, vec![2]);
        assert!(
            !app.log.search_mode,
            "the filter applies without opening the search prompt"
        );
    }

    /// Pressing it again on the same author is the way back out: the
    /// filter leaves the caret among that author's commits, so there is
    /// no row to move to that would clear it by other means.
    #[test]
    fn a_second_press_lifts_the_author_filter() {
        let mut app = log_with_two_authors();

        handle_log(press(KeyCode::Char('A')), &mut app);
        handle_log(press(KeyCode::Char('A')), &mut app);

        assert!(app.log.search_query.is_empty());
        assert!(
            app.log.filtered.is_empty(),
            "an empty filter shows them all"
        );
    }

    /// The term is matched against the whole `Name <email>`, so a bare
    /// name, a bare address and a pasted identity all land on the same
    /// commits — and a term that matches neither half matches nothing.
    #[test]
    fn the_author_term_matches_either_half_of_the_identity() {
        let mut app = log_with_two_authors();

        for term in [
            "paskidev",
            "old@proton.me",
            "paskidev <old@proton.me>",
            "PASKIDEV",
        ] {
            app.log.search_query = format!("author:{}", term);
            app.log_update_filter();
            assert_eq!(app.log.filtered, vec![0, 1], "term {:?}", term);
        }

        app.log.search_query = "author:nobody@example.test".into();
        app.log_update_filter();
        assert!(app.log.filtered.is_empty());
    }

    /// A bare `author:` is a query halfway through being typed. Treating
    /// it as a term that matches nothing would blank the list under the
    /// cursor between the colon and the first letter.
    #[test]
    fn a_bare_author_prefix_keeps_the_list_whole() {
        let mut app = log_with_two_authors();

        app.log.search_query = "author:".into();
        app.log_update_filter();

        assert_eq!(app.log.filtered, vec![0, 1, 2]);
    }

    /// The plain search matches the address too. It always claimed to
    /// search the author, but it read the name only, so searching for
    /// the address you are trying to get rid of found nothing.
    #[test]
    fn the_plain_search_reaches_the_email() {
        let mut app = log_with_two_authors();

        app.log.search_query = "proton.me".into();
        app.log_update_filter();

        assert_eq!(app.log.filtered, vec![0, 1]);
    }

    /// A filtered list pages like an unfiltered one: `j` past the last
    /// match asks for the next slice of history, because the filter only
    /// ever saw the commits already fetched. With everything loaded that
    /// ask is a no-op and the caret holds still.
    #[test]
    fn the_bottom_of_a_filtered_list_holds_when_all_is_loaded() {
        let mut app = log_with_two_authors();
        app.log.all_loaded = true;
        app.log.search_query = "author:paskidev".into();
        app.log_update_filter();
        app.log.idx = 1; // the last of the two matches

        handle_log(press(KeyCode::Char('j')), &mut app);

        assert_eq!(
            app.log.idx, 1,
            "and it does not fall onto a filtered-out row"
        );
    }

    /// The qualifier is case-insensitive and only a qualifier at the
    /// front. `get` does the bounds check, so a query opening with a
    /// multi-byte character is a miss and not a panic on a char
    /// boundary.
    #[test]
    fn the_qualifier_is_parsed_off_the_front_only() {
        assert_eq!(author_query("author:paski"), Some("paski"));
        assert_eq!(author_query("AUTHOR: paski "), Some("paski"));
        assert_eq!(author_query("author:"), Some(""));
        assert_eq!(author_query("fix the author: line"), None);
        assert_eq!(author_query("Peñalver"), None);
        assert_eq!(author_query("añ"), None);
    }
}
