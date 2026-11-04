//! The dialog that asks before patches that should not change are changed
//! (`protected`), laid out as the ask dialog is: on its top, that the
//! operation changes them, or, before $EDITOR opens on an edit by hand,
//! that the edit may; a line for each, its number, its revision, its status
//! and what that status means; how long ago those statuses were fetched;
//! then what to type, a field to type it in, and `< Change them >` and `<
//! Cancel >` (`< Change it >` for one patch). It is strict: Change them,
//! and Enter, do nothing until the field says `accept`, in lower case and
//! nothing else, and the dialog says so under its buttons. Esc, Cancel, or
//! a click out of the dialog cancels it: nothing is written, and the bottom
//! line says that nothing changed, and why. Once `accept` is typed, what was
//! asked is done again as it was asked (`Again`), those patches confirmed
//! for its one operation (`Db::confirm`): the operation the database
//! refused, which wrote nothing, and whose check of what applies kept what
//! it found; or the edit by hand, whose $EDITOR opens then, the confirmation
//! going with it to the close of its review, which writes it.

use super::reviewing::ToClose;
use super::*;
use crate::protected::Refused;

/// The dialog's lines, from the top: its field, its buttons'.
pub const FIELD_LINE: usize = 0;
pub const BUTTONS_LINE: usize = 1;

/// The dialog's buttons, from the left: Change them, Cancel.
pub const CHANGE: usize = 0;
pub const CANCEL: usize = 1;

/// What the field must say, to the letter, for Change them to change them.
pub const ACCEPT: &str = "accept";

/// What was asked when the dialog opened: done again as it was asked once
/// `accept` is typed.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Again {
    /// What a menu's item does, or an Enter that does it: the write it made
    /// was refused, or it is an edit by hand, asked before $EDITOR opens.
    Action(Action),
    /// The patch picked up at `from` put down at `to`, which was refused.
    Move { from: usize, to: usize },
    /// The review closed as it was asked, from it or from the banner, which
    /// was refused: closed again, as the check of the stack the operations
    /// accepted leave was made already.
    Close(ToClose),
}

/// The dialog, open: the patches it asks about (`Refused`); whether it asks
/// before $EDITOR opens on an edit by hand, which may change them, rather
/// than after a write that would; what is typed in its field; the line the
/// cursor is on, `FIELD_LINE` or `BUTTONS_LINE`, and the button it is on
/// there; the cell clicked to open it, when a click did, which it is drawn
/// next to; and what was asked (`again`), with the patches confirmed for it
/// before, when it asked already of others (`also`), which typing accept
/// here confirms too.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectedDialog {
    pub refused: Refused,
    pub hand: bool,
    pub typed: String,
    pub line: usize,
    pub button: usize,
    pub at: Option<(u16, u16)>,
    pub(super) also: Vec<String>,
    pub(super) again: Again,
}

impl ProtectedDialog {
    /// Whether the field says accept, to the letter: then Change them
    /// changes them.
    pub fn ready(&self) -> bool {
        self.typed == ACCEPT
    }

    /// Its buttons as drawn, from the left: `< Change them >`, `< Cancel >`.
    pub fn drawn(&self) -> [String; 2] {
        [format!("< {} >", self.refused.change_label()), "< Cancel >".into()]
    }

    /// What it says under its buttons while Change them is grayed: that the
    /// field must say accept, and how.
    pub fn note(&self) -> Option<String> {
        (!self.ready()).then(|| format!("{} is grayed until the field says accept, in lower case and nothing else.", self.refused.change_label()))
    }
}

/// Whether `action` is an edit by hand in $EDITOR, of a patch, its code or
/// the stack as a list, which may change what it edits: whether it goes on
/// with an edit not taken (`Edit it again`, a conflict resolved by hand),
/// whose patches confirmed it keeps; none for any other action.
pub(super) fn hand_edit(action: &Action) -> Option<bool> {
    match action {
        Action::EditPatch | Action::EditCode | Action::EditList | Action::EditWorkList | Action::EditWorkPatch(..) | Action::EditWorkFile(..) => Some(false),
        Action::EditAgain | Action::Resolve => Some(true),
        _ => None,
    }
}

impl App {
    /// The dialog opened on `refused`, nothing typed, the cursor in its
    /// field: `again`, what was asked; `hand`, before $EDITOR opens on an
    /// edit by hand; `also`, the patches confirmed for it before.
    pub(super) fn ask_protected(&mut self, refused: Refused, again: Again, hand: bool, also: Vec<String>) {
        self.mode = Mode::Protected(Box::new(ProtectedDialog {
            refused,
            hand,
            typed: String::new(),
            line: FIELD_LINE,
            button: CANCEL,
            at: None,
            also,
            again,
        }));
    }

