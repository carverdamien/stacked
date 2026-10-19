//! The review of what the agent brought back. On the left, its parts, in
//! three sections: the stack operations its change makes, a line for each
//! patch; the comments it opened; its replies. On the right, the part the
//! cursor is on as it would be: the stack diff of the change, or the
//! comment in its place, only to look at. Each part is accepted or
//! rejected, the change of the stack as one; then the review is saved to go
//! on later, thrown away, or closed, what was accepted applied.

use std::collections::BTreeMap;

use super::*;
use crate::db::Run;
use crate::held::{Held as Work, Part};
use crate::history::Diffed;

/// A review being made: of what run `run` brought back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reviewing {
    pub run: String,
    /// Each part marked: accepted, or rejected. As saved, and since.
    pub marks: BTreeMap<String, bool>,
    /// The comments' marks when the change of the stack was rejected,
    /// rejecting them with it: they come back if it is accepted again.
    before: Option<BTreeMap<String, bool>>,
    /// The line of the list the cursor is on, and the first one shown.
    pub sel: usize,
    top: usize,
    /// Whether the cursor is in the other pane, to scroll it, and how far.
    pub in_main: bool,
    scroll: usize,
}

/// A line of the review's list: what it says, how, and the part it is of
/// (none, a section's title); for the change's, its place in the diff.
pub(super) struct Line {
    text: String,
    style: Style,
    part: Option<Part>,
    entry: Option<usize>,
}

/// What a part's mark says, as the list shows it.
fn mark_of(m: Option<bool>) -> (&'static str, Style) {
    match m {
        Some(true) => ("✓", color::add()),
        Some(false) => ("✗", color::del()),
        None => ("·", Style::new()),
    }
}

/// A line of the stack diff, as one of the stack operations: what is done
/// to each patch.
fn operation(d: &Diffed) -> Option<(String, Style)> {
    Some(match d {
        Diffed::Base { what, was, is } => (format!("{what}  {was} → {is}"), color::hunk()),
        Diffed::Patch { at, title, added: true, .. } => (format!("add     {at} {title}"), color::add()),
        Diffed::Patch {
            at, title, changed: true, was, ..
        } => (format!("modify  {at} {title}{}", was.map_or(String::new(), |w| format!(", from {w}"))), color::hunk()),
        Diffed::Patch { at, title, was: Some(w), .. } => (format!("move    {at} {title}, from {w}"), Style::new()),
        Diffed::Patch { at, title, .. } => (format!("noop    {at} {title}"), color::dim()),
        Diffed::Dropped { was, title } => (format!("remove  {was} {title}"), color::del()),
        _ => return None,
    })
}

/// A comment's first words.
fn first_words(text: &str) -> String {
    thread_turns(text).first().map(|t| t.body.lines().next().unwrap_or("").to_string()).unwrap_or_default()
}

impl App {
    /// What run `run` brought back.
    fn work(&self, run: &str) -> Option<(&Run, &Work)> {
        let r = self.db.run(run)?;
        Some((r, r.held.as_ref()?))
    }

    /// The parts of what is reviewed still to resolve: the replies taken
    /// already left out.
    fn review_parts(&self) -> Vec<Part> {
        let Some(rv) = &self.reviewing else { return vec![] };
        let Some((_, h)) = self.work(&rv.run) else { return vec![] };
        let taken = self.db.review(&rv.run).map(|v| v.taken.clone()).unwrap_or_default();
        h.parts().into_iter().filter(|p| !matches!(p, Part::Reply(k) if taken.contains(k))).collect()
    }

    /// Opens the review of what run `run` brought back, with its marks as
    /// last saved; the cursor on `part`, else on the first not marked.
    pub(super) fn open_review(&mut self, run: &str, part: Option<Part>) {
        let Some(_) = self.work(run) else { return };
        let marks = self.db.review(run).map(|v| v.marks.clone()).unwrap_or_default();
        self.reviewing = Some(Reviewing {
            run: run.into(),
            marks,
            ..Default::default()
        });
        self.screen = Screen::Review;
        self.on_banner = false;
        let lines = self.review_lines();
        let marks = self.reviewing.as_ref().map(|r| r.marks.clone()).unwrap_or_default();
        let at = match part {
            Some(p) => lines.iter().position(|l| l.part == Some(p)),
            None => lines.iter().position(|l| l.part.is_some_and(|p| !marks.contains_key(&p.key()))),
        };
        let at = at.or_else(|| lines.iter().position(|l| l.part.is_some())).unwrap_or(0);
        if let Some(rv) = &mut self.reviewing {
            rv.sel = at;
        }
    }

