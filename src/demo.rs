//! `stacked demo`: a tour of the editor, a few minutes long, on a game of
//! tic-tac-toe played in a file, board.txt, a move a patch. Its database
//! and its repository are kept in memory (memory.rs): nothing is written
//! on disk. Each step says what to press, and why; Tab does it for you, as
//! waiting does, so that the tour ends in under five minutes.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use crate::ask;
use crate::db::{Db, Error, Note, Record, Result, Run, RunState, Way};
use crate::mbox::Draft;
use crate::paint::{color, put, window, wrap};
use crate::thread::{Role, append_turn};
use crate::tui::{App, Effect, Focus, Layout, Mode, Screen};

/// Where the tour's database is: kept in memory, and nowhere a file could
/// be made, should it not be.
const DB: &str = "/nonexistent/stacked-demo/tictactoe.jsonl";
const REPO: &str = "memory:tictactoe";
const STACK: &str = "game";
const FILE: &str = "board.txt";

/// The moves, a patch each: who, where (0 to 8, row by row), its title,
/// why.
const MOVES: [(char, usize, &str, &str); 5] = [
    ('X', 4, "X takes the center", "Four lines go through the center, more than through any other square."),
    ('O', 0, "O takes the top left corner", "A corner, against the center."),
    ('X', 2, "X takes the top right corner", "The diagonal through the center is X's now, but for one square."),
    ('O', 8, "O takes the bottom right corner", "The other corner."),
    ('X', 3, "X takes the middle of the left side", "Two in the middle row."),
];

/// O's mistake, patch 4, as the stand-in for the agent makes it again: O
/// blocks the diagonal.
const BLOCK: (usize, &str, &str) = (
    6,
    "O blocks the diagonal at the bottom left",
    "X had the top right corner and the center: the bottom left corner was X's win.",
);

/// How long the stand-in takes over each thing it writes.
const PACE: Duration = Duration::from_millis(600);

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

/// board.txt from `before` to `after`, as `git diff` writes it.
fn diff(before: &str, after: &str) -> String {
    let d = similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
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

/// The stand-in for the agent, answering asks on the tour's repository:
/// what claude would, made here, written a little at a time as claude
/// writes. Asked to change the patch of O's mistake, it makes O block the
/// diagonal; else it says what it is.
fn agent(path: &Path, id: &str) -> Result<()> {
    let run = Db::load(path)?.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let info = ask::info(&run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    let events = [
        json!({"type": "system", "subtype": "init", "model": "the tour's stand-in for claude"}),
        json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": 180}),
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": format!("stacked show {STACK}")}}]}}),
        json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "The bottom left corner is X's win: O has to take it."}]}}),
    ];
    for event in events {
        std::thread::sleep(PACE);
        Db::append(path, &[Record::Event { run: id.into(), event }])?;
    }
    std::thread::sleep(PACE);
    Db::edit(path, |db| {
        let s = db.stack(STACK)?.clone();
        let mistake = s.patches.iter().position(|p| db.patch(p).title() == MOVES[3].2);
        let on = |change: &str| match &info.scope {
            ask::Scope::Stack => true,
            ask::Scope::Patch(c) => c == change,
        };
        let fix = mistake.filter(|&at| info.way == Way::Change && on(&db.patch(&s.patches[at]).change));
        let (reply, summary, rev) = match fix {
            Some(at) => {
                let before = played(db, at)?;
                let after = mark(&before, BLOCK.0, 'O');
                let d = draft('O', BLOCK.1, BLOCK.2, diff(&before, &after), 4);
                let rev = crate::ops::replace(db, STACK, at, d, &format!("agent: patch {}", at + 1))?;
                (
                    "Moved O to the bottom left corner: X had the top right corner and the center, so that corner was X's win. The patches above still apply on it.".to_string(),
                    format!("agent: patch {}, @{rev}; 1 comment answered", at + 1),
                    Some(rev),
                )
            }
            None if info.way == Way::Change => (
                "I am the tour's stand-in for the agent, and only know O's mistake: ask me on patch 4. In your stacks, claude reads your comment and the code around it, in a worktree of its own, changes what you asked, and answers here.".to_string(),
                "the agent answered".to_string(),
                None,
            ),
            None => (
                "Asked to answer only, I change nothing: Ask agent, rather, lets me change the patch. In your stacks, claude answers here, having read the code around it.".to_string(),
                "the agent answered".to_string(),
                None,
            ),
        };
        for sh in &info.notes {
            if let Some(n) = db.note(&sh.change, &sh.id).cloned() {
                db.set_note(Note {
                    text: append_turn(&n.text, Role::Agent, &reply, Some(info.way.name())),
                    failed: None,
                    ..n
                })?;
            }
        }
        db.add_run(Run {
            state: RunState::Done,
            summary,
            output: reply.clone(),
            rev,
            pid: None,
            ..run
        })?;
        Ok(())
    })
}

