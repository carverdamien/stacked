use std::path::Path;
use std::process::{Command, Stdio};

use stacked::db::{Db, Record};
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

fn stacked(db: &Path, args: &[&str], stdin: &str, editor: Option<&Path>) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_stacked"));
    cmd.arg("-f").arg(db).args(args).env_remove("VISUAL");
    match editor {
        Some(e) => cmd.env("EDITOR", e),
        None => cmd.env("EDITOR", "false"),
    };
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

fn ok(db: &Path, args: &[&str], stdin: &str) -> String {
    let (ok, out, err) = stacked(db, args, stdin, None);
    assert!(ok, "stacked {args:?}: {err}");
    out
}

/// A repo with a base commit and three on top, the last with a title that
/// is not ASCII and a body.
fn repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);
    git(dir, &["tag", "base"]);
    std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
    git(dir, &["commit", "-qam", "Bug 1 - change two"]);
    std::fs::write(dir.join("b.txt"), "new\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "Bug 2 - add b\n\nA body.\n\nMore body."]);
    std::fs::write(dir.join("a.txt"), "1\n2\nthree\n").unwrap();
    git(dir, &["commit", "-qam", "Bug 3 - café, a very long title that format-patch will fold over more than one line"]);
}

#[test]
fn round_trip_through_git() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    repo(&src);
    let db = tmp.path().join("db.jsonl");
    let mail = git(&src, &["format-patch", "--stdout", "base..HEAD"]);
    ok(&db, &["import", "s"], &mail);
    let shown = ok(&db, &["show", "s"], "");
    let titles: Vec<&str> = shown.lines().map(|l| l.split('\t').nth(3).unwrap()).collect();
    assert_eq!(
        titles,
        [
            "Bug 1 - change two",
            "Bug 2 - add b",
            "Bug 3 - café, a very long title that format-patch will fold over more than one line"
        ]
    );
    let one = ok(&db, &["show", "s", "2"], "");
    assert!(one.contains("Subject: Bug 2 - add b\n\nA body.\n\nMore body.\n---\ndiff --git"), "{one}");
    assert!(one.starts_with("From: A U Thor <author@example.com>\n"), "{one}");

    let out = ok(&db, &["export", "s"], "");
    let dst = tmp.path().join("dst");
    git(tmp.path(), &["clone", "-q", src.to_str().unwrap(), "dst"]);
    git(&dst, &["checkout", "-q", "base"]);
    let mut am = Command::new("git")
        .args(["am", "-q"])
        .current_dir(&dst)
        .env("GIT_COMMITTER_NAME", "C")
        .env("GIT_COMMITTER_EMAIL", "c@example.com")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    am.stdin.take().unwrap().write_all(out.as_bytes()).unwrap();
    assert!(am.wait().unwrap().success());
    assert_eq!(git(&dst, &["rev-parse", "HEAD^{tree}"]), git(&src, &["rev-parse", "HEAD^{tree}"]));
    assert_eq!(
        git(&dst, &["log", "--format=%an <%ae>%n%B", "base..HEAD"]),
        git(&src, &["log", "--format=%an <%ae>%n%B", "base..HEAD"])
    );
}

