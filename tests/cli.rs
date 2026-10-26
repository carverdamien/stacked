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
    // Unchained: a change waits for the review of the one before it.
    let unchained = e.root.join("unchained.toml");
    std::fs::write(&unchained, "[agent]\nchain = false\n").unwrap();
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
    // A commit on top that is not the stack's.
    git(&e.src, &["checkout", "-q", "main"]);
    std::fs::write(e.src.join("c.txt"), "c\n").unwrap();
    git(&e.src, &["add", "c.txt"]);
    git(&e.src, &["commit", "-qm", "Mine"]);
    assert!(e.ok(&["head", "s"]).ends_with("is at patch 3 of s, then 1 commit not in the stack\n"));
    // The same patch, its message other: still the stack's, by its diff.
    git(&e.src, &["checkout", "-q", "--detach", "main~1"]);
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 3 - other words"]);
    let out = e.ok(&["head", "s"]);
    assert!(out.ends_with("is at patch 3 of s\nthe message of patch 3 differs here: \"Bug 3 - other words\"\n"), "{out}");
    // Not on the stack's base.
    git(&e.src, &["checkout", "-q", "-b", "other", "base"]);
    std::fs::write(e.src.join("d.txt"), "d\n").unwrap();
    git(&e.src, &["add", "d.txt"]);
    git(&e.src, &["commit", "-qm", "Other"]);
    assert!(e.ok(&["head", "s"]).contains("is not on s"));
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
    // A commit whose message is not the patch's is not taken: the patch is.
    git(&e.src, &["checkout", "-q", "--detach", "main~2"]);
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 1 - other words"]);
    e.ok(&["head", "s", "--to", "1"]);
    assert_eq!(git(&e.src, &["log", "-1", "--format=%s"]).trim(), "Bug 1 - change two");
}

#[test]
fn a_message_that_differs_in_the_checkout_goes_either_way() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let subject = |rev: &str| git(&e.src, &["log", "-1", "--format=%s", rev]).trim().to_string();
    let title = "Bug 3 - café, a very long title that format-patch will fold over more than one line";
    // A branch's top: patch 3 by its diff, its message other.
    git(&e.src, &["checkout", "-q", "-b", "mine", "main"]);
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 3 - other words"]);
    let out = e.ok(&["head", "s"]);
    assert!(out.ends_with("is at patch 3 of s\nthe message of patch 3 differs here: \"Bug 3 - other words\"\n"), "{out}");
    // Into the checkout: the commit made again on the same files, the
    // branch moved to it.
    let tree = git(&e.src, &["rev-parse", "mine^{tree}"]);
    let out = e.ok(&["head", "s", "--messages-to-checkout"]);
    assert!(out.starts_with("mine has the stack's message of patch 3: 1 commit made again"), "{out}");
    assert_eq!((subject("mine"), git(&e.src, &["rev-parse", "mine^{tree}"])), (title.to_string(), tree));
    assert!(!e.ok(&["head", "s"]).contains("differs"));
    // Into the stack: an edit by hand, to review; accepted, the patch has it.
    git(&e.src, &["commit", "-q", "--amend", "-m", "Bug 3 - other words"]);
    let (fine, _, err) = e.run(&["head", "s", "--messages-to-stack", "3"]);
    assert!(fine && err.contains("took the message of patch 3 from your checkout: it waits for your review"), "{err}");
    e.ok(&["review", "s", "--accept-all"]);
    assert!(e.ok(&["show", "s", "3"]).contains("Bug 3 - other words"));
    assert!(!e.ok(&["head", "s"]).contains("differs"));
    let (fine, _, err) = e.run(&["head", "s", "--messages-to-stack"]);
    assert!(fine && err.contains("no message differs"), "{err}");
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
    let claude = fake_claude(&e, "claude-wait.sh", "sleep 1", r#"{"replies": {"c1": "Seen."}}"#);
    // Unchained: each waits for what goes before it, as self-review's do.
    let unchained = e.root.join("unchained.toml");
    std::fs::write(&unchained, "[agent]\nchain = false\n").unwrap();
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
    // Each goes in turn, on its own; what each brings back waits for review,
    // the first landed first.
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
    assert_eq!(read.matches("agent: Seen.").count(), 3, "{read}");
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
    assert_eq!(read.matches("agent: Seen.").count(), 2, "{read}");
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
        "set -e\nsed -i '' 's/^2$/deux/' a.txt\ngit -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a",
        r#"{"replies": {"c1": "Spelled it deux."}}"#,
    );
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--wait"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{out}{err}");
    let landed = e.ok(&["landed", "s"]);
    let run = landed.split('\t').next().unwrap().to_string();
    assert!(landed.contains("Spelled it deux."), "{landed}");
    // Edited by hand from its review: patch 1 as the work leaves it.
    let editor = e.script("editor.sh", "sed -i '' 's/^+deux$/+two, spelled/' \"$1\"\n");
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
    assert!(e.ok(&["comments", "s"]).contains("agent: Spelled it deux."));
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
    assert!(e.ok(&["comments", "s", "--show", "closed"]).contains("agent: Because."));
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
    assert!(landed.lines().count() == 1 && landed.ends_with("\t1 asking comment to close or keep, nothing brought back\n"), "{landed}");
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
fn a_reply_taken_at_once_leaves_its_work_to_review_while_its_comment_is_open() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let id = comment(&e, &["--patch", "1", "Why", "2?"]);
    let claude = fake_claude(&e, "claude-answer.sh", "", r#"{"replies": {"c1": "Because."}}"#);
    let env = [("STACKED_CLAUDE", claude.as_path())];
    let ask = |e: &Env| {
        let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &id, "--answer-only", "--wait"], "", &env);
        assert!(fine && out.contains("waits for your review: 1 reply"), "{out}{err}");
        stacked::held::queue(&Db::load(&e.db).unwrap(), "s")[0].id.clone()
    };
    // Taken at once, as Enter on the comment takes it: the reply is in its
    // thread, and the work still waits, for the comment to be closed or
    // kept.
    let run = ask(&e);
    Db::edit(&e.db, |d| stacked::held::take(d, &run, 0)).unwrap();
    assert!(e.ok(&["comments", "s"]).contains("agent: Because."));
    let landed = e.ok(&["landed", "s"]);
    assert!(landed.lines().count() == 1 && landed.starts_with(&run) && landed.ends_with("\t1 asking comment to close or keep\n"), "{landed}");
    // Its review closed, the comment kept: open, the reply in its thread.
    assert_eq!(e.ok(&["review", "s", "--accept-all"]), format!("{run}\treviewed: 1 reply accepted\n"));
    assert_eq!(e.ok(&["landed", "s"]), "");
    assert!(e.ok(&["comments", "s"]).contains("agent: Because."));
    // Again, the review closing the comment this time: closed, its thread
    // holding both replies.
    e.ok(&["reply", "s", &id, "Really?"]);
    let run = ask(&e);
    Db::edit(&e.db, |d| stacked::held::take(d, &run, 0)).unwrap();
    assert!(e.ok(&["landed", "s"]).starts_with(&run));
    let out = e.ok(&["review", "s", "--accept-all", "--close-asking"]);
    assert_eq!(out, format!("{run}\treviewed: 1 reply accepted; 1 asking comment closed\n"));
    assert_eq!(e.ok(&["comments", "s"]), "");
    let closed = e.ok(&["comments", "s", "--show", "closed"]);
    assert_eq!(closed.matches("agent: Because.").count(), 2, "{closed}");
}

