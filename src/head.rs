//! Where HEAD is, in the checkout stacked runs in, against a stack: on its
//! base with some of its patches as commits, a commit each, then maybe
//! commits that are not the stack's, forked from it; or on another base. A
//! commit is a patch when
//! its diff is, by `git patch-id`; its message may still differ, and the
//! commit is marked so: `≠` when its message says something other than the
//! patch's; `!`, with the moz-phab feature, when it names a Differential
//! Revision the patch does not name (none, or another), which wins over
//! `≠`. stacked never writes a commit's message into the stack.
//!
//! And HEAD moved to a place on the stack (`move_to`, Move HEAD): detached
//! there, the commits the checkout has that are the stack's taken as they
//! are, the rest applied, the branch it was on as it was. A commit marked
//! `!` is never rewritten nor dropped: everything up to and including the
//! last of them is kept as it is, and HEAD goes nowhere below it. Above it,
//! Move HEAD replaces commits only when HEAD is exactly the commit stacked
//! last left it at, in this checkout on this stack (the database remembers
//! it, each move replacing it: `Db::moved`), whatever the stack has become
//! since; or when HEAD is on the stack, its commits the stack's first
//! patches with nothing above them and no mark. Else it is refused, with
//! why: the commits might be yours, made or amended outside stacked. HEAD
//! following a review (`follow`) moves only from where it was seen in sync
//! with the stack. Each move remembers where it left HEAD.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::{Db, Patch, Result, Stack, err};
use crate::git::{self, git, git_in};
use crate::ops;

/// Where HEAD is.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    /// The checkout's top.
    pub dir: PathBuf,
    /// The branch it is on, if it is on one; its commit.
    pub branch: Option<String>,
    pub commit: String,
    pub at: At,
}

#[derive(Debug, Clone, PartialEq)]
pub enum At {
    /// On the stack's base, its first patches as commits, if any, then the
    /// commits of its own, if any.
    Stack(OnStack),
    /// Not on the stack: its base is not below HEAD.
    Elsewhere,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OnStack {
    /// The commits that are the stack's first patches, bottom first: the
    /// trunk HEAD and the stack share.
    pub commits: Vec<String>,
    /// Of those, the places (from 0) that are marked: the commit's message
    /// as taken, and its mark.
    pub marks: Vec<(usize, String, Mark)>,
    /// The commits above them to HEAD, which are not the stack's, bottom
    /// first: with them, HEAD forks from the stack.
    pub above: Vec<String>,
    /// The subjects of the first of those, as many as `SUBJECTS`.
    pub subjects: Vec<String>,
}

/// How many of the commits HEAD alone has are read by their subject: as
/// many as the patches pane draws.
pub const SUBJECTS: usize = 3;

/// The mark of a commit of HEAD's with the same diff as its patch, whose
/// message is not the patch's.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// `≠`: its message says something other than the patch's, as compared
    /// (`compared`).
    Message,
    /// `!`, with the moz-phab feature: it names a Differential Revision,
    /// `theirs` (`D123`), that the patch does not name; the patch's own, if
    /// it names one.
    Revision { theirs: String, ours: Option<String> },
}

impl Mark {
    /// As the patches pane draws it.
    pub fn sign(&self) -> &'static str {
        match self {
            Mark::Message => "≠",
            Mark::Revision { .. } => "!",
        }
    }

    /// What it says of the commit of patch `k` (from 0): `the commit of
    /// patch 2 names D34, which the patch does not name`.
    pub fn said(&self, k: usize) -> String {
        let n = k + 1;
        match self {
            Mark::Message => format!("the commit of patch {n} has a message other than the patch's"),
            Mark::Revision { theirs, ours: None } => format!("the commit of patch {n} names {theirs}, which the patch does not name"),
            Mark::Revision { theirs, ours: Some(o) } => format!("the commit of patch {n} names {theirs}, where the patch names {o}"),
        }
    }
}

/// Which case lets Move HEAD replace HEAD's commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// HEAD is exactly the commit stacked last left it at, in this checkout
    /// on this stack: Move HEAD replaces it with the stack as it is now,
    /// whatever the stack has become since.
    AsLeft,
    /// HEAD is on the stack, its commits the stack's first patches, with
    /// nothing above them and no mark.
    OnStack,
}

/// What Move HEAD may do from where HEAD is: the case that lets it, else
/// why it is refused; and how many patches HEAD keeps as they are in the
/// checkout, up to and including its last commit marked `!`, below which
/// it goes nowhere (none when no commit is so marked).
#[derive(Debug, Clone, PartialEq)]
pub struct Moves {
    pub case: std::result::Result<Case, String>,
    pub locked: usize,
}

