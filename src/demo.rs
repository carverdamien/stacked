//! `stacked demo`: a tour of the editor, a few minutes long, on a game of
//! tic-tac-toe played in a file, board.txt, a move a patch. Its database
//! and its repository are kept in memory (memory.rs): nothing is written
//! on disk. Each step says what to press, and why, and waits for you to do
//! it; Tab does it for you.

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
/// why. O's fourth move is a mistake, and X's fifth makes it pay for it.
const MOVES: [(char, usize, &str, &str); 5] = [
    ('X', 4, "X takes the center", "Four lines go through the center, more than through any other square."),
    ('O', 0, "O takes the top left corner", "A corner, the answer to the center."),
    ('X', 2, "X takes the top right corner", "With the center, the diagonal is X's but for its bottom left corner."),
    ('O', 8, "O takes the bottom right corner", "The corner across from O's first."),
    ('X', 6, "X takes the bottom left corner, and wins", "Three on the diagonal: the top right corner, the center, the bottom left."),
];

/// The fourth move and the fifth, as the stand-in for the agent makes them
/// again: O blocks the diagonal, and then X blocks O.
const FIXES: [(char, usize, &str, &str); 2] = [
    ('O', 6, "O blocks the diagonal at the bottom left", "X had the top right corner and the center: the bottom left corner was X's win."),
    ('X', 3, "X blocks O's left column", "O had the top left and the bottom left corners: the middle of the left side was O's win. X has two in the middle row, too."),
];

