//! StackED's windows on macOS: one for each database opened, each with an
//! editor of its own (`Session`), all in one process. Each window is a
//! viewport of eframe's, deferred: painted by itself, when it needs to be.
//! eframe's root viewport, which it cannot do without, is kept hidden; its
//! passes keep the set of windows. What the File menu opens comes in by a
//! channel. Closing the last window leaves StackED running, as macOS has
//! it, until it is quit.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use super::{Session, Window};
use crate::db::{Db, Error, Result};
use crate::state;

/// A database opened for a window of its own: where it is, and its editor.
pub struct Opened {
    pub path: PathBuf,
    pub session: Session,
}

/// The folder the editor of the database at `db` runs in, as if started
/// there: the database's own.
pub fn folder(db: &Path) -> PathBuf {
    match db.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_default(),
    }
}

/// The database at `db` opened for a window of its own, its editor run in
/// the database's folder.
pub fn open(db: &Path) -> Result<Opened> {
    let path = std::path::absolute(db).unwrap_or_else(|_| db.to_path_buf());
    let session = Session::open(&path, &folder(&path))?;
    Ok(Opened { path, session })
}

/// What is said when the database at `db` cannot be opened, `e` being why.
pub fn not_opened(db: &Path, e: &Error) -> String {
    format!("StackED cannot open {}: {}", db.display(), e.0)
}

/// What Open Recent calls each of `dbs`: its file's name, with its folder
/// after it when another of the same name is listed too.
pub fn titles(dbs: &[PathBuf]) -> Vec<String> {
    let name = |p: &PathBuf| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
    let home = std::env::var_os("HOME").map(PathBuf::from);
    dbs.iter()
        .map(|p| {
            let n = name(p);
            match p.parent() {
                Some(dir) if dbs.iter().filter(|q| name(q) == n).count() > 1 => {
                    let dir = match home.as_deref().and_then(|h| dir.strip_prefix(h).ok()) {
                        Some(rest) => Path::new("~").join(rest),
                        None => dir.to_path_buf(),
                    };
                    format!("{n} — {}", dir.display())
                }
                _ => n,
            }
        })
        .collect()
}

/// What the File menu asks of the system: AppKit's panels and alerts on
/// macOS (`menu.rs`), the tests' stand-ins in theirs.
pub trait Ask {
    /// The databases picked in the Open panel, any number of .jsonl files:
    /// none if it is cancelled.
    fn databases(&self) -> Vec<PathBuf>;
    /// Where a new database is to be made, as named in the Save panel: none
    /// if it is cancelled.
    fn new_database(&self) -> Option<PathBuf>;
    /// `text` said in an alert.
    fn alert(&self, text: &str);
}

/// What the File menu's items do, their menu aside: each database opened
/// is sent to the window set, which is then woken to take it.
pub struct File {
    to: Sender<Opened>,
    wake: Box<dyn Fn()>,
    state: PathBuf,
}

impl File {
    /// The File menu of the window set `to` sends to and `wake` wakes, the
    /// recent databases kept in the state file `state`.
    pub fn new(to: Sender<Opened>, wake: Box<dyn Fn()>, state: PathBuf) -> File {
        File { to, wake, state }
    }

    /// File → Open…: each database picked opened in a new window, even one
    /// open in another window already.
    pub fn open(&self, ask: &dyn Ask) {
        for db in ask.databases() {
            self.open_one(&db, ask);
        }
    }

    /// File → New…: an empty database made where the Save panel says, then
    /// opened in a new window. A file there already is never replaced (the
    /// panel offers to): nothing is made, and an alert says why.
    pub fn create(&self, ask: &dyn Ask) {
        let Some(db) = ask.new_database() else { return };
        let there = || std::fs::symlink_metadata(&db).is_ok();
        let refused = || {
            ask.alert(&format!(
                "{} is there already. StackED does not replace a file: delete it first, outside StackED, to make a new database of that name.",
                db.display()
            ))
        };
        if there() {
            return refused();
        }
        match Db::create(&db) {
            Ok(()) => self.open_one(&db, ask),
            // Made meanwhile, by something else: left as it is.
            Err(_) if there() => refused(),
            Err(e) => ask.alert(&format!("StackED cannot make the database {}", e.0)),
        }
    }

