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

const TEN: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";

/// A repo with a base commit and three on top that apply in any order, the
/// last with a title that is not ASCII and long enough to be folded.
fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
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

fn stacked_in(
    dir: &Path,
    db: &Path,
    args: &[&str],
    stdin: &str,
    env: &[(&str, &Path)],
) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_stacked"));
    cmd.current_dir(dir)
        .arg("-f")
        .arg(db)
        .args(args)
        .env_remove("VISUAL")
        .env("EDITOR", "false")
        // No agent of the user's is ever asked: a test asks its stand-in.
        .env("STACKED_CLAUDE", "/usr/bin/false")
        .env("STACKED_CONFIG", "/nonexistent/stacked.toml")
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
        r#"case "$1" in
*.todo) grep -v '^#' "$1" | grep . | tail -r | sed '1s/^pick/reword/;$s/^pick/drop/' > "$1.new" && mv "$1.new" "$1" ;;
*.txt) printf 'Bug 3 - reworded\n\nNew body.\n' > "$1" ;;
esac
"#,
    );
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s"], "", &[("EDITOR", &editor)]);
    assert!(fine, "{err}");
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
    assert!(text.starts_with(&torn), "nothing is cut off:\n{text}");
    assert_eq!(rev, 4, "the torn line keeps its number");
    let db = Db::load(&path).unwrap();
    assert_eq!(db.stacks().len(), 2);
    assert!(matches!(db.rev(3), Some(Record::Broken)));
    // Garbage anywhere is passed over, and what follows still read.
    std::fs::write(&path, format!("{text}garbage\n{{\"t\":\"head\",\"name\":\"t\",\"rev\":99,\"op\":\"x\",\"time\":0}}\n")).unwrap();
    let db = Db::load(&path).unwrap();
    assert_eq!(db.stacks().len(), 2);
    assert_eq!(db.warnings.len(), 3, "{:?}", db.warnings);
    assert!(db.warnings[1].contains(":5:"), "{:?}", db.warnings);
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
    assert_eq!(reader.records, full.records);
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
        assert!(std::fs::read(&path).unwrap().starts_with(&before));
    }
    Db::edit(&path, |db| ops::move_patch(db, "s", 0, 2)).unwrap();
    Db::edit(&path, |db| db.undo("s")).unwrap();
    let db = Db::load(&path).unwrap();
    assert!(matches!(db.records[0], Record::Stacked { version: 2 }));
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
    let (fine, _, err) = stacked_in(&e.src, &e.db, &["edit", "s", "1"], "", &[("EDITOR", &editor)]);
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
  grep '^pick' "$1" | sort -k3 > "$1.new" && mv "$1.new" "$1"
else
  grep '^pick' "$1" | tail -r > "$1.new" && mv "$1.new" "$1"
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
    for _ in 0..200 {
        let db = Db::load(&e.db).unwrap();
        let runs = db.runs();
        if runs.iter().filter(|r| r.state != RunState::Started).count() >= n
            && runs.iter().all(|r| r.state != RunState::Started)
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the runs did not land:\n{}", e.ok(&["runs"]));
}

