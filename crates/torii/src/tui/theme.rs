//! The visual language of the TUI, in one place.
//!
//! The chrome this module builds is the one drawn on gitorii.com: a single
//! window rather than a stack of nested boxes, hairline rules where the site
//! puts a 1px border, and the brand red spent only on the caret, the badge and
//! the focus ring. `gitorii-web/src/routes/theme.css` says it plainly — "Torii
//! red is an accent and nothing else" — and this file is that sentence applied
//! to a terminal.
//!
//! Views should reach for `panel_title`, `caret`, `rule_style` and friends
//! instead of building their own `Block::default().borders(Borders::ALL)`, so
//! that the next change of mind happens here and not in twenty-two files.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders},
    Frame,
};

use super::app::App;

// ── Palette ──────────────────────────────────────────────────────────────────
//
// Token for token with the site, with one deliberate departure noted below.

/// `--ink`: the text you are meant to read.
pub const INK: Color = Color::Rgb(242, 239, 236);
/// `--ink-dim`: labels, secondary columns, the view name in the chrome bar.
pub const INK_DIM: Color = Color::Rgb(168, 162, 157);
/// `--ink-faint`: key hints, inactive sidebar entries, timestamps.
pub const INK_FAINT: Color = Color::Rgb(111, 104, 99);

/// The site's `--rule` is `#2b2422`, which works because the page also owns
/// its background (`--term-bg`, near black). A terminal keeps whatever
/// background the user chose, so the rule is lifted until it survives on the
/// mid-greys people actually run, while staying quieter than [`INK_FAINT`].
pub const RULE: Color = Color::Rgb(74, 63, 60);

/// `--ok`: synced, succeeded, clean.
pub const OK: Color = Color::Rgb(74, 222, 128);
/// `--partial`: behind, pending, needs attention but is not an error.
pub const WARN: Color = Color::Rgb(217, 164, 65);
/// Failure. Distinct from the brand accent on purpose: red-as-status and
/// red-as-identity must not be the same red, or neither reads.
pub const BAD: Color = Color::Rgb(224, 76, 76);

/// The accent, for the caret and the badge. Comes from settings so a user who
/// picked their own brand colour keeps it.
pub fn accent(app: &App) -> Color {
    app.brand_color()
}

/// The selected row's background. The site uses `--accent-soft`, an alpha
/// wash; a terminal cell has no alpha, so settings hold the baked colour.
pub fn selection(app: &App) -> Color {
    app.selected_bg()
}

// ── Chrome ───────────────────────────────────────────────────────────────────

/// The one border on screen: the window itself.
pub fn frame(app: &App) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(RULE))
}

/// A hairline down the right edge — how a column is separated from the next
/// one now that columns are no longer boxes.
pub fn divider_right() -> Block<'static> {
    Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(RULE))
}

/// Where a vertical rule meets a horizontal one, and which way it points.
#[derive(Clone, Copy, PartialEq)]
pub enum Tick {
    /// A vertical rule starts below this row: `┬`.
    Down,
    /// A vertical rule ends above this row: `┴`.
    Up,
}

/// Draw a horizontal rule across `area`, tying it into the window border on
/// both sides and into any vertical rules that meet it.
///
/// `area` is the *inner* row — the one between the window's left and right
/// border columns — so the junction glyphs land on the border itself and the
/// rule reads as part of the frame rather than a line floating inside it.
///
/// `ticks` are absolute x positions of vertical rules crossing this row.
pub fn hrule(f: &mut Frame, area: Rect, ticks: &[(u16, Tick)]) {
    if area.width == 0 {
        return;
    }
    let style = Style::default().fg(RULE);

    // A rule laid over a column rule cuts the junction rather than erasing
    // it. The chrome draws its bottom rule after the views have drawn their
    // columns, so without this every column would stop a row short of it.
    for x in area.left()..area.right() {
        let over_column = f
            .buffer_mut()
            .cell((x, area.y))
            .is_some_and(|c| c.symbol() == "│");
        put(f, x, area.y, if over_column { "┴" } else { "─" }, style);
    }
    // The window's own border columns become junctions: a rule that stops one
    // cell short of the frame reads as a line dropped inside a box.
    put(f, area.left().saturating_sub(1), area.y, "├", style);
    put(f, area.right(), area.y, "┤", style);

    for (x, tick) in ticks {
        tie(f, *x, area.y, *tick);
    }
}

