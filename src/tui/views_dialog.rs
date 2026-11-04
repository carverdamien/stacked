//! The dialog of the views of the interdiffs, which `1` opens on every
//! screen that shows them (`App::inter_screen`), laid out as the ask dialog
//! is: a line for each choice, `Label: [ value ]`, one short line under it
//! saying what the value set draws, then Done and Cancel. Compare: the two
//! versions' diffs (the interdiff), or the code, the change from the old
//! version to the new one as a patch of its own; View: how what is compared
//! is drawn, the diffs as an interdiff, in columns or side by side, the code
//! unified, side by side or compact; Lines: every line, or only what the
//! patches changed. It opens on the choices as they are, never unset, as
//! nothing chosen here is risky. ← → step a line's value, never round past
//! either end; Enter opens the list of a line's values under it, as the ask
//! dialog's lists open (`ask_dialog::list_moved`, `draw_options`); the
//! screen behind follows each value at once, keeping its place, and the
//! status line says what it now draws. Compare changed, View keeps a value
//! of the same name in the other's list, else takes its first. Where the
//! screen cannot read the versions' bases, the code cannot be compared, the
//! dialog saying why. Done keeps the choices, for every interdiff while the
//! editor is open; Cancel, Esc and a click out of the dialog put back those
//! it opened on.

use super::*;
use crate::config::InterLines;

/// The dialog's lines, from the top: what is compared, its view, the lines
/// drawn, then the buttons'.
pub const COMPARE_LINE: usize = 0;
pub const VIEW_LINE: usize = 1;
pub const LINES_LINE: usize = 2;
pub const BUTTONS_LINE: usize = 3;

/// The dialog's buttons, from the left.
pub const DONE: usize = 0;
pub const CANCEL: usize = 1;

/// What the dialog says on its box's top.
pub const TITLE: &str = "the view of the interdiffs";

/// The dialog of the views of the interdiffs, open: the line the cursor is
/// on; the list of that line's values, open under it, by the place in it of
/// the one its cursor is on, none while it is closed; the button the
/// cursor is on, on the buttons' line; and the view and the lines as the
/// dialog opened on them, which Cancel puts back. The values set are the
/// editor's own (`App::inter`, `App::inter_lines`), which the screen draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewsDialog {
    pub line: usize,
    pub list: Option<usize>,
    pub button: usize,
    pub was: (InterLayout, InterLines),
}

/// The views of the two diffs, in the order the dialog lists them; the
/// code's; and the lines.
const DIFFS: [InterLayout; 3] = [InterLayout::Interdiff, InterLayout::Columns, InterLayout::Sides];
const CODE: [InterLayout; 3] = [InterLayout::ChangeUnified, InterLayout::ChangeSides, InterLayout::ChangeCompact];
const LINES: [InterLines; 2] = [InterLines::Every, InterLines::Changed];

/// The views of what view `v` compares, in order.
fn views_of(v: InterLayout) -> &'static [InterLayout; 3] {
    if v.change() { &CODE } else { &DIFFS }
}

/// What is compared, the code or the two diffs, as the dialog names it.
fn compared(code: bool) -> &'static str {
    if code { "the code" } else { "the two diffs" }
}

/// View `v`, as the dialog names it.
fn named(v: InterLayout) -> &'static str {
    match v {
        InterLayout::Interdiff => "interdiff",
        InterLayout::Columns => "columns",
        InterLayout::Sides | InterLayout::ChangeSides => "side by side",
        InterLayout::ChangeUnified => "unified",
        InterLayout::ChangeCompact => "compact",
    }
}

/// Lines `l`, as the dialog names them.
fn lines_named(l: InterLines) -> &'static str {
    match l {
        InterLines::Every => "every line",
        InterLines::Changed => "only what changed",
    }
}

/// What is compared, said in the line under its own.
fn compared_said(code: bool) -> &'static str {
    match code {
        false => "each version's diff, compared line by line",
        true => "each file from the old version to the new one",
    }
}

