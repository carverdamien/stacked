//! A patch sent to Phabricator as work in progress, from its menu: what is
//! sent read first, away from the screen, nothing sent yet. For a new
//! revision, a dialog then says what is sent and where. For a revision
//! there already, a screen of its own shows what replaces what: what is
//! sent and where, said; the title and summary, Phabricator's then what is
//! sent; what else changes (the parent, the bug, the status); and the
//! code, each file as Phabricator's latest diff leaves it, then as the
//! patch does, both on the patches below it, as the other diffs are shown.
//! Enter sends it, once the revision is read again and seen to be as it
//! was; Esc sends nothing. While HEAD is on the stack, a patch whose
//! commit in the workspace names a revision it does not is held back, its
//! item grayed, before anything is read. Built with the `moz-phab`
//! feature.

use super::compared::{diff_rows, message_rows, row, said_rows};
use super::*;
use crate::submit::{Prepared, and};

/// What the bar says while a submit is read, or sent.
pub(super) const SUBMITTING: &str = "submitting";

/// A submit read, waiting on you: its dialog open, for a new revision; or
/// its review, on its screen.
pub struct Submitting {
    pub ready: Prepared,
    /// The review's rows, and what they were built for: the width they
    /// are folded to, and whether the code is side by side.
    rows: Vec<Row>,
    built: Option<(usize, bool)>,
    /// The line the cursor is on, and the first shown: of the rows,
    /// unified; of the lines pairing them, side by side.
    pub cursor: usize,
    top: usize,
    lines: usize,
    /// The code side by side, rather than unified.
    pub side: bool,
    /// The screen it was opened from, to go back to.
    back: Screen,
}

impl Submitting {
    fn new(ready: Prepared, back: Screen, side: bool) -> Submitting {
        Submitting {
            ready,
            rows: Vec::new(),
            built: None,
            cursor: 0,
            top: 0,
            lines: 0,
            side,
            back,
        }
    }

    /// What its rows read, each its parts one after another: for the tests.
    pub fn texts(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.text.clone()).collect()
    }
}

/// The rows of the review of `p`, folded to `w`: what is sent and where,
/// said, and that the code does not change when it does not; then only
/// what changes: the title and summary, Phabricator's then what is sent;
/// the parent and the bug, before then after, and the status; the code,
/// each file from Phabricator's latest diff to the patch.
fn review_rows(p: &Prepared, w: usize) -> Vec<Row> {
    let mut rows = vec![row(Kind::File, "what is sent")];
    let n = p.number();
    let id = p.revision.as_ref().map_or("the revision".to_string(), |r| r.id.clone());
    if !p.new_diff {
        said_rows(&mut rows, Kind::Meta, &format!("The code does not change: the latest diff of {id} is patch {n} as it is now."), w);
    }
    for s in p.said() {
        said_rows(&mut rows, Kind::Meta, &s, w);
    }
    // The title and summary, as an interdiff compares two messages.
    let before = p.message_before().unwrap_or_default();
    let after = p.message_after();
    if before != after {
        rows.push(row(Kind::Blank, ""));
        rows.push(row(Kind::File, "the title and summary: Phabricator's, then what is sent"));
        message_rows(&mut rows, &before, &after, w);
    }
    // What else changes, before then after; the status, always.
    rows.push(row(Kind::Blank, ""));
    rows.push(row(Kind::File, "what else changes"));
    let none = |l: &[String]| if l.is_empty() { "none".to_string() } else { and(l) };
    if p.parent_changes() {
        diff_rows(&mut rows, &format!("-parent: {}", none(&p.parents_now)), &[], w);
        diff_rows(&mut rows, &format!("+parent: {}", p.parent_after()), &[], w);
    }
    if let (Some(r), Some(b)) = (&p.revision, &p.bug)
        && r.bug != *b
    {
        let was = if r.bug.is_empty() { "none" } else { r.bug.as_str() };
        diff_rows(&mut rows, &format!("-bug: {was}"), &[], w);
        diff_rows(&mut rows, &format!("+bug: {b}"), &[], w);
    }
    match &p.revision {
        Some(r) if r.status != "changes-planned" => {
            diff_rows(&mut rows, &format!("-status: {}", p.status_was()), &[], w);
            diff_rows(&mut rows, "+status: Changes Planned", &[], w);
        }
        _ => diff_rows(&mut rows, &format!(" status: {}", p.status_change()), &[], w),
    }
    // The code, each file as both leave it.
    if let Some(code) = &p.code {
        rows.push(row(Kind::Blank, ""));
        rows.push(row(Kind::File, "the code: Phabricator's, then what is sent"));
        let said = format!("Each file as the latest diff of {id} leaves it, then as patch {n} leaves it, both on the patches below patch {n}.");
        said_rows(&mut rows, Kind::Meta, p.code_said.as_deref().unwrap_or(&said), w);
        rows.push(row(Kind::Blank, ""));
        rows.extend(code_rows(&p.change, code, w));
    }
    rows
}

