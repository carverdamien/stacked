//! Asking claude, which is sending it comments. The user's worktree is never
//! touched.
//!
//! A comment is written first, then sent, in one of three ways (`Way`):
//! claude may change the code, answer only, or build and test. Where the
//! comment is sets what claude may change (`Scope`): a comment on the stack,
//! the whole stack; a comment on a patch or on one of its lines, only that
//! patch, unless it is sent to the whole stack. Comments sent together go
//! in one ask.
//!
//! A sent comment waits as ready, and goes as soon as nothing stands in its
//! way, as in self-review: one changing ask at a time on a stack, answer-only
//! asks together, a build and test once no change is running, one at a time.
//! `launch` starts what can go; it runs when a comment is sent and when an
//! ask lands, so what waits goes on its own.
//!
//! A change or an answer gets a git worktree of its own. For the stack, all
//! its patches as commits; for a patch, the stack up to it: the patches below
//! as commits and the patch on top, the patches above to read only. A patch
//! change checks, with `stacked fits`, that the patches above still apply on
//! it; if they would not, claude changes nothing and the comment is promoted
//! to the stack. A build and test runs in the repository, given the stack by
//! `stacked export`, in a workspace of claude's choosing.
//!
//! When claude exits, `stacked land` files what it did: a patch change slots
//! into the stack as it is by then, the comments on lines a split moved
//! following them; replies land as turns of their threads; an answer may open
//! comments of its own within the scope, as a review does. Everything is in
//! the database: the run as it starts, each line claude writes, and what came
//! of it.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::claude::{allowed_tools, format_event, parse_claude_replies, parse_review_notes};
use crate::db::{
    Db, Error, Moved, Note, Patch, Queued, Record, Result, Run, RunState, Stack, Way, err, now,
    stack_change,
};
use crate::git;
use crate::mbox::{format_mbox, parse_mbox};
use crate::ops::{base_of, plan_drafts};
use crate::thread::{AGENT_OPEN, FENCE_CLOSE, Role, append_turn, last_reply_way, last_role, thread_turns};
use crate::view::{MSG_PATH, find_line, mode, note_lines, title_key};

/// What an ask may change: the whole stack, or the patch of this change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Stack,
    Patch(String),
}

/// The scope a comment is in.
pub fn scope_of(n: &Note) -> Scope {
    if n.on_stack() {
        Scope::Stack
    } else {
        Scope::Patch(n.change.clone())
    }
}

/// The scope an ask of `n` has: the one it is in, or the whole stack when
/// it was sent so.
fn asked_scope(n: &Note) -> Scope {
    match &n.queued {
        Some(q) if q.stack => Scope::Stack,
        _ => scope_of(n),
    }
}

/// The scope run `id` was started with, if it is an ask.
pub fn run_scope(db: &Db, id: &str) -> Option<Scope> {
    light(db.run(id)?).map(|l| l.scope)
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
    pub way: Way,
    pub scope: Scope,
    /// The stack the ask started from.
    pub from: usize,
    pub notes: Vec<Shown>,
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub repo: String,
    /// The worktree made for it.
    #[serde(default)]
    pub worktree: Option<String>,
    /// For a patch: its id when the ask started, and where it was.
    #[serde(default)]
    pub patch: Option<(String, usize)>,
    /// Each commit made in the worktree, bottom first, and the patch it is.
    #[serde(default)]
    pub commits: Vec<(String, String)>,
    #[serde(default)]
    pub prompt: String,
}

pub fn info(r: &Run) -> Option<Info> {
    serde_json::from_value(r.ask.clone()?).ok()
}

/// What deciding what goes needs of an ask: read from its run without the
/// rest (its prompt, its commits), as it is read on every redraw.
#[derive(Debug, Clone, Deserialize)]
struct Light {
    way: Way,
    scope: Scope,
    notes: Vec<Shown>,
}

fn light(r: &Run) -> Option<Light> {
    Light::deserialize(r.ask.as_ref()?).ok()
}

/// Whether claude may be sent `n`: the last word is yours or a reviewer's,
/// or claude's why it moved the comment to the stack, where it waits to be
/// sent again.
pub fn sendable(n: &Note) -> bool {
    n.queued.is_none()
        && match last_role(&n.text) {
            Some(Role::Agent) => last_reply_way(&n.text).as_deref() == Some("moved"),
            Some(_) => true,
            None => false,
        }
}

/// Whether `n` may be sent to build and test: a comment on the stack, or on
/// a patch as a whole (its title), not on a line.
pub fn can_test(db: &Db, n: &Note) -> bool {
    n.on_stack()
        || (n.file == MSG_PATH
            && db
                .versions(&n.change)
                .last()
                .is_some_and(|p| title_key(p) == n.line))
}

/// Is run `r` still going: started, and its process there, or about to be?
pub fn busy(r: &Run) -> bool {
    r.state == RunState::Started && r.pid.is_none_or(alive)
}

pub fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Is run `r` running: started, and its process there.
pub fn running(r: &Run) -> bool {
    r.state == RunState::Started
        && match r.pid {
            Some(pid) => alive(pid),
            // Being started: its worktree made.
            None => r.starter.is_some_and(alive),
        }
}

/// The asks of stack `name` still going, with what each was started with.
fn asks_going<'a>(db: &'a Db, name: &str) -> Vec<(&'a Run, Light)> {
    db.runs()
        .into_iter()
        .filter(|r| r.stack == name && busy(r))
        .filter_map(|r| Some((r, light(r)?)))
        .collect()
}

/// The comments the agent is working on, to how it was sent them and the
/// run.
pub fn with_agent(db: &Db) -> HashMap<(String, String), (Way, String)> {
    let mut out = HashMap::new();
    for r in db.runs().into_iter().filter(|r| busy(r)) {
        if let Some(i) = light(r) {
            for n in &i.notes {
                out.insert((n.change.clone(), n.id.clone()), (i.way, r.id.clone()));
            }
        }
    }
    out
}

/// What a comment sent on stack `name` in `way` would wait on, if anything.
pub fn waits_on(db: &Db, name: &str, way: Way) -> Option<String> {
    let going = asks_going(db, name);
    let change = going.iter().find(|(_, i)| i.way == Way::Change);
    if let Some((_, i)) = change {
        let on = match &i.scope {
            Scope::Stack => "the stack".to_string(),
            Scope::Patch(c) => format!("patch {}", position(db, name, c).map_or("?".into(), |p| (p + 1).to_string())),
        };
        return Some(format!("once the agent's change on {on} lands"));
    }
    match way {
        Way::Change if going.iter().any(|(_, i)| i.way == Way::Answer) => {
            Some("once the running answers land".into())
        }
        Way::Test if going.iter().any(|(_, i)| i.way == Way::Test) => {
            Some("once the running build and test lands".into())
        }
        _ => None,
    }
}