    /// The stack as it is, and as accepting the change would make it.
    fn review_stacks(&self) -> Option<(Stack, Stack)> {
        let rv = self.reviewing.as_ref()?;
        let (r, h) = self.work(&rv.run)?;
        let c = h.stack.as_ref()?;
        let now = self.db.stack(&r.stack).ok()?.clone();
        let next = Stack {
            patches: c.patches.clone(),
            base: c.base.clone().or(now.base.clone()),
            ..now.clone()
        };
        Some((now, next))
    }

    /// The review's list: its three sections, each part a line, the change
    /// of the stack a line for each patch.
    pub(super) fn review_lines(&self) -> Vec<Line> {
        let mut out = Vec::new();
        let Some(rv) = &self.reviewing else { return out };
        let Some((_, h)) = self.work(&rv.run) else { return out };
        let parts = self.review_parts();
        let mark = |p: Part| rv.marks.get(&p.key()).copied();
        let title = |text: &str| Line {
            text: text.into(),
            style: color::file(),
            part: None,
            entry: None,
        };
        if let Some((now, next)) = self.review_stacks() {
            let said = match mark(Part::Stack) {
                Some(true) => "accepted",
                Some(false) => "rejected",
                None => "to review",
            };
            out.push(Line {
                text: format!(" stack operations · {said}"),
                style: color::file(),
                part: Some(Part::Stack),
                entry: None,
            });
            let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), false);
            for (k, d) in diff.iter().enumerate() {
                if let Some((text, style)) = operation(d) {
                    out.push(Line {
                        text: format!("   {text}"),
                        style,
                        part: Some(Part::Stack),
                        entry: Some(k),
                    });
                }
            }
        }
        let opened: Vec<Part> = parts.iter().copied().filter(|p| matches!(p, Part::Opened(_))).collect();
        if !opened.is_empty() {
            out.push(title(" comments opened"));
            for p in opened {
                let Part::Opened(k) = p else { continue };
                let n = &h.opened[k];
                let (m, st) = mark_of(mark(p));
                out.push(Line {
                    text: format!("   {m} {}: {} · {}", n.file, n.preview, first_words(&n.text)),
                    style: st,
                    part: Some(p),
                    entry: None,
                });
            }
        }
        let replies: Vec<Part> = parts.iter().copied().filter(|p| matches!(p, Part::Reply(_))).collect();
        if !replies.is_empty() {
            out.push(title(" replies"));
            for p in replies {
                let Part::Reply(k) = p else { continue };
                let rp = &h.replies[k];
                let at = match self.db.note(&rp.change, &rp.id) {
                    Some(n) if n.on_stack() => "the stack".to_string(),
                    Some(n) => format!("{}: {}", n.file, n.preview),
                    None => "a comment deleted".to_string(),
                };
                let (m, st) = mark_of(mark(p));
                out.push(Line {
                    text: format!("   {m} {at} · {}", rp.text.lines().next().unwrap_or("")),
                    style: st,
                    part: Some(p),
                    entry: None,
                });
            }
        }
        out
    }

    /// What the part the cursor is on would be, `w` wide: the stack diff of
    /// the change, or the comment in its place; and the line to show
    /// first.
    fn review_view(&self, line: Option<&Line>, w: usize) -> (String, Vec<(String, Style)>, usize) {
        let Some((rv, line)) = self.reviewing.as_ref().zip(line) else { return (String::new(), vec![], 0) };
        let Some((r, h)) = self.work(&rv.run) else { return (String::new(), vec![], 0) };
        match line.part {
            Some(Part::Stack) | None => {
                let Some((now, next)) = self.review_stacks() else {
                    return ("the agent changed nothing of the stack".into(), vec![], 0);
                };
                let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), true);
                let mut lines = Vec::new();
                let mut first = 0;
                for (k, (text, st)) in diff_lines(&diff).into_iter().enumerate() {
                    if line.entry == Some(k) {
                        first = lines.len();
                    }
                    lines.extend(fold(&text, w.saturating_sub(1), 6).into_iter().map(|l| (l, st)));
                }
                let of = h.stack.as_ref().map_or(String::new(), |c| c.of.clone());
                (format!(" the change of {of}: what accepting it does to the stack"), lines, first)
            }
            Some(p) => {
                let (n, said) = match p {
                    Part::Reply(k) => {
                        let rp = &h.replies[k];
                        let Some(n) = self.db.note(&rp.change, &rp.id) else {
                            return ("its comment was deleted: nothing to answer".into(), vec![], 0);
                        };
                        let text = append_turn(&n.text, Role::Agent, &rp.text, Some(&rp.tag));
                        let said = match &rp.promote {
                            Some(why) => format!(" its reply, as it would be in its thread; then it moves to the stack's comments: {why}"),
                            None => " its reply, as it would be in its thread".to_string(),
                        };
                        (Note { text, ..n.clone() }, said)
                    }
                    Part::Opened(k) => (h.opened[k].clone(), " a comment it opened, where it would be".to_string()),
                    Part::Stack => unreachable!(),
                };
                let deco = Deco::default();
                let rows = if n.on_stack() {
                    let Ok(s) = self.db.stack(&r.stack) else { return (said, vec![], 0) };
                    let titles: Vec<String> = s.patches.iter().map(|id| self.db.patch(id).title().to_string()).collect();
                    stack_rows(&format!("the stack {}", r.stack), &titles, &[&n], w, &deco)
                } else {
                    let p = self
                        .db
                        .stack(&r.stack)
                        .ok()
                        .and_then(|s| s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == n.change));
                    let Some(p) = p else { return (said, vec![], 0) };
                    patch_rows(p, &[&n], w, &deco)
                };
                let head = rows.iter().position(|row| row.head && row.note.as_deref() == Some(&n.id)).unwrap_or(0);
                let lines = rows.iter().map(|row| (row.text.clone(), row_style(row, false))).collect();
                (said, lines, head)
            }
        }
    }

    /// The review: its list on the left, what the cursor's part would be on
    /// the right.
    pub(super) fn draw_review(&mut self, buf: &mut Buffer, body: Rect) {
        let (w, h) = (body.width as usize, body.height as usize);
        let lines = self.review_lines();
        let Some(rv) = self.reviewing.clone() else { return };
        let list_w = if w < 60 { w } else { (w * 2 / 5).max(30) };
        let list = Rect::new(body.x, body.y, list_w as u16, body.height);
        let title = self.work(&rv.run).map_or(String::new(), |(r, h)| {
            let asked = if r.request.is_empty() { String::new() } else { format!(" · asked \"{}\"", r.request) };
            format!(" the agent's work: {}{asked}", h.said())
        });
        put(buf, list, 0, 0, list_w, &fit(&title, list_w), color::header());
        let rows = h.saturating_sub(1);
        let mut top = rv.top;
        if rv.sel < top {
            top = rv.sel;
        } else if rv.sel >= top + rows {
            top = rv.sel + 1 - rows;
        }
        for (y, (i, l)) in lines.iter().enumerate().skip(top).take(rows).enumerate() {
            let mut st = l.style;
            if i == rv.sel {
                st = st.add_modifier(if rv.in_main { Modifier::BOLD } else { Modifier::REVERSED });
            }
            put(buf, list, 0, y + 1, list_w, &fit(&l.text, list_w), st);
            spot_in(&self.spots, list, (0, y + 1, list_w, 1), Spot::ReviewLine(i));
        }
        if let Some(r) = &mut self.reviewing {
            r.top = top;
        }
        if w < 60 {
            return;
        }
        for y in 0..body.height {
            buf.set_string(body.x + list_w as u16, body.y + y, "│", color::meta());
        }
        let main_w = w - list_w - 1;
        let main = Rect::new(body.x + (list_w + 1) as u16, body.y, main_w as u16, body.height);
        let (said, view, first) = self.review_view(lines.get(rv.sel), main_w.saturating_sub(1));
        let mut head = color::header();
        if rv.in_main {
            head = head.add_modifier(Modifier::UNDERLINED);
        }
        put(buf, main, 0, 0, main_w, &fit(&said, main_w), head);
        // Where the part is, unless scrolled by hand.
        let scroll = if rv.in_main { rv.scroll } else { first.saturating_sub(rows / 3) };
        let scroll = scroll.min(view.len().saturating_sub(1));
        if let Some(r) = &mut self.reviewing {
            r.scroll = scroll;
        }
        for (y, (l, st)) in view.iter().skip(scroll).take(rows).enumerate() {
            put(buf, main, 0, y + 1, main_w, &fit(l, main_w), *st);
        }
    }

    /// A key in the review: ↑↓ through its parts (or the other pane, → in
    /// it, ← out), Enter to accept or reject, Esc to save, throw away or
    /// close it.
    pub(super) fn review_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.review_lines();
        let page = self.page();
        let Some(rv) = self.reviewing.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let parts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.part.is_some()).map(|(i, _)| i).collect();
        let step = |from: usize, down: bool, n: usize| -> usize {
            let mut at = parts.iter().position(|&i| i == from).unwrap_or(0);
            for _ in 0..n {
                at = if down { (at + 1).min(parts.len().saturating_sub(1)) } else { at.saturating_sub(1) };
            }
            parts.get(at).copied().unwrap_or(from)
        };
        match (rv.in_main, k.code) {
            (_, KeyCode::Esc) => {
                self.mode = Mode::Menu(self.review_dialog());
            }
            (_, KeyCode::Enter) => {
                if let Some(p) = lines.get(rv.sel).and_then(|l| l.part) {
                    let accept = rv.marks.get(&p.key()) != Some(&true);
                    self.mark(p, accept);
                }
            }
            (false, KeyCode::Right) if self.body_w >= 60 => rv.in_main = true,
            (true, KeyCode::Left) => rv.in_main = false,
            (false, code) => {
                rv.sel = match code {
                    KeyCode::Down => step(rv.sel, true, 1),
                    KeyCode::Up => step(rv.sel, false, 1),
                    KeyCode::PageDown => step(rv.sel, true, page),
                    KeyCode::PageUp => step(rv.sel, false, page),
                    KeyCode::Home => parts.first().copied().unwrap_or(0),
                    KeyCode::End => parts.last().copied().unwrap_or(0),
                    _ => rv.sel,
                };
            }
            (true, code) => {
                rv.scroll = match code {
                    KeyCode::Down => rv.scroll + 1,
                    KeyCode::Up => rv.scroll.saturating_sub(1),
                    KeyCode::PageDown => rv.scroll + page,
                    KeyCode::PageUp => rv.scroll.saturating_sub(page),
                    KeyCode::Home => 0,
                    _ => rv.scroll,
                };
            }
        }
        Effect::None
    }

    /// Part `p` marked accepted, or rejected. The change of the stack
    /// rejected rejects the comments of the work with it; accepted again,
    /// they are as they were. A comment is not accepted while the change it
    /// goes with is rejected.
    pub(super) fn mark(&mut self, p: Part, accept: bool) {
        let comments: Vec<Part> = self.review_parts().into_iter().filter(|p| *p != Part::Stack).collect();
        let Some(rv) = self.reviewing.as_mut() else { return };
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
        if p == Part::Stack {
            if !accept && !rejected {
                rv.before = Some(comments.iter().filter_map(|c| Some((c.key(), *rv.marks.get(&c.key())?))).collect());
                for c in &comments {
                    rv.marks.insert(c.key(), false);
                }
            }
            if accept && rejected {
                let before = rv.before.take().unwrap_or_default();
                for c in &comments {
                    match before.get(&c.key()) {
                        Some(m) => rv.marks.insert(c.key(), *m),
                        None => rv.marks.remove(&c.key()),
                    };
                }
            }
        } else if accept && rejected {
            self.status = "the agent's change of the stack is rejected, and the comments with it: accept the change first".into();
            return;
        }
        rv.marks.insert(p.key(), accept);
    }

    /// Every comment of the review marked accepted, or rejected.
    pub(super) fn mark_comments(&mut self, accept: bool) {
        for p in self.review_parts().into_iter().filter(|p| *p != Part::Stack) {
            self.mark(p, accept);
        }
    }

    /// What is marked, by kind of part: a line for each.
    pub(super) fn review_summary(&self) -> String {
        let Some(rv) = &self.reviewing else { return String::new() };
        let parts = self.review_parts();
        let mark = |p: &Part| rv.marks.get(&p.key()).copied();
        let mut out = Vec::new();
        if parts.contains(&Part::Stack) {
            out.push(format!(
                "the change of the stack: {}",
                match mark(&Part::Stack) {
                    Some(true) => "accepted",
                    Some(false) => "rejected",
                    None => "to review",
                }
            ));
        }
        for (what, of) in [("comments opened", 0), ("replies", 1)] {
            let these: Vec<&Part> = parts.iter().filter(|p| matches!((of, p), (0, Part::Opened(_)) | (1, Part::Reply(_)))).collect();
            if these.is_empty() {
                continue;
            }
            let count = |m: Option<bool>| these.iter().filter(|p| mark(p) == m).count();
            let mut said = Vec::new();
            for (n, word) in [(count(Some(true)), "accepted"), (count(Some(false)), "rejected"), (count(None), "to review")] {
                if n > 0 {
                    said.push(format!("{n} {word}"));
                }
            }
            out.push(format!("{what}: {}", said.join(", ")));
        }
        out.join("\n")
    }

    /// What Esc asks in the review: what is marked, said; then save the
    /// marks, throw away those made since it was opened, or close it.
    pub(super) fn review_dialog(&self) -> Menu {
        let parts = self.review_parts();
        let done = self.reviewing.as_ref().is_some_and(|rv| parts.iter().all(|p| rv.marks.contains_key(&p.key())));
        let items = vec![
            Item::new("Save", Action::ReviewSave),
            Item::new("Discard", Action::ReviewDiscard),
            Item::new("Close review", Action::ReviewClose(None)).when(done),
            Item::new("Accept all remaining and Close", Action::ReviewClose(Some(true))),
            Item::new("Reject all remaining and Close", Action::ReviewClose(Some(false))),
            Item::line(),
            Item::new("Keep reviewing", Action::Cancel),
        ];
        let mut m = Menu::new("your review of the agent's work", items, None);
        m.note = Some(self.review_summary());
        m
    }

    /// The menu of the part the cursor is on, in the review.
    pub(super) fn review_items(&self, items: &mut Vec<Item>) {
        let lines = self.review_lines();
        let Some(rv) = &self.reviewing else { return };
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
        if let Some(p) = lines.get(rv.sel).and_then(|l| l.part) {
            let may_accept = p == Part::Stack || !rejected;
            match rv.marks.get(&p.key()) {
                Some(true) => items.push(Item::new("Mark as rejected", Action::MarkPart(false))),
                Some(false) => items.push(Item::new("Mark as accepted", Action::MarkPart(true)).when(may_accept)),
                None => {
                    items.push(Item::new("Mark as accepted", Action::MarkPart(true)).when(may_accept));
                    items.push(Item::new("Mark as rejected", Action::MarkPart(false)));
                }
            }
        }
        let comments = self.review_parts().iter().any(|p| *p != Part::Stack);
        items.push(Item::new("Mark all comments accepted", Action::MarkComments(true)).when(comments && !rejected));
        items.push(Item::new("Mark all comments rejected", Action::MarkComments(false)).when(comments));
    }

    /// The part the cursor is on, in the review.
    pub(super) fn review_part(&self) -> Option<Part> {
        let rv = self.reviewing.as_ref()?;
        self.review_lines().get(rv.sel).and_then(|l| l.part)
    }

    /// Leaves the review, saved or not, for the main screen.
    pub(super) fn leave_review(&mut self) {
        self.reviewing = None;
        self.screen = Screen::Main;
    }

    /// The review's marks saved, to go on from them later.
    pub(super) fn review_save(&mut self) {
        let Some(rv) = self.reviewing.clone() else { return };
        if self.edit(|db| crate::held::save(db, &rv.run, &rv.marks)).is_some() {
            self.status = "your marks are saved: the review goes on from them when you open it again".into();
            self.leave_review();
        }
    }

    /// The review closed: what is accepted applied, the rest dropped; the
    /// parts not marked accepted or rejected as `rest` says.
    pub(super) fn review_close(&mut self, rest: Option<bool>) {
        let Some(rv) = self.reviewing.clone() else { return };
        if let Some(said) = self.edit(|db| crate::held::close(db, &rv.run, &rv.marks, rest)) {
            self.status = format!("reviewed: {said}");
            self.leave_review();
            // What waited on the review may go.
            self.launch();
        }
    }

    /// The landed asks waiting for review on this stack, the first landed
    /// first: their runs.
    pub(super) fn queue(&self) -> Vec<String> {
        crate::held::queue(&self.db, &self.stack).into_iter().map(|r| r.id.clone()).collect()
    }

    /// What the banner says: how much landed work waits for review, and
    /// which the next is.
    pub(super) fn banner(&self) -> Option<String> {
        let q = self.queue();
        let (r, h) = self.work(q.first()?)?;
        let n = q.len();
        let head = if n == 1 { "1 landed ask waits for your review".to_string() } else { format!("{n} landed asks wait for your review") };
        let asked = if r.request.is_empty() { String::new() } else { format!(", asked \"{}\"", r.request) };
        Some(format!(" ⟳ {head} · the next: {}{asked}", h.said()))
    }

    /// The banner's menu: each landed ask, to review.
    pub(super) fn banner_items(&self, items: &mut Vec<Item>) {
        for id in self.queue() {
            let Some((r, h)) = self.work(&id) else { continue };
            let asked = if r.request.is_empty() { String::new() } else { format!(", asked \"{}\"", short(&r.request, 40)) };
            items.push(Item::new(format!("Review {}{asked}", h.said()), Action::Review(id.clone(), None)));
        }
    }
}