#[test]
fn a_comment_on_the_stack_lets_the_agent_change_it_all() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["Squash", "the", "top", "into", "the", "bottom,", "and", "add", "c."]);
    let before = e.column(2);
    let user_head = git(&e.src, &["rev-parse", "HEAD"]);
    // Squash the top patch into the bottom one, and add one on top.
    let claude = fake_claude(
        &e,
        "claude-edit.sh",
        r#"set -e
test "$(git rev-parse --show-toplevel)" != "$STACKED_SRC"
a=$(git rev-parse HEAD~2) b=$(git rev-parse HEAD~1) c=$(git rev-parse HEAD)
seq="${TMPDIR:-/tmp}/seq-$$.sh"
printf '#!/bin/sh\nprintf "pick %s\\nfixup %s\\npick %s\\n" > "$1"\n' $a $c $b > "$seq"
chmod +x "$seq"
GIT_SEQUENCE_EDITOR="$seq" git -c user.name=C -c user.email=c@x rebase -q -i HEAD~3 >&2
rm "$seq"
echo extra > c.txt && git add c.txt && git -c user.name=C -c user.email=c@x commit -qm "Bug 4 - add c""#,
        r#"Done. {"replies": {"c1": "Squashed it, and added c."}}"#,
    );
    let (fine, out, err) = send(&e, &claude, &[&id], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    assert!(out.contains(&format!("{id}\tsent (may change the code)")), "{out}");
    assert!(out.contains(&format!("{id}\tthe stack\tagent replied (change)")), "{out}");
    assert!(out.contains("  agent: Squashed it, and added c."), "{out}");
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
        runs.contains("change s \"Squash the top into the bottom, and add c.\"\tdone\treviewed: its change accepted; 1 reply accepted"),
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

    // An answer works in the repository as it is, unless the settings say
    // otherwise: then in the stack's own worktree for answers.
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, "[agent]\nanswer_only_worktree = \"ondemand\"\nworktree = \"pwd\"\n").unwrap();
    let id = comment(&e, &["Why?"]);
    assert!(send(&e, &claude, &[&id, "--answer-only"], &[("STACKED_CONFIG", &settings)]).0);
    assert_eq!(pwd(), e.src.join(".stacked/s/answer-only-agent").display().to_string());
    // A change in pwd: the user's checkout, moved to the stack while the
    // agent works, back where it was after.
    let head = git(&e.src, &["rev-parse", "HEAD"]);
    let id = comment(&e, &["Change", "it."]);
    let (fine, _, err) = send(&e, &claude, &[&id], &[("STACKED_CONFIG", &settings)]);
    assert!(fine, "{err}");
    assert_eq!(pwd(), e.src.display().to_string());
    assert!(std::fs::read_to_string(&subject).unwrap().starts_with("Bug 3 - café"), "at the stack's top");
    assert_eq!(git(&e.src, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&e.src, &["symbolic-ref", "--short", "HEAD"]).trim(), "main");
    // Not while it has changes to files git tracks.
    std::fs::write(e.src.join("a.txt"), "mine\n").unwrap();
    let id = comment(&e, &["Again."]);
    let (fine, out, _) = send(&e, &claude, &[&id], &[("STACKED_CONFIG", &settings)]);
    assert!(!fine && out.contains("has changes to files git tracks"), "{out}");
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
    for _ in 0..100 {
        if !agent.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!agent.exists(), "removed");
    assert!(answering.join("mine.txt").exists(), "kept: it holds something uncommitted");
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
    assert!(out.contains("\tpatch 3 · b.txt: new\tagent replied (change)"), "{out}");
    assert!(out.contains("agent: Split: b is a patch of its own."), "{out}");
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
    assert!(all.contains("This needs more than the patch: patch 3 says two as well"), "{all}");
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
    assert!(c.contains(&format!("{id}\tpatch 1 · a.txt: 2")) && c.contains("agent: Tested, in patch 4."), "{c}");
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
    let claude = fake_claude(
        &e,
        "claude-slow.sh",
        r#"sleep 2; echo x >> a.txt; git -c user.name=C -c user.email=c@x commit -qam "Bug 3 - café, a very long title that format-patch will fold over more than one line" --amend"#,
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
    // Landed, its change waits for your review, and the stack with it.
    landed(&e, 1);
    let (fine, _, err) = e.run(&["drop", "s", "1"]);
    let why = "the agent's change of the stack waits for your review: the stack can be edited by hand again once you have reviewed it";
    assert!(!fine && err.contains(why), "{err}");
    let (fine, _, err) = e.run(&["reply", "s", &id, "Also", "this."]);
    assert!(!fine && err.contains("the agent's reply to it waits for your review"), "{err}");
    assert!(!e.ok(&["comments", "s"]).contains("agent: Amended."));
    let reviewed = e.ok(&["review", "s", "--accept-all"]);
    assert!(reviewed.contains("reviewed: its change accepted; 1 reply accepted"), "{reviewed}");
    assert!(e.ok(&["comments", "s"]).contains("agent: Amended."));
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
    let env = [("STACKED_CLAUDE", claude.as_path())];
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
    assert!(review.contains(&format!("{id}\tthe stack\tthe agent's reply waits for your review")), "{review}");
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
fn several_asks_wait_their_turn() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let one = comment(&e, &["First."]);
    let two = comment(&e, &["Second."]);
    let three = comment(&e, &["--patch", "2", "Why b?"]);
    let claude = fake_claude(&e, "claude-wait.sh", "sleep 1", r#"{"replies": {"c1": "Seen."}}"#);
    let env = [("STACKED_CLAUDE", claude.as_path())];
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
    // Each goes in turn, on its own; what each brings back waits for review,
    // the first landed first.
    landed(&e, 3);
    let runs = e.ok(&["runs"]);
    assert_eq!(runs.lines().filter(|l| l.contains("\tdone\t")).count(), 3, "{runs}");
    let queue = e.ok(&["landed", "s"]);
    assert_eq!(queue.lines().filter(|l| !l.starts_with(' ')).count(), 3, "{queue}");
    for _ in 0..3 {
        e.ok(&["review", "s", "--accept-all"]);
    }
    assert_eq!(e.ok(&["landed", "s"]), "");
    let read = e.ok(&["comments", "s", "--show", "to-read"]);
    assert_eq!(read.matches("agent: Seen.").count(), 3, "{read}");
    assert!(e.ok(&["comments", "s", "--show", "to-read-only"]).contains(&three));
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
    // it runs in the repository as it is, no worktree made for it, and is
    // told to read the stack through stacked.
    let review = fake_claude(
        &e,
        "claude-review.sh",
        r#"test "$PWD" = "$STACKED_SRC" || exit 3"#,
        r#"{"replies": {"c1": "Two things."}, "comments": [{"patch": 2, "file": "b.txt", "side": "+", "line": "new", "text": "Say what b is for."},
            {"patch": 1, "file": "nowhere", "side": "+", "line": "?", "text": "On the whole patch."}]}"#,
    );
    let id = comment(&e, &["Review", "it."]);
    let (fine, out, err) = send(&e, &review, &[&id, "--answer-only"], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    let db = Db::load(&e.db).unwrap();
    let info = stacked::ask::info(db.runs().last().unwrap()).unwrap();
    assert!(info.worktree.is_none(), "no worktree: {:?}", info.worktree);
    assert!(info.prompt.contains("the user's repository, as it is checked out now"), "{}", info.prompt);
    assert!(info.prompt.contains("show s --rev") && info.prompt.contains("N --path FILE"), "{}", info.prompt);
    drop(db);
    assert!(out.contains("agent replied (answer only) · opened 2 comments"), "{out}");
    let c = e.ok(&["comments", "s", "--patch", "2"]);
    assert!(c.contains("b.txt: new") && c.contains("agent: Say what b is for."), "{c}");
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("patch 1, as a whole") && c.contains("On the whole patch."), "{c}");
    assert!(e.ok(&["runs"]).contains("reviewed: 2 opened comments accepted; 1 reply accepted"));

    // A build and test runs where claude likes, here the repo, and reads
    // the stack with the command it is given.
    let test = fake_claude(
        &e,
        "claude-test.sh",
        r#"test "$PWD" = "$STACKED_SRC" || exit 3"#,
        r#"{"replies": {"c1": "Built them."}, "patches": {"1": "builds; 3 tests pass", "3": "fails: a.txt"}}"#,
    );
    let id = comment(&e, &["Build", "and", "test."]);
    let (fine, _, err) = send(&e, &test, &[&id, "--build-and-test"], &[("STACKED_SRC", &e.src)]);
    assert!(fine, "{err}");
    let runs = e.ok(&["runs"]);
    assert!(runs.lines().last().is_some_and(|l| l.ends_with("reviewed: 2 opened comments accepted; 1 reply accepted")), "{runs}");
    assert!(e.ok(&["comments", "s", "--patch", "3"]).contains("fails: a.txt (tested @"));
    let db = Db::load(&e.db).unwrap();
    let info = stacked::ask::info(db.runs().last().unwrap()).unwrap();
    assert!(info.prompt.contains("export s --rev"), "{}", info.prompt);
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
        db.records.iter().filter(|r| matches!(r, Record::Patch(_))).count()
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
    // reviewer's turn of it. A new thread rejected is not pulled again.
    let mut later = first.clone();
    later.push(remote("c4", Some("b.txt"), 1, "Ping?", Some("c3")));
    later.push(remote("c5", None, 0, "Also, the title.", None));
    let run = Db::edit(&e.db, |db| stacked::review::file(db, "s", "pull-3", 1, &pulled(later.clone()))).unwrap();
    assert_eq!(run.summary, "2 new comments from 1 revision: they wait for your review");
    let h = run.held.unwrap();
    assert_eq!((h.replies.len(), h.opened.len(), h.replies[0].who()), (1, 1, "reviewer"));
    let marks = [("reply 0".to_string(), true), ("opened 0".to_string(), false)].into();
    Db::edit(&e.db, |db| stacked::held::close(db, "pull-3", &marks, None)).unwrap();
    let db = Db::load(&e.db).unwrap();
    let inline = db.note(&p.change, "phab:c1").unwrap();
    assert!(inline.text.ends_with("```reviewer reviewer\nPing?\n```"), "{}", inline.text);
    assert!(inline.remote.contains(&"c4".to_string()), "{:?}", inline.remote);
    assert!(db.note(&p.change, "phab:c5").is_none(), "rejected, it is not filed");
    let again = Db::edit(&e.db, |db| stacked::review::file(db, "s", "pull-4", 1, &pulled(later))).unwrap();
    assert_eq!(again.summary, "nothing new from 1 revision", "nor pulled again");
}