/// `code`, a diff of change `change`'s files from one version to another,
/// as the rows the middle draws a patch's files in, folded to `w`: each
/// file's name, its hunks and their lines, what changed in a line marked.
/// Drawn only: no row is a line of a patch, so none takes a comment nor a
/// read mark, nor is found again as one.
pub(super) fn code_rows(change: &str, code: &str, w: usize) -> Vec<Row> {
    let code = Patch::new(change.to_string(), None, None, String::new(), code.to_string());
    let all = patch_rows(&code, &[], w, &Deco::default());
    // Its message's rows, empty, left out.
    let from = all.iter().position(|r| r.kind == Kind::Blank).map_or(0, |k| k + 1);
    all.into_iter()
        .skip(from)
        .map(|r| Row {
            kind: r.kind,
            text: r.text,
            cont: r.cont,
            words: r.words,
            ..Default::default()
        })
        .collect()
}

impl App {
    /// Why the patch at `at` is not to be submitted now, if it is not:
    /// HEAD, as last seen for the stack as it is, has its commit naming a
    /// revision the patch does not (`submit::held_by_head`). None when HEAD
    /// is elsewhere, or not known for the stack as it is: then nothing is
    /// guessed.
    pub(super) fn submit_held(&self, at: usize) -> Option<String> {
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        let seen = self.head.as_ref().filter(|h| h.name == self.stack && h.rev == rev && !h.stale)?;
        crate::submit::held_by_head(seen.head.as_ref()?, at)
    }

    /// Submit to Phabricator as WIP, from the patch's menu: what sending
    /// the patch picked sends, read away from the screen; nothing is sent.
    /// Not while its commit in the workspace names a revision it does not.
    pub(super) fn submit_patch(&mut self) {
        if self.bg.doing.contains(&SUBMITTING) {
            self.status = "a patch is being submitted to Phabricator already: submit this one once that is done".into();
            return;
        }
        let at = self.sel;
        if let Some(why) = self.submit_held(at) {
            self.status = format!("{why} Nothing was sent.");
            return;
        }
        self.bg.doing.push(SUBMITTING);
        self.status = format!("reading Phabricator to show what submitting patch {} sends: nothing is sent yet", at + 1);
        let (path, name) = (self.path.clone(), self.stack.clone());
        self.spawn(move || Done::Submitting(crate::submit::prepare(&path, &name, at).map(Box::new)));
    }

    /// What reading what is sent came to: the dialog of a new revision, the
    /// review of a revision there already; or, said, why nothing is sent,
    /// or that nothing is to be.
    pub(super) fn submit_read(&mut self, read: std::result::Result<Box<Prepared>, String>) {
        self.bg.doing.retain(|d| *d != SUBMITTING);
        let p = match read {
            Ok(p) => *p,
            Err(why) => {
                self.status = why;
                return;
            }
        };
        if p.nothing() {
            self.status = p.nothing_said();
            return;
        }
        let n = p.number();
        if p.stack != self.stack {
            self.status = format!(
                "what submitting patch {n} of {} sends was read, but you are on {} now, so nothing was sent: submit it again from its menu",
                p.stack, self.stack
            );
            return;
        }
        // Not over what is being written, typed or moved.
        if self.writing().is_some() || !matches!(self.mode, Mode::Normal | Mode::Menu(_) | Mode::Help { .. }) {
            self.status = format!(
                "what submitting patch {n} sends was read, but something is being written or moved, so nothing was sent: submit it again once that is done"
            );
            return;
        }
        let side = matches!(self.layout, Layout::Split | Layout::Compact);
        match &p.revision {
            None => {
                let items = vec![
                    Item::new("Send it to Phabricator", Action::SendSubmit),
                    Item::new("Cancel: send nothing", Action::CancelSubmit),
                ];
                let mut m = Menu::new(format!("send patch {n} to Phabricator as a new revision?"), items, None);
                m.note = Some(p.said().join("\n"));
                self.status.clear();
                self.mode = Mode::Menu(m);
                self.submitting = Some(Box::new(Submitting::new(p, self.screen, side)));
            }
            Some(_) => {
                // The bar says how to send it, and how not to.
                self.status.clear();
                self.mode = Mode::Normal;
                self.submitting = Some(Box::new(Submitting::new(p, self.screen, side)));
                self.screen = Screen::Submit;
            }
        }
    }

