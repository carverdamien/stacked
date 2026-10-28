//! The sync of the commit messages: while HEAD of your checkout is on the
//! stack and the messages of some patches differ from their commits', the
//! banner's first line, and no landed work is reviewed before they are
//! synced. On the left, each of those patches, with the message it keeps:
//! still to pick, the stack's (`stacked`) or your workspace's
//! (`workspace`); or nothing to pick, the two differing by the Differential
//! Revision alone, one naming none. On the right, the two messages of the
//! patch the cursor is on, side by side, the stack's on the left (one over
//! the other where the pane is too narrow for two), each with its own line
//! breaks, what one has and the other has not coloured, where their lines
//! wrap aside (`messages::compare`). The patch's message is picked with
//! keys of the sync's own, from its menu or with a click; once every patch
//! has its pick, closing asks first, then the message kept goes to each
//! side that lacks it, all at once (`head::sync`). Esc leaves, the picks
//! kept while the editor runs.

use super::*;
use crate::head::{At, Differs, Keep, OnStack};
use crate::keys::Key;
use crate::messages::Marked;

/// A patch whose message differs from its commit's in your workspace: its
/// place (from 0), its version in the stack, its commit's message as taken,
/// and how the two differ.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Differing {
    pub at: usize,
    pub patch: String,
    pub theirs: String,
    pub differs: Differs,
}

/// Where a patch of the sync stands: its message still to pick; picked;
/// or nothing to pick, the two differing by the revision alone, the side
/// that names it kept (`Differs::taken`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Stand {
    ToPick,
    Picked(Keep),
    Nothing(Keep),
}

impl Stand {
    /// The side whose message is kept, if one is.
    fn kept(self) -> Option<Keep> {
        match self {
            Stand::ToPick => None,
            Stand::Picked(k) | Stand::Nothing(k) => Some(k),
        }
    }

    /// As the list says it, and how.
    fn said(self) -> (&'static str, Style) {
        match self {
            Stand::ToPick => ("to pick", Style::new()),
            Stand::Picked(Keep::Stack) => ("stacked", color::del()),
            Stand::Picked(Keep::Workspace) => ("workspace", color::add()),
            Stand::Nothing(_) => ("nothing to pick", color::dim()),
        }
    }
}

/// The sync of the commit messages, open.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Syncing {
    /// The patch the cursor is on, by its line in the list; the first line
    /// shown.
    pub sel: usize,
    top: usize,
    /// Whether the cursor is in the messages, on the right, to scroll them,
    /// and how far; and whether they are wide, the list out of sight.
    pub in_main: bool,
    scroll: usize,
    pub wide: bool,
}

impl Syncing {
    /// The cursor on line `i` of the list.
    pub(super) fn pick_line(&mut self, i: usize) {
        if self.sel != i {
            self.scroll = 0;
        }
        self.sel = i;
        self.in_main = false;
        self.wide = false;
    }
}

/// What each side's message is called.
fn whose(k: Keep) -> &'static str {
    match k {
        Keep::Stack => "the stack's message",
        Keep::Workspace => "your workspace's message",
    }
}

/// A message's lines, as compared (`messages::compare`), `w` wide: what of
/// each the other has not in `word`, the rest as any text; a break that
/// only this message has marked, `↵` at the end of its line, `¶` on its
/// empty line; a line longer than `w` folded, between words where it can,
/// each piece after the first `↪` and what comes next.
fn message_rows(lines: &[Marked], word: Style, w: usize) -> Vec<Parts> {
    let mut out = Vec::new();
    for l in lines {
        // An empty part first: what folds is padded as any text.
        let mut parts: Parts = vec![(String::new(), Style::new())];
        parts.extend(split_words(&l.text, &l.words, Style::new(), word));
        if l.broken {
            parts.push((if l.text.trim().is_empty() { "¶" } else { "↵" }.to_string(), word));
            parts.push((String::new(), Style::new()));
        }
        for (k, piece) in fold_parts(&parts, w.saturating_sub(1).max(1), 0).into_iter().enumerate() {
            match k {
                0 => out.push(piece),
                _ => out.push([("↪".to_string(), color::dim())].into_iter().chain(piece).collect()),
            }
        }
    }
    out
}

