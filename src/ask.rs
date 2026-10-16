//! Asking claude. The user's worktree is never touched.
//!
//! - An edit asks claude to change the stack as it is told: add a patch
//!   anywhere, reorder, merge or split them, or answer the comments by
//!   changing the code.
//! - A rebase asks it to move the stack onto another commit, resolving what
//!   conflicts.
//! - A review asks it to read the stack and say what a reviewer would: its
//!   notes land as comments on the lines they quote.
//! - A test asks it to build and test the patches. Where is claude's call,
//!   by its own rules (CLAUDE.md): it makes a workspace or reuses one. It
//!   runs in the repository and reads the stack with `stacked export`; what
//!   it found lands as a comment on each patch's title.
//!
//! An edit, a rebase or a review gets a git worktree of its own: the
//! stack's base checked out and its patches applied as commits. After an
//! edit or a rebase, the commits are read back as a new stack, edited from
//! the one the ask started from; if the stack has moved on since, that is a
//! branch of its tree and the stack stays where it is.
//!
//! Everything about an ask is in the database: a run record when it starts,
//! all it was started with in it; an event record per line claude writes,
//! appended as it writes them by `stacked record`; and a run record of what
//! came of it, once `stacked land` has filed it. Claude's replies to the
//! comments it was shown land as turns of their threads. It runs in a
//! process group of its own, so quitting the editor leaves it be.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::claude::{allowed_tools, format_event, parse_claude_replies, parse_review_notes};
use crate::db::{Db, Error, Note, Result, Run, RunState, Stack, err, now};
use crate::git;
use crate::mbox::{format_mbox, parse_mbox};
use crate::ops::{base_of, match_drafts};
use crate::thread::{CLAUDE_OPEN, FENCE_CLOSE, Role, append_turn, last_role};
use crate::view::{MSG_PATH, find_line, note_lines, title_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Edit,
    Rebase,
    Review,
    Test,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Edit => "edit",
            Kind::Rebase => "rebase",
            Kind::Review => "review",
            Kind::Test => "test",
        }
    }

    /// Does it get a worktree made for it?
    fn scratch(self) -> bool {
        self != Kind::Test
    }
}

/// A comment claude is shown, by the label the prompt gives it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shown {
    pub label: String,
    pub change: String,
    pub id: String,
}

/// All an ask is started with, kept in its run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    pub kind: Kind,
    /// The stack the ask started from.
    pub from: usize,
    pub base: String,
    /// For a rebase, the commit the stack goes onto.
    #[serde(default)]
    pub onto: Option<String>,
    pub repo: String,
    /// The worktree made for it.
    #[serde(default)]
    pub worktree: Option<String>,
    /// The patch the user was on, by id.
    #[serde(default)]
    pub focus: Option<String>,
    pub notes: Vec<Shown>,
    /// Each commit made in the worktree, and the patch it is.
    pub commits: Vec<(String, String)>,
    pub prompt: String,
}

pub fn info(r: &Run) -> Option<Info> {
    serde_json::from_value(r.ask.clone()?).ok()
}

/// The comments still waiting on an answer: the last word is yours.
fn open_notes<'a>(db: &'a Db, s: &Stack) -> Vec<&'a Note> {
    s.patches
        .iter()
        .flat_map(|id| db.notes(&db.patch(id).change))
        .filter(|n| last_role(&n.text) == Some(Role::You))
        .collect()
}

const IN_SCRATCH: &str = "The current directory is a git worktree made for you: the stack is \
the commits BASE..HEAD, each commit a patch, listed below bottom first.";

const KEEP_TITLES: &str = "Keep each commit's title unless the request is to change it: a patch \
is followed across its versions by its title. When you are done, everything must be committed \
and HEAD must be the top of the stack. You cannot build the project or run its tests; reason \
from the code. Do not push or fetch.";