#[test]
fn apply_is_the_only_change_to_the_worktree() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["drop", "s", "2"]);
    std::fs::write(e.src.join("a.txt"), "dirty\n").unwrap();
    let (fine, _, err) = e.run(&["apply", "s"]);
    assert!(!fine && err.contains("uncommitted"), "{err}");
    git(&e.src, &["checkout", "--", "a.txt"]);
    let out = e.ok(&["apply", "s", "--branch", "s"]);
    assert!(out.contains("applied"), "{out}");
    assert_eq!(git(&e.src, &["branch", "--show-current"]).trim(), "s");
    let log = git(&e.src, &["log", "--format=%s", "base..HEAD"]);
    assert_eq!(log.lines().count(), 2, "{log}");
    assert!(!e.src.join("b.txt").exists());
    assert!(e.ok(&["runs"]).contains("apply s\tdone"));
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
        let mut all = vec!["edit", "s"];
        all.extend(extra);
        stacked_in(&e.src, &e.db, &all, "", &[("EDITOR", &vim)])
    };
    let said = || std::fs::read_to_string(&args).unwrap();

    // The patch as mail, at a line.
    script("sed -i '' 's/^Subject: Bug 2 - add b$/Subject: Bug 2 - add bee/' \"$file\"");
    let (fine, _, err) = edit(&["2", "--line", "7"]);
    assert!(fine, "{err}");
    assert_eq!(said().lines().last(), Some("+7"));
    assert_eq!(e.column(3)[1], "Bug 2 - add bee");

    // A file as the patch leaves it, at a line: the patch is made again
    // from it, the rest of it as it was.
    script("sed -i '' 's/^2$/deux/' \"$file\"");
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "2"]);
    assert!(fine && err.contains("edited a.txt in Bug 1 - change two"), "{err}");
    assert_eq!(said().lines().last(), Some("+2"));
    let mail = e.ok(&["show", "s", "1"]);
    assert!(mail.contains("-two\n+deux\n") && mail.contains("Subject: Bug 1 - change two"), "{mail}");
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2);
    assert!(e.ok(&["check"]).contains("s: applies"));

    // One the patch above cannot take is not taken; it is kept, and taken
    // up again from where it was left.
    script("sed -i '' 's/^eight$/huit/' \"$file\"");
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "8"]);
    assert!(!fine && err.contains("patch 3 (Bug 3 - café") && err.contains("does not apply"), "{err}");
    let kept = err.split("Your edit is kept in ").nth(1).unwrap().split(':').next().unwrap().to_string();
    assert!(std::fs::read_to_string(&kept).unwrap().contains("huit"));
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2, "nothing changed");
    script("sed -i '' 's/^huit$/eight/' \"$file\"");
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--from", &kept]);
    assert!(fine && err.contains("saved as it was"), "back as it was: {err}");
    let (fine, _, err) = edit(&["1", "--path", "b.txt"]);
    assert!(!fine && err.contains("does not change b.txt"), "{err}");
}

