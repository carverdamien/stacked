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

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    src: PathBuf,
    db: PathBuf,
}

/// A repo with a base commit and three on top, the last with a title that
/// is not ASCII and long enough to be folded.
fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let src = root.join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    std::fs::write(src.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    git(&src, &["tag", "base"]);
    std::fs::write(src.join("a.txt"), "one\n2\nthree\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - change two"]);
    std::fs::write(src.join("b.txt"), "new\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b\n\nA body.\n\nMore body."]);
    std::fs::write(src.join("a.txt"), "1\n2\nthree\n").unwrap();
    git(
        &src,
        &["commit", "-qam", "Bug 3 - café, a very long title that format-patch will fold over more than one line"],
    );
    let db = root.join("db.jsonl");
    Env { _tmp: tmp, root, src, db }
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
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db.jsonl");
    Db::edit(&path, |db| db.new_stack("s")).unwrap();
    let good = std::fs::read_to_string(&path).unwrap();
    // A write cut short by a crash.
    let torn = format!("{good}{{\"t\":\"stack\",\"na");
    std::fs::write(&path, &torn).unwrap();
    let db = Db::load(&path).unwrap();
    assert_eq!(db.stacks().len(), 1);
    assert_eq!(db.warnings.len(), 1, "{:?}", db.warnings);
    let rev = Db::edit(&path, |db| db.new_stack("t")).unwrap();
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
fn records_only_ever_append() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db.jsonl");
    let d = |m: &str| Draft {
        message: m.into(),
        diff: "diff --git a/x b/x\n".into(),
        author: Some("A <a@b>".into()),
        ..Default::default()
    };
    Db::edit(&path, |db| db.new_stack("s")).unwrap();
    for m in ["one", "two", "three"] {
        let before = std::fs::read(&path).unwrap();
        Db::edit(&path, |db| ops::add(db, "s", d(m), None)).unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(&before));
    }
    Db::edit(&path, |db| ops::move_patch(db, "s", 0, 2)).unwrap();
    Db::edit(&path, |db| db.undo("s")).unwrap();
    let db = Db::load(&path).unwrap();
    assert!(matches!(db.records[0], Record::Stacked { version: 1 }));
    let s = db.stack("s").unwrap();
    let titles: Vec<&str> = s.patches.iter().map(|id| db.patch(id).title()).collect();
    assert_eq!(titles, ["one", "two", "three"]);
}

/// Runs an ask with `claude` standing in for claude, and waits for it.
fn ask(e: &Env, claude: &Path, args: &[&str]) -> String {
    let mut all = vec!["ask", "s"];
    all.extend(args);
    all.push("--wait");
    let (fine, out, err) = stacked_in(&e.src, &e.db, &all, "", &[("STACKED_CLAUDE", claude)]);
    assert!(fine, "{err}");
    out
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

#[test]
fn claude_edits_in_a_worktree_of_its_own() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    e.ok(&["comment", "s", "2", "Please split the body change out."]);
    let before = e.column(2);
    let user_head = git(&e.src, &["rev-parse", "HEAD"]);
    // Squash the top patch into the bottom one, and add one in the middle.
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
        "Done. {\"c1\": \"Split it out.\"}",
    );
    let out = stacked_in(
        &e.src,
        &e.db,
        &["ask", "s", "edit", "squash", "the", "top", "into", "the", "bottom", "--wait"],
        "",
        &[("STACKED_CLAUDE", &claude), ("STACKED_SRC", &e.src)],
    );
    assert!(out.0, "{}", out.2);
    assert!(out.1.contains("done") && out.1.contains("claude's edit is @"), "{}", out.1);
    assert_eq!(e.column(3), ["Bug 1 - change two", "Bug 2 - add b", "Bug 4 - add c"]);
    let after = e.column(2);
    assert_ne!(after[0], before[0], "the bottom patch has a new version");
    assert_eq!(after[1], before[1], "the middle patch is the same");
    assert_eq!(e.ok(&["versions", "s", "1"]).lines().count(), 2);
    // Its reply is a turn of the comment's thread.
    let comments = e.ok(&["comment", "s", "2"]);
    assert!(comments.contains("```claude edit\nSplit it out.\n```"), "{comments}");
    // The user's worktree did not move, and claude's is gone.
    assert_eq!(git(&e.src, &["rev-parse", "HEAD"]), user_head);
    assert_eq!(git(&e.src, &["worktree", "list"]).lines().count(), 1);
    // All of it is in the database: the run and what claude wrote.
    let runs = e.ok(&["runs"]);
    assert!(runs.contains("edit s \"squash the top into the bottom\"\tdone"), "{runs}");
    let id = runs.split('\t').next().unwrap();
    let transcript = e.ok(&["transcript", id]);
    assert!(transcript.contains("● session started · fake") && transcript.contains("exit 0"), "{transcript}");
    // Undo puts back the stack from before the ask.
    e.ok(&["undo", "s"]);
    assert_eq!(e.column(2), before);
}