#[test]
fn taking_the_last_reply_closes_the_review_once_no_comment_it_answers_is_open() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    // Closed while its reply waits: taking the reply closes the review.
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
    assert!(runs.starts_with(&format!("{run}\t")) && runs.ends_with("\tdone\treviewed: 1 reply accepted\n"), "{runs}");
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
    assert!(comments.contains("\tthe stack\tmoved from patch 2") && comments.contains("agent: Patch 3 has to change too."), "{comments}");
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
    assert!(shown.contains("agent: Say two.") && shown.contains("agent: And test it."), "{shown}");
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
            "set -e\nif grep -qx deux a.txt; then sed -i '' 's/^deux$/2 spelled/' a.txt; else sed -i '' 's/^2$/deux/' a.txt; fi\n\
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a\nprintf '%s\\n=====\\n' \"$2\" >> {}",
            prompts.display()
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
    assert_eq!(thread, ["  you: Spell it out.", "  agent: Done.", "  you: Not like this.", "  agent: Done."], "{shown}");
    assert!(shown.contains(&mine), "{shown}");
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
             *) if grep -qx deux a.txt; then sed -i '' 's/^deux$/2 spelled/' a.txt; else sed -i '' 's/^2$/deux/' a.txt; fi\n     \
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a ;;\nesac",
            p = prompts.display(),
            g = gate.display()
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    );
    // Unchained: a change waits for the review of the one before it.
    let unchained = e.root.join("unchained.toml");
    std::fs::write(&unchained, "[agent]\nchain = false\n").unwrap();
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
             *) if grep -qx deux a.txt; then {wait}sed -i '' 's/^deux$/2 spelled/' a.txt; else sed -i '' 's/^2$/deux/' a.txt; fi\n     \
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a ;;\nesac",
            p = prompts.display()
        ),
        r#"{"replies": {"c1": "Done."}}"#,
    )
}

/// The runs waiting for review on `s`, by id, the first landed first.
fn landed_ids(e: &Env) -> Vec<String> {
    e.ok(&["landed", "s"]).lines().filter(|l| !l.starts_with(' ')).map(|l| l.split('\t').next().unwrap().to_string()).collect()
}

