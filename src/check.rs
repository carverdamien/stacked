//! Whether a stack's patches apply cleanly, each on the ones before it, to
//! its base: what must hold of every stack before it goes in the database.
//!
//! The patches are applied with `git apply --cached` to an index of their
//! own, which holds only the files they touch as the base has them: neither
//! the worktree nor the repository's index is touched, and it takes as long
//! in mozilla-central as in a toy. Should a patch fail there, the check is
//! made again against the whole base before it is believed, so a path the
//! patches touch and the list missed cannot make a good patch fail.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::db::{Error, Patch, Result, err};
use crate::git::{git, git_env};

/// The stacks seen to apply, by `key`: checked once per process.
static APPLIES: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn key(repo: &Path, base: &str, patches: &[&Patch]) -> String {
    let mut s = format!("{}\0{base}", repo.display());
    for p in patches {
        s.push('\0');
        s.push_str(&p.id);
    }
    sha1_smol::Sha1::from(s).digest().to_string()
}

/// Whether `patches` may be what stack `s` holds: they apply to its base,
/// each on the ones before it. A stack with no base, or whose repository or
/// base is not there, cannot be checked, and so cannot change: it is
/// read-only, comments aside.
pub fn stack(s: &crate::db::Stack, patches: &[&Patch]) -> Result<()> {
    let (repo, base) = can_check(s)?;
    applies(Path::new(repo), base, patches).map_err(|e| Error(format!("{}; nothing changed", e.0)))
}

/// The repository and base `s` is checked against, if they are there.
pub fn can_check(s: &crate::db::Stack) -> Result<(&str, &str)> {
    let (Some(repo), Some(base)) = (&s.repo, &s.base) else {
        return err(format!(
            "{} is read-only: it has no base commit to check its patches against (`stacked base {} REV` gives it one)",
            s.name, s.name
        ));
    };
    reachable(Path::new(repo), base).map_err(|e| Error(format!("{} is read-only: {}", s.name, e.0)))?;
    Ok((repo, base))
}

/// Is `base` a commit of the repository at `repo`?
pub fn reachable(repo: &Path, base: &str) -> Result<()> {
    #[cfg(feature = "demo")]
    if let Some(tree) = crate::memory::tree(repo, base) {
        return tree.map(|_| ());
    }
    if !repo.is_dir() {
        return err(format!("its repository, {}, is not there", repo.display()));
    }
    git(repo, &["cat-file", "-e", &format!("{base}^{{commit}}")])
        .map(|_| ())
        .map_err(|_| Error(format!("its base, {}, is not a commit of {}", short(base), repo.display())))
}

fn short(base: &str) -> &str {
    &base[..12.min(base.len())]
}

/// Checks that `patches` apply, in order, to `base` in `repo`: the first
/// that does not, and git's word on why, is the error.
pub fn applies(repo: &Path, base: &str, patches: &[&Patch]) -> Result<()> {
    // A repository kept in memory: applied there, as git would.
    #[cfg(feature = "demo")]
    if let Some(tree) = crate::memory::tree(repo, base) {
        let mut tree = tree?;
        for (i, p) in patches.iter().enumerate() {
            crate::memory::apply(&mut tree, &p.diff).map_err(|e| {
                let on = if i == 0 {
                    format!("its base, {}", short(base))
                } else {
                    "the patches below it".to_string()
                };
                Error(format!("patch {} ({}) does not apply to {on} ({})", i + 1, p.title(), e.0))
            })?;
        }
        return Ok(());
    }
    let key = key(repo, base, patches);
    if APPLIES.lock().unwrap().as_ref().is_some_and(|s| s.contains(&key)) {
        return Ok(());
    }
    reachable(repo, base)?;
    for (i, p) in patches.iter().enumerate() {
        if !p.diff.lines().any(|l| l.starts_with("diff ")) {
            return err(format!(
                "patch {} ({}) has no diff, and git am cannot apply that",
                i + 1,
                p.title()
            ));
        }
    }
    if !patches.is_empty() && try_apply(repo, base, Some(&touched(repo, patches)), patches).is_err() {
        try_apply(repo, base, None, patches)?;
    }
    APPLIES.lock().unwrap().get_or_insert_with(HashSet::new).insert(key);
    Ok(())
}

