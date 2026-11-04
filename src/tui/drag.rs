//! HEAD dragged with the mouse, in the browser and in StackED, which send
//! the left button pressed on a cell (`App::down`), the mouse over another
//! with the button down (`App::drag`) and the button let go (`App::up`);
//! a terminal sends none. A press on HEAD's row, on the main screen and in
//! no mode, is held: let go where it was pressed, it is the click it ever
//! was; dragged onto another cell, it moves HEAD as `Move HEAD…` does, each
//! of its refusals said as it says it, HEAD then following the mouse over
//! the patches as the mouse going by places it, and put down where it is
//! when the button is let go over the patches, as Enter puts it down. Let
//! go anywhere else, or after Esc, HEAD is left where it was. A press
//! anywhere else is a click at once, and a drag from there is the mouse
//! going by, as they were.

use super::*;

/// The left button pressed on HEAD's row, and not let go yet: the cell it
/// was pressed on, and whether the mouse dragged HEAD from there since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Held {
    at: (u16, u16),
    dragging: bool,
}

impl App {
    /// The left button pressed on `(x, y)` of the screen, as it was last
    /// drawn. On HEAD's row, in no mode, held for a drag (`drag`), the
    /// click it would be waiting for the button to be let go (`up`); else
    /// a click, as `click` does it, at once.
    pub fn down(&mut self, x: u16, y: u16) -> Effect {
        self.mouse = true;
        self.held = None;
        if self.drags_at(x, y) {
            self.held = Some(Held { at: (x, y), dragging: false });
            return Effect::None;
        }
        self.click(x, y, false)
    }

    /// The mouse over `(x, y)` with the left button down. Held on HEAD's
    /// row and over another cell now: HEAD moved as `Move HEAD…` moves it,
    /// which says why when it would not move HEAD, in a view of its own;
    /// then, HEAD being moved, under the patch the mouse is over, as the
    /// mouse going by places it. Else the mouse going by, as `hover` takes
    /// it.
    pub fn drag(&mut self, x: u16, y: u16) {
        match self.held {
            Some(Held { at, dragging: false }) if at == (x, y) => self.mouse = true,
            Some(Held { at, dragging: false }) => {
                self.mouse = true;
                self.perform(Action::MoveHead(None));
                match self.mode {
                    Mode::MovingHead { .. } => {
                        self.held = Some(Held { at, dragging: true });
                        self.hover(x, y);
                    }
                    _ => self.held = None,
                }
            }
            Some(Held { dragging: true, .. }) if matches!(self.mode, Mode::MovingHead { .. }) => self.hover(x, y),
            Some(_) => {
                self.held = None;
                self.hover(x, y);
            }
            None => self.hover(x, y),
        }
    }

    /// The left button let go over `at` of the screen, none off it. Held on
    /// HEAD's row and not dragged: the click it is, on where it was pressed.
    /// HEAD dragged, and being moved still: let go over the patches, put
    /// down there, under the patch let go on (under the header, at the
    /// base), or where it is drawn, as Enter puts it down, but below its
    /// last commit marked `!`, which is said as a click there says it, HEAD
    /// being moved still; let go out of the patches, left where it was, as
    /// Esc leaves it. Else nothing.
    pub fn up(&mut self, at: Option<(u16, u16)>) -> Effect {
        match self.held.take() {
            Some(Held { at: (x, y), dragging: false }) => self.click(x, y, false),
            Some(Held { dragging: true, .. }) if matches!(self.mode, Mode::MovingHead { .. }) => {
                let Some((x, y)) = at.filter(|&(x, y)| self.over_patches(x, y)) else {
                    self.mode = Mode::Normal;
                    return Effect::None;
                };
                // On the screen as it was last drawn, as a click is; Enter
                // then on the screen as it would be drawn now.
                self.undrawn = true;
                if let Some(Spot::Patch(under)) = self.spot_at(x, y) {
                    let low = self.head_here().map_or(0, crate::head::Head::locked);
                    let there = under.map_or(0, |under| under + 1);
                    if there < low {
                        self.status = self.head_here().and_then(crate::head::Head::lock_said).unwrap_or_default();
                        return Effect::None;
                    }
                    if let Mode::MovingHead { to } = &mut self.mode {
                        *to = there;
                    }
                }
                self.pressed(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            }
            _ => Effect::None,
        }
    }

    /// Where a press of the left button is held for a drag, on the screen
    /// as it was last drawn: HEAD's row, on the main screen, in no mode.
    /// StackED selects no text from there.
    pub fn drags(&self) -> Vec<Rect> {
        if !self.drags_here() {
            return Vec::new();
        }
        self.spots.borrow().iter().filter(|(_, s)| *s == Spot::HeadRow).map(|(r, _)| *r).collect()
    }

    /// Whether a press is held for a drag anywhere: on the main screen, in
    /// no mode.
    fn drags_here(&self) -> bool {
        self.screen == Screen::Main && matches!(self.mode, Mode::Normal)
    }

    /// Whether a press on `(x, y)` is held for a drag (`drags`): HEAD's row
    /// drawn there, on top.
    fn drags_at(&self, x: u16, y: u16) -> bool {
        self.drags_here() && self.spot_at(x, y) == Some(Spot::HeadRow)
    }

    /// Whether `(x, y)` is in the patches pane, as it was last drawn.
    fn over_patches(&self, x: u16, y: u16) -> bool {
        let at = ratatui::layout::Position::new(x, y);
        self.scrolls.borrow().iter().any(|(r, place)| *place == Wheeled::Patches && r.contains(at))
    }
}
