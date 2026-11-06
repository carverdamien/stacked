//! The dialog that takes the number of the bug the agent drafted, once its
//! review closed and Bugzilla's form to file it opened in your browser,
//! filled in (`bug`), laid out as the dialog that asks before patches that
//! should not change are changed: on its top, the patch it is for; that the
//! form is open, and what it files; to submit it on Bugzilla and give the
//! bug's number here; a field for the number, or a link to the bug pasted;
//! then its buttons, `< Put it in the title >`, `< Open the form again >`
//! and `< Cancel >`. Put it in the title, and Enter in the field, do
//! nothing until the field holds a bug's number (`bug::number_in`), and the
//! dialog says so under its buttons. Then the patch's title names the bug,
//! as moz-phab
//! writes it (`Bug N - title`): a new version of the patch, its message
//! alone changed, written as any change of the stack is, the dialog that
//! asks before patches that should not change are changed asking first
//! when it would change one. Open the form again opens it once more, the
//! dialog staying. Esc, Cancel, or a click out of the dialog closes it:
//! nothing changes, and the form stays open in the browser, to file the bug
//! all the same. A bug drafted for a patch whose title names one already,
//! another one, as a comment that says bug asks: no dialog, the form alone
//! opening, and the title keeping its bug.

use super::*;
use crate::bug::Draft;

/// The dialog's lines, from the top: its field, its buttons'.
pub const FIELD_LINE: usize = 0;
pub const BUTTONS_LINE: usize = 1;

/// The dialog's buttons, from the left: Put it in the title, Open the form
/// again, Cancel.
pub const PUT: usize = 0;
pub const AGAIN: usize = 1;
pub const CANCEL: usize = 2;

/// The dialog, open: the bug drafted; the patch it is for, by its change;
/// what is typed in its field; the line the cursor is on, `FIELD_LINE` or
/// `BUTTONS_LINE`, and the button it is on there; and the cell clicked to
/// open it, when a click did, which it is drawn next to.
#[derive(Debug, Clone, PartialEq)]
pub struct BugDialog {
    pub draft: Draft,
    pub change: String,
    pub typed: String,
    pub line: usize,
    pub button: usize,
    pub at: Option<(u16, u16)>,
}

impl BugDialog {
    /// The number of the bug the field names, if it names one.
    pub fn number(&self) -> Option<u64> {
        crate::bug::number_in(&self.typed)
    }

    /// Whether button `b` can be pressed: Put it in the title once the field
    /// holds a bug's number; the others always.
    pub fn pressable(&self, b: usize) -> bool {
        b != PUT || self.number().is_some()
    }

    /// Its buttons as drawn, from the left.
    pub fn drawn() -> [String; 3] {
        ["< Put it in the title >".into(), "< Open the form again >".into(), "< Cancel >".into()]
    }
}

impl App {
    /// The form to file `draft`, the bug drafted for the patch of `change`,
    /// opened in your browser, filled in, on the settings' Bugzilla
    /// (`reviews.bugzilla`), as a link is opened (`open_link`: in the tests,
    /// by their stand-in); then the dialog that takes the bug's number,
    /// nothing typed, the cursor in its field. The patch's title naming a
    /// bug already, the bug drafted being another one, no dialog: the title
    /// keeps its bug, and the bottom line says so once the form is open.
    /// The bottom line says what closing the review did until the form is
    /// open.
    pub(super) fn file_bug(&mut self, draft: Draft, change: String) {
        let reviewed = self.status.clone();
        let kept = self.bug_kept(&change);
        self.open_form_then(&draft, kept.as_deref());
        if !reviewed.is_empty() {
            self.status = reviewed;
        }
        if kept.is_none() {
            self.mode = Mode::Bug(Box::new(BugDialog { draft, change, typed: String::new(), line: FIELD_LINE, button: CANCEL, at: None }));
        }
    }