impl App {
    /// Where HEAD was when last looked at, if it was looked at on this
    /// stack as it is now.
    fn fresh_head(&self) -> Option<&HeadSeen> {
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        self.head.as_ref().filter(|h| h.name == self.stack && h.rev == rev)
    }

    /// HEAD as last looked at, on this stack as it is now, and what it has
    /// of the stack: none if it is not on the stack.
    fn listed(&self) -> Option<(&crate::head::Head, &OnStack)> {
        let h = self.fresh_head()?.head.as_ref()?;
        match &h.at {
            At::Stack(on) => Some((h, on)),
            At::Elsewhere => None,
        }
    }

    /// The patches whose message differs from its commit's in your
    /// workspace, HEAD as last looked at on the stack as it is: none while
    /// HEAD is not on it, nor while the agent works in your checkout, HEAD
    /// its own then.
    pub(super) fn differing(&self) -> Vec<Differing> {
        let Some((h, on)) = self.listed() else { return vec![] };
        let Some(s) = self.current() else { return vec![] };
        if crate::head::agent_in(&self.db, &h.dir) {
            return vec![];
        }
        on.messages
            .iter()
            .filter_map(|(k, theirs, d)| {
                Some(Differing {
                    at: *k,
                    patch: s.patches.get(*k)?.clone(),
                    theirs: theirs.clone(),
                    differs: d.clone(),
                })
            })
            .collect()
    }

    /// The patches whose commit messages are to sync, said: `patches 2 and
    /// 3`; none when none is.
    pub(super) fn sync_patches(&self) -> Option<String> {
        let places: Vec<usize> = self.differing().iter().map(|d| d.at).collect();
        (!places.is_empty()).then(|| crate::head::patches_said(&places))
    }

    /// Why no landed work is reviewed now, if none is: the commit messages
    /// of some patches differ from their commits', and are synced first.
    pub(super) fn sync_first(&self) -> Option<String> {
        let of = self.sync_patches()?;
        Some(format!(
            "the commit messages of {of} differ from your workspace's: sync them first, from the banner's first line, then review what landed"
        ))
    }

    /// What the banner's line says of the commit messages to sync, first.
    pub(super) fn sync_said(&self) -> Option<String> {
        let of = self.sync_patches()?;
        Some(format!("the commit messages of {of} differ from your workspace's: sync them first"))
    }

    /// The message picked for patch `d`, if one is.
    fn pick_of(&self, d: &Differing) -> Option<Keep> {
        self.sync_picks.get(&(d.patch.clone(), d.theirs.clone())).copied()
    }

    /// Where patch `d` stands.
    pub(super) fn stand(&self, d: &Differing) -> Stand {
        match d.differs.taken() {
            Some((k, _)) => Stand::Nothing(k),
            None => self.pick_of(d).map_or(Stand::ToPick, Stand::Picked),
        }
    }

    /// The picks kept only for the patches that still differ, HEAD looked
    /// at on the stack as it is: not while the agent works in your
    /// checkout, HEAD its own then.
    pub(super) fn keep_sync_picks(&mut self) {
        let Some(seen) = self.fresh_head() else { return };
        if seen.head.as_ref().is_some_and(|h| crate::head::agent_in(&self.db, &h.dir)) {
            return;
        }
        let still: HashSet<(String, String)> = self.differing().into_iter().map(|d| (d.patch, d.theirs)).collect();
        self.sync_picks.retain(|k, _| still.contains(k));
    }

    /// Opens the sync of the commit messages, the cursor on the first patch
    /// still to pick, the picks made before kept.
    pub(super) fn open_sync(&mut self) {
        let list = self.differing();
        if list.is_empty() {
            self.status = "the commit messages are the same in the stack and in your workspace: nothing is left to sync".into();
            return;
        }
        let sel = list.iter().position(|d| self.stand(d) == Stand::ToPick).unwrap_or(0);
        self.syncing = Some(Syncing { sel, ..Default::default() });
        self.screen = Screen::Sync;
        self.on_banner = false;
        // Where HEAD is, looked at again: it may have moved.
        self.look_at_head();
    }

    /// Leaves the sync, its picks kept.
    pub(super) fn leave_sync(&mut self) {
        self.syncing = None;
        self.screen = Screen::Main;
        self.status = "you left the sync of the commit messages: your picks are kept while stacked runs, and the banner's first line opens it again".into();
    }

