//! Where HEAD is, in the checkout stacked runs in, against a stack: on its
//! base with some of its patches as commits, a commit each, then maybe
//! commits that are not the stack's; or elsewhere. A commit is a patch when
//! its diff is, by `git patch-id`; its message may still differ. And HEAD
//! moved to a place on the stack: detached there, the commits the checkout
//! has that are the stack's taken as they are, the rest applied; the branch
//! it was on as it was.

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
    /// Of those, the places (from 0) whose message is not the patch's, and
    /// the commit's message.
    pub messages: Vec<(usize, String)>,
    /// The commits above them to HEAD, which are not the stack's.
    pub above: Vec<String>,
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

    /// The places (from 0) of the patches whose message differs.
    pub fn differ(&self) -> Vec<usize> {
        match &self.at {
            At::Stack(on) => on.messages.iter().map(|(k, _)| *k).collect(),
            At::Elsewhere => vec![],
        }
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
        if norm(&m) != norm(&patches[k].message) {
            on.messages.push((k, norm(&m)));
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

/// A message as compared, and as taken: each line's end trimmed, no blank
/// line at either end.
fn norm(m: &str) -> String {
    let lines: Vec<&str> = m.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |e| e + 1);
    lines[start..end].join("\n")
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
    let agent = db
        .runs()
        .into_iter()
        .filter(|r| crate::ask::running(r))
        .filter_map(crate::ask::info)
        .any(|i| i.back.is_some() && i.worktree.as_deref().map(Path::new) == Some(dir.as_path()));
    if agent {
        return err(format!("the agent is working in {}, your checkout: {nothing} until its ask lands", dir.display()));
    }
    Ok((dir, base))
}

/// HEAD of the checkout at `here` moved to stack `name` with its first `n`
/// patches applied: detached there, the commits the checkout has that are
/// those patches, messages too, taken as they are, the rest applied as
/// commits (unsigned); the branch it was on as it was. What came of it,
/// said.
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
            let same = on.messages.first().map_or(on.commits.len(), |(k, _)| *k);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_compared_without_the_blanks_at_its_ends() {
        assert_eq!(norm("Bug 1 - two  \n\nBecause.\n\n"), "Bug 1 - two\n\nBecause.");
        assert_eq!(norm("\n\nBug 1\n"), "Bug 1");
        assert_eq!(norm(""), "");
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
