//! Why Move HEAD does not move HEAD, and what you can do so that it can: a
//! view of its own, which `Move HEAD…` opens, from the stack's menu, a
//! patch's or HEAD's line's, while Move HEAD would not move HEAD from where
//! it is (`head::Moves`), instead of moving it. Only to read, laid out as a
//! review is: on the left, where HEAD is, then each thing that keeps Move
//! HEAD from moving HEAD, a line each, under a title for each kind: a
//! commit only your checkout has, after the stack's patches it has
//! (`OnStack::above`); a commit with its patch's diff whose message is not
//! the patch's (`≠`), or names a revision the patch does not name (`!`);
//! or HEAD on another base. On the right, the line the cursor is on: the
//! commit as `git show` shows it, drawn as the middle draws a patch; the
//! patch's message then the commit's, compared where their lines wrap
//! aside (`messages::diff`), as the reviews compare messages; or HEAD and
//! the stack's base. Under it, stacked's comment says what you can do in
//! your checkout, the git commands to run there written out, their commits
//! named, so that Move HEAD can move HEAD: stacked runs none of them.
//!
//! What it shows of the commits is read of the checkout away from the
//! screen as it opens (`head::read`), `…` said meanwhile. HEAD is looked at
//! again every few seconds, as ever: the view follows it, and says so once
//! nothing keeps Move HEAD from moving HEAD.

use super::compared::{commit_rows, message_rows, row, said_rows};
use super::*;
use crate::head::{At, Case, Head, Mark, Moves, ONLY, Read, SHOWN};

/// What the bar says while what the view shows is read of the checkout.
pub(super) const READING: &str = "reading your checkout";

/// The name of the branch the view's comments suggest, to keep commits on:
/// any name not taken will do.
const BRANCH: &str = "my-work";

/// What a line of the view's list is: where HEAD is, said first; a commit
/// only the checkout has, by its place among those (`OnStack::above`); the
/// commit of patch `k` (from 0), marked; or HEAD on another base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Where,
    Own(usize),
    Marked(usize),
    Elsewhere,
}

/// What the right pane says of a line of the list, in order, each drawn as
/// it is: a heading; a sentence, folded between words; a command to run in
/// your checkout, on a line of its own; a line drawn as it is; what is being
/// read, quietly; a commit read whole, as the middle draws a patch; two
/// messages compared where their lines wrap aside, the patch's then the
/// commit's; an empty line.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Heading(String),
    Text(String),
    Command(String),
    Line(String),
    Waiting(String),
    Commit(crate::head::Commit),
    Messages(String, String),
    Blank,
}

/// A line of the view's list: what it says, how, and what it is: none for
/// a title, or a line drawn only.
struct ListLine {
    text: String,
    style: Style,
    why: Option<Why>,
}

/// The view: of HEAD `head` of stack `name` on `base` (the stack at `rev`,
/// each patch's title and message by its place), and what Move HEAD may do
/// from there; what is read of the checkout for it, none until it is in;
/// its list; where its cursor is, in the list or in the pane on the right,
/// that pane wide or not; that pane's title and rows, and the line and
/// width they were built for; and the screen it was opened from, to go
/// back to.
pub struct HeadWhy {
    pub head: Head,
    pub moves: Moves,
    name: String,
    base: String,
    rev: Option<usize>,
    patches: Vec<(String, String)>,
    pub read: Option<std::result::Result<Read, String>>,
    lines: Vec<ListLine>,
    pub sel: usize,
    top: usize,
    pub in_main: bool,
    pub scroll: usize,
    pub wide: bool,
    title: String,
    rows: Vec<Row>,
    built: Option<(usize, usize)>,
    back: Screen,
}

/// `sha`, its first twelve characters, as the commands name a commit.
fn sha12(sha: &str) -> &str {
    &sha[..12.min(sha.len())]
}

/// `sha`, its first eight characters, as the patches pane names a commit.
fn sha8(sha: &str) -> &str {
    &sha[..8.min(sha.len())]
}

/// What Move HEAD does in case `c`, said.
fn case_said(c: Case) -> &'static str {
    match c {
        Case::AsLeft => "HEAD is as stacked last left it, so Move HEAD replaces HEAD with the stack as it is now",
        Case::OnStack => "HEAD is on the stack with no marks, so Move HEAD may put HEAD at any patch",
    }
}

/// `pieces` as the rows of the pane on the right, folded to `w`: a heading
/// between words too; an empty line once, after a commit's rows, which end
/// with one.
fn rows_of(pieces: &[Piece], w: usize) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for p in pieces {
        match p {
            Piece::Heading(t) => {
                for (k, piece) in wrap(t, w.max(10)).into_iter().enumerate() {
                    rows.push(Row {
                        cont: k > 0,
                        ..row(Kind::File, piece)
                    });
                }
            }
            Piece::Text(t) => said_rows(&mut rows, Kind::Msg, t, w),
            Piece::Command(c) => rows.push(row(Kind::Msg, format!("      {c}"))),
            Piece::Line(t) => rows.push(row(Kind::Msg, format!("   {t}"))),
            Piece::Waiting(t) => said_rows(&mut rows, Kind::Stream, t, w),
            Piece::Commit(c) => rows.extend(commit_rows(c, w)),
            Piece::Messages(ours, theirs) => message_rows(&mut rows, ours, theirs, w),
            Piece::Blank if rows.last().is_some_and(|r| r.kind == Kind::Blank) => {}
            Piece::Blank => rows.push(row(Kind::Blank, "")),
        }
    }
    rows
}

