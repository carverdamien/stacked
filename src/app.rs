//! `stacked app`: StackED, the editor in a window of its own. The editor
//! runs as the terminal runs it, on a thread of its own, its screen drawn
//! into a buffer of the window's size. The window paints the buffer's cells
//! as a terminal would, a grid of a monospace font in their colours, and
//! sends back its keys, clicks, wheel and pastes, and the hover while it is
//! the active window, as `stacked web` does for a browser. The window is
//! eframe's: egui, drawn with OpenGL.
//!
//! What runs `$EDITOR` runs it where `stacked app` was started, in its
//! terminal; the window waits meanwhile, and says so.
//!
//! StackED is an app of many windows (`windows`), on every system, one for
//! each database opened, each with an editor of its own. Each window has
//! egui's menu bar at its top (`menu`), whose File menu opens them,
//! through the system's dialogs (`dialogs`); the databases opened last are
//! kept between runs, with those stacked's editor opens (`crate::state`).

pub mod dialogs;
pub mod menu;
pub mod windows;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Vec2, pos2, vec2};
use ratatui::buffer::{Buffer, Cell};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use self::menu::{Bar, Menu};
use crate::config::program;
use crate::db::{Error, Result};
use crate::tui::{App, Effect, Turned};

/// The size of the grid's font, in points.
const FONT_SIZE: f32 = 14.0;

/// The room around the grid, in points: the window's corners are round.
const PAD: f32 = 6.0;

/// A cell of Menlo, of DejaVu Sans Mono or of Hack, at `FONT_SIZE`, as a
/// fraction of it: to size the window before its fonts are loaded.
const CELL: (f32, f32) = (0.602, 1.165);

/// The menu bar's height, in points, about: to size the window before it
/// is drawn.
const BAR: f32 = 24.0;

/// The terminal's sixteen colours, as web/client.rs has them: a dark
/// terminal's.
const PALETTE: [Color32; 16] = [
    Color32::from_rgb(0x00, 0x00, 0x00),
    Color32::from_rgb(0xcd, 0x31, 0x31),
    Color32::from_rgb(0x0d, 0xbc, 0x79),
    Color32::from_rgb(0xe5, 0xe5, 0x10),
    Color32::from_rgb(0x24, 0x72, 0xc8),
    Color32::from_rgb(0xbc, 0x3f, 0xbc),
    Color32::from_rgb(0x11, 0xa8, 0xcd),
    Color32::from_rgb(0xe5, 0xe5, 0xe5),
    Color32::from_rgb(0x76, 0x76, 0x76),
    Color32::from_rgb(0xf1, 0x4c, 0x4c),
    Color32::from_rgb(0x23, 0xd1, 0x8b),
    Color32::from_rgb(0xf5, 0xf5, 0x43),
    Color32::from_rgb(0x3b, 0x8e, 0xea),
    Color32::from_rgb(0xd6, 0x70, 0xd6),
    Color32::from_rgb(0x29, 0xb8, 0xdb),
    Color32::from_rgb(0xff, 0xff, 0xff),
];

/// The window's own colours, as the page's are: its text, and behind it.
const FG: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const BG: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);

/// What lights text selected with the mouse, over its colours.
const SELECTED: Color32 = Color32::from_rgba_premultiplied(0x2a, 0x45, 0x6e, 0x70);

/// How much of its colour dim text keeps, the rest the colour behind it.
const DIM: f32 = 0.6;

/// The points a trackpad turns for a line, as the browser's pixels are.
const NOTCH: f32 = 40.0;

/// How long the editor waits for the window before it looks again: for
/// what the agent does, landing meanwhile.
const TICK: Duration = Duration::from_millis(250);

/// What the window sends the editor.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A key, as the terminal would have sent it.
    Key(KeyEvent),
    /// A click on a cell, with the right button if `right`.
    Click { x: u16, y: u16, right: bool },
    /// The left button pressed on a cell where a press is held for a drag
    /// (`Output::Screen`'s drags: HEAD's row): the editor clicks there once
    /// the button is let go there, or drags HEAD.
    Down { x: u16, y: u16 },
    /// The mouse over another cell, the button pressed so still down.
    Drag { x: u16, y: u16 },
    /// That button let go, over a cell if the mouse is over one.
    Up { at: Option<(u16, u16)> },
    /// The wheel turned over a cell, if the mouse is over one.
    Wheel { at: Option<(u16, u16)>, turned: Turned },
    /// The mouse over another cell.
    Hover(u16, u16),
    /// The mouse gone from the grid.
    Unhover,
    /// The grid's size: columns, rows.
    Size(u16, u16),
}

/// What the editor sends the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// The screen drawn, where the cursor is while text is written, where
    /// text was drawn as one (`App::regions`), which text selected keeps
    /// to, and where a press of the left button is held for a drag
    /// (`App::drags`: HEAD's row), where no text is selected.
    Screen(Buffer, Option<(u16, u16)>, Vec<Rect>, Vec<Rect>),
    /// `$EDITOR` runs: the window waits for it.
    Editing,
    /// The editor was quit.
    Bye,
}

/// `stacked app`: StackED's window on the database at `path`, its editor
/// run where `stacked app` was started, as the editor in a terminal is.
/// The File menu opens more, each database's editor in that database's
/// folder; StackED runs until it is quit, or its last window is closed.
pub fn run(path: &Path) -> Result<()> {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let session = Session::open(&path, &std::env::current_dir().unwrap_or_default())?;
    windows::run(vec![windows::Opened { path, session }], Vec::new(), false, Arc::new(dialogs::System))
}

/// StackED as the system opens its app (from the Dock or the Finder on
/// macOS, from its launcher elsewhere): a window again for each database
/// whose window was open when it was last quit, those no longer there left
/// out; with none, a window on `named` (from `$STACKED_FILE`, the
/// settings' `file`) if there is one; else an empty window, its menu bar
/// there to open one. Each database's editor runs in its folder. What goes
/// wrong opening one is said in an alert once StackED runs, after `said`,
/// what StackED says as it starts.
pub fn dock(named: Option<std::path::PathBuf>, mut said: Vec<String>) -> Result<()> {
    let mut dbs = crate::state::reopen(&crate::home::get().state);
    if dbs.is_empty() {
        dbs.extend(named.map(|f| std::path::absolute(&f).unwrap_or(f)));
    }
    let mut first = Vec::new();
    for db in dbs {
        match windows::open(&db) {
            Ok(o) => first.push(o),
            Err(e) => said.push(windows::not_opened(&db, &e)),
        }
    }
    windows::run(first, said, true, Arc::new(dialogs::System))
}

/// `text` said in an alert of the system's, as StackED says what went
/// wrong where there is no terminal to say it in, and on stderr.
pub fn alert(text: &str) {
    dialogs::alert(text);
}

/// The window of the database at `path`, or of none: named after it, as
/// large as 120 cells by 36 under its menu bar, and no smaller than the
/// editor draws well; its icon StackED's, the Dock's on macOS.
fn builder(path: Option<&Path>) -> egui::ViewportBuilder {
    static ICON: OnceLock<Arc<egui::IconData>> = OnceLock::new();
    let room = |cols: f32, rows: f32| vec2(cols * CELL.0 * FONT_SIZE + 2.0 * PAD, rows * CELL.1 * FONT_SIZE + 2.0 * PAD + BAR);
    egui::ViewportBuilder::default()
        .with_title(path.map_or_else(|| "StackED".to_string(), windows::title))
        .with_app_id(crate::launcher::ID)
        .with_icon(ICON.get_or_init(|| Arc::new(crate::icon::data(512))).clone())
        .with_inner_size(room(120.0, 36.0))
        .with_min_inner_size(room(MIN.0 as f32, MIN.1 as f32))
}

/// The window an editor wakes: its context and viewport, once there is
/// one. The root's window taking another's over, it changes.
type Waker = Arc<Mutex<Option<(egui::Context, egui::ViewportId)>>>;

/// The editor on a thread of its own, for a window: what is done in the
/// window sent to it, what it draws sent back.
pub struct Session {
    to: Option<Sender<Input>>,
    from: Receiver<Output>,
    thread: Option<JoinHandle<()>>,
    /// The window's context and viewport once there is one, woken by what
    /// is sent.
    waker: Waker,
    /// Whether the editor's thread ended in a panic.
    failed: Arc<AtomicBool>,
}

