//! The banner, under the top bar of the main screen: the agent's work on
//! the stack. What landed and waits for your review, what the agent works
//! on, and the comments that wait their turn: said in one line; while the
//! cursor is on it, open, a line for each, to review what landed or go to
//! the comment of the rest. Esc goes to it from the panes, and from it back
//! where the cursor was.

use super::reviewing::first_words;
use super::*;

/// A line of the banner, open: a piece of the agent's work on the stack.
#[derive(Debug, Clone, PartialEq)]
pub enum BannerLine {
    /// Landed, and waiting for your review: its run.
    Landed(String),
    /// The agent works on it: its run.
    Going(String),
    /// A comment ready to go, waiting its turn.
    Ready(String, String),
}

impl App {
    /// The agent's work on this stack, as the banner lists it: what
    /// landed, the first landed first; what the agent works on, the first
    /// asked first; then the comments that wait their turn.
    pub fn banner_lines(&self) -> Vec<BannerLine> {
        let mut out: Vec<BannerLine> = self.queue().into_iter().map(BannerLine::Landed).collect();
        out.extend(
            self.db
                .runs()
                .into_iter()
                .filter(|r| r.stack == self.stack && r.ask.is_some() && ask::busy(r))
                .map(|r| BannerLine::Going(r.id.clone())),
        );
        out.extend(
            self.db
                .stack_notes(&self.stack)
                .into_iter()
                .filter(|n| n.queued.is_some())
                .map(|n| BannerLine::Ready(n.change.clone(), n.id.clone())),
        );
        out
    }

    /// What the banner says in its line: how much landed work waits for
    /// review, and which the next is; how much the agent works on; how many
    /// comments wait their turn. None when there is none of it.
    pub(super) fn banner(&self) -> Option<String> {
        let lines = self.banner_lines();
        let count = |f: fn(&BannerLine) -> bool| lines.iter().filter(|l| f(l)).count();
        let landed = count(|l| matches!(l, BannerLine::Landed(_)));
        let going = count(|l| matches!(l, BannerLine::Going(_)));
        let ready = count(|l| matches!(l, BannerLine::Ready(..)));
        let mut said = Vec::new();
        if landed > 0 {
            let mut head = if landed == 1 {
                "1 landed ask waits for your review".to_string()
            } else {
                format!("{landed} landed asks wait for your review")
            };
            if let Some(BannerLine::Landed(run)) = lines.first()
                && let Some((r, h)) = self.work(run)
            {
                let asked = if r.request.is_empty() { String::new() } else { format!(", asked \"{}\"", r.request) };
                head += &format!(" · the next: {}{asked}", h.said());
            }
            said.push(head);
        }
        if going > 0 {
            said.push(format!("the agent works on {going} ask{}", plural(going)));
        }
        if ready > 0 {
            said.push(if ready == 1 { "1 comment waits its turn".to_string() } else { format!("{ready} comments wait their turn") });
        }
        if said.is_empty() {
            return None;
        }
        // As its lines mark them, and the top bar the agents going (●).
        let mark = match (landed, going) {
            (0, 0) => "◌",
            (0, _) => "●",
            _ => "⟳",
        };
        Some(format!(" {mark} {}", said.join(" · ")))
    }

    /// The banner's menu: each landed ask, to review.
    pub(super) fn banner_items(&self, items: &mut Vec<Item>) {
        for id in self.queue() {
            let Some((r, h)) = self.work(&id) else { continue };
            let asked = if r.request.is_empty() { String::new() } else { format!(", asked \"{}\"", short(&r.request, 40)) };
            items.push(Item::new(format!("Review {}{asked}", h.said()), Action::Review(id.clone(), None)));
        }
    }

    /// A comment in a line: where it is, and its first words.
    fn comment_said(&self, n: &Note) -> String {
        format!("{} · \"{}\"", where_on(&self.db, &self.stack, n), short(&first_words(&n.text), 40))
    }

