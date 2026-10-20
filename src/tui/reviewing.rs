//! The review of what landed: what the agent brought back, or the reviews
//! a pull brought from Phabricator. On the left, its parts, in
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
    /// Whether the cursor is in the other pane, to scroll it, and how far;
    /// and whether that pane is wide, the list out of sight.
    pub in_main: bool,
    scroll: usize,
    pub wide: bool,
    /// The replies' marks when the comment they answer was marked to close,
    /// accepting them with it: they come back if it is kept again.
    asked_before: BTreeMap<String, Option<bool>>,
}

/// A line of the review's list: what it says, how, and the part it is of
/// (none, a section's title); for the change's, its place in the diff.
pub(super) struct Line {
    text: String,
    style: Style,
    part: Option<Part>,
    entry: Option<usize>,
    /// The comment the ask was about it is, by its place among them: kept,
    /// or closed with the review.
    asking: Option<usize>,
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
pub(super) fn first_words(text: &str) -> String {
    thread_turns(text).first().map(|t| t.body.lines().next().unwrap_or("").to_string()).unwrap_or_default()
}

impl App {
    /// What run `run` brought back.
    pub(super) fn work(&self, run: &str) -> Option<(&Run, &Work)> {
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
    /// last saved; the cursor on `part`, else where it was when the review
    /// was left, else on the first not marked.
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
        let was = self.review_was(run).filter(|&k| lines.get(k).is_some_and(|l| l.part.is_some()));
        let at = match part {
            Some(p) => lines.iter().position(|l| l.part == Some(p)),
            None => was.or_else(|| lines.iter().position(|l| l.part.is_some_and(|p| !marks.contains_key(&p.key())))),
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

    /// The comments the ask of what is reviewed was about, still open, each
    /// with its place among them.
    fn asking_open(&self) -> Vec<(usize, Note)> {
        let Some((r, _)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return vec![] };
        crate::held::asking_of(r)
            .into_iter()
            .enumerate()
            .filter_map(|(k, (c, i))| Some((k, self.db.note(&c, &i)?.clone())))
            .collect()
    }

    /// The review's list: on top, the comments that asked, each kept or
    /// closed with the review; then its three sections, each part a line,
    /// the change of the stack a line for each patch.
    pub(super) fn review_lines(&self) -> Vec<Line> {
        let mut out = Vec::new();
        let Some(rv) = &self.reviewing else { return out };
        let Some((_, h)) = self.work(&rv.run) else { return out };
        let asking = self.asking_open();
        if !asking.is_empty() {
            out.push(Line {
                text: (if asking.len() == 1 { " asking comment" } else { " asking comments" }).into(),
                style: color::file(),
                part: None,
                entry: None,
                asking: None,
            });
            for (k, n) in asking {
                let at = if n.on_stack() { "the stack".to_string() } else { format!("{}: {}", n.file, n.preview) };
                let (m, st) = match rv.marks.get(&crate::held::asking_key(k)) {
                    Some(true) => ("close", color::del()),
                    _ => ("keep ", Style::new()),
                };
                out.push(Line {
                    text: format!("   {m} {at} · {}", first_words(&n.text)),
                    style: st,
                    part: None,
                    entry: None,
                    asking: Some(k),
                });
            }
        }
        let parts = self.review_parts();
        let mark = |p: Part| rv.marks.get(&p.key()).copied();
        let title = |text: &str| Line {
            text: text.into(),
            style: color::file(),
            part: None,
            entry: None,
            asking: None,
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
                asking: None,
            });
            let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), false);
            for (k, d) in diff.iter().enumerate() {
                if let Some((text, style)) = operation(d) {
                    out.push(Line {
                        text: format!("   {text}"),
                        style,
                        part: Some(Part::Stack),
                        entry: Some(k),
                        asking: None,
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
                    asking: None,
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
                    asking: None,
                });
            }
        }
        out
    }