#[test]
fn every_ask_may_use_the_tools_the_settings_add() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, "[agent]\ntools = [\"mcp__bugzilla__*\", \"Bash(jq *)\"]\n").unwrap();
    let args = e.root.join("args");
    let claude = fake_claude(&e, "claude-tools.sh", &format!("echo \"$*\" > {}", args.display()), r#"{"replies": {"c1": "Read it."}}"#);
    let id = comment(&e, &["What", "does", "bug", "1", "say?"]);
    let (fine, _, err) = send(&e, &claude, &[&id, "--answer-only"], &[("STACKED_CONFIG", &settings)]);
    assert!(fine, "{err}");
    // An answer's tools, then the settings' own.
    let said = std::fs::read_to_string(&args).unwrap();
    let tools = said.split("--allowedTools ").nth(1).unwrap().split(" --").next().unwrap().trim_end();
    assert!(tools.contains("Read,") && tools.ends_with(",mcp__bugzilla__*,Bash(jq *)"), "{tools}");
    assert!(!tools.contains("Edit") && !tools.contains("Write"), "{tools}");
    // What it was not let use is said, in its stream and in its run, with
    // how to allow it.
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
    let said = "the agent was not allowed to use mcp__moz__get_bug, Bash(curl …): your claude settings, or stacked's agent.tools, allow more";
    assert!(runs.contains(said), "{runs}");
    let run = runs.lines().last().unwrap().split('\t').next().unwrap().to_string();
    let transcript = e.ok(&["transcript", &run]);
    assert!(transcript.contains("⛔ not allowed: mcp__moz__get_bug") && transcript.contains("⛔ not allowed: Bash: curl -s"), "{transcript}");
}