/// Where the patch of `change` is in stack `name`, from 0.
fn position(db: &Db, name: &str, change: &str) -> Option<usize> {
    db.stack(name)
        .ok()?
        .patches
        .iter()
        .position(|id| db.patch(id).change == change)
}

/// Sends `notes` (change, id) of stack `name` to the agent, in `way`, to
/// the whole stack if `whole`: they wait as ready, together, and go as soon
/// as nothing stands in their way. What they wait on, if they could not go
/// at once.
pub fn send(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool) -> Result<Option<String>> {
    queue(db_path, name, notes, way, whole)?;
    launch(db_path)?;
    let db = Db::load(db_path)?;
    let waiting = notes
        .iter()
        .any(|(c, i)| db.note(c, i).is_some_and(|n| n.queued.is_some()));
    Ok(if waiting {
        Some(waits_on(&db, name, way).unwrap_or_else(|| "its turn".into()))
    } else {
        None
    })
}

/// Puts `notes` of stack `name` on the agent's queue, in `way`, together:
/// `launch` starts them when nothing stands in their way. The editor
/// launches elsewhere than where it draws: making a worktree takes time.
/// With `whole`, a patch's comments are sent to the whole stack: the agent
/// may change any patch, add patches anywhere, reorder them.
pub fn queue(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to send");
    }
    if whole && way != Way::Change {
        return err("only a change is asked of the whole stack");
    }
    let batch = format!("b{}", crate::db::unique());
    // Checked first with no lock held (it takes a moment on a big
    // repository); under it, the check is remembered.
    if let Ok(db) = Db::load(db_path)
        && let Ok(s) = db.stack(name)
    {
        let _ = db.verify(s);
    }
    Db::edit(db_path, |db| {
        // Claude works on the stack as it applies: one that does not, or
        // cannot be checked, cannot be sent anything.
        let s = db.stack(name)?.clone();
        db.verify(&s)
            .map_err(|e| Error(format!("cannot send: {}", e.0.trim_end_matches("; nothing changed"))))?;
        let busy = with_agent(db);
        let mut scope = None;
        for (change, id) in notes {
            let Some(n) = db.note(change, id).cloned() else {
                return err("no such comment");
            };
            if busy.contains_key(&(change.clone(), id.clone())) {
                return err("the agent is already working on it");
            }
            if way == Way::Test && !can_test(db, &n) {
                return err("only a comment on the stack or on a patch as a whole can be sent to build and test");
            }
            if way != Way::Answer && n.about.is_some() {
                return err("a comment on the history is asked about to answer only: it changes nothing");
            }
            let reach = if whole { Scope::Stack } else { scope_of(&n) };
            match &scope {
                None => scope = Some(reach),
                Some(s) if *s != reach => return err("comments sent together must be on one patch, or all on the stack"),
                _ => {}
            }
            db.set_note(Note {
                queued: Some(Queued {
                    way,
                    batch: batch.clone(),
                    stack: whole && !n.on_stack(),
                }),
                failed: None,
                ..n
            })?;
        }
        Ok(())
    })
}

/// Puts comment (change, id) back on the agent's queue as it was, `q`:
/// taken off while it was edited, and left as it was. Not if it is on the
/// queue again already, or with the agent.
pub fn requeue(db_path: &Path, change: &str, id: &str, q: Queued) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(n) = db.note(change, id).cloned() else { return Ok(()) };
        if n.queued.is_some() || with_agent(db).contains_key(&(change.to_string(), id.to_string())) {
            return Ok(());
        }
        db.set_note(Note { queued: Some(q), ..n })
    })
}

/// Takes `notes` off claude's queue, unsent.
pub fn unsend(db_path: &Path, notes: &[(String, String)]) -> Result<()> {
    Db::edit(db_path, |db| {
        for (c, i) in notes {
            if let Some(n) = db.note(c, i).cloned() {
                db.set_note(Note { queued: None, ..n })?;
            }
        }
        Ok(())
    })
}

/// Starts every ask that may go now; the runs started.
pub fn launch(db_path: &Path) -> Result<Vec<String>> {
    if !db_path.exists() {
        return Ok(vec![]);
    }
    let claimed = Db::edit(db_path, claim)?;
    for id in &claimed {
        if let Err(e) = start(db_path, id) {
            fail_start(db_path, id, &e.0)?;
        }
    }
    Ok(claimed)
}

/// Under the lock: the ready batches that may go now, each a run begun,
/// its comments taken off the queue. Their run ids.
fn claim(db: &mut Db) -> Result<Vec<String>> {
    // The ready batches, oldest first: stack, way, scope, comments.
    let mut order: Vec<String> = Vec::new();
    let mut batches: HashMap<String, (String, Way, Scope, Vec<Note>)> = HashMap::new();
    for s in db.stacks() {
        for n in db.stack_notes(&s.name) {
            let Some(q) = &n.queued else { continue };
            let entry = batches.entry(q.batch.clone()).or_insert_with(|| {
                order.push(q.batch.clone());
                (s.name.clone(), q.way, asked_scope(n), Vec::new())
            });
            entry.3.push(n.clone());
        }
    }
    order.sort();
    let mut started = Vec::new();
    // Per stack: what is going, as the batches are started.
    let mut state: HashMap<String, (bool, usize, bool, bool)> = HashMap::new();
    for b in &order {
        let (name, way, scope, notes) = batches[b].clone();
        let st = state.entry(name.clone()).or_insert_with(|| {
            let going = asks_going(db, &name);
            (
                going.iter().any(|(_, i)| i.way == Way::Change),
                going.iter().filter(|(_, i)| i.way == Way::Answer).count(),
                going.iter().any(|(_, i)| i.way == Way::Test),
                false,
            )
        });
        let (change, answers, test, change_waits) = st;
        let go = match way {
            // A change has the stack to itself; one waiting holds back the
            // answers behind it, so it goes next.
            Way::Change => {
                if !*change && *answers == 0 {
                    *change = true;
                    true
                } else {
                    *change_waits = true;
                    false
                }
            }
            Way::Answer => {
                let go = !*change && !*change_waits;
                if go {
                    *answers += 1;
                }
                go
            }
            Way::Test => {
                let go = !*change && !*test;
                if go {
                    *test = true;
                }
                go
            }
        };
        if !go {
            continue;
        }
        let Some((from, _)) = db.head(&name) else { continue };
        let id = crate::db::unique();
        let shown = notes
            .iter()
            .enumerate()
            .map(|(i, n)| Shown {
                label: format!("c{}", i + 1),
                change: n.change.clone(),
                id: n.id.clone(),
            })
            .collect();
        let info = Info {
            way,
            scope,
            from,
            notes: shown,
            base: String::new(),
            repo: String::new(),
            worktree: None,
            patch: None,
            commits: vec![],
            prompt: String::new(),
        };
        db.add_run(Run {
            id: id.clone(),
            what: way.name().into(),
            stack: name.clone(),
            state: RunState::Started,
            request: notes.first().map(|n| first_line(&n.text)).unwrap_or_default(),
            ask: Some(serde_json::to_value(&info).unwrap()),
            starter: Some(std::process::id()),
            ..Default::default()
        })?;
        for n in notes {
            db.set_note(Note { queued: None, ..n })?;
        }
        started.push(id);
    }
    Ok(started)
}

