//! The banner, under the top bar of the main screen: what waits on you on
//! the stack. What landed and waits for your review (an ask's work, a
//! pull's comments from Phabricator), and every comment open, with what the
//! agent works on and what waits its turn: said in one line; while the
//! cursor is on it, open, a line for each, what landed on top, then the
//! comments, the most recently updated first, to review what landed or go
//! to a comment. Esc goes to it from the panes, and from it back where the
//! cursor was.

use super::reviewing::first_words;
use super::*;

/// How the work run `r` holds landed, said after what it holds: pulled
/// from Phabricator, or what the agent was asked (cut to `room`).
pub(super) fn landed_how(r: &crate::db::Run, room: usize) -> String {
    if crate::held::pulled(r) {
        ", pulled from Phabricator".into()
    } else if r.request.is_empty() {
        String::new()
    } else {
        format!(", asked \"{}\"", short(&r.request, room))
    }
}

/// A line of the banner, open.
#[derive(Debug, Clone, PartialEq)]
pub enum BannerLine {
    /// Landed, and waiting for your review: its run.
    Landed(String),
    /// A comment open on the stack: (change, id).
    Comment(String, String),
}

impl App {
    /// What the banner lists: what landed, the first landed first; then
    /// every comment open on the stack, the most recently updated first.
    pub fn banner_lines(&self) -> Vec<BannerLine> {
        let mut out: Vec<BannerLine> = self.queue().into_iter().map(BannerLine::Landed).collect();
        let mut open: Vec<(usize, BannerLine)> = self
            .db
            .stack_notes(&self.stack)
            .into_iter()
            .map(|n| {
                let updated = self.db.note_history(&n.change, &n.id).last().map_or(0, |(rev, _)| *rev);
                (updated, BannerLine::Comment(n.change.clone(), n.id.clone()))
            })
            .collect();
        open.sort_by(|a, b| b.0.cmp(&a.0));
        out.extend(open.into_iter().map(|(_, l)| l));
        out
    }

    /// What the banner says in its line: how much landed work waits for
    /// review, and which the next is; how much the agent works on; how many
    /// comments wait their turn; how many are open. None when there is
    /// none of it.
    pub(super) fn banner(&self) -> Option<String> {
        let lines = self.banner_lines();
        let landed = lines.iter().filter(|l| matches!(l, BannerLine::Landed(_))).count();
        let open = lines.len() - landed;
        let going = self.db.runs().into_iter().filter(|r| r.stack == self.stack && r.ask.is_some() && ask::busy(r)).count();
        let ready = self.db.stack_notes(&self.stack).into_iter().filter(|n| n.queued.is_some()).count();
        let of = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
        let mut said = Vec::new();
        if landed > 0 {
            // The asks' work, and the reviews pulled, each counted.
            let pulls = lines.iter().filter(|l| matches!(l, BannerLine::Landed(run) if self.db.run(run).is_some_and(crate::held::pulled))).count();
            let what = match (landed - pulls, pulls) {
                (asks, 0) => of(asks, "landed ask", "landed asks"),
                (0, pulls) => of(pulls, "pull from Phabricator", "pulls from Phabricator"),
                (asks, pulls) => format!("{} and {}", of(asks, "landed ask", "landed asks"), of(pulls, "pull from Phabricator", "pulls from Phabricator")),
            };
            let mut head = format!("{what} {} for your review", if landed == 1 { "waits" } else { "wait" });
            if let Some(BannerLine::Landed(run)) = lines.first()
                && let Some((r, h)) = self.work(run)
            {
                head += &format!(" · the next: {}{}", h.said(), landed_how(r, usize::MAX));
            }
            said.push(head);
        }
        if going > 0 {
            said.push(format!("the agent works on {}", of(going, "ask", "asks")));
        }
        if ready > 0 {
            said.push(if ready == 1 { "1 comment waits its turn".to_string() } else { format!("{ready} comments wait their turn") });
        }
        if open > 0 {
            said.push(of(open, "open comment", "open comments"));
        }
        if said.is_empty() {
            return None;
        }
        // As its lines mark them, and the top bar the agents going (●).
        let mark = match (landed, going, ready) {
            (0, 0, 0) => "○",
            (0, 0, _) => "◌",
            (0, _, _) => "●",
            _ => "⟳",
        };
        Some(format!(" {mark} {}", said.join(" · ")))
    }

    /// The banner's menu, to pick from: each landed ask and each pull, to
    /// review; then each comment open, the most recently updated first, to
    /// go to.
    pub(super) fn banner_items(&self, items: &mut Vec<Item>) {
        for id in self.queue() {
            let Some((r, h)) = self.work(&id) else { continue };
            items.push(Item::new(format!("Review {}{}", h.said(), landed_how(r, 40)), Action::Review(id.clone(), None)));
        }
        let comments: Vec<(String, String)> = self
            .banner_lines()
            .into_iter()
            .filter_map(|l| match l {
                BannerLine::Comment(change, id) => Some((change, id)),
                BannerLine::Landed(_) => None,
            })
            .collect();
        if !items.is_empty() && !comments.is_empty() {
            items.push(Item::line());
        }
        for (change, id) in comments {
            let Some(n) = self.db.note(&change, &id) else { continue };
            items.push(Item::new(format!("Go to {}", self.comment_said(n)), Action::GoToComment(change, id)));
        }
    }

    /// A comment in a line: where it is, and its first words.
    fn comment_said(&self, n: &Note) -> String {
        format!("{} · \"{}\"", where_on(&self.db, &self.stack, n), short(&first_words(&n.text), 40))
    }

    /// What line `l` of the banner, open, says: what landed, to review; a
    /// comment, marked as the comments pane marks it, where it stands (with
    /// the agent, the last line it wrote).
    fn banner_line(&self, l: &BannerLine) -> String {
        match l {
            BannerLine::Landed(run) => match self.work(run) {
                Some((r, h)) => format!("   ⟳ Review {}{}", h.said(), landed_how(r, 40)),
                None => format!("   ⟳ Review {run}"),
            },
            BannerLine::Comment(change, id) => {
                let Some(n) = self.db.note(change, id) else { return String::new() };
                let (busy, held) = (self.busy(), self.held_replies());
                let mark = self.note_mark(n, &busy, &held);
                let state = note_state(&self.db, &self.stack, n, &busy);
                let state = if state.is_empty() { String::new() } else { format!(" · {state}") };
                // With the agent: what it does now, the last line it wrote,
                // or the command run to start it.
                let doing = busy
                    .get(&(n.change.clone(), n.id.clone()))
                    .and_then(|(_, run)| ask::tail(&self.db, run, 1).pop())
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .map_or(String::new(), |l| format!(" · {l}"));
                format!("   {mark} {}{state}{doing}", self.comment_said(n))
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
    /// stack's header; Enter reviews what landed, or goes to the comment;
    /// Esc goes back where the cursor was.
    pub(super) fn banner_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.banner_lines();
        let last = lines.len().saturating_sub(1);
        let page = (self.body_h / 2).max(1);
        match k.code {
            KeyCode::Enter => match lines.get(self.banner_sel).cloned() {
                Some(BannerLine::Landed(run)) => self.open_review(&run, None),
                Some(BannerLine::Comment(change, id)) => {
                    self.on_banner = false;
                    self.go_to(&change, &id);
                }
                None => {}
            },
            KeyCode::Esc => self.on_banner = false,
            KeyCode::Down if self.banner_sel >= last => {
                self.on_banner = false;
                if !self.list.shown() {
                    self.list = Pane::Expanded;
                }
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
