//! The editor in `stacked web`'s page: a terminal editor run in a
//! pseudo-terminal of stacked's own (`Pty`), in a session of its own whose
//! controlling terminal that is, what it writes there read on a thread of
//! its own; its screen as a terminal shows what it writes (`Screen`,
//! vt100's), which `stacked web` sends the pages as it sends its own
//! (`Screen::buffer`, then `web::rows`), what the editor asks of its
//! terminal answered as xterm answers it (`Answers`); and what the pages
//! send made the bytes xterm sends for it: keys (`key`), pastes (`paste`)
//! and the mouse (`Screen::mouse`). The same code on Linux and on macOS,
//! whose libc differ: each call's arguments cast to what each system takes
//! (`openpty`'s pointers, `ioctl`'s request), no name of a pseudo-terminal
//! asked for, as macOS has no `ptsname_r`, and select rather than poll,
//! which macOS does not answer for a device.

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::num::NonZeroU16;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// The terminal the editor is told it runs in: xterm's, of 256 colours,
/// which its screen draws, and RGB too (`Screen::buffer`).
const TERM: &str = "xterm-256color";

/// How long the thread reading the editor's terminal waits for what it
/// writes, before it looks again whether it is to stop (`Pty::close`).
const READING: Duration = Duration::from_millis(20);

/// How long an editor hung up on has to exit, it and what it started in
/// its terminal, before what is left of them is killed (`Pty::hang_up`).
pub const HANGING_UP: Duration = Duration::from_secs(2);

/// A terminal editor running in a pseudo-terminal of stacked's own: the
/// master side of that terminal, which stacked writes the editor's input
/// to and reads what it writes from, until it is closed; the editor's
/// process; what it was written that its terminal did not take yet; the
/// thread that reads it, and what tells that thread to stop. Dropped
/// before the editor exited, the editor is hung up on, so that it is never
/// left running.
pub struct Pty {
    master: Option<Arc<File>>,
    child: Child,
    pending: Vec<u8>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    exited: bool,
}

impl Pty {
    /// `cmd` started in a new pseudo-terminal of `cols` by `rows`: its
    /// standard input, output and error that terminal, in a session of its
    /// own whose controlling terminal it is, told `TERM`, and neither
    /// `COLUMNS` nor `LINES`, which would say another size than its
    /// terminal's. What it writes is given to `out` as it comes, from a
    /// thread of its own, then None once there is no more: all that held
    /// its terminal gone, or the terminal closed. An error if the terminal
    /// cannot be made, said so, or if `cmd` cannot start, as
    /// `Command::spawn` says it.
    pub fn start(cmd: &mut Command, cols: u16, rows: u16, out: impl Fn(Option<Vec<u8>>) + Send + 'static) -> std::io::Result<Pty> {
        let (master, slave) = open(cols, rows)
            .map_err(|e| std::io::Error::new(e.kind(), format!("no terminal could be made for it in the page ({e}); nothing changed")))?;
        cmd.stdin(Stdio::from(slave.try_clone()?)).stdout(Stdio::from(slave.try_clone()?)).stderr(Stdio::from(slave));
        cmd.env("TERM", TERM).env_remove("COLUMNS").env_remove("LINES");
        // SAFETY: between the fork and the exec, setsid and ioctl alone,
        // which a child may call there.
        unsafe {
            cmd.pre_exec(|| {
                // A session of its own, whose controlling terminal is its
                // standard input's, the pseudo-terminal, set up already.
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let started = cmd.spawn();
        // The terminal's side held here, closed: the editor holds the only
        // ones, so that once it and what it started are gone, no more comes.
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = started?;
        let master = Arc::new(File::from(master));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (master, stop) = (master.clone(), stop.clone());
            std::thread::spawn(move || read(&master, &stop, out))
        };
        Ok(Pty { master: Some(master), child, pending: Vec::new(), stop, reader: Some(reader), exited: false })
    }

    /// `bytes` for the editor, written to its terminal after what it did
    /// not take yet: as much as it takes now, the rest kept for `flush`.
    pub fn write(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        self.flush();
    }

    /// What the editor's terminal did not take yet, written as far as it
    /// takes it now, never waited for, as an editor may read nothing for a
    /// while; dropped once the terminal takes nothing more, the editor gone,
    /// or the terminal closed.
    pub fn flush(&mut self) {
        let Some(master) = &self.master else {
            self.pending.clear();
            return;
        };
        let mut terminal: &File = master;
        while !self.pending.is_empty() {
            match terminal.write(&self.pending) {
                Ok(n) if n > 0 => {
                    self.pending.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                _ => self.pending.clear(),
            }
        }
    }

    /// Whether some of what the editor was written waits for its terminal
    /// to take it.
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The editor's terminal made `cols` by `rows`, which the system tells
    /// the editor (SIGWINCH).
    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        let Some(master) = &self.master else { return Ok(()) };
        let size = winsize(cols, rows);
        // SAFETY: TIOCSWINSZ reads the size it is given; the request is cast
        // to what this system's ioctl takes.
        if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size as *const libc::winsize) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// How the editor exited, once it did, its terminal then closed; None
    /// while it runs.
    pub fn exited(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.exited = true;
            self.close();
        }
        Ok(status)
    }

    /// The terminal closed: the thread reading it stopped, then its master
    /// side closed, which hangs up what still holds its other side.
    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.master = None;
        self.pending.clear();
    }

