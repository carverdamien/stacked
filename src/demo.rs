//! `stacked demo`: a tour of the editor, on a game of tic-tac-toe played in
//! a file, board.txt, a move a patch: fourteen sections, the most useful
//! first. Its database and its repository are kept in memory (memory.rs):
//! nothing is written on disk but `[demo] seen` in the settings, so that the
//! next tour starts on the list of its sections, to start at any of them.
//! Each step says what to press, and why, and waits for you to do it; Tab
//! does it for you.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use crate::ask;
use crate::db::{Db, Effort, Error, Note, Record, Result, Run, RunState, Way, stack_change};
use crate::keys::Key;
use crate::mbox::Draft;
use crate::paint::{color, on_cursor, put, window, wrap};
use crate::thread::{Role, append_turn, last_role, thread_turns};
use crate::tui::{App, Effect, Focus, Layout, Mode, Screen};

/// Where the tour's database is: kept in memory, and nowhere a file could
/// be made, should it not be.
const DB: &str = "/nonexistent/stacked-demo/tictactoe.jsonl";
const REPO: &str = "memory:tictactoe";
const STACK: &str = "game";
const FILE: &str = "board.txt";

/// The moves, a patch each: who, where (0 to 8, row by row), its title,
/// why. O's fourth move is a mistake, and X's fifth makes it pay for it.
const MOVES: [(char, usize, &str, &str); 5] = [
    ('X', 4, "X takes the center", "Four lines go through the center, more than through any other square."),
    ('O', 0, "O takes the top left corner", "A corner is the best answer to X in the center."),
    ('X', 2, "X takes the top right corner", "With the center and the top right corner, X needs only the bottom left corner to win on the diagonal."),
    ('O', 8, "O takes the bottom right corner", "O takes the corner across from its first move."),
    ('X', 6, "X takes the bottom left corner, and wins", "X has three on the diagonal: the top right corner, the center and the bottom left corner."),
];

/// The fourth move and the fifth, as the stand-in for the agent makes them
/// again: O blocks the diagonal, and then X blocks O.
const FIXES: [(char, usize, &str, &str); 2] = [
    ('O', 6, "O blocks the diagonal at the bottom left", "X has the top right corner and the center, so the bottom left corner would win for X."),
    ('X', 3, "X blocks O's left column", "O has the top left and the bottom left corners, so the middle of the left side would win for O. Taking it, X also has two in the middle row."),
];

/// O's next move, as the stand-in plays it when a comment on the stack asks
/// it to.
const NEXT: (char, usize, &str, &str) = (
    'O',
    5,
    "O blocks X's middle row",
    "X has the middle of the left side and the center, so the middle of the right side would win for X.",
);

/// Why O's fix needs more than its patch, as the agent says it.
const MORE: &str = "if O takes the bottom left corner in patch 4, X can no longer win there in patch 5, so patch 5 has to change too";

/// What the step on the views of the code says of them, first; then of 3,
/// each file whole, and of going back.
const VIEWS: &str = "Press 1 for the menu of the views of the code, and pick one: unified, side by side, side by side compact, or unified split in two. Split in two, the middle shows the patch unified in each half, each half with a cursor of its own, and ← → go from one half to the other.";
const VIEWS_WHOLE: &str = "Press 3 in any of them to see each file whole rather than its hunks: here each hunk holds the whole board already, so only its @@ line goes. Press 3 again for the hunks. Then press 1 and pick Unified, to see the code unified again, with its hunks.";

/// The question the tour asks the agent for you, answer only.
const QUESTION: &str = "Who is winning?";

/// The comment on the stack the tour writes for you.
const PLAY: &str = "Play O's next move.";

/// How long the stand-in takes over each thing it writes, unless quick.
const PACE: Duration = Duration::from_millis(900);

/// The tours whose stand-in answers, and whose moves are played, without
/// waiting: what going to a section does for the sections before it, and
/// the tests.
static QUICK: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn quick(path: &Path) -> bool {
    QUICK.lock().is_ok_and(|q| q.contains(path))
}

/// How long the stand-in of the tour at `path` takes over each thing it
/// writes.
fn pace(path: &Path) -> Duration {
    if quick(path) { Duration::ZERO } else { PACE }
}

/// The board with `cells` (row by row, `.` for an empty one) drawn.
pub fn board(cells: &[char; 9]) -> String {
    let mut out = String::from("Tic-tac-toe: X plays first.\n\n");
    for row in 0..3 {
        out.push_str("     |     |\n");
        out.push_str(&format!("  {}  |  {}  |  {}\n", cells[row * 3], cells[row * 3 + 1], cells[row * 3 + 2]));
        out.push_str("     |     |\n");
        if row < 2 {
            out.push_str("-----+-----+-----\n");
        }
    }
    out
}

/// `text`, a board, with `who` in `cell`.
fn mark(text: &str, cell: usize, who: char) -> String {
    let (line, col) = (3 + cell / 3 * 4, 2 + cell % 3 * 6);
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    if let Some(l) = lines.get_mut(line) {
        let mut chars: Vec<char> = l.chars().collect();
        if col < chars.len() {
            chars[col] = who;
        }
        *l = chars.into_iter().collect();
    }
    lines.join("\n") + "\n"
}

/// board.txt from `before` to `after`, as `git diff` writes it, but with
/// the whole board around the move: the moves before it are seen with it.
/// So a patch applies on the board it was played on, and nowhere else.
fn diff(before: &str, after: &str) -> String {
    let whole = before.lines().count().max(after.lines().count());
    let d = similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(whole)
        .header(&format!("a/{FILE}"), &format!("b/{FILE}"))
        .to_string();
    format!("diff --git a/{FILE} b/{FILE}\n{d}")
}

/// A move as a patch: `who` played it, `n`th.
fn draft(who: char, title: &str, why: &str, diff: String, n: usize) -> Draft {
    Draft {
        commit: None,
        author: Some(format!("{who} <{}@tictactoe.example>", who.to_ascii_lowercase())),
        date: Some(format!("Sun, 18 Oct 2026 10:0{n}:00 +0000")),
        message: format!("{title}\n\n{why}"),
        diff,
    }
}

/// board.txt as the stack's patches leave it, the first `n` of them.
fn played(db: &Db, n: usize) -> Result<String> {
    let s = db.stack(STACK)?;
    let base = s.base.clone().unwrap_or_default();
    let mut tree = crate::memory::tree(Path::new(REPO), &base).unwrap_or_else(|| Err(Error("no board".into())))?;
    for id in s.patches.iter().take(n) {
        crate::memory::apply(&mut tree, &db.patch(id).diff)?;
    }
    Ok(tree.get(FILE).cloned().unwrap_or_default())
}

/// What the stand-in does for an ask, from where the game is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    /// Asked of patch 4 alone: its fix needs patch 5 too.
    More,
    /// O's mistake made good: the patches at these places, made again.
    Fix(usize, usize),
    /// O's next move, as a patch on top.
    Next,
    /// Asked to answer only.
    Answer,
    /// Asked to build and test.
    Test,
    /// Anything else it is asked to change.
    Other,
}

fn plan(db: &Db, info: &ask::Info) -> Plan {
    let Ok(s) = db.stack(STACK) else { return Plan::Other };
    let at = |title: &str| s.patches.iter().position(|p| db.patch(p).title() == title);
    let (mistake, win) = (at(MOVES[3].2), at(MOVES[4].2));
    let both = mistake.zip(win).filter(|(m, w)| m < w);
    let change = mistake.map(|m| db.patch(&s.patches[m]).change.clone());
    let on_mistake = matches!(&info.scope, ask::Scope::Patch(c) if Some(c) == change.as_ref());
    let on_stack = info.scope == ask::Scope::Stack;
    match (info.way, both) {
        (Way::Answer, _) => Plan::Answer,
        (Way::Test, _) => Plan::Test,
        (Way::Change, Some(_)) if on_mistake => Plan::More,
        (Way::Change, Some((m, w))) if on_stack => Plan::Fix(m, w),
        (Way::Change, None) if on_stack && at(FIXES[1].2).is_some() && at(NEXT.2).is_none() => Plan::Next,
        _ => Plan::Other,
    }
}

