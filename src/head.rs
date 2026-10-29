//! Where HEAD is, in the checkout stacked runs in, against a stack: on its
//! base with some of its patches as commits, a commit each, then maybe
//! commits that are not the stack's; or elsewhere. A commit is a patch when
//! its diff is, by `git patch-id`; its message may still differ. The sync of
//! the commit messages makes them the same, as you pick a patch at a time:
//! the message kept, the stack's or the commit's, goes to each side that
//! lacks it, all at once or not at all (`sync`). With the moz-phab feature,
//! the trailer moz-phab writes to name the revision is not lost: of two
//! messages that differ by that trailer alone, one naming none, the one
//! that names it is kept, with nothing to pick; a message picked that names
//! none takes the other's. And HEAD moved to a place on the stack: detached
//! there, the commits the checkout has that are the stack's taken as they
//! are, the rest applied; the branch it was on as it was.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::{Db, Patch, Result, Stack, err};
use crate::git::{self, git, git_env, git_in};
use crate::mbox::Draft;
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
    /// Of those, the places (from 0) whose message is not the patch's, as
    /// compared (`compared`): the commit's message, and how the two differ.
    pub messages: Vec<(usize, String, Differs)>,
    /// The commits above them to HEAD, which are not the stack's.
    pub above: Vec<String>,
}

/// How a commit's message differs from its patch's.
#[derive(Debug, Clone, PartialEq)]
pub enum Differs {
    /// In what it says: the sync keeps the one picked.
    Text,
    /// With the moz-phab feature, by the trailer moz-phab writes to name the
    /// revision alone: the revision the patch names, then the one its commit
    /// names (`D123`), one of them none, or each another. Of one naming
    /// none, the sync keeps the one that names it, with nothing to pick; of
    /// two naming each another, the one picked.
    Revision(Option<String>, Option<String>),
}

impl Differs {
    /// Whether the sync keeps the message picked: not when the two differ
    /// by the revision alone, one naming none (`taken`).
    pub fn needs_pick(&self) -> bool {
        self.taken().is_none()
    }

    /// Of two messages that differ by the revision they name alone, one
    /// naming none: the side whose message names it, which the sync keeps,
    /// and the revision (`D123`).
    pub fn taken(&self) -> Option<(Keep, &str)> {
        match self {
            Differs::Revision(None, Some(d)) => Some((Keep::Workspace, d)),
            Differs::Revision(Some(d), None) => Some((Keep::Stack, d)),
            _ => None,
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
}

/// Which of a patch's two messages the sync of the commit messages keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keep {
    /// The stack's: the patch's own.
    Stack,
    /// Its commit's, in your workspace.
    Workspace,
}

/// What a patch and its commit both say once synced: the message kept;
/// with the moz-phab feature, the revision it takes from the other message,
/// which names one where it names none (`D123`); and the revision the other
/// message names, which it drops, naming another of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub message: String,
    pub carried: Option<String>,
    pub dropped: Option<String>,
}

