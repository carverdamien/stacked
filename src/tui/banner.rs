//! The banner, under the top bar of the main screen: what waits on you on
//! the stack. The commit messages to sync, while those of some patches
//! differ from their commits' in your workspace, before anything else; what
//! landed and waits for your review (an ask's work, a pull's comments from
//! Phabricator), which waits until those messages are synced; a pull from
//! Phabricator under way and where it is, the batch (the comments asked
//! about later, to send together), and every comment open, with what the
//! agent works on and what waits its turn: said in one line; while the
//! cursor is on it, open, a line for each, in sections under their titles:
//! the commit messages to sync, in a section of their own, first; what
//! landed and changes nothing (read only), the pull under way last; then
//! what changes the stack (edits), each the one you review now first, then
//! those going on from an edit under review; then the batch and the
//! comments, the most recently updated first. Enter opens the sync of the
//! commit messages, reviews what landed, sends the batch, goes to a
//! comment, or, on the pull under way, says what it brings. Esc goes to it
//! from the panes, and from it back where the cursor was.

use super::reviewing::first_words;
use super::*;

/// How the work run `r` holds landed, said after what it holds: pulled
/// from Phabricator, your edit by hand, or what the agent was asked (cut
/// to `room`), and whether you edited it by hand since.
pub(super) fn landed_how(r: &crate::db::Run, room: usize) -> String {
    if crate::held::pulled(r) {
        return ", pulled from Phabricator".into();
    }
    if crate::held::by_hand(r) {
        return format!(", by hand: {}", short(&r.request, room));
    }
    let mut out = if r.request.is_empty() { String::new() } else { format!(", asked \"{}\"", short(&r.request, room)) };
    if r.held.as_ref().is_some_and(|h| !h.edits.is_empty()) {
        out.push_str(", edited by hand");
    }
    out
}

/// What a pull under way brings, said on its banner line.
#[cfg(feature = "moz-phab")]
pub(super) const PULL_BRINGS: &str = "what is new from Phabricator will wait here for your review once the pull is done";

/// `a`, `a and b`, `a, b and c`.
fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A line of the banner, open.
#[derive(Debug, Clone, PartialEq)]
pub enum BannerLine {
    /// The commit messages of some patches differ from their commits' in
    /// your workspace: to sync, before any landed work is reviewed.
    Sync,
    /// Landed, and waiting for your review: its run.
    Landed(String),
    /// A pull from Phabricator under way: what it brings is read only, to
    /// review.
    #[cfg(feature = "moz-phab")]
    Pulling,
    /// The batch: the comments asked about later, to send.
    Batch,
    /// A comment open on the stack: (change, id).
    Comment(String, String),
}

impl App {
    /// The pull from Phabricator under way on the stack shown, if one is:
    /// where it is.
    #[cfg(feature = "moz-phab")]
    pub(super) fn pulling(&self) -> Option<&crate::review::Pulling> {
        self.bg.pull.as_ref().filter(|(name, _)| *name == self.stack).map(|(_, p)| p)
    }

    /// What the banner lists, section by section: the commit messages to
    /// sync, if some differ; what landed and changes nothing, then the pull
    /// under way; what changes the stack, each the first landed first, the
    /// one you review now before those going on from an edit under review;
    /// the batch, if there is one; then every comment open on the stack,
    /// the most recently updated first.
    pub fn banner_lines(&self) -> Vec<BannerLine> {
        let landed = self.queue();
        let mut out: Vec<BannerLine> = Vec::new();
        if self.sync_first().is_some() {
            out.push(BannerLine::Sync);
        }
        for edits in [false, true] {
            for later in [false, true] {
                out.extend(
                    landed
                        .iter()
                        .filter(|id| self.banner_section(&BannerLine::Landed((*id).clone())) == 1 + usize::from(edits))
                        .filter(|id| self.db.run(id).is_some_and(|r| ask::guessing(&self.db, r).is_some()) == later)
                        .map(|id| BannerLine::Landed(id.clone())),
                );
            }
            #[cfg(feature = "moz-phab")]
            if !edits && self.pulling().is_some() {
                out.push(BannerLine::Pulling);
            }
        }
        if self.batched_count() > 0 {
            out.push(BannerLine::Batch);
        }
        let mut open: Vec<(usize, BannerLine)> = self
            .db
            .stack_notes(&self.stack)
            .into_iter()
            .map(|n| {
                let updated = self.db.note_rev(&n.change, &n.id).unwrap_or(0);
                (updated, BannerLine::Comment(n.change.clone(), n.id.clone()))
            })
            .collect();
        open.sort_by(|a, b| b.0.cmp(&a.0));
        out.extend(open.into_iter().map(|(_, l)| l));
        out
    }