fn intro(kind: Kind) -> String {
    match kind {
        Kind::Edit => format!(
            "You are editing a stack of patches. {IN_SCRATCH} Change the stack as the request \
             says by rewriting those commits with git: edit files and `git commit --amend`, \
             `git commit --fixup` then `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash BASE`, \
             `git rebase -i` with a GIT_SEQUENCE_EDITOR script to reorder, drop or squash, \
             `git reset` and `git commit` to split, a new commit where a new patch belongs. Do \
             not touch commits below BASE. {KEEP_TITLES}"
        ),
        Kind::Rebase => format!(
            "You are rebasing a stack of patches. {IN_SCRATCH} Move it onto commit ONTO: \
             `git rebase --onto ONTO BASE`, resolving any conflicts so each patch still does \
             what it did, and `git rebase --continue` until it is done. {KEEP_TITLES}"
        ),
        Kind::Review => format!(
            "You are reviewing a stack of patches. {IN_SCRATCH} `git show` each to read it. Do \
             NOT modify anything. You cannot build the project or run its tests; reason from \
             the code. Report what a careful reviewer would point out, one comment per point, \
             anchored to a line of a patch's diff (or to its title for a comment on the patch \
             as a whole), without repeating what the comments below already say."
        ),
        Kind::Test => "You are building and testing a stack of patches. `EXPORT` prints them as \
             mail for `git am`, bottom first as listed below; they apply to commit BASE. Build \
             them and run the tests that matter for what they change, patch by patch where \
             that is worth it. Where to do it is your call, by your own rules for this \
             repository: make a workspace or reuse one. Never use the user's current worktree, \
             nor change a branch of the user's. Do not push."
            .into(),
    }
}

fn prompt(info: &Info, db: &Db, s: &Stack, request: &str, export: &str) -> String {
    let mut p = intro(info.kind)
        .replace("BASE", &info.base)
        .replace("ONTO", info.onto.as_deref().unwrap_or(""))
        .replace("EXPORT", export);
    p.push_str("\n\nThe stack, bottom first:\n");
    for (i, id) in s.patches.iter().enumerate() {
        let sha = info.commits.get(i).map_or("", |(c, _)| &c[..12]);
        p.push_str(&format!("  {} {sha} {}\n", i + 1, db.patch(id).title()));
    }
    if let Some(f) = &info.focus
        && let Some(i) = s.patches.iter().position(|id| id == f)
    {
        p.push_str(&format!("\nThe user is on patch {}: {}\n", i + 1, db.patch(f).title()));
    }
    if !info.notes.is_empty() {
        p.push_str("\nComments on the patches, each a thread, the last word the user's:\n");
        for sh in &info.notes {
            let Some(n) = db.note(&sh.change, &sh.id) else {
                continue;
            };
            let at = s
                .patches
                .iter()
                .position(|id| db.patch(id).change == sh.change)
                .map_or(String::new(), |i| format!("patch {}", i + 1));
            p.push_str(&format!(
                "\n[{}] {at}, {} line `{}`:\n{}\n",
                sh.label, n.file, n.preview, n.text
            ));
        }
    }
    if !request.trim().is_empty() {
        p.push_str(&format!("\nThe request:\n{}\n", request.trim()));
    }
    p.push_str(match info.kind {
        Kind::Edit | Kind::Rebase => {
            "\nEnd your reply with a JSON object mapping the label of each comment you answered \
             ([c1] is \"c1\") to your reply to it, over several short lines."
        }
        Kind::Review => {
            "\nOutput ONLY a JSON object {\"comments\": [{\"patch\": its number, \"file\": the \
             path as the diff names it, \"side\": \"+\" for an added line, \"-\" for a removed \
             one, \" \" for context, \"line\": that line's exact text without its leading \
             +/-/space, \"text\": your comment}], \"replies\": {label: your reply}}, the replies \
             answering any of the comments above you have something to say to."
        }
        Kind::Test => {
            "\nEnd your reply with a JSON object {\"patches\": {its number: what building and \
             testing it found, over several short lines, with the commands run}, \"replies\": \
             {label: your reply}}."
        }
    });
    p
}

/// Claude's lines, stdout and stderr, go to `stacked record`, then a line
/// saying how it exited; then `stacked land` files what it did.
const WRAPPER: &str = r#"trap '' HUP INT
id="$1"; cwd="$2"; exe="$3"; db="$4"; shift 4
cd "$cwd" || exit 1
{ "${STACKED_CLAUDE:-claude}" -p "$("$exe" -f "$db" prompt "$id")" "$@" < /dev/null 2>&1
  echo "{\"type\":\"exit\",\"code\":$?}"; } | "$exe" -f "$db" record "$id"
"$exe" -f "$db" land "$id" > /dev/null 2>&1
"#;

/// What an ask is started with.
pub struct Spec<'a> {
    pub kind: Kind,
    pub stack: &'a str,
    pub request: &'a str,
    pub focus: Option<&'a str>,
    /// For a rebase: the commit to go onto, as git names it.
    pub onto: Option<&'a str>,
}

