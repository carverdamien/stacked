//! The compact layout, after self-review's (copied, not shared): a patch's
//! old side and new side in panes of their own, each its lines one after the
//! other with no gap where the other side has more, scrolled together or one
//! alone. A patch is one region: the panes scroll over all of it.

use std::collections::HashMap;

use crate::rows::{Kind, Row};

/// Which pane the arrows scroll: the old side, both, or the new side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scroll {
    Left,
    #[default]
    Both,
    Right,
}

impl Scroll {
    pub fn name(self) -> &'static str {
        match self {
            Scroll::Left => "scrolling the old side",
            Scroll::Both => "scrolling both",
            Scroll::Right => "scrolling the new side",
        }
    }

    /// For the top bar.
    pub fn mark(self) -> &'static str {
        match self {
            Scroll::Left => "<",
            Scroll::Both => "<>",
            Scroll::Right => ">",
        }
    }
}

/// The rows each pane draws, in order, and where each row is in them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Panes {
    pub left: Vec<usize>,
    pub right: Vec<usize>,
    pub lpos: HashMap<usize, usize>,
    pub rpos: HashMap<usize, usize>,
}

/// A row both panes draw, across the width: not a line of one side.
pub fn spans(kind: Kind) -> bool {
    !matches!(kind, Kind::Add | Kind::Del | Kind::Ctx)
}

/// Splits rows into the old-side and new-side streams the panes draw. A
/// comment, and claude's stream under it, goes with the line it is on.
pub fn pane_streams(rows: &[Row]) -> Panes {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Left,
        Right,
        Both,
    }
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut owner = S::Both;
    for (i, row) in rows.iter().enumerate() {
        let side = if matches!(row.kind, Kind::Comment | Kind::Stream) {
            owner
        } else {
            owner = match row.kind {
                Kind::Del => S::Left,
                Kind::Add => S::Right,
                _ => S::Both,
            };
            owner
        };
        if side != S::Right {
            left.push(i);
        }
        if side != S::Left {
            right.push(i);
        }
    }
    let lpos = left.iter().enumerate().map(|(n, &r)| (r, n)).collect();
    let rpos = right.iter().enumerate().map(|(n, &r)| (r, n)).collect();
    Panes {
        left,
        right,
        lpos,
        rpos,
    }
}

/// Where the compact panes are: the first row each shows, the screen line
/// the cursor is on, and which pane the arrows scroll.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compact {
    pub top: [usize; 2],
    pub y: usize,
    pub scroll: Scroll,
    /// The row the cursor was last on: when the cursor is on another, it
    /// was moved by row (to a comment), and the panes go to it.
    pub cursor: Option<usize>,
    /// The cursor was moved by row onto `cursor`: it stays on it, whichever
    /// pane it is in, until it moves by line.
    held: bool,
}

impl Compact {
    /// The pane the cursor is in: the old side's when it alone scrolls,
    /// else the new side's.
    pub fn side(&self) -> usize {
        if self.scroll == Scroll::Left { 0 } else { 1 }
    }

    fn stream(p: &Panes, which: usize) -> &Vec<usize> {
        if which == 0 { &p.left } else { &p.right }
    }

    fn pos(p: &Panes, which: usize, row: usize) -> Option<usize> {
        if which == 0 { p.lpos.get(&row) } else { p.rpos.get(&row) }.copied()
    }

    /// How far down a pane can scroll: its last row at the bottom.
    fn limit(p: &Panes, which: usize, h: usize) -> usize {
        Self::stream(p, which).len().saturating_sub(h.max(1))
    }

    /// The screen lines: a row from each pane per line.
    pub fn lines(&self, p: &Panes, h: usize) -> Vec<(Option<usize>, Option<usize>)> {
        let [li, ri] = self.top;
        (0..h)
            .map(|k| (p.left.get(li + k).copied(), p.right.get(ri + k).copied()))
            .collect()
    }