impl Moves {
    /// Whether HEAD may be moved to the place with `n` patches.
    pub fn to(&self, n: usize) -> bool {
        self.case.is_ok() && n >= self.locked
    }

    /// Whether a file may be edited by hand in this checkout, moved to the
    /// patch for the edit and back: only while Move HEAD may move HEAD,
    /// and no commit is marked `!`.
    pub fn edits_here(&self) -> bool {
        self.case.is_ok() && self.locked == 0
    }
}

impl Head {
    /// Where it is in git's words: `on main at 1a2b3c4d5e6f`, `detached at
    /// 1a2b3c4d5e6f`.
    pub fn git_said(&self) -> String {
        let short = &self.commit[..12.min(self.commit.len())];
        match &self.branch {
            Some(b) => format!("on {b} at {short}"),
            None => format!("detached at {short}"),
        }
    }

    /// The branch HEAD is on, else its commit, short.
    pub fn on(&self) -> String {
        self.branch.clone().unwrap_or_else(|| self.commit[..8.min(self.commit.len())].to_string())
    }

    /// How many of the stack's patches it has, if it is on the stack.
    pub fn patches(&self) -> Option<usize> {
        match &self.at {
            At::Stack(on) => Some(on.commits.len()),
            At::Elsewhere => None,
        }
    }

    /// The mark of the commit of patch `k` (from 0), if it has one.
    pub fn mark(&self, k: usize) -> Option<&Mark> {
        let At::Stack(on) = &self.at else { return None };
        on.marks.iter().find(|(at, _, _)| *at == k).map(|(_, _, m)| m)
    }

    /// Whether it is in sync with a stack of `n` patches: on its top, each
    /// patch a commit with the patch's message, its trailer naming the
    /// revision too, and no commit above.
    pub fn in_sync(&self, n: usize) -> bool {
        matches!(&self.at, At::Stack(on) if on.commits.len() == n && on.marks.is_empty() && on.above.is_empty())
    }

    /// How many patches HEAD keeps as they are, up to and including its
    /// last commit marked `!`: none when no commit is.
    pub fn locked(&self) -> usize {
        let At::Stack(on) = &self.at else { return 0 };
        on.marks.iter().filter(|(_, _, m)| matches!(m, Mark::Revision { .. })).map(|(k, _, _)| k + 1).max().unwrap_or(0)
    }

    /// Why HEAD goes nowhere below its last commit marked `!`, said, if one
    /// is: `HEAD is locked below patch 2: the commit of patch 2 names D34,
    /// which the patch does not name`.
    pub fn lock_said(&self) -> Option<String> {
        let n = self.locked();
        let m = self.mark(n.checked_sub(1)?)?;
        Some(format!("HEAD is locked below patch {n}: {}", m.said(n - 1)))
    }

    /// What Move HEAD may do from here, stacked having last left HEAD at
    /// commit `left` in this checkout on this stack, if it did.
    pub fn moves(&self, left: Option<&str>) -> Moves {
        let locked = self.locked();
        let case = if left == Some(self.commit.as_str()) {
            Ok(Case::AsLeft)
        } else {
            match &self.at {
                At::Stack(on) if on.above.is_empty() && on.marks.is_empty() => Ok(Case::OnStack),
                At::Stack(on) => Err(not_made(on)),
                At::Elsewhere => Err(format!("HEAD is not on the stack's base, nor where stacked last left it: {ONLY}")),
            }
        };
        Moves { case, locked }
    }

    /// How many of HEAD's commits a move to the place with `n` patches
    /// keeps as they are: those up to and including the last marked `!`,
    /// then those after it that are their patch, message too, up to the
    /// place.
    fn keeps(&self, n: usize) -> usize {
        let At::Stack(on) = &self.at else { return 0 };
        let mut keep = self.locked();
        while keep < on.commits.len() && keep < n && self.mark(keep).is_none() {
            keep += 1;
        }
        keep
    }

    /// Where it is on stack `name`, said: `at patch 2 of s, then 1 commit
    /// not in the stack`, `not on s`.
    pub fn said(&self, name: &str) -> String {
        let At::Stack(on) = &self.at else {
            return format!("not on {name}: the stack's base is not below it");
        };
        let mut out = format!("at {}", place(name, on.commits.len()));
        if !on.above.is_empty() {
            let n = on.above.len();
            out += &format!(", then {n} commit{} not in the stack", if n == 1 { "" } else { "s" });
        }
        out
    }
}

