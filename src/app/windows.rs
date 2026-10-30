//! StackED's windows: one for each database opened, each with an editor
//! of its own (`Session`), all in one process, on every system. Each is a
//! viewport of eframe's: the first is eframe's root viewport, which eframe
//! cannot do without, the others deferred, each painted by itself when it
//! needs to be. The root's passes keep the set: what the File menu opens
//! comes in by a channel, and each window closed is let go. The root's own
//! window closed while others are open, it takes over the latest of them:
//! that window's editor, title, place and size, that one's viewport going.
//! The last window closed quits StackED, as Quit does: no window is left
//! for a menu bar to be in. With no database to open, the root's window is
//! empty, its menu bar there to open one.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use super::menu::{Bar, Menu};
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

/// What the root's window says with no database open.
const EMPTY: &str = "No database is open in StackED.\nOpen… in the File menu opens one, and New… makes a new one.";

/// The title of the window on the database at `path`.
pub fn title(path: &Path) -> String {
    format!("StackED · {}", path.file_name().unwrap_or(path.as_os_str()).to_string_lossy())
}

/// What the File menu asks of the system: its dialogs, rfd's
/// (`dialogs::System`); the tests' stand-ins in theirs.
pub trait Ask {
    /// The databases picked in the Open dialog, any number of .jsonl files:
    /// none if it is cancelled.
    fn databases(&self) -> Vec<PathBuf>;
    /// Where a new database is to be made, as named in the Save dialog: none
    /// if it is cancelled.
    fn new_database(&self) -> Option<PathBuf>;
    /// `text` said in an alert.
    fn alert(&self, text: &str);
}

/// What the File menu's items do, their menu aside: each database opened
/// is sent to the window set, which is then woken to take it.
pub struct File {
    to: Sender<Opened>,
    wake: Box<dyn Fn() + Send + Sync>,
    state: PathBuf,
}

impl File {
    /// The File menu of the window set `to` sends to and `wake` wakes, the
    /// recent databases kept in the state file `state`.
    pub fn new(to: Sender<Opened>, wake: Box<dyn Fn() + Send + Sync>, state: PathBuf) -> File {
        File { to, wake, state }
    }

    /// File → Open…: each database picked opened in a new window, even one
    /// open in another window already.
    pub fn open(&self, ask: &dyn Ask) {
        for db in ask.databases() {
            self.open_one(&db, ask);
        }
    }

    /// File → New…: an empty database made where the Save dialog says, then
    /// opened in a new window. A file there already is never replaced (the
    /// dialog offers to): nothing is made, and an alert says why.
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

/// StackED's windows, as eframe runs them from its root viewport.
pub struct Windows {
    /// The root viewport's window, if it is on a database: none, it is
    /// empty, its menu bar there to open one.
    root: Option<(PathBuf, Arc<Mutex<Window>>)>,
    /// The empty root's bar.
    bar: Bar,
    /// The other windows, each a viewport of its own, the latest last.
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
    /// The state file, where the recent databases are kept; and, opened as
    /// an app (`keeps`), the windows open, those last kept in `kept`.
    state: PathBuf,
    keeps: bool,
    kept: Option<Vec<PathBuf>>,
    /// Says what went wrong: in an alert, opened as an app; else on stderr.
    say: Box<dyn Fn(String)>,
    /// The menu bar of every window.
    menu: Arc<Menu>,
}

impl Windows {
    /// No window yet: they come from `from`, each with the fonts' faces
    /// `real`, the name of `$EDITOR`'s program and the menu bar `menu`; the
    /// state is kept in `state`, the windows open too if `keeps`; what goes
    /// wrong is told to `say`.
    pub fn new(from: Receiver<Opened>, state: PathBuf, keeps: bool, real: [bool; 4], program: String, say: Box<dyn Fn(String)>, menu: Arc<Menu>) -> Windows {
        Windows {
            root: None,
            bar: Bar::default(),
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
            menu,
        }
    }