impl ListLine {
    fn of(text: impl Into<String>, style: Style, why: Option<Why>) -> ListLine {
        ListLine { text: text.into(), style, why }
    }

    fn title(text: &str) -> ListLine {
        ListLine::of(text, color::file(), None)
    }
}

impl HeadWhy {
    /// The view of HEAD `head` of stack `name` of `db`, at `rev`, Move HEAD
    /// doing as `moves` says from there; opened from screen `back`. Its
    /// cursor on its first line, where HEAD is.
    pub(super) fn new(db: &Db, name: &str, head: Head, moves: Moves, rev: Option<usize>, back: Screen) -> HeadWhy {
        let s = db.stack(name).ok();
        let patches = s.map_or(Vec::new(), |s| {
            s.patches
                .iter()
                .map(|id| {
                    let p = db.patch(id);
                    (p.title().to_string(), p.message.clone())
                })
                .collect()
        });
        let mut v = HeadWhy {
            head,
            moves,
            name: name.to_string(),
            base: s.and_then(|s| s.base.clone()).unwrap_or_default(),
            rev,
            patches,
            read: None,
            lines: Vec::new(),
            sel: 0,
            top: 0,
            in_main: false,
            scroll: 0,
            wide: false,
            title: String::new(),
            rows: Vec::new(),
            built: None,
            back,
        };
        v.lines = v.list();
        v
    }

    /// What the list says, a line each: for the tests.
    pub fn texts(&self) -> Vec<String> {
        self.lines.iter().map(|l| l.text.clone()).collect()
    }

    /// What the line the cursor is on is.
    pub fn here(&self) -> Why {
        self.lines.get(self.sel).and_then(|l| l.why).unwrap_or(Why::Where)
    }

    /// What the pane on the right says of the line the cursor is on: its
    /// title, then its pieces.
    pub fn said_here(&self) -> (String, Vec<Piece>) {
        self.said(self.here())
    }

    /// The list: where HEAD is, then, while Move HEAD would not move HEAD,
    /// each thing that keeps it from it, under a title for each kind: the
    /// first commits only the checkout has, as many as `SHOWN`, by their
    /// subjects, how many more said after them; the commits marked `≠`, then
    /// those marked `!`, each by its patch's place and title; or HEAD on
    /// another base.
    fn list(&self) -> Vec<ListLine> {
        let mut out = vec![ListLine::of(" where HEAD is, and why", color::file(), Some(Why::Where))];
        if self.moves.case.is_ok() {
            return out;
        }
        match &self.head.at {
            At::Stack(on) => {
                if !on.above.is_empty() {
                    out.push(ListLine::title(" commits only your checkout has"));
                    for (j, sha) in on.above.iter().take(SHOWN).enumerate() {
                        let subject = on.subjects.get(j).map_or("", String::as_str);
                        out.push(ListLine::of(format!("   ○ {} {subject}", sha8(sha)), Style::new(), Some(Why::Own(j))));
                    }
                    if on.above.len() > SHOWN {
                        let more = on.above.len() - SHOWN;
                        out.push(ListLine::of(format!("   ○ … {} more commit{}", crate::paint::thousands(more), plural(more)), color::dim(), None));
                    }
                }
                for (title, revision) in [(" messages other than their patch's", false), (" revisions their patch does not name", true)] {
                    let marked: Vec<&(usize, String, Mark)> = on.marks.iter().filter(|(_, _, m)| matches!(m, Mark::Revision { .. }) == revision).collect();
                    if marked.is_empty() {
                        continue;
                    }
                    out.push(ListLine::title(title));
                    for (k, _, m) in marked {
                        let t = self.patches.get(*k).map_or("", |(t, _)| t.as_str());
                        out.push(ListLine::of(format!("   {} {} {t}", m.sign(), k + 1), Style::new(), Some(Why::Marked(*k))));
                    }
                }
            }
            At::Elsewhere => {
                out.push(ListLine::title(" HEAD on another base"));
                out.push(ListLine::of(format!("   ▶ HEAD {}: off the stack", self.head.on()), Style::new(), Some(Why::Elsewhere)));
            }
        }
        out
    }

    /// What the pane on the right says of `why`: its title, then its pieces.
    pub fn said(&self, why: Why) -> (String, Vec<Piece>) {
        match why {
            Why::Where => self.where_said(),
            Why::Own(j) => self.own_said(j),
            Why::Marked(k) => self.marked_said(k),
            Why::Elsewhere => self.elsewhere_said(),
        }
    }