/// When Move HEAD is offered, said after why it is not.
pub const ONLY: &str = "Move HEAD is offered only when HEAD is on the stack with no marks, or as stacked last left it";

/// Why Move HEAD does not replace the commits of `on`, HEAD not where
/// stacked last left it: what of them stacked did not make.
fn not_made(on: &OnStack) -> String {
    let mut what = Vec::new();
    if !on.above.is_empty() {
        let n = on.above.len();
        let (commits, are) = if n == 1 { ("1 commit".to_string(), "is") } else { (format!("{n} commits"), "are") };
        let after = match on.commits.len() {
            0 => "the base".to_string(),
            k => format!("patch {k}"),
        };
        what.push(format!("{commits} after {after} {are} not the stack's"));
    }
    let messages: Vec<usize> = on.marks.iter().filter(|(_, _, m)| *m == Mark::Message).map(|(k, _, _)| *k).collect();
    if !messages.is_empty() {
        let (s, have) = if messages.len() == 1 { ("", "has") } else { ("s", "have") };
        what.push(format!("the commit{s} of {} {have} a message other than the stack's", patches_said(&messages)));
    }
    for (k, _, m) in on.marks.iter().filter(|(_, _, m)| *m != Mark::Message) {
        what.push(m.said(*k));
    }
    format!("HEAD has commits stacked did not make, as {}: {ONLY}", and(&what))
}

/// `a`, `a and b`, `a, b and c`.
fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `the base of s`, `patch 2 of s`: HEAD's place with `n` of stack `name`'s
/// patches.
pub fn place(name: &str, n: usize) -> String {
    match n {
        0 => format!("the base of {name}"),
        n => format!("patch {n} of {name}"),
    }
}

/// `patch 2`, `patches 2 and 3`, `patches 1, 2 and 4`: places from 0.
pub fn patches_said(places: &[usize]) -> String {
    let n: Vec<String> = places.iter().map(|k| (k + 1).to_string()).collect();
    match n.as_slice() {
        [one] => format!("patch {one}"),
        [rest @ .., last] => format!("patches {} and {last}", rest.join(", ")),
        [] => "no patch".into(),
    }
}

/// Where HEAD is in the checkout at `here`, against stack `name`: none if
/// `here` is no checkout of the stack's repository, or the stack has no
/// base.
pub fn find(db: &Db, name: &str, here: &Path) -> Result<Option<Head>> {
    let s = db.stack(name)?;
    let patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    of_stack(s, &patches, here)
}

/// As `find`, of stack `s` of `patches`.
pub fn of_stack(s: &Stack, patches: &[&Patch], here: &Path) -> Result<Option<Head>> {
    let Ok((repo, base)) = ops::base_of(s) else { return Ok(None) };
    let Ok(dir) = git::checkout_of(&repo, here) else { return Ok(None) };
    Ok(Some(locate(&dir, &base, patches)?))
}

/// Where HEAD is in the checkout at `here` on stack `name` of `db`, and
/// what Move HEAD may do from there: none if `here` is no checkout of the
/// stack's repository.
pub fn moves_of(db: &Db, name: &str, here: &Path) -> Result<Option<(Head, Moves)>> {
    let Some(h) = find(db, name, here)? else { return Ok(None) };
    let moves = h.moves(db.moved(name, &h.dir));
    Ok(Some((h, moves)))
}

/// Where HEAD is at `dir`, against the stack of `patches` on `base`.
fn locate(dir: &Path, base: &str, patches: &[&Patch]) -> Result<Head> {
    let commit = git::rev_parse(dir, "HEAD")?;
    let branch = match git::head_of(dir)? {
        git::Head::Branch(b) => Some(b),
        git::Head::Commit(_) => None,
    };
    let at = match git::rev_parse(dir, base) {
        Ok(base) => on_stack(dir, &base, &commit, patches)?,
        Err(_) => At::Elsewhere,
    };
    Ok(Head { dir: dir.to_path_buf(), branch, commit, at })
}