/// View `v`, said in the line under its own.
fn view_said(v: InterLayout) -> &'static str {
    match v {
        InterLayout::Interdiff => "a sign for which version has the line, then its own",
        InterLayout::Columns => "a column per version: what its patch does with the line",
        InterLayout::Sides | InterLayout::ChangeSides => "the old version on the left, the new one on the right",
        InterLayout::ChangeUnified => "- for the old version's own lines, + for the new one's",
        InterLayout::ChangeCompact => "side by side, with no gap where one side has more lines",
    }
}

/// Lines `l`, said in the line under their own.
fn lines_said(l: InterLines) -> &'static str {
    match l {
        InterLines::Every => "context, index lines and the base's changes too",
        InterLines::Changed => "no context, no index lines, none of the base's changes",
    }
}

/// What is compared, said whole, as `?` on its line says it.
fn compared_whole(code: bool) -> &'static str {
    match code {
        false => {
            "The two diffs: the interdiff, each version's diff compared with the other's line by line, drawn as an interdiff, in a column per version, or side by side."
        }
        true => {
            "The code: the change from the old version to the new one as a patch of its own, each file as the old version leaves it on its own base, then as the new one leaves it on its own, drawn as the middle draws a patch: unified, side by side, or side by side and compact."
        }
    }
}

/// View `v`, said whole, as `?` on its line says it.
fn view_whole(v: InterLayout) -> &'static str {
    match v {
        InterLayout::Interdiff => {
            "Interdiff: before each line, a sign for which version has it, a space for both, - for the old version alone, + for the new one alone; then that version's own sign."
        }
        InterLayout::Columns => {
            "Columns, a column per version: before each line, what the old version's patch does with it, then what the new one's does, + it adds the line, - it removes it, a space it leaves it as it is, = the version has it as a header or a line of its message, · the version does not have it."
        }
        InterLayout::Sides => {
            "Side by side: the old version on the left and the new one on the right, each line as its own diff draws it, the lines both have dim."
        }
        InterLayout::ChangeUnified => {
            "Unified: each file as the old version leaves it on its base, then as the new one leaves it on its own, - before a line only the old version's code has, + before one only the new one's has, the lines the change of base made dim."
        }
        InterLayout::ChangeSides => {
            "Side by side: each file as the old version leaves it on its base on the left, as the new one leaves it on its own on the right, the lines the change of base made dim."
        }
        InterLayout::ChangeCompact => {
            "Compact: the old version's lines on the left and the new one's on the right, each side's one after another, with no gap where the other has more, the lines the change of base made dim."
        }
    }
}

/// Lines `l`, said whole, as `?` on their line says it.
fn lines_whole(l: InterLines) -> &'static str {
    match l {
        InterLines::Every => {
            "Every line: of each interdiff, the lines both versions have alike, the context lines of their diffs, their index lines and the hunk headers of both versions too; of the change, the lines the change of base made, dim."
        }
        InterLines::Changed => {
            "Only what changed: of each interdiff, the lines both versions have alike, the context lines of their diffs and their index lines left out, and two hunk headers that differ only in their numbers drawn once, as the new version has them; of the change, the lines the change of base made left out, and a hunk or a file with nothing else; where lines are left out, a line says how many."
        }
    }
}

impl App {
    /// 1 on a screen of interdiffs: the dialog of their views, on the
    /// choices as they are, the cursor on what is compared.
    pub(super) fn open_views(&mut self) {
        self.mode = Mode::Views(Box::new(ViewsDialog { line: COMPARE_LINE, list: None, button: DONE, was: (self.inter, self.inter_lines) }));
    }

    /// The values of line `line` of the dialog, as drawn, in order, each
    /// with whether it can be set here: what is compared (the code not,
    /// where the versions' bases cannot be read, `change_why`), the views of
    /// it, or the lines; none on the buttons' line.
    pub fn views_values(&self, line: usize) -> Vec<(&'static str, bool)> {
        match line {
            COMPARE_LINE => vec![(compared(false), true), (compared(true), self.change_why().is_none() || self.inter.change())],
            VIEW_LINE => views_of(self.inter).iter().map(|&v| (named(v), true)).collect(),
            LINES_LINE => LINES.iter().map(|&l| (lines_named(l), true)).collect(),
            _ => vec![],
        }
    }