    /// Where HEAD is, and why Move HEAD does not move it: what the lines
    /// under it are, and that the view follows HEAD. Or that nothing keeps
    /// Move HEAD from moving HEAD now.
    fn where_said(&self) -> (String, Vec<Piece>) {
        let h = &self.head;
        let mut out = vec![Piece::Text(format!("HEAD of your checkout, {}, is {}.", h.git_said(), h.said(&self.name))), Piece::Blank];
        let why = match &self.moves.case {
            Ok(c) => {
                out.push(Piece::Text(format!("Nothing keeps Move HEAD from moving HEAD now: {}. Press Esc, then choose Move HEAD… again to move HEAD.", case_said(*c))));
                return (" where HEAD is: nothing keeps Move HEAD from moving it".into(), out);
            }
            Err(why) => why,
        };
        out.extend([
            Piece::Text(format!("Move HEAD does not move HEAD from there: {why}.")),
            Piece::Blank,
            Piece::Text("Each line under this one in the list is something that keeps Move HEAD from moving HEAD. On each line, this pane shows what the line is about, then stacked's comment, which says what you can do in your checkout, with the commands to run there. stacked runs none of these commands for you.".into()),
            Piece::Blank,
            Piece::Text("Once your checkout changes, stacked reads HEAD again, and this view with it. Once nothing keeps Move HEAD from moving HEAD, choosing Move HEAD… moves HEAD.".into()),
        ]);
        (" where HEAD is, and why Move HEAD does not move it".into(), out)
    }

