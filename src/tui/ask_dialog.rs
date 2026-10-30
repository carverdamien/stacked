//! The dialog every "Ask agent…" item opens, in the place of asking from
//! the menu: a comment's, a question's on the history, the comments listed
//! (`Ask agent about all…`), and the comment being written, from the menu
//! Esc opens. On its first line, what the agent may do, its permission; on
//! the second, how hard it works, its effort. Both are unset as it opens,
//! and each is set with Enter on its line: the first option, then each
//! Enter the next, round past the last; ← → change nothing there, so that
//! nothing is set by mistake. Under them, its buttons: Ask Now, Ask Later
//! (in the stack's batch) and Cancel, which Esc is too. Ask Now and Ask
//! Later are grayed until both lines are set, and Ask Now while the agent
//! cannot be reached (`reach`): its program not found, or the network not
//! reaching the host claude talks to, checked away from the screen as the
//! dialog opens, unless it was a moment ago. The dialog says what keeps
//! each one grayed. Nothing set in it is kept: each dialog opens unset.

use super::*;
use crate::reach;

/// What the agent may do, as the dialog's first line sets it: its options,
/// in this order, those that apply where it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Answer,
    ChangePatch,
    ChangeStack,
    Test,
    #[cfg(feature = "try-push")]
    Try,
}

impl Permission {
    /// As the dialog draws it.
    pub fn label(self) -> &'static str {
        match self {
            Permission::Answer => "answer only",
            Permission::ChangePatch => "change patch",
            Permission::ChangeStack => "change stack",
            Permission::Test => "build and test",
            #[cfg(feature = "try-push")]
            Permission::Try => "try push",
        }
    }

    /// How the agent is asked.
    pub fn way(self) -> Way {
        match self {
            Permission::Answer => Way::Answer,
            Permission::ChangePatch | Permission::ChangeStack => Way::Change,
            Permission::Test => Way::Test,
            #[cfg(feature = "try-push")]
            Permission::Try => Way::Try,
        }
    }

    /// What it lets the agent do, as the line under the permission says
    /// it: of one comment, or of each of the comments listed.
    fn said(self, listed: bool) -> &'static str {
        match (self, listed) {
            (Permission::Answer, false) => "The agent answers, and changes nothing.",
            (Permission::Answer, true) => "The agent answers each comment, and changes nothing.",
            (Permission::ChangePatch, false) => "The agent may change the code within the comment's place: its patch, and no other.",
            (Permission::ChangePatch, true) => "The agent may change the code within each comment's place: its patch, and no other.",
            (Permission::ChangeStack, _) => {
                "The agent works with the whole stack in its worktree: it may change any patch, add patches anywhere and reorder them."
            }
            (Permission::Test, _) => "The agent builds and tests in the stack's build worktree, and changes nothing.",
            #[cfg(feature = "try-push")]
            (Permission::Try, _) => {
                "The agent selects the CI tasks the comment asks for, for a try push, each with why: it pushes nothing and changes nothing, and the tasks wait for your review."
            }
        }
    }

    /// What a comment is asked for with it, as the dialog says that none of
    /// those listed can be.
    fn asked(self) -> &'static str {
        match self {
            Permission::Answer => "to answer",
            Permission::ChangePatch | Permission::ChangeStack => "to change the code",
            Permission::Test => "to build and test",
            #[cfg(feature = "try-push")]
            Permission::Try => "for a try push",
        }
    }
}

/// What a dialog asks the agent about.
#[derive(Debug, Clone, PartialEq)]
pub enum AskOf {
    /// Comment (change, id), from its menu.
    Comment(String, String),
    /// The comments listed that can be asked about, from the menu of the
    /// stack's header or of the comments pane's header: build and test
    /// among the ways if `test`.
    Listed { test: bool },
    /// The comment being written, from the menu Esc opens: saved first if
    /// it `changed`, else asked about as it is.
    Written { changed: bool },
}

/// The dialog's buttons, from the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Now,
    Later,
    Cancel,
}

impl Button {
    pub const ALL: [Button; 3] = [Button::Now, Button::Later, Button::Cancel];