fn on_stack(dir: &Path, base: &str, head: &str, patches: &[&Patch]) -> Result<At> {
    if head == base {
        return Ok(At::Stack(OnStack::default()));
    }
    if git(dir, &["merge-base", "--is-ancestor", base, head]).is_err() {
        return Ok(At::Elsewhere);
    }
    let all: Vec<String> = git(dir, &["rev-list", "--reverse", "--topo-order", &format!("{base}..{head}")])?
        .lines()
        .map(String::from)
        .collect();
    let n = all.len().min(patches.len());
    let ids = commit_ids(dir, &all[..n])?;
    let messages = commit_messages(dir, &all[..n])?;
    let theirs = patch_ids(dir, &patches[..n])?;
    let mut on = OnStack::default();
    for (k, sha) in all[..n].iter().enumerate() {
        if ids.get(sha).is_none_or(|id| theirs.get(&patches[k].id) != Some(id)) {
            break;
        }
        let m = messages.get(sha).cloned().unwrap_or_default();
        if let Some(mark) = mark(&patches[k].message, &m) {
            on.marks.push((k, norm(&m), mark));
        }
        on.commits.push(sha.clone());
    }
    // The rest forks from the stack: what of it the patches pane draws, by
    // subject.
    on.above = all[on.commits.len()..].to_vec();
    on.subjects = subjects(dir, &on.above[..on.above.len().min(SUBJECTS)])?;
    Ok(At::Stack(on))
}

/// The subject of each of `shas`, in their order.
fn subjects(dir: &Path, shas: &[String]) -> Result<Vec<String>> {
    if shas.is_empty() {
        return Ok(vec![]);
    }
    let mut args = vec!["log", "--no-walk=unsorted", "--format=%s"];
    args.extend(shas.iter().map(String::as_str));
    Ok(git(dir, &args)?.lines().map(String::from).collect())
}

/// The patch id of each of `shas` that has a diff, by sha.
fn commit_ids(dir: &Path, shas: &[String]) -> Result<HashMap<String, String>> {
    if shas.is_empty() {
        return Ok(HashMap::new());
    }
    let mut args = vec!["log", "-p", "--no-walk=unsorted"];
    args.extend(shas.iter().map(String::as_str));
    let log = git(dir, &args)?;
    ids_of(&git_in(dir, &["patch-id", "--stable"], Some(&log))?)
}

/// The patch id of each of `patches`, by the patch's id.
fn patch_ids(dir: &Path, patches: &[&Patch]) -> Result<HashMap<String, String>> {
    if patches.is_empty() {
        return Ok(HashMap::new());
    }
    ids_of(&git_in(dir, &["patch-id", "--stable"], Some(&crate::mbox::format_mbox(patches)))?)
}

/// `git patch-id`'s lines, a patch id then a commit's, as commit to patch id.
fn ids_of(out: &str) -> Result<HashMap<String, String>> {
    Ok(out
        .lines()
        .filter_map(|l| {
            let (id, of) = l.split_once(' ')?;
            Some((of.trim().to_string(), id.to_string()))
        })
        .collect())
}

/// The message of each of `shas`, by sha.
fn commit_messages(dir: &Path, shas: &[String]) -> Result<HashMap<String, String>> {
    if shas.is_empty() {
        return Ok(HashMap::new());
    }
    let mut args = vec!["log", "--no-walk=unsorted", "--format=%H%x00%B%x00"];
    args.extend(shas.iter().map(String::as_str));
    let out = git(dir, &args)?;
    let parts: Vec<&str> = out.split('\0').collect();
    Ok(parts
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].trim().to_string(), c[1].to_string()))
        .collect())
}

/// A message as taken: each line's end trimmed, no blank line at either
/// end.
fn norm(m: &str) -> String {
    let lines: Vec<&str> = m.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |e| e + 1);
    lines[start..end].join("\n")
}

/// A message as compared with another: as taken (`norm`), empty lines in
/// a row within it one, as `git am` leaves them. Two messages that differ
/// by no more than that are the same, as HEAD moved by stacked, which
/// applies patches with `git am`, has them; each is written as it is.
fn compared(m: &str) -> String {
    let taken = norm(m);
    let mut lines: Vec<&str> = Vec::new();
    for l in taken.split('\n') {
        if !(l.is_empty() && lines.last().is_some_and(|p| p.is_empty())) {
            lines.push(l);
        }
    }
    lines.join("\n")
}

/// Whether two messages are the same, as compared (`compared`): what a
/// commit marked has, taken into its patch, unmarks it.
pub fn same_message(a: &str, b: &str) -> bool {
    compared(a) == compared(b)
}