    /// What the comment of `why` says last: that Move HEAD can move HEAD
    /// then; once the other lines of the list are dealt with too, if any
    /// other keeps Move HEAD from moving HEAD. The commits only the checkout
    /// has are dealt with together.
    fn then_said(&self, why: Why) -> &'static str {
        let others = self.lines.iter().filter_map(|l| l.why).any(|w| match (w, why) {
            (Why::Where, _) => false,
            (Why::Own(_), Why::Own(_)) => false,
            (w, why) => w != why,
        });
        match others {
            true => "Then, once the other lines of the list are dealt with too, Move HEAD can move HEAD.",
            false => "Then Move HEAD can move HEAD.",
        }
    }

    /// The commit only the checkout has at `j` among those: the commit as
    /// git shows it, once read; then stacked's comment, the same for each of
    /// those commits, as they are dealt with together.
    fn own_said(&self, j: usize) -> (String, Vec<Piece>) {
        let At::Stack(on) = &self.head.at else { return (String::new(), vec![]) };
        let Some(sha) = on.above.get(j) else { return (String::new(), vec![]) };
        let mut out = vec![Piece::Heading("the commit, as git show shows it".into())];
        match &self.read {
            None => out.push(Piece::Waiting("Reading the commit from your checkout…".into())),
            Some(Err(e)) => out.push(Piece::Text(format!("stacked could not read the commit from your checkout: {e}"))),
            Some(Ok(r)) => match r.own.iter().find(|c| c.sha == *sha) {
                None => out.push(Piece::Text("stacked could not read the commit from your checkout: git did not show it.".into())),
                Some(c) => {
                    out.push(Piece::Commit(c.clone()));
                    match c.cut {
                        Some(n) => out.push(Piece::Text(format!(
                            "Its diff has {} lines, too many to show here: `git show {}` shows it.",
                            crate::paint::thousands(n),
                            sha12(sha)
                        ))),
                        None if c.diff.is_empty() => out.push(Piece::Text("git shows no diff of this commit: it is a merge, or it changes no file.".into())),
                        None => {}
                    }
                }
            },
        }
        out.extend([Piece::Blank, Piece::Heading("stacked's comment".into())]);
        let (n, k) = (on.above.len(), on.commits.len());
        let after = if k == 0 { "the base".to_string() } else { format!("patch {k}") };
        let which = match n {
            1 => format!("This commit, after {after}, is not one of the stack's patches: only your checkout has it."),
            _ => format!(
                "This commit is one of the {} commits after {after} that are not the stack's patches: only your checkout has them.",
                crate::paint::thousands(n)
            ),
        };
        // This commit, or these: the comment is of all of them.
        let (these, it, are) = if n == 1 { ("this commit", "it", "is") } else { ("these commits", "them", "are") };
        let those = if n == 1 { "the commit" } else { "the commits" };
        let branch = self.head.branch.clone();
        let kept = match &branch {
            Some(b) => format!(
                "{} {} on {b}, the branch HEAD is on: Move HEAD leaves {b} where it is, and so does `git switch --detach`.",
                capitalized(these),
                if n == 1 { "stays" } else { "stay" }
            ),
            None => format!("HEAD is on no branch, so no branch keeps {these}: once HEAD moves away from {it}, {those} {are} left on no branch."),
        };
        out.push(Piece::Text(format!(
            "{which} Move HEAD would move HEAD away from {these}, which stacked did not make: stacked never does that, as {those} may be yours. {kept}"
        )));
        out.extend([Piece::Blank, Piece::Text("What you can do, in your checkout:".into())]);
        let back = if branch.is_some() { "Go back" } else { "Then go back" };
        if branch.is_none() {
            out.push(Piece::Text(format!(
                "Put {these} on a branch first, to keep {it}. The branch is named {BRANCH} here, but any name not taken will do:"
            )));
            out.push(Piece::Command(format!("git branch {BRANCH} {}", sha12(on.above.last().unwrap_or(sha)))));
        }
        let (to, there) = match on.commits.last() {
            Some(c) => (c.as_str(), format!("{back} to the commit of patch {k}, the stack's last commit in your checkout:")),
            None => (self.base.as_str(), format!("{back} to the stack's base, as your checkout has none of the stack's patches:")),
        };
        out.push(Piece::Text(there));
        out.push(Piece::Command(format!("git switch --detach {}", sha12(to))));
        let on = branch.unwrap_or_else(|| BRANCH.to_string());
        out.push(Piece::Text(format!("{} `git switch {on}` takes you back to {these} whenever you want.", self.then_said(Why::Own(j)))));
        (format!(" commit {}, only in your checkout", sha12(sha)), out)
    }

    /// The commit of patch `k`, marked: the patch's message then the
    /// commit's, compared; then stacked's comment: what the mark is, why
    /// Move HEAD does not move HEAD over it, and the two ways to clear it,
    /// Reconcile with HEAD or the commit reworded in your checkout.
    fn marked_said(&self, k: usize) -> (String, Vec<Piece>) {
        let At::Stack(on) = &self.head.at else { return (String::new(), vec![]) };
        let (Some((_, theirs, m)), Some(sha)) = (on.marks.iter().find(|(at, _, _)| *at == k), on.commits.get(k)) else {
            return (String::new(), vec![]);
        };
        let ours = self.patches.get(k).map_or("", |(_, m)| m.as_str());
        let n = k + 1;
        let mut out = vec![
            Piece::Heading(format!("the message of patch {n} in the stack, then of its commit in your checkout")),
            Piece::Messages(ours.to_string(), theirs.clone()),
            Piece::Blank,
            Piece::Heading("stacked's comment".into()),
        ];
        let marked = "above, the lines marked - are the patch's, and those marked + the commit's";
        let (title, reconcile) = match m {
            Mark::Message => {
                out.push(Piece::Text(format!(
                    "The commit {} of your checkout has the diff of patch {n}, but another message: {marked}. Move HEAD would make that commit again from the patch, writing the patch's message over the commit's, and stacked never rewrites a commit it did not make: the commit's message may be yours, amended outside stacked.",
                    sha12(sha)
                )));
                (
                    format!(" the commit of patch {n}, {}: another message", sha12(sha)),
                    "it gives the patch its commit's message, in an edit for your review. Once you accept the edit, the two messages are the same, and the mark is gone.".to_string(),
                )
            }
            Mark::Revision { theirs: t, ours: o } => {
                let names = match o {
                    None => format!("names {t}, which the patch does not name"),
                    Some(o) => format!("names {t}, where the patch names {o}"),
                };
                out.push(Piece::Text(format!(
                    "The commit {} of your checkout has the diff of patch {n}, but it {names}: {marked}. HEAD is locked below this patch: Move HEAD never rewrites nor drops a commit that names a revision its patch does not name, so Move HEAD keeps this commit and the commits under it as they are, and puts HEAD nowhere below this patch. While the commit is marked, Move HEAD does not move HEAD at all: {ONLY}.",
                    sha12(sha)
                )));
                (
                    format!(" the commit of patch {n}, {}: it names {t}", sha12(sha)),
                    format!("it gives the patch its commit's message, {t} with it, in an edit for your review. Once you accept the edit, the patch names {t} too, and the mark is gone."),
                )
            }
        };
        out.extend([
            Piece::Blank,
            Piece::Text("What you can do:".into()),
            Piece::Text(format!("Choose Reconcile with HEAD, in the menu of patch {n} or of HEAD's line in the patches pane: {reconcile}")),
        ]);
        let no_longer = match m {
            Mark::Revision { theirs: t, .. } => format!(", so that it no longer names {t}"),
            Mark::Message => String::new(),
        };
        out.push(Piece::Text(format!("Or give the commit the patch's message, in your checkout{no_longer}. Run this command:")));
        if *sha == self.head.commit {
            out.push(Piece::Command("git commit --amend".into()));
            out.push(Piece::Text(format!("Then write the message of patch {n} as the stack has it.")));
        } else {
            out.push(Piece::Command(format!("git rebase -i {}^", sha12(sha))));
            out.push(Piece::Text(format!(
                "In the list it opens, change `pick` to `reword` on the first line, the commit's, then write the message of patch {n} as the stack has it."
            )));
        }
        out.push(Piece::Text(self.then_said(Why::Marked(k)).into()));
        (title, out)
    }

    /// HEAD on another base: HEAD and the base, how they stand once read;
    /// then stacked's comment: why Move HEAD does not move HEAD from there,
    /// and how to put HEAD on the base, its commits kept.
    fn elsewhere_said(&self) -> (String, Vec<Piece>) {
        let h = &self.head;
        let base = sha12(&self.base);
        let on = match &h.branch {
            Some(b) => format!("on {b}"),
            None => "detached".to_string(),
        };
        let apart = match &self.read {
            Some(Ok(r)) => r.apart.as_ref(),
            _ => None,
        };
        let mut out = vec![Piece::Heading("HEAD and the stack's base".into())];
        match apart {
            None => {
                out.push(Piece::Line(format!("HEAD      {}, {on}", sha12(&h.commit))));
                out.push(Piece::Line(format!("the base  {base}")));
                out.push(Piece::Blank);
                out.push(match &self.read {
                    Some(Err(e)) => Piece::Text(format!("stacked could not read how HEAD and the stack's base stand from your checkout: {e}")),
                    _ => Piece::Waiting("Reading how HEAD and the stack's base stand, from your checkout…".into()),
                });
            }
            Some(a) => {
                out.push(Piece::Line(format!("HEAD      {}, {on}: {}", sha12(&h.commit), a.subject)));
                out.push(Piece::Line(match &a.base {
                    Some(s) => format!("the base  {base}: {s}"),
                    None => format!("the base  {base}, which your checkout does not have"),
                }));
                out.push(Piece::Blank);
                match (&a.base, &a.shared) {
                    (None, _) => {}
                    (Some(_), None) => out.push(Piece::Text("They share no commit: HEAD's history is not the base's.".into())),
                    (Some(_), Some((_, 0, theirs))) => out.push(Piece::Text(format!(
                        "HEAD is under the base, {} commit{} behind it: HEAD has no commit the base does not have.",
                        crate::paint::thousands(*theirs),
                        plural(*theirs)
                    ))),
                    (Some(_), Some((s, ours, theirs))) => out.push(Piece::Text(format!(
                        "They share {}: HEAD has {} commit{} since it, and the base {}.",
                        sha12(s),
                        crate::paint::thousands(*ours),
                        plural(*ours),
                        crate::paint::thousands(*theirs)
                    ))),
                }
                if !a.latest.is_empty() {
                    out.push(Piece::Text(match a.shared {
                        Some(_) => "HEAD's latest commits since then, the newest first:".into(),
                        None => "HEAD's latest commits, the newest first:".into(),
                    }));
                    for (sha, s) in &a.latest {
                        out.push(Piece::Line(format!("○ {} {s}", sha8(sha))));
                    }
                    if let Some((_, ours, _)) = a.shared
                        && ours > a.latest.len()
                    {
                        let more = ours - a.latest.len();
                        out.push(Piece::Line(format!("○ … {} more commit{}", crate::paint::thousands(more), plural(more))));
                    }
                }
            }
        }
        out.extend([
            Piece::Blank,
            Piece::Heading("stacked's comment".into()),
            Piece::Text("Move HEAD moves HEAD along the stack: to its base, or to a patch, the patches under it applied there. From another base, Move HEAD would move HEAD away from the commits HEAD has there, and stacked never moves HEAD away from commits stacked did not make: those commits may be yours.".into()),
            Piece::Blank,
            Piece::Text("What you can do, in your checkout:".into()),
            Piece::Text("Commit or stash the changes you have not committed, if you have any: Move HEAD does not move HEAD while files git tracks have changes.".into()),
        ]);
        // On no branch, with commits of its own, or not known yet to have none.
        let own = !matches!(apart.and_then(|a| a.shared.as_ref()), Some((_, 0, _)));
        if h.branch.is_none() && own {
            out.push(Piece::Text(format!(
                "HEAD is on no branch: put its commits on a branch first, to keep them. The branch is named {BRANCH} here, but any name not taken will do:"
            )));
            out.push(Piece::Command(format!("git branch {BRANCH} {}", sha12(&h.commit))));
        }
        out.push(Piece::Text(match apart {
            Some(a) if a.base.is_none() => format!("Your checkout does not have the stack's base, {base}: fetch it into your checkout first, then switch your checkout to it:"),
            _ => "Then switch your checkout to the stack's base:".into(),
        }));
        out.push(Piece::Command(format!("git switch --detach {base}")));
        let back = match &h.branch {
            Some(b) => format!(" `git switch {b}` takes you back to HEAD's commits whenever you want."),
            None if own => format!(" `git switch {BRANCH}` takes you back to HEAD's commits whenever you want."),
            None => String::new(),
        };
        out.push(Piece::Text(format!("Then Move HEAD can move HEAD to any patch of the stack.{back}")));
        out.push(Piece::Text("Or keep HEAD where it is, and leave Move HEAD alone.".into()));
        (" HEAD, not on the stack's base".into(), out)
    }
}