#[test]
fn the_tools_every_ask_may_use_are_set_up_with_the_agent() {
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
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, "[agent]\ntools = [\"mcp__notes__write\"]\n").unwrap();
    // All of moz's; notes' one by one, the one that reads; of the others,
    // WebFetch alone.
    let started = std::time::Instant::now();
    let (fine, _, err) = stacked_in(
        &e.src,
        &e.db,
        &["setup", "agent", "s"],
        "y\ne\ny\nn\nn\nn\nn\ny\n",
        &[("STACKED_CLAUDE", &claude), ("STACKED_CONFIG", &settings)],
    );
    assert!(fine, "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "stopped once it said what it has");
    assert_eq!(std::fs::read_to_string(&pwd).unwrap().trim(), e.src.display().to_string(), "in the stack's repository");
    assert!(err.contains("The MCP server moz: get_bug, get_revision."), "{err}");
    assert!(err.contains("Let every ask use Edit? It changes things: an answer could too. [y/N]"), "{err}");
    // Written in the checkout's own settings, which it said first; the
    // settings' file as it was.
    let local = e.src.join(".stacked/local.toml");
    assert!(err.contains(&format!("Now, agent.tools in {}: mcp__notes__write.", settings.display())), "{err}");
    assert!(err.contains(&format!("What you let every ask use is written in {}.", local.display())), "{err}");
    let (_, out, _) = stacked_in(&e.src, &e.db, &["config", "agent.tools"], "", &[("STACKED_CONFIG", &settings)]);
    assert_eq!(out, "mcp__moz__*\nmcp__notes__read\nWebFetch\n", "the write of notes taken out");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "[agent]\ntools = [\"mcp__moz__*\", \"mcp__notes__read\", \"WebFetch\"]\n");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "[agent]\ntools = [\"mcp__notes__write\"]\n");
    // With --global, in the settings' file, starting from its own; the
    // checkout's still the one used there, as said.
    let (fine, _, err) = stacked_in(
        &e.src,
        &e.db,
        &["setup", "agent", "s", "--global"],
        "n\ne\ny\ny\nn\nn\nn\nn\n",
        &[("STACKED_CLAUDE", &claude), ("STACKED_CONFIG", &settings)],
    );
    assert!(fine, "{err}");
    assert!(err.contains(&format!("What you let every ask use is written in {}.", settings.display())), "{err}");
    assert!(err.contains("sets agent.tools too, and is the one used in its checkout"), "{err}");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "[agent]\ntools = [\"mcp__notes__read\", \"mcp__notes__write\"]\n");
    // Outside a checkout, refused before the agent is asked.
    std::fs::remove_file(&pwd).unwrap();
    let (fine, _, err) = stacked_in(&e.root, &e.db, &["setup", "agent"], "", &[("STACKED_CLAUDE", &claude), ("STACKED_CONFIG", &settings)]);
    assert!(!fine && err.contains("is not in a git checkout"), "{err}");
    assert!(!pwd.exists(), "the agent was not started");
}