    pub fn label(self) -> &'static str {
        match self {
            Button::Now => "Ask Now",
            Button::Later => "Ask Later",
            Button::Cancel => "Cancel",
        }
    }

    /// As the dialog draws it: `< Ask Now >`.
    pub fn drawn(self) -> String {
        format!("< {} >", self.label())
    }

    /// Its place, from the left.
    pub fn at(self) -> usize {
        Button::ALL.iter().position(|b| *b == self).unwrap_or(0)
    }
}

/// The dialog's lines, from the top: the permission's, the effort's, the
/// buttons'.
pub const PERMISSION_LINE: usize = 0;
pub const EFFORT_LINE: usize = 1;
pub const BUTTONS_LINE: usize = 2;

/// The dialog that asks the agent, open.
#[derive(Debug, Clone, PartialEq)]
pub struct AskDialog {
    /// What its box says on its top: what is asked about.
    pub title: String,
    pub about: AskOf,
    /// Whether what it asks about is on the stack rather than a patch: its
    /// change stack then changes the stack as any of the stack's comments
    /// does, rather than a patch's comment asked of the whole stack.
    pub on_stack: bool,
    /// The permissions that apply here, in order; of the comments listed,
    /// each with how many of them it asks about.
    pub permissions: Vec<(Permission, Option<usize>)>,
    /// The permission set, by its place among those: none until Enter sets
    /// one.
    pub permission: Option<usize>,
    /// The effort set: none until Enter sets one.
    pub effort: Option<Effort>,
    /// The line the cursor is on: `PERMISSION_LINE`, `EFFORT_LINE` or
    /// `BUTTONS_LINE`.
    pub line: usize,
    /// The button the cursor is on, on the buttons' line.
    pub button: Button,
    /// Why the agent's program cannot be run, if it cannot: Ask Now is
    /// grayed.
    pub missing: Option<String>,
    /// Where the network is checked to reach, if it is checked.
    pub target: Option<reach::Target>,
    /// The cell of the screen that was clicked to open it, when a click
    /// opened it: it is drawn next to that cell.
    pub at: Option<(u16, u16)>,
}

impl AskDialog {
    /// The permission set, if one is.
    pub fn permission(&self) -> Option<Permission> {
        self.permission.and_then(|k| self.permissions.get(k)).map(|(p, _)| *p)
    }

    /// The permissions that apply here, as drawn, in order.
    pub fn permission_labels(&self) -> Vec<&'static str> {
        self.permissions.iter().map(|(p, _)| p.label()).collect()
    }

    /// Whether both lines are set.
    pub fn set(&self) -> bool {
        self.permission.is_some() && self.effort.is_some()
    }

    /// Whether `p` asks a patch's comment of the whole stack.
    fn whole(&self, p: Permission) -> bool {
        p == Permission::ChangeStack && !self.on_stack && !matches!(self.about, AskOf::Listed { .. })
    }

    /// Of the comments listed, how many the permission set asks about.
    fn count(&self) -> Option<usize> {
        self.permission.and_then(|k| self.permissions.get(k)).and_then(|(_, n)| *n)
    }

    /// Enter on the permission's line: the first, then the next, round
    /// past the last.
    fn step_permission(&mut self) {
        let n = self.permissions.len().max(1);
        self.permission = Some(self.permission.map_or(0, |k| (k + 1) % n));
    }

    /// Enter on the effort's line: low, then the next level, round past the
    /// last.
    fn step_effort(&mut self) {
        let n = Effort::ALL.len();
        self.effort = Some(self.effort.map_or(Effort::ALL[0], |e| Effort::ALL[(e as usize + 1) % n]));
    }
}

/// `labels`, in the order Enter sets them, round past the last: `answer
/// only first, then change patch and change stack, round past the last`;
/// one alone, `answer only, the only one here`.
fn in_turn(labels: &[&str]) -> String {
    match labels {
        [] => String::new(),
        [one] => format!("{one}, the only one here"),
        [first, rest @ ..] => {
            let (last, between) = rest.split_last().unwrap_or((first, &[]));
            match between {
                [] => format!("{first} first, then {last}, round past the last"),
                _ => format!("{first} first, then {} and {last}, round past the last", between.join(", ")),
            }
        }
    }
}