impl App {
    /// Why Move HEAD does not move HEAD, and what you can do so that it
    /// can, in a view of its own, opened from the screen this is on; what
    /// it shows of the commits read of the checkout away from the screen.
    pub(super) fn open_head_why(&mut self) {
        let (Some(h), Some(moves)) = (self.head_here().cloned(), self.head_moves_here()) else { return };
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        self.head_why = Some(Box::new(HeadWhy::new(&self.db, &self.stack, h, moves, rev, self.screen)));
        self.screen = Screen::HeadWhy;
        self.read_head_why();
    }

    /// What the view shows of the commits read of the checkout, away from
    /// the screen (`head::read`): the bar says so meanwhile.
    fn read_head_why(&mut self) {
        let Some(v) = &self.head_why else { return };
        let (h, base) = (v.head.clone(), v.base.clone());
        self.bg.doing.push(READING);
        self.spawn(move || {
            let read = crate::head::read(&h, &base).map_err(|e| e.0);
            Done::HeadRead(Box::new((h, read)))
        });
    }

    /// What was read of the checkout for the view of HEAD `h`: the view's,
    /// if it is still of that HEAD.
    pub(super) fn head_why_read(&mut self, h: Head, read: std::result::Result<Read, String>) {
        if let Some(k) = self.bg.doing.iter().position(|d| *d == READING) {
            self.bg.doing.remove(k);
        }
        if let Some(v) = self.head_why.as_mut().filter(|v| v.head == h) {
            v.read = Some(read);
            v.built = None;
        }
    }

