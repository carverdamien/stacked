use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use stacked::db::{Db, Record, RunState};
use stacked::mbox::{Draft, decode_words, parse_mail, parse_mbox};
use stacked::ops;

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8(out.stdout).unwrap()
}

/// Commits and tags in `repo` unsigned, whatever the user's git config.
fn unsigned(repo: &Path) {
    git(repo, &["config", "commit.gpgsign", "false"]);
    git(repo, &["config", "tag.gpgsign", "false"]);
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    src: PathBuf,
    db: PathBuf,
}

/// The test's folder is removed once the test is over, but only once
/// nothing works in it any more: an ask the test left going (its claude,
/// the stacked that records and lands what claude did) would write in the
/// folder as it is removed, and some of it would be left. When the test
/// failed, the folder is removed at once: the test may have left an agent
/// waiting for ever.
impl Drop for Env {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            quiet(&self.root);
        }
    }
}

/// How long a test waits for what it waits for before it fails: long, as
/// the tests of this file run at once, each its asks and its worktrees,
/// and other suites may run meanwhile. Nothing waits that long but a test
/// that fails.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// Looks for what `there` finds, every tenth of a second, until it finds
/// it: that. Once `PATIENCE` is over, the test fails, `never` saying what
/// never came.
fn eventually<T>(mut there: impl FnMut() -> Option<T>, never: impl FnOnce() -> String) -> T {
    let started = std::time::Instant::now();
    loop {
        if let Some(found) = there() {
            return found;
        }
        if started.elapsed() > PATIENCE {
            panic!("{}", never());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Waits until no process works in `dir`, a test's folder: none whose
/// command names what is in it, as those of an ask do. With no `ps` to
/// say, it does not wait.
fn quiet(dir: &Path) {
    let inside = format!("{}/", dir.display());
    let working = || -> Option<String> {
        let ps = Command::new("ps").args(["axww", "-o", "command="]).output().ok()?;
        Some(String::from_utf8_lossy(&ps.stdout).lines().filter(|l| l.contains(&inside)).collect::<Vec<_>>().join("\n"))
    };
    eventually(|| working().is_none_or(|w| w.is_empty()).then_some(()), || format!("still at work in {}:\n{}", dir.display(), working().unwrap_or_default()));
}

const TEN: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";

/// The settings of a test's home: the agent's table alone, which a test
/// adds to. No ask waits for a setup: the first goes at once.
const HOME_SETTINGS: &str = "[agent]\n";

/// `line`, a row of the setup as one with two columns draws it (read only's
/// box from column 1, change's from 12, what the row says from 21), as this
/// stacked draws it: with the `try-push` feature, a column for try pushes
/// at 20, ticked as the read-only one, what the row says from 30, and the
/// columns' heading its own. Any other line, as it is.
fn two(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let cell = |at: usize, w: usize| chars.iter().skip(at).take(w).collect::<String>().trim().to_string();
    let (r, c, text) = (cell(1, 9), cell(12, 6), chars.iter().skip(21).collect::<String>());
    let centred = |t: &str, w: usize| {
        let left = (w - t.chars().count()) / 2;
        format!("{}{t}{}", " ".repeat(left), " ".repeat(w - t.chars().count() - left))
    };
    let lay = |cells: &[&str], cols: &[(usize, usize)], at: usize, text: &str| {
        let mut out = String::new();
        for (t, (start, w)) in cells.iter().zip(cols) {
            out += &" ".repeat(start - out.chars().count());
            out += &centred(t, *w);
        }
        out += &" ".repeat(at - out.chars().count());
        (out + text).trim_end().to_string()
    };
    if !cfg!(feature = "try-push") || lay(&[&r, &c], &[(1, 9), (12, 6)], 21, &text) != line {
        return line.to_string();
    }
    let cols = [(1, 9), (12, 6), (20, 8)];
    match r.as_str() {
        "read only" => lay(
            &["read only", "change", "try push"],
            &cols,
            30,
            "read only is for answer-only asks; change is for asks that change the stack; try push is for asks that select a try push's tasks",
        ),
        _ => lay(&[&r, &c, &r], &cols, 30, &text),
    }
}

/// What the setup says it saves in try_tools, `tools`, after the other
/// lists: nothing without the `try-push` feature.
fn try_said(tools: &str) -> String {
    if cfg!(feature = "try-push") { format!("; try_tools: {tools}") } else { String::new() }
}

/// The line the setup writes for try_tools, `tools` as the file writes
/// them: none without the `try-push` feature.
fn try_line(tools: &str) -> String {
    if cfg!(feature = "try-push") { format!("try_tools = [{tools}]\n") } else { String::new() }
}

/// A repo with a base commit and three on top that apply in any order, the
/// last with a title that is not ASCII and long enough to be folded. The
/// test's home holds settings of its own (`HOME_SETTINGS`).
fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("home")).unwrap();
    std::fs::write(root.join("home/config.toml"), HOME_SETTINGS).unwrap();
    let src = root.join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    unsigned(&src);
    std::fs::write(src.join("a.txt"), TEN).unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    git(&src, &["tag", "base"]);
    std::fs::write(src.join("a.txt"), TEN.replace("two", "2")).unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - change two"]);
    std::fs::write(src.join("b.txt"), "new\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b\n\nA body.\n\nMore body."]);
    std::fs::write(src.join("a.txt"), TEN.replace("two", "2").replace("nine", "9")).unwrap();
    git(
        &src,
        &["commit", "-qam", "Bug 3 - café, a very long title that format-patch will fold over more than one line"],
    );
    let db = root.join("db.jsonl");
    Env { _tmp: tmp, root, src, db }
}

/// A repo of one commit, for the database alone: where it is, and the
/// commit.
fn tiny() -> (tempfile::TempDir, PathBuf, String) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap().join("r");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    unsigned(&repo);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    (tmp, repo, base)
}

/// A diff making `file` hold `line`.
fn new_file(file: &str, line: &str) -> String {
    format!("diff --git a/{file} b/{file}\nnew file mode 100644\n--- /dev/null\n+++ b/{file}\n@@ -0,0 +1 @@\n+{line}\n")
}

/// stacked's own things (its settings, the databases opened last) kept, by
/// a run of it as the tests run it, in `home`, a folder of the test's
/// (`$STACKED_HOME`): never in the home of whoever runs the tests, none of
/// whose files is read, written or moved. A test gives a settings' file of
/// its own as `$STACKED_CONFIG`, over the folder's. The agents' rebase
/// plans are kept in `todo` beside it (`$STACKED_TODO`): never in the
/// temporary directory's folder, which every stacked shares. What else the
/// run writes in the temporary directory (an edit it could not take, kept
/// for `--from`) goes in `tmp`, another folder of the test's beside it
/// (`$TMPDIR`): removed with the test's folder, none of it is left in the
/// temporary directory, which every process shares.
fn kept_in<'a>(cmd: &'a mut Command, home: &Path) -> &'a mut Command {
    let tmp = home.with_file_name("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    cmd.env("STACKED_HOME", home)
        .env_remove("STACKED_CONFIG")
        .env_remove("STACKED_APP_STATE")
        .env("STACKED_TODO", home.with_file_name("todo"))
        .env("TMPDIR", tmp)
}

/// What runs stacked as a run of yours: no override, so that the files of
/// before in `home`, a home of the test's standing in for one of yours,
/// are moved into its folder.
fn as_yours(home: &Path) -> [(&'static str, &Path); 2] {
    [("HOME", home), ("STACKED_HOME", Path::new(""))]
}

/// stacked run in `dir` on the database `db`, its own things kept in
/// `home` beside the database (`kept_in`), with `env` over what the tests
/// set.
fn stacked_in(
    dir: &Path,
    db: &Path,
    args: &[&str],
    stdin: &str,
    env: &[(&str, &Path)],
) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_stacked"));
    kept_in(&mut cmd, &db.with_file_name("home"))
        .current_dir(dir)
        .arg("-f")
        .arg(db)
        .args(args)
        .env_remove("VISUAL")
        .env("EDITOR", "false")
        // No agent of the user's is ever asked: a test asks its stand-in.
        .env("STACKED_CLAUDE", "/usr/bin/false")
        .env("GIT_AUTHOR_NAME", "A U Thor")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "C O Mitter")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

impl Env {
    fn run(&self, args: &[&str]) -> (bool, String, String) {
        stacked_in(&self.src, &self.db, args, "", &[])
    }

    fn ok(&self, args: &[&str]) -> String {
        let (ok, out, err) = self.run(args);
        assert!(ok, "stacked {args:?}: {err}");
        out
    }

    fn column(&self, n: usize) -> Vec<String> {
        self.ok(&["show", "s"])
            .lines()
            .map(|l| l.split('\t').nth(n).unwrap().to_string())
            .collect()
    }

    fn titles(&self) -> Vec<String> {
        self.column(3).iter().map(|t| t.chars().take(9).collect()).collect()
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        Command::new("chmod").arg("+x").arg(&path).status().unwrap();
        path
    }
}

/// What `sed -i '' SCRIPT FILE` does with BSD's sed, as a command of the
/// shell's that any sed runs: GNU's reads `-i ''` otherwise, `''` as its
/// script. `file` is named as the shell reads it (`a.txt`, `$1`), and
/// written anew from what `script` makes of it; in a subshell, the two
/// steps fail as the one command did, which `set -e` stops at.
fn sed_i(script: &str, file: &str) -> String {
    format!("(sed '{script}' \"{file}\" > \"{file}.new\" && mv \"{file}.new\" \"{file}\")")
}

/// A command of the shell's that prints the lines it reads, the last
/// first, as BSD's `tail -r` and GNU's `tac` do: neither has the other's.
const REVERSED: &str = "sed -n '1!G;h;$p'";

#[test]
fn round_trip_through_git() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    assert_eq!(
        e.column(3),
        [
            "Bug 1 - change two",
            "Bug 2 - add b",
            "Bug 3 - café, a very long title that format-patch will fold over more than one line"
        ]
    );
    let one = e.ok(&["show", "s", "2"]);
    assert!(one.contains("Subject: Bug 2 - add b\n\nA body.\n\nMore body.\n---\ndiff --git"), "{one}");
    assert!(one.starts_with("From: A U Thor <author@example.com>\n"), "{one}");

    // Mail on stdin does as well as a range.
    let mail = git(&e.src, &["format-patch", "--stdout", "--base=base", "base..HEAD"]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["import", "t"], &mail, &[]);
    assert!(fine, "{err}");

    let out = e.ok(&["export", "s"]);
    let dst = e.root.join("dst");
    git(&e.root, &["clone", "-q", e.src.to_str().unwrap(), "dst"]);
    git(&dst, &["checkout", "-q", "base"]);
    let mut am = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "am", "-q"])
        .current_dir(&dst)
        .env("GIT_COMMITTER_NAME", "C")
        .env("GIT_COMMITTER_EMAIL", "c@example.com")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    am.stdin.take().unwrap().write_all(out.as_bytes()).unwrap();
    assert!(am.wait().unwrap().success());
    assert_eq!(git(&dst, &["rev-parse", "HEAD^{tree}"]), git(&e.src, &["rev-parse", "HEAD^{tree}"]));
    assert_eq!(
        git(&dst, &["log", "--format=%an <%ae>%n%B", "base..HEAD"]),
        git(&e.src, &["log", "--format=%an <%ae>%n%B", "base..HEAD"])
    );
}

#[test]
fn the_edits_make_a_tree() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["move", "s", "3", "1"]);
    assert_eq!(e.titles(), ["Bug 3 - c", "Bug 1 - c", "Bug 2 - a"]);
    e.ok(&["drop", "s", "2"]);
    assert_eq!(e.titles(), ["Bug 3 - c", "Bug 2 - a"]);
    e.ok(&["undo", "s"]);
    e.ok(&["undo", "s"]);
    assert_eq!(e.titles(), ["Bug 1 - c", "Bug 2 - a", "Bug 3 - c"]);
    let (fine, _, err) = e.run(&["undo", "s"]);
    assert!(!fine && err.contains("nothing to undo"), "{err}");
    e.ok(&["redo", "s"]);
    assert_eq!(e.titles(), ["Bug 3 - c", "Bug 1 - c", "Bug 2 - a"]);

    // An edit from here is a branch: the drop stays in the tree.
    e.ok(&["drop", "s", "1"]);
    assert_eq!(e.titles(), ["Bug 1 - c", "Bug 2 - a"]);
    let tree = e.ok(&["tree", "s"]);
    let lines: Vec<&str> = tree.lines().collect();
    assert_eq!(lines.len(), 4, "{tree}");
    assert!(lines[0].starts_with("● "), "the head, newest first:\n{tree}");
    assert!(lines[1].starts_with("│ ○ "), "the old drop, on a lane of its own:\n{tree}");
    assert!(lines[2].starts_with("○ ┘ "), "both edited from the move:\n{tree}");
    // Any stack of the tree can be picked to go on from.
    let old_drop = lines[1].split('@').nth(1).unwrap().split('\t').next().unwrap();
    e.ok(&["select", old_drop]);
    assert_eq!(e.titles(), ["Bug 3 - c", "Bug 2 - a"]);
    // Undo after a select goes to what that stack was edited from.
    e.ok(&["undo", "s"]);
    assert_eq!(e.titles(), ["Bug 3 - c", "Bug 1 - c", "Bug 2 - a"]);

    e.ok(&["delete", "s"]);
    assert!(!e.ok(&["stacks"]).contains("s\t"));
    e.ok(&["undo", "s"]);
    assert_eq!(e.titles(), ["Bug 3 - c", "Bug 1 - c", "Bug 2 - a"]);
    let log = e.ok(&["log", "s"]);
    assert!(log.contains("→ deleted") && log.contains("undo delete"), "{log}");
}

#[test]
fn todo_list_in_the_editor() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // The editor reverses the list, rewords the patch that was on top and
    // drops the bottom one; then rewrites the message it is given.
    let editor = e.script(
        "editor.sh",
        &format!(
            r#"case "$1" in
*.todo) grep -v '^#' "$1" | grep . | {REVERSED} | sed '1s/^pick/reword/;$s/^pick/drop/' > "$1.new" && mv "$1.new" "$1" ;;
*.txt) printf 'Bug 3 - reworded\n\nNew body.\n' > "$1" ;;
esac
"#
        ),
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s"], "", &[("EDITOR", &editor)]);
    assert!(fine, "{err}");
    // It lands for your review, as the agent's work does: the stack is as
    // it was until it is accepted.
    assert!(err.contains("the stack edited as a list: reword") && err.contains("it waits for your review"), "{err}");
    assert_eq!(e.column(3).len(), 3);
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.contains("a change of the stack") && landed.contains("change\treword"), "{landed}");
    let (fine, _, err) = e.run(&["edit", "s", "1"]);
    assert!(!fine && err.contains("your edit by hand of the stack waits for your review"), "one at a time: {err}");
    e.ok(&["review", "s", "--accept-all"]);
    assert_eq!(e.column(3), ["Bug 3 - reworded", "Bug 2 - add b"]);
    let log = e.ok(&["log", "s"]);
    assert!(log.lines().last().unwrap().contains("reword"), "{log}");
    // The reworded patch is a new version of the old one, its diff the same.
    let versions = e.ok(&["versions", "s", "1"]);
    assert_eq!(versions.lines().count(), 2, "{versions}");
    let inter = e.ok(&["interdiff", "s", "1"]);
    assert!(inter.contains("-Subject: Bug 3 - café") && inter.contains("+Subject: Bug 3 - reworded"), "{inter}");
    assert!(!inter.contains("+-"), "the diff did not change: {inter}");

    // An editor that fails changes nothing.
    let before = std::fs::read(&e.db).unwrap();
    assert!(!e.run(&["edit", "s"]).0);
    assert_eq!(std::fs::read(&e.db).unwrap(), before);
}

#[test]
fn an_interdiff_shows_a_paragraph_only_wrapped_otherwise_as_the_new_version_wraps_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // Patch 2 reworded from the list: its first paragraph over two lines,
    // the same words.
    let editor = e.script(
        "editor.sh",
        r#"case "$1" in
*.todo) sed '/Bug 2/s/^pick/reword/' "$1" > "$1.new" && mv "$1.new" "$1" ;;
*.txt) printf 'Bug 2 - add b\n\nA\nbody.\n\nMore body.\n' > "$1" ;;
esac
"#,
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s"], "", &[("EDITOR", &editor)]);
    assert!(fine, "{err}");
    e.ok(&["review", "s", "--accept-all"]);
    assert_eq!(e.ok(&["versions", "s", "2"]).lines().count(), 2);
    // What changed: the paragraph as the new version wraps it, among the
    // lines kept as they were; no line taken out or put in.
    let inter = e.ok(&["interdiff", "s", "2"]);
    assert!(inter.contains("\n Subject: Bug 2 - add b\n \n A\n body.\n \n More body.\n ---\n"), "{inter}");
    assert!(!inter.lines().skip(2).any(|l| l.starts_with('-') || l.starts_with('+')), "{inter}");
}

#[test]
fn reimport_keeps_changes() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let changes = e.column(1);
    let ids = e.column(2);
    // Amend the middle patch in git and import the stack again.
    git(&e.src, &["checkout", "-q", "HEAD~1"]);
    std::fs::write(e.src.join("b.txt"), "newer\n").unwrap();
    git(&e.src, &["commit", "-q", "--amend", "-a", "--no-edit"]);
    e.ok(&["import", "--replace", "s", "base"]);
    assert_eq!(e.column(1), changes[..2]);
    assert_eq!(e.column(2)[0], ids[0], "the same patch is stored once");
    assert_ne!(e.column(2)[1], ids[1]);
    let inter = e.ok(&["interdiff", "s", "2"]);
    assert!(inter.contains("-+new\n") && inter.contains("++newer\n"), "{inter}");
}

#[test]
fn reimport_keeps_a_patch_whose_lines_a_change_below_moved() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let ids = e.column(2);
    let stored = e.ok(&["show", "s", "3"]);
    // Three lines added on top of a.txt by patch 1, in git, the patches
    // above it made again on it: git writes patch 3's hunk three lines
    // lower, its `index` line naming other blobs.
    let one = git(&e.src, &["rev-parse", "main~2"]);
    git(&e.src, &["checkout", "-q", "--detach", one.trim()]);
    std::fs::write(e.src.join("a.txt"), format!("zero\nzero\nzero\n{}", TEN.replace("two", "2"))).unwrap();
    git(&e.src, &["commit", "-q", "--amend", "-a", "--no-edit"]);
    git(&e.src, &["cherry-pick", "main~1", "main"]);
    let moved = git(&e.src, &["format-patch", "-1", "--stdout", "HEAD"]);
    assert!(stored.contains("\n@@ -6,5 +6,5 @@") && moved.contains("\n@@ -9,5 +9,5 @@"), "{stored}{moved}");
    e.ok(&["import", "--replace", "s", "base"]);
    // Imported again, only patch 1 is a new version: patch 3 is the patch
    // it was, its diff as stored.
    let now = e.column(2);
    assert_ne!(now[0], ids[0]);
    assert_eq!(now[1..], ids[1..]);
    assert_eq!(e.ok(&["show", "s", "3"]), stored);
}

#[test]
fn reimport_makes_a_new_version_of_a_patch_whose_hunk_moved_to_lines_like_its_own() {
    let e = env();
    two_copies_stack(&e, 1);
    let ids = e.column(2);
    // In git, patch 3's line moved from the first copy to the second: its
    // diff has the same lines, and says another place.
    git(&e.src, &["checkout", "-q", "copies"]);
    let moved = two_copies(Some(2)).replace("    filler 2\n", "    FILLER 2\n");
    std::fs::write(e.src.join("t.txt"), &moved).unwrap();
    git(&e.src, &["commit", "-q", "--amend", "-a", "--no-edit"]);
    e.ok(&["import", "--replace", "s", "tbase"]);
    let now = e.column(2);
    assert_eq!(now[..2], ids[..2]);
    assert_ne!(now[2], ids[2]);
    assert_eq!(e.ok(&["show", "s", "3", "--path", "t.txt"]), moved);
}

#[test]
fn a_broken_line_is_passed_over_never_cut() {
    let (tmp, repo, base) = tiny();
    let repo = repo.to_str().unwrap();
    let path = tmp.path().join("db.jsonl");
    Db::edit(&path, |db| db.new_stack("s", &base, repo)).unwrap();
    let good = std::fs::read_to_string(&path).unwrap();
    // A write cut short by a crash.
    let torn = format!("{good}{{\"t\":\"stack\",\"na");
    std::fs::write(&path, &torn).unwrap();
    let db = Db::load(&path).unwrap();
    assert_eq!(db.stacks().len(), 1);
    assert_eq!(db.warnings.len(), 1, "{:?}", db.warnings);
    let rev = Db::edit(&path, |db| db.new_stack("t", &base, repo)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    // After the header, which a checkpoint's pointer is written over.
    assert!(text[128..].starts_with(&torn[128..]), "nothing is cut off:\n{text}");
    assert_eq!(rev, 4, "the torn line keeps its number");
    let db = Db::load(&path).unwrap();
    assert_eq!(db.stacks().len(), 2);
    assert!(matches!(db.rev(3), Some(Record::Broken)));
    // Garbage anywhere is passed over, and what follows still read.
    std::fs::write(&path, format!("{text}garbage\n{{\"t\":\"head\",\"name\":\"t\",\"rev\":99,\"op\":\"x\",\"time\":0}}\n")).unwrap();
    let db = Db::load(&path).unwrap();
    assert_eq!(db.stacks().len(), 2);
    assert_eq!(db.warnings.len(), 3, "{:?}", db.warnings);
    let no = text.lines().count() + 1;
    assert!(db.warnings[1].contains(&format!(":{no}:")), "{:?}", db.warnings);
}

#[test]
fn a_reader_reads_only_what_was_appended() {
    let (tmp, repo, base) = tiny();
    let path = tmp.path().join("db.jsonl");
    Db::edit(&path, |db| db.new_stack("s", &base, repo.to_str().unwrap())).unwrap();
    let mut reader = Db::load(&path).unwrap();
    assert!(!reader.catch_up().unwrap());
    // Another process is half way through writing a line.
    let line = serde_json::to_string(&Record::Head(stacked::db::Head {
        name: "s".into(),
        rev: None,
        op: "delete".into(),
        time: 0,
    }))
    .unwrap();
    let (a, b) = line.split_at(10);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(a.as_bytes()).unwrap();
    assert!(reader.catch_up().unwrap());
    assert!(reader.stack("s").is_ok(), "half a line is not read");
    f.write_all(format!("{b}\n").as_bytes()).unwrap();
    assert!(reader.catch_up().unwrap());
    assert!(reader.stack("s").is_err(), "the whole line is");
    assert!(reader.warnings.is_empty(), "{:?}", reader.warnings);
    Db::edit(&path, |db| db.undo("s")).unwrap();
    reader.catch_up().unwrap();
    let full = Db::load(&path).unwrap();
    assert_eq!(reader.records(), full.records());
    assert_eq!(reader.head("s").map(|(r, _)| r), full.head("s").map(|(r, _)| r));
    // The lines claude writes are appended without reading the file.
    Db::append(&path, &[Record::Event { run: "r".into(), event: serde_json::json!({"type": "x"}) }]).unwrap();
    reader.catch_up().unwrap();
    assert_eq!(reader.events("r").len(), 1);
}

#[test]
fn records_only_ever_append() {
    let (tmp, repo, base) = tiny();
    let path = tmp.path().join("db.jsonl");
    let d = |m: &str| Draft {
        message: m.into(),
        diff: new_file(&format!("{m}.txt"), m),
        author: Some("A <a@b>".into()),
        ..Default::default()
    };
    Db::edit(&path, |db| db.new_stack("s", &base, repo.to_str().unwrap())).unwrap();
    for m in ["one", "two", "three"] {
        let before = std::fs::read(&path).unwrap();
        Db::edit(&path, |db| ops::add(db, "s", d(m), None)).unwrap();
        // After the header, which a checkpoint's pointer is written over.
        assert!(std::fs::read(&path).unwrap()[128..].starts_with(&before[128..]));
    }
    Db::edit(&path, |db| ops::move_patch(db, "s", 0, 2)).unwrap();
    Db::edit(&path, |db| db.undo("s")).unwrap();
    let db = Db::load(&path).unwrap();
    assert!(matches!(db.records()[0], Record::Stacked { version: 3 }));
    let s = db.stack("s").unwrap();
    let titles: Vec<&str> = s.patches.iter().map(|id| db.patch(id).title()).collect();
    assert_eq!(titles, ["one", "two", "three"]);
}

/// A repo whose fourth patch changes again a line the first one changed:
/// it applies on top of the first, not without it.
fn dependent() -> Env {
    let e = env();
    std::fs::write(e.src.join("a.txt"), TEN.replace("two", "II").replace("nine", "9")).unwrap();
    git(&e.src, &["commit", "-qam", "Bug 4 - two again"]);
    e
}

#[test]
fn only_patches_that_apply_go_in() {
    let e = dependent();
    e.ok(&["import", "s", "base"]);
    let before = std::fs::read(&e.db).unwrap();
    let unchanged = |what: &str| assert_eq!(std::fs::read(&e.db).unwrap(), before, "{what} wrote nothing");

    let (fine, _, err) = e.run(&["move", "s", "4", "1"]);
    assert!(!fine && err.contains("patch 1 (Bug 4 - two again) does not apply to its base"), "{err}");
    assert!(err.contains("nothing changed"), "{err}");
    unchanged("a move");
    let (fine, _, err) = e.run(&["drop", "s", "1"]);
    assert!(!fine && err.contains("patch 3 (Bug 4 - two again) does not apply to the patches below it"), "{err}");
    unchanged("a drop");
    // What does apply goes in.
    e.ok(&["move", "s", "3", "1"]);
    e.ok(&["drop", "s", "1"]);
    assert_eq!(e.titles(), ["Bug 1 - c", "Bug 2 - a", "Bug 4 - t"]);
    let before = std::fs::read(&e.db).unwrap();
    let unchanged = |what: &str| assert_eq!(std::fs::read(&e.db).unwrap(), before, "{what} wrote nothing");

    // A diff that does not apply, by hand.
    let bad = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-zwei\n+drei\n";
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["add", "s", "-m", "Bug 5 - nonsense"], bad, &[]);
    assert!(!fine && err.contains("patch 4 (Bug 5 - nonsense) does not apply"), "{err}");
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["add", "s", "-m", "Bug 5 - no diff"], "", &[]);
    assert!(!fine && err.contains("has no diff"), "{err}");
    unchanged("an add");
    // Mail that needs a patch the stack does not have.
    let mail = git(&e.src, &["format-patch", "--stdout", "--base=base", "HEAD~1..HEAD"]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["import", "t", "--base", "base"], &mail, &[]);
    assert!(!fine && err.contains("does not apply to its base"), "{err}");
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["import", "t"], &mail.replace("base-commit", "x"), &[]);
    assert!(!fine && err.contains("does not say what it applies to"), "{err}");
    // A base the patches do not apply to.
    let (fine, _, err) = e.run(&["base", "s", "HEAD"]);
    assert!(!fine && err.contains("does not apply to its base"), "{err}");
    unchanged("an import or a base");
    assert!(e.ok(&["check"]).contains("s: applies"));
}

#[test]
fn a_hand_edit_that_does_not_apply_comes_back() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // The first time, the editor breaks the hunk; given it back with why,
    // it puts it right and changes the title.
    let editor = e.script(
        "editor.sh",
        &format!(
            r#"if grep -q '^# stacked: ' "$1"; then
  grep '^# stacked: ' "$1" > {why}
  sed -e '/^# stacked: /d' -e 's/^ ONE$/ one/' -e 's/^Subject: Bug 1 - change two$/Subject: Bug 1 - change two, by hand/' "$1" > "$1.new" && mv "$1.new" "$1"
else
  sed 's/^ one$/ ONE/' "$1" > "$1.new" && mv "$1.new" "$1"
fi
"#,
            why = e.root.join("why").display()
        ),
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s", "1", "--accept"], "", &[("EDITOR", &editor)]);
    assert!(fine, "{err}");
    let why = std::fs::read_to_string(e.root.join("why")).unwrap();
    assert!(why.contains("patch 1 (Bug 1 - change two) does not apply to its base"), "{why}");
    assert!(why.contains("fix it and save, or empty the file to give up"), "{why}");
    assert_eq!(e.column(3)[0], "Bug 1 - change two, by hand");
    // Emptied, it changes nothing.
    let before = std::fs::read(&e.db).unwrap();
    let empty = e.script("empty.sh", ": > \"$1\"\n");
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s", "1"], "", &[("EDITOR", &empty)]);
    assert!(fine && err.contains("emptied; nothing changed"), "{err}");
    assert_eq!(std::fs::read(&e.db).unwrap(), before);
}

#[test]
fn a_todo_list_that_does_not_apply_comes_back() {
    let e = dependent();
    e.ok(&["import", "s", "base"]);
    // Puts Bug 4 first; given the list back, puts it back last.
    let editor = e.script(
        "todo.sh",
        &format!(
            r#"if grep -q '^# stacked: ' "$1"; then
  grep '^# stacked: ' "$1" > {why}
  grep '^pick' "$1" | LC_ALL=C sort -k3 > "$1.new" && mv "$1.new" "$1"
else
  grep '^pick' "$1" | {REVERSED} > "$1.new" && mv "$1.new" "$1"
fi
"#,
            why = e.root.join("why").display()
        ),
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s"], "", &[("EDITOR", &editor)]);
    assert!(fine, "{err}");
    let why = std::fs::read_to_string(e.root.join("why")).unwrap();
    assert!(why.contains("patch 1 (Bug 4 - two again) does not apply to its base"), "{why}");
    assert_eq!(e.titles(), ["Bug 1 - c", "Bug 2 - a", "Bug 3 - c", "Bug 4 - t"]);
}

/// Appends a stack as a stacked before the check might have: no base, or
/// one somewhere else, or patches that do not apply.
fn old_stack(e: &Env, drafts: &[(&str, &str)], base: Option<&str>, repo: Option<&str>) {
    let mut recs = Vec::new();
    let mut ids = Vec::new();
    for (title, diff) in drafts {
        let p = ops::new_patch(Draft {
            message: title.to_string(),
            diff: diff.to_string(),
            author: Some("A <a@b>".into()),
            ..Default::default()
        });
        ids.push(p.id.clone());
        recs.push(Record::Patch(p));
    }
    recs.push(Record::Stack(stacked::db::Stack {
        name: "s".into(),
        patches: ids,
        base: base.map(String::from),
        repo: repo.map(String::from),
        parent: None,
        op: "from before".into(),
        time: 0,
    }));
    Db::append(&e.db, &recs).unwrap();
}

#[test]
fn a_stack_that_cannot_be_checked_takes_only_comments() {
    let e = env();
    let one = new_file("one.txt", "one");
    let two = new_file("two.txt", "two");
    old_stack(&e, &[("Bug 1 - one", &one), ("Bug 2 - two", &two)], None, None);
    let before = std::fs::read(&e.db).unwrap();
    for args in [&["move", "s", "2", "1"][..], &["drop", "s", "1"], &["delete", "s"]] {
        let (fine, _, err) = e.run(args);
        assert!(!fine && err.contains("s is read-only: it has no base commit"), "{args:?}: {err}");
    }
    let (fine, _, err) = e.run(&["edit", "s", "1"]);
    assert!(!fine && err.contains("read-only"), "no editor is opened: {err}");
    assert_eq!(std::fs::read(&e.db).unwrap(), before);
    assert!(e.ok(&["check"]).contains("s: s is read-only"));
    // Comments still go in.
    e.ok(&["comment", "s", "--patch", "1", "Still", "readable."]);
    assert!(e.ok(&["comments", "s", "--patch", "1"]).contains("you: Still readable."));
    e.ok(&["comment", "s", "--patch", "1", "--line", "one.txt:+one", "On", "the", "line."]);
    e.ok(&["comment", "s", "On", "the", "stack."]);
    assert_eq!(e.ok(&["comments", "s"]).lines().filter(|l| !l.starts_with(' ')).count(), 3);
    // But claude is sent none: it works on a stack as it applies.
    let id = e.ok(&["comment", "s", "Anything."]);
    let (fine, _, err) = e.run(&["send", "s", id.trim()]);
    assert!(!fine && err.contains("cannot send: s is read-only"), "{err}");
    // Given a base it applies to, it can change again.
    e.ok(&["base", "s", "base"]);
    e.ok(&["move", "s", "2", "1"]);
    assert_eq!(e.column(3), ["Bug 2 - two", "Bug 1 - one"]);

    // Nor can one whose repository is not there.
    let e = env();
    let base = git(&e.src, &["rev-parse", "base"]).trim().to_string();
    old_stack(&e, &[("Bug 1 - one", &one)], Some(&base), Some("/nowhere"));
    let (fine, _, err) = e.run(&["drop", "s", "1"]);
    assert!(!fine && err.contains("its repository, /nowhere, is not there"), "{err}");
    e.ok(&["comment", "s", "--patch", "1", "Fine."]);
}

#[test]
fn a_stack_from_before_the_check_is_not_gone_back_to() {
    let e = env();
    let base = git(&e.src, &["rev-parse", "base"]).trim().to_string();
    let src = e.src.to_str().unwrap();
    // On a.txt as it is not: it does not apply.
    let bad = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-uno\n+one\n";
    old_stack(&e, &[("Bug 1 - bad", bad), ("Bug 2 - two", &new_file("two.txt", "two"))], Some(&base), Some(src));
    let check = e.ok(&["check"]);
    assert!(check.contains("s: patch 1 (Bug 1 - bad) does not apply to its base"), "{check}");
    // An edit that makes it apply goes in; going back to it does not.
    e.ok(&["drop", "s", "1"]);
    assert_eq!(e.column(3), ["Bug 2 - two"]);
    let (fine, _, err) = e.run(&["undo", "s"]);
    assert!(!fine && err.contains("does not apply"), "{err}");
    // Nor is it picked from the tree.
    let old = Db::load(&e.db).unwrap().tree("s")[0].0;
    let (fine, _, err) = e.run(&["select", &old.to_string()]);
    assert!(!fine && err.contains("does not apply"), "{err}");
}

/// Sends comments of stack s to `claude`, standing in for claude, and
/// waits for them to be answered, what it brings back accepted.
fn send(e: &Env, claude: &Path, args: &[&str], env: &[(&str, &Path)]) -> (bool, String, String) {
    let mut all = vec!["send", "s"];
    all.extend(args);
    all.extend(["--wait", "--accept"]);
    let mut envs = vec![("STACKED_CLAUDE", claude)];
    envs.extend(env);
    stacked_in(&e.src, &e.db, &all, "", &envs)
}

/// The comment just written's short id.
fn comment(e: &Env, args: &[&str]) -> String {
    let mut all = vec!["comment", "s"];
    all.extend(args);
    e.ok(&all).trim().to_string()
}

/// A claude that runs `$body` and says `$result`, as stream-json.
fn fake_claude(e: &Env, name: &str, body: &str, result: &str) -> PathBuf {
    let result = serde_json::to_string(&serde_json::json!({"type": "result", "result": result})).unwrap();
    e.script(
        name,
        &format!(
            "echo '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\n{body}\ncat <<'EOF'\n{result}\nEOF\n"
        ),
    )
}

/// Waits for `n` runs to have landed, and none to be going.
fn landed(e: &Env, n: usize) {
    eventually(
        || {
            let db = Db::load(&e.db).unwrap();
            let runs = db.runs();
            (runs.iter().filter(|r| r.state != RunState::Started).count() >= n && runs.iter().all(|r| r.state != RunState::Started))
                .then_some(())
        },
        || format!("the runs did not land:\n{}", e.ok(&["runs"])),
    )
}

#[test]
fn a_comment_on_the_stack_lets_the_agent_change_it_all() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["Squash", "the", "top", "into", "the", "bottom,", "and", "add", "c."]);
    let before = e.column(2);
    let user_head = git(&e.src, &["rev-parse", "HEAD"]);
    // Squash the top patch into the bottom one, and add one on top: the
    // plan written to the todo file the prompt names, which claude may write
    // in (`--add-dir`), a plain `git rebase -i` taking it, as claude refuses
    // a command that sets an editor. With no plan, or one that runs a
    // command, no rebase; what would run another program refused claude.
    let claude = fake_claude(
        &e,
        "claude-edit.sh",
        r#"set -e
test "$(git rev-parse --show-toplevel)" != "$STACKED_SRC"
a=$(git rev-parse HEAD~2) b=$(git rev-parse HEAD~1) c=$(git rev-parse HEAD)
todo=$(printf '%s\n' "$2" | sed -n 's/^The rebase todo file: //p')
test -n "$todo" && test ! -e "$todo"
printf '%s\n' "$@" | grep -qx -- "$(dirname "$todo")"
printf '%s\n' "$@" | grep -qF -- 'Bash(git rebase *--exec*)'
if refused=$(git -c user.name=C -c user.email=c@x rebase -q -i HEAD~3 2>&1); then echo "no plan, rebased: $refused" >&2; exit 1; fi
printf 'exec touch ran\npick %s\n' $a > "$todo"
if refused=$(git -c user.name=C -c user.email=c@x rebase -q -i HEAD~3 2>&1); then echo "ran: $refused" >&2; exit 1; fi
case "$refused" in *"runs a command"*) ;; *) echo "$refused" >&2; exit 1;; esac
test ! -e ran
printf 'pick %s\nfixup %s\npick %s\n' $a $c $b > "$todo"
git -c user.name=C -c user.email=c@x rebase -q -i HEAD~3 >&2
echo extra > c.txt && git add c.txt && git -c user.name=C -c user.email=c@x commit -qm "Bug 4 - add c""#,
        r#"Done. {"replies": {"c1": "Squashed it, and added c."}}"#,
    );
    let (fine, out, err) = send(&e, &claude, &[&id], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    assert!(out.contains(&format!("{id}\tsent (may change the code)")), "{out}");
    assert!(out.contains(&format!("{id}\tthe stack\tagent replied (change, at high)")), "{out}");
    // The plan's folder, the test's own, is gone once the ask landed.
    let plans = e.root.join("todo");
    assert!(plans.is_dir() && std::fs::read_dir(&plans).unwrap().next().is_none(), "{}", plans.display());
    assert!(out.contains("  agent(high): Squashed it, and added c."), "{out}");
    assert_eq!(e.column(3), ["Bug 1 - change two", "Bug 2 - add b", "Bug 4 - add c"]);
    let after = e.column(2);
    assert_ne!(after[0], before[0], "the bottom patch has a new version");
    assert_eq!(after[1], before[1], "the middle patch is the same");
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2);
    // The user's worktree did not move; claude's is kept, the stack's own,
    // for the next change to take.
    assert_eq!(git(&e.src, &["rev-parse", "HEAD"]), user_head);
    let list = git(&e.src, &["worktree", "list"]);
    assert!(list.lines().count() == 2 && list.contains("/.stacked/s/agent "), "{list}");
    // All of it is in the database: the run and what claude wrote.
    let runs = e.ok(&["runs"]);
    assert!(
        runs.contains("change(high) s \"Squash the top into the bottom, and add c.\"\tdone\treviewed: its change accepted; 1 reply accepted"),
        "{runs}"
    );
    let run = runs.split('\t').next().unwrap();
    let transcript = e.ok(&["transcript", run]);
    assert!(transcript.contains("● session started · fake") && transcript.contains("exit 0"), "{transcript}");
    // Answered, it is not sent again by --all.
    let (fine, _, err) = e.run(&["send", "s", "--all"]);
    assert!(!fine && err.contains("no comment to send"), "{err}");
    // Undo puts back the stack from before the ask.
    e.ok(&["undo", "s"]);
    assert_eq!(e.column(2), before);
}