    /// The editor hung up on, as its terminal goes: SIGHUP, then SIGCONT for
    /// one stopped, sent to its process group and to the one its terminal
    /// has in the foreground, the terminal then closed, which the system
    /// says to it too; then waited for, it and what is left of those
    /// groups, `HANGING_UP` at most, and whatever is left of them killed.
    fn hang_up(&mut self) {
        let pid = self.child.id() as libc::pid_t;
        // SAFETY: tcgetpgrp only reads the terminal's foreground group.
        let ahead = self.master.as_ref().map_or(-1, |m| unsafe { libc::tcgetpgrp(m.as_raw_fd()) });
        let mut groups = vec![pid];
        if ahead > 0 && ahead != pid {
            groups.push(ahead);
        }
        signal(&groups, libc::SIGHUP);
        signal(&groups, libc::SIGCONT);
        self.close();
        let started = Instant::now();
        loop {
            if !self.exited && matches!(self.child.try_wait(), Ok(Some(_))) {
                self.exited = true;
            }
            let left: Vec<libc::pid_t> = groups.iter().copied().filter(|g| there(*g)).collect();
            if (self.exited && left.is_empty()) || started.elapsed() >= HANGING_UP {
                signal(&left, libc::SIGKILL);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !self.exited {
            let _ = self.child.wait();
            self.exited = true;
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        if self.exited {
            self.close();
        } else {
            self.hang_up();
        }
    }
}

/// `sig` sent to each of `groups`, process groups by their number.
fn signal(groups: &[libc::pid_t], sig: libc::c_int) {
    for group in groups {
        // SAFETY: kill only sends a signal, to the group alone, its number
        // below 0.
        unsafe { libc::kill(-group, sig) };
    }
}

/// Whether process group `group` has a process left that stacked may
/// signal.
fn there(group: libc::pid_t) -> bool {
    // SAFETY: signal 0 only asks whether there is one.
    unsafe { libc::kill(-group, 0) == 0 }
}

/// A terminal's size of `cols` by `rows`, as the system has it.
fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

/// A new pseudo-terminal of `cols` by `rows`: its master side, read and
/// written without waiting, below FD_SETSIZE, for select; and its other
/// side, the editor's terminal. Each is closed on exec, so that nothing
/// else stacked starts holds it.
fn open(cols: u16, rows: u16) -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut size = winsize(cols, rows);
    let (mut master, mut slave): (libc::c_int, libc::c_int) = (-1, -1);
    // SAFETY: openpty fills in the two descriptors it is given room for,
    // with no name asked for, its terminal's settings left as they are, and
    // the size given; each pointer cast to what this system's openpty takes,
    // a `*const` on Linux, a `*mut` on macOS.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut::<libc::termios>() as _,
            &mut size as *mut libc::winsize as _,
        )
    };
    if opened != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openpty opened both, and nothing else owns them.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    for fd in [&master, &slave] {
        with_flag(fd, libc::F_GETFD, libc::F_SETFD, libc::FD_CLOEXEC)?;
    }
    with_flag(&master, libc::F_GETFL, libc::F_SETFL, libc::O_NONBLOCK)?;
    if master.as_raw_fd() >= libc::FD_SETSIZE as libc::c_int {
        return Err(std::io::Error::other("too many files are open"));
    }
    Ok((master, slave))
}

/// `flag` added to those of `fd` that `get` reads and `set` writes.
fn with_flag(fd: &OwnedFd, get: libc::c_int, set: libc::c_int, flag: libc::c_int) -> std::io::Result<()> {
    // SAFETY: fcntl reads and writes the flags of a descriptor open here.
    let done = unsafe {
        let was = libc::fcntl(fd.as_raw_fd(), get);
        was >= 0 && libc::fcntl(fd.as_raw_fd(), set, was | flag) >= 0
    };
    if done { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// What the editor writes in its terminal, read from `master` as it comes,
/// each piece given to `out`, until there is no more, the terminal's other
/// side closed by all that held it, or `stop` is set; then None.
fn read(master: &File, stop: &AtomicBool, out: impl Fn(Option<Vec<u8>>)) {
    let mut buf = vec![0u8; 1 << 16];
    let mut terminal = master;
    while !stop.load(Ordering::SeqCst) {
        match readable(master.as_raw_fd(), READING) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => break,
        }
        match terminal.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out(Some(buf[..n].to_vec())),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
            // EIO, as Linux says that the other side is closed by all.
            Err(_) => break,
        }
    }
    out(None);
}

/// Whether `fd` has something to read, or its end, within `wait`: asked of
/// select, which macOS answers for a pseudo-terminal too.
fn readable(fd: RawFd, wait: Duration) -> std::io::Result<bool> {
    // SAFETY: the set is made for `fd`, below FD_SETSIZE (`open`), and
    // select is given it, and how long to wait, each as this system has it.
    let n = unsafe {
        let mut set: libc::fd_set = std::mem::zeroed();
        libc::FD_ZERO(&mut set);
        libc::FD_SET(fd, &mut set);
        let mut time = libc::timeval { tv_sec: wait.as_secs() as _, tv_usec: wait.subsec_micros() as _ };
        libc::select(fd + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), &mut time)
    };
    match n {
        0.. => Ok(n > 0),
        _ => match std::io::Error::last_os_error() {
            e if e.kind() == ErrorKind::Interrupted => Ok(false),
            e => Err(e),
        },
    }
}

/// The editor's screen as a terminal shows what it writes (vt100's
/// parser, with no scrollback: the page shows what is on the screen
/// alone), what it asked of its terminal meanwhile answered (`Answers`);
/// and what the page's mouse did, for the reports the editor asks for
/// (`Screen::mouse`).
pub struct Screen {
    parser: vt100::Parser<Answers>,
    mouse: Mouse,
}

