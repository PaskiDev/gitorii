use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::tui::app::{App, DiffLine, DiffLineKind};
use super::super::ui::{C_SUBTLE, C_DIM, C_GREEN, C_RED};

const BG_ADDED:   Color = Color::Rgb(20,  50,  20);
const BG_REMOVED: Color = Color::Rgb(50,  15,  15);
const BG_ADDED_HL:   Color = Color::Rgb(30,  90,  30);
const BG_REMOVED_HL: Color = Color::Rgb(90,  25,  25);
const C_HUNK:  Color = Color::Rgb(100, 160, 220);
const BG_HUNK: Color = Color::Rgb(15,  25,  40);

// ── LCS-based character diff ──────────────────────────────────────────────────

fn lcs(a: &[char], b: &[char]) -> Vec<Vec<usize>> {
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i-1] == b[j-1] { dp[i-1][j-1] + 1 } else { dp[i-1][j].max(dp[i][j-1]) };
        }
    }
    dp
}

// Returns (removed_highlights, added_highlights) — bool vecs marking changed chars
fn char_diff(removed: &str, added: &str) -> (Vec<bool>, Vec<bool>) {
    let a: Vec<char> = removed.chars().collect();
    let b: Vec<char> = added.chars().collect();
    let dp = lcs(&a, &b);

    let mut hl_a = vec![true; a.len()];
    let mut hl_b = vec![true; b.len()];

    let mut i = a.len();
    let mut j = b.len();
    while i > 0 && j > 0 {
        if a[i-1] == b[j-1] {
            hl_a[i-1] = false;
            hl_b[j-1] = false;
            i -= 1; j -= 1;
        } else if dp[i-1][j] >= dp[i][j-1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    (hl_a, hl_b)
}

// ── Render helpers ────────────────────────────────────────────────────────────

fn line_no_span(no: Option<u32>) -> Span<'static> {
    match no {
        Some(n) => Span::styled(format!("{:>4} ", n), Style::default().fg(C_DIM)),
        None    => Span::styled("     ", Style::default().fg(C_DIM)),
    }
}

fn render_plain_line(line: &DiffLine, fg: Color, bg: Color, prefix: &'static str) -> ListItem<'static> {
    let content: String = line.content.clone();
    ListItem::new(Line::from(vec![
        line_no_span(line.line_no),
        Span::styled(prefix, Style::default().fg(fg).bg(bg)),
        Span::styled(content, Style::default().fg(fg).bg(bg)),
    ]))
}

fn render_highlighted_pair(
    removed: &DiffLine,
    added: &DiffLine,
) -> (ListItem<'static>, ListItem<'static>) {
    let (hl_rem, hl_add) = char_diff(&removed.content, &added.content);

    let rem_spans = build_highlighted_spans(&removed.content, &hl_rem, C_RED, BG_REMOVED, BG_REMOVED_HL);
    let add_spans = build_highlighted_spans(&added.content,   &hl_add, C_GREEN, BG_ADDED, BG_ADDED_HL);

    let mut rem_line = vec![line_no_span(removed.line_no), Span::styled("- ", Style::default().fg(C_RED).bg(BG_REMOVED))];
    rem_line.extend(rem_spans);

    let mut add_line = vec![line_no_span(added.line_no), Span::styled("+ ", Style::default().fg(C_GREEN).bg(BG_ADDED))];
    add_line.extend(add_spans);

    (ListItem::new(Line::from(rem_line)), ListItem::new(Line::from(add_line)))
}

fn build_highlighted_spans(text: &str, highlights: &[bool], fg: Color, bg: Color, bg_hl: Color) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = vec![];
    let mut i = 0;
    while i < chars.len() {
        let hl = highlights[i];
        let mut j = i + 1;
        while j < chars.len() && highlights[j] == hl { j += 1; }
        let chunk: String = chars[i..j].iter().collect();
        spans.push(Span::styled(chunk, Style::default().fg(fg).bg(if hl { bg_hl } else { bg })));
        i = j;
    }
    spans
}

// ── Main render ───────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let bc = app.brand_color();

    // Header
    let inner_w = chunks[0].width.saturating_sub(2) as usize;
    let title_str = "⛩  diff";
    let file_str = app.diff.title.clone();
    let pad = inner_w.saturating_sub(title_str.chars().count() + file_str.chars().count() + 3);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(title_str, Style::default().fg(bc).add_modifier(Modifier::BOLD)),
            Span::raw(" ".repeat(pad)),
            Span::styled(file_str, Style::default().fg(C_SUBTLE)),
            Span::raw(" "),
        ]))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_type(app.border_type())
            .border_style(Style::default().fg(bc))),
        chunks[0],
    );

    // Build display lines with paired +/- highlighting
    let lines = &app.diff.lines;
    let mut items: Vec<ListItem> = vec![];
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        match line.kind {
            DiffLineKind::Header => {
                // File header — bold brand color
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("     ", Style::default()),
                    Span::styled(line.content.clone(), Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                ])));
                i += 1;
            }
            DiffLineKind::HunkHeader => {
                // Hunk separator bar
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("     ", Style::default().bg(BG_HUNK)),
                    Span::styled(line.content.clone(), Style::default().fg(C_HUNK).bg(BG_HUNK).add_modifier(Modifier::BOLD)),
                ])).style(Style::default().bg(BG_HUNK)));
                i += 1;
            }
            DiffLineKind::Removed => {
                // Look ahead for a paired Added line
                if i + 1 < lines.len() && lines[i + 1].kind == DiffLineKind::Added {
                    let (rem_item, add_item) = render_highlighted_pair(&lines[i], &lines[i + 1]);
                    items.push(rem_item);
                    items.push(add_item);
                    i += 2;
                } else {
                    items.push(render_plain_line(line, C_RED, BG_REMOVED, "- "));
                    i += 1;
                }
            }
            DiffLineKind::Added => {
                items.push(render_plain_line(line, C_GREEN, BG_ADDED, "+ "));
                i += 1;
            }
            DiffLineKind::Context => {
                items.push(ListItem::new(Line::from(vec![
                    line_no_span(line.line_no),
                    Span::styled("  ", Style::default()),
                    Span::styled(line.content.clone(), Style::default().fg(C_SUBTLE)),
                ])));
                i += 1;
            }
        }
    }

    // Apply scroll
    let visible: Vec<ListItem> = items.into_iter().skip(app.diff.scroll).collect();

    f.render_widget(
        List::new(visible)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(bc))),
        chunks[1],
    );

    // Footer
    let total = app.diff.lines.len();
    let pct = if total == 0 { 0 } else { (app.diff.scroll * 100) / total.max(1) };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" scroll  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[PgUp/PgDn]", Style::default().fg(bc)),
            Span::styled(" page  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" back  ", Style::default().fg(C_SUBTLE)),
            Span::styled(format!("{}%  {}/{} lines", pct, app.diff.scroll, total), Style::default().fg(C_DIM)),
        ])),
        chunks[2],
    );
}