#[test]
fn the_agents_work_in_worktrees_of_the_stack_s_own_reused_while_clean() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let here = e.root.join("here");
    let subject = e.root.join("subject");
    let claude = fake_claude(
        &e,
        "claude-here.sh",
        &format!("pwd > {}\ngit log --format=%s -1 > {}", here.display(), subject.display()),
        r#"{"replies": {"c1": "Looked."}}"#,
    );
    let pwd = || std::fs::read_to_string(&here).unwrap().trim().to_string();
    let last = || {
        let runs = e.ok(&["runs"]);
        let run = runs.lines().last().unwrap().split('\t').next().unwrap().to_string();
        e.ok(&["transcript", &run])
    };
    let agent = e.src.join(".stacked/s/agent");
    // A change works in the stack's own worktree, made the first time.
    let id = comment(&e, &["Look."]);
    let (fine, _, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{err}");
    assert_eq!(pwd(), agent.display().to_string());
    assert!(last().contains(&format!("$ git worktree add --detach {}", agent.display())), "{}", last());
    // The next takes it again: only what differs is written.
    let id = comment(&e, &["Look", "again."]);
    assert!(send(&e, &claude, &[&id], &[]).0);
    assert_eq!(pwd(), agent.display().to_string());
    let t = last();
    assert!(t.contains("$ git switch --detach") && t.contains(&format!(", in {}", agent.display())) && !t.contains("worktree add"), "{t}");
    // One an agent left unfinished work in stays as it is, to look at; the
    // next takes another.
    let messy = fake_claude(&e, "claude-messy.sh", "echo half > half.txt", r#"{"replies": {"c1": "Half done."}}"#);
    let id = comment(&e, &["Do", "half."]);
    let (fine, out, _) = send(&e, &messy, &[&id], &[]);
    assert!(!fine && out.contains(&format!("the agent left work uncommitted, or a rebase half done: it is left in {}", agent.display())), "{out}");
    assert!(agent.join("half.txt").exists());
    let id = comment(&e, &["Once", "more."]);
    assert!(send(&e, &claude, &[&id], &[]).0);
    assert_eq!(pwd(), e.src.join(".stacked/s/agent-2").display().to_string());

    // An answer works in the stack's own worktree for answers, as a change
    // does, the stack applied there: never in your checkout, whose HEAD,
    // branch and files are as they were while the agent works, and after,
    // a change of yours there uncommitted.
    let during = e.root.join("during");
    let yours_now = || git(&e.src, &["rev-parse", "HEAD"]) + &git(&e.src, &["symbolic-ref", "--short", "HEAD"]) + &git(&e.src, &["status", "--porcelain"]);
    let looking = fake_claude(
        &e,
        "claude-looking.sh",
        &format!(
            "pwd > {}\ngit log --format=%s -1 > {}\n{{ git -C \"$STACKED_SRC\" rev-parse HEAD; git -C \"$STACKED_SRC\" symbolic-ref --short HEAD; git -C \"$STACKED_SRC\" status --porcelain; }} > {}",
            here.display(),
            subject.display(),
            during.display()
        ),
        r#"{"replies": {"c1": "Looked."}}"#,
    );
    std::fs::write(e.src.join("a.txt"), "mine\n").unwrap();
    let yours = yours_now();
    assert!(yours.ends_with("main\n M a.txt\n"), "{yours}");
    let id = comment(&e, &["Why?"]);
    let (fine, _, err) = send(&e, &looking, &[&id, "--answer-only"], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    assert_eq!(pwd(), e.src.join(".stacked/s/answer-only-agent").display().to_string());
    assert!(std::fs::read_to_string(&subject).unwrap().starts_with("Bug 3 - café"), "at the stack's top");
    assert_eq!(std::fs::read_to_string(&during).unwrap(), yours, "your checkout while the agent works");
    assert_eq!(yours_now(), yours, "and after");
    assert!(last().contains("$ git worktree add --detach") && last().contains("answer-only-agent"), "{}", last());
    // A settings' file of before that says "pwd", your checkout: read as
    // "ondemand", and said; the answer and the change each work in a
    // worktree of the stack's own, your checkout as it was.
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, format!("{HOME_SETTINGS}answer_only_worktree = \"pwd\"\nworktree = \"pwd\"\n")).unwrap();
    let said = |key: &str| {
        format!(
            "stacked: {}: {key} = \"pwd\" is no longer supported: asks run in a worktree of their own; read as \"ondemand\"",
            settings.display()
        )
    };
    let env = [("STACKED_CONFIG", settings.as_path()), ("STACKED_SRC", e.src.as_path())];
    let id = comment(&e, &["Why", "again?"]);
    let (fine, _, err) = send(&e, &looking, &[&id, "--answer-only"], &env);
    assert!(fine, "{err}");
    assert!(err.contains(&said("agent.answer_only_worktree")) && err.contains(&said("agent.worktree")), "{err}");
    assert_eq!(pwd(), e.src.join(".stacked/s/answer-only-agent").display().to_string());
    assert_eq!(std::fs::read_to_string(&during).unwrap(), yours);
    let id = comment(&e, &["Change", "it."]);
    let (fine, _, err) = send(&e, &looking, &[&id], &env);
    assert!(fine, "{err}");
    assert_eq!(pwd(), e.src.join(".stacked/s/agent-2").display().to_string());
    assert_eq!(std::fs::read_to_string(&during).unwrap(), yours);
    assert_eq!(yours_now(), yours);
    assert_eq!(std::fs::read_to_string(e.src.join("a.txt")).unwrap(), "mine\n");
}

#[test]
fn the_agents_worktrees_are_made_ahead_and_removed_away_from_the_editor() {
    use stacked::ask::{AGENT, ANSWERING, forget, precreate};
    let e = env();
    e.ok(&["import", "s", "base"]);
    // Made ahead, the whole stack applied in each.
    let (repo, dir) = precreate(&e.db, "s", &[AGENT, ANSWERING], None).unwrap();
    assert_eq!(dir, e.src.join(".stacked/s"));
    let (agent, answering) = (dir.join("agent"), dir.join("answer-only-agent"));
    assert!(git(&agent, &["log", "--format=%s", "-1"]).starts_with("Bug 3 - café"));
    assert!(answering.join(".git").exists());
    // An ask takes one, with nothing to make.
    let claude = fake_claude(&e, "claude-ok.sh", "", r#"{"replies": {"c1": "Fine."}}"#);
    let id = comment(&e, &["Look."]);
    assert!(send(&e, &claude, &[&id], &[]).0);
    let runs = e.ok(&["runs"]);
    let run = runs.lines().last().unwrap().split('\t').next().unwrap().to_string();
    let t = e.ok(&["transcript", &run]);
    assert!(t.contains("$ git switch --detach") && !t.contains("worktree add"), "{t}");
    // Removed away from the editor, once it exits: but one holding anything
    // uncommitted, as git refuses to.
    std::fs::write(answering.join("mine.txt"), "x\n").unwrap();
    forget(&e.db, &[(repo, dir.clone(), vec![AGENT, ANSWERING])]);
    eventually(|| (!agent.exists()).then_some(()), || "not removed".into());
    assert!(!agent.exists(), "removed");
    assert!(answering.join("mine.txt").exists(), "kept: it holds something uncommitted");
}

#[test]
fn stacked_config_refuses_pwd_every_ask_running_in_a_worktree_of_its_own() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let settings = e.root.join("home/config.toml");
    // Refused, with why, in the settings' file and in the checkout's own:
    // nothing written in either.
    for key in ["agent.worktree", "agent.answer_only_worktree"] {
        let why = format!("{key} = \"pwd\" is no longer supported: asks run in a worktree of their own; nothing written");
        for args in [vec!["config", key, "pwd"], vec!["config", "--local", key, "pwd"]] {
            let (fine, _, err) = e.run(&args);
            assert!(!fine && err.contains(&why), "{args:?}: {err}");
        }
    }
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), HOME_SETTINGS);
    assert!(!e.src.join(".stacked/local.toml").exists());
    // What is neither, refused too; ondemand and precreate written.
    let (fine, _, err) = e.run(&["config", "agent.worktree", "here"]);
    assert!(!fine && err.contains("agent.worktree is ondemand or precreate, not \"here\""), "{err}");
    e.ok(&["config", "agent.answer_only_worktree", "precreate"]);
    assert_eq!(e.ok(&["config", "agent.answer_only_worktree"]), "precreate\n");
    // A file that still says pwd is read as ondemand, and said: by every
    // command, and by `config`, which prints it as it is read.
    std::fs::write(&settings, format!("{HOME_SETTINGS}worktree = \"pwd\"\n")).unwrap();
    let said = format!(
        "{}: agent.worktree = \"pwd\" is no longer supported: asks run in a worktree of their own; read as \"ondemand\"",
        settings.display()
    );
    assert_eq!(e.ok(&["config", "agent.worktree"]), "ondemand\n");
    let out = e.ok(&["config"]);
    assert!(out.contains(&format!("\n# wrong: {said}\n")) && out.contains("\nworktree = \"ondemand\"\n"), "{out}");
    assert!(out.contains("\n# Every ask runs in a worktree of its own, never in your checkout."), "{out}");
    let (fine, _, err) = e.run(&["stacks"]);
    assert!(fine && err.contains(&format!("stacked: {said}")), "{err}");
    // As the command's help says.
    let help = e.ok(&["config", "--help"]);
    assert!(help.contains("Every ask of the agent runs in a worktree of its own, never in your checkout"), "{help}");
}

/// A run as a stacked of before wrote it, of an ask it started in your
/// checkout `at`, as it could then (`pwd`, an answer's default): `way`
/// asked of the whole stack about comment `id`, its stream `events`. An
/// answer worked there as the checkout was, with no worktree; a change had
/// the checkout itself for its worktree, moved to the stack, where it was
/// to go back to (`back`), the files git did not track then, and
/// `commits`, those made there of the patches, each with its patch. Its
/// agent is not known to this stacked: it started ten minutes ago, its
/// process not written.
fn ask_of_before(e: &Env, at: &Path, run: &str, way: &str, id: &str, commits: &[(String, String)], events: &[serde_json::Value]) {
    let db = Db::load(&e.db).unwrap();
    let n = db.find_note("s", id).unwrap().clone();
    let (from, s) = db.head("s").unwrap();
    let base = s.base.clone().unwrap();
    drop(db);
    let at = at.display().to_string();
    let mut ask = serde_json::json!({
        "way": way, "scope": "stack", "from": from,
        "notes": [{"label": "c1", "change": n.change, "id": n.id}],
        "base": base, "repo": at, "prompt": "The current directory is the user's own checkout, put at the stack for you.",
    });
    if way == "change" {
        ask["worktree"] = serde_json::json!(at);
        ask["kept"] = serde_json::json!(false);
        ask["back"] = serde_json::json!({"Branch": "mine"});
        ask["untracked"] = serde_json::json!([]);
        ask["commits"] = serde_json::json!(commits);
    }
    let request = stacked::thread::thread_turns(&n.text).last().unwrap().body.lines().next().unwrap().to_string();
    // Written as that stacked wrote them, its time with it.
    let started = Record::Run(stacked::db::Run {
        id: run.into(),
        what: way.into(),
        stack: "s".into(),
        state: RunState::Started,
        request,
        ask: Some(ask),
        time: stacked::db::now() - 600,
        ..Default::default()
    });
    let stream = events.iter().map(|event| Record::Event { run: run.into(), event: event.clone() });
    Db::append(&e.db, &std::iter::once(started).chain(stream).collect::<Vec<_>>()).unwrap();
}

#[test]
fn the_asks_a_stacked_of_before_started_in_your_checkout_land_or_are_found_dead_the_checkout_left_as_it_is() {
    let e = env();
    // Your checkout, a worktree of the repository's own, on a branch of
    // yours: the stack imported there. git would remove it if asked to.
    let mine = e.root.join("mine");
    git(&e.src, &["worktree", "add", "-q", "-b", "mine", mine.to_str().unwrap(), "main"]);
    let (fine, _, err) = stacked_in(&mine, &e.db, &["import", "s", "base"], "", &[]);
    assert!(fine, "{err}");
    let at = mine.display().to_string();
    let worktrees = || git(&e.src, &["worktree", "list", "--porcelain"]).lines().filter(|l| l.starts_with("worktree ")).map(String::from).collect::<Vec<_>>();
    let listed = worktrees();
    assert_eq!(listed, [format!("worktree {}", e.src.display()), format!("worktree {at}")]);
    let branch = git(&mine, &["rev-parse", "mine"]);
    let ended = |result: &str| {
        vec![
            serde_json::json!({"type": "system", "subtype": "init", "model": "fake"}),
            serde_json::json!({"type": "result", "result": result}),
            serde_json::json!({"type": "exit", "code": 0}),
        ]
    };
    // An answer it ran in your checkout as it was: read as it was written,
    // it lands, its reply held for your review; the checkout untouched.
    let id = comment(&e, &["Why?"]);
    ask_of_before(&e, &mine, "old-answer", "answer", &id, &[], &ended(r#"{"replies": {"c1": "Because."}}"#));
    let db = Db::load(&e.db).unwrap();
    let i = stacked::ask::info(db.run("old-answer").unwrap()).expect("read as it was written");
    assert_eq!((i.worktree, i.repo.as_str()), (None, at.as_str()));
    drop(db);
    let landed = e.ok(&["land", "old-answer"]);
    assert!(landed.contains("\tanswer s \"Why?\"\tdone\tits work waits for your review"), "{landed}");
    e.ok(&["review", "s", "old-answer", "--accept-all"]);
    assert!(e.ok(&["comments", "s"]).contains("agent: Because."), "{}", e.ok(&["comments", "s"]));
    let yours = (git(&mine, &["rev-parse", "HEAD"]), git(&mine, &["branch", "--show-current"]), git(&mine, &["status", "--porcelain"]));
    assert_eq!(yours, (branch.clone(), "mine\n".to_string(), String::new()));
    assert_eq!(worktrees(), listed);
    // A change it made in your checkout, moved to the stack: read as it
    // was written, it lands, its change held for your review; your checkout
    // is left as the agent left it, at the stack, never moved back nor
    // removed, your branch as it was.
    let patches = Db::load(&e.db).unwrap().stack("s").unwrap().patches.clone();
    let shas: Vec<String> = git(&mine, &["rev-list", "--reverse", "base..mine"]).lines().map(String::from).collect();
    let commits: Vec<(String, String)> = shas.into_iter().zip(patches).collect();
    git(&mine, &["switch", "-q", "--detach", "mine"]);
    std::fs::write(mine.join("c.txt"), "c\n").unwrap();
    git(&mine, &["add", "c.txt"]);
    git(&mine, &["commit", "-q", "--amend", "--no-edit"]);
    let theirs = git(&mine, &["rev-parse", "HEAD"]);
    let id = comment(&e, &["Add", "c."]);
    ask_of_before(&e, &mine, "old-change", "change", &id, &commits, &ended(r#"{"replies": {"c1": "Added c."}}"#));
    let db = Db::load(&e.db).unwrap();
    let i = stacked::ask::info(db.run("old-change").unwrap()).expect("read as it was written");
    assert_eq!((i.worktree.as_deref(), i.kept, i.commits.len()), (Some(at.as_str()), false, 3));
    drop(db);
    let landed = e.ok(&["land", "old-change"]);
    assert!(landed.contains("\tchange s \"Add c.\"\tdone\tits work waits for your review"), "{landed}");
    let left = |what: &str| {
        assert!(mine.join(".git").is_file() && mine.join("a.txt").is_file(), "{what}: the checkout is there");
        assert_eq!(git(&mine, &["rev-parse", "HEAD"]), theirs, "{what}: HEAD as the agent left it");
        assert_eq!(git(&mine, &["branch", "--show-current"]).trim(), "", "{what}: still at the stack, not moved back");
        assert_eq!(git(&mine, &["rev-parse", "mine"]), branch, "{what}: your branch as it was");
        assert_eq!(worktrees(), listed, "{what}: no worktree removed nor made");
    };
    left("landed");
    e.ok(&["review", "s", "old-change", "--accept-all"]);
    assert_eq!(e.ok(&["show", "s", "3", "--path", "c.txt"]), "c\n");
    // Found dead, its agent gone: each lands as failed, as any ask does;
    // what the change's agent left in your checkout is said, and left there.
    std::fs::write(mine.join("half.txt"), "half\n").unwrap();
    let id = comment(&e, &["Split", "it."]);
    let now: Vec<(String, String)> = git(&mine, &["rev-list", "--reverse", "base..HEAD"])
        .lines()
        .map(String::from)
        .zip(Db::load(&e.db).unwrap().stack("s").unwrap().patches.clone())
        .collect();
    ask_of_before(&e, &mine, "old-dead-change", "change", &id, &now, &[serde_json::json!({"type": "text", "text": "Splitting it."})]);
    let id = comment(&e, &["And", "why?"]);
    ask_of_before(&e, &mine, "old-dead-answer", "answer", &id, &[], &[serde_json::json!({"type": "text", "text": "Reading it."})]);
    let runs = e.ok(&["runs"]);
    assert!(
        runs.contains(&format!("\tchange s \"Split it.\"\tfailed\tthe agent failed (killed): Splitting it.; its work is left in {at}\n")),
        "{runs}"
    );
    assert!(runs.contains("\tanswer s \"And why?\"\tfailed\tthe agent failed (killed): Reading it.\n"), "{runs}");
    assert_eq!(std::fs::read_to_string(mine.join("half.txt")).unwrap(), "half\n");
    left("found dead");
    // Their comments failed, to ask again.
    let failed = e.ok(&["comments", "s", "--show", "failed"]);
    assert!(failed.contains("you: Split it.") && failed.contains("you: And why?"), "{failed}");
}

#[test]
fn a_comment_on_a_patch_lets_the_agent_change_only_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "2", "--line", "b.txt:+new", "Split", "b", "out."]);
    assert!(e.ok(&["comments", "s", "--patch", "2"]).contains(&format!("{id}\tpatch 2 · b.txt: new\n  you: Split b out.")));
    // In a worktree of the stack up to patch 2: it splits the patch in two,
    // the title staying on the first, and checks the patch above still
    // applies, with the command the prompt gives it.
    let claude = fake_claude(
        &e,
        "claude-split.sh",
        &format!(
            r#"set -e
test "$(git log --format=%s -1)" = "Bug 2 - add b"
test "$(git rev-list --count HEAD)" = 3
git reset -q HEAD~1
echo x > x.txt && git add x.txt && git -c user.name=C -c user.email=c@x commit -qm "Bug 2 - add b"
git add b.txt && git -c user.name=C -c user.email=c@x commit -qm "Bug 2b - b itself"
fits=$(printf '%s\n' "$2" | sed -n 's/.*run `\([^`]* fits [^`]*\)`.*/\1/p' | head -1)
$fits > {out}"#,
            out = e.root.join("fits.txt").display()
        ),
        r#"{"replies": {"c1": "Split: b is a patch of its own."}}"#,
    );
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{err}");
    assert_eq!(
        std::fs::read_to_string(e.root.join("fits.txt")).unwrap().trim(),
        "fits: the patches above still apply on this change"
    );
    assert_eq!(e.titles(), ["Bug 1 - c", "Bug 2 - a", "Bug 2b - ", "Bug 3 - c"]);
    // The comment followed its line to the piece that has it.
    assert!(out.contains("\tpatch 3 · b.txt: new\tagent replied (change, at high)"), "{out}");
    assert!(out.contains("agent(high): Split: b is a patch of its own."), "{out}");
    assert!(e.ok(&["comments", "s", "--patch", "2"]).is_empty());
    assert!(e.ok(&["log", "s"]).contains("agent: patch 2 split in 2"));

    // Needing more than its patch, the comment goes to the stack, to be sent
    // again from there.
    let id = comment(&e, &["--patch", "1", "Rename", "two", "everywhere."]);
    let promote = fake_claude(
        &e,
        "claude-promote.sh",
        "",
        r#"{"replies": {"c1": "Patch 3 says two as well."}, "promote": "patch 3 says two as well"}"#,
    );
    let (fine, _, err) = send(&e, &promote, &[&id], &[]);
    assert!(fine, "{err}");
    let all = e.ok(&["comments", "s", "--show", "failed"]);
    assert!(all.contains("\tthe stack\tmoved from patch 1 · ✉ message: Bug 1 - change two"), "{all}");
    assert!(all.contains("  agent(high): This needs more than the patch: patch 3 says two as well\n"), "{all}");
    assert!(e.ok(&["comments", "s", "--patch", "1"]).is_empty());
    // There, it waits to be sent again.
    assert!(e.ok(&["comments", "s", "--show", "to-send"]).contains("\tthe stack\tmoved from patch 1"));
}

#[test]
fn a_comment_on_a_patch_sent_to_the_whole_stack_may_change_any_of_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "--line", "a.txt:+2", "Test", "this,", "in", "a", "patch", "on", "top."]);
    // The worktree holds the whole stack, the patches above too: a new
    // commit on top of it all.
    let claude = fake_claude(
        &e,
        "claude-whole.sh",
        r#"set -e
test "$(git rev-list --count HEAD)" = 4
echo t > t.txt && git add t.txt && git -c user.name=C -c user.email=c@x commit -qm "Bug 4 - test 2""#,
        r#"{"replies": {"c1": "Tested, in patch 4."}}"#,
    );
    let (fine, out, err) = send(&e, &claude, &[&id, "--whole-stack"], &[]);
    assert!(fine, "{err}");
    assert!(out.contains(&format!("{id}\tsent (may change the whole stack)")), "{out}");
    assert_eq!(e.titles(), ["Bug 1 - c", "Bug 2 - a", "Bug 3 - c", "Bug 4 - t"]);
    assert!(e.ok(&["log", "s"]).contains("agent: the stack"));
    // Its reply in its thread, the comment still on its line of patch 1.
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains(&format!("{id}\tpatch 1 · a.txt: 2")) && c.contains("agent(high): Tested, in patch 4."), "{c}");
    // Only a change goes to the whole stack.
    let (fine, _, err) = e.run(&["send", "s", &id, "--whole-stack", "--answer-only"]);
    assert!(!fine && err.contains("cannot be used with"), "{err}");
}

#[test]
fn the_stack_is_not_edited_by_hand_while_the_agent_changes_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["Amend", "the", "top."]);
    let line = comment(&e, &["--patch", "1", "--line", "a.txt:+2", "Why", "2?"]);
    // The agent amends the top once the test says so: until the gate is
    // there, it is at work.
    let gate = e.root.join("gate");
    let claude = fake_claude(
        &e,
        "claude-gated.sh",
        &format!(
            r#"until test -e {}; do sleep 0.1; done; echo x >> a.txt; git -c user.name=C -c user.email=c@x commit -qam "Bug 3 - café, a very long title that format-patch will fold over more than one line" --amend"#,
            gate.display()
        ),
        r#"{"replies": {"c1": "Amended."}}"#,
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{err}");
    // Its edit is made from the stack as it took it: nothing else changes
    // the stack meanwhile, nor is its comment written on.
    let why = "the agent is changing the stack: the stack can be edited by hand again once you have reviewed the agent's work";
    for args in [&["drop", "s", "1"][..], &["move", "s", "1", "2"], &["undo", "s"], &["delete", "s"]] {
        let (fine, _, err) = e.run(args);
        assert!(!fine && err.contains(why), "{args:?}: {err}");
    }
    let (fine, _, err) = e.run(&["reply", "s", &id, "Also", "this."]);
    assert!(!fine && err.contains("the agent is working on it"), "{err}");
    // Comments, others than its own, are written on as ever.
    e.ok(&["reply", "s", &line, "Really?"]);
    // Let through the gate, it lands: its change waits for your review, and
    // the stack with it.
    std::fs::write(&gate, "").unwrap();
    landed(&e, 1);
    let (fine, _, err) = e.run(&["drop", "s", "1"]);
    let why = "the agent's change of the stack waits for your review: the stack can be edited by hand again once you have reviewed it";
    assert!(!fine && err.contains(why), "{err}");
    let (fine, _, err) = e.run(&["reply", "s", &id, "Also", "this."]);
    assert!(!fine && err.contains("the agent's reply to it waits for your review"), "{err}");
    assert!(!e.ok(&["comments", "s"]).contains("agent(high): Amended."));
    let reviewed = e.ok(&["review", "s", "--accept-all"]);
    assert!(reviewed.contains("reviewed: its change accepted; 1 reply accepted"), "{reviewed}");
    assert!(e.ok(&["comments", "s"]).contains("agent(high): Amended."));
    assert!(e.column(3)[2].starts_with("Bug 3 - café"), "the agent's change is the stack's");
    e.ok(&["drop", "s", "1"]);
    assert_eq!(e.column(3).len(), 2, "reviewed, the stack is the user's again");
}

#[test]
fn what_the_agent_brings_back_waits_for_review_and_may_be_rejected() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let before = e.column(2);
    let id = comment(&e, &["Amend", "the", "top."]);
    let next = comment(&e, &["Then", "this."]);
    let claude = fake_claude(
        &e,
        "claude-amend.sh",
        r#"echo x >> a.txt; git -c user.name=C -c user.email=c@x commit -qam "Bug 3 - amended" --amend"#,
        r#"{"replies": {"c1": "Amended."}}"#,
    );
    // Unchained: a change waits for the review of the one before it.
    let unchained = e.root.join("unchained.toml");
    std::fs::write(&unchained, format!("{HOME_SETTINGS}chain = false\n")).unwrap();
    let env = [("STACKED_CLAUDE", claude.as_path()), ("STACKED_CONFIG", unchained.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &env);
    assert!(fine, "{err}");
    // Landed, nothing of it is applied: it waits for review, said so.
    assert!(out.contains("waits for your review: a change of the stack, 1 reply"), "{out}");
    assert_eq!(e.column(2), before, "the stack is as it was");
    assert!(!e.ok(&["comments", "s"]).contains("Amended."));
    let queue = e.ok(&["landed", "s"]);
    assert!(queue.contains("a change of the stack, 1 reply") && queue.contains("  change\tagent: the stack"), "{queue}");
    assert!(queue.contains(&format!("  reply\t{id}\tAmended.")), "{queue}");
    let review = e.ok(&["comments", "s", "--show", "to-review"]);
    assert!(review.contains(&format!("{id}\tthe stack\tthe agent's reply (high) waits for your review")), "{review}");
    assert!(!e.ok(&["comments", "s", "--show", "to-send"]).contains(&id), "not to send again before");
    // The next change goes from the stack as the review leaves it.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &next], "", &env);
    assert!(fine, "{err}");
    assert!(out.contains("it goes once you have reviewed the agent's change of the stack"), "{out}");
    // Rejected, it is dropped for good, its reply with it; the comment may be
    // asked again. Then the next change goes.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--reject-all"], "", &env);
    assert!(fine && out.contains("reviewed: its change rejected; 1 reply rejected"), "{out}{err}");
    assert_eq!(e.column(2), before);
    assert!(!e.ok(&["comments", "s"]).contains("Amended."));
    assert!(e.ok(&["comments", "s", "--show", "to-send"]).contains(&id));
    landed(&e, 2);
    let queue = e.ok(&["landed", "s"]);
    assert!(queue.contains(&format!("  reply\t{next}\tAmended.")), "the next one, landed: {queue}");
    let (fine, _, err) = e.run(&["review", "s", "nope", "--accept-all"]);
    assert!(!fine && err.contains("no work on s waits for review as nope"), "{err}");
}

#[test]
fn a_reply_with_a_code_block_of_its_own_stays_the_agent_s_once_accepted() {
    use stacked::thread::{Role, thread_turns};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["Rewrite", "it."]);
    // Its block's fence names no language, as the agent writes it.
    let reply = "Shorter:\n\n```\n  returns the list.\n```\n\nThat is all.";
    let claude = fake_claude(&e, "claude-code.sh", "", &serde_json::json!({"replies": {"c1": reply}}).to_string());
    let (fine, _, err) = send(&e, &claude, &[&id, "--answer-only"], &[]);
    assert!(fine, "{err}");
    let text = Db::load(&e.db).unwrap().find_note("s", &id).unwrap().text.clone();
    let turns: Vec<(Role, String)> = thread_turns(&text).into_iter().map(|t| (t.role, t.body)).collect();
    assert_eq!(turns, [(Role::You, "Rewrite it.".to_string()), (Role::Agent, reply.to_string())], "{text}");
}

#[test]
fn head_is_said_against_the_stack() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let sha = |rev: &str| git(&e.src, &["rev-parse", rev]).trim().to_string();
    let short = |rev: &str| sha(rev)[..12].to_string();
    // The branch the stack was imported from: its commits are its patches.
    assert_eq!(e.ok(&["head", "s"]), format!("HEAD, on main at {}, is at patch 3 of s\n", short("main")));
    git(&e.src, &["checkout", "-q", "--detach", "main~1"]);
    assert_eq!(e.ok(&["head", "s"]), format!("HEAD, detached at {}, is at patch 2 of s\n", short("main~1")));
    git(&e.src, &["checkout", "-q", "base"]);
    assert!(e.ok(&["head", "s"]).ends_with("is at the base of s\n"));
    // A commit on top that is not the stack's: Move HEAD would not move
    // HEAD, and why is said.
    git(&e.src, &["checkout", "-q", "main"]);
    std::fs::write(e.src.join("c.txt"), "c\n").unwrap();
    git(&e.src, &["add", "c.txt"]);
    git(&e.src, &["commit", "-qm", "Mine"]);
    let out = e.ok(&["head", "s"]);
    assert!(
        out.ends_with("is at patch 3 of s, then 1 commit not in the stack\n--to N is refused: HEAD has commits stacked did not make, as 1 commit after patch 3 is not the stack's: Move HEAD moves HEAD only when HEAD is on the stack with no marks, or as stacked last left it\n"),
        "{out}"
    );
    // The same patch, its message other: still the stack's, by its diff;
    // its message said, and nothing of it goes into the stack.
    git(&e.src, &["checkout", "-q", "--detach", "main~1"]);
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 3 - other words"]);
    let out = e.ok(&["head", "s"]);
    let said = "is at patch 3 of s\nthe message of patch 3 differs from its commit's: \"Bug 3 - other words\"\n--to N is refused: ";
    assert!(out.contains(said), "{out}");
    for flag in ["--messages-to-stack", "--messages-to-checkout"] {
        let (fine, _, err) = e.run(&["head", "s", flag]);
        assert!(!fine && err.contains(&format!("unexpected argument '{flag}'")), "{err}");
    }
    // Forked from the stack's base, none of its patches.
    git(&e.src, &["checkout", "-q", "-b", "other", "base"]);
    std::fs::write(e.src.join("d.txt"), "d\n").unwrap();
    git(&e.src, &["add", "d.txt"]);
    git(&e.src, &["commit", "-qm", "Other"]);
    assert!(e.ok(&["head", "s"]).contains("is at the base of s, then 1 commit not in the stack\n"));
    // On another base.
    git(&e.src, &["checkout", "-q", "--orphan", "lone"]);
    git(&e.src, &["commit", "-qm", "Lone"]);
    assert!(e.ok(&["head", "s"]).contains("is not on s: the stack's base is not below it\n"));
    // Not a checkout of its repository.
    let (fine, _, err) = stacked_in(&e.root, &e.db, &["head", "s"], "", &[]);
    assert!(!fine && err.contains("is not a checkout of s's repository"), "{err}");
}

#[test]
fn head_moves_along_the_stack_detached_its_branch_as_it_was() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let sha = |rev: &str| git(&e.src, &["rev-parse", rev]).trim().to_string();
    let top = sha("main");
    // Down the stack: the checkout's own commit, taken as it is.
    assert_eq!(e.ok(&["head", "s", "--to", "2"]), "HEAD is at patch 2 of s, detached: main is as it was\n");
    assert_eq!((sha("HEAD"), sha("main")), (sha("main~1"), top.clone()));
    // Up again: what HEAD has not, applied; the files as the stack leaves
    // them.
    assert_eq!(e.ok(&["head", "s", "--to", "3"]), "HEAD is at patch 3 of s, detached\n");
    assert_eq!(sha("HEAD^{tree}"), sha("main^{tree}"));
    assert!(e.ok(&["head", "s"]).ends_with("is at patch 3 of s\n"));
    // To the base; and there again, nothing to do.
    e.ok(&["head", "s", "--to", "0"]);
    assert_eq!(sha("HEAD"), sha("base"));
    assert_eq!(e.ok(&["head", "s", "--to", "0"]), format!("HEAD is at the base of s already, on {}\n", &sha("base")[..8]));
    let (fine, _, err) = e.run(&["head", "s", "--to", "4"]);
    assert!(!fine && err.contains("s has 3 patches"), "{err}");
    // Not with changes of yours to carry along.
    std::fs::write(e.src.join("a.txt"), "mine\n").unwrap();
    let (fine, _, err) = e.run(&["head", "s", "--to", "1"]);
    assert!(!fine && err.contains("has uncommitted changes: commit or stash them first; HEAD did not move"), "{err}");
    assert_eq!(sha("HEAD"), sha("base"));
    git(&e.src, &["checkout", "-q", "--", "a.txt"]);
    // A commit whose message is not the patch's, reworded outside stacked:
    // it might be yours, and HEAD does not move.
    git(&e.src, &["checkout", "-q", "--detach", "main~2"]);
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 1 - other words"]);
    let reworded = sha("HEAD");
    let (fine, _, err) = e.run(&["head", "s", "--to", "2"]);
    assert!(
        !fine && err.contains("HEAD has commits stacked did not make, as the commit of patch 1 has a message other than the stack's: Move HEAD moves HEAD only when HEAD is on the stack with no marks, or as stacked last left it; HEAD did not move"),
        "{err}"
    );
    assert_eq!(sha("HEAD"), reworded);
}

/// The commit message of patch `k` (from 0) of stack `s`, `message` as a
/// new version: as stacked's own submit gives a patch its revision.
fn reword(db: &Path, k: usize, message: &str) {
    Db::edit(db, |d| {
        let mut patches = d.stack("s")?.patches.clone();
        let old = d.patch(&patches[k]).clone();
        let draft = stacked::mbox::Draft { message: message.into(), ..stacked::mbox::Draft::from(&old) };
        patches[k] = d.add_patch(stacked::ops::revise(&old, draft))?;
        d.set_stack("s", patches, "reword").map(|_| ())
    })
    .unwrap();
}

#[test]
fn head_moves_only_as_stacked_left_it_or_on_the_stack_with_no_mark() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let sha = |rev: &str| git(&e.src, &["rev-parse", rev]).trim().to_string();
    let top = e.src.canonicalize().unwrap();
    let left = || Db::load(&e.db).unwrap().moved("s", &top).map(String::from);
    // On the stack with no mark: moved; where stacked left it, remembered.
    assert_eq!(left(), None);
    assert_eq!(e.ok(&["head", "s", "--to", "2"]), "HEAD is at patch 2 of s, detached: main is as it was\n");
    assert_eq!(left(), Some(sha("HEAD")));
    // A commit of yours on top: refused, said by `stacked head` too.
    std::fs::write(e.src.join("c.txt"), "c\n").unwrap();
    git(&e.src, &["add", "c.txt"]);
    git(&e.src, &["commit", "-qm", "Mine"]);
    let mine = sha("HEAD");
    let why = "HEAD has commits stacked did not make, as 1 commit after patch 2 is not the stack's: Move HEAD moves HEAD only when HEAD is on the stack with no marks, or as stacked last left it";
    let (fine, _, err) = e.run(&["head", "s", "--to", "3"]);
    assert!(!fine && err.contains(&format!("{why}; HEAD did not move")), "{err}");
    assert_eq!(sha("HEAD"), mine);
    assert!(e.ok(&["head", "s"]).ends_with(&format!("is at patch 2 of s, then 1 commit not in the stack\n--to N is refused: {why}\n")));
    // Back where stacked left it: the stack as it is now, whatever it has
    // become since, patch 1 reworded in the stack.
    git(&e.src, &["checkout", "-q", "--detach", "HEAD~1"]);
    reword(&e.db, 0, "Bug 1 - change two, reworded");
    let said = e.ok(&["head", "s"]);
    assert!(said.ends_with("is at patch 2 of s\nthe message of patch 1 differs from its commit's: \"Bug 1 - change two\"\n"), "{said}");
    assert_eq!(e.ok(&["head", "s", "--to", "3"]), "HEAD is at patch 3 of s, detached\n");
    assert_eq!(git(&e.src, &["log", "-3", "--format=%s"]).lines().nth(2), Some("Bug 1 - change two, reworded"));
    assert_eq!(left(), Some(sha("HEAD")));
    // Left there, a new stacked run remembers it: patch 2 reworded, HEAD
    // follows it to patch 1 again, the commit made again from its patch.
    reword(&e.db, 1, "Bug 2 - add b, reworded");
    assert_eq!(e.ok(&["head", "s", "--to", "1"]), "HEAD is at patch 1 of s, detached\n");
    assert_eq!(e.ok(&["head", "s", "--to", "3"]), "HEAD is at patch 3 of s, detached\n");
    assert_eq!(git(&e.src, &["log", "-2", "--format=%s"]).lines().nth(1), Some("Bug 2 - add b, reworded"));
    // Reworded outside stacked: no longer where stacked left it, refused.
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 3 - mine"]);
    let (fine, _, err) = e.run(&["head", "s", "--to", "2"]);
    assert!(!fine && err.contains("as the commit of patch 3 has a message other than the stack's"), "{err}");
}

