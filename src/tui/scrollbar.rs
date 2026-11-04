//! The scroll bar: on the right edge of the place the cursor is in
//! (`App::barred`), while what it holds does not fit, more of it above or
//! below what it shows, a column a character wide, as high as the place's
//! rows that scroll. Its track is a light line, `│`, as the borders are, and
//! on it the thumb is a block, `█`, both in the colour of the place's
//! border, so that the thumb stands out by its shape alone. The thumb is as
//! long as the share of what the place holds that it shows, a row at least,
//! and a row short of the track; it is where the view is, at the track's
//! top only while the view is at the first line, at its bottom only once it
//! is at the last, as far as the track has the rows for that. A place with
//! a border on its right draws the bar in it, and loses no width: a box, and
//! a pane with a line between it and the pane beyond. A pane on the right
//! edge of the screen draws it in its own last column, which the pane then
//! leaves to the bar: its lines cut at its edge are cut a column sooner, and
//! those folded a column short of it, as the code is, leave that column
//! blank already. The places the cursor is not in draw nothing new, nor does one
//! that fits. Each draw says where its place is and what it shows
//! (`scrollbar`); the bar is drawn last, over what is there, so that it
//! follows the view however it moves: by the keys, by the wheel, by a
//! search jumping.

use super::*;

/// The track of the scroll bar, and its thumb.
const TRACK: &str = "│";
const THUMB: &str = "█";

/// What a place shows of what it holds, in its lines, or in its items when
/// each is as high as the others: the first it shows, how many it has room
/// for, and how many it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Shown {
    pub top: usize,
    pub rows: usize,
    pub all: usize,
}

impl Shown {
    /// Whether it holds more than it shows: lines above the first it shows,
    /// or below the last.
    pub fn more(self) -> bool {
        self.top > 0 || self.top + self.rows < self.all
    }

    /// The thumb on a track `h` rows high: its first row, and how many rows
    /// it takes. As long as the share of the lines shown, rounded, a row at
    /// least and a row short of the track, which then says that there is
    /// more; at the track's top while the view is at the first line, at its
    /// bottom once the view is at the last (its last line shown, or the
    /// view past it), and else where the view is between, a row from either
    /// end where the track has the room.
    pub fn thumb(self, h: usize) -> (usize, usize) {
        let all = self.all.max(1);
        let seen = self.rows.min(self.all.saturating_sub(self.top));
        let len = ((2 * h * seen + all) / (2 * all)).clamp(1, h.saturating_sub(1).max(1));
        let room = h.saturating_sub(len);
        let last = self.all.saturating_sub(self.rows);
        let at = match self.top {
            0 => 0,
            top if top >= last => room,
            top => {
                let at = (2 * room * top + last) / (2 * last);
                if room >= 2 { at.clamp(1, room - 1) } else { at }
            }
        };
        (at, len)
    }
}

/// The column of the scroll bar of a pane at `area` of `screen`, and
/// whether it is a border: the line on the pane's right, between it and the
/// pane beyond, where the pane does not reach the right edge of the screen,
/// as every pane that does not has such a line; else the pane's own last
/// column.
fn bar_column(area: Rect, screen: Rect) -> (u16, bool) {
    match area.right() < screen.right() {
        true => (area.right(), true),
        false => (area.right().saturating_sub(1), false),
    }
}

/// A scroll bar to draw, as a place's draw said it (`App::scrollbar`): its
/// column on the screen, its first row and how many rows it has, whether
/// that column is the pane's own last rather than a border, what the place
/// shows, and the colour of the place's border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Bar {
    x: u16,
    y: u16,
    h: u16,
    own: bool,
    shown: Shown,
    st: Style,
}