/// Starts an ask: its worktree made, claude started. Its run's id.
pub fn start(db_path: &Path, spec: &Spec) -> Result<String> {
    let db = Db::load(db_path)?;
    let s = db.stack(spec.stack)?;
    let (rev, _) = db.head(spec.stack).unwrap();
    let (repo, base) = base_of(s)?;
    let onto = match (spec.kind, spec.onto) {
        (Kind::Rebase, Some(o)) => Some(git::rev_parse(&repo, o)?),
        (Kind::Rebase, None) => return err("rebase onto what?"),
        _ => None,
    };
    let id = format!("{}-{}", now(), std::process::id());
    let mut shas = Vec::new();
    let mut worktree = None;
    if spec.kind.scratch() {
        let wt = std::env::temp_dir().join(format!("stacked-{id}"));
        let patches: Vec<_> = s.patches.iter().map(|id| db.patch(id)).collect();
        git::scratch(&repo, &wt, &base, &format_mbox(&patches))?;
        shas = git::git(&wt, &["rev-list", "--reverse", &format!("{base}..HEAD")])?
            .lines()
            .map(String::from)
            .collect();
        if shas.len() != s.patches.len() {
            git::remove_scratch(&repo, &wt);
            return err(format!(
                "{} patches made {} commits: is one empty?",
                s.patches.len(),
                shas.len()
            ));
        }
        worktree = Some(wt.to_string_lossy().into_owned());
    }
    let exe = std::env::current_exe().map_err(|e| Error(e.to_string()))?;
    let db_abs = db_path.canonicalize().map_err(|e| Error(e.to_string()))?;
    let export = format!(
        "{} -f {} export {} --rev {rev}",
        exe.display(),
        db_abs.display(),
        spec.stack
    );
    let mut info = Info {
        kind: spec.kind,
        from: rev,
        base,
        onto,
        repo: repo.to_string_lossy().into_owned(),
        worktree,
        focus: spec.focus.map(String::from),
        notes: open_notes(&db, s)
            .iter()
            .enumerate()
            .map(|(i, n)| Shown {
                label: format!("c{}", i + 1),
                change: n.change.clone(),
                id: n.id.clone(),
            })
            .collect(),
        commits: shas.into_iter().zip(s.patches.iter().cloned()).collect(),
        prompt: String::new(),
    };
    info.prompt = prompt(&info, &db, s, spec.request, &export);
    let mut run = Run {
        id: id.clone(),
        what: spec.kind.name().into(),
        stack: spec.stack.into(),
        state: RunState::Started,
        request: spec.request.into(),
        ask: Some(serde_json::to_value(&info).unwrap()),
        ..Default::default()
    };
    Db::edit(db_path, |db| db.add_run(run.clone()))?;
    let cwd = info.worktree.clone().map_or(repo.clone(), PathBuf::from);
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(WRAPPER).arg("sh").arg(&id).arg(&cwd).arg(&exe).arg(&db_abs);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    match spec.kind {
        Kind::Edit | Kind::Rebase => {
            cmd.args(["--permission-mode", "acceptEdits"]);
            cmd.args(["--allowedTools", &allowed_tools()]);
        }
        Kind::Review => {
            cmd.args(["--permission-mode", "default"]);
            cmd.args([
                "--allowedTools",
                "Read,Grep,Glob,LS,Bash(git show *),Bash(git log *),Bash(git diff *)",
            ]);
        }
        // Building and testing runs what the project needs: Bash at large.
        Kind::Test => {
            cmd.args(["--permission-mode", "acceptEdits"]);
            cmd.args(["--allowedTools", &format!("Bash,{}", allowed_tools())]);
        }
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd.spawn().map_err(|e| Error(format!("sh: {e}")))?;
    run.pid = Some(child.id());
    Db::edit(db_path, |db| {
        // Unless it already landed: claude may be that quick.
        if db.run(&id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run)?;
        }
        Ok(())
    })?;
    Ok(id)
}