/// What the sync of the commit messages does, as picked: what each patch
/// whose message differs and its commit both say then, by its place (from
/// 0); of those, the patches whose message changes, a new version each,
/// and the commits whose message changes, each with what it says then;
/// and how many commits are made again, from the first of those up to
/// HEAD.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub kept: Vec<(usize, Kept)>,
    pub patches: Vec<(usize, String)>,
    pub commits: Vec<(usize, String)>,
    pub remade: usize,
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
    /// commit's.
    pub fn differ(&self) -> Vec<usize> {
        match &self.at {
            At::Stack(on) => on.messages.iter().map(|(k, _, _)| *k).collect(),
            At::Elsewhere => vec![],
        }
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

/// How commit message `theirs` differs from its patch's, `ours`, each as
/// compared (`compared`), if it does: in what it says; or, with the
/// moz-phab feature, by the revision named alone, the two the same without
/// the trailer moz-phab writes (`Differs::Revision`).
fn differs(ours: &str, theirs: &str) -> Option<Differs> {
    if compared(ours) == compared(theirs) {
        return None;
    }
    #[cfg(feature = "moz-phab")]
    if compared(&without_trailer(ours)) == compared(&without_trailer(theirs)) {
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

/// The revision message `m` names in the trailer moz-phab writes, `D123`,
/// if it has that trailer.
#[cfg(feature = "moz-phab")]
pub fn revision_in(m: &str) -> Option<String> {
    revision_named(m)
}

/// Without the moz-phab feature, no message names a revision: the trailer
/// is a line as any other.
#[cfg(not(feature = "moz-phab"))]
pub fn revision_in(_: &str) -> Option<String> {
    None
}

/// The revision trailer `line` names: `D123`, after its link's last slash.
#[cfg(feature = "moz-phab")]
fn revision_of(line: &str) -> String {
    let line = line.trim();
    line.rsplit('/').next().unwrap_or(line).to_string()
}

/// Message `source` with the trailer moz-phab writes to name the revision
/// (`progress::revision_trailers`) that message `target` has and `source`
/// has not: in the place of `source`'s own when it has another, else at
/// the end of its paragraph of trailers, or after an empty line when it
/// ends in none, as moz-phab puts it. With the revision taken so, as
/// `D123`.
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

/// Without the moz-phab feature, message `source` as it is.
#[cfg(not(feature = "moz-phab"))]
fn with_trailer(source: &str, _: &str) -> (String, Option<String>) {
    (source.to_string(), None)
}

/// Whether the lines of a message, `lines`, end in a paragraph of trailers
/// after an empty line: each of its lines a trailer (`trailer`). A submit
/// to Phabricator puts a new revision's trailer as the sync does.
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

/// What patch message `ours` and its commit's, `theirs`, differing as `d`
/// says, both say once synced, `keep` picking one: the one picked, as it
/// is; but with the moz-phab feature, the other's trailer naming its
/// revision added when the one picked names none and the other names one,
/// where `with_trailer` puts it; and of two that differ by that trailer
/// alone, one naming none, the one that names it, whatever is picked. Of
/// two naming each another, the one picked keeps its own, the other's
/// dropped. None when one must be picked and none is.
pub fn kept(ours: &str, theirs: &str, d: &Differs, keep: Option<Keep>) -> Option<Kept> {
    let side = d.taken().map(|(k, _)| k).or(keep)?;
    let (picked, other) = match side {
        Keep::Stack => (ours, theirs),
        Keep::Workspace => (theirs, ours),
    };
    let (own, its) = (revision_in(picked), revision_in(other));
    if own.is_none() && its.is_some() {
        let (message, carried) = with_trailer(picked, other);
        return Some(Kept { message, carried, dropped: None });
    }
    let dropped = its.filter(|r| own.as_ref().is_some_and(|o| o != r));
    Some(Kept {
        message: picked.to_string(),
        carried: None,
        dropped,
    })
}

/// What the sync of the commit messages of `on` does, the stack's own
/// messages being `messages` (by place, from 0), the message of each patch
/// kept as `picks` picks (by place, from 0; see `kept`): its plan; else the
/// places of the patches still to pick.
pub fn plan(on: &OnStack, messages: &[&str], picks: &[(usize, Keep)]) -> std::result::Result<Plan, Vec<usize>> {
    let mut out = Plan::default();
    let mut left = Vec::new();
    for (k, theirs, d) in &on.messages {
        let ours = messages.get(*k).copied().unwrap_or_default();
        let pick = picks.iter().find(|(at, _)| at == k).map(|(_, p)| *p);
        let Some(kept) = kept(ours, theirs, d, pick) else {
            left.push(*k);
            continue;
        };
        // Each side whose message is another, as compared, takes it as it
        // is.
        let said = compared(&kept.message);
        if said != compared(ours) {
            out.patches.push((*k, kept.message.clone()));
        }
        if said != compared(theirs) {
            out.commits.push((*k, kept.message.clone()));
        }
        out.kept.push((*k, kept));
    }
    if !left.is_empty() {
        return Err(left);
    }
    out.remade = out.commits.first().map_or(0, |(k, _)| on.commits.len() - k + on.above.len());
    Ok(out)
}

/// The checkout at `here`, a checkout of stack `s`'s repository, made sure
/// of before HEAD is moved or its commits made again: nothing of yours
/// uncommitted to carry along. No agent works there: every ask works in a
/// worktree of its own.
fn ready(s: &Stack, here: &Path, nothing: &str) -> Result<(PathBuf, String)> {
    let (repo, base) = ops::base_of(s)?;
    let dir = git::checkout_of(&repo, here)?;
    if git::unfinished_tracked(&dir)? {
        return err(format!("{} has uncommitted changes: commit or stash them first; {nothing}", dir.display()));
    }
    Ok((dir, base))
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
    let (dir, base) = ready(&s, here, "HEAD did not move")?;
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

/// The commits of `on` from the first `put` names (its place, from 0) up
/// to HEAD, made again in the checkout at `dir`, each on the one before
/// (the stack's base, `base`, under the first patch), its files, author
/// and date as they were, its message the one `put` gives it, if it gives
/// one (`git commit-tree`, signed as git says). Nothing points at them
/// yet: the new top.
fn remake(dir: &Path, base: &str, on: &OnStack, put: &[(usize, String)]) -> Result<Option<String>> {
    let Some(first) = put.first().map(|(k, _)| *k) else { return Ok(None) };
    let chain: Vec<&String> = on.commits[first..].iter().chain(&on.above).collect();
    let mut parent = match first {
        0 => git::rev_parse(dir, base)?,
        k => on.commits[k - 1].clone(),
    };
    for (i, c) in chain.iter().enumerate() {
        let shown = git(dir, &["log", "-1", "--format=%P%x00%T%x00%an%x00%ae%x00%ad%x00%B", "--date=raw", c])?;
        let f: Vec<&str> = shown.splitn(6, '\0').collect();
        let [parents, tree, an, ae, ad, message] = f.as_slice() else {
            return err(format!("git log said nothing of {c}"));
        };
        if parents.split_whitespace().count() != 1 {
            return err(format!("{} is a merge: its commits are not made again; nothing changed", &c[..12.min(c.len())]));
        }
        let message = match put.iter().find(|(k, _)| *k == first + i) {
            Some((_, ours)) => format!("{}\n", ours.trim_end()),
            None => message.to_string(),
        };
        let env = [
            ("GIT_AUTHOR_NAME", Path::new(an)),
            ("GIT_AUTHOR_EMAIL", Path::new(ae)),
            ("GIT_AUTHOR_DATE", Path::new(ad)),
        ];
        parent = git_env(dir, &["commit-tree", tree, "-p", &parent, "-F", "-"], Some(&message), &env)?.trim().to_string();
    }
    Ok(Some(parent))
}

/// The branch HEAD of `head` is on, else HEAD itself, moved from commit
/// `from` to commit `to`, only if it is at `from` still; `why`, as its
/// reflog says it.
fn move_head_ref(head: &Head, from: &str, to: &str, why: &str) -> Result<()> {
    match &head.branch {
        Some(b) => git(&head.dir, &["update-ref", "-m", why, &format!("refs/heads/{b}"), to, from])?,
        None => git(&head.dir, &["update-ref", "--no-deref", "-m", why, "HEAD", to, from])?,
    };
    Ok(())
}

/// The commit messages of stack `name`'s patches and of their commits in
/// the checkout at `here` made the same, as the sync of the commit
/// messages picks them, all at once or not at all (`plan`): each patch the
/// message kept where its own differs, a new version at once, the history
/// saying it is the sync's, the changes of the stack waiting for review
/// made from the stack so (`held::carry`); each commit the message kept
/// where its own differs, the commits from the first of those up to HEAD
/// made again on the same files (`remake`), the branch HEAD is on, else
/// HEAD, moved to them. Not if the stack, at `rev` when HEAD was looked at,
/// or HEAD, `seen` then, changed since; nor if a commit is to be made again
/// and the checkout is not ready (`ready`), nor if a patch is to change
/// while the agent changes the stack. What came of it, said.
pub fn sync(path: &Path, name: &str, here: &Path, rev: Option<usize>, seen: &Head, picks: &[(usize, Keep)]) -> Result<String> {
    let nothing = "nothing changed";
    let (mut db, mut lock) = Db::lock(path)?;
    let s = db.stack(name)?.clone();
    let was = db.head(name).map(|(r, _)| r);
    let (repo, base) = ops::base_of(&s)?;
    let dir = git::checkout_of(&repo, here)?;
    let patches: Vec<Patch> = s.patches.iter().map(|id| db.patch(id).clone()).collect();
    let head = locate(&dir, &base, &patches.iter().collect::<Vec<_>>())?;
    if was != rev || head != *seen {
        return err(format!("the stack or HEAD changed since the sync listed them: {nothing}, and the sync lists them as they are now"));
    }
    let At::Stack(on) = &head.at else {
        return err(format!("HEAD is not on {name}: {nothing}"));
    };
    let messages: Vec<&str> = patches.iter().map(|p| p.message.as_str()).collect();
    let plan = match plan(on, &messages, picks) {
        Ok(p) => p,
        Err(left) => return err(format!("{} still to pick: {nothing}", patches_said(&left))),
    };
    if !plan.commits.is_empty() {
        ready(&s, here, nothing)?;
    }
    if !plan.patches.is_empty()
        && let Some(what) = crate::ask::changing(&db, name)
    {
        return err(format!("{what}, so the stack takes no new version of a patch until that work lands: {nothing}"));
    }
    // The commits made first, which nothing points at yet.
    let top = remake(&dir, &base, on, &plan.commits)?;
    // The new versions, then the changes waiting for review made from them.
    let mut carried = Vec::new();
    if !plan.patches.is_empty() {
        let mut ids = s.patches.clone();
        let (mut done, mut new) = (Vec::new(), HashMap::new());
        for (k, message) in &plan.patches {
            let old = &patches[*k];
            let id = db.add_patch(ops::revise(old, Draft { message: message.clone(), ..Draft::from(old) }))?;
            done.push(format!("edit {} as {}", &old.id[..8], &id[..8]));
            new.insert(old.id.clone(), id.clone());
            ids[*k] = id;
        }
        let of = patches_said(&plan.patches.iter().map(|(k, _)| *k).collect::<Vec<_>>());
        let to = db.set_stack(name, ids, &format!("sync the commit messages of {of} with your workspace: {}", done.join("; ")))?;
        carried = crate::held::carry(&mut db, name, was, to, &new)?;
    }
    // HEAD moved, then the database written: HEAD back if that fails.
    let synced: Vec<usize> = plan.kept.iter().map(|(k, _)| *k).collect();
    if let Some(top) = &top {
        move_head_ref(&head, &head.commit, top, &format!("stacked: the commit messages of {} synced with {name}", patches_said(&synced)))?;
    }
    if let Err(e) = db.save(&mut lock) {
        if let Some(top) = &top {
            let _ = move_head_ref(&head, top, &head.commit, "stacked: the sync of the commit messages undone");
        }
        return Err(e);
    }
    let mut said = Vec::new();
    if !plan.patches.is_empty() {
        let n = plan.patches.len();
        let of = patches_said(&plan.patches.iter().map(|(k, _)| *k).collect::<Vec<_>>());
        said.push(format!("{of} {} a new version in the stack", if n == 1 { "has" } else { "have" }));
    }
    if top.is_some() {
        let n = plan.remade;
        let on = head.branch.as_deref().unwrap_or("HEAD");
        said.push(format!("{on} has {n} commit{} made again, the files as they were", if n == 1 { "" } else { "s" }));
    }
    let mut out = format!("synced the commit messages of {}: {}", patches_said(&synced), said.join(", and "));
    for c in carried {
        out += &format!("; {c}, which waits for your review, goes on from the new versions");
    }
    Ok(out)
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
    fn empty_lines_in_a_row_are_one_as_git_am_leaves_them_and_a_message_kept_is_written_as_it_is() {
        // Two empty lines under the title, then one of spaces: one, as git
        // am leaves them; as taken, they stay.
        let run = "Bug 1 - two\n\n\n  \nBecause.";
        assert_eq!((compared(run), norm(run)), ("Bug 1 - two\n\nBecause.".to_string(), "Bug 1 - two\n\n\n\nBecause.".to_string()));
        assert_eq!(differs(run, "Bug 1 - two\n\nBecause.\n"), None);
        // A real difference beside them is one still, on either side.
        assert_eq!(differs(run, "Bug 1 - two\n\nWhy."), Some(Differs::Text));
        assert_eq!(differs("Bug 1 - two\n\nBecause.", "Bug 1 - two\n\n\nWhy."), Some(Differs::Text));
        #[cfg(feature = "moz-phab")]
        assert_eq!(differs(&format!("{run}\n\n{TRAILER}"), "Bug 1 - two\n\nBecause."), Some(Differs::Revision(Some("D123".into()), None)));
        // Kept, a message is written as it is, its empty lines too, on the
        // side whose message is another.
        let on = OnStack {
            commits: vec!["a".into()],
            messages: vec![(0, "Bug 1 - two\n\nWhy.".into(), Differs::Text)],
            above: vec![],
        };
        let p = plan(&on, &[run], &[(0, Keep::Stack)]).unwrap();
        assert_eq!((p.patches, p.commits), (vec![], vec![(0, run.to_string())]));
        let theirs = "Bug 1 - two\n\n\nWhy.";
        let on = OnStack {
            messages: vec![(0, theirs.into(), Differs::Text)],
            ..on
        };
        let p = plan(&on, &["Bug 1 - two\n\nBecause."], &[(0, Keep::Workspace)]).unwrap();
        assert_eq!((p.patches, p.commits), (vec![(0, theirs.to_string())], vec![]));
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn messages_that_differ_by_the_revision_alone_have_nothing_to_pick_unless_each_names_another() {
        let bare = "Bug 1 - two\n\nBecause.";
        let with = |d: &str| format!("{bare}\n\n{}", TRAILER.replace("D123", d));
        assert_eq!(differs(&with("D123"), &with("D123")), None);
        assert_eq!(differs(bare, &format!("{bare}\n")), None);
        // The commit's revision alone: the commit's kept. The patch's
        // alone: the patch's. Each its own: one to pick.
        let commit = differs(bare, &with("D123")).unwrap();
        assert_eq!(commit, Differs::Revision(None, Some("D123".into())));
        assert_eq!((commit.needs_pick(), commit.taken()), (false, Some((Keep::Workspace, "D123"))));
        let patch = differs(&with("D123"), bare).unwrap();
        assert_eq!((patch.needs_pick(), patch.taken()), (false, Some((Keep::Stack, "D123"))));
        let apart = differs(&with("D12"), &with("D34")).unwrap();
        assert_eq!((apart.needs_pick(), apart.taken()), (true, None));
        assert_eq!(apart.revisions(2).as_deref(), Some("patch 3 names D12 in the stack and D34 in your workspace's commit"));
        assert_eq!(commit.revisions(2).as_deref(), Some("patch 3 names no revision in the stack and D123 in your workspace's commit"));
        let signed = format!("{bare}\n\nReviewed-by: ann");
        assert_eq!(differs(&signed, &format!("{signed}\n{TRAILER}")), Some(Differs::Revision(None, Some("D123".into()))));
        // What they say otherwise: one to pick, whatever revision they name.
        assert_eq!(differs(&with("D12"), "Bug 1 - two\n\nWhy."), Some(Differs::Text));
        assert!(Differs::Text.needs_pick() && Differs::Text.revisions(2).is_none());
        // The same line in the middle of the message is not the trailer.
        assert_eq!(differs(&format!("Bug 1 - two\n\n{TRAILER}\n\nBecause."), bare), Some(Differs::Text));
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_trailer_is_added_where_moz_phab_puts_it() {
        let target = format!("Bug 1 - old\n\n{TRAILER}");
        let kept = |m: String| (m, Some("D123".to_string()));
        // After an empty line when the message ends in no trailers, at the
        // end of its trailers when it does.
        assert_eq!(with_trailer("Bug 1 - new\n\nBecause.\n", &target), kept(format!("Bug 1 - new\n\nBecause.\n\n{TRAILER}")));
        assert_eq!(with_trailer("Bug 1 - new", &target), kept(format!("Bug 1 - new\n\n{TRAILER}")));
        let signed = "Bug 1 - new\n\nBecause.\n\nReviewed-by: ann";
        assert_eq!(with_trailer(signed, &target), kept(format!("{signed}\n{TRAILER}")));
        // Another revision of its own: the other in its place.
        let other = TRAILER.replace("D123", "D7");
        assert_eq!(with_trailer(&format!("Bug 1 - new\n\n{other}\nReviewed-by: ann"), &target), kept(format!("Bug 1 - new\n\n{TRAILER}\nReviewed-by: ann")));
        // The same of its own, or none to take: as it is.
        let same = format!("Bug 1 - new\n\n{TRAILER}");
        assert_eq!(with_trailer(&same, &target), (same.clone(), None));
        assert_eq!(with_trailer(&same, "Bug 1 - old"), (same, None));
        assert_eq!(with_trailer("Bug 1 - new", "Bug 1 - old"), ("Bug 1 - new".to_string(), None));
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn the_message_kept_is_the_one_picked_with_the_revision_only_the_other_names() {
        let with = |m: &str, d: &str| format!("{m}\n\n{}", TRAILER.replace("D123", d));
        let k = |message: String, carried: Option<&str>, dropped: Option<&str>| {
            Some(Kept {
                message,
                carried: carried.map(String::from),
                dropped: dropped.map(String::from),
            })
        };
        // What they say differs: none kept until one is picked; the one
        // picked as it is.
        let (ours, theirs) = ("Bug 1 - two\n\nBecause.", "Bug 1 - two\n\nWhy.");
        assert_eq!(kept(ours, theirs, &Differs::Text, None), None);
        assert_eq!(kept(ours, theirs, &Differs::Text, Some(Keep::Workspace)), k(theirs.into(), None, None));
        // The one picked names no revision, the other one: it takes it.
        let (ours, theirs) = (with("Bug 1 - two\n\nBecause.", "D1"), "Bug 1 - two\n\nWhy.");
        let both = k(with("Bug 1 - two\n\nWhy.", "D1"), Some("D1"), None);
        assert_eq!(kept(&ours, theirs, &Differs::Text, Some(Keep::Workspace)), both);
        assert_eq!(kept(&ours, theirs, &Differs::Text, Some(Keep::Stack)), k(ours.clone(), None, None));
        // Each names another: the one picked keeps its own, the other's
        // dropped.
        let (ours, theirs) = (with("Bug 1 - two", "D1"), with("Bug 1 - two", "D2"));
        let apart = differs(&ours, &theirs).unwrap();
        assert_eq!(kept(&ours, &theirs, &apart, None), None);
        assert_eq!(kept(&ours, &theirs, &apart, Some(Keep::Stack)), k(ours.clone(), None, Some("D2")));
        assert_eq!(kept(&ours, &theirs, &apart, Some(Keep::Workspace)), k(theirs.clone(), None, Some("D1")));
        // One alone names it: that one, as it is, whatever is picked.
        let one = differs("Bug 1 - two", &theirs).unwrap();
        for pick in [None, Some(Keep::Stack), Some(Keep::Workspace)] {
            assert_eq!(kept("Bug 1 - two", &theirs, &one, pick), k(theirs.clone(), None, None), "{pick:?}");
        }
    }

    #[test]
    fn a_plan_says_what_changes_on_each_side_and_how_many_commits_are_made_again() {
        let on = OnStack {
            commits: vec!["a".into(), "b".into(), "c".into()],
            messages: vec![(0, "Bug 1 - one".into(), Differs::Text), (2, "Bug 3 - three".into(), Differs::Text)],
            above: vec!["d".into()],
        };
        let ours = ["Bug 1 - first", "Bug 2", "Bug 3 - third"];
        // Still to pick: said.
        assert_eq!(plan(&on, &ours, &[(0, Keep::Stack)]), Err(vec![2]));
        assert_eq!(plan(&on, &ours, &[]), Err(vec![0, 2]));
        // The stack's message of patch 1, the commit's of patch 3: commit 1
        // made again, and every one above it.
        let p = plan(&on, &ours, &[(0, Keep::Stack), (2, Keep::Workspace)]).unwrap();
        assert_eq!(p.patches, [(2, "Bug 3 - three".to_string())]);
        assert_eq!(p.commits, [(0, "Bug 1 - first".to_string())]);
        assert_eq!(p.remade, 4);
        // The commits' messages both: no commit made again.
        let p = plan(&on, &ours, &[(0, Keep::Workspace), (2, Keep::Workspace)]).unwrap();
        assert_eq!((p.patches.len(), p.commits.len(), p.remade), (2, 0, 0));
    }

    #[test]
    fn head_is_out_of_sync_while_revisions_differ() {
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
        assert_eq!(h.differ(), [1, 2], "each to sync, the one with something to pick and the one with nothing");
        assert!(!h.in_sync(3), "following the stack, HEAD would lose the revisions only the commits name");
    }

    #[cfg(not(feature = "moz-phab"))]
    #[test]
    fn without_moz_phab_the_trailer_is_compared_and_kept_as_any_line() {
        let with = format!("Bug 1 - two\n\nBecause.\n\n{TRAILER}");
        assert_eq!(differs("Bug 1 - two\n\nBecause.", &with), Some(Differs::Text));
        assert_eq!(with_trailer("Bug 1 - new", &with), ("Bug 1 - new".to_string(), None));
        assert_eq!(revision_in(&with), None);
        // The one picked, as it is: no revision taken from the other.
        let k = kept("Bug 1 - two\n\nBecause.", &with, &Differs::Text, Some(Keep::Stack)).unwrap();
        assert_eq!((k.message.as_str(), k.carried, k.dropped), ("Bug 1 - two\n\nBecause.", None, None));
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