    /// The databases File → Open Recent lists, the most recent first: those
    /// no longer there are taken off the list kept.
    pub fn recent(&self) -> Vec<PathBuf> {
        state::recent(&self.state)
    }

    /// An item of File → Open Recent: its database opened in a new window.
    /// One no longer there is said to be so, and taken off the list.
    pub fn open_recent(&self, db: &Path, ask: &dyn Ask) {
        if db.is_file() {
            return self.open_one(db, ask);
        }
        state::recent(&self.state);
        ask.alert(&format!("{} is no longer there: StackED took it off Open Recent.", db.display()));
    }

    /// File → Open Recent → Clear Menu: no database is recent.
    pub fn clear(&self) {
        state::clear(&self.state);
    }

    /// The database at `db` opened in a new window, else why not said.
    fn open_one(&self, db: &Path, ask: &dyn Ask) {
        match open(db) {
            Ok(opened) => {
                let _ = self.to.send(opened);
                (self.wake)();
            }
            Err(e) => ask.alert(&not_opened(db, &e)),
        }
    }
}

/// A window of the set: its viewport, the database it is on, and the
/// window itself, which its viewport's passes paint.
struct Shown {
    id: egui::ViewportId,
    path: PathBuf,
    builder: egui::ViewportBuilder,
    window: Arc<Mutex<Window>>,
}

/// StackED's windows, as eframe runs them from its hidden root viewport.
pub struct Windows {
    shown: Vec<Shown>,
    /// The editors of the windows closed, until they have left.
    leaving: Vec<(PathBuf, Session)>,
    /// What the File menu opens.
    from: Receiver<Opened>,
    /// Which faces the fonts have of their own, and the program `$EDITOR`
    /// runs, as each window has them.
    real: [bool; 4],
    program: String,
    /// The windows made so far: each a viewport of its own.
    made: u64,
    /// The state file, where the recent databases are kept; and, for
    /// StackED.app (`keeps`), the windows open, those last kept in `kept`.
    state: PathBuf,
    keeps: bool,
    kept: Option<Vec<PathBuf>>,
    /// Says what went wrong: in an alert, opened from the Dock; else on
    /// stderr.
    say: Box<dyn Fn(String)>,
}

impl Windows {
    /// No window yet: they come from `from`, each with the fonts' faces
    /// `real` and the name of `$EDITOR`'s program; the state is kept in
    /// `state`, the windows open too if `keeps`; what goes wrong is told to
    /// `say`.
    pub fn new(from: Receiver<Opened>, state: PathBuf, keeps: bool, real: [bool; 4], program: String, say: Box<dyn Fn(String)>) -> Windows {
        Windows {
            shown: Vec::new(),
            leaving: Vec::new(),
            from,
            real,
            program,
            made: 0,
            state,
            keeps,
            kept: None,
            say,
        }
    }

    /// A pass of the root viewport: its window hidden (eframe shows it once
    /// painted, and nothing is to show it again) and never closed but by
    /// Quit; what the File menu opened given windows of their own, the
    /// windows closed let go, and the rest shown.
    fn pass(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        while let Ok(opened) = self.from.try_recv() {
            self.add(ctx, opened);
        }
        let (gone, shown): (Vec<Shown>, Vec<Shown>) =
            std::mem::take(&mut self.shown).into_iter().partition(|s| s.window.lock().unwrap_or_else(PoisonError::into_inner).gone);
        self.shown = shown;
        for s in gone {
            self.let_go(s);
        }
        self.reap();
        for s in &self.shown {
            show(ctx, s);
        }
        self.keep();
    }

    /// `opened` given a window, its viewport's own, its database now the
    /// most recent.
    fn add(&mut self, ctx: &egui::Context, opened: Opened) {
        let id = egui::ViewportId::from_hash_of(("StackED window", self.made));
        self.made += 1;
        let _ = opened.session.waker.set((ctx.clone(), id));
        state::opened(&self.state, &opened.path);
        self.shown.push(Shown {
            id,
            builder: super::builder(&opened.path),
            path: opened.path,
            window: Arc::new(Mutex::new(Window::with(opened.session, self.real, self.program.clone()))),
        });
    }

