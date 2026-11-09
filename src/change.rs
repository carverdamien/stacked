//! The change from one version of a patch to another, as a patch of its
//! own: each file either version touches, as the old version leaves it on
//! its own base, then as the new one leaves it on its own, as a diff
//! (`read`); nothing is made again on another base. Where the two bases
//! differ, the lines of that diff that the change of base made, and neither
//! patch, are told apart, for the views to draw dim (`Change::dim`). The
//! files are read from the repository alone, as `3` reads a patch's files
//! whole (`check::around`), away from the screen (`Reader`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::check::{Left, Sides};
use crate::diff::{parse_diff, unquote};

/// A version of a patch on its own base, as its files are read: the
/// repository; the commit its base is, and the diffs of the patches below
/// it, bottom first, applied on that in turn; and its own diff, applied on
/// those. With `or_alone`, where they do not apply so, its own diff on the
/// base alone: a revision's latest diff on Phabricator, whose base may be
/// the commit under the whole series or its parent's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    pub repo: PathBuf,
    pub base: String,
    pub below: Vec<String>,
    pub diff: String,
    pub or_alone: bool,
}

/// The change from one version of a patch to another (`read`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Change {
    /// Each file either version touches that the two leave otherwise, from
    /// the old version's to the new one's, as git writes a diff; empty
    /// where they leave every one of them the same.
    pub diff: String,
    /// The lines of `diff`, by their places there, from 0, that the change
    /// of base made rather than either patch: a line taken out that the old
    /// version's base has and the new one's has not, which the old version
    /// keeps; a line put in that the new version's base alone has, which the
    /// new version keeps. Neither where the other version takes out a line
    /// of the base there (`changed`).
    pub dim: HashSet<usize>,
    /// Whether the two bases differ in a file either version touches.
    pub rebased: bool,
    /// Each file of `diff`, by its path, as the old version leaves it and as
    /// the new one does: what `3` shows it whole from.
    pub files: Arc<crate::whole::Files>,
}

/// How long working out what of a file the change of base made may take:
/// past it, the lines are matched as far as they were.
const MATCHED_IN: Duration = Duration::from_secs(2);

/// The change from version `old` to version `new`, each on its own base,
/// named `names` (the old one's, the new one's) where it does not apply:
/// the files either touches (`check::paths_of`), the paths they rename
/// from among them, read before and after each version, as each leaves
/// them on its own base (`check::around`), then diffed (`changed`). A file
/// only the change of base touches is left out. Why not, when a version
/// does not apply on its base, or the files cannot be read.
pub fn read(old: &Side, new: &Side, names: (&str, &str)) -> Result<Change, String> {
    let mut paths: BTreeSet<String> = crate::check::paths_of(&old.repo, &[old.diff.as_str()]);
    paths.extend(crate::check::paths_of(&new.repo, &[new.diff.as_str()]));
    let paths: Vec<String> = paths.into_iter().collect();
    let around = |s: &Side, name: &str| -> Result<(Vec<Left>, Vec<Left>), String> {
        let below: Vec<&str> = s.below.iter().map(String::as_str).collect();
        let mut read = crate::check::around(&s.repo, &s.base, &below, &s.diff, &paths).map_err(|e| e.0)?;
        if read.is_none() && s.or_alone && !below.is_empty() {
            read = crate::check::around(&s.repo, &s.base, &[], &s.diff, &paths).map_err(|e| e.0)?;
        }
        read.ok_or_else(|| {
            let base = &s.base[..12.min(s.base.len())];
            match s.below.len() {
                0 => format!("{name} does not apply on its base, {base}"),
                _ => format!("{name} does not apply on its base, {base}, with the patches below it"),
            }
        })
    };
    let (a, b) = around(old, names.0)?;
    let (a2, c) = around(new, names.1)?;
    Ok(changed(&paths, (&a, &b, &old.diff), (&a2, &c, &new.diff)))
}