#[cfg(feature = "moz-phab")]
#[test]
fn head_goes_nowhere_below_a_commit_that_names_a_revision_its_patch_does_not() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let sha = |rev: &str| git(&e.src, &["rev-parse", rev]).trim().to_string();
    // Patch 2 names D34, as stacked's own submit leaves it; HEAD moved there
    // with it, then the stack's patch 2 reworded without it.
    reword(&e.db, 1, &format!("Bug 2 - add b\n\nA body.\n\nMore body.\n\n{}", trailer("D34")));
    git(&e.src, &["checkout", "-q", "--detach", "base"]);
    e.ok(&["head", "s", "--to", "3"]);
    let named = sha("HEAD~1");
    reword(&e.db, 1, "Bug 2 - add b, reworded");
    // Locked below patch 2: said, and refused there.
    let lock = "HEAD is locked below patch 2: the commit of patch 2 names D34, which the patch does not name";
    let said = e.ok(&["head", "s"]);
    assert!(said.ends_with(&format!("is at patch 3 of s\npatch 2 names no revision in the stack and D34 in your workspace's commit\n{lock}\n")), "{said}");
    for n in ["0", "1"] {
        let (fine, _, err) = e.run(&["head", "s", "--to", n]);
        assert!(!fine && err.contains(&format!("{lock}, and Move HEAD keeps every commit up to it as it is; HEAD did not move")), "{n}: {err}");
    }
    // At patch 2 and above: every commit up to it kept as it is, its
    // revision with it; patch 3 applied on it.
    assert_eq!(e.ok(&["head", "s", "--to", "2"]), "HEAD is at patch 2 of s, detached\n");
    assert_eq!(sha("HEAD"), named);
    assert_eq!(e.ok(&["head", "s", "--to", "3"]), "HEAD is at patch 3 of s, detached\n");
    assert_eq!(sha("HEAD~1"), named);
    assert!(git(&e.src, &["log", "-1", "--format=%B", "HEAD~1"]).contains(&trailer("D34")));
    // A revision named outside stacked, as `moz-phab submit` names it:
    // HEAD not where stacked left it, refused, and nothing lets it.
    git(&e.src, &["checkout", "-q", "main"]);
    git(&e.src, &["commit", "-q", "--amend", "-m", &format!("Bug 3 - café, a very long title that format-patch will fold over more than one line\n\n{}", trailer("D35"))]);
    let (fine, _, err) = e.run(&["head", "s", "--to", "3"]);
    assert!(!fine && err.contains("the commit of patch 3 names D35, which the patch does not name: Move HEAD moves HEAD only when"), "{err}");
}

/// The trailer moz-phab writes to name revision `d`.
#[cfg(feature = "moz-phab")]
fn trailer(d: &str) -> String {
    format!("Differential Revision: https://phabricator.services.mozilla.com/{d}")
}

#[cfg(feature = "moz-phab")]
#[test]
fn head_says_which_revision_each_side_names_when_their_messages_differ_by_it_alone() {
    let e = env();
    // The top commit as moz-phab leaves it, its trailer last, imported so; a
    // title that format-patch folds over more than one line.
    let title = "Bug 3 - café, a very long title that format-patch will fold over more than one line";
    git(&e.src, &["commit", "-q", "--amend", "-m", &format!("{title}\n\n{}", trailer("D3"))]);
    e.ok(&["import", "s", "base"]);
    // Reworded by hand without it, the rest of its message the patch's.
    git(&e.src, &["commit", "-q", "--amend", "-m", title]);
    let out = e.ok(&["head", "s"]);
    let said = "is at patch 3 of s\npatch 3 names D3 in the stack and no revision in your workspace's commit\n--to N is refused: ";
    assert!(out.contains(said), "{out}");
    // Naming another.
    git(&e.src, &["commit", "-q", "--amend", "-m", &format!("{title}\n\n{}", trailer("D33"))]);
    let out = e.ok(&["head", "s"]);
    assert!(out.contains("\npatch 3 names D3 in the stack and D33 in your workspace's commit\n"), "{out}");
}

#[test]
fn a_file_is_shown_as_a_patch_leaves_it_after_a_rename_of_one_changed_twice() {
    // Two patches change x.txt, the third renames it with a change: as one
    // input, `git apply` refuses the three, though each applies in turn.
    let e = env();
    let lines: String = (1..=100).map(|n| format!("{n}\n")).collect();
    std::fs::write(e.src.join("x.txt"), &lines).unwrap();
    git(&e.src, &["add", "x.txt"]);
    git(&e.src, &["commit", "-qm", "x"]);
    git(&e.src, &["tag", "x-base"]);
    let edit = |path: &str, from: &str, to: &str| {
        let text = std::fs::read_to_string(e.src.join(path)).unwrap();
        std::fs::write(e.src.join(path), text.replace(from, to)).unwrap();
    };
    edit("x.txt", "\n3\n4\n", "\nthree\nfour\n");
    git(&e.src, &["commit", "-qam", "Bug 4 - change x"]);
    edit("x.txt", "\n50\n", "\nfifty\n");
    git(&e.src, &["commit", "-qam", "Bug 5 - change x again"]);
    git(&e.src, &["mv", "x.txt", "y.txt"]);
    edit("y.txt", "\n52\n", "\nfifty-two\n");
    git(&e.src, &["commit", "-qam", "Bug 6 - rename x to y"]);
    e.ok(&["import", "r", "x-base"]);
    assert!(e.ok(&["show", "r", "3"]).contains("rename from x.txt"), "a rename, as git writes it");
    let shown = e.ok(&["show", "r", "3", "--path", "y.txt"]);
    assert!(shown.contains("\nthree\nfour\n") && shown.contains("\nfifty\n51\nfifty-two\n"), "{shown}");
    // A file the stack does not change, as its base has it.
    assert!(e.ok(&["show", "r", "3", "--path", "b.txt"]).contains("new"));
}

#[test]
fn a_commit_the_agent_rewords_is_a_new_version_of_its_patch() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let d = Db::load(&e.db).unwrap();
    let old = d.patch(&d.stack("s").unwrap().patches[2]).clone();
    drop(d);
    // The agent amends the top commit's message: the patch's new version,
    // not a patch of its own.
    let id = comment(&e, &["Say", "what", "bug", "3", "is."]);
    let claude = fake_claude(
        &e,
        "claude-reword.sh",
        "git -c user.name=C -c user.email=c@x commit -q --amend -m 'Bug 3 - say what it is'",
        r#"{"replies": {"c1": "Reworded."}}"#,
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &[("STACKED_CLAUDE", claude.as_path())]);
    assert!(fine, "{err}");
    let d = Db::load(&e.db).unwrap();
    let r = stacked::held::queue(&d, "s").into_iter().next().unwrap().clone();
    let c = r.held.as_ref().unwrap().stack.clone().unwrap();
    let new = d.patch(&c.patches[2]);
    assert_eq!((new.change.as_str(), new.title()), (old.change.as_str(), "Bug 3 - say what it is"));
    assert_eq!(c.patches.len(), 3);
}

#[test]
fn what_is_attached_to_a_comment_goes_with_it_to_the_agent_open_or_closed() {
    use stacked::db::Note;
    use stacked::refs::{Attached, Pointer, Pointing, To};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let open = comment(&e, &["--patch", "1", "Use", "a", "map."]);
    let closed = comment(&e, &["--patch", "2", "Keep", "the", "vector."]);
    let asked = comment(&e, &["Which", "is", "better,", "@r1", "or", "@r2?"]);
    let d = Db::load(&e.db).unwrap();
    let find = |short: &str| d.find_note("s", short).unwrap().clone();
    let (o, c, a) = (find(&open), find(&closed), find(&asked));
    let to = |n: &Note| To::Comment {
        change: n.change.clone(),
        id: n.id.clone(),
    };
    let attached = vec![
        Attached { n: 1, to: to(&o) },
        Attached { n: 2, to: to(&c) },
        Attached {
            n: 3,
            to: To::Lines("a.txt:1-2 @1".into()),
        },
        Attached {
            n: 4,
            to: To::Lines("b.txt @2".into()),
        },
    ];
    Db::edit(&e.db, |db| {
        db.set_note(Note { attached, ..a.clone() })?;
        db.set_note(Note { text: String::new(), ..c.clone() })
    })
    .unwrap();
    // A mention naming nothing attached: not sent.
    let bad = comment(&e, &["See", "@r9."]);
    let (fine, _, err) = e.run(&["send", "s", &bad, "--answer-only"]);
    assert!(!fine && err.contains("@r9 names no reference attached to the comment"), "{err}");
    // Sent, what it has attached is told the agent with it, open or closed;
    // the agent points at comments by a label and by an id.
    let prompts = e.root.join("prompts.txt");
    let result = format!(r#"{{"replies": {{"c1": {{"text": "As @r1 says.", "refs": [{{"comment": "c1"}}, {{"comment": "{open}"}}, {{"comment": "nowhere"}}]}}}}}}"#);
    let claude = fake_claude(&e, "claude-attached.sh", &format!("printf '%s' \"$2\" > {}", prompts.display()), &result);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &asked, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", claude.as_path())]);
    assert!(fine, "{out}{err}");
    let prompt = std::fs::read_to_string(&prompts).unwrap();
    for said in [
        "Attached to it, as what it says names them (@r1; @r{1,2} several; @r* all), for what they say; they are not asked of you:".to_string(),
        format!("  r1 [{open}] patch 1, its message, line `Bug 1 - change two` (open):"),
        "    The user: Use a map.".into(),
        format!("  r2 [{closed}] patch 2, its message, line `Bug 2 - add b` (closed):"),
        "    The user: Keep the vector.".into(),
        "  r3: a.txt:1-2 · as patch 1 leaves it:".into(),
        "        1 │ one\n        2 │ 2\n".into(),
        "  r4: b.txt · as patch 2 leaves it:\n        1 │ new\n".into(),
        "{\"comment\": its label".into(),
        "(The references you attach to your reply to it are numbered from r5 on.)".into(),
    ] {
        assert!(prompt.contains(&said), "{said:?} in:\n{prompt}");
    }
    // The reply, held for review, points at the comments it named, those
    // there are.
    let d = Db::load(&e.db).unwrap();
    let r = stacked::held::queue(&d, "s")[0].clone();
    let (body, pointers) = stacked::refs::split(&r.held.as_ref().unwrap().replies[0].text);
    assert_eq!(body, "As @r1 says.");
    // Numbered after the comment's own, the one that is none leaving its
    // number out.
    assert_eq!(
        pointers,
        [
            Pointing {
                n: Some(5),
                to: Pointer::Comment {
                    change: a.change.clone(),
                    id: a.id.clone()
                }
            },
            Pointing {
                n: Some(6),
                to: Pointer::Comment {
                    change: o.change.clone(),
                    id: o.id.clone()
                }
            },
        ]
    );
    // What is attached, listed with the comment.
    let listed = e.ok(&["comments", "s"]);
    assert!(listed.contains(&format!("  r1 ↗ {open}\t")), "{listed}");
    assert!(listed.contains(&format!("  r2 ↗ {closed}\t")) && listed.contains("(closed)"), "{listed}");
    assert!(listed.contains("  r3 ⇢ a.txt:1-2 · as patch 1 leaves it"), "{listed}");
    assert!(listed.contains("  r4 ⇢ b.txt · as patch 2 leaves it"), "{listed}");
}

#[test]
fn an_agent_attaches_references_to_its_reply_and_to_the_comments_it_opens() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let prompts = e.root.join("prompts.txt");
    let result = r#"{"replies": {"c1": {"text": "See there.", "refs": [{"file": "a.txt", "lines": "3,1-2", "patch": 1}, {"file": "b.txt", "lines": "1"}, {"file": "b.txt", "patch": 2}, {"file": "c.txt", "lines": "x"}, {"file": "a.txt", "lines": "9", "patch": 0}]}}, "comments": [{"patch": 1, "file": "a.txt", "side": "+", "line": "2", "text": "And here.", "refs": [{"file": "a.txt", "lines": "2", "patch": 0}]}]}"#;
    let claude = fake_claude(&e, "claude-refs.sh", &format!("printf '%s' \"$2\" > {}", prompts.display()), result);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", claude.as_path())]);
    assert!(fine, "{out}{err}");
    // Told how to attach them, lines or a file whole.
    let prompt = std::fs::read_to_string(&prompts).unwrap();
    assert!(prompt.contains("{\"text\": what you write, \"refs\": [...]}") && prompt.contains("\"lines\": \"120-140,200-210\" (none, the whole file)"), "{prompt}");
    // Kept in what it wrote, a refs block each, the ranges merged, each
    // numbered: after the comment's own (it has none), and from r1 in a
    // comment it opens; a file whole, as it is; one that is no reference,
    // left out with its number, so that what names the others still does.
    let d = Db::load(&e.db).unwrap();
    let r = stacked::held::queue(&d, "s")[0].clone();
    let h = r.held.as_ref().unwrap();
    assert_eq!(h.replies[0].text, "See there.\n\n```refs\nr1 a.txt:1-3 @1\nr2 b.txt:1\nr3 b.txt @2\nr5 a.txt:9 @0\n```");
    assert!(h.opened[0].text.contains("And here.\n\n```refs\nr1 a.txt:2 @0\n```"), "{}", h.opened[0].text);
}

#[test]
fn a_web_link_attached_goes_to_the_agent_as_a_link_and_the_agent_gives_its_own() {
    use stacked::db::Note;
    use stacked::refs::{Attached, To};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Is", "it", "as", "@r1", "says?"]);
    let bug = "https://bugzilla.mozilla.org/show_bug.cgi?id=1234567";
    let n = Db::load(&e.db).unwrap().find_note("s", &id).unwrap().clone();
    let attached = vec![Attached { n: 1, to: To::Link(bug.into()) }];
    Db::edit(&e.db, |db| db.set_note(Note { attached, ..n.clone() })).unwrap();
    // Listed with the comment, whole.
    assert!(e.ok(&["comments", "s"]).contains(&format!("  r1 ⇗ {bug}")));
    // Sent, the agent is told it as a link it need not fetch, and to point
    // at the stack first. It gives links of its own, one not of the web.
    let prompts = e.root.join("prompts.txt");
    let result = r#"{"replies": {"c1": {"text": "As @r2 says, and @r4.", "refs": [{"url": "https://example.com/spec#a"}, {"url": "ftp://example.com/x"}, {"file": "a.txt", "lines": "2", "patch": 1}]}}}"#;
    let claude = fake_claude(&e, "claude-links.sh", &format!("printf '%s' \"$2\" > {}", prompts.display()), result);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", claude.as_path())]);
    assert!(fine, "{out}{err}");
    let prompt = std::fs::read_to_string(&prompts).unwrap();
    for said in [
        format!("  r1: a web link, {bug} (you need not fetch it, and may not be able to: your tools may not reach the network)\n"),
        "or, to point at a page of the web, {\"url\": \"https://example.com/a/b\" (its whole address, https or http)}".into(),
        "Point at the stack first: when the code or the comments of the stack say what you mean".into(),
        "(The references you attach to your reply to it are numbered from r2 on.)".into(),
    ] {
        assert!(prompt.contains(&said), "{said:?} in:\n{prompt}");
    }
    // Its reply holds its link whole, numbered after the comment's own; the
    // one not of the web is left out, with its number.
    let d = Db::load(&e.db).unwrap();
    let r = stacked::held::queue(&d, "s")[0].clone();
    assert_eq!(r.held.as_ref().unwrap().replies[0].text, "As @r2 says, and @r4.\n\n```refs\nr2 https://example.com/spec#a\nr4 a.txt:2 @1\n```");
    // Accepted, the thread lists it.
    e.ok(&["review", "s", "--accept-all"]);
    let listed = e.ok(&["comments", "s"]);
    assert!(listed.contains("agent(high): As @r2 says, and @r4.") && listed.contains("  r2 https://example.com/spec#a\n"), "{listed}");
    assert!(listed.contains(&format!("  r1 ⇗ {bug}")), "{listed}");
}

#[test]
fn a_comment_on_a_file_is_on_it_as_a_whole_for_you_and_the_agent() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // On patch 1's a.txt, its name: listed so; again there, a turn of it.
    let id = comment(&e, &["--patch", "1", "--path", "a.txt", "Why", "this", "file?"]);
    assert_eq!(comment(&e, &["--patch", "1", "--path", "a.txt", "And", "its", "name?"]), id, "a turn of it");
    let listed = e.ok(&["comments", "s"]);
    assert!(listed.contains("patch 1 · a.txt, as a whole"), "{listed}");
    assert!(listed.contains("you: Why this file?\n    And its name?"), "{listed}");
    // A file the patch does not change: said; a line and a file at once,
    // refused.
    let (fine, _, err) = e.run(&["comment", "s", "--patch", "1", "--path", "b.txt", "No."]);
    assert!(!fine && err.contains("patch 1 does not change b.txt"), "{err}");
    let (fine, _, err) = e.run(&["comment", "s", "--patch", "1", "--path", "a.txt", "--line", "a.txt:+2", "No."]);
    assert!(!fine && err.contains("cannot be used with"), "{err}");
    // Asked: the agent is told it is on the file as a whole; one it opens
    // with no line is on its file as a whole too.
    let prompts = e.root.join("prompts.txt");
    let result = r#"{"replies": {"c1": "Because."}, "comments": [{"patch": 1, "file": "a.txt", "side": "+", "line": "", "text": "And its tests."}]}"#;
    let claude = fake_claude(&e, "claude-files.sh", &format!("printf '%s' \"$2\" > {}", prompts.display()), result);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", claude.as_path())]);
    assert!(fine, "{out}{err}");
    let prompt = std::fs::read_to_string(&prompts).unwrap();
    assert!(prompt.contains("patch 1, the file a.txt as a whole:") && prompt.contains("or \"\" for the file as a whole"), "{prompt}");
    let d = Db::load(&e.db).unwrap();
    let r = stacked::held::queue(&d, "s")[0].clone();
    let opened = &r.held.as_ref().unwrap().opened[0];
    assert!(stacked::diff::is_file_key(&opened.line) && opened.file == "a.txt", "{opened:?}");
    assert_eq!(opened.where_said(), "a.txt, as a whole");
}

#[test]
fn several_asks_wait_their_turn() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let one = comment(&e, &["First."]);
    let two = comment(&e, &["Second."]);
    let three = comment(&e, &["--patch", "2", "Why b?"]);
    // The agent works until the test says so: until the gate is there.
    let gate = e.root.join("gate");
    let claude = fake_claude(
        &e,
        "claude-wait.sh",
        &format!("until test -e {}; do sleep 0.1; done", gate.display()),
        r#"{"replies": {"c1": "Seen."}}"#,
    );
    // Unchained: each waits for what goes before it, as self-review's do.
    let unchained = e.root.join("unchained.toml");
    std::fs::write(&unchained, format!("{HOME_SETTINGS}chain = false\n")).unwrap();
    let env = [("STACKED_CLAUDE", claude.as_path()), ("STACKED_CONFIG", unchained.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &one], "", &env);
    assert!(fine && out.contains("sent"), "{out}{err}");
    // A change has the stack to itself: what comes after waits, ready.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &two], "", &env);
    assert!(fine, "{err}");
    assert!(out.contains("ready (may change the code), it goes once the agent's change on the stack lands"), "{out}");
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &three, "--answer-only"], "", &env);
    assert!(fine, "{err}");
    assert!(out.contains("ready (answer only)"), "{out}");
    let waiting = e.ok(&["comments", "s", "--show", "in-progress"]);
    assert_eq!(waiting.lines().filter(|l| !l.starts_with(' ')).count(), 3, "{waiting}");
    // Let through the gate, each goes in turn, on its own; what each brings
    // back waits for review, the first landed first.
    std::fs::write(&gate, "").unwrap();
    landed(&e, 3);
    let runs = e.ok(&["runs"]);
    assert_eq!(runs.lines().filter(|l| l.contains("\tdone\t")).count(), 3, "{runs}");
    let queue = e.ok(&["landed", "s"]);
    assert_eq!(queue.lines().filter(|l| !l.starts_with(' ')).count(), 3, "{queue}");
    for _ in 0..3 {
        let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--accept-all"], "", &env);
        assert!(fine, "{out}{err}");
    }
    assert_eq!(e.ok(&["landed", "s"]), "");
    let read = e.ok(&["comments", "s", "--show", "to-read"]);
    assert_eq!(read.matches("agent(high): Seen.").count(), 3, "{read}");
    assert!(e.ok(&["comments", "s", "--show", "to-read-only"]).contains(&three));
}

#[test]
fn one_build_and_test_goes_at_a_time_of_every_stack() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["import", "t", "base"]);
    let on_s = comment(&e, &["Build", "it."]);
    let on_t = e.ok(&["comment", "t", "Build", "it", "too."]).trim().to_string();
    // A build fails if another is going; one goes on once the test says so.
    let (going, gate) = (e.root.join("building"), e.root.join("gate"));
    let claude = fake_claude(
        &e,
        "claude-build.sh",
        &format!("test -e {g} && exit 9; touch {g}; until test -e {o}; do sleep 0.1; done; rm {g}", g = going.display(), o = gate.display()),
        r#"{"replies": {"c1": "Built."}}"#,
    );
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &on_s, "--build-and-test"], "", &env);
    assert!(fine && out.contains("sent"), "{out}{err}");
    // Another stack's waits for it, ready.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "t", &on_t, "--build-and-test"], "", &env);
    assert!(fine, "{err}");
    assert!(out.contains("ready (build and test), it goes once the running build and test of s lands"), "{out}");
    // Each goes in turn, in its stack's build worktree.
    std::fs::write(&gate, "").unwrap();
    landed(&e, 2);
    let db = Db::load(&e.db).unwrap();
    for name in ["s", "t"] {
        let r = db.runs().into_iter().find(|r| r.stack == name).unwrap();
        assert_eq!(r.state, RunState::Done, "{}", r.summary);
        let w = stacked::ask::info(r).and_then(|i| i.worktree).unwrap_or_default();
        assert!(w.ends_with(&format!("/.stacked/{name}/build")), "{w}");
    }
}

#[test]
fn comments_batched_wait_until_the_batch_is_sent() {
    use stacked::ask::{Scope, info};
    use stacked::db::Way;
    let e = env();
    e.ok(&["import", "s", "base"]);
    let one = comment(&e, &["First."]);
    let two = comment(&e, &["--patch", "2", "Why b?"]);
    let three = comment(&e, &["--patch", "1", "Why 2?"]);
    let claude = fake_claude(&e, "claude-batch.sh", "", r#"{"replies": {"c1": "Seen.", "c2": "Seen too."}}"#);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    // Batched, they wait: nothing is started.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &one, &two, "--later"], "", &env);
    assert!(fine, "{err}");
    assert_eq!(out, format!("{one}\tbatched (may change the code)\n{two}\tbatched (may change the code)\n"));
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &three, "--later", "--answer-only"], "", &env);
    assert!(fine && out == format!("{three}\tbatched (answer only)\n"), "{out}{err}");
    assert_eq!(e.ok(&["runs"]), "");
    let shown = e.ok(&["comments", "s"]);
    assert_eq!(shown.matches("batched (may change the code)").count(), 2, "{shown}");
    assert!(shown.contains("batched (answer only)"), "{shown}");
    // In progress, as self-review has them.
    let waiting = e.ok(&["comments", "s", "--show", "in-progress"]);
    assert_eq!(waiting.lines().filter(|l| !l.starts_with(' ')).count(), 3, "{waiting}");
    // Sent: the changes together, to one agent, of the whole stack as they
    // are on the stack and on a patch; the answer on its own.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", "--batch", "--wait", "--accept"], "", &env);
    assert!(fine, "{out}{err}");
    let db = Db::load(&e.db).unwrap();
    let mut asks: Vec<(Way, bool, usize)> = db.runs().into_iter().filter_map(info).map(|i| (i.way, i.scope == Scope::Stack, i.notes.len())).collect();
    asks.sort_by_key(|(w, ..)| *w == Way::Answer);
    assert_eq!(asks, [(Way::Change, true, 2), (Way::Answer, false, 1)]);
    let read = e.ok(&["comments", "s", "--show", "to-read"]);
    assert_eq!(read.matches("agent(high): Seen.").count(), 2, "{read}");
    // Nothing left to send.
    let (fine, _, err) = e.run(&["send", "s", "--batch"]);
    assert!(!fine && err.contains("the batch is empty"), "{err}");
}

#[test]
fn landed_work_is_edited_by_hand_in_its_place() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let claude = fake_claude(
        &e,
        "claude-spell.sh",
        &format!(
            "set -e\n{}\ngit -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a",
            sed_i("s/^2$/deux/", "a.txt")
        ),
        r#"{"replies": {"c1": "Spelled it deux."}}"#,
    );
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{out}{err}");
    let landed = e.ok(&["landed", "s"]);
    let run = landed.split('\t').next().unwrap().to_string();
    assert!(landed.contains("Spelled it deux."), "{landed}");
    // Edited by hand from its review: patch 1 as the work leaves it.
    let editor = e.script("editor.sh", &format!("{}\n", sed_i("s/^+deux$/+two, spelled/", "$1")));
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s", "1", "--work", &run[..8]], "", &[("EDITOR", &editor)]);
    assert!(fine && err.contains("it waits for your review"), "{err}");
    // That work is rejected; in its place, the agent's with your edit in it,
    // its reply with it.
    let landed = e.ok(&["landed", "s"]);
    assert_eq!(landed.lines().filter(|l| !l.starts_with(' ')).count(), 1, "{landed}");
    assert!(!landed.starts_with(&run) && landed.contains("Spelled it deux."), "{landed}");
    assert!(landed.contains("\tagent: patch 1; edit "), "{landed}");
    let runs = e.ok(&["runs", "s"]);
    assert!(runs.contains("reviewed: rejected for your edit of it by hand"), "{runs}");
    e.ok(&["review", "s", "--accept-all"]);
    assert!(e.ok(&["show", "s", "1"]).contains("+two, spelled\n"));
    assert!(e.ok(&["comments", "s"]).contains("agent(high): Spelled it deux."));
    // Nothing waits for review: none to edit.
    let (fine, _, err) = e.run(&["edit", "s", "1", "--work", &run[..8]]);
    assert!(!fine && err.contains("no work on s waits for review"), "{err}");
}

#[test]
fn the_asking_comment_is_closed_with_its_review_if_said() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let claude = fake_claude(&e, "claude-answer.sh", "", r#"{"replies": {"c1": "Because."}}"#);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{out}{err}");
    let out = e.ok(&["review", "s", "--accept-all", "--close-asking"]);
    assert!(out.contains("reviewed: 1 reply accepted; 1 asking comment closed"), "{out}");
    assert_eq!(e.ok(&["comments", "s"]), "");
    assert!(e.ok(&["comments", "s", "--show", "closed"]).contains("agent(high): Because."));
}

#[test]
fn work_that_brought_nothing_back_waits_for_review_while_its_comment_is_open() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    // The agent replies to nothing, and opens no comment.
    let claude = fake_claude(&e, "claude-nothing.sh", "", r#"{"replies": {}}"#);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    // Its comment still open, the work waits for your review all the same,
    // said so wherever it is listed.
    assert!(out.contains("\twaits for your review: 1 asking comment to close or keep, nothing brought back\n"), "{out}");
    let runs = e.ok(&["runs", "s"]);
    assert!(
        runs.contains("\tdone\tthe agent brought nothing back, and the comment it was asked about is still open: its work waits for your review\n"),
        "{runs}"
    );
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.lines().count() == 1 && landed.ends_with("\t1 asking comment to close or keep, nothing brought back\tasked at high\n"), "{landed}");
    // Closed with nothing to mark, the comment kept: nothing waits, and the
    // comment is open still, to ask again.
    let out = e.ok(&["review", "s", "--accept-all"]);
    assert!(out.ends_with("\treviewed: the agent brought nothing back\n"), "{out}");
    assert_eq!(e.ok(&["landed", "s"]), "");
    assert!(e.ok(&["comments", "s", "--show", "to-send"]).contains(&id));
    // Asked again, and nothing back again: this review closes the comment.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &env);
    assert!(fine && out.contains("waits for your review: 1 asking comment to close or keep"), "{out}{err}");
    let out = e.ok(&["review", "s", "--accept-all", "--close-asking"]);
    assert!(out.ends_with("\treviewed: the agent brought nothing back; 1 asking comment closed\n"), "{out}");
    assert_eq!(e.ok(&["comments", "s"]), "");
    assert!(e.ok(&["comments", "s", "--show", "closed"]).contains("you: Why 2?"));
    // A change that changes nothing, and says nothing, waits so too.
    let id = comment(&e, &["--patch", "3", "Shorter?"]);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &env);
    assert!(fine && out.contains("\twaits for your review: 1 asking comment to close or keep, nothing brought back\n"), "{out}{err}");
    assert!(e.ok(&["review", "s", "--accept-all"]).ends_with("\treviewed: the agent brought nothing back\n"));
    // Accepted as it lands, it waits for nothing.
    let id = comment(&e, &["--patch", "2", "And", "b?"]);
    let (fine, out, err) = send(&e, &claude, &[&id, "--answer-only"], &[]);
    assert!(fine && !out.contains("waits for your review"), "{out}{err}");
    assert_eq!(e.ok(&["landed", "s"]), "");
    assert!(e.ok(&["runs", "s"]).ends_with("\tdone\treviewed: the agent brought nothing back\n"));
}

/// Run `run` of stack s, an answer about `notes` (change, id), landed as
/// if the agent had run and answered `result`: the stream in the database,
/// no agent run.
fn landed_answer(e: &Env, run: &str, notes: &[(String, String)], result: &str) -> stacked::db::Run {
    Db::edit(&e.db, |d| {
        let from = d.head("s").unwrap().0;
        let shown: Vec<serde_json::Value> =
            notes.iter().enumerate().map(|(k, (c, i))| serde_json::json!({"label": format!("c{}", k + 1), "change": c, "id": i})).collect();
        d.add_run(stacked::db::Run {
            id: run.into(),
            what: "answer".into(),
            stack: "s".into(),
            state: RunState::Started,
            ask: Some(serde_json::json!({"way": "answer", "scope": "stack", "from": from, "notes": shown})),
            ..Default::default()
        })?;
        d.add_event(run, serde_json::json!({"type": "result", "result": result}))?;
        d.add_event(run, serde_json::json!({"type": "exit", "code": 0}))
    })
    .unwrap();
    stacked::ask::land(&e.db, run, None).unwrap()
}

/// Closes comment (change, id), as its menu's `Close this comment…` does.
fn close_comment(e: &Env, (change, id): &(String, String)) {
    Db::edit(&e.db, |d| {
        let n = d.note(change, id).cloned().unwrap();
        d.set_note(stacked::db::Note { text: String::new(), ..n })
    })
    .unwrap();
}

#[test]
fn work_that_brought_nothing_back_is_held_only_while_a_comment_it_was_asked_about_is_open() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let (one, two) = (comment(&e, &["One?"]), comment(&e, &["Two?"]));
    let d = Db::load(&e.db).unwrap();
    let at = |id: &str| d.find_note("s", id).map(|n| (n.change.clone(), n.id.clone())).unwrap();
    let notes = [at(&one), at(&two)];
    // One of the two closed meanwhile: held for the other.
    close_comment(&e, &notes[0]);
    let r1 = landed_answer(&e, "r1", &notes, r#"{"replies": {}}"#);
    assert_eq!(r1.summary, "the agent brought nothing back, and 1 of the 2 comments it was asked about is still open: its work waits for your review");
    assert!(r1.held.as_ref().is_some_and(stacked::held::Held::is_empty));
    assert!(e.ok(&["landed", "s"]).starts_with("r1\t"), "listed");
    // Both closed meanwhile: not held, nothing to review.
    close_comment(&e, &notes[1]);
    let r2 = landed_answer(&e, "r2", &notes, r#"{"replies": {}}"#);
    assert_eq!((r2.summary.as_str(), r2.held.is_none()), ("the agent brought nothing back", true));
    // The first still waits, said as it is now: no comment left to close or
    // keep. Its review closes with nothing to mark.
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.lines().count() == 1 && landed.starts_with("r1\t") && landed.ends_with("\tnothing brought back\n"), "{landed}");
    assert_eq!(e.ok(&["review", "s", "--accept-all"]), "r1\treviewed: the agent brought nothing back\n");
    assert_eq!(e.ok(&["landed", "s"]), "");
    // A failed ask is not held, its comment open.
    let three = comment(&e, &["Three?"]);
    let d = Db::load(&e.db).unwrap();
    let n = d.find_note("s", &three).unwrap();
    Db::edit(&e.db, |d| {
        let from = d.head("s").unwrap().0;
        let ask = serde_json::json!({"way": "answer", "scope": "stack", "from": from, "notes": [{"label": "c1", "change": n.change, "id": n.id}]});
        d.add_run(stacked::db::Run {
            id: "r3".into(),
            what: "answer".into(),
            stack: "s".into(),
            state: RunState::Started,
            ask: Some(ask),
            ..Default::default()
        })?;
        d.add_event("r3", serde_json::json!({"type": "exit", "code": 1}))
    })
    .unwrap();
    let r3 = stacked::ask::land(&e.db, "r3", None).unwrap();
    assert!(r3.state == RunState::Failed && r3.held.is_none(), "{r3:?}");
    assert_eq!(e.ok(&["landed", "s"]), "");
}

#[test]
fn a_reply_taken_at_once_closes_the_review_of_its_work_its_comment_left_open() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let claude = fake_claude(&e, "claude-answer.sh", "", r#"{"replies": {"c1": "Because."}}"#);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine && out.contains("waits for your review: 1 reply"), "{out}{err}");
    // Taken at once, as Enter on the comment takes it: the reply is in its
    // thread, and nothing else is left of the work, so its review closes.
    let run = stacked::held::queue(&Db::load(&e.db).unwrap(), "s")[0].id.clone();
    Db::edit(&e.db, |d| stacked::held::take(d, &run, 0)).unwrap();
    assert_eq!(e.ok(&["landed", "s"]), "");
    let runs = e.ok(&["runs", "s"]);
    assert!(runs.starts_with(&format!("{run}\t")) && runs.ends_with("\tdone\treviewed: 1 reply accepted\n"), "{runs}");
    // The comment stays open, the reply its last word: to read, and to
    // reply to, as any comment.
    assert_eq!(e.ok(&["comments", "s", "--show", "to-review"]), "");
    assert!(e.ok(&["comments", "s", "--show", "to-read"]).contains("agent(high): Because."));
    e.ok(&["reply", "s", &id, "Really?"]);
    assert!(e.ok(&["comments", "s", "--show", "to-send"]).contains("you: Really?"));
}

#[test]
fn taking_the_last_reply_closes_the_review_its_comments_open_or_not() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // Two replies: one taken, the other is what is left to review.
    let (one, two) = (comment(&e, &["One?"]), comment(&e, &["Two?"]));
    let d = Db::load(&e.db).unwrap();
    let at = |id: &str| d.find_note("s", id).map(|n| (n.change.clone(), n.id.clone())).unwrap();
    let r1 = landed_answer(&e, "r1", &[at(&one), at(&two)], r#"{"replies": {"c1": "For one.", "c2": "For two."}}"#);
    let k = r1.held.as_ref().and_then(|h| h.replies.iter().position(|rp| rp.text == "For one.")).unwrap();
    Db::edit(&e.db, |d| stacked::held::take(d, "r1", k)).unwrap();
    let landed = e.ok(&["landed", "s"]);
    let lines: Vec<&str> = landed.lines().collect();
    assert!(lines.len() == 2 && lines[0].starts_with("r1\t") && lines[0].ends_with("\t1 reply"), "{landed}");
    assert!(lines[1].starts_with("  reply\t") && lines[1].ends_with("\tFor two."), "{landed}");
    // The other taken: the review closes, both comments open still, each
    // its reply in its thread.
    Db::edit(&e.db, |d| stacked::held::take(d, "r1", 1 - k)).unwrap();
    assert_eq!(e.ok(&["landed", "s"]), "");
    assert!(e.ok(&["runs", "s"]).ends_with("\tdone\treviewed: 2 replies accepted\n"));
    let read = e.ok(&["comments", "s", "--show", "to-read"]);
    assert!(read.contains("agent: For one.") && read.contains("agent: For two."), "{read}");
    // Closed while its reply waits: taking the reply closes the review all
    // the same.
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let claude = fake_claude(&e, "claude-answer.sh", "", r#"{"replies": {"c1": "Because."}}"#);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{out}{err}");
    let d = Db::load(&e.db).unwrap();
    let run = stacked::held::queue(&d, "s")[0].id.clone();
    let n = d.find_note("s", &id).unwrap();
    close_comment(&e, &(n.change.clone(), n.id.clone()));
    Db::edit(&e.db, |d| stacked::held::take(d, &run, 0)).unwrap();
    assert_eq!(e.ok(&["landed", "s"]), "");
    let runs = e.ok(&["runs", "s"]);
    let last = runs.lines().last().unwrap_or_default();
    assert!(last.starts_with(&format!("{run}\t")) && last.ends_with("\tdone\treviewed: 1 reply accepted"), "{runs}");
    // Moved to the stack's comments as its reply is taken, the comment is
    // gone from where it was asked: the review closes.
    let id = comment(&e, &["--patch", "2", "Add", "c", "too."]);
    let promote = fake_claude(&e, "claude-promote.sh", "", r#"{"replies": {"c1": "Patch 3 has to change too."}, "promote": "patch 3 has to change too"}"#);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &[("STACKED_CLAUDE", &promote)]);
    assert!(fine && out.contains("waits for your review: 1 reply"), "{out}{err}");
    let run = stacked::held::queue(&Db::load(&e.db).unwrap(), "s")[0].id.clone();
    Db::edit(&e.db, |d| stacked::held::take(d, &run, 0)).unwrap();
    assert_eq!(e.ok(&["landed", "s"]), "");
    let comments = e.ok(&["comments", "s"]);
    assert!(comments.contains("\tthe stack\tmoved from patch 2") && comments.contains("agent(high): Patch 3 has to change too."), "{comments}");
}

#[test]
fn a_reply_is_taken_at_once_only_when_replies_are_all_its_work_holds() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["Why?"]);
    let d = Db::load(&e.db).unwrap();
    let n = d.find_note("s", &id).map(|n| (n.change.clone(), n.id.clone())).unwrap();
    // A reply, and a comment the agent opened: not taken at once, but
    // reviewed together.
    let opened = r#"{"replies": {"c1": "Because."}, "comments": [{"patch": 1, "file": "a.txt", "side": "+", "line": "2", "text": "Say two."}]}"#;
    landed_answer(&e, "r1", &[n], opened);
    let refused = Db::edit(&e.db, |d| stacked::held::take(d, "r1", 0)).unwrap_err();
    assert_eq!(refused.0, "this reply goes with the comments the agent opened: review them together");
    assert!(e.ok(&["comments", "s", "--show", "to-review"]).contains(&id), "its reply waits still");
    assert_eq!(e.ok(&["review", "s", "--accept-all"]), "r1\treviewed: 1 opened comment accepted; 1 reply accepted\n");
    assert_eq!(e.ok(&["landed", "s"]), "");
}