/// What a dialog opens with: what its box says on its top, whether what it
/// asks about is on the stack, and the permissions that apply there.
type Opened = (String, bool, Vec<(Permission, Option<usize>)>);

/// Whether Ask Now can reach the agent, as the dialog says it.
enum Reach {
    Yes,
    /// The network being checked: to reach the host, said.
    Checking(String),
    /// The agent's program not found: why.
    Missing(String),
    /// The network not reaching the host, said.
    Unreached(String),
}

impl App {
    /// The dialog that asks the agent about `about`, opened with nothing
    /// set: the agent's program looked for, and the network checked away
    /// from the screen, unless it was a moment ago.
    pub(super) fn open_ask(&mut self, about: AskOf) {
        let Some((title, on_stack, permissions)) = self.asking_of(&about) else {
            self.status = "there is nothing to ask the agent about here any more".into();
            return;
        };
        let missing = self.agent_missing();
        let target = self.reach_target();
        if missing.is_none()
            && let Some(t) = &target
        {
            self.check_reach(t);
        }
        self.mode = Mode::Ask(Box::new(AskDialog {
            title,
            about,
            on_stack,
            permissions,
            permission: None,
            effort: None,
            line: PERMISSION_LINE,
            button: Button::Now,
            missing,
            target,
            at: None,
        }));
    }

    /// What the dialog asking about `about` says on its top, whether that is
    /// on the stack, and the permissions that apply, in order, as the menus
    /// had them: a patch's comment may be asked to change its patch, or the
    /// whole stack; the stack's, the stack; the stack's and a patch's as a
    /// whole, to build and test, and, built with the `try-push` feature, to
    /// select the CI tasks of a try push (not in the tour, whose game has
    /// no CI to push to); a question on the history, to answer alone. None
    /// if what it asks about is gone.
    fn asking_of(&self, about: &AskOf) -> Option<Opened> {
        let ways = |patch: bool, test: bool| {
            let mut out = vec![Permission::Answer];
            if patch {
                out.push(Permission::ChangePatch);
            }
            out.push(Permission::ChangeStack);
            if test {
                out.push(Permission::Test);
            }
            #[cfg(feature = "try-push")]
            if test && !self.in_memory() {
                out.push(Permission::Try);
            }
            out.into_iter().map(|p| (p, None)).collect::<Vec<_>>()
        };
        match about {
            AskOf::Comment(change, id) => {
                let n = self.db.note(change, id)?;
                if let Some(rev) = n.about {
                    return Some((format!("ask the agent about the question on @{rev}"), true, vec![(Permission::Answer, None)]));
                }
                let title = format!("ask the agent about the comment on {}", where_on(&self.db, &self.stack, n));
                Some((title, n.on_stack(), ways(!n.on_stack(), ask::can_test(&self.db, n))))
            }
            AskOf::Listed { test } => {
                let place = if self.on_stack { "the stack".to_string() } else { format!("patch {}", self.sel + 1) };
                let change = if self.on_stack { Permission::ChangeStack } else { Permission::ChangePatch };
                let mut all = vec![
                    (Permission::Answer, Some(self.listed_sendable(Way::Answer).len())),
                    (change, Some(self.listed_sendable(Way::Change).len())),
                ];
                if *test {
                    all.push((Permission::Test, Some(self.listed_sendable(Way::Test).len())));
                }
                Some((format!("ask the agent about the comments listed on {place}"), self.on_stack, all))
            }
            AskOf::Written { changed } => {
                let e = self.pending.as_ref()?;
                let title = self.written_title(&e.target, *changed);
                if self.answer_only(&e.target) {
                    return Some((title, true, vec![(Permission::Answer, None)]));
                }
                let (patch, test) = self.written_ways(&e.target);
                Some((title, !patch, ways(patch, test)))
            }
        }
    }

