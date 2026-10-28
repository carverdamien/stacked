//! Where HEAD is, in the checkout stacked runs in, against a stack: on its
//! base with some of its patches as commits, a commit each, then maybe
//! commits that are not the stack's; or elsewhere. A commit is a patch when
//! its diff is, by `git patch-id`; its message may still differ, and is made
//! the same either way: the patch's taken from the commit (an edit by hand,
//! for your review), or the commit's from the patch (the commits from it on
//! made again, their files as they were). With the moz-phab feature, the
//! trailer moz-phab writes to name the revision is not lost either way: the
//! message written keeps the one of the message it is written over, and of
//! two messages that differ by that trailer alone, only the way that
//! carries a revision over to the side that names none is offered. And HEAD
//! moved to a place on the stack: detached there, the commits the checkout
//! has that are the stack's taken as they are, the rest applied; the branch
//! it was on as it was.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::{Db, Patch, Result, Stack, err};
use crate::git::{self, git, git_env, git_in};
use crate::mbox::Draft;
use crate::ops::{self, Edited};

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
    /// On the stack's base, its first patches as commits.
    Stack(OnStack),
    /// Not on the stack: its base is not below HEAD, or the commit after it
    /// is not the stack's.
    Elsewhere,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OnStack {
    /// The commits that are the stack's first patches, bottom first.
    pub commits: Vec<String>,
    /// Of those, the places (from 0) whose message is not the patch's: the
    /// commit's message, and how the two differ.
    pub messages: Vec<(usize, String, Differs)>,
    /// The commits above them to HEAD, which are not the stack's.
    pub above: Vec<String>,
}

/// How a commit's message differs from its patch's.
#[derive(Debug, Clone, PartialEq)]
pub enum Differs {
    /// In what it says: a sync either way changes the message it writes
    /// over.
    Text,
    /// With the moz-phab feature, by the trailer moz-phab writes to name the
    /// revision alone: the revision the patch names, then the one its commit
    /// names (`D123`), one of them none, or each another. A sync keeps the
    /// revision of the message it writes over, so only the way that carries
    /// a revision over to the side that names none changes a message.
    Revision(Option<String>, Option<String>),
}

impl Differs {
    /// Whether workspace → stacked changes the patch's message.
    pub fn to_stack(&self) -> bool {
        match self {
            Differs::Text => true,
            Differs::Revision(ours, theirs) => ours.is_none() && theirs.is_some(),
        }
    }

    /// Whether stacked → workspace changes the commit's message.
    pub fn to_checkout(&self) -> bool {
        match self {
            Differs::Text => true,
            Differs::Revision(ours, theirs) => ours.is_some() && theirs.is_none(),
        }
    }

    /// What patch `k` (from 0) and its commit name, when their messages
    /// differ by the revision alone: `patch 3 names D12 in the stack and
    /// D34 in your workspace's commit`. None when what they say differs.
    pub fn revisions(&self, k: usize) -> Option<String> {
        let Differs::Revision(ours, theirs) = self else { return None };
        let name = |r: &Option<String>| r.clone().unwrap_or_else(|| "no revision".into());
        Some(format!("patch {} names {} in the stack and {} in your workspace's commit", k + 1, name(ours), name(theirs)))
    }