/// The lines of file `l`, none where it is not there or is not text.
fn text_lines(l: &Left) -> Vec<&str> {
    l.as_ref().and_then(|(_, t)| t.as_deref()).map_or_else(Vec::new, |t| t.lines().collect())
}

/// The lines of file `l` as `whole` shows them: none where it is not there
/// or is not text.
fn shown_lines(l: &Left) -> Option<Vec<String>> {
    l.as_ref().and_then(|(_, t)| t.as_deref()).map(|t| t.lines().map(String::from).collect())
}

/// How the lines of `a` are those of `b`, as a diff matches them, in
/// order: each op, its tag and the lines of each it takes.
fn matched<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(similar::DiffTag, Range<usize>, Range<usize>)> {
    similar::capture_diff_slices_deadline(similar::Algorithm::Myers, a, b, Some(Instant::now() + MATCHED_IN))
        .iter()
        .map(|op| op.as_tag_tuple())
        .collect()
}

/// What a version's files are of its base: each line of each file after
/// it, by the file's place among the paths, the line of the base it is,
/// the base file's place and its line, from 0, or none for a line the
/// version puts in; and the lines of each file of the base it takes out.
struct Base {
    of: Vec<Vec<Option<(usize, usize)>>>,
    taken: Vec<HashSet<usize>>,
}

/// Of a version whose diff is `diff`, its files at `paths` (each by its
/// place, `at`) before it, `before`, and after it, `after`: what each line
/// after it is of the base (`Base`), each file matched with the one it is
/// renamed or copied from, as the diff says, else with itself. A file
/// renamed away takes none of its lines out: they go with it.
fn of_base(paths: &[String], at: &HashMap<&str, usize>, before: &[Left], after: &[Left], diff: &str) -> Base {
    let mut from: HashMap<String, String> = HashMap::new();
    for fd in parse_diff(diff, "") {
        if let Some(src) = fd.header.iter().find_map(|l| l.strip_prefix("rename from ").or_else(|| l.strip_prefix("copy from "))) {
            from.insert(fd.path.clone(), unquote(src, false));
        }
    }
    let away: HashSet<&str> = from.values().map(String::as_str).collect();
    let mut out = Base { of: vec![Vec::new(); paths.len()], taken: vec![HashSet::new(); paths.len()] };
    for (k, path) in paths.iter().enumerate() {
        if after[k].is_none() && away.contains(path.as_str()) {
            continue;
        }
        let j = from.get(path).and_then(|s| at.get(s.as_str())).copied().unwrap_or(k);
        let (x, y) = (text_lines(&before[j]), text_lines(&after[k]));
        let mut of = vec![None; y.len()];
        for (tag, o, n) in matched(&x, &y) {
            match tag {
                similar::DiffTag::Equal => {
                    for (i, m) in o.zip(n) {
                        of[m] = Some((j, i));
                    }
                }
                _ => out.taken[j].extend(o),
            }
        }
        out.of[k] = of;
    }
    out
}

/// How one base's file, `a`, became the other's, `b`: its hunks, each the
/// lines of each it holds, as a diff of the two matches their lines; and,
/// by line, the hunk each line of `a` the other has not is in, and each of
/// `b` the one has not.
#[derive(Default)]
struct Rebased {
    hunks: Vec<(Range<usize>, Range<usize>)>,
    gone: HashMap<usize, usize>,
    came: HashMap<usize, usize>,
}

/// What became of file `a` of one base in `b` of the other (`Rebased`).
fn rebased(a: &[&str], b: &[&str]) -> Rebased {
    let mut out = Rebased::default();
    let mut open: Option<(Range<usize>, Range<usize>)> = None;
    for (tag, o, n) in matched(a, b) {
        if tag == similar::DiffTag::Equal {
            out.hunks.extend(open.take());
            continue;
        }
        open = Some(match open.take() {
            Some((x, y)) => (x.start..o.end, y.start..n.end),
            None => (o, n),
        });
    }
    out.hunks.extend(open);
    for (h, (x, y)) in out.hunks.iter().enumerate() {
        out.gone.extend(x.clone().map(|i| (i, h)));
        out.came.extend(y.clone().map(|i| (i, h)));
    }
    out
}