/// Appends each line of `input` to run `id`'s events: JSON as it is, any
/// other line as `{"type": "text", "text": ...}`.
pub fn record(db_path: &Path, id: &str, input: impl BufRead) -> Result<()> {
    for line in input.lines() {
        let line = line.map_err(|e| Error(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<Value>(&line)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({"type": "text", "text": line}));
        Db::edit(db_path, |db| db.add_event(id, event).map(|_| ()))?;
    }
    Ok(())
}

/// What claude said last: the `result` of its stream.
fn result_text(events: &[&Value]) -> String {
    events
        .iter()
        .filter(|v| v.get("type").and_then(Value::as_str) == Some("result"))
        .filter_map(|v| v.get("result").and_then(Value::as_str).map(String::from))
        .last()
        .unwrap_or_default()
}

fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

/// The object under `key` in the JSON object `text` ends with.
fn object(text: &str, key: &str) -> Map<String, Value> {
    let (Some(i), Some(j)) = (text.find('{'), text.rfind('}')) else {
        return Map::new();
    };
    if j < i {
        return Map::new();
    }
    serde_json::from_str::<Value>(&text[i..=j])
        .ok()
        .and_then(|v| v.get(key).and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

/// What landing an ask did, before it is a run.
struct Landed {
    ok: bool,
    summary: String,
    rev: Option<usize>,
}

/// Files what a finished ask did; once, however often it is called. `exit`
/// says how claude exited when its events cannot (it was killed).
pub fn land(db_path: &Path, id: &str, exit: Option<&str>) -> Result<Run> {
    let mut wt = None;
    let run = Db::edit(db_path, |db| {
        let Some(started) = db.run(id).cloned() else {
            return err(format!("no run {id}"));
        };
        // Under the lock, so two landings cannot both file it.
        if started.state != RunState::Started {
            return Ok(started);
        }
        let Some(info) = info(&started) else {
            return err(format!("run {id} is not an ask"));
        };
        wt = info.worktree.clone().map(|w| (info.repo.clone(), w));
        let events = db.events(id);
        let code = events
            .iter()
            .rev()
            .find(|e| e.get("type").and_then(Value::as_str) == Some("exit"))
            .map(|e| str_of(e.get("code")))
            .or(exit.map(String::from))
            .unwrap_or_else(|| "unknown".into());
        let output = result_text(&events);
        let landed = if code != "0" {
            let said: Vec<String> = events
                .iter()
                .filter(|e| e.get("type").and_then(Value::as_str) == Some("text"))
                .map(|e| str_of(e.get("text")))
                .collect();
            Landed {
                ok: false,
                summary: format!("claude failed ({code}): {}", said.join("\n").trim()),
                rev: None,
            }
        } else {
            file(db, &started, &info, &output)?
        };
        let run = Run {
            state: if landed.ok { RunState::Done } else { RunState::Failed },
            summary: landed.summary,
            output,
            rev: landed.rev,
            pid: None,
            ..started
        };
        db.add_run(run.clone())?;
        Ok(run)
    })?;
    if run.state == RunState::Done
        && let Some((repo, wt)) = wt
    {
        git::remove_scratch(Path::new(&repo), Path::new(&wt));
    }
    Ok(run)
}

fn file(db: &mut Db, run: &Run, info: &Info, result: &str) -> Result<Landed> {
    let mut summary = Vec::new();
    let mut rev = None;
    let from = db.node(info.from)?.clone();
    let replies = match info.kind {
        Kind::Edit | Kind::Rebase => {
            let wt = PathBuf::from(info.worktree.as_deref().unwrap_or_default());
            let status = git::git(&wt, &["status"])?;
            if git::dirty(&wt)? || status.contains("rebase in progress") {
                return Ok(Landed {
                    ok: false,
                    summary: format!("claude did not finish: see {}", wt.display()),
                    rev: None,
                });
            }
            let base = info.onto.clone().unwrap_or_else(|| info.base.clone());
            let mail = git::git(
                &wt,
                &["format-patch", "--stdout", "--no-signature", "--keep-subject", &format!("{base}..HEAD")],
            )?;
            let drafts = parse_mbox(&mail)?;
            let olds: Vec<_> = from.patches.iter().map(|id| db.patch(id).clone()).collect();
            let by_commit: HashMap<String, String> = info.commits.iter().cloned().collect();
            let patches = match_drafts(db, &olds, drafts, &by_commit)?;
            if patches == from.patches && Some(&base) == from.base.as_ref() {
                summary.push("the stack is unchanged".to_string());
            } else {
                let head = db.head(&run.stack).map(|(r, _)| r);
                let what = match info.kind {
                    Kind::Rebase => format!("rebase onto {}", &base[..12]),
                    _ => short(&run.request, 60),
                };
                let made = db.add_node(Stack {
                    patches,
                    base: Some(base),
                    parent: Some(info.from),
                    op: format!("claude: {what}"),
                    time: now(),
                    ..from
                })?;
                rev = Some(made);
                if head != Some(info.from) {
                    db.move_head_to(&run.stack, head, format!("claude's edit landed as a branch, @{made}"))?;
                    summary.push(format!("the stack moved on since; claude's edit is a branch, @{made}"));
                } else {
                    summary.push(format!("claude's edit is @{made}"));
                }
            }
            parse_claude_replies(result).unwrap_or_default()
        }
        Kind::Review => {
            let notes = parse_review_notes(result).unwrap_or_default();
            let mut filed = 0;
            for (n, note) in notes.iter().enumerate() {
                let at = note
                    .get("patch")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                    .map(|n| n as usize);
                let Some(id) = at.and_then(|i| i.checked_sub(1)).and_then(|i| from.patches.get(i))
                else {
                    continue;
                };
                let p = db.patch(id).clone();
                let file = str_of(note.get("file"));
                let text = str_of(note.get("line"));
                let side = str_of(note.get("side")).chars().next().unwrap_or(' ');
                let (line, file) = match find_line(&note_lines(&p), &file, side, &text) {
                    Some(k) => (k, file),
                    None => (title_key(&p), MSG_PATH.to_string()),
                };
                db.set_note(Note {
                    change: p.change.clone(),
                    id: format!("claude:{}:{n}", run.id),
                    line,
                    text: format!(
                        "{CLAUDE_OPEN} review\n{}\n{FENCE_CLOSE}",
                        str_of(note.get("text")).trim()
                    ),
                    file,
                    preview: text.chars().take(80).collect(),
                    by: Some("claude".into()),
                    remote: vec![],
                    time: now(),
                })?;
                filed += 1;
            }
            summary.push(format!("claude's review left {filed} comments"));
            object(result, "replies")
        }
        Kind::Test => {
            let found = object(result, "patches");
            for (i, id) in from.patches.iter().enumerate() {
                let Some(v) = found.get(&(i + 1).to_string()) else {
                    continue;
                };
                let p = db.patch(id).clone();
                db.set_note(Note {
                    change: p.change.clone(),
                    id: format!("claude:{}:test:{}", run.id, i + 1),
                    line: title_key(&p),
                    text: format!("{CLAUDE_OPEN} test\n{}\n{FENCE_CLOSE}", str_of(Some(v)).trim()),
                    file: MSG_PATH.into(),
                    preview: p.title().chars().take(80).collect(),
                    by: Some("claude".into()),
                    remote: vec![],
                    time: now(),
                })?;
            }
            summary.push(if found.is_empty() {
                format!("claude's test run: {}", short(result, 200))
            } else {
                format!("claude tested {} patches", found.len())
            });
            object(result, "replies")
        }
    };
    let mut answered = 0;
    for sh in &info.notes {
        let Some(reply) = replies.get(&sh.label).map(|v| str_of(Some(v))) else {
            continue;
        };
        let Some(n) = db.note(&sh.change, &sh.id).cloned() else {
            continue;
        };
        db.set_note(Note {
            text: append_turn(&n.text, Role::Claude, &reply, Some(info.kind.name())),
            ..n
        })?;
        answered += 1;
    }
    if answered > 0 {
        summary.push(format!("{answered} comments answered"));
    }
    Ok(Landed {
        ok: true,
        summary: summary.join("; "),
        rev,
    })
}

fn short(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    } else if s.is_empty() {
        "answer the comments".into()
    } else {
        s
    }
}

pub fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Is run `r` still going: started, and its process there?
pub fn running(r: &Run) -> bool {
    r.state == RunState::Started && r.pid.is_some_and(alive)
}

/// Lands every ask whose claude is gone without its wrapper landing it (it
/// was killed); how many.
pub fn land_orphans(db_path: &Path, db: &Db) -> usize {
    let orphans: Vec<String> = db
        .runs()
        .into_iter()
        .filter(|r| r.ask.is_some() && r.state == RunState::Started && r.pid.is_some() && !running(r))
        .map(|r| r.id.clone())
        .collect();
    orphans
        .iter()
        .filter(|id| land(db_path, id, Some("killed")).is_ok())
        .count()
}

/// The last `n` lines of what run `id`'s claude wrote, as a transcript.
pub fn tail(db: &Db, id: &str, n: usize) -> Vec<String> {
    let mut lines: Vec<String> = db
        .events(id)
        .into_iter()
        .flat_map(|v| match v.get("type").and_then(Value::as_str) {
            Some("text") => vec![str_of(v.get("text"))],
            Some("exit") => vec![format!("exit {}", str_of(v.get("code")))],
            _ => format_event(v),
        })
        .collect();
    let keep = lines.len().saturating_sub(n);
    lines.drain(..keep);
    lines
}

/// Stops a running ask: its process group, claude and all.
pub fn stop(r: &Run) {
    if let Some(pid) = r.pid {
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
    }
}