#[test]
fn edits_undo_and_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    repo(&src);
    let db = tmp.path().join("db.jsonl");
    ok(&db, &["import", "s"], &git(&src, &["format-patch", "--stdout", "base..HEAD"]));
    let titles = |db: &Path| -> Vec<String> {
        ok(db, &["show", "s"], "")
            .lines()
            .map(|l| l.split('\t').nth(3).unwrap()[..9].to_string())
            .collect()
    };
    ok(&db, &["move", "s", "3", "1"], "");
    assert_eq!(titles(&db), ["Bug 3 - c", "Bug 1 - c", "Bug 2 - a"]);
    ok(&db, &["drop", "s", "2"], "");
    assert_eq!(titles(&db), ["Bug 3 - c", "Bug 2 - a"]);
    ok(&db, &["undo", "s"], "");
    assert_eq!(titles(&db), ["Bug 3 - c", "Bug 1 - c", "Bug 2 - a"]);
    ok(&db, &["undo", "s"], "");
    assert_eq!(titles(&db), ["Bug 1 - c", "Bug 2 - a", "Bug 3 - c"]);
    // The import made the stack: there is nothing before it.
    let (fine, _, err) = stacked(&db, &["undo", "s"], "", None);
    assert!(!fine && err.contains("nothing to undo"), "{err}");

    // Every edit is in the log, undos too, and any of them can be gone back to.
    let log = ok(&db, &["log", "s"], "");
    assert_eq!(log.lines().count(), 5, "{log}");
    let drop_rev = log.lines().find(|l| l.contains("\tdrop ")).unwrap();
    let rev = drop_rev.split('\t').next().unwrap();
    ok(&db, &["restore", "s", rev], "");
    assert_eq!(titles(&db), ["Bug 3 - c", "Bug 2 - a"]);

    ok(&db, &["delete", "s"], "");
    assert_eq!(ok(&db, &["stacks"], ""), "");
    ok(&db, &["undo", "s"], "");
    assert_eq!(titles(&db), ["Bug 3 - c", "Bug 2 - a"]);
}

#[test]
fn todo_list_in_the_editor() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    repo(&src);
    let db = tmp.path().join("db.jsonl");
    ok(&db, &["import", "s"], &git(&src, &["format-patch", "--stdout", "base..HEAD"]));
    // The editor reverses the list, rewords the patch that was on top and
    // drops the bottom one; then rewrites the message it is given.
    let editor = tmp.path().join("editor.sh");
    std::fs::write(
        &editor,
        r#"#!/bin/sh
case "$1" in
*.todo) grep -v '^#' "$1" | grep . | tail -r | sed '1s/^pick/reword/;$s/^pick/drop/' > "$1.new" && mv "$1.new" "$1" ;;
*.txt) printf 'Bug 3 - reworded\n\nNew body.\n' > "$1" ;;
esac
"#,
    )
    .unwrap();
    Command::new("chmod").arg("+x").arg(&editor).status().unwrap();
    let (fine, _, err) = stacked(&db, &["edit", "s"], "", Some(&editor));
    assert!(fine, "{err}");
    let shown = ok(&db, &["show", "s"], "");
    let titles: Vec<&str> = shown.lines().map(|l| l.split('\t').nth(3).unwrap()).collect();
    assert_eq!(titles, ["Bug 3 - reworded", "Bug 2 - add b"]);
    let log = ok(&db, &["log", "s"], "");
    assert!(log.lines().last().unwrap().contains("reword"), "{log}");
    // The reworded patch is a new version of the old one, its diff the same.
    let versions = ok(&db, &["versions", "s", "1"], "");
    assert_eq!(versions.lines().count(), 2, "{versions}");
    let inter = ok(&db, &["interdiff", "s", "1"], "");
    assert!(inter.contains("-Subject: Bug 3 - café") && inter.contains("+Subject: Bug 3 - reworded"), "{inter}");
    assert!(!inter.contains("+-"), "the diff did not change: {inter}");

    // An editor that fails changes nothing.
    let before = std::fs::read(&db).unwrap();
    let (fine, _, _) = stacked(&db, &["edit", "s"], "", None);
    assert!(!fine);
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn reimport_keeps_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    repo(&src);
    let db = tmp.path().join("db.jsonl");
    ok(&db, &["import", "s"], &git(&src, &["format-patch", "--stdout", "base..HEAD"]));
    let before = ok(&db, &["show", "s"], "");
    // Amend the middle patch in git and import the stack again.
    git(&src, &["checkout", "-q", "HEAD~1"]);
    std::fs::write(src.join("b.txt"), "newer\n").unwrap();
    git(&src, &["commit", "-q", "--amend", "-a", "--no-edit"]);
    ok(&db, &["import", "--replace", "s"], &git(&src, &["format-patch", "--stdout", "base..HEAD"]));
    let after = ok(&db, &["show", "s"], "");
    let changes = |s: &str| s.lines().map(|l| l.split('\t').nth(1).unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(changes(&after), changes(&before)[..2]);
    let ids = |s: &str| s.lines().map(|l| l.split('\t').nth(2).unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(ids(&after)[0], ids(&before)[0], "the same patch is stored once");
    assert_ne!(ids(&after)[1], ids(&before)[1]);
    let inter = ok(&db, &["interdiff", "s", "2"], "");
    assert!(inter.contains("-+new\n") && inter.contains("++newer\n"), "{inter}");
}

#[test]
fn torn_last_line_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db.jsonl");
    Db::edit(&path, |db| db.new_stack("s")).unwrap();
    let good = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{good}{{\"t\":\"stack\",\"na")).unwrap();
    assert_eq!(Db::load(&path).unwrap().stacks().len(), 1);
    Db::edit(&path, |db| db.new_stack("t")).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(&good) && !text.contains("\"na\n") && text.ends_with('\n'), "{text}");
    assert_eq!(Db::load(&path).unwrap().stacks().len(), 2);
    // A broken line that is not the last is an error, with its line.
    std::fs::write(&path, format!("{good}garbage\n{good}")).unwrap();
    let e = Db::load(&path).err().unwrap().0;
    assert!(e.contains(":3:"), "{e}");
}