    /// HEAD looked at again, the view open: made again if HEAD, what Move
    /// HEAD may do or the stack changed, the cursor on the line it was on
    /// if it is still there; the checkout read again if HEAD changed.
    pub(super) fn follow_head_why(&mut self) {
        let Some(v) = self.head_why.as_ref().filter(|_| self.screen == Screen::HeadWhy) else { return };
        let (Some(h), Some(moves)) = (self.head_here().cloned(), self.head_moves_here()) else { return };
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        if v.head == h && v.moves == moves && v.rev == rev {
            return;
        }
        let again = v.head != h;
        let mut new = HeadWhy::new(&self.db, &self.stack, h, moves, rev, v.back);
        if !again {
            new.read = v.read.clone();
        }
        if let Some(at) = new.lines.iter().position(|l| l.why == Some(v.here())) {
            (new.sel, new.in_main, new.scroll, new.wide) = (at, v.in_main, v.scroll, v.wide);
        }
        self.head_why = Some(Box::new(new));
        if again {
            self.read_head_why();
        }
    }

    /// The view left, for the screen it was opened from.
    fn leave_head_why(&mut self) {
        self.screen = self.head_why.take().map_or(Screen::Main, |v| v.back);
    }

    /// A key in the view: ↑↓ from line to line of its list, → (or Enter)
    /// into the pane on the right to scroll it, → again to see it wide, the
    /// list out of sight, ← back; Esc, or ← from the list, back to where it
    /// was opened from.
    pub(super) fn head_why_key(&mut self, k: KeyEvent) -> Effect {
        let (page, narrow) = (self.page(), self.body_w < 60);
        let Some(v) = self.head_why.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let stops: Vec<usize> = v.lines.iter().enumerate().filter(|(_, l)| l.why.is_some()).map(|(i, _)| i).collect();
        let step = |from: usize, down: bool, n: usize| -> usize {
            let mut at = stops.iter().position(|&i| i == from).unwrap_or(0);
            for _ in 0..n {
                at = if down { (at + 1).min(stops.len().saturating_sub(1)) } else { at.saturating_sub(1) };
            }
            stops.get(at).copied().unwrap_or(from)
        };
        let last = v.rows.len().saturating_sub(1);
        match (v.in_main, k.code) {
            (_, KeyCode::Esc) | (false, KeyCode::Left) => self.leave_head_why(),
            (false, KeyCode::Right | KeyCode::Enter) => {
                (v.in_main, v.wide) = (true, narrow);
            }
            (true, KeyCode::Right) => v.wide = true,
            (true, KeyCode::Left) if v.wide && !narrow => v.wide = false,
            (true, KeyCode::Left) => (v.in_main, v.wide) = (false, false),
            (false, code) => {
                let was = v.sel;
                v.sel = match code {
                    KeyCode::Down => step(v.sel, true, 1),
                    KeyCode::Up => step(v.sel, false, 1),
                    KeyCode::PageDown => step(v.sel, true, page),
                    KeyCode::PageUp => step(v.sel, false, page),
                    KeyCode::Home => stops.first().copied().unwrap_or(0),
                    KeyCode::End => stops.last().copied().unwrap_or(0),
                    _ => v.sel,
                };
                if v.sel != was {
                    v.scroll = 0;
                }
            }
            (true, code) => {
                v.scroll = match code {
                    KeyCode::Down => (v.scroll + 1).min(last),
                    KeyCode::Up => v.scroll.saturating_sub(1),
                    KeyCode::PageDown => (v.scroll + page).min(last),
                    KeyCode::PageUp => v.scroll.saturating_sub(page),
                    KeyCode::Home => 0,
                    KeyCode::End => last,
                    _ => v.scroll,
                };
            }
        }
        Effect::None
    }

    /// The line `i` of the view's list picked, as the arrows would: the
    /// cursor in the list.
    pub(super) fn pick_why_line(&mut self, i: usize) {
        if let Some(v) = self.head_why.as_mut()
            && v.lines.get(i).is_some_and(|l| l.why.is_some())
        {
            if v.sel != i {
                v.scroll = 0;
            }
            (v.sel, v.in_main, v.wide) = (i, false, false);
        }
    }

    /// Whether the view's cursor is on line `i` of its list.
    pub(super) fn on_why_line(&self, i: usize) -> bool {
        self.head_why.as_ref().is_some_and(|v| v.sel == i && !v.in_main)
    }