/// What the stand-in writes as it works, as claude writes it.
fn events(plan: Plan) -> Vec<serde_json::Value> {
    let said = |t: &str| json!({"type": "assistant", "message": {"content": [{"type": "text", "text": t}]}});
    let ran = |c: &str| json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": c}}]}});
    let thought = |n: u64| json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": n});
    let mut out = vec![json!({"type": "system", "subtype": "init", "model": "the tour's stand-in for claude"})];
    match plan {
        Plan::Fix(..) => out.extend([
            thought(180),
            ran(&format!("stacked show {STACK}")),
            said("X wins at the bottom left corner in patch 5, so O has to take that corner in patch 4, and then X has to play somewhere else in patch 5."),
            thought(420),
            ran(&format!("stacked export {STACK}")),
            said("Patch 4 and patch 5 both change: O blocks the diagonal, and X blocks O's left column."),
        ]),
        Plan::More => out.extend([
            thought(120),
            ran(&format!("stacked show {STACK}")),
            said("O can only block X at the bottom left corner, the square X wins on in patch 5: patch 5 has to change too, and only patch 4 is mine to change."),
        ]),
        Plan::Next => out.extend([
            ran(&format!("stacked show {STACK}")),
            said("X has two in the middle row: O has to block it, at the middle of the right side."),
        ]),
        Plan::Answer => out.push(said("Reading the board as the stack's patches leave it.")),
        Plan::Test | Plan::Other => {}
    }
    out
}

/// The stand-in for the agent, answering asks on the tour's repository:
/// what claude would, made here, written a little at a time as claude
/// writes, from where the game is. Asked to change the whole stack about
/// O's mistake, it makes the game again from there: O blocks the diagonal,
/// and X then blocks O. Asked to change patch 4 alone, it says that needs
/// more than the patch, as claude would, and the comment goes to the
/// stack. Asked on the stack after that, it plays O's next move, a patch
/// on top; asked to answer only, it says who is winning.
fn agent(path: &Path, id: &str) -> Result<()> {
    let run = Db::load(path)?.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let info = ask::info(&run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    for event in events(plan(&Db::load(path)?, &info)) {
        std::thread::sleep(pace(path));
        Db::append(path, &[Record::Event { run: id.into(), event }])?;
    }
    std::thread::sleep(pace(path));
    Db::edit(path, |db| {
        // Stopped meanwhile, it stays as it landed: failed.
        if db.run(id).is_none_or(|r| r.state != RunState::Started) {
            return Ok(());
        }
        let s = db.stack(STACK)?.clone();
        let has = |db: &Db, title: &str| s.patches.iter().any(|p| db.patch(p).title() == title);
        // Its reply to each comment it was asked about, held for review, as
        // a real landing holds it.
        let asked: Vec<(String, String)> = info
            .notes
            .iter()
            .filter(|sh| db.note(&sh.change, &sh.id).is_some())
            .map(|sh| (sh.change.clone(), sh.id.clone()))
            .collect();
        let replies = |text: &str, promote: Option<&str>| -> Vec<crate::held::Reply> {
            asked
                .iter()
                .map(|(change, id)| crate::held::Reply {
                    change: change.clone(),
                    id: id.clone(),
                    text: text.into(),
                    tag: info.way.name().into(),
                    promote: promote.map(String::from),
                    effort: info.effort,
                    ..Default::default()
                })
                .collect()
        };
        let change = |patches: Vec<String>| crate::held::Change {
            patches,
            base: s.base.clone(),
            op: "agent: the stack".into(),
            of: "the stack".into(),
            from: None,
        };
        // As the game is now: it may have gone on meanwhile.
        let (reply, stack) = match plan(db, &info) {
            Plan::More => {
                let held = crate::held::Held {
                    at: crate::db::now(),
                    replies: replies("", Some(MORE)),
                    ..Default::default()
                };
                let (summary, held) = held_or_in(db, &run, &info, held)?;
                db.add_run(Run {
                    state: RunState::Done,
                    summary,
                    held,
                    pid: None,
                    ..run
                })?;
                return Ok(());
            }
            Plan::Fix(m, w) => {
                // Patch 4 again, then patch 5 on it: O where X won.
                let (o, cell, title, why) = FIXES[0];
                let before = played(db, m)?;
                let after = mark(&before, cell, o);
                let fourth = crate::ops::revise(db.patch(&s.patches[m]), draft(o, title, why, diff(&before, &after), 4));
                let before = mark(&mark(&played(db, w)?, MOVES[3].1, '.'), cell, o);
                let (x, cell, title, why) = FIXES[1];
                let after = mark(&before, cell, x);
                let fifth = crate::ops::revise(db.patch(&s.patches[w]), draft(x, title, why, diff(&before, &after), 5));
                let mut patches = s.patches.clone();
                patches[m] = db.add_patch(fourth)?;
                patches[w] = db.add_patch(fifth)?;
                (
                    "Patch 4: O takes the bottom left corner, which blocks X's diagonal. Patch 5 changes with it, because X can no longer win at the bottom left: now X takes the middle of the left side, which blocks O's left column.".to_string(),
                    Some(change(patches)),
                )
            }
            Plan::Next => {
                let (o, cell, title, why) = NEXT;
                let n = s.patches.len();
                let before = played(db, n)?;
                let after = mark(&before, cell, o);
                let mut patches = s.patches.clone();
                patches.push(db.add_patch(crate::ops::new_patch(draft(o, title, why, diff(&before, &after), n + 1)))?);
                (
                    "Patch 6, on top: O takes the middle of the right side, which blocks X's middle row. No one has won yet, and X plays next.".to_string(),
                    Some(change(patches)),
                )
            }
            Plan::Answer => {
                let reply = if has(db, NEXT.2) {
                    "No one has won yet: O blocked X's middle row in patch 6, and X plays next. I changed nothing."
                } else if has(db, FIXES[1].2) {
                    "No one has won yet, and X is ahead: X has two in the middle row, so O has to block it next, at the middle of the right side. I changed nothing."
                } else {
                    "X is winning: X wins at the bottom left corner in patch 5, as O did not block it in patch 4. You asked me to answer only, so I changed nothing: to fix O's mistake, ask the agent about the whole stack."
                };
                (reply.to_string(), None)
            }
            Plan::Test => (
                "In the tour I cannot build or run anything. In your own stacks, claude builds and tests the stack in the stack's build worktree, and says on each patch what it found.".to_string(),
                None,
            ),
            Plan::Other => (
                "I am the tour's stand-in for the agent, and I only know the game's next moves. In your own stacks, claude reads your comment and the code around it, changes what you asked in a worktree of its own, and answers here.".to_string(),
                None,
            ),
        };
        let mut held = crate::held::Held {
            at: crate::db::now(),
            stack,
            replies: replies(&reply, None),
            ..Default::default()
        };
        // What each operation of its change needs, as a real landing works it
        // out: the tour's repository is kept in memory, where each check is
        // made at once.
        held.needs = crate::operations::needs_of(db, &run, &held);
        let (summary, held) = held_or_in(db, &run, &info, held)?;
        db.add_run(Run {
            state: RunState::Done,
            summary,
            output: reply.clone(),
            held,
            pid: None,
            ..run
        })?;
        Ok(())
    })
}

/// What the tour's stand-in brought back for run `run`, of ask `info`,
/// held for your review, as a real landing holds it: a follow-up's comes
/// into the review of the work it follows up, while that work waits for
/// your review (`held::come_in`). What its run says, and what it holds.
fn held_or_in(db: &mut Db, run: &Run, info: &ask::Info, held: crate::held::Held) -> Result<(String, Option<crate::held::Held>)> {
    Ok(match crate::held::come_in(db, run, info, &held, true)? {
        Some(said) => (said, None),
        None => (format!("its work waits for your review: {}", held.said()), Some(held)),
    })
}

/// A step of the tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// What the tour is.
    Welcome,
    /// The five moves played.
    Played,
    /// Patch 3 picked.
    Pick,
    /// Patch 3 read.
    Read,
    /// A comment on O's mistake.
    Comment,
    /// The agent asked to fix it, and watched at work.
    Ask,
    /// What the agent brought back reviewed, from the banner, and accepted.
    Review,
    /// A question, asked answer only.
    Question,
    /// Patch 1 marked read.
    Reading,
    /// Back to commenting.
    Commenting,
    /// The three views of the code.
    Views,
    /// History… opened.
    History,
    /// History… read, and left.
    InHistory,
    /// Patch 4's versions.
    Versions,
    /// A move refused.
    Move,
    /// Patch 5 dropped.
    Drop,
    /// The drop undone.
    Undo,
    /// The tree of edits.
    Tree,
    /// A comment on the stack: the agent adds a patch.
    StackComment,
    /// Runs….
    Runs,
    /// `?`, and the settings.
    Help,
    /// What next, on a project of yours.
    End,
}