    /// Moves whichever panes the scroll holds, by `delta` lines, none past
    /// its end. Whether anything moved.
    pub fn scroll_by(&mut self, p: &Panes, delta: isize, h: usize) -> bool {
        self.held = false;
        let mut moved = false;
        for _ in 0..delta.unsigned_abs() {
            if p.left.is_empty() && p.right.is_empty() {
                break;
            }
            let [li, ri] = self.top;
            let (l_stuck, r_stuck) = if delta > 0 {
                (li >= Self::limit(p, 0, h), ri >= Self::limit(p, 1, h))
            } else {
                (li == 0, ri == 0)
            };
            let mv = |v: usize| if delta > 0 { v + 1 } else { v - 1 };
            let next = match self.scroll {
                Scroll::Both => [if l_stuck { li } else { mv(li) }, if r_stuck { ri } else { mv(ri) }],
                Scroll::Left if !l_stuck => [mv(li), ri],
                Scroll::Right if !r_stuck => [li, mv(ri)],
                _ => [li, ri],
            };
            if next == self.top {
                break;
            }
            self.top = next;
            moved = true;
        }
        moved
    }

    /// The cursor `delta` lines down (up), the panes scrolling when it
    /// would leave the screen; never onto a line with nothing on it.
    pub fn step(&mut self, p: &Panes, delta: isize, h: usize) {
        self.held = false;
        let h = h.max(1);
        let mut y = self.y as isize + delta;
        if y < 0 {
            self.scroll_by(p, y, h);
            y = 0;
        } else if y >= h as isize {
            self.scroll_by(p, y - h as isize + 1, h);
            y = h as isize - 1;
        }
        let lines = self.lines(p, h);
        let last = lines.iter().rposition(|(l, r)| l.is_some() || r.is_some()).unwrap_or(0);
        self.y = (y as usize).min(last);
    }

    /// The row under the cursor: the one it was moved onto, else the
    /// cursor's pane's, else the other's.
    pub fn under(&self, p: &Panes, h: usize) -> Option<usize> {
        let lines = self.lines(p, h);
        let (l, r) = *lines.get(self.y.min(lines.len().saturating_sub(1)))?;
        if self.held && self.cursor.is_some() && (l == self.cursor || r == self.cursor) {
            return self.cursor;
        }
        if self.side() == 0 { l.or(r) } else { r.or(l) }
    }

    /// The row under the cursor, kept as the one the cursor is on.
    pub fn settle(&mut self, p: &Panes, h: usize) -> Option<usize> {
        let under = self.under(p, h)?;
        self.cursor = Some(under);
        Some(under)
    }

    /// Puts the panes on `row` when the cursor was moved by row: the row
    /// a third down the screen, the other pane lined up on the nearest row
    /// both show.
    pub fn resync(&mut self, p: &Panes, row: usize, h: usize) {
        if self.cursor == Some(row) {
            return;
        }
        self.place(p, row, h, None);
    }

    /// Puts the panes on `row` when the cursor was moved on to it, on the
    /// cursor's line `y`: the panes scroll up to it, past their ends if it
    /// takes that, as the other views' rows do; the other pane lined up on
    /// the nearest row both show.
    pub fn keep(&mut self, p: &Panes, row: usize, y: usize, h: usize) {
        self.place(p, row, h, Some(y));
    }

    /// The panes on `row`, the cursor on it: on line `y`, else where it is
    /// if its pane shows it, or the other does, else a third down.
    fn place(&mut self, p: &Panes, row: usize, h: usize, y: Option<usize>) {
        let h = h.max(1);
        let mut mine = self.side();
        let pos = match Self::pos(p, mine, row) {
            Some(pos) => pos,
            None => {
                mine = 1 - mine;
                match Self::pos(p, mine, row) {
                    Some(pos) => pos,
                    None => return,
                }
            }
        };
        self.cursor = Some(row);
        self.held = true;
        // On screen already, in its pane, else in the other: the cursor
        // goes to it there, the panes as they are, as the mouse over a row
        // the wheel scrolled into sight puts the cursor on it.
        if y.is_none() {
            let lines = self.lines(p, h);
            for which in [mine, 1 - mine] {
                for (y, (l, r)) in lines.iter().enumerate() {
                    if (if which == 0 { *l } else { *r }) == Some(row) {
                        self.y = y;
                        return;
                    }
                }
            }
        }
        let other = 1 - mine;
        // On line `y`, as far as that takes the panes; else no further than
        // their ends.
        let limit = |which: usize| if y.is_some() { usize::MAX } else { Self::limit(p, which, h) };
        let mut top = [0; 2];
        top[mine] = pos.saturating_sub(y.unwrap_or(h / 3).min(h - 1)).min(limit(mine));
        let shared = (0..=row).rev().find(|r| p.lpos.contains_key(r) && p.rpos.contains_key(r));
        top[other] = match shared {
            Some(s) => {
                let at = Self::pos(p, mine, s).unwrap_or(0) as isize - top[mine] as isize;
                (Self::pos(p, other, s).unwrap_or(0) as isize - at).max(0) as usize
            }
            None => top[mine],
        }
        .min(limit(other));
        self.top = top;
        self.y = pos - top[mine];
    }