    /// The patch the cursor is on in the sync, and the list it is in.
    fn sync_here(&self) -> (Vec<Differing>, Option<usize>) {
        let list = self.differing();
        let sel = self.syncing.as_ref().map(|sv| sv.sel.min(list.len().saturating_sub(1)));
        let at = sel.filter(|&i| i < list.len());
        (list, at)
    }

    /// The patch the cursor is on keeping `keep`'s message.
    pub(super) fn sync_keep(&mut self, keep: Keep) {
        if self.bg.doing.contains(&SYNCING) {
            self.status = "the commit messages are being synced: wait for it to be done".into();
            return;
        }
        let (list, at) = self.sync_here();
        let Some(d) = at.and_then(|i| list.get(i)) else { return };
        if let Some((_, rev)) = d.differs.taken() {
            self.status = format!("patch {} has nothing to pick: its two messages differ by the Differential Revision {rev} alone, which the side that names none takes", d.at + 1);
            return;
        }
        self.sync_picks.insert((d.patch.clone(), d.theirs.clone()), keep);
        let left = list.iter().filter(|d| self.stand(d) == Stand::ToPick).count();
        let then = if left == 0 { ": every patch has its pick, and Enter syncs them" } else { "" };
        self.status = self.say_keys(format!("patch {} keeps {}{then}", d.at + 1, whose(keep)));
    }

    /// A click on the message of side `keep`: kept for the patch the cursor
    /// is on; a right click, the menu.
    pub(super) fn sync_click(&mut self, keep: Keep, right: bool) -> Effect {
        if right {
            self.menu_here();
        } else {
            self.sync_keep(keep);
        }
        Effect::None
    }

    /// The patches still to pick, by place.
    fn sync_left(&self, list: &[Differing]) -> Vec<usize> {
        list.iter().filter(|d| self.stand(d) == Stand::ToPick).map(|d| d.at).collect()
    }

    /// What the sync would do as picked: the patches, and its plan; none
    /// with a patch still to pick.
    fn sync_plan(&self) -> Option<(Vec<Differing>, crate::head::Plan)> {
        let (_, on) = self.listed()?;
        let s = self.current()?;
        let list = self.differing();
        let messages: Vec<&str> = s.patches.iter().map(|id| self.db.patch(id).message.as_str()).collect();
        let picks: Vec<(usize, Keep)> = list.iter().filter_map(|d| Some((d.at, self.pick_of(d)?))).collect();
        let plan = crate::head::plan(on, &messages, &picks).ok()?;
        Some((list, plan))
    }

    /// Closing the sync: once every patch has its pick, asked first, what
    /// it does said; else what is left, said.
    pub(super) fn ask_sync(&mut self) {
        let (list, _) = self.sync_here();
        if list.is_empty() {
            self.status = "nothing is left to sync: the commit messages are the same in the stack and in your workspace".into();
            return;
        }
        let left = self.sync_left(&list);
        if !left.is_empty() {
            let (s, w) = (self.cfg.keys.label(Key::KeepStack), self.cfg.keys.label(Key::KeepWorkspace));
            let are = if left.len() == 1 { "is" } else { "are" };
            self.status = format!(
                "{} {are} still to pick: {s} keeps the stack's message and {w} your workspace's, for the patch the cursor is on",
                crate::head::patches_said(&left)
            );
            return;
        }
        match self.sync_plan() {
            Some((list, plan)) => self.mode = Mode::Menu(self.sync_dialog(&list, &plan)),
            None => self.status = "HEAD of your checkout is being looked at again: sync once it has been".into(),
        }
    }

    /// What closing the sync asks first: what goes where, said.
    fn sync_dialog(&self, list: &[Differing], plan: &crate::head::Plan) -> Menu {
        let mut m = Menu::new(
            "sync the commit messages?",
            vec![Item::new("Sync them", Action::SyncNow), Item::new("Keep picking", Action::Cancel)],
            None,
        );
        m.sel = 1;
        m.note = Some(self.sync_plan_said(list, plan).join("\n"));
        m
    }