/// The tour's steps, in their order.
pub const TOUR: [Step; 22] = [
    Step::Welcome,
    Step::Played,
    Step::Pick,
    Step::Read,
    Step::Comment,
    Step::Ask,
    Step::Review,
    Step::Question,
    Step::Reading,
    Step::Commenting,
    Step::Views,
    Step::History,
    Step::InHistory,
    Step::Versions,
    Step::Move,
    Step::Drop,
    Step::Undo,
    Step::Tree,
    Step::StackComment,
    Step::Runs,
    Step::Help,
    Step::End,
];

/// The tour's steps.
pub const STEPS: usize = TOUR.len();

/// A part of the tour: what it teaches, and its first step. It goes on to
/// the next one.
pub struct Section {
    pub title: &'static str,
    pub first: Step,
}

/// The tour's sections, the most useful first.
pub const SECTIONS: [Section; 14] = [
    Section { title: "Learn how to read a stack and its patches", first: Step::Welcome },
    Section { title: "Learn how to comment on a line", first: Step::Comment },
    Section { title: "Learn how to ask the agent to change the code", first: Step::Ask },
    Section { title: "Learn how to review what the agent did", first: Step::Review },
    Section { title: "Learn how to reply to the agent, and ask it a question", first: Step::Question },
    Section { title: "Learn how to keep track of what you have read", first: Step::Reading },
    Section { title: "Learn how to switch how the code is drawn", first: Step::Views },
    Section { title: "Learn about the history of the stack and of its patches", first: Step::History },
    Section { title: "Learn how to move a patch in the stack", first: Step::Move },
    Section { title: "Learn how to drop a patch, and undo any edit", first: Step::Drop },
    Section { title: "Learn how to ask the agent to add a patch", first: Step::StackComment },
    Section { title: "Learn how to see what the agent did", first: Step::Runs },
    Section { title: "Learn where help is, and how to change the keys", first: Step::Help },
    Section { title: "Learn how to start on a project of yours", first: Step::End },
];

/// Where `step` is in the tour, from 0.
pub fn place(step: Step) -> usize {
    TOUR.iter().position(|s| *s == step).unwrap_or(0)
}

/// The section the tour's step `at` is in.
pub fn section_of(at: usize) -> usize {
    SECTIONS.iter().rposition(|s| place(s.first) <= at).unwrap_or(0)
}

/// Where the tour is: its step, the patches it plays.
pub struct Tour {
    path: PathBuf,
    /// The step it is at, in `TOUR`.
    at: usize,
    /// Enter, at a step where it goes on.
    entered: bool,
    /// The moves still to play, as the tour adds them, and when it last did.
    adding: Option<(usize, Instant)>,
    /// What the tour says of itself at the step: it did it for you.
    note: String,
    /// What the step has you look at, looked at: the screen it opens, a
    /// view of the code, help.
    seen: bool,
    /// The move tried at its step was refused: what Esc then ends.
    refused: bool,
    /// The tour was opened before: any of its sections can be gone to.
    sections: bool,
    /// The list of the sections, shown, with the one picked; and whether it
    /// is over the tour, rather than before it.
    menu: Option<(usize, bool)>,
    over: bool,
}