/// Whether process `pid` is still there, a moment given to go.
fn still_there(pid: u32) -> bool {
    for _ in 0..40 {
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
    let mut going = None;
    for _ in 0..200 {
        let d = Db::load(&e.db).unwrap();
        going = d.runs().into_iter().find(|r| r.state == RunState::Started).and_then(|r| Some((r.id.clone(), r.pid?)));
        if going.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let (guess, pid) = going.expect("the guess works");
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
    // The answer goes at once, on the stack as it is, not the change's.
    let (fine, out, err) = stacked_in(&e.src, &e.db, &["send", "s", &why, "--answer-only", "--wait"], "", &env);
    assert!(fine, "{out}{err}");
    let told = std::fs::read_to_string(&prompts).unwrap();
    let asked = told.split("=====").nth(1).unwrap();
    assert!(asked.contains("show s --rev ") && !asked.contains("--work"), "{told}");
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
             *) until test -e {g}; do sleep 0.1; done; sed -i '' 's/^2$/deux/' a.txt\n     \
             git -c user.name=C -c user.email=c@x commit -q --amend --no-edit -a ;;\nesac",
            p = prompts.display(),
            g = gate.display()
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
    assert!(e.ok(&["comments", "s", "--patch", "3"]).contains("fails: a.txt (tested @"));
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
    // Accepted, it is in the place of her turn, your answer after it.
    assert_eq!(close("pull-2", true), "1 edited comment accepted");
    assert_eq!(thread("phab:a1"), format!("{}\n{}\nBecause.", turn("What is b for?"), turn("And why now?")));
    // Its edit again, rejected: not pulled again, and the thread as it was.
    let run = pull("pull-4", &[v3.clone(), a2.clone()]);
    assert_eq!(run.held.unwrap().replies[0].was.as_deref(), Some("What is b for?"));
    assert_eq!(close("pull-4", false), "1 edited comment rejected");
    assert_eq!(pull("pull-5", &[v3, a2]).summary, "nothing new from 1 revision");
    assert_eq!(thread("phab:a1"), format!("{}\n{}\nBecause.", turn("What is b for?"), turn("And why now?")));
    // Edited once more, and her reply too: each an edit of the turn it was
    // filed as, the one rejected never filed.
    let v4 = ann("a1v4", "Is b needed?", None, &[("a1v3", "What is b for, really?"), ("a1v2", "What is b for?"), ("a1", "What is b?")]);
    let r2 = ann("a2v2", "And why now, in this patch?", Some("a1"), &[("a2", "And why now?")]);
    let run = pull("pull-6", &[v4.clone(), r2.clone()]);
    assert_eq!(run.summary, "2 edited comments from 1 revision: they wait for your review");
    close("pull-6", true);
    assert_eq!(thread("phab:a1"), format!("{}\n{}\nBecause.", turn("Is b needed?"), turn("And why now, in this patch?")));
    let n = Db::load(&e.db).unwrap().note(&p.change, "phab:a1").unwrap().clone();
    assert_eq!(n.remote, ["a1", "a2", "a1v2", "a1v4", "a2v2"]);
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
        stacked::ask::promote(db, "s", &n, "it is the whole stack's")
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
    assert!(moved.text.starts_with(&format!("{}\n{}\n", turn("Typo there."), turn("Still."))), "{}", moved.text);
    assert!(moved.text.ends_with(&turn("Ping?")), "{}", moved.text);
    drop(db);
    // Its thread closed, an edit of it is not pulled.
    Db::edit(&e.db, |db| {
        let n = db.note(&p.change, "phab:a1").unwrap().clone();
        db.set_note(Note { text: String::new(), ..n })
    })
    .unwrap();
    let v5 = ann("a1v5", "Never mind.", None, &[("a1v4", "Is b needed?"), ("a1v3", ""), ("a1v2", ""), ("a1", "")]);
    assert_eq!(pull("pull-9", &[v5, r2]).summary, "nothing new from 1 revision");
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
    std::fs::write(&settings, "[agent]\nschema = false\n").unwrap();
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
        let mut all = vec!["edit", "s", "--accept"];
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
    // Told that a tool refused is not allowed, not a service saying no.
    assert!(said.contains("Say so in what you write: that the tool was not allowed, naming it"), "{said}");
    assert!(said.contains("never that the service behind it refused, failed or would not let you read"), "{said}");
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
        let out = Command::new("zsh")
            .args(["-fc", &script, "zsh"])
            .arg(&function)
            .current_dir(&e.src)
            .env("PATH", &path)
            .env("STACKED_FILE", &e.db)
            .env("STACKED_CONFIG", "/nonexistent/stacked.toml")
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
        let out = Command::new(env!("CARGO_BIN_EXE_stacked"))
            .args(args)
            .current_dir(&e.src)
            .env("STACKED_CONFIG", "/nonexistent/stacked.toml")
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