#[test]
fn an_answer_opens_a_comment_for_each_issue_several_on_a_line() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let prompt = e.root.join("prompt.txt");
    let claude = fake_claude(
        &e,
        "claude-two.sh",
        &format!("printf '%s' \"$2\" > {}", prompt.display()),
        r#"{"replies": {"c1": "Because."}, "comments": [{"patch": 1, "file": "a.txt", "side": "+", "line": "2", "text": "Say two."}, {"patch": 1, "file": "a.txt", "side": "+", "line": "2", "text": "And test it."}]}"#,
    );
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait", "--accept"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{out}{err}");
    // Told one issue a comment, it opens two on one line: both there, apart.
    let told = std::fs::read_to_string(&prompt).unwrap();
    assert!(told.contains("Each comment you open is about one issue: a line with two issues gets two comments"), "{told}");
    let shown = e.ok(&["comments", "s", "--patch", "1"]);
    assert_eq!(shown.lines().filter(|l| l.contains("\tpatch 1 · a.txt: 2")).count(), 2, "{shown}");
    assert!(shown.contains("agent(high): Say two.") && shown.contains("agent(high): And test it."), "{shown}");
}

#[test]
fn landed_work_with_what_you_wrote_in_its_review_goes_back_to_the_agent_from_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let prompts = e.root.join("prompts.txt");
    // A first round makes 2 `deux`; a second, finding `deux` where the first
    // left it, makes it `2 spelled`.
    let claude = fake_claude(
        &e,
        "claude-rounds.sh",
        &format!(
            "set -e\nif grep -qx deux a.txt; then {spelled}; else {deux}; fi\n\
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a\nprintf '%s\\n=====\\n' \"$2\" >> {}",
            prompts.display(),
            spelled = sed_i("s/^deux$/2 spelled/", "a.txt"),
            deux = sed_i("s/^2$/deux/", "a.txt")
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    );
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let work = e.ok(&["landed", "s"]).split('\t').next().unwrap().to_string();
    // In its review: a comment on its new line, an answer to its reply.
    let mine = comment(&e, &["--patch", "1", "--line", "a.txt:+deux", "--work", &work[..8], "Not", "French."]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["reply", "s", &id, "--answer", "Not", "like", "this."], "", &env);
    assert!(fine, "{err}");
    // Not accepted, nor rejected: it goes back to the agent.
    let (fine, _, err) = e.run(&["review", "s", "--accept-all"]);
    assert!(!fine && err.contains("sent back to the agent"), "{err}");
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--send-back"], "", &env);
    assert!(fine && out.contains("sent back to the agent (may change the code): 2 comments to ask again"), "{out}{err}");
    landed(&e, 2);
    // The second round went on from the first one's work, told so: what it
    // changed then, as a stack diff, and how to print the stack it made, as
    // a round asked again is told; its work lands in place of the first's.
    let told = std::fs::read_to_string(&prompts).unwrap();
    let second = told.split("=====").nth(1).unwrap();
    assert!(second.contains("This is another round") && second.contains("Not French.") && second.contains("Not like this."), "{second}");
    assert!(second.contains(&format!("export s --work {work}` prints the stack you made")), "{second}");
    let diff = second.split("What was changed then, as a stack diff:\n").nth(1).unwrap_or_else(|| panic!("{second}"));
    assert!(diff.contains("-2") && diff.contains("+deux"), "{second}");
    // A change is told too that a tool refused is not allowed.
    let first = told.split("=====").next().unwrap();
    assert!(first.contains("that the tool was not allowed, naming it"), "{first}");
    let landed_now = e.ok(&["landed", "s"]);
    assert_eq!(landed_now.lines().filter(|l| !l.starts_with(' ')).count(), 1, "{landed_now}");
    assert!(!landed_now.starts_with(&work), "{landed_now}");
    assert!(e.ok(&["runs", "s"]).contains("reviewed: sent back to the agent, 2 comments to ask again"));
    // Accepted: the change of both rounds, and the thread as it went.
    e.ok(&["review", "s", "--accept-all"]);
    assert!(e.ok(&["show", "s", "1"]).contains("+2 spelled\n"));
    let shown = e.ok(&["comments", "s"]);
    let thread: Vec<&str> = shown.lines().skip_while(|l| !l.starts_with(&id)).skip(1).take_while(|l| l.starts_with(' ')).collect();
    assert_eq!(thread, ["  you: Spell it out.", "  agent(high): Done.", "  you: Not like this.", "  agent(high): Done."], "{shown}");
    assert!(shown.contains(&mine), "{shown}");
}

#[test]
fn a_comment_on_a_patch_the_work_adds_goes_back_to_the_agent_with_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let prompts = e.root.join("prompts.txt");
    // A first round adds a patch of its own over patch 1, making c.txt; a
    // second finds it there, and answers.
    let claude = fake_claude(
        &e,
        "claude-adds.sh",
        &format!(
            "set -e\nif ! test -e c.txt; then printf 'cee\\n' > c.txt && git add c.txt && git -c user.name=C -c user.email=c@x commit -q -m 'Bug 1 - a c too'; fi\n\
             printf '%s\\n=====\\n' \"$2\" >> {}",
            prompts.display()
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    );
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let work = e.ok(&["landed", "s"]).split('\t').next().unwrap().to_string();
    assert!(e.ok(&["show", "s", "--work", &work[..8]]).contains("Bug 1 - a c too"), "the work adds patch 2");
    // In its review, a comment on that patch: the work goes back to the
    // agent with it, neither accepted nor rejected.
    let mine = comment(&e, &["--patch", "2", "--line", "c.txt:+cee", "--work", &work[..8], "Why", "a", "c?"]);
    for all in ["--accept-all", "--reject-all"] {
        let (fine, _, err) = e.run(&["review", "s", all]);
        assert!(!fine && err.contains("sent back to the agent"), "{all}: {err}");
    }
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--send-back"], "", &env);
    assert!(fine && out.contains(": 1 comment to ask again"), "{out}{err}");
    landed(&e, 2);
    // The next round is asked about it, on that patch of the stack it goes on
    // from; accepted, the stack holds the patch, and the comment its reply.
    let told = std::fs::read_to_string(&prompts).unwrap();
    let second = told.split("=====").nth(1).unwrap();
    assert!(second.contains("Why a c?") && second.contains("patch 2, c.txt line `cee`"), "{second}");
    e.ok(&["review", "s", "--accept-all"]);
    assert!(e.ok(&["show", "s"]).contains("Bug 1 - a c too"));
    let shown = e.ok(&["comments", "s"]);
    let thread: Vec<&str> = shown.lines().skip_while(|l| !l.starts_with(&mine)).skip(1).take_while(|l| l.starts_with(' ')).collect();
    assert_eq!(thread, ["  you: Why a c?", "  agent(high): Done."], "{shown}");
}

#[test]
fn a_round_sent_back_goes_first_and_the_stack_waits_for_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let (prompts, gate) = (e.root.join("prompts.txt"), e.root.join("gate"));
    // A change makes 2 `deux`, then `2 spelled`; the answer waits for the
    // gate. Each says what it was asked.
    let claude = fake_claude(
        &e,
        "claude-first.sh",
        &format!(
            "set -e\nprintf '%s\\n=====\\n' \"$2\" >> {p}\ncase \"$2\" in\n  *'Why b?'*) until test -e {g}; do sleep 0.1; done ;;\n  \
             *) if grep -qx deux a.txt; then {spelled}; else {deux}; fi\n     \
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a ;;\nesac",
            p = prompts.display(),
            g = gate.display(),
            spelled = sed_i("s/^deux$/2 spelled/", "a.txt"),
            deux = sed_i("s/^2$/deux/", "a.txt")
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    );
    // Unchained: a change waits for the review of the one before it.
    let unchained = e.root.join("unchained.toml");
    std::fs::write(&unchained, format!("{HOME_SETTINGS}chain = false\n")).unwrap();
    let env = [("STACKED_CLAUDE", claude.as_path()), ("STACKED_CONFIG", unchained.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    // While its work waits for review, another change waits its turn, and an
    // answer goes.
    let other = comment(&e, &["--patch", "3", "And", "this."]);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &other], "", &env);
    assert!(fine && out.contains("it goes once you have reviewed the agent's change of patch 1"), "{out}{err}");
    let why = comment(&e, &["--patch", "2", "Why", "b?"]);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &why, "--answer-only"], "", &env);
    assert!(fine, "{out}{err}");
    // Sent back: the round waits for the answer, and the stack for the round.
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["reply", "s", &id, "--answer", "Not", "like", "this."], "", &env);
    assert!(fine, "{err}");
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--send-back"], "", &env);
    assert!(fine, "{out}{err}");
    let (fine, _, err) = e.run(&["drop", "s", "3"]);
    assert!(!fine && err.contains("the work you sent back waits to go back to the agent"), "{err}");
    // The answer lands: the round goes next, before the change sent before it.
    std::fs::write(&gate, "").unwrap();
    landed(&e, 3);
    let told = std::fs::read_to_string(&prompts).unwrap();
    let asked: Vec<&str> = told.split("=====").collect();
    assert!(asked.len() == 4 && asked[2].contains("This is another round"), "{told}");
    assert!(!told.contains("And this."), "the change sent before it waits:\n{told}");
}

/// A claude for the chain: a change makes 2 `deux`, then `2 spelled` (once
/// `gate` is there, if given); an answer, `Why b?`, changes nothing. Each
/// writes what it was asked in `prompts`.
fn chain_claude(e: &Env, prompts: &Path, gate: Option<&Path>) -> PathBuf {
    let wait = gate.map_or(String::new(), |g| format!("until test -e {}; do sleep 0.1; done; ", g.display()));
    fake_claude(
        e,
        "claude-chain.sh",
        &format!(
            "set -e\nprintf '%s\\n=====\\n' \"$2\" >> {p}\ncase \"$2\" in\n  *'Why b?'*) ;;\n  \
             *) if grep -qx deux a.txt; then {wait}{spelled}; else {deux}; fi\n     \
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a ;;\nesac",
            p = prompts.display(),
            spelled = sed_i("s/^deux$/2 spelled/", "a.txt"),
            deux = sed_i("s/^2$/deux/", "a.txt")
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    )
}

/// The runs waiting for review on `s`, by id, the first landed first.
fn landed_ids(e: &Env) -> Vec<String> {
    e.ok(&["landed", "s"]).lines().filter(|l| !l.starts_with(' ')).map(|l| l.split('\t').next().unwrap().to_string()).collect()
}

/// Whether process `pid` is still there, given as long as a test waits to
/// go.
fn still_there(pid: u32) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < PATIENCE {
        let there = std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        if !there {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
}

#[test]
fn a_change_goes_on_from_the_change_under_review_and_is_reviewed_after_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let first = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let second = comment(&e, &["--patch", "1", "Spell", "it", "more."]);
    let prompts = e.root.join("prompts.txt");
    let claude = chain_claude(&e, &prompts, None);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &first, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let one = landed_ids(&e).remove(0);
    // While it waits for review, the next change goes at once, from the stack
    // it makes: it finds `deux` where the first left it.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &second, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let told = std::fs::read_to_string(&prompts).unwrap();
    assert!(told.split("=====").nth(1).unwrap().contains(&format!("export s --work {one}")), "{told}");
    // Reviewed after it, not before, said so.
    let ids = landed_ids(&e);
    assert_eq!(ids.len(), 2);
    assert!(e.ok(&["landed", "s"]).contains(&format!("goes on from {one}, which waits for your review")));
    let (fine, _, err) = e.run(&["review", "s", &ids[1], "--accept-all"]);
    assert!(!fine && err.contains("review that one first"), "{err}");
    // The first accepted, the second is a change of the stack as any.
    e.ok(&["review", "s", "--accept-all"]);
    assert!(!e.ok(&["landed", "s"]).contains("goes on from"));
    e.ok(&["review", "s", "--accept-all"]);
    assert!(e.ok(&["show", "s", "1"]).contains("+2 spelled\n"));
}

#[test]
fn what_went_on_from_a_change_rejected_is_dropped_its_agent_stopped_and_asked_again() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let first = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let second = comment(&e, &["--patch", "1", "Spell", "it", "more."]);
    let (prompts, gate) = (e.root.join("prompts.txt"), e.root.join("gate"));
    let claude = chain_claude(&e, &prompts, Some(&gate));
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &first, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    // The next goes on from it, and works on, waiting for the gate.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &second], "", &env);
    assert!(fine, "{out}{err}");
    let (guess, pid) = eventually(
        || {
            let d = Db::load(&e.db).unwrap();
            d.runs().into_iter().find(|r| r.state == RunState::Started).and_then(|r| Some((r.id.clone(), r.pid?)))
        },
        || "the guess never worked".into(),
    );
    // Rejected: what went on from it is dropped, its agent stopped, and it
    // is asked again, from the stack as it is.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--reject-all"], "", &env);
    assert!(fine, "{out}{err}");
    landed(&e, 3);
    assert!(!still_there(pid), "its agent is stopped");
    let d = Db::load(&e.db).unwrap();
    let dropped = d.run(&guess).unwrap().clone();
    drop(d);
    assert!(dropped.state == RunState::Failed && dropped.summary == "dropped: the work it went on from was rejected; asked again", "{dropped:?}");
    assert_eq!(landed_ids(&e).len(), 1);
    let told = std::fs::read_to_string(&prompts).unwrap();
    assert!(!told.contains("This was asked before"), "nothing of it landed:\n{told}");
    e.ok(&["review", "s", "--accept-all"]);
    let shown = e.ok(&["show", "s", "1"]);
    assert!(shown.contains("+deux\n") && !shown.contains("spelled"), "{shown}");
    std::fs::write(&gate, "").unwrap();
}

/// The rev a prompt reads the stack at: its `--rev N`.
fn rev_in(prompt: &str) -> String {
    prompt.split("--rev ").nth(1).map(|r| r.chars().take_while(char::is_ascii_digit).collect()).unwrap_or_default()
}

#[test]
fn an_answer_runs_on_the_stack_as_it_is_and_accepting_an_edit_asks_it_again() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let first = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let why = comment(&e, &["--patch", "2", "Why", "b?"]);
    let prompts = e.root.join("prompts.txt");
    let claude = chain_claude(&e, &prompts, None);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &first, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let one = landed_ids(&e).remove(0);
    // The answer goes at once, on the stack as it is, not the change's: in
    // a worktree of its own, the stack up to its patch, the whole stack as
    // it is printed by `export --rev`.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &why, "--answer-only", "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let told = std::fs::read_to_string(&prompts).unwrap();
    let asked = told.split("=====").nth(1).unwrap();
    assert!(asked.contains("a git worktree made for you") && asked.contains("export s --rev ") && !asked.contains("--work"), "{told}");
    assert!(e.ok(&["show", "s", "1", "--work", &one]).contains("+deux\n"));
    assert!(!e.ok(&["landed", "s"]).contains("goes on from"));
    // The change accepted, the answer is asked again, on the stack it makes.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", &one, "--accept-all"], "", &env);
    assert!(fine && out.contains("; 1 answer-only ask asked again"), "{out}{err}");
    landed(&e, 3);
    let told = std::fs::read_to_string(&prompts).unwrap();
    let asked: Vec<&str> = told.split("=====").collect();
    assert!(asked.len() == 4 && asked[2].contains("Why b?"), "{told}");
    assert_ne!(rev_in(asked[1]), rev_in(asked[2]), "{told}");
    assert!(asked[2].contains("This was asked before") && asked[2].contains("What was answered then to c1:\nDone."), "{told}");
    assert!(e.ok(&["runs", "s"]).contains("dropped: an edit of the stack was accepted meanwhile; asked again"));
    assert_eq!(landed_ids(&e).len(), 1);
}

#[test]
fn an_answer_runs_alongside_a_running_edit_and_rejecting_the_edit_keeps_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let first = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let why = comment(&e, &["--patch", "2", "Why", "b?"]);
    let (prompts, gate) = (e.root.join("prompts.txt"), e.root.join("gate"));
    // The change waits for the gate; the answer does not.
    let claude = fake_claude(
        &e,
        "claude-alongside.sh",
        &format!(
            "set -e\nprintf '%s\\n=====\\n' \"$2\" >> {p}\ncase \"$2\" in\n  *'Why b?'*) ;;\n  \
             *) until test -e {g}; do sleep 0.1; done; {deux}\n     \
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a ;;\nesac",
            p = prompts.display(),
            g = gate.display(),
            deux = sed_i("s/^2$/deux/", "a.txt")
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    );
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &first], "", &env);
    assert!(fine, "{out}{err}");
    // While the change works, the answer goes, and lands.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &why, "--answer-only", "--wait"], "", &env);
    assert!(fine && !out.contains("once the agent's change"), "{out}{err}");
    let answer = landed_ids(&e);
    assert_eq!(answer.len(), 1);
    // The change lands; rejected, the stack is as it was, and the answer
    // stays.
    std::fs::write(&gate, "").unwrap();
    landed(&e, 2);
    let change = landed_ids(&e).into_iter().find(|id| *id != answer[0]).unwrap();
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", &change, "--reject-all"], "", &env);
    assert!(fine && !out.contains("asked again"), "{out}{err}");
    assert_eq!(landed_ids(&e), answer);
}

#[test]
fn what_went_on_from_a_change_sent_back_starts_again_from_its_round() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let first = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    let second = comment(&e, &["--patch", "1", "Spell", "it", "more."]);
    let prompts = e.root.join("prompts.txt");
    let claude = chain_claude(&e, &prompts, None);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &first, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &second, "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    // Sent back: what went on from it is dropped; the round goes first, and
    // the second again after it, going on from the round's work.
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["reply", "s", &first, "--answer", "Not", "like", "this."], "", &env);
    assert!(fine, "{err}");
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", "--send-back"], "", &env);
    assert!(fine, "{out}{err}");
    landed(&e, 4);
    let told = std::fs::read_to_string(&prompts).unwrap();
    let asked: Vec<&str> = told.split("=====").collect();
    assert!(asked.len() == 5 && asked[2].contains("This is another round"), "{told}");
    let ids = landed_ids(&e);
    assert_eq!(ids.len(), 2, "the round's work, and the second's on it");
    assert!(asked[3].contains("Spell it more.") && asked[3].contains(&format!("--work {}", ids[0])), "{told}");
    let runs = e.ok(&["runs", "s"]);
    let dropped = runs.lines().find(|l| l.contains("dropped: the work it went on from was sent back; asked again")).unwrap();
    let dropped = dropped.split('\t').next().unwrap();
    assert!(asked[3].contains("This was asked before, going on from an edit that was then dropped"), "{told}");
    assert!(asked[3].contains(&format!("export s --work {dropped}")) && asked[3].contains("++2 spelled"), "{told}");
}