impl App {
    /// The place the cursor is in, the one that draws its scroll bar while
    /// what it holds does not fit (`scrollbar`): a box open over the screen,
    /// as the wheel scrolls it alone (a menu, what `?` says, a reference's
    /// preview, the comment being written); the agent's stream read in
    /// place; the patches, while a patch or HEAD is moved there. Else, on
    /// the main screen, the banner while the cursor is on it, or the pane
    /// the cursor is in, of the middle split in two the half it is in; on a
    /// screen of its own, the part of it the cursor is in. The search of
    /// the middle puts the cursor in the middle. None in a dialog (the ask
    /// dialog, the views', the one where accept is typed), nor while a line
    /// is typed in a box: none of them scrolls.
    pub(super) fn barred(&self) -> Option<Wheeled> {
        match &self.mode {
            Mode::Menu(_) => return Some(Wheeled::Menu),
            Mode::Help { .. } => return Some(Wheeled::Help),
            Mode::Preview(_) => return Some(Wheeled::Preview),
            Mode::Edit(_) => return Some(Wheeled::Editor),
            Mode::Stream { .. } => return Some(Wheeled::Stream),
            Mode::Moving { .. } | Mode::MovingHead { .. } => return Some(Wheeled::Patches),
            Mode::Ask(_) | Mode::Views(_) | Mode::Protected(_) | Mode::Lines(_) | Mode::Prompt(_) => return None,
            Mode::Normal | Mode::Find(_) => {}
        }
        Some(match self.screen {
            Screen::Main if self.on_banner => Wheeled::Banner,
            Screen::Main => match self.focus {
                Focus::List => Wheeled::Patches,
                Focus::Diff => Wheeled::Middle,
                Focus::Notes => Wheeled::Notes,
            },
            Screen::History if self.hist_focus == HistFocus::Diff => Wheeled::HistDiff,
            Screen::Tree | Screen::Stacks | Screen::Versions | Screen::Runs | Screen::History => Wheeled::Picks,
            Screen::Text => Wheeled::Text,
            Screen::Transcript => Wheeled::Transcript,
            Screen::Review if self.reviewing.as_ref().is_some_and(|r| r.in_main) => Wheeled::ReviewMain,
            Screen::Review => Wheeled::ReviewList,
            Screen::Search => Wheeled::Found,
            #[cfg(feature = "moz-phab")]
            Screen::Submit => Wheeled::Submit,
            Screen::HeadWhy if self.head_why.as_ref().is_some_and(|v| v.in_main) => Wheeled::WhyMain,
            Screen::HeadWhy => Wheeled::WhyList,
        })
    }

    /// `place`'s scroll bar, in column `x` of the screen from row `y`, `h`
    /// rows, that column a pane's own last (`own`) or a border, for what the
    /// place shows, `s`, in `st`: kept, to be drawn last (`draw_scrollbar`),
    /// when `place` is the one the cursor is in and holds more than it
    /// shows. Whether it is.
    pub(super) fn scrollbar(&self, place: Wheeled, (x, y, h): (u16, u16, usize), own: bool, s: Shown, st: Style) -> bool {
        if h == 0 || !s.more() || self.barred() != Some(place) {
            return false;
        }
        self.bar.set(Some(Bar { x, y, h: h as u16, own, shown: s, st }));
        true
    }

    /// A pane's scroll bar (`scrollbar`), beside its rows `y` to `y + h` of
    /// `area`, in its column (`bar_column`), in the colour of the panes'
    /// borders. The width the pane has left for what it holds: a column
    /// less while the bar is in its last.
    pub(super) fn pane_bar(&self, place: Wheeled, area: Rect, screen: Rect, (y, h): (usize, usize), s: Shown) -> usize {
        let w = area.width as usize;
        let (x, border) = bar_column(area, screen);
        match self.scrollbar(place, (x, area.y + y as u16, h), !border, s, color::meta()) {
            true if !border => w.saturating_sub(1),
            _ => w,
        }
    }

    /// The scroll bar of the agent's stream read in place, beside its lines
    /// as the middle at `area` drew them (`drawn`, each row with the line
    /// it is on), in the middle's column for it (`bar_column`): the lines of
    /// the stream from the first it shows, as many as it has room for, of
    /// all of them, as the rows of its comment cut it (`deco`).
    pub(super) fn stream_bar(&self, area: Rect, screen: Rect, drawn: &[(usize, usize, usize)]) {
        let Mode::Stream { change, id, up } = &self.mode else { return };
        let mut ys = drawn.iter().filter(|d| self.rows[d.0].stream && self.rows[d.0].note.as_ref() == Some(id)).map(|d| d.1);
        let Some(first) = ys.next() else { return };
        let last = ys.fold(first, usize::max);
        let all = self.busy().get(&(change.clone(), id.clone())).map_or(0, |(_, run)| ask::tail(&self.db, run, usize::MAX).len());
        let room = (self.body_h / 2).max(5);
        let end = all.saturating_sub(*up);
        let top = end.saturating_sub(room);
        let (x, border) = bar_column(area, screen);
        self.scrollbar(Wheeled::Stream, (x, area.y + first as u16, last + 1 - first), !border, Shown { top, rows: end - top, all }, color::meta());
    }