#[test]
fn records_only_ever_append() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db.jsonl");
    let mut sizes = vec![];
    let d = |m: &str| Draft { message: m.into(), diff: "diff --git a/x b/x\n".into(), author: Some("A <a@b>".into()), date: None };
    Db::edit(&path, |db| db.new_stack("s")).unwrap();
    for m in ["one", "two", "three"] {
        let before = std::fs::read(&path).unwrap();
        Db::edit(&path, |db| ops::add(db, "s", d(m), None)).unwrap();
        let after = std::fs::read(&path).unwrap();
        assert!(after.starts_with(&before));
        sizes.push(after.len());
    }
    Db::edit(&path, |db| ops::move_patch(db, "s", 0, 2)).unwrap();
    Db::edit(&path, |db| db.undo("s")).unwrap();
    let db = Db::load(&path).unwrap();
    assert!(matches!(db.records[0], Record::Stacked { version: 1 }));
    let s = db.stack("s").unwrap();
    let titles: Vec<&str> = s.patches.iter().map(|id| db.patch(id).title()).collect();
    assert_eq!(titles, ["one", "two", "three"]);
}

#[test]
fn mail_parsing() {
    assert_eq!(decode_words("=?UTF-8?q?caf=C3=A9_au_lait?="), "café au lait");
    assert_eq!(decode_words("=?UTF-8?q?a?= =?UTF-8?q?b?= c"), "ab c");
    assert_eq!(decode_words("=?utf-8?b?Y2Fmw6k=?="), "café");
    let d = parse_mail("Subject: [PATCH 1/2] Title\n\nBody\n---\n a | 1 +\n\ndiff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-- \n+x\n-- \n2.50.0\n\n").unwrap();
    assert_eq!(d.message, "Title\n\nBody");
    assert_eq!(d.diff, "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-- \n+x\n", "a removed `- ` line is not the signature");
    // A bare message and diff, as the patch is edited, with no headers.
    let d = parse_mail("Title only\n---\ndiff --git a/a b/a\n").unwrap();
    assert_eq!((d.message.as_str(), d.diff.as_str()), ("Title only", "diff --git a/a b/a\n"));
    assert!(parse_mbox("").unwrap().is_empty());
    assert!(parse_mail("\n\n---\ndiff --git a/a b/a\n").is_err());
}
