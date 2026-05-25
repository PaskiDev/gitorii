//! Commit graph rendering — lane-based ASCII like `git log --graph --all`.
//!
//! Pure logic, no TUI / git2 dependency. Take a topologically-sorted slice of
//! `GraphCommit` (id + parent ids), produce a `Vec<GraphRow>` whose
//! `lane_glyphs` field is the prefix to render before the commit subject.
//!
//! Rendering vocabulary (single-cell glyphs + spaces between lanes):
//!   `*`  the active commit on its lane
//!   `|`  a continuing lane
//!   `\\` lane joining to the right (merge incoming or fork)
//!   `/`  lane joining to the left (merge incoming from right or fork)
//!   ` `  empty lane
//!
//! Each row produces TWO lines:
//!   1. "commit line"   — one column per active lane: `*` for the commit's
//!      lane, `|` for others.
//!   2. "transition line" (optional) — only present when lanes split or merge
//!      between this commit and the next. Shows `\` / `/` / `|` joins.
//!
//! For simplicity:
//! - commit's first parent inherits its lane;
//! - extra parents (merge) open new lanes to the right;
//! - when a lane's tip is no longer referenced by any later commit, it closes
//!   on the next transition line as `/` joining toward its first-parent lane.

/// Visual glyph set for the graph. Pure data, no rendering — `render` reads
/// the glyphs from the supplied set when emitting commit / transition lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphStyle {
    /// Plain ASCII (`* | / \`). Maximum portability.
    Ascii,
    /// Unicode box-drawing curves (`● │ ╮ ╰`). Recommended default.
    Curves,
    /// Heavy box-drawing (`⬢ ┃ ┓ ┗`). Bold, attention-grabbing.
    Heavy,
    /// Wide spacing + filled circles. Compact one-line per commit.
    Bubbles,
    /// Same glyphs as Bubbles but each commit spans 2 lines: a node row
    /// followed by an info-padding row. Use with `expanded_extra_lines()`.
    BubblesX,
}

impl GraphStyle {
    pub fn from_str(s: &str) -> Self {
        match s {
            "ascii" => Self::Ascii,
            "heavy" => Self::Heavy,
            "bubbles" => Self::Bubbles,
            "bubbles-x" | "bubbles_x" | "bubblesx" => Self::BubblesX,
            _ => Self::Curves,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Curves => "curves",
            Self::Heavy => "heavy",
            Self::Bubbles => "bubbles",
            Self::BubblesX => "bubbles-x",
        }
    }

    /// Glyph for the active commit on its lane, varying by parent count:
    /// 0 = root, 1 = normal, ≥2 = merge.
    ///
    /// Curves / Bubbles / BubblesX share the bullseye family (〇 ⦿ ◉) for a
    /// "node-on-a-line" look that reads like a graph editor (≈ kraken).
    pub fn commit_glyph(self, parent_count: usize) -> char {
        match (self, parent_count) {
            (Self::Ascii, _) => '*',
            (Self::Heavy, 0) => '□',
            (Self::Heavy, 1) => '◉',
            (Self::Heavy, _) => '◆',
            (Self::Curves | Self::Bubbles | Self::BubblesX, 0) => '〇',
            (Self::Curves | Self::Bubbles | Self::BubblesX, 1) => '⦿',
            (Self::Curves | Self::Bubbles | Self::BubblesX, _) => '◉',
        }
    }

    /// Vertical lane-continues glyph.
    pub fn lane_glyph(self) -> char {
        match self {
            Self::Ascii => '|',
            Self::Heavy => '┃',
            Self::Curves | Self::Bubbles | Self::BubblesX => '│',
        }
    }

    /// Glyph for a lane closing toward the left (fork-end / merge-target).
    /// Curves / Bubbles use ◟ (lower-left half-circle) to suggest a smooth
    /// arc from the closing lane into its target.
    pub fn close_left_glyph(self) -> char {
        match self {
            Self::Ascii => '/',
            Self::Heavy => '┛',
            Self::Curves | Self::Bubbles | Self::BubblesX => '◟',
        }
    }