impl Session {
    /// The editor on the database at `path`, opened, with no window yet, as
    /// if started in `dir` (`App::new_in`): an error if it cannot be, before
    /// there is a window.
    pub fn open(path: &Path, dir: &Path) -> Result<Session> {
        let (to, inputs) = mpsc::channel();
        let (outputs, from) = mpsc::channel();
        let (opened, open) = mpsc::channel();
        let waker: Waker = Arc::default();
        let wake = waker.clone();
        let (path, dir) = (path.to_path_buf(), dir.to_path_buf());
        let thread = std::thread::Builder::new()
            .name("stacked app".into())
            .spawn(move || {
                let app = match App::new_in(&path, &dir) {
                    Ok(app) => app,
                    Err(e) => {
                        let _ = opened.send(Err(e));
                        return;
                    }
                };
                let _ = opened.send(Ok(()));
                work(app, &inputs, &outputs, &|| {
                    if let Some((ctx, id)) = &*wake.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
                        ctx.request_repaint_of(*id);
                    }
                });
            })
            .map_err(|e| Error(format!("cannot start the editor: {e}")))?;
        open.recv().map_err(|_| Error("the editor stopped as it opened".into()))??;
        Ok(Session {
            to: Some(to),
            from,
            thread: Some(thread),
            waker,
            failed: Arc::default(),
        })
    }

    /// A session with no editor, gone already: what a window's is once the
    /// window set has let its own go.
    fn gone() -> Session {
        Session {
            to: None,
            from: mpsc::channel().1,
            thread: None,
            waker: Arc::default(),
            failed: Arc::default(),
        }
    }

    /// From now on, what the editor sends wakes the viewport `id` of `ctx`:
    /// the window that shows it.
    fn wakes(&self, ctx: &egui::Context, id: egui::ViewportId) {
        *self.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((ctx.clone(), id));
    }

    /// Sends the editor what was done in the window.
    pub fn send(&self, input: Input) {
        if let Some(to) = &self.to {
            let _ = to.send(input);
        }
    }

    /// What the editor sent, waited for as long as `wait`: none if nothing
    /// came, or the editor is gone.
    pub fn recv(&self, wait: Duration) -> Option<Output> {
        self.from.recv_timeout(wait).ok()
    }

    /// The editor told it is done with, its channel closed: it leaves on its
    /// thread, which `close` waits for.
    fn let_go(&mut self) {
        self.to = None;
    }

    /// Whether the editor's thread has ended, or there is none.
    fn left(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Done with: the editor told so, its channel closed, and waited for, as
    /// it does what it does on leaving (an edit in `$EDITOR` taken, the
    /// agents' worktrees made ahead removed).
    fn close(&mut self) {
        self.to = None;
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            self.failed.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// The least the editor is drawn at, in cells: what it draws well, as the
/// browser's is.
const MIN: (u16, u16) = (40, 10);

/// A grid of `cols` by `rows`, within what the editor draws well.
fn fitted(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(MIN.0, 1000), rows.clamp(MIN.1, 500))
}

/// The editor as the terminal runs it: what the window sends done, then its
/// screen drawn at the window's size and sent if it changed, `wake` called
/// after each thing sent; what the agent does lands meanwhile. Until it is
/// quit, or the window is gone; the agent's asks still running are said
/// then, as the terminal's editor says them.
fn work(mut app: App, inputs: &Receiver<Input>, outputs: &Sender<Output>, wake: &dyn Fn()) {
    let send = |o: Output| {
        let _ = outputs.send(o);
        wake();
    };
    let mut size = None;
    let mut shown: Option<Output> = None;
    let mut launched = Instant::now();
    'run: loop {
        let first = match inputs.recv_timeout(TICK) {
            Ok(i) => Some(i),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        // All that came meanwhile, before it is drawn: a paste is many keys.
        for input in first.into_iter().chain(std::iter::from_fn(|| inputs.try_recv().ok())) {
            let effect = match input {
                Input::Key(k) => app.key(k),
                Input::Click { x, y, right } => app.click(x, y, right),
                Input::Down { x, y } => app.down(x, y),
                Input::Drag { x, y } => {
                    app.drag(x, y);
                    Effect::None
                }
                Input::Up { at } => app.up(at),
                Input::Wheel { at, turned } => app.wheel(at, turned),
                Input::Hover(x, y) => {
                    app.hover(x, y);
                    Effect::None
                }
                Input::Unhover => {
                    app.unhover();
                    Effect::None
                }
                Input::Size(cols, rows) => {
                    size = Some(fitted(cols, rows));
                    Effect::None
                }
            };
            match effect {
                Effect::None => {}
                Effect::Quit => {
                    send(Output::Bye);
                    break 'run;
                }
                // $EDITOR, where `stacked app` was started: the window waits,
                // and is sent the screen after, changed or not.
                e => {
                    send(Output::Editing);
                    if let Err(e) = app.effect(e) {
                        app.status = e.0;
                    }
                    shown = None;
                }
            }
        }
        app.refresh();
        if launched.elapsed() > Duration::from_secs(5) {
            app.launch();
            launched = Instant::now();
        }
        // Nothing is drawn before the window says its size.
        let Some((cols, rows)) = size else { continue };
        let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
        let cursor = app.draw(&mut buf);
        let screen = Output::Screen(buf, cursor, app.regions(), app.drags());
        if shown.as_ref() != Some(&screen) {
            send(screen.clone());
            shown = Some(screen);
        }
    }
    app.leave();
    app.say_still_running();
}

/// A screen the editor sent: its cells, where its cursor is while text is
/// written, and its regions.
type Screen = (Buffer, Option<(u16, u16)>, Vec<Rect>);

/// StackED's window, as eframe runs it: its menu bar, the editor's screen
/// painted under it, what is done in the window sent to the editor.
pub struct Window {
    session: Session,
    /// Which faces the fonts have of their own, by `Face`: the others are
    /// made, bold drawn twice a pixel apart, italics slanted.
    real: [bool; 4],
    /// The screen last sent.
    screen: Option<Screen>,
    /// That screen as it is painted, until it or the cell changes.
    painted: Option<Painted>,
    /// Which characters fit the grid, by face: a run of them stays on it.
    /// For the cell they were found with.
    fits: HashMap<(char, Face), bool>,
    cell: Vec2,
    /// Where the grid's first cell was last painted.
    origin: Pos2,
    /// The grid's size last sent.
    sent: Option<(u16, u16)>,
    pointer: Pointer,
    /// `$EDITOR` runs: nothing is sent the editor meanwhile.
    editing: bool,
    /// The program `$EDITOR` runs, as the window names it meanwhile.
    program: String,
    /// The editor is gone, or the window was closed: the window closes,
    /// and its editor is let go.
    gone: bool,
    /// The menu bar, the window set's: none in a window alone, as the tests
    /// paint one.
    menu: Option<Arc<Menu>>,
    /// What the bar keeps of this window's.
    bar: Bar,
    /// The viewport that shows the window: the root's, or one of its own.
    viewport: egui::ViewportId,
}

impl Window {
    /// The window of `session`, with the fonts' faces `real`, `$EDITOR`
    /// running `program`, and the menu bar `menu`.
    fn with(session: Session, real: [bool; 4], program: String, menu: Option<Arc<Menu>>) -> Window {
        Window {
            session,
            real,
            screen: None,
            painted: None,
            fits: HashMap::new(),
            cell: Vec2::ZERO,
            origin: Pos2::ZERO,
            sent: None,
            pointer: Pointer::default(),
            editing: false,
            program,
            gone: false,
            menu,
            bar: Bar::default(),
            viewport: egui::ViewportId::ROOT,
        }
    }

    /// A frame: what the editor sent taken, the menu bar drawn and what is
    /// picked in it done, what was done in the window sent the editor, the
    /// screen painted under the bar.
    fn frame(&mut self, ctx: &egui::Context) {
        loop {
            match self.session.from.try_recv() {
                Ok(Output::Screen(buf, cursor, regions, drags)) => {
                    self.screen = Some((buf, cursor, regions));
                    self.pointer.drags = drags;
                    self.painted = None;
                    self.editing = false;
                }
                Ok(Output::Editing) => self.editing = true,
                Ok(Output::Bye) | Err(TryRecvError::Disconnected) => {
                    if !self.gone {
                        self.gone = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        let writing = self.screen.as_ref().and_then(|(_, cursor, _)| *cursor);
        // A menu of the bar's open, as the pass before left it: what is done
        // in the window is the menu's, not the editor's.
        let menu_open = egui::Popup::is_any_open(ctx);
        if let Some(menu) = self.menu.clone()
            && let Some(item) = menu.bar(ctx, &mut self.bar)
        {
            menu.act(ctx, item);
        }
        let regular = FontId::new(FONT_SIZE, Face::Regular.family());
        let cell = ctx.fonts_mut(|f| vec2(f.glyph_width(&regular, 'M'), f.row_height(&regular)));
        if cell != self.cell {
            self.cell = cell;
            self.fits.clear();
            self.painted = None;
        }
        // Under the bar.
        let area = ctx.available_rect();
        let grid = Grid::new(area, cell);
        self.origin = grid.origin;
        if self.sent != Some((grid.cols, grid.rows)) {
            self.sent = Some((grid.cols, grid.rows));
            self.session.send(Input::Size(grid.cols, grid.rows));
            // Another grid: what was selected is not where it was.
            self.pointer.selected = None;
        }
        if !self.editing && !self.gone {
            // Whether the window is the active one, as egui has it for this
            // window's viewport: what eframe asks the window at each pass,
            // else what the window was last told.
            let (mut events, focused) = ctx.input(|i| (i.events.clone(), i.viewport().focused.unwrap_or(i.focused)));
            // A menu open, or egui's text field with the focus: what is done
            // is theirs, and the mouse is gone from the grid.
            if menu_open || ctx.wants_keyboard_input() {
                events = vec![egui::Event::PointerGone];
            }
            // ⌘ is the window's own on macOS, as egui knows the system.
            let mac = ctx.os() == egui::os::OperatingSystem::Mac;
            for input in inputs(&events, &grid, &mut self.pointer, focused, writing.is_some(), mac) {
                self.session.send(input);
            }
        }
        // ⌘C: the text selected, as the screen has it now.
        if std::mem::take(&mut self.pointer.copy)
            && let (Some(s), Some((buf, _, regions))) = (&self.pointer.selected, &self.screen)
        {
            ctx.copy_text(selected_text(buf, s, region_at(regions, s.from, buf.area)));
        }
        if self.painted.is_none()
            && let Some((buf, cursor, _)) = &self.screen
        {
            let fits = &mut self.fits;
            self.painted = Some(painted(buf, *cursor, |c, face| {
                *fits.entry((c, face)).or_insert_with(|| {
                    let font = FontId::new(FONT_SIZE, face.family());
                    ctx.fonts_mut(|f| (f.glyph_width(&font, c) - cell.x).abs() < 0.01)
                })
            }));
        }
        let painter = ctx.layer_painter(egui::LayerId::background());
        if let Some(p) = &self.painted {
            paint(&painter, &grid, p, &self.real);
        }
        // What is selected, lit over what is painted.
        if let (Some(s), Some((buf, _, regions))) = (&self.pointer.selected, &self.screen) {
            for (y, x0, x1) in selected_rows(s, region_at(regions, s.from, buf.area)) {
                painter.rect_filled(grid.area(x0, y, x1 + 1 - x0, 1), 0.0, SELECTED);
            }
        }
        if self.editing {
            waiting(&painter, area, &self.program);
        } else if let Some((x, y)) = writing {
            // Where text is written: an input method composes there.
            ctx.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    rect: grid.area(0, 0, grid.cols, grid.rows),
                    cursor_rect: grid.area(x, y, 1, 1),
                })
            });
        }
    }
}

/// A face of the grid's font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Face {
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

impl Face {
    const ALL: [Face; 4] = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic];

    fn of(bold: bool, italic: bool) -> Face {
        match (bold, italic) {
            (false, false) => Face::Regular,
            (true, false) => Face::Bold,
            (false, true) => Face::Italic,
            (true, true) => Face::BoldItalic,
        }
    }

    fn bold(self) -> bool {
        matches!(self, Face::Bold | Face::BoldItalic)
    }

    fn italic(self) -> bool {
        matches!(self, Face::Italic | Face::BoldItalic)
    }

    /// Its family among the window's fonts.
    fn family(self) -> FontFamily {
        FontFamily::Name(
            match self {
                Face::Regular => "grid",
                Face::Bold => "grid bold",
                Face::Italic => "grid italic",
                Face::BoldItalic => "grid bold italic",
            }
            .into(),
        )
    }
}

/// The families the grid's text is in, the first of them the system has:
/// Menlo, macOS's monospace font; DejaVu Sans Mono, Linux's, which Menlo
/// was made from. With neither, it is in egui's own Hack, made from it too.
const GRID: [&str; 2] = ["Menlo", "DejaVu Sans Mono"];

/// The families that draw what the grid's font and Hack lack (⟳), the first
/// of them the system has: Apple Symbols on macOS; on Linux DejaVu Sans,
/// else Noto's symbols.
const SYMBOLS: [&str; 4] = ["Apple Symbols", "DejaVu Sans", "Noto Sans Symbols2", "Noto Sans Symbols"];

/// A face of a font of the system's: its family, its file's bytes, kept
/// for as long as the program runs (egui holds its fonts so), and the
/// face's index among those of the file.
struct Found {
    family: String,
    data: &'static [u8],
    index: u32,
}

/// The system's fonts for the grid, as fontdb finds them where the system
/// keeps its fonts (on Linux, where fontconfig's settings say): the faces
/// of the first of `GRID` the system has, by `Face`, each the one nearest
/// its weight; and the first of `SYMBOLS`. Found once, however many windows
/// there are, each file read once.
fn found() -> &'static ([Option<Found>; 4], Option<Found>) {
    static FOUND: OnceLock<([Option<Found>; 4], Option<Found>)> = OnceLock::new();
    FOUND.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut files: HashMap<std::path::PathBuf, &'static [u8]> = HashMap::new();
        // The face `info` of `family`, its file read unless it was already.
        let mut read = |info: &fontdb::FaceInfo, family: &str| -> Option<Found> {
            let path = match &info.source {
                fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => path.clone(),
                fontdb::Source::Binary(_) => return None,
            };
            let data = match files.get(&path) {
                Some(data) => *data,
                None => {
                    let data: &'static [u8] = Box::leak(db.with_face_data(info.id, |data, _| data.to_vec())?.into_boxed_slice());
                    files.insert(path, data);
                    data
                }
            };
            Some(Found {
                family: family.to_string(),
                data,
                index: info.index,
            })
        };
        let of = |family: &str| db.faces().filter(|i| i.families.iter().any(|(name, _)| name == family)).collect::<Vec<_>>();
        let plain = |i: &&fontdb::FaceInfo| i.style == fontdb::Style::Normal && i.weight == fontdb::Weight::NORMAL;
        let mut faces: [Option<Found>; 4] = Default::default();
        if let Some((family, all)) = GRID.into_iter().map(|f| (f, of(f))).find(|(_, all)| all.iter().any(plain)) {
            for face in Face::ALL {
                let weight = if face.bold() { 700 } else { 400 };
                let nearest = all
                    .iter()
                    .filter(|i| (i.weight.0 >= 600) == face.bold() && (i.style != fontdb::Style::Normal) == face.italic())
                    .min_by_key(|i| i.weight.0.abs_diff(weight));
                faces[face as usize] = nearest.and_then(|i| read(i, family));
            }
        }
        let symbols = SYMBOLS.into_iter().find_map(|family| of(family).into_iter().find(plain).and_then(|i| read(i, family)));
        (faces, symbols)
    })
}