    /// Back to the top, for another patch: the scroll stays.
    pub fn reset(&mut self) {
        *self = Compact {
            scroll: self.scroll,
            ..Default::default()
        };
    }

    pub fn to_start(&mut self) {
        self.held = false;
        self.top = [0, 0];
        self.y = 0;
    }

    pub fn to_end(&mut self, p: &Panes, h: usize) {
        self.top = [Self::limit(p, 0, h), Self::limit(p, 1, h)];
        self.y = h;
        self.step(p, 0, h);
    }

    /// ← and →: the scroll one way, Left, Both, Right. False at the end:
    /// the arrow goes on, to the pane beyond.
    pub fn turn(&mut self, right: bool) -> bool {
        self.held = false;
        self.scroll = match (self.scroll, right) {
            (Scroll::Left, true) => Scroll::Both,
            (Scroll::Both, true) => Scroll::Right,
            (Scroll::Right, false) => Scroll::Both,
            (Scroll::Both, false) => Scroll::Left,
            _ => return false,
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(kinds: &[Kind]) -> Vec<Row> {
        kinds
            .iter()
            .map(|&kind| Row {
                kind,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn sides_go_their_own_ways_and_comments_with_their_line() {
        use Kind::*;
        let r = rows(&[Hunk, Ctx, Del, Comment, Add, Add, Comment, Ctx]);
        let p = pane_streams(&r);
        assert_eq!(p.left, [0, 1, 2, 3, 7]);
        assert_eq!(p.right, [0, 1, 4, 5, 6, 7]);
    }

    #[test]
    fn panes_scroll_together_or_alone_and_stop_at_their_ends() {
        use Kind::*;
        // The new side is longer: four lines added.
        let r = rows(&[Hunk, Del, Add, Add, Add, Add, Ctx]);
        let p = pane_streams(&r);
        let mut c = Compact::default();
        let h = 3;
        c.step(&p, 5, h);
        assert_eq!(c.y, 2);
        // The old side (Hunk, Del, Ctx) fits: only the new one scrolls.
        assert_eq!(c.top, [0, 3]);
        c.to_start();
        c.scroll = Scroll::Left;
        assert!(!c.scroll_by(&p, 1, h), "the old side is all on screen");
        c.scroll = Scroll::Right;
        assert!(c.scroll_by(&p, 1, h));
        assert_eq!(c.top, [0, 1]);
        assert!(c.turn(false) && c.scroll == Scroll::Both);
        assert!(c.turn(false) && c.scroll == Scroll::Left);
        assert!(!c.turn(false), "further left: the pane beyond");
    }

    #[test]
    fn a_row_moved_to_is_lined_up_on_both_sides() {
        use Kind::*;
        // 0 the hunk, 1-10 context, 11-30 added, 31 context, 32 removed,
        // 33-42 context.
        let mut kinds = vec![Hunk];
        kinds.extend([Ctx; 10]);
        kinds.extend([Add; 20]);
        kinds.extend([Ctx, Del]);
        kinds.extend([Ctx; 10]);
        let p = pane_streams(&rows(&kinds));
        let mut c = Compact::default();
        let h = 6;
        let (ctx, del) = (31, 32);
        c.resync(&p, del, h);
        let lines = c.lines(&p, h);
        // The context line above it is on the same screen line both sides.
        let y = lines.iter().position(|(l, _)| *l == Some(ctx)).unwrap();
        assert_eq!(lines[y].1, Some(ctx), "{lines:?}");
        // The cursor is on the removed line, though the new side's pane is
        // the one that scrolls.
        assert_eq!(c.under(&p, h), Some(del));
        c.step(&p, 1, h);
        assert_ne!(c.settle(&p, h), Some(del), "moved by line, it leaves it");
        // On screen already, the panes stay where they are.
        let top = c.top;
        c.resync(&p, del, h);
        assert_eq!((c.top, c.under(&p, h)), (top, Some(del)));
    }
}
