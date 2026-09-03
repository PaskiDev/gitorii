//! Where things are on screen, so a click can find them.
//!
//! The TUI draws in immediate mode: nothing survives a frame, so nothing knows
//! where it ended up. A pointer needs the opposite — a map from a cell to the
//! thing drawn there. So the render registers what it draws, and the map is
//! rebuilt every frame; it can never be stale, because it never outlives the
//! picture it describes.
//!
//! Zones are named by string rather than by an enum of every list in the
//! program: a new view registers its rows without touching this file, and a
//! view that forgets simply has no pointer support instead of a wrong one.

use ratatui::layout::Rect;

/// What a cell belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zone {
    /// A row of the sidebar, by index.
    Sidebar(usize),
    /// A row of a named list — `("log", 12)`, `("safety.rules", 3)`.
    Row { list: String, index: usize },
    /// A tab of a named strip — `("config", 1)`.
    Tab { strip: String, index: usize },
    /// A key of the hint line, carrying the key as written: `a`, `Enter`.
    Key(String),
}

/// The zones of one frame, in the order they were registered.
///
/// Later registrations win, which is what makes popups work: an overlay is
/// drawn last, so its rows sit on top of whatever they cover.
#[derive(Debug, Default)]
pub struct HitMap {
    zones: Vec<(Rect, Zone)>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.zones.clear();
    }

    pub fn push(&mut self, area: Rect, zone: Zone) {
        if area.width > 0 && area.height > 0 {
            self.zones.push((area, zone));
        }
    }

    /// Register one row per line of `area`, starting at `first` — the index of
    /// the item drawn on the first visible line, which is not 0 once a list
    /// has scrolled.
    pub fn rows(&mut self, area: Rect, list: &str, first: usize, count: usize) {
        for line in 0..area.height {
            let index = first + line as usize;
            if index >= first + count {
                break;
            }
            self.push(
                Rect::new(area.x, area.y + line, area.width, 1),
                Zone::Row {
                    list: list.to_string(),
                    index,
                },
            );
        }
    }

    /// What is at `(x, y)`, or nothing.
    pub fn at(&self, x: u16, y: u16) -> Option<&Zone> {
        self.zones
            .iter()
            .rev()
            .find(|(area, _)| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
            .map(|(_, zone)| zone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_finds_the_zone_drawn_there() {
        let mut map = HitMap::default();
        map.push(Rect::new(0, 0, 10, 3), Zone::Sidebar(0));

        assert_eq!(map.at(5, 1), Some(&Zone::Sidebar(0)));
        assert_eq!(map.at(10, 1), None, "one past the right edge");
        assert_eq!(map.at(5, 3), None, "one past the bottom");
    }

    /// A popup is drawn last, so it must win the cells it covers — otherwise
    /// a click on a dropdown would reach the list behind it.
    #[test]
    fn what_was_drawn_last_wins() {
        let mut map = HitMap::default();
        map.push(Rect::new(0, 0, 20, 10), Zone::Sidebar(0));
        map.push(
            Rect::new(2, 2, 6, 2),
            Zone::Row {
                list: "popup".into(),
                index: 1,
            },
        );

        assert_eq!(
            map.at(3, 3),
            Some(&Zone::Row {
                list: "popup".into(),
                index: 1
            })
        );
        assert_eq!(map.at(15, 3), Some(&Zone::Sidebar(0)));
    }

    /// Rows are registered from the first *visible* item: a list scrolled by
    /// twenty would otherwise hand back the index of the wrong commit.
    #[test]
    fn rows_count_from_the_first_visible_item() {
        let mut map = HitMap::default();
        map.rows(Rect::new(0, 5, 30, 3), "log", 20, 100);

        assert_eq!(
            map.at(1, 5),
            Some(&Zone::Row {
                list: "log".into(),
                index: 20
            })
        );
        assert_eq!(
            map.at(1, 7),
            Some(&Zone::Row {
                list: "log".into(),
                index: 22
            })
        );
    }

    /// A list shorter than its pane registers nothing below its last item, so
    /// clicking the empty space under it does not select a row that is not
    /// there.
    #[test]
    fn empty_space_below_a_short_list_is_not_a_row() {
        let mut map = HitMap::default();
        map.rows(Rect::new(0, 0, 20, 10), "tags", 0, 2);

        assert!(map.at(1, 1).is_some());
        assert_eq!(map.at(1, 5), None);
    }
}