    /// The section of the banner line `l` is in: 0, the commit messages to
    /// sync; 1, read only, what landed and changes nothing; 2, edits, what
    /// changes the stack; 3, the batch and the comments.
    fn banner_section(&self, l: &BannerLine) -> usize {
        match l {
            BannerLine::Sync => 0,
            BannerLine::Landed(id) => 1 + usize::from(self.work(id).is_some_and(|(_, h)| h.stack.is_some())),
            #[cfg(feature = "moz-phab")]
            BannerLine::Pulling => 1,
            _ => 3,
        }
    }

    /// The rows of the banner, open, for `lines`: each line, by its place,
    /// under its section's title where a section starts.
    pub(super) fn banner_rows(&self, lines: &[BannerLine]) -> Vec<(Option<usize>, usize)> {
        let mut rows: Vec<(Option<usize>, usize)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            let k = self.banner_section(l);
            if rows.last().is_none_or(|&(_, last)| last != k) {
                rows.push((None, k));
            }
            rows.push((Some(i), k));
        }
        rows
    }

    /// What the banner says in its line: first, the commit messages to
    /// sync, if some differ; how much landed work waits for review, and
    /// which the next is; where a pull from Phabricator under way is; how
    /// many asks are batched; how much the agent works on; how many
    /// comments wait their turn; how many are open. None when there is none
    /// of it.
    pub(super) fn banner(&self) -> Option<String> {
        let lines = self.banner_lines();
        let landed = lines.iter().filter(|l| matches!(l, BannerLine::Landed(_))).count();
        let open = lines.iter().filter(|l| matches!(l, BannerLine::Comment(..))).count();
        let batched = self.batched_count();
        let going = self.db.live_runs().into_iter().filter(|r| r.stack == self.stack && r.ask.is_some() && ask::busy(r)).count();
        let ready = self.ready();
        let of = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
        let mut said = Vec::new();
        let sync = self.sync_said();
        said.extend(sync.clone());
        if landed > 0 {
            // The asks' work, your edits by hand and the reviews pulled,
            // each counted.
            let kind = |is: fn(&crate::db::Run) -> bool| {
                lines.iter().filter(|l| matches!(l, BannerLine::Landed(run) if self.db.run(run).is_some_and(is))).count()
            };
            let (pulls, hands) = (kind(crate::held::pulled), kind(crate::held::by_hand));
            let mut kinds = Vec::new();
            for (n, one, many) in [
                (landed - pulls - hands, "landed ask", "landed asks"),
                (hands, "edit by hand", "edits by hand"),
                (pulls, "pull from Phabricator", "pulls from Phabricator"),
            ] {
                if n > 0 {
                    kinds.push(of(n, one, many));
                }
            }
            let what = and(&kinds);
            let mut head = format!("{what} {} for your review", if landed == 1 { "waits" } else { "wait" });
            // Gone on from an edit under review, guessing: reviewed after it.
            let later = lines
                .iter()
                .filter(|l| matches!(l, BannerLine::Landed(run) if self.db.run(run).is_some_and(|r| ask::guessing(&self.db, r).is_some())))
                .count();
            if later > 0 {
                head += &format!(", {later} of {} after an edit under review", if landed == 1 { "it" } else { "them" });
            }
            let next = lines.iter().find_map(|l| match l {
                BannerLine::Landed(run) => self.work(run).filter(|(r, _)| ask::guessing(&self.db, r).is_none()),
                _ => None,
            });
            if let Some((r, _)) = next {
                head += &format!(" · the next: {}{}", crate::held::left(&self.db, r), landed_how(r, usize::MAX));
            }
            said.push(head);
        }
        #[cfg(feature = "moz-phab")]
        let pulling = match self.pulling() {
            Some(p) => {
                said.push(format!("{}…", p.said()));
                true
            }
            None => false,
        };
        #[cfg(not(feature = "moz-phab"))]
        let pulling = false;
        if batched > 0 {
            said.push(of(batched, "batched ask", "batched asks"));
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
        // As its lines mark them, and the top bar the agents going (●): the
        // commit messages to sync first.
        let mark = match (landed, going, pulling, ready, batched) {
            _ if sync.is_some() => "✉",
            (0, 0, false, 0, 0) => "○",
            (0, 0, false, 0, _) => "◇",
            (0, 0, false, _, _) => "◌",
            (0, 0, true, _, _) => "⇣",
            (0, _, _, _, _) => "●",
            _ => "⟳",
        };
        Some(format!(" {mark} {}", said.join(" · ")))
    }

    /// The banner's menu, to pick from, all of it: the sync of the commit
    /// messages, while some differ, first; each landed ask and each pull, to
    /// review, once those are synced; the batch, to send; every comment the
    /// agent can be asked about, checked if it is in the batch; then each
    /// comment open, the most recently updated first, to go to.
    pub(super) fn banner_items(&self, items: &mut Vec<Item>) {
        let synced = match self.sync_patches() {
            Some(of) => {
                items.push(Item::new(format!("Sync the commit messages of {of}…"), Action::OpenSync));
                items.push(Item::line());
                false
            }
            None => true,
        };
        for id in self.queue() {
            let Some((r, _)) = self.work(&id) else { continue };
            let first = ask::guessing(&self.db, r).is_none();
            let left = crate::held::left(&self.db, r);
            items.push(Item::new(format!("Review {left}{}", landed_how(r, 40)), Action::Review(id.clone(), None)).when(first && synced));
        }
        let batch = self.batch_items();
        if !items.is_empty() && !batch.is_empty() {
            items.push(Item::line());
        }
        items.extend(batch);
        let comments: Vec<(String, String)> = self
            .banner_lines()
            .into_iter()
            .filter_map(|l| match l {
                BannerLine::Comment(change, id) => Some((change, id)),
                _ => None,
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

    /// The batch's items: what it holds sent, each way (the edits to a
    /// single agent; the answers to a single agent, or fanned out, an agent
    /// each; each build and test on its own), each at the effort it was
    /// batched at unless another is picked; then every comment the agent
    /// can be asked about, checked if it is in the batch, as it is asked.
    fn batch_items(&self) -> Vec<Item> {
        let w = self.read_only().is_none();
        let mut out = Vec::new();
        let count = |way: Way| ask::batched(&self.db, &self.stack, way).len();
        let effort = |way: Way| self.effort_choices(ask::batched_effort(&self.db, &self.stack, way));
        let (edits, answers, tests) = (count(Way::Change), count(Way::Answer), count(Way::Test));
        if edits > 0 {
            let label = format!("Send {edits} edit{} to a single agent", plural(edits));
            out.push(Item::new(label, Action::SendBatch(Way::Change, false)).when(w).with(effort(Way::Change)));
        }
        if answers > 0 {
            let list = vec![
                Choice::new("single", "a single agent", "one agent answers them all, in one ask", Some(Action::SendBatch(Way::Answer, false))),
                Choice::new("fan", "fan out", "each goes to an agent of its own, all at once", Some(Action::SendBatch(Way::Answer, true))),
            ];
            let choices = Choices::new("answers", "to whom", list, self.db.choice(&self.stack, "answers"));
            let label = format!("Send {answers} answer{} only to", plural(answers));
            out.push(Item::new(label, Action::SendBatch(Way::Answer, false)).when(w).with(choices).with(effort(Way::Answer)));
        }
        if tests > 0 {
            let label = format!("Send {tests} build{} and test{}, each on its own", plural(tests), plural(tests));
            out.push(Item::new(label, Action::SendBatch(Way::Test, true)).when(w).with(effort(Way::Test)));
        }
        let asks = self.asks_list();
        if !out.is_empty() && !asks.is_empty() {
            out.push(Item::line());
        }
        for n in asks {
            let batched = n.queued.as_ref().filter(|q| q.batched());
            let how = batched.map_or(String::new(), |q| format!(" · {}", q.said()));
            let item = Item::new(format!("{}{how}", self.comment_said(n)), Action::Toggle(n.change.clone(), n.id.clone()));
            out.push(item.when(w).checked(batched.is_some()));
        }
        out
    }

    /// Every comment of the stack the agent can be asked about now, or
    /// batched, in the order they were written: the batch's list, which
    /// checking one does not reorder.
    fn asks_list(&self) -> Vec<&Note> {
        let (busy, held) = (self.busy(), self.held_replies());
        let mut out: Vec<(usize, &Note)> = self
            .db
            .stack_notes(&self.stack)
            .into_iter()
            .filter(|n| {
                let key = (n.change.clone(), n.id.clone());
                n.queued.as_ref().is_some_and(|q| q.batched()) || (ask::sendable(n) && !busy.contains_key(&key) && !held.contains_key(&key))
            })
            .map(|n| (self.db.note_first(&n.change, &n.id).unwrap_or(0), n))
            .collect();
        out.sort_by_key(|(written, _)| *written);
        out.into_iter().map(|(_, n)| n).collect()
    }

    /// What the batch holds, in a few words: `2 edits, 1 answer only`.
    fn batch_said(&self) -> String {
        let count = |way: Way| ask::batched(&self.db, &self.stack, way).len();
        let mut parts = Vec::new();
        for (n, one, many) in [
            (count(Way::Change), "edit", "edits"),
            (count(Way::Answer), "answer only", "answers only"),
            (count(Way::Test), "build and test", "builds and tests"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        parts.join(", ")
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
            BannerLine::Sync => {
                let of = self.sync_patches().unwrap_or_default();
                format!("   ✉ Sync the commit messages of {of}: the stack and your workspace differ")
            }
            BannerLine::Landed(run) => match self.work(run) {
                // Gone on from the work above it, guessing: reviewed after it.
                Some((r, _)) if ask::guessing(&self.db, r).is_some() => {
                    format!("   ◌ {}{} · after the edit under review, which you review first", crate::held::left(&self.db, r), landed_how(r, 40))
                }
                Some((r, _)) => format!("   ⟳ Review {}{}", crate::held::left(&self.db, r), landed_how(r, 40)),
                None => format!("   ⟳ Review {run}"),
            },
            #[cfg(feature = "moz-phab")]
            BannerLine::Pulling => {
                let said = self.pulling().map(crate::review::Pulling::said).unwrap_or_default();
                let mut c = said.chars();
                let said = c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect());
                format!("   ⇣ {said}…")
            }
            BannerLine::Batch => format!("   ◇ Send the batch… {}", self.batch_said()),
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
    /// stack's header; Enter opens the sync of the commit messages, reviews
    /// what landed (once those are synced), opens the menu on the batch, or
    /// goes to the comment; Esc goes back where the cursor was.
    pub(super) fn banner_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.banner_lines();
        let last = lines.len().saturating_sub(1);
        let page = (self.body_h / 2).max(1);
        match k.code {
            KeyCode::Enter => match lines.get(self.banner_sel).cloned() {
                Some(BannerLine::Sync) => self.open_sync(),
                Some(BannerLine::Landed(run)) => self.open_review(&run, None),
                #[cfg(feature = "moz-phab")]
                Some(BannerLine::Pulling) => self.status = PULL_BRINGS.into(),
                // The whole menu, on what sends the batch.
                Some(BannerLine::Batch) => {
                    let mut m = self.main_menu();
                    if let Some(k) = m.items.iter().position(|i| matches!(i.action, Action::SendBatch(..))) {
                        m.sel = k;
                    }
                    self.mode = Mode::Menu(m);
                }
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
                    self.list = PatchesPane::Expanded;
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
    /// row for each of `lines` under its section's title, as many as `room`
    /// holds, the one picked marked and in sight, its title too where it
    /// can be.
    pub(super) fn draw_banner(&mut self, buf: &mut Buffer, area: Rect, text: &str, lines: &[BannerLine], room: usize) {
        let w = area.width as usize;
        put(buf, area, 0, 1, w, &fit(text, w), color::header());
        spot_in(&self.spots, area, (0, 1, w, 1), Spot::Banner);
        if lines.is_empty() || room == 0 {
            return;
        }
        self.banner_sel = self.banner_sel.min(lines.len() - 1);
        let rows = self.banner_rows(lines);
        let at = rows.iter().position(|&(i, _)| i == Some(self.banner_sel)).unwrap_or(0);
        let titled = if at > 0 && rows[at - 1].0.is_none() { at - 1 } else { at };
        if titled < self.banner_top {
            self.banner_top = titled;
        } else if at >= self.banner_top + room {
            self.banner_top = at + 1 - room;
        }
        for (y, &(i, k)) in rows.iter().skip(self.banner_top).take(room).enumerate() {
            let Some(i) = i else {
                let title = match k {
                    0 => " commit messages, to sync first",
                    1 => " read only, to review",
                    2 => " edits, to review",
                    _ => " open comments",
                };
                put(buf, area, 0, 2 + y, w, &fit(title, w), color::file());
                continue;
            };
            let st = if i == self.banner_sel { on_cursor(Style::new()) } else { Style::new() };
            put(buf, area, 0, 2 + y, w, &fit(&self.banner_line(&lines[i]), w), st);
            spot_in(&self.spots, area, (0, 2 + y, w, 1), Spot::BannerLine(i));
        }
    }
}