/// The fonts the grid is drawn with, and which of its faces they have of
/// their own: the first of `GRID` the system has (Menlo on macOS, DejaVu
/// Sans Mono on Linux), its four faces, then egui's Hack, then the first of
/// `SYMBOLS` for what both lack (⟳). With none of `GRID`, Hack, its bold and
/// italics made. After them, the rest of egui's own: its emoji, among
/// others.
fn fonts() -> (egui::FontDefinitions, [bool; 4]) {
    let mut defs = egui::FontDefinitions::default();
    let own = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let (found, symbols) = found();
    let mut add = |name: String, f: &Found| {
        let data = egui::FontData {
            font: std::borrow::Cow::Borrowed(f.data),
            index: f.index,
            tweak: Default::default(),
        };
        defs.font_data.insert(name.clone(), Arc::new(data));
        name
    };
    let faces: [Option<String>; 4] = Face::ALL.map(|face| found[face as usize].as_ref().map(|f| add(format!("{} {face:?}", f.family), f)));
    let symbols = symbols.as_ref().map(|f| add(f.family.clone(), f));
    let mut regular: Vec<String> = faces[Face::Regular as usize].iter().cloned().collect();
    let mut own = own.into_iter();
    regular.extend(own.next());
    regular.extend(symbols);
    regular.extend(own);
    let mut real = [true; 4];
    for face in Face::ALL {
        let mut chain = Vec::new();
        if face != Face::Regular {
            match &faces[face as usize] {
                Some(name) => chain.push(name.clone()),
                None => real[face as usize] = false,
            }
        }
        chain.extend(regular.iter().cloned());
        defs.families.insert(face.family(), chain);
    }
    (defs, real)
}

/// A colour of the editor's: none for the window's own, one of the sixteen,
/// one of xterm's 256, or as it is.
fn colour(c: Color) -> Option<Color32> {
    let named = match c {
        Color::Reset => return None,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Rgb(r, g, b) => return Some(Color32::from_rgb(r, g, b)),
        Color::Indexed(i) if i < 16 => i,
        // xterm's 256: a cube of six levels, then grays.
        Color::Indexed(i) if i < 232 => {
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            let i = i - 16;
            return Some(Color32::from_rgb(level(i / 36), level(i / 6 % 6), level(i % 6)));
        }
        Color::Indexed(i) => {
            let g = 8 + (i - 232) * 10;
            return Some(Color32::from_rgb(g, g, g));
        }
    };
    Some(PALETTE[named as usize])
}

/// `keep` of colour `a`, the rest of `b`.
fn mix(a: Color32, b: Color32, keep: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 * keep + y as f32 * (1.0 - keep)).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// How a cell looks, as a terminal draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    fg: Color32,
    /// Behind it: none for the window's own.
    bg: Option<Color32>,
    face: Face,
    under: bool,
    through: bool,
    hidden: bool,
}

impl Look {
    fn of(cell: &Cell) -> Look {
        let m = cell.modifier;
        let (fg, bg) = match (colour(cell.fg), colour(cell.bg)) {
            // Reversed: the colours swapped, the window's own among them.
            (fg, bg) if m.contains(Modifier::REVERSED) => (bg.unwrap_or(BG), Some(fg.unwrap_or(FG))),
            (fg, bg) => (fg.unwrap_or(FG), bg),
        };
        Look {
            // Dim: part of the way to what is behind it.
            fg: if m.contains(Modifier::DIM) { mix(fg, bg.unwrap_or(BG), DIM) } else { fg },
            bg,
            face: Face::of(m.contains(Modifier::BOLD), m.contains(Modifier::ITALIC)),
            under: m.contains(Modifier::UNDERLINED),
            through: m.contains(Modifier::CROSSED_OUT),
            hidden: m.contains(Modifier::HIDDEN),
        }
    }
}

/// A screen as the window paints it, by cells.
#[derive(Debug, Default, PartialEq)]
struct Painted {
    /// What is not on the window's own colour: a row's cells of one colour
    /// together.
    fills: Vec<Fill>,
    /// The text: a run of a row's cells of one colour and face, each a
    /// character that fits the grid, the blanks between them in it; else a
    /// cell alone, its glyph centred on it.
    texts: Vec<Text>,
    /// What is underlined or struck through.
    lines: Vec<Line>,
    /// What is drawn rather than written: the lines of boxes, and blocks.
    drawn: Vec<Drawing>,
    /// Where the cursor is, while text is written.
    cursor: Option<(u16, u16)>,
}

#[derive(Debug, Clone, PartialEq)]
struct Drawing {
    x: u16,
    y: u16,
    what: Drawn,
    colour: Color32,
}

/// What a terminal draws itself of a character, rather than take it from
/// its font, so that the lines of boxes meet from cell to cell: a line's
/// arms, and a block's part of its cell.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drawn {
    /// The arms going left, right, up and down from the centre: 0 for
    /// none, 1 light, 2 heavy.
    Arms([u8; 4]),
    /// The part of the cell covered, its corners as fractions of it: left,
    /// top, right, bottom.
    Block([f32; 4]),
}

/// How `c` is drawn, if it is.
fn drawn(c: char) -> Option<Drawn> {
    let arms = |l, r, u, d| Some(Drawn::Arms([l, r, u, d]));
    let block = |l, t, r, b| Some(Drawn::Block([l, t, r, b]));
    match c {
        '─' => arms(1, 1, 0, 0),
        '━' => arms(2, 2, 0, 0),
        '│' => arms(0, 0, 1, 1),
        '┃' => arms(0, 0, 2, 2),
        '┌' | '╭' => arms(0, 1, 0, 1),
        '┏' => arms(0, 2, 0, 2),
        '┐' | '╮' => arms(1, 0, 0, 1),
        '┓' => arms(2, 0, 0, 2),
        '└' | '╰' => arms(0, 1, 1, 0),
        '┗' => arms(0, 2, 2, 0),
        '┘' | '╯' => arms(1, 0, 1, 0),
        '┛' => arms(2, 0, 2, 0),
        '├' => arms(0, 1, 1, 1),
        '┣' => arms(0, 2, 2, 2),
        '┤' => arms(1, 0, 1, 1),
        '┫' => arms(2, 0, 2, 2),
        '┬' => arms(1, 1, 0, 1),
        '┳' => arms(2, 2, 0, 2),
        '┴' => arms(1, 1, 1, 0),
        '┻' => arms(2, 2, 2, 0),
        '┼' => arms(1, 1, 1, 1),
        '╋' => arms(2, 2, 2, 2),
        '╴' => arms(1, 0, 0, 0),
        '╵' => arms(0, 0, 1, 0),
        '╶' => arms(0, 1, 0, 0),
        '╷' => arms(0, 0, 0, 1),
        '╸' => arms(2, 0, 0, 0),
        '╹' => arms(0, 0, 2, 0),
        '╺' => arms(0, 2, 0, 0),
        '╻' => arms(0, 0, 0, 2),
        '▀' => block(0.0, 0.0, 1.0, 0.5),
        // ▁ to █: the lower eighths.
        '\u{2581}'..='\u{2588}' => block(0.0, 1.0 - (c as u32 - 0x2580) as f32 / 8.0, 1.0, 1.0),
        // ▉ to ▏: the left eighths, seven of them to one.
        '\u{2589}'..='\u{258f}' => block(0.0, 0.0, (0x2590 - c as u32) as f32 / 8.0, 1.0),
        '▐' => block(0.5, 0.0, 1.0, 1.0),
        '▔' => block(0.0, 0.0, 1.0, 0.125),
        '▕' => block(0.875, 0.0, 1.0, 1.0),
        _ => None,
    }
}

