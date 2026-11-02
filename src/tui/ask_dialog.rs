//! The dialog every "Ask agent…" item opens, in the place of asking from
//! the menu: a comment's, a question's on the history, the comments listed
//! (`Ask agent about all…`), and the comment being written, from the menu
//! Esc opens. On its first line, what the agent may do, its permission; on
//! the second, how hard it works, its effort. Both are unset as it opens.
//! On a line, ← or → sets its first option, whichever the arrow, so that a
//! stray key never sets the most permissive or the most expensive; then →
//! the next and ← the one before, never round past either end, as the
//! menus' dropdowns go. Enter on a line opens the list of its options under
//! it, over the dialog, its cursor on the one set (the first, unset): ↑↓
//! there go round past either end as in a menu, Home and End to the first
//! and the last; Enter sets the line to the option the cursor is on, Esc
//! leaves the line as it was, and either closes the list alone. While it is
//! open, nothing else in the dialog takes keys. Under the lines, its
//! buttons: Ask Now, Ask Later (in the stack's batch) and Cancel, which Esc
//! is too. Ask Now and Ask Later are grayed until both lines are set, and
//! Ask Now while the agent cannot be reached (`reach`): its program not
//! found, or the network not reaching the host claude talks to, checked
//! away from the screen as the dialog opens, unless it was a moment ago.
//! The dialog says what keeps each one grayed. Nothing set in it is kept:
//! each dialog opens unset.

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
    /// The permission set, by its place among those: none until ← →, or
    /// the list Enter opens, sets one.
    pub permission: Option<usize>,
    /// The effort set: none until ← →, or the list Enter opens, sets one.
    pub effort: Option<Effort>,
    /// The line the cursor is on: `PERMISSION_LINE`, `EFFORT_LINE` or
    /// `BUTTONS_LINE`.
    pub line: usize,
    /// The list of the options of the line the cursor is on, open under
    /// that line: the place in it of the option its cursor is on. None
    /// while no list is open.
    pub list: Option<usize>,
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

    /// The options of line `line`, as drawn, in order: the permissions that
    /// apply here, or the efforts, low to max; none on the buttons' line.
    pub fn options(&self, line: usize) -> Vec<&'static str> {
        match line {
            PERMISSION_LINE => self.permission_labels(),
            EFFORT_LINE => Effort::NAMES.to_vec(),
            _ => vec![],
        }
    }

    /// The place among line `line`'s options of the one set, if one is.
    pub fn set_at(&self, line: usize) -> Option<usize> {
        match line {
            PERMISSION_LINE => self.permission,
            EFFORT_LINE => self.effort.map(|e| e as usize),
            _ => None,
        }
    }

    /// Line `line` set to its option at place `k`, if it has one there.
    fn set_to(&mut self, line: usize, k: usize) {
        match line {
            PERMISSION_LINE if k < self.permissions.len() => self.permission = Some(k),
            EFFORT_LINE if k < Effort::ALL.len() => self.effort = Some(Effort::ALL[k]),
            _ => {}
        }
    }

    /// ← (not `forward`) or → on line `line`: unset, its first option,
    /// whichever the arrow; else the next or the one before, never round
    /// past either end, so that ← held ends on the first and → held on the
    /// last.
    fn step(&mut self, line: usize, forward: bool) {
        let n = self.options(line).len();
        if n == 0 {
            return;
        }
        let k = match (self.set_at(line), forward) {
            (None, _) => 0,
            (Some(k), true) => (k + 1).min(n - 1),
            (Some(k), false) => k.saturating_sub(1),
        };
        self.set_to(line, k);
    }

    /// Enter on the line the cursor is on: the list of its options opened,
    /// its cursor on the one set, or on the first while the line is unset.
    fn open_list(&mut self) {
        self.list = Some(self.set_at(self.line).unwrap_or(0));
    }

    /// A key in the list open, the only keys the dialog takes while it is
    /// open: ↑↓ from option to option, round past either end as in a menu,
    /// Home and End to the first and the last; Enter sets the line to the
    /// option the cursor is on, and Esc leaves the line as it was: either
    /// closes the list alone, the cursor staying on that line. Any other key
    /// does nothing.
    fn list_key(&mut self, code: KeyCode) {
        let Some(at) = self.list else { return };
        let n = self.options(self.line).len().max(1);
        match code {
            KeyCode::Down => self.list = Some((at + 1) % n),
            KeyCode::Up => self.list = Some((at + n - 1) % n),
            KeyCode::Home => self.list = Some(0),
            KeyCode::End => self.list = Some(n - 1),
            KeyCode::Enter => {
                self.set_to(self.line, at);
                self.list = None;
            }
            KeyCode::Esc => self.list = None,
            _ => {}
        }
    }
}

