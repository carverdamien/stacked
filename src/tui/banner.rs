//! The banner, under the top bar of the main screen: what waits on you on
//! the stack. What landed and waits for your review (an ask's work, a
//! pull's comments from Phabricator), a pull from Phabricator under way and
//! where it is, the batch (the comments asked about later, to send
//! together), and every comment open, with what the agent works on and what
//! waits its turn: said in one line; while the cursor is on it, open, a
//! line for each, in sections under their titles: what landed and changes
//! nothing (read only), the pull under way last; then what changes the
//! stack (edits), each the one you review now first, then those going on
//! from an edit under review; then the batch and the comments, the most
//! recently updated first. Enter reviews what landed, sends the batch, goes
//! to a comment, or, on the pull under way, says what it brings. Esc goes
//! to it from the panes, and from it back where the cursor was.
//!
//! What waits its turn in the agent's queue is taken out of it from here,
//! as from its own menus, each asked first: a comment's ask, the comment
//! staying open; the stack's lint job; the stack's reconcile with HEAD,
//! which Enter on its line offers to drop. What waits after it goes on.

use super::reviewing::first_words;
use super::*;

/// How the work run `r` holds landed, said after what it holds: pulled
/// from Phabricator, your edit by hand, or what the agent was asked (cut
/// to `room`) and at what effort, and whether you edited it by hand since.
pub(super) fn landed_how(r: &crate::db::Run, room: usize) -> String {
    if crate::held::pulled(r) {
        return ", pulled from Phabricator".into();
    }
    if crate::held::by_hand(r) {
        return format!(", by hand: {}", short(&r.request, room));
    }
    if crate::held::linted(r) {
        return format!(", by the linter: {}", short(&r.request, room));
    }
    if crate::reconcile::is(r) {
        return ", reconciled with HEAD: its commits' messages".into();
    }
    // The effort it was asked at before what was asked, which may be long.
    let at = crate::thread::asked_at(ask::effort_said(r));
    let mut out = match r.request.is_empty() {
        true if at.is_empty() => String::new(),
        true => format!(", asked{at}"),
        false if at.is_empty() => format!(", asked \"{}\"", short(&r.request, room)),
        false => format!(", asked{at}: \"{}\"", short(&r.request, room)),
    };
    if r.held.as_ref().is_some_and(|h| !h.edits.is_empty()) {
        out.push_str(", edited by hand");
    }
    out
}

/// What a pull under way brings, said on its banner line.
#[cfg(feature = "moz-phab")]
pub(super) const PULL_BRINGS: &str = "what is new from Phabricator will wait here for your review once the pull is done";

/// The kind of ask a comment waiting its turn is queued as, as taking it
/// out of the queue names it: `edit ask`, `answer-only ask`.
pub(super) fn ask_said(q: &crate::db::Queued) -> &'static str {
    match q.way {
        Way::Change => "edit ask",
        Way::Answer => "answer-only ask",
        Way::Test => "build and test ask",
        #[cfg(feature = "try-push")]
        Way::Try => "try push ask",
    }
}

/// Whose comment `n` of stack `name` is, as taking its ask out of the
/// queue names it: `patch 2's comment`, `the stack's comment`.
fn comment_of(db: &crate::db::Db, name: &str, n: &Note) -> String {
    if let Some(rev) = n.about {
        return format!("the comment on @{rev}");
    }
    if n.on_stack() {
        return "the stack's comment".into();
    }
    match db.stack(name).ok().and_then(|s| s.patches.iter().position(|id| db.patch(id).change == n.change)) {
        Some(k) => format!("patch {}'s comment", k + 1),
        None => "the comment on a patch no longer in the stack".into(),
    }
}