#[test]
fn the_agent_rebases_reviews_and_tests() {
    let e = env();
    // A commit on main for the stack to go onto.
    git(&e.src, &["checkout", "-q", "-b", "upstream", "base"]);
    std::fs::write(e.src.join("z.txt"), "z\n").unwrap();
    git(&e.src, &["add", "."]);
    git(&e.src, &["commit", "-qm", "upstream"]);
    git(&e.src, &["checkout", "-q", "main"]);
    e.ok(&["import", "s", "base"]);

    let onto = git(&e.src, &["rev-parse", "upstream"]).trim().to_string();
    let base = git(&e.src, &["rev-parse", "base"]).trim().to_string();
    let rebase = fake_claude(
        &e,
        "claude-rebase.sh",
        &format!("git rebase -q --onto {onto} {base}"),
        &format!(r#"Rebased. {{"replies": {{"c1": "Rebased."}}, "base": "{onto}"}}"#),
    );
    let id = comment(&e, &["Rebase", "onto", "upstream."]);
    let (fine, _, err) = send(&e, &rebase, &[&id], &[]);
    assert!(fine, "{err}");
    let mail = e.ok(&["export", "s"]);
    assert!(!mail.contains("Subject: upstream"), "the commit it went onto is no patch");
    let dst = e.root.join("dst");
    git(&e.root, &["clone", "-q", e.src.to_str().unwrap(), "dst"]);
    git(&dst, &["checkout", "-q", &onto]);
    let mut am = Command::new("git").args(["-c", "commit.gpgsign=false", "am", "-q"]).current_dir(&dst).stdin(Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    am.stdin.take().unwrap().write_all(mail.as_bytes()).unwrap();
    assert!(am.wait().unwrap().success(), "the stack applies onto upstream");
    assert!(e.ok(&["log", "s"]).contains(&format!("agent: rebase onto {}", &onto[..12])));
    assert!(e.ok(&["stacks"]).contains("3 patches"));

    // A review is an answer that opens comments of its own. As any answer,
    // it runs in the stack's own worktree for answers, the stack applied
    // there, never in your checkout, and is told so.
    let review = fake_claude(
        &e,
        "claude-review.sh",
        r#"case "$PWD" in */.stacked/s/answer-only-agent) ;; *) exit 3;; esac; test "$PWD" != "$STACKED_SRC" || exit 4; test "$(git rev-list --count HEAD ^upstream)" = 3 || exit 5"#,
        r#"{"replies": {"c1": "Two things."}, "comments": [{"patch": 2, "file": "b.txt", "side": "+", "line": "new", "text": "Say what b is for."},
            {"patch": 1, "file": "nowhere", "side": "+", "line": "?", "text": "On the whole patch."}]}"#,
    );
    let id = comment(&e, &["Review", "it."]);
    let (fine, out, err) = send(&e, &review, &[&id, "--answer-only"], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    let db = Db::load(&e.db).unwrap();
    let info = stacked::ask::info(db.runs().last().unwrap()).unwrap();
    assert!(info.worktree.as_deref().is_some_and(|w| w.ends_with("/.stacked/s/answer-only-agent")), "{:?}", info.worktree);
    assert!(info.prompt.contains("The current directory is a git worktree made for you"), "{}", info.prompt);
    assert!(!info.prompt.contains("the user's repository"), "{}", info.prompt);
    drop(db);
    assert!(out.contains("agent replied (answer only, at high) · opened 2 comments"), "{out}");
    let c = e.ok(&["comments", "s", "--patch", "2"]);
    assert!(c.contains("b.txt: new") && c.contains("agent(high): Say what b is for."), "{c}");
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("patch 1, as a whole") && c.contains("On the whole patch."), "{c}");
    assert!(e.ok(&["runs"]).contains("reviewed: 2 opened comments accepted; 1 reply accepted"));

    // A build and test runs in the stack's build worktree, the stack applied
    // in it: there it builds, leaving what it made.
    let test = fake_claude(
        &e,
        "claude-test.sh",
        r#"case "$PWD" in */.stacked/s/build) ;; *) exit 3;; esac; test "$(git rev-list --count HEAD ^upstream)" = 3 || exit 4; echo o > built.o"#,
        r#"{"replies": {"c1": "Built them."}, "patches": {"1": "builds; 3 tests pass", "3": "fails: a.txt"}}"#,
    );
    let id = comment(&e, &["Build", "and", "test."]);
    let (fine, _, err) = send(&e, &test, &[&id, "--build-and-test"], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    let runs = e.ok(&["runs"]);
    assert!(runs.lines().last().is_some_and(|l| l.ends_with("reviewed: 2 opened comments accepted; 1 reply accepted")), "{runs}");
    assert!(e.ok(&["comments", "s", "--patch", "3"]).contains("  agent(high): fails: a.txt (tested @"), "a finding, at the effort the build and test was asked at");
    let db = Db::load(&e.db).unwrap();
    let info = stacked::ask::info(db.runs().last().unwrap()).unwrap();
    assert!(info.worktree.as_deref().is_some_and(|w| w.ends_with("/.stacked/s/build")), "{:?}", info.worktree);
    assert!(info.prompt.contains("a git worktree of the stack's own, for its builds"), "{}", info.prompt);
    drop(db);
    // The next build takes it again, and builds on what is there.
    let again = fake_claude(
        &e,
        "claude-test-again.sh",
        r#"case "$PWD" in */.stacked/s/build) ;; *) exit 3;; esac; test -e built.o || exit 5"#,
        r#"{"replies": {"c1": "Built again."}}"#,
    );
    let id = comment(&e, &["Build", "again."]);
    let (fine, _, err) = send(&e, &again, &[&id, "--build-and-test"], &[]);
    assert!(fine, "{err}");
    // A line's comment is not for building and testing.
    let line = comment(&e, &["--patch", "2", "--line", "b.txt:+new", "Test", "this."]);
    let (fine, _, err) = e.run(&["send", "s", &line, "--build-and-test"]);
    assert!(!fine && err.contains("only a comment on the stack or on a patch as a whole"), "{err}");

    // A claude that fails changes nothing but says so, and the comment can
    // be sent again.
    let bad = e.script("claude-bad.sh", "echo oops >&2; exit 7");
    let id = comment(&e, &["Anything."]);
    let (fine, out, err) = send(&e, &bad, &[&id], &[]);
    assert!(!fine && err.contains("1 of them failed"), "{err}");
    assert!(out.contains("ask failed: the agent failed (7): oops"), "{out}");
    assert!(e.ok(&["comments", "s", "--show", "to-send"]).contains(&id));
}

/// Stack s on `tbase`, a commit adding t.txt (thirty lines, none starting
/// as git's headings do, with a letter): patch 1 adds `top` under line 2,
/// patch 2 adds c.txt, patch 3 changes line 25. Then patch 1's new version
/// adds twelve lines under `top`, and patch 3 keeps the diff it was stored
/// with, as a patch above an edited one does: git applies it twelve lines
/// down, where it would write its hunk with other numbers, under another
/// heading, and with other blobs on its `index` line. What `show s 3` says
/// of it, as stored.
fn lines_moved_below_patch_3(e: &Env) -> String {
    let t: String = (1..=30).map(|n| format!("    line {n}\n")).collect();
    git(&e.src, &["checkout", "-q", "-b", "moved", "base"]);
    std::fs::write(e.src.join("t.txt"), &t).unwrap();
    git(&e.src, &["add", "t.txt"]);
    git(&e.src, &["commit", "-qm", "t"]);
    git(&e.src, &["tag", "tbase"]);
    let top = t.replace("    line 2\n", "    line 2\ntop\n");
    std::fs::write(e.src.join("t.txt"), &top).unwrap();
    git(&e.src, &["commit", "-qam", "Bug 11 - top"]);
    std::fs::write(e.src.join("c.txt"), "c\n").unwrap();
    git(&e.src, &["add", "c.txt"]);
    git(&e.src, &["commit", "-qm", "Bug 12 - add c"]);
    std::fs::write(e.src.join("t.txt"), top.replace("    line 25\n", "    LINE 25\n")).unwrap();
    git(&e.src, &["commit", "-qam", "Bug 13 - line 25"]);
    e.ok(&["import", "s", "tbase"]);
    // Patch 1's new version, as git writes it.
    git(&e.src, &["checkout", "-q", "--detach", "tbase"]);
    let more: String = (1..=12).map(|n| format!("more {n}\n")).collect();
    std::fs::write(e.src.join("t.txt"), t.replace("    line 2\n", &format!("    line 2\ntop\n{more}"))).unwrap();
    git(&e.src, &["commit", "-qam", "Bug 11 - top"]);
    let mail = git(&e.src, &["format-patch", "-1", "--stdout", "HEAD"]);
    git(&e.src, &["checkout", "-q", "main"]);
    let draft = parse_mbox(&mail).unwrap().remove(0);
    Db::edit(&e.db, |db| ops::replace(db, "s", 0, draft, "edit")).unwrap();
    let stored = e.ok(&["show", "s", "3"]);
    assert!(stored.contains("@@ -23,7 +23,7 @@ top\n"), "{stored}");
    stored
}

#[test]
fn a_patch_the_agent_s_rebase_only_moved_keeps_its_version() {
    let e = env();
    let stored = lines_moved_below_patch_3(&e);
    let before = e.column(2);
    // Patch 2 reworded, every commit made again on the way, as a rebase of
    // the whole stack makes them: patch 3's diff comes back as git writes
    // it now, `@@ -35,7 +35,7 @@ more 12`.
    let claude = fake_claude(
        &e,
        "claude-reword.sh",
        r#"set -e
todo=$(printf '%s\n' "$2" | sed -n 's/^The rebase todo file: //p')
a=$(git rev-parse HEAD~2) b=$(git rev-parse HEAD~1) c=$(git rev-parse HEAD)
printf 'pick %s\nedit %s\npick %s\n' $a $b $c > "$todo"
git -c user.name=C -c user.email=c@x rebase -q -i --force-rebase HEAD~3 >&2
git -c user.name=C -c user.email=c@x commit -q --amend -m "Bug 12 - add c, said otherwise"
git -c user.name=C -c user.email=c@x rebase --continue >&2
git format-patch -1 --stdout HEAD | grep -q '^@@ -35,7 +35,7 @@ more 12$'"#,
        r#"{"replies": {"c1": "Reworded patch 2."}}"#,
    );
    let id = comment(&e, &["Reword", "patch", "2."]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    assert!(e.ok(&["runs"]).contains("reviewed: its change accepted; 1 reply accepted"));
    // Only patch 2 is a new version: patch 3 is the patch it was, its diff
    // as stored.
    let after = e.column(2);
    assert_eq!([after[0] == before[0], after[1] == before[1], after[2] == before[2]], [true, false, true], "{before:?} {after:?}");
    assert_eq!(e.column(3)[1], "Bug 12 - add c, said otherwise");
    assert_eq!(e.ok(&["versions", "s", "3"]).lines().count(), 1);
    assert_eq!(e.ok(&["show", "s", "3"]), stored);
    // The stack diff of the change marks patch 2 alone, reworded.
    let db = Db::load(&e.db).unwrap();
    let diff = stacked::history::what_changed(&db, db.head("s").unwrap().0);
    assert_eq!(stacked::history::summary(&diff), "~1");
    let changed: Vec<(usize, bool)> = diff
        .iter()
        .filter_map(|d| match d {
            stacked::history::Diffed::Patch { at, changed: true, reworded, .. } => Some((*at, *reworded)),
            _ => None,
        })
        .collect();
    assert_eq!(changed, [(2, true)]);
}

#[test]
fn an_agent_s_change_of_a_patch_that_only_moves_its_lines_changes_nothing() {
    let e = env();
    let stored = lines_moved_below_patch_3(&e);
    let before = e.column(2);
    // Patch 3's commit left as it was, then made again at another time:
    // either way, its diff comes back as git writes it now, and the change
    // holds nothing that accepting it would change.
    for (name, body) in [
        ("claude-leave.sh", ""),
        ("claude-amend.sh", "git -c user.name=C -c user.email=c@x commit -q --amend --no-edit --date=now"),
    ] {
        let claude = fake_claude(&e, name, body, r#"{"replies": {"c1": "Nothing to change."}}"#);
        let id = comment(&e, &["--patch", "3", "Fine?"]);
        let (fine, out, err) = send(&e, &claude, &[&id], &[]);
        assert!(fine, "{name}: {out}{err}");
        let runs = e.ok(&["runs"]);
        assert!(runs.lines().last().is_some_and(|l| l.ends_with("\treviewed: 1 reply accepted")), "{name}: {runs}");
        assert_eq!(e.column(2), before, "{name}");
        assert_eq!(e.ok(&["versions", "s", "3"]).lines().count(), 1, "{name}");
        assert_eq!(e.ok(&["show", "s", "3"]), stored, "{name}");
    }
}

/// Messages that git writes otherwise than stacked stores them, each a
/// patch's, bottom first, with an author of its own for some: two empty
/// lines under the title, as the user's had once the sync of the commit
/// messages of before took them from the checkout (patch 2 is the one the agent rewords, patch 1 is below it);
/// empty lines in a row, before the trailer moz-phab writes too; white
/// space at the end of lines and of the title; a first paragraph of two
/// lines, which git takes for a title and a body; carriage returns; a
/// prefix, and runs of spaces, in the title; a newline and empty lines at
/// the end; a line of three dashes, where `git am` ends the message; a
/// line of the body that it takes for the author; an author's name that
/// format-patch quotes. Then two that git writes as they are: lines that
/// start with `#`, and a title that is not ASCII, long enough to be folded.
const AS_GIT_WRITES: &[(&str, Option<&str>)] = &[
    ("Bug 1 - below the reworded patch\n\n\nTwo empty lines under the title.", None),
    ("Bug 2 - to reword\n\n\nTwo empty lines under the title, here too.", None),
    ("Bug 3 - two empty lines under the title\n\n\nAs the user's messages had them, taken from the checkout by a sync.", None),
    (
        "Bug 4 - empty lines in a row\n\nThe first paragraph.\n\n\n\nThe second, three empty lines above it.\n\n\nDifferential Revision: https://phabricator.services.mozilla.com/D1234",
        None,
    ),
    ("Bug 5 - white space at the end of lines  \n\nTwo spaces end this line.  \nA tab ends this one.\t\n   \nAbove, a line of spaces.", None),
    ("Bug 6 - a first paragraph\nof two lines\n\nThen the body.", None),
    ("Bug 7 - carriage returns\r\n\r\nEach line ends with one.\r\nThis one too.\r\n", None),
    ("[PATCH v2] Bug 8 - a title after a prefix\n\nAnd a body.", None),
    ("Bug 9 -  runs  of   spaces\n\nIn the title.", None),
    ("Bug 10 - a newline and empty lines at the end\n\nThe body.\n\n\n", None),
    ("Bug 11 - three dashes\n\nAbove them.\n---\nBelow them, which git am leaves out.", None),
    ("Bug 12 - a line taken for the author\n\nFrom: Someone Else <else@example.com>\nThe line above starts the body.", None),
    ("Bug 13 - an author's name that format-patch quotes\n\nThe body.", Some("Thor, A. U. <author@example.com>")),
    ("Bug 14 - lines starting with a hash\n\n# Not a comment.\n#Nor this one.", None),
    ("Bug 15 - café, a title that is not ASCII, long enough that format-patch folds it over more than one line\n\nÜnïcödé.", None),
];

/// Stack s on `base`, a patch for each of `messages` adding a file of its
/// own, with that message, and that author if one is given: the ids of its
/// patches.
fn stack_of_messages(e: &Env, messages: &[(&str, Option<&str>)]) -> Vec<String> {
    git(&e.src, &["checkout", "-q", "-b", "messages", "base"]);
    for k in 0..messages.len() {
        std::fs::write(e.src.join(format!("m{}.txt", k + 1)), format!("{}\n", k + 1)).unwrap();
        git(&e.src, &["add", "."]);
        git(&e.src, &["commit", "-qm", &format!("Bug {} - m{}", k + 1, k + 1)]);
    }
    e.ok(&["import", "s", "base"]);
    git(&e.src, &["checkout", "-q", "main"]);
    Db::edit(&e.db, |db| {
        for (k, (message, author)) in messages.iter().enumerate() {
            let old = db.patch(&db.stack("s")?.patches[k]).clone();
            let author = author.map(String::from).or(old.author.clone());
            ops::replace(db, "s", k, Draft { message: message.to_string(), author, ..Draft::from(&old) }, "edit")?;
        }
        Ok(())
    })
    .unwrap();
    let db = Db::load(&e.db).unwrap();
    db.stack("s").unwrap().patches.clone()
}

/// The patches the stack diff of stack s's last change marks as changed:
/// their places (from 1), and whether each was reworded.
fn changed_by_the_last_change(e: &Env) -> (String, Vec<(usize, bool)>) {
    let db = Db::load(&e.db).unwrap();
    let diff = stacked::history::what_changed(&db, db.head("s").unwrap().0);
    let changed = diff
        .iter()
        .filter_map(|d| match d {
            stacked::history::Diffed::Patch { at, changed: true, reworded, .. } => Some((*at, *reworded)),
            _ => None,
        })
        .collect();
    (stacked::history::summary(&diff), changed)
}

#[test]
fn patches_the_agent_s_rebase_did_not_touch_keep_their_versions_whatever_git_makes_of_their_messages() {
    let e = env();
    let before = stack_of_messages(&e, AS_GIT_WRITES);
    let n = AS_GIT_WRITES.len();
    // Patch 2 reworded in a rebase of the whole stack, as the user's agent
    // reworded patches in the middle of theirs: patch 1's commit left as it
    // was, those above made again on the new one. Each message comes back
    // as git wrote it, from `git am` on.
    let claude = fake_claude(
        &e,
        "claude-reword.sh",
        &format!(
            r#"set -e
todo=$(printf '%s\n' "$2" | sed -n 's/^The rebase todo file: //p')
git rev-list --reverse HEAD~{n}..HEAD | sed '2s/^/edit /;2!s/^/pick /' > "$todo"
git -c user.name=C -c user.email=c@x rebase -q -i HEAD~{n} >&2
git -c user.name=C -c user.email=c@x commit -q --amend -m "Bug 2 - reworded" -m "Two empty lines under the title, here too."
git -c user.name=C -c user.email=c@x rebase --continue >&2
git log -1 --format=%an HEAD~3 | grep -qx 'Someone Else'"#
        ),
        r#"{"replies": {"c1": "Reworded patch 2."}}"#,
    );
    let id = comment(&e, &["Reword", "patch", "2."]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    assert!(e.ok(&["runs"]).contains("reviewed: its change accepted; 1 reply accepted"));
    // Only patch 2 is a new version: every other patch is the patch it was,
    // below the reworded one or above it, its message and author as stored.
    let db = Db::load(&e.db).unwrap();
    let after = db.stack("s").unwrap().patches.clone();
    let kept: Vec<bool> = before.iter().zip(&after).map(|(b, a)| b == a).collect();
    let mut want = vec![true; n];
    want[1] = false;
    assert_eq!(kept, want, "{before:?} {after:?}");
    for (k, (message, author)) in AS_GIT_WRITES.iter().enumerate().filter(|(k, _)| *k != 1) {
        let p = db.patch(&after[k]);
        assert_eq!(p.message, *message, "patch {}", k + 1);
        assert_eq!(p.author.as_deref(), Some(author.unwrap_or("A U Thor <author@example.com>")), "patch {}", k + 1);
    }
    assert_eq!(db.patch(&after[1]).message, "Bug 2 - reworded\n\nTwo empty lines under the title, here too.");
    // The stack diff of the change marks patch 2 alone, reworded.
    assert_eq!(changed_by_the_last_change(&e), ("~1".to_string(), vec![(2, true)]));
}

#[test]
fn a_message_the_agent_changed_in_its_white_space_alone_is_a_new_version_reworded() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let before = e.column(2);
    // An empty line more between the two paragraphs of patch 2, its message
    // committed verbatim: git gives the message as stored back with one
    // empty line there, not two.
    let claude = fake_claude(
        &e,
        "claude-space.sh",
        r#"set -e
todo=$(printf '%s\n' "$2" | sed -n 's/^The rebase todo file: //p')
git rev-list --reverse HEAD~3..HEAD | sed '2s/^/edit /;2!s/^/pick /' > "$todo"
git -c user.name=C -c user.email=c@x rebase -q -i HEAD~3 >&2
printf 'Bug 2 - add b\n\nA body.\n\n\nMore body.\n' | git -c user.name=C -c user.email=c@x commit -q --amend --cleanup=verbatim -F -
git -c user.name=C -c user.email=c@x rebase --continue >&2"#,
        r#"{"replies": {"c1": "Spaced patch 2."}}"#,
    );
    let id = comment(&e, &["Space", "patch", "2."]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    let after = e.column(2);
    assert_eq!([after[0] == before[0], after[1] == before[1], after[2] == before[2]], [true, false, true], "{before:?} {after:?}");
    let db = Db::load(&e.db).unwrap();
    let s = db.stack("s").unwrap();
    assert_eq!(db.patch(&s.patches[1]).message, "Bug 2 - add b\n\nA body.\n\n\nMore body.");
    assert_eq!(changed_by_the_last_change(&e), ("~1".to_string(), vec![(2, true)]));
}

#[test]
fn a_patch_the_agent_left_keeps_its_message_as_stored_and_one_whose_code_it_changed_too() {
    let e = env();
    let messages = [
        ("Bug 1 - one\n\n\nTwo empty lines under the title.", None),
        ("Bug 2 - two  \n\n\nSpaces end the title, and two empty lines are under it.\n\nSo do spaces this line.  \nThe end.", None),
        ("Bug 3 - three\n\n\nTwo empty lines under the title.", None),
    ];
    let before = stack_of_messages(&e, &messages);
    // Patch 2's commit left as it was: nothing that accepting the change
    // would change.
    let claude = fake_claude(&e, "claude-leave.sh", "", r#"{"replies": {"c1": "Nothing to change."}}"#);
    let id = comment(&e, &["--patch", "2", "Fine?"]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    let runs = e.ok(&["runs"]);
    assert!(runs.lines().last().is_some_and(|l| l.ends_with("\treviewed: 1 reply accepted")), "{runs}");
    assert_eq!(Db::load(&e.db).unwrap().stack("s").unwrap().patches, before);
    // Its code changed, its message as git gave it back: its new version,
    // with the message as stored, changed and not reworded.
    let claude = fake_claude(
        &e,
        "claude-code.sh",
        "echo more >> m2.txt\ngit -c user.name=C -c user.email=c@x commit -q --amend -a --no-edit",
        r#"{"replies": {"c1": "More in m2.txt."}}"#,
    );
    let id = comment(&e, &["--patch", "2", "More?"]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    let db = Db::load(&e.db).unwrap();
    let after = db.stack("s").unwrap().patches.clone();
    assert_eq!([after[0] == before[0], after[1] == before[1], after[2] == before[2]], [true, false, true], "{before:?} {after:?}");
    let p = db.patch(&after[1]);
    assert_eq!((p.message.as_str(), p.author.as_deref()), (messages[1].0, Some("A U Thor <author@example.com>")));
    assert!(p.diff.contains("+more\n"), "{}", p.diff);
    assert_eq!(changed_by_the_last_change(&e), ("~1".to_string(), vec![(2, false)]));
}

/// t.txt with the same six lines twice, eight lines apart: `head`, the
/// first copy, eight lines of filler, the second copy, three more. With
/// `new_in`, `NEW` is added in that copy (1 or 2), after its `c`.
fn two_copies(new_in: Option<usize>) -> String {
    let copy = |n: usize| {
        let new = if new_in == Some(n) { "    NEW\n" } else { "" };
        format!("    a\n    b\n    c\n{new}    d\n    e\n    f\n")
    };
    let filler: String = (1..=8).map(|n| format!("    filler {n}\n")).collect();
    format!("    head\n{}{filler}{}    tail 1\n    tail 2\n    tail 3\n", copy(1), copy(2))
}

/// Stack s on `tbase`, a commit adding `two_copies(None)` as t.txt: patch
/// 1 changes its second line of filler, patch 2 adds c.txt, and patch 3
/// adds `NEW` in copy `new_in`. The checkout is left on main, the commits
/// on branch `copies`.
fn two_copies_stack(e: &Env, new_in: usize) {
    let t = two_copies(None);
    git(&e.src, &["checkout", "-q", "-b", "copies", "base"]);
    std::fs::write(e.src.join("t.txt"), &t).unwrap();
    git(&e.src, &["add", "t.txt"]);
    git(&e.src, &["commit", "-qm", "t"]);
    git(&e.src, &["tag", "tbase"]);
    let filler = |t: String| t.replace("    filler 2\n", "    FILLER 2\n");
    std::fs::write(e.src.join("t.txt"), filler(t)).unwrap();
    git(&e.src, &["commit", "-qam", "Bug 21 - filler two"]);
    std::fs::write(e.src.join("c.txt"), "c\n").unwrap();
    git(&e.src, &["add", "c.txt"]);
    git(&e.src, &["commit", "-qm", "Bug 22 - add c"]);
    std::fs::write(e.src.join("t.txt"), filler(two_copies(Some(new_in)))).unwrap();
    git(&e.src, &["commit", "-qam", "Bug 23 - new"]);
    e.ok(&["import", "s", "tbase"]);
    git(&e.src, &["checkout", "-q", "main"]);
}

#[test]
fn a_kept_diff_that_git_would_apply_at_another_copy_of_its_lines_is_a_new_version() {
    let e = env();
    two_copies_stack(&e, 2);
    let before = e.column(2);
    // Patch 1 made again with thirty lines more between the two copies,
    // the patches above rebased on it: git takes patch 3's line into the
    // second copy, now thirty lines lower. Patch 3's diff is of the same
    // effect, but as stored it says the second copy's old place, nearer
    // the first copy now, where git would apply it.
    let more: String = (1..=30).map(|n| format!("    more {n}\n")).collect();
    let longer = |t: String| t.replace("    filler 2\n", "    FILLER 2\n").replace("    filler 4\n", &format!("    filler 4\n{more}"));
    let one = e.root.join("one.txt");
    std::fs::write(&one, longer(two_copies(None))).unwrap();
    let claude = fake_claude(
        &e,
        "claude-longer.sh",
        &format!(
            r#"set -e
todo=$(printf '%s\n' "$2" | sed -n 's/^The rebase todo file: //p')
a=$(git rev-parse HEAD~2) b=$(git rev-parse HEAD~1) c=$(git rev-parse HEAD)
printf 'edit %s\npick %s\npick %s\n' $a $b $c > "$todo"
git -c user.name=C -c user.email=c@x rebase -q -i HEAD~3 >&2
cp {} t.txt
git -c user.name=C -c user.email=c@x commit -q --amend -a --no-edit
git -c user.name=C -c user.email=c@x rebase --continue >&2"#,
            one.display()
        ),
        r#"{"replies": {"c1": "Patch 1 is longer."}}"#,
    );
    let id = comment(&e, &["Make", "patch", "1", "longer."]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    // Patch 1 and patch 3 are new versions, patch 2 is as it was: the stack
    // leaves t.txt as the agent left it, the line in the second copy.
    let after = e.column(2);
    assert_eq!([after[0] == before[0], after[1] == before[1], after[2] == before[2]], [false, true, false], "{before:?} {after:?}");
    assert_eq!(e.ok(&["versions", "s", "3"]).lines().count(), 2);
    assert_eq!(e.ok(&["show", "s", "3", "--path", "t.txt"]), longer(two_copies(Some(2))));
}

#[test]
fn a_hunk_the_agent_moved_to_lines_like_its_own_is_a_new_version() {
    let e = env();
    two_copies_stack(&e, 1);
    let before = e.column(2);
    // Patch 3's line moved from the first copy to the second: the same
    // lines around it, kept and added, in another place.
    let moved = e.root.join("moved.txt");
    std::fs::write(&moved, two_copies(Some(2)).replace("    filler 2\n", "    FILLER 2\n")).unwrap();
    let claude = fake_claude(
        &e,
        "claude-move.sh",
        &format!("cp {} t.txt && git -c user.name=C -c user.email=c@x commit -q --amend -a --no-edit", moved.display()),
        r#"{"replies": {"c1": "Moved to the second copy."}}"#,
    );
    let id = comment(&e, &["--patch", "3", "In", "the", "second", "copy."]);
    let (fine, out, err) = send(&e, &claude, &[&id], &[]);
    assert!(fine, "{out}{err}");
    assert!(e.ok(&["runs"]).contains("reviewed: its change accepted; 1 reply accepted"));
    let after = e.column(2);
    assert_eq!(after[..2], before[..2]);
    assert_ne!(after[2], before[2]);
    assert_eq!(e.ok(&["show", "s", "3", "--path", "t.txt"]), std::fs::read_to_string(&moved).unwrap());
}

#[test]
fn unfinished_work_is_kept_and_a_stack_that_does_not_apply_takes_no_ask() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let before = e.column(2);
    let messy = fake_claude(&e, "claude-messy.sh", "echo half > new.txt", r#"{"replies": {"c1": "done, I think"}}"#);
    let id = comment(&e, &["Add", "new.txt."]);
    let (fine, out, _) = send(&e, &messy, &[&id], &[]);
    assert!(!fine && out.contains("ask failed") && out.contains("left work uncommitted"), "{out}");
    let runs = e.ok(&["runs"]);
    let kept = runs.split("it is left in ").nth(1).unwrap().trim().to_string();
    assert!(Path::new(&kept).join("new.txt").exists(), "{kept}");
    assert_eq!(e.column(2), before, "nothing changed");
    git(&e.src, &["worktree", "remove", "--force", &kept]);

    // A stack from before the check, whose patches do not apply to its
    // base, is sent nothing, and nothing is left behind.
    let other = git(&e.src, &["rev-parse", "HEAD"]).trim().to_string();
    let db = Db::load(&e.db).unwrap();
    let p = db.patch(&db.stack("s").unwrap().patches[0]).clone();
    drop(db);
    old_stack(&e, &[(p.title(), &p.diff)], Some(&other), Some(e.src.to_str().unwrap()));
    let id = comment(&e, &["Anything."]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id], "", &[("STACKED_CLAUDE", &messy)]);
    assert!(!fine && err.contains("cannot send: patch 1") && err.contains("does not apply to its base"), "{err}");
    assert_eq!(git(&e.src, &["worktree", "list"]).lines().count(), 1);
}

#[test]
fn what_the_agent_makes_is_checked_too() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let before = e.column(2);
    let patches = || {
        let db = Db::load(&e.db).unwrap();
        db.records().iter().filter(|r| matches!(r, Record::Patch(_))).count()
    };
    let stored = patches();
    // An empty commit is a patch with no diff: it does not apply.
    let empty = fake_claude(
        &e,
        "claude-empty.sh",
        r#"git -c user.name=C -c user.email=c@x commit -q --allow-empty -m "Bug 9 - nothing""#,
        r#"{"replies": {"c1": "added it"}}"#,
    );
    let id = comment(&e, &["Add", "an", "empty", "patch."]);
    let (fine, out, _) = send(&e, &empty, &[&id], &[]);
    assert!(!fine && out.contains("has no diff"), "{out}");
    let runs = e.ok(&["runs"]);
    let kept = runs.split("its work is left in ").nth(1).unwrap().trim().to_string();
    assert!(Path::new(&kept).is_dir(), "{kept}");
    assert_eq!(e.column(2), before, "the stack is as it was");
    assert_eq!(patches(), stored, "no patch of it is stored");
    git(&e.src, &["worktree", "remove", "--force", &kept]);
}

#[cfg(feature = "moz-phab")]
#[test]
fn reviews_from_phabricator_wait_for_review_then_are_threads() {
    use stacked::phab::{Pulled, Remote, RevisionComments};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let db = Db::load(&e.db).unwrap();
    let p = db.patch(&db.stack("s").unwrap().patches[1]).clone();
    let remote = |phid: &str, path: Option<&str>, line: usize, text: &str, reply: Option<&str>| Remote {
        phid: phid.into(),
        author: "reviewer".into(),
        text: text.into(),
        created: 0,
        path: path.map(String::from),
        line,
        line_text: path.map(|_| "new".into()),
        reply_to: reply.map(String::from),
        earlier: vec![],
    };
    let pulled = |comments: Vec<Remote>| -> Result<Pulled, String> {
        Ok(Pulled {
            revisions: vec![RevisionComments {
                revision: "D1".into(),
                source: p.change.clone(),
                comments,
            }],
        })
    };
    let first = vec![
        remote("c1", Some("b.txt"), 1, "What is b?", None),
        remote("c2", None, 0, "Looks fine overall.", None),
        remote("c3", Some("b.txt"), 1, "And why now?", Some("c1")),
    ];
    // Pulled, the reviews wait for review, as the agent's work does: nothing
    // is filed yet. A reply to a thread new with it is a turn of it.
    let run = Db::edit(&e.db, |db| stacked::review::file(db, "s", "pull-1", 1, &pulled(first.clone()))).unwrap();
    assert_eq!(run.summary, "3 new comments from 1 revision: they wait for your review");
    assert!(Db::load(&e.db).unwrap().notes(&p.change).is_empty(), "nothing filed before it is reviewed");
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.starts_with("pull-1\t") && landed.contains("\t2 opened comments\n"), "{landed}");
    // Pulled again meanwhile: nothing twice.
    let again = Db::edit(&e.db, |db| stacked::review::file(db, "s", "pull-2", 1, &pulled(first.clone()))).unwrap();
    assert_eq!((again.summary.as_str(), again.held.is_none()), ("nothing new from 1 revision", true));
    // Accepted, they are threads.
    let out = e.ok(&["review", "s", "pull-1", "--accept-all"]);
    assert!(out.contains("pull-1\treviewed: 2 opened comments accepted"), "{out}");
    let db = Db::load(&e.db).unwrap();
    let notes = db.notes(&p.change);
    assert_eq!(notes.len(), 2, "a reply is a turn of its thread");
    let inline = notes.iter().find(|n| n.id == "phab:c1").unwrap();
    assert_eq!(inline.text, "```reviewer reviewer\nWhat is b?\n```\n```reviewer reviewer\nAnd why now?\n```");
    let rows = stacked::rows::patch_rows(&p, &notes, 80, &Default::default());
    let at = rows.iter().position(|r| r.text == "+new").unwrap();
    assert!(rows[at + 1].text.contains("reviewer's comment"), "{:#?}", &rows[at..]);
    let general = notes.iter().find(|n| n.id == "phab:c2").unwrap();
    assert_eq!(general.line, stacked::view::title_key(&p), "a general comment is on the title");
    // Later, a reply to a thread there is a reply held: accepted, the
    // reviewer's turn of it. None is rejected: a pull's review files them.
    let mut later = first.clone();
    later.push(remote("c4", Some("b.txt"), 1, "Ping?", Some("c3")));
    later.push(remote("c5", None, 0, "Also, the title.", None));
    let run = Db::edit(&e.db, |db| stacked::review::file(db, "s", "pull-3", 1, &pulled(later.clone()))).unwrap();
    assert_eq!(run.summary, "2 new comments from 1 revision: they wait for your review");
    let h = run.held.unwrap();
    assert_eq!((h.replies.len(), h.opened.len(), h.replies[0].who()), (1, 1, "reviewer"));
    let refused = "a comment pulled from Phabricator cannot be rejected: close the review to file it";
    let (fine, _, err) = e.run(&["review", "s", "pull-3", "--reject-all"]);
    assert!(!fine && err.contains(refused), "{err}");
    let marks = [("reply 0".to_string(), true), ("opened 0".to_string(), false)].into();
    assert_eq!(Db::edit(&e.db, |db| stacked::held::close(db, "pull-3", &marks, None)).unwrap_err().0, refused);
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.contains("\n  reply\t") && landed.contains("\n  opened\t") && !landed.contains("accepted") && !landed.contains("rejected"), "{landed}");
    let out = e.ok(&["review", "s", "pull-3", "--accept-all"]);
    assert!(out.contains("pull-3\treviewed: 1 opened comment accepted; 1 reply accepted"), "{out}");
    let db = Db::load(&e.db).unwrap();
    let inline = db.note(&p.change, "phab:c1").unwrap();
    assert!(inline.text.ends_with("```reviewer reviewer\nPing?\n```"), "{}", inline.text);
    assert!(inline.remote.contains(&"c4".to_string()), "{:?}", inline.remote);
    assert!(db.note(&p.change, "phab:c5").is_some(), "filed");
    let again = Db::edit(&e.db, |db| stacked::review::file(db, "s", "pull-4", 1, &pulled(later))).unwrap();
    assert_eq!(again.summary, "nothing new from 1 revision", "nor pulled again");
}

/// Pull `run` filed on stack `s` of `e`, revision D1 of patch 2: the
/// comments `comments`, as Phabricator has them.
#[cfg(feature = "moz-phab")]
fn pull_into(e: &Env, run: &str, comments: &[stacked::phab::Remote]) -> stacked::db::Run {
    use stacked::phab::{Pulled, RevisionComments};
    let db = Db::load(&e.db).unwrap();
    let change = db.patch(&db.stack("s").unwrap().patches[1]).change.clone();
    let pulled = Ok(Pulled {
        revisions: vec![RevisionComments {
            revision: "D1".into(),
            source: change,
            comments: comments.to_vec(),
        }],
    });
    drop(db);
    Db::edit(&e.db, |db| stacked::review::file(db, "s", run, 1, &pulled)).unwrap()
}

/// ann's comment `phid` as Phabricator has it, on `b.txt`'s line 1: `text`,
/// a reply to `reply` if any, its versions before `earlier`.
#[cfg(feature = "moz-phab")]
fn by_ann(phid: &str, text: &str, reply: Option<&str>, earlier: &[(&str, &str)]) -> stacked::phab::Remote {
    stacked::phab::Remote {
        phid: phid.into(),
        author: "ann".into(),
        text: text.into(),
        path: Some("b.txt".into()),
        line: 1,
        line_text: Some("new".into()),
        reply_to: reply.map(String::from),
        earlier: earlier.iter().map(|(phid, text)| stacked::phab::Version { phid: phid.to_string(), text: text.to_string() }).collect(),
        ..Default::default()
    }
}

#[cfg(feature = "moz-phab")]
#[test]
fn a_reviewer_s_reply_to_a_comment_you_closed_reopens_it_once_accepted() {
    use stacked::db::Note;
    use stacked::thread::{Role, append_turn, fenced};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let db = Db::load(&e.db).unwrap();
    let p = db.patch(&db.stack("s").unwrap().patches[1]).clone();
    drop(db);
    let turn = |text: &str| fenced("reviewer ann", text);
    let c1 = by_ann("c1", "What is b?", None, &[]);
    let c2 = by_ann("c2", "Ping?", Some("c1"), &[]);
    // Filed, answered, then closed.
    pull_into(&e, "pull-1", std::slice::from_ref(&c1));
    e.ok(&["review", "s", "pull-1", "--accept-all"]);
    let filed = Db::edit(&e.db, |db| {
        let n = db.note(&p.change, "phab:c1").unwrap().clone();
        let n = Note { text: append_turn(&n.text, Role::You, "Because.", None), ..n };
        db.set_note(n.clone())?;
        db.set_note(Note { text: String::new(), ..n.clone() })?;
        Ok(n)
    })
    .unwrap();
    // The Phabricator line counts no comment of a thread closed.
    let counted = |comments: &[stacked::phab::Remote]| stacked::review::not_pulled(&Db::load(&e.db).unwrap(), "s", &p.change, comments);
    assert_eq!(counted(&[c1.clone(), c2.clone()]), (0, 0));
    // ann's reply to it since: pulled, held with the thread as it was when
    // closed, for its review to show it.
    let run = pull_into(&e, "pull-2", &[c1.clone(), c2.clone()]);
    assert_eq!(run.summary, "1 new comment from 1 revision: it waits for your review");
    let rp = run.held.unwrap().replies.remove(0);
    assert_eq!((rp.id.as_str(), rp.closed.map(|c| c.note.text)), ("phab:c1", Some(filed.text.clone())));
    assert_eq!(counted(&[c1.clone(), c2.clone()]), (0, 0), "nor while it waits");
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.contains("\tPing?\tto a comment you closed: accepting it reopens the comment\n"), "{landed}");
    // Not rejected: accepted, it reopens the thread as it was, her reply its
    // last turn, its comment's id kept.
    let (fine, _, err) = e.run(&["review", "s", "pull-2", "--reject-all"]);
    assert!(!fine && err.contains("cannot be rejected"), "{err}");
    let out = e.ok(&["review", "s", "pull-2", "--accept-all"]);
    assert!(out.contains("pull-2\treviewed: 1 reply accepted; 1 comment reopened"), "{out}");
    let db = Db::load(&e.db).unwrap();
    let n = db.note(&p.change, "phab:c1").unwrap();
    assert_eq!(n.text, format!("{}\n{}", filed.text, turn("Ping?")));
    assert_eq!(n.remote, ["c1", "c2"]);
    drop(db);
    assert_eq!(pull_into(&e, "pull-3", &[c1.clone(), c2.clone()]).summary, "nothing new from 1 revision");
    // Moved to the stack's comments, and closed there: a reply reopens it
    // there, where it was last, the patch's own left closed.
    Db::edit(&e.db, |db| {
        let n = db.note(&p.change, "phab:c1").unwrap().clone();
        stacked::ask::promote(db, "s", &n, "it is the whole stack's", None)?;
        let moved = db.notes("stack:s").into_iter().find(|n| n.id.starts_with("moved:")).unwrap().clone();
        db.set_note(Note { text: String::new(), ..moved })
    })
    .unwrap();
    let c3 = by_ann("c3", "Still?", Some("c2"), &[]);
    let run = pull_into(&e, "pull-4", &[c1.clone(), c2.clone(), c3.clone()]);
    let rp = run.held.unwrap().replies.remove(0);
    assert!(rp.change == "stack:s" && rp.id.starts_with("moved:") && rp.closed.is_some(), "{rp:?}");
    let out = e.ok(&["review", "s", "pull-4", "--accept-all"]);
    assert!(out.contains("reviewed: 1 reply accepted; 1 comment reopened"), "{out}");
    let db = Db::load(&e.db).unwrap();
    let moved = db.note("stack:s", &rp.id).unwrap();
    assert!(moved.text.ends_with(&turn("Still?")) && moved.remote.contains(&"c3".to_string()), "{}", moved.text);
    assert!(db.note(&p.change, "phab:c1").is_none(), "its patch's own, still closed");
    drop(db);
    // Written since the pull, and closed again: as the history has it last.
    Db::edit(&e.db, |db| {
        let n = db.note("stack:s", &rp.id).unwrap().clone();
        db.set_note(Note { text: String::new(), ..n })
    })
    .unwrap();
    let c4 = by_ann("c4", "Hello?", Some("c3"), &[]);
    pull_into(&e, "pull-5", &[c1, c2, c3, c4]);
    Db::edit(&e.db, |db| {
        let was = db.deleted_notes("s", "stack:s").into_iter().map(|(_, n)| n).find(|n| n.id == rp.id).unwrap();
        let more = Note { text: append_turn(&was.text, Role::You, "Gone, sorry.", None), ..was };
        db.set_note(more.clone())?;
        db.set_note(Note { text: String::new(), ..more })
    })
    .unwrap();
    let out = e.ok(&["review", "s", "pull-5", "--accept-all"]);
    assert!(out.contains("reviewed: 1 reply accepted; 1 comment reopened"), "{out}");
    let moved = Db::load(&e.db).unwrap().note("stack:s", &rp.id).unwrap().clone();
    assert!(moved.text.ends_with(&format!("Gone, sorry.\n{}", turn("Hello?"))), "{}", moved.text);
}

#[cfg(feature = "moz-phab")]
#[test]
fn comments_a_review_of_before_rejected_come_back_once_and_are_filed() {
    use stacked::db::{Note, Run, RunState};
    use stacked::held::{Held, Reply, Review};
    use stacked::thread::fenced;
    let e = env();
    e.ok(&["import", "s", "base"]);
    let db = Db::load(&e.db).unwrap();
    let p = db.patch(&db.stack("s").unwrap().patches[1]).clone();
    drop(db);
    // Filed: ann's thread, a1.
    let a1 = by_ann("a1", "What is b?", None, &[]);
    pull_into(&e, "pull-1", std::slice::from_ref(&a1));
    e.ok(&["review", "s", "pull-1", "--accept-all"]);
    // A pull reviewed by a stacked of before, which rejected all it held: a
    // new thread, n1; a reply to a1, a2; an edit of a1, a1v2.
    let n1 = by_ann("n1", "Also, the title.", None, &[]);
    let a2 = by_ann("a2", "Ping?", Some("a1"), &[]);
    let a1v2 = by_ann("a1v2", "What is b for?", None, &[("a1", "What is b?")]);
    Db::edit(&e.db, |db| {
        let thread = db.note(&p.change, "phab:a1").unwrap().clone();
        let reply = |text: &str, remote: &str, was: Option<&str>| Reply {
            change: p.change.clone(),
            id: "phab:a1".into(),
            text: text.into(),
            reviewer: Some("ann".into()),
            remote: Some(remote.into()),
            was: was.map(String::from),
            ..Default::default()
        };
        let opened = Note {
            change: p.change.clone(),
            id: "phab:n1".into(),
            line: thread.line.clone(),
            text: fenced("reviewer ann", "Also, the title."),
            by: Some("ann".into()),
            remote: vec!["n1".into()],
            ..Default::default()
        };
        let run = Run {
            id: "pull-0".into(),
            what: "pull".into(),
            stack: "s".into(),
            state: RunState::Done,
            held: Some(Held {
                at: stacked::db::now(),
                opened: vec![opened],
                replies: vec![reply("Ping?", "a2", None), reply("What is b for?", "a1v2", Some("What is b?"))],
                ..Default::default()
            }),
            ..Default::default()
        };
        db.add_run(run.clone())?;
        let marks = [("opened 0", false), ("reply 0", false), ("reply 1", false)].into_iter().map(|(k, m)| (k.to_string(), m)).collect();
        db.set_review("pull-0", Review { marks, closed: true, ..Default::default() })?;
        db.add_run(Run { summary: "reviewed: 1 opened comment rejected; 1 reply rejected; 1 edited comment rejected".into(), ..run })
            .map(|_| ())
    })
    .unwrap();
    // Nothing remembers that: each comes back once, then is filed.
    let comments = [a1v2.clone(), a2.clone(), n1.clone()];
    let run = pull_into(&e, "pull-2", &comments);
    assert_eq!(run.summary, "2 new comments and 1 edited comment from 1 revision: they wait for your review");
    let out = e.ok(&["review", "s", "pull-2", "--accept-all"]);
    assert!(out.contains("reviewed: 1 opened comment accepted; 1 reply accepted; 1 edited comment accepted"), "{out}");
    let db = Db::load(&e.db).unwrap();
    assert!(db.note(&p.change, "phab:n1").is_some());
    let n = db.note(&p.change, "phab:a1").unwrap();
    let said: Vec<String> = stacked::thread::all_turns(&n.text).iter().enumerate().map(|(i, t)| format!("{}: {}", t.name(i), t.body)).collect();
    assert_eq!(said, ["ann's comment (redacted): What is b?", "ann's edit of the comment that said \"What is b?\": What is b for?", "ann's reply: Ping?"]);
    drop(db);
    assert_eq!(pull_into(&e, "pull-3", &comments).summary, "nothing new from 1 revision");
}

#[cfg(feature = "moz-phab")]
#[test]
fn a_comment_edited_on_phabricator_is_held_as_an_edit_of_its_turn() {
    use stacked::db::Note;
    use stacked::phab::{Pulled, Remote, RevisionComments, Version};
    use stacked::thread::{Role, append_turn};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let db = Db::load(&e.db).unwrap();
    let p = db.patch(&db.stack("s").unwrap().patches[1]).clone();
    drop(db);
    // ann's comment as Phabricator has it, its id this version's; edited,
    // what it was, the version before first.
    let ann = |id: &str, text: &str, reply: Option<&str>, earlier: &[(&str, &str)]| Remote {
        phid: id.into(),
        author: "ann".into(),
        text: text.into(),
        path: Some("b.txt".into()),
        line: 1,
        line_text: Some("new".into()),
        reply_to: reply.map(String::from),
        earlier: earlier.iter().map(|(phid, text)| Version { phid: phid.to_string(), text: text.to_string() }).collect(),
        ..Default::default()
    };
    let pull = |run: &str, comments: &[Remote]| {
        let pulled = Ok(Pulled {
            revisions: vec![RevisionComments {
                revision: "D1".into(),
                source: p.change.clone(),
                comments: comments.to_vec(),
            }],
        });
        Db::edit(&e.db, |db| stacked::review::file(db, "s", run, 1, &pulled)).unwrap()
    };
    let close = |run: &str, accept: bool| Db::edit(&e.db, |db| stacked::held::close(db, run, &Default::default(), Some(accept))).unwrap();
    let thread = |id: &str| Db::load(&e.db).unwrap().note(&p.change, id).map(|n| n.text.clone()).unwrap_or_default();
    let turn = |text: &str| format!("```reviewer ann\n{text}\n```");

    // ann's comment and her reply to it, filed; you answer.
    let a1 = ann("a1", "What is b?", None, &[]);
    let a2 = ann("a2", "And why now?", Some("a1"), &[]);
    pull("pull-1", &[a1, a2.clone()]);
    close("pull-1", true);
    Db::edit(&e.db, |db| {
        let n = db.note(&p.change, "phab:a1").unwrap().clone();
        db.set_note(Note {
            text: append_turn(&n.text, Role::You, "Because.", None),
            ..n
        })
    })
    .unwrap();
    // She edits her comment: the edit waits for your review, nothing filed.
    let redacted = |text: &str| format!("```reviewer ann redacted\n{text}\n```");
    let edit = |was: &str, text: &str| format!("```reviewer ann edit {was}\n{text}\n```");
    let edit_redacted = |was: &str, text: &str| format!("```reviewer ann redacted edit {was}\n{text}\n```");
    let v2 = ann("a1v2", "What is b for?", None, &[("a1", "What is b?")]);
    let run = pull("pull-2", &[v2, a2.clone()]);
    assert_eq!(run.summary, "1 edited comment from 1 revision: it waits for your review");
    let h = run.held.unwrap();
    assert_eq!((h.replies.len(), h.opened.len()), (1, 0));
    assert_eq!((h.replies[0].id.as_str(), h.replies[0].was.as_deref(), h.replies[0].said()), ("phab:a1", Some("What is b?"), "ann's edit".to_string()));
    assert_eq!(thread("phab:a1"), format!("{}\n{}\nBecause.", turn("What is b?"), turn("And why now?")));
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.contains("\t1 edited comment\n") && landed.contains("\n  edited\t") && landed.ends_with("\tWhat is b for?\n"), "{landed}");
    // Edited again while that waits: pulled once its review closes.
    let v3 = ann("a1v3", "What is b for, really?", None, &[("a1v2", "What is b for?"), ("a1", "What is b?")]);
    assert_eq!(pull("pull-3", &[v3.clone(), a2.clone()]).summary, "nothing new from 1 revision");
    // Accepted, it is her latest turn, quoting what it edits; her turn as
    // it was stays, redacted, your answer after it.
    assert_eq!(close("pull-2", true), "1 edited comment accepted");
    let after_v2 = format!("{}\n{}\nBecause.\n{}", redacted("What is b?"), turn("And why now?"), edit("What is b?", "What is b for?"));
    assert_eq!(thread("phab:a1"), after_v2);
    // Its edit again: held, and not rejected; accepted, the latest version
    // redacted in its turn.
    let run = pull("pull-4", &[v3.clone(), a2.clone()]);
    assert_eq!(run.held.unwrap().replies[0].was.as_deref(), Some("What is b for?"));
    assert_eq!(
        Db::edit(&e.db, |db| stacked::held::close(db, "pull-4", &Default::default(), Some(false))).unwrap_err().0,
        "a comment pulled from Phabricator cannot be rejected: close the review to file it"
    );
    assert_eq!(close("pull-4", true), "1 edited comment accepted");
    assert_eq!(pull("pull-5", &[v3, a2]).summary, "nothing new from 1 revision");
    let after_v3 = format!(
        "{}\n{}\nBecause.\n{}\n{}",
        redacted("What is b?"),
        turn("And why now?"),
        edit_redacted("What is b?", "What is b for?"),
        edit("What is b for?", "What is b for, really?")
    );
    assert_eq!(thread("phab:a1"), after_v3);
    // Edited once more, and her reply too: each an edit of the turn of its
    // latest version.
    let v4 = ann("a1v4", "Is b needed?", None, &[("a1v3", "What is b for, really?"), ("a1v2", "What is b for?"), ("a1", "What is b?")]);
    let r2 = ann("a2v2", "And why now, in this patch?", Some("a1"), &[("a2", "And why now?")]);
    let run = pull("pull-6", &[v4.clone(), r2.clone()]);
    assert_eq!(run.summary, "2 edited comments from 1 revision: they wait for your review");
    close("pull-6", true);
    let after_v4 = format!(
        "{}\n{}\nBecause.\n{}\n{}\n{}\n{}",
        redacted("What is b?"),
        redacted("And why now?"),
        edit_redacted("What is b?", "What is b for?"),
        edit_redacted("What is b for?", "What is b for, really?"),
        edit("What is b for, really?", "Is b needed?"),
        edit("And why now?", "And why now, in this patch?")
    );
    assert_eq!(thread("phab:a1"), after_v4);
    let n = Db::load(&e.db).unwrap().note(&p.change, "phab:a1").unwrap().clone();
    assert_eq!(n.remote, ["a1", "a2", "a1v2", "a1v3", "a1v4", "a2v2"]);
    // A comment edited before it was first pulled is new, as it is now; a
    // reply to its version before is in its thread.
    let b1 = ann("b1v2", "Typo here.", None, &[("b1", "Tpyo here.")]);
    let b2 = ann("b2", "Still.", Some("b1"), &[]);
    let run = pull("pull-7", &[v4.clone(), r2.clone(), b1, b2]);
    assert_eq!(run.summary, "2 new comments from 1 revision: they wait for your review");
    let h = run.held.unwrap();
    assert_eq!(h.opened.len(), 1, "{h:?}");
    assert_eq!(h.opened[0].text, format!("{}\n{}", turn("Typo here."), turn("Still.")));
    close("pull-7", true);
    // Its thread moved to the stack's comments, an edit and a reply go to it
    // there.
    Db::edit(&e.db, |db| {
        let n = db.note(&p.change, "phab:b1v2").unwrap().clone();
        stacked::ask::promote(db, "s", &n, "it is the whole stack's", None)
    })
    .unwrap();
    let b1 = ann("b1v3", "Typo there.", None, &[("b1v2", "Typo here."), ("b1", "Tpyo here.")]);
    let b3 = ann("b3", "Ping?", Some("b1v2"), &[]);
    let run = pull("pull-8", &[v4.clone(), r2.clone(), b1, ann("b2", "Still.", Some("b1"), &[]), b3]);
    assert_eq!(run.summary, "1 new comment and 1 edited comment from 1 revision: they wait for your review");
    let h = run.held.unwrap();
    assert!(h.replies.iter().all(|rp| rp.change == "stack:s" && rp.id.starts_with("moved:")), "{h:?}");
    close("pull-8", true);
    let db = Db::load(&e.db).unwrap();
    let moved = db.notes("stack:s").into_iter().find(|n| n.id.starts_with("moved:")).unwrap().clone();
    assert!(moved.text.starts_with(&format!("{}\n{}\n", redacted("Typo here."), turn("Still."))), "{}", moved.text);
    assert!(moved.text.ends_with(&format!("{}\n{}", edit("Typo here.", "Typo there."), turn("Ping?"))), "{}", moved.text);
    drop(db);
    // Its thread closed, an edit of it is held all the same, with the
    // thread as it was when closed; accepted, it reopens it.
    Db::edit(&e.db, |db| {
        let n = db.note(&p.change, "phab:a1").unwrap().clone();
        db.set_note(Note { text: String::new(), ..n })
    })
    .unwrap();
    let v5 = ann("a1v5", "Never mind.", None, &[("a1v4", "Is b needed?"), ("a1v3", ""), ("a1v2", ""), ("a1", "")]);
    let run = pull("pull-9", &[v5.clone(), r2.clone()]);
    assert_eq!(run.summary, "1 edited comment from 1 revision: it waits for your review");
    assert_eq!(run.held.unwrap().replies[0].closed.as_ref().map(|c| c.note.text.clone()), Some(after_v4.clone()));
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.contains("\tNever mind.\tto a comment you closed: accepting it reopens the comment\n"), "{landed}");
    assert_eq!(close("pull-9", true), "1 edited comment accepted; 1 comment reopened");
    let reopened = after_v4.replace(&edit("What is b for, really?", "Is b needed?"), &edit_redacted("What is b for, really?", "Is b needed?"));
    assert_eq!(thread("phab:a1"), format!("{reopened}\n{}", edit("Is b needed?", "Never mind.")));
    // Its history says so: closed, reopened, then her edit.
    let d = Db::load(&e.db).unwrap();
    let mut said = Vec::new();
    let mut before: Option<&Note> = None;
    for (_, x) in d.note_history(&p.change, "phab:a1") {
        said.push(stacked::history::note_change(before, x));
        before = Some(x);
    }
    assert_eq!(said[said.len() - 3..], ["closed", "reopened", "a reviewer edited a comment on Phabricator"]);
    drop(d);
    assert_eq!(pull("pull-10", &[v5, r2]).summary, "nothing new from 1 revision");
}

#[test]
fn the_agents_answer_is_asked_as_structured_output_and_read_from_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let args = e.root.join("args");
    // A claude that keeps what it is run with, then ends as `result` says,
    // with `code`.
    let keeps = format!("for a in \"$@\"; do printf '%s\\0' \"$a\"; done > {}", args.display());
    let ends = |name: &str, result: serde_json::Value, code: u8| {
        e.script(
            name,
            &format!("{keeps}\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\ncat <<'EOF'\n{result}\nEOF\nexit {code}\n"),
        )
    };
    let run_with = || -> Vec<String> { std::fs::read_to_string(&args).unwrap().split('\0').map(String::from).collect() };
    let after = |all: &[String], flag: &str| all.iter().position(|a| a == flag).map(|k| all[k + 1].clone());
    // Asked as structured output, of what the ask asks for; answered so,
    // the text of its result empty.
    let given = ends(
        "claude-structured.sh",
        serde_json::json!({"type": "result", "subtype": "success", "result": "", "structured_output": {"replies": {"c1": "As given."}}}),
        0,
    );
    let id = comment(&e, &["Why?"]);
    let (fine, out, err) = send(&e, &given, &[&id, "--answer-only"], &[]);
    assert!(fine, "{out}{err}");
    assert!(e.ok(&["comments", "s"]).contains("As given."), "{}", e.ok(&["comments", "s"]));
    let all = run_with();
    let schema: serde_json::Value = serde_json::from_str(&after(&all, "--json-schema").expect("asked as structured output")).unwrap();
    assert!(schema["properties"]["replies"]["properties"]["c1"].is_object(), "{schema}");
    assert!(schema["properties"]["comments"]["items"].is_object(), "an answer's comments: {schema}");
    assert!(after(&all, "--allowedTools").unwrap().split(',').any(|t| t == "StructuredOutput"), "{all:?}");
    assert!(after(&all, "-p").unwrap().contains("Give your answer as the structured output asked of you"));
    // Off in the settings: none of it, the answer read from what the agent
    // writes, the prose around it left out.
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, format!("{HOME_SETTINGS}schema = false\n")).unwrap();
    let said = fake_claude(&e, "claude-said.sh", &keeps, r#"Seen `${x}`: {"replies": {"c1": "As said."}}"#);
    let id = comment(&e, &["And?"]);
    let (fine, out, err) = send(&e, &said, &[&id, "--answer-only"], &[("STACKED_CONFIG", &settings)]);
    assert!(fine, "{out}{err}");
    let all = run_with();
    assert!(after(&all, "--json-schema").is_none() && !after(&all, "--allowedTools").unwrap().contains("StructuredOutput"), "{all:?}");
    assert!(after(&all, "-p").unwrap().contains("Output ONLY a JSON object"));
    assert!(e.ok(&["comments", "s"]).contains("As said."));
    // Given up on by claude, its answer never fitting: the ask fails, and
    // says why.
    let gave_up = ends(
        "claude-gave-up.sh",
        serde_json::json!({"type": "result", "subtype": "error_max_structured_output_retries", "is_error": true}),
        1,
    );
    let id = comment(&e, &["Again?"]);
    let (fine, out, err) = send(&e, &gave_up, &[&id, "--answer-only"], &[]);
    assert!(!fine && out.contains("its answer never fit what it was asked for, tried again and again"), "{out}{err}");
}