    /// What the sync does as `plan` says, of the patches of `list`, in
    /// sentences.
    fn sync_plan_said(&self, list: &[Differing], plan: &crate::head::Plan) -> Vec<String> {
        let patches = |ks: &[usize]| {
            let said = crate::head::patches_said(ks);
            let mut c = said.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        };
        let s = |ks: &[usize]| if ks.len() == 1 { "s" } else { "" };
        let on_stack = |k: usize| plan.patches.iter().any(|(at, _)| *at == k);
        let on_commit = |k: usize| plan.commits.iter().any(|(at, _)| *at == k);
        let (mut stack, mut commits) = (Vec::new(), Vec::new());
        let mut said = Vec::new();
        for (k, kept) in &plan.kept {
            let Some(d) = list.iter().find(|d| d.at == *k) else { continue };
            let n = k + 1;
            match (d.differs.taken(), self.stand(d).kept(), on_stack(*k), on_commit(*k)) {
                (Some((Keep::Workspace, rev)), ..) => said.push(format!("Patch {n} takes the Differential Revision {rev} from your workspace's commit.")),
                (Some((Keep::Stack, rev)), ..) => said.push(format!("The commit of patch {n} takes the Differential Revision {rev} from the stack.")),
                (None, Some(Keep::Workspace), true, false) => stack.push(*k),
                (None, Some(Keep::Stack), false, true) => commits.push(*k),
                (None, Some(Keep::Workspace), _, _) => said.push(format!(
                    "Patch {n} takes your workspace's message, and its commit takes the Differential Revision {} of the stack's.",
                    kept.carried.clone().unwrap_or_default()
                )),
                (None, Some(Keep::Stack), _, _) => said.push(format!(
                    "The commit of patch {n} takes the stack's message, and the patch takes the Differential Revision {} of your workspace's commit.",
                    kept.carried.clone().unwrap_or_default()
                )),
                (None, None, ..) => {}
            }
            if let Some(rev) = &kept.dropped {
                said.push(match self.stand(d).kept() {
                    Some(Keep::Workspace) => format!("The Differential Revision {rev} that patch {n} names in the stack is dropped: your workspace's message names another."),
                    _ => format!("The Differential Revision {rev} that the commit of patch {n} names is dropped: the stack's message names another."),
                });
            }
        }
        let mut out = Vec::new();
        if !stack.is_empty() {
            out.push(format!("{} take{} your workspace's message.", patches(&stack), s(&stack)));
        }
        if !commits.is_empty() {
            let of = crate::head::patches_said(&commits);
            out.push(format!("The commit{} of {of} take{} the stack's message.", if commits.len() == 1 { "" } else { "s" }, s(&commits)));
        }
        out.extend(said);
        if !plan.patches.is_empty() {
            out.push("The stack takes the new versions of its patches at once: your picks are their review.".into());
            if let Some(r) = crate::held::change_waiting(&self.db, &self.stack)
                && let Some(c) = r.held.as_ref().and_then(|h| h.stack.as_ref())
            {
                out.push(format!("{} of {}, which waits for your review, goes on from them.", capitalized(ask::whose_change(r)), c.of));
            }
        }
        if plan.remade > 0 {
            let n = plan.remade;
            let on = self.listed().map_or("HEAD".into(), |(h, _)| h.branch.clone().unwrap_or_else(|| "HEAD".into()));
            let (commits, are) = if n == 1 { ("1 commit".to_string(), "is") } else { (format!("{n} commits"), "are") };
            out.push(format!("{commits} {are} made again in your workspace, the files as they were, and {on} moves to them."));
        }
        out
    }

    /// The sync done, as picked, away from the screen: the stack and HEAD
    /// as listed checked again first, nothing changing if either changed.
    pub(super) fn sync_now(&mut self) {
        if self.bg.doing.contains(&SYNCING) {
            self.status = "the commit messages are being synced already".into();
            return;
        }
        let Some((rev, Some(head))) = self.fresh_head().map(|h| (h.rev, h.head.clone())) else {
            self.status = "HEAD of your checkout is being looked at again: sync once it has been".into();
            return;
        };
        let picks: Vec<(usize, Keep)> = self.differing().iter().filter_map(|d| Some((d.at, self.pick_of(d)?))).collect();
        self.bg.doing.push(SYNCING);
        self.status = "syncing the commit messages…".into();
        let (path, name, here) = (self.path.clone(), self.stack.clone(), self.checkout.clone());
        self.spawn(move || Done::Synced(crate::head::sync(&path, &name, &here, rev, &head, &picks).map_err(|e| e.0)));
    }

