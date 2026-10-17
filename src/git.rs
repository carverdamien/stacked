//! What stacked asks of git: mail from a range, a worktree of its own for
//! claude, and putting a stack into the user's worktree when asked to.

use std::io::Write;
use std::path::Path;
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

/// A worktree of its own at `at`, detached at `base`, with `mbox` applied;
/// none if the patches do not apply.
pub fn scratch(repo: &Path, at: &Path, base: &str, mbox: &str) -> Result<()> {
    git(repo, &["worktree", "add", "--quiet", "--detach", &at.to_string_lossy(), base])?;
    if !mbox.is_empty()
        && let Err(e) = git_in(at, &["-c", "commit.gpgsign=false", "am", "--quiet", "--keep-non-patch"], Some(mbox))
    {
        remove_scratch(repo, at);
        return err(format!("the stack does not apply to its base: {}", e.0));
    }
    Ok(())
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