/// What dropping run `id` of stack `name`, a lint job or a reconcile with
/// HEAD waiting its turn, frees, said: the edits asked after it, which no
/// longer wait for it. None when no edit waits after it.
pub(super) fn edits_after_said(db: &crate::db::Db, name: &str, id: &str) -> Option<String> {
    match ask::edits_after(db, name, id) {
        0 => None,
        1 => Some("The edit asked after it no longer waits for it.".into()),
        k => Some(format!("The {k} edits asked after it no longer wait for it.")),
    }
}

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
    /// Landed, and waiting for your review: its run.
    Landed(String),
    /// A pull from Phabricator under way: what it brings is read only, to
    /// review.
    #[cfg(feature = "moz-phab")]
    Pulling,
    /// A try push under way, followed: its run.
    #[cfg(feature = "try-push")]
    Push(String),
    /// The stack's lint job, waiting its turn or running: its run.
    Lint(String),
    /// The stack's reconcile with HEAD, waiting its turn: its run.
    Reconcile(String),
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

    /// What the banner lists, section by section: what landed and changes
    /// nothing, then the pull under way; what changes the stack, each the
    /// first landed first, the one you review now before those going on
    /// from an edit under review; the batch, if there is one; then every
    /// comment open on the stack, the most recently updated first.
    pub fn banner_lines(&self) -> Vec<BannerLine> {
        let landed = self.queue();
        let mut out: Vec<BannerLine> = Vec::new();
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
        // The stack's reconcile with HEAD waiting its turn, among the edits.
        out.extend(crate::reconcile::pending(&self.db, &self.stack).filter(|r| r.state == crate::db::RunState::Started).map(|r| BannerLine::Reconcile(r.id.clone())));
        // The stack's lint job, waiting or running.
        out.extend(crate::lint::pending(&self.db, &self.stack).map(|r| BannerLine::Lint(r.id.clone())));
        // The try pushes of the stack under way, the first started first.
        #[cfg(feature = "try-push")]
        out.extend(self.pushes().into_iter().map(BannerLine::Push));
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

    /// The section of the banner line `l` is in: 1, read only, what landed
    /// and changes nothing; 2, edits, what changes the stack; 5, the stack's
    /// lint job; 4, the try pushes under way; 3, the batch and the comments.
    fn banner_section(&self, l: &BannerLine) -> usize {
        match l {
            BannerLine::Landed(id) => 1 + usize::from(self.work(id).is_some_and(|(_, h)| h.stack.is_some())),
            #[cfg(feature = "moz-phab")]
            BannerLine::Pulling => 1,
            #[cfg(feature = "try-push")]
            BannerLine::Push(_) => 4,
            BannerLine::Lint(_) => 5,
            BannerLine::Reconcile(_) => 2,
            _ => 3,
        }
    }

    /// Where the stack's lint job `run` is, said on its banner line and in
    /// the banner's head: `lints patch 2 of 5`, `waits its turn to lint the
    /// stack`.
    fn lint_progress(&self, run: &str) -> String {
        self.db.run(run).and_then(|r| r.lint.as_deref()).map(crate::lint::Job::said).unwrap_or_default()
    }

    /// What the stack's reconcile with HEAD, waiting its turn, waits for,
    /// said.
    pub(super) fn reconcile_waits(&self) -> String {
        self.reconcile_waits_of(&self.stack)
    }

    /// As `reconcile_waits`, of stack `name`'s reconcile.
    pub(super) fn reconcile_waits_of(&self, name: &str) -> String {
        let on = crate::ask::reconcile_waits_on(&self.db, name).unwrap_or_else(|| "next".into());
        format!("Reconcile with HEAD waits its turn: stacked makes it {on}, and it then waits for your review")
    }

    // -- what waits its turn, taken out of the queue ------------------------ //

    /// The item that drops the stack's reconcile with HEAD waiting its
    /// turn, asking first, if one waits.
    pub(super) fn reconcile_drop_item(&self) -> Option<Item> {
        let r = crate::reconcile::pending(&self.db, &self.stack).filter(|r| r.state == RunState::Started)?;
        Some(Item::new("Drop the waiting reconcile with HEAD…", Action::AskDropReconcile(r.id.clone())))
    }

    /// Reconcile `run`, if it waits its turn.
    fn reconcile_waiting(&self, run: &str) -> Option<&crate::db::Run> {
        self.db.run(run).filter(|r| crate::reconcile::is(r) && r.state == RunState::Started)
    }

    /// The menu of reconcile `run` waiting its turn, which Enter on its line
    /// opens, in the banner and in the runs: what it waits for, and its
    /// drop. None once it no longer waits.
    pub(super) fn reconcile_menu(&self, run: &str) -> Option<Menu> {
        let r = self.reconcile_waiting(run)?;
        let items = vec![Item::new("Drop the waiting reconcile with HEAD…", Action::AskDropReconcile(run.into()))];
        let mut m = Menu::new("reconcile with HEAD, waiting its turn", items, None);
        m.note = Some(format!("{}.", self.reconcile_waits_of(&r.stack)));
        Some(m)
    }

    /// What dropping reconcile `run`, waiting its turn, asks first: that
    /// nothing was changed, and what is then of the edits asked after it.
    /// None once it no longer waits.
    pub(super) fn drop_reconcile_ask(&self, run: &str) -> Option<Menu> {
        let r = self.reconcile_waiting(run)?;
        let mut said = vec!["It is taken off the queue: nothing is taken from HEAD, nothing changes in the stack, and the marks stay.".to_string()];
        said.extend(edits_after_said(&self.db, &r.stack, run));
        let yes = Item::new("Drop it", Action::DropReconcile(run.into()));
        let mut m = Menu::ask("drop the reconcile with HEAD that waits its turn? Nothing was changed yet", yes, "Keep it", true);
        m.note = Some(said.join("\n"));
        Some(m)
    }

    /// Reconcile `run`, waiting its turn, dropped: the status line says so,
    /// and what waited behind it goes.
    pub(super) fn drop_reconcile(&mut self, run: &str) {
        match crate::reconcile::drop_waiting(&self.path, run) {
            Ok((r, dropped)) => {
                self.reload();
                self.status = match (dropped, crate::held::waiting(&self.db, &r)) {
                    (true, _) => "the waiting reconcile with HEAD was dropped: nothing was taken from HEAD, and the stack is as it was".into(),
                    (false, true) => "the reconcile with HEAD was made before it could be dropped: it waits for your review".into(),
                    (false, false) => format!("the reconcile with HEAD was over before it could be dropped: {}", r.summary),
                };
                self.launch();
            }
            Err(e) => self.status = e.0,
        }
    }

    /// What taking comment (change, id) back from the agent's queue asks
    /// first, if it waits its turn there: what is taken out, the comment
    /// staying open; the comments asked with it, which still wait their
    /// turn; of a round sent back, what still goes back to the agent, the
    /// work staying as you sent it back. None if it does not wait.
    pub(super) fn unsend_ask(&self, change: &str, id: &str) -> Option<Menu> {
        let n = self.db.note(change, id)?;
        let q = n.queued.as_ref().filter(|q| !q.batched())?;
        let title = format!("take the {} on {} out of the queue? The comment stays open", ask_said(q), comment_of(&self.db, &self.stack, n));
        let note = match (q.on.is_some(), ask::asked_with(&self.db, &self.stack, &q.batch, change, id)) {
            (true, 0) => Some(
                "This comment goes back to the agent with the work you sent back, and nothing else does: taken out of the queue, nothing goes back to the agent. The work stays as you sent it back, its review closed and its change not applied."
                    .to_string(),
            ),
            (true, 1) => Some("This comment goes back to the agent with the work you sent back, as 1 other comment does: that one still goes back with the work.".to_string()),
            (true, k) => Some(format!("This comment goes back to the agent with the work you sent back, as {k} other comments do: those still go back with the work.")),
            (false, 0) => None,
            (false, 1) => Some("1 other comment, asked together with this one, still waits its turn.".to_string()),
            (false, k) => Some(format!("{k} other comments, asked together with this one, still wait their turn.")),
        };
        let yes = Item::new("Take it back", Action::Unsend(change.into(), id.into()));
        let mut m = Menu::ask(title, yes, "Keep it in the queue", true);
        m.note = note;
        Some(m)
    }

    /// Comment (change, id) taken back from the agent's queue, the comment
    /// open: the status line says what was taken out, and what waited behind
    /// it goes. The agent's turn come meanwhile, it says so.
    pub(super) fn take_back(&mut self, change: &str, id: &str) {
        let taken = ask::take_back(&self.path, change, id);
        self.reload();
        match taken {
            Ok(Some(q)) => {
                let of = self.db.note(change, id).map_or_else(|| "the comment".to_string(), |n| comment_of(&self.db, &self.stack, n));
                let alone = q.on.is_some() && ask::asked_with(&self.db, &self.stack, &q.batch, change, id) == 0;
                let then = if alone { "nothing goes back to the agent with the work you sent back" } else { "is not sent" };
                self.status = format!("the {} on {of} is taken out of the queue: the comment stays open, and {then}", ask_said(&q));
                // Written again, its line moved among the banner's comments,
                // the most recently updated first: the cursor goes with it.
                let line = BannerLine::Comment(change.to_string(), id.to_string());
                if let Some(k) = self.banner_lines().iter().position(|l| *l == line).filter(|_| self.on_banner) {
                    self.banner_sel = k;
                }
                self.launch();
            }
            Ok(None) if self.busy().contains_key(&(change.to_string(), id.to_string())) => {
                self.status = "the agent's turn came for this comment meanwhile: the agent works on it now".into();
            }
            Ok(None) => self.status = "this comment no longer waits its turn in the agent's queue".into(),
            Err(e) => self.status = e.0,
        }
    }

    /// The try pushes of the stack under way: their runs, the first started
    /// first.
    #[cfg(feature = "try-push")]
    pub(super) fn pushes(&self) -> Vec<String> {
        self.db
            .live_runs()
            .into_iter()
            .filter(|r| r.stack == self.stack && r.state == crate::db::RunState::Started && r.push.is_some())
            .map(|r| r.id.clone())
            .collect()
    }

    /// Where try push `run` is, said on its banner line and in the banner's
    /// head: what mach does, then Lando's state of it, then its tasks.
    #[cfg(feature = "try-push")]
    fn push_progress(&self, run: &str) -> String {
        let Some(p) = self.db.run(run).and_then(|r| r.push.as_ref()) else { return String::new() };
        crate::try_push::state_said(p).trim_start_matches("try push: ").to_string()
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

    /// What the banner says in its line: how much landed work waits for
    /// review, and which the next is; where a pull from Phabricator under way is; how
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
        if landed > 0 {
            // The asks' work, your edits by hand and the reviews pulled,
            // each counted.
            let kind = |is: fn(&crate::db::Run) -> bool| {
                lines.iter().filter(|l| matches!(l, BannerLine::Landed(run) if self.db.run(run).is_some_and(is))).count()
            };
            let (pulls, hands, lints) = (kind(crate::held::pulled), kind(crate::held::by_hand), kind(crate::held::linted));
            let reconciles = kind(crate::reconcile::is);
            let mut kinds = Vec::new();
            for (n, one, many) in [
                (landed - pulls - hands - lints - reconciles, "landed ask", "landed asks"),
                (hands, "edit by hand", "edits by hand"),
                (lints, "lint of the stack", "lints of the stack"),
                (reconciles, "reconcile with HEAD", "reconciles with HEAD"),
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
        // The try pushes under way: where the one is, or how many.
        #[cfg(feature = "try-push")]
        let pushes = {
            let pushes = self.pushes();
            match pushes.as_slice() {
                [] => {}
                [one] => said.push(format!("1 try push: {}", self.push_progress(one))),
                many => said.push(format!("{} try pushes", many.len())),
            }
            pushes.len()
        };
        #[cfg(not(feature = "try-push"))]
        let pushes = 0;
        // The stack's lint job: where it is.
        let lint = crate::lint::pending(&self.db, &self.stack);
        if let Some(r) = lint {
            said.push(format!("the linter {}", self.lint_progress(&r.id)));
        }
        let linting = usize::from(lint.and_then(|r| r.lint.as_deref()).is_some_and(|j| j.step == crate::lint::Step::Running));
        // The reconcile with HEAD waiting its turn.
        if lines.iter().any(|l| matches!(l, BannerLine::Reconcile(_))) {
            said.push("Reconcile with HEAD waits its turn".into());
        }
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
        // As its lines mark them, and the top bar the agents going (●).
        let going = going + pushes + linting;
        // A lint job waiting its turn, as a comment waiting its turn is.
        let ready = ready + usize::from(lint.is_some()) - linting;
        let mark = match (landed, going, pulling, ready, batched) {
            (0, 0, false, 0, 0) => "○",
            (0, 0, false, 0, _) => "◇",
            (0, 0, false, _, _) => "◌",
            (0, 0, true, _, _) => "⇣",
            (0, _, _, _, _) => "●",
            _ => "⟳",
        };
        Some(format!(" {mark} {}", said.join(" · ")))
    }

    /// The banner's menu, to pick from, all of it: each landed ask and each
    /// pull, to review; the stack's reconcile with HEAD waiting its turn, to
    /// drop; the stack's lint job, to read and to stop or drop; the batch, to
    /// send; every comment the agent can be asked about, checked if it is in
    /// the batch; then each comment open, the most recently updated first,
    /// to go to, and, waiting its turn, to take back from the queue.
    pub(super) fn banner_items(&self, items: &mut Vec<Item>) {
        for id in self.queue() {
            let Some((r, _)) = self.work(&id) else { continue };
            let first = ask::guessing(&self.db, r).is_none();
            let left = crate::held::left(&self.db, r);
            items.push(Item::new(format!("Review {left}{}", landed_how(r, 40)), Action::Review(id.clone(), None)).when(first));
        }
        items.extend(self.reconcile_drop_item());
        // The stack's lint job: what the linter writes, as Enter on its line
        // shows it; the linter stopped, or the job dropped.
        if let Some(r) = crate::lint::pending(&self.db, &self.stack) {
            items.push(Item::new("Read what the linter writes", Action::Transcript(r.id.clone())));
            items.extend(self.lint_stop_item(&r.id));
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
            let said = self.comment_said(n);
            let waiting = n.queued.as_ref().is_some_and(|q| !q.batched());
            items.push(Item::new(format!("Go to {said}"), Action::GoToComment(change.clone(), id.clone())));
            // Waiting its turn: taken back, once asked, the comment open.
            if waiting {
                items.push(Item::new(format!("Take back the ask on {said}…"), Action::AskUnsend(change, id)));
            }
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
        #[cfg(feature = "try-push")]
        {
            let tries = count(Way::Try);
            if tries > 0 {
                let label = format!("Send {tries} try push{}, each on its own", if tries == 1 { "" } else { "es" });
                out.push(Item::new(label, Action::SendBatch(Way::Try, true)).when(w).with(effort(Way::Try)));
            }
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
        #[cfg(feature = "try-push")]
        let tries = count(Way::Try);
        #[cfg(not(feature = "try-push"))]
        let tries = 0;
        for (n, one, many) in [
            (count(Way::Change), "edit", "edits"),
            (count(Way::Answer), "answer only", "answers only"),
            (count(Way::Test), "build and test", "builds and tests"),
            (tries, "try push", "try pushes"),
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
            #[cfg(feature = "try-push")]
            BannerLine::Push(run) => {
                let of = self
                    .db
                    .run(run)
                    .and_then(|r| r.push.as_ref())
                    .and_then(|p| self.db.note(&p.change, &p.id))
                    .map_or(String::new(), |n| format!(" of {}", self.comment_said(n)));
                format!("   ⇡ Try push{of}: {}", self.push_progress(run))
            }
            // The stack's lint job: where it is, then, running, the last line
            // of its stream.
            BannerLine::Lint(run) => {
                let Some(r) = self.db.run(run) else { return String::new() };
                let running = r.lint.as_deref().is_some_and(|j| j.step == crate::lint::Step::Running);
                let mark = if running { "●" } else { "◌" };
                let doing = ask::tail(&self.db, run, 1)
                    .pop()
                    .map(|l| l.trim().to_string())
                    .filter(|l| running && !l.is_empty())
                    .map_or(String::new(), |l| format!(" · {l}"));
                format!("   {mark} The linter {} ({}){doing}", self.lint_progress(run), r.request)
            }
            BannerLine::Reconcile(run) => {
                let of = self.db.run(run).map_or(String::new(), |r| r.request.clone());
                format!("   ◌ Reconcile with HEAD, waiting its turn: {of}")
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
    /// stack's header; Enter reviews what landed, opens the menu on the
    /// batch, goes to the comment, or, on a reconcile with HEAD waiting its
    /// turn, opens its menu; Esc goes back where the cursor was.
    pub(super) fn banner_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.banner_lines();
        let last = lines.len().saturating_sub(1);
        let page = (self.body_h / 2).max(1);
        match k.code {
            KeyCode::Enter => match lines.get(self.banner_sel).cloned() {
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
                // A try push: to the comment that asked for it.
                #[cfg(feature = "try-push")]
                Some(BannerLine::Push(run)) => {
                    if let Some(p) = self.db.run(&run).and_then(|r| r.push.clone()) {
                        self.on_banner = false;
                        self.go_to(&p.change, &p.id);
                    }
                }
                // The stack's lint job: what the linter writes.
                Some(BannerLine::Lint(run)) => {
                    self.on_banner = false;
                    return self.perform(Action::Transcript(run));
                }
                // A reconcile with HEAD waiting its turn: its menu, what it
                // waits for, and its drop.
                Some(BannerLine::Reconcile(run)) => {
                    if let Some(m) = self.reconcile_menu(&run) {
                        self.mode = Mode::Menu(m);
                    }
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
                    1 => " read only, to review",
                    2 => " edits, to review",
                    4 => " try pushes, followed",
                    5 => " the linter",
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