/// The mark of a commit whose message is `theirs`, its patch's `ours`, if
/// it has one: `!` when it names a revision the patch does not, else `≠`
/// when the two say otherwise, as compared (`compared`).
fn mark(ours: &str, theirs: &str) -> Option<Mark> {
    #[cfg(feature = "moz-phab")]
    if let Some(t) = revision_named(theirs) {
        let o = revision_named(ours);
        if o.as_deref() != Some(t.as_str()) {
            return Some(Mark::Revision { theirs: t, ours: o });
        }
    }
    (compared(ours) != compared(theirs)).then_some(Mark::Message)
}

/// What the mark `m` of the commit of patch `k` (from 0) says of the two
/// messages, the patch's `ours` and the commit's `theirs`, as `stacked
/// head` lists it: `patch 3 names D12 in the stack and D34 in your
/// workspace's commit` when the two differ by the revision they name, or
/// would but for it; else `the message of patch 3 differs from its
/// commit's: "Bug 3 - other words"`, its first line.
pub fn mark_said(k: usize, ours: &str, theirs: &str, m: &Mark) -> String {
    let n = k + 1;
    match (m, named_alone(ours, theirs)) {
        (Mark::Revision { theirs: t, ours: o }, _) => {
            format!("patch {n} names {} in the stack and {t} in your workspace's commit", o.as_deref().unwrap_or("no revision"))
        }
        (Mark::Message, Some(o)) => format!("patch {n} names {o} in the stack and no revision in your workspace's commit"),
        (Mark::Message, None) => format!("the message of patch {n} differs from its commit's: \"{}\"", theirs.lines().next().unwrap_or("")),
    }
}

/// The revision patch message `ours` names, when commit message `theirs`
/// is the same but for that trailer, which it has not.
#[cfg(feature = "moz-phab")]
fn named_alone(ours: &str, theirs: &str) -> Option<String> {
    (compared(&without_trailer(ours)) == compared(&without_trailer(theirs))).then(|| revision_named(ours)).flatten()
}

/// Without the moz-phab feature, no message names a revision.
#[cfg(not(feature = "moz-phab"))]
fn named_alone(_: &str, _: &str) -> Option<String> {
    None
}

/// Message `m` without the trailer moz-phab writes to name its revision
/// (`progress::revision_trailers`).
#[cfg(feature = "moz-phab")]
fn without_trailer(m: &str) -> String {
    let trailers = crate::progress::revision_trailers(m);
    let kept: Vec<&str> = m.split('\n').enumerate().filter(|(i, _)| !trailers.contains(i)).map(|(_, l)| l).collect();
    kept.join("\n")
}

/// The revision the trailer moz-phab writes in message `m` names, `D123`,
/// if it has that trailer (`progress::revision_trailers`).
#[cfg(feature = "moz-phab")]
fn revision_named(m: &str) -> Option<String> {
    let at = *crate::progress::revision_trailers(m).last()?;
    m.split('\n').nth(at).map(revision_of)
}

/// The revision trailer `line` names: `D123`, after its link's last slash.
#[cfg(feature = "moz-phab")]
fn revision_of(line: &str) -> String {
    let line = line.trim();
    line.rsplit('/').next().unwrap_or(line).to_string()
}

/// Whether the lines of a message, `lines`, end in a paragraph of trailers
/// after an empty line: each of its lines a trailer (`trailer`). A submit
/// to Phabricator puts a new revision's trailer there, as moz-phab does.
#[cfg(feature = "moz-phab")]
pub(crate) fn ends_in_trailers(lines: &[&str]) -> bool {
    let start = lines.iter().rposition(|l| l.trim().is_empty()).map(|e| e + 1);
    start.is_some_and(|s| s < lines.len() && lines[s..].iter().all(|l| trailer(l)))
}

/// Whether `line` of a message is a trailer: a key, `: ` and a value, the
/// key a letter, then letters, digits, hyphens and spaces, as in
/// `Reviewed-by: ann`.
pub(crate) fn trailer(line: &str) -> bool {
    line.split_once(": ").is_some_and(|(key, _)| {
        key.starts_with(|c: char| c.is_ascii_alphabetic()) && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
    })
}

/// The checkout at `here`, a checkout of stack `s`'s repository, made sure
/// of before HEAD is moved: nothing of yours uncommitted to carry along. No
/// agent works there: every ask works in a worktree of its own.
fn ready(s: &Stack, here: &Path, nothing: &str) -> Result<(PathBuf, String)> {
    let (repo, base) = ops::base_of(s)?;
    let dir = git::checkout_of(&repo, here)?;
    if git::unfinished_tracked(&dir)? {
        return err(format!("{} has uncommitted changes: commit or stash them first; {nothing}", dir.display()));
    }
    Ok((dir, base))
}