    /// The window `shown` closed: its editor told so, to leave on its
    /// thread, which is not waited for here.
    fn let_go(&mut self, shown: Shown) {
        let mut window = shown.window.lock().unwrap_or_else(PoisonError::into_inner);
        let mut session = std::mem::replace(&mut window.session, Session::gone());
        session.let_go();
        self.leaving.push((shown.path, session));
    }

    /// The editors of windows closed that have left: done with, and said if
    /// one stopped on an error of its own.
    fn reap(&mut self) {
        let say = &self.say;
        self.leaving.retain_mut(|(path, session)| {
            if !session.left() {
                return true;
            }
            session.close();
            if session.failed.load(std::sync::atomic::Ordering::Relaxed) {
                say(stopped(path));
            }
            false
        });
    }

    /// The windows open kept in the state, if they changed and StackED.app
    /// keeps them, to open them again when it next starts.
    fn keep(&mut self) {
        if !self.keeps {
            return;
        }
        let now: Vec<PathBuf> = self.shown.iter().map(|s| s.path.clone()).collect();
        if self.kept.as_ref() != Some(&now) {
            state::keep_open(&self.state, &now);
            self.kept = Some(now);
        }
    }

    /// Quit: the windows open kept, then every editor told to leave at
    /// once, and each waited for as it does what it does on leaving.
    fn quit(&mut self) {
        self.kept = None;
        self.keep();
        let shown = std::mem::take(&mut self.shown);
        for s in shown {
            self.let_go(s);
        }
        for (path, session) in &mut self.leaving {
            session.close();
            if session.failed.load(std::sync::atomic::Ordering::Relaxed) {
                // Not eprintln!, which panics with nowhere to say it.
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "stacked: {}", stopped(path));
            }
        }
        self.leaving.clear();
    }
}

/// What is said of the editor of the database at `path` that stopped on an
/// error of its own (a panic, said on stderr as it happened).
fn stopped(path: &Path) -> String {
    format!("the editor of {} stopped on an error", path.display())
}

/// The window `shown`, its viewport's pass to be run whenever it needs
/// painting: closed by its button or File → Close Window, or its editor
/// quit, it is gone, for the root's next pass to let go.
fn show(ctx: &egui::Context, shown: &Shown) {
    let window = shown.window.clone();
    ctx.show_viewport_deferred(shown.id, shown.builder.clone(), move |ctx, _| {
        let mut window = window.lock().unwrap_or_else(PoisonError::into_inner);
        if ctx.input(|i| i.viewport().close_requested()) {
            window.gone = true;
        }
        if !window.gone {
            window.frame(ctx);
        }
        if window.gone {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    });
}

impl eframe::App for Windows {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pass(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        super::BG.to_normalized_gamma_f32()
    }

    /// Quit (⌘Q, or macOS logging out): each window's editor leaves as it
    /// does when its window closes, and is waited for.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.quit();
    }
}

/// StackED's windows until it is quit: one for each of `first`, opened
/// already, and one for each database the File menu opens; `said`, what
/// went wrong opening others, is said in alerts once StackED runs. `keeps`
/// for StackED.app, whose windows open are kept, to open them again. The
/// state is kept where stacked keeps it (`home`).
#[cfg(target_os = "macos")]
pub fn run(first: Vec<Opened>, said: Vec<String>, keeps: bool) -> Result<()> {
    let state = crate::home::get().state.clone();
    let (to, from) = std::sync::mpsc::channel();
    for opened in first {
        let _ = to.send(opened);
    }
    let options = eframe::NativeOptions {
        // The root viewport's window, hidden once painted: small and bare
        // meanwhile. Its icon is the Dock's.
        viewport: egui::ViewportBuilder::default()
            .with_title("StackED")
            .with_app_id("stacked")
            .with_icon(Arc::new(crate::icon::data(512)))
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size([1.0, 1.0]),
        ..Default::default()
    };
    eframe::run_native(
        "StackED",
        options,
        Box::new(move |cc| {
            let (fonts, real) = super::fonts();
            cc.egui_ctx.set_fonts(fonts);
            let ctx = cc.egui_ctx.clone();
            let wake = move || ctx.request_repaint_of(egui::ViewportId::ROOT);
            super::menu::install(File::new(to, Box::new(wake), state.clone()));
            for text in said {
                super::menu::alert_later(&text);
            }
            let say: Box<dyn Fn(String)> = match crate::bundle::no_terminal() {
                true => Box::new(|text| super::menu::alert_later(&text)),
                false => Box::new(|text| {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "stacked: {text}");
                }),
            };
            let program = super::program(&crate::config::get().editor());
            Ok(Box::new(Windows::new(from, state, keeps, real, program, say)))
        }),
    )
    .map_err(|e| Error(format!("StackED cannot open its windows: {e}")))
}

