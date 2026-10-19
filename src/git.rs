//! What stacked asks of git: mail from a range, a worktree of its own for
//! claude, a checkout to edit a patch in, and putting a stack into the
//! user's worktree when asked to.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::db::{Error, Result, err};

/// Runs git in `dir`; its stdout, or its stderr as the error.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    git_in(dir, args, None)
}

pub fn git_in(dir: &Path, args: &[&str], stdin: Option<&str>) -> Result<String> {
    git_env(dir, args, stdin, &[])
}

/// As `git_in`, with `env` set for git.
pub fn git_env(
    dir: &Path,
    args: &[&str],
    stdin: Option<&str>,
    env: &[(&str, &Path)],
) -> Result<String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (*k, *v)))
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error(format!("git: {e}")))?;
    // Written meanwhile, from a thread: a git that answers as it reads
    // would otherwise wait on its output being read while this waits on its
    // input being taken. One that stops reading early says why on stderr.
    let writer = stdin.map(|text| {
        let mut pipe = child.stdin.take().unwrap();
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
        })
    });
    let out = child
        .wait_with_output()
        .map_err(|e| Error(format!("git: {e}")))?;
    if let Some(w) = writer {
        let _ = w.join();
    }
    if !out.status.success() {
        return err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn toplevel(dir: &Path) -> Result<String> {
    Ok(git(dir, &["rev-parse", "--show-toplevel"])?.trim().to_string())
}

pub fn rev_parse(dir: &Path, rev: &str) -> Result<String> {
    Ok(git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")])
        .map_err(|_| Error(format!("no commit {rev}")))?
        .trim()
        .to_string())
}

/// The mail of `range` (`main..HEAD`, or `main` for `main..HEAD`) and the
/// commit it applies to.
pub fn format_patch(dir: &Path, range: &str) -> Result<(String, String)> {
    let (from, to) = match range.split_once("..") {
        Some((a, b)) => (a, if b.is_empty() { "HEAD" } else { b }),
        None => (range, "HEAD"),
    };
    let base = git(dir, &["merge-base", from, to])?.trim().to_string();
    let mail = git(
        dir,
        &["format-patch", "--stdout", "--no-signature", "--keep-subject", &format!("{base}..{to}")],
    )?;
    Ok((mail, base))
}

/// Uncommitted changes to tracked files, if any.
pub fn dirty(dir: &Path) -> Result<bool> {
    Ok(!git(dir, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty())
}

/// Anything not committed, untracked files too.
pub fn unfinished(dir: &Path) -> Result<bool> {
    let status = git(dir, &["status", "--porcelain"])?;
    let gitdir = git(dir, &["rev-parse", "--git-dir"])?;
    let gitdir = Path::new(gitdir.trim());
    let busy = ["rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD"]
        .iter()
        .any(|f| dir.join(gitdir).join(f).exists());
    Ok(!status.trim().is_empty() || busy)
}

/// The top of the checkout at `dir`, if it is a checkout of the same
/// repository as `repo`'s: one whose objects are `repo`'s.
pub fn checkout_of(repo: &Path, dir: &Path) -> Result<PathBuf> {
    let top = toplevel(dir).map_err(|_| Error(format!("{} is not a git checkout", dir.display())))?;
    let common = |d: &Path| -> Result<PathBuf> {
        let c = git(d, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let c = PathBuf::from(c.trim());
        Ok(c.canonicalize().unwrap_or(c))
    };
    if common(dir)? != common(repo)? {
        return err(format!("{} is not a checkout of {}", dir.display(), repo.display()));
    }
    Ok(PathBuf::from(top))
}

/// Where stack `name`'s worktrees are, `.stacked/NAME` at the top of the
/// checkout `repo`, `.stacked/` kept out of git's sight in every checkout of
/// the repository: yours, `user`, and the agents'. One made before there
/// were others, `.stacked/NAME` itself, is moved to `user` in it.
pub fn stack_dir(repo: &Path, name: &str) -> Result<PathBuf> {
    let top = PathBuf::from(toplevel(repo)?);
    exclude(&top, "/.stacked/")?;
    let dir = top.join(".stacked").join(name);
    if dir.join(".git").is_file() {
        let aside = top.join(".stacked").join(format!("{name}.user"));
        git(&top, &["worktree", "move", &dir.to_string_lossy(), &aside.to_string_lossy()])?;
        std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
        git(&top, &["worktree", "move", &aside.to_string_lossy(), &dir.join("user").to_string_lossy()])?;
    }
    Ok(dir)
}

/// Your worktree of stack `name`, `.stacked/NAME/user` at the top of the
/// checkout `repo`: made at `base` the first time, and kept.
pub fn kept_worktree(repo: &Path, name: &str, base: &str) -> Result<PathBuf> {
    let dir = stack_dir(repo, name)?.join("user");
    if !dir.join(".git").exists() {
        let _ = git(repo, &["worktree", "prune"]);
        git(repo, &["worktree", "add", "--quiet", "--detach", &dir.to_string_lossy(), base])?;
    }
    Ok(dir)
}

/// The worktree at `at` put at `base` with `mbox` applied, `n` patches: made
/// if it is not there, else moved there (it holds nothing unfinished, so
/// nothing is lost, and only what differs is written). Not applied, it is
/// left at `base`. `step` is told each git command before it runs.
pub fn ready(repo: &Path, at: &Path, base: &str, mbox: &str, n: usize, step: &dyn Fn(String)) -> Result<()> {
    let short = &base[..12.min(base.len())];
    if at.join(".git").exists() {
        step(format!("git switch --detach {short}, in {}", at.display()));
        git(at, &["switch", "--quiet", "--detach", base])?;
    } else {
        step(format!("git worktree add --detach {} {short}", at.display()));
        let _ = git(repo, &["worktree", "prune"]);
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error(format!("{}: {e}", parent.display())))?;
        }
        git(repo, &["worktree", "add", "--quiet", "--detach", &at.to_string_lossy(), base])?;
    }
    if mbox.is_empty() {
        return Ok(());
    }
    step(format!("git am --keep-non-patch, {n} patch{}", if n == 1 { "" } else { "es" }));
    am(at, mbox).map_err(|e| Error(format!("the stack does not apply to its base: {}", e.0)))
}

/// `pattern` in the repository's `info/exclude`, unless it is there.
pub fn exclude(repo: &Path, pattern: &str) -> Result<()> {
    let common = git(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let file = PathBuf::from(common.trim()).join("info").join("exclude");
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    if text.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(pattern);
    text.push('\n');
    let say = |e: std::io::Error| Error(format!("{}: {e}", file.display()));
    std::fs::create_dir_all(file.parent().unwrap_or(Path::new("."))).map_err(say)?;
    std::fs::write(&file, text).map_err(say)
}

/// Changes to the files git tracks at `dir`, or a rebase, merge or
/// cherry-pick under way there: what moving the checkout would lose.
pub fn unfinished_tracked(dir: &Path) -> Result<bool> {
    if dirty(dir)? {
        return Ok(true);
    }
    let gitdir = git(dir, &["rev-parse", "--absolute-git-dir"])?;
    let gitdir = Path::new(gitdir.trim());
    Ok(["rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD"]
        .iter()
        .any(|f| gitdir.join(f).exists()))
}

/// Where a checkout is: on a branch, or at a commit.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Head {
    Branch(String),
    Commit(String),
}

/// Where the checkout at `dir` is.
pub fn head_of(dir: &Path) -> Result<Head> {
    match git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(b) if !b.trim().is_empty() => Ok(Head::Branch(b.trim().to_string())),
        _ => Ok(Head::Commit(rev_parse(dir, "HEAD")?)),
    }
}