/// HEAD of the checkout at `here` moved to stack `name` with its first `n`
/// patches applied, as Move HEAD moves it: only as `Head::moves` lets it,
/// HEAD as stacked last left it or on the stack with no mark, and to no
/// place below its last commit marked `!`; else refused, with why. Detached
/// there, the commits the checkout has that are kept (`Head::keeps`) taken
/// as they are, the rest applied as commits (unsigned); the branch it was
/// on as it was; and where it left HEAD remembered. What came of it, said.
pub fn move_to(path: &Path, name: &str, here: &Path, n: usize) -> Result<String> {
    let db = Db::load(path)?;
    let s = db.stack(name)?.clone();
    if n > s.patches.len() {
        return err(format!("{name} has {} patches", s.patches.len()));
    }
    let (dir, base) = ready(&s, here, "HEAD did not move")?;
    let patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    let head = locate(&dir, &base, &patches)?;
    let moves = head.moves(db.moved(name, &dir));
    if let Err(why) = &moves.case {
        return err(format!("{why}; HEAD did not move"));
    }
    if n < moves.locked {
        return err(format!("{}, and Move HEAD keeps every commit up to it as it is; HEAD did not move", head.lock_said().unwrap_or_default()));
    }
    put(path, name, &dir, &base, &patches, &head, n)
}

/// HEAD of the checkout at `here` following stack `name` to its top as a
/// review closes: only from `seen`, the commit it was at when it was seen
/// in sync with the stack before the review changed it; else refused.
/// Moved as `move_to` moves it, the commits marked `!` kept too, and where
/// it left HEAD remembered. What came of it, said.
pub fn follow(path: &Path, name: &str, here: &Path, seen: &str) -> Result<String> {
    let db = Db::load(path)?;
    let s = db.stack(name)?.clone();
    let (dir, base) = ready(&s, here, "HEAD did not move")?;
    let patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    let head = locate(&dir, &base, &patches)?;
    if head.commit != seen {
        return err("HEAD moved since it was seen in sync with the stack; HEAD did not move");
    }
    put(path, name, &dir, &base, &patches, &head, s.patches.len())
}