    /// The submit seen, sent, away from the screen: the revision read
    /// again first, and nothing sent if it changed.
    pub(super) fn submit_send(&mut self) {
        let Some(sub) = self.submitting.take() else { return };
        if self.screen == Screen::Submit {
            self.screen = sub.back;
        }
        if self.bg.doing.contains(&SUBMITTING) {
            self.status = "a patch is being submitted to Phabricator already, so nothing was sent: submit this one again once that is done".into();
            return;
        }
        // HEAD seen meanwhile with the patch's commit naming another
        // revision: not sent either.
        if sub.ready.stack == self.stack
            && let Some(why) = self.submit_held(sub.ready.at)
        {
            self.status = format!("{why} Nothing was sent.");
            return;
        }
        self.bg.doing.push(SUBMITTING);
        self.status = format!("sending patch {} to Phabricator…", sub.ready.number());
        // The commit it makes is the workspace's when HEAD of the checkout
        // stacked runs in has it.
        let (path, here) = (self.path.clone(), self.checkout.clone());
        self.spawn(move || Done::Said(SUBMITTING, crate::submit::send(&path, &sub.ready, &here)));
    }

    /// The submit left, nothing sent.
    pub(super) fn submit_cancel(&mut self) {
        if let Some(sub) = self.submitting.take()
            && self.screen == Screen::Submit
        {
            self.screen = sub.back;
        }
        self.status = "nothing was sent to Phabricator".into();
    }

    /// The code of the review side by side (`side`), or unified: from its
    /// top.
    pub(super) fn submit_side(&mut self, side: bool) {
        if let Some(sub) = self.submitting.as_mut() {
            (sub.side, sub.cursor, sub.top, sub.built) = (side, 0, 0, None);
        }
    }

    /// The review's own keys: ↑↓ and the rest scroll, Enter sends, Esc
    /// sends nothing, 1 puts the code side by side or unified again.
    pub(super) fn submit_key(&mut self, k: KeyEvent) -> Effect {
        let page = self.page();
        match k.code {
            KeyCode::Enter => return self.perform(Action::SendSubmit),
            KeyCode::Esc => return self.perform(Action::CancelSubmit),
            KeyCode::Char('1') => {
                let side = self.submitting.as_ref().is_some_and(|s| !s.side);
                return self.perform(Action::SubmitSide(side));
            }
            _ => {}
        }
        let Some(sub) = self.submitting.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let last = sub.lines.saturating_sub(1);
        sub.cursor = match k.code {
            KeyCode::Down => (sub.cursor + 1).min(last),
            KeyCode::Up => sub.cursor.saturating_sub(1),
            KeyCode::PageDown => (sub.cursor + page).min(last),
            KeyCode::PageUp => sub.cursor.saturating_sub(page),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => sub.cursor,
        };
        Effect::None
    }

    /// The review: its title, then its rows from the top shown, unified or
    /// side by side, the cursor's line lit.
    pub(super) fn draw_submit(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Some(sub) = self.submitting.as_mut() else { return };
        // Too narrow for two sides: unified meanwhile.
        let side = sub.side && w >= 63;
        let half = w.saturating_sub(3) / 2;
        let fold = if side { half } else { w };
        if sub.built != Some((fold, side)) {
            sub.rows = review_rows(&sub.ready, fold.saturating_sub(1));
            sub.built = Some((fold, side));
        }
        let r = sub.ready.revision.as_ref().map_or(String::new(), |r| format!("{}: ", r.id));
        put(buf, area, 0, 0, w, &fit(&format!(" {r}what is sent to Phabricator, for your review"), w), color::file());
        let (lines, _) = split_lines(&sub.rows);
        sub.lines = if side { lines.len() } else { sub.rows.len() };
        sub.cursor = sub.cursor.min(sub.lines.saturating_sub(1));
        let rows_h = h.saturating_sub(1).max(1);
        if sub.cursor < sub.top {
            sub.top = sub.cursor;
        } else if sub.cursor >= sub.top + rows_h {
            sub.top = sub.cursor + 1 - rows_h;
        }
        for (y, k) in (sub.top..sub.lines).take(rows_h).enumerate() {
            let lit = k == sub.cursor;
            if !side {
                put_parts(buf, area, 0, y + 1, w, &row_parts(&sub.rows[k], lit));
                continue;
            }
            let line = lines[k];
            if let Some(i) = line.span {
                put_parts(buf, area, 0, y + 1, w, &row_parts(&sub.rows[i], lit));
                continue;
            }
            put(buf, area, half + 1, y + 1, 1, "│", color::meta());
            for (x, idx) in [(0, line.left), (half + 2, line.right)] {
                match idx {
                    None => put(buf, area, x, y + 1, half, &fit("~", half), color::dim()),
                    Some(i) => put_parts(buf, area, x, y + 1, half, &row_parts(&sub.rows[i], lit)),
                }
            }
        }
    }