/// The change, from the files at `paths` as each version leaves them: of
/// the old one, its files before it, `a`, and after, `b`, and its diff; of
/// the new one, `a2`, `c` and its diff. Each file the two leave otherwise,
/// from `b` to `c`, as git writes a diff (`check::file_diff`), and its two
/// sides, for `3`. Where the bases differ in a file (`a`, `a2`), a line of
/// the diff the change of base made (`Change::dim`): taken out, a line of
/// `b` that the old version keeps of its base, and that the new base has
/// not, while the new version takes out nothing of what the new base has in
/// its place; put in, a line of `c` that the new version keeps of its base,
/// and that the old base has not, while the old version takes out nothing
/// of what the old base has in its place. Every line either patch puts in,
/// or takes out, is its own.
fn changed(paths: &[String], (a, b, old): (&[Left], &[Left], &str), (a2, c, new): (&[Left], &[Left], &str)) -> Change {
    let at: HashMap<&str, usize> = paths.iter().enumerate().map(|(k, p)| (p.as_str(), k)).collect();
    let diff: String = paths.iter().enumerate().filter_map(|(k, path)| crate::check::file_diff(path, &b[k], &c[k])).collect();
    let rebased_in: Vec<bool> = (0..paths.len()).map(|k| a[k] != a2[k]).collect();
    let (was, is) = (of_base(paths, &at, a, b, old), of_base(paths, &at, a2, c, new));
    let bases: Vec<Rebased> = (0..paths.len()).map(|k| if rebased_in[k] { rebased(&text_lines(&a[k]), &text_lines(&a2[k])) } else { Rebased::default() }).collect();
    let none_of = |taken: &HashSet<usize>, r: &Range<usize>| !r.clone().any(|i| taken.contains(&i));
    let mut dim = HashSet::new();
    let mut files = HashMap::new();
    for fd in parse_diff(&diff, "") {
        let Some(&k) = at.get(fd.path.as_str()) else { continue };
        files.insert(fd.path.clone(), Sides { old: shown_lines(&b[k]), new: shown_lines(&c[k]) });
        for l in fd.hunks.iter().flat_map(|h| &h.lines) {
            let made = match l.origin {
                '-' => was.of[k].get(l.old_no.wrapping_sub(1)).copied().flatten().is_some_and(|(j, x)| {
                    let r = &bases[j];
                    r.gone.get(&x).is_some_and(|&h| none_of(&is.taken[j], &r.hunks[h].1))
                }),
                '+' => is.of[k].get(l.new_no.wrapping_sub(1)).copied().flatten().is_some_and(|(j, y)| {
                    let r = &bases[j];
                    r.came.get(&y).is_some_and(|&h| none_of(&was.taken[j], &r.hunks[h].0))
                }),
                _ => false,
            };
            if made {
                dim.insert(l.at);
            }
        }
    }
    Change { diff, dim, rebased: rebased_in.contains(&true), files: Arc::new(files) }
}

/// What reading a change came to: the change, or why it could not be read.
pub type Read = Result<Arc<Change>, String>;

/// How many changes are read at once, each in a thread of its own: the
/// others wait their turn.
const AT_ONCE: usize = 2;

/// What a change is read from: the two versions, and their names.
type Job = (Side, Side, [String; 2]);

/// The changes read, by a key of what they are read from, kept for as long
/// as the editor runs; those being read, each in a thread of its own, a few
/// at once (`AT_ONCE`), and those waiting their turn; and how many reads
/// have ended, for what is drawn of them to be drawn again. Asked from what
/// draws, which only reads the editor.
#[derive(Default)]
pub struct Reader {
    read: RefCell<HashMap<String, Read>>,
    going: RefCell<HashMap<String, std::thread::JoinHandle<Read>>>,
    waiting: RefCell<Vec<(String, Job)>>,
    ended: Cell<u64>,
}