/// Where the tour is: its step, since when, the patches it plays.
pub struct Tour {
    path: PathBuf,
    step: usize,
    since: Instant,
    /// The last key the reader pressed: the tour does not hurry them.
    pressed: Instant,
    /// Enter, at a step where it goes on.
    entered: bool,
    /// The moves still to play, as the tour adds them, and when it last did.
    adding: Option<(usize, Instant)>,
    /// What the tour says of itself at the step: it did it for you.
    note: String,
    over: bool,
}

/// The tour's steps.
pub const STEPS: usize = 12;

impl Tour {
    /// The tour, at its start: an empty stack on a board of nothing, in
    /// memory, and the editor on it.
    pub fn new() -> Result<(App, Tour)> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(if n == 0 { DB.to_string() } else { DB.replace(".jsonl", &format!("-{}.jsonl", n + 1)) });
        crate::memory::keep(&path);
        let empty = board(&['.'; 9]);
        let base = sha1_smol::Sha1::from(&empty).digest().to_string();
        crate::memory::plant(REPO, &base, [(FILE.to_string(), empty)].into());
        crate::memory::stand_in(agent);
        Db::edit(&path, |db| db.new_stack(STACK, &base, REPO))?;
        let app = App::new(&path)?;
        let now = Instant::now();
        Ok((
            app,
            Tour {
                path,
                step: 0,
                since: now,
                pressed: now,
                entered: false,
                adding: None,
                note: String::new(),
                over: false,
            },
        ))
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn over(&self) -> bool {
        self.over
    }

    /// How long the tour takes, were nothing pressed: every step's wait.
    pub fn longest() -> Duration {
        (0..STEPS).map(wait).sum()
    }

    /// The patch of O's mistake, or of its fix: where it is in the stack.
    fn mistake(&self, app: &App) -> Option<usize> {
        let s = app.db.stack(STACK).ok()?;
        s.patches
            .iter()
            .position(|p| [MOVES[3].2, BLOCK.1].contains(&app.db.patch(p).title()))
    }

    /// Whether what the step asks is done.
    pub fn done(&self, app: &App) -> bool {
        let s = app.db.stack(STACK).ok();
        let n = s.map_or(0, |s| s.patches.len());
        // What the stack's last record did: an edit, or a move of its head.
        let last_op = || match app.db.log(Some(STACK)).last() {
            Some((_, Record::Stack(s))) => s.op.clone(),
            Some((_, Record::Head(h))) => h.op.clone(),
            _ => String::new(),
        };
        match self.step {
            0 => self.entered,
            1 => n == MOVES.len() && self.adding.is_none(),
            2 => app.focus == Focus::List && !app.on_stack && app.sel == 2,
            3 => app.focus == Focus::Diff && !app.on_stack && app.sel == 2,
            4 => self.mistake(app).is_some_and(|at| {
                let change = &app.db.patch(&s.unwrap().patches[at]).change;
                !app.db.notes(change).is_empty()
            }),
            5 => s.is_some_and(|s| s.patches.iter().any(|p| app.db.patch(p).title() == BLOCK.1)),
            6 => app.screen == Screen::History,
            7 => app.screen == Screen::Main,
            8 => last_op().starts_with("move"),
            9 => last_op().starts_with("undo"),
            10 => app.layout != Layout::Unified || app.reading,
            _ => self.entered,
        }
    }

    /// Whether Enter is the tour's at this step: only to go on.
    fn enters(&self) -> bool {
        matches!(self.step, 0 | 1) || self.step == STEPS - 1
    }