    /// What the review's bar says: how to send it, how to send nothing; on a
    /// line of the title and summary that has links, how to open them.
    pub(super) fn submit_hint(&self) -> String {
        let side = if self.submitting.as_ref().is_some_and(|s| s.side) { "1: unified" } else { "1: side by side" };
        let open = self.open_hint();
        format!("↑↓: scroll · enter: send it to Phabricator · esc: send nothing · {side} · {open}space: menu · ?: what can I do here")
    }

    /// The lines of the title and summary the review's cursor is on, each as
    /// it is written, its pieces joined and its sign taken off: one, or side
    /// by side, those of the two sides. None elsewhere in the review.
    pub(super) fn submit_message_lines(&self) -> Vec<String> {
        let Some(sub) = self.submitting.as_ref() else { return Vec::new() };
        let rows = &sub.rows;
        // Under their heading, as `review_rows` writes it, up to the empty
        // row after them; what is said over them (`message_rows`) aside.
        let heading = "the title and summary: Phabricator's, then what is sent";
        let Some(head) = rows.iter().position(|r| r.kind == Kind::File && r.text == heading) else { return Vec::new() };
        let end = rows[head..].iter().position(|r| r.kind == Kind::Blank).map_or(rows.len(), |k| head + k);
        let under: Vec<usize> = match sub.built {
            Some((_, true)) => split_lines(rows).0.get(sub.cursor).map_or(Vec::new(), |l| l.span.into_iter().chain(l.left).chain(l.right).collect()),
            _ => vec![sub.cursor],
        };
        let mut out: Vec<String> = Vec::new();
        for i in under.into_iter().filter(|&i| head < i && i < end && rows[i].kind != Kind::Meta) {
            let Some(first) = (head + 1..=i).rev().find(|&k| !rows[k].cont) else { continue };
            let last = (first + 1..end).find(|&k| !rows[k].cont).unwrap_or(end);
            let line: String = rows[first..last].iter().map(|r| r.text.strip_prefix(crate::rows::FOLDED).filter(|_| r.cont).unwrap_or(r.text.as_str())).collect();
            let mut text = line.chars();
            text.next();
            let text = text.as_str().to_string();
            if !out.contains(&text) {
                out.push(text);
            }
        }
        out
    }

    /// What `?` says on the review.
    pub(super) fn submit_help(&self) -> Vec<String> {
        let id = self.submitting.as_ref().and_then(|s| s.ready.revision.as_ref()).map_or("the revision".to_string(), |r| r.id.clone());
        vec![
            format!(
                "This is what is sent to Phabricator, for you to review before it is: at the top, what is sent and where; then, of what changes, the title and summary, Phabricator's then what is sent; the parent and the bug, before then after, and the status; and the code, each file as the latest diff of {id} leaves it, then as the patch leaves it, both on the patches below it."
            ),
            format!("Press Enter to send it: {id} is read again first, and if it changed on Phabricator since this was read, nothing is sent. Press Esc to send nothing."),
            "Press ↑↓ (pgup pgdn home end) to scroll, and 1 to show the code side by side, or unified again.".into(),
        ]
    }

    /// The review's menu: send it, or send nothing; the code side by side,
    /// or unified.
    pub(super) fn submit_items(&self, items: &mut Vec<Item>) {
        items.push(Item::new("Send it to Phabricator", Action::SendSubmit));
        items.push(Item::new("Cancel: send nothing", Action::CancelSubmit));
        items.push(Item::line());
        match self.submitting.as_ref().is_some_and(|s| s.side) {
            true => items.push(Item::new("Show the code unified", Action::SubmitSide(false))),
            false => items.push(Item::new("Show the code side by side", Action::SubmitSide(true))),
        }
    }
}