    /// What line `l` of the banner, open, says.
    fn banner_line(&self, l: &BannerLine) -> String {
        match l {
            BannerLine::Landed(run) => match self.work(run) {
                Some((r, h)) => {
                    let asked = if r.request.is_empty() { String::new() } else { format!(", asked \"{}\"", short(&r.request, 40)) };
                    format!("   ⟳ Review {}{asked}", h.said())
                }
                None => format!("   ⟳ Review {run}"),
            },
            BannerLine::Going(run) => {
                let Some(i) = self.db.run(run).and_then(ask::info) else {
                    return format!("   ● With the agent: {run}");
                };
                let notes: Vec<&Note> = i.notes.iter().filter_map(|s| self.db.note(&s.change, &s.id)).collect();
                let what = match notes.as_slice() {
                    [] => "its comments are gone".to_string(),
                    [n] => self.comment_said(n),
                    [n, rest @ ..] => format!("{} and {} more", self.comment_said(n), rest.len()),
                };
                // What it does now: the last line it wrote, or the command
                // run to start it.
                let doing = ask::tail(&self.db, run, 1)
                    .pop()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .map_or(String::new(), |l| format!(" · {l}"));
                format!("   ● With the agent ({}): {what}{doing}", i.way.said())
            }
            BannerLine::Ready(change, id) => {
                let Some(n) = self.db.note(change, id) else { return String::new() };
                let Some(q) = &n.queued else { return String::new() };
                let on = ask::waits_on(&self.db, &self.stack, q.way).unwrap_or_else(|| "next".into());
                format!("   ◌ Ready ({}), {on}: {}", q.said(), self.comment_said(n))
            }
        }
    }

    /// The cursor on the banner, open, on its line `at`. Where it was is
    /// kept: it goes back there.
    pub(super) fn open_banner(&mut self, at: usize) {
        self.on_banner = true;
        self.banner_sel = at;
        self.banner_top = 0;
    }

    /// A key on the banner, open: ↑↓ another line, past the last the
    /// stack's header; Enter reviews what landed, or goes to the comment
    /// the agent works on, or that waits; Esc goes back where the cursor
    /// was.
    pub(super) fn banner_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.banner_lines();
        let last = lines.len().saturating_sub(1);
        let page = (self.body_h / 2).max(1);
        match k.code {
            KeyCode::Enter => match lines.get(self.banner_sel).cloned() {
                Some(BannerLine::Landed(run)) => self.open_review(&run, None),
                Some(BannerLine::Going(run)) => {
                    if let Some(s) = self.db.run(&run).and_then(ask::info).and_then(|i| i.notes.into_iter().next()) {
                        self.on_banner = false;
                        self.go_to(&s.change, &s.id);
                    }
                }
                Some(BannerLine::Ready(change, id)) => {
                    self.on_banner = false;
                    self.go_to(&change, &id);
                }
                None => {}
            },
            KeyCode::Esc => self.on_banner = false,
            KeyCode::Down if self.banner_sel >= last => {
                self.on_banner = false;
                self.list = true;
                self.focus = Focus::List;
                self.on_stack = true;
            }
            KeyCode::Down => self.banner_sel += 1,
            KeyCode::Up => self.banner_sel = self.banner_sel.saturating_sub(1),
            KeyCode::PageDown => self.banner_sel = (self.banner_sel + page).min(last),
            KeyCode::PageUp => self.banner_sel = self.banner_sel.saturating_sub(page),
            KeyCode::Home => self.banner_sel = 0,
            KeyCode::End => self.banner_sel = last,
            _ => {}
        }
        Effect::None
    }

    /// The banner under the top bar: `text` on its line; open, under it, a
    /// line for each of `lines`, as many as `room` holds, the one picked
    /// marked and in sight.
    pub(super) fn draw_banner(&mut self, buf: &mut Buffer, area: Rect, text: &str, lines: &[BannerLine], room: usize) {
        let w = area.width as usize;
        put(buf, area, 0, 1, w, &fit(text, w), color::header());
        spot_in(&self.spots, area, (0, 1, w, 1), Spot::Banner);
        if lines.is_empty() || room == 0 {
            return;
        }
        self.banner_sel = self.banner_sel.min(lines.len() - 1);
        if self.banner_sel < self.banner_top {
            self.banner_top = self.banner_sel;
        } else if self.banner_sel >= self.banner_top + room {
            self.banner_top = self.banner_sel + 1 - room;
        }
        for (y, (i, l)) in lines.iter().enumerate().skip(self.banner_top).take(room).enumerate() {
            let st = if i == self.banner_sel { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
            put(buf, area, 0, 2 + y, w, &fit(&self.banner_line(l), w), st);
            spot_in(&self.spots, area, (0, 2 + y, w, 1), Spot::BannerLine(i));
        }
    }
}