fn first_line(text: &str) -> String {
    thread_turns(text)
        .last()
        .map(|t| t.body.lines().next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// A run that could not start: failed, and its comments with it.
fn fail_start(db_path: &Path, id: &str, why: &str) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).cloned() else { return Ok(()) };
        if let Some(i) = info(&r) {
            mark_failed(db, &i, why)?;
        }
        db.add_run(Run {
            state: RunState::Failed,
            summary: format!("could not start: {why}"),
            pid: None,
            ..r
        })?;
        Ok(())
    })
}

fn mark_failed(db: &mut Db, i: &Info, why: &str) -> Result<()> {
    for sh in &i.notes {
        if let Some(n) = db.note(&sh.change, &sh.id).cloned() {
            db.set_note(Note {
                failed: Some(why.into()),
                ..n
            })?;
        }
    }
    Ok(())
}

/// A comment's place in the stack, as the prompt says it.
fn place(db: &Db, s: &Stack, n: &Note) -> String {
    if let Some(rev) = n.about {
        return format!("on the stack's history, about @{rev} ({})", n.preview);
    }
    if n.on_stack() {
        return match &n.moved {
            Some(m) => format!("on the stack (moved there from {})", where_was(db, s, m)),
            None => "on the stack".into(),
        };
    }
    let at = s
        .patches
        .iter()
        .position(|id| db.patch(id).change == n.change)
        .map_or("a patch no longer in the stack".to_string(), |i| format!("patch {}", i + 1));
    if n.file == MSG_PATH {
        format!("{at}, its message, line `{}`", n.preview)
    } else {
        format!("{at}, {} line `{}`", n.file, n.preview)
    }
}

fn where_was(db: &Db, s: &Stack, m: &Moved) -> String {
    let at = s
        .patches
        .iter()
        .position(|id| db.patch(id).change == m.change)
        .map_or("a patch".to_string(), |i| format!("patch {}", i + 1));
    format!("{at}, {} line `{}`", m.file, m.preview)
}

/// For comments on the stack's history: the history, oldest first, and
/// each record asked about in full; with the commands that show more.
/// Nothing for others.
fn history_part(db: &Db, i: &Info, s: &Stack, cmd: &str) -> String {
    let revs: Vec<usize> = i
        .notes
        .iter()
        .filter_map(|sh| db.note(&sh.change, &sh.id)?.about)
        .collect();
    if revs.is_empty() {
        return String::new();
    }
    let name = &s.name;
    let mut entries = crate::history::entries(db, name);
    entries.reverse();
    // The latest of a long history.
    let skip = entries.len().saturating_sub(300);
    let mut p = format!(
        "\nThe stack's history, oldest first: each record that changed it (an edit, an undo, a redo, a \
         select), by its rev, and what it changed in short (+ patches added, − dropped, ~ given a new \
         version, ↕ moved){}:\n",
        if skip > 0 { format!(" ({skip} older ones left out)") } else { String::new() }
    );
    for e in &entries[skip..] {
        p.push_str(&format!("  {} {}\n", crate::db::local_time(e.time), e.text));
    }
    p.push_str(
        "\nA record asked about comes with its stack diff: the stack after it, bottom first, each patch \
         by its place and marked (+ added, ~ a new version, what changed in its mail indented under it, \
         ↕ moved, from where it `was`; unmarked, as it was), and each patch dropped, marked −.\n",
    );
    for rev in revs {
        p.push_str(&format!("\nWhat @{rev} was:\n"));
        for l in crate::history::about(db, name, rev).into_iter().take(400) {
            p.push_str(&format!("  {l}\n"));
        }
    }
    p.push_str(&format!(
        "\nMore of it: `{cmd} log {name}` (every edit, and the stack each was made from), \
         `{cmd} tree {name}`, `{cmd} export {name} --rev @N` (the stack as it was at @N, as mail), \
         `{cmd} versions {name} PATCH` and `{cmd} interdiff {name} PATCH --from ID` (a patch's \
         versions, and what changed between them), `{cmd} comments {name}`. PATCH is a position \
         from 1, or an id. Answer about the history as asked; the comments you open go on lines \
         of the stack as it is now.\n"
    ));
    p
}