    /// The place among line `line`'s values of the one set.
    pub(super) fn views_set(&self, line: usize) -> usize {
        match line {
            COMPARE_LINE => self.inter.change() as usize,
            VIEW_LINE => views_of(self.inter).iter().position(|&v| v == self.inter).unwrap_or(0),
            LINES_LINE => LINES.iter().position(|&l| l == self.inter_lines).unwrap_or(0),
            _ => 0,
        }
    }

    /// Line `line` set to its value at place `k`, the screen following at
    /// once, keeping its place (`pick_inter`, `pick_lines`): what is
    /// compared changed, the view of the same name in the other's list,
    /// else its first. A value that cannot be set here, nothing set, and
    /// the status line says why.
    fn views_set_to(&mut self, line: usize, k: usize) {
        if !self.views_values(line).get(k).is_some_and(|(_, on)| *on) {
            if let (COMPARE_LINE, Some(why)) = (line, self.change_why()) {
                self.status = format!("the code cannot be compared here: {why}");
            }
            return;
        }
        if k == self.views_set(line) {
            return;
        }
        match line {
            COMPARE_LINE => {
                let to = if k == 1 { &CODE } else { &DIFFS };
                let v = to.iter().copied().find(|v| named(*v) == named(self.inter)).unwrap_or(to[0]);
                self.pick_inter(v);
            }
            VIEW_LINE => self.pick_inter(views_of(self.inter)[k.min(2)]),
            LINES_LINE => self.pick_lines(LINES[k.min(1)]),
            _ => {}
        }
    }

    /// ← (not `forward`) or → on line `line`: the value before or after the
    /// one set, past one that cannot be set here, never round past either
    /// end, so that ← held ends on the first and → held on the last.
    fn views_step(&mut self, line: usize, forward: bool) {
        let values = self.views_values(line);
        let at = self.views_set(line);
        let next = match forward {
            true => (at + 1..values.len()).find(|&k| values[k].1),
            false => (0..at).rev().find(|&k| values[k].1),
        };
        match next {
            Some(k) => self.views_set_to(line, k),
            // The code past the end, where it cannot be compared: said why.
            None if forward && line == COMPARE_LINE && values.get(at + 1).is_some_and(|(_, on)| !on) => self.views_set_to(line, at + 1),
            None => {}
        }
    }

    /// A key in the dialog `d`: ↑↓ from line to line and to the buttons;
    /// on a line, ← → step its value, and Enter opens the list of its
    /// values; on the buttons, ← → from one to the other, and Enter presses
    /// the one the cursor is on; Esc cancels. The list open, its own keys
    /// alone, as the ask dialog's: ↑↓ round past either end, Home and End,
    /// Enter setting the line to the value the cursor is on, Esc leaving it
    /// as it was, either closing the list alone.
    pub(super) fn views_key(&mut self, mut d: ViewsDialog, k: KeyEvent) -> Effect {
        if let Some(at) = d.list {
            let n = self.views_values(d.line).len().max(1);
            match k.code {
                KeyCode::Enter => {
                    d.list = None;
                    self.views_set_to(d.line, at);
                }
                KeyCode::Esc => d.list = None,
                code => d.list = Some(ask_dialog::list_moved(at, n, code).unwrap_or(at)),
            }
            self.mode = Mode::Views(Box::new(d));
            return Effect::None;
        }
        match k.code {
            KeyCode::Down => d.line = (d.line + 1) % 4,
            KeyCode::Up => d.line = (d.line + 3) % 4,
            KeyCode::Home => d.line = COMPARE_LINE,
            KeyCode::End => d.line = BUTTONS_LINE,
            KeyCode::Left if d.line == BUTTONS_LINE => d.button = DONE,
            KeyCode::Right if d.line == BUTTONS_LINE => d.button = CANCEL,
            KeyCode::Left | KeyCode::Right => self.views_step(d.line, k.code == KeyCode::Right),
            KeyCode::Enter if d.line == BUTTONS_LINE => {
                return match d.button {
                    DONE => self.views_done(&d),
                    _ => self.views_cancel(&d),
                };
            }
            KeyCode::Enter => d.list = Some(self.views_set(d.line)),
            KeyCode::Esc => return self.views_cancel(&d),
            _ => {}
        }
        self.mode = Mode::Views(Box::new(d));
        Effect::None
    }