    /// Of a comment written for `t`: whether it is a patch's, which may be
    /// asked of the whole stack; and whether it may be asked to build and
    /// test, and for a try push, as the stack's comments and a patch's as a
    /// whole may: one written there, or one there replied to or written
    /// again, as its own menu has them.
    fn written_ways(&self, t: &Target) -> (bool, bool) {
        let patch = match t {
            Target::Patch(_) | Target::Line { .. } => true,
            Target::Reply { change, .. } | Target::Edit { change, .. } | Target::Turn { change, .. } | Target::Rewind { change, .. } => {
                *change != stack_change(&self.stack)
            }
            _ => false,
        };
        let test = match t {
            Target::Stack | Target::Patch(_) => true,
            Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. } => {
                self.db.note(change, id).is_some_and(|n| ask::can_test(&self.db, n))
            }
            _ => false,
        };
        (patch, test)
    }

    /// What the dialog says on its top of a comment written for `t`: saved
    /// first if it `changed`, else asked about as it is.
    fn written_title(&self, t: &Target, changed: bool) -> String {
        let patch = |change: &str| {
            self.current()
                .and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == change))
                .map_or("a patch".to_string(), |i| format!("patch {}", i + 1))
        };
        let note = |change: &str, id: &str| self.db.note(change, id).map_or("its place".to_string(), |n| where_on(&self.db, &self.stack, n));
        let (what, place) = match t {
            Target::Stack => ("comment", "the stack".to_string()),
            Target::Patch(change) => ("comment", format!("{}, as a whole", patch(change))),
            Target::Line { change, key, file, .. } if crate::diff::is_file_key(key) => ("comment", format!("{} · {file}, as a whole", patch(change))),
            Target::Line { change, file, preview, .. } => ("comment", format!("{} · {file}: {}", patch(change), short(preview, 30))),
            Target::Reply { change, id } => ("reply", note(change, id)),
            Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. } => ("comment", note(change, id)),
            Target::About(rev) => ("question", format!("@{rev}")),
            _ => ("comment", "its place".to_string()),
        };
        match changed {
            true => format!("save the {what} on {place}, then ask the agent about it"),
            false => format!("ask the agent about the {what} on {place}, as it is"),
        }
    }

    /// Why the agent's program cannot be run, if it cannot: the program the
    /// tests stand in with, else the one the settings name
    /// (`$STACKED_CLAUDE`, `agent.command`, else `claude`), looked for as
    /// the asks run it. Never in the tour, whose agent is a stand-in in
    /// memory.
    fn agent_missing(&self) -> Option<String> {
        if self.in_memory() {
            return None;
        }
        let program = ask::claude_stand_in().map_or_else(|| self.cfg.agent_command(), |p| p.to_string_lossy().into_owned());
        reach::missing(&program, std::env::var_os("PATH").as_deref())
    }

    /// Where the network is checked to reach, as a dialog opens: none where
    /// it is not checked, in the tour, with `agent.reach_check` off in the
    /// settings, or from a process that keeps nothing with no stand-in set
    /// (`reach::checker`).
    fn reach_target(&self) -> Option<reach::Target> {
        if self.in_memory() || !self.cfg.agent.reach_check || reach::checker(&self.path).is_none() {
            return None;
        }
        Some(reach::target_here())
    }

    /// The network checked to reach `t`, away from the screen: unless it is
    /// being checked already, or was a moment ago (`reach::KEPT`).
    fn check_reach(&mut self, t: &reach::Target) {
        if self.bg.reaching.is_some() {
            return;
        }
        if self.reached.as_ref().is_some_and(|(at, _, when)| at == t && when.elapsed() < reach::KEPT) {
            return;
        }
        let Some(check) = reach::checker(&self.path) else { return };
        self.bg.reaching = Some(t.clone());
        let t = t.clone();
        self.spawn(move || {
            let ok = check(&t);
            Done::Reached(t, ok)
        });
    }

    /// What checking whether the network reaches `t` came to: kept, for
    /// the dialogs opened in a moment, and the one open, which says it.
    pub(super) fn reach_found(&mut self, t: reach::Target, ok: bool) {
        self.bg.reaching = None;
        self.reached = Some((t, ok, Instant::now()));
    }

    /// Whether Ask Now can reach the agent, for dialog `d`.
    fn reach_of(&self, d: &AskDialog) -> Reach {
        if let Some(why) = &d.missing {
            return Reach::Missing(why.clone());
        }
        let Some(t) = &d.target else { return Reach::Yes };
        if self.bg.reaching.as_ref() == Some(t) {
            return Reach::Checking(t.said.clone());
        }
        match &self.reached {
            Some((at, true, _)) if at == t => Reach::Yes,
            Some((at, false, _)) if at == t => Reach::Unreached(t.said.clone()),
            _ => Reach::Checking(t.said.clone()),
        }
    }

    /// Which of dialog `d`'s buttons can be pressed, from the left: Ask Now
    /// and Ask Later once both lines are set (and, of the comments listed,
    /// one is asked about), Ask Now while the agent can be reached; Cancel
    /// always.
    pub fn ask_buttons(&self, d: &AskDialog) -> [bool; 3] {
        let ready = d.set() && d.count() != Some(0);
        [ready && matches!(self.reach_of(d), Reach::Yes), ready, true]
    }

    /// What dialog `d` says under its buttons: what keeps Ask Now and Ask
    /// Later grayed, if anything does.
    pub fn ask_note(&self, d: &AskDialog) -> Vec<String> {
        let mut out = Vec::new();
        match (d.permission(), d.effort) {
            (None, None) => out.push("Ask Now and Ask Later are grayed until the permission and the effort are set.".to_string()),
            (None, Some(_)) => out.push("Ask Now and Ask Later are grayed until the permission is set.".to_string()),
            (Some(_), None) => out.push("Ask Now and Ask Later are grayed until the effort is set.".to_string()),
            (Some(p), Some(_)) if d.count() == Some(0) => {
                out.push(format!("Ask Now and Ask Later are grayed: no comment listed can be asked {}.", p.asked()));
            }
            _ => {}
        }
        match self.reach_of(d) {
            Reach::Yes => {}
            Reach::Checking(said) => out.push(format!("Ask Now is grayed while stacked checks that the network reaches {said}.")),
            Reach::Missing(why) => out.push(format!("Ask Now is grayed: {why}.")),
            Reach::Unreached(said) => {
                out.push(format!("Ask Now is grayed: the network does not reach {said}."));
                out.push("If claude reaches it all the same, `stacked config agent.reach_check false` turns this check off.".to_string());
            }
        }
        out
    }

    /// What the line under dialog `d`'s permission says: what the one set
    /// lets the agent do, and of the comments listed, how many it asks
    /// about; unset, what Enter sets.
    fn permission_said(&self, d: &AskDialog) -> String {
        let listed = matches!(d.about, AskOf::Listed { .. });
        match (d.permission(), d.count()) {
            (None, _) => format!("Not set yet: Enter sets {}.", in_turn(&d.permission_labels())),
            (Some(p), Some(0)) => format!("{} No comment listed can be asked so.", p.said(listed)),
            (Some(p), Some(1)) => format!("{} 1 comment listed is asked about.", p.said(listed)),
            (Some(p), Some(n)) => format!("{} {n} comments listed are asked about.", p.said(listed)),
            (Some(p), None) => p.said(listed).to_string(),
        }
    }

    /// What the line under dialog `d`'s effort says.
    fn effort_said(d: &AskDialog) -> String {
        match d.effort {
            None => format!("Not set yet: Enter sets {}.", in_turn(&Effort::NAMES)),
            Some(e) => format!("claude is given --effort {}.", e.name()),
        }
    }

    /// What dialog `d`'s lines say, from the top, as drawn: the
    /// permission's, the line under it, the effort's, the line under it.
    pub fn ask_lines(&self, d: &AskDialog) -> [String; 4] {
        let value = |v: Option<&str>| v.map_or("[ — ]".to_string(), |v| format!("[{v}]"));
        [
            format!("Permission: {}", value(d.permission().map(Permission::label))),
            self.permission_said(d),
            format!("Effort: {}", value(d.effort.map(Effort::name))),
            App::effort_said(d),
        ]
    }

    /// A key in the ask dialog `d`: ↑↓ from line to line and to the
    /// buttons; Enter steps a line, or presses the button the cursor is on;
    /// ← → from button to button, past those grayed, and nothing on the two
    /// lines; Esc cancels.
    pub(super) fn ask_key(&mut self, mut d: AskDialog, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Down => d.line = (d.line + 1) % 3,
            KeyCode::Up => d.line = (d.line + 2) % 3,
            KeyCode::Home => d.line = PERMISSION_LINE,
            KeyCode::End => d.line = BUTTONS_LINE,
            KeyCode::Left | KeyCode::Right if d.line == BUTTONS_LINE => {
                let on = self.ask_buttons(&d);
                let at = d.button.at();
                let next = match k.code {
                    KeyCode::Right => (at + 1..Button::ALL.len()).find(|n| on[*n]),
                    _ => (0..at).rev().find(|n| on[*n]),
                };
                if let Some(n) = next {
                    d.button = Button::ALL[n];
                }
            }
            // On the two lines, nothing: Enter alone sets them.
            KeyCode::Left | KeyCode::Right => {}
            KeyCode::Enter => match d.line {
                PERMISSION_LINE => d.step_permission(),
                EFFORT_LINE => d.step_effort(),
                _ => return self.press_ask(d),
            },
            KeyCode::Esc => return self.cancel_ask(d),
            _ => {}
        }
        self.mode = Mode::Ask(Box::new(d));
        Effect::None
    }

    /// The button the cursor is on in dialog `d`, pressed: grayed, nothing
    /// done, and the status says why.
    fn press_ask(&mut self, d: AskDialog) -> Effect {
        if !self.ask_buttons(&d)[d.button.at()] {
            let label = d.button.label();
            self.status = self.ask_note(&d).into_iter().find(|l| l.contains(label)).unwrap_or_else(|| format!("{label} is grayed."));
            self.mode = Mode::Ask(Box::new(d));
            return Effect::None;
        }
        match d.button {
            Button::Cancel => self.cancel_ask(d),
            b => {
                self.ask_from(d, b == Button::Later);
                Effect::None
            }
        }
    }

    /// Dialog `d` closed, nothing asked: from the menu of the comment being
    /// written, back to writing it, as it was.
    fn cancel_ask(&mut self, d: AskDialog) -> Effect {
        self.mode = Mode::Normal;
        if matches!(d.about, AskOf::Written { .. })
            && let Some(e) = self.pending.take()
        {
            self.mode = Mode::Edit(Box::new(e));
        }
        Effect::None
    }

    /// What dialog `d` asks, asked as its lines set it: now, or `later`, in
    /// the stack's batch.
    fn ask_from(&mut self, d: AskDialog, later: bool) {
        let (Some(p), Some(effort)) = (d.permission(), d.effort) else { return };
        let (way, whole) = (p.way(), d.whole(p));
        self.mode = Mode::Normal;
        match d.about {
            AskOf::Comment(change, id) => self.now_or_later(vec![(change, id)], way, whole, effort, later),
            AskOf::Listed { .. } => {
                let notes = self.listed_sendable(way);
                self.now_or_later(notes, way, false, effort, later);
            }
            AskOf::Written { .. } => self.ask_written(way, whole, effort, later),
        }
    }

    /// `notes` asked about now, on the agent's queue, or `later`, in the
    /// stack's batch.
    fn now_or_later(&mut self, notes: Vec<(String, String)>, way: Way, whole: bool, effort: Effort, later: bool) {
        match later {
            true => self.batch(notes, way, whole, effort),
            false => self.send(notes, way, whole, effort),
        }
    }

    /// The comment being written saved, unless it is as it was, then asked
    /// about, now or `later`: taken by the agent meanwhile, it stays in the
    /// editor; not saved, nothing is asked, and the comment goes back on
    /// the queue as it was, if it was on it.
    fn ask_written(&mut self, way: Way, whole: bool, effort: Effort, later: bool) {
        let Some(e) = self.pending.take() else { return };
        if let Some(why) = self.locked_target(&e.target) {
            self.status = why;
            self.mode = Mode::Edit(Box::new(e));
            return;
        }
        let text = e.ed.text();
        match self.save_attached(&e.target, &text, &e.attached) {
            Some(saved) => self.now_or_later(vec![saved], way, whole, effort, later),
            None => self.settle_edit(&e, false),
        }
        self.say_detached();
    }

    /// What can be done in dialog `d`, said: what Enter does first, then
    /// where the arrows go, and that nothing is kept.
    pub(super) fn ask_help(&self, d: &AskDialog) -> Vec<String> {
        let on = self.ask_buttons(d);
        let written = match d.about {
            AskOf::Written { changed: true } => "save the comment, then ",
            _ => "",
        };
        let first = match (d.line, d.button) {
            (PERMISSION_LINE, _) => format!(
                "Press Enter to set what the agent may do: {}. The line under the permission says what the one set lets the agent do.",
                in_turn(&d.permission_labels())
            ),
            (EFFORT_LINE, _) => format!(
                "Press Enter to set how hard the agent works, as claude's --effort takes it: {}.",
                in_turn(&Effort::NAMES)
            ),
            (_, b) if !on[b.at()] => self.ask_note(d).into_iter().find(|l| l.contains(b.label())).unwrap_or_else(|| format!("{} is grayed.", b.label())),
            (_, Button::Now) => format!("Press Enter to {written}ask the agent now, as the two lines say: the ask starts at once, or waits its turn."),
            (_, Button::Later) => {
                format!("Press Enter to {written}put the ask in the stack's batch, as the two lines say: it waits there until the banner sends the batch.")
            }
            (_, Button::Cancel) => match d.about {
                AskOf::Written { .. } => "Press Enter to close the dialog and go back to writing the comment: nothing is saved, and nothing is asked.".into(),
                _ => "Press Enter to close the dialog: nothing is asked.".into(),
            },
        };
        vec![
            first,
            "Press ↑↓ to go from line to line and to the buttons. On the buttons, ← → go from one to the next; on the two lines, they change nothing. Esc closes the dialog, as Cancel does."
                .into(),
            "Nothing is set as the dialog opens, and nothing set in it is kept: each dialog opens unset.".into(),
        ]
    }

    /// The keys there are in dialog `d`.
    pub(super) fn ask_hint(d: &AskDialog) -> String {
        match d.line {
            PERMISSION_LINE => "↑↓ choose · enter: set the permission · esc: cancel · ?: what it does".into(),
            EFFORT_LINE => "↑↓ choose · enter: set the effort · esc: cancel · ?: what it does".into(),
            _ => "↑↓ choose · ←→: another button · enter: press it · esc: cancel · ?: what it does".into(),
        }
    }

    /// The ask dialog, if one is open: its lines, the line under each, its
    /// buttons, then what keeps them grayed; beside the comment being
    /// written, as a menu is, or next to the cell clicked to open it.
    pub(super) fn draw_ask(&mut self, buf: &mut Buffer, body: Rect) {
        let Mode::Ask(d) = &self.mode else { return };
        let d = (**d).clone();
        let on = self.ask_buttons(&d);
        let note = self.ask_note(&d);
        let [permission, permission_said, effort, effort_said] = self.ask_lines(&d);
        let screen = buf.area;
        let area = match d.at {
            Some(_) => Rect::new(screen.x, screen.y + 1, screen.width, screen.height.saturating_sub(2)),
            None => body,
        };
        let (w, h) = (area.width as usize, area.height as usize);
        let bw = (d.title.chars().count() + 6).clamp(64, 78).min(w.saturating_sub(2));
        // The lines under the two, indented, as many as the longest of
        // what they may say takes, so that the box keeps its size as they
        // are set.
        let said_w = bw.saturating_sub(6).max(1);
        let tallest = d
            .permissions
            .iter()
            .enumerate()
            .map(|(k, _)| AskDialog { permission: Some(k), ..d.clone() })
            .chain([AskDialog { permission: None, ..d.clone() }])
            .map(|one| wrap(&self.permission_said(&one), said_w).len())
            .max()
            .unwrap_or(1);
        let mut under_permission = wrap(&permission_said, said_w);
        under_permission.resize(tallest, String::new());
        let unset = wrap(&App::effort_said(&AskDialog { effort: None, ..d.clone() }), said_w).len();
        let mut under_effort = wrap(&effort_said, said_w);
        under_effort.resize(unset.max(under_effort.len()), String::new());
        let note: Vec<String> = note.iter().flat_map(|l| wrap(l, bw.saturating_sub(4))).collect();
        // Each line from the top: what it says, in its style, and what a
        // click there is on; none for the buttons' line.
        let line_st = |k: usize| if d.line == k { on_cursor(Style::new()) } else { Style::new() };
        type Drawn = Option<(String, Style, Option<Spot>)>;
        let plain = |text: String| -> Drawn { Some((text, Style::new(), None)) };
        let mut rows: Vec<Drawn> = vec![Some((permission, line_st(PERMISSION_LINE), Some(Spot::AskLine(PERMISSION_LINE))))];
        rows.extend(under_permission.iter().map(|l| plain(format!("  {l}"))));
        rows.push(Some((effort, line_st(EFFORT_LINE), Some(Spot::AskLine(EFFORT_LINE)))));
        rows.extend(under_effort.iter().map(|l| plain(format!("  {l}"))));
        rows.push(plain(String::new()));
        rows.push(None);
        if !note.is_empty() {
            rows.push(plain(String::new()));
            rows.extend(note.iter().map(|l| Some((l.clone(), color::note(), None))));
        }
        let bh = (rows.len() + 2).min(h);
        let (x, y) = self.boxed_at(area, bw, bh, d.at);
        window(buf, area, x, y, bw, bh, &d.title, color::meta());
        spot_in(&self.spots, area, (x, y, bw, bh), Spot::Box);
        for (k, row) in rows.iter().enumerate().take(bh.saturating_sub(2)) {
            let r = y + 1 + k;
            match row {
                Some((text, st, spot)) => {
                    put(buf, area, x + 1, r, bw - 2, &fit(&format!(" {text}"), bw - 2), *st);
                    if let Some(s) = spot {
                        spot_in(&self.spots, area, (x + 1, r, bw - 2, 1), *s);
                    }
                }
                // The buttons, each a spot of its own: grayed, dim; the one
                // the cursor is on, on the buttons' line, under the cursor.
                None => {
                    let mut bx = x + 2;
                    for (n, b) in Button::ALL.into_iter().enumerate() {
                        let drawn = b.drawn();
                        let len = drawn.chars().count();
                        let mut st = if on[n] { Style::new() } else { color::dim() };
                        if d.line == BUTTONS_LINE && d.button == b {
                            st = on_cursor(st);
                        }
                        if bx + len < x + bw - 1 {
                            put(buf, area, bx, r, len, &drawn, st);
                            spot_in(&self.spots, area, (bx, r, len, 1), Spot::AskButton(b));
                        }
                        bx += len + 3;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_are_said_in_the_order_enter_sets_them() {
        assert_eq!(in_turn(&["answer only"]), "answer only, the only one here");
        assert_eq!(in_turn(&["answer only", "change stack"]), "answer only first, then change stack, round past the last");
        assert_eq!(
            in_turn(&["answer only", "change patch", "change stack"]),
            "answer only first, then change patch and change stack, round past the last"
        );
        assert_eq!(in_turn(&Effort::NAMES), "low first, then medium, high, xhigh and max, round past the last");
    }

    #[test]
    fn enter_steps_each_line_from_unset_round_past_the_last() {
        let mut d = AskDialog {
            title: String::new(),
            about: AskOf::Listed { test: false },
            on_stack: false,
            permissions: vec![(Permission::Answer, None), (Permission::ChangePatch, None), (Permission::ChangeStack, None)],
            permission: None,
            effort: None,
            line: PERMISSION_LINE,
            button: Button::Now,
            missing: None,
            target: None,
            at: None,
        };
        let mut seen = Vec::new();
        for _ in 0..4 {
            d.step_permission();
            seen.push(d.permission().map(Permission::label));
        }
        assert_eq!(seen, [Some("answer only"), Some("change patch"), Some("change stack"), Some("answer only")]);
        let mut seen = Vec::new();
        for _ in 0..6 {
            d.step_effort();
            seen.push(d.effort.map(Effort::name));
        }
        assert_eq!(seen, ["low", "medium", "high", "xhigh", "max", "low"].map(Some));
        // Of the comments listed, a change is never of the whole stack; of a
        // patch's comment, change stack is.
        assert!(!d.whole(Permission::ChangeStack));
        d.about = AskOf::Comment("c".into(), "i".into());
        assert!(d.whole(Permission::ChangeStack) && !d.whole(Permission::ChangePatch));
        d.on_stack = true;
        assert!(!d.whole(Permission::ChangeStack));
    }
}