/// A thread as a conversation, for claude.
fn transcript(text: &str) -> String {
    thread_turns(text)
        .iter()
        .map(|t| {
            let who = match t.role {
                Role::You => "The user".to_string(),
                Role::Agent => "You (the agent, earlier)".to_string(),
                Role::Reviewer => format!("{} (a reviewer, on Phabricator)", t.tag.as_deref().unwrap_or("someone")),
            };
            format!("{who}: {}", t.body)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const KEEP_TITLES: &str = "Keep each commit's title unless asked to change it: a patch is \
followed across its versions by its title. When you are done, everything must be committed \
and HEAD must be the top. You cannot build the project or run its tests here; reason from the \
code. Do not push or fetch, nor stash.";

/// The prompt for an ask: what it may do, the stack, the comments, and
/// what to answer with.
fn prompt(db: &Db, i: &Info, s: &Stack, cmd: &str, run: &str) -> String {
    let n = s.patches.len();
    let k = i.patch.as_ref().map(|(_, k)| *k);
    let export = format!("{cmd} export {} --rev {}", s.name, i.from);
    let mut p = match (i.way, &i.scope) {
        (Way::Change, Scope::Stack) => format!(
            "You are changing a stack of patches, as the comments below ask. The current \
             directory is a git worktree made for you: the stack is the commits {base}..HEAD, \
             each commit a patch, listed below bottom first. You may change any of them: \
             edit files and `git commit --amend`, `git commit --fixup` then \
             `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash {base}`, `git rebase -i` \
             with a GIT_SEQUENCE_EDITOR script to reorder, drop or squash, `git reset` and \
             `git commit` to split, a new commit where a new patch belongs. Do not touch \
             commits below {base}. To rebase the stack onto another commit, `git rebase \
             --onto COMMIT {base}`, and say onto what: \"base\": COMMIT's full sha, in the \
             JSON you end with. {KEEP_TITLES}",
            base = i.base
        ),
        (Way::Change, Scope::Patch(_)) => format!(
            "You are changing one patch of a stack, patch {k} of {n}, as the comments below \
             ask. The current directory is a git worktree made for you: the commits \
             {base}..HEAD are the stack up to that patch, the patches below it and the patch \
             itself on top, at HEAD. You may change only this patch: amend HEAD, or split it \
             into several commits in its place (its title stays on the first). Do not touch \
             the commits below it. The patches above it are not here: you may read them, not \
             change them: `{export}` prints the whole stack as mail. Before you finish, run \
             `{cmd} fits {run}`: it says whether the patches above still apply on your change. \
             If they would not, or if what is asked needs another patch to change too, change \
             nothing: `git reset --hard {orig}` puts the patch back as it was, and answer \
             with \"promote\" saying why, so the comment goes to the stack. {KEEP_TITLES}",
            k = k.map_or(0, |k| k + 1),
            base = i.base,
            orig = i.commits.last().map_or("HEAD", |c| c.0.as_str())
        ),
        (Way::Answer, Scope::Stack) => format!(
            "You are answering comments on a stack of patches. The current directory is a git \
             worktree made for you: the stack is the commits {base}..HEAD, each commit a \
             patch, listed below bottom first; `git show` each to read it. Do NOT modify \
             anything. You cannot build the project or run its tests; reason from the code. \
             Besides your answers, you may open comments of your own on lines of any patch, \
             as a careful reviewer would, without repeating what is already said.",
            base = i.base
        ),
        (Way::Answer, Scope::Patch(_)) => format!(
            "You are answering comments on one patch of a stack, patch {k} of {n}. The current \
             directory is a git worktree made for you: the commits {base}..HEAD are the stack \
             up to that patch, the patch itself at HEAD; `git show` to read them; `{export}` \
             prints the whole stack, the patches above included. Do NOT modify anything. You \
             cannot build the project or run its tests; reason from the code. Besides your \
             answers, you may open comments of your own on lines of this patch, as a careful \
             reviewer would, without repeating what is already said.",
            k = k.map_or(0, |k| k + 1),
            base = i.base
        ),
        (Way::Test, scope) => {
            let what = match scope {
                Scope::Stack => "the stack's patches".to_string(),
                Scope::Patch(_) => format!("patch {} (with the patches below it)", k.map_or(0, |k| k + 1)),
            };
            format!(
                "You are building and testing {what}. `{export}` prints the patches as mail for \
                 `git am`, bottom first as listed below; they apply to commit {base}. Build them \
                 and run the tests that matter for what they change. Where to do it is your \
                 call, by your own rules for this repository: make a workspace or reuse one. \
                 Never use the user's current worktree, nor change a branch of the user's. Do \
                 not push. You change nothing in the stack: what you find is your answer.",
                base = i.base
            )
        }
    };
    p.push_str("\n\nThe stack, bottom first:\n");
    for (at, id) in s.patches.iter().enumerate() {
        let mark = if Some(at) == k { " ← this patch" } else { "" };
        p.push_str(&format!("  {} {}{mark}\n", at + 1, db.patch(id).title()));
    }
    p.push_str("\nThe comments, each a thread, the last word the user's or a reviewer's:\n");
    for sh in &i.notes {
        let Some(n) = db.note(&sh.change, &sh.id) else { continue };
        p.push_str(&format!("\n[{}] {}:\n{}\n", sh.label, place(db, s, n), transcript(&n.text)));
    }
    p.push_str(&history_part(db, i, s, cmd));
    p.push_str(match i.way {
        Way::Change => match i.scope {
            Scope::Stack => {
                "\nEnd your reply with a JSON object: {\"replies\": {label: your reply to that \
                 comment, saying what you changed}}, labels as above ([c1] is \"c1\"); with \
                 \"base\": the commit's sha, when you rebased the stack onto another."
            }
            Scope::Patch(_) => {
                "\nEnd your reply with a JSON object: {\"replies\": {label: your reply to that \
                 comment, saying what you changed}}, labels as above ([c1] is \"c1\"); with \
                 \"promote\": why, when the comments need more than this patch."
            }
        },
        Way::Answer => {
            "\nOutput ONLY a JSON object: {\"replies\": {label: your answer}, \"comments\": \
             [{\"patch\": its number, \"file\": the path as the diff names it, \"side\": \"+\" \
             for an added line, \"-\" for a removed one, \" \" for context, \"line\": that \
             line's exact text without its leading +/-/space, \"text\": your comment}]}, the \
             comments optional."
        }
        Way::Test => {
            "\nEnd your reply with a JSON object: {\"replies\": {label: what you found, with \
             the commands run}, \"patches\": {its number: what building and testing it found}}."
        }
    });
    p
}

/// The program run as claude, when not `$STACKED_CLAUDE`'s or `claude`,
/// and the stacked an ask calls back (`prompt`, `record`, `land`), when not
/// this one: the editor's tests set them here, as they run the editor in a
/// process of their own and cannot each set the environment they share.
static CLAUDE: std::sync::Mutex<Option<(PathBuf, PathBuf)>> = std::sync::Mutex::new(None);

/// Runs `claude` as claude from now on, the asks calling `stacked` back.
pub fn stand_in(claude: &Path, stacked: &Path) {
    *CLAUDE.lock().unwrap() = Some((claude.to_path_buf(), stacked.to_path_buf()));
}

/// Has the git commands `c` runs make commits and tags unsigned, whatever
/// the user's git config says: settings added to those of `GIT_CONFIG_COUNT`.
fn unsigned(c: &mut Command) {
    let n: usize = std::env::var("GIT_CONFIG_COUNT").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    c.env("GIT_CONFIG_COUNT", (n + 2).to_string());
    for (k, (key, value)) in [("commit.gpgsign", "false"), ("tag.gpgsign", "false")].into_iter().enumerate() {
        c.env(format!("GIT_CONFIG_KEY_{}", n + k), key);
        c.env(format!("GIT_CONFIG_VALUE_{}", n + k), value);
    }
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

/// Starts claimed run `id`: its worktree made, its prompt written, claude
/// started.
fn start(db_path: &Path, id: &str) -> Result<()> {
    let db = Db::load(db_path)?;
    let run = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let mut i = info(&run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    let s = db.node(i.from)?.clone();
    let (repo, base) = base_of(&s)?;
    i.base = base.clone();
    i.repo = repo.to_string_lossy().into_owned();
    let upto = match &i.scope {
        Scope::Stack => s.patches.len(),
        Scope::Patch(c) => {
            let k = s
                .patches
                .iter()
                .position(|p| db.patch(p).change == *c)
                .ok_or_else(|| Error("its patch is no longer in the stack".into()))?;
            i.patch = Some((s.patches[k].clone(), k));
            k + 1
        }
    };
    if i.way != Way::Test {
        let wt = crate::config::get().worktree(id);
        let patches: Vec<&Patch> = s.patches[..upto].iter().map(|p| db.patch(p)).collect();
        git::scratch(&repo, &wt, &base, &format_mbox(&patches))?;
        let shas: Vec<String> = git::git(&wt, &["rev-list", "--reverse", &format!("{base}..HEAD")])?
            .lines()
            .map(String::from)
            .collect();
        if shas.len() != upto {
            git::remove_scratch(&repo, &wt);
            return err(format!("{upto} patches made {} commits: is one empty?", shas.len()));
        }
        i.commits = shas.into_iter().zip(s.patches[..upto].iter().cloned()).collect();
        i.worktree = Some(wt.to_string_lossy().into_owned());
    }
    let exe = match CLAUDE.lock().unwrap().clone() {
        Some((_, stacked)) => stacked,
        None => std::env::current_exe().map_err(|e| Error(e.to_string()))?,
    };
    let db_abs = db_path.canonicalize().map_err(|e| Error(e.to_string()))?;
    let cmd = format!("{} -f {}", exe.display(), db_abs.display());
    i.prompt = prompt(&db, &i, &s, &cmd, id);
    let cwd = i.worktree.clone().map_or(repo.clone(), PathBuf::from);
    let tools = match i.way {
        Way::Change => format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools()),
        Way::Answer => format!(
            "Read,Grep,Glob,LS,Bash(git show *),Bash(git log *),Bash(git diff *),Bash({cmd} export *),\
             Bash({cmd} log *),Bash({cmd} tree *),Bash({cmd} versions *),Bash({cmd} interdiff *),\
             Bash({cmd} show *),Bash({cmd} comments *)"
        ),
        // Building and testing runs what the project needs: Bash at large.
        Way::Test => format!("Bash,{}", allowed_tools()),
    };
    let permission = match i.way {
        Way::Answer => "default",
        _ => "acceptEdits",
    };
    let mut run = Run {
        ask: Some(serde_json::to_value(&i).unwrap()),
        ..run
    };
    let landed = Db::edit(db_path, |db| {
        // Unless it was landed meanwhile, taken for one left behind.
        if db.run(id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run.clone())?;
            Ok(false)
        } else {
            Ok(true)
        }
    })?;
    if landed {
        if let Some(wt) = &i.worktree {
            git::remove_scratch(&repo, Path::new(wt));
        }
        return err("it was landed while its worktree was made");
    }
    let mut c = Command::new("sh");
    c.arg("-c").arg(WRAPPER).arg("sh").arg(id).arg(&cwd).arg(&exe).arg(&db_abs);
    c.args(["--output-format", "stream-json", "--verbose"]);
    c.args(["--permission-mode", permission, "--allowedTools", &tools]);
    // What the settings add: a model, arguments of the user's.
    c.args(crate::config::get().agent_args());
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // The program run as claude: the tests' stand-in, else $STACKED_CLAUDE,
    // the settings' or claude.
    let claude = match CLAUDE.lock().unwrap().clone() {
        Some((claude, _)) => claude.to_string_lossy().into_owned(),
        None => crate::config::get().agent_command(),
    };
    c.env("STACKED_CLAUDE", claude);
    // The agent's commits are read back as patches, then gone: not signed,
    // which would want the user's key for each.
    unsigned(&mut c);
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let child = c.spawn().map_err(|e| Error(format!("sh: {e}")))?;
    run.pid = Some(child.id());
    Db::edit(db_path, |db| {
        // Unless it already landed: claude may be that quick.
        if db.run(id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run)?;
        }
        Ok(())
    })
}

/// Appends each line of `input` to run `id`'s events as it comes: JSON as
/// it is, any other line as `{"type": "text", "text": ...}`.
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
        Db::append(db_path, &[Record::Event { run: id.into(), event }])?;
    }
    Ok(())
}