impl Tour {
    /// The tour, at its start: an empty stack on a board of nothing, in
    /// memory, and the editor on it.
    pub fn new() -> Result<(App, Tour)> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        // Each tour a file of its own, of the same name.
        let path = match n {
            0 => PathBuf::from(DB),
            n => Path::new(DB).with_file_name(n.to_string()).join(Path::new(DB).file_name().unwrap_or_default()),
        };
        crate::memory::keep(&path);
        let empty = board(&['.'; 9]);
        let base = sha1_smol::Sha1::from(&empty).digest().to_string();
        crate::memory::plant(REPO, &base, [(FILE.to_string(), empty)].into());
        crate::memory::stand_in(agent);
        Db::edit(&path, |db| db.new_stack(STACK, &base, REPO))?;
        let app = App::new(&path)?;
        Ok((
            app,
            Tour {
                path,
                at: 0,
                entered: false,
                adding: None,
                note: String::new(),
                seen: false,
                refused: false,
                sections: false,
                menu: None,
                over: false,
            },
        ))
    }

    /// The step it is at.
    pub fn step(&self) -> Step {
        TOUR[self.at]
    }

    /// Where it is in the tour, from 0.
    pub fn index(&self) -> usize {
        self.at
    }

    pub fn over(&self) -> bool {
        self.over
    }

    /// The stand-in answering, and the moves played, without waiting.
    pub fn quick(&self, on: bool) {
        if let Ok(mut q) = QUICK.lock() {
            if on {
                q.insert(self.path.clone());
            } else {
                q.remove(&self.path);
            }
        }
    }

    /// For a tour opened before: it starts on the list of its sections, and
    /// shift-Tab lists them again.
    pub fn offer_sections(&mut self) {
        self.sections = true;
        self.menu = Some((0, false));
    }

    /// The section picked in the list of the sections, while it is shown.
    pub fn menu(&self) -> Option<usize> {
        self.menu.map(|(pick, _)| pick)
    }

    /// Where the patch of one of `titles` is in the stack.
    fn find(app: &App, titles: [&str; 2]) -> Option<usize> {
        let s = app.db.stack(STACK).ok()?;
        s.patches.iter().position(|p| titles.contains(&app.db.patch(p).title()))
    }

    /// The patch of O's mistake, or of its fix: where it is in the stack.
    fn mistake(&self, app: &App) -> Option<usize> {
        Tour::find(app, [MOVES[3].2, FIXES[0].2])
    }

    /// Whether the stack has a patch titled `title`.
    fn has(app: &App, title: &str) -> bool {
        app.db.stack(STACK).is_ok_and(|s| s.patches.iter().any(|p| app.db.patch(p).title() == title))
    }

    /// The comments to ask about O's mistake: on its patch, or moved from
    /// there to the stack; and whether they are on the patch.
    fn asking(&self, app: &App) -> (Vec<(String, String)>, bool) {
        let busy = ask::with_agent(&app.db);
        let ready = |n: &&Note| ask::sendable(n) && !busy.contains_key(&(n.change.clone(), n.id.clone()));
        let on_patch: Vec<(String, String)> = self
            .mistake(app)
            .map(|at| app.db.patch(&app.db.stack(STACK).unwrap().patches[at]).change.clone())
            .map(|c| app.db.notes(&c).into_iter().filter(ready).map(|n| (n.change.clone(), n.id.clone())).collect())
            .unwrap_or_default();
        if !on_patch.is_empty() {
            return (on_patch, true);
        }
        let moved = app
            .db
            .stack_notes(STACK)
            .into_iter()
            .filter(|n| n.moved.is_some())
            .filter(ready)
            .map(|n| (n.change.clone(), n.id.clone()))
            .collect();
        (moved, false)
    }

    /// Your comment on O's mistake: on its patch, or moved from there to the
    /// stack's comments.
    fn thread(&self, app: &App) -> Option<Note> {
        let s = app.db.stack(STACK).ok()?;
        let on_patch = self.mistake(app).map(|at| app.db.patch(&s.patches[at]).change.clone());
        let mut notes = on_patch.map(|c| app.db.notes(&c)).unwrap_or_default();
        notes.extend(app.db.stack_notes(STACK).into_iter().filter(|n| n.moved.is_some()));
        notes.into_iter().next().cloned()
    }

    /// The landed ask whose work waits for review on the tour's stack, the
    /// first landed, and whether it changes the stack.
    fn landed(app: &App) -> Option<(String, bool)> {
        let r = crate::held::queue(&app.db, STACK).into_iter().next()?;
        Some((r.id.clone(), r.held.as_ref().is_some_and(|h| h.stack.is_some())))
    }

    /// What waits for review accepted, whole, as a review accepting all of
    /// it does; a review opened left first.
    fn accept_landed(&self, app: &mut App) {
        if app.screen == Screen::Review {
            app.reviewing = None;
            app.screen = Screen::Main;
        }
        while let Some((id, _)) = Tour::landed(app) {
            if let Err(e) = Db::edit(&self.path, |db| crate::held::close(db, &id, &Default::default(), Some(true))) {
                app.status = e.0;
                break;
            }
            app.refresh();
        }
        app.launch();
    }

    /// Whether the agent works on something, or a comment waits for it.
    fn working(app: &App) -> bool {
        !ask::with_agent(&app.db).is_empty() || app.db.stack_notes(STACK).iter().any(|n| n.queued.as_ref().is_some_and(|q| !q.batched()))
    }

    /// Whether every changed line of patch `i` (from 0) is marked read.
    fn read(app: &App, i: usize) -> bool {
        let Some(id) = app.db.stack(STACK).ok().and_then(|s| s.patches.get(i).cloned()) else {
            return false;
        };
        let p = app.db.patch(&id);
        let keys = crate::progress::read_keys(&app.db, p);
        crate::progress::changed_lines(p).iter().filter(|(_, blank)| !blank).all(|(k, _)| keys.contains(k))
    }

    /// What the stack's last record did: an edit, or a move of its head.
    fn last_op(app: &App) -> String {
        match app.db.log(Some(STACK)).last() {
            Some((_, Record::Stack(s))) => s.op.clone(),
            Some((_, Record::Head(h))) => h.op.clone(),
            _ => String::new(),
        }
    }

    /// Whether what the step asks is done.
    pub fn done(&self, app: &App) -> bool {
        let n = app.db.stack(STACK).map_or(0, |s| s.patches.len());
        let on_main = app.screen == Screen::Main && matches!(app.mode, Mode::Normal);
        match self.step() {
            Step::Welcome | Step::End => self.entered,
            Step::Played => n == MOVES.len() && self.adding.is_none(),
            Step::Pick => app.focus == Focus::List && !app.on_stack && app.sel == 2,
            Step::Read => app.focus == Focus::Diff && !app.on_stack && app.sel == 2,
            Step::Comment => self.mistake(app).is_some_and(|at| {
                let s = app.db.stack(STACK).unwrap();
                !app.db.notes(&app.db.patch(&s.patches[at]).change).is_empty()
            }),
            // Landed, its change waiting for review; or accepted already.
            Step::Ask => Tour::has(app, FIXES[0].2) || Tour::landed(app).is_some_and(|(_, change)| change),
            Step::Review => Tour::has(app, FIXES[0].2) && app.screen == Screen::Main,
            // The agent's answer, the thread's last turn.
            Step::Question => self.thread(app).is_some_and(|n| {
                thread_turns(&n.text).last().is_some_and(|t| t.role == Role::Agent && t.tag.as_deref() == Some(Way::Answer.name()))
            }),
            Step::Reading => Tour::read(app, 0),
            Step::Commenting => !app.reading,
            Step::Views => self.seen && app.layout == Layout::Unified && !app.whole_files,
            Step::History => app.screen == Screen::History,
            Step::InHistory => app.screen == Screen::Main,
            Step::Versions | Step::Tree | Step::Runs => self.seen && on_main,
            // Refused, and no longer moving: Esc left the patch where it was.
            Step::Move => (self.refused || Tour::last_op(app).starts_with("move")) && !matches!(app.mode, Mode::Moving { .. }),
            Step::Drop => n + 1 == MOVES.len() && Tour::last_op(app).starts_with("drop"),
            Step::Undo => n == MOVES.len() && Tour::last_op(app).starts_with("undo"),
            Step::StackComment => Tour::has(app, NEXT.2) && app.screen == Screen::Main,
            Step::Help => self.seen && !matches!(app.mode, Mode::Help { .. }),
        }
    }

    /// Whether Enter is the tour's at this step: only to go on.
    fn enters(&self) -> bool {
        matches!(self.step(), Step::Welcome | Step::Played | Step::End)
    }

    /// What the tour says at the step, its keys as the settings name them;
    /// and, at its end, the board.
    pub fn say(&self, app: &App) -> (String, Option<String>) {
        let working = Tour::working(app);
        let text: String = match self.step() {
            Step::Welcome if self.sections => "Welcome to stacked, an editor of stacks of patches. In stacked you read patches, comment on their lines, ask an agent about them and reorder them, and stacked keeps every edit, so you can undo it. This tour plays tic-tac-toe in a file, board.txt, with one patch per move, in 14 sections, the most useful first. Each step waits for you to do what it says; Tab does the step for you, shift-Tab lists the sections, to go to any of them, and ctrl-c leaves the tour. Press Enter to go on.".into(),
            Step::Welcome => "Welcome to stacked, an editor of stacks of patches. In stacked you read patches, comment on their lines, ask an agent about them and reorder them, and stacked keeps every edit, so you can undo it. This tour plays tic-tac-toe in a file, board.txt, with one patch per move. It has 14 sections, the most useful first, and takes about fifteen minutes. Each step waits for you to do what it says; Tab does the step for you, and ctrl-c leaves the tour. The tour writes nothing on disk but one setting, [demo] seen, in stacked's settings file: the next tour starts on the list of its sections, to start at any of them. Press Enter to go on.".into(),
            Step::Played => "The project has one file, board.txt, with an empty board, and one stack, game, with no patches yet. (`stacked new game --base main` makes an empty stack, and `stacked import game main..HEAD` makes a stack of the commits on top of main.) Press Enter, and the tour plays five moves, one patch for each.".into(),
            Step::Pick => "On the left is the stack, game: one patch per move, in the order the moves were played, from patch 1, X takes the center. Press ↑↓ to pick a patch: pick patch 3, X takes the top right corner.".into(),
            Step::Read => "Press → to read patch 3, X takes the top right corner. The middle shows the whole board: the moves before patch 3 as they are, and the row that patch 3 changes, as it was (−) and as it is (+). (git's patches show 3 lines around a change; the tour's patches show the whole board.) ↑↓ go through patch 3's lines, and ← goes back to the patches.".into(),
            Step::Comment => "Patch 4 is a mistake: O takes the bottom right corner, which lets X win at the bottom left in patch 5. Comment on patch 4, as in a review: press ← to go to the patches, ↓ to pick patch 4, → to read it, and ↓ to go to its + line. Then press Enter, write why it is a mistake, press Esc, and choose Save it.".into(),
            Step::Ask if Tour::landed(app).is_some() => "The agent answered that fixing O's mistake needs more than patch 4, as patch 5 has to change too. Its answer waits for your review: your comment's first line says so, and so does the banner at the top. Nothing the agent brings back is applied before you review it. Accepting this answer moves your comment to the stack's own comments, where the agent may change any patch: press Enter on your comment to accept it.".into(),
            Step::Ask if working => "The agent is working on your comment: the comment's first line says with the agent, and the last lines the agent writes show under it. Press Enter on your comment to open all it writes, as it writes, and Esc to fold it again. (In the tour, a stand-in for the agent answers, in memory. In your own stacks, claude answers, and makes its changes in a worktree of its own.)".into(),
            Step::Ask if !self.asking(app).1 && !self.asking(app).0.is_empty() => "You asked the agent about patch 4 alone, and the agent says that fixing O's mistake needs more than patch 4: patch 5, X's win, has to change too. So your comment moved from patch 4 to the stack's own comments, and the cursor moved with it. Ask the agent from there, where it may change any patch: press Space and choose Ask agent…, press → on Permission until it says change stack, press ↓ and → to set Effort, then press ↓ to Ask Now, and Enter.".into(),
            Step::Ask => "Ask the agent to fix O's mistake in patch 4. The fix changes patch 5 too, because X can no longer win where O blocks. So with the cursor on your comment on patch 4, press Space and choose Ask agent…: a dialog opens with nothing set, so that no ask goes as the last one did by mistake. Press → on Permission until it says change stack, so that the agent may change any patch, or press Enter there and pick change stack in the list that opens. Press ↓ and → to set Effort to low, which is enough here. Then press ↓ to Ask Now, and Enter.".into(),
            Step::Review if app.screen == Screen::Review => "This is the review of the agent's work. On the left are its parts: the stack operations of its change, a line for each patch (patches 4 and 5 are modified), and its reply to your comment. On the right is what the part the cursor is on would be: the stack diff of the change, or the reply as the last turn of its thread. Press ↓ to go through the parts and Enter on each to accept it (Enter again puts it back to review; rejecting a part is in its menu, Space). Then press Esc and choose Close review: what you accepted is applied then.".into(),
            Step::Review if Tour::landed(app).is_some() => "The agent's work has landed, and nothing changed under you: it waits for your review, and the banner at the top says so. Nothing of it is applied before you review it, and the stack is not edited by hand meanwhile. Press Esc: the banner opens, a line for each piece of the agent's work, the landed one picked. Then press Enter to review the agent's work.".into(),
            Step::Review => "You rejected the agent's change, so the stack is as it was. Ask the agent again: on your comment, press Space and choose Ask agent…, set Permission to change stack and Effort with → on each, then press Enter on Ask Now.".into(),
            Step::Question if Tour::landed(app).is_some() => "The agent's answer has landed, and waits for your review: your comment's first line says so, and the comments pane marks the comment ◆. An answer changes nothing in the stack, so it can be accepted at once, without the review: press Enter on your comment to accept the answer.".into(),
            Step::Question if working => "The agent is answering your question. Asked that way, it reads the code and answers, and changes nothing.".into(),
            Step::Question => {
                let moved = self.thread(app).is_some_and(|n| n.moved.is_some());
                let place = if moved { "among the stack's own comments" } else { "on patch 4" };
                format!(
                    "The agent rewrote patches 4 and 5: now O blocks the diagonal, and X blocks O's left column. Its reply is the last turn of your comment's thread, {place}: the comments pane, on the right, marks the comment ✓, and the pane's header says 1 to read. Reply with a question: with the cursor on your comment (→ goes to the comments pane, and Enter there goes to the comment), press Enter, write \"Who is winning?\", press Esc and choose Ask agent…, set Permission to answer only and Effort with → on each, then press Enter on Ask Now. Asked that way, the agent answers, and changes nothing."
                )
            }
            Step::Reading => "The agent answered your question, and changed nothing. Next, keep track of what you have read. Press 2 to switch to reading: each patch on the left then counts its changed lines read, as 0/2, and the bar at the top counts them too. In the list of patches (← from the middle), pick patch 1 and press → to read it. Then press Space: it marks the block of code at the cursor read and goes on to the next line not read, and Delete or Backspace marks it unread again. Mark patch 1 read.".into(),
            Step::Commenting => "Patch 1 is read: the list marks it ✓. Read marks are kept by the lines' content, so a new version of a patch keeps them on the lines it did not change. Press 2 again to switch back to commenting, where Space on a line opens its menu.".into(),
            Step::Views if !app.sides_fit() => format!("{VIEWS} Every view but unified needs a wider middle than this one: picking one makes its room, the comments made compact or hidden first, then the patches hidden, and the status line says how to show them again. Pick each view in turn. {VIEWS_WHOLE}"),
            Step::Views => format!("{VIEWS} Pick each view in turn. {VIEWS_WHOLE}"),
            Step::History => "Every change of the stack is kept. History… shows them: press ← to go to the patches and ↑ until the stack's header (≡ game) is picked, then press Space and choose History….".into(),
            Step::InHistory => "This is the history of the stack, game, newest change first. Beside the picked change is its stack diff: the patches before and after that change, and under each new version of a patch, what changed in it. Press ↑↓ to pick another change, and Esc to leave the history.".into(),
            Step::Versions => "A patch's versions are kept too. In the list, pick patch 4, O blocks the diagonal at the bottom left, press Space and choose Versions of this patch…: every version of patch 4, oldest first, and beside the picked one, what changed from the version before it. Press Esc to come back.".into(),
            Step::Move if self.refused => "stacked refused to move patch 5: patch 5 was made on the board that patches 1 to 4 leave, so it does not apply to an empty board. stacked checks whether patches apply; it does not know the rules of the game. You are still moving patch 5, and could try another place: press Esc to stop moving it, and leave it where it was.".into(),
            Step::Move => "A patch can only go where it applies. In the list of patches, press ↓ until patch 5, X blocks O's left column, is picked. Then press Space and choose Move this patch…, press ↑ until patch 5 is at the top of the list, and press Enter to put it there.".into(),
            Step::Drop => "A patch can be dropped. Pick patch 5, X blocks O's left column, press Space and choose Drop this patch…. stacked asks first: choose Drop it. The board then loses X's last move. (Dropping a patch with others above it is refused here, as they were made on the board it leaves.)".into(),
            Step::Undo => "Every edit can be undone and redone, the agent's too. Press Space and choose Undo, near the end of the menu: each Undo is named after the edit it takes back, here Undo: drop. Patch 5 comes back. Redo, under it, would drop it again.".into(),
            Step::Tree => "Every state the stack was in is kept, as a tree of edits. On the stack's header (≡ game), press Space and choose Tree of edits…: ● marks the state you are on, and the stack without patch 5 is there too, as a branch to go back to. Press Esc to come back.".into(),
            Step::StackComment if app.screen == Screen::Review => "Accept the agent's change, which adds patch 6, and its reply: press Enter on each part, then press Esc and choose Close review.".into(),
            Step::StackComment if Tour::landed(app).is_some() => "The agent's work has landed: its change adds patch 6, O's next move, and it waits for your review. Review it as before: press Esc to open the banner, press Enter, accept each part with Enter, then press Esc and choose Close review.".into(),
            Step::StackComment if working => "The agent is working on the stack's comment. The stack's own comments show in the middle while the stack's header (≡ game) is picked, and this one says with the agent there.".into(),
            Step::StackComment => "A comment on the stack is asked about the whole stack: there the agent may change any patch, and add new ones, as for a rebase. On the stack's header (≡ game), press Space and choose Comment on the stack…, write \"Play O's next move.\", press Esc and choose Ask agent…, set Permission to change stack and Effort with → on each, then press Enter on Ask Now.".into(),
            Step::Runs => "The agent added patch 6: O blocks X's middle row. Runs… lists everything the agent was asked, newest first, and what came of each: press Space and choose Runs…, near the end of any menu. Enter on a run shows all the agent wrote, Esc goes back to the list, and Esc again comes back to the stack.".into(),
            Step::Help => "Press ? anywhere: it says what you can do where the cursor is, and where the arrows go from there. Read it, then press any key to close it. The keys are settings too: `stacked config` prints every setting in the form of stacked's settings file, ~/.stacked/config.toml, and its [keys] can make any key another.".into(),
            Step::End => "That is stacked. In a project of yours, `stacked import NAME main..HEAD` makes a stack of the commits on top of main, `stacked` opens the editor, `stacked web` opens the editor in a browser, and `stacked --help` lists the other commands. Next time, `stacked demo` starts on the list of its sections. Below is board.txt as the stack's patches leave it. Press Enter to leave the tour.".into(),
        };
        let end = (self.step() == Step::End).then(|| {
            let n = app.db.stack(STACK).map_or(0, |s| s.patches.len());
            played(&app.db, n).unwrap_or_default()
        });
        (app.say_keys(text), end)
    }

    /// The step done for you. A step that opens a screen opens it, and done
    /// again leaves it.
    pub fn show(&mut self, app: &mut App) {
        let path = self.path.clone();
        let fail = |app: &mut App, e: Error| app.status = e.0;
        let n = app.db.stack(STACK).map_or(0, |s| s.patches.len());
        match self.step() {
            Step::Welcome | Step::End => self.entered = true,
            Step::Played => {
                if self.adding.is_none() && n == 0 {
                    self.adding = Some((0, Instant::now() - Duration::from_secs(1)));
                }
            }
            Step::Pick => {
                app.focus = Focus::List;
                app.on_stack = false;
                app.sel = 2;
            }
            Step::Read => {
                app.on_stack = false;
                app.sel = 2;
                app.focus = Focus::Diff;
            }
            Step::Comment => {
                let Some(at) = self.mistake(app) else { return };
                let p = app.db.patch(&app.db.stack(STACK).unwrap().patches[at]).clone();
                let Some(line) = crate::view::note_lines(&p).into_iter().find(|l| l.origin == '+') else { return };
                let note = Note {
                    change: p.change.clone(),
                    id: line.key.clone(),
                    line: line.key.clone(),
                    text: "O has to block X's diagonal here: X wins at the bottom left in patch 5.".into(),
                    file: line.file.clone(),
                    preview: line.text.clone(),
                    ..Default::default()
                };
                if let Err(e) = Db::edit(&path, |db| db.set_note(note)) {
                    fail(app, e);
                }
                app.on_stack = false;
                app.sel = at;
                app.focus = Focus::Diff;
            }
            // The answer that moves the comment to the stack, accepted.
            Step::Ask if Tour::landed(app).is_some() => self.accept_landed(app),
            Step::Ask | Step::Review if Tour::landed(app).is_none() && !Tour::has(app, FIXES[0].2) => {
                // Of the whole stack: from the patch, or from the stack the
                // comment went to.
                let (notes, on_patch) = self.asking(app);
                if notes.is_empty() {
                    return;
                }
                match ask::queue(&path, STACK, &notes, Way::Change, on_patch, Effort::default()) {
                    Ok(()) => app.launch(),
                    Err(e) => fail(app, e),
                }
            }
            Step::Ask => {}
            Step::Review | Step::Question | Step::StackComment if Tour::landed(app).is_some() || app.screen == Screen::Review => {
                self.accept_landed(app)
            }
            Step::Review => {}
            Step::Question => {
                let Some(n) = self.thread(app) else { return };
                if Tour::working(app) {
                    return;
                }
                // Your question, unless you wrote it already, then asked.
                let mut asked: Result<()> = Ok(());
                if last_role(&n.text) != Some(Role::You) {
                    let note = Note {
                        text: append_turn(&n.text, Role::You, QUESTION, None),
                        failed: None,
                        ..n.clone()
                    };
                    asked = Db::edit(&path, |db| db.set_note(note)).map(|_| ());
                }
                match asked.and_then(|_| ask::queue(&path, STACK, &[(n.change.clone(), n.id.clone())], Way::Answer, false, Effort::default())) {
                    Ok(()) => app.launch(),
                    Err(e) => fail(app, e),
                }
            }
            Step::Reading => {
                app.on_stack = false;
                app.sel = 0;
                app.focus = Focus::Diff;
                if !app.reading {
                    app.press(Key::Read);
                }
                let first = app.db.stack(STACK).ok().and_then(|s| s.patches.first().cloned());
                if let Some(p) = first.map(|id| app.db.patch(&id).clone())
                    && !Tour::read(app, 0)
                {
                    let keys = crate::progress::with_sections(&p, crate::progress::changed_keys(&p));
                    if let Err(e) = Db::edit(&path, |db| db.mark_read(&p.change, keys)) {
                        fail(app, e);
                    }
                }
            }
            Step::Commenting => {
                if app.reading {
                    app.press(Key::Read);
                }
            }
            Step::Views => {
                self.seen = true;
                app.list = crate::config::PatchesPane::Expanded;
                app.set_layout(Layout::Unified);
                app.set_whole(false);
            }
            // From the stack's header, as by hand: back from the history,
            // the list of patches is where the next step starts.
            Step::History => {
                app.focus = Focus::List;
                app.on_stack = true;
                app.screen = Screen::History;
            }
            Step::InHistory => app.screen = Screen::Main,
            Step::Versions => {
                if app.screen == Screen::Versions {
                    app.press(Key::Back);
                } else if let Some(at) = self.mistake(app) {
                    app.screen = Screen::Main;
                    app.focus = Focus::List;
                    app.on_stack = false;
                    app.sel = at;
                    choose(app, "Versions of this patch");
                }
            }
            Step::Move => {
                // A patch being moved by hand meanwhile: left where it was.
                if matches!(app.mode, Mode::Moving { .. }) {
                    app.mode = Mode::Normal;
                }
                let Some(at) = Tour::find(app, [MOVES[4].2, FIXES[1].2]) else { return };
                // Picked in the list of patches, as the step says.
                app.focus = Focus::List;
                app.on_stack = false;
                app.sel = at;
                match Db::edit(&path, |db| crate::ops::move_patch(db, STACK, at, 0)) {
                    Ok(_) => app.status = "moved".into(),
                    Err(e) => {
                        self.refused = true;
                        fail(app, e);
                    }
                }
            }
            Step::Drop => {
                let Some(at) = Tour::find(app, [FIXES[1].2, MOVES[4].2]) else { return };
                app.focus = Focus::List;
                app.on_stack = false;
                app.sel = at;
                if n == MOVES.len()
                    && let Err(e) = Db::edit(&path, |db| crate::ops::drop_patches(db, STACK, &[at]))
                {
                    fail(app, e);
                }
            }
            Step::Undo => {
                if Tour::last_op(app).starts_with("drop")
                    && let Err(e) = Db::edit(&path, |db| db.undo(STACK))
                {
                    fail(app, e);
                }
            }
            Step::Tree => {
                if app.screen == Screen::Tree {
                    app.press(Key::Back);
                } else {
                    app.screen = Screen::Main;
                    app.focus = Focus::List;
                    app.on_stack = true;
                    choose(app, "Tree of edits");
                }
            }
            Step::StackComment => {
                app.focus = Focus::List;
                app.on_stack = true;
                if Tour::working(app) || Tour::has(app, NEXT.2) {
                    return;
                }
                let note = Note {
                    change: stack_change(STACK),
                    id: format!("s{}", crate::db::unique()),
                    text: PLAY.into(),
                    ..Default::default()
                };
                let asked = (note.change.clone(), note.id.clone());
                match Db::edit(&path, |db| db.set_note(note)).and_then(|_| ask::queue(&path, STACK, &[asked], Way::Change, false, Effort::default())) {
                    Ok(()) => app.launch(),
                    Err(e) => fail(app, e),
                }
            }
            Step::Runs => {
                if matches!(app.screen, Screen::Runs | Screen::Transcript) {
                    app.press(Key::Back);
                } else {
                    app.screen = Screen::Main;
                    choose(app, "Runs");
                }
            }
            Step::Help => {
                if matches!(app.mode, Mode::Help { .. }) {
                    app.press(Key::Back);
                } else {
                    app.press(Key::Help);
                }
            }
        }
        app.refresh();
    }

    /// What the tour does as time goes: the moves played one by one, and the
    /// next step once the step is done, by you or for you.
    pub fn tick(&mut self, app: &mut App) {
        if let Some((k, at)) = self.adding
            && (quick(&self.path) || at.elapsed() >= Duration::from_millis(400))
        {
            let n = k;
            let (who, cell, title, why) = MOVES[n];
            let res = Db::edit(&self.path, |db| {
                let before = played(db, n)?;
                let after = mark(&before, cell, who);
                crate::ops::add(db, STACK, draft(who, title, why, diff(&before, &after), n + 1), None).map(|_| ())
            });
            if let Err(e) = res {
                app.status = e.0;
            }
            self.adding = (n + 1 < MOVES.len()).then(|| (n + 1, Instant::now()));
            app.refresh();
        }
        // What the step has you look at, looked at; the move refused, which
        // the status line says only until the next key.
        match self.step() {
            Step::Move if app.status.contains("does not apply") => self.refused = true,
            Step::Views if app.layout != Layout::Unified => self.seen = true,
            Step::Versions if app.screen == Screen::Versions => self.seen = true,
            Step::Tree if app.screen == Screen::Tree => self.seen = true,
            Step::Runs if matches!(app.screen, Screen::Runs | Screen::Transcript) => self.seen = true,
            Step::Help if matches!(app.mode, Mode::Help { .. }) => self.seen = true,
            _ => {}
        }
        if self.done(app) {
            if self.at == STEPS - 1 {
                self.over = true;
                return;
            }
            self.at += 1;
            self.entered = false;
            self.note.clear();
            self.seen = false;
            self.refused = false;
        }
    }

    /// Whether the step waits on work done elsewhere: the moves played, the
    /// agent.
    pub fn waiting(&self, app: &App) -> bool {
        self.adding.is_some() || Tour::working(app)
    }

    /// The steps up to the tour's step `to`, each done for you as Tab does
    /// it, the stand-in and the moves without waiting.
    pub fn forward(&mut self, app: &mut App, to: usize) -> Result<()> {
        self.quick(true);
        let res = self.forward_quick(app, to.min(STEPS - 1));
        self.quick(false);
        res
    }

    fn forward_quick(&mut self, app: &mut App, to: usize) -> Result<()> {
        let started = Instant::now();
        while self.at < to {
            if started.elapsed() > Duration::from_secs(30) {
                return Err(Error(format!("the tour could not do its step {} for you", self.at + 1)));
            }
            let was = self.at;
            self.show(app);
            loop {
                std::thread::sleep(Duration::from_millis(2));
                app.refresh();
                self.tick(app);
                if self.at != was || !self.waiting(app) {
                    break;
                }
            }
        }
        Ok(())
    }

    /// The tour again, from the start of section `k`: a new game, the steps
    /// before it done for you.
    fn jump(&mut self, app: &mut App, k: usize) -> Result<()> {
        let (mut a, mut t) = Tour::new()?;
        t.sections = self.sections;
        t.forward(&mut a, place(SECTIONS[k].first))?;
        // What the steps done for you said is not the reader's.
        a.status.clear();
        if k > 0 {
            t.note = "the sections before it done for you".into();
        }
        *app = a;
        *self = t;
        Ok(())
    }

    /// A key, the list of the sections shown: ↑↓ pick one, Enter starts it,
    /// Esc goes back to the tour (or, before it, starts it at its start).
    pub fn menu_key(&mut self, app: &mut App, k: KeyCode) -> Result<()> {
        let Some((pick, over)) = self.menu else { return Ok(()) };
        let last = SECTIONS.len() - 1;
        let pick = match k {
            KeyCode::Up => pick.checked_sub(1).unwrap_or(last),
            KeyCode::Down => (pick + 1) % SECTIONS.len(),
            KeyCode::Home | KeyCode::PageUp => 0,
            KeyCode::End | KeyCode::PageDown => last,
            KeyCode::Enter => {
                self.menu = None;
                // Before the tour, its first section is where it is.
                if over || pick > 0 {
                    self.jump(app, pick)?;
                }
                return Ok(());
            }
            KeyCode::Esc | KeyCode::BackTab => {
                self.menu = None;
                return Ok(());
            }
            _ => pick,
        };
        self.menu = Some((pick, over));
        Ok(())
    }

    /// The panel's lines, title and foot, and which line is picked: the
    /// step's words, or the list of the sections.
    fn panel(&self, app: &App, w: usize) -> (Vec<String>, String, String, Option<usize>) {
        if let Some((pick, over)) = self.menu {
            let intro = if over {
                "The tour's sections, the most useful first. Pick one to start it, on a new game, played as the sections before it leave it."
            } else {
                "You have opened the tour before: start at any of its sections, the most useful first. The game is then played as the sections before it leave it, done for you."
            };
            let mut lines = wrap(intro, w);
            lines.push(String::new());
            let first = lines.len();
            for (i, s) in SECTIONS.iter().enumerate() {
                lines.push(format!("{} {:>2}. {}", if i == pick { "▸" } else { " " }, i + 1, s.title));
            }
            let back = if over { "back to the tour" } else { "start at the beginning" };
            let foot = format!(" ↑↓: pick · Enter: start there · Esc: {back} · ctrl-c: leave ");
            return (lines, "stacked demo · its sections".into(), foot, Some(first + pick));
        }
        let (text, end) = self.say(app);
        let mut lines = wrap(&text, w);
        if let Some(board) = end {
            lines.push(String::new());
            lines.extend(board.lines().map(|l| format!("    {l}")));
        }
        let title = format!("stacked demo · step {} of {STEPS} · {}", self.at + 1, SECTIONS[section_of(self.at)].title);
        let mut foot = if self.sections {
            " Tab: do it for me · shift-Tab: sections · ctrl-c: leave ".to_string()
        } else {
            " Tab: do it for me · ctrl-c: leave ".to_string()
        };
        if !self.note.is_empty() {
            foot = format!(" {} ·{foot}", self.note);
        }
        (lines, title, foot, None)
    }

    /// The editor drawn into `buf`, the tour's words under it, or the list
    /// of its sections: where the terminal's cursor goes.
    pub fn draw(&self, app: &mut App, buf: &mut Buffer) -> Option<(u16, u16)> {
        let area = buf.area;
        let (w, h) = (area.width as usize, area.height as usize);
        let tw = w.saturating_sub(4).max(10);
        let (lines, title, foot, picked) = self.panel(app, tw);
        // Too small for both: the editor alone, or the list alone.
        let small = h < 12 || w < 40;
        if small && picked.is_none() {
            return app.draw(buf);
        }
        let most = if picked.is_some() { h * 3 / 4 } else { h / 2 };
        let ph = if small { h } else { (lines.len() + 2).min(most).max(3) };
        let mut cursor = None;
        if ph < h {
            let top = Rect::new(area.x, area.y, area.width, (h - ph) as u16);
            let mut screen = Buffer::empty(top);
            cursor = app.draw(&mut screen);
            buf.merge(&screen);
        }
        window(buf, area, 0, h - ph, w, ph, &title, color::note());
        let room = ph.saturating_sub(2);
        // The picked section in sight.
        let skip = picked.map_or(0, |p| (p + 1).saturating_sub(room));
        for (row, (i, l)) in lines.iter().enumerate().skip(skip).take(room).enumerate() {
            let style = if Some(i) == picked { on_cursor(Style::new()) } else { Style::new() };
            put(buf, area, 2, h - ph + 1 + row, tw, l, style);
        }
        let x = w.saturating_sub(foot.chars().count() + 2);
        put(buf, area, x, h - 1, w - x, &foot, color::meta().add_modifier(Modifier::DIM));
        if picked.is_some() { None } else { cursor }
    }

    /// The tour, in the terminal, until its end or ctrl-c.
    fn play(&mut self, terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
        let io = |e: std::io::Error| Error(e.to_string());
        loop {
            app.refresh();
            if self.menu.is_none() {
                self.tick(app);
            }
            if self.over {
                return Ok(());
            }
            terminal
                .draw(|f| {
                    if let Some((x, y)) = self.draw(app, f.buffer_mut()) {
                        f.set_cursor_position((x, y));
                    }
                })
                .map_err(io)?;
            if !event::poll(Duration::from_millis(150)).map_err(io)? {
                continue;
            }
            let Event::Key(k) = event::read().map_err(io)? else { continue };
            if k.kind == KeyEventKind::Release {
                continue;
            }
            if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
            if self.menu.is_some() {
                // The keys as the settings name them.
                let code = app.cfg.keys.translate(&k).map_or(k.code, |t| t.code);
                if let Err(e) = self.menu_key(app, code) {
                    app.status = e.0;
                }
                continue;
            }
            // Writing a comment, or searching the middle, Tab and shift-Tab
            // are the editor's.
            let writing = matches!(app.mode, Mode::Edit(_) | Mode::Find(_));
            if k.code == KeyCode::BackTab && self.sections && !writing {
                self.menu = Some((section_of(self.at), true));
                continue;
            }
            if k.code == KeyCode::Tab && !writing {
                self.note = "done for you".into();
                self.show(app);
                continue;
            }
            if k.code == KeyCode::Enter && self.enters() && matches!(app.mode, Mode::Normal) {
                self.show(app);
                continue;
            }
            match app.key(k) {
                Effect::Quit => return Ok(()),
                Effect::None => {}
                // Refused, kept in memory as it is: it says so.
                e => {
                    let _ = app.effect(e);
                }
            }
        }
    }
}