/// The rectangles that draw `d` in `cell`, a light line `thick` thick, a
/// heavy one twice that: each arm from the cell's edge to past its centre,
/// by half the line across it, so that corners close.
fn strokes(d: Drawn, cell: egui::Rect, thick: f32) -> Vec<egui::Rect> {
    let at = |fx: f32, fy: f32| pos2(cell.left() + fx * cell.width(), cell.top() + fy * cell.height());
    match d {
        Drawn::Block([l, t, r, b]) => vec![egui::Rect::from_min_max(at(l, t), at(r, b))],
        Drawn::Arms([l, r, u, d]) => {
            let c = cell.center();
            let half = |weight: u8| thick * weight as f32 / 2.0;
            let (h, v) = (half(u.max(d)), half(l.max(r)));
            let mut out = Vec::new();
            if l > 0 {
                out.push(egui::Rect::from_min_max(pos2(cell.left(), c.y - half(l)), pos2(c.x + h, c.y + half(l))));
            }
            if r > 0 {
                out.push(egui::Rect::from_min_max(pos2(c.x - h, c.y - half(r)), pos2(cell.right(), c.y + half(r))));
            }
            if u > 0 {
                out.push(egui::Rect::from_min_max(pos2(c.x - half(u), cell.top()), pos2(c.x + half(u), c.y + v)));
            }
            if d > 0 {
                out.push(egui::Rect::from_min_max(pos2(c.x - half(d), c.y - v), pos2(c.x + half(d), cell.bottom())));
            }
            out
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Fill {
    x: u16,
    y: u16,
    w: u16,
    colour: Color32,
}

#[derive(Debug, Clone, PartialEq)]
struct Text {
    x: u16,
    y: u16,
    /// The cells it covers.
    w: u16,
    text: String,
    colour: Color32,
    face: Face,
    alone: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Line {
    x: u16,
    y: u16,
    w: u16,
    colour: Color32,
    /// Struck through, rather than underlined.
    through: bool,
}

/// The screen `buf` as the window paints it, the cursor at `cursor`;
/// `fits` says whether a character is a cell wide in a face, as the grid's
/// cells are. A wide character covers its next cell, as in a terminal.
fn painted(buf: &Buffer, cursor: Option<(u16, u16)>, mut fits: impl FnMut(char, Face) -> bool) -> Painted {
    let mut out = Painted {
        cursor,
        ..Painted::default()
    };
    let area = buf.area;
    for y in 0..area.height {
        // The run of text going on, with the blanks after it.
        let mut run: Option<Text> = None;
        let mut x = 0;
        while x < area.width {
            let cell = &buf[(area.x + x, area.y + y)];
            let symbol = cell.symbol();
            let w = (symbol.width().max(1) as u16).min(area.width - x);
            let look = Look::of(cell);
            if let Some(bg) = look.bg {
                match out.fills.last_mut() {
                    Some(f) if f.y == y && f.x + f.w == x && f.colour == bg => f.w += w,
                    _ => out.fills.push(Fill { x, y, w, colour: bg }),
                }
            }
            for through in [false, true] {
                if (through && look.through) || (!through && look.under) {
                    match out.lines.iter_mut().rev().find(|l| l.through == through) {
                        Some(l) if l.y == y && l.x + l.w == x && l.colour == look.fg => l.w += w,
                        _ => out.lines.push(Line {
                            x,
                            y,
                            w,
                            colour: look.fg,
                            through,
                        }),
                    }
                }
            }
            let text: String = symbol.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
            let mut chars = text.chars();
            let one = match (chars.next(), chars.next()) {
                (None, _) => Some(' '),
                (Some(c), None) => Some(c),
                _ => None,
            };
            let shape = one.filter(|_| w == 1 && !look.hidden).and_then(drawn);
            if let Some(what) = shape {
                out.drawn.push(Drawing {
                    x,
                    y,
                    what,
                    colour: look.fg,
                });
            }
            match one {
                // Nothing to see, or nothing written: the run goes on over
                // it, if more comes.
                _ if look.hidden || one == Some(' ') || shape.is_some() => {
                    if let Some(r) = &mut run {
                        r.text.extend(std::iter::repeat_n(' ', w as usize));
                        r.w += w;
                    }
                }
                Some(c) if w == 1 && fits(c, look.face) => match &mut run {
                    Some(r) if r.colour == look.fg && r.face == look.face => {
                        r.text.push(c);
                        r.w += 1;
                    }
                    _ => {
                        end(&mut out.texts, run.take());
                        run = Some(Text {
                            x,
                            y,
                            w: 1,
                            text: c.to_string(),
                            colour: look.fg,
                            face: look.face,
                            alone: false,
                        });
                    }
                },
                _ => {
                    end(&mut out.texts, run.take());
                    out.texts.push(Text {
                        x,
                        y,
                        w,
                        text,
                        colour: look.fg,
                        face: look.face,
                        alone: true,
                    });
                }
            }
            x += w;
        }
        end(&mut out.texts, run);
    }
    out
}

/// A run of text ended: kept without the blanks at its end.
fn end(texts: &mut Vec<Text>, run: Option<Text>) {
    let Some(mut r) = run else { return };
    let kept = r.text.trim_end_matches(' ').len();
    r.w -= (r.text.len() - kept) as u16;
    r.text.truncate(kept);
    if !r.text.is_empty() {
        texts.push(r);
    }
}

/// Where the cells are in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    origin: Pos2,
    cell: Vec2,
    cols: u16,
    rows: u16,
}

impl Grid {
    /// The cells of `cell` points that fit in `area`, inside its room.
    fn new(area: egui::Rect, cell: Vec2) -> Grid {
        let fit = |room: f32, size: f32| match size > 0.0 {
            true => ((room - 2.0 * PAD) / size).floor().clamp(1.0, u16::MAX as f32) as u16,
            false => 1,
        };
        Grid {
            origin: area.min + vec2(PAD, PAD),
            cell,
            cols: fit(area.width(), cell.x),
            rows: fit(area.height(), cell.y),
        }
    }

    /// The cell at `pos`, if it is on one.
    fn cell_at(&self, pos: Pos2) -> Option<(u16, u16)> {
        let (x, y) = ((pos.x - self.origin.x) / self.cell.x, (pos.y - self.origin.y) / self.cell.y);
        let on = x >= 0.0 && y >= 0.0 && x < self.cols as f32 && y < self.rows as f32;
        on.then_some((x as u16, y as u16))
    }

    /// The cell nearest `pos`: the one it is on, else one on the grid's
    /// edge, as a drag past it selects up to there.
    fn nearest(&self, pos: Pos2) -> (u16, u16) {
        let at = |p: f32, origin: f32, size: f32, n: u16| ((p - origin) / size).floor().clamp(0.0, (n - 1) as f32) as u16;
        (at(pos.x, self.origin.x, self.cell.x, self.cols), at(pos.y, self.origin.y, self.cell.y, self.rows))
    }

    /// The left of column `x`, the top of row `y`.
    fn corner(&self, x: u16, y: u16) -> Pos2 {
        self.origin + vec2(x as f32 * self.cell.x, y as f32 * self.cell.y)
    }

    /// The area of `w` by `h` cells from cell (x, y). Cells side by side
    /// share their edge, to the bit.
    fn area(&self, x: u16, y: u16, w: u16, h: u16) -> egui::Rect {
        egui::Rect::from_min_max(self.corner(x, y), self.corner(x + w, y + h))
    }
}

/// What the mouse has done: the cell it is over, how far the wheel turned
/// short of a line, the text it selects, and what it drags.
#[derive(Debug, Default)]
struct Pointer {
    over: Option<(u16, u16)>,
    /// The window is not the active one: the mouse is not followed. Where
    /// the mouse was last seen over the window meanwhile, unless it left.
    away: bool,
    seen: Option<Pos2>,
    /// Where the mouse was last seen over the window, whatever it was
    /// doing, followed or not, unless it left: what the wheel turns over.
    at: Option<Pos2>,
    turned: f32,
    /// The left button held down, since it was pressed on that cell, with
    /// alt or not.
    down: Option<((u16, u16), bool)>,
    /// What was dragged over: selected, until a click or a key.
    selected: Option<Selection>,
    /// ⌘C with text selected (ctrl-c elsewhere): it, to copy.
    copy: bool,
    /// Where a press of the left button is held for the editor to drag
    /// what it is on, HEAD's row, as the screen last sent says: no text is
    /// selected from there.
    drags: Vec<Rect>,
    /// The left button held down since it was pressed there, and the cell
    /// the editor was told the mouse is over since, the one pressed at
    /// first.
    dragging: Option<(u16, u16)>,
}

/// Text selected with the mouse, from the cell the drag started on to the
/// one it is on, or ended on: a block of it with alt, else the lines from
/// one to the other. It keeps to the region it starts in (a pane, a box).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Selection {
    from: (u16, u16),
    to: (u16, u16),
    block: bool,
}

/// The region `at` is in: the last of `regions` it is in, drawn on top,
/// else the whole `screen`.
fn region_at(regions: &[Rect], at: (u16, u16), screen: Rect) -> Rect {
    let at = ratatui::layout::Position::new(at.0, at.1);
    regions.iter().rev().find(|r| r.contains(at)).copied().unwrap_or(screen)
}

/// The cells `s` selects in `region`, a row at a time: the row, and its
/// first and last column.
fn selected_rows(s: &Selection, region: Rect) -> Vec<(u16, u16, u16)> {
    if region.is_empty() {
        return Vec::new();
    }
    let (left, right) = (region.x, region.right() - 1);
    let kept = |(x, y): (u16, u16)| (x.clamp(left, right), y.clamp(region.y, region.bottom() - 1));
    let (a, b) = (kept(s.from), kept(s.to));
    let (first, last) = if (a.1, a.0) <= (b.1, b.0) { (a, b) } else { (b, a) };
    (first.1..=last.1)
        .map(|y| match s.block {
            true => (y, a.0.min(b.0), a.0.max(b.0)),
            false => (y, if y == first.1 { first.0 } else { left }, if y == last.1 { last.0 } else { right }),
        })
        .collect()
}

/// The text `s` selects of `buf`, in `region`: its rows, each without the
/// blanks at its end, a character as wide as two cells once.
fn selected_text(buf: &Buffer, s: &Selection, region: Rect) -> String {
    let rows: Vec<String> = selected_rows(s, region)
        .into_iter()
        .map(|(y, x0, x1)| {
            let mut row = String::new();
            let mut x = x0;
            while x <= x1 {
                let symbol = buf[(x, y)].symbol();
                row.push_str(symbol);
                x += symbol.width().max(1) as u16;
            }
            row.trim_end().to_string()
        })
        .collect();
    rows.join("\n")
}

impl Pointer {
    /// The window the active one or not, as `focused` says. When it stops
    /// being the active one, the mouse is said gone, and is not followed;
    /// when it is again, the mouse is said over the cell where it was last
    /// seen meanwhile, at once, unless a button is down. Else the mouse is
    /// followed from its next movement.
    fn focus(&mut self, focused: bool, grid: &Grid) -> Option<Input> {
        let away = !focused;
        if self.away == away {
            return None;
        }
        self.away = away;
        if away {
            return self.over.take().map(|_| Input::Unhover);
        }
        let at = grid.cell_at(self.seen.take()?).filter(|_| self.down.is_none() && self.dragging.is_none())?;
        self.over = Some(at);
        Some(Input::Hover(at.0, at.1))
    }

    /// The wheel turned `dy` of `unit` (more than 0 brings what is above),
    /// with what it turned before: the lines it turned, or the pages, down
    /// (up, below 0), once it turned one or more; a line for 40 points.
    fn wheel(&mut self, unit: egui::MouseWheelUnit, dy: f32) -> Option<Turned> {
        let (lines, pages) = match unit {
            egui::MouseWheelUnit::Point => (dy / NOTCH, false),
            egui::MouseWheelUnit::Line => (dy, false),
            egui::MouseWheelUnit::Page => (dy, true),
        };
        self.turned += lines;
        let mut down = 0;
        while self.turned.abs() >= 1.0 {
            down -= self.turned.signum() as i32;
            self.turned -= self.turned.signum();
        }
        match (down, pages) {
            (0, _) => None,
            (n, true) => Some(Turned::Pages(n)),
            (n, false) => Some(Turned::Lines(n)),
        }
    }
}

/// The window's events, for the editor: keys as the terminal would send
/// them (with ⌘, on macOS, the window's own: ⌘Q, ⌘V, ⌘+); a click on a
/// cell once the button is let go where it was pressed, ctrl-click the
/// right button's on macOS, which clicks on its press; the mouse over
/// another cell, or gone from the grid, while the window is the active one
/// (`focused`, as `Pointer::focus` takes it); the wheel turned over the
/// cell the mouse was last seen over, whether the window is the active one
/// or not, with the lines or the pages it turned; a paste typed, while
/// text is written (`writing`), and nothing elsewhere. A drag with the left
/// button selects the text it goes over, in `pointer`, the mouse over
/// nothing meanwhile; ⌘C (ctrl-c elsewhere) copies it. But from where the
/// editor drags what is pressed (`Pointer::drags`, HEAD's row): the press,
/// each other cell the mouse goes over with the button down, and the
/// button let go are the editor's, and nothing is selected.
fn inputs(events: &[egui::Event], grid: &Grid, pointer: &mut Pointer, focused: bool, writing: bool, mac: bool) -> Vec<Input> {
    let mut out = Vec::new();
    out.extend(pointer.focus(focused, grid));
    // The last key pressed: whether what alt typed with it is its own.
    let mut last = None;
    for e in events {
        // Where the mouse is, for the wheel, whatever else it does.
        match e {
            egui::Event::PointerMoved(pos) => pointer.at = Some(*pos),
            egui::Event::PointerGone => pointer.at = None,
            _ => {}
        }
        match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                last = Some((*key, *modifiers));
                let key = pressed(*key, *modifiers, mac);
                // A key for the editor: the selection gone with what it lit.
                if key.is_some() {
                    pointer.selected = None;
                }
                out.extend(key.map(Input::Key));
            }
            egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                let keys: Vec<Input> = text.chars().filter(|c| !c.is_control()).map(|c| Input::Key(typed(c, last))).collect();
                if !keys.is_empty() {
                    pointer.selected = None;
                }
                out.extend(keys);
            }
            egui::Event::Paste(text) if writing => out.extend(pasted(text).into_iter().map(Input::Key)),
            // ⌘C, ctrl-c elsewhere (egui's copy): the text selected; with
            // none, elsewhere than on macOS, the editor's ctrl-c all the
            // same, as ctrl-x is.
            egui::Event::Copy if pointer.selected.is_some() => pointer.copy = true,
            egui::Event::Copy if !mac => out.push(Input::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))),
            egui::Event::Cut if !mac => out.push(Input::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))),
            // Pressed where the editor drags from: each other cell the mouse
            // goes over said, for the editor to drag HEAD there.
            egui::Event::PointerMoved(pos) if pointer.dragging.is_some() => {
                if let Some(at) = grid.cell_at(*pos)
                    && pointer.dragging != Some(at)
                {
                    pointer.dragging = Some(at);
                    out.push(Input::Drag { x: at.0, y: at.1 });
                }
            }
            // Dragged: the text from where it started to here selected, the
            // cursor left where it is.
            egui::Event::PointerMoved(pos) if pointer.down.is_some() => {
                let (from, block) = pointer.down.unwrap();
                let to = grid.nearest(*pos);
                if to != from || pointer.selected.is_some() {
                    pointer.selected = Some(Selection { from, to, block });
                }
            }
            // Not the active window: the mouse is not followed, but where it
            // is seen is kept, for when the window is the active one again.
            egui::Event::PointerMoved(pos) if pointer.away => pointer.seen = Some(*pos),
            egui::Event::PointerMoved(pos) => match grid.cell_at(*pos) {
                Some(at) if pointer.over != Some(at) => {
                    pointer.over = Some(at);
                    out.push(Input::Hover(at.0, at.1));
                }
                None if pointer.over.is_some() => {
                    pointer.over = None;
                    out.push(Input::Unhover);
                }
                _ => {}
            },
            egui::Event::PointerGone => {
                pointer.seen = None;
                if pointer.over.take().is_some() {
                    out.push(Input::Unhover);
                }
            }
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers,
            } => {
                let right = match button {
                    egui::PointerButton::Secondary => true,
                    egui::PointerButton::Primary => mac && modifiers.ctrl,
                    _ => continue,
                };
                match (right, pressed) {
                    (true, true) => {
                        pointer.selected = None;
                        if let Some((x, y)) = grid.cell_at(*pos) {
                            out.push(Input::Click { x, y, right });
                        }
                    }
                    (true, false) => {}
                    // Pressed where the editor drags from (HEAD's row): held
                    // for the editor, which clicks there once the button is
                    // let go there, or drags; nothing selected.
                    (false, true) => {
                        pointer.selected = None;
                        let at = grid.cell_at(*pos);
                        let drags = |&(x, y): &(u16, u16)| pointer.drags.iter().any(|r| r.contains(ratatui::layout::Position::new(x, y)));
                        if let Some(at) = at.filter(drags) {
                            pointer.dragging = Some(at);
                            out.push(Input::Down { x: at.0, y: at.1 });
                            continue;
                        }
                        pointer.down = at.map(|at| (at, modifiers.alt));
                    }
                    // Held for the editor: let go, over a cell or not. Else let
                    // go where it was pressed: a click; elsewhere, what it went
                    // over selected.
                    (false, false) if pointer.dragging.is_some() => {
                        pointer.dragging = None;
                        out.push(Input::Up { at: grid.cell_at(*pos) });
                    }
                    (false, false) => {
                        if let Some((from, block)) = pointer.down.take() {
                            let to = grid.nearest(*pos);
                            match pointer.selected.is_none() && to == from {
                                true => out.push(Input::Click { x: from.0, y: from.1, right }),
                                false => pointer.selected = Some(Selection { from, to, block }),
                            }
                        }
                    }
                }
            }
            egui::Event::MouseWheel { unit, delta, .. } => {
                if let Some(turned) = pointer.wheel(*unit, delta.y) {
                    out.push(Input::Wheel { at: pointer.at.and_then(|p| grid.cell_at(p)), turned });
                }
            }
            _ => {}
        }
    }
    out
}