/// How run `id`'s agent exited: as its stream says, else `exit` (it was
/// killed), else unknown.
fn exit_code(db: &Db, id: &str, exit: Option<&str>) -> String {
    db.events(id)
        .iter()
        .rev()
        .find(|e| e.get("type").and_then(Value::as_str) == Some("exit"))
        .map(|e| str_of(e.get("code")))
        .or(exit.map(String::from))
        .unwrap_or_else(|| "unknown".into())
}

/// What the agent said last: the `result` of its stream.
fn result_text(events: &[&Value]) -> String {
    events
        .iter()
        .rev()
        .filter(|v| v.get("type").and_then(Value::as_str) == Some("result"))
        .find_map(|v| v.get("result").and_then(Value::as_str).map(String::from))
        .unwrap_or_default()
}

fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

/// The JSON object `text` ends with.
fn last_object(text: &str) -> Option<Map<String, Value>> {
    let (i, j) = (text.find('{')?, text.rfind('}')?);
    if j < i {
        return None;
    }
    serde_json::from_str::<Value>(&text[i..=j]).ok()?.as_object().cloned()
}

/// What claude replied to each comment: `replies`, else labels at the top.
fn replies_of(text: &str) -> Map<String, Value> {
    match last_object(text).and_then(|o| o.get("replies").and_then(Value::as_object).cloned()) {
        Some(r) => r,
        None => parse_claude_replies(text).unwrap_or_default(),
    }
}

/// What landing an ask did, before it is a run.
struct Landed {
    ok: bool,
    summary: String,
    rev: Option<usize>,
    /// Leave the worktree made for it: it holds work not filed.
    keep: bool,
}

impl Landed {
    fn done(summary: impl Into<String>, rev: Option<usize>) -> Landed {
        Landed {
            ok: true,
            summary: summary.into(),
            rev,
            keep: false,
        }
    }

    fn failed(summary: impl Into<String>, keep: bool) -> Landed {
        Landed {
            ok: false,
            summary: summary.into(),
            rev: None,
            keep,
        }
    }
}