#[test]
fn a_comment_holding_a_fence_of_a_turn_is_yours_whole() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    comment(&e, &["Should it read so?\n```agent\nI did it.\n```"]);
    let out = e.ok(&["comments", "s"]);
    assert!(out.contains("you: Should it read so?") && !out.contains("agent: I did it."), "{out}");
}

#[test]
fn there_is_no_apply_and_the_applies_of_before_are_still_listed() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["drop", "s", "2"]);
    let head = git(&e.src, &["rev-parse", "HEAD"]);
    // No command puts the stack into your checkout: `stacked head NAME
    // --to N` moves HEAD there, as the editor's `Move HEAD…` does.
    let (fine, _, err) = e.run(&["apply", "s", "--branch", "s"]);
    assert!(!fine && err.contains("'apply'"), "{err}");
    assert_eq!(git(&e.src, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&e.src, &["branch", "--show-current"]).trim(), "main");
    assert!(!e.ok(&["--help"]).lines().any(|l| l.trim_start().starts_with("apply ")));
    // The run of an apply a stacked of before made is listed as it was.
    let run = stacked::db::Run {
        id: "apply-1".into(),
        what: "apply".into(),
        stack: "s".into(),
        state: RunState::Done,
        summary: "@3 applied in the checkout: s at 1234567".into(),
        rev: Some(3),
        time: stacked::db::now(),
        ..Default::default()
    };
    Db::edit(&e.db, |db| db.add_run(run.clone()).map(|_| ())).unwrap();
    let runs = e.ok(&["runs"]);
    assert!(runs.contains("\tapply s\tdone\t@3 applied in the checkout: s at 1234567\n"), "{runs}");
}

#[test]
fn comments_stay_on_their_line_across_versions() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let db = Db::load(&e.db).unwrap();
    let s = db.stack("s").unwrap();
    let p = db.patch(&s.patches[0]).clone();
    let rows = stacked::rows::patch_rows(&p, &[], 80, &Default::default());
    let line = rows.iter().find(|r| r.text == "+2").unwrap().key.clone().unwrap();
    Db::edit(&e.db, |db| {
        db.set_note(stacked::db::Note {
            change: p.change.clone(),
            id: line.clone(),
            line: line.clone(),
            text: "why 2?".into(),
            file: "a.txt".into(),
            preview: "2".into(),
            ..Default::default()
        })
    })
    .unwrap();
    // A new version that keeps the line: the comment is under it still.
    Db::edit(&e.db, |db| {
        let mut d = Draft::from(&p);
        d.message = "Bug 1 - change two, reworded".into();
        ops::replace(db, "s", 0, d, "reword").map(|_| ())
    })
    .unwrap();
    let db = Db::load(&e.db).unwrap();
    let s = db.stack("s").unwrap();
    let p2 = db.patch(&s.patches[0]);
    assert_ne!(p2.id, p.id);
    let notes = db.notes(&p2.change);
    let rows = stacked::rows::patch_rows(p2, &notes, 80, &Default::default());
    let at = rows.iter().position(|r| r.text == "+2").unwrap();
    assert!(rows[at + 1].head && rows[at + 3].text.contains("why 2?"), "{:#?}", &rows[at..at + 4]);
    let (lines, _) = stacked::rows::split_lines(&rows);
    assert!(lines.iter().any(|l| l.left.is_some() && l.right.is_some() && l.left != l.right));
}

#[test]
fn a_patch_is_edited_where_the_cursor_is() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // An editor named vim, as stacked knows it: told the line as +LINE.
    let bin = e.root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let args = e.root.join("args");
    let vim = bin.join("vim");
    let script = |body: &str| {
        std::fs::write(&vim, format!("#!/bin/sh\necho \"$1\" >> {}\nfile=\"$2\"\n{body}\n", args.display())).unwrap();
        Command::new("chmod").arg("+x").arg(&vim).status().unwrap();
    };
    let edit = |extra: &[&str]| {
        let mut all = vec!["edit", "s", "--accept"];
        all.extend(extra);
        stacked_in(&e.src, &e.db, &all, "", &[("EDITOR", &vim)])
    };
    let said = || std::fs::read_to_string(&args).unwrap();

    // The patch as mail, at a line.
    script(&sed_i("s/^Subject: Bug 2 - add b$/Subject: Bug 2 - add bee/", "$file"));
    let (fine, _, err) = edit(&["2", "--line", "7"]);
    assert!(fine, "{err}");
    assert_eq!(said().lines().last(), Some("+7"));
    assert_eq!(e.column(3)[1], "Bug 2 - add bee");

    // A file as the patch leaves it, at a line: the patch is made again
    // from it, the rest of it as it was.
    script(&sed_i("s/^2$/deux/", "$file"));
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "2"]);
    assert!(fine && err.contains("edited a.txt in Bug 1 - change two"), "{err}");
    assert_eq!(said().lines().last(), Some("+2"));
    let mail = e.ok(&["show", "s", "1"]);
    assert!(mail.contains("-two\n+deux\n") && mail.contains("Subject: Bug 1 - change two"), "{mail}");
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2);
    assert!(e.ok(&["check"]).contains("s: applies"));

    // One the patch above cannot take is not taken; it is kept, and taken
    // up again from where it was left. It is kept in the temporary
    // directory, which is a folder of the test's here.
    script(&sed_i("s/^eight$/huit/", "$file"));
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "8"]);
    assert!(!fine && err.contains("patch 3 (Bug 3 - café") && err.contains("does not apply"), "{err}");
    let kept = err.split("Your edit is kept in ").nth(1).unwrap().split(':').next().unwrap().to_string();
    assert!(Path::new(&kept).starts_with(e.root.join("tmp")), "{kept}");
    assert!(std::fs::read_to_string(&kept).unwrap().contains("huit"));
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2, "nothing changed");
    script(&sed_i("s/^huit$/eight/", "$file"));
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--from", &kept]);
    assert!(fine && err.contains("saved as it was"), "back as it was: {err}");
    let (fine, _, err) = edit(&["1", "--path", "b.txt"]);
    assert!(!fine && err.contains("does not change b.txt"), "{err}");
}

#[test]
fn each_kind_of_ask_may_use_what_the_settings_add_for_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let settings = e.root.join("settings.toml");
    std::fs::write(
        &settings,
        format!("{HOME_SETTINGS}tools = [\"mcp__bugzilla__*\"]\nanswer_tools = [\"Bash(jq *)\"]\nchange_tools = [\"Bash(make check *)\"]\n"),
    )
    .unwrap();
    // Its arguments a line each: the one after --allowedTools, the tools.
    let args = e.root.join("args");
    let claude = fake_claude(&e, "claude-tools.sh", &format!("printf '%s\\n' \"$@\" > {}", args.display()), r#"{"replies": {"c1": "Read it."}}"#);
    let said = || std::fs::read_to_string(&args).unwrap();
    let tools = || said().lines().skip_while(|l| *l != "--allowedTools").nth(1).unwrap().to_string();
    let id = comment(&e, &["What", "does", "bug", "1", "say?"]);
    let (fine, _, err) = send(&e, &claude, &[&id, "--answer-only"], &[("STACKED_CONFIG", &settings)]);
    assert!(fine, "{err}");
    // An answer's own tools, then its list, then every ask's: no curl, as
    // its list no longer has it.
    let t = tools();
    assert!(t.contains("Read,") && t.ends_with(",Bash(jq *),mcp__bugzilla__*"), "{t}");
    for no in ["Edit", "Write", "curl", "make check"] {
        assert!(!t.contains(no), "{no} in {t}");
    }
    // Told that a tool refused is not allowed, not a service saying no; and,
    // as curl is not let run, not to read the web with it.
    assert!(said().contains("Say so in what you write: that the tool was not allowed, naming it"), "{}", said());
    assert!(said().contains("never that the service behind it refused, failed or would not let you read"), "{}", said());
    assert!(!said().contains("read the web with `curl`") && said().contains("You may use the tools of MCP servers you are let use to answer"), "{}", said());
    // A change: its own list, then every ask's; nothing of the answers'.
    let id = comment(&e, &["Change", "nothing."]);
    let (fine, _, err) = send(&e, &claude, &[&id], &[("STACKED_CONFIG", &settings)]);
    assert!(fine, "{err}");
    let t = tools();
    assert!(t.contains("Bash(git commit *)") && t.ends_with(",Bash(make check *),mcp__bugzilla__*"), "{t}");
    assert!(!t.contains("jq") && !t.contains("curl"), "{t}");
    // What it was not let use is said, in its stream and in its run, with
    // the list of its kind that lets it use more.
    let result = serde_json::json!({"type": "result", "result": "{\"replies\": {\"c1\": \"I could not read it.\"}}",
        "permission_denials": [{"tool_name": "mcp__moz__get_bug", "tool_input": {"id": 1}},
            {"tool_name": "Bash", "tool_input": {"command": "curl -s https://bugzilla.mozilla.org/rest/bug/1 | python3 -m json.tool"}}]});
    let refused = e.script(
        "claude-refused.sh",
        &format!("echo '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\ncat <<'EOF'\n{result}\nEOF\n"),
    );
    let id = comment(&e, &["And", "bug", "2?"]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", &refused)]);
    assert!(fine, "{err}");
    let runs = e.ok(&["runs"]);
    // A pipe's commands each its own: curl, which answers may run, is none.
    let said = "the agent was not allowed to use mcp__moz__get_bug, Bash(python3 -m json.tool *): the review of its work lists each, to allow for this work or from now on; agent.answer_tools lets answer-only asks use more, and `stacked setup agent` sets it; your claude settings can allow more too";
    assert!(runs.contains(said), "{runs}");
    let run = runs.lines().last().unwrap().split('\t').next().unwrap().to_string();
    let transcript = e.ok(&["transcript", &run]);
    assert!(transcript.contains("⛔ not allowed: mcp__moz__get_bug") && transcript.contains("⛔ not allowed: Bash: curl -s"), "{transcript}");
    let id = comment(&e, &["And", "change", "it."]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &[("STACKED_CLAUDE", &refused)]);
    assert!(fine, "{err}");
    let said = "the agent was not allowed to use mcp__moz__get_bug, Bash(curl *), Bash(python3 -m json.tool *): the review of its work lists each, to allow for this work or from now on; agent.change_tools lets asks that change the stack use more, and `stacked setup agent` sets it; your claude settings can allow more too";
    assert!(e.ok(&["runs"]).contains(said), "{}", e.ok(&["runs"]));
}

#[test]
fn an_ask_that_changes_the_stack_may_run_the_checkout_s_linter_and_is_told_so() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // Its arguments a line each: the tools after --allowedTools, and the
    // prompt.
    let args = e.root.join("args");
    let claude = fake_claude(&e, "claude-lint.sh", &format!("printf '%s\\n' \"$@\" > {}", args.display()), r#"{"replies": {"c1": "Seen."}}"#);
    // A comment on the stack, or on patch 1, asked `how`: what the ask
    // may use, and all it was given.
    let ask = |patch: bool, how: &[&str]| {
        let mut words = if patch { vec!["--patch", "1"] } else { vec![] };
        words.extend(["Tidy", "it."]);
        let id = comment(&e, &words);
        let (fine, _, err) = send(&e, &claude, &[&[id.as_str()][..], how].concat(), &[]);
        assert!(fine, "{how:?}: {err}");
        let said = std::fs::read_to_string(&args).unwrap();
        let tools = said.lines().skip_while(|l| *l != "--allowedTools").nth(1).unwrap().to_string();
        (tools, said)
    };
    let told = |command: &str| format!("This checkout's linter is `{command}`: you may run it on the files you change, to check them.");
    let none = "This checkout's linter";
    // No linter: none given, none told.
    let (tools, said) = ask(false, &[]);
    assert!(tools.ends_with(" export *),StructuredOutput"), "{tools}");
    assert!(!said.contains(none), "{said}");
    // ./mach at the top of the checkout, a stand-in the tests never run: a
    // change of the stack or of a patch may run its lint, with or without
    // --fix, and is told so; an answer and a build and test are not given
    // it (a build and test runs any command already).
    std::fs::write(e.src.join("mach"), "#!/bin/sh\necho 'not the real mach: the tests never run it' >&2\nexit 2\n").unwrap();
    for patch in [false, true] {
        let (tools, said) = ask(patch, &[]);
        assert!(tools.ends_with(" export *),Bash(./mach lint *),StructuredOutput"), "{tools}");
        assert!(said.contains(&told("./mach lint --fix")), "{said}");
    }
    for how in ["--answer-only", "--build-and-test"] {
        let (tools, said) = ask(false, &[how]);
        assert!(!tools.contains("Bash(./mach lint *)"), "{how}: {tools}");
        assert!(!said.contains(none), "{how}: {said}");
    }
    // One the checkout sets, over ./mach: as narrow as a refused call of it
    // is offered.
    e.ok(&["setup", "lint", "eslint", "--fix"]);
    let (tools, said) = ask(false, &[]);
    assert!(tools.ends_with(" export *),Bash(eslint *),StructuredOutput") && !tools.contains("mach lint"), "{tools}");
    assert!(said.contains(&told("eslint --fix")), "{said}");
    // Lint turned off, ./mach or not: none.
    e.ok(&["setup", "lint", "--off"]);
    let (tools, said) = ask(false, &[]);
    assert!(tools.ends_with(" export *),StructuredOutput"), "{tools}");
    assert!(!said.contains(none), "{said}");
}

#[test]
fn what_your_claude_settings_deny_in_the_checkout_is_refused_to_every_ask() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // Yours, untracked: what it denies, every ask is refused, wherever it
    // works. What the tracked one denies, a worktree has already.
    std::fs::create_dir(e.src.join(".claude")).unwrap();
    std::fs::write(e.src.join(".claude/settings.local.json"), r#"{"permissions": {"deny": ["Bash(rm:*)", "WebFetch"], "allow": ["Bash(lint:*)"]}}"#).unwrap();
    std::fs::write(e.src.join(".claude/settings.json"), r#"{"permissions": {"deny": ["Bash(tracked:*)"]}}"#).unwrap();
    // Its arguments a line each: the one after --disallowedTools, what it
    // is refused.
    let args = e.root.join("args");
    let claude = fake_claude(&e, "claude-args.sh", &format!("printf '%s\\n' \"$@\" > {}", args.display()), r#"{"replies": {"c1": "Seen."}}"#);
    let refused = || std::fs::read_to_string(&args).unwrap().lines().skip_while(|l| *l != "--disallowedTools").nth(1).map(String::from);
    // A change, and an answer: what stacked refuses every ask but builds and
    // tests, then what you deny; to an answer, between them, what would make
    // the commands it is given write a file or run a program.
    let reads = stacked::claude::read_refused();
    for way in [None, Some("--answer-only")] {
        let id = comment(&e, &["Look", "at", "it."]);
        let (fine, _, err) = send(&e, &claude, &[&[id.as_str()][..], way.as_slice()].concat(), &[]);
        assert!(fine, "{way:?}: {err}");
        let r = refused().unwrap();
        assert!(r.starts_with("Bash(git rebase *--exec*),") && r.ends_with(",Bash(rm:*),WebFetch") && !r.contains("tracked"), "{way:?}: {r}");
        assert_eq!(r.contains(&format!(",{reads},Bash(rm:*),")), way.is_some(), "{way:?}: {r}");
    }
    // A build and test, which runs what the project needs: what you deny.
    let id = comment(&e, &["Look", "at", "it."]);
    let (fine, _, err) = send(&e, &claude, &[&id, "--build-and-test"], &[]);
    assert!(fine, "{err}");
    assert_eq!(refused().as_deref(), Some("Bash(rm:*),WebFetch"));
    // Nothing denied: a build and test is refused nothing, an answer what
    // stacked refuses.
    std::fs::remove_file(e.src.join(".claude/settings.local.json")).unwrap();
    let id = comment(&e, &["And", "now?"]);
    let (fine, _, err) = send(&e, &claude, &[&id, "--answer-only"], &[]);
    assert!(fine, "{err}");
    assert_eq!(refused(), Some(format!("{},{reads}", stacked::claude::refused_tools())));
    let id = comment(&e, &["And", "built?"]);
    let (fine, _, err) = send(&e, &claude, &[&id, "--build-and-test"], &[]);
    assert!(fine, "{err}");
    assert_eq!(refused(), None);
}

/// A line of a stand-in's script that adds to `file` the effort it was
/// given, a line for each ask: what followed each `--effort`, or `none`.
fn effort_said(file: &Path) -> String {
    format!(
        r#"e=; prev=; for a in "$@"; do if test "$prev" = --effort; then e="$e${{e:+ }}$a"; fi; prev=$a; done; echo "${{e:-none}}" >> '{}'"#,
        file.display()
    )
}

#[test]
fn send_asks_at_the_effort_given_else_at_high() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let efforts = e.root.join("efforts");
    let claude = fake_claude(&e, "claude-effort.sh", &effort_said(&efforts), r#"{"replies": {"c1": "Seen."}}"#);
    let ask = |args: &[&str], env: &[(&str, &Path)]| {
        let id = comment(&e, &["Why?"]);
        let mut all = vec![id.as_str(), "--answer-only"];
        all.extend(args);
        let (fine, _, err) = send(&e, &claude, &all, env);
        assert!(fine, "{args:?}: {err}");
    };
    // None given: high. Given: that one.
    ask(&[], &[]);
    ask(&["--effort", "xhigh"], &[]);
    // The effort an editor of before kept in the state file of its home,
    // as the one its menus picked last: no longer read, high still.
    let state = e.root.join("home/state.json");
    std::fs::create_dir_all(state.parent().unwrap()).unwrap();
    std::fs::write(&state, "{\"recent\": [], \"open\": [], \"effort\": \"low\"}\n").unwrap();
    ask(&[], &[]);
    // The settings' own taken out, the ask's in its place.
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, format!("{HOME_SETTINGS}args = [\"--effort\", \"max\"]\n")).unwrap();
    ask(&["--effort", "medium"], &[("STACKED_CONFIG", &settings)]);
    ask(&[], &[("STACKED_CONFIG", &settings)]);
    let said: Vec<String> = std::fs::read_to_string(&efforts).unwrap().lines().map(String::from).collect();
    assert_eq!(said, ["high", "xhigh", "high", "medium", "high"]);
    // A level claude does not take is refused, the levels it takes said:
    // nothing is sent.
    let id = comment(&e, &["Harder?"]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--effort", "huge"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(!fine && err.contains("invalid value 'huge' for '--effort <LEVEL>'"), "{err}");
    assert!(err.contains("[possible values: low, medium, high, xhigh, max]"), "{err}");
    let db = Db::load(&e.db).unwrap();
    assert!(db.runs().len() == 5 && db.stack_notes("s").iter().all(|n| n.queued.is_none()), "{:?}", db.runs());
}

#[test]
fn a_batch_and_work_sent_back_go_at_the_effort_they_were_asked_at_unless_another_is_given() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let efforts = e.root.join("efforts");
    let claude = fake_claude(&e, "claude-effort.sh", &effort_said(&efforts), r#"{"replies": {"c1": "Seen."}}"#);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let run = |args: &[&str]| {
        let (fine, out, err) = stacked_in(&e.src, &e.db, args, "", &env);
        assert!(fine, "{args:?}: {out}{err}");
    };
    let said = || std::fs::read_to_string(&efforts).unwrap().lines().map(String::from).collect::<Vec<_>>();
    // Batched at low, it goes at low.
    let id = comment(&e, &["Why?"]);
    run(&["send", "s", &id, "--later", "--answer-only", "--effort", "low"]);
    run(&["send", "s", "--batch", "--wait"]);
    assert_eq!(said(), ["low"]);
    // Its work answered in its review, it goes back at the effort it was
    // asked at; at the one given, if one is.
    run(&["reply", "s", &id, "--answer", "Why", "not?"]);
    run(&["review", "s", "--send-back"]);
    landed(&e, 2);
    run(&["reply", "s", &id, "--answer", "And", "then?"]);
    run(&["review", "s", "--send-back", "--effort", "xhigh"]);
    landed(&e, 3);
    assert_eq!(said(), ["low", "low", "xhigh"]);
    // Only with --send-back.
    let (fine, _, err) = e.run(&["review", "s", "--accept-all", "--effort", "low"]);
    assert!(!fine && err.contains("--send-back"), "{err}");
}

#[test]
fn the_effort_the_agent_was_asked_at_is_said_wherever_the_command_line_names_its_work() {
    use stacked::thread::{Role, append_turn, thread_turns};
    let e = env();
    e.ok(&["import", "s", "base"]);
    // A thread of before: the agent's turn in it of no effort.
    let old = comment(&e, &["--patch", "2", "Why", "b?"]);
    Db::edit(&e.db, |db| {
        let n = db.find_note("s", &old)?.clone();
        db.set_note(stacked::db::Note { text: append_turn(&n.text, Role::Agent, "It was so.", Some("answer")), ..n })
    })
    .unwrap();
    // Asked at max: it replies, and opens a comment of its own.
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let efforts = e.root.join("efforts");
    let answer = r#"{"replies": {"c1": "Because."}, "comments": [{"patch": 1, "file": "a.txt", "side": "+", "line": "2", "text": "Say two."}]}"#;
    let claude = fake_claude(&e, "claude-max.sh", &effort_said(&efforts), answer);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--effort", "max", "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{out}{err}");
    assert_eq!(std::fs::read_to_string(&efforts).unwrap(), "max\n");
    // Waiting for your review: `landed` says the effort after what is left
    // of it, `runs` after what the run is, the comment of its reply; the
    // transcript starts as claude was given it.
    let landed = e.ok(&["landed", "s"]);
    let first = landed.lines().next().unwrap();
    assert!(first.ends_with("\t1 reply, 1 opened comment\tasked at max"), "{landed}");
    let runs = e.ok(&["runs", "s"]);
    assert!(runs.contains("\tanswer(max) s \"Why 2?\"\tdone\tits work waits for your review: 1 reply, 1 opened comment\n"), "{runs}");
    let review = e.ok(&["comments", "s", "--show", "to-review"]);
    assert!(review.contains(&format!("{id}\tpatch 1, as a whole\tthe agent's reply (max) waits for your review\n")), "{review}");
    let run = first.split('\t').next().unwrap();
    let transcript = e.ok(&["transcript", run]);
    assert!(transcript.lines().any(|l| l.starts_with("$ ") && l.contains(" --effort max, in ")), "{transcript}");
    // Accepted: its turns headed with it, the comment it opened too; the
    // agent's turn of before as ever.
    e.ok(&["review", "s", "--accept-all"]);
    let c = e.ok(&["comments", "s"]);
    let asked = format!("{id}\tpatch 1, as a whole\tagent replied (answer only, at max) · opened 1 comment\n  you: Why 2?\n  agent(max): Because.\n");
    assert!(c.contains(&asked), "{c}");
    assert!(c.contains("\tpatch 1 · a.txt: 2\tagent replied (answer only, at max)\n  agent(max): Say two.\n"), "{c}");
    assert!(c.contains(&format!("{old}\tpatch 2, as a whole\tagent replied (answer only)\n  you: Why b?\n  agent: It was so.\n")), "{c}");
    // The effort is on the fence of the agent's turn, kept as the thread is
    // written again; the turn of before has none.
    e.ok(&["reply", "s", &id, "Thanks."]);
    let db = Db::load(&e.db).unwrap();
    let text = db.find_note("s", &id).unwrap().text.clone();
    assert!(text.contains("\n```agent answer effort max\nBecause.\n```\nThanks."), "{text}");
    let efforts: Vec<Option<String>> = thread_turns(&text).into_iter().map(|t| t.effort).collect();
    assert_eq!(efforts, [None, Some("max".to_string()), None]);
    let old = db.find_note("s", &old).unwrap().text.clone();
    assert_eq!(old, "Why b?\n```agent answer\nIt was so.\n```");
    drop(db);
    // Moved to the stack's comments by a change asked at low, replying
    // nothing: the agent's turn that moves it says it.
    let moved = comment(&e, &["--patch", "1", "Rename", "two."]);
    let promote = fake_claude(&e, "claude-moves.sh", "", r#"{"replies": {}, "promote": "every patch says two"}"#);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &moved, "--effort", "low", "--wait"], "", &[("STACKED_CLAUDE", &promote)]);
    assert!(fine, "{out}{err}");
    e.ok(&["review", "s", "--accept-all"]);
    let c = e.ok(&["comments", "s", "--show", "failed"]);
    assert!(c.contains("\tthe stack\tmoved from patch 1") && c.contains("  agent(low): This needs more than the patch: every patch says two\n"), "{c}");
}

#[test]
fn the_first_ask_goes_at_once_with_stacked_s_defaults() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // The settings of one who never set up what each kind of ask may use;
    // then of one whose stacked of before saved the setup, its mark passed
    // over.
    let fresh = e.root.join("fresh.toml");
    let args = e.root.join("args");
    let claude = fake_claude(&e, "claude-seen.sh", &format!("printf '%s\\n' \"$@\" > {}", args.display()), r#"{"replies": {"c1": "Seen."}}"#);
    let tools = || std::fs::read_to_string(&args).unwrap().lines().skip_while(|l| *l != "--allowedTools").nth(1).unwrap().to_string();
    let env = [("STACKED_CLAUDE", claude.as_path()), ("STACKED_CONFIG", fresh.as_path())];
    for settings in ["", "[agent]\nsetup = 1\n"] {
        std::fs::write(&fresh, settings).unwrap();
        // Each kind of ask goes at once, with stacked's defaults: curl and
        // jq for answers, nothing more for changes.
        let id = comment(&e, &["Why?"]);
        let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait", "--accept"], "", &env);
        assert!(fine && out.contains("Seen."), "{out}{err}");
        assert!(!err.contains("setup"), "{err}");
        assert!(tools().ends_with(",Bash(curl *),Bash(jq *)"), "{}", tools());
        let id = comment(&e, &["Change", "nothing."]);
        let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait", "--accept"], "", &env);
        assert!(fine && out.contains("Seen."), "{out}{err}");
        assert!(tools().ends_with(" export *),StructuredOutput"), "{}", tools());
        // Batched, it goes with the batch.
        let id = comment(&e, &["Later?"]);
        let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--later"], "", &env);
        assert!(fine, "{err}");
        let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", "--batch", "--wait", "--accept"], "", &env);
        assert!(fine && out.contains("Seen."), "{out}{err}");
        // Nothing written in the settings.
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), settings);
    }
    // The setup still saves each list, and no mark of its own.
    let init = serde_json::json!({"type": "system", "subtype": "init", "model": "fake", "tools": ["Read", "Bash"], "mcp_servers": []});
    let agent = e.script("claude-init.sh", &format!("echo '{init}'\nsleep 30\n"));
    std::fs::write(&fresh, "").unwrap();
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["setup", "agent", "s", "--global"], "", &[("STACKED_CLAUDE", &agent), ("STACKED_CONFIG", &fresh)]);
    assert!(fine, "{err}");
    assert!(err.contains(&format!("stacked: the setup is saved in {}: change_tools: none; answer_tools: Bash(curl *), Bash(jq *)", fresh.display())), "{err}");
    assert_eq!(
        std::fs::read_to_string(&fresh).unwrap(),
        format!("[agent]\nchange_tools = []\nanswer_tools = [\"Bash(curl *)\", \"Bash(jq *)\"]\n{}", try_line("\"Bash(curl *)\", \"Bash(jq *)\""))
    );
}

#[test]
fn what_each_kind_of_ask_may_use_is_set_up_with_the_agent() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // A claude that says what it has, then would go on: it is stopped once
    // it said it.
    let init = serde_json::json!({"type": "system", "subtype": "init", "model": "fake",
        "tools": ["Task", "Bash", "Glob", "Grep", "LS", "Read", "Edit", "WebFetch",
            "mcp__moz__get_bug", "mcp__moz__get_revision", "mcp__notes__read", "mcp__notes__write"],
        "mcp_servers": [{"name": "moz", "status": "connected"}, {"name": "notes", "status": "connected"}]});
    let pwd = e.root.join("pwd");
    let claude = e.script("claude-init.sh", &format!("pwd > {}\necho '{init}'\nsleep 30\n", pwd.display()));
    // Settings with every ask let use notes' write, the setup not done.
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, "[agent]\ntools = [\"mcp__notes__write\"]\n").unwrap();
    // The checkout's instructions name a program, there on a PATH of the
    // test's own; its claude settings, yours, allow a command.
    let bin = e.root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("searchfox-cli"), "#!/bin/sh\n").unwrap();
    Command::new("chmod").arg("+x").arg(bin.join("searchfox-cli")).status().unwrap();
    let path = PathBuf::from(format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    std::fs::write(e.src.join("AGENTS.md"), "Search the code:\n\n```sh\nsearchfox-cli --define Foo\n```\n").unwrap();
    std::fs::create_dir(e.src.join(".claude")).unwrap();
    std::fs::write(e.src.join(".claude/settings.local.json"), r#"{"permissions": {"allow": ["Bash(lint:*)"]}}"#).unwrap();
    let env = [("STACKED_CLAUDE", claude.as_path()), ("STACKED_CONFIG", settings.as_path()), ("PATH", path.as_path())];
    // One list, a row at a time, each asked for both kinds of ask: curl,
    // jq, the skills, lint and searchfox-cli kept as they are; all of moz's
    // for changes; notes' each, the one that reads for changes, write kept;
    // nothing else. Then a command added, for changes.
    let started = std::time::Instant::now();
    let input = "n\n\n\n\n\n\nc\ne\nc\n\n\n\n\n\nBash(make check *)\nc\n\n";
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["setup", "agent", "s"], input, &env);
    assert!(fine, "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "stopped once it said what it has");
    assert_eq!(std::fs::read_to_string(&pwd).unwrap().trim(), e.src.display().to_string(), "in the stack's repository");
    let local = e.src.join(".stacked/local.toml");
    for line in [
        format!("stacked: the agent, in {}, has 12 tools. What each kind of ask may use is saved in {}; Enter keeps what is ticked.", e.src.display(), local.display()),
        two(" read only  change   read only is for answer-only asks; change is for asks that change the stack"),
        two("  always             commands that only read: grep, rg, cat, ls, diff…, and git blame, ls-files, rev-parse…"),
        two("            always   commands that read: grep, rg, find, cat, sed, sort…"),
        two("            always   the checkout's linter, when it has one: this checkout has none"),
        two("    [x]      [ ]     Bash(lint *)   allowed in .claude/settings.local.json"),
        two("    [x]      [x]       write"),
        "Keep them as they are? [Y/n: one by one] ".into(),
        "  Which asks may use Bash(lint *), allowed in .claude/settings.local.json? [R: read only, c: change, b: both, n: neither]".into(),
        "  Which asks may use Bash(searchfox-cli *), named in AGENTS.md? [r: read only, c: change, b: both, N: neither]".into(),
        "  The MCP server moz has these tools: get_bug, get_revision.".into(),
        "  Which asks may use WebFetch, one of the agent's tools? It can reach the network: a change could touch a remote. [r: read only, c: change, b: both, N: neither]".into(),
        "  Bash(make check *): added.".into(),
    ] {
        assert!(err.contains(&line), "{line:?} in:\n{err}");
    }
    // Written in the checkout's own settings, both lists and the setup
    // done, notes' write left to agent.tools as both still let it; lint the
    // newer way, though your claude settings write it the older; the
    // settings' file as it was.
    // Try pushes, line by line, as read only: what answers may use.
    let said = format!(
        "change_tools: mcp__moz__*, mcp__notes__read, Bash(make check *); answer_tools: Bash(curl *), Bash(jq *), Bash(lint *){}",
        try_said("Bash(curl *), Bash(jq *), Bash(lint *)")
    );
    assert!(err.contains(&format!("stacked: the setup is saved in {}: {said}", local.display())), "{err}");
    let saved = format!(
        "[agent]\nchange_tools = [\"mcp__moz__*\", \"mcp__notes__read\", \"Bash(make check *)\"]\nanswer_tools = [\"Bash(curl *)\", \"Bash(jq *)\", \"Bash(lint *)\"]\n{}",
        try_line("\"Bash(curl *)\", \"Bash(jq *)\", \"Bash(lint *)\"")
    );
    assert_eq!(std::fs::read_to_string(&local).unwrap(), saved);
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "[agent]\ntools = [\"mcp__notes__write\"]\n");
    assert_eq!(
        std::fs::read_to_string(e.src.join(".claude/settings.local.json")).unwrap(),
        r#"{"permissions": {"allow": ["Bash(lint:*)"]}}"#,
        "your claude settings never written"
    );
    // Again, as it is, the checkout's own list holding lint the older way,
    // as an earlier stacked saved it: the settings' ticks now, the lists
    // set. The instructions now name lint too, there on PATH: still one
    // row, the newer way, and lint saved so. The checkout has ./mach at
    // its top now, a stand-in never run: the linter changes are given is
    // named.
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["config", "--local", "agent.answer_tools", "Bash(curl *)", "Bash(jq *)", "Bash(lint:*)"], "", &env);
    assert!(fine, "{err}");
    std::fs::write(bin.join("lint"), "#!/bin/sh\n").unwrap();
    Command::new("chmod").arg("+x").arg(bin.join("lint")).status().unwrap();
    std::fs::write(e.src.join("AGENTS.md"), "Search the code:\n\n```sh\nsearchfox-cli --define Foo\nlint --all\n```\n").unwrap();
    std::fs::write(e.src.join("mach"), "#!/bin/sh\necho 'not the real mach: the tests never run it' >&2\nexit 2\n").unwrap();
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["setup", "agent", "s"], "", &env);
    assert!(fine, "{err}");
    for line in [
        "            always   this checkout's linter, ./mach lint --fix, to run on the files a change changes (Bash(./mach lint *))",
        "    [ ]      [x]     all of the MCP server moz's tools, those to come too (mcp__moz__*)",
        "    [ ]      [x]     Bash(make check *)   in the settings",
        "    [x]      [ ]     Bash(lint *)   allowed in .claude/settings.local.json; named in AGENTS.md",
    ]
    .map(two)
    {
        assert!(err.lines().any(|l| l == line), "{line:?} in:\n{err}");
    }
    assert!(!err.contains("Bash(lint:*)"), "{err}");
    assert!(err.contains(&format!("stacked: the setup is saved in {}: {said}", local.display())), "{err}");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), saved);
    // With --global, in the settings' file, starting from its own: lint
    // ticked for answers as your claude settings allow it, the newer way.
    // The checkout's still the one used there, as said.
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["setup", "agent", "s", "--global"], "", &env);
    assert!(fine, "{err}");
    assert!(err.contains(&format!("What each kind of ask may use is saved in {};", settings.display())), "{err}");
    assert!(err.contains("sets agent.change_tools too, and is the one used in its checkout"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        format!(
            "[agent]\ntools = [\"mcp__notes__write\"]\nchange_tools = []\nanswer_tools = [\"Bash(curl *)\", \"Bash(jq *)\", \"Bash(lint *)\"]\n{}",
            try_line("\"Bash(curl *)\", \"Bash(jq *)\", \"Bash(lint *)\"")
        )
    );
    // Outside a checkout, refused before the agent is asked.
    std::fs::remove_file(&pwd).unwrap();
    let (fine, _, err) = stacked_in(&e.root, &e.db, &["setup", "agent"], "", &env);
    assert!(!fine && err.contains("is not in a git checkout"), "{err}");
    assert!(!pwd.exists(), "the agent was not started");
}