    /// The view: its list on the left, the line the cursor is on lit; the
    /// pane on the right, what it says of that line, from where it is
    /// scrolled to. Too narrow for both side by side, the one the cursor is
    /// in. Its rows are made again only for another line, or another width,
    /// or once what is read of the checkout is in.
    pub(super) fn draw_head_why(&mut self, buf: &mut Buffer, body: Rect) {
        let (w, h) = (body.width as usize, body.height as usize);
        let Some(v) = self.head_why.as_mut() else { return };
        let list_w = if w < 60 { w } else { (w * 2 / 5).max(30) };
        let wide = v.in_main && (v.wide || w < 60);
        let room = h.saturating_sub(1);
        if !wide {
            let list = Rect::new(body.x, body.y, list_w as u16, body.height);
            put(buf, list, 0, 0, list_w, &fit(" why Move HEAD does not move HEAD", list_w), color::header());
            if v.sel < v.top {
                v.top = v.sel;
            } else if v.sel >= v.top + room {
                v.top = v.sel + 1 - room;
            }
            for (y, (i, l)) in v.lines.iter().enumerate().skip(v.top).take(room).enumerate() {
                let mut st = l.style;
                if i == v.sel {
                    st = if v.in_main { st.patch(color::picked()) } else { on_cursor(st) };
                }
                put(buf, list, 0, y + 1, list_w, &fit(&l.text, list_w), st);
                if l.why.is_some() {
                    spot_in(&self.spots, list, (0, y + 1, list_w, 1), Spot::WhyLine(i));
                }
            }
            if w < 60 {
                self.areas.borrow_mut().push(list);
                return;
            }
            for y in 0..body.height {
                buf.set_string(body.x + list_w as u16, body.y + y, "│", color::meta());
            }
            self.areas.borrow_mut().push(list);
        }
        let (main_x, main_w) = if wide { (0, w) } else { (list_w + 1, w - list_w - 1) };
        let main = Rect::new(body.x + main_x as u16, body.y, main_w as u16, body.height);
        self.areas.borrow_mut().push(main);
        let fold = main_w.saturating_sub(1);
        if v.built != Some((v.sel, fold)) {
            let (title, pieces) = v.said_here();
            (v.title, v.rows, v.built) = (title, rows_of(&pieces, fold), Some((v.sel, fold)));
        }
        let mut head = color::header();
        if v.in_main {
            head = head.add_modifier(Modifier::UNDERLINED);
        }
        put(buf, main, 0, 0, main_w, &fit(&v.title, main_w), head);
        v.scroll = v.scroll.min(v.rows.len().saturating_sub(1));
        for (y, r) in v.rows.iter().skip(v.scroll).take(room).enumerate() {
            put_parts(buf, main, 0, y + 1, main_w, &row_parts(r, false));
        }
    }

    /// What the bar says in the view: the keys there are, in the list or in
    /// the pane on the right.
    pub(super) fn head_why_hint(&self) -> String {
        match self.head_why.as_ref() {
            Some(v) if v.in_main && (v.wide || self.body_w < 60) => "↑↓: scroll · ←: narrower · esc: back · ?: what can I do here".into(),
            Some(v) if v.in_main => "↑↓: scroll · →: wider · ←: back to the list · esc: back · ?: what can I do here".into(),
            _ => "↑↓ choose · →: read it · esc: back · space: menu · ?: what can I do here".into(),
        }
    }