    /// A pass of the root viewport: what the File menu opened given windows
    /// (the root's first, if it is empty); the root's window closed handed
    /// over to the latest other, or, the last, let close, StackED quitting;
    /// the other windows closed let go; then the root's window painted, and
    /// the rest shown.
    fn pass(&mut self, ctx: &egui::Context) {
        while let Ok(opened) = self.from.try_recv() {
            self.add(ctx, opened);
        }
        // The last window closed is let close: StackED quits, the window kept
        // to open again (`quit`, as eframe exits).
        if ctx.input(|i| i.viewport().close_requested())
            && !self.menu.quitting()
            && let Some(at) = self.shown.iter().rposition(|s| !lock(&s.window).gone)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let next = self.shown.remove(at);
            self.take_over(ctx, next);
        }
        let (gone, shown): (Vec<Shown>, Vec<Shown>) = std::mem::take(&mut self.shown).into_iter().partition(|s| lock(&s.window).gone);
        self.shown = shown;
        for s in gone {
            self.let_go(s.path, &s.window);
        }
        self.reap();
        match &self.root {
            Some((_, window)) => lock(window).frame(ctx),
            None => self.empty(ctx),
        }
        for s in &self.shown {
            show(ctx, s);
        }
        self.keep();
    }

    /// The root's window with no database: its menu bar, and what to do.
    fn empty(&mut self, ctx: &egui::Context) {
        if let Some(item) = self.menu.bar(ctx, &mut self.bar) {
            self.menu.act(ctx, item);
        }
        let area = ctx.available_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.text(area.center(), egui::Align2::CENTER_CENTER, EMPTY, egui::FontId::proportional(15.0), super::FG);
    }