/// `labels` one after the other, the last after `and`: `low, medium and
/// high`.
fn and_list(labels: &[&str]) -> String {
    match labels.split_last() {
        None => String::new(),
        Some((last, [])) => last.to_string(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// `labels`, one to pick, the last after `or`: `answer only, change patch
/// or change stack`; one alone, `answer only, the only one here`.
fn one_of(labels: &[&str]) -> String {
    match labels.split_last() {
        None => String::new(),
        Some((last, [])) => format!("{last}, the only one here"),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// What ← → and Enter do on a line not set yet, of options `labels` in
/// order: `← or → sets answer only, then steps to change patch and change
/// stack. Enter opens the list of them`; one alone, `← or → sets answer
/// only, the only one here. Enter opens the list, which holds that one
/// alone`.
fn unset_keys(labels: &[&str]) -> String {
    match labels {
        [] => String::new(),
        [one] => format!("← or → sets {one}, the only one here. Enter opens the list, which holds that one alone"),
        [first, rest @ ..] => format!("← or → sets {first}, then steps to {}. Enter opens the list of them", and_list(rest)),
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
            list: None,
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
    /// about; unset, what ← → set and what Enter opens.
    fn permission_said(&self, d: &AskDialog) -> String {
        let listed = matches!(d.about, AskOf::Listed { .. });
        match (d.permission(), d.count()) {
            (None, _) => format!("Not set yet: {}.", unset_keys(&d.permission_labels())),
            (Some(p), Some(0)) => format!("{} No comment listed can be asked so.", p.said(listed)),
            (Some(p), Some(1)) => format!("{} 1 comment listed is asked about.", p.said(listed)),
            (Some(p), Some(n)) => format!("{} {n} comments listed are asked about.", p.said(listed)),
            (Some(p), None) => p.said(listed).to_string(),
        }
    }

    /// What the line under dialog `d`'s effort says.
    fn effort_said(d: &AskDialog) -> String {
        match d.effort {
            None => format!("Not set yet: {}.", unset_keys(&Effort::NAMES)),
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
    /// buttons; on the two lines, ← → step the option set, the first from
    /// unset, and Enter opens the list of the line's options; on the
    /// buttons, ← → from button to button, past those grayed, and Enter
    /// presses the one the cursor is on; Esc cancels. The list open, its own
    /// keys alone (`AskDialog::list_key`).
    pub(super) fn ask_key(&mut self, mut d: AskDialog, k: KeyEvent) -> Effect {
        if d.list.is_some() {
            d.list_key(k.code);
            self.mode = Mode::Ask(Box::new(d));
            return Effect::None;
        }
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
            // On the two lines, the option set stepped: the first from
            // unset, whichever the arrow, then none past either end.
            KeyCode::Left | KeyCode::Right => d.step(d.line, k.code == KeyCode::Right),
            KeyCode::Enter => match d.line {
                PERMISSION_LINE | EFFORT_LINE => d.open_list(),
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
    /// where the arrows go, and that nothing is kept. The list open, what
    /// Enter sets there, and how to go through the list or leave it.
    pub(super) fn ask_help(&self, d: &AskDialog) -> Vec<String> {
        let kept = "Nothing is set as the dialog opens, and nothing set in it is kept: each dialog opens unset.".to_string();
        if let Some(at) = d.list {
            let (what, said) = match d.line {
                PERMISSION_LINE => ("permission", self.permission_said(&AskDialog { permission: Some(at), ..d.clone() })),
                _ => ("effort", App::effort_said(&AskDialog { effort: Effort::ALL.get(at).copied(), ..d.clone() })),
            };
            let option = d.options(d.line).get(at).copied().unwrap_or_default();
            return vec![
                format!("Press Enter to set the {what} to {option} and close the list. {said}"),
                format!("Press ↑↓ for another option, Esc to close the list and leave the {what} as it was."),
                kept,
            ];
        }
        let on = self.ask_buttons(d);
        let written = match d.about {
            AskOf::Written { changed: true } => "save the comment, then ",
            _ => "",
        };
        let first = match (d.line, d.button) {
            (PERMISSION_LINE, _) => format!(
                "Press Enter to open the list of what the agent may do here, to pick one: {}. The line under the permission says what the one set lets the agent do.",
                one_of(&d.permission_labels())
            ),
            (EFFORT_LINE, _) => format!(
                "Press Enter to open the list of how hard the agent works, as claude's --effort takes it, to pick one: {}.",
                one_of(&Effort::NAMES)
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
        let arrows = match d.line {
            PERMISSION_LINE | EFFORT_LINE => {
                let (what, from_unset) = match d.line {
                    PERMISSION_LINE => ("permission", d.permission_labels().first().copied().unwrap_or_default()),
                    _ => ("effort", Effort::NAMES[0]),
                };
                format!(
                    "Press ← or → to set the {what} without the list: from unset, either sets {from_unset}; then → steps to the next and ← to the one before, and neither goes round past either end. Press ↑↓ to go from line to line and to the buttons. Esc closes the dialog, as Cancel does."
                )
            }
            _ => "Press ↑↓ to go from line to line and to the buttons. On the buttons, ← → go from one to the next, past those grayed; on the two lines, they set the permission and the effort. Esc closes the dialog, as Cancel does."
                .into(),
        };
        vec![first, arrows, kept]
    }

    /// The keys there are in dialog `d`.
    pub(super) fn ask_hint(d: &AskDialog) -> String {
        match (d.line, d.list) {
            (PERMISSION_LINE, Some(_)) => "↑↓ choose · enter: set the permission · esc: close the list · ?: what it does".into(),
            (EFFORT_LINE, Some(_)) => "↑↓ choose · enter: set the effort · esc: close the list · ?: what it does".into(),
            (PERMISSION_LINE, None) => "↑↓ choose · ←→: set the permission · enter: open the list · esc: cancel · ?: what it does".into(),
            (EFFORT_LINE, None) => "↑↓ choose · ←→: set the effort · enter: open the list · esc: cancel · ?: what it does".into(),
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
        // Where each line's value starts, `[ — ]`: the list of its options
        // opens under it.
        let value_at = |line: &str| line.chars().take_while(|c| *c != '[').count();
        let (permission_col, effort_col) = (value_at(&permission), value_at(&effort));
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
        // The list of the options of the line the cursor is on, open: just
        // under that line, from the column of its value, over the dialog and
        // past its edge if need be; above the line where the area has no
        // room under it. Framed as a menu is, the option its cursor is on
        // under the cursor, each option a spot of its own, which a click
        // picks.
        let Some(at) = d.list else { return };
        let (k, col) = match d.line {
            PERMISSION_LINE => (0, permission_col),
            EFFORT_LINE => (1 + under_permission.len(), effort_col),
            _ => return,
        };
        // The line itself not drawn, the box too small for it: no list.
        if k + 2 >= bh {
            return;
        }
        let options = d.options(d.line);
        let lw = options.iter().map(|o| o.chars().count()).max().unwrap_or(0) + 4;
        let lh = options.len() + 2;
        let lx = (x + 2 + col).min(w.saturating_sub(lw));
        let ly = next_to(area.y + (y + 1 + k) as u16, area.y, lh, h);
        window(buf, area, lx, ly, lw, lh, "", color::meta());
        for (j, option) in options.iter().enumerate() {
            let st = if j == at { on_cursor(Style::new()) } else { Style::new() };
            put(buf, area, lx + 1, ly + 1 + j, lw - 2, &fit(&format!(" {option}"), lw - 2), st);
            spot_in(&self.spots, area, (lx + 1, ly + 1 + j, lw - 2, 1), Spot::AskOption(j));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_are_said_in_the_order_the_arrows_step_through_them() {
        assert_eq!(unset_keys(&["answer only"]), "← or → sets answer only, the only one here. Enter opens the list, which holds that one alone");
        assert_eq!(unset_keys(&["answer only", "change stack"]), "← or → sets answer only, then steps to change stack. Enter opens the list of them");
        assert_eq!(
            unset_keys(&["answer only", "change patch", "change stack"]),
            "← or → sets answer only, then steps to change patch and change stack. Enter opens the list of them"
        );
        assert_eq!(unset_keys(&Effort::NAMES), "← or → sets low, then steps to medium, high, xhigh and max. Enter opens the list of them");
        // As `?` says what the list holds, to pick one.
        assert_eq!(one_of(&["answer only"]), "answer only, the only one here");
        assert_eq!(one_of(&["answer only", "change stack"]), "answer only or change stack");
        assert_eq!(one_of(&Effort::NAMES), Effort::listed());
    }

    /// A dialog of the comments listed, nothing set, the cursor on its
    /// permission's line.
    fn listed() -> AskDialog {
        AskDialog {
            title: String::new(),
            about: AskOf::Listed { test: false },
            on_stack: false,
            permissions: vec![(Permission::Answer, None), (Permission::ChangePatch, None), (Permission::ChangeStack, None)],
            permission: None,
            effort: None,
            line: PERMISSION_LINE,
            list: None,
            button: Button::Now,
            missing: None,
            target: None,
            at: None,
        }
    }

    #[test]
    fn the_arrows_step_each_line_from_unset_to_its_first_option_and_stop_at_either_end() {
        // From unset, either arrow sets the first option.
        for forward in [false, true] {
            let mut d = listed();
            d.step(PERMISSION_LINE, forward);
            d.step(EFFORT_LINE, forward);
            assert_eq!((d.permission().map(Permission::label), d.effort.map(Effort::name)), (Some("answer only"), Some("low")), "{forward}");
        }
        // Then → steps on, and stops on the last; ← back, and stops on the
        // first.
        let mut d = listed();
        let mut seen = Vec::new();
        for forward in [true, true, true, true, false, false, false] {
            d.step(PERMISSION_LINE, forward);
            seen.push(d.permission().map(Permission::label).unwrap_or_default());
        }
        assert_eq!(seen, ["answer only", "change patch", "change stack", "change stack", "change patch", "answer only", "answer only"]);
        let mut seen = Vec::new();
        for forward in [true; 6].into_iter().chain([false; 6]) {
            d.step(EFFORT_LINE, forward);
            seen.push(d.effort.map(Effort::name).unwrap_or_default());
        }
        assert_eq!(seen, ["low", "medium", "high", "xhigh", "max", "max", "xhigh", "high", "medium", "low", "low", "low"]);
        // The buttons' line has no options: nothing set there.
        let before = d.clone();
        d.step(BUTTONS_LINE, true);
        assert_eq!(d, before);
        // Of the comments listed, a change is never of the whole stack; of a
        // patch's comment, change stack is.
        assert!(!d.whole(Permission::ChangeStack));
        d.about = AskOf::Comment("c".into(), "i".into());
        assert!(d.whole(Permission::ChangeStack) && !d.whole(Permission::ChangePatch));
        d.on_stack = true;
        assert!(!d.whole(Permission::ChangeStack));
    }

    #[test]
    fn the_list_of_a_line_s_options_goes_round_as_a_menu_does_and_closes_with_enter_or_esc() {
        let mut d = listed();
        // Unset, it opens on the first; ↑ from there goes round to the
        // last, ↓ from the last round to the first; Home and End go to
        // either end. Nothing is set meanwhile.
        d.open_list();
        assert_eq!(d.list, Some(0));
        let mut seen = Vec::new();
        for code in [KeyCode::Up, KeyCode::Down, KeyCode::Down, KeyCode::End, KeyCode::Home] {
            d.list_key(code);
            seen.push(d.list.unwrap_or(9));
        }
        assert_eq!(seen, [2, 0, 1, 2, 0]);
        // ← →, and the keys the list does not take, do nothing in it.
        for code in [KeyCode::Left, KeyCode::Right, KeyCode::Tab, KeyCode::Char(' ')] {
            d.list_key(code);
        }
        assert_eq!((d.list, d.permission), (Some(0), None));
        // Enter sets the option the cursor is on and closes the list, the
        // cursor still on that line.
        d.list_key(KeyCode::End);
        d.list_key(KeyCode::Enter);
        assert_eq!((d.list, d.permission().map(Permission::label), d.line), (None, Some("change stack"), PERMISSION_LINE));
        // Opened again, it is on the one set; Esc leaves the line as it was.
        d.open_list();
        assert_eq!(d.list, Some(2));
        d.list_key(KeyCode::Up);
        d.list_key(KeyCode::Esc);
        assert_eq!((d.list, d.permission().map(Permission::label)), (None, Some("change stack")));
        // The effort's: low to max, on the level set.
        d.line = EFFORT_LINE;
        d.open_list();
        assert_eq!((d.list, d.options(EFFORT_LINE).len()), (Some(0), 5));
        d.list_key(KeyCode::Up);
        d.list_key(KeyCode::Enter);
        assert_eq!(d.effort.map(Effort::name), Some("max"));
        d.open_list();
        assert_eq!(d.list, Some(4));
    }
}
