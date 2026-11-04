//! The wheel, in the browser and in StackED, which send it with the cell
//! under the mouse and the lines it turned (`App::wheel`); a terminal sends
//! none. While the mouse is in use, from a movement of it or a click until
//! the next key, the wheel scrolls the pane or the box drawn under the
//! mouse: its view moves by the lines turned, and stops at its first and
//! its last lines, as the arrows stop it. Where the cursor follows the
//! mouse, the cursor goes on the line then under the mouse, as the mouse
//! over that line puts it; a side pane, where the mouse only marks the line
//! under it, keeps what it picked, in sight or not, and the line then under
//! the mouse is marked. Each place stays where the wheel left it until the
//! next key, which brings what it picked back into sight. While the mouse
//! is not in use, the wheel is ↑↓ where the cursor is, as it ever was,
//! until the mouse moves again.

use super::*;

/// How far the wheel turned: in lines, or in pages, as StackED's wheel
/// turns in pages too; down, or up below 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turned {
    Lines(i32),
    Pages(i32),
}

/// A place the wheel scrolls, by where it was drawn (`App::scrolls_in`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Wheeled {
    /// The banner, open.
    Banner,
    Patches,
    /// The middle, or the half of it split in two that the cursor is in;
    /// and the other half.
    Middle,
    Beside,
    Notes,
    /// The stream of the comment the agent works on, read in place: over
    /// the middle, while it is open.
    Stream,
    /// The list of the tree, the stacks, the runs, the versions or the
    /// history.
    Picks,
    /// History…'s stack diff.
    HistDiff,
    /// A text, or interdiffs, on a screen of their own.
    Text,
    Transcript,
    /// The review's list, and its pane on the right.
    ReviewList,
    ReviewMain,
    /// What the search found.
    Found,
    /// What a submit to Phabricator sends, for your review.
    #[cfg(feature = "moz-phab")]
    Submit,
    /// The view of why Move HEAD does not move HEAD: its list, and its pane
    /// on the right.
    WhyList,
    WhyMain,
    Menu,
    /// What `?` says.
    Help,
    /// A reference's preview.
    Preview,
    /// The box of the comment being written.
    Editor,
}

impl Wheeled {
    /// What stays where the wheel left it once the wheel scrolled this
    /// place (`App::free`): a place with a pick, or a cursor, that its
    /// draw keeps in sight; the middle, for either half of it. None for a
    /// place with neither, for the box of the comment being written, which
    /// keeps its cursor in it, and for a menu, which keeps its own.
    fn stays(self) -> Option<Wheeled> {
        match self {
            Wheeled::Beside => Some(Wheeled::Middle),
            Wheeled::Banner
            | Wheeled::Patches
            | Wheeled::Middle
            | Wheeled::Notes
            | Wheeled::Picks
            | Wheeled::HistDiff
            | Wheeled::ReviewList
            | Wheeled::ReviewMain
            | Wheeled::Found
            | Wheeled::WhyList => Some(self),
            #[cfg(feature = "moz-phab")]
            Wheeled::Submit => Some(self),
            Wheeled::Stream | Wheeled::Text | Wheeled::Transcript | Wheeled::WhyMain | Wheeled::Menu | Wheeled::Help | Wheeled::Preview | Wheeled::Editor => None,
        }
    }
}

/// `top` moved `n` lines down (up, below 0): from 0 to `last` at most.
pub(super) fn moved(top: usize, n: isize, last: usize) -> usize {
    top.saturating_add_signed(n).min(last)
}

impl App {
    /// The wheel turned `turned` over the cell `at` of the screen, if it is
    /// over one. While the mouse is in use (`mouse`), the place drawn there
    /// scrolls, as the module says (`scrolled_at`); over none, nothing
    /// does. Else the wheel is ↑ or ↓ for each line, pgup or pgdn for each
    /// page, as those keys are, where the cursor is.
    pub fn wheel(&mut self, at: Option<(u16, u16)>, turned: Turned) -> Effect {
        if !self.mouse {
            let (n, up, down) = match turned {
                Turned::Lines(n) => (n, KeyCode::Up, KeyCode::Down),
                Turned::Pages(n) => (n, KeyCode::PageUp, KeyCode::PageDown),
            };
            let code = if n < 0 { up } else { down };
            for _ in 0..n.unsigned_abs() {
                let e = self.key(KeyEvent::new(code, KeyModifiers::NONE));
                if e != Effect::None {
                    return e;
                }
            }
            return Effect::None;
        }
        let Some((x, y)) = at else { return Effect::None };
        // On the screen as it was last drawn, as a click is, once what was
        // done since is drawn.
        self.drawn_again();
        let Some((place, area)) = self.scrolled_at(x, y) else { return Effect::None };
        // A page: the place's height, a line less.
        let page = (area.height as isize - 1).max(1);
        let lines = match turned {
            Turned::Lines(n) => n as isize,
            Turned::Pages(n) => n as isize * page,
        };
        self.scroll(place, lines);
        self.undrawn = true;
        self.drawn_again();
        self.wheel_turn.set(None);
        // What is under the mouse now: marked, or picked, as the mouse over
        // it marks it or picks it.
        self.hover(x, y);
        Effect::None
    }