/// A side of a screen, `n` cells, one at least.
fn side(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

impl Screen {
    /// A blank screen of `cols` by `rows`, the cursor at its top left.
    pub fn new(cols: u16, rows: u16) -> Screen {
        Screen {
            parser: vt100::Parser::new_with_callbacks(side(rows), side(cols), 0, Answers::default()),
            mouse: Mouse::default(),
        }
    }

    /// `bytes`, which the editor wrote, shown; what it asked of its terminal
    /// meanwhile answered, the answers given back, to write to it.
    pub fn process(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.process(bytes);
        std::mem::take(&mut self.parser.callbacks_mut().0)
    }

    /// The screen made `cols` by `rows`, as its terminal is.
    pub fn set_size(&mut self, cols: u16, rows: u16) {
        self.parser.set_size(side(rows), side(cols));
    }

    /// Whether the editor asked for the cursor keys of application mode
    /// (DECCKM), which `key` sends as SS3.
    pub fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    /// Whether the editor asked for pastes bracketed (mode 2004).
    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    /// The screen as a buffer, as stacked draws its own, for `web::rows`:
    /// each cell's character, a wide one covering the next cell; its
    /// colours, the page's own, one of 256 or RGB; its bold, dim, italic,
    /// underline and inverse; and the cell of the cursor, where the editor
    /// put it, none while it hides it.
    pub fn buffer(&self) -> (Buffer, Option<(u16, u16)>) {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
        for y in 0..rows {
            for x in 0..cols {
                let Some(cell) = screen.cell(y, x) else { continue };
                // The second half of a wide character, which the first covers.
                if cell.is_wide_continuation() {
                    continue;
                }
                let text = cell.contents();
                buf[(x, y)].set_symbol(if text.is_empty() { " " } else { text }).set_style(style(cell));
            }
        }
        let cursor = (!screen.hide_cursor()).then(|| {
            let (y, x) = screen.cursor_position();
            // Past the last column, as after a row written whole: on it.
            let (x, y) = (x.min(cols - 1), y.min(rows - 1));
            // On the second half of a wide character: on the character.
            match screen.cell(y, x) {
                Some(c) if c.is_wide_continuation() && x > 0 => (x - 1, y),
                _ => (x, y),
            }
        });
        (buf, cursor)
    }

    /// What a page's line about the mouse is to the editor, `m X Y l|r`
    /// (pressed over a cell, the left button or the right), `u X Y` or `u`
    /// (the left let go, over a cell or none), `d X Y` (moved, the left
    /// down), `v X Y` (moved), `w X Y N` (the wheel turned N lines, down,
    /// up below 0), as web.rs reads them: as xterm reports it, once the
    /// editor asked for the mouse (`:set mouse=a` in vim), and only what it
    /// asked for, presses alone, releases too, the mouse moved with a button
    /// down, or anywhere; in the encoding it asked for, SGR's (`report`).
    /// Else nothing; nor for a cell off its screen, nor for the wheel turned
    /// over none. A press of the right button is let go at once, as the page
    /// does not say when it is; the wheel turned a line for each notch, a
    /// screen's lines at most.
    pub fn mouse(&mut self, parts: &[&str]) -> Vec<u8> {
        use vt100::MouseProtocolMode as Mode;
        let screen = self.parser.screen();
        let (mode, encoding) = (screen.mouse_protocol_mode(), screen.mouse_protocol_encoding());
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let cell = |x: &str, y: &str| -> Option<(u16, u16)> {
            let (x, y): (u16, u16) = (x.parse().ok()?, y.parse().ok()?);
            (x < cols && y < rows).then_some((x, y))
        };
        let mouse = &mut self.mouse;
        // Each a button's code, its cell, and whether it is pressed.
        let mut said: Vec<(u8, (u16, u16), bool)> = Vec::new();
        match parts {
            ["m", x, y, button] => {
                if let Some(at) = cell(x, y) {
                    mouse.at = at;
                    if *button == "r" {
                        said.extend([(2, at, true), (2, at, false)]);
                    } else {
                        mouse.down = true;
                        said.push((0, at, true));
                    }
                }
            }
            ["u", rest @ ..] => {
                let at = match rest {
                    [x, y] => cell(x, y),
                    _ => None,
                };
                let at = at.unwrap_or(mouse.at);
                if std::mem::take(&mut mouse.down) {
                    said.push((0, at, false));
                }
            }
            ["d", x, y] => {
                if let Some(at) = cell(x, y) {
                    mouse.at = at;
                    if matches!(mode, Mode::ButtonMotion | Mode::AnyMotion) {
                        said.push((32, at, true));
                    }
                }
            }
            ["v", x, y] => {
                if let Some(at) = cell(x, y) {
                    mouse.at = at;
                    if mode == Mode::AnyMotion {
                        said.push((35, at, true));
                    }
                }
            }
            ["w", x, y, n] => {
                if let (Some(at), Ok(n)) = (cell(x, y), n.parse::<i32>()) {
                    mouse.at = at;
                    let button = if n > 0 { 65 } else { 64 };
                    for _ in 0..n.unsigned_abs().min(rows.into()) {
                        said.push((button, at, true));
                    }
                }
            }
            _ => {}
        }
        if mode == Mode::None {
            return Vec::new();
        }
        said.into_iter()
            .filter(|(_, _, pressed)| *pressed || mode != Mode::Press)
            .flat_map(|(button, at, pressed)| report(encoding, button, at, pressed))
            .collect()
    }
}

/// A cell's colours and attributes, as ratatui has them.
fn style(cell: &vt100::Cell) -> Style {
    let mut mods = Modifier::empty();
    for (on, m) in [
        (cell.bold(), Modifier::BOLD),
        (cell.dim(), Modifier::DIM),
        (cell.italic(), Modifier::ITALIC),
        (cell.underline(), Modifier::UNDERLINED),
        (cell.inverse(), Modifier::REVERSED),
    ] {
        if on {
            mods |= m;
        }
    }
    Style::new().fg(colour(cell.fgcolor())).bg(colour(cell.bgcolor())).add_modifier(mods)
}

/// A colour of the terminal's, as ratatui has it: the page's own, one of
/// 256 (the first 16 the page's), or RGB.
fn colour(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// What the page's mouse did that the editor's reports of it need: the
/// cell where it was last, and whether the left button is down.
#[derive(Default)]
struct Mouse {
    at: (u16, u16),
    down: bool,
}

/// A report of the mouse, as xterm writes it in `encoding`, for `button`
/// (0 the left, 2 the right, 64 and 65 the wheel up and down, 32 more for
/// the mouse moved, 35 moved with no button down) at the cell `at`, pressed
/// or let go: SGR's, `ESC [ < B ; X ; Y M`, `m` once let go, the cells from
/// 1; else xterm's first, `ESC [ M`, then the button, the column and the
/// row, each 32 more, a byte (the cells from 1, a release the button 3),
/// none past what a byte holds; or each a character of UTF-8, for mode
/// 1005's, none past 2047.
fn report(encoding: vt100::MouseProtocolEncoding, button: u8, (x, y): (u16, u16), pressed: bool) -> Vec<u8> {
    use vt100::MouseProtocolEncoding as Encoding;
    if encoding == Encoding::Sgr {
        return format!("\x1b[<{button};{};{}{}", x + 1, y + 1, if pressed { 'M' } else { 'm' }).into_bytes();
    }
    let code = u32::from(if pressed { button } else { 3 }) + 32;
    let mut out = b"\x1b[M".to_vec();
    for v in [code, u32::from(x) + 33, u32::from(y) + 33] {
        match encoding {
            Encoding::Utf8 => match char::from_u32(v).filter(|_| v < 2048) {
                Some(c) => out.extend_from_slice(c.to_string().as_bytes()),
                None => return Vec::new(),
            },
            _ => match u8::try_from(v) {
                Ok(b) => out.push(b),
                Err(_) => return Vec::new(),
            },
        }
    }
    out
}

/// The page's own colours, as web/index.html sets them (`--fg`, `--bg`), as
/// xterm says a colour it is asked for (OSC 10 and 11).
const FG: &str = "rgb:cccc/cccc/cccc";
const BG: &str = "rgb:1e1e/1e1e/1e1e";

/// What the editor asked of its terminal, answered as xterm answers it, to
/// write back to it: what terminal it is (DA, as libvterm answers it, a
/// VT100 with advanced video; and, asked with `>`, as libvterm answers that
/// too, version 100, which vim takes for libvterm's, a terminal that
/// reports the mouse as SGR does); whether it is well (DSR 5); where its
/// cursor is (DSR 6, and DEC's, with `?`); and its colours, the page's own
/// (OSC 10 and 11, asked with `?`), by which an editor tells a dark page
/// from a light one. Nothing else is answered: an editor does without an
/// answer it does not get.
#[derive(Default)]
struct Answers(Vec<u8>);

impl vt100::Callbacks for Answers {
    fn unhandled_csi(&mut self, screen: &mut vt100::Screen, i1: Option<u8>, _: Option<u8>, params: &[&[u16]], c: char) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let (rows, cols) = screen.size();
        let (y, x) = screen.cursor_position();
        let at = format!("{};{}", y.min(rows.get() - 1) + 1, x.min(cols.get() - 1) + 1);
        let answer = match (i1, c, first) {
            (None, 'c', 0) => "\x1b[?1;2c".to_string(),
            (Some(b'>'), 'c', 0) => "\x1b[>0;100;0c".to_string(),
            (None, 'n', 5) => "\x1b[0n".to_string(),
            (None, 'n', 6) => format!("\x1b[{at}R"),
            (Some(b'?'), 'n', 6) => format!("\x1b[?{at}R"),
            _ => return,
        };
        self.0.extend_from_slice(answer.as_bytes());
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let (which, colour) = match params {
            [b"10", b"?"] => ("10", FG),
            [b"11", b"?"] => ("11", BG),
            _ => return,
        };
        self.0.extend_from_slice(format!("\x1b]{which};{colour}\x1b\\").as_bytes());
    }
}

/// The bytes xterm sends for the key a page's line names after its `k`,
/// `NAME MODS` or `c HEX MODS` (a character, by its code), MODS `c` ctrl,
/// `a` alt, `s` shift, or `-`, as web.rs reads them. The arrows, Home and
/// End as CSI (`ESC [ A`), or as SS3 (`ESC O A`) in the cursor keys'
/// application mode, as the editor asks for it (`application`); Insert,
/// Delete, Page Up and Down, F5 to F12 as CSI with their number (`ESC [ 2
/// ~`), F1 to F4 as SS3 (`ESC O P`); with a modifier, each as CSI with
/// xterm's parameter for it (`ESC [ 1 ; 5 A` ctrl-↑, `ESC [ 15 ; 2 ~`
/// shift-F5), whatever the mode. A character as UTF-8: with ctrl, its
/// control byte (`control`), else as it is; Enter a carriage return, Tab a
/// tab, Shift-Tab `ESC [ Z`, Backspace DEL (BS with ctrl), Esc ESC; each
/// of those with alt after ESC. None for a line that names no key.
pub fn key(parts: &[&str], application: bool) -> Option<Vec<u8>> {
    let (name, mods, typed) = match parts {
        ["c", hex, mods] => ("c", *mods, Some(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?)),
        [name, mods] => (*name, *mods, None),
        _ => return None,
    };
    let (ctrl, alt, shift) = (mods.contains('c'), mods.contains('a'), mods.contains('s'));
    // xterm's parameter for the modifiers: 1 and each one's bit.
    let m = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let alt_first = |bytes: &[u8]| if alt { [b"\x1b".as_slice(), bytes].concat() } else { bytes.to_vec() };
    // A key of the cursor's, or one of F1 to F4, by its letter.
    let lettered = |letter: char, ss3: bool| match (m, ss3) {
        (1, true) => format!("\x1bO{letter}").into_bytes(),
        (1, false) => format!("\x1b[{letter}").into_bytes(),
        _ => format!("\x1b[1;{m}{letter}").into_bytes(),
    };
    // One of the keypad's others, or one of F5 to F12, by its number.
    let numbered = |n: u8| match m {
        1 => format!("\x1b[{n}~").into_bytes(),
        _ => format!("\x1b[{n};{m}~").into_bytes(),
    };
    Some(match name {
        "c" => {
            let c = typed?;
            match control(c).filter(|_| ctrl) {
                Some(b) => alt_first(&[b]),
                None => alt_first(c.to_string().as_bytes()),
            }
        }
        "Up" => lettered('A', application),
        "Down" => lettered('B', application),
        "Right" => lettered('C', application),
        "Left" => lettered('D', application),
        "Home" => lettered('H', application),
        "End" => lettered('F', application),
        "Insert" => numbered(2),
        "Delete" => numbered(3),
        "PageUp" => numbered(5),
        "PageDown" => numbered(6),
        "F1" => lettered('P', true),
        "F2" => lettered('Q', true),
        "F3" => lettered('R', true),
        "F4" => lettered('S', true),
        "F5" => numbered(15),
        "F6" => numbered(17),
        "F7" => numbered(18),
        "F8" => numbered(19),
        "F9" => numbered(20),
        "F10" => numbered(21),
        "F11" => numbered(23),
        "F12" => numbered(24),
        "Enter" => alt_first(b"\r"),
        "Tab" => alt_first(b"\t"),
        "BackTab" => alt_first(b"\x1b[Z"),
        "Backspace" => alt_first(if ctrl { b"\x08" } else { b"\x7f" }),
        "Esc" => alt_first(b"\x1b"),
        _ => return None,
    })
}

/// The control byte ctrl makes of `c`, as xterm has it: a letter's (ctrl-a
/// 1, ctrl-z 26); those of `@`, space and 2 (0), `[` and 3 (27, ESC), `\`
/// and 4, `]` and 5, `^` and 6, `_`, `/` and 7 (28 to 31), and `?` and 8
/// (127, DEL). None for another, which ctrl leaves as it is.
fn control(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' | 'A'..='Z' => c as u8 & 0x1f,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '/' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// Text pasted in the page, as the editor is given it: each of its new
/// lines a carriage return, as xterm pastes them, a tab as it is, any other
/// control character dropped, ESC among them, which could end a bracketed
/// paste early, or be keys; bracketed (`ESC [ 200 ~`, then `ESC [ 201 ~`)
/// when the editor asked for it.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\n");
    let mut out = String::with_capacity(text.len() + 12);
    if bracketed {
        out.push_str("\x1b[200~");
    }
    for c in text.chars() {
        match c {
            '\n' | '\r' => out.push('\r'),
            '\t' => out.push('\t'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    if bracketed {
        out.push_str("\x1b[201~");
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;

    /// The bytes for the key `line` names, as a page writes it after `k`.
    fn keyed(line: &str, application: bool) -> Option<Vec<u8>> {
        key(&line.split_whitespace().collect::<Vec<_>>(), application)
    }

    #[test]
    fn keys_are_the_bytes_xterm_sends_as_the_editor_asks_for_the_cursor_keys() {
        let k = |line: &str| keyed(line, false).unwrap();
        let app = |line: &str| keyed(line, true).unwrap();
        // Characters as UTF-8; Enter, Tab, Shift-Tab, Backspace and Esc.
        assert_eq!(k("c 61 -"), b"a");
        assert_eq!(k("c 41 -"), b"A", "shifted already");
        assert_eq!(k("c e9 -"), "é".as_bytes());
        assert_eq!(k("c 65e5 -"), "日".as_bytes());
        assert_eq!(k("Enter -"), b"\r");
        assert_eq!(k("Tab -"), b"\t");
        assert_eq!(k("BackTab -"), b"\x1b[Z");
        assert_eq!(k("Backspace -"), b"\x7f");
        assert_eq!(k("Backspace c"), b"\x08");
        assert_eq!(k("Esc -"), b"\x1b");
        // Ctrl with a letter: its control byte; with those xterm gives one.
        assert_eq!(k("c 61 c"), [1]);
        assert_eq!(k("c 5a c"), [26]);
        assert_eq!(k("c 63 c"), [3], "ctrl-c");
        assert_eq!(k("c 20 c"), [0]);
        assert_eq!(k("c 5b c"), [0x1b]);
        assert_eq!(k("c 5d c"), [0x1d]);
        assert_eq!(k("c 3f c"), [0x7f]);
        assert_eq!(k("c 31 c"), b"1", "none for 1: as it is");
        // Alt: ESC first.
        assert_eq!(k("c 66 a"), b"\x1bf");
        assert_eq!(k("c 62 ca"), b"\x1b\x02");
        assert_eq!(k("Enter a"), b"\x1b\r");
        assert_eq!(k("Backspace a"), b"\x1b\x7f");
        // The arrows, Home and End: CSI, SS3 in application mode; with a
        // modifier, CSI and its parameter whatever the mode.
        for (name, letter) in [("Up", 'A'), ("Down", 'B'), ("Right", 'C'), ("Left", 'D'), ("Home", 'H'), ("End", 'F')] {
            assert_eq!(k(&format!("{name} -")), format!("\x1b[{letter}").as_bytes(), "{name}");
            assert_eq!(app(&format!("{name} -")), format!("\x1bO{letter}").as_bytes(), "{name}");
            for (mods, m) in [("s", 2), ("a", 3), ("as", 4), ("c", 5), ("cs", 6), ("ca", 7), ("cas", 8)] {
                let said = format!("\x1b[1;{m}{letter}");
                assert_eq!(k(&format!("{name} {mods}")), said.as_bytes(), "{name} {mods}");
                assert_eq!(app(&format!("{name} {mods}")), said.as_bytes(), "{name} {mods}");
            }
        }
        // Insert, Delete, Page Up and Down: CSI with their number, then the
        // modifiers' parameter.
        for (name, n) in [("Insert", 2), ("Delete", 3), ("PageUp", 5), ("PageDown", 6)] {
            assert_eq!(k(&format!("{name} -")), format!("\x1b[{n}~").as_bytes());
            assert_eq!(app(&format!("{name} -")), format!("\x1b[{n}~").as_bytes());
            assert_eq!(k(&format!("{name} c")), format!("\x1b[{n};5~").as_bytes());
        }
        // F1 to F4: SS3, then CSI with a modifier; F5 to F12: CSI with their
        // numbers, which skip 16 and 22.
        for (name, letter) in [("F1", 'P'), ("F2", 'Q'), ("F3", 'R'), ("F4", 'S')] {
            assert_eq!(k(&format!("{name} -")), format!("\x1bO{letter}").as_bytes());
            assert_eq!(k(&format!("{name} s")), format!("\x1b[1;2{letter}").as_bytes());
        }
        for (name, n) in [("F5", 15), ("F6", 17), ("F7", 18), ("F8", 19), ("F9", 20), ("F10", 21), ("F11", 23), ("F12", 24)] {
            assert_eq!(k(&format!("{name} -")), format!("\x1b[{n}~").as_bytes());
            assert_eq!(k(&format!("{name} s")), format!("\x1b[{n};2~").as_bytes());
        }
        // Not a key: none.
        assert_eq!(keyed("F13 -", false), None);
        assert_eq!(keyed("c zz -", false), None);
        assert_eq!(keyed("Enter", false), None);
    }

    #[test]
    fn a_paste_is_its_text_its_new_lines_enter_bracketed_when_the_editor_asked() {
        let text = "one\r\ntwo\nthree\tfour\u{1b}[201~five";
        // Its new lines Enter's carriage return, a tab as it is, Esc dropped,
        // so that the text cannot end the bracketed paste early.
        assert_eq!(paste(text, false), b"one\rtwo\rthree\tfour[201~five");
        assert_eq!(paste(text, true), b"\x1b[200~one\rtwo\rthree\tfour[201~five\x1b[201~");
        assert_eq!(paste("日本", true), "\x1b[200~日本\x1b[201~".as_bytes());
        assert_eq!(paste("", false), b"");
    }

    #[test]
    fn the_screen_is_a_buffer_its_colours_attributes_wide_characters_and_cursor_as_the_editor_wrote_them() {
        let mut s = Screen::new(10, 3);
        // Red, then 256's 196 on RGB, bold and underlined; then a wide
        // character, inverse and italic, dim; the cursor put on the second
        // row.
        let answers = s.process(b"\x1b[31mr\x1b[0m\x1b[38;5;196;48;2;16;32;48;1;4mx\x1b[0m\x1b[7;3m\xe6\x97\xa5\x1b[0m\x1b[2mz\x1b[0m\x1b[2;4H");
        assert!(answers.is_empty());
        let (buf, cursor) = s.buffer();
        assert_eq!(buf.area, Rect::new(0, 0, 10, 3));
        let cell = |x: u16, y: u16| &buf[(x, y)];
        assert_eq!((cell(0, 0).symbol(), cell(0, 0).fg, cell(0, 0).bg), ("r", Color::Indexed(1), Color::Reset));
        assert!(cell(0, 0).modifier.is_empty());
        assert_eq!((cell(1, 0).symbol(), cell(1, 0).fg, cell(1, 0).bg), ("x", Color::Indexed(196), Color::Rgb(16, 32, 48)));
        assert_eq!(cell(1, 0).modifier, Modifier::BOLD | Modifier::UNDERLINED);
        assert_eq!(cell(2, 0).symbol(), "日");
        assert_eq!(cell(2, 0).modifier, Modifier::REVERSED | Modifier::ITALIC);
        assert_eq!((cell(4, 0).symbol(), cell(4, 0).modifier), ("z", Modifier::DIM), "after the wide character's two cells");
        assert_eq!(cursor, Some((3, 1)));
        // As stacked web sends a row: the wide character covers its next
        // cell, the cursor marked on its own.
        let rows = crate::web::rows(&buf, cursor);
        assert_eq!(rows[0], "r 0 1,,0\u{1f}r\u{1e}#ff0000,#102030,9\u{1f}x\u{1e},,68\u{1f}日\u{1e},,2\u{1f}z\u{1e},,0\u{1f}     ");
        assert_eq!(rows[1], "r 1 ,,0\u{1f}   \u{1e},,65536\u{1f} \u{1e},,0\u{1f}      ");
        // The cursor hidden: none; shown again, back.
        s.process(b"\x1b[?25l");
        assert_eq!(s.buffer().1, None);
        s.process(b"\x1b[?25h\x1b[3;10H");
        assert_eq!(s.buffer().1, Some((9, 2)));
        // On the second half of a wide character: on the character.
        s.process(b"\x1b[1;4H");
        assert_eq!(s.buffer().1, Some((2, 0)));
        // Made larger, as its terminal is: blank where it grew.
        s.set_size(12, 4);
        let (buf, _) = s.buffer();
        assert_eq!((buf.area.width, buf.area.height, buf[(11, 3)].symbol()), (12, 4, " "));
        assert_eq!(buf[(0, 0)].symbol(), "r");
    }

    #[test]
    fn what_the_editor_asks_of_its_terminal_is_answered_as_xterm_answers_it_and_its_modes_followed() {
        let mut s = Screen::new(20, 5);
        // What it is, and which version; whether it is well; where the
        // cursor is, from 1, plain and DEC's.
        assert_eq!(s.process(b"\x1b[c"), b"\x1b[?1;2c");
        assert_eq!(s.process(b"\x1b[0c"), b"\x1b[?1;2c");
        assert_eq!(s.process(b"\x1b[>c"), b"\x1b[>0;100;0c");
        assert_eq!(s.process(b"\x1b[5n"), b"\x1b[0n");
        assert_eq!(s.process(b"\x1b[3;7H\x1b[6n"), b"\x1b[3;7R");
        assert_eq!(s.process(b"\x1b[?6n"), b"\x1b[?3;7R");
        // Several at once, in order, what is written between them shown.
        assert_eq!(s.process(b"\x1b[5nab\x1b[6n"), b"\x1b[0n\x1b[3;9R");
        // Its colours, the page's own.
        assert_eq!(s.process(b"\x1b]11;?\x07"), b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\");
        assert_eq!(s.process(b"\x1b]10;?\x1b\\"), b"\x1b]10;rgb:cccc/cccc/cccc\x1b\\");
        // Nothing else.
        assert!(s.process(b"\x1b[?u\x1b[14t\x1b]12;?\x07").is_empty());
        // The cursor keys' mode and bracketed pastes, as it asks.
        assert!(!s.application_cursor() && !s.bracketed_paste());
        s.process(b"\x1b[?1h\x1b[?2004h");
        assert!(s.application_cursor() && s.bracketed_paste());
        s.process(b"\x1b[?1l\x1b[?2004l");
        assert!(!s.application_cursor() && !s.bracketed_paste());
    }

    #[test]
    fn the_mouse_is_reported_as_the_editor_asks_for_it_and_else_not_at_all() {
        let line = |s: &mut Screen, l: &str| s.mouse(&l.split_whitespace().collect::<Vec<_>>());
        let mut s = Screen::new(20, 5);
        // Not asked for: nothing.
        for l in ["m 3 2 l", "u 3 2", "w 3 2 1", "v 4 2", "d 4 2"] {
            assert!(line(&mut s, l).is_empty(), "{l}");
        }
        // Presses and releases, as SGR, from 1: the left button, the right
        // let go at once, the wheel a report for each line; not the mouse
        // moved, nor a cell off the screen.
        s.process(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(line(&mut s, "m 3 2 l"), b"\x1b[<0;4;3M");
        assert_eq!(line(&mut s, "u 5 2"), b"\x1b[<0;6;3m");
        assert_eq!(line(&mut s, "u 5 2"), b"", "let go once");
        assert_eq!(line(&mut s, "m 0 0 r"), b"\x1b[<2;1;1M\x1b[<2;1;1m");
        assert_eq!(line(&mut s, "w 1 1 2"), b"\x1b[<65;2;2M\x1b[<65;2;2M");
        assert_eq!(line(&mut s, "w 1 1 -1"), b"\x1b[<64;2;2M");
        assert_eq!(line(&mut s, "w -1"), b"", "over no cell");
        assert_eq!(line(&mut s, "v 4 2"), b"");
        assert_eq!(line(&mut s, "m 20 2 l"), b"", "off the screen");
        // The mouse moved with the button down, once asked for; let go off
        // the screen, where it was last.
        s.process(b"\x1b[?1002h");
        assert_eq!(line(&mut s, "m 3 2 l"), b"\x1b[<0;4;3M");
        assert_eq!(line(&mut s, "d 4 2"), b"\x1b[<32;5;3M");
        assert_eq!(line(&mut s, "v 6 2"), b"", "with no button down");
        assert_eq!(line(&mut s, "u"), b"\x1b[<0;7;3m");
        // Moved anywhere, once asked for.
        s.process(b"\x1b[?1003h");
        assert_eq!(line(&mut s, "v 6 3"), b"\x1b[<35;7;4M");
        // Without SGR: xterm's first encoding, a byte each, 32 more.
        s.process(b"\x1b[?1006l");
        assert_eq!(line(&mut s, "m 3 2 l"), b"\x1b[M\x20\x24\x23");
        assert_eq!(line(&mut s, "u 3 2"), b"\x1b[M\x23\x24\x23");
        // Presses alone, as X10 asked for them: no release.
        s.process(b"\x1b[?1003l\x1b[?9h");
        assert_eq!(line(&mut s, "m 0 0 r"), b"\x1b[M\x22\x21\x21");
        // Turned off: nothing.
        s.process(b"\x1b[?9l");
        assert!(line(&mut s, "m 3 2 l").is_empty());
    }

    /// A stand-in for an editor, in `dir`, that writes `script`.
    fn stand_in(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
        let path = dir.join("editor.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// What a stand-in wrote in its terminal, as it is read.
    struct Written {
        rx: mpsc::Receiver<Option<Vec<u8>>>,
        all: String,
    }

    impl Written {
        /// All it wrote, read until it holds `end`.
        fn until(&mut self, end: &str) -> &str {
            let started = Instant::now();
            while !self.all.contains(end) && started.elapsed() < Duration::from_secs(60) {
                match self.rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(Some(more)) => self.all.push_str(&String::from_utf8_lossy(&more)),
                    _ => break,
                }
            }
            &self.all
        }
    }

    /// `script`, a stand-in's, started in a terminal of `cols` by `rows`, as
    /// `run_editor` runs an editor, with sh: it, and what it writes there.
    fn started(script: &std::path::Path, cols: u16, rows: u16, cmd: impl FnOnce(&mut Command)) -> (Pty, Written) {
        let mut command = Command::new("sh");
        command.arg("-c").arg(format!("exec '{}'", script.display()));
        cmd(&mut command);
        let (tx, rx) = mpsc::channel();
        let pty = Pty::start(&mut command, cols, rows, move |o| {
            let _ = tx.send(o);
        })
        .unwrap();
        (pty, Written { rx, all: String::new() })
    }

    #[test]
    fn an_editor_runs_in_a_terminal_of_its_own_in_a_session_of_its_own_sized_as_said() {
        let tmp = tempfile::tempdir().unwrap();
        // Its terminal: its size, the controlling terminal of a session it
        // leads, xterm's, LINES taken away; then a line typed, and its size
        // once resized.
        let editor = stand_in(
            tmp.path(),
            "printf 'size %s\\n' \"$(stty size)\"\n\
             [ \"$(ps -o pgid= -p $$ | tr -d ' ')\" = \"$$\" ] && : < /dev/tty && printf 'leads %s\\n' \"$TERM\"\n\
             printf 'lines %s.\\n' \"${LINES:-none}\"\n\
             IFS= read -r line\n\
             printf 'typed %s\\n' \"$line\"\n\
             IFS= read -r line\n\
             printf 'now %s\\n' \"$(stty size)\"\n\
             exit 3",
        );
        let (mut pty, mut written) = started(&editor, 33, 7, |c| {
            c.env("LINES", "99");
        });
        let said = written.until("lines ").to_string();
        assert!(said.contains("size 7 33") && said.contains("leads xterm-256color"), "{said:?}");
        // Its new lines made CR LF by its terminal, as a terminal's are.
        let said = written.until(".\r\n").to_string();
        assert!(said.contains("lines none."), "{said:?}");
        // A key, typed: echoed by its terminal, then read.
        pty.write(&key(&["c", "68", "-"], false).unwrap());
        pty.write(&key(&["Enter", "-"], false).unwrap());
        let said = written.until("typed h").to_string();
        assert!(said.contains("typed h"), "{said:?}");
        // Resized, then a line more.
        pty.resize(50, 12).unwrap();
        pty.write(b"\r");
        let said = written.until("now 12 50").to_string();
        assert!(said.contains("now 12 50"), "{said:?}");
        // Exited: how, its terminal closed, read to its end.
        let started = Instant::now();
        let status = loop {
            if let Some(s) = pty.exited().unwrap() {
                break s;
            }
            assert!(started.elapsed() < Duration::from_secs(60), "still running");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(3));
        while let Ok(Some(_)) = written.rx.recv_timeout(Duration::from_secs(60)) {}
        assert!(pty.master.is_none());
    }

    #[test]
    fn an_editor_hung_up_on_is_gone_with_what_it_started_even_one_that_does_not_hear_it() {
        let tmp = tempfile::tempdir().unwrap();
        for ignored in [false, true] {
            // It and a process it started, both waiting, the hangup heard
            // or not.
            let trap = if ignored { "trap '' HUP\n" } else { "" };
            let editor = stand_in(tmp.path(), &format!("{trap}sleep 300 &\necho \"pids $$ $!.\"\nwait"));
            let (pty, mut written) = started(&editor, 80, 24, |_| {});
            let said = written.until(".\r\n").to_string();
            let pids: Vec<libc::pid_t> = said.split_whitespace().filter_map(|w| w.trim_end_matches('.').parse().ok()).collect();
            assert_eq!(pids.len(), 2, "{said:?}");
            let started = Instant::now();
            drop(pty);
            // Gone, the one that does not hear it killed once HANGING_UP is
            // past; the other, at once. Each reaped, by stacked or by the
            // system, before it is said gone.
            let gone = |pid: libc::pid_t| unsafe { libc::kill(pid, 0) } != 0;
            while !pids.iter().all(|p| gone(*p)) {
                assert!(started.elapsed() < Duration::from_secs(60), "left running: {pids:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
            if ignored {
                assert!(started.elapsed() >= HANGING_UP, "killed {:?} after", started.elapsed());
            }
        }
    }
}