    /// Done: the choices kept, for every interdiff while the editor is
    /// open, and the dialog closed; the status line saying what is drawn,
    /// of what changed since it opened.
    pub(super) fn views_done(&mut self, d: &ViewsDialog) -> Effect {
        self.mode = Mode::Normal;
        self.status = match (self.inter != d.was.0, self.inter_lines != d.was.1) {
            (true, _) => self.inter_status(self.inter),
            (false, true) => App::lines_status(self.inter_lines),
            _ => String::new(),
        };
        Effect::None
    }

    /// Cancel, Esc, a click out of the dialog: the view and the lines it
    /// opened on put back, the screen keeping its place, and the dialog
    /// closed.
    pub(super) fn views_cancel(&mut self, d: &ViewsDialog) -> Effect {
        self.mode = Mode::Normal;
        if self.inter != d.was.0 {
            self.pick_inter(d.was.0);
        }
        if self.inter_lines != d.was.1 {
            self.pick_lines(d.was.1);
        }
        self.status.clear();
        Effect::None
    }

    /// What the line under line `line` says of the value set: in a word,
    /// what it draws; a view this pane is too narrow for, that it is drawn
    /// otherwise meanwhile.
    fn views_under(&self, line: usize) -> String {
        let need = self.cfg.view.split_width;
        match line {
            COMPARE_LINE => compared_said(self.inter.change()).into(),
            VIEW_LINE if self.inter.sides() && self.inter_narrow() => match self.inter {
                InterLayout::Sides => format!("too narrow here, under {need} columns: drawn as an interdiff"),
                _ => format!("too narrow here, under {need} columns: drawn unified"),
            },
            VIEW_LINE => view_said(self.inter).into(),
            _ => lines_said(self.inter_lines).into(),
        }
    }

    /// What the dialog's lines say, from the top, as drawn: what is
    /// compared, the line under it, the view, the line under it, the lines,
    /// the line under them.
    pub(super) fn views_lines(&self) -> [String; 6] {
        [
            format!("Compare: [ {} ]", compared(self.inter.change())),
            self.views_under(COMPARE_LINE),
            format!("View: [ {} ]", named(self.inter)),
            self.views_under(VIEW_LINE),
            format!("Lines: [ {} ]", lines_named(self.inter_lines)),
            self.views_under(LINES_LINE),
        ]
    }

    /// What the dialog says under its buttons: where the screen cannot read
    /// the versions' bases, why the code cannot be compared there.
    fn views_note(&self) -> Vec<String> {
        match self.change_why().filter(|_| !self.inter.change()) {
            Some(why) => vec![format!("The code cannot be compared here: {why}.")],
            None => vec![],
        }
    }