impl Reader {
    /// The change of `key`, read by now, or why it could not be; else it is
    /// read from what `job` gives, unless it is being read already, or
    /// waiting its turn, and None meanwhile.
    pub fn get(&self, key: &str, job: impl FnOnce() -> Job) -> Option<Read> {
        self.reap();
        if let Some(r) = self.read.borrow().get(key) {
            return Some(r.clone());
        }
        if self.going.borrow().contains_key(key) || self.waiting.borrow().iter().any(|(k, _)| k == key) {
            return None;
        }
        self.waiting.borrow_mut().push((key.to_string(), job()));
        self.start();
        None
    }

    /// The reads that ended taken in: how many have, in all.
    pub fn ended(&self) -> u64 {
        self.reap();
        self.ended.get()
    }

    /// Waits for every read under way, and every one waiting its turn, to
    /// end.
    pub fn wait(&self) {
        loop {
            let going: Vec<(String, std::thread::JoinHandle<Read>)> = self.going.borrow_mut().drain().collect();
            if going.is_empty() {
                return;
            }
            for (key, h) in going {
                self.read.borrow_mut().insert(key, h.join().unwrap_or_else(|_| Err("reading them failed".into())));
                self.ended.set(self.ended.get() + 1);
            }
            self.start();
        }
    }

    /// The reads that ended, taken in, and the next ones started.
    fn reap(&self) {
        let done: Vec<String> = self.going.borrow().iter().filter(|(_, h)| h.is_finished()).map(|(k, _)| k.clone()).collect();
        for key in done {
            let Some(h) = self.going.borrow_mut().remove(&key) else { continue };
            self.read.borrow_mut().insert(key, h.join().unwrap_or_else(|_| Err("reading them failed".into())));
            self.ended.set(self.ended.get() + 1);
        }
        self.start();
    }

