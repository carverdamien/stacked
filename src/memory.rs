//! Files and repositories kept in memory, for `stacked demo`, which writes
//! nothing on disk. A database whose path is kept here is read and
//! appended to here, under a lock of this process's; a repository named
//! `memory:NAME` is a tree of files here, at one base, its patches applied
//! as `git apply` applies them: each hunk's lines where its header says, or
//! the nearest place they are, else it does not apply. An ask on such a
//! repository goes to a stand-in for the agent, which the demo names.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::db::{Error, Result, err};

type Shared = Arc<Mutex<Vec<u8>>>;

static FILES: Mutex<Option<HashMap<PathBuf, Shared>>> = Mutex::new(None);

/// The lock of the files kept here, as flock is of those on disk.
static LOCK: Mutex<()> = Mutex::new(());

/// The repositories, by name: a base and its files.
static TREES: Mutex<Option<HashMap<String, (String, Tree)>>> = Mutex::new(None);

/// Who answers asks on the repositories here: the database's path, the
/// run's id.
type Agent = fn(&Path, &str) -> Result<()>;
static AGENT: Mutex<Option<Agent>> = Mutex::new(None);

/// A repository's files, by path.
pub type Tree = BTreeMap<String, String>;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keeps the database at `path` here, empty.
pub fn keep(path: &Path) {
    locked(&FILES).get_or_insert_with(HashMap::new).insert(path.to_path_buf(), Arc::default());
}

/// The bytes of the file at `path`, if it is kept here.
pub fn file(path: &Path) -> Option<Shared> {
    locked(&FILES).as_ref()?.get(path).cloned()
}

/// Whether the file at `path` is kept here.
pub fn is_file(path: &Path) -> bool {
    file(path).is_some()
}

/// The lock of the files kept here.
pub fn lock() -> MutexGuard<'static, ()> {
    locked(&LOCK)
}

/// A repository of `files` at `base`, named `memory:NAME`.
pub fn plant(repo: &str, base: &str, files: Tree) {
    locked(&TREES).get_or_insert_with(HashMap::new).insert(repo.to_string(), (base.to_string(), files));
}

/// Whether `repo` is a repository of here.
pub fn is_repo(repo: &Path) -> bool {
    repo.to_str().is_some_and(|r| r.starts_with("memory:"))
}

/// The files of `repo` at `base`: none if it is not a repository of here,
/// an error if it is not at that base.
pub fn tree(repo: &Path, base: &str) -> Option<Result<Tree>> {
    if !is_repo(repo) {
        return None;
    }
    let trees = locked(&TREES);
    let found = trees.as_ref().and_then(|t| t.get(repo.to_str()?));
    Some(match found {
        Some((b, files)) if b == base => Ok(files.clone()),
        Some(_) => err(format!("its base, {}, is not a commit of {}", &base[..12.min(base.len())], repo.display())),
        None => err(format!("its repository, {}, is not there", repo.display())),
    })
}

/// Names who answers asks on the repositories here.
pub fn stand_in(agent: Agent) {
    *locked(&AGENT) = Some(agent);
}

/// Run `id` of the database at `path`, an ask on a repository of here:
/// answered by the stand-in.
pub fn agent(path: &Path, id: &str) -> Result<()> {
    let agent = *locked(&AGENT);
    match agent {
        Some(f) => f(path, id),
        None => err("there is no agent for a repository kept in memory"),
    }
}

/// A file of a diff: where it was and is (none for no file), its hunks.
#[derive(Debug, Default)]
struct FilePatch {
    old: Option<String>,
    new: Option<String>,
    hunks: Vec<Hunk>,
}

/// A hunk: where it starts in the old file (from 1), its lines before (the
/// context and what it takes out) and after (the context and what it puts
/// in).
#[derive(Debug, Default)]
struct Hunk {
    start: usize,
    before: Vec<String>,
    after: Vec<String>,
}

/// The path of a `---` or `+++` line, `a/` or `b/` taken off; none for
/// /dev/null.
fn side(path: &str) -> Option<String> {
    let path = path.split('\t').next().unwrap_or(path).trim_end();
    if path == "/dev/null" {
        return None;
    }
    Some(path.strip_prefix("a/").or_else(|| path.strip_prefix("b/")).unwrap_or(path).to_string())
}

fn parse(diff: &str) -> Result<Vec<FilePatch>> {
    let mut files: Vec<FilePatch> = Vec::new();
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            files.push(FilePatch::default());
        } else if let Some(p) = line.strip_prefix("--- ") {
            if files.is_empty() {
                files.push(FilePatch::default());
            }
            files.last_mut().unwrap().old = side(p);
        } else if let Some(p) = line.strip_prefix("+++ ") {
            if let Some(f) = files.last_mut() {
                f.new = side(p);
            }
        } else if let Some(h) = line.strip_prefix("@@ -") {
            let start = h.split([',', ' ']).next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let Some(f) = files.last_mut() else { return err("a hunk before its file") };
            f.hunks.push(Hunk { start, ..Default::default() });
        } else if let Some(h) = files.last_mut().and_then(|f| f.hunks.last_mut()) {
            match line.chars().next() {
                Some(' ') => {
                    h.before.push(line[1..].to_string());
                    h.after.push(line[1..].to_string());
                }
                Some('-') => h.before.push(line[1..].to_string()),
                Some('+') => h.after.push(line[1..].to_string()),
                // A context line of nothing, as some write it.
                None => {
                    h.before.push(String::new());
                    h.after.push(String::new());
                }
                _ => {}
            }
        }
    }
    Ok(files)
}