    /// What the tour says at the step, its keys as the settings name them;
    /// and, at its end, the board.
    pub fn say(&self, app: &App) -> (String, Option<String>) {
        let text = match self.step {
            0 => "Welcome to stacked, an editor of stacks of patches: you read them, comment on their lines, ask an agent about them, reorder them, and every edit is kept, to undo. This tour plays tic-tac-toe in a file, board.txt, a move a patch. It takes about four minutes, and writes nothing on disk. Press Enter to go on. At every step Tab does it for you, as does waiting; ctrl-c leaves.",
            1 => "The project: board.txt, an empty board. The stack, game, is empty too (`stacked new game --base main` makes one). Stacks are made of commits, as `stacked import game main..HEAD` makes them. Here the tour plays five moves, a patch each: press Enter.",
            2 => "Each move is a patch, bottom first, as they were committed. ↓ and ↑ pick one: pick patch 3, X takes the top right corner.",
            3 => "Press → to read it, in the middle: the line it changes, as it was (−) and is (+), among a few as they are. ↓↑ go through its lines; ← goes back to the patches.",
            4 => "Patch 4 is a mistake: O takes the bottom right corner, and X can win at the bottom left. Comment on it, as in a review: ← to the patches, ↓ to patch 4, → to read it, ↓ to its + line, then Enter, write why, Esc, and Save it.",
            5 => "Ask the agent: with the cursor on your comment, Space, then Ask agent. It may change the patch your comment is on, and answers in the thread. (A stand-in answers here, in memory; in your stacks claude does, in a worktree of its own.)",
            6 => "Patch 4 has a new version: O blocks the diagonal, and patch 5 still applies on it. Only patches that apply go in. History… shows what changed the stack: ← to the patches, ↑ to the stack's header (≡ game), Space, then History….",
            7 => "Newest first, each change of the stack, and beside it its stack diff: the patches as they were and are, and what changed in a new version, under it. ↓↑ for another; Esc leaves the history.",
            8 => "A patch moves where it still applies. Pick patch 5, X takes the middle of the left side; Space, Move this patch…, then ↑ twice and Enter: it goes before patch 3. (Above patch 1, it would not apply: it needs X in the center.)",
            9 => "Every edit can be undone, and redone: Space, then Undo, among the menu's last items. The tree of edits keeps every state the stack was in: nothing is lost.",
            10 => "Press 1: it switches how the code is drawn (unified, side by side, compact). 2 switches to reading, where Space marks a block of code read. And ? says what can be done where the cursor is.",
            _ => "That is stacked. On a project of yours: `stacked import NAME main..HEAD` makes a stack of its commits, `stacked` opens the editor on it, `stacked web` in a browser, `stacked --help` says the rest. The board, its patches applied, below. Press Enter to leave.",
        };
        let end = (self.step == STEPS - 1).then(|| {
            let n = app.db.stack(STACK).map_or(0, |s| s.patches.len());
            played(&app.db, n).unwrap_or_default()
        });
        (app.say_keys(text.to_string()), end)
    }