    /// What `?` says in dialog `d`: on a line, the value set, said whole, and
    /// on a pane too narrow for it, that it is drawn otherwise meanwhile;
    /// what Enter and ← → do there; where the arrows go, and what Done and
    /// Cancel do. The list open, what Enter sets there, said whole. On a
    /// button, what pressing it does.
    pub(super) fn views_help(&self, d: &ViewsDialog) -> Vec<String> {
        let what = |line: usize| match line {
            COMPARE_LINE => "what is compared",
            VIEW_LINE => "the view",
            _ => "the lines",
        };
        let whole = |line: usize, k: usize| -> String {
            match line {
                COMPARE_LINE => compared_whole(k == 1).into(),
                VIEW_LINE => view_whole(views_of(self.inter)[k.min(2)]).into(),
                _ => lines_whole(LINES[k.min(1)]).into(),
            }
        };
        let done = "Done keeps what is set, for every interdiff on every screen, as long as the editor is open; Cancel, Esc and a click out of the dialog put back what the dialog opened on.";
        if let Some(at) = d.list {
            let value = self.views_values(d.line).get(at).map_or("", |(v, _)| v);
            return vec![
                format!("Press Enter to set {} to {value} and close the list: the screen follows at once. {}", what(d.line), whole(d.line, at)),
                "Press ↑↓ for another value, Esc to close the list and set nothing.".into(),
                done.into(),
            ];
        }
        if d.line == BUTTONS_LINE {
            let first = match d.button {
                DONE => "Press Enter to keep what is set, for every interdiff on every screen, as long as the editor is open, and close the dialog.",
                _ => "Press Enter to put back what the dialog opened on, and close it.",
            };
            return vec![
                first.into(),
                "Press ↑↓ to go from line to line and to the buttons. On the buttons, ← → go from one to the other; on the three lines, they set what is compared, the view and the lines. Esc closes the dialog, as Cancel does.".into(),
            ];
        }
        let names: Vec<&str> = self.views_values(d.line).iter().map(|(v, _)| *v).collect();
        let mut out = vec![whole(d.line, self.views_set(d.line))];
        let need = self.cfg.view.split_width;
        if d.line == VIEW_LINE && self.inter.sides() && self.inter_narrow() {
            out.push(match self.inter {
                InterLayout::Sides => {
                    format!("This pane is under {need} columns, too narrow for side by side: its interdiffs are drawn as interdiffs until it has room.")
                }
                InterLayout::ChangeSides => {
                    format!("This pane is under {need} columns, too narrow for the code side by side: the code is drawn unified until the pane has room.")
                }
                _ => format!(
                    "This pane is under {need} columns, too narrow for the code side by side and compact: the code is drawn unified until the pane has room."
                ),
            });
        }
        if let (COMPARE_LINE, Some(why)) = (d.line, self.change_why().filter(|_| !self.inter.change())) {
            out.push(format!("The code cannot be compared here: {why}."));
        }
        let list = match d.line {
            COMPARE_LINE => "what can be compared",
            VIEW_LINE if self.inter.change() => "the views of the code",
            VIEW_LINE => "the views of the two diffs",
            _ => "the lines that can be drawn",
        };
        out.push(format!(
            "Press Enter to open the list of {list}, to pick one: {}. Press ← or → to set {} without the list: → steps to the next and ← to the one before, and neither goes round past either end. The screen follows each value at once, keeping its place.",
            ask_dialog::one_of(&names),
            what(d.line)
        ));
        out.push(format!("Press ↑↓ to go from line to line and to the buttons. {done}"));
        out
    }

    /// The keys there are in dialog `d`, as the ask dialog says its own.
    pub(super) fn views_hint(d: &ViewsDialog) -> String {
        let what = match d.line {
            COMPARE_LINE => "what is compared",
            VIEW_LINE => "the view",
            _ => "the lines",
        };
        match (d.line, d.list) {
            (_, Some(_)) => format!("↑↓ choose · enter: set {what} · esc: close the list · ?: what it does"),
            (BUTTONS_LINE, None) => "↑↓ choose · ←→: another button · enter: press it · esc: cancel · ?: what it does".into(),
            _ => format!("↑↓ choose · ←→: set {what} · enter: open the list · esc: cancel · ?: what it does"),
        }
    }