    /// The screen drawn again away from the terminal, at the size it was
    /// last drawn, if anything was done since: for the wheel, which scrolls
    /// what is drawn, typed or not.
    fn drawn_again(&mut self) {
        if self.undrawn
            && let Some(area) = self.drawn
        {
            self.draw(&mut Buffer::empty(area));
        }
    }

    /// What the wheel scrolls at `(x, y)` of the screen as it was last
    /// drawn, and where that was drawn: the last place drawn there, on top.
    /// A box open over the screen (a menu, what `?` says, a reference's
    /// preview, the comment being written) scrolls alone, and nothing out
    /// of it does, as the mouse going by out of it moves nothing; nor does
    /// anything while the ask dialog is open or a line is typed, which do
    /// not scroll. The stream read in place scrolls over the middle, a
    /// patch or HEAD being moved over the patches, and nothing elsewhere.
    fn scrolled_at(&self, x: u16, y: u16) -> Option<(Wheeled, Rect)> {
        let at = ratatui::layout::Position::new(x, y);
        let (area, place) = self.scrolls.borrow().iter().rev().find(|(r, _)| r.contains(at)).copied()?;
        let alone = |only: Wheeled| (place == only).then_some((place, area));
        match &self.mode {
            Mode::Normal => Some((place, area)),
            Mode::Menu(_) => alone(Wheeled::Menu),
            Mode::Help { .. } => alone(Wheeled::Help),
            Mode::Preview(_) => alone(Wheeled::Preview),
            Mode::Edit(_) => alone(Wheeled::Editor),
            Mode::Stream { .. } => alone(Wheeled::Middle).map(|(_, area)| (Wheeled::Stream, area)),
            Mode::Moving { .. } | Mode::MovingHead { .. } => alone(Wheeled::Patches),
            Mode::Ask(_) | Mode::Lines(_) | Mode::Prompt(_) | Mode::Find(_) => None,
        }
    }

    /// `place` scrolled `lines` down (up, below 0). What its keys scroll in
    /// place, at once, as they do: the stream, a reference's preview, the
    /// comment being written. Anything else as it is drawn next, its view
    /// moved by them (`turned`); one with a pick or a cursor then stays
    /// where the wheel left it until the next key (`free`).
    fn scroll(&mut self, place: Wheeled, lines: isize) {
        match place {
            Wheeled::Stream => {
                let page = (self.body_h / 2).max(5);
                let all = match &self.mode {
                    Mode::Stream { change, id, .. } => self.busy().get(&(change.clone(), id.clone())).map_or(0, |(_, run)| ask::tail(&self.db, run, usize::MAX).len()),
                    _ => return,
                };
                // Lines up from its end: down goes towards its end, and no
                // further up than its start, as the keys go.
                if let Mode::Stream { up, .. } = &mut self.mode {
                    *up = moved(*up, -lines, all.saturating_sub(page));
                }
            }
            Wheeled::Preview => {
                if let Mode::Preview(p) = &mut self.mode {
                    p.top = moved(p.top, lines, p.lines.len().saturating_sub(1));
                }
            }
            Wheeled::Editor => {
                if let Mode::Edit(e) = &mut self.mode {
                    e.ed.scroll(lines);
                }
            }
            _ => {
                if let Mode::Menu(m) = &mut self.mode
                    && place == Wheeled::Menu
                {
                    m.wheeled = true;
                }
                if let Some(p) = place.stays()
                    && !self.free(p)
                {
                    self.wheeled.push((self.screen, p));
                }
                self.wheel_turn.set(Some((place, lines)));
            }
        }
    }

    /// The lines the wheel turned over `place`, once: for its view to move
    /// by as it is drawn.
    pub(super) fn turned(&self, place: Wheeled) -> Option<isize> {
        let (_, lines) = self.wheel_turn.get().filter(|(p, _)| *p == place)?;
        self.wheel_turn.set(None);
        Some(lines)
    }

    /// Whether `place` stays where the wheel left it: the wheel scrolled it
    /// on this screen since the last key, and what it picked is in sight or
    /// not. Else its draw keeps that in sight, as ever.
    pub(super) fn free(&self, place: Wheeled) -> bool {
        self.wheeled.contains(&(self.screen, place))
    }

    /// That `place` is drawn at `area` of the screen: what the wheel scrolls
    /// there, but where something drawn later is.
    pub(super) fn scrolls_in(&self, area: Rect, place: Wheeled) {
        if !area.is_empty() {
            self.scrolls.borrow_mut().push((area, place));
        }
    }

    /// That `place` is drawn at `(x, y)` of `area`, `w` by `h`, as much of
    /// it as `area` holds: a box, as `spot_in` has it.
    pub(super) fn scrolls_on(&self, area: Rect, (x, y, w, h): (usize, usize, usize, usize), place: Wheeled) {
        let w = w.min((area.width as usize).saturating_sub(x));
        let h = h.min((area.height as usize).saturating_sub(y));
        if w > 0 && h > 0 {
            self.scrolls_in(Rect::new(area.x + x as u16, area.y + y as u16, w as u16, h as u16), place);
        }
    }
}