/// Mark that a vertical rule meets an already-drawn horizontal one at
/// `(x, y)`, and pick the glyph from what is there.
///
/// The chrome draws its rules before a view draws its columns, so a view's
/// column rule would otherwise start out of thin air just below the header
/// rule. It calls this to reach back up one row and cut the tee. A rule met
/// from both sides becomes a cross; anything that is not a rule is left
/// alone, so this can never scribble over text.
pub fn tie(f: &mut Frame, x: u16, y: u16, tick: Tick) {
    let existing = f
        .buffer_mut()
        .cell((x, y))
        .map(|c| c.symbol().to_string())
        .unwrap_or_default();
    let glyph = match (existing.as_str(), tick) {
        ("─", Tick::Down) => "┬",
        ("─", Tick::Up) => "┴",
        ("┬", Tick::Up) | ("┴", Tick::Down) | ("┼", _) => "┼",
        ("┬", Tick::Down) => "┬",
        ("┴", Tick::Up) => "┴",
        _ => return,
    };
    put(f, x, y, glyph, Style::default().fg(RULE));
}

/// Tie a view's column rules into whatever rule sits directly above its
/// content, so the columns start from the chrome instead of beside it.
pub fn tie_above(f: &mut Frame, area: Rect, xs: &[u16]) {
    let Some(y) = area.y.checked_sub(1) else {
        return;
    };
    for x in xs {
        tie(f, *x, y, Tick::Down);
    }
}

/// The same for the rule directly below the content. The chrome draws both of
/// its body rules before handing the body to a view, so a view can reach into
/// either of them.
pub fn tie_below(f: &mut Frame, area: Rect, xs: &[u16]) {
    let y = area.y + area.height;
    for x in xs {
        tie(f, *x, y, Tick::Up);
    }
}

/// A rule across a view's content, tied into the chrome on both sides.
///
/// A view is handed its content inset by one column from the sidebar's rule
/// on the left and the window border on the right, so that text never touches
/// a line. A rule drawn inside that content has to reach back across the
/// inset to meet them, which is what this does.
pub fn hrule_content(f: &mut Frame, area: Rect, ticks: &[(u16, Tick)]) {
    let widened = Rect {
        x: area.x.saturating_sub(1),
        y: area.y,
        width: area.width.saturating_add(2),
        height: area.height,
    };
    hrule(f, widened, ticks);
}

/// Write a single glyph, ignoring positions outside the viewport.
fn put(f: &mut Frame, x: u16, y: u16, symbol: &str, style: Style) {
    let inside = f.area();
    if x < inside.left() || x >= inside.right() || y < inside.top() || y >= inside.bottom() {
        return;
    }
    if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
        cell.set_symbol(symbol);
        cell.set_style(style);
    }
}

// ── Content ──────────────────────────────────────────────────────────────────

/// Split a pane into its heading row and the body below it — the shape a
/// boxed title used to give for free.
pub fn heading_and_body(area: Rect) -> [Rect; 2] {
    let rows = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
        ])
        .split(area);
    [rows[0], rows[1]]
}

/// The heading of a de-boxed panel: a word, a count, and nothing drawn around
/// it. The active panel is the one in full ink.
pub fn panel_title(label: &str, count: Option<usize>, active: bool) -> Vec<Span<'static>> {
    let style = if active {
        Style::default().fg(INK).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(INK_FAINT)
    };
    let mut spans = vec![Span::styled(label.to_string(), style)];
    if let Some(n) = count {
        spans.push(Span::styled(
            format!(" ({})", n),
            Style::default().fg(INK_FAINT),
        ));
    }
    spans
}

/// The selection marker. The site uses `›`; a block or a filled triangle reads
/// as a cursor from a different program.
pub fn caret(app: &App, selected: bool) -> Span<'static> {
    if selected {
        Span::styled("› ", Style::default().fg(accent(app)))
    } else {
        Span::raw("  ")
    }
}

/// Row style for a list entry.
pub fn row_style(app: &App, selected: bool) -> Style {
    if selected {
        Style::default()
            .bg(selection(app))
            .fg(INK)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(INK_DIM)
    }
}

/// One key hint, as the foot of the site's window sets them: the key in the
/// accent, the verb in faint ink, two spaces between pairs.
pub fn key_hint(app: &App, key: &str, label: &str) -> [Span<'static>; 2] {
    [
        Span::styled(key.to_string(), Style::default().fg(accent(app))),
        Span::styled(format!("  {}  ", label), Style::default().fg(INK_FAINT)),
    ]
}

/// Strip the square brackets an older chrome wrapped every key in.
///
/// The hint line is assembled across two thousand lines of per-view match
/// arms; rather than edit each one, the finished line passes through here on
/// its way to the screen. `[Enter]` becomes `Enter `, which is what the site
/// draws, and a span that is not a key is left alone.
pub fn unbracket(span: Span<'_>) -> Span<'_> {
    let content = span.content.as_ref();
    let trimmed = content.trim_matches(' ');
    if trimmed.len() < 3 || !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return span;
    }
    let lead = content.len() - content.trim_start_matches(' ').len();
    let inner = &trimmed[1..trimmed.len() - 1];
    let style = span.style;
    Span::styled(format!("{}{} ", " ".repeat(lead), inner), style)
}