    /// The dialog of the views of the interdiffs, if it is open: its lines,
    /// the line under each, its buttons, then why the code cannot be
    /// compared, if it cannot; in the middle of the screen, as a menu is.
    /// The list of the values of the line the cursor is on, open, under it.
    pub(super) fn draw_views(&mut self, buf: &mut Buffer, body: Rect) {
        let Mode::Views(d) = &self.mode else { return };
        let d = (**d).clone();
        let [compare, compare_said, view, view_said, lines, lines_said] = self.views_lines();
        let (w, h) = (body.width as usize, body.height as usize);
        let bw = 64.min(w.saturating_sub(2));
        let said_w = bw.saturating_sub(6).max(1);
        let note: Vec<String> = self.views_note().iter().flat_map(|l| wrap(l, bw.saturating_sub(4))).collect();
        let line_st = |k: usize| if d.line == k { on_cursor(Style::new()) } else { Style::new() };
        type Drawn = Option<(String, Style, Option<Spot>)>;
        let plain = |text: String| -> Drawn { Some((text, Style::new(), None)) };
        let mut rows: Vec<Drawn> = Vec::new();
        // Each line, the row it is drawn on, from the top of the box's
        // inside: the list of its values opens under it.
        let mut at = [0; 3];
        for (k, (line, said)) in [(compare, compare_said), (view, view_said), (lines, lines_said)].into_iter().enumerate() {
            if k > 0 {
                rows.push(plain(String::new()));
            }
            at[k] = rows.len();
            rows.push(Some((line, line_st(k), Some(Spot::ViewsLine(k)))));
            rows.extend(wrap(&said, said_w).into_iter().map(|l| plain(format!("  {l}"))));
        }
        rows.push(plain(String::new()));
        rows.push(None);
        if !note.is_empty() {
            rows.push(plain(String::new()));
            rows.extend(note.iter().map(|l| Some((l.clone(), color::note(), None))));
        }
        let bh = (rows.len() + 2).min(h);
        let (x, y) = self.boxed_at(body, bw, bh, None);
        window(buf, body, x, y, bw, bh, TITLE, color::meta());
        spot_in(&self.spots, body, (x, y, bw, bh), Spot::Box);
        for (k, row) in rows.iter().enumerate().take(bh.saturating_sub(2)) {
            let r = y + 1 + k;
            match row {
                Some((text, st, spot)) => {
                    put(buf, body, x + 1, r, bw - 2, &fit(&format!(" {text}"), bw - 2), *st);
                    if let Some(s) = spot {
                        spot_in(&self.spots, body, (x + 1, r, bw - 2, 1), *s);
                    }
                }
                None => {
                    let buttons: Vec<(String, bool, bool)> = ["Done", "Cancel"]
                        .iter()
                        .enumerate()
                        .map(|(b, label)| (format!("< {label} >"), true, d.line == BUTTONS_LINE && d.button == b))
                        .collect();
                    self.draw_buttons(buf, body, (x, r, bw), &buttons, Spot::ViewsButton);
                }
            }
        }
        // The list of the values of the line the cursor is on, open: under
        // that line, from the column of its value.
        let Some(on) = d.list.filter(|_| d.line < BUTTONS_LINE) else { return };
        if at[d.line] + 2 >= bh {
            return;
        }
        let label = self.views_lines()[d.line * 2].clone();
        let col = label.chars().take_while(|c| *c != '[').count();
        let values = self.views_values(d.line);
        self.draw_options(buf, body, (x + 2 + col, body.y + (y + 1 + at[d.line]) as u16), &values, on, Spot::ViewsOption);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_value_has_a_short_name_a_line_under_it_and_its_whole_words_for_question_mark() {
        // Each line under a value one row of the dialog, 64 wide: 58 for
        // the line, past the frame, the margin and the indent.
        let short = |said: &str| said.chars().count() <= 56;
        // The views of each, side by side in both, each said in a line.
        let names: Vec<&str> = DIFFS.iter().chain(&CODE).map(|&v| named(v)).collect();
        assert_eq!(names, ["interdiff", "columns", "side by side", "unified", "side by side", "compact"]);
        for v in DIFFS.iter().chain(&CODE) {
            assert!(short(view_said(*v)) && view_whole(*v).len() > view_said(*v).len(), "{v:?}");
            assert!(views_of(*v).contains(v));
        }
        for l in LINES {
            assert!(short(lines_said(l)) && lines_whole(l).len() > lines_said(l).len(), "{l:?}");
        }
        assert_eq!((compared(false), compared(true)), ("the two diffs", "the code"));
        assert!(short(compared_said(false)) && short(compared_said(true)));
        assert!(compared_whole(true).len() > compared_said(true).len() && compared_whole(false).len() > compared_said(false).len());
    }
}