/// A directory of the check's own, gone when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "stacked-check-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An index of the check's own, holding `paths` of a base, or all of it.
/// What applying writes, the patched files' blobs, goes in an object store
/// of its own too: the repository's is only read.
struct Index {
    _dir: Scratch,
    index: PathBuf,
    objects: PathBuf,
    theirs: PathBuf,
}

impl Index {
    fn new(repo: &Path, base: &str, paths: Option<&BTreeSet<String>>) -> Result<Index> {
        let dir = Scratch::new()?;
        let index = dir.0.join("index");
        let objects = dir.0.join("objects");
        std::fs::create_dir_all(&objects).map_err(|e| Error(format!("{}: {e}", objects.display())))?;
        let theirs = PathBuf::from(
            git(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?.trim(),
        )
        .join("objects");
        let ix = Index {
            _dir: dir,
            index,
            objects,
            theirs,
        };
        let env = ix.env();
        match paths {
            None => {
                git_env(repo, &["read-tree", base], None, &env)?;
            }
            Some(paths) => {
                git_env(repo, &["read-tree", "--empty"], None, &env)?;
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                for chunk in paths.chunks(500) {
                    let mut args = vec!["--literal-pathspecs", "ls-tree", "-r", "-z", base, "--"];
                    args.extend(chunk);
                    let entries = git(repo, &args)?;
                    if !entries.is_empty() {
                        git_env(repo, &["update-index", "-z", "--index-info"], Some(&entries), &env)?;
                    }
                }
            }
        }
        Ok(ix)
    }

    fn env(&self) -> [(&'static str, &Path); 3] {
        [
            ("GIT_INDEX_FILE", self.index.as_path()),
            ("GIT_OBJECT_DIRECTORY", self.objects.as_path()),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES", self.theirs.as_path()),
        ]
    }

    fn apply(&self, repo: &Path, diff: &str) -> Result<String> {
        git_env(repo, &["apply", "--cached", "--whitespace=nowarn"], Some(diff), &self.env())
    }

    /// Applies `diffs` in turn, each in a `git apply` of its own: given as
    /// one input, a file two of them change and a later one renames with a
    /// change, git refuses (`patch failed`), though each applies on the ones
    /// before it. The first that does not apply, by place, and git's word.
    fn apply_each(&self, repo: &Path, diffs: &[&str]) -> std::result::Result<(), (usize, Error)> {
        for (k, d) in diffs.iter().enumerate() {
            self.apply(repo, d).map_err(|e| (k, e))?;
        }
        Ok(())
    }

    /// Whether `diff` would apply, nothing changed.
    fn would_apply(&self, repo: &Path, diff: &str) -> bool {
        git_env(repo, &["apply", "--cached", "--check", "--whitespace=nowarn"], Some(diff), &self.env()).is_ok()
    }

    /// What the index holds at `paths`, as text lines: None where it holds
    /// nothing, or something not text.
    fn files(&self, repo: &Path, paths: &[String]) -> Result<Vec<Option<Vec<String>>>> {
        Ok(self
            .texts(repo, paths)?
            .into_iter()
            .map(|t| t.map(|t| t.lines().map(String::from).collect()))
            .collect())
    }

    /// What the index holds at `paths`, as text: None where it holds
    /// nothing, or something not text.
    fn texts(&self, repo: &Path, paths: &[String]) -> Result<Vec<Option<String>>> {
        let mut args = vec!["--literal-pathspecs", "ls-files", "-s", "-z", "--"];
        args.extend(paths.iter().map(String::as_str));
        let listed = git_env(repo, &args, None, &self.env())?;
        let mut blob_of = HashMap::new();
        for entry in listed.split('\0').filter(|e| !e.is_empty()) {
            if let Some((meta, path)) = entry.split_once('\t')
                && let Some(id) = meta.split(' ').nth(1)
            {
                blob_of.insert(path.to_string(), id.to_string());
            }
        }
        let ids: Vec<String> = paths.iter().filter_map(|p| blob_of.get(p).cloned()).collect();
        let mut texts = blobs(repo, &ids, &self.env())?.into_iter();
        Ok(paths
            .iter()
            .map(|p| if blob_of.contains_key(p) { texts.next().flatten() } else { None })
            .collect())
    }
}

/// The blobs `ids` through one `git cat-file --batch`, as text: None for
/// one that is not there, or is not text.
fn blobs(repo: &Path, ids: &[String], env: &[(&str, &Path)]) -> Result<Vec<Option<String>>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(repo)
        .envs(env.iter().map(|(k, v)| (*k, *v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error(format!("git: {e}")))?;
    let input: String = ids.iter().map(|id| format!("{id}\n")).collect();
    let mut stdin = child.stdin.take().unwrap();
    // Written meanwhile: git answers as it reads.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().map_err(|e| Error(format!("git: {e}")))?;
    let _ = writer.join();
    let b = out.stdout;
    let mut at = 0;
    let mut texts = Vec::new();
    for _ in ids {
        let Some(end) = b[at.min(b.len())..].iter().position(|&c| c == b'\n') else { break };
        let header = String::from_utf8_lossy(&b[at..at + end]).into_owned();
        at += end + 1;
        let size = match header.split(' ').collect::<Vec<_>>()[..] {
            [_, "blob", size] => size.parse::<usize>().unwrap_or(0),
            _ => {
                texts.push(None);
                continue;
            }
        };
        let body = &b[at.min(b.len())..(at + size).min(b.len())];
        at += size + 1;
        texts.push((!body.contains(&0)).then(|| String::from_utf8_lossy(body).into_owned()));
    }
    texts.resize(ids.len(), None);
    Ok(texts)
}

/// Applies `patches` in turn to an index holding `paths` of `base`, or all
/// of it: all at once, as one series, and only if that fails one by one,
/// to tell which. `git apply` changes nothing when it fails, so the one by
/// one start from the base.
fn try_apply(repo: &Path, base: &str, paths: Option<&BTreeSet<String>>, patches: &[&Patch]) -> Result<()> {
    let ix = Index::new(repo, base, paths)?;
    let series: String = patches.iter().map(|p| p.diff.as_str()).collect();
    if ix.apply(repo, &series).is_ok() {
        return Ok(());
    }
    for (i, p) in patches.iter().enumerate() {
        ix.apply(repo, &p.diff).map_err(|e| {
            // git's first word on it, as `patch failed: a.txt:12`.
            let said = e.0.split_once(": ").map_or(e.0.as_str(), |(_, w)| w);
            let why = said.lines().next().unwrap_or("").trim_start_matches("error: ");
            let on = if i == 0 {
                format!("its base, {}", short(base))
            } else {
                "the patches below it".to_string()
            };
            Error(format!("patch {} ({}) does not apply to {on} ({why})", i + 1, p.title()))
        })?;
    }
    Ok(())
}

/// An index of its own on a base, holding the files some diffs touch, as
/// the check's: diffs applied to it in turn, and files read from it, the
/// repository only read. What resolving a conflict by hand starts from is
/// found there. A failure on it is not checked again against the whole
/// base: what it finds is only offered, and what is then written is
/// checked as anything is.
pub struct Pad {
    repo: PathBuf,
    ix: Index,
}

impl Pad {
    /// A pad on `base` in `repo`, holding the files `diffs` touch, and the
    /// files at `also`, as the base has them.
    pub fn new(repo: &Path, base: &str, diffs: &[&str], also: &[&str]) -> Result<Pad> {
        #[cfg(feature = "demo")]
        if crate::memory::tree(repo, base).is_some() {
            return err("a repository kept in memory has no index to apply to");
        }
        reachable(repo, base)?;
        let mut paths = touched_by(repo, diffs);
        paths.extend(also.iter().map(|p| p.to_string()));
        let ix = Index::new(repo, base, Some(&paths))?;
        Ok(Pad { repo: repo.to_path_buf(), ix })
    }

    /// Applies `diff`, all of it or, when it does not apply, none of it:
    /// whether it did.
    pub fn apply(&self, diff: &str) -> bool {
        self.ix.apply(&self.repo, diff).is_ok()
    }

    /// Applies `diffs` in turn, each on its own, as a series is: the first
    /// that does not apply, by place, if one does not.
    pub fn apply_each(&self, diffs: &[&str]) -> std::result::Result<(), usize> {
        self.ix.apply_each(&self.repo, diffs).map_err(|(k, _)| k)
    }

    /// Whether `diff` would apply, nothing changed.
    pub fn would_apply(&self, diff: &str) -> bool {
        self.ix.would_apply(&self.repo, diff)
    }

    /// The file at `path`: its text, None where it is not there or is not
    /// text.
    pub fn text(&self, path: &str) -> Result<Option<String>> {
        Ok(self.ix.texts(&self.repo, &[path.to_string()])?.pop().flatten())
    }
}

/// The file the last of `patches` changes, as it is before it (at
/// `old_path`) and after it (at `path`): its text, None where it is not
/// there or is not text. Read as `versions` reads them.
pub fn texts(repo: &Path, base: &str, patches: &[&Patch], old_path: &str, path: &str) -> Result<(Option<String>, Option<String>)> {
    let Some((last, below)) = patches.split_last() else {
        return Ok((None, None));
    };
    #[cfg(feature = "demo")]
    if let Some(tree) = crate::memory::tree(repo, base) {
        let mut tree = tree?;
        for p in below {
            crate::memory::apply(&mut tree, &p.diff)?;
        }
        let before = tree.get(old_path).cloned();
        crate::memory::apply(&mut tree, &last.diff)?;
        return Ok((before, tree.get(path).cloned()));
    }
    reachable(repo, base)?;
    let ix = Index::new(repo, base, Some(&touched(repo, patches)))?;
    ix.apply_each(repo, &below.iter().map(|p| p.diff.as_str()).collect::<Vec<_>>()).map_err(|(_, e)| e)?;
    let before = ix.texts(repo, &[old_path.to_string()])?.pop().flatten();
    ix.apply(repo, &last.diff)?;
    let after = ix.texts(repo, &[path.to_string()])?.pop().flatten();
    Ok((before, after))
}

/// A file a patch changes, as it is before and after: its lines, None
/// where it is not there (added, deleted) or is not text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sides {
    pub old: Option<Vec<String>>,
    pub new: Option<Vec<String>>,
}

/// The files the last of `patches` changes, by the path it names after,
/// before and after it: on `base` with the patches below applied, in an
/// index of its own as `applies` does. The repository is only read.
pub fn versions(repo: &Path, base: &str, patches: &[&Patch]) -> Result<HashMap<String, Sides>> {
    let Some((last, below)) = patches.split_last() else {
        return Ok(HashMap::new());
    };
    #[cfg(feature = "demo")]
    if let Some(tree) = crate::memory::tree(repo, base) {
        let mut tree = tree?;
        for p in below {
            crate::memory::apply(&mut tree, &p.diff)?;
        }
        let files = crate::diff::parse_diff(&last.diff, "");
        let lines = |t: &crate::memory::Tree, path: &str| t.get(path).map(|t| t.lines().map(String::from).collect());
        let before: Vec<Option<Vec<String>>> = files.iter().map(|f| lines(&tree, &f.path)).collect();
        crate::memory::apply(&mut tree, &last.diff)?;
        return Ok(files
            .iter()
            .zip(before)
            .map(|(f, old)| (f.path.clone(), Sides { old, new: lines(&tree, &f.path) }))
            .collect());
    }
    reachable(repo, base)?;
    let ix = Index::new(repo, base, Some(&touched(repo, patches)))?;
    ix.apply_each(repo, &below.iter().map(|p| p.diff.as_str()).collect::<Vec<_>>()).map_err(|(_, e)| e)?;
    let files = crate::diff::parse_diff(&last.diff, "");
    let new: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let old: Vec<String> = files
        .iter()
        .map(|f| {
            f.header
                .iter()
                .find_map(|l| l.strip_prefix("rename from ").or_else(|| l.strip_prefix("copy from ")))
                .map_or(f.path.clone(), |p| unquote(p, false))
        })
        .collect();
    let before = ix.files(repo, &old)?;
    ix.apply(repo, &last.diff)?;
    let after = ix.files(repo, &new)?;
    Ok(new
        .into_iter()
        .zip(before.into_iter().zip(after))
        .map(|(path, (old, new))| (path, Sides { old, new }))
        .collect())
}

/// Every path the patches name, before or after: as git reads them, and
/// from their headers.
fn touched(repo: &Path, patches: &[&Patch]) -> BTreeSet<String> {
    touched_by(repo, &patches.iter().map(|p| p.diff.as_str()).collect::<Vec<_>>())
}

/// Every path the `diffs` name, as `touched` reads them.
fn touched_by(repo: &Path, diffs: &[&str]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let all: String = diffs.join("\n");
    if let Ok(out) = crate::git::git_in(repo, &["apply", "--numstat", "-z"], Some(&all)) {
        for entry in out.split('\0') {
            if let Some(path) = entry.splitn(3, '\t').nth(2).filter(|p| !p.is_empty()) {
                paths.insert(path.to_string());
            }
        }
    }
    // A line of a hunk can look like a header (a removed `-- x` is
    // `--- x`): a path too many only costs a lookup.
    for line in all.lines() {
        let side = ["--- ", "+++ "].iter().find_map(|h| line.strip_prefix(h));
        let moved = ["rename from ", "rename to ", "copy from ", "copy to "]
            .iter()
            .find_map(|h| line.strip_prefix(h));
        if let Some(p) = side {
            let p = p.trim_end_matches('\t');
            if p.starts_with('"') {
                paths.insert(unquote(p, true));
            } else if let Some(p) = p.strip_prefix("a/").or_else(|| p.strip_prefix("b/")) {
                paths.insert(p.to_string());
            }
        } else if let Some(p) = moved {
            paths.insert(unquote(p, false));
        } else if let Some(rest) = line.strip_prefix("diff --git ") {
            // `a/X b/X`, when both sides are the same path.
            let rest = rest.trim_end();
            if rest.len() % 2 == 1 {
                let half = (rest.len() - 1) / 2;
                if let (Some(a), Some(b)) = (rest.get(..half), rest.get(half + 1..))
                    && let (Some(a), Some(b)) = (a.strip_prefix("a/"), b.strip_prefix("b/"))
                    && a == b
                {
                    paths.insert(a.to_string());
                }
            }
        }
    }
    paths
}

/// A path as git writes it in a header: bare, or quoted as C would,
/// `"a/caf\303\251"`. `sided`: it has the `a/` or `b/` of its side.
fn unquote(p: &str, sided: bool) -> String {
    let Some(inner) = p.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return p.to_string();
    };
    let mut out = Vec::new();
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        if let Some(o) = b.get(i + 1..i + 4).filter(|o| o.iter().all(|d| (b'0'..=b'7').contains(d))) {
            out.push(u8::from_str_radix(std::str::from_utf8(o).unwrap(), 8).unwrap_or(b'?'));
            i += 4;
            continue;
        }
        out.push(match b[i + 1] {
            b'a' => 7,
            b'b' => 8,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 11,
            b'f' => 12,
            b'r' => b'\r',
            c => c,
        });
        i += 2;
    }
    let s = String::from_utf8_lossy(&out).into_owned();
    match s.get(..2) {
        Some("a/") | Some("b/") if sided => s[2..].to_string(),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "a")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "a")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    fn patch(diff: String) -> Patch {
        Patch::new("c".into(), None, None, "p".into(), diff)
    }

    /// A base with a.txt, k.txt and o.txt, and a commit changing a.txt and
    /// renaming o.txt: its diff, against the base.
    fn repo() -> (tempfile::TempDir, PathBuf, String, Patch) {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path().canonicalize().unwrap();
        git_ok(&r, &["init", "-q", "-b", "main"]);
        git_ok(&r, &["config", "commit.gpgsign", "false"]);
        std::fs::write(r.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(r.join("k.txt"), "keep\n").unwrap();
        std::fs::write(r.join("o.txt"), "old\n").unwrap();
        git_ok(&r, &["add", "."]);
        git_ok(&r, &["commit", "-qm", "base"]);
        let base = git_ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
        std::fs::write(r.join("a.txt"), "one\n2\n").unwrap();
        git_ok(&r, &["mv", "o.txt", "n.txt"]);
        git_ok(&r, &["commit", "-qam", "change"]);
        let diff = git_ok(&r, &["diff", "-M", &base, "HEAD"]);
        (tmp, r, base, patch(diff))
    }

    #[test]
    fn the_index_holds_only_what_is_touched() {
        let (_tmp, r, base, p) = repo();
        let paths = touched(&r, &[&p]);
        assert!(["a.txt", "o.txt", "n.txt"].iter().all(|f| paths.contains(*f)), "{paths:?}");
        assert!(!paths.contains("k.txt"), "{paths:?}");
        // It applies on what the cut-down index holds, without the whole
        // tree to fall back on.
        try_apply(&r, &base, Some(&paths), &[&p]).unwrap();
        // And applied twice, it does not.
        let e = try_apply(&r, &base, Some(&paths), &[&p, &p]).unwrap_err();
        assert!(e.0.contains("patch 2 (p) does not apply to the patches below it"), "{}", e.0);
    }

    #[test]
    fn the_repository_is_only_read() {
        let (_tmp, r, base, p) = repo();
        let objects = || git_ok(&r, &["count-objects", "-v"]);
        let status = || git_ok(&r, &["status", "--porcelain"]);
        let (before, clean) = (objects(), status());
        let changed = patch(p.diff.replace("+2", "+zwei"));
        applies(&r, &base, &[&changed]).unwrap();
        assert_eq!(objects(), before, "no blob of the patched files is written");
        assert_eq!(status(), clean);
        assert!(reachable(&r, &"0".repeat(40)).is_err());
        let e = applies(&r, &base, &[&patch(String::new())]).unwrap_err();
        assert!(e.0.contains("has no diff"), "{}", e.0);
    }

    #[test]
    fn reads_both_versions_of_what_a_patch_changes() {
        let (_tmp, r, base, p) = repo();
        let lines = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let v = versions(&r, &base, &[&p]).unwrap();
        assert_eq!(v["a.txt"], Sides { old: lines(&["one", "two"]), new: lines(&["one", "2"]) });
        // A rename reads the old path before, the new one after.
        assert_eq!(v["n.txt"], Sides { old: lines(&["old"]), new: lines(&["old"]) });
        // On a patch below: the version it left.
        let next = patch("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-2\n+3\n".into());
        let v = versions(&r, &base, &[&p, &next]).unwrap();
        assert_eq!(v["a.txt"], Sides { old: lines(&["one", "2"]), new: lines(&["one", "3"]) });
        // An added file has no old side.
        let add = patch("diff --git a/z.txt b/z.txt\nnew file mode 100644\n--- /dev/null\n+++ b/z.txt\n@@ -0,0 +1 @@\n+z\n".into());
        let v = versions(&r, &base, &[&add]).unwrap();
        assert_eq!(v["z.txt"], Sides { old: None, new: lines(&["z"]) });
    }

    #[test]
    fn a_pad_takes_diffs_in_turn_and_tells_which_do_not_apply() {
        let (_tmp, r, base, p) = repo();
        let two = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+zwei\n";
        let pad = Pad::new(&r, &base, &[&p.diff, two], &[]).unwrap();
        assert_eq!(pad.text("a.txt").unwrap().as_deref(), Some("one\ntwo\n"));
        assert!(pad.would_apply(two) && pad.would_apply(&p.diff));
        assert!(pad.apply(&p.diff));
        assert_eq!(pad.text("a.txt").unwrap().as_deref(), Some("one\n2\n"));
        assert_eq!(pad.text("n.txt").unwrap().as_deref(), Some("old\n"));
        assert_eq!(pad.text("o.txt").unwrap(), None, "renamed");
        // What no longer applies on it is not taken, nothing of it.
        assert!(!pad.would_apply(two) && !pad.apply(&p.diff));
        assert_eq!(pad.text("a.txt").unwrap().as_deref(), Some("one\n2\n"));
    }

    #[test]
    fn unquotes_paths() {
        assert_eq!(unquote("plain/path.rs", false), "plain/path.rs");
        assert_eq!(unquote("\"a/sp\\tace\"", true), "sp\tace");
        assert_eq!(unquote("\"b/caf\\303\\251.txt\"", true), "café.txt");
        assert_eq!(unquote("\"a/x\\\"y\"", false), "a/x\"y");
    }
}