    /// What the part the cursor is on would be, `w` wide: the stack diff of
    /// the change, or the comment in its place; and the line to show
    /// first.
    fn review_view(&self, line: Option<&Line>, w: usize) -> (String, Vec<Vec<(String, Style)>>, usize) {
        let Some((rv, line)) = self.reviewing.as_ref().zip(line) else { return (String::new(), vec![], 0) };
        let Some((r, h)) = self.work(&rv.run) else { return (String::new(), vec![], 0) };
        // A comment that asked: as it is now.
        if let Some(k) = line.asking
            && let Some((_, n)) = self.asking_open().into_iter().find(|(j, _)| *j == k)
        {
            let said = match rv.marks.get(&crate::held::asking_key(k)) {
                Some(true) => " the comment that asked, as it is: closed with the review",
                _ => " the comment that asked, as it is: kept open",
            };
            return self.comment_view(r, &n, said.into(), w);
        }
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
                    lines.extend(fold(&text, w.saturating_sub(1), 6).into_iter().map(|l| vec![(l, st)]));
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
                        let text = rp.after(&n.text);
                        let said = match &rp.promote {
                            Some(why) => format!(" its reply, as it would be in its thread; then it moves to the stack's comments: {why}"),
                            None => " its reply, as it would be in its thread".to_string(),
                        };
                        (Note { text, ..n.clone() }, said)
                    }
                    Part::Opened(k) if crate::held::pulled(r) => (h.opened[k].clone(), " a reviewer's comment, where it would be".to_string()),
                    Part::Opened(k) => (h.opened[k].clone(), " a comment it opened, where it would be".to_string()),
                    Part::Stack => unreachable!(),
                };
                self.comment_view(r, &n, said, w)
            }
        }
    }

    /// Comment `n` of the work of run `r` where it is, `w` wide, `said`
    /// above it; and the line to show first, its own.
    fn comment_view(&self, r: &Run, n: &Note, said: String, w: usize) -> (String, Vec<Vec<(String, Style)>>, usize) {
        let deco = Deco::default();
        let rows = if n.on_stack() {
            let Ok(s) = self.db.stack(&r.stack) else { return (said, vec![], 0) };
            let titles: Vec<String> = s.patches.iter().map(|id| self.db.patch(id).title().to_string()).collect();
            stack_rows(&format!("the stack {}", r.stack), &titles, &[n], w, &deco)
        } else {
            let p = self
                .db
                .stack(&r.stack)
                .ok()
                .and_then(|s| s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == n.change));
            let Some(p) = p else { return (said, vec![], 0) };
            patch_rows(p, &[n], w, &deco)
        };
        let head = rows.iter().position(|row| row.head && row.note.as_deref() == Some(&n.id)).unwrap_or(0);
        let lines = rows.iter().map(|row| row_parts(row, false)).collect();
        (said, lines, head)
    }

    /// The review: its list on the left, what the cursor's part would be on
    /// the right.
    pub(super) fn draw_review(&mut self, buf: &mut Buffer, body: Rect) {
        let (w, h) = (body.width as usize, body.height as usize);
        let lines = self.review_lines();
        let Some(rv) = self.reviewing.clone() else { return };
        let list_w = if w < 60 { w } else { (w * 2 / 5).max(30) };
        let list = Rect::new(body.x, body.y, list_w as u16, body.height);
        // Wide, the part the cursor is on has it all, the list out of sight.
        let wide = rv.in_main && rv.wide;
        let rows = h.saturating_sub(1);
        if !wide {
            let title = self.work(&rv.run).map_or(String::new(), |(r, h)| {
                if crate::held::pulled(r) {
                    return format!(" pulled from Phabricator: {}", h.said());
                }
                if crate::held::by_hand(r) {
                    return format!(" your edit by hand: {} · {}", h.said(), h.edits.join(", then "));
                }
                let asked = if r.request.is_empty() { String::new() } else { format!(" · asked \"{}\"", r.request) };
                let edited = if h.edits.is_empty() { String::new() } else { format!(" · then you {}", h.edits.join(", then ")) };
                format!(" the agent's work: {}{asked}{edited}", h.said())
            });
            put(buf, list, 0, 0, list_w, &fit(&title, list_w), color::header());
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
        }
        let (main_x, main_w) = if wide { (0, w) } else { (list_w + 1, w - list_w - 1) };
        let main = Rect::new(body.x + main_x as u16, body.y, main_w as u16, body.height);
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
        for (y, parts) in view.iter().skip(scroll).take(rows).enumerate() {
            put_parts(buf, main, 0, y + 1, main_w, parts);
        }
    }

    /// A key in the review: ↑↓ through its parts (or the other pane, → in
    /// it, ← out), Enter to accept, or put back to review what is accepted
    /// (Space's menu rejects), Esc to save, throw away or close it.
    pub(super) fn review_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.review_lines();
        let page = self.page();
        let Some(rv) = self.reviewing.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let parts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.part.is_some() || l.asking.is_some()).map(|(i, _)| i).collect();
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
                // A comment that asked: closed with the review, or kept.
                if let Some(k) = lines.get(rv.sel).and_then(|l| l.asking) {
                    let close = rv.marks.get(&crate::held::asking_key(k)) != Some(&true);
                    self.mark_asking(k, close);
                } else if let Some(p) = lines.get(rv.sel).and_then(|l| l.part) {
                    match rv.marks.get(&p.key()) {
                        Some(true) => {
                            rv.marks.remove(&p.key());
                        }
                        _ => self.mark(p, true),
                    }
                }
            }
            // → into the pane on the right, to scroll it; → again, wide, the
            // list out of sight; ← back. Too narrow for both side by side,
            // it is wide at once.
            (false, KeyCode::Right) => {
                rv.in_main = true;
                rv.wide = self.body_w < 60;
            }
            (true, KeyCode::Right) => rv.wide = true,
            (true, KeyCode::Left) if rv.wide && self.body_w >= 60 => rv.wide = false,
            (true, KeyCode::Left) => {
                rv.in_main = false;
                rv.wide = false;
            }
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

    /// The comment the ask was about at place `k` marked to close with the
    /// review (`close`), or to keep. Closed, its replies go with it: they
    /// are accepted (while the change they go with is not rejected), and
    /// are as they were once it is kept again.
    pub(super) fn mark_asking(&mut self, k: usize, close: bool) {
        let Some((c, i)) = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run)).and_then(|r| crate::held::asking_of(r).get(k).cloned()) else {
            return;
        };
        let parts = self.review_parts();
        let Some((_, h)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return };
        let replies: Vec<Part> = h
            .replies
            .iter()
            .enumerate()
            .filter(|(_, rp)| rp.change == c && rp.id == i)
            .map(|(j, _)| Part::Reply(j))
            .filter(|p| parts.contains(p))
            .collect();
        let Some(rv) = self.reviewing.as_mut() else { return };
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
        if close {
            rv.marks.insert(crate::held::asking_key(k), true);
            if !rejected {
                for p in &replies {
                    let was = rv.marks.get(&p.key()).copied();
                    rv.asked_before.entry(p.key()).or_insert(was);
                    rv.marks.insert(p.key(), true);
                }
            }
        } else {
            rv.marks.remove(&crate::held::asking_key(k));
            for p in &replies {
                match rv.asked_before.remove(&p.key()) {
                    Some(Some(m)) => {
                        rv.marks.insert(p.key(), m);
                    }
                    Some(None) => {
                        rv.marks.remove(&p.key());
                    }
                    None => {}
                }
            }
        }
    }

    /// The review's keys, where the cursor is: the list, or the pane on its
    /// right, wide or not.
    pub(super) fn review_hint(&self) -> String {
        let enter = self.review_enter();
        match &self.reviewing {
            Some(rv) if rv.in_main && rv.wide => format!("↑↓: scroll · {enter} · ←: narrower · esc: save, discard or close"),
            Some(rv) if rv.in_main => format!("↑↓: scroll · {enter} · →: wider · ←: back to the list · esc: save, discard or close"),
            _ => format!("↑↓ choose · {enter} · →: look closer · space: menu · esc: save, discard or close"),
        }
    }

    /// What Enter does on the review's line the cursor is on, said.
    pub(super) fn review_enter(&self) -> &'static str {
        let Some(rv) = &self.reviewing else { return "enter: accept" };
        let lines = self.review_lines();
        let Some(line) = lines.get(rv.sel) else { return "enter: accept" };
        match (line.asking, line.part) {
            (Some(k), _) if rv.marks.get(&crate::held::asking_key(k)) == Some(&true) => "enter: keep it open",
            (Some(_), _) => "enter: close it with the review",
            (None, Some(p)) if rv.marks.get(&p.key()) == Some(&true) => "enter: to review again",
            _ => "enter: accept",
        }
    }

    /// Every reply of the review marked accepted, or rejected; or, not
    /// `replies`, every comment the work opened.
    pub(super) fn mark_all(&mut self, replies: bool, accept: bool) {
        let these: Vec<Part> = self
            .review_parts()
            .into_iter()
            .filter(|p| match p {
                Part::Reply(_) => replies,
                Part::Opened(_) => !replies,
                Part::Stack => false,
            })
            .collect();
        for p in these {
            self.mark(p, accept);
        }
    }

    /// What is marked, by kind of part: a line for each.
    pub(super) fn review_summary(&self) -> String {
        let Some(rv) = &self.reviewing else { return String::new() };
        let parts = self.review_parts();
        let mark = |p: &Part| rv.marks.get(&p.key()).copied();
        let mut out = Vec::new();
        let asking = self.asking_open();
        if !asking.is_empty() {
            let closing = asking.iter().filter(|(k, _)| rv.marks.get(&crate::held::asking_key(*k)) == Some(&true)).count();
            let mut said = Vec::new();
            for (n, word) in [(closing, "to close"), (asking.len() - closing, "kept")] {
                if n > 0 {
                    said.push(format!("{n} {word}"));
                }
            }
            out.push(format!("{}: {}", if asking.len() == 1 { "asking comment" } else { "asking comments" }, said.join(", ")));
        }
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
        let mut items = vec![
            Item::new("Save", Action::ReviewSave),
            Item::new("Discard", Action::ReviewDiscard),
            Item::new("Close review", Action::ReviewClose(None)).when(done),
            Item::new("Accept all remaining and Close", Action::ReviewClose(Some(true))),
            Item::new("Reject all remaining and Close", Action::ReviewClose(Some(false))),
        ];
        items.extend([Item::line(), Item::new("Keep reviewing", Action::Cancel)]);
        let run = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run));
        let of = match run {
            Some(r) if crate::held::pulled(r) => "the comments pulled",
            Some(r) if crate::held::by_hand(r) => "your edit by hand",
            _ => "the agent's work",
        };
        let mut m = Menu::new(format!("your review of {of}"), items, None);
        m.note = Some(self.review_summary());
        m
    }

    /// The menu of the part the cursor is on, in the review.
    pub(super) fn review_items(&self, items: &mut Vec<Item>) {
        let lines = self.review_lines();
        let Some(rv) = &self.reviewing else { return };
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
        // A comment that asked: closed with the review, or kept.
        if let Some(k) = lines.get(rv.sel).and_then(|l| l.asking) {
            let closing = rv.marks.get(&crate::held::asking_key(k)) == Some(&true);
            items.push(Item::new("Close it with the review", Action::CloseAsking(k, true)).when(!closing));
            items.push(Item::new("Keep it open", Action::CloseAsking(k, false)).when(closing));
        }
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
        // The comments it opened, and its replies, each kind apart.
        let parts = self.review_parts();
        if parts.iter().any(|p| matches!(p, Part::Opened(_))) {
            items.push(Item::new("Mark all opened comments accepted", Action::MarkOpened(true)).when(!rejected));
            items.push(Item::new("Mark all opened comments rejected", Action::MarkOpened(false)));
        }
        if parts.iter().any(|p| matches!(p, Part::Reply(_))) {
            items.push(Item::new("Mark all replies accepted", Action::MarkReplies(true)).when(!rejected));
            items.push(Item::new("Mark all replies rejected", Action::MarkReplies(false)));
        }
        // Edited by hand from here, the stack the work makes: the patch the
        // cursor is on, as mail or a file of it; the stack as a list.
        if self.read_only().is_none()
            && let Some((now, next)) = self.review_stacks()
        {
            items.push(Item::line());
            let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), false);
            let at = lines.get(rv.sel).and_then(|l| l.entry).and_then(|k| match diff.get(k)? {
                Diffed::Patch { at, .. } => Some(at - 1),
                _ => None,
            });
            if let Some(at) = at
                && let Some(id) = next.patches.get(at)
            {
                items.push(Item::new("Edit this patch in $EDITOR", Action::EditWorkPatch(at)));
                for f in crate::diff::parse_diff(&self.db.patch(id).diff, "") {
                    let short = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                    items.push(Item::new(format!("Edit {short} in $EDITOR"), Action::EditWorkFile(at, f.path.clone())).with(self.edit_choices()));
                }
            }
            items.push(Item::new("Edit the stack as a list in $EDITOR", Action::EditWorkList));
        }
    }

    /// The edit by hand `action` asks for in the review, of the stack the
    /// work makes; a file opened at its first change.
    pub(super) fn work_edit(&self, action: &Action) -> Option<Effect> {
        let run = self.reviewing.as_ref()?.run.clone();
        let edit = match action {
            Action::EditWorkList => WorkEdit::List,
            Action::EditWorkPatch(at) => WorkEdit::Mail(*at),
            Action::EditWorkFile(at, file) => {
                let (_, next) = self.review_stacks()?;
                let p = self.db.patch(next.patches.get(*at)?);
                let line = crate::diff::parse_diff(&p.diff, "")
                    .into_iter()
                    .find(|f| f.path == *file)
                    .and_then(|f| f.hunks.iter().flat_map(|h| h.lines.iter()).find(|l| l.origin != ' ').map(|l| l.new_no))
                    .unwrap_or(1)
                    .max(1);
                WorkEdit::Code {
                    at: *at,
                    file: file.clone(),
                    line,
                    again: None,
                }
            }
            _ => return None,
        };
        Some(Effect::EditWork { run, edit })
    }

    /// The marks made so far in the review of `run`, for the work that takes
    /// its place: all but its change of the stack's, which is new; the
    /// comments as they were before that change was rejected, if it was.
    pub(super) fn carried_marks(&self, run: &str) -> BTreeMap<String, bool> {
        let (mut marks, before) = match &self.reviewing {
            Some(rv) if rv.run == run => (rv.marks.clone(), rv.before.clone()),
            _ => (self.db.review(run).map(|v| v.marks.clone()).unwrap_or_default(), None),
        };
        if marks.remove(&Part::Stack.key()) == Some(false) {
            marks = before.unwrap_or_default();
        }
        marks
    }

    /// The part the cursor is on, in the review.
    pub(super) fn review_part(&self) -> Option<Part> {
        let rv = self.reviewing.as_ref()?;
        self.review_lines().get(rv.sel).and_then(|l| l.part)
    }

    /// Leaves the review, saved or not, for the main screen.
    pub(super) fn leave_review(&mut self) {
        self.keep_review();
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
}