    /// Of an edit by hand in $EDITOR that `action` asks for, the patches
    /// that should not change it may change, not confirmed for it yet, if
    /// there are any, to ask about before the editor opens: the patch
    /// edited (as mail); with the patches above it, which conflicts with it
    /// resolved by hand rebase, for its code; every patch for the stack as a
    /// list, which may edit or reword any. Of the stack as it is, or as the
    /// landed work it edits from its review would make it, then by the
    /// versions of the stack as it is (`protected::of_edit`). Going on with
    /// an edit not taken, those confirmed for that edit are not asked about
    /// again (`hand_ok`). None for any other action.
    pub(super) fn hand_refused(&self, action: &Action) -> Option<Refused> {
        let continuing = hand_edit(action)?;
        let all = usize::MAX;
        let (work, places) = match action {
            Action::EditPatch if self.patch_id().is_some() => (None, self.sel..self.sel + 1),
            Action::EditCode if self.code_place().is_some() => (None, self.sel..all),
            Action::EditList => (None, 0..all),
            Action::EditAgain if self.refused.is_some() => match &self.refused_work {
                Some((run, at)) => (Some(run.clone()), *at..all),
                None => (None, self.sel..all),
            },
            Action::Resolve => {
                let c = self.conflict.as_ref()?;
                (c.work.clone(), c.at..all)
            }
            Action::EditWorkList | Action::EditWorkPatch(..) | Action::EditWorkFile(..) => {
                let run = self.reviewing.as_ref()?.run.clone();
                match action {
                    Action::EditWorkPatch(at, _) => (Some(run), *at..at + 1),
                    Action::EditWorkFile(at, ..) => (Some(run), *at..all),
                    _ => (Some(run), 0..all),
                }
            }
            _ => return None,
        };
        let from = ops::edited_from(&self.db, &self.stack, work.as_deref()).ok()?;
        let covered = |id: &String| self.hand_once.contains(id) || (continuing && self.hand_ok.contains(id));
        let patches: Vec<crate::protected::Changed> =
            crate::protected::of_edit(&self.db, &self.stack, &from, places).into_iter().filter(|c| !covered(&c.id)).collect();
        (!patches.is_empty()).then(|| Refused { stack: self.stack.clone(), patches })
    }

    /// A key in dialog `d`: what is typed goes in its field, the cursor
    /// going there, Backspace taking its last character back; ↑↓ and Tab go
    /// from the field to the buttons and back; on the buttons, ← → go from
    /// one to the other, past Change them while it is grayed; Enter presses
    /// the button the cursor is on, Change them from the field; Esc cancels.
    /// The keys come as they are typed, not as the settings map them.
    pub(super) fn protected_key(&mut self, mut d: ProtectedDialog, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Esc => return self.protected_cancel(&d),
            KeyCode::Enter if d.line == BUTTONS_LINE && d.button == CANCEL => return self.protected_cancel(&d),
            KeyCode::Enter => return self.protected_change(d),
            KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => d.line = (d.line + 1) % 2,
            KeyCode::Left if d.line == BUTTONS_LINE && d.ready() => d.button = CHANGE,
            KeyCode::Right if d.line == BUTTONS_LINE => d.button = CANCEL,
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
        // Grayed again, Change them is no button the cursor stays on.
        if !d.ready() && d.button == CHANGE {
            d.button = CANCEL;
        }
        self.mode = Mode::Protected(Box::new(d));
        Effect::None
    }

    /// Change them, pressed, or Enter in the field: while the field does
    /// not say accept, nothing, the dialog staying, and the bottom line
    /// saying why; else what was asked, done again with the patches
    /// confirmed (`protected_go`).
    fn protected_change(&mut self, d: ProtectedDialog) -> Effect {
        if let Some(why) = d.note() {
            self.status = why;
            self.mode = Mode::Protected(Box::new(d));
            return Effect::None;
        }
        self.protected_go(d)
    }

    /// Esc, Cancel, a click out of dialog `d`: closed, nothing written, the
    /// bottom line saying that nothing changed, and why. Asked as an edit
    /// not taken was to be edited again or its conflict resolved, the menu
    /// of that edit again, which keeps it.
    pub(super) fn protected_cancel(&mut self, d: &ProtectedDialog) -> Effect {
        self.mode = Mode::Normal;
        self.status = d.refused.not_typed(d.hand);
        if let Again::Action(a) = &d.again
            && hand_edit(a) == Some(true)
            && self.refused.is_some()
        {
            self.not_taken(self.status.clone());
        }
        Effect::None
    }

    /// `accept` typed in dialog `d`: what was asked done again as it was
    /// asked, the patches it named, and those confirmed before it, confirmed
    /// for its one operation, then for no other. The operation refused is
    /// the next edit of the database (`App::edit` takes them); an edit by
    /// hand, the edit $EDITOR now opens on (`hand_once`, which `perform`
    /// passes on to it). What that gives back for $EDITOR, the edit by hand.
    fn protected_go(&mut self, d: ProtectedDialog) -> Effect {
        self.mode = Mode::Normal;
        let mut ids = d.also;
        ids.extend(d.refused.ids());
        self.confirmed = ids.clone();
        self.hand_once = ids;
        let e = match d.again {
            Again::Action(a) => self.perform(a),
            Again::Move { from, to } => {
                self.move_patch_to(from, to);
                Effect::None
            }
            Again::Close(c) => {
                self.close_again(c);
                Effect::None
            }
        };
        self.confirmed.clear();
        self.hand_once.clear();
        e
    }