    /// Of the patch of `change`, its title naming a bug already, what the
    /// bottom line says once the form is open: that the title keeps it
    /// (`Patch 3's title keeps bug 1234567.`). None when it names none.
    fn bug_kept(&self, change: &str) -> Option<String> {
        #[cfg(feature = "moz-phab")]
        {
            let n = crate::bug::named_in(self.db.latest_version(change)?.title())?;
            let at = self.current().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == change));
            Some(match at {
                Some(at) => format!("Patch {}'s title keeps bug {n}.", at + 1),
                None => format!("The patch's title keeps bug {n}."),
            })
        }
        #[cfg(not(feature = "moz-phab"))]
        {
            let _ = change;
            None
        }
    }

    /// The form to file `draft` opened in your browser, away from the
    /// screen, the bottom line saying it is, or why it could not be: its
    /// link is long, and is never shown.
    fn open_form(&mut self, draft: &Draft) {
        self.open_form_then(draft, None);
    }

    /// The form to file `draft` opened, as `open_form` opens it, the bottom
    /// line saying `then` after it once it is open.
    fn open_form_then(&mut self, draft: &Draft, then: Option<&str>) {
        let then = then.map_or(String::new(), |t| format!(" {t}"));
        let url = crate::bug::form_url(&self.cfg.reviews.bugzilla, draft);
        if self.bg.doing.contains(&OPENING) {
            self.status = "a link is being opened already: wait for it, then open the form again".into();
            return;
        }
        self.bg.doing.push(OPENING);
        let said = draft.said();
        self.status = format!("The form to file the bug is opening in your browser: {said}.");
        self.spawn(move || {
            let done = match crate::refs::open_url(&url) {
                Ok(_) => format!("The form to file the bug is open in your browser: {said}.{then}"),
                Err(why) => why.replace("the link could not be opened", "the form to file the bug could not be opened"),
            };
            Done::Said(OPENING, done)
        });
    }

    /// The patch the bug in dialog `d` is for, by its place from 0 in the
    /// stack as it is, and its title.
    fn bug_patch(&self, d: &BugDialog) -> Option<(usize, String)> {
        let s = self.current()?;
        let at = s.patches.iter().position(|p| self.db.patch(p).change == d.change)?;
        Some((at, self.db.patch(&s.patches[at]).title().to_string()))
    }

    /// What dialog `d` says above its field, a sentence each.
    pub fn bug_lines(&self, d: &BugDialog) -> Vec<String> {
        vec![
            format!("The form to file the bug is open in your browser: {}.", d.draft.said()),
            "Review it there and submit it on Bugzilla, then give the bug's number here, or paste the link to the bug.".into(),
        ]
    }

    /// What dialog `d` says under its buttons: what keeps Put it in the
    /// title grayed; else what the patch's title becomes.
    pub fn bug_note(&self, d: &BugDialog) -> String {
        match (d.number(), self.bug_patch(d)) {
            (None, _) => "Put it in the title is grayed until the field holds the bug's number, or a link to the bug.".into(),
            (Some(n), Some((at, title))) => format!("The title of patch {} becomes “{}”.", at + 1, crate::bug::titled(&title, n)),
            (Some(_), None) => "The patch the bug was drafted for is no longer in the stack.".into(),
        }
    }

    /// A key in dialog `d`: what is typed goes in its field, the cursor
    /// going there, Backspace taking its last character back; ↑↓ and Tab go
    /// from the field to the buttons and back; on the buttons, ← → go from
    /// one to the next, past Put it in the title while it is grayed; Enter
    /// presses the button the cursor is on, Put it in the title from the
    /// field; Esc cancels. The keys come as they are typed, not as the
    /// settings map them.
    pub(super) fn bug_key(&mut self, mut d: BugDialog, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Esc => return self.bug_cancel(&d),
            KeyCode::Enter if d.line == BUTTONS_LINE => return self.bug_press(d),
            KeyCode::Enter => return self.bug_put(d),
            KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => d.line = (d.line + 1) % 2,
            KeyCode::Left if d.line == BUTTONS_LINE => {
                if let Some(b) = (0..d.button).rev().find(|b| d.pressable(*b)) {
                    d.button = b;
                }
            }
            KeyCode::Right if d.line == BUTTONS_LINE => d.button = (d.button + 1).min(CANCEL),
            KeyCode::Backspace => {
                d.typed.pop();
                d.line = FIELD_LINE;
            }
            KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) => {
                d.typed.push(c);
                d.line = FIELD_LINE;
            }
            _ => {}
        }
        // Grayed again, Put it in the title is no button the cursor stays on.
        if !d.pressable(d.button) {
            d.button = CANCEL;
        }
        self.mode = Mode::Bug(Box::new(d));
        Effect::None
    }

    /// The button the cursor is on in dialog `d`, pressed.
    pub(super) fn bug_press(&mut self, d: BugDialog) -> Effect {
        match d.button {
            PUT => self.bug_put(d),
            AGAIN => {
                self.open_form(&d.draft);
                self.mode = Mode::Bug(Box::new(d));
                Effect::None
            }
            _ => self.bug_cancel(&d),
        }
    }

    /// Put it in the title, pressed, or Enter in the field: while the field
    /// holds no bug's number, nothing, the dialog staying, and the bottom
    /// line saying why; else the dialog closed, and the patch's title naming
    /// the bug, as any change of the stack is written (`perform`).
    fn bug_put(&mut self, d: BugDialog) -> Effect {
        let Some(n) = d.number() else {
            self.status = self.bug_note(&d);
            self.mode = Mode::Bug(Box::new(d));
            return Effect::None;
        };
        self.mode = Mode::Normal;
        self.perform(Action::PutBug(d.change, n))
    }

    /// Bug `n` put in the title of the patch of `change`: a new version of
    /// it, its message alone changed, written at once; refused as any
    /// change of the stack is, the dialog that asks before patches that
    /// should not change are changed asking first, and doing it again once
    /// accept is typed. The bottom line says what came of it.
    pub(super) fn put_bug(&mut self, change: &str, n: u64) {
        #[cfg(feature = "moz-phab")]
        {
            let name = self.stack.clone();
            if let Some(at) = self.edit(|db| crate::bug::put_in_title(db, &name, change, n)) {
                let title = self.current().and_then(|s| s.patches.get(at)).map(|p| self.db.patch(p).title().to_string()).unwrap_or_default();
                self.status = format!("The title of patch {} names bug {n} now: “{title}”. The next submit to Phabricator sends it.", at + 1);
            }
        }
        #[cfg(not(feature = "moz-phab"))]
        {
            let _ = (change, n);
            self.status = "this stacked is built without the moz-phab feature: it puts no bug in a title".into();
        }
    }

    /// Esc, Cancel, a click out of dialog `d`: closed, nothing written, the
    /// bottom line saying so.
    pub(super) fn bug_cancel(&mut self, d: &BugDialog) -> Effect {
        self.mode = Mode::Normal;
        let of = self.bug_patch(d).map_or("the patch".to_string(), |(at, _)| format!("patch {}", at + 1));
        self.status = format!("Nothing changed: the title of {of} is as it was. The form stays open in your browser, to file the bug all the same.");
        Effect::None
    }

    /// What can be done in dialog `d`, said.
    pub(super) fn bug_help(&self, d: &BugDialog) -> Vec<String> {
        let first = match (d.line, d.button) {
            (BUTTONS_LINE, CANCEL) => "Press Enter to close the dialog: nothing changes, and the form stays open in your browser.".to_string(),
            (BUTTONS_LINE, AGAIN) => "Press Enter to open the form to file the bug again, filled in with the agent's draft: the dialog stays open.".to_string(),
            _ => "Type the bug's number, or paste the link to the bug, once you have submitted the form on Bugzilla; then press Enter, or Put it in the title, to put the bug in the patch's title, as moz-phab writes it: a new version of the patch, its message alone changed.".to_string(),
        };
        vec![
            first,
            "Until the field holds a bug's number, Put it in the title and Enter in the field do nothing. Press ↑↓ or Tab to go from the field to the buttons and back, ← → from one button to the next. Esc closes the dialog, as Cancel does, and nothing changes.".into(),
        ]
    }

    /// The keys there are in dialog `d`.
    pub(super) fn bug_hint(d: &BugDialog) -> String {
        match d.line {
            FIELD_LINE => "type the bug's number · enter: put it in the title · ↓: the buttons · esc: cancel".into(),
            _ => "←→: another button · enter: press it · ↑: the field · esc: cancel".into(),
        }
    }

    /// The dialog, if one is open: on its top the patch it is for, then what
    /// it says, its field, its buttons, and what Put it in the title does or
    /// what keeps it grayed; in the middle of the screen, or next to the
    /// cell clicked to open it. Where the terminal's cursor goes: at the end
    /// of what is typed, while the cursor is in the field.
    pub(super) fn draw_bug(&mut self, buf: &mut Buffer, body: Rect) -> Option<(u16, u16)> {
        let Mode::Bug(d) = &self.mode else { return None };
        let d = (**d).clone();
        let title = match self.bug_patch(&d) {
            Some((at, _)) => format!("file the bug drafted for patch {}", at + 1),
            None => "file the bug the agent drafted".to_string(),
        };
        let screen = buf.area;
        let area = match d.at {
            Some(_) => Rect::new(screen.x, screen.y + 1, screen.width, screen.height.saturating_sub(2)),
            None => body,
        };
        let (w, h) = (area.width as usize, area.height as usize);
        let bw = 72.min(w.saturating_sub(2));
        let text_w = bw.saturating_sub(4).max(1);
        /// A row of the dialog: a line it says, in its style; its field;
        /// its buttons.
        enum Row {
            Said(String, Style),
            Field,
            Buttons,
        }
        let said = |text: &str, st: Style| -> Vec<Row> { wrap(text, text_w).into_iter().map(|l| Row::Said(l, st)).collect() };
        let mut rows: Vec<Row> = Vec::new();
        for (k, line) in self.bug_lines(&d).iter().enumerate() {
            if k > 0 {
                rows.push(Row::Said(String::new(), Style::new()));
            }
            rows.extend(said(line, Style::new()));
        }
        rows.push(Row::Said(String::new(), Style::new()));
        rows.push(Row::Field);
        rows.push(Row::Said(String::new(), Style::new()));
        rows.push(Row::Buttons);
        rows.push(Row::Said(String::new(), Style::new()));
        let st = if d.number().is_some() { Style::new() } else { color::note() };
        rows.extend(said(&self.bug_note(&d), st));
        let bh = (rows.len() + 2).min(h);
        let (x, y) = self.boxed_at(area, bw, bh, d.at);
        window(buf, area, x, y, bw, bh, &title, color::meta());
        spot_in(&self.spots, area, (x, y, bw, bh), Spot::Box);
        let mut cursor = None;
        for (k, row) in rows.iter().enumerate().take(bh.saturating_sub(2)) {
            let r = y + 1 + k;
            match row {
                Row::Said(text, st) => put(buf, area, x + 1, r, bw - 2, &fit(&format!(" {text}"), bw - 2), *st),
                // The field, as wide as the box lets it be: what is typed, its
                // end in sight; under the cursor while the cursor is in it,
                // the terminal's cursor after what is typed.
                Row::Field => {
                    let inner = text_w.saturating_sub(2).max(1);
                    let typed: Vec<char> = d.typed.chars().collect();
                    let shown: String = typed[typed.len().saturating_sub(inner - 1)..].iter().collect();
                    let st = if d.line == FIELD_LINE { on_cursor(Style::new()) } else { Style::new() };
                    put(buf, area, x + 1, r, 1, " ", Style::new());
                    put(buf, area, x + 2, r, inner + 2, &format!("[{}]", fit(&shown, inner)), st);
                    spot_in(&self.spots, area, (x + 2, r, inner + 2, 1), Spot::BugField);
                    if d.line == FIELD_LINE && r < area.height as usize {
                        cursor = Some((area.x + (x + 3 + shown.chars().count()) as u16, area.y + r as u16));
                    }
                }
                // The buttons, each a spot of its own: Put it in the title dim
                // while the field holds no bug's number; the one the cursor is
                // on, on the buttons' line, under the cursor.
                Row::Buttons => {
                    let buttons: Vec<(String, bool, bool)> = BugDialog::drawn()
                        .into_iter()
                        .enumerate()
                        .map(|(b, drawn)| (drawn, d.pressable(b), d.line == BUTTONS_LINE && d.button == b))
                        .collect();
                    self.draw_buttons(buf, area, (x, r, bw), &buttons, Spot::BugButton);
                }
            }
        }
        cursor
    }
}