/// The checkout at `dir` back at `head`.
pub fn back_to(dir: &Path, head: &Head) -> Result<()> {
    match head {
        Head::Branch(b) => git(dir, &["switch", "--quiet", b])?,
        Head::Commit(c) => git(dir, &["switch", "--quiet", "--detach", c])?,
    };
    Ok(())
}

/// The files at `dir` git neither tracks nor ignores.
pub fn untracked(dir: &Path) -> Result<std::collections::BTreeSet<String>> {
    Ok(git(dir, &["ls-files", "--others", "--exclude-standard", "-z"])?
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect())
}

/// What is not committed at `dir` undone: the files git tracks as they
/// are in its commit, and the files new since `before` gone. A `git am`
/// left midway is given up.
pub fn discard(dir: &Path, before: &std::collections::BTreeSet<String>) {
    if git(dir, &["rev-parse", "--absolute-git-dir"]).is_ok_and(|g| Path::new(g.trim()).join("rebase-apply").exists()) {
        let _ = git(dir, &["am", "--abort"]);
    }
    let _ = git(dir, &["reset", "--quiet", "--hard"]);
    if let Ok(now) = untracked(dir) {
        for f in now.difference(before) {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }
}

/// `mbox` applied at `dir` as commits, unsigned; none of it if it does
/// not all apply.
pub fn am(dir: &Path, mbox: &str) -> Result<()> {
    if mbox.is_empty() {
        return Ok(());
    }
    if let Err(e) = git_in(dir, &["-c", "commit.gpgsign=false", "am", "--quiet", "--keep-non-patch"], Some(mbox)) {
        let _ = git(dir, &["am", "--abort"]);
        return Err(e);
    }
    Ok(())
}

pub fn remove_scratch(repo: &Path, at: &Path) {
    let _ = git(repo, &["worktree", "remove", "--force", &at.to_string_lossy()]);
    let _ = git(repo, &["worktree", "prune"]);
}

/// Puts the stack into the worktree of `repo`: `base` checked out and
/// `mbox` applied, on `branch` if one is named (made or reset there), else
/// detached. Only when nothing is uncommitted, so nothing is lost.
pub fn apply(repo: &Path, base: &str, mbox: &str, branch: Option<&str>) -> Result<String> {
    if dirty(repo)? {
        return err(format!("{} has uncommitted changes; nothing applied", repo.display()));
    }
    match branch {
        Some(b) => git(repo, &["switch", "--quiet", "--force-create", b, base])?,
        None => git(repo, &["switch", "--quiet", "--detach", base])?,
    };
    if !mbox.is_empty()
        && let Err(e) = git_in(repo, &["am", "--quiet", "--3way", "--keep-non-patch"], Some(mbox))
    {
        let _ = git(repo, &["am", "--abort"]);
        return err(format!("{}; the worktree is left at the base", e.0));
    }
    Ok(git(repo, &["rev-parse", "--short", "HEAD"])?.trim().to_string())
}