    /// The reads waiting their turn started, the first first, as long as
    /// fewer than `AT_ONCE` are under way.
    fn start(&self) {
        while self.going.borrow().len() < AT_ONCE {
            let next = {
                let mut waiting = self.waiting.borrow_mut();
                if waiting.is_empty() {
                    return;
                }
                waiting.remove(0)
            };
            let (key, (old, new, names)) = next;
            let h = std::thread::spawn(move || read(&old, &new, (&names[0], &names[1])).map(Arc::new));
            self.going.borrow_mut().insert(key, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of these lines, as the check reads one.
    fn file(lines: &[&str]) -> Left {
        Some(("100644".into(), Some(lines.iter().map(|l| format!("{l}\n")).collect())))
    }

    /// Each line of the change's diff under its file's header, as it is
    /// written, `~` before one the change of base made.
    fn said(c: &Change) -> Vec<String> {
        let mut out = Vec::new();
        for fd in parse_diff(&c.diff, "") {
            for l in fd.hunks.iter().flat_map(|h| &h.lines) {
                out.push(format!("{}{}", if c.dim.contains(&l.at) { "~" } else { "" }, l.text));
            }
        }
        out
    }

    /// The change of one file, `f`, from `old` (its base `a`, as it leaves
    /// it `b`) to `new` (`a2`, `c`), each version's diff named only by the
    /// file it touches.
    fn one(a: &[&str], b: &[&str], a2: &[&str], c: &[&str]) -> Change {
        let paths = vec!["f".to_string()];
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n";
        changed(&paths, (&[file(a)], &[file(b)], diff), (&[file(a2)], &[file(c)], diff))
    }

    #[test]
    fn on_one_base_the_change_is_each_file_from_one_version_to_the_other_and_nothing_is_dim() {
        let base = ["a", "b", "c"];
        let c = one(&base, &["a", "B1", "c"], &base, &["a", "B2", "c", "d"]);
        assert!(!c.rebased && c.dim.is_empty(), "{c:?}");
        assert_eq!(said(&c), [" a", "-B1", "+B2", " c", "+d"]);
        // Each file as each version leaves it, for 3.
        let sides = &c.files["f"];
        assert_eq!((sides.old.as_deref().map(<[String]>::len), sides.new.as_deref().map(<[String]>::len)), (Some(3), Some(4)));
        // The same files: no change.
        assert_eq!(one(&base, &["a", "B", "c"], &base, &["a", "B", "c"]).diff, "");
    }

    #[test]
    fn across_a_change_of_base_the_lines_it_made_are_dim_and_the_patches_lines_are_not() {
        // The base changes `two` into `TWO`, far from both patches, which
        // put in `x` and then `y` at the top.
        let a = ["one", "two", "three", "four", "five", "six", "seven"];
        let a2 = ["one", "TWO", "three", "four", "five", "six", "seven"];
        let b = ["x", "one", "two", "three", "four", "five", "six", "seven"];
        let c = ["y", "one", "TWO", "three", "four", "five", "six", "seven"];
        let ch = one(&a, &b, &a2, &c);
        assert!(ch.rebased);
        assert_eq!(said(&ch), ["-x", "+y", " one", "~-two", "~+TWO", " three", " four", " five"]);
    }

    #[test]
    fn where_a_patch_takes_out_a_line_the_change_of_base_touched_nothing_there_is_dim() {
        let a = ["one", "two", "three"];
        let a2 = ["one", "TWO", "three"];
        // The old version takes `two` out; the new one keeps `TWO`: the new
        // version no longer takes the line out, which is its own.
        let ch = one(&a, &["one", "three"], &a2, &a2);
        assert_eq!(said(&ch), [" one", "+TWO", " three"]);
        // The old version keeps `two`; the new one takes `TWO` out.
        let ch = one(&a, &a, &a2, &["one", "three"]);
        assert_eq!(said(&ch), [" one", "-two", " three"]);
        // Both take it out: nothing differs.
        assert_eq!(one(&a, &["one", "three"], &a2, &["one", "three"]).diff, "");
        // Both keep it: the change of base's alone.
        assert_eq!(said(&one(&a, &a, &a2, &a2)), [" one", "~-two", "~+TWO", " three"]);
        // A line each puts in beside it: its own, the base's dim.
        assert_eq!(said(&one(&a, &["one", "two", "x", "three"], &a2, &["one", "TWO", "y", "three"])), [" one", "~-two", "-x", "~+TWO", "+y", " three"]);
    }

    #[test]
    fn a_line_moved_into_a_patch_below_or_out_of_one_is_no_change() {
        let a = ["one", "two"];
        // The old version puts `z` in; the new one finds it in its base,
        // which a patch below now puts in, and keeps it.
        let ch = one(&a, &["one", "z", "two"], &["one", "z", "two"], &["one", "z", "two", "w"]);
        assert_eq!(said(&ch), [" one", " z", " two", "+w"]);
        // Out of one: the old base has `z`, the new one has not, and the new
        // version puts it in itself.
        let ch = one(&["one", "z", "two"], &["one", "z", "two"], &a, &["one", "z", "two"]);
        assert_eq!(ch.diff, "", "{ch:?}");
        // Moved below, then changed there: the old version's line is its
        // own, the one of the new base dim.
        let ch = one(&a, &["one", "z", "two"], &["one", "Z", "two"], &["one", "Z", "two"]);
        assert_eq!(said(&ch), [" one", "-z", "~+Z", " two"]);
    }

    #[test]
    fn a_file_the_change_of_base_adds_deletes_or_renames_and_one_only_a_version_touches() {
        let paths: Vec<String> = ["f", "g"].map(String::from).to_vec();
        let touch = |p: &str| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n");
        // The new base renames `f` to `g`, which the new version changes
        // there; the old version changed `f`.
        let (a, b) = ([file(&["k", "l"]), None], [file(&["k", "L"]), None]);
        let (a2, c) = ([None, file(&["k", "l"])], [None, file(&["k", "l", "m"])]);
        let ch = changed(&paths, (&a, &b, &touch("f")), (&a2, &c, &touch("g")));
        // `f` gone: the old version's line its own, the line of its base
        // the new base has not dim; `g` come: the new base's lines dim, the
        // new version's own not.
        assert_eq!(said(&ch), ["~-k", "-L", "~+k", "~+l", "+m"]);
        assert!(ch.diff.contains("deleted file mode 100644") && ch.diff.contains("new file mode 100644"), "{}", ch.diff);
        // A file the new version alone touches: the old version leaves it as
        // its base has it, on one base.
        let (a, b) = ([file(&["k"]), file(&["p"])], [file(&["K"]), file(&["p"])]);
        let (a2, c) = ([file(&["k"]), file(&["p"])], [file(&["K"]), file(&["p", "q"])]);
        let ch = changed(&paths, (&a, &b, &touch("f")), (&a2, &c, &touch("g")));
        assert_eq!((said(&ch), ch.dim.is_empty()), (vec![" p".to_string(), "+q".to_string()], true));
        // The old version renames `f` to `g`, the new one does not: `f`
        // put in again, `g` taken out, all of it its own.
        let renamed = "diff --git a/f b/g\nsimilarity index 100%\nrename from f\nrename to g\n";
        let ch = changed(&paths, (&[file(&["k"]), None], &[None, file(&["k"])], renamed), (&[file(&["k"]), None], &[file(&["k"]), None], &touch("f")));
        assert_eq!((said(&ch), ch.dim.is_empty()), (vec!["+k".to_string(), "-k".to_string()], true));
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) -> String {
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

    #[test]
    fn the_files_are_read_from_the_repository_each_version_on_its_own_base_a_file_only_the_base_touches_left_out() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path().canonicalize().unwrap();
        git_ok(&r, &["init", "-q", "-b", "main"]);
        let lines = |n: usize, top: &str| -> String { std::iter::once(top.to_string()).chain((1..n).map(|k| format!("line {k}"))).map(|l| l + "\n").collect() };
        std::fs::write(r.join("a.txt"), lines(9, "start")).unwrap();
        std::fs::write(r.join("other.txt"), "other\n").unwrap();
        git_ok(&r, &["add", "."]);
        git_ok(&r, &["-c", "commit.gpgsign=false", "commit", "-qm", "one"]);
        let one = git_ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
        // The new base: a.txt's last line changed, and a file neither
        // version touches.
        std::fs::write(r.join("a.txt"), lines(9, "start").replace("line 8", "LINE 8")).unwrap();
        std::fs::write(r.join("other.txt"), "OTHER\n").unwrap();
        git_ok(&r, &["-c", "commit.gpgsign=false", "commit", "-qam", "two"]);
        let two = git_ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
        let edit = |base: &str, top: &str| -> String {
            git_ok(&r, &["checkout", "-q", base]);
            std::fs::write(r.join("a.txt"), std::fs::read_to_string(r.join("a.txt")).unwrap().replacen("start", top, 1)).unwrap();
            let d = git_ok(&r, &["diff"]);
            git_ok(&r, &["checkout", "-q", "--", "a.txt"]);
            d
        };
        let (old, new) = (edit(&one, "OLD"), edit(&two, "NEW"));
        let side = |base: &str, diff: &str| Side { repo: r.clone(), base: base.into(), below: vec![], diff: diff.into(), or_alone: false };
        let c = read(&side(&one, &old), &side(&two, &new), ("v1", "v2")).unwrap();
        assert!(c.rebased && !c.diff.contains("other.txt"), "{}", c.diff);
        assert_eq!(said(&c), ["-OLD", "+NEW", " line 1", " line 2", " line 3", " line 5", " line 6", " line 7", "~-line 8", "~+LINE 8"]);
        // On one base, nothing dim.
        let same = read(&side(&two, &old), &side(&two, &new), ("v1", "v2")).unwrap();
        assert!(!same.rebased && same.dim.is_empty() && same.diff.contains("-OLD\n+NEW\n"), "{same:?}");
        // Not on its base: said so, by the name given.
        let e = read(&side(&one, &old.replace("-start", "-nowhere")), &side(&two, &new), ("v1", "v2")).unwrap_err();
        assert!(e.starts_with("v1 does not apply on its base, "), "{e}");
    }
}