    /// What the sync came to: synced, the sync closed, its picks gone; else
    /// why nothing changed, the sync staying. Where HEAD is, looked at again.
    pub(super) fn synced(&mut self, done: std::result::Result<String, String>) {
        self.reload();
        match done {
            Ok(said) => {
                self.sync_picks.clear();
                self.syncing = None;
                if self.screen == Screen::Sync {
                    self.screen = Screen::Main;
                }
                self.status = said;
            }
            Err(why) => self.status = why,
        }
        self.look_at_head();
    }

    /// What the sync's menu says, when an item of it cannot be chosen: of
    /// the patch the cursor is on, what it keeps; what is left to pick.
    fn sync_note(&self) -> Vec<String> {
        let (list, at) = self.sync_here();
        let mut out = Vec::new();
        if let Some(d) = at.and_then(|i| list.get(i)) {
            match (self.stand(d), d.differs.taken()) {
                (Stand::Nothing(_), Some((_, rev))) => out.push(format!(
                    "Patch {} has nothing to pick: its two messages differ by the Differential Revision {rev} alone, which the side that names none takes.",
                    d.at + 1
                )),
                (Stand::Picked(k), _) => out.push(format!("Patch {} keeps {}.", d.at + 1, whose(k))),
                _ => {}
            }
        }
        let left = self.sync_left(&list);
        if !left.is_empty() {
            let are = if left.len() == 1 { "is" } else { "are" };
            out.push(format!("{} {are} still to pick: the sync closes once every patch has its pick.", capitalized(&crate::head::patches_said(&left))));
        }
        out
    }

    /// The sync's menu: the message the patch the cursor is on keeps; the
    /// sync closed, once every patch has its pick; or left.
    pub(super) fn sync_items(&self, items: &mut Vec<Item>) {
        let (list, at) = self.sync_here();
        if let Some(d) = at.and_then(|i| list.get(i)) {
            let stand = self.stand(d);
            for keep in [Keep::Stack, Keep::Workspace] {
                let label = format!("Keep {}", whose(keep));
                items.push(Item::new(label, Action::SyncKeep(keep)).when(stand == Stand::ToPick || stand == Stand::Picked(other(keep))));
            }
            items.push(Item::line());
        }
        items.push(Item::new("Sync the commit messages…", Action::AskSync).when(!list.is_empty() && self.sync_left(&list).is_empty()));
        items.push(Item::new("Leave, keeping your picks", Action::LeaveSync));
    }

    /// What the menu says of the sync, joined to what it says otherwise.
    pub(super) fn sync_menu_note(&self) -> Option<String> {
        let note = self.sync_note();
        (!note.is_empty()).then(|| note.join("\n"))
    }

    /// What the notes over the messages of patch `d` say: what it has to
    /// pick, or nothing to; and what of the revisions they name the message
    /// kept keeps, with the moz-phab feature.
    fn sync_notes(&self, d: &Differing) -> Vec<String> {
        let mut out = Vec::new();
        if let Some((k, rev)) = d.differs.taken() {
            out.push(match k {
                Keep::Workspace => format!("The stack takes the Differential Revision {rev} from your workspace's commit."),
                Keep::Stack => format!("Your workspace's commit takes the Differential Revision {rev} from the stack."),
            });
            return out;
        }
        let ours = self.db.patch(&d.patch).message.clone();
        let (a, b) = (crate::head::revision_in(&ours), crate::head::revision_in(&d.theirs));
        let pick = self.pick_of(d);
        match pick.and_then(|k| crate::head::kept(&ours, &d.theirs, &d.differs, Some(k)).map(|kept| (k, kept))) {
            None => {
                match (&a, &b) {
                    (Some(a), Some(b)) if a != b => out.push(format!("The stack's message names {a}, your workspace's {b}: the one kept keeps its own.")),
                    (Some(a), None) => out.push(format!("Only the stack's message names a revision, {a}: the message kept names it too.")),
                    (None, Some(b)) => out.push(format!("Only your workspace's message names a revision, {b}: the message kept names it too.")),
                    _ => {}
                }
                out.push("Pick the message this patch keeps: both its sides then say it.".into());
            }
            Some((k, kept)) => {
                out.push(match k {
                    Keep::Stack => "Once synced, its commit in your workspace takes the stack's message.".to_string(),
                    Keep::Workspace => "Once synced, the patch takes your workspace's message, a new version in the stack.".to_string(),
                });
                if let Some(rev) = &kept.carried {
                    out.push(match k {
                        Keep::Stack => format!("The stack's message names no revision: it takes {rev}, which your workspace's names."),
                        Keep::Workspace => format!("Your workspace's message names no revision: it takes {rev}, which the stack's names."),
                    });
                }
                if let Some(rev) = &kept.dropped {
                    out.push(match k {
                        Keep::Stack => format!("Keeping the stack's message drops {rev}, which your workspace's names."),
                        Keep::Workspace => format!("Keeping your workspace's message drops {rev}, which the stack's names."),
                    });
                }
            }
        }
        out
    }