    /// What `?` says in the view.
    pub(super) fn head_why_help(&self) -> Vec<String> {
        let marks = if cfg!(feature = "moz-phab") {
            "a commit with its patch's diff but another message (≠), or naming a revision its patch does not name (!)"
        } else {
            "a commit with its patch's diff but another message (≠)"
        };
        let keys = match self.head_why.as_ref() {
            Some(v) if v.in_main => "Press ↑↓ (pgup pgdn home end) to scroll what the pane on the right says, → to see it wide, the list out of sight, and ← to come back to the list; Esc to go back.",
            _ => "Press ↑↓ for another line of the list; → or Enter to scroll what the pane on the right says, → again to see it wide, and ← to come back; Esc, or ←, to go back.",
        };
        vec![
            "This view says why Move HEAD does not move HEAD of your checkout from where HEAD is, and what you can do in your checkout so that Move HEAD can move HEAD. The view is only to read: nothing in it changes your checkout or the stack, and stacked runs none of the commands it gives.".into(),
            format!(
                "On the left, where HEAD is, then each thing that keeps Move HEAD from moving HEAD, a line each: a commit only your checkout has; {marks}; or HEAD on another base. On the right, the line the cursor is on: what it is, then stacked's comment, which says what you can do about it."
            ),
            "Once your checkout changes, stacked reads HEAD again, and this view with it.".into(),
            keys.into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::head::OnStack;

    /// HEAD at `c`, on main or detached, on the stack with `commits`,
    /// `marks` and `above`.
    fn head(c: &str, branch: Option<&str>, commits: &[&str], marks: Vec<(usize, &str, Mark)>, above: &[&str]) -> Head {
        Head {
            dir: std::path::PathBuf::new(),
            branch: branch.map(String::from),
            commit: c.into(),
            at: At::Stack(OnStack {
                commits: commits.iter().map(|s| s.to_string()).collect(),
                marks: marks.into_iter().map(|(k, m, mark)| (k, m.to_string(), mark)).collect(),
                above: above.iter().map(|s| s.to_string()).collect(),
                subjects: above.iter().map(|s| format!("Subject of {s}")).collect(),
            }),
        }
    }

    /// The view of `h`, as Move HEAD sees it with nothing remembered, the
    /// stack's patches `Bug N - part N`, its base `bbbbbbbbbbbbbbbb`.
    fn view(h: Head) -> HeadWhy {
        let moves = h.moves(None);
        let n = match &h.at {
            At::Stack(on) => on.commits.len().max(2),
            At::Elsewhere => 2,
        };
        let mut v = HeadWhy {
            head: h,
            moves,
            name: "s".into(),
            base: "bbbbbbbbbbbbbbbbbbbb".into(),
            rev: None,
            patches: (1..=n).map(|k| (format!("Bug {k} - part {k}"), format!("Bug {k} - part {k}"))).collect(),
            read: None,
            lines: Vec::new(),
            sel: 0,
            top: 0,
            in_main: false,
            scroll: 0,
            wide: false,
            title: String::new(),
            rows: Vec::new(),
            built: None,
            back: Screen::Main,
        };
        v.lines = v.list();
        v
    }

    /// What the pieces of `why` say that is a command.
    fn commands(v: &HeadWhy, why: Why) -> Vec<String> {
        v.said(why).1.into_iter().filter_map(|p| if let Piece::Command(c) = p { Some(c) } else { None }).collect()
    }

    #[test]
    fn each_thing_that_keeps_move_head_from_moving_head_is_a_line_of_the_list() {
        let h = head("dddddddddddddddd", Some("main"), &["aaaaaaaaaaaaaaaa"], vec![(0, "Bug 1 - other words", Mark::Message)], &["cccccccccccccccc", "dddddddddddddddd"]);
        let v = view(h);
        assert_eq!(
            v.texts(),
            [
                " where HEAD is, and why",
                " commits only your checkout has",
                "   ○ cccccccc Subject of cccccccccccccccc",
                "   ○ dddddddd Subject of dddddddddddddddd",
                " messages other than their patch's",
                "   ≠ 1 Bug 1 - part 1",
            ]
        );
        // On a branch: it keeps them; back to patch 1's commit.
        assert_eq!(commands(&v, Why::Own(1)), ["git switch --detach aaaaaaaaaaaa"]);
        // The commit below the top: reworded with a rebase.
        assert_eq!(commands(&v, Why::Marked(0)), ["git rebase -i aaaaaaaaaaaa^"]);
        // Nothing marked, nothing above: nothing keeps it, said.
        let v = view(head("aaaaaaaaaaaaaaaa", None, &["aaaaaaaaaaaaaaaa"], vec![], &[]));
        assert_eq!(v.texts(), [" where HEAD is, and why"]);
        assert!(matches!(&v.said(Why::Where).1[2], Piece::Text(t) if t.starts_with("Nothing keeps Move HEAD from moving HEAD now")), "{:?}", v.said(Why::Where));
    }

    #[test]
    fn detached_the_commits_only_the_checkout_has_are_put_on_a_branch_first_and_head_goes_back_to_the_base_with_no_patch() {
        let v = view(head("dddddddddddddddd", None, &[], vec![], &["cccccccccccccccc", "dddddddddddddddd"]));
        assert_eq!(commands(&v, Why::Own(0)), ["git branch my-work dddddddddddd", "git switch --detach bbbbbbbbbbbb"]);
        let texts: Vec<String> = v.said(Why::Own(0)).1.into_iter().filter_map(|p| if let Piece::Text(t) = p { Some(t) } else { None }).collect();
        assert!(texts[0].starts_with("This commit is one of the 2 commits after the base that are not the stack's patches: only your checkout has them. "), "{texts:?}");
        assert!(texts.contains(&"Then go back to the stack's base, as your checkout has none of the stack's patches:".to_string()), "{texts:?}");
        assert!(texts.last().is_some_and(|t| t == "Then Move HEAD can move HEAD. `git switch my-work` takes you back to these commits whenever you want."), "{texts:?}");
    }

    #[test]
    fn the_top_commit_is_amended_and_one_marked_bang_locks_head() {
        let bang = Mark::Revision { theirs: "D34".into(), ours: None };
        let v = view(head("aaaaaaaaaaaaaaaa", None, &["aaaaaaaaaaaaaaaa"], vec![(0, "Bug 1 - part 1\n\nDifferential Revision: https://x/D34", bang)], &[]));
        assert_eq!(v.texts(), [" where HEAD is, and why", " revisions their patch does not name", "   ! 1 Bug 1 - part 1"]);
        assert_eq!(commands(&v, Why::Marked(0)), ["git commit --amend"]);
        let (title, pieces) = v.said(Why::Marked(0));
        assert_eq!(title, " the commit of patch 1, aaaaaaaaaaaa: it names D34");
        assert!(pieces.iter().any(|p| matches!(p, Piece::Text(t) if t.contains(" HEAD is locked below this patch: "))), "{pieces:?}");
        assert!(pieces.contains(&Piece::Messages("Bug 1 - part 1".into(), "Bug 1 - part 1\n\nDifferential Revision: https://x/D34".into())));
    }
}