    /// The step done for you.
    pub fn show(&mut self, app: &mut App) {
        let path = self.path.clone();
        let fail = |app: &mut App, e: Error| app.status = e.0;
        match self.step {
            0 | 11 => self.entered = true,
            1 => {
                if self.adding.is_none() && app.db.stack(STACK).map_or(0, |s| s.patches.len()) == 0 {
                    self.adding = Some((0, Instant::now() - Duration::from_secs(1)));
                }
            }
            2 => {
                app.focus = Focus::List;
                app.on_stack = false;
                app.sel = 2;
            }
            3 => {
                app.on_stack = false;
                app.sel = 2;
                app.focus = Focus::Diff;
            }
            4 => {
                let Some(at) = self.mistake(app) else { return };
                let p = app.db.patch(&app.db.stack(STACK).unwrap().patches[at]).clone();
                let Some(line) = crate::view::note_lines(&p).into_iter().find(|l| l.origin == '+') else { return };
                let note = Note {
                    change: p.change.clone(),
                    id: line.key.clone(),
                    line: line.key.clone(),
                    text: "O has to block the diagonal: X wins at the bottom left.".into(),
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
            5 => {
                let Some(at) = self.mistake(app) else { return };
                let change = app.db.patch(&app.db.stack(STACK).unwrap().patches[at]).change.clone();
                let busy = ask::with_agent(&app.db);
                let notes: Vec<(String, String)> = app
                    .db
                    .notes(&change)
                    .into_iter()
                    .filter(|n| ask::sendable(n) && !busy.contains_key(&(n.change.clone(), n.id.clone())))
                    .map(|n| (n.change.clone(), n.id.clone()))
                    .collect();
                if notes.is_empty() {
                    return;
                }
                match ask::queue(&path, STACK, &notes, Way::Change, false) {
                    Ok(()) => app.launch(),
                    Err(e) => fail(app, e),
                }
            }
            6 => app.screen = Screen::History,
            7 => app.screen = Screen::Main,
            8 => {
                if let Err(e) = Db::edit(&path, |db| crate::ops::move_patch(db, STACK, 4, 2)) {
                    fail(app, e);
                }
            }
            9 => {
                if let Err(e) = Db::edit(&path, |db| db.undo(STACK)) {
                    fail(app, e);
                }
            }
            _ => {
                app.key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
                if app.layout == Layout::Unified {
                    app.key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
                }
            }
        }
        app.refresh();
    }

    /// What the tour does as time goes: the moves played one by one, the
    /// step done for you once waited for, the next once it is done.
    pub fn tick(&mut self, app: &mut App) {
        if let Some((k, at)) = self.adding
            && at.elapsed() >= Duration::from_millis(400)
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
        if self.done(app) {
            if self.step == STEPS - 1 {
                self.over = true;
                return;
            }
            self.step += 1;
            self.since = Instant::now();
            self.entered = false;
            self.note.clear();
            return;
        }
        // Done for you, once waited for, the reader doing nothing.
        let idle = self.since.max(self.pressed).elapsed();
        if idle >= wait(self.step) && matches!(app.mode, Mode::Normal) && self.adding.is_none() {
            self.note = "done for you".into();
            self.show(app);
            self.since = Instant::now();
        }
    }

    /// What is left to wait, at the step.
    fn left(&self) -> Duration {
        wait(self.step).saturating_sub(self.since.max(self.pressed).elapsed())
    }

    /// The editor drawn into `buf`, the tour's words under it: where the
    /// terminal's cursor goes.
    pub fn draw(&self, app: &mut App, buf: &mut Buffer) -> Option<(u16, u16)> {
        let area = buf.area;
        let (w, h) = (area.width as usize, area.height as usize);
        let (text, end) = self.say(app);
        let mut lines = wrap(&text, w.saturating_sub(4).max(10));
        if let Some(board) = end {
            lines.push(String::new());
            lines.extend(board.lines().map(|l| format!("    {l}")));
        }
        // Too small for both: the editor alone.
        if h < 12 || w < 40 {
            return app.draw(buf);
        }
        let ph = (lines.len() + 2).min(h / 2);
        let top = Rect::new(area.x, area.y, area.width, (h - ph) as u16);
        let mut screen = Buffer::empty(top);
        let cursor = app.draw(&mut screen);
        buf.merge(&screen);
        let title = format!("stacked demo · {} of {STEPS}", self.step + 1);
        window(buf, area, 0, h - ph, w, ph, &title, color::comment());
        for (i, l) in lines.iter().take(ph.saturating_sub(2)).enumerate() {
            put(buf, area, 2, h - ph + 1 + i, w.saturating_sub(4), l, Style::new());
        }
        let mut foot = format!(" Tab: do it for me · ctrl-c: leave · on its own in {}s ", self.left().as_secs());
        if !self.note.is_empty() {
            foot = format!(" {} ·{foot}", self.note);
        }
        let x = w.saturating_sub(foot.chars().count() + 2);
        put(buf, area, x, h - 1, w - x, &foot, color::meta().add_modifier(Modifier::DIM));
        cursor
    }

    /// The tour, in the terminal, until its end or ctrl-c.
    fn play(&mut self, terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
        let io = |e: std::io::Error| Error(e.to_string());
        loop {
            app.refresh();
            self.tick(app);
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
            self.pressed = Instant::now();
            if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
            let writing = matches!(app.mode, Mode::Edit(_));
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

/// How long step `k` waits for the reader before it is done for them.
fn wait(k: usize) -> Duration {
    Duration::from_secs(match k {
        0 => 15,
        1 => 10,
        4 => 40,
        5 | 6 => 20,
        8 => 25,
        k if k == STEPS - 1 => 20,
        _ => 15,
    })
}

/// The tour, in this terminal.
pub fn run() -> Result<()> {
    let (mut app, mut tour) = Tour::new()?;
    let mut terminal = ratatui::init();
    let res = tour.play(&mut terminal, &mut app);
    ratatui::restore();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_are_marks_on_the_board() {
        let empty = board(&['.'; 9]);
        assert_eq!(empty.lines().count(), 13);
        let x = mark(&empty, 4, 'X');
        assert_eq!(x.lines().nth(7).unwrap(), "  .  |  X  |  .");
        let o = mark(&x, 6, 'O');
        assert_eq!(o.lines().nth(11).unwrap(), "  O  |  .  |  .");
        // The rows far enough apart that a move in one, its context with
        // it, touches no other row's marks.
        let d = diff(&empty, &x);
        assert!(d.starts_with("diff --git a/board.txt b/board.txt\n--- a/board.txt\n+++ b/board.txt\n@@ "), "{d}");
        assert!(!d.lines().any(|l| l == "   .  |  .  |  ."), "another row's marks, as context:\n{d}");
    }
}