#[test]
fn a_checkout_s_own_settings_override_the_others_there() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, format!("{HOME_SETTINGS}tools = [\"mcp__bugzilla__*\"]\n[view]\nlayout = \"compact\"\n")).unwrap();
    let config = |dir: &Path, args: &[&str]| {
        let mut all = vec!["config"];
        all.extend(args);
        stacked_in(dir, &e.db, &all, "", &[("STACKED_CONFIG", &settings)])
    };
    let local = e.src.join(".stacked/local.toml");
    // Written in the checkout's own, kept out of git's sight.
    let (fine, _, err) = config(&e.src, &["--local", "agent.tools", "mcp__moz__*", "Bash(jq *)"]);
    assert!(fine && err.contains(&format!("agent.tools written in {}", local.display())), "{err}");
    assert_eq!(git(&e.src, &["status", "--porcelain"]), "");
    // There, it is used over the settings' file, which has the rest; not
    // elsewhere.
    assert_eq!(config(&e.src, &["agent.tools"]).1, "mcp__moz__*\nBash(jq *)\n");
    assert_eq!(config(&e.src, &["view.layout"]).1, "compact\n");
    assert_eq!(config(&e.root, &["agent.tools"]).1, "mcp__bugzilla__*\n");
    assert_eq!(config(&e.src, &["--local", "agent.tools"]).1, "mcp__moz__*\nBash(jq *)\n");
    let (fine, _, err) = config(&e.src, &["--local", "view.layout"]);
    assert!(!fine && err.contains(&format!("view.layout is not set in {}", local.display())), "{err}");
    // `config` says both files, and which settings are the checkout's;
    // `config --local`, that file alone.
    let (_, out, _) = config(&e.src, &[]);
    assert!(out.contains(&format!("# then from {}, this checkout's own, over it\n", local.display())), "{out}");
    assert!(out.contains("\ntools = [\"mcp__moz__*\", \"Bash(jq *)\"]   # .stacked/local.toml\n"), "{out}");
    assert!(out.contains("\nlayout = \"compact\"\n"), "{out}");
    let (_, out, _) = config(&e.src, &["--local"]);
    assert!(out.ends_with("\n[agent]\ntools = [\"mcp__moz__*\", \"Bash(jq *)\"]\n"), "{out}");
    // Written in the settings' file, what the checkout sets is said to win
    // there.
    let (fine, _, err) = config(&e.src, &["agent.tools", "WebFetch"]);
    assert!(fine && err.contains(&format!("{} sets agent.tools too, and is the one used in its checkout", local.display())), "{err}");
    // An ask on the stack has its checkout's, wherever stacked runs.
    let args = e.root.join("args");
    let claude = fake_claude(&e, "claude-tools.sh", &format!("echo \"$*\" > {}", args.display()), r#"{"replies": {"c1": "Read it."}}"#);
    let id = comment(&e, &["What", "does", "bug", "1", "say?"]);
    let (fine, _, err) = stacked_in(
        &e.root,
        &e.db,
        &["send", "s", &id, "--answer-only", "--wait", "--accept"],
        "",
        &[("STACKED_CLAUDE", &claude), ("STACKED_CONFIG", &settings)],
    );
    assert!(fine, "{err}");
    let said = std::fs::read_to_string(&args).unwrap();
    let tools = said.split("--allowedTools ").nth(1).unwrap().split(" --").next().unwrap().trim_end();
    assert!(tools.ends_with(",mcp__moz__*,Bash(jq *)") && !tools.contains("WebFetch") && !tools.contains("bugzilla"), "{tools}");
    // Taken out: the settings' file's again.
    let (fine, _, err) = config(&e.src, &["--local", "--unset", "agent.tools"]);
    assert!(fine && err.contains(&format!("agent.tools taken out of {}", local.display())), "{err}");
    assert_eq!(config(&e.src, &["agent.tools"]).1, "WebFetch\n");
    // One git tracks came with the repository, as any setting in it would:
    // not read, said, and not written.
    assert!(config(&e.src, &["--local", "editor", "hx"]).0);
    assert_eq!(config(&e.src, &["editor"]).1, "hx\n");
    git(&e.src, &["add", "-f", ".stacked/local.toml"]);
    let (fine, _, err) = config(&e.src, &["editor"]);
    assert!(!fine && err.contains("editor is not set"), "{err}");
    let (_, out, _) = config(&e.src, &[]);
    assert!(out.contains("git tracks it, and a checkout's own settings are yours, not the repository's; it is not read"), "{out}");
    let (fine, _, err) = config(&e.src, &["--local", "editor", "vim"]);
    assert!(!fine && err.contains("; nothing written"), "{err}");
    // Outside a checkout, there is none.
    let (fine, _, err) = config(&e.root, &["--local", "editor", "vim"]);
    assert!(!fine && err.contains("is not in a git checkout"), "{err}");
}

#[test]
fn the_linter_is_set_in_the_checkout_s_own_settings_alone() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let local = e.src.join(".stacked/local.toml");
    let global = e.root.join("home/config.toml");
    // None set, and no ./mach: said how to set one.
    let (fine, _, err) = e.run(&["setup", "lint"]);
    assert!(!fine && err.contains("no linter is set for this checkout: `stacked setup lint COMMAND…` sets one"), "{err}");
    // Set: in the checkout's own settings, kept out of git's sight, never in
    // the settings' file; the words as given, quoted where they need it.
    let (fine, _, err) = e.run(&["setup", "lint", "./lint.sh", "--fix", "my file"]);
    assert!(fine, "{err}");
    assert!(err.contains(&format!("the linter of this checkout is `./lint.sh --fix 'my file'`, written in {}", local.display())), "{err}");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "[lint]\ncommand = \"./lint.sh --fix 'my file'\"\n");
    assert_eq!(std::fs::read_to_string(&global).unwrap(), HOME_SETTINGS);
    assert_eq!(git(&e.src, &["status", "--porcelain"]), "");
    // Printed, with where it comes from; read as any setting is.
    let (fine, out, err) = e.run(&["setup", "lint"]);
    assert!(fine && out == "./lint.sh --fix 'my file'\n", "{out}{err}");
    assert!(err.contains(&format!("this checkout's linter, as {} sets it", local.display())), "{err}");
    assert_eq!(e.ok(&["config", "lint.command"]), "./lint.sh --fix 'my file'\n");
    assert_eq!(e.ok(&["config", "--local", "lint.command"]), "./lint.sh --fix 'my file'\n");
    // Written by `setup lint` alone: `config` refuses, the checkout's own or
    // the settings' file, nothing written.
    for args in [&["config", "lint.command", "x"][..], &["config", "--local", "lint.command", "x"]] {
        let (fine, _, err) = e.run(args);
        assert!(!fine && err.contains("lint.command is written by `stacked setup lint COMMAND…`"), "{args:?}: {err}");
    }
    assert_eq!(std::fs::read_to_string(&global).unwrap(), HOME_SETTINGS);
    // Turned off: an empty command, said so.
    let (fine, _, err) = e.run(&["setup", "lint", "--off"]);
    assert!(fine && err.contains("lint is turned off in this checkout: command = \"\" is written in"), "{err}");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "[lint]\ncommand = \"\"\n");
    let (fine, out, err) = e.run(&["setup", "lint"]);
    assert!(fine && out.is_empty() && err.contains("lint is turned off in this checkout"), "{out}{err}");
    // A checkout whose top has ./mach: Firefox's by default, nothing
    // written; off as it was set, until the setting is taken out.
    std::fs::write(e.src.join("mach"), "#!/bin/sh\nexit 2\n").unwrap();
    let (_, out, _) = e.run(&["setup", "lint"]);
    assert!(out.is_empty(), "off is kept over the default: {out}");
    e.ok(&["config", "--local", "--unset", "lint.command"]);
    let (fine, out, err) = e.run(&["setup", "lint"]);
    assert!(fine && out == "./mach lint --fix\n", "{out}{err}");
    assert!(err.contains("as ./mach is at its top") && err.contains("nothing is written"), "{err}");
    let (fine, _, err) = e.run(&["config", "--local", "lint.command"]);
    assert!(!fine && err.contains("lint.command is not set in"), "nothing written: {err}");
    // A settings' file that sets one: passed over, and said.
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, format!("{HOME_SETTINGS}[lint]\ncommand = \"rm -rf\"\n")).unwrap();
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["setup", "lint"], "", &[("STACKED_CONFIG", &settings)]);
    assert!(fine && out == "./mach lint --fix\n", "{out}{err}");
    assert!(err.contains("lint.command is a checkout's own setting, read only from its .stacked/local.toml"), "{err}");
    // One git tracks is not written; nor is one outside a checkout.
    git(&e.src, &["add", "-f", ".stacked/local.toml"]);
    let (fine, _, err) = e.run(&["setup", "lint", "./lint.sh"]);
    assert!(!fine && err.contains("git tracks it") && err.contains("nothing written"), "{err}");
    let (fine, _, err) = stacked_in(&e.root, &e.db, &["setup", "lint", "./lint.sh"], "", &[]);
    assert!(!fine && err.contains("is not in a git checkout"), "{err}");
}

#[test]
fn a_file_is_shown_as_a_patch_leaves_it() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // As each patch leaves it, the patches below with it, no checkout made.
    assert_eq!(e.ok(&["show", "s", "1", "--path", "a.txt"]), TEN.replace("two", "2"));
    assert_eq!(e.ok(&["show", "s", "3", "--path", "a.txt"]), TEN.replace("two", "2").replace("nine", "9"));
    assert_eq!(e.ok(&["show", "s", "2", "--path", "b.txt"]), "new\n");
    let (fine, _, err) = e.run(&["show", "s", "1", "--path", "b.txt"]);
    assert!(!fine && err.contains("as patch 1 leaves it, b.txt is not there"), "{err}");
    // As the stack was at a rev; a file no patch changes, as the base has
    // it.
    let rev = e.ok(&["log", "s"]).split('\t').next().unwrap().to_string();
    e.ok(&["drop", "s", "1"]);
    assert_eq!(e.ok(&["show", "s", "1", "--path", "a.txt"]), TEN, "no patch below changes it now");
    assert_eq!(e.ok(&["show", "s", "1", "--path", "a.txt", "--rev", &rev]), TEN.replace("two", "2"));
    assert_eq!(e.ok(&["show", "s", "--rev", &rev]).lines().count(), 3);
}

#[test]
fn a_later_patch_that_no_longer_applies_is_resolved_by_hand() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // An editor named vim, told the line as +LINE, that takes a step of
    // its own each time it runs: what it was shown, and where, are kept.
    let bin = e.root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let vim = bin.join("vim");
    let r = e.root.display().to_string();
    std::fs::write(
        &vim,
        format!(
            "#!/bin/sh\nn=$(( $(cat {r}/count 2>/dev/null || echo 0) + 1 ))\necho $n > {r}/count\necho \"$1\" > {r}/line$n\ncp \"$2\" {r}/shown$n\nsh {r}/step$n \"$2\"\n"
        ),
    )
    .unwrap();
    Command::new("chmod").arg("+x").arg(&vim).status().unwrap();
    let steps = |steps: &[&str]| {
        let _ = std::fs::remove_file(e.root.join("count"));
        for (i, s) in steps.iter().enumerate() {
            std::fs::write(e.root.join(format!("step{}", i + 1)), s).unwrap();
        }
    };
    let shown = |n: usize| std::fs::read_to_string(e.root.join(format!("shown{n}"))).unwrap();
    let line = |n: usize| std::fs::read_to_string(e.root.join(format!("line{n}"))).unwrap().trim().to_string();
    let edit = |extra: &[&str]| {
        let mut all = vec!["edit", "s", "--accept"];
        all.extend(extra);
        stacked_in(&e.src, &e.db, &all, "", &[("EDITOR", &vim)])
    };
    // A step that writes `text` in the file.
    let put = |n: usize, text: &str| {
        let file = e.root.join(format!("text{n}"));
        std::fs::write(&file, text).unwrap();
        format!("cp {} \"$1\"", file.display())
    };
    let huit = &sed_i("s/^eight$/huit/", "$1");

    // Patch 3 changes nine, under eight: eight edited in patch 1, patch 3
    // no longer applies, and the edit is not taken; the error says how to
    // resolve it.
    steps(&[huit]);
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "2"]);
    assert!(!fine && err.contains("patch 3 (Bug 3 - café") && err.contains("does not apply"), "{err}");
    assert!(err.contains("; with --resolve, a.txt of patch 3 opens next, to resolve the conflict by hand"), "{err}");
    // With --resolve, it does: a.txt as patch 3 would leave it on the edit,
    // opened at the conflict. Saved with its markers in, it is not
    // resolved, and nothing changed.
    steps(&[huit, "true"]);
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "2", "--resolve"]);
    assert!(!fine && err.contains("a.txt still has conflict markers"), "{err}");
    assert!(err.contains(" --resolve` takes it up again"), "{err}");
    assert!(shown(2).ends_with("seven\n<<<<<<< your edit\nhuit\nnine\n=======\neight\n9\n>>>>>>> patch 3\nten\n"), "{}", shown(2));
    assert_eq!(line(2), "+8");
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 1, "nothing changed");
    // Resolved, both are new versions, in one change of the stack.
    let both = put(2, &TEN.replace("two", "2").replace("eight", "huit").replace("nine", "9"));
    steps(&[huit, &both]);
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "2", "--resolve"]);
    assert!(fine && err.contains("edited a.txt in Bug 1 - change two, and resolved patch 3 by hand"), "{err}");
    assert!(e.ok(&["show", "s", "1"]).contains("-eight\n+huit\n"));
    let three = e.ok(&["show", "s", "3"]);
    assert!(three.contains(" huit\n-nine\n+9\n"), "{three}");
    assert!(three.contains("Subject: Bug 3 - café"), "its message as it was: {three}");
    assert_eq!(e.ok(&["versions", "s", "3"]).lines().count(), 2);
    assert!(e.ok(&["check"]).contains("s: applies"));
    let log = e.ok(&["log", "s"]);
    assert_eq!(log.matches(", resolve ").count(), 1, "one change: {log}");

    // One conflict after another: patch 3 has one, then patch 4's change
    // merges with the edit without one, and is only saved as it opens.
    let ten = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -7,4 +7,4 @@\n seven\n huit\n 9\n-ten\n+10\n";
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["add", "s", "-m", "Bug 4 - ten"], ten, &[]);
    assert!(fine, "{err}");
    let loud = &sed_i("s/^huit$/HUIT/", "$1");
    let both = put(2, &TEN.replace("two", "2").replace("eight", "HUIT").replace("nine", "9"));
    steps(&[loud, &both, "true"]);
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--resolve"]);
    assert!(fine && err.contains("edited a.txt in Bug 1 - change two, and resolved patches 3 and 4 by hand"), "{err}");
    assert!(shown(2).contains("<<<<<<< your edit\nHUIT\nnine\n=======\nhuit\n9\n>>>>>>> patch 3\n"), "{}", shown(2));
    assert_eq!(shown(3), TEN.replace("two", "2").replace("eight", "HUIT").replace("nine", "9").replace("ten", "10"));
    assert_eq!(line(3), "+1", "no conflict to open at");
    assert!(e.ok(&["show", "s", "4"]).contains(" HUIT\n 9\n-ten\n+10\n"));
    assert!(e.ok(&["check"]).contains("s: applies"));
}

#[test]
fn a_file_is_edited_in_the_stack_s_worktree_or_in_this_checkout() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // An editor named vim, told the line as +LINE: it says where it ran,
    // and at which commit, then edits the file.
    let bin = e.root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let said = e.root.join("said");
    let vim = bin.join("vim");
    let script = |body: &str| {
        let log = format!("echo \"$1 $(pwd) $(git log -1 --format=%s)\" >> {}", said.display());
        std::fs::write(&vim, format!("#!/bin/sh\n{log}\nfile=\"$2\"\n{body}\n")).unwrap();
        Command::new("chmod").arg("+x").arg(&vim).status().unwrap();
    };
    let edit = |extra: &[&str]| {
        let mut all = vec!["edit", "s", "--accept"];
        all.extend(extra);
        stacked_in(&e.src, &e.db, &all, "", &[("EDITOR", &vim)])
    };
    let last = || std::fs::read_to_string(&said).unwrap().lines().last().unwrap().to_string();
    let head = || git(&e.src, &["rev-parse", "HEAD"]);
    let branch = || git(&e.src, &["symbolic-ref", "--short", "HEAD"]).trim().to_string();
    let top = head();

    // In the stack's own worktree, .stacked/s/user, made the first time: at
    // patch 1, the editor in it; what the edit changes there, a new file
    // too, is patch 1 again. One of before, .stacked/s itself, is moved
    // there.
    git(&e.src, &["worktree", "add", "--quiet", "--detach", e.src.join(".stacked/s").to_str().unwrap(), "base"]);
    script(&format!("{}\necho new > c.txt", sed_i("s/^2$/deux/", "$file")));
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "2", "--in", "worktree"]);
    assert!(fine && err.contains("edited a.txt and c.txt in Bug 1 - change two"), "{err}");
    let kept = e.src.join(".stacked/s/user");
    assert_eq!(last(), format!("+2 {} Bug 1 - change two", kept.display()));
    let list = git(&e.src, &["worktree", "list"]);
    assert!(list.contains("/.stacked/s/user ") && !list.contains("/.stacked/s "), "moved: {list}");
    let mail = e.ok(&["show", "s", "1"]);
    assert!(mail.contains("-two\n+deux\n") && mail.contains("+++ b/c.txt"), "{mail}");
    assert!(mail.contains("Subject: Bug 1 - change two"), "{mail}");
    assert!(e.ok(&["check"]).contains("s: applies"));
    // Out of git's sight in the repository, which is as it was.
    assert_eq!(git(&e.src, &["status", "--porcelain"]), "");
    assert_eq!((head(), branch()), (top.clone(), "main".to_string()));
    assert!(std::fs::read_to_string(e.src.join(".git/info/exclude")).unwrap().contains("/.stacked/"));
    // Kept: the next edit moves it to its patch.
    script(&sed_i("s/^new$/newer/", "$file"));
    let (fine, _, err) = edit(&["2", "--path", "b.txt", "--line", "1", "--in", "worktree"]);
    assert!(fine && err.contains("edited b.txt in Bug 2 - add b"), "{err}");
    assert_eq!(last(), format!("+1 {} Bug 2 - add b", kept.display()));
    assert_eq!(git(&e.src, &["worktree", "list"]).lines().count(), 2);

    // In this checkout: not while Move HEAD would not move HEAD, which on
    // main forks from the stack the edits made at its base.
    script(&sed_i("s/^newer$/fresh/", "$file"));
    let (fine, _, err) = edit(&["2", "--path", "b.txt", "--line", "1", "--in", "pwd"]);
    assert!(
        !fine && err.contains("this checkout is not offered for edits by hand: HEAD has commits stacked did not make, as 3 commits after the base are not the stack's: Move HEAD moves HEAD only when"),
        "{err}"
    );
    assert_eq!((head(), branch()), (top.clone(), "main".to_string()));
    // Put on the stack's base, then moved to its top by stacked: moved to
    // patch 2 for the edit, then back there, the files of yours git does
    // not track left alone.
    git(&e.src, &["checkout", "-q", "--detach", "base"]);
    e.ok(&["head", "s", "--to", "3"]);
    let top = head();
    std::fs::write(e.src.join("notes.txt"), "mine\n").unwrap();
    let (fine, _, err) = edit(&["2", "--path", "b.txt", "--line", "1", "--in", "pwd"]);
    assert!(fine && err.contains("edited b.txt in Bug 2 - add b"), "{err}");
    assert_eq!(last(), format!("+1 {} Bug 2 - add b", e.src.display()));
    assert!(e.ok(&["show", "s", "2"]).contains("+fresh"));
    assert_eq!(head(), top);
    assert_eq!(git(&e.src, &["status", "--porcelain"]), "?? notes.txt\n");
    // Not over changes of yours to what git tracks.
    std::fs::write(e.src.join("a.txt"), "mine\n").unwrap();
    let (fine, _, err) = edit(&["2", "--path", "b.txt", "--in", "pwd"]);
    assert!(!fine && err.contains("has uncommitted changes"), "{err}");
    git(&e.src, &["checkout", "a.txt"]);

    // Closed without saving, in either: nothing changed.
    let size = std::fs::metadata(&e.db).unwrap().len();
    script("true");
    for w in ["pwd", "worktree"] {
        let (fine, _, err) = edit(&["1", "--path", "a.txt", "--in", w]);
        assert!(fine && err.contains("saved as it was: nothing changed"), "{w}: {err}");
    }
    assert_eq!(std::fs::metadata(&e.db).unwrap().len(), size);
    assert_eq!(head(), top);

    // Without --in, where the stack's menus picked. One the patch above
    // cannot take is not taken: it is kept in a commit, and taken up again
    // from there.
    Db::edit(&e.db, |db| db.set_choice("s", "edit", "worktree")).unwrap();
    script(&sed_i("s/^eight$/huit/", "$file"));
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "8"]);
    assert!(!fine && err.contains("patch 3 (Bug 3 - café") && err.contains("does not apply"), "{err}");
    assert_eq!(last(), format!("+8 {} Bug 1 - change two", kept.display()));
    assert!(err.contains("--in worktree --from "), "{err}");
    let commit = err.split("--from ").nth(1).unwrap().split('`').next().unwrap().to_string();
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2, "nothing changed");
    script(&sed_i("s/^huit$/eight/", "$file"));
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--in", "worktree", "--from", &commit]);
    assert!(fine && err.contains("saved as it was"), "back as it was: {err}");
}

#[test]
fn one_setting_is_read_written_and_taken_out() {
    let e = env();
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, "# mine\n[view]\nlayout = \"compact\"\n").unwrap();
    let config = |args: &[&str]| {
        let mut all = vec!["config"];
        all.extend(args);
        stacked_in(&e.src, &e.db, &all, "", &[("STACKED_CONFIG", &settings)])
    };
    let (fine, out, _) = config(&["view.layout"]);
    assert!(fine && out == "compact\n", "{out}");
    let (fine, _, err) = config(&["editor"]);
    assert!(!fine && err.contains("editor is not set"), "{err}");
    let (fine, _, err) = config(&["editor", "hx"]);
    assert!(fine && err.contains("editor written in"), "{err}");
    assert_eq!(config(&["editor"]).1, "hx\n");
    assert!(config(&["keys.down", "down", "j"]).0);
    assert_eq!(config(&["keys.down"]).1, "down\nj\n");
    let (fine, _, err) = config(&["--unset", "view.layout"]);
    assert!(fine && err.contains("view.layout taken out"), "{err}");
    assert_eq!(config(&["view.layout"]).1, "unified\n");
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        "editor = \"hx\"\n# mine\n[view]\n\n[keys]\ndown = [\"down\", \"j\"]\n"
    );
    // Refused: the file as it was.
    let (fine, _, err) = config(&["view.layout", "sideways"]);
    assert!(!fine && err.contains("nothing written"), "{err}");
    let (fine, _, err) = config(&["colour", "red"]);
    assert!(!fine && err.contains("not a setting"), "{err}");
    // All of them, as before.
    let (fine, out, _) = config(&[]);
    assert!(fine && out.contains("editor = \"hx\"") && out.contains("down = [\"down\", \"j\"]"), "{out}");
    assert!(out.contains("\n[colors]\n") && out.contains("\nyou = \"172\"\n"), "{out}");
    assert!(out.contains("\npicked = \"normal 238\"\n"), "{out}");
    // A colour, as git writes one; refused if it is not one.
    assert!(config(&["colors.you", "208 bold"]).0);
    assert_eq!(config(&["colors.you"]).1, "208 bold\n");
    let (fine, _, err) = config(&["colors.agent", "orangeish"]);
    assert!(!fine && err.contains("colors.agent: \"orangeish\" is not a colour"), "{err}");
}

#[test]
fn closed_comments_are_listed_as_they_were() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    assert_eq!(e.ok(&["comments", "s", "--show", "closed"]), "");
    // Closed as the editor closes one: its text emptied.
    Db::edit(&e.db, |db| {
        let n = db.stack_notes("s").into_iter().next().cloned().unwrap();
        db.set_note(stacked::db::Note { text: String::new(), ..n })
    })
    .unwrap();
    assert!(!e.ok(&["comments", "s"]).contains("Why 2?"));
    let out = e.ok(&["comments", "s", "--show", "closed"]);
    assert!(out.starts_with(&id) && out.contains("\tclosed @") && out.contains("  you: Why 2?"), "{out}");
    assert_eq!(e.ok(&["comments", "s", "--patch", "1", "--show", "closed"]), out);
    assert_eq!(e.ok(&["comments", "s", "--patch", "2", "--show", "closed"]), "");
    // As it was called before: deleted.
    assert_eq!(e.ok(&["comments", "s", "--show", "deleted"]), out);
}

#[test]
fn what_you_said_last_may_be_written_again() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    e.ok(&["reply", "s", &id, "--edit", "Why", "2, not two?"]);
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("you: Why 2, not two?") && !c.contains("you: Why 2?\n"), "{c}");
    // Answered, the last word is the agent's.
    let answer = fake_claude(&e, "claude-seen.sh", "", r#"{"replies": {"c1": "Seen."}}"#);
    let (fine, _, err) = send(&e, &answer, &[&id, "--answer-only"], &[]);
    assert!(fine, "{err}");
    let (fine, _, err) = e.run(&["reply", "s", &id, "--edit", "Hm."]);
    assert!(!fine && err.contains("the last word is not yours"), "{err}");
    // Replied to, the reply is what is written again, the rest as it was.
    e.ok(&["reply", "s", &id, "Thanks."]);
    e.ok(&["reply", "s", &id, "--edit", "Thanks, and why 9?"]);
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("you: Why 2, not two?") && c.contains("agent(high): Seen.") && c.contains("you: Thanks, and why 9?"), "{c}");
    assert!(!c.contains("you: Thanks.\n"), "{c}");
}

#[test]
fn any_turn_may_be_written_again_and_the_conversation_rewound() {
    use stacked::thread::{Role, append_turn};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    // Answered at length, thanked, answered again; and a turn deleted the
    // first way, which nothing shows, nor counts.
    Db::edit(&e.db, |db| {
        let n = db.stack_notes("s").into_iter().next().cloned().unwrap();
        let text = append_turn(&n.text, Role::Agent, "Seen, at length.", Some("answer"));
        let text = append_turn(&text, Role::You, "Thanks.", None);
        let text = format!("{text}\n```deleted agent answer\n> Gone.\n```");
        db.set_note(stacked::db::Note { text: append_turn(&text, Role::Agent, "You are welcome.", Some("answer")), ..n })
    })
    .unwrap();
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("\n  you: Why 2?\n  agent: Seen, at length.\n  you: Thanks.\n  agent: You are welcome.\n") && !c.contains("Gone"), "{c}");
    // The agent's turn, the second, trimmed.
    e.ok(&["reply", "s", &id, "--edit", "--turn", "2", "Seen."]);
    assert!(e.ok(&["comments", "s", "--patch", "1"]).contains("\n  agent: Seen.\n  you: Thanks.\n"));
    // Rewound from the agent's last turn, the fourth: from your turn above
    // it, the third, which the new text takes the place of, and the rest.
    e.ok(&["reply", "s", &id, "--rewind", "4", "Thanks, and why 9?"]);
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("\n  you: Why 2?\n  agent: Seen.\n  you: Thanks, and why 9?\n") && !c.contains("welcome"), "{c}");
    for (args, says) in [
        (&["--rewind", "9", "Hm."][..], "there is no turn 9: the thread has 3"),
        (&["--rewind", "2", "--edit", "Hm."], "cannot be used with"),
    ] {
        let (fine, _, err) = e.run(&[&["reply", "s", &id][..], args].concat());
        assert!(!fine && err.contains(says), "{args:?}: {err}");
    }
    // From the comment itself, the whole thread is written again; the
    // history says so.
    e.ok(&["reply", "s", &id, "--rewind", "1", "Why", "two?"]);
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.ends_with("\n  you: Why two?\n"), "{c}");
    let d = Db::load(&e.db).unwrap();
    let n = d.stack_notes("s").into_iter().next().cloned().unwrap();
    let mut said = Vec::new();
    let mut before: Option<&stacked::db::Note> = None;
    for (_, x) in d.note_history(&n.change, &n.id) {
        said.push(stacked::history::note_change(before, x));
        before = Some(x);
    }
    assert!(said.contains(&"you rewound the conversation from your reply".to_string()), "{said:?}");
    assert_eq!(said.last().map(String::as_str), Some("you rewound the conversation from your comment"), "{said:?}");
}

#[test]
fn a_reference_no_longer_named_is_detached_by_an_edit_or_a_rewind() {
    use stacked::refs::{Attached, To};
    use stacked::thread::{Role, all_turns, append_turn};
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    // r1 named by your reply, r2 attached for what it says; the agent names
    // its r3, and attaches r4 without naming it.
    Db::edit(&e.db, |db| {
        let n = db.find_note("s", &id)?.clone();
        let text = append_turn(&n.text, Role::Agent, "As @r3 shows.\n\n```refs\nr3 a.txt:2 @1\nr4 a.txt:3 @1\n```", Some("answer"));
        let lines = |n: usize, l: &str| Attached { n, to: To::Lines(l.into()) };
        db.set_note(stacked::db::Note {
            text: append_turn(&text, Role::You, "And @r1?", None),
            attached: vec![lines(1, "a.txt:1 @1"), lines(2, "a.txt:5 @1")],
            ..n
        })
    })
    .unwrap();
    // Your reply written again without naming r1: it is detached, said so.
    assert_eq!(e.ok(&["reply", "s", &id, "--edit", "And", "then?"]), format!("{id}\tr1 is no longer named: it is detached\n"));
    let listed = e.ok(&["comments", "s"]);
    assert!(!listed.contains("r1 ⇢") && listed.contains("\n  r2 ⇢ a.txt:5 · as patch 1 leaves it\n"), "{listed}");
    // The agent's reply written again keeps its references, but r3, which
    // it no longer names; r4, which it never named, stays.
    assert_eq!(e.ok(&["reply", "s", &id, "--edit", "--turn", "2", "Seen."]), format!("{id}\tr3 is no longer named: it is detached\n"));
    let n = Db::load(&e.db).unwrap().find_note("s", &id).unwrap().clone();
    assert_eq!(all_turns(&n.text)[1].body, "Seen.\n\n```refs\nr4 a.txt:3 @1\n```");
    // Rewound from the comment itself: r4 goes with the agent's reply, r2
    // stays, never named; no number is given again.
    assert_eq!(e.ok(&["reply", "s", &id, "--rewind", "1", "Why", "two?"]), "");
    let n = Db::load(&e.db).unwrap().find_note("s", &id).unwrap().clone();
    assert_eq!((n.attached.iter().map(Attached::name).collect::<Vec<_>>(), stacked::refs::next_of(&n)), (vec!["r2".to_string()], 5));
}

#[test]
fn a_trim_writes_a_new_database_from_a_rev_on() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["move", "s", "3", "1"]);
    let id = comment(&e, &["--patch", "2", "--line", "a.txt:+2", "Why", "2?"]);
    e.ok(&["reply", "s", &id, "Because."]);
    comment(&e, &["Rebase", "it."]);
    let answer = fake_claude(&e, "claude-answer.sh", "", r#"{"replies": {"c1": "Seen."}}"#);
    let (fine, _, err) = send(&e, &answer, &[&id, "--answer-only"], &[]);
    assert!(fine, "{err}");
    let d = Db::load(&e.db).unwrap();
    let p = d.patch(&d.stack("s").unwrap().patches[1]).clone();
    drop(d);
    let keys = stacked::rows::patch_rows(&p, &[], 80, &Default::default())
        .into_iter()
        .filter(|r| r.text == "+2")
        .filter_map(|r| r.key)
        .collect::<Vec<_>>();
    Db::edit(&e.db, |db| {
        db.set_view("s", "to read")?;
        let n = db.find_note("s", &id)?.clone();
        db.set_collapsed(&n.change, &n.id, true)?;
        // What the agent brought back, waiting for review, half marked.
        let from = db.head("s").unwrap().0;
        db.add_run(stacked::db::Run {
            id: "run-held".into(),
            what: "answer".into(),
            stack: "s".into(),
            state: RunState::Done,
            ask: Some(serde_json::json!({"way": "answer", "scope": "stack", "from": from, "notes": []})),
            held: Some(stacked::held::Held {
                at: 1,
                replies: vec![stacked::held::Reply {
                    change: n.change.clone(),
                    id: n.id.clone(),
                    text: "Kept.".into(),
                    tag: "answer".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        })?;
        db.set_review(
            "run-held",
            stacked::held::Review {
                marks: [("reply 0".to_string(), false)].into(),
                ..Default::default()
            },
        )?;
        db.mark_read(&p.change, keys.clone()).map(|_| ())
    })
    .unwrap();
    // The cut: what there is now is the start; then a drop and an undo.
    let cut = Db::load(&e.db).unwrap().last_rev() + 1;
    e.ok(&["drop", "s", "1"]);
    e.ok(&["undo", "s"]);
    let before = std::fs::read(&e.db).unwrap();
    let new = e.root.join("new.jsonl");
    let out = e.ok(&["trim", new.to_str().unwrap(), "--from", &format!("@{cut}")]);
    assert!(out.contains(&format!("wrote {}", new.display())) && out.contains(&format!("from @{cut} on")), "{out}");
    assert_eq!(std::fs::read(&e.db).unwrap(), before, "the old file is only read");
    let on = |args: &[&str]| {
        let (fine, out, err) = stacked_in(&e.src, &new, args, "", &[]);
        assert!(fine, "stacked {args:?} on the new file: {err}");
        out
    };
    // The same stack, the same comments, filter and read marks, the comment
    // collapsed still.
    assert_eq!(on(&["show", "s"]), e.ok(&["show", "s"]));
    assert_eq!(on(&["comments", "s"]), e.ok(&["comments", "s"]));
    let made = Db::load(&new).unwrap();
    assert_eq!(made.view("s"), Some("to read"));
    assert_eq!(made.read_keys(&p.change), keys.into_iter().collect());
    let n = made.find_note("s", &id).unwrap();
    assert!(made.collapsed(&n.change, &n.id));
    let queue = stacked::held::queue(&made, "s");
    assert_eq!(queue.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["run-held"], "what waits for review, kept");
    assert_eq!(made.review("run-held").unwrap().marks.get("reply 0"), Some(&false), "its marks too");
    assert!(made.warnings.is_empty() && on(&["check"]).contains("s: applies"));
    // The history kept: its start, then the drop and the undo; the ask that
    // landed before is left out.
    let log = on(&["log", "s"]);
    let lines: Vec<&str> = log.lines().collect();
    assert!(lines[0].contains("(the start of a trimmed history)"), "{log}");
    assert!(log.contains("drop ") && log.contains("undo"), "{log}");
    // The ask had landed before the cut, and was reviewed: only what waits
    // for review is kept.
    let runs = on(&["runs"]);
    assert!(runs.lines().count() == 1 && runs.starts_with("run-held"), "{runs}");
    // It undoes as the old one does: back before the drop is its start.
    let (fine, _, err) = stacked_in(&e.src, &new, &["undo", "s"], "", &[]);
    assert!(!fine && err.contains("nothing to undo"), "{err}");
    assert!(on(&["redo", "s"]).is_empty());
    assert_eq!(on(&["show", "s"]).lines().count(), 2, "redo is the drop");
    // Only what there is now; never over a file.
    let now = e.root.join("now.jsonl");
    e.ok(&["trim", now.to_str().unwrap()]);
    let made = Db::load(&now).unwrap();
    assert_eq!(made.tree("s").len(), 1, "one state of the stack");
    assert_eq!(made.notes(&p.change).len(), 1);
    let (fine, _, err) = e.run(&["trim", now.to_str().unwrap()]);
    assert!(!fine && err.contains("is there already"), "{err}");
    let (fine, _, err) = e.run(&["trim", e.root.join("x.jsonl").to_str().unwrap(), "--from", "@9999"]);
    assert!(!fine && err.contains("is not a rev"), "{err}");
}

#[test]
fn the_agent_answers_on_the_history() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["move", "s", "3", "1"]);
    let log = e.ok(&["log", "s"]);
    let moved = log.lines().find(|l| l.contains("\tmove ")).unwrap();
    let rev: usize = moved.split('\t').next().unwrap().trim_start_matches('@').parse().unwrap();
    let id = comment(&e, &["--about", &format!("@{rev}"), "Why", "was", "it", "moved?"]);
    assert!(e.ok(&["comments", "s"]).contains(&format!("{id}\tthe history, @{rev} ↕1 · move")));
    // Asked about to answer only: it changes nothing.
    let (fine, _, err) = e.run(&["send", "s", &id]);
    assert!(!fine && err.contains("asked about to answer only"), "{err}");
    let (fine, _, err) = e.run(&["comment", "s", "--about", "@999", "What?"]);
    assert!(!fine && err.contains("is not in the history of s"), "{err}");
    // Told the history and what the move did, and given the commands that
    // read the history, the agent answers, and opens a comment on a line.
    let told = e.root.join("told");
    let agent = fake_claude(
        &e,
        "agent-history.sh",
        &format!("printf '%s\\n' \"$@\" > {}", told.display()),
        r#"{"replies": {"c1": "To put c first."}, "comments": [{"patch": 1, "file": "a.txt", "side": "+", "line": "9", "text": "This is the one moved."}]}"#,
    );
    let (fine, out, err) = send(&e, &agent, &[&id, "--answer-only"], &[]);
    assert!(fine, "{err}");
    assert!(out.contains("agent(high): To put c first.") && out.contains("opened 1 comment"), "{out}");
    let told = std::fs::read_to_string(&told).unwrap();
    assert!(told.contains("on the stack's history, about @") && told.contains("The stack's history, oldest first"), "{told}");
    assert!(told.contains(&format!("What @{rev} was:")) && told.contains("↕ Bug 3 - café"), "{told}");
    assert!(told.contains("export s --rev @N") && told.contains("Bash(") && told.contains(" log *)"), "{told}");
    assert!(!told.contains("acceptEdits"), "it changes nothing");
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("a.txt: 9") && c.contains("agent(high): This is the one moved."), "{c}");
}

#[test]
fn an_ask_being_started_is_not_taken_for_one_left_behind() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let from = Db::load(&e.db).unwrap().head("s").unwrap().0;
    // Started long ago, as a worktree taking minutes to make.
    let run = |id: &str, starter: u32| {
        Record::Run(stacked::db::Run {
            id: id.into(),
            what: "answer".into(),
            stack: "s".into(),
            state: RunState::Started,
            ask: Some(serde_json::json!({"way": "answer", "scope": "stack", "from": from, "notes": []})),
            starter: Some(starter),
            time: 1,
            ..Default::default()
        })
    };
    // A process that is gone, for a starter that is.
    let mut gone = Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    Db::append(&e.db, &[run("slow", std::process::id()), run("lost", dead)]).unwrap();
    // Still being started while who started it lives.
    let db = Db::load(&e.db).unwrap();
    assert_eq!(stacked::ask::land_orphans(&e.db, &db), 1);
    let db = Db::load(&e.db).unwrap();
    assert_eq!(db.run("slow").unwrap().state, RunState::Started, "being started");
    assert!(stacked::ask::running(db.run("slow").unwrap()));
    assert_eq!(db.run("lost").unwrap().state, RunState::Failed, "who started it is gone");
}

/// The process group of an ask's wrapper `0`, its agent deaf to SIGTERM:
/// killed should the test fail, as nothing else would stop it.
struct Deaf(u32);

impl Drop for Deaf {
    fn drop(&mut self) {
        if std::thread::panicking() {
            unsafe { libc::kill(-(self.0 as i32), libc::SIGKILL) };
        }
    }
}

#[test]
fn an_agent_stopped_lands_its_ask_at_once_killed_if_it_will_not_exit() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "two?"]);
    // A claude deaf to SIGTERM, at work until the gate is there, that says
    // where it runs.
    let (gate, at) = (e.root.join("gate"), e.root.join("agent.pid"));
    let body = format!("trap '' TERM\necho $$ > {}\nuntil test -e {}; do sleep 0.1; done", at.display(), gate.display());
    let claude = fake_claude(&e, "claude-deaf.sh", &body, r#"{"replies": {"c1": "Too late."}}"#);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only"], "", &[("STACKED_CLAUDE", claude.as_path())]);
    assert!(fine, "{out}{err}");
    let agent = eventually(|| std::fs::read_to_string(&at).ok().and_then(|p| p.trim().parse::<u32>().ok()), || "the agent never worked".into());
    let (run, pid) = Db::load(&e.db).unwrap().runs().iter().find(|r| r.state == RunState::Started).map(|r| (r.id.clone(), r.pid)).unwrap();
    let _deaf = Deaf(pid.expect("its wrapper runs"));
    // The shell offers it to stop, as it runs.
    let offered = e.ok(&["__complete", "--", "stop", ""]);
    assert_eq!(offered, format!("{run}:answer · running\n"));
    // Stopped, by the start of its id: killed, as it will not exit, and its
    // ask landed at once, as failed, said so on its comment.
    let start = &run[..run.rfind('-').unwrap()];
    let out = e.ok(&["stop", start]);
    assert!(out.starts_with(&run) && out.trim_end().ends_with("\tfailed\tyou stopped the agent"), "{out}");
    assert!(!still_there(agent), "killed");
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("ask failed: you stopped the agent"), "{c}");
    assert!(e.ok(&["transcript", &run]).contains("■ you stopped the agent"));
    // Nothing to stop any more.
    let (fine, _, err) = e.run(&["stop", &run]);
    assert!(!fine && err.contains(&format!("{run} is not running: its ask landed, failed: you stopped the agent")), "{err}");
    assert_eq!(e.ok(&["__complete", "--", "stop", ""]), "");
    let (fine, _, err) = e.run(&["stop", "nothing"]);
    assert!(!fine && err.contains("no run nothing: `stacked runs` lists them"), "{err}");
    std::fs::write(&gate, "").unwrap();
}