    /// A box's scroll bar (`scrollbar`), in the box's right border, beside
    /// its `h` rows that scroll from the first inside it: the box at `(x,
    /// y)` of `area`, `w` wide, as much of it as `area` holds, its border in
    /// `st`.
    pub(super) fn box_bar(&self, place: Wheeled, area: Rect, (x, y, w): (usize, usize, usize), h: usize, s: Shown, st: Style) {
        let w = w.min((area.width as usize).saturating_sub(x));
        let h = h.min((area.height as usize).saturating_sub(y + 1));
        if w > 0 {
            self.scrollbar(place, (area.x + (x + w - 1) as u16, area.y + (y + 1) as u16, h), false, s, st);
        }
    }

    /// The scroll bar kept as the screen was drawn (`scrollbar`), drawn over
    /// what is there: its track, and on it its thumb (`Shown::thumb`), on
    /// the terminal's own background, as the borders are. In a pane's own
    /// last column, that column is then no longer part of the pane, for the
    /// text selected there in StackED to keep to what the pane holds.
    pub(super) fn draw_scrollbar(&self, buf: &mut Buffer) {
        let Some(b) = self.bar.get() else { return };
        let (at, len) = b.shown.thumb(b.h as usize);
        for k in 0..b.h {
            let (x, y) = (b.x, b.y + k);
            if !buf.area.contains(ratatui::layout::Position::new(x, y)) {
                continue;
            }
            let thumb = (at..at + len).contains(&(k as usize));
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_symbol(if thumb { THUMB } else { TRACK }).set_style(b.st);
        }
        if !b.own {
            return;
        }
        for r in self.areas.borrow_mut().iter_mut() {
            if r.right() == b.x + 1 && r.contains(ratatui::layout::Position::new(b.x, b.y)) {
                r.width -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thumb of a track `h` rows high, for a view of `rows` lines from
    /// line `top` of `all`.
    fn thumb(h: usize, top: usize, rows: usize, all: usize) -> (usize, usize) {
        Shown { top, rows, all }.thumb(h)
    }

    #[test]
    fn the_thumb_is_the_share_shown_and_where_the_view_is() {
        // A tenth of it shown: a row of ten, at the top at the first line, at
        // the bottom at the last, and halfway halfway.
        assert_eq!(thumb(10, 0, 10, 100), (0, 1));
        assert_eq!(thumb(10, 90, 10, 100), (9, 1));
        assert_eq!(thumb(10, 45, 10, 100), (5, 1));
        // Half of it: five rows, from the top, the middle or the bottom.
        assert_eq!(thumb(10, 0, 10, 20), (0, 5));
        assert_eq!(thumb(10, 5, 10, 20), (3, 5));
        assert_eq!(thumb(10, 10, 10, 20), (5, 5));
        // A line from either end: a row from either end of the track, never
        // at it, as the view is not.
        assert_eq!(thumb(10, 1, 10, 100), (1, 1));
        assert_eq!(thumb(10, 89, 10, 100), (8, 1));
        // A share too small for a row: a row all the same.
        assert_eq!(thumb(10, 0, 10, 10_000), (0, 1));
        // All of it but a line: a row short of the track, which says that
        // there is more; on a track of a row, that row.
        assert_eq!(thumb(10, 0, 10, 11), (0, 9));
        assert_eq!(thumb(10, 1, 10, 11), (1, 9));
        assert_eq!(thumb(1, 3, 1, 5), (0, 1));
        // A view past its last line, as a text scrolled until its last line
        // is the first shown: at the bottom, as long as what it shows, a
        // quarter of it.
        assert_eq!(thumb(10, 15, 10, 20), (7, 3));
    }

    #[test]
    fn a_place_holds_more_than_it_shows_with_lines_above_or_below() {
        assert!(!Shown { top: 0, rows: 10, all: 10 }.more());
        assert!(!Shown { top: 0, rows: 10, all: 3 }.more());
        assert!(Shown { top: 0, rows: 10, all: 11 }.more());
        assert!(Shown { top: 1, rows: 10, all: 11 }.more());
        assert!(Shown { top: 2, rows: 10, all: 3 }.more(), "scrolled past its first line, though it would fit");
    }
}