#[cfg(test)]
mod tests {
    use super::super::{Input, Output, PAD};
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    /// A session whose editor is the test: what its window sends it, and
    /// where to send it what the editor would.
    fn stand_in() -> (Session, Receiver<Input>, Sender<Output>) {
        let (to, inputs) = mpsc::channel();
        let (outputs, from) = mpsc::channel();
        let session = Session {
            to: Some(to),
            from,
            thread: None,
            waker: Arc::default(),
            failed: Arc::default(),
        };
        (session, inputs, outputs)
    }

    /// The window set's root viewport's pass.
    fn root(ctx: &egui::Context, windows: &mut Windows) -> egui::FullOutput {
        let input = egui::RawInput {
            viewport_id: egui::ViewportId::ROOT,
            ..Default::default()
        };
        ctx.run(input, |ctx| windows.pass(ctx))
    }

    /// A pass of the window of viewport `id`, as `out`, the root's, said to
    /// run it, its window 800 by 600, `events` its viewport's (a close).
    fn window(ctx: &egui::Context, out: &egui::FullOutput, id: egui::ViewportId, events: Vec<egui::ViewportEvent>) {
        let pass = out.viewport_output[&id].viewport_ui_cb.clone().expect("a deferred viewport");
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        input.viewports.insert(id, egui::ViewportInfo { events, ..Default::default() });
        let _ = ctx.run(input, |ctx| pass(ctx));
    }