/// `tree` with `diff` applied, as `git apply` would, or why not: as git
/// says it, `patch failed: board.txt:4`.
pub fn apply(tree: &mut Tree, diff: &str) -> Result<()> {
    for f in parse(diff)? {
        let path = f.new.clone().or_else(|| f.old.clone()).ok_or_else(|| Error("a file with no name".into()))?;
        let text = match &f.old {
            Some(old) => tree.get(old).cloned().ok_or_else(|| Error(format!("{old}: does not exist in index")))?,
            None if tree.contains_key(&path) => return err(format!("{path}: already exists in index")),
            None => String::new(),
        };
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        // Each hunk at or after the one before, where its header says, or
        // the nearest place its lines are.
        let mut from = 0usize;
        let mut shift = 0isize;
        for h in &f.hunks {
            let want = (h.start.max(1) as isize - 1 + shift).max(from as isize) as usize;
            let fits = |at: usize| at + h.before.len() <= lines.len() && lines[at..at + h.before.len()] == h.before[..];
            let at = (0..=lines.len())
                .flat_map(|d| [want.checked_add(d), want.checked_sub(d)])
                .flatten()
                .filter(|&at| at >= from && at <= lines.len())
                .find(|&at| fits(at))
                .ok_or_else(|| Error(format!("patch failed: {path}:{}", h.start)))?;
            lines.splice(at..at + h.before.len(), h.after.iter().cloned());
            from = at + h.after.len();
            shift += h.after.len() as isize - h.before.len() as isize;
        }
        if let Some(old) = &f.old {
            tree.remove(old);
        }
        match &f.new {
            Some(new) => {
                let mut out = lines.join("\n");
                if !lines.is_empty() {
                    out.push('\n');
                }
                tree.insert(new.clone(), out);
            }
            // Deleted: nothing must be left of it.
            None if !lines.is_empty() => return err(format!("patch failed: {path}: it is not all deleted")),
            None => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(path: &str, before: &str, after: &str) -> String {
        let d = similar::TextDiff::from_lines(before, after)
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string();
        format!("diff --git a/{path} b/{path}\n{d}")
    }

    #[test]
    fn diffs_apply_as_git_applies_them() {
        let board = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
        let mut tree: Tree = [("b.txt".to_string(), board.to_string())].into();
        // Two changes far apart: each applies on its own, in either order.
        let top = diff("b.txt", board, &board.replace("two", "TWO"));
        let bottom = diff("b.txt", board, &board.replace("nine", "NINE"));
        apply(&mut tree, &bottom).unwrap();
        apply(&mut tree, &top).unwrap();
        assert_eq!(tree["b.txt"], board.replace("two", "TWO").replace("nine", "NINE"));
        // What it changes is changed already: not again.
        let e = apply(&mut tree, &top).unwrap_err();
        assert_eq!(e.0, "patch failed: b.txt:1");
        // Found where lines were added above it.
        let mut moved: Tree = [("b.txt".to_string(), format!("zero\nzero\n{board}"))].into();
        apply(&mut moved, &bottom).unwrap();
        assert!(moved["b.txt"].contains("NINE"));
        // A new file, then deleted.
        let mut t = Tree::new();
        apply(&mut t, "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+a\n+b\n").unwrap();
        assert_eq!(t["n"], "a\nb\n");
        apply(&mut t, "diff --git a/n b/n\ndeleted file mode 100644\n--- a/n\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n").unwrap();
        assert!(t.is_empty());
        assert!(apply(&mut t, &top).unwrap_err().0.contains("does not exist"));
    }

    #[test]
    fn files_and_repositories_are_kept_here() {
        let path = Path::new("memory-test/db.jsonl");
        assert!(!is_file(path));
        keep(path);
        file(path).unwrap().lock().unwrap().extend_from_slice(b"x");
        assert_eq!(*file(path).unwrap().lock().unwrap(), b"x");
        plant("memory:t", "base", [("f".to_string(), "x\n".to_string())].into());
        assert!(is_repo(Path::new("memory:t")) && !is_repo(Path::new("/tmp")));
        assert_eq!(tree(Path::new("memory:t"), "base").unwrap().unwrap()["f"], "x\n");
        assert!(tree(Path::new("memory:t"), "other").unwrap().is_err());
        assert!(tree(Path::new("/tmp"), "base").is_none());
    }
}
