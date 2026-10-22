//! What a new stack is made from, in the editor, read first to be seen
//! before it is imported: the commits of a range of the checkout stacked
//! runs in (its branch on its upstream, another branch on its own, or a
//! range given by hand), its staged and unstaged changes on top if asked.
//! After self-review's ranges: a branch is on its upstream, else on the
//! remote's HEAD; and a range given by hand is said as it is typed, taken
//! only once it holds a commit.

use std::path::Path;

use crate::db::{Db, Result};
use crate::git::{format_patch, git};
use crate::mbox::{Draft, parse_mbox};

/// The branch checked out at `repo`, if one is.
pub fn current_branch(repo: &Path) -> Option<String> {
    let out = git(repo, &["symbolic-ref", "--short", "-q", "HEAD"]).ok()?;
    Some(out.trim().to_string()).filter(|b| !b.is_empty())
}

/// The local branches of `repo`, by name.
pub fn branches(repo: &Path) -> Vec<String> {
    git(repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .map(|out| out.lines().filter(|b| !b.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

/// What `branch` (none: the one checked out) is on: its upstream, else the
/// remote's HEAD, else main or master but itself; none if none of them is
/// there.
pub fn upstream(repo: &Path, branch: Option<&str>) -> Option<String> {
    let own = match branch {
        Some(b) => format!("{b}@{{upstream}}"),
        None => "@{upstream}".to_string(),
    };
    for r in [own.as_str(), "origin/HEAD"] {
        if let Ok(out) = git(repo, &["rev-parse", "--abbrev-ref", r]) {
            let out = out.trim();
            if !out.is_empty() {
                return Some(out.to_string());
            }
        }
    }
    let me = branch.map(String::from).or_else(|| current_branch(repo));
    ["main", "master"]
        .into_iter()
        .filter(|b| me.as_deref() != Some(*b))
        .find(|b| git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_ok())
        .map(String::from)
}

/// `A..B` as (A, B), `A` as (A, HEAD): an end left out is HEAD.
pub fn ends(spec: &str) -> (String, String) {
    let spec = spec.trim();
    let or_head = |s: &str| if s.is_empty() { "HEAD".to_string() } else { s.to_string() };
    match spec.split_once("..") {
        Some((a, b)) => (or_head(a), or_head(b)),
        None => (spec.to_string(), "HEAD".to_string()),
    }
}

/// How many commits range `spec` holds, from the merge base of its ends, as
/// an import takes them; none if an end is not there.
pub fn count(repo: &Path, spec: &str) -> Option<usize> {
    let (a, b) = ends(spec);
    let base = git(repo, &["merge-base", &a, &b]).ok()?;
    git(repo, &["rev-list", "--count", &format!("{}..{b}", base.trim())]).ok()?.trim().parse().ok()
}

/// What range `spec`, given by hand, holds, said; and whether it can be
/// imported: given, its ends there, a commit in it at least.
pub fn check_range(repo: &Path, spec: &str) -> (String, bool) {
    if spec.trim().is_empty() {
        return ("give a commit, or a base..tip range".into(), false);
    }
    match count(repo, spec) {
        None => ("unknown ref".into(), false),
        Some(0) => ("0 commits: nothing to import".into(), false),
        Some(1) => ("1 commit".into(), true),
        Some(n) => (format!("{n} commits"), true),
    }
}

/// Whether the index, and the files, of the checkout at `repo` hold changes
/// not committed: (staged, unstaged), files git does not track left out.
pub fn uncommitted(repo: &Path) -> (bool, bool) {
    let Ok(out) = git(repo, &["status", "--porcelain", "--untracked-files=no"]) else {
        return (false, false);
    };
    let (mut staged, mut unstaged) = (false, false);
    for b in out.lines().map(str::as_bytes).filter(|b| b.len() >= 2) {
        staged |= !matches!(b[0], b' ' | b'?');
        unstaged |= !matches!(b[1], b' ' | b'?');
    }
    (staged, unstaged)
}

/// What an import reads, before it is made: its drafts, bottom first, and
/// the commit they go on.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub drafts: Vec<Draft>,
    pub base: String,
}

/// Range `spec` of the checkout at `repo`, read: its commits from the merge
/// base of its ends, a draft each; and, a draft each on top, the checkout's
/// staged changes if `staged` says, then its unstaged ones if `unstaged`
/// does.
pub fn read_range(repo: &Path, spec: &str, staged: bool, unstaged: bool) -> Result<Read> {
    let (a, b) = ends(spec);
    let (mail, base) = format_patch(repo, &format!("{a}..{b}"))?;
    let mut drafts = if mail.trim().is_empty() { vec![] } else { parse_mbox(&mail)? };
    let changes: [(bool, &str, &[&str]); 2] = [
        (staged, "Staged changes", &["diff", "--cached", "--binary", "--no-color", "--no-ext-diff"]),
        (unstaged, "Unstaged changes", &["diff", "--binary", "--no-color", "--no-ext-diff"]),
    ];
    for (on, title, args) in changes {
        if !on {
            continue;
        }
        let diff = git(repo, args)?;
        if !diff.trim().is_empty() {
            drafts.push(Draft {
                message: title.into(),
                diff,
                ..Default::default()
            });
        }
    }
    Ok(Read { drafts, base })
}

/// A name for a new stack of `db`, from `want` (a branch's, say): its
/// letters, digits and `-._` kept, any other character a `-`; then, taken by
/// a stack, with `-2`, `-3`… after it.
pub fn free_name(db: &Db, want: &str) -> String {
    let kept: String = want.trim().chars().map(|c| if c.is_ascii_alphanumeric() || "-._".contains(c) { c } else { '-' }).collect();
    let kept = kept.trim_start_matches('-');
    let kept = if kept.is_empty() { "stack" } else { kept };
    let names = db.names();
    (1..)
        .map(|n| if n == 1 { kept.to_string() } else { format!("{kept}-{n}") })
        .find(|n| !names.contains(&n.as_str()))
        .unwrap()
}

/// What is wrong with `name` for a new stack of `db`, if anything.
pub fn name_wrong(db: &Db, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("give it a name".into());
    }
    if name.contains(|c: char| c.is_whitespace() || c == '/') || name.starts_with('-') {
        return Some(format!("{name:?} cannot name a stack: no blank, no /, no - first"));
    }
    if db.names().contains(&name) {
        return Some(format!("there is a stack {name} already"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "A U Thor")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_COMMITTER_NAME", "C O Mitter")
            .env("GIT_COMMITTER_EMAIL", "committer@example.com")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A repository: main, a file; a branch `feature` on it, main its
    /// upstream, two commits; and `other`, on main, its own upstream none.
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        run(d, &["init", "-q", "-b", "main"]);
        run(d, &["config", "commit.gpgsign", "false"]);
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        run(d, &["add", "."]);
        run(d, &["commit", "-qm", "base"]);
        run(d, &["branch", "other"]);
        run(d, &["checkout", "-qb", "feature"]);
        run(d, &["branch", "-q", "--set-upstream-to=main"]);
        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        run(d, &["commit", "-qam", "Bug 1 - add two"]);
        std::fs::write(d.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        run(d, &["commit", "-qam", "Bug 2 - add three"]);
        tmp
    }

    #[test]
    fn a_branch_is_on_its_upstream_else_the_remotes_head_else_main() {
        let tmp = repo();
        let d = tmp.path();
        assert_eq!(current_branch(d).as_deref(), Some("feature"));
        assert_eq!(branches(d), ["feature", "main", "other"]);
        assert_eq!(upstream(d, None).as_deref(), Some("main"));
        assert_eq!(upstream(d, Some("other")).as_deref(), Some("main"), "no upstream of its own: main");
        assert_eq!(upstream(d, Some("main")), None, "not on itself");
        // Detached: no branch, main still what it is on.
        run(d, &["checkout", "-q", "--detach"]);
        assert_eq!(current_branch(d), None);
        assert_eq!(upstream(d, None).as_deref(), Some("main"));
    }

    #[test]
    fn a_range_given_by_hand_is_said_and_taken_only_with_a_commit() {
        let tmp = repo();
        let d = tmp.path();
        assert_eq!(check_range(d, " "), ("give a commit, or a base..tip range".to_string(), false));
        assert_eq!(check_range(d, "nope..HEAD"), ("unknown ref".to_string(), false));
        assert_eq!(check_range(d, "HEAD..HEAD"), ("0 commits: nothing to import".to_string(), false));
        assert_eq!(check_range(d, "main..HEAD"), ("2 commits".to_string(), true));
        assert_eq!(check_range(d, "main"), ("2 commits".to_string(), true), "an end left out is HEAD");
        assert_eq!(check_range(d, "HEAD~1.."), ("1 commit".to_string(), true));
        assert_eq!(check_range(d, "feature..other"), ("0 commits: nothing to import".to_string(), false));
    }

    #[test]
    fn a_range_is_read_its_uncommitted_changes_on_top_if_asked() {
        let tmp = repo();
        let d = tmp.path();
        let r = read_range(d, "main..HEAD", true, true).unwrap();
        assert_eq!(r.drafts.iter().map(Draft::title).collect::<Vec<_>>(), ["Bug 1 - add two", "Bug 2 - add three"]);
        assert_eq!(r.base, git(d, &["rev-parse", "main"]).unwrap().trim());
        assert_eq!(uncommitted(d), (false, false));
        // Staged, then more not staged.
        std::fs::write(d.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        run(d, &["add", "a.txt"]);
        std::fs::write(d.join("a.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        assert_eq!(uncommitted(d), (true, true));
        let r = read_range(d, "main..HEAD", true, true).unwrap();
        let titles: Vec<&str> = r.drafts.iter().map(Draft::title).collect();
        assert_eq!(titles, ["Bug 1 - add two", "Bug 2 - add three", "Staged changes", "Unstaged changes"]);
        assert!(r.drafts[2].diff.contains("+four") && !r.drafts[2].diff.contains("+five"), "{}", r.drafts[2].diff);
        assert!(r.drafts[3].diff.contains("+five") && !r.drafts[3].diff.contains("+four"), "{}", r.drafts[3].diff);
        let r = read_range(d, "main..HEAD", false, true).unwrap();
        assert_eq!(r.drafts.last().map(Draft::title), Some("Unstaged changes"));
        assert_eq!(r.drafts.len(), 3);
        // Nothing staged: no draft for it, asked or not.
        run(d, &["reset", "-q"]);
        assert_eq!(uncommitted(d), (false, true));
        let r = read_range(d, "main..HEAD", true, false).unwrap();
        assert_eq!(r.drafts.len(), 2);
    }

    #[test]
    fn a_new_stack_is_named_as_no_other_is() {
        let tmp = repo();
        let d = tmp.path();
        let path = d.join("db.jsonl");
        let base = git(d, &["rev-parse", "main"]).unwrap();
        Db::edit(&path, |db| db.new_stack("feature", base.trim(), &d.to_string_lossy()).map(|_| ())).unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!(free_name(&db, "feature"), "feature-2");
        assert_eq!(free_name(&db, "bug/1 two"), "bug-1-two");
        assert_eq!(free_name(&db, "--"), "stack");
        assert_eq!(name_wrong(&db, "feature").as_deref(), Some("there is a stack feature already"));
        assert!(name_wrong(&db, "a b").is_some() && name_wrong(&db, "-a").is_some() && name_wrong(&db, "a/b").is_some());
        assert_eq!(name_wrong(&db, " "), Some("give it a name".to_string()));
        assert_eq!(name_wrong(&db, "fix"), None);
    }
}