/// Space, then the menu's item whose label starts with `label`: ↓ to it,
/// and Enter, as by hand. Whether the menu had it.
fn choose(app: &mut App, label: &str) -> bool {
    if !matches!(app.mode, Mode::Menu(_)) {
        app.press(Key::Menu);
    }
    let found = match &app.mode {
        Mode::Menu(m) => m.items.iter().position(|i| i.label.starts_with(label)).map(|want| (want, m.items.len())),
        _ => None,
    };
    let Some((want, len)) = found else {
        if matches!(app.mode, Mode::Menu(_)) {
            app.press(Key::Back);
        }
        return false;
    };
    for _ in 0..len * 2 {
        if matches!(&app.mode, Mode::Menu(m) if m.sel == want) {
            break;
        }
        app.press(Key::Down);
    }
    app.press(Key::Enter);
    true
}

/// The tour, in this terminal. Opened before, it starts on the list of its
/// sections; and the settings say it was opened, for the next time.
pub fn run() -> Result<()> {
    let seen = crate::config::get().demo.seen;
    let remembered = crate::config::remember_demo(&crate::config::path());
    let (mut app, mut tour) = Tour::new()?;
    if seen {
        tour.offer_sections();
    }
    if let Err(e) = remembered {
        app.status = format!("the settings cannot say the tour was opened: {e}");
    }
    let mut terminal = ratatui::init();
    let res = tour.play(&mut terminal, &mut app);
    ratatui::restore();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: [[usize; 3]; 8] = [[0, 1, 2], [3, 4, 5], [6, 7, 8], [0, 3, 6], [1, 4, 7], [2, 5, 8], [0, 4, 8], [2, 4, 6]];

    fn won(cells: &[char; 9], who: char) -> bool {
        LINES.iter().any(|l| l.iter().all(|&c| cells[c] == who))
    }

    /// The squares that would win for `who`: a line of theirs but for one
    /// empty square.
    fn threats(cells: &[char; 9], who: char) -> Vec<usize> {
        let mut out: Vec<usize> = LINES
            .iter()
            .filter_map(|l| {
                let mine = l.iter().filter(|&&c| cells[c] == who).count();
                let empty: Vec<usize> = l.iter().copied().filter(|&c| cells[c] == '.').collect();
                (mine == 2 && empty.len() == 1).then(|| empty[0])
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// `moves` played as a game must be: X first, then in turn, on empty
    /// squares, none after a win.
    fn play(moves: &[(char, usize)]) -> [char; 9] {
        let mut cells = ['.'; 9];
        for (i, &(who, cell)) in moves.iter().enumerate() {
            assert_eq!(who, if i % 2 == 0 { 'X' } else { 'O' }, "move {}: in turn", i + 1);
            assert_eq!(cells[cell], '.', "move {}: an empty square", i + 1);
            assert!(!won(&cells, 'X') && !won(&cells, 'O'), "move {}: after a win", i + 1);
            cells[cell] = who;
        }
        cells
    }

    #[test]
    fn the_game_makes_sense_as_played_and_as_fixed() {
        let moves: Vec<(char, usize)> = MOVES.iter().map(|m| (m.0, m.1)).collect();
        // X's second move threatens the diagonal; O's mistake leaves it,
        // and X takes it.
        assert_eq!(threats(&play(&moves[..3]), 'X'), [6]);
        assert_eq!(threats(&play(&moves[..4]), 'X'), [6], "O does not block");
        assert!(won(&play(&moves), 'X'));
        // Made again: O blocks, and so threatens the left column, which X
        // blocks, X then two in the middle row; no one has won.
        let fixed: Vec<(char, usize)> = moves[..3].iter().copied().chain(FIXES.iter().map(|m| (m.0, m.1))).collect();
        let four = play(&fixed[..4]);
        assert!(threats(&four, 'X').is_empty(), "blocked");
        assert_eq!(threats(&four, 'O'), [3]);
        let five = play(&fixed);
        assert!(threats(&five, 'O').is_empty() && !won(&five, 'X') && !won(&five, 'O'));
        assert_eq!(threats(&five, 'X'), [5]);
        // O's next move blocks X's middle row, as the agent says: no threat
        // left, no one has won.
        let six = play(&[fixed, vec![(NEXT.0, NEXT.1)]].concat());
        assert!(threats(&six, 'X').is_empty() && threats(&six, 'O').is_empty());
        assert!(!won(&six, 'X') && !won(&six, 'O'));
    }

    #[test]
    fn moves_are_marks_on_the_board() {
        let empty = board(&['.'; 9]);
        assert_eq!(empty.lines().count(), 13);
        let x = mark(&empty, 4, 'X');
        assert_eq!(x.lines().nth(7).unwrap(), "  .  |  X  |  .");
        let o = mark(&x, 6, 'O');
        assert_eq!(o.lines().nth(11).unwrap(), "  O  |  .  |  .");
        // The whole board around a move, the moves before it with it.
        let d = diff(&x, &o);
        assert!(d.starts_with("diff --git a/board.txt b/board.txt\n--- a/board.txt\n+++ b/board.txt\n@@ -1,13 +1,13 @@\n"), "{d}");
        assert!(d.lines().any(|l| l == "   .  |  X  |  ."), "the move before, as context:\n{d}");
    }

    #[test]
    fn the_sections_are_in_the_tour_s_order() {
        assert_eq!(place(SECTIONS[0].first), 0);
        for pair in SECTIONS.windows(2) {
            assert!(place(pair[0].first) < place(pair[1].first), "{} before {}", pair[0].title, pair[1].title);
        }
        assert_eq!(section_of(STEPS - 1), SECTIONS.len() - 1);
        assert!(SECTIONS.iter().all(|s| s.title.starts_with("Learn ")));
    }
}