/// A key pressed, as the terminal would send it: none for a character
/// typed (the text that comes with it is what is typed) unless with ctrl,
/// and none for the window's own (⌘, on macOS).
fn pressed(key: egui::Key, mods: egui::Modifiers, mac: bool) -> Option<KeyEvent> {
    use egui::Key as K;
    if mods.mac_cmd || (mac && mods.command) {
        return None;
    }
    let mut m = KeyModifiers::NONE;
    for (on, k) in [(mods.ctrl, KeyModifiers::CONTROL), (mods.alt, KeyModifiers::ALT), (mods.shift, KeyModifiers::SHIFT)] {
        if on {
            m |= k;
        }
    }
    let code = match key {
        K::Enter => KeyCode::Enter,
        K::Escape => KeyCode::Esc,
        K::Backspace => KeyCode::Backspace,
        K::Delete => KeyCode::Delete,
        K::Insert => KeyCode::Insert,
        K::Tab if mods.shift => {
            m.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        K::Tab => KeyCode::Tab,
        K::ArrowUp => KeyCode::Up,
        K::ArrowDown => KeyCode::Down,
        K::ArrowLeft => KeyCode::Left,
        K::ArrowRight => KeyCode::Right,
        K::PageUp => KeyCode::PageUp,
        K::PageDown => KeyCode::PageDown,
        K::Home => KeyCode::Home,
        K::End => KeyCode::End,
        _ => match key.name().strip_prefix('F').and_then(|n| n.parse().ok()) {
            Some(n) => KeyCode::F(n),
            // A character with ctrl: no text comes with it. Shift is in the
            // character, as the keys' settings have it.
            None if mods.ctrl => {
                m.remove(KeyModifiers::SHIFT);
                KeyCode::Char(key_char(key)?)
            }
            None => return None,
        },
    };
    Some(KeyEvent::new(code, m))
}

/// The character a key is for, unshifted: `a` for A.
fn key_char(key: egui::Key) -> Option<char> {
    match key {
        egui::Key::Minus => Some('-'),
        egui::Key::Space => Some(' '),
        egui::Key::Quote => Some('\''),
        _ => {
            let mut cs = key.symbol_or_name().chars();
            match (cs.next(), cs.next()) {
                (Some(c), None) if c.is_ascii_graphic() => Some(c.to_ascii_lowercase()),
                _ => None,
            }
        }
    }
}

/// A character typed after the key `last`: with alt, if alt was down and
/// the key typed its own character rather than one alt made of it (on
/// macOS, option types ∂ for d, é for e then e).
fn typed(c: char, last: Option<(egui::Key, egui::Modifiers)>) -> KeyEvent {
    let alt = last.is_some_and(|(key, m)| m.alt && !m.ctrl && key_char(key) == Some(c.to_ascii_lowercase()));
    KeyEvent::new(KeyCode::Char(c), if alt { KeyModifiers::ALT } else { KeyModifiers::NONE })
}

/// Text pasted, as the keys that type it: a line's end Enter, a tab a
/// space, the rest of what does not print left out.
fn pasted(text: &str) -> Vec<KeyEvent> {
    text.chars()
        .filter_map(|c| match c {
            '\n' => Some(KeyCode::Enter),
            '\t' => Some(KeyCode::Char(' ')),
            c if c.is_control() => None,
            c => Some(KeyCode::Char(c)),
        })
        .map(|code| KeyEvent::new(code, KeyModifiers::NONE))
        .collect()
}

/// Paints `p` on the grid: what is behind the text, the text, then what is
/// drawn, the text's lines and the cursor. A face the fonts lack is made:
/// bold drawn twice, a pixel apart; italics slanted.
fn paint(painter: &egui::Painter, grid: &Grid, p: &Painted, real: &[bool; 4]) {
    let ppp = painter.pixels_per_point();
    let snap = |r: egui::Rect| r.round_to_pixels(ppp);
    let mut behind = egui::Mesh::default();
    for f in &p.fills {
        behind.add_colored_rect(snap(grid.area(f.x, f.y, f.w, 1)), f.colour);
    }
    painter.add(egui::Shape::mesh(behind));
    let fonts = Face::ALL.map(|face| FontId::new(FONT_SIZE, face.family()));
    let pixel = 1.0 / ppp;
    for t in &p.texts {
        let made = !real[t.face as usize];
        let format = egui::TextFormat {
            font_id: fonts[t.face as usize].clone(),
            color: t.colour,
            italics: made && t.face.italic(),
            ..Default::default()
        };
        let galley = painter.layout_job(egui::text::LayoutJob::single_section(t.text.clone(), format));
        let at = grid.area(t.x, t.y, t.w, 1);
        let x = if t.alone { at.center().x - galley.size().x / 2.0 } else { at.left() };
        let pos = pos2(x, at.top()).round_to_pixels(ppp);
        if made && t.face.bold() {
            painter.galley(pos + vec2(pixel, 0.0), galley.clone(), t.colour);
        }
        painter.galley(pos, galley, t.colour);
    }
    let mut over = egui::Mesh::default();
    // A light line: a point, in whole pixels, as thick in every cell.
    let thick = (FONT_SIZE / 14.0 * ppp).round().max(1.0) / ppp;
    for d in &p.drawn {
        for r in strokes(d.what, grid.area(d.x, d.y, 1, 1), thick) {
            over.add_colored_rect(snap(r), d.colour);
        }
    }
    for l in &p.lines {
        let at = grid.area(l.x, l.y, l.w, 1);
        let y = at.top() + at.height() * if l.through { 0.57 } else { 0.9 };
        over.add_colored_rect(snap(egui::Rect::from_min_max(pos2(at.left(), y), pos2(at.right(), y + thick))), l.colour);
    }
    if let Some((x, y)) = p.cursor {
        let at = grid.area(x, y, 1, 1);
        over.add_colored_rect(snap(egui::Rect::from_min_size(at.min, vec2(2.0 * thick, at.height()))), FG);
    }
    painter.add(egui::Shape::mesh(over));
}

/// Over the screen while `$EDITOR` runs `program`, in `area`: what the
/// window waits for.
fn waiting(painter: &egui::Painter, area: egui::Rect, program: &str) {
    // Opened as an app: in a terminal window of its own.
    let place = match crate::bundle::no_terminal() {
        true => crate::launcher::IN_TERMINAL,
        false => "in the terminal StackED was started from",
    };
    painter.rect_filled(area, 0.0, Color32::from_black_alpha(190));
    painter.text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        format!("The text is open in {program}, {place} if {program} runs in one.\nStackED goes on once {program} exits."),
        FontId::proportional(15.0),
        Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};
    use ratatui::style::Style;

    fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    fn plain(code: KeyCode) -> Input {
        Input::Key(key(code, KeyModifiers::NONE))
    }

    #[test]
    fn colours_are_the_terminals() {
        assert_eq!(colour(Color::Reset), None, "the window's own");
        assert_eq!(colour(Color::Green), Some(Color32::from_rgb(0x0d, 0xbc, 0x79)));
        assert_eq!(colour(Color::Indexed(12)), colour(Color::LightBlue));
        assert_eq!(colour(Color::Indexed(196)), Some(Color32::from_rgb(0xff, 0, 0)));
        assert_eq!(colour(Color::Indexed(238)), Some(Color32::from_rgb(0x44, 0x44, 0x44)));
        assert_eq!(colour(Color::Rgb(1, 2, 3)), Some(Color32::from_rgb(1, 2, 3)));
    }

    #[test]
    fn a_screen_is_painted_as_a_terminal_draws_it() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
        buf.set_string(0, 0, "ab c", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD));
        buf.set_string(5, 0, "日→x", Style::new().bg(Color::Indexed(238)));
        buf.set_string(0, 1, "rev", Style::new().add_modifier(Modifier::REVERSED));
        buf.set_string(4, 1, "dim", Style::new().fg(Color::Rgb(200, 100, 0)).add_modifier(Modifier::DIM | Modifier::UNDERLINED));
        buf.set_string(8, 1, "it", Style::new().add_modifier(Modifier::ITALIC | Modifier::HIDDEN));
        buf.set_string(0, 2, "e\u{301}q", Style::new().add_modifier(Modifier::ITALIC | Modifier::CROSSED_OUT));
        // Only ASCII fits the grid here: → is drawn alone.
        let p = painted(&buf, Some((3, 2)), |c, _| c.is_ascii());
        let text = |x, y, w, text: &str, colour, face, alone| Text {
            x,
            y,
            w,
            text: text.into(),
            colour,
            face,
            alone,
        };
        let grey = Color32::from_rgb(0x44, 0x44, 0x44);
        let dim = Color32::from_rgb(132, 72, 12);
        assert_eq!(
            p.texts,
            [
                // The blank between them in the run, the blanks after it not.
                text(0, 0, 4, "ab c", PALETTE[2], Face::Bold, false),
                // The wide character covers two cells.
                text(5, 0, 2, "日", FG, Face::Regular, true),
                text(7, 0, 1, "→", FG, Face::Regular, true),
                text(8, 0, 1, "x", FG, Face::Regular, false),
                // Reversed: the window's colours swapped.
                text(0, 1, 3, "rev", BG, Face::Regular, false),
                // Dim: 60% of its colour, the rest the window's.
                text(4, 1, 3, "dim", dim, Face::Regular, false),
                // e and its accent, one cell of two characters, alone.
                text(0, 2, 1, "e\u{301}", FG, Face::Italic, true),
                text(1, 2, 1, "q", FG, Face::Italic, false),
            ]
        );
        assert_eq!(
            p.fills,
            [
                Fill { x: 5, y: 0, w: 4, colour: grey },
                Fill { x: 0, y: 1, w: 3, colour: FG },
            ]
        );
        assert_eq!(
            p.lines,
            [
                Line { x: 4, y: 1, w: 3, colour: dim, through: false },
                Line { x: 0, y: 2, w: 2, colour: FG, through: true },
            ]
        );
        assert_eq!(p.cursor, Some((3, 2)));
        assert_eq!(p.drawn, []);
    }

    #[test]
    fn boxes_are_drawn_so_that_their_lines_meet() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 3));
        buf.set_string(0, 0, "┌─┐", Style::new().fg(Color::Blue));
        buf.set_string(0, 1, "│x│▏", Style::new().fg(Color::Blue));
        buf.set_string(0, 2, "└─┘", Style::new().fg(Color::Blue).add_modifier(Modifier::HIDDEN));
        let p = painted(&buf, None, |_, _| true);
        let at: Vec<(u16, u16, Drawn)> = p.drawn.iter().map(|d| (d.x, d.y, d.what)).collect();
        assert_eq!(
            at,
            [
                (0, 0, Drawn::Arms([0, 1, 0, 1])),
                (1, 0, Drawn::Arms([1, 1, 0, 0])),
                (2, 0, Drawn::Arms([1, 0, 0, 1])),
                (0, 1, Drawn::Arms([0, 0, 1, 1])),
                (2, 1, Drawn::Arms([0, 0, 1, 1])),
                (3, 1, Drawn::Block([0.0, 0.0, 0.125, 1.0])),
            ],
            "hidden, nothing"
        );
        assert!(p.drawn.iter().all(|d| d.colour == PALETTE[4]));
        // What is drawn is not written; the run goes on over it.
        assert_eq!(p.texts.iter().map(|t| (t.x, t.y, t.text.as_str())).collect::<Vec<_>>(), [(1, 1, "x")]);
        assert_eq!(drawn('█'), Some(Drawn::Block([0.0, 0.0, 1.0, 1.0])));
        assert_eq!(drawn('▁'), Some(Drawn::Block([0.0, 0.875, 1.0, 1.0])));
        assert_eq!(drawn('a'), None);
        // ┌ in a cell of 10 by 20, lines 2 thick: its arms meet at the centre.
        let cell = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 20.0));
        assert_eq!(
            strokes(Drawn::Arms([0, 1, 0, 1]), cell, 2.0),
            [
                egui::Rect::from_min_max(pos2(4.0, 9.0), pos2(10.0, 11.0)),
                egui::Rect::from_min_max(pos2(4.0, 9.0), pos2(6.0, 20.0)),
            ]
        );
        // │ heavy: 4 thick, edge to edge.
        assert_eq!(
            strokes(Drawn::Arms([0, 0, 2, 2]), cell, 2.0),
            [
                egui::Rect::from_min_max(pos2(3.0, 0.0), pos2(7.0, 10.0)),
                egui::Rect::from_min_max(pos2(3.0, 10.0), pos2(7.0, 20.0)),
            ]
        );
        assert_eq!(
            strokes(Drawn::Block([0.0, 0.5, 1.0, 1.0]), cell, 2.0),
            [egui::Rect::from_min_max(pos2(0.0, 10.0), pos2(10.0, 20.0))]
        );
    }

    #[test]
    fn keys_are_sent_as_the_terminal_sends_them() {
        let none = Modifiers::NONE;
        let k = |code, m| Some(key(code, m));
        assert_eq!(pressed(Key::Enter, none, true), k(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(pressed(Key::Escape, none, true), k(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(pressed(Key::Tab, Modifiers::SHIFT, true), k(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(pressed(Key::ArrowDown, Modifiers::SHIFT, true), k(KeyCode::Down, KeyModifiers::SHIFT));
        assert_eq!(pressed(Key::ArrowLeft, Modifiers::ALT, true), k(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(pressed(Key::F5, none, true), k(KeyCode::F(5), KeyModifiers::NONE));
        assert_eq!(pressed(Key::C, Modifiers::CTRL, true), k(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(pressed(Key::Minus, Modifiers::CTRL | Modifiers::SHIFT, true), k(KeyCode::Char('-'), KeyModifiers::CONTROL));
        // A character typed: its text says it.
        assert_eq!(pressed(Key::A, none, true), None);
        assert_eq!(pressed(Key::Space, none, true), None);
        // ⌘ on macOS: the window's own.
        let cmd = Modifiers {
            mac_cmd: true,
            command: true,
            ..none
        };
        assert_eq!(pressed(Key::Q, cmd, true), None);
        assert_eq!(pressed(Key::ArrowDown, cmd, true), None);
        // Elsewhere egui's command is ctrl: ctrl-c is the editor's still.
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..none
        };
        assert_eq!(pressed(Key::C, ctrl, false), k(KeyCode::Char('c'), KeyModifiers::CONTROL));
        // Alt typing a character of its own, on macOS, types it; alt with a
        // key's own character is alt-that.
        assert_eq!(typed('∂', Some((Key::D, Modifiers::ALT))), key(KeyCode::Char('∂'), KeyModifiers::NONE));
        assert_eq!(typed('d', Some((Key::D, Modifiers::ALT))), key(KeyCode::Char('d'), KeyModifiers::ALT));
        assert_eq!(typed('D', Some((Key::D, Modifiers::ALT | Modifiers::SHIFT))), key(KeyCode::Char('D'), KeyModifiers::ALT));
        assert_eq!(typed('?', None), key(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(
            pasted("a\r\nb\tc\u{7}"),
            [
                key(KeyCode::Char('a'), KeyModifiers::NONE),
                key(KeyCode::Enter, KeyModifiers::NONE),
                key(KeyCode::Char('b'), KeyModifiers::NONE),
                key(KeyCode::Char(' '), KeyModifiers::NONE),
                key(KeyCode::Char('c'), KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn the_window_s_events_are_the_editor_s_input() {
        // Cells of 10 by 20 points, from (6, 6): 8 by 4 of them.
        let grid = Grid::new(egui::Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)), vec2(10.0, 20.0));
        assert_eq!((grid.cols, grid.rows), (8, 4));
        assert_eq!(grid.cell_at(pos2(6.0, 6.0)), Some((0, 0)));
        assert_eq!(grid.cell_at(pos2(16.0, 26.0)), Some((1, 1)));
        assert_eq!(grid.cell_at(pos2(5.0, 30.0)), None, "in the room around the grid");
        assert_eq!(grid.cell_at(pos2(86.0, 30.0)), None, "past its last column");
        let mut pointer = Pointer::default();
        let mut run = |events: Vec<egui::Event>, writing: bool, mac: bool| inputs(&events, &grid, &mut pointer, true, writing, mac);
        let moved = |x, y| egui::Event::PointerMoved(pos2(x, y));
        let click = |button, pressed, modifiers| egui::Event::PointerButton {
            pos: pos2(16.0, 26.0),
            button,
            pressed,
            modifiers,
        };
        let wheel = |unit, dy| egui::Event::MouseWheel {
            unit,
            delta: vec2(0.0, dy),
            modifiers: Modifiers::NONE,
        };
        // Over another cell: said once; off the grid, or gone: said too.
        assert_eq!(
            run(vec![moved(16.0, 26.0), moved(17.0, 27.0), moved(200.0, 200.0), egui::Event::PointerGone], false, true),
            [Input::Hover(1, 1), Input::Unhover]
        );
        assert_eq!(run(vec![moved(26.0, 6.0), egui::Event::PointerGone], false, true), [Input::Hover(2, 0), Input::Unhover]);
        // A click once let go where it was pressed; the right button, and
        // ctrl-click on macOS, a right click on its press.
        let ctrl = Modifiers::CTRL;
        assert_eq!(
            run(
                vec![
                    click(egui::PointerButton::Primary, true, Modifiers::NONE),
                    click(egui::PointerButton::Primary, false, Modifiers::NONE),
                    click(egui::PointerButton::Secondary, true, Modifiers::NONE),
                    click(egui::PointerButton::Primary, true, ctrl),
                    click(egui::PointerButton::Middle, true, Modifiers::NONE),
                ],
                false,
                true
            ),
            [
                Input::Click { x: 1, y: 1, right: false },
                Input::Click { x: 1, y: 1, right: true },
                Input::Click { x: 1, y: 1, right: true },
            ]
        );
        assert_eq!(run(vec![click(egui::PointerButton::Primary, true, ctrl)], false, false), []);
        assert_eq!(
            run(vec![click(egui::PointerButton::Primary, false, ctrl)], false, false),
            [Input::Click { x: 1, y: 1, right: false }]
        );
        // The wheel, over the cell the mouse was last seen over: a line for
        // 40 points, with what it turned before, down; more than 0 brings
        // what is above, a line up. Pages as pages.
        let wheeled = |at, turned| Input::Wheel { at, turned };
        assert_eq!(run(vec![moved(16.0, 26.0), wheel(egui::MouseWheelUnit::Point, -30.0)], false, true), [Input::Hover(1, 1)]);
        assert_eq!(run(vec![wheel(egui::MouseWheelUnit::Point, -30.0)], false, true), [wheeled(Some((1, 1)), Turned::Lines(1))]);
        assert_eq!(run(vec![wheel(egui::MouseWheelUnit::Point, 100.0)], false, true), [wheeled(Some((1, 1)), Turned::Lines(-2))]);
        assert_eq!(
            run(vec![wheel(egui::MouseWheelUnit::Line, -1.0), wheel(egui::MouseWheelUnit::Page, 1.0)], false, true),
            [wheeled(Some((1, 1)), Turned::Lines(1)), wheeled(Some((1, 1)), Turned::Pages(-1))]
        );
        // While a comment is written as well: the editor scrolls its box with
        // them, or takes them for ↑↓.
        assert_eq!(run(vec![wheel(egui::MouseWheelUnit::Line, -2.0)], true, true), [wheeled(Some((1, 1)), Turned::Lines(2))]);
        // Off the grid, or gone from the window: over no cell.
        assert_eq!(
            run(vec![moved(200.0, 200.0), wheel(egui::MouseWheelUnit::Line, -1.0)], false, true),
            [Input::Unhover, wheeled(None, Turned::Lines(1))]
        );
        assert_eq!(
            run(vec![moved(26.0, 6.0), egui::Event::PointerGone, wheel(egui::MouseWheelUnit::Line, 1.0)], false, true),
            [Input::Hover(2, 0), Input::Unhover, wheeled(None, Turned::Lines(-1))]
        );
        // A key and the text it types: the text; ctrl, the key.
        let press = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        assert_eq!(
            run(
                vec![
                    press(Key::Num1, Modifiers::NONE),
                    egui::Event::Text("1".into()),
                    press(Key::C, ctrl),
                    press(Key::ArrowUp, Modifiers::NONE),
                ],
                false,
                true
            ),
            [plain(KeyCode::Char('1')), Input::Key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)), plain(KeyCode::Up)]
        );
        // A paste types, while text is written; elsewhere, nothing.
        assert_eq!(run(vec![egui::Event::Paste("a\nb".into())], false, true), []);
        assert_eq!(
            run(vec![egui::Event::Paste("a\nb".into())], true, true),
            [plain(KeyCode::Char('a')), plain(KeyCode::Enter), plain(KeyCode::Char('b'))]
        );
        // Copy and cut: ctrl-c and ctrl-x elsewhere than on macOS, where
        // they are ⌘C and ⌘X.
        assert_eq!(run(vec![egui::Event::Copy, egui::Event::Cut], false, true), []);
        assert_eq!(
            run(vec![egui::Event::Copy], false, false),
            [Input::Key(key(KeyCode::Char('c'), KeyModifiers::CONTROL))]
        );
    }

    #[test]
    fn a_drag_selects_text_which_copy_copies() {
        // Cells of 10 by 20 points, from (6, 6): 8 by 4 of them.
        let grid = Grid::new(egui::Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)), vec2(10.0, 20.0));
        let mut pointer = Pointer::default();
        let run = |pointer: &mut Pointer, events: Vec<egui::Event>, mac: bool| inputs(&events, &grid, pointer, true, false, mac);
        let on = |x: u16, y: u16| pos2(6.0 + 10.0 * x as f32 + 1.0, 6.0 + 20.0 * y as f32 + 1.0);
        let button = |pos, pressed, modifiers| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        let drag = |from: Pos2, to: Pos2, modifiers| {
            vec![button(from, true, modifiers), egui::Event::PointerMoved(to), button(to, false, modifiers)]
        };
        // Dragged with the button down: no click, and the mouse over nothing;
        // what it went over selected, from where it was pressed.
        assert_eq!(run(&mut pointer, drag(on(1, 1), on(3, 2), Modifiers::NONE), true), []);
        assert_eq!(pointer.selected, Some(Selection { from: (1, 1), to: (3, 2), block: false }));
        // With alt, a block of it; past the grid, up to its edge.
        run(&mut pointer, drag(on(2, 0), pos2(500.0, 500.0), Modifiers::ALT), true);
        assert_eq!(pointer.selected, Some(Selection { from: (2, 0), to: (7, 3), block: true }));
        // Copied with ⌘C, ctrl-c elsewhere: the editor's ctrl-c then is not.
        assert_eq!(run(&mut pointer, vec![egui::Event::Copy], false), []);
        assert!(std::mem::take(&mut pointer.copy));
        assert_eq!(run(&mut pointer, vec![egui::Event::Copy], true), []);
        assert!(std::mem::take(&mut pointer.copy) && pointer.selected.is_some(), "kept, to copy again");
        // A key for the editor: the selection gone. A click too, once let go
        // where it was pressed.
        assert_eq!(run(&mut pointer, vec![egui::Event::Text("j".into())], true), [plain(KeyCode::Char('j'))]);
        assert_eq!(pointer.selected, None);
        run(&mut pointer, drag(on(1, 1), on(2, 1), Modifiers::NONE), true);
        assert!(pointer.selected.is_some());
        assert_eq!(
            run(&mut pointer, drag(on(1, 1), on(1, 1), Modifiers::NONE), true),
            [Input::Click { x: 1, y: 1, right: false }]
        );
        assert_eq!(pointer.selected, None);
        // Nothing selected: ctrl-c is the editor's, elsewhere than on macOS.
        assert_eq!(
            run(&mut pointer, vec![egui::Event::Copy], false),
            [Input::Key(key(KeyCode::Char('c'), KeyModifiers::CONTROL))]
        );
    }

    #[test]
    fn the_mouse_is_followed_only_while_the_window_is_the_active_one() {
        // Cells of 10 by 20 points, from (6, 6): 8 by 4 of them.
        let grid = Grid::new(egui::Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)), vec2(10.0, 20.0));
        let mut pointer = Pointer::default();
        let run = |pointer: &mut Pointer, events: Vec<egui::Event>, focused: bool| inputs(&events, &grid, pointer, focused, false, true);
        let on = |x: u16, y: u16| pos2(6.0 + 10.0 * x as f32 + 1.0, 6.0 + 20.0 * y as f32 + 1.0);
        let moved = |x, y| egui::Event::PointerMoved(on(x, y));
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(run(&mut pointer, vec![moved(1, 1)], true), [Input::Hover(1, 1)]);
        // No longer the active window: the mouse said gone at once, then not
        // followed.
        assert_eq!(run(&mut pointer, vec![], false), [Input::Unhover]);
        assert_eq!(run(&mut pointer, vec![moved(2, 1), moved(3, 2)], false), []);
        // A click and the wheel act there all the same, the wheel over the
        // cell where the mouse was seen.
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, -1.0),
            modifiers: Modifiers::NONE,
        };
        assert_eq!(
            run(&mut pointer, vec![button(on(3, 2), true), button(on(3, 2), false), wheel], false),
            [Input::Click { x: 3, y: 2, right: false }, Input::Wheel { at: Some((3, 2)), turned: Turned::Lines(1) }]
        );
        // The active window again: the mouse said over the cell where it was
        // last seen, at once, then followed.
        assert_eq!(run(&mut pointer, vec![], true), [Input::Hover(3, 2)]);
        assert_eq!(run(&mut pointer, vec![moved(4, 2)], true), [Input::Hover(4, 2)]);
        // Gone from the window meanwhile, or not seen over it since: the
        // mouse followed from its next movement.
        assert_eq!(run(&mut pointer, vec![moved(5, 2), egui::Event::PointerGone], false), [Input::Unhover]);
        assert_eq!(run(&mut pointer, vec![], true), []);
        assert_eq!(run(&mut pointer, vec![moved(5, 3)], true), [Input::Hover(5, 3)]);
        assert_eq!(run(&mut pointer, vec![], false), [Input::Unhover]);
        assert_eq!(run(&mut pointer, vec![], true), []);
        assert_eq!(run(&mut pointer, vec![moved(5, 3)], true), [Input::Hover(5, 3)]);
        // Pressed while the window is not the active one, then dragged once
        // it is: the mouse said over nothing while its button is down, and
        // what it went over selected, as ever.
        assert_eq!(run(&mut pointer, vec![moved(2, 0), button(on(2, 0), true)], false), [Input::Unhover]);
        assert_eq!(run(&mut pointer, vec![], true), []);
        assert_eq!(run(&mut pointer, vec![moved(4, 0), button(on(4, 0), false)], true), []);
        assert_eq!(pointer.selected, Some(Selection { from: (2, 0), to: (4, 0), block: false }));
        assert_eq!(run(&mut pointer, vec![moved(4, 1)], true), [Input::Hover(4, 1)]);
    }

    #[test]
    fn a_press_where_the_editor_drags_is_the_editor_s_and_every_other_drag_selects_text() {
        // Cells of 10 by 20 points, from (6, 6): 8 by 4 of them; HEAD's row
        // the third, where the screen sent says a press is held for a drag.
        let grid = Grid::new(egui::Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)), vec2(10.0, 20.0));
        let mut pointer = Pointer {
            drags: vec![Rect::new(0, 2, 8, 1)],
            ..Pointer::default()
        };
        let run = |pointer: &mut Pointer, events: Vec<egui::Event>| inputs(&events, &grid, pointer, true, false, true);
        let on = |x: u16, y: u16| pos2(6.0 + 10.0 * x as f32 + 1.0, 6.0 + 20.0 * y as f32 + 1.0);
        let moved = |x, y| egui::Event::PointerMoved(on(x, y));
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        // Pressed there: the editor's, nothing selected; each other cell the
        // mouse goes over with the button down, said once; let go, said with
        // the cell it is over.
        assert_eq!(run(&mut pointer, vec![button(on(1, 2), true)]), [Input::Down { x: 1, y: 2 }]);
        assert_eq!(
            run(&mut pointer, vec![moved(1, 2), moved(3, 1), moved(3, 1), moved(4, 0)]),
            [Input::Drag { x: 3, y: 1 }, Input::Drag { x: 4, y: 0 }]
        );
        assert_eq!(run(&mut pointer, vec![button(on(4, 0), false)]), [Input::Up { at: Some((4, 0)) }]);
        assert_eq!(pointer.selected, None);
        // Let go where it was pressed: the editor's too, which clicks there;
        // let go off the grid, over no cell.
        assert_eq!(
            run(&mut pointer, vec![button(on(2, 2), true), button(on(2, 2), false)]),
            [Input::Down { x: 2, y: 2 }, Input::Up { at: Some((2, 2)) }]
        );
        assert_eq!(
            run(&mut pointer, vec![button(on(2, 2), true), button(pos2(500.0, 500.0), false)]),
            [Input::Down { x: 2, y: 2 }, Input::Up { at: None }]
        );
        // Anywhere else, a drag selects text as ever, and a press let go
        // where it was pressed is a click.
        assert_eq!(run(&mut pointer, vec![button(on(1, 1), true), moved(3, 1), button(on(3, 1), false)]), []);
        assert_eq!(pointer.selected, Some(Selection { from: (1, 1), to: (3, 1), block: false }));
        assert_eq!(run(&mut pointer, vec![button(on(1, 3), true), button(on(1, 3), false)]), [Input::Click { x: 1, y: 3, right: false }]);
        // No row of the editor's to drag: as anywhere else.
        pointer.drags.clear();
        assert_eq!(run(&mut pointer, vec![button(on(1, 2), true), button(on(1, 2), false)]), [Input::Click { x: 1, y: 2, right: false }]);
    }

    #[test]
    fn the_text_selected_keeps_to_the_region_it_starts_in() {
        // Two panes, a line between them, and the bar above them.
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        buf.set_string(0, 0, " bar: 3/5 lines", Style::new());
        for (y, left, right) in [(1, "patch one", "fn one() {"), (2, "patch two", "    1 +"), (3, "", "}")] {
            buf.set_string(0, y, left, Style::new());
            buf.set_string(9, y, "│", Style::new());
            buf.set_string(10, y, right, Style::new());
        }
        let (list, code) = (Rect::new(0, 1, 9, 3), Rect::new(10, 1, 10, 3));
        let regions = [Rect::new(0, 1, 20, 3), list, code];
        let text = |buf: &Buffer, from, to, block| {
            let s = Selection { from, to, block };
            selected_text(buf, &s, region_at(&regions, from, buf.area))
        };
        // From a cell of the code to one of another line: the lines between
        // whole, but none of the list beside them, each line's blanks left
        // out at its end.
        assert_eq!(text(&buf, (13, 1), (11, 3), false), "one() {\n    1 +\n}");
        // Dragged up, the same.
        assert_eq!(text(&buf, (11, 3), (13, 1), false), "one() {\n    1 +\n}");
        // Out of its pane, into the list: up to its edge.
        assert_eq!(text(&buf, (10, 1), (2, 3), false), "fn one() {\n    1 +\n}");
        // A block: its columns only.
        assert_eq!(text(&buf, (10, 1), (11, 3), true), "fn\n\n}");
        // In the bar, in no pane: the whole screen's row.
        assert_eq!(text(&buf, (1, 0), (19, 0), false), "bar: 3/5 lines");
        // A character two cells wide, once.
        buf.set_string(10, 3, "日本", Style::new());
        assert_eq!(text(&buf, (10, 3), (13, 3), false), "日本");
    }

    /// A session whose editor is the test: what the window sends it, and
    /// where to send it screens.
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

    fn frame(ctx: &egui::Context, window: &mut Window, events: Vec<egui::Event>) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| window.frame(ctx))
    }

    /// A frame, as `frame` runs one, the window the active one or not as
    /// `focused` says, which eframe says twice: in the input, and in the
    /// viewport's info.
    fn frame_focused(ctx: &egui::Context, window: &mut Window, focused: bool, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            focused,
            ..Default::default()
        };
        let info = egui::ViewportInfo {
            focused: Some(focused),
            ..Default::default()
        };
        input.viewports.insert(egui::ViewportId::ROOT, info);
        ctx.run(input, |ctx| window.frame(ctx))
    }

    /// The text painted, and the colour each was painted in.
    fn texts(out: &egui::FullOutput) -> Vec<(String, Color32, Pos2)> {
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.fallback_color, t.pos)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_window_paints_what_the_editor_draws_and_sends_it_what_is_done() {
        let (session, inputs, outputs) = stand_in();
        let ctx = egui::Context::default();
        let (fonts, real) = fonts();
        ctx.set_fonts(fonts);
        let mut window = Window::with(session, real, program("/usr/bin/vim -p"), None);
        assert_eq!(window.program, "vim");
        // The first frame says the grid's size: as many cells as fit.
        frame(&ctx, &mut window, vec![]);
        let cell = window.cell;
        assert!(cell.x > 5.0 && cell.y > cell.x, "{cell:?}");
        let (cols, rows) = (((800.0 - 2.0 * PAD) / cell.x) as u16, ((600.0 - 2.0 * PAD) / cell.y) as u16);
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [Input::Size(cols, rows)]);
        // The editor's screen, painted: its text in its colours, on its
        // cells; what is behind it, filled.
        let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
        buf.set_string(2, 0, "hello", Style::new().fg(Color::Green));
        buf.set_string(0, 1, "picked", Style::new().bg(Color::Blue));
        outputs.send(Output::Screen(buf.clone(), None, vec![], vec![])).unwrap();
        let out = frame(&ctx, &mut window, vec![]);
        let painted = texts(&out);
        let at = |x: u16, y: u16| pos2(PAD + x as f32 * cell.x, PAD + y as f32 * cell.y);
        assert!(painted.iter().any(|(t, c, pos)| t == "hello" && *c == PALETTE[2] && pos.distance(at(2, 0)) < 1.0), "{painted:?}");
        assert!(painted.iter().any(|(t, c, _)| t == "picked" && *c == FG), "{painted:?}");
        let filled = out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Mesh(m) => m.vertices.iter().any(|v| v.color == PALETTE[4]),
            _ => false,
        });
        assert!(filled, "the picked row, on blue");
        // What is done in the window: sent the editor as it would be in a
        // terminal.
        let press = egui::Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let click = egui::Event::PointerButton {
            pos: at(3, 1) + vec2(1.0, 1.0),
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, &mut window, vec![press.clone(), egui::Event::Text("x".into()), click]);
        assert_eq!(
            inputs.try_iter().collect::<Vec<_>>(),
            [plain(KeyCode::Down), plain(KeyCode::Char('x')), Input::Click { x: 3, y: 1, right: true }]
        );
        // While text is written, the cursor is painted, and a paste types.
        outputs.send(Output::Screen(buf.clone(), Some((4, 1)), vec![], vec![])).unwrap();
        let out = frame(&ctx, &mut window, vec![egui::Event::Paste("ok".into())]);
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [plain(KeyCode::Char('o')), plain(KeyCode::Char('k'))]);
        assert!(out.platform_output.ime.is_some(), "an input method composes where text is written");
        // A face the fonts lack is made: bold drawn twice, a pixel apart.
        let mut bold = buf.clone();
        bold.set_string(0, 2, "strong", Style::new().add_modifier(Modifier::BOLD));
        outputs.send(Output::Screen(bold, None, vec![], vec![])).unwrap();
        window.real = [true, false, false, false];
        let out = frame(&ctx, &mut window, vec![]);
        let strong: Vec<Pos2> = texts(&out).into_iter().filter(|(t, ..)| t == "strong").map(|(.., pos)| pos).collect();
        assert_eq!(strong.len(), 2, "{strong:?}");
        assert!((strong[0].x - strong[1].x).abs() == 1.0 && strong[0].y == strong[1].y, "{strong:?}");
        // $EDITOR runs: nothing is sent the editor until its next screen.
        outputs.send(Output::Editing).unwrap();
        let out = frame(&ctx, &mut window, vec![press.clone()]);
        assert_eq!(inputs.try_iter().count(), 0);
        assert!(texts(&out).iter().any(|(t, ..)| t.contains("StackED goes on once vim exits")), "it says what it waits for");
        outputs.send(Output::Screen(buf, None, vec![], vec![])).unwrap();
        frame(&ctx, &mut window, vec![press]);
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [plain(KeyCode::Down)]);
        // The window grown: the grid's new size sent.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| window.frame(ctx));
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [Input::Size(((1000.0 - 2.0 * PAD) / cell.x) as u16, rows)]);
        // The editor quit: the window closes.
        outputs.send(Output::Bye).unwrap();
        let out = frame(&ctx, &mut window, vec![]);
        let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::Close), "{commands:?}");
    }

    #[test]
    fn text_dragged_over_in_the_window_is_lit_and_copied() {
        let (session, inputs, outputs) = stand_in();
        let ctx = egui::Context::default();
        let (fonts, real) = fonts();
        ctx.set_fonts(fonts);
        let mut window = Window::with(session, real, program("vim"), None);
        frame(&ctx, &mut window, vec![]);
        let cell = window.cell;
        let Some(Input::Size(cols, rows)) = inputs.try_iter().next() else { panic!("no size") };
        // A pane of patches, and one of code beside it.
        let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
        buf.set_string(0, 1, "patch one", Style::new());
        buf.set_string(10, 1, "let x = 1;", Style::new());
        buf.set_string(0, 2, "patch two", Style::new());
        buf.set_string(10, 2, "let y = 2;", Style::new());
        let regions = vec![Rect::new(0, 1, 9, rows - 2), Rect::new(10, 1, cols - 10, rows - 2)];
        outputs.send(Output::Screen(buf, None, regions, vec![])).unwrap();
        frame(&ctx, &mut window, vec![]);
        let at = |x: u16, y: u16| pos2(PAD + (x as f32 + 0.5) * cell.x, PAD + (y as f32 + 0.5) * cell.y);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        // Dragged from the x of the first line to the 2 of the second:
        // nothing sent the editor, and what it went over lit.
        let out = frame(&ctx, &mut window, vec![button(at(14, 1), true), egui::Event::PointerMoved(at(18, 2)), button(at(18, 2), false)]);
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), []);
        let lit = out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == SELECTED));
        assert!(lit, "what is selected, lit");
        // ⌘C: copied, the code only.
        let out = frame(&ctx, &mut window, vec![egui::Event::Copy]);
        let copied = &out.platform_output.commands;
        assert!(
            copied.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "x = 1;\nlet y = 2")),
            "{copied:?}"
        );
    }

    #[test]
    fn the_window_follows_the_mouse_only_while_it_is_the_active_one() {
        let (session, inputs, outputs) = stand_in();
        let ctx = egui::Context::default();
        let (fonts, real) = fonts();
        ctx.set_fonts(fonts);
        let mut window = Window::with(session, real, program("vim"), None);
        frame(&ctx, &mut window, vec![]);
        let cell = window.cell;
        let Some(Input::Size(cols, rows)) = inputs.try_iter().next() else { panic!("no size") };
        let on = |x: u16, y: u16| pos2(PAD + (x as f32 + 0.5) * cell.x, PAD + (y as f32 + 0.5) * cell.y);
        let moved = |x, y| egui::Event::PointerMoved(on(x, y));
        let pass = |window: &mut Window, focused: bool, events: Vec<egui::Event>| {
            frame_focused(&ctx, window, focused, events);
            inputs.try_iter().collect::<Vec<_>>()
        };
        assert_eq!(pass(&mut window, true, vec![moved(2, 1)]), [Input::Hover(2, 1)]);
        // Another window made the active one: the hover gone at once, and the
        // mouse not followed over this one, where a click is sent all the
        // same.
        assert_eq!(pass(&mut window, false, vec![egui::Event::WindowFocused(false), moved(3, 1)]), [Input::Unhover]);
        let right = egui::Event::PointerButton {
            pos: on(4, 1),
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(pass(&mut window, false, vec![moved(4, 1), right]), [Input::Click { x: 4, y: 1, right: true }]);
        // This one the active window again: the mouse said over the cell
        // where it was last seen, at once.
        assert_eq!(pass(&mut window, true, vec![egui::Event::WindowFocused(true)]), [Input::Hover(4, 1)]);
        // `$EDITOR` runs, and its terminal becomes the active window: nothing
        // is sent meanwhile, and the hover goes once the editor's next screen
        // comes.
        outputs.send(Output::Editing).unwrap();
        assert_eq!(pass(&mut window, false, vec![egui::Event::WindowFocused(false)]), []);
        outputs.send(Output::Screen(Buffer::empty(Rect::new(0, 0, cols, rows)), None, vec![], vec![])).unwrap();
        assert_eq!(pass(&mut window, false, vec![]), [Input::Unhover]);
        // Where egui has no viewport's info to say it, the input says whether
        // the window is the active one: it is, as in the frames of the other
        // tests, then it is not.
        frame(&ctx, &mut window, vec![moved(5, 2)]);
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [Input::Hover(5, 2)]);
        let area = Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        let input = egui::RawInput {
            screen_rect: area,
            events: vec![moved(6, 2)],
            focused: false,
            ..Default::default()
        };
        assert_eq!(input.viewports[&egui::ViewportId::ROOT].focused, None);
        let _ = ctx.run(input, |ctx| window.frame(ctx));
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [Input::Unhover]);
        // Where the two say otherwise, the viewport's info, which eframe asks
        // of the window at each pass, is taken over the input.
        frame(&ctx, &mut window, vec![moved(6, 2)]);
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [Input::Hover(6, 2)]);
        let mut input = egui::RawInput {
            screen_rect: area,
            focused: true,
            ..Default::default()
        };
        let info = egui::ViewportInfo {
            focused: Some(false),
            ..Default::default()
        };
        input.viewports.insert(egui::ViewportId::ROOT, info);
        let _ = ctx.run(input, |ctx| window.frame(ctx));
        assert_eq!(inputs.try_iter().collect::<Vec<_>>(), [Input::Unhover]);
    }

    #[test]
    fn the_grid_is_the_system_s_monospace_font_else_hack_and_has_every_glyph_stacked_draws() {
        let (fonts, real) = fonts();
        let (found, symbols) = found();
        let chain = &fonts.families[&Face::Regular.family()];
        // The first of the families the system has, its four faces, then
        // egui's Hack; else Hack first, its bold and italics made.
        match &found[Face::Regular as usize] {
            Some(f) => {
                assert!(GRID.contains(&f.family.as_str()), "{}", f.family);
                assert_eq!(chain[..2], [format!("{} Regular", f.family), "Hack".to_string()]);
                assert_eq!(real, found.each_ref().map(|f| f.is_some()));
            }
            None => assert_eq!((chain[0].as_str(), real), ("Hack", [true, false, false, false])),
        }
        // Where macOS keeps Menlo, the grid is Menlo, its own bold and
        // italics, then Apple Symbols, as it always was; where Linux keeps
        // DejaVu Sans Mono, the grid is that, DejaVu Sans after it.
        let first = found[Face::Regular as usize].as_ref().map(|f| f.family.as_str());
        let symbols = symbols.as_ref().map(|f| f.family.as_str());
        if Path::new("/System/Library/Fonts/Menlo.ttc").exists() {
            assert_eq!((first, real, symbols), (Some("Menlo"), [true; 4], Some("Apple Symbols")));
        }
        if Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf").exists() {
            assert_eq!(first, Some("DejaVu Sans Mono"));
        }
        // With a font for symbols, each face has every character stacked
        // draws.
        if symbols.is_none() {
            return;
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let drawn = "·…─↑↓→←│−↕●⇢✓○–▶↗⇗↪•◇◌└┼⋯✗┘⟳≡◆✉▲▼⇣✎▏┌┐▎▸‹›█";
        for face in Face::ALL {
            let font = FontId::new(FONT_SIZE, face.family());
            let missing: String = drawn.chars().filter(|c| !ctx.fonts_mut(|f| f.has_glyph(&font, *c))).collect();
            assert_eq!(missing, "", "{face:?}, with {symbols:?}");
        }
    }
}