    /// The sync's keys, where the cursor is: the list, or the messages.
    pub(super) fn sync_hint(&self) -> String {
        let (s, w) = (self.cfg.keys.label(Key::KeepStack), self.cfg.keys.label(Key::KeepWorkspace));
        let keep = format!("{s}: keep the stack's · {w}: keep your workspace's");
        let esc = "esc: leave";
        match &self.syncing {
            Some(sv) if sv.in_main && sv.wide && self.body_w >= 60 => format!("↑↓: scroll · {keep} · enter: sync · ←: narrower · {esc}"),
            Some(sv) if sv.in_main && self.body_w < 60 => format!("↑↓: scroll · {keep} · enter: sync · ←: back to the list · {esc}"),
            Some(sv) if sv.in_main => format!("↑↓: scroll · {keep} · enter: sync · →: wider · ←: back to the list · {esc}"),
            _ => format!("↑↓ choose · {keep} · enter: sync · →: look closer · space: menu · {esc}"),
        }
    }

    /// What `?` says in the sync.
    pub(super) fn sync_help(&self) -> Vec<String> {
        let (s, w) = (self.cfg.keys.label(Key::KeepStack), self.cfg.keys.label(Key::KeepWorkspace));
        let mut out = vec![
            "The sync of the commit messages: each patch whose message differs from its commit's in your workspace, and the message it keeps, the stack's or your workspace's. On the right are the patch's two messages, the stack's first: the words both have, in order, are drawn as any text, however their lines wrap; what only the stack's has is in the colour of words removed, and what only your workspace's has, in the colour of words added. In code (a fenced block, a line indented by a tab or by four spaces, a command after the shell's $ prompt), the end of each line and the white space within it count too.".to_string(),
            format!("Press {s} to keep the stack's message for the patch the cursor is on, {w} to keep your workspace's, or click the message to keep; its menu has both too."),
        ];
        if cfg!(feature = "moz-phab") {
            out.push("Two messages that differ by the Differential Revision alone, one naming none, have nothing to pick: the side that names none takes it. A message kept that names no revision takes the other's.".into());
        }
        out.push("Press Enter, once every patch has its pick, to sync them, once you say so: the message kept goes to each side that lacks it, all at once. The stack takes new versions of its patches at once, with no review of their own, and the commits are made again in your workspace, from the first that changes up to HEAD, the files as they were.".into());
        out.push("Press ↑↓ for another patch; → to scroll the messages, → again to see them wide, the list out of sight, and ← to come back.".into());
        out.push("Press Esc to leave the sync: your picks are kept while stacked runs, and the banner's first line opens it again. No landed work is reviewed before the commit messages are synced.".into());
        out
    }