    /// `opened` given a window, its database now the most recent: the
    /// root's, if it is empty; else a viewport of its own.
    fn add(&mut self, ctx: &egui::Context, opened: Opened) {
        state::opened(&self.state, &opened.path);
        let mut window = Window::with(opened.session, self.real, self.program.clone(), Some(self.menu.clone()));
        if self.root.is_none() {
            window.viewport = egui::ViewportId::ROOT;
            window.session.wakes(ctx, egui::ViewportId::ROOT);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title(&opened.path)));
            self.root = Some((opened.path, Arc::new(Mutex::new(window))));
            return;
        }
        let id = egui::ViewportId::from_hash_of(("StackED window", self.made));
        self.made += 1;
        window.viewport = id;
        window.session.wakes(ctx, id);
        self.shown.push(Shown {
            id,
            builder: super::builder(Some(&opened.path)),
            path: opened.path,
            window: Arc::new(Mutex::new(window)),
        });
    }

    /// The root's window closed, with `next` open: the root's window let
    /// go, and `next`'s taken into the root, with its title, its place and
    /// its size, where the system lets a window be moved; `next`'s viewport,
    /// not shown again, goes.
    fn take_over(&mut self, ctx: &egui::Context, next: Shown) {
        if let Some((path, window)) = self.root.take() {
            self.let_go(path, &window);
        }
        {
            let mut window = lock(&next.window);
            window.viewport = egui::ViewportId::ROOT;
            window.session.wakes(ctx, egui::ViewportId::ROOT);
            // Its grid's size said again, as the root's window has it.
            window.sent = None;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title(&next.path)));
        let info = ctx.input(|i| i.raw.viewports.get(&next.id).cloned()).unwrap_or_default();
        if let Some(outer) = info.outer_rect {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer.min));
        }
        if let Some(inner) = info.inner_rect {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner.size()));
        }
        self.root = Some((next.path, next.window));
    }

    /// The window on `path` closed: its editor told so, to leave on its
    /// thread, which is not waited for here.
    fn let_go(&mut self, path: PathBuf, window: &Arc<Mutex<Window>>) {
        let mut window = lock(window);
        window.gone = true;
        let mut session = std::mem::replace(&mut window.session, Session::gone());
        session.let_go();
        self.leaving.push((path, session));
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

    /// The databases of the windows open, the root's first.
    fn open_now(&self) -> Vec<PathBuf> {
        self.root.iter().map(|(p, _)| p.clone()).chain(self.shown.iter().map(|s| s.path.clone())).collect()
    }

    /// The windows open kept in the state, if they changed and StackED was
    /// opened as an app, which keeps them, to open them again when it next
    /// starts.
    fn keep(&mut self) {
        if !self.keeps {
            return;
        }
        let now = self.open_now();
        if self.kept.as_ref() != Some(&now) {
            state::keep_open(&self.state, &now);
            self.kept = Some(now);
        }
    }

    /// Quit, or the last window closed: the windows open kept, then every
    /// editor told to leave at once, and each waited for as it does what it
    /// does on leaving.
    fn quit(&mut self) {
        self.kept = None;
        self.keep();
        if let Some((path, window)) = self.root.take() {
            self.let_go(path, &window);
        }
        for s in std::mem::take(&mut self.shown) {
            self.let_go(s.path, &s.window);
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

/// The window held in `window`, whatever a pass that panicked left it as.
fn lock(window: &Mutex<Window>) -> std::sync::MutexGuard<'_, Window> {
    window.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What is said of the editor of the database at `path` that stopped on an
/// error of its own (a panic, said on stderr as it happened).
fn stopped(path: &Path) -> String {
    format!("the editor of {} stopped on an error", path.display())
}

/// The window `shown`, its viewport's pass to be run whenever it needs
/// painting: closed by its button or File → Close Window, or its editor
/// quit, it is gone, for the root's next pass to let go. Once the root's
/// window has taken it over, its viewport paints it no more.
fn show(ctx: &egui::Context, shown: &Shown) {
    let window = shown.window.clone();
    let id = shown.id;
    ctx.show_viewport_deferred(id, shown.builder.clone(), move |ctx, _| {
        let mut window = lock(&window);
        if window.viewport != id {
            return;
        }
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

    /// Quit (File → Quit StackED, ⌘Q on macOS, logging out), or the last
    /// window closed: each window's editor leaves as it does when its window
    /// closes, and is waited for.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.quit();
    }
}

/// StackED's windows until it is quit, or its last window closed: one for
/// each of `first`, opened already (the root's, empty if there is none),
/// and one for each database the File menu opens, its dialogs asked of
/// `ask`; `said`, what went wrong opening others, is said in alerts once
/// StackED runs. `keeps` for StackED opened as an app, whose windows open
/// are kept, to open them again. The state is kept where stacked keeps it
/// (`home`).
pub fn run(first: Vec<Opened>, said: Vec<String>, keeps: bool, ask: Arc<dyn Ask + Send + Sync>) -> Result<()> {
    let state = crate::home::get().state.clone();
    let builder = super::builder(first.first().map(|o| o.path.as_path()));
    let (to, from) = std::sync::mpsc::channel();
    for opened in first {
        let _ = to.send(opened);
    }
    let options = eframe::NativeOptions {
        viewport: builder,
        ..Default::default()
    };
    eframe::run_native(
        "StackED",
        options,
        Box::new(move |cc| {
            let (fonts, real) = super::fonts();
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            let ctx = cc.egui_ctx.clone();
            let wake = move || ctx.request_repaint_of(egui::ViewportId::ROOT);
            let menu = Menu::new(Arc::new(File::new(to, Box::new(wake), state.clone())), ask);
            for text in said {
                menu.alert(text);
            }
            let alerts = menu.clone();
            let say: Box<dyn Fn(String)> = match crate::bundle::no_terminal() {
                true => Box::new(move |text| alerts.alert(text)),
                false => Box::new(|text| {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "stacked: {text}");
                }),
            };
            let program = super::program(&crate::config::get().editor());
            Ok(Box::new(Windows::new(from, state, keeps, real, program, say, menu)))
        }),
    )
    .map_err(|e| Error(format!("StackED cannot open its windows: {e}")))
}

#[cfg(test)]
mod tests {
    use super::super::{Input, Output, PAD};
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

    /// The dialogs, as no test answers them here: how many times they were
    /// asked.
    #[derive(Default)]
    struct NoDialogs(AtomicUsize);

    impl Ask for NoDialogs {
        fn databases(&self) -> Vec<PathBuf> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Vec::new()
        }

        fn new_database(&self) -> Option<PathBuf> {
            self.0.fetch_add(1, Ordering::Relaxed);
            None
        }

        fn alert(&self, _text: &str) {}
    }

    /// The window set's root viewport's pass, its window 800 by 600,
    /// `events` its viewport's (a close), `infos` those of the other
    /// viewports, as eframe says them.
    fn root_with(ctx: &egui::Context, windows: &mut Windows, events: Vec<egui::ViewportEvent>, infos: Vec<(egui::ViewportId, egui::ViewportInfo)>) -> egui::FullOutput {
        let mut input = egui::RawInput {
            viewport_id: egui::ViewportId::ROOT,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        input.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo { events, ..Default::default() });
        input.viewports.extend(infos);
        ctx.run(input, |ctx| windows.pass(ctx))
    }

    /// The window set's root viewport's pass.
    fn root(ctx: &egui::Context, windows: &mut Windows) -> egui::FullOutput {
        root_with(ctx, windows, vec![], vec![])
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

    /// The commands `out` gives the root's window.
    fn commands(out: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
        out.viewport_output[&egui::ViewportId::ROOT].commands.clone()
    }

    /// The texts `out` paints.
    fn texts(out: &egui::FullOutput) -> Vec<String> {
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// A window set keeping its state in `state`, and its windows open too
    /// if `keeps`; what opens windows in it; and its context.
    fn set(state: &Path, keeps: bool) -> (Windows, Sender<Opened>, egui::Context) {
        let (windows, to, ctx, _) = set_asked(state, keeps);
        (windows, to, ctx)
    }

    /// As `set`, and its dialogs, to count how many times they were asked.
    fn set_asked(state: &Path, keeps: bool) -> (Windows, Sender<Opened>, egui::Context, Arc<NoDialogs>) {
        let (to, from) = mpsc::channel();
        let ctx = egui::Context::default();
        let (fonts, real) = super::super::fonts();
        ctx.set_fonts(fonts);
        // As eframe has it: each viewport a window of its own.
        ctx.set_embed_viewports(false);
        let dialogs = Arc::new(NoDialogs::default());
        let menu = Menu::new(Arc::new(File::new(to.clone(), Box::new(|| {}), state.to_path_buf())), dialogs.clone());
        let windows = Windows::new(from, state.to_path_buf(), keeps, real, "vim".into(), Box::new(|_| {}), menu);
        (windows, to, ctx, dialogs)
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

    /// The grid's size, as the editor `inputs` come to was last told it.
    fn size(inputs: &Receiver<Input>) -> Option<(u16, u16)> {
        inputs.try_iter().filter_map(|i| if let Input::Size(c, r) = i { Some((c, r)) } else { None }).last()
    }

    #[test]
    fn each_database_opened_is_a_window_of_its_own_with_its_own_editor() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, false);
        // No database: the root's window empty, its menu bar there, and no
        // other window.
        let out = root(&ctx, &mut windows);
        assert!(windows_in(&out).is_empty());
        let painted = texts(&out);
        assert!(painted.iter().any(|t| t == "File") && painted.iter().any(|t| t.starts_with("No database is open in StackED.")), "{painted:?}");
        // Two databases opened, the same one twice: the root's window takes
        // the first, and two windows more.
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
        assert!(commands(&out).contains(&egui::ViewportCommand::Title("StackED · a.jsonl".into())), "{:?}", commands(&out));
        let ids = windows_in(&out);
        assert_eq!(ids.len(), 2);
        assert_eq!(out.viewport_output[&ids[0]].builder.title.as_deref().map(|t| t.starts_with("StackED · ")), Some(true));
        // Each window's pass tells its own editor the grid's size, the
        // root's in the root's pass: as many cells as fit under the bar.
        for id in &ids {
            window(&ctx, &out, *id, vec![]);
        }
        for (inputs, _) in &editors {
            let (cols, rows) = size(inputs).expect("a size");
            assert!(cols as f32 > (800.0 - 2.0 * PAD) / 20.0 && rows > 10, "{cols} by {rows}");
        }
        // Recent, the latest first, once each.
        assert_eq!(state::recent(&state), [a.clone(), b.clone()]);
        // Not opened as an app: no windows kept to open again.
        assert!(state::reopen(&state).is_empty());
    }

    #[test]
    fn a_window_closed_ends_its_editor_and_the_root_s_takes_over_the_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, true);
        let (a, b, c) = (tmp.path().join("a.jsonl"), tmp.path().join("b.jsonl"), tmp.path().join("c.jsonl"));
        let mut editors = Vec::new();
        for path in [&a, &b, &c] {
            std::fs::write(path, "").unwrap();
            let (session, inputs, outputs) = stand_in();
            to.send(Opened { path: path.clone(), session }).unwrap();
            editors.push((inputs, outputs));
        }
        let out = root(&ctx, &mut windows);
        let ids: Vec<egui::ViewportId> = windows.shown.iter().map(|s| s.id).collect();
        assert_eq!(windows_in(&out).len(), 2);
        // Opened as an app: the windows open kept, to open again.
        assert_eq!(state::reopen(&state), [a.clone(), b.clone(), c.clone()]);
        // b's closed by its button (File → Close Window is the same): gone
        // at the root's next pass, its editor told so, the others' not.
        window(&ctx, &out, ids[0], vec![egui::ViewportEvent::Close]);
        let out = root(&ctx, &mut windows);
        assert_eq!(windows_in(&out), [ids[1]]);
        assert!(told_to_leave(&editors[1].0));
        assert!(matches!(editors[0].0.try_recv(), Err(mpsc::TryRecvError::Empty) | Ok(_)));
        assert_eq!(state::reopen(&state), [a.clone(), c.clone()]);
        // The root's, a's, closed while c's is open: not closed, but a's
        // editor told to leave, and c's window taken into it, with its
        // title, its place and its size; c's viewport gone.
        let place = egui::ViewportInfo {
            outer_rect: Some(egui::Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(700.0, 500.0))),
            inner_rect: Some(egui::Rect::from_min_size(egui::pos2(300.0, 228.0), egui::vec2(700.0, 472.0))),
            ..Default::default()
        };
        let out = root_with(&ctx, &mut windows, vec![egui::ViewportEvent::Close], vec![(ids[1], place)]);
        let said = commands(&out);
        for command in [
            egui::ViewportCommand::CancelClose,
            egui::ViewportCommand::Title("StackED · c.jsonl".into()),
            egui::ViewportCommand::OuterPosition(egui::pos2(300.0, 200.0)),
            egui::ViewportCommand::InnerSize(egui::vec2(700.0, 472.0)),
        ] {
            assert!(said.contains(&command), "{command:?}: {said:?}");
        }
        assert!(windows_in(&out).is_empty(), "c's viewport gone");
        assert!(told_to_leave(&editors[0].0));
        assert_eq!(state::reopen(&state), std::slice::from_ref(&c));
        // c's editor, now the root's window's: told the grid's size in the
        // root's passes, and woken there.
        root(&ctx, &mut windows);
        assert!(size(&editors[2].0).is_some());
        assert_eq!(windows.root.as_ref().map(|(p, _)| p.clone()), Some(c.clone()));
        // c's editor quit: its window, the last, closes, and is not kept
        // from closing; StackED quits, the window kept to open again.
        editors[2].1.send(Output::Bye).unwrap();
        let out = root(&ctx, &mut windows);
        assert!(commands(&out).contains(&egui::ViewportCommand::Close));
        let out = root_with(&ctx, &mut windows, vec![egui::ViewportEvent::Close], vec![]);
        assert!(!commands(&out).contains(&egui::ViewportCommand::CancelClose));
        windows.quit();
        assert!(told_to_leave(&editors[2].0));
        assert_eq!(state::reopen(&state), std::slice::from_ref(&c));
        assert_eq!(state::recent(&state), [c.clone(), b.clone(), a.clone()], "each recent still");
    }

    #[test]
    fn quit_from_the_file_menu_closes_every_window_and_keeps_them_open() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, true);
        let mut editors = Vec::new();
        for name in ["a.jsonl", "b.jsonl"] {
            let path = tmp.path().join(name);
            std::fs::write(&path, "").unwrap();
            let (session, inputs, outputs) = stand_in();
            to.send(Opened { path, session }).unwrap();
            editors.push((inputs, outputs));
        }
        root(&ctx, &mut windows);
        // Quit: the root's window closes, and is let close with the other
        // open, which is not taken over.
        let _ = ctx.run(egui::RawInput::default(), |ctx| windows.menu.act(ctx, super::super::menu::Item::Quit));
        let out = root_with(&ctx, &mut windows, vec![egui::ViewportEvent::Close], vec![]);
        assert!(!commands(&out).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(windows.shown.len(), 1);
        windows.quit();
        assert_eq!(state::reopen(&state).len(), 2, "both, to open again");
        for (inputs, _) in &editors {
            assert!(told_to_leave(inputs));
        }
    }

    #[test]
    fn each_window_follows_the_mouse_only_while_it_is_the_active_one() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, false);
        let mut editors = Vec::new();
        for name in ["root.jsonl", "a.jsonl", "b.jsonl"] {
            let path = tmp.path().join(name);
            std::fs::write(&path, "").unwrap();
            let (session, inputs, outputs) = stand_in();
            to.send(Opened { path, session }).unwrap();
            editors.push((inputs, outputs));
        }
        let editors = &editors[1..];
        let out = root(&ctx, &mut windows);
        let ids: Vec<egui::ViewportId> = windows.shown.iter().map(|s| s.id).collect();
        for id in &ids {
            window(&ctx, &out, *id, vec![]);
        }
        let (cell, origin) = {
            let w = windows.shown[0].window.lock().unwrap();
            (w.cell, w.origin)
        };
        let on = |x: u16, y: u16| origin + egui::vec2((x as f32 + 0.5) * cell.x, (y as f32 + 0.5) * cell.y);
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
    fn the_menu_bar_is_over_the_grid_and_what_is_done_in_its_menus_is_not_the_editor_s() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (mut windows, to, ctx) = set(&state, false);
        let path = tmp.path().join("a.jsonl");
        std::fs::write(&path, "").unwrap();
        let (session, inputs, _outputs) = stand_in();
        to.send(Opened { path, session }).unwrap();
        let out = root(&ctx, &mut windows);
        // The grid under the bar: its first row below it, as many rows as fit
        // there.
        let (cell, origin) = {
            let w = windows.root.as_ref().unwrap().1.lock().unwrap();
            (w.cell, w.origin)
        };
        let file = out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "File" => Some(t.pos + t.galley.rect.center().to_vec2()),
            _ => None,
        });
        let file = file.expect("the bar's File menu");
        assert!(origin.y > file.y + 8.0 && origin.x == PAD, "{origin:?}, File at {file:?}");
        let (_, rows) = size(&inputs).unwrap();
        assert_eq!(rows, ((600.0 - origin.y - PAD) / cell.y) as u16);
        let pass = |windows: &mut Windows, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                viewport_id: egui::ViewportId::ROOT,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| windows.pass(ctx));
            inputs.try_iter().collect::<Vec<_>>()
        };
        let click = |pos: egui::Pos2| {
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            vec![egui::Event::PointerMoved(pos), button(true), button(false)]
        };
        let j = egui::Event::Text("j".into());
        // A click on File, over the grid's room: not the editor's. With the
        // menu open, a click on the grid and a key are the menu's: the menu
        // closes, and the editor is sent nothing.
        assert_eq!(pass(&mut windows, click(file)), []);
        let on_grid = origin + egui::vec2(60.5 * cell.x, 20.5 * cell.y);
        let mut events = click(on_grid);
        events.push(j.clone());
        assert_eq!(pass(&mut windows, events), []);
        // The menu closed: the window's keys and clicks are the editor's again.
        let mut events = click(on_grid);
        events.push(j);
        assert_eq!(
            pass(&mut windows, events),
            [Input::Hover(60, 20), Input::Click { x: 60, y: 20, right: false }, Input::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))]
        );
    }

    #[test]
    fn every_key_goes_to_the_editor_and_the_menu_takes_none_on_either_system() {
        use egui::os::OperatingSystem;
        for os in [OperatingSystem::Nix, OperatingSystem::Mac] {
            let tmp = tempfile::tempdir().unwrap();
            let state = tmp.path().join("state.json");
            let (mut windows, to, ctx, asked) = set_asked(&state, false);
            ctx.set_os(os);
            let path = tmp.path().join("a.jsonl");
            std::fs::write(&path, "").unwrap();
            let (session, inputs, _outputs) = stand_in();
            to.send(Opened { path, session }).unwrap();
            root(&ctx, &mut windows);
            // A pass of the root's window, `events` done in it: what it
            // painted, and what its editor was sent, but the grid's size.
            let pass = |windows: &mut Windows, events: Vec<egui::Event>| {
                let input = egui::RawInput {
                    viewport_id: egui::ViewportId::ROOT,
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    events,
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| windows.pass(ctx));
                let sent: Vec<Input> = inputs.try_iter().filter(|i| !matches!(i, Input::Size(..))).collect();
                (out, sent)
            };
            let key = |key, modifiers| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            let letters = [egui::Key::N, egui::Key::O, egui::Key::W, egui::Key::Q];
            // ctrl with n, o, w and q, as egui-winit sends them (egui's
            // command too, elsewhere than on macOS): the editor's, in turn; no
            // window closed, no dialog asked.
            let ctrl = egui::Modifiers {
                ctrl: true,
                command: os != OperatingSystem::Mac,
                ..Default::default()
            };
            let (out, sent) = pass(&mut windows, letters.map(|k| key(k, ctrl)).to_vec());
            let ctrl_keys = ['n', 'o', 'w', 'q'].map(|c| Input::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
            assert_eq!(sent, ctrl_keys, "{os:?}");
            assert!(!commands(&out).contains(&egui::ViewportCommand::Close) && !windows.menu.busy(), "{os:?}");
            // ⌘ with them on macOS: neither the menu's, which takes no key,
            // nor the editor's, which reads no ⌘.
            if os == OperatingSystem::Mac {
                let (out, sent) = pass(&mut windows, letters.map(|k| key(k, egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND)).to_vec());
                assert_eq!(sent, []);
                assert!(!commands(&out).contains(&egui::ViewportCommand::Close) && !windows.menu.busy());
            }
            assert_eq!(asked.0.load(Ordering::Relaxed), 0, "{os:?}");
            // The File menu, open: its items, and no shortcut beside them.
            let file = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "File" => Some(t.pos + t.galley.rect.center().to_vec2()),
                _ => None,
            });
            let file = file.expect("the bar's File menu");
            let button = |pressed| egui::Event::PointerButton {
                pos: file,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            pass(&mut windows, vec![egui::Event::PointerMoved(file), button(true), button(false)]);
            let (out, _) = pass(&mut windows, vec![]);
            let shown = texts(&out);
            assert!(shown.contains(&"New…".to_string()) && shown.contains(&"Quit StackED".to_string()), "{shown:?}");
            assert!(!shown.iter().any(|t| t.contains('⌘') || t.contains("Ctrl")), "{os:?}: {shown:?}");
        }
    }

    #[test]
    fn the_bar_and_the_empty_window_are_drawn_whole_by_egui_s_own_text_font() {
        let ctx = egui::Context::default();
        ctx.set_fonts(super::super::fonts().0);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(15.0);
        let (_, rows) = &super::super::menu::rows()[0];
        let shown: String = rows.iter().filter(|r| *r != "—").cloned().chain(["File".into(), "Clear Menu".into(), EMPTY.into()]).collect();
        let missing: String = shown.chars().filter(|c| !c.is_whitespace() && !ctx.fonts_mut(|f| f.has_glyph(&font, *c))).collect();
        assert_eq!(missing, "");
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
        assert!(windows.root.is_none() && windows.shown.is_empty() && windows.leaving.is_empty());
        assert_eq!(state::reopen(&state).len(), 2, "the windows open at Quit, to open again");
    }

    #[test]
    fn a_database_s_editor_runs_in_its_folder_and_open_recent_names_each() {
        assert_eq!(folder(Path::new("/work/repo/stacked.jsonl")), Path::new("/work/repo"));
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let dbs = [home.join("a/stacked.jsonl"), PathBuf::from("/b/stacked.jsonl"), PathBuf::from("/c/other.jsonl")];
        assert_eq!(titles(&dbs), ["stacked.jsonl — ~/a", "stacked.jsonl — /b", "other.jsonl"]);
        assert_eq!(title(Path::new("/w/x.jsonl")), "StackED · x.jsonl");
    }
}