/// Why O's fix needs more than its patch, as the agent says it.
const MORE: &str = "O blocking the bottom left corner takes the square X wins on in patch 5: patch 5 has to change too";

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
/// writes. Asked to change the whole stack about O's mistake, it makes the
/// game again from there: O blocks the diagonal, and X then blocks O. Asked
/// to change patch 4 alone, it says that needs more than the patch, as
/// claude would, and the comment goes to the stack. Else it says what it is.
fn agent(path: &Path, id: &str) -> Result<()> {
    let run = Db::load(path)?.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let info = ask::info(&run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    let events = [
        json!({"type": "system", "subtype": "init", "model": "the tour's stand-in for claude"}),
        json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": 180}),
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": format!("stacked show {STACK}")}}]}}),
        json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "The bottom left corner is X's win in patch 5: O has to take it, and X then plays elsewhere."}]}}),
    ];
    for event in events {
        std::thread::sleep(PACE);
        Db::append(path, &[Record::Event { run: id.into(), event }])?;
    }
    std::thread::sleep(PACE);
    Db::edit(path, |db| {
        let s = db.stack(STACK)?.clone();
        let at = |title: &str| s.patches.iter().position(|p| db.patch(p).title() == title);
        let (mistake, win) = (at(MOVES[3].2), at(MOVES[4].2));
        let change = mistake.map(|m| db.patch(&s.patches[m]).change.clone());
        let on_mistake = matches!(&info.scope, ask::Scope::Patch(c) if Some(c) == change.as_ref());
        let both = mistake.zip(win).filter(|(m, w)| m < w);
        // Asked about patch 4 alone: the fix needs patch 5 too.
        if info.way == Way::Change && on_mistake && both.is_some() {
            for sh in &info.notes {
                if let Some(n) = db.note(&sh.change, &sh.id).cloned() {
                    ask::promote(db, STACK, &n, MORE)?;
                }
            }
            db.add_run(Run {
                state: RunState::Done,
                summary: format!("moved 1 comment to the stack: {MORE}"),
                pid: None,
                ..run
            })?;
            return Ok(());
        }
        let fix = both.filter(|_| info.way == Way::Change && info.scope == ask::Scope::Stack);
        let (reply, summary, rev) = match fix {
            Some((m, w)) => {
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
                let rev = db.set_stack(STACK, patches, "agent: the stack")?;
                (
                    "Patch 4: O takes the bottom left corner, which blocks X's diagonal. Patch 5 changes with it, as X can no longer win there: X takes the middle of the left side, which blocks O's left column.".to_string(),
                    format!("the agent changed the stack, @{rev}; 1 comment answered"),
                    Some(rev),
                )
            }
            None if info.way == Way::Change => (
                "I am the tour's stand-in for the agent, and only know O's mistake, in patch 4. In your stacks, claude reads your comment and the code around it, in a worktree of its own, changes what you asked, and answers here.".to_string(),
                "the agent answered".to_string(),
                None,
            ),
            None => (
                "Asked to answer only, I change nothing: O's mistake is fixed asking the whole stack. In your stacks, claude answers here, having read the code around it.".to_string(),
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

/// Where the tour is: its step, the patches it plays.
pub struct Tour {
    path: PathBuf,
    step: usize,
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
        Ok((
            app,
            Tour {
                path,
                step: 0,
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

    /// Where the patch of one of `titles` is in the stack.
    fn find(app: &App, titles: [&str; 2]) -> Option<usize> {
        let s = app.db.stack(STACK).ok()?;
        s.patches.iter().position(|p| titles.contains(&app.db.patch(p).title()))
    }

    /// The patch of O's mistake, or of its fix: where it is in the stack.
    fn mistake(&self, app: &App) -> Option<usize> {
        Tour::find(app, [MOVES[3].2, FIXES[0].2])
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
            5 => s.is_some_and(|s| s.patches.iter().any(|p| app.db.patch(p).title() == FIXES[0].2)),
            6 => app.screen == Screen::History,
            7 => app.screen == Screen::Main,
            8 => app.status.contains("does not apply") || last_op().starts_with("move"),
            9 => last_op().starts_with("redo"),
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
        // Asked about patch 4 alone, the comment went to the stack.
        let moved = {
            let (notes, on_patch) = self.asking(app);
            !on_patch && !notes.is_empty()
        };
        // A patch moved where it applied: what Undo takes back first.
        let reordered = app.db.log(Some(STACK)).iter().any(|(_, r)| matches!(r, Record::Stack(s) if s.op.starts_with("move")));
        let text = match self.step {
            0 => "Welcome to stacked, an editor of stacks of patches: you read them, comment on their lines, ask an agent about them, reorder them, and every edit is kept, to undo. This tour plays tic-tac-toe in a file, board.txt, a move a patch. It takes a few minutes, and writes nothing on disk. Press Enter to go on. Each step waits for you; Tab does it for you, and ctrl-c leaves.",
            1 => "The project: board.txt, an empty board. The stack, game, is empty too (`stacked new game --base main` makes one). Stacks are made of commits, as `stacked import game main..HEAD` makes them. Here the tour plays five moves, a patch each: press Enter.",
            2 => "Each move is a patch, bottom first, as they were committed. ↓ and ↑ pick one: pick patch 3, X takes the top right corner.",
            3 => "Press → to read it, in the middle: the line it changes, as it was (−) and is (+), among a few as they are. ↓↑ go through its lines; ← goes back to the patches.",
            4 => "Patch 4 is a mistake: O takes the bottom right corner, and in patch 5 X wins at the bottom left. Comment on patch 4, as in a review: ← to the patches, ↓ to patch 4, → to read it, ↓ to its + line, then Enter, write why, Esc, and Save it.",
            5 if moved => "Asked about patch 4 alone, the agent says the fix needs more than the patch, and your comment moved to the stack's own comments, the cursor with it. There, Space, then Ask agent: on the stack, it may change any patch.",
            5 => "Ask the agent to fix it. The fix changes patch 5 too, as X can no longer win where O blocks: so with the cursor on your comment, Space, then Ask agent, the whole stack. (A stand-in answers here, in memory; in your stacks claude does, in a worktree of its own.)",
            6 => "Patches 4 and 5 have new versions: O blocks the diagonal, and X blocks O's left column. Only patches that apply go in. History… shows what changed the stack: ← to the patches, ↑ to the stack's header (≡ game), Space, then History….",
            7 => "Newest first, each change of the stack, and beside it its stack diff: the patches as they were and are, and what changed in a new version, under it. ↓↑ for another; Esc leaves the history.",
            8 => "A patch goes only where it applies. Pick patch 5, X blocks O's left column; Space, Move this patch…, ↑ to the top, then Enter: refused, as it needs X in the center, from patch 1. stacked knows whether patches apply, not the rules of the game.",
            9 if reordered => "Every edit can be undone, and redone. Space, then Undo (among the menu's last items, named after the edit it takes back): your move is taken back, the patches in the order they were played. Then Space, then Redo: moved again. The tree of edits keeps every state the stack was in.",
            9 => "Every edit can be undone, and redone. Space, then Undo (among the menu's last items, named after the edit it takes back): the agent's change is taken back, and patches 4 and 5 are O's mistake and X's win again. Then Space, then Redo: the fix is back. The tree of edits keeps every state the stack was in.",
            10 if !app.sides_fit() => "Press 1: it switches how the code is drawn (unified, side by side, compact). Side by side needs a wider middle than this one: ← on the patches hides them (→ on the comments hides those). 2 switches to reading, where Space marks a block of code read. And ? says what can be done where the cursor is.",
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
                // Of the whole stack: from the patch, or from the stack the
                // comment went to.
                let (notes, on_patch) = self.asking(app);
                if notes.is_empty() {
                    return;
                }
                match ask::queue(&path, STACK, &notes, Way::Change, on_patch) {
                    Ok(()) => app.launch(),
                    Err(e) => fail(app, e),
                }
            }
            6 => app.screen = Screen::History,
            7 => app.screen = Screen::Main,
            8 => {
                let Some(at) = Tour::find(app, [MOVES[4].2, FIXES[1].2]) else { return };
                match Db::edit(&path, |db| crate::ops::move_patch(db, STACK, at, 0)) {
                    Ok(_) => app.status = "moved".into(),
                    Err(e) => fail(app, e),
                }
            }
            9 => {
                if let Err(e) = Db::edit(&path, |db| db.undo(STACK).and_then(|_| db.redo(STACK))) {
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

    /// What the tour does as time goes: the moves played one by one, and the
    /// next step once the step is done, by you or for you.
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
            self.entered = false;
            self.note.clear();
        }
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
        let mut foot = " Tab: do it for me · ctrl-c: leave ".to_string();
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
    }

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