#[test]
fn an_ask_landing_after_the_stack_moved_is_a_branch() {
    let e = env();
    e.ok(&["import", "s", "base"]);
    let claude = fake_claude(
        &e,
        "claude-slow.sh",
        r#"sleep 1; echo x >> a.txt; git -c user.name=C -c user.email=c@x commit -qam "Bug 3 - café, a very long title that format-patch will fold over more than one line" --amend"#,
        "ok",
    );
    let (fine, id, err) = stacked_in(&e.src, &e.db, &["ask", "s", "edit", "amend"], "", &[("STACKED_CLAUDE", &claude)]);
    assert!(fine, "{err}");
    e.ok(&["drop", "s", "1"]);
    for _ in 0..100 {
        let db = Db::load(&e.db).unwrap();
        if db.run(id.trim()).unwrap().state != RunState::Started {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(e.column(3).len(), 2, "the stack stays where the user took it");
    let tree = e.ok(&["tree", "s"]);
    assert!(tree.contains("claude: amend"), "{tree}");
    assert!(e.ok(&["runs"]).contains("is a branch"));
}

#[test]
fn claude_rebases_reviews_and_tests() {
    let e = env();
    // A commit on main for the stack to go onto.
    git(&e.src, &["checkout", "-q", "-b", "upstream", "base"]);
    std::fs::write(e.src.join("z.txt"), "z\n").unwrap();
    git(&e.src, &["add", "."]);
    git(&e.src, &["commit", "-qm", "upstream"]);
    git(&e.src, &["checkout", "-q", "main"]);
    e.ok(&["import", "s", "base"]);

    let claude = fake_claude(&e, "claude-rebase.sh", "git rebase -q --onto \"$ONTO\" \"$BASE\"", "rebased");
    let onto = git(&e.src, &["rev-parse", "upstream"]).trim().to_string();
    let base = git(&e.src, &["rev-parse", "base"]).trim().to_string();
    let script = e.script(
        "claude-rebase-env.sh",
        &format!("ONTO={onto} BASE={base} exec {} \"$@\"", claude.display()),
    );
    let out = ask(&e, &script, &["rebase", "--onto", "upstream"]);
    assert!(out.contains("done"), "{out}");
    let mail = e.ok(&["export", "s"]);
    let dst = e.root.join("dst");
    git(&e.root, &["clone", "-q", e.src.to_str().unwrap(), "dst"]);
    git(&dst, &["checkout", "-q", &onto]);
    let mut am = Command::new("git").args(["am", "-q"]).current_dir(&dst).stdin(Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    am.stdin.take().unwrap().write_all(mail.as_bytes()).unwrap();
    assert!(am.wait().unwrap().success(), "the stack applies onto upstream");
    assert!(e.ok(&["log", "s"]).contains(&format!("claude: rebase onto {}", &onto[..12])));

    let review = fake_claude(
        &e,
        "claude-review.sh",
        "",
        r#"{"comments": [{"patch": 2, "file": "b.txt", "side": "+", "line": "new", "text": "Say what b is for."},
            {"patch": 1, "file": "nowhere", "side": "+", "line": "?", "text": "On the whole patch."}]}"#,
    );
    let out = ask(&e, &review, &["review"]);
    assert!(out.contains("left 2 comments"), "{out}");
    let c = e.ok(&["comment", "s", "2"]);
    assert!(c.contains("b.txt: new") && c.contains("Say what b is for."), "{c}");
    let c = e.ok(&["comment", "s", "1"]);
    assert!(c.contains("On the whole patch."), "{c}");

    // A test runs where claude likes, here the repo, and reads the stack
    // with the command it is given.
    let test = fake_claude(
        &e,
        "claude-test.sh",
        r#"test "$PWD" = "$STACKED_SRC" || exit 3"#,
        r#"{"patches": {"1": "builds; 3 tests pass", "3": "fails: a.txt"}}"#,
    );
    let out = stacked_in(
        &e.src,
        &e.db,
        &["ask", "s", "test", "--wait"],
        "",
        &[("STACKED_CLAUDE", &test), ("STACKED_SRC", &e.src)],
    );
    assert!(out.1.contains("claude tested 2 patches"), "{} {}", out.1, out.2);
    assert!(e.ok(&["comment", "s", "3"]).contains("fails: a.txt"));
    let db = Db::load(&e.db).unwrap();
    let id = db.runs().last().unwrap().id.clone();
    let info = stacked::ask::info(db.run(&id).unwrap()).unwrap();
    assert!(info.prompt.contains("export s --rev"), "{}", info.prompt);

    // A claude that fails changes nothing but says so.
    let bad = e.script("claude-bad.sh", "echo oops >&2; exit 7");
    let out = ask(&e, &bad, &["edit", "anything"]);
    assert!(out.contains("failed") && out.contains("claude failed (7): oops"), "{out}");
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
    let rows = stacked::rows::patch_rows(&p, &[], 80);
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
    let rows = stacked::rows::patch_rows(p2, &notes, 80);
    let at = rows.iter().position(|r| r.text == "+2").unwrap();
    assert!(rows[at + 1].head && rows[at + 3].text.contains("why 2?"), "{:#?}", &rows[at..at + 4]);
    let (lines, _) = stacked::rows::split_lines(&rows);
    assert!(lines.iter().any(|l| l.left.is_some() && l.right.is_some() && l.left != l.right));
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