    /// What can be done in dialog `d`, said: on the field and on Change
    /// them, what typing accept does, and how it is typed; on Cancel, that
    /// nothing changes; and where the keys go.
    pub(super) fn protected_help(&self, d: &ProtectedDialog) -> Vec<String> {
        let label = d.refused.change_label();
        let them = if d.refused.patches.len() == 1 { "it" } else { "them" };
        let first = match (d.line, d.button) {
            (BUTTONS_LINE, CANCEL) => "Press Enter to close the dialog: nothing changes, and the bottom line says why.".to_string(),
            _ if d.hand => format!(
                "Type accept, in lower case and nothing else, then press Enter, or {label}, to open $EDITOR on the edit all the same: closing its review then changes {them} with no one asked again."
            ),
            _ => format!("Type accept, in lower case and nothing else, then press Enter, or {label}, to change {them} all the same."),
        };
        vec![
            first,
            format!("Until the field says accept, {label} and Enter do nothing. Press ↑↓ or Tab to go from the field to the buttons and back, ← → from one button to the other. Esc closes the dialog, as Cancel does, and nothing changes."),
        ]
    }

    /// The keys there are in dialog `d`.
    pub(super) fn protected_hint(d: &ProtectedDialog) -> String {
        let label = d.refused.change_label().to_lowercase();
        match d.line {
            FIELD_LINE => format!("type accept · enter: {label} · ↓: the buttons · esc: cancel"),
            _ => "←→: another button · enter: press it · ↑: the field · esc: cancel".into(),
        }
    }

    /// The dialog, if one is open: on its top what it asks about, then a
    /// line for each patch, how long ago their statuses were fetched, what
    /// to type, its field, its buttons, and what keeps Change them grayed;
    /// in the middle of the screen, as the ask dialog is, or next to the
    /// cell clicked to open it. Where the terminal's cursor goes: at the end
    /// of what is typed, while the cursor is in the field.
    pub(super) fn draw_protected(&mut self, buf: &mut Buffer, body: Rect) -> Option<(u16, u16)> {
        let Mode::Protected(d) = &self.mode else { return None };
        let d = (**d).clone();
        let title = d.refused.title(d.hand);
        let screen = buf.area;
        let area = match d.at {
            Some(_) => Rect::new(screen.x, screen.y + 1, screen.width, screen.height.saturating_sub(2)),
            None => body,
        };
        let (w, h) = (area.width as usize, area.height as usize);
        // As wide as its title, or a patch's line, but within the ask
        // dialog's widths.
        let widest = d.refused.lines().iter().map(|l| l.chars().count() + 2).chain([title.chars().count()]).max().unwrap_or(0);
        let bw = (widest + 6).clamp(64, 78).min(w.saturating_sub(2));
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
        // Each patch's line, folded under its start.
        for line in d.refused.lines() {
            for (k, l) in wrap(&line, text_w.saturating_sub(2).max(1)).into_iter().enumerate() {
                rows.push(Row::Said(if k == 0 { l } else { format!("  {l}") }, Style::new()));
            }
        }
        let fetched = d.refused.fetched(crate::db::now());
        if !fetched.is_empty() {
            rows.push(Row::Said(String::new(), Style::new()));
            rows.extend(said(&fetched, Style::new()));
        }
        rows.push(Row::Said(String::new(), Style::new()));
        rows.extend(said(&d.refused.type_said(), Style::new()));
        rows.push(Row::Field);
        rows.push(Row::Said(String::new(), Style::new()));
        rows.push(Row::Buttons);
        if let Some(note) = d.note() {
            rows.push(Row::Said(String::new(), Style::new()));
            rows.extend(said(&note, color::note()));
        }
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
                    spot_in(&self.spots, area, (x + 2, r, inner + 2, 1), Spot::ProtectedField);
                    if d.line == FIELD_LINE && r < area.height as usize {
                        cursor = Some((area.x + (x + 3 + shown.chars().count()) as u16, area.y + r as u16));
                    }
                }
                // The buttons, each a spot of its own: Change them dim while
                // the field does not say accept; the one the cursor is on,
                // on the buttons' line, under the cursor.
                Row::Buttons => {
                    let on = [d.ready(), true];
                    let buttons: Vec<(String, bool, bool)> =
                        d.drawn().into_iter().enumerate().map(|(b, drawn)| (drawn, on[b], d.line == BUTTONS_LINE && d.button == b)).collect();
                    self.draw_buttons(buf, area, (x, r, bw), &buttons, Spot::ProtectedButton);
                }
            }
        }
        cursor
    }
}