/// Files what a finished ask did; once, however often it is called. `exit`
/// says how claude exited when its events cannot (it was killed). Then
/// starts what was waiting behind it.
pub fn land(db_path: &Path, id: &str, exit: Option<&str>) -> Result<Run> {
    // What landing does is done once first, on a copy, with no lock held:
    // what takes long (the stack made checked to apply, on a big
    // repository) is then remembered, and under the lock it is only
    // written. Nothing of the copy is written.
    if let Ok(mut dry) = Db::load(db_path)
        && let Some(started) = dry.run(id).cloned()
        && started.state == RunState::Started
        && let Some(i) = info(&started)
        && exit_code(&dry, id, exit) == "0"
    {
        let output = result_text(&dry.events(id));
        let _ = file(&mut dry, &started, &i, &output);
    }
    let mut wt = None;
    let run = Db::edit(db_path, |db| {
        let Some(started) = db.run(id).cloned() else {
            return err(format!("no run {id}"));
        };
        // Under the lock, so two landings cannot both file it.
        if started.state != RunState::Started {
            return Ok(started);
        }
        let Some(i) = info(&started) else {
            return err(format!("run {id} is not an ask"));
        };
        wt = i.worktree.clone().map(|w| (i.repo.clone(), w));
        let events = db.events(id);
        let code = exit_code(db, id, exit);
        let output = result_text(&events);
        let landed = if code != "0" {
            let said: Vec<String> = events
                .iter()
                .filter(|e| e.get("type").and_then(Value::as_str) == Some("text"))
                .map(|e| str_of(e.get("text")))
                .collect();
            let keep = i
                .worktree
                .as_deref()
                .is_some_and(|w| git::unfinished(Path::new(w)).unwrap_or(false));
            let mut summary = format!("the agent failed ({code}): {}", said.join("\n").trim());
            if keep {
                summary += &format!("; its work is left in {}", i.worktree.as_deref().unwrap());
            }
            Landed::failed(summary, keep)
        } else {
            // Whatever goes wrong filing it is what came of it: a run left
            // started would be landed again and again.
            file(db, &started, &i, &output).unwrap_or_else(|e| {
                let at = i.worktree.as_deref().map_or(String::new(), |w| format!("; its work is left in {w}"));
                Landed::failed(format!("could not file what the agent did: {}{at}", e.0), true)
            })
        };
        if !landed.ok {
            mark_failed(db, &i, &landed.summary)?;
        }
        if landed.keep {
            wt = None;
        }
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
    if let Some((repo, wt)) = wt {
        git::remove_scratch(Path::new(&repo), Path::new(&wt));
    }
    launch(db_path)?;
    Ok(run)
}

/// Adds claude's replies to the comments it was sent, as turns `way` names.
fn reply(db: &mut Db, i: &Info, replies: &Map<String, Value>) -> Result<usize> {
    let mut answered = 0;
    for sh in &i.notes {
        let Some(text) = replies.get(&sh.label).map(|v| str_of(Some(v))) else {
            continue;
        };
        let Some(n) = db.note(&sh.change, &sh.id).cloned() else { continue };
        db.set_note(Note {
            text: append_turn(&n.text, Role::Agent, &text, Some(i.way.name())),
            failed: None,
            ..n
        })?;
        answered += 1;
    }
    Ok(answered)
}

fn file(db: &mut Db, run: &Run, i: &Info, result: &str) -> Result<Landed> {
    let replies = replies_of(result);
    match (i.way, &i.scope) {
        (Way::Change, Scope::Patch(change)) => land_patch(db, run, i, change, result, &replies),
        (Way::Change, Scope::Stack) => land_stack(db, run, i, result, &replies),
        (Way::Answer, _) => {
            let opened = open_comments(db, run, i, result)?;
            let answered = reply(db, i, &replies)?;
            let mut said = format!("the agent answered {answered} comment{}", plural(answered));
            if opened > 0 {
                said += &format!(" and opened {opened}");
            }
            Ok(Landed::done(said, None))
        }
        (Way::Test, _) => {
            let tested = file_tests(db, run, i, result)?;
            reply(db, i, &replies)?;
            Ok(Landed::done(format!("the agent tested @{}: {tested} patch{}", i.from, if tested == 1 { "" } else { "es" }), None))
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A patch change landing: the patch's new version, or the pieces of it,
/// in its place in the stack as it is now; or, when claude said it needs
/// more than the patch, the comments promoted to the stack.
fn land_patch(
    db: &mut Db,
    run: &Run,
    i: &Info,
    change: &str,
    result: &str,
    replies: &Map<String, Value>,
) -> Result<Landed> {
    let (old_id, k) = i.patch.clone().ok_or_else(|| Error("no patch".into()))?;
    if let Some(why) = last_object(result).and_then(|o| o.get("promote").map(|v| str_of(Some(v)))).filter(|w| !w.is_empty()) {
        reply(db, i, replies)?;
        let mut moved = 0;
        for sh in &i.notes {
            if let Some(n) = db.note(&sh.change, &sh.id).cloned() {
                promote(db, &run.stack, &n, &why)?;
                moved += 1;
            }
        }
        return Ok(Landed::done(
            format!("moved {moved} comment{} to the stack: {why}", plural(moved)),
            None,
        ));
    }
    let wt = PathBuf::from(i.worktree.as_deref().unwrap_or_default());
    if git::unfinished(&wt)? {
        return Ok(Landed::failed(
            format!("the agent left work uncommitted, or a rebase half done: it is left in {}", wt.display()),
            true,
        ));
    }
    let below: Vec<&str> = i.commits[..k].iter().map(|c| c.0.as_str()).collect();
    let parent = below.last().copied().unwrap_or(i.base.as_str()).to_string();
    let now_below = git::git(&wt, &["rev-list", "--reverse", &format!("{}..{parent}", i.base)]).unwrap_or_default();
    if now_below.lines().collect::<Vec<_>>() != below {
        return Ok(Landed::failed(format!("the agent changed the patches below patch {}", k + 1), true));
    }
    let pieces = pieces_since(db, &wt, &parent, &old_id, i)?;
    if pieces.is_empty() {
        return Ok(Landed::failed(format!("nothing is left of patch {}", k + 1), true));
    }
    let s = db.stack(&run.stack)?.clone();
    let Some(at) = s.patches.iter().position(|p| *p == old_id) else {
        return Ok(Landed::failed(
            format!("patch {} changed while the agent worked on it", k + 1),
            true,
        ));
    };
    let ids: Vec<String> = pieces.iter().map(|p| p.id.clone()).collect();
    if ids == [old_id.clone()] {
        let answered = reply(db, i, replies)?;
        return Ok(Landed::done(
            format!("the agent changed nothing; it answered {answered} comment{}", plural(answered)),
            None,
        ));
    }
    let mut patches = s.patches.clone();
    patches.splice(at..=at, ids.iter().cloned());
    for p in &pieces {
        db.add_patch(p.clone())?;
    }
    let what = match pieces.len() {
        1 => format!("agent: patch {}", at + 1),
        n => format!("agent: patch {} split in {n}", at + 1),
    };
    let rev = db.set_stack(&run.stack, patches, &what)?;
    // Replied to where they are, then the comments go with their lines.
    let answered = reply(db, i, replies)?;
    let olds = [db.patch(&old_id).clone()];
    follow_lines(db, &olds, &ids)?;
    let _ = change;
    Ok(Landed::done(
        format!("{what}, @{rev}; {answered} comment{} answered", plural(answered)),
        Some(rev),
    ))
}

/// The patches the commits after `parent` in `wt` are: the first, or the
/// one keeping its title, a new version of `old_id`; the rest new.
fn pieces_since(db: &Db, wt: &Path, parent: &str, old_id: &str, i: &Info) -> Result<Vec<Patch>> {
    let mail = git::git(
        wt,
        &["format-patch", "--stdout", "--no-signature", "--keep-subject", &format!("{parent}..HEAD")],
    )?;
    let drafts = parse_mbox(&mail)?;
    let by_commit: HashMap<String, String> = i.commits.iter().cloned().collect();
    Ok(plan_drafts(&[db.patch(old_id).clone()], drafts, &by_commit))
}

/// A stack change landing: the commits read back as a stack made from the
/// one the ask started from. If the stack moved on meanwhile, it is a branch
/// of the tree, and the stack stays where it was taken.
fn land_stack(db: &mut Db, run: &Run, i: &Info, result: &str, replies: &Map<String, Value>) -> Result<Landed> {
    let wt = PathBuf::from(i.worktree.as_deref().unwrap_or_default());
    if git::unfinished(&wt)? {
        return Ok(Landed::failed(
            format!("the agent left work uncommitted, or a rebase half done: it is left in {}", wt.display()),
            true,
        ));
    }
    let head = db.head(&run.stack).map(|(r, _)| r);
    let from = db.node(i.from)?.clone();
    // Rebased, the stack is on the commit claude says; else on its base,
    // which must still be under it.
    let said = last_object(result).and_then(|o| o.get("base").map(|v| str_of(Some(v)))).filter(|b| !b.is_empty());
    let base = match &said {
        Some(b) => match git::git(&wt, &["rev-parse", "--verify", "--quiet", &format!("{b}^{{commit}}")]) {
            Ok(sha) => sha.trim().to_string(),
            Err(_) => return Ok(Landed::failed(format!("the agent rebased onto {b}, which is not a commit"), true)),
        },
        None => i.base.clone(),
    };
    if git::git(&wt, &["merge-base", "--is-ancestor", &base, "HEAD"]).is_err() {
        return Ok(Landed::failed(
            format!("the stack is no longer on {}, and the agent did not say onto what it went", &base[..12.min(base.len())]),
            true,
        ));
    }
    let mail = git::git(
        &wt,
        &["format-patch", "--stdout", "--no-signature", "--keep-subject", &format!("{base}..HEAD")],
    )?;
    let drafts = parse_mbox(&mail)?;
    let olds: Vec<Patch> = from.patches.iter().map(|id| db.patch(id).clone()).collect();
    let by_commit: HashMap<String, String> = i.commits.iter().cloned().collect();
    let planned = plan_drafts(&olds, drafts, &by_commit);
    let ids: Vec<String> = planned.iter().map(|p| p.id.clone()).collect();
    let answered;
    if ids == from.patches && base == i.base {
        answered = reply(db, i, replies)?;
        return Ok(Landed::done(
            format!("the agent changed nothing; it answered {answered} comment{}", plural(answered)),
            None,
        ));
    }
    let next = Stack {
        patches: ids.clone(),
        base: Some(base.clone()),
        parent: Some(i.from),
        op: if base == i.base {
            "agent: the stack".into()
        } else {
            format!("agent: rebase onto {}", &base[..12])
        },
        time: now(),
        ..from.clone()
    };
    crate::check::stack(&next, &planned.iter().collect::<Vec<_>>())?;
    for p in &planned {
        db.add_patch(p.clone())?;
    }
    let rev = db.add_node(next)?;
    let branch = head != Some(i.from);
    if branch {
        db.restore_head(&run.stack, head, format!("the agent's edit @{rev} is a branch"))?;
    }
    answered = reply(db, i, replies)?;
    follow_lines(db, &olds, &ids)?;
    let mut said = format!("the agent changed the stack, @{rev}; {answered} comment{} answered", plural(answered));
    if branch {
        said += "; the stack had moved on, so it is a branch: the tree of edits has both";
    }
    Ok(Landed::done(said, Some(rev)))
}

/// The comments on lines that their own change's version in `now` no
/// longer has go to the patch of `now` that has their line, as after a
/// split or a merge. How many moved.
fn follow_lines(db: &mut Db, olds: &[Patch], now: &[String]) -> Result<usize> {
    let patches: Vec<Patch> = now.iter().map(|id| db.patch(id).clone()).collect();
    let keys: Vec<(String, HashSet<String>)> = patches
        .iter()
        .map(|p| (p.change.clone(), note_lines(p).into_iter().map(|l| l.key).collect()))
        .collect();
    let mut moved = 0;
    for old in olds {
        let own = keys.iter().find(|(c, _)| *c == old.change);
        let notes: Vec<Note> = db.notes(&old.change).into_iter().cloned().collect();
        for n in notes {
            if n.line.is_empty() || own.is_some_and(|(_, k)| k.contains(&n.line)) {
                continue;
            }
            // A key is the change's, then the line's: put it under each other.
            let Some((_, rest)) = n.line.split_once('\0') else { continue };
            for (change, k) in &keys {
                if *change == old.change {
                    continue;
                }
                let key = format!("{}\0{rest}", mode(change));
                if k.contains(&key) {
                    let id = if n.id == n.line { key.clone() } else { n.id.clone() };
                    db.set_note(Note {
                        change: change.clone(),
                        id,
                        line: key,
                        ..n.clone()
                    })?;
                    db.set_note(Note {
                        text: String::new(),
                        ..n.clone()
                    })?;
                    moved += 1;
                    break;
                }
            }
        }
    }
    Ok(moved)
}

/// Moves comment `n` to stack `name`'s own comments, `why` claude's last
/// turn: it needs more than its patch. It waits there to be sent again.
pub fn promote(db: &mut Db, name: &str, n: &Note, why: &str) -> Result<()> {
    let moved = Moved {
        change: n.change.clone(),
        id: n.id.clone(),
        line: n.line.clone(),
        file: n.file.clone(),
        preview: n.preview.clone(),
    };
    db.set_note(Note {
        change: stack_change(name),
        id: format!("moved:{}", crate::db::short_id(&n.change, &n.id)),
        line: String::new(),
        text: append_turn(&n.text, Role::Agent, &format!("This needs more than the patch: {why}"), Some("moved")),
        moved: Some(moved),
        queued: None,
        failed: None,
        ..n.clone()
    })?;
    db.set_note(Note {
        text: String::new(),
        ..n.clone()
    })
}

/// Moves a promoted comment back where it came from.
pub fn move_back(db: &mut Db, n: &Note) -> Result<()> {
    let Some(m) = n.moved.clone() else {
        return err("it was not moved");
    };
    db.set_note(Note {
        change: m.change,
        id: m.id,
        line: m.line,
        file: m.file,
        preview: m.preview,
        moved: None,
        queued: None,
        failed: None,
        ..n.clone()
    })?;
    db.set_note(Note {
        text: String::new(),
        ..n.clone()
    })
}

/// The comments an answer opened, filed: on the lines they quote, within
/// the ask's scope, each opened by the comment that asked. How many.
fn open_comments(db: &mut Db, run: &Run, i: &Info, result: &str) -> Result<usize> {
    let notes = parse_review_notes(result).unwrap_or_default();
    let from = db.node(i.from)?.clone();
    let asked = i.notes.first().map(|n| (n.change.clone(), n.id.clone()));
    let mut filed = 0;
    for (n, note) in notes.iter().enumerate() {
        let at = note
            .get("patch")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .map(|n| n as usize);
        let Some(id) = at.and_then(|a| a.checked_sub(1)).and_then(|a| from.patches.get(a)) else {
            continue;
        };
        let p = db.patch(id).clone();
        if let Scope::Patch(c) = &i.scope
            && *c != p.change
        {
            continue;
        }
        let file = str_of(note.get("file"));
        let text = str_of(note.get("line"));
        let side = str_of(note.get("side")).chars().next().unwrap_or(' ');
        let (line, file) = match find_line(&note_lines(&p), &file, side, &text) {
            Some(k) => (k, file),
            None => (title_key(&p), MSG_PATH.to_string()),
        };
        db.set_note(Note {
            change: p.change.clone(),
            id: format!("agent:{}:{n}", run.id),
            line,
            text: format!("{AGENT_OPEN} answer\n{}\n{FENCE_CLOSE}", str_of(note.get("text")).trim()),
            file,
            preview: text.chars().take(80).collect(),
            by: Some("agent".into()),
            opened_by: asked.clone(),
            time: now(),
            ..Default::default()
        })?;
        filed += 1;
    }
    Ok(filed)
}

/// A build and test's findings, filed on each patch's title, opened by the
/// comment that asked. How many patches.
fn file_tests(db: &mut Db, run: &Run, i: &Info, result: &str) -> Result<usize> {
    let found = last_object(result)
        .and_then(|o| o.get("patches").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    let from = db.node(i.from)?.clone();
    let asked = i.notes.first().map(|n| (n.change.clone(), n.id.clone()));
    let mut tested = 0;
    for (at, id) in from.patches.iter().enumerate() {
        let Some(v) = found.get(&(at + 1).to_string()) else { continue };
        let p = db.patch(id).clone();
        if let Scope::Patch(c) = &i.scope
            && *c != p.change
        {
            continue;
        }
        db.set_note(Note {
            change: p.change.clone(),
            id: format!("agent:{}:test:{}", run.id, at + 1),
            line: title_key(&p),
            text: format!("{AGENT_OPEN} test\n{} (tested @{})\n{FENCE_CLOSE}", str_of(Some(v)).trim(), i.from),
            file: MSG_PATH.into(),
            preview: p.title().chars().take(80).collect(),
            by: Some("agent".into()),
            opened_by: asked.clone(),
            time: now(),
            ..Default::default()
        })?;
        tested += 1;
    }
    Ok(tested)
}

/// For claude, while it changes a patch: whether the patches above still
/// apply on what is in its worktree now.
pub fn fits(db_path: &Path, id: &str) -> Result<String> {
    let db = Db::load(db_path)?;
    let run = db.run(id).ok_or_else(|| Error(format!("no run {id}")))?;
    let i = info(run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    let (Some((old_id, k)), Some(wt)) = (i.patch.clone(), i.worktree.clone()) else {
        return err("this ask is not changing a patch");
    };
    let wt = PathBuf::from(wt);
    let parent = i.commits[..k].last().map_or(i.base.clone(), |c| c.0.clone());
    let pieces = pieces_since(&db, &wt, &parent, &old_id, &i)?;
    let from = db.node(i.from)?;
    let mut patches: Vec<Patch> = from.patches[..k].iter().map(|p| db.patch(p).clone()).collect();
    patches.extend(pieces);
    patches.extend(from.patches[k + 1..].iter().map(|p| db.patch(p).clone()));
    crate::check::stack(from, &patches.iter().collect::<Vec<_>>())
        .map(|_| "fits: the patches above still apply on this change".into())
        .map_err(|e| Error(format!("does not fit: {}", e.0.trim_end_matches("; nothing changed"))))
}

/// Lands every ask whose claude is gone without its wrapper landing it (it
/// was killed), or that never started; how many.
pub fn land_orphans(db_path: &Path, db: &Db) -> usize {
    let stale: Vec<String> = db
        .runs()
        .into_iter()
        .filter(|r| r.ask.is_some() && r.state == RunState::Started)
        .filter(|r| match (r.pid, r.starter) {
            (Some(pid), _) => !alive(pid),
            (None, Some(starter)) => !alive(starter),
            (None, None) => now() - r.time > 120,
        })
        .map(|r| r.id.clone())
        .collect();
    stale
        .iter()
        .filter(|id| land(db_path, id, Some("killed")).is_ok())
        .count()
}

/// The last `n` lines of what run `id`'s agent wrote, as a transcript:
/// read from the end, as it is on every redraw while the agent writes.
pub fn tail(db: &Db, id: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Its thinking counted as it goes: of the counts in a row, the last.
    let mut counted = false;
    for v in db.events(id).into_iter().rev() {
        if out.len() >= n {
            break;
        }
        let count = v.get("type").and_then(Value::as_str) == Some("system")
            && v.get("subtype").and_then(Value::as_str) == Some("thinking_tokens");
        if count && counted {
            continue;
        }
        let lines = match v.get("type").and_then(Value::as_str) {
            Some("text") => vec![str_of(v.get("text"))],
            Some("exit") => vec![format!("exit {}", str_of(v.get("code")))],
            _ => format_event(v),
        };
        if !lines.is_empty() {
            counted = count;
        }
        out.extend(lines.into_iter().rev());
    }
    out.truncate(n);
    out.reverse();
    out
}

/// Stops a running ask: its process group, claude and all.
pub fn stop(r: &Run) {
    if let Some(pid) = r.pid {
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Record, VERSION};

    /// A database holding run `r`'s stream, `events`.
    fn streamed(events: &[Value]) -> Db {
        let mut bytes = serde_json::to_string(&Record::Stacked { version: VERSION }).unwrap() + "\n";
        for e in events {
            bytes += &serde_json::to_string(&Record::Event { run: "r".into(), event: e.clone() }).unwrap();
            bytes.push('\n');
        }
        Db::from_bytes(Path::new("t.jsonl"), bytes.as_bytes()).unwrap()
    }

    #[test]
    fn the_stream_says_the_agent_is_thinking() {
        // As claude writes it, its thinking kept from the stream: counted,
        // a block with no words in it.
        let count = |n: u64| serde_json::json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": n});
        let mut events = vec![
            serde_json::json!({"type": "system", "subtype": "dev_intent", "kind": "android_app"}),
            serde_json::json!({"type": "system", "subtype": "init", "model": "claude-opus-5-5"}),
        ];
        events.extend([50, 200, 350].map(count));
        // Thinking still: its count as it is now, one line.
        let db = streamed(&events);
        assert_eq!(tail(&db, "r", 10), ["● session started · claude-opus-5-5", "✻ thinking · ~350 tokens"]);
        events.extend([500, 688].map(count));
        events.extend([
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "thinking", "thinking": "", "signature": "x"}]}}),
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": "git status --short"}}]}}),
            serde_json::json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "clean"}]}}),
            count(120),
            count(240),
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Nothing changed."}]}}),
            serde_json::json!({"type": "result", "result": "{}"}),
            serde_json::json!({"type": "exit", "code": 0}),
        ]);
        let db = streamed(&events);
        assert_eq!(
            tail(&db, "r", 20),
            [
                "● session started · claude-opus-5-5",
                "✻ thinking · ~688 tokens",
                "⚙ Bash: git status --short",
                "  ↳ clean",
                "✻ thinking · ~240 tokens",
                "Nothing changed.",
                "✓ the agent finished",
                "exit 0",
            ]
        );
        assert_eq!(tail(&db, "r", 3), ["Nothing changed.", "✓ the agent finished", "exit 0"]);
    }
}