#[test]
fn the_settings_are_a_file_in_home() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let cfg = e.root.join("stacked.toml");
    std::fs::write(
        &cfg,
        format!("file = \"{}\"\neditor = \"vim\"\n[view]\nlayout = \"sideways\"\n", e.db.display()),
    )
    .unwrap();
    // `config` prints every setting, the file's and the defaults, and what
    // is wrong; what it prints reads back.
    let home = e.root.join("home");
    let run = |args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_stacked"));
        kept_in(&mut cmd, &home).current_dir(&e.src).args(args).env("STACKED_CONFIG", &cfg).env_remove("STACKED_FILE");
        let out = cmd.output().unwrap();
        (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    let (out, _) = run(&["config"]);
    assert!(out.starts_with(&format!("# read from {}", cfg.display())), "{out}");
    assert!(out.contains("# wrong: ") && out.contains("view.layout: \"sideways\""), "{out}");
    assert!(out.contains("editor = \"vim\"") && out.contains("layout = \"unified\""), "{out}");
    assert!(out.contains("menu = [\"space\"]") && out.contains("[agent]"), "{out}");
    let copy = e.root.join("copy.toml");
    std::fs::write(&copy, &out).unwrap();
    let again = kept_in(&mut Command::new(env!("CARGO_BIN_EXE_stacked")), &home).arg("config").env("STACKED_CONFIG", &copy).output().unwrap();
    assert!(!String::from_utf8(again.stdout).unwrap().contains("# wrong"), "what it prints is a good file");
    // The database the settings name is the one opened when none is; what
    // is wrong with them is said by every command.
    let (out, err) = run(&["stacks"]);
    assert!(out.starts_with("s\t3 patches"), "{out}");
    assert!(err.contains("view.layout"), "{err}");
}

/// The names under `dir`, each with what its file holds: all there is, to
/// see that nothing was moved, made or changed.
fn listing(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let name = p.strip_prefix(dir).unwrap().display().to_string();
            if e.file_type().unwrap().is_dir() {
                todo.push(p);
                out.push((name, "/".into()));
            } else {
                out.push((name, std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// A home folder of the test's, `someone` in `root`, as one of yours was
/// before stacked kept its things in ~/.stacked: its settings (`editor =
/// "hx"`) in ~/.stacked.toml, and StackED's state, `db` the database opened
/// last, in ~/Library/Application Support/StackED.
fn home_of_before(root: &Path, db: &Path) -> PathBuf {
    let home = root.join("someone");
    let stacked = home.join("Library/Application Support/StackED");
    std::fs::create_dir_all(&stacked).unwrap();
    std::fs::write(home.join(".stacked.toml"), "editor = \"hx\"\n").unwrap();
    std::fs::write(stacked.join("state.json"), serde_json::json!({"recent": [db]}).to_string()).unwrap();
    home
}

#[test]
fn a_run_of_stacked_as_the_tests_run_it_reaches_nothing_in_the_home() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // A home holding what a run of yours would read and move stands in for
    // that of whoever runs the tests.
    let home = home_of_before(&e.root, &e.db);
    let before = listing(&home);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["config"], "", &[("HOME", &home)]);
    assert!(fine, "{err}");
    // Where it reads its settings, as it says: the test's folder, beside
    // the database, holding the test's own (`HOME_SETTINGS`).
    let settings = e.root.join("home/config.toml");
    assert_eq!(out.lines().next(), Some(format!("# read from {}", settings.display()).as_str()), "{out}");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), HOME_SETTINGS);
    assert!(out.contains("\n# editor = ...") && !out.contains("editor = \"hx\""), "the home's settings not read: {out}");
    // Nothing of the home's read, moved, made or said.
    assert_eq!(listing(&home), before);
    assert!(!err.contains("moved") && !err.contains("no longer read"), "{err}");
    // Nor of the home of whoever runs the tests.
    let real = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    for theirs in [real.join(".stacked"), real.join(".stacked.toml"), real.join("Library/Application Support/StackED")] {
        assert!(!settings.starts_with(&theirs), "{}", theirs.display());
    }
}

#[test]
fn the_settings_are_in_stacked_s_folder_the_one_stacked_home_names_else_the_home_s() {
    let e = env();
    // A folder of the test's, named: made by the first setting written.
    let folder = e.root.join("fresh");
    let run = |args: &[&str]| stacked_in(&e.src, &e.db, args, "", &[("STACKED_HOME", &folder)]);
    assert!(!folder.exists());
    let (fine, _, err) = run(&["config", "editor", "hx"]);
    assert!(fine && err.contains(&format!("editor written in {}", folder.join("config.toml").display())), "{err}");
    assert_eq!(std::fs::read_to_string(folder.join("config.toml")).unwrap(), "editor = \"hx\"\n");
    assert_eq!(run(&["config", "editor"]).1, "hx\n");
    assert!(run(&["config"]).1.starts_with(&format!("# read from {}\n", folder.join("config.toml").display())));
    // Another folder, named: its own.
    let other = e.root.join("other");
    let (_, out, _) = stacked_in(&e.src, &e.db, &["config"], "", &[("STACKED_HOME", &other)]);
    assert!(out.starts_with(&format!("# none in {}\n", other.join("config.toml").display())), "{out}");
    // $STACKED_CONFIG names the file itself, over the folder's.
    let mine = e.root.join("mine.toml");
    std::fs::write(&mine, "editor = \"vim\"\n").unwrap();
    assert_eq!(stacked_in(&e.src, &e.db, &["config", "editor"], "", &[("STACKED_CONFIG", &mine)]).1, "vim\n");
    // With no override, ~/.stacked/config.toml: of a home of the test's.
    let home = e.root.join("someone");
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["config", "editor", "kak"], "", &as_yours(&home));
    let theirs = home.join(".stacked/config.toml");
    assert!(fine && err.contains(&format!("editor written in {}", theirs.display())), "{err}");
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "editor = \"kak\"\n");
}

#[test]
fn a_run_of_yours_moves_the_files_of_before_into_stacked_s_folder_once_and_says_so() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let home = home_of_before(&e.root, &e.db);
    let (old, new) = (home.join(".stacked.toml"), home.join(".stacked/config.toml"));
    let (old_state, new_state) = (home.join("Library/Application Support/StackED/state.json"), home.join(".stacked/state.json"));
    // The shell's completion, which stacked's function runs: nothing moved,
    // nothing said.
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["__complete", "--", ""], "", &as_yours(&home));
    assert!(fine && err.is_empty(), "{err}");
    assert!(old.exists() && old_state.exists() && !home.join(".stacked").exists());
    // A run of yours: each moved, as it was, and said.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["config", "editor"], "", &as_yours(&home));
    assert!(fine && out == "hx\n", "{out}{err}");
    assert!(err.contains(&format!("stacked: your settings were moved from {} to {}", old.display(), new.display())), "{err}");
    assert!(
        err.contains(&format!("stacked: StackED's list of the databases opened last was moved from {} to {}", old_state.display(), new_state.display())),
        "{err}"
    );
    assert!(!old.exists() && !old_state.exists());
    assert_eq!(stacked::state::recent(&new_state), std::slice::from_ref(&e.db), "the list as it was");
    // Once: the next run says nothing of it.
    let (_, out, err) = stacked_in(&e.src, &e.db, &["config"], "", &as_yours(&home));
    assert!(out.starts_with(&format!("# read from {}\n", new.display())), "{out}");
    assert!(!err.contains("moved") && !err.contains("no longer read"), "{err}");
    // One of before there again, as an older stacked would write it: the
    // folder's read, never written over; the other left as it is, and said
    // to be read no longer.
    std::fs::write(&old, "editor = \"vim\"\n").unwrap();
    let (_, out, err) = stacked_in(&e.src, &e.db, &["config", "editor"], "", &as_yours(&home));
    assert_eq!(out, "hx\n");
    assert!(err.contains(&format!("stacked: {} is no longer read, as stacked reads its settings from {}", old.display(), new.display())), "{err}");
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "editor = \"vim\"\n");
    assert_eq!(std::fs::read_to_string(&new).unwrap(), "editor = \"hx\"\n");
}

#[test]
fn mail_parsing() {
    assert_eq!(decode_words("=?UTF-8?q?caf=C3=A9_au_lait?="), "café au lait");
    assert_eq!(decode_words("=?UTF-8?q?a?= =?UTF-8?q?b?= c"), "ab c");
    assert_eq!(decode_words("=?utf-8?b?Y2Fmw6k=?="), "café");
    let d = parse_mail("Subject: [PATCH 1/2] Title\n\nBody\n---\n a | 1 +\n\ndiff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-- \n+x\n-- \n2.50.0\n\n").unwrap();
    assert_eq!(d.message, "Title\n\nBody");
    assert_eq!(d.diff, "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-- \n+x\n", "a removed `- ` line is not the signature");
    let d = parse_mail("Title only\n---\ndiff --git a/a b/a\n").unwrap();
    assert_eq!((d.message.as_str(), d.diff.as_str()), ("Title only", "diff --git a/a b/a\n"));
    assert!(parse_mbox("").unwrap().is_empty());
    assert!(parse_mail("\n\n---\ndiff --git a/a b/a\n").is_err());
}

#[cfg(feature = "moz-phab")]
#[test]
fn a_stack_is_imported_from_phabricator_by_its_revision() {
    use stacked::phab::{Revision, import, stack_stand_in};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    // The settings of no file: a developer's own must not change it.
    stacked::config::init(stacked::config::Config::default());
    let e = env();
    let base = git(&e.src, &["rev-parse", "base"]).trim().to_string();
    // Phabricator as the test has it: D21 on base, D22 on it, read from
    // either, or from its link; each read counted.
    let on = Arc::new(Mutex::new(base.clone()));
    let reads = Arc::new(AtomicUsize::new(0));
    let (at, read) = (on.clone(), reads.clone());
    stack_stand_in(Some(Box::new(move |rev, link| {
        read.fetch_add(1, Ordering::SeqCst);
        if !["D21", "D22"].contains(&rev) || link.is_some_and(|l| l != format!("https://phabricator.example.com/{rev}")) {
            return Err(format!("not as the test reads it: {rev} {link:?}"));
        }
        let trailer = |n: u32| format!("\n\nDifferential Revision: https://phabricator.example.com/D{n}");
        Ok(vec![
            Revision {
                id: "D21".into(),
                message: format!("Bug 1 - change two{}", trailer(21)),
                author: Some("Ann <ann@example.com>".into()),
                diff: "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,5 +1,5 @@\n one\n-two\n+2\n three\n four\n five\n".into(),
                base: Some(at.lock().unwrap().clone()),
            },
            Revision {
                id: "D22".into(),
                message: format!("Bug 2 - add b{}", trailer(22)),
                author: None,
                diff: new_file("b.txt", "new"),
                base: None,
            },
        ])
    })));
    // Its number alone: the stack of both, named as it, on the commit the
    // bottom one is on.
    assert_eq!(import(&e.db, &e.src, "22", None).unwrap(), "imported 2 patches from D21..D22 as D22");
    let d = Db::load(&e.db).unwrap();
    let s = d.stack("D22").unwrap();
    assert_eq!(s.base.as_deref(), Some(base.as_str()));
    let ps: Vec<stacked::db::Patch> = s.patches.iter().map(|id| d.patch(id).clone()).collect();
    assert_eq!(ps.iter().map(|p| p.title().to_string()).collect::<Vec<_>>(), ["Bug 1 - change two", "Bug 2 - add b"]);
    assert_eq!(ps[0].author.as_deref(), Some("Ann <ann@example.com>"));
    assert!(ps[1].message.ends_with("Differential Revision: https://phabricator.example.com/D22"), "{}", ps[1].message);
    drop(d);
    // Its link, as the browser gives it; the revision again, a name free.
    assert_eq!(import(&e.db, &e.src, "https://phabricator.example.com/D21#inline-5", None).unwrap(), "imported 2 patches from D21..D22 as D21");
    assert_eq!(import(&e.db, &e.src, "D22", None).unwrap(), "imported 2 patches from D21..D22 as D22-2");
    // Named: so; a name taken, or none can have, refused before Phabricator
    // is read.
    assert_eq!(import(&e.db, &e.src, "D22", Some("fix")).unwrap(), "imported 2 patches from D21..D22 as fix");
    let before = reads.load(Ordering::SeqCst);
    assert_eq!(import(&e.db, &e.src, "D22", Some("fix")).unwrap_err().0, "there is a stack fix already");
    assert_eq!(import(&e.db, &e.src, "D22", Some("a b")).unwrap_err().0, "\"a b\" cannot name a stack: no blank, no /, no - first");
    assert_eq!(reads.load(Ordering::SeqCst), before);
    // What names no revision, said.
    assert_eq!(import(&e.db, &e.src, "D2x", None).unwrap_err().0, "\"D2x\" names no revision: give D123, 123, or its link");
    // On a commit the repository does not have: said, nothing imported.
    *on.lock().unwrap() = "0123456789abcdef0123456789abcdef01234567".into();
    assert_eq!(
        import(&e.db, &e.src, "D22", None).unwrap_err().0,
        "D21 is on 0123456789ab, which this repository does not have: fetch it first"
    );
    assert_eq!(Db::load(&e.db).unwrap().names().len(), 4);
    stack_stand_in(None);
}

#[cfg(feature = "moz-phab")]
#[test]
fn moz_phab_says_what_it_cannot_do_before_reaching_phabricator() {
    let e = env();
    let (fine, _, err) = e.run(&["moz-phab", "import", "D2x"]);
    assert!(!fine && err.contains("\"D2x\" names no revision: give D123, 123, or its link"), "{err}");
    // Reviews from nowhere, as the settings say: neither pulled nor
    // imported.
    let settings = e.root.join("nowhere.toml");
    std::fs::write(&settings, "[reviews]\nsource = \"none\"\n").unwrap();
    let env = [("STACKED_CONFIG", settings.as_path())];
    for (args, said) in [(&["moz-phab", "import", "D21"][..], "revisions come from nowhere"), (&["moz-phab", "pull", "s"][..], "reviews come from nowhere")] {
        let (fine, _, err) = stacked_in(&e.src, &e.db, args, "", &env);
        assert!(!fine && err.contains(&format!("{said}: reviews.source is \"none\" in {}", settings.display())), "{args:?}: {err}");
    }
}

#[test]
fn phabricator_s_commands_are_there_when_built_with_it() {
    let e = env();
    let listed = |help: &str, cmd: &str| help.lines().any(|l| l.trim_start().starts_with(&format!("{cmd} ")));
    let help = e.ok(&["--help"]);
    assert_eq!(listed(&help, "moz-phab"), cfg!(feature = "moz-phab"), "{help}");
    assert!(!listed(&help, "pull"), "under moz-phab, if anywhere: {help}");
    #[cfg(feature = "moz-phab")]
    {
        let help = e.ok(&["moz-phab", "--help"]);
        assert!(listed(&help, "import") && listed(&help, "pull"), "{help}");
    }
}

#[test]
fn the_shell_completes_the_commands_and_what_the_database_holds() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let offered = |words: &[&str]| e.ok(&[&["__complete", "--"][..], words].concat());
    // The commands, each with what it does; not those hidden.
    let commands = offered(&[""]);
    assert!(commands.lines().any(|l| l.starts_with("show:")), "{commands}");
    assert!(commands.lines().any(|l| l.starts_with("completions:")), "{commands}");
    assert!(!commands.contains("__complete"), "{commands}");
    // The stacks, and a stack's patches with their titles.
    assert_eq!(offered(&["show", ""]), "s:3 patches\n");
    let patches = offered(&["show", "s", ""]);
    assert!(patches.starts_with("1:Bug 1 - change two\n2:Bug 2 - add b\n3:Bug 3 - café"), "{patches}");
    // A command's options; what an option lists; a file's, files.
    let options = offered(&["comments", "s", "--"]);
    assert!(options.lines().any(|l| l == "--show") && options.contains("--file:The database\n"), "{options}");
    assert!(offered(&["comments", "s", "--show", ""]).lines().any(|l| l.starts_with("to-send:")));
    assert_eq!(offered(&["-f", ""]), ":files\n");
    // A comment, to reply to: its id, and what it says.
    let id = comment(&e, &["Why", "this?"]);
    assert!(offered(&["reply", "s", ""]).lines().any(|l| l == format!("{id}:you: Why this?")), "{}", offered(&["reply", "s", ""]));
    // The function zsh runs, run as completion runs it, on the words up to
    // the cursor's: `_describe` given what to offer, `_files` asked for
    // files. No rc file of the user's is read (`-f`).
    let function = e.root.join("_stacked");
    std::fs::write(&function, e.ok(&["completions", "zsh"])).unwrap();
    let bin = Path::new(env!("CARGO_BIN_EXE_stacked")).parent().unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let zsh = |words: &str| {
        let script = format!(
            "compdef() {{ :; }}; _describe() {{ print -rl -- \"${{(@P)2}}\"; }}; _files() {{ print files; }}; source \"$1\"; words=({words}); CURRENT=${{#words}}; _stacked"
        );
        // The stacked it runs keeps its own things in the test's folder.
        let out = kept_in(&mut Command::new("zsh"), &e.root.join("home"))
            .args(["-fc", &script, "zsh"])
            .arg(&function)
            .current_dir(&e.src)
            .env("PATH", &path)
            .env("STACKED_FILE", &e.db)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    if Command::new("zsh").arg("--version").output().is_err() {
        return;
    }
    assert!(zsh("stacked show s ''").starts_with("1:Bug 1 - change two\n2:Bug 2 - add b\n"), "{}", zsh("stacked show s ''"));
    assert_eq!(zsh("stacked -f ''"), "files\n");
}

#[test]
fn a_word_that_is_no_command_is_not_taken_for_a_database() {
    let e = env();
    // Run with no -f: the word first.
    let first = |args: &[&str]| {
        let out = kept_in(&mut Command::new(env!("CARGO_BIN_EXE_stacked")), &e.root.join("home"))
            .args(args)
            .current_dir(&e.src)
            .env_remove("STACKED_FILE")
            .output()
            .unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())
    };
    // A command mistyped: said, with the one near it; no database made.
    let (ok, _, err) = first(&["shwo"]);
    assert!(!ok && err.contains("unrecognized subcommand 'shwo'") && err.contains("'show'"), "{err}");
    assert!(!e.src.join("shwo").exists());
    // A database, by its path, or a file there: opened (`--help` says how,
    // with no editor started).
    let opens = |word: &str| {
        let (ok, out, err) = first(&[word, "--help"]);
        assert!(ok && out.starts_with("Open the editor"), "{word}: {out}{err}");
    };
    opens("bug123.jsonl");
    opens("./bug123");
    std::fs::write(e.src.join("mine"), "").unwrap();
    opens("mine");
}

/// A denial of claude's: a call of `tool`, given `input`, as its result
/// lists it (`permission_denials`).
fn denial(tool: &str, input: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"tool_name": tool, "tool_input": input})
}

/// A stand-in for claude, `name`: as it starts, it adds to `tools` the
/// tools it was let use (`--allowedTools`), a line for each ask. Asked to
/// spell a line out, it changes patch 1 as `chain_claude`'s does, and
/// answers `Done.`; asked anything else, its result says it was refused
/// `denials`, and it answers `answer`, then exits with `exit`.
fn refusing_claude(e: &Env, name: &str, tools: &Path, denials: &[serde_json::Value], answer: &serde_json::Value, exit: i32) -> PathBuf {
    let result = serde_json::json!({"type": "result", "is_error": exit != 0, "result": answer.to_string(), "permission_denials": denials});
    let done = serde_json::json!({"type": "result", "result": r#"{"replies": {"c1": "Done."}}"#});
    e.script(
        name,
        &format!(
            "prev=; for a in \"$@\"; do if test \"$prev\" = --allowedTools; then printf '%s\\n' \"$a\" >> '{tools}'; fi; prev=$a; done\n\
             echo '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\n\
             case \"$2\" in\n\
             *'Spell it'*)\n\
             {deux}\n\
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a\n\
             cat <<'EOF'\n{done}\nEOF\n\
             ;;\n\
             *)\n\
             cat <<'EOF'\n{result}\nEOF\n\
             exit {exit} ;;\n\
             esac\n",
            tools = tools.display(),
            deux = sed_i("s/^2$/deux/", "a.txt")
        ),
    )
}

/// Tool `k` the agent could not use, of the work run `run` holds, marked `a`
/// in its review, as the editor marks it, and saved.
fn mark_tool(e: &Env, run: &str, k: usize, a: stacked::refusals::Allow) {
    Db::edit(&e.db, |d| {
        let mut marks = d.review(run).map(|v| v.marks.clone()).unwrap_or_default();
        stacked::refusals::set_state(&mut marks, k, a);
        stacked::held::save(d, run, &marks)
    })
    .unwrap();
}

/// The tools the stand-in was let use by the ask that started last, as
/// `refusing_claude` wrote them in `tools`.
fn last_given(tools: &Path) -> String {
    std::fs::read_to_string(tools).unwrap().lines().last().unwrap().to_string()
}

#[test]
fn the_calls_the_agent_was_refused_land_as_items_one_a_rule_even_from_a_failed_run() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let tools = e.root.join("tools");
    let denials = [
        denial("Bash", serde_json::json!({"command": "searchfox-cli --define Foo"})),
        denial("Bash", serde_json::json!({"command": "searchfox-cli --define Bar"})),
        denial("Bash", serde_json::json!({"command": "./mach try fuzzy --no-push -q x"})),
        denial("mcp__moz__get_bug", serde_json::json!({"id": 1})),
        denial("Edit", serde_json::json!({"file_path": "a.txt"})),
    ];
    let answer = serde_json::json!({"replies": {"c1": "Read otherwise."}, "refused": [{"tool": "Bash: searchfox-cli --define Foo", "why": "To find Foo; I used rg instead."}]});
    let claude = refusing_claude(&e, "claude-refused.sh", &tools, &denials, &answer, 0);
    let id = comment(&e, &["Where", "is", "Foo?"]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{err}");
    // In the work's review: one item a rule, in the order refused, each
    // with the first call it tried; Edit noted, as an answer only reads.
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.lines().next().unwrap().ends_with("\t1 reply, 4 tools the agent could not use\tasked at high"), "{landed}");
    for line in [
        "  tool\tBash(searchfox-cli *)\trefused\tsearchfox-cli --define Foo",
        "  tool\tBash(./mach try fuzzy *)\trefused\t./mach try fuzzy --no-push -q x",
        "  tool\tmcp__moz__get_bug\trefused\tmcp__moz__get_bug: {\"id\": 1}",
        "  tool\tEdit\tnoted\tEdit: a.txt",
    ] {
        assert!(landed.lines().any(|l| l == line), "{line:?} in:\n{landed}");
    }
    let run = landed.split('\t').next().unwrap().to_string();
    let d = Db::load(&e.db).unwrap();
    let h = d.run(&run).unwrap().held.clone().unwrap();
    assert_eq!(h.tools[0].calls, ["searchfox-cli --define Foo", "searchfox-cli --define Bar"]);
    assert_eq!((h.tools[0].why.as_str(), h.tools[1].why.as_str()), ("To find Foo; I used rg instead.", ""), "unexplained, an item all the same");
    assert_eq!(h.tools[1].rules, ["Bash(./mach try fuzzy *)", "Bash(./mach try *)", "Bash(./mach *)"]);
    assert_eq!(h.tools[2].rules, ["mcp__moz__get_bug", "mcp__moz__*"]);
    assert_eq!(h.tools[3].noted.as_deref(), Some("it changes things, and answer-only asks only read"));
    // Told not to stop at a refusal, but to work around it, and to say why
    // it needed each tool refused.
    let prompt = stacked::ask::info(d.run(&run).unwrap()).unwrap().prompt;
    drop(d);
    for said in [
        "Do not stop there: work around the refusal with what you may use, and finish what you were asked as well as you can.",
        "And list each tool you were refused in your answer, as \"refused\": [{\"tool\": the tool as you called it",
        "\"why\": why you needed it, and how you worked around it}]: the user reviews each, and may let you use it.",
    ] {
        assert!(prompt.contains(said), "{said:?} in {prompt}");
    }
    e.ok(&["review", "s", "--reject-all"]);
    // A run that failed, and was refused tools: it lands for review all the
    // same, failed, to allow them and send it back.
    let failing = refusing_claude(&e, "claude-failing.sh", &tools, &denials[..1], &serde_json::json!({"replies": {}}), 1);
    let id = comment(&e, &["And", "Bar?"]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &[("STACKED_CLAUDE", &failing)]);
    assert!(!fine && err.contains("1 of them failed"), "{err}");
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.lines().next().unwrap().ends_with("\t1 tool the agent could not use\tasked at high"), "{landed}");
    let d = Db::load(&e.db).unwrap();
    let r = stacked::held::queue(&d, "s")[0].clone();
    assert_eq!(r.state, RunState::Failed);
    assert!(r.summary.starts_with("the agent failed (1)"), "{}", r.summary);
    assert!(r.summary.contains("; the agent was not allowed to use Bash(searchfox-cli *): the review of its work lists each, to allow for this work or from now on"), "{}", r.summary);
}

#[test]
fn a_tool_allowed_from_now_on_is_saved_in_the_checkout_s_own_list_for_its_kind_alone() {
    use stacked::refusals::Allow;
    let e = env();
    e.ok(&["import", "s", "base"]);
    let tools = e.root.join("tools");
    let local = e.src.join(".stacked/local.toml");
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    let searchfox = [denial("Bash", serde_json::json!({"command": "searchfox-cli --define Foo"}))];
    let claude = refusing_claude(&e, "claude-refused.sh", &tools, &searchfox, &serde_json::json!({"replies": {"c1": "Seen."}}), 0);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let ask = |args: &[&str]| -> String {
        let id = comment(&e, &["Where", "is", "Foo?"]);
        let mut all = vec!["send", "s", id.as_str()];
        all.extend(args);
        all.push("--wait");
        let (fine, _, err) = stacked_in(&e.src, &e.db, &all, "", &env);
        assert!(fine, "{args:?}: {err}");
        landed_ids(&e).pop().unwrap()
    };
    // Closed, accepted: what its review allowed from now on is saved in the
    // checkout's own settings, in answers' list of their own; neither the
    // settings' file nor any file of the repository written.
    let run = ask(&["--answer-only"]);
    mark_tool(&e, &run, 0, Allow::Always);
    let (fine, out, err) = e.run(&["review", "s", &run, "--accept-all"]);
    let saved = format!("; saved for good in {}, for answer-only asks: `Bash(searchfox-cli *)`", local.display());
    assert!(fine && out.contains(&saved), "{out}{err}");
    assert_eq!(read(&local), "[agent]\nanswer_accepted = [\"Bash(searchfox-cli *)\"]\n");
    assert_eq!(read(&e.root.join("home/config.toml")), HOME_SETTINGS);
    assert_eq!(git(&e.src, &["status", "--porcelain"]), "");
    // The next answer is given it, after its own list; a change is not.
    let run = ask(&["--answer-only"]);
    assert!(last_given(&tools).ends_with(",Bash(curl *),Bash(jq *),Bash(searchfox-cli *)"), "{}", last_given(&tools));
    e.ok(&["review", "s", &run, "--reject-all"]);
    let run = ask(&[]);
    assert!(!last_given(&tools).contains("searchfox"), "{}", last_given(&tools));
    // Rejected: saved all the same, for changes, in their own list.
    mark_tool(&e, &run, 0, Allow::Always);
    let (fine, out, err) = e.run(&["review", "s", &run, "--reject-all"]);
    assert!(fine && out.contains(&format!("; saved for good in {}, for the asks that change the stack: `Bash(searchfox-cli *)`", local.display())), "{out}{err}");
    assert_eq!(read(&local), "[agent]\nanswer_accepted = [\"Bash(searchfox-cli *)\"]\nchange_accepted = [\"Bash(searchfox-cli *)\"]\n");
    // Sent back, nothing written but the tool allowed: saved first, for
    // builds and tests in their own list, and the next round given it.
    let web = [denial("WebFetch", serde_json::json!({"url": "https://example.com/a"}))];
    let fetching = refusing_claude(&e, "claude-fetching.sh", &tools, &web, &serde_json::json!({"replies": {"c1": "Built."}}), 0);
    let id = comment(&e, &["Build", "it."]);
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--build-and-test", "--wait"], "", &[("STACKED_CLAUDE", &fetching)]);
    assert!(fine, "{err}");
    let run = landed_ids(&e).pop().unwrap();
    assert!(!last_given(&tools).contains("WebFetch"), "{}", last_given(&tools));
    mark_tool(&e, &run, 0, Allow::Always);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", &run, "--send-back"], "", &[("STACKED_CLAUDE", &fetching)]);
    assert!(fine && out.contains(&format!("; saved for good in {}, for builds and tests: `WebFetch`", local.display())), "{out}{err}");
    assert!(read(&local).ends_with("\ntest_accepted = [\"WebFetch\"]\n"), "{}", read(&local));
    landed(&e, 5);
    assert!(last_given(&tools).ends_with(",WebFetch"), "{}", last_given(&tools));
    e.ok(&["review", "s", "--reject-all"]);
    // Where git tracks the checkout's own settings, nothing is saved there,
    // and the work sent back is allowed it for this work instead, as said.
    git(&e.src, &["add", "-f", ".stacked/local.toml"]);
    let run = ask(&["--answer-only"]);
    mark_tool(&e, &run, 0, Allow::Always);
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["review", "s", &run, "--send-back"], "", &env);
    let not = format!("; `Bash(searchfox-cli *)` could not be saved: {}: git tracks it", local.display());
    assert!(fine && out.contains(&not) && out.contains("; allowed for this work instead"), "{out}{err}");
    assert!(out.contains("; allowed for this work: Bash(searchfox-cli *)"), "{out}");
    landed(&e, 7);
    assert!(last_given(&tools).ends_with(",Bash(searchfox-cli *)"), "{}", last_given(&tools));
    git(&e.src, &["rm", "-q", "--cached", ".stacked/local.toml"]);
}

#[test]
fn a_tool_allowed_for_this_work_is_given_every_round_of_it_until_its_review_closes() {
    use stacked::refusals::Allow;
    let e = env();
    e.ok(&["import", "s", "base"]);
    let tools = e.root.join("tools");
    let searchfox = [denial("Bash", serde_json::json!({"command": "searchfox-cli --define Foo"}))];
    let claude = refusing_claude(&e, "claude-refused.sh", &tools, &searchfox, &serde_json::json!({"replies": {"c1": "Seen."}}), 0);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let run = |args: &[&str]| {
        let (fine, out, err) = stacked_in(&e.src, &e.db, args, "", &env);
        assert!(fine, "{args:?}: {out}{err}");
        out
    };
    let rule = "Bash(searchfox-cli *)";
    let why = comment(&e, &["--patch", "2", "Why", "b?"]);
    run(&["send", "s", &why, "--answer-only", "--wait"]);
    let first = landed_ids(&e).pop().unwrap();
    assert!(!last_given(&tools).contains("searchfox"));
    // Allowed for this work, and sent back: the next round given it, its
    // stream and the status saying so, the comment told; nothing saved.
    mark_tool(&e, &first, 0, Allow::Work);
    let out = run(&["review", "s", "--send-back"]);
    assert!(out.contains(&format!("; allowed for this work: {rule}")), "{out}");
    landed(&e, 2);
    assert!(last_given(&tools).ends_with(&format!(",{rule}")), "{}", last_given(&tools));
    let second = landed_ids(&e).pop().unwrap();
    assert!(e.ok(&["transcript", &second]).starts_with(&format!("✓ allowed for this work: {rule}\n")), "{}", e.ok(&["transcript", &second]));
    assert!(e.ok(&["comments", "s"]).contains(&format!("- `{rule}`: you may use it now, for this work.")), "{}", e.ok(&["comments", "s"]));
    assert!(!e.src.join(".stacked/local.toml").exists(), "never saved");
    // Sent back again, for what you wrote: given it still.
    run(&["reply", "s", &why, "--answer", "And", "c?"]);
    run(&["review", "s", "--send-back"]);
    landed(&e, 3);
    assert!(last_given(&tools).ends_with(&format!(",{rule}")), "{}", last_given(&tools));
    // A change accepted meanwhile, chained: the answer asked again on the
    // stack it makes, by stacked, given it still.
    let spell = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    run(&["send", "s", &spell, "--wait"]);
    let change = landed_ids(&e).pop().unwrap();
    let out = run(&["review", "s", &change, "--accept-all"]);
    assert!(out.contains("; 1 answer-only ask asked again"), "{out}");
    landed(&e, 5);
    assert!(last_given(&tools).ends_with(&format!(",{rule}")), "{}", last_given(&tools));
    let again = landed_ids(&e).pop().unwrap();
    assert!(e.ok(&["transcript", &again]).starts_with(&format!("✓ allowed for this work: {rule}\n")), "{}", e.ok(&["transcript", &again]));
    // Its review closed: no other ask is given it.
    run(&["review", "s", "--reject-all"]);
    let other = comment(&e, &["And", "this?"]);
    run(&["send", "s", &other, "--answer-only", "--wait"]);
    assert!(!last_given(&tools).contains("searchfox"), "{}", last_given(&tools));
}

/// A line of the setup asked line by line, of a rule the reviews allowed
/// from now on: its box `mark` in the first column, then `text`, as this
/// stacked draws it (what the row says from column 21, or 30 with the
/// `try-push` feature).
fn accepted_line(mark: &str, text: &str) -> String {
    let at = if cfg!(feature = "try-push") { 30 } else { 21 };
    format!("    {mark}{}{text}", " ".repeat(at - 7))
}

#[test]
fn the_setup_lists_what_reviews_allowed_from_now_on_and_takes_out_what_you_untick() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let local = e.src.join(".stacked/local.toml");
    e.ok(&["config", "--local", "agent.answer_accepted", "Bash(searchfox-cli *)", "WebFetch"]);
    e.ok(&["config", "--local", "agent.test_accepted", "Bash(./mach build *)"]);
    let init = serde_json::json!({"type": "system", "subtype": "init", "model": "fake", "tools": ["Read"], "mcp_servers": []});
    let agent = e.script("claude-init.sh", &format!("echo '{init}'\nsleep 30\n"));
    let env = [("STACKED_CLAUDE", agent.as_path())];
    // Listed, a line each, under where they are; kept as they are.
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["setup", "agent", "s"], "", &env);
    assert!(fine, "{err}");
    for line in [
        format!(" Allowed from now on in reviews of the agent's work, in {}: untick a rule to take it out", local.display()),
        accepted_line("[x]", "Bash(searchfox-cli *)   for answer-only asks"),
        accepted_line("[x]", "WebFetch   for answer-only asks"),
        accepted_line("[x]", "Bash(./mach build *)   for builds and tests"),
    ] {
        assert!(err.lines().any(|l| l == line), "{line:?} in:\n{err}");
    }
    assert!(!err.contains("taken out of"), "{err}");
    assert!(std::fs::read_to_string(&local).unwrap().contains("answer_accepted = [\"Bash(searchfox-cli *)\", \"WebFetch\"]"));
    // One by one: WebFetch taken out, the rest kept; written in the
    // checkout's own settings, with --global too.
    let input = "n\n\nn\n\n\n\n\n\n";
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["setup", "agent", "s", "--global"], input, &env);
    assert!(fine, "{err}");
    assert!(err.contains("  Keep WebFetch, allowed from now on in a review, for answer-only asks? [Y/n]"), "{err}");
    assert!(err.contains(&format!("stacked: taken out of what reviews allowed from now on, in {}: answer_accepted: Bash(searchfox-cli *)", local.display())), "{err}");
    let now = std::fs::read_to_string(&local).unwrap();
    assert!(now.contains("answer_accepted = [\"Bash(searchfox-cli *)\"]") && now.contains("test_accepted = [\"Bash(./mach build *)\"]"), "{now}");
    assert!(!std::fs::read_to_string(e.root.join("home/config.toml")).unwrap().contains("accepted"), "never in the settings' file");
}

#[test]
fn work_dropped_to_be_asked_again_saves_what_its_review_allowed_from_now_on() {
    use stacked::refusals::Allow;
    let e = env();
    e.ok(&["import", "s", "base"]);
    let (tools, local) = (e.root.join("tools"), e.src.join(".stacked/local.toml"));
    let searchfox = [denial("Bash", serde_json::json!({"command": "searchfox-cli --define Foo"}))];
    let claude = refusing_claude(&e, "claude-refused.sh", &tools, &searchfox, &serde_json::json!({"replies": {"c1": "Seen."}}), 0);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let run = |args: &[&str]| {
        let (fine, out, err) = stacked_in(&e.src, &e.db, args, "", &env);
        assert!(fine, "{args:?}: {out}{err}");
        out
    };
    // An answer's tool marked allowed from now on, its review saved, not
    // closed.
    let why = comment(&e, &["--patch", "2", "Why", "b?"]);
    run(&["send", "s", &why, "--answer-only", "--wait"]);
    let answer = landed_ids(&e).pop().unwrap();
    mark_tool(&e, &answer, 0, Allow::Always);
    assert!(!local.exists());
    // A change accepted meanwhile, chained: the answer dropped, to be asked
    // again, its review closed, and what it allowed from now on saved, as
    // its run says.
    let spell = comment(&e, &["--patch", "1", "Spell", "it", "out."]);
    run(&["send", "s", &spell, "--wait"]);
    let change = landed_ids(&e).pop().unwrap();
    run(&["review", "s", &change, "--accept-all"]);
    landed(&e, 3);
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "[agent]\nanswer_accepted = [\"Bash(searchfox-cli *)\"]\n");
    let d = Db::load(&e.db).unwrap();
    let summary = d.run(&answer).unwrap().summary.clone();
    assert!(summary.starts_with("dropped: an edit of the stack was accepted meanwhile; asked again; saved for good in "), "{summary}");
    drop(d);
    // The answer asked again is given it, from now on.
    assert!(last_given(&tools).ends_with(",Bash(curl *),Bash(jq *),Bash(searchfox-cli *)"), "{}", last_given(&tools));
}