/// HEAD at `head`, in the checkout at `dir`, moved to the place with `n`
/// of `patches` on `base`: detached at the last commit it keeps
/// (`Head::keeps`), else at the base, the patches after it applied; the
/// branch it was on as it was, HEAD back where it was if a patch does not
/// apply. Where it left HEAD remembered, for stack `name` in the database
/// at `path`.
fn put(path: &Path, name: &str, dir: &Path, base: &str, patches: &[&Patch], head: &Head, n: usize) -> Result<String> {
    let keep = head.keeps(n);
    let start = match (&head.at, keep) {
        (At::Stack(on), k) if k > 0 => on.commits[k - 1].clone(),
        _ => git::rev_parse(dir, base)?,
    };
    let there = place(name, n);
    if keep == n && start == head.commit {
        return Ok(format!("HEAD is at {there} already, on {}", head.on()));
    }
    let back = git::head_of(dir)?;
    git(dir, &["switch", "--quiet", "--detach", &start])?;
    if n > keep
        && let Err(e) = git::am(dir, &crate::mbox::format_mbox(&patches[keep..n]))
    {
        let _ = git::back_to(dir, &back);
        return err(format!("{}; HEAD is back where it was", e.0));
    }
    let mut said = match &head.branch {
        Some(b) => format!("HEAD is at {there}, detached: {b} is as it was"),
        None => format!("HEAD is at {there}, detached"),
    };
    let left = git::rev_parse(dir, "HEAD").and_then(|c| Db::edit(path, |db| db.set_moved(name, dir, &c)));
    if let Err(e) = left {
        said += &format!("; stacked could not remember where it left HEAD: {}", e.0);
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_compared_without_the_blanks_at_its_ends() {
        assert_eq!(norm("Bug 1 - two  \n\nBecause.\n\n"), "Bug 1 - two\n\nBecause.");
        assert_eq!(norm("\n\nBug 1\n"), "Bug 1");
        assert_eq!(norm(""), "");
    }

    /// The trailer moz-phab writes.
    const TRAILER: &str = "Differential Revision: https://phabricator.services.mozilla.com/D123";

    #[test]
    fn empty_lines_in_a_row_are_one_as_git_am_leaves_them() {
        // Two empty lines under the title, then one of spaces: one, as git
        // am leaves them; as taken, they stay.
        let run = "Bug 1 - two\n\n\n  \nBecause.";
        assert_eq!((compared(run), norm(run)), ("Bug 1 - two\n\nBecause.".to_string(), "Bug 1 - two\n\n\n\nBecause.".to_string()));
        assert_eq!(mark(run, "Bug 1 - two\n\nBecause.\n"), None);
        // A real difference beside them is one still, on either side.
        assert_eq!(mark(run, "Bug 1 - two\n\nWhy."), Some(Mark::Message));
        assert_eq!(mark("Bug 1 - two\n\nBecause.", "Bug 1 - two\n\n\nWhy."), Some(Mark::Message));
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_commit_naming_a_revision_its_patch_does_not_is_marked_so_whatever_else_differs() {
        let bare = "Bug 1 - two\n\nBecause.";
        let with = |d: &str| format!("{bare}\n\n{}", TRAILER.replace("D123", d));
        let named = |theirs: &str, ours: Option<&str>| Some(Mark::Revision { theirs: theirs.into(), ours: ours.map(String::from) });
        assert_eq!(mark(&with("D123"), &with("D123")), None);
        assert_eq!(mark(bare, &format!("{bare}\n")), None);
        // The commit names one, the patch none or another: `!`, whatever
        // else they say.
        assert_eq!(mark(bare, &with("D123")), named("D123", None));
        assert_eq!(mark(&with("D12"), &with("D34")), named("D34", Some("D12")));
        assert_eq!(mark("Bug 1 - two\n\nWhy.", &with("D34")), named("D34", None));
        let signed = format!("{bare}\n\nReviewed-by: ann");
        assert_eq!(mark(&signed, &format!("{signed}\n{TRAILER}")), named("D123", None));
        // The patch alone names one: its message differs, `≠`.
        assert_eq!(mark(&with("D123"), bare), Some(Mark::Message));
        // The same line in the middle of the message is not the trailer.
        assert_eq!(mark(bare, &format!("Bug 1 - two\n\n{TRAILER}\n\nBecause.")), Some(Mark::Message));
        // As `stacked head` lists them.
        let said = |ours: &str, theirs: &str| mark_said(2, ours, theirs, &mark(ours, theirs).unwrap());
        assert_eq!(said(&with("D12"), &with("D34")), "patch 3 names D12 in the stack and D34 in your workspace's commit");
        assert_eq!(said(bare, &with("D34")), "patch 3 names no revision in the stack and D34 in your workspace's commit");
        assert_eq!(said(&with("D12"), bare), "patch 3 names D12 in the stack and no revision in your workspace's commit");
        assert_eq!(said(&with("D12"), "Bug 1 - other\n\nWhy."), "the message of patch 3 differs from its commit's: \"Bug 1 - other\"");
    }

    /// HEAD at commit `c`, on the stack with `commits`, `marks` and
    /// `above`.
    fn head(c: &str, commits: &[&str], marks: Vec<(usize, Mark)>, above: &[&str]) -> Head {
        Head {
            dir: PathBuf::new(),
            branch: None,
            commit: c.into(),
            at: At::Stack(OnStack {
                commits: commits.iter().map(|s| s.to_string()).collect(),
                marks: marks.into_iter().map(|(k, m)| (k, "theirs".into(), m)).collect(),
                above: above.iter().map(|s| s.to_string()).collect(),
                subjects: vec![],
            }),
        }
    }

    fn d34(ours: Option<&str>) -> Mark {
        Mark::Revision { theirs: "D34".into(), ours: ours.map(String::from) }
    }

    #[test]
    fn move_head_replaces_commits_as_stacked_left_them_or_on_the_stack_with_no_mark() {
        // On the stack, no mark: anywhere, the identical commits kept.
        let h = head("b", &["a", "b"], vec![], &[]);
        let m = h.moves(None);
        assert_eq!((m.case.clone(), m.locked, m.to(0), m.edits_here()), (Ok(Case::OnStack), 0, true, true));
        assert_eq!((h.keeps(1), h.keeps(2)), (1, 2));
        assert!(h.in_sync(2) && !h.in_sync(3));
        // A commit of its own above: refused, unless stacked left it there.
        let h = head("c", &["a", "b"], vec![], &["c"]);
        let own = format!("HEAD has commits stacked did not make, as 1 commit after patch 2 is not the stack's: {ONLY}");
        assert_eq!((h.moves(None).case, h.moves(Some("b")).case), (Err(own.clone()), Err(own)));
        assert_eq!(h.moves(Some("c")).case, Ok(Case::AsLeft));
        // A message other than the stack's: the same; as stacked left it,
        // the commit is made again from its patch.
        let h = head("d", &["a", "b"], vec![(0, Mark::Message), (1, Mark::Message)], &["c", "d"]);
        assert_eq!(
            h.moves(None).case,
            Err(format!("HEAD has commits stacked did not make, as 2 commits after patch 2 are not the stack's and the commits of patches 1 and 2 have a message other than the stack's: {ONLY}"))
        );
        let h = head("b", &["a", "b"], vec![(1, Mark::Message)], &[]);
        assert_eq!((h.moves(Some("b")).case, h.keeps(2)), (Ok(Case::AsLeft), 1));
        // Elsewhere: as stacked left it alone.
        let elsewhere = Head { at: At::Elsewhere, ..head("x", &[], vec![], &[]) };
        assert_eq!(elsewhere.moves(None).case, Err(format!("HEAD is not on the stack's base, nor where stacked last left it: {ONLY}")));
        assert_eq!((elsewhere.moves(Some("x")).case, elsewhere.keeps(2)), (Ok(Case::AsLeft), 0));
    }

    #[test]
    fn a_commit_marked_bang_keeps_every_commit_up_to_it_and_head_nowhere_below() {
        // Patch 2's commit names D34, which the patch does not: locked
        // below patch 2, whatever the case.
        let h = head("c", &["a", "b", "c"], vec![(0, Mark::Message), (1, d34(None)), (2, Mark::Message)], &[]);
        assert_eq!(h.locked(), 2);
        assert_eq!(h.lock_said().as_deref(), Some("HEAD is locked below patch 2: the commit of patch 2 names D34, which the patch does not name"));
        assert!(h.moves(None).case.is_err());
        // As stacked left it: from patch 2 up, the commits up to it kept as
        // they are, patch 1's other message too; patch 3's made again.
        let m = h.moves(Some("c"));
        assert_eq!((m.case.clone(), m.locked), (Ok(Case::AsLeft), 2));
        assert_eq!((m.to(1), m.to(2), m.to(4), m.edits_here()), (false, true, true, false));
        assert_eq!((h.keeps(2), h.keeps(3), h.keeps(4)), (2, 2, 2));
        // The commits after it that are their patches, kept too.
        let h = head("d", &["a", "b", "c", "d"], vec![(1, d34(Some("D12")))], &[]);
        assert_eq!((h.keeps(2), h.keeps(3), h.keeps(4)), (2, 3, 4));
        assert_eq!(h.lock_said().as_deref(), Some("HEAD is locked below patch 2: the commit of patch 2 names D34, where the patch names D12"));
        // The last of two.
        let h = head("c", &["a", "b", "c"], vec![(0, d34(None)), (2, d34(None))], &[]);
        assert_eq!(h.locked(), 3);
        let why = h.moves(None).case.unwrap_err();
        assert!(why.contains("the commit of patch 1 names D34, which the patch does not name and the commit of patch 3 names D34"), "{why}");
        // No `!`, nothing locked.
        assert_eq!(head("b", &["a", "b"], vec![(1, Mark::Message)], &[]).lock_said(), None);
    }

    #[cfg(not(feature = "moz-phab"))]
    #[test]
    fn without_moz_phab_the_trailer_is_compared_as_any_line() {
        let with = format!("Bug 1 - two\n\nBecause.\n\n{TRAILER}");
        assert_eq!(mark("Bug 1 - two\n\nBecause.", &with), Some(Mark::Message));
        assert_eq!(mark_said(0, "Bug 1 - two\n\nBecause.", &with, &Mark::Message), "the message of patch 1 differs from its commit's: \"Bug 1 - two\"");
    }

    #[test]
    fn places_are_said_from_one() {
        assert_eq!(patches_said(&[1]), "patch 2");
        assert_eq!(patches_said(&[0, 2]), "patches 1 and 3");
        assert_eq!(patches_said(&[0, 1, 3]), "patches 1, 2 and 4");
        assert_eq!(place("s", 0), "the base of s");
        assert_eq!(place("s", 2), "patch 2 of s");
    }

    #[test]
    fn git_patch_id_lines_map_a_commit_to_its_patch_id() {
        let out = "aaa 111\nbbb 222\n";
        let m = ids_of(out).unwrap();
        assert_eq!(m.get("111").map(String::as_str), Some("aaa"));
        assert_eq!(m.get("222").map(String::as_str), Some("bbb"));
    }
}