    /// What a sync does of patch `k` (from 0), when its message and its
    /// commit's differ by the revision alone, its ways named `to_stack` and
    /// `to_checkout`: `patch 3 names no revision in the stack and D34 in
    /// your workspace's commit: workspace → stacked takes D34 into the
    /// stack`. None when what they say differs.
    pub fn said(&self, k: usize, to_stack: &str, to_checkout: &str) -> Option<String> {
        let names = self.revisions(k)?;
        let way = match self {
            Differs::Revision(None, Some(d)) => format!("{to_stack} takes {d} into the stack"),
            Differs::Revision(Some(d), None) => format!("{to_checkout} puts {d} on the commit"),
            _ => "neither way of syncing changes a message, so HEAD is not in sync with the stack".into(),
        };
        Some(format!("{names}: {way}"))
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

    /// The places (from 0) of the patches whose message differs from its
    /// commit's, a way of syncing changing one of them.
    pub fn differ(&self) -> Vec<usize> {
        match &self.at {
            At::Stack(on) => on.messages.iter().filter(|(_, _, d)| d.to_stack() || d.to_checkout()).map(|(k, _, _)| *k).collect(),
            At::Elsewhere => vec![],
        }
    }

    /// How the message of the patch at `k` (from 0) differs from its
    /// commit's, if it does.
    pub fn differs(&self, k: usize) -> Option<&Differs> {
        match &self.at {
            At::Stack(on) => on.messages.iter().find(|(at, _, _)| *at == k).map(|(_, _, d)| d),
            At::Elsewhere => None,
        }
    }

    /// Whether stacked → workspace would change a commit's message, then
    /// whether workspace → stacked would change a patch's: of the patch at
    /// `k` (from 0), else of any.
    pub fn ways(&self, k: Option<usize>) -> (bool, bool) {
        let At::Stack(on) = &self.at else { return (false, false) };
        on.messages
            .iter()
            .filter(|(at, _, _)| k.is_none_or(|k| *at == k))
            .fold((false, false), |(c, s), (_, _, d)| (c || d.to_checkout(), s || d.to_stack()))
    }

    /// Whether it is in sync with a stack of `n` patches: on its top, each
    /// patch a commit with the patch's message, its trailer naming the
    /// revision too, and no commit above.
    pub fn in_sync(&self, n: usize) -> bool {
        matches!(&self.at, At::Stack(on) if on.commits.len() == n && on.messages.is_empty() && on.above.is_empty())
    }

    /// Where it is on stack `name`, said: `at patch 2 of s, then 1 commit
    /// not in the stack`, `not on s`.
    pub fn said(&self, name: &str) -> String {
        let At::Stack(on) = &self.at else {
            return format!("not on {name}: the stack's base is not below it, or the commit after the base is not the stack's");
        };
        let mut out = format!("at {}", place(name, on.commits.len()));
        if !on.above.is_empty() {
            let n = on.above.len();
            out += &format!(", then {n} commit{} not in the stack", if n == 1 { "" } else { "s" });
        }
        out
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
        if let Some(d) = differs(&patches[k].message, &m) {
            on.messages.push((k, norm(&m), d));
        }
        on.commits.push(sha.clone());
    }
    if on.commits.is_empty() {
        return Ok(At::Elsewhere);
    }
    on.above = all[on.commits.len()..].to_vec();
    Ok(At::Stack(on))
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

/// How commit message `theirs` differs from its patch's, `ours`, each as
/// taken (`norm`), if it does: in what it says; or, with the moz-phab
/// feature, by the revision named alone, the two the same without the
/// trailer moz-phab writes (`Differs::Revision`).
fn differs(ours: &str, theirs: &str) -> Option<Differs> {
    if norm(ours) == norm(theirs) {
        return None;
    }
    #[cfg(feature = "moz-phab")]
    if norm(&without_trailer(ours)) == norm(&without_trailer(theirs)) {
        let (a, b) = (revision_named(ours), revision_named(theirs));
        if a != b {
            return Some(Differs::Revision(a, b));
        }
    }
    Some(Differs::Text)
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

/// Message `source` as a sync writes it over message `target`: keeping the
/// trailer moz-phab writes to name the revision (`progress::revision_trailers`)
/// that `target` has and `source` has not, in the place of `source`'s own
/// when it has another, else at the end of its paragraph of trailers, or
/// after an empty line when it ends in none, as moz-phab puts it. With the
/// revision kept so, as `D123`.
#[cfg(feature = "moz-phab")]
fn with_trailer(source: &str, target: &str) -> (String, Option<String>) {
    let theirs: Vec<&str> = target.split('\n').collect();
    let Some(kept) = crate::progress::revision_trailers(target).last().map(|&i| theirs[i].trim()) else {
        return (source.to_string(), None);
    };
    let ours = source.trim_end();
    let lines: Vec<&str> = ours.split('\n').collect();
    let own = crate::progress::revision_trailers(ours);
    if own.iter().any(|&i| lines[i].trim() == kept) {
        return (source.to_string(), None);
    }
    let written: Vec<&str> = match own.first() {
        Some(&first) => lines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i == first || !own.contains(i))
            .map(|(i, l)| if i == first { kept } else { *l })
            .collect(),
        None if ends_in_trailers(&lines) => lines.iter().copied().chain([kept]).collect(),
        None => lines.iter().copied().chain(["", kept]).collect(),
    };
    (written.join("\n"), Some(revision_of(kept)))
}

/// Without the moz-phab feature, a sync writes message `source` as it is.
#[cfg(not(feature = "moz-phab"))]
fn with_trailer(source: &str, _: &str) -> (String, Option<String>) {
    (source.to_string(), None)
}

/// Whether the lines of a message, `lines`, end in a paragraph of trailers
/// after an empty line: each of its lines a key, `: ` and a value, the key
/// a letter, then letters, digits, hyphens and spaces, as in `Reviewed-by:
/// ann`.
#[cfg(feature = "moz-phab")]
fn ends_in_trailers(lines: &[&str]) -> bool {
    let trailer = |l: &&str| {
        l.split_once(": ").is_some_and(|(key, _)| {
            key.starts_with(|c: char| c.is_ascii_alphabetic()) && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
        })
    };
    let start = lines.iter().rposition(|l| l.trim().is_empty()).map(|e| e + 1);
    start.is_some_and(|s| s < lines.len() && lines[s..].iter().all(trailer))
}

/// What a sync says of the revisions it kept, `kept` (`D123`), of the
/// messages it wrote over, those being `one`'s, or `more`'s for more than
/// one: `kept the Differential Revision D123 your workspace's commit had`.
/// None when it kept none.
#[cfg(feature = "moz-phab")]
fn kept_said(kept: &[String], one: &str, more: &str) -> Option<String> {
    match kept {
        [] => None,
        [d] => Some(format!("kept the Differential Revision {d} {one} had")),
        [rest @ .., last] => Some(format!("kept the Differential Revisions {} and {last} {more} had", rest.join(", "))),
    }
}

/// Without the moz-phab feature, a sync keeps no revision, and says none.
#[cfg(not(feature = "moz-phab"))]
fn kept_said(_: &[String], _: &str, _: &str) -> Option<String> {
    None
}

/// The checkout at `here`, a checkout of stack `s`'s repository, made sure
/// of before HEAD is moved or its commits made again: nothing of yours
/// uncommitted to carry along, and no agent working there.
fn ready(db: &Db, s: &Stack, here: &Path, nothing: &str) -> Result<(PathBuf, String)> {
    let (repo, base) = ops::base_of(s)?;
    let dir = git::checkout_of(&repo, here)?;
    if git::unfinished_tracked(&dir)? {
        return err(format!("{} has uncommitted changes: commit or stash them first; {nothing}", dir.display()));
    }
    if agent_in(db, &dir) {
        return err(format!("the agent is working in {}, your checkout: {nothing} until its ask lands", dir.display()));
    }
    Ok((dir, base))
}

/// Whether an agent works in the checkout at `dir`, moved to its stack for
/// a change asked there (`agent.worktree = "pwd"`), its ask still running.
pub fn agent_in(db: &Db, dir: &Path) -> bool {
    db.live_runs()
        .into_iter()
        .filter(|r| crate::ask::running(r))
        .filter_map(crate::ask::info)
        .any(|i| i.back.is_some() && i.worktree.as_deref().map(Path::new) == Some(dir))
}

/// HEAD of the checkout at `here` moved to stack `name` with its first `n`
/// patches applied: detached there, the commits the checkout has that are
/// those patches, messages too (a message that differs by the revision it
/// names alone as well), taken as they are, the rest applied as commits
/// (unsigned); the branch it was on as it was. What came of it, said.
pub fn move_to(path: &Path, name: &str, here: &Path, n: usize) -> Result<String> {
    let db = Db::load(path)?;
    let s = db.stack(name)?.clone();
    if n > s.patches.len() {
        return err(format!("{name} has {} patches", s.patches.len()));
    }
    let (dir, base) = ready(&db, &s, here, "HEAD did not move")?;
    let patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    let head = locate(&dir, &base, &patches)?;
    // What it has of the stack, as the stack has it, up to the place.
    let kept: Vec<String> = match &head.at {
        At::Stack(on) => {
            let same = on.messages.iter().find(|(_, _, d)| *d == Differs::Text).map_or(on.commits.len(), |(k, _, _)| *k);
            on.commits[..same.min(n)].to_vec()
        }
        At::Elsewhere => vec![],
    };
    let start = match kept.last() {
        Some(c) => c.clone(),
        None => git::rev_parse(&dir, &base)?,
    };
    let there = place(name, n);
    if kept.len() == n && start == head.commit {
        return Ok(format!("HEAD is at {there} already, on {}", head.on()));
    }
    let back = git::head_of(&dir)?;
    git(&dir, &["switch", "--quiet", "--detach", &start])?;
    if n > kept.len()
        && let Err(e) = git::am(&dir, &crate::mbox::format_mbox(&patches[kept.len()..n]))
    {
        let _ = git::back_to(&dir, &back);
        return err(format!("{}; HEAD is back where it was", e.0));
    }
    Ok(match &head.branch {
        Some(b) => format!("HEAD is at {there}, detached: {b} is as it was"),
        None => format!("HEAD is at {there}, detached"),
    })
}

/// The places (from 0) of `head`'s commits whose message differs that
/// `places` names, all of them if it names none, of those a sync changes
/// a message of, as `way` says (`Differs::to_stack`): each place, and the
/// commit's message.
fn picked(head: &Head, places: &[usize], way: fn(&Differs) -> bool) -> Vec<(usize, String)> {
    match &head.at {
        At::Stack(on) => on
            .messages
            .iter()
            .filter(|(k, _, d)| (places.is_empty() || places.contains(k)) && way(d))
            .map(|(k, m, _)| (*k, m.clone()))
            .collect(),
        At::Elsewhere => vec![],
    }
}

/// What a sync `way` (`stacked → workspace`) that changes no `whose`'s
/// message (`commit`) says, of `head`'s patches at `places` (all of them if
/// none): why, when their messages differ from their commits' by the
/// revision alone; else that no message differs from `other`'s (`the
/// stack's`).
fn unchanged(head: &Head, places: &[usize], way: &str, whose: &str, other: &str) -> String {
    let apart: Vec<String> = match &head.at {
        At::Stack(on) => on
            .messages
            .iter()
            .filter(|(k, _, _)| places.is_empty() || places.contains(k))
            .filter_map(|(k, _, d)| d.revisions(*k))
            .collect(),
        At::Elsewhere => vec![],
    };
    match apart.is_empty() {
        true => format!("no message differs from {other}: nothing changed"),
        false => format!("{way} would change no {whose}'s message, as {}: nothing changed", apart.join(", and ")),
    }
}

/// The messages of the commits of the checkout at `here` that are stack
/// `name`'s patches at `places` (from 0; all of those whose message differs
/// if none), taken as those patches' own, each keeping the patch's trailer
/// naming its revision (`with_trailer`): an edit by hand, landing for your
/// review. Of those patches, only the ones whose message the commit's would
/// change.
pub fn messages_to_stack(path: &Path, name: &str, here: &Path, places: &[usize]) -> Result<Edited> {
    let db = Db::load(path)?;
    let Some(head) = find(&db, name, here)? else {
        return err(format!("{} is not a checkout of {name}'s repository", here.display()));
    };
    let taken = picked(&head, places, Differs::to_stack);
    if taken.is_empty() {
        return Ok(Edited::Unchanged(unchanged(&head, places, "workspace → stacked", "patch", "your checkout's")));
    }
    let was = db.stack(name)?.patches.clone();
    drop(db);
    Db::edit(path, |db| {
        ops::may_edit(db, name, None)?;
        if db.stack(name)?.patches != was {
            return err(format!("{name} changed meanwhile; nothing changed"));
        }
        let mut patches = was.clone();
        let mut done = Vec::new();
        let mut kept = Vec::new();
        for (k, message) in &taken {
            let old = db.patch(&patches[*k]).clone();
            let (message, revision) = with_trailer(message, &old.message);
            kept.extend(revision);
            let d = Draft { message, ..Draft::from(&old) };
            if let Some((p, op)) = ops::revised(db, &patches, *k, d)? {
                patches = p;
                done.push(op);
            }
        }
        let of = patches_said(&taken.iter().map(|(k, _)| *k).collect::<Vec<_>>());
        let mut said = format!("took the message of {of} from your checkout");
        if let Some(k) = kept_said(&kept, "the patch", "the patches") {
            said += &format!(" and {k}");
        }
        ops::land(db, name, &was, patches, &done.join("; "), &of, &said, None)
    })
}

/// The messages of the commits of the checkout at `here` that are stack
/// `name`'s patches at `places` (from 0; all of those whose message differs
/// if none) made those patches' own, each keeping the commit's trailer
/// naming its revision (`with_trailer`): those commits, and every one above
/// them to HEAD, made again on the same files (`git commit-tree`, signed as
/// git says), the branch HEAD is on, else HEAD, moved to them. Of those
/// commits, only the ones whose message the patch's would change: with
/// none to change, no commit is made again. What came of it, said.
pub fn messages_to_checkout(path: &Path, name: &str, here: &Path, places: &[usize]) -> Result<String> {
    let db = Db::load(path)?;
    let s = db.stack(name)?.clone();
    let (dir, base) = ready(&db, &s, here, "nothing changed")?;
    let patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    let head = locate(&dir, &base, &patches)?;
    // The messages written, each with the revision it keeps, if any: of
    // those that change the commit's.
    let mut put: Vec<(usize, String, Option<String>)> = Vec::new();
    for (k, theirs) in picked(&head, places, Differs::to_checkout) {
        let (ours, revision) = with_trailer(&patches[k].message, &theirs);
        if norm(&ours) != theirs {
            put.push((k, ours, revision));
        }
    }
    let (At::Stack(on), Some(first)) = (&head.at, put.first().map(|(k, _, _)| *k)) else {
        return Ok(unchanged(&head, places, "stacked → workspace", "commit", "the stack's"));
    };
    // From the first to HEAD, each made again on the one before.
    let chain: Vec<&String> = on.commits[first..].iter().chain(&on.above).collect();
    let mut parent = match first {
        0 => git::rev_parse(&dir, &base)?,
        k => on.commits[k - 1].clone(),
    };
    for (i, c) in chain.iter().enumerate() {
        let shown = git(&dir, &["log", "-1", "--format=%P%x00%T%x00%an%x00%ae%x00%ad%x00%B", "--date=raw", c])?;
        let f: Vec<&str> = shown.splitn(6, '\0').collect();
        let [parents, tree, an, ae, ad, message] = f.as_slice() else {
            return err(format!("git log said nothing of {c}"));
        };
        if parents.split_whitespace().count() != 1 {
            return err(format!("{} is a merge: its commits are not made again; nothing changed", &c[..12.min(c.len())]));
        }
        let message = match put.iter().find(|(k, _, _)| *k == first + i) {
            Some((_, ours, _)) => format!("{}\n", ours.trim_end()),
            None => message.to_string(),
        };
        let env = [
            ("GIT_AUTHOR_NAME", Path::new(an)),
            ("GIT_AUTHOR_EMAIL", Path::new(ae)),
            ("GIT_AUTHOR_DATE", Path::new(ad)),
        ];
        parent = git_env(&dir, &["commit-tree", tree, "-p", &parent, "-F", "-"], Some(&message), &env)?.trim().to_string();
    }
    let synced: Vec<usize> = put.iter().map(|(k, _, _)| *k).collect();
    let why = format!("stacked: the messages of {} from {name}", patches_said(&synced));
    match &head.branch {
        Some(b) => git(&dir, &["update-ref", "-m", &why, &format!("refs/heads/{b}"), &parent, &head.commit])?,
        None => git(&dir, &["update-ref", "--no-deref", "-m", &why, "HEAD", &parent, &head.commit])?,
    };
    let n = chain.len();
    let mut said = format!(
        "{} has the stack's message of {}: {n} commit{} made again, the files as they were",
        head.on(),
        patches_said(&synced),
        if n == 1 { "" } else { "s" }
    );
    let kept: Vec<String> = put.into_iter().filter_map(|(_, _, r)| r).collect();
    if let Some(k) = kept_said(&kept, "your workspace's commit", "your workspace's commits") {
        said += &format!("; {k}");
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

    #[cfg(feature = "moz-phab")]
    #[test]
    fn messages_that_differ_by_the_revision_alone_are_synced_one_way_or_neither() {
        let bare = "Bug 1 - two\n\nBecause.";
        let with = |d: &str| format!("{bare}\n\n{}", TRAILER.replace("D123", d));
        assert_eq!(differs(&with("D123"), &with("D123")), None);
        assert_eq!(differs(bare, &format!("{bare}\n")), None);
        // The commit's revision alone: workspace → stacked. The patch's
        // alone: stacked → workspace. Each its own: neither.
        let commit = differs(bare, &with("D123")).unwrap();
        assert_eq!(commit, Differs::Revision(None, Some("D123".into())));
        assert_eq!((commit.to_stack(), commit.to_checkout()), (true, false));
        let patch = differs(&with("D123"), bare).unwrap();
        assert_eq!((patch.to_stack(), patch.to_checkout()), (false, true));
        let apart = differs(&with("D12"), &with("D34")).unwrap();
        assert_eq!((apart.to_stack(), apart.to_checkout()), (false, false));
        assert_eq!(apart.revisions(2).as_deref(), Some("patch 3 names D12 in the stack and D34 in your workspace's commit"));
        let signed = format!("{bare}\n\nReviewed-by: ann");
        assert_eq!(differs(&signed, &format!("{signed}\n{TRAILER}")), Some(Differs::Revision(None, Some("D123".into()))));
        // What they say otherwise: either way, whatever revision they name.
        assert_eq!(differs(&with("D12"), "Bug 1 - two\n\nWhy."), Some(Differs::Text));
        // The same line in the middle of the message is not the trailer.
        assert_eq!(differs(&format!("Bug 1 - two\n\n{TRAILER}\n\nBecause."), bare), Some(Differs::Text));
        // What each way does, said, the ways named as the editor names them.
        let (s, c) = ("workspace → stacked", "stacked → workspace");
        let said = "patch 3 names no revision in the stack and D123 in your workspace's commit: workspace → stacked takes D123 into the stack";
        assert_eq!(commit.said(2, s, c).as_deref(), Some(said));
        let said = "patch 3 names D123 in the stack and no revision in your workspace's commit: stacked → workspace puts D123 on the commit";
        assert_eq!(patch.said(2, s, c).as_deref(), Some(said));
        let said = "patch 3 names D12 in the stack and D34 in your workspace's commit: neither way of syncing changes a message, so HEAD is not in sync with the stack";
        assert_eq!(apart.said(2, s, c).as_deref(), Some(said));
        assert_eq!(Differs::Text.said(2, s, c), None);
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_sync_keeps_the_trailer_of_the_message_it_writes_over() {
        let target = format!("Bug 1 - old\n\n{TRAILER}");
        let kept = |m: String| (m, Some("D123".to_string()));
        // After an empty line when the message ends in no trailers, at the
        // end of its trailers when it does.
        assert_eq!(with_trailer("Bug 1 - new\n\nBecause.\n", &target), kept(format!("Bug 1 - new\n\nBecause.\n\n{TRAILER}")));
        assert_eq!(with_trailer("Bug 1 - new", &target), kept(format!("Bug 1 - new\n\n{TRAILER}")));
        let signed = "Bug 1 - new\n\nBecause.\n\nReviewed-by: ann";
        assert_eq!(with_trailer(signed, &target), kept(format!("{signed}\n{TRAILER}")));
        // Another revision of its own: the one written over in its place.
        let other = TRAILER.replace("D123", "D7");
        assert_eq!(with_trailer(&format!("Bug 1 - new\n\n{other}\nReviewed-by: ann"), &target), kept(format!("Bug 1 - new\n\n{TRAILER}\nReviewed-by: ann")));
        // The same of its own, or none written over: as it is.
        let same = format!("Bug 1 - new\n\n{TRAILER}");
        assert_eq!(with_trailer(&same, &target), (same.clone(), None));
        assert_eq!(with_trailer(&same, "Bug 1 - old"), (same, None));
        assert_eq!(with_trailer("Bug 1 - new", "Bug 1 - old"), ("Bug 1 - new".to_string(), None));
        // What a sync says of them.
        assert_eq!(kept_said(&[], "the patch", "the patches"), None);
        let one = kept_said(&["D1".into()], "your workspace's commit", "your workspace's commits");
        assert_eq!(one.as_deref(), Some("kept the Differential Revision D1 your workspace's commit had"));
        let three = kept_said(&["D1".into(), "D2".into(), "D3".into()], "the patch", "the patches");
        assert_eq!(three.as_deref(), Some("kept the Differential Revisions D1, D2 and D3 the patches had"));
    }

    #[test]
    fn head_is_out_of_sync_while_revisions_differ_and_offers_the_ways_that_change_a_message() {
        let on = OnStack {
            commits: vec!["a".into(), "b".into(), "c".into()],
            messages: vec![
                (1, "Bug 2".into(), Differs::Revision(None, Some("D2".into()))),
                (2, "Bug 3".into(), Differs::Revision(Some("D3".into()), Some("D33".into()))),
            ],
            ..Default::default()
        };
        let h = Head {
            dir: PathBuf::new(),
            branch: None,
            commit: "c".into(),
            at: At::Stack(on),
        };
        assert_eq!(h.differ(), [1], "patch 3's, each naming its own revision, is not to sync");
        assert_eq!((h.ways(Some(1)), h.ways(Some(2)), h.ways(None)), ((false, true), (false, false), (false, true)));
        assert!(!h.in_sync(3), "following the stack, HEAD would lose the revisions only the commits name");
    }

    #[cfg(not(feature = "moz-phab"))]
    #[test]
    fn without_moz_phab_the_trailer_is_compared_and_written_as_any_line() {
        let with = format!("Bug 1 - two\n\nBecause.\n\n{TRAILER}");
        assert_eq!(differs("Bug 1 - two\n\nBecause.", &with), Some(Differs::Text));
        assert_eq!(with_trailer("Bug 1 - new", &with), ("Bug 1 - new".to_string(), None));
        assert_eq!(kept_said(&["D1".into()], "the patch", "the patches"), None);
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