    /// A key in the sync: ↑↓ another patch (or, in the messages, scroll
    /// them: → into them, → again wide, ← back); the sync's own keys keep
    /// a message; Enter closes it, asked first; Esc leaves it.
    pub(super) fn sync_key(&mut self, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Esc => {
                self.leave_sync();
                return Effect::None;
            }
            KeyCode::Enter => {
                self.ask_sync();
                return Effect::None;
            }
            code if code == Key::KeepStack.event().code => {
                self.sync_keep(Keep::Stack);
                return Effect::None;
            }
            code if code == Key::KeepWorkspace.event().code => {
                self.sync_keep(Keep::Workspace);
                return Effect::None;
            }
            _ => {}
        }
        let n = self.differing().len();
        let (page, narrow) = (self.page(), self.body_w < 60);
        let Some(sv) = self.syncing.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let last = n.saturating_sub(1);
        match (sv.in_main, k.code) {
            (false, KeyCode::Right) => {
                sv.in_main = true;
                sv.wide = narrow;
                sv.scroll = 0;
            }
            (true, KeyCode::Right) => sv.wide = true,
            (true, KeyCode::Left) if sv.wide && !narrow => sv.wide = false,
            (true, KeyCode::Left) => {
                sv.in_main = false;
                sv.wide = false;
            }
            (false, code) => {
                let sel = match code {
                    KeyCode::Down => (sv.sel + 1).min(last),
                    KeyCode::Up => sv.sel.saturating_sub(1),
                    KeyCode::PageDown => (sv.sel + page).min(last),
                    KeyCode::PageUp => sv.sel.saturating_sub(page),
                    KeyCode::Home => 0,
                    KeyCode::End => last,
                    _ => sv.sel,
                };
                sv.pick_line(sel);
            }
            (true, code) => {
                sv.scroll = match code {
                    KeyCode::Down => sv.scroll + 1,
                    KeyCode::Up => sv.scroll.saturating_sub(1),
                    KeyCode::PageDown => sv.scroll + page,
                    KeyCode::PageUp => sv.scroll.saturating_sub(page),
                    KeyCode::Home => 0,
                    _ => sv.scroll,
                };
            }
        }
        Effect::None
    }

    /// The sync: its list on the left, the messages of the patch the cursor
    /// is on, on the right.
    pub(super) fn draw_sync(&mut self, buf: &mut Buffer, body: Rect) {
        let (w, h) = (body.width as usize, body.height as usize);
        let list = self.differing();
        let Some(mut sv) = self.syncing.clone() else { return };
        sv.sel = sv.sel.min(list.len().saturating_sub(1));
        let rows = h.saturating_sub(1);
        let narrow = w < 60;
        let list_w = if narrow { w } else { (w / 4).clamp(24, 40) };
        // Wide, the messages have it all, the list out of sight.
        let wide = sv.in_main && (sv.wide || narrow);
        if !wide {
            let area = Rect::new(body.x, body.y, list_w as u16, body.height);
            let left = list.iter().filter(|d| self.stand(d) == Stand::ToPick).count();
            let title = match (list.len(), left) {
                (0, _) => " commit messages: none to sync".to_string(),
                (_, 0) => " commit messages: all picked".to_string(),
                (_, left) => format!(" commit messages: {left} to pick"),
            };
            put(buf, area, 0, 0, list_w, &fit(&title, list_w), color::header());
            if sv.sel < sv.top {
                sv.top = sv.sel;
            } else if sv.sel >= sv.top + rows {
                sv.top = sv.sel + 1 - rows;
            }
            for (y, (i, d)) in list.iter().enumerate().skip(sv.top).take(rows).enumerate() {
                let (state, st) = self.stand(d).said();
                let title = self.db.patch(&d.patch).title().to_string();
                let parts: Parts = vec![
                    ("   ".to_string(), Style::new()),
                    (state.to_string(), st),
                    (format!(" · {} {title}", d.at + 1), Style::new()),
                ];
                let parts: Parts = match i == sv.sel {
                    true if sv.in_main => parts.into_iter().map(|(t, s)| (t, s.patch(color::picked()))).collect(),
                    true => parts.into_iter().map(|(t, s)| (t, on_cursor(s))).collect(),
                    false => parts,
                };
                put_parts(buf, area, 0, y + 1, list_w, &parts);
                spot_in(&self.spots, area, (0, y + 1, list_w, 1), Spot::SyncLine(i));
            }
            self.areas.borrow_mut().push(area);
            if narrow {
                self.syncing = Some(sv);
                return;
            }
            for y in 0..body.height {
                buf.set_string(body.x + list_w as u16, body.y + y, "│", color::meta());
            }
        }
        let (main_x, main_w) = if wide { (0, w) } else { (list_w + 1, w - list_w - 1) };
        let main = Rect::new(body.x + main_x as u16, body.y, main_w as u16, body.height);
        let mut head = color::header();
        if sv.in_main {
            head = head.add_modifier(Modifier::UNDERLINED);
        }
        let Some(d) = list.get(sv.sel) else {
            let said = match self.fresh_head() {
                None => " HEAD of your checkout is being looked at again…".to_string(),
                Some(_) => self.say_keys(" nothing is left to sync: the commit messages are the same in the stack and in your workspace. Esc leaves.".into()),
            };
            put(buf, main, 0, 0, main_w, &fit(&said, main_w), head);
            self.syncing = Some(sv);
            return;
        };
        let p = self.db.patch(&d.patch).clone();
        put(buf, main, 0, 0, main_w, &fit(&format!(" patch {}: {}", d.at + 1, p.title()), main_w), head);
        // What this patch has to pick, and what it keeps.
        let mut y = 1;
        for line in self.sync_notes(d).iter().flat_map(|n| wrap(n, main_w.saturating_sub(2).max(1))) {
            if y + 2 >= h {
                break;
            }
            put(buf, main, 1, y, main_w.saturating_sub(1), &line, color::note());
            y += 1;
        }
        let kept = self.stand(d).kept();
        let title = |k: Keep| match kept == Some(k) {
            true => format!(" {} · kept", whose(k)),
            false => format!(" {}", whose(k)),
        };
        let (ours, theirs) = crate::messages::compare(&p.message, &d.theirs);
        let room = h.saturating_sub(y + 1);
        if main_w >= self.cfg.view.split_width {
            // Side by side, the stack's on the left.
            let left_w = (main_w - 1) / 2;
            let right_w = main_w - left_w - 1;
            // Each a column in from its edge, under its title.
            let a = message_rows(&ours, color::del_word(), left_w - 1);
            let b = message_rows(&theirs, color::add_word(), right_w - 1);
            sv.scroll = sv.scroll.min(a.len().max(b.len()).saturating_sub(room));
            let tall = (h - y) as u16;
            let left = Rect::new(main.x, main.y + y as u16, left_w as u16, tall);
            let right = Rect::new(main.x + left_w as u16 + 1, main.y + y as u16, right_w as u16, tall);
            for (area, rows, k, cw) in [(left, &a, Keep::Stack, left_w), (right, &b, Keep::Workspace, right_w)] {
                put(buf, area, 0, 0, cw, &fit(&title(k), cw), color::file());
                for (i, row) in rows.iter().skip(sv.scroll).take(room).enumerate() {
                    put_parts(buf, area, 1, 1 + i, cw - 1, row);
                }
                spot_in(&self.spots, area, (0, 0, cw, h - y), Spot::SyncSide(k));
                self.areas.borrow_mut().push(area);
            }
            for row in y..h {
                buf.set_string(main.x + left_w as u16, main.y + row as u16, "│", color::meta());
            }
        } else {
            // One over the other, the stack's first.
            // Its title on the pane's edge, its lines a column in.
            let mut all: Vec<(Parts, usize, Keep)> = vec![(vec![(title(Keep::Stack), color::file())], 0, Keep::Stack)];
            all.extend(message_rows(&ours, color::del_word(), main_w - 1).into_iter().map(|r| (r, 1, Keep::Stack)));
            all.push((vec![], 0, Keep::Stack));
            all.push((vec![(title(Keep::Workspace), color::file())], 0, Keep::Workspace));
            all.extend(message_rows(&theirs, color::add_word(), main_w - 1).into_iter().map(|r| (r, 1, Keep::Workspace)));
            let room = h.saturating_sub(y);
            sv.scroll = sv.scroll.min(all.len().saturating_sub(room));
            let area = Rect::new(main.x, main.y + y as u16, main.width, (h - y) as u16);
            for (i, (row, x, k)) in all.iter().skip(sv.scroll).take(room).enumerate() {
                put_parts(buf, area, *x, i, main_w - x, row);
                spot_in(&self.spots, area, (0, i, main_w, 1), Spot::SyncSide(*k));
            }
            self.areas.borrow_mut().push(area);
        }
        self.syncing = Some(sv);
    }
}

/// The other side.
fn other(k: Keep) -> Keep {
    match k {
        Keep::Stack => Keep::Workspace,
        Keep::Workspace => Keep::Stack,
    }
}

/// `text` with its first letter a capital.
fn capitalized(text: &str) -> String {
    let mut c = text.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}