#[test]
fn a_checkout_s_own_settings_override_the_others_there() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let settings = e.root.join("settings.toml");
    std::fs::write(&settings, "[agent]\ntools = [\"mcp__bugzilla__*\"]\n[view]\nlayout = \"compact\"\n").unwrap();
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
        let mut all = vec!["edit", "s"];
        all.extend(extra);
        stacked_in(&e.src, &e.db, &all, "", &[("EDITOR", &vim)])
    };
    // A step that writes `text` in the file.
    let put = |n: usize, text: &str| {
        let file = e.root.join(format!("text{n}"));
        std::fs::write(&file, text).unwrap();
        format!("cp {} \"$1\"", file.display())
    };
    let huit = "sed -i '' 's/^eight$/huit/' \"$1\"";

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
    let loud = "sed -i '' 's/^huit$/HUIT/' \"$1\"";
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
        let mut all = vec!["edit", "s"];
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
    script("sed -i '' 's/^2$/deux/' \"$file\"\necho new > c.txt");
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
    script("sed -i '' 's/^new$/newer/' \"$file\"");
    let (fine, _, err) = edit(&["2", "--path", "b.txt", "--line", "1", "--in", "worktree"]);
    assert!(fine && err.contains("edited b.txt in Bug 2 - add b"), "{err}");
    assert_eq!(last(), format!("+1 {} Bug 2 - add b", kept.display()));
    assert_eq!(git(&e.src, &["worktree", "list"]).lines().count(), 2);

    // In this checkout: moved to patch 2 for the edit, then back on main,
    // the files of yours git does not track left alone.
    std::fs::write(e.src.join("notes.txt"), "mine\n").unwrap();
    script("sed -i '' 's/^newer$/fresh/' \"$file\"");
    let (fine, _, err) = edit(&["2", "--path", "b.txt", "--line", "1", "--in", "pwd"]);
    assert!(fine && err.contains("edited b.txt in Bug 2 - add b"), "{err}");
    assert_eq!(last(), format!("+1 {} Bug 2 - add b", e.src.display()));
    assert!(e.ok(&["show", "s", "2"]).contains("+fresh"));
    assert_eq!((head(), branch()), (top.clone(), "main".to_string()));
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
    assert_eq!((head(), branch()), (top.clone(), "main".to_string()));

    // Without --in, where the stack's menus picked. One the patch above
    // cannot take is not taken: it is kept in a commit, and taken up again
    // from there.
    Db::edit(&e.db, |db| db.set_choice("s", "edit", "worktree")).unwrap();
    script("sed -i '' 's/^eight$/huit/' \"$file\"");
    let (fine, _, err) = edit(&["1", "--path", "a.txt", "--line", "8"]);
    assert!(!fine && err.contains("patch 3 (Bug 3 - café") && err.contains("does not apply"), "{err}");
    assert_eq!(last(), format!("+8 {} Bug 1 - change two", kept.display()));
    assert!(err.contains("--in worktree --from "), "{err}");
    let commit = err.split("--from ").nth(1).unwrap().split('`').next().unwrap().to_string();
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2, "nothing changed");
    script("sed -i '' 's/^huit$/eight/' \"$file\"");
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
    // A colour, as git writes one; refused if it is not one.
    assert!(config(&["colors.you", "208 bold"]).0);
    assert_eq!(config(&["colors.you"]).1, "208 bold\n");
    let (fine, _, err) = config(&["colors.agent", "orangeish"]);
    assert!(!fine && err.contains("colors.agent: \"orangeish\" is not a colour"), "{err}");
}