    /// A pass of the window of viewport `id`, as `window` runs one, the
    /// window the active one or not as `focused` says, which eframe says
    /// twice: in the input, and in the viewport's info. `events`, what was
    /// done in the window.
    fn window_focused(ctx: &egui::Context, out: &egui::FullOutput, id: egui::ViewportId, focused: bool, events: Vec<egui::Event>) {
        let pass = out.viewport_output[&id].viewport_ui_cb.clone().expect("a deferred viewport");
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events,
            focused,
            ..Default::default()
        };
        let info = egui::ViewportInfo {
            focused: Some(focused),
            ..Default::default()
        };
        input.viewports.insert(id, info);
        let _ = ctx.run(input, |ctx| pass(ctx));
    }

    /// The windows `out` shows, but the root.
    fn windows_in(out: &egui::FullOutput) -> Vec<egui::ViewportId> {
        let mut ids: Vec<egui::ViewportId> = out.viewport_output.keys().copied().filter(|id| *id != egui::ViewportId::ROOT).collect();
        ids.sort_by_key(|id| format!("{id:?}"));
        ids
    }

    /// A window set keeping its state in `state`, and its windows open too
    /// if `keeps`; what opens windows in it; and its context.
    fn set(state: &Path, keeps: bool) -> (Windows, Sender<Opened>, egui::Context) {
        let (to, from) = mpsc::channel();
        let ctx = egui::Context::default();
        let (fonts, real) = super::super::fonts();
        ctx.set_fonts(fonts);
        // As eframe has it: each viewport a window of its own.
        ctx.set_embed_viewports(false);
        let windows = Windows::new(from, state.to_path_buf(), keeps, real, "vim".into(), Box::new(|_| {}));
        (windows, to, ctx)
    }

    /// Whether the editor `inputs` come to was told to leave: its channel
    /// closed, once what was sent before is read.
    fn told_to_leave(inputs: &Receiver<Input>) -> bool {
        loop {
            match inputs.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return true,
                Err(mpsc::RecvTimeoutError::Timeout) => return false,
            }
        }
    }

    #[test]
    fn each_database_opened_is_a_window_of_its_own_with_its_own_editor() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, false);
        // No window: the root's own hidden, and nothing else.
        let out = root(&ctx, &mut windows);
        let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::Visible(false)), "{commands:?}");
        assert!(windows_in(&out).is_empty());
        // Two databases opened, the same one twice: three windows.
        let a = tmp.path().join("a.jsonl");
        let b = tmp.path().join("b.jsonl");
        let mut editors = Vec::new();
        for path in [&a, &b, &a] {
            std::fs::write(path, "").unwrap();
            let (session, inputs, outputs) = stand_in();
            to.send(Opened { path: path.clone(), session }).unwrap();
            editors.push((inputs, outputs));
        }
        let out = root(&ctx, &mut windows);
        let ids = windows_in(&out);
        assert_eq!(ids.len(), 3);
        assert_eq!(out.viewport_output[&ids[0]].builder.title.as_deref().map(|t| t.starts_with("StackED · ")), Some(true));
        // Each window's pass tells its own editor the grid's size.
        for id in &ids {
            window(&ctx, &out, *id, vec![]);
        }
        let cell = |inputs: &Receiver<Input>| match inputs.try_iter().next() {
            Some(Input::Size(cols, rows)) => (cols, rows),
            other => panic!("{other:?}"),
        };
        for (inputs, _) in &editors {
            let (cols, rows) = cell(inputs);
            assert!(cols as f32 > (800.0 - 2.0 * PAD) / 20.0 && rows > 10, "{cols} by {rows}");
        }
        // Recent, the latest first, once each.
        assert_eq!(state::recent(&state), [a.clone(), b.clone()]);
        // Not StackED.app's: no windows kept to open again.
        assert!(state::reopen(&state).is_empty());
    }

    #[test]
    fn a_window_closed_ends_its_editor_and_the_last_closed_leaves_stacked_running() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, true);
        let (a, b) = (tmp.path().join("a.jsonl"), tmp.path().join("b.jsonl"));
        let mut editors = Vec::new();
        for path in [&a, &b] {
            std::fs::write(path, "").unwrap();
            let (session, inputs, outputs) = stand_in();
            to.send(Opened { path: path.clone(), session }).unwrap();
            editors.push((inputs, outputs));
        }
        let out = root(&ctx, &mut windows);
        let ids: Vec<egui::ViewportId> = windows.shown.iter().map(|s| s.id).collect();
        assert_eq!(windows_in(&out).len(), 2);
        // StackED.app's: the windows open kept, to open again.
        assert_eq!(state::reopen(&state), [a.clone(), b.clone()]);
        // The first closed by its button (File → Close Window is the same):
        // gone at the root's next pass, its editor told so, the other's not.
        window(&ctx, &out, ids[0], vec![egui::ViewportEvent::Close]);
        let out = root(&ctx, &mut windows);
        assert_eq!(windows_in(&out), [ids[1]]);
        assert!(told_to_leave(&editors[0].0));
        assert!(matches!(editors[1].0.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert_eq!(state::reopen(&state), std::slice::from_ref(&b));
        // The second's editor quit: its window gone too. StackED runs on, no
        // window open, its root not closed.
        editors[1].1.send(Output::Bye).unwrap();
        window(&ctx, &out, ids[1], vec![]);
        let out = root(&ctx, &mut windows);
        assert!(windows_in(&out).is_empty());
        assert!(!out.viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::Close));
        assert!(told_to_leave(&editors[1].0));
        assert!(state::reopen(&state).is_empty());
        assert_eq!(state::recent(&state), [b.clone(), a.clone()], "both recent still");
        // The root's window asked to close, as it never is but by Quit: not.
        let input = egui::RawInput {
            viewport_id: egui::ViewportId::ROOT,
            viewports: std::iter::once((egui::ViewportId::ROOT, egui::ViewportInfo { events: vec![egui::ViewportEvent::Close], ..Default::default() })).collect(),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| windows.pass(ctx));
        assert!(out.viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::CancelClose));
    }

    #[test]
    fn each_window_follows_the_mouse_only_while_it_is_the_active_one() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, false);
        let mut editors = Vec::new();
        for name in ["a.jsonl", "b.jsonl"] {
            let path = tmp.path().join(name);
            std::fs::write(&path, "").unwrap();
            let (session, inputs, outputs) = stand_in();
            to.send(Opened { path, session }).unwrap();
            editors.push((inputs, outputs));
        }
        let out = root(&ctx, &mut windows);
        let ids: Vec<egui::ViewportId> = windows.shown.iter().map(|s| s.id).collect();
        for id in &ids {
            window(&ctx, &out, *id, vec![]);
        }
        let cell = windows.shown[0].window.lock().unwrap().cell;
        let on = |x: u16, y: u16| egui::pos2(PAD + (x as f32 + 0.5) * cell.x, PAD + (y as f32 + 0.5) * cell.y);
        let moved = |x, y| egui::Event::PointerMoved(on(x, y));
        // What each window sent its editor, but the grid's size.
        let sent = |w: usize| editors[w].0.try_iter().filter(|i| !matches!(i, Input::Size(..))).collect::<Vec<_>>();
        // The second window the active one, the first behind it: the mouse
        // going over each in turn is followed over the second only.
        window_focused(&ctx, &out, ids[0], false, vec![moved(1, 1)]);
        window_focused(&ctx, &out, ids[1], true, vec![moved(2, 2)]);
        assert_eq!(sent(0), []);
        assert_eq!(sent(1), [Input::Hover(2, 2)]);
        // A click on the first makes it the active one: the second's hover
        // goes, and the first says the mouse over the cell where it was last
        // seen, at once, then the click.
        let button = |pressed| egui::Event::PointerButton {
            pos: on(1, 1),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        window_focused(&ctx, &out, ids[1], false, vec![egui::Event::WindowFocused(false)]);
        window_focused(&ctx, &out, ids[0], true, vec![egui::Event::WindowFocused(true), moved(1, 1), button(true), button(false)]);
        assert_eq!(sent(1), [Input::Unhover]);
        assert_eq!(sent(0), [Input::Hover(1, 1), Input::Click { x: 1, y: 1, right: false }]);
        // Each keeps its own: the mouse over the second, behind now, is not
        // followed; over the first, it is.
        window_focused(&ctx, &out, ids[1], false, vec![moved(3, 3)]);
        window_focused(&ctx, &out, ids[0], true, vec![moved(4, 4)]);
        assert_eq!(sent(1), []);
        assert_eq!(sent(0), [Input::Hover(4, 4)]);
    }

    #[test]
    fn quit_keeps_the_windows_open_and_waits_for_every_editor_to_leave() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, true);
        let mut left = Vec::new();
        for name in ["a.jsonl", "b.jsonl"] {
            let path = tmp.path().join(name);
            std::fs::write(&path, "").unwrap();
            // An editor on a thread, which takes a moment to leave once told.
            let (mut session, inputs, _outputs) = stand_in();
            let done = Arc::new(AtomicBool::new(false));
            let flag = done.clone();
            session.thread = Some(std::thread::spawn(move || {
                while inputs.recv().is_ok() {}
                std::thread::sleep(std::time::Duration::from_millis(200));
                flag.store(true, Ordering::Relaxed);
            }));
            to.send(Opened { path, session }).unwrap();
            left.push(done);
        }
        root(&ctx, &mut windows);
        windows.quit();
        assert!(left.iter().all(|l| l.load(Ordering::Relaxed)), "each editor left before Quit is done");
        assert!(windows.shown.is_empty() && windows.leaving.is_empty());
        assert_eq!(state::reopen(&state).len(), 2, "the windows open at Quit, to open again");
    }

    #[test]
    fn a_database_s_editor_runs_in_its_folder_and_open_recent_names_each() {
        assert_eq!(folder(Path::new("/work/repo/stacked.jsonl")), Path::new("/work/repo"));
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let dbs = [home.join("a/stacked.jsonl"), PathBuf::from("/b/stacked.jsonl"), PathBuf::from("/c/other.jsonl")];
        assert_eq!(titles(&dbs), ["stacked.jsonl — ~/a", "stacked.jsonl — /b", "other.jsonl"]);
    }
}