    /// Glyph for a lane opening toward the right (new merge parent).
    /// Curves / Bubbles use ◝ (upper-right half-circle) — mirror of ◟.
    pub fn open_right_glyph(self) -> char {
        match self {
            Self::Ascii => '\\',
            Self::Heavy => '┓',
            Self::Curves | Self::Bubbles | Self::BubblesX => '◝',
        }
    }

    /// Cells of horizontal padding between lanes. Bubbles styles use wider
    /// spacing so commit nodes have room to breathe.
    pub fn lane_spacing(self) -> usize {
        match self {
            Self::Ascii | Self::Curves | Self::Heavy => 1,
            Self::Bubbles | Self::BubblesX => 3,
        }
    }

    /// Number of *extra* padding lines to insert below each commit row.
    /// 0 for compact styles. BubblesX uses 1 (commit row + breather row).
    pub fn expanded_extra_lines(self) -> usize {
        match self {
            Self::BubblesX => 1,
            _ => 0,
        }
    }
}

impl Default for GraphStyle {
    fn default() -> Self {
        Self::Curves
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub id: String,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// Prefix line: lane glyphs at the moment of this commit (e.g. "* | |").
    pub commit_line: String,
    /// Optional transition line shown BEFORE the next commit_line. Empty if
    /// no lane changes happen between this commit and the next.
    pub transition_line: String,
    /// Lane index where the commit sits (0-based, useful for coloring).
    pub lane: usize,
    /// Number of parents (1 = normal, 2+ = merge, 0 = root).
    pub parent_count: usize,
}

/// Render the commit graph using the default ASCII style. Kept for callers
/// that don't need to choose a style. New code should prefer `render_with`.
#[allow(dead_code)]
pub fn render(commits: &[GraphCommit]) -> Vec<GraphRow> {
    render_with(commits, GraphStyle::Ascii)
}

/// Render the commit graph for an ordered list of commits with a chosen
/// glyph style.
///
/// Input order MUST be topological (children before parents). Use git2's
/// `Sort::TOPOLOGICAL | Sort::TIME` for live data.
pub fn render_with(commits: &[GraphCommit], style: GraphStyle) -> Vec<GraphRow> {
    let mut rows = Vec::with_capacity(commits.len());
    // Each lane holds the OID it currently expects to render next, or None.
    let mut lanes: Vec<Option<String>> = Vec::new();

    for (idx, commit) in commits.iter().enumerate() {
        // 1. Find or assign this commit's lane.
        let lane = match lanes.iter().position(|l| l.as_deref() == Some(&commit.id)) {
            Some(i) => i,
            None => {
                // First time seen — append a new lane on the right.
                lanes.push(Some(commit.id.clone()));
                lanes.len() - 1
            }
        };

        // 2. Build the commit line. Use the styled glyphs.
        let parent_count = commit.parents.len();
        let pre_width = active_width(&lanes);
        let lane_g = style.lane_glyph();
        let commit_g = style.commit_glyph(parent_count);
        let spacing = style.lane_spacing();
        let pad: String = std::iter::repeat(' ').take(spacing).collect();
        let mut commit_line = String::with_capacity(pre_width * (1 + spacing));
        for i in 0..pre_width {
            if i > 0 {
                commit_line.push_str(&pad);
            }
            if i == lane {
                commit_line.push(commit_g);
            } else if lanes[i].is_some() {
                commit_line.push(lane_g);
            } else {
                commit_line.push(' ');
            }
        }

        // 3. Replace this commit's lane with its first parent (if any), open
        //    additional lanes for extra parents.
        if parent_count == 0 {
            lanes[lane] = None;
        } else {
            // First parent stays on this lane (or merges into existing lane
            // already holding this parent).
            let first = commit.parents[0].clone();
            // If another lane already expects this OID, close ours and the
            // transition will join our lane to that one.
            let already = lanes
                .iter()
                .enumerate()
                .find(|(i, l)| *i != lane && l.as_deref() == Some(&first));
            if already.is_some() {
                lanes[lane] = None;
            } else {
                lanes[lane] = Some(first);
            }

            // Extra parents (merge): each opens (or joins) a lane.
            for p in &commit.parents[1..] {
                if !lanes.iter().any(|l| l.as_deref() == Some(p.as_str())) {
                    // Reuse a free slot if any, else append.
                    let slot = lanes.iter().position(|l| l.is_none());
                    match slot {
                        Some(i) => lanes[i] = Some(p.clone()),
                        None => lanes.push(Some(p.clone())),
                    }
                }
            }
        }

        // 4. Compute transition line vs the previous lanes snapshot.
        //    For now we emit a simple straight-down or `/` close transition.
        //    Full merge zigzags would need pre/post diff with `\` cells.
        let post_width = active_width(&lanes);
        let width = pre_width.max(post_width);
        let close_g = style.close_left_glyph();
        let open_g = style.open_right_glyph();
        let mut transition = String::with_capacity(width * (1 + spacing));
        let mut any_change = false;
        for i in 0..width {
            if i > 0 {
                transition.push_str(&pad);
            }
            let was_active = i < pre_width
                && (i == lane || lanes_at_commit_active(&lanes, i, lane, commit, idx));
            let now_active = i < lanes.len() && lanes[i].is_some();
            if i == lane && parent_count >= 2 {
                transition.push(lane_g);
            } else if !now_active && was_active {
                transition.push(close_g);
                any_change = true;
            } else if now_active {
                transition.push(lane_g);
            } else {
                transition.push(' ');
            }
        }
        // Mark newly-opened merge parent lanes (right of `lane`) with open_g.
        // Replace by lane index — char-aware so it works with multi-byte
        // Unicode glyphs and variable spacing.
        if parent_count >= 2 {
            let new_parent_ids: Vec<&String> = commit.parents[1..].iter().collect();
            for npid in new_parent_ids {
                if let Some(i) = lanes.iter().position(|l| l.as_deref() == Some(npid.as_str())) {
                    if i > lane {
                        let chars: Vec<char> = transition.chars().collect();
                        let cell = i * (1 + spacing);
                        if cell < chars.len() {
                            let cur = chars[cell];
                            if cur == lane_g || cur == ' ' {
                                let mut new_chars = chars;
                                new_chars[cell] = open_g;
                                transition = new_chars.into_iter().collect();
                                any_change = true;
                            }
                        }
                    }
                }
            }
        }

        // Trim trailing whitespace from transition so empty stays empty.
        let trimmed = transition.trim_end().to_string();
        let transition_final = if any_change && !trimmed.is_empty() {
            trimmed
        } else {
            String::new()
        };

        // 5. Trim trailing tail of None lanes for compact rendering.
        while lanes.last().map(|l| l.is_none()).unwrap_or(false) {
            lanes.pop();
        }

        rows.push(GraphRow {
            commit_line: commit_line.trim_end().to_string(),
            transition_line: transition_final,
            lane,
            parent_count,
        });
    }

    rows
}

fn active_width(lanes: &[Option<String>]) -> usize {
    lanes
        .iter()
        .rposition(|l| l.is_some())
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// Build a "breather" row that mirrors the current lanes but only shows
/// vertical glyphs — useful for expanded styles (BubblesX) which insert one
/// or more padding rows between commits to leave breathing room.
///
/// Pass the commit_line of the commit we're padding under; this function
/// derives lane positions from it (any non-space char becomes a lane glyph
/// of `style.lane_glyph()`).
pub fn padding_row(commit_line: &str, style: GraphStyle) -> String {
    let lane_g = style.lane_glyph();
    commit_line
        .chars()
        .map(|c| if c == ' ' { ' ' } else { lane_g })
        .collect()
}

/// Stub used only inside `render` to keep pre-mutation reasoning explicit.
/// Always returns true — kept for future expansion when we need to compare
/// pre/post lane snapshots properly.
fn lanes_at_commit_active(
    _lanes_after: &[Option<String>],
    _i: usize,
    _commit_lane: usize,
    _commit: &GraphCommit,
    _idx: usize,
) -> bool {
    true
}

/// Map an arbitrary lane index to a stable ANSI 256 color. Useful for TUI
/// callers that want consistent colour-per-lane across renders.
pub fn lane_color(lane: usize) -> u8 {
    // Hand-picked vivid hues, well-spaced in HSL. Stable per-lane between
    // renders so a branch keeps the same colour as you scroll.
    const PALETTE: &[u8] = &[
        39,   // bright cyan
        208,  // orange
        207,  // pink/magenta
        226,  // yellow
        46,   // green
        99,   // purple
        202,  // red-orange
        51,   // turquoise
        220,  // gold
        129,  // violet
    ];
    PALETTE[lane % PALETTE.len()]
}

/// Format a single ref label with a leading icon for visual scanning.
/// Used by both CLI graph output and TUI graph view.
///
///   `HEAD -> main`     → `★ HEAD -> main`
///   `HEAD`             → `★ HEAD (detached)`
///   `tag: v0.6.0`      → `◆ v0.6.0`
///   `main` (branch)    → `▸ main`
pub fn format_ref_badge(raw: &str) -> String {
    if raw.starts_with("HEAD -> ") {
        format!("★ {}", raw)
    } else if raw == "HEAD" {
        "★ HEAD (detached)".to_string()
    } else if let Some(name) = raw.strip_prefix("tag: ") {
        format!("◆ {}", name)
    } else {
        format!("▸ {}", raw)
    }
}

/// Color hint for a ref badge (ANSI 256). Brand-consistent: HEAD bright,
/// tags gold, branches cyan. Currently used only by the TUI badge renderer
/// (planned); CLI prints unstyled badges.
#[allow(dead_code)]
pub fn ref_badge_color(raw: &str) -> u8 {
    if raw.starts_with("HEAD") {
        199 // hot pink
    } else if raw.starts_with("tag: ") {
        220 // gold
    } else {
        51 // turquoise
    }
}

// ============================================================================
// git2 integration
// ============================================================================

/// One commit with everything a TUI row needs: graph topology + decorations.
#[derive(Debug, Clone)]
#[allow(dead_code)] // author + timestamp consumed by callers we'll add later
pub struct DecoratedCommit {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub timestamp: i64,
    pub parents: Vec<String>,
    /// Refs that point at this commit, formatted: ["HEAD -> main", "tag: v0.6.0"].
    pub refs: Vec<String>,
}

/// Walk repository commits topologically and decorate with refs/tags.
///
/// `include_all` = true → include every local branch + tag tip as a starting
/// point (mimics `git log --all`). False → only HEAD.
pub fn walk_repo(
    repo: &git2::Repository,
    limit: usize,
    include_all: bool,
) -> Result<Vec<DecoratedCommit>, git2::Error> {
    use std::collections::HashMap;

    // Build oid → labels map by scanning refs once.
    let mut labels: HashMap<git2::Oid, Vec<String>> = HashMap::new();
    let head_oid = repo.head().ok().and_then(|h| h.target());
    let head_name = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    for r in repo.references()?.flatten() {
        let Some(oid) = r.target() else { continue };
        let Some(name) = r.name() else { continue };
        let label = if let Some(short) = name.strip_prefix("refs/heads/") {
            if Some(oid) == head_oid && head_name.as_deref() == Some(short) {
                format!("HEAD -> {}", short)
            } else {
                short.to_string()
            }
        } else if let Some(short) = name.strip_prefix("refs/tags/") {
            format!("tag: {}", short)
        } else if let Some(short) = name.strip_prefix("refs/remotes/") {
            // Skip remotes for now (saturate). Caller can extend later.
            let _ = short;
            continue;
        } else {
            continue;
        };
        labels.entry(oid).or_default().push(label);
    }

    // Detached HEAD case: ensure HEAD label appears.
    if let (Some(oid), Some(_)) = (head_oid, head_name.as_ref()) {
        let entry = labels.entry(oid).or_default();
        if !entry.iter().any(|s| s.starts_with("HEAD")) {
            entry.insert(0, "HEAD".to_string());
        }
    }

    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

    if include_all {
        for r in repo.references()?.flatten() {
            let Some(name) = r.name() else { continue };
            if name.starts_with("refs/heads/") || name.starts_with("refs/tags/") {
                if let Some(oid) = r.target() {
                    let _ = walk.push(oid);
                }
            }
        }
    } else {
        walk.push_head()?;
    }

    let mut out = Vec::with_capacity(limit);
    for oid_res in walk.take(limit) {
        let oid = oid_res?;
        let commit = repo.find_commit(oid)?;
        let id = oid.to_string();
        let short_id = id.chars().take(7).collect();
        let summary = commit.summary().unwrap_or("").to_string();
        let author = commit
            .author()
            .name()
            .unwrap_or("")
            .to_string();
        let timestamp = commit.time().seconds();
        let parents: Vec<String> = commit.parent_ids().map(|p| p.to_string()).collect();
        let refs = labels.remove(&oid).unwrap_or_default();
        out.push(DecoratedCommit {
            id,
            short_id,
            summary,
            author,
            timestamp,
            parents,
            refs,
        });
    }
    Ok(out)
}

/// Convenience: walk + render with default ASCII style.
#[allow(dead_code)]
pub fn render_repo(
    repo: &git2::Repository,
    limit: usize,
    include_all: bool,
) -> Result<Vec<(DecoratedCommit, GraphRow)>, git2::Error> {
    render_repo_with(repo, limit, include_all, GraphStyle::Ascii)
}

/// Walk + render with a chosen glyph style.
pub fn render_repo_with(
    repo: &git2::Repository,
    limit: usize,
    include_all: bool,
    style: GraphStyle,
) -> Result<Vec<(DecoratedCommit, GraphRow)>, git2::Error> {
    let commits = walk_repo(repo, limit, include_all)?;
    let graph_input: Vec<GraphCommit> = commits
        .iter()
        .map(|c| GraphCommit {
            id: c.id.clone(),
            parents: c.parents.clone(),
        })
        .collect();
    let rows = render_with(&graph_input, style);
    Ok(commits.into_iter().zip(rows.into_iter()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parents: &[&str]) -> GraphCommit {
        GraphCommit {
            id: id.to_string(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn linear_history() {
        let commits = vec![c("c", &["b"]), c("b", &["a"]), c("a", &[])];
        let rows = render(&commits);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].commit_line, "*");
        assert_eq!(rows[1].commit_line, "*");
        assert_eq!(rows[2].commit_line, "*");
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[2].parent_count, 0);
    }

    #[test]
    fn simple_merge() {
        // d is a merge of b and c; both have parent a.
        //   *   d (b, c)
        //   |\
        //   | * c
        //   * | b
        //   |/
        //   * a
        let commits = vec![
            c("d", &["b", "c"]),
            c("b", &["a"]),
            c("c", &["a"]),
            c("a", &[]),
        ];
        let rows = render(&commits);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].parent_count, 2);
        // d sits on lane 0, opens lane 1 for parent c.
        assert_eq!(rows[0].lane, 0);
        assert!(rows[0].transition_line.contains('\\'));
        // a closes both lanes — last commit, no parents.
        assert_eq!(rows[3].parent_count, 0);
    }

    #[test]
    fn fork_then_close() {
        // c branches off from a:
        //   * c (a)
        //   | * b (a)
        //   |/
        //   * a
        let commits = vec![c("c", &["a"]), c("b", &["a"]), c("a", &[])];
        let rows = render(&commits);
        assert_eq!(rows.len(), 3);
        // After c, lane 0 expects a. b appears, gets new lane (1).
        // When a appears, it occupies lane 0; lane 1 closes with '/'.
        assert_eq!(rows[0].commit_line, "*");
        assert!(rows[1].commit_line.contains('*'));
    }

    #[test]
    fn lane_color_stable() {
        assert_eq!(lane_color(0), lane_color(0));
        assert_ne!(lane_color(0), lane_color(1));
    }

    #[test]
    fn empty_input() {
        assert_eq!(render(&[]), vec![]);
    }
}