#[test]
fn deleted_comments_are_listed_as_they_were() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    assert_eq!(e.ok(&["comments", "s", "--show", "deleted"]), "");
    // Deleted as the editor deletes one: its text emptied.
    Db::edit(&e.db, |db| {
        let n = db.stack_notes("s").into_iter().next().cloned().unwrap();
        db.set_note(stacked::db::Note { text: String::new(), ..n })
    })
    .unwrap();
    assert!(!e.ok(&["comments", "s"]).contains("Why 2?"));
    let out = e.ok(&["comments", "s", "--show", "deleted"]);
    assert!(out.starts_with(&id) && out.contains("\tdeleted @") && out.contains("  you: Why 2?"), "{out}");
    assert_eq!(e.ok(&["comments", "s", "--patch", "1", "--show", "deleted"]), out);
    assert_eq!(e.ok(&["comments", "s", "--patch", "2", "--show", "deleted"]), "");
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
    assert!(c.contains("you: Why 2, not two?") && c.contains("agent: Seen.") && c.contains("you: Thanks, and why 9?"), "{c}");
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
    let cut = Db::load(&e.db).unwrap().records.len() + 1;
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
    assert!(out.contains("agent: To put c first.") && out.contains("opened 1 comment"), "{out}");
    let told = std::fs::read_to_string(&told).unwrap();
    assert!(told.contains("on the stack's history, about @") && told.contains("The stack's history, oldest first"), "{told}");
    assert!(told.contains(&format!("What @{rev} was:")) && told.contains("↕ Bug 3 - café"), "{told}");
    assert!(told.contains("export s --rev @N") && told.contains("Bash(") && told.contains(" log *)"), "{told}");
    assert!(!told.contains("acceptEdits"), "it changes nothing");
    let c = e.ok(&["comments", "s", "--patch", "1"]);
    assert!(c.contains("a.txt: 9") && c.contains("agent: This is the one moved."), "{c}");
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
    let run = |args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_stacked"));
        cmd.current_dir(&e.src).args(args).env("STACKED_CONFIG", &cfg).env_remove("STACKED_FILE");
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
    let again = Command::new(env!("CARGO_BIN_EXE_stacked")).arg("config").env("STACKED_CONFIG", &copy).output().unwrap();
    assert!(!String::from_utf8(again.stdout).unwrap().contains("# wrong"), "what it prints is a good file");
    // The database the settings name is the one opened when none is; what
    // is wrong with them is said by every command.
    let (out, err) = run(&["stacks"]);
    assert!(out.starts_with("s\t3 patches"), "{out}");
    assert!(err.contains("view.layout"), "{err}");
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
