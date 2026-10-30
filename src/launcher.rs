//! What StackED is to the system it runs on: an app of its own, which
//! `stacked app --bundle` makes, and which the system opens with no
//! terminal (`bundle` has what stacked does then, the same everywhere).
//!
//! On macOS, StackED.app (`mac`): this stacked copied in, StackED's icon
//! drawn into its icon file, the whole signed ad hoc; opened from the Dock
//! or the Finder; `$EDITOR` run in a window of Terminal's. On Linux and the
//! other desktops that follow freedesktop.org (`desktop`): a launcher, a
//! .desktop file running this stacked, and StackED's icon beside it, in the
//! folders the desktop reads; opened from the desktop's applications;
//! `$EDITOR` run in a terminal of the system's. Both parts are built and
//! tested on every system; this system's is the one the rest of stacked
//! asks for (`make`, `opened`, `terminal`, `IN_TERMINAL`), so that nothing
//! else of stacked's tells the systems apart.
//!
//! And what only macOS needs of its own toolkit: while one of its panels
//! is open, the Edit menu's shortcuts on the menu bar, which is the only
//! way AppKit gives ⌘C and ⌘V to the panel's text fields
//! (`with_edit_keys`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::db::{Error, Result};

/// What the system knows StackED by: its bundle's identifier on macOS, its
/// launcher's name and its icon's elsewhere, and its windows' app id.
pub const ID: &str = "io.github.carverdamien.stacked";

/// What `stacked app --bundle` made: where (printed alone, on stdout), and
/// what is said of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub at: PathBuf,
    pub said: String,
}

#[cfg(target_os = "macos")]
use mac as this;
#[cfg(not(target_os = "macos"))]
use desktop as this;

/// `stacked app --bundle [DIR]`: StackED made an app of this system's, in
/// `dir` if given, else where the system keeps the user's apps.
pub fn make(dir: Option<PathBuf>) -> Result<Made> {
    this::made(dir.map(|d| std::path::absolute(&d).unwrap_or(d)))
}

/// Whether this stacked, at `exe` and given `args`, was opened as this
/// system opens StackED's app.
pub fn opened(exe: &Path, args: &[String]) -> bool {
    this::opened(exe, args)
}

/// Runs `command`, for sh, in a window of a terminal of this system's: an
/// error if none can be opened.
pub fn terminal(command: &str) -> Result<()> {
    this::terminal(command)
}

/// Where `$EDITOR` runs once StackED's app is opened with no terminal, as
/// the window waiting for it says.
pub const IN_TERMINAL: &str = this::IN_TERMINAL;

/// `f`, a dialog of the system's, run with what it needs of StackED: on
/// macOS, on the main thread, with the Edit menu's shortcuts on the menu
/// bar while it runs (`edit_keys`); elsewhere, as it is.
#[cfg(target_os = "macos")]
pub fn with_edit_keys<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    edit_keys::while_open(f)
}

#[cfg(not(target_os = "macos"))]
pub fn with_edit_keys<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    f()
}

/// `program` run with `args`, for what it does: what it says if it fails.
fn run(program: &str, args: &[&OsStr]) -> Result<()> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error(format!("{program}: {e}")))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(Error(format!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim()))),
    }
}

/// `bytes` written to `path` whole: beside it first, then renamed over it,
/// so that what reads the folder never sees it half written.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<()> {
    let at = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(at)?;
    }
    let mut part = path.as_os_str().to_owned();
    part.push(format!(".{}", std::process::id()));
    std::fs::write(&part, bytes).and_then(|_| std::fs::rename(&part, path)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        at(e)
    })
}

/// StackED.app, as macOS opens an app: from the Dock or the Finder.
pub mod mac {
    use std::collections::HashMap;
    use std::collections::hash_map::Entry;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    use eframe::icon_data::IconDataExt;

    use super::{ID, Made, run};
    use crate::db::{Error, Result};

    /// The bundle's name.
    pub const NAME: &str = "StackED.app";

    /// Where `$EDITOR` runs, opened from the Dock.
    pub const IN_TERMINAL: &str = "in a window of Terminal's";

    /// The pictures of the icon file, as `iconutil` names them, and the size
    /// of each, in pixels.
    const PICTURES: [(&str, u32); 10] = [
        ("16x16", 16),
        ("16x16@2x", 32),
        ("32x32", 32),
        ("32x32@2x", 64),
        ("128x128", 128),
        ("128x128@2x", 256),
        ("256x256", 256),
        ("256x256@2x", 512),
        ("512x512", 512),
        ("512x512@2x", 1024),
    ];

    /// The bundle's Info.plist: its name, what macOS knows it by, the
    /// program it runs (stacked), its icon, and its version, this stacked's.
    pub fn info_plist() -> String {
        let version = env!("CARGO_PKG_VERSION");
        let keys = [
            ("CFBundleDevelopmentRegion", "en"),
            ("CFBundleDisplayName", "StackED"),
            ("CFBundleExecutable", "stacked"),
            ("CFBundleIconFile", "StackED"),
            ("CFBundleIdentifier", ID),
            ("CFBundleInfoDictionaryVersion", "6.0"),
            ("CFBundleName", "StackED"),
            ("CFBundlePackageType", "APPL"),
            ("CFBundleShortVersionString", version),
            ("CFBundleVersion", version),
            ("LSApplicationCategoryType", "public.app-category.developer-tools"),
        ];
        let mut out = String::from(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
        ));
        for (key, value) in keys {
            out.push_str(&format!("\t<key>{key}</key>\n\t<string>{value}</string>\n"));
        }
        out.push_str("\t<key>NSHighResolutionCapable</key>\n\t<true/>\n</dict>\n</plist>\n");
        out
    }

    /// StackED.app made in `dir`, made anew if it is there already: where
    /// it is. Anything else of its name there is left alone.
    pub fn make(dir: &Path) -> Result<PathBuf> {
        let at = |e: std::io::Error, p: &Path| Error(format!("{}: {e}", p.display()));
        let app = dir.join(NAME);
        let exe = std::env::current_exe().map_err(|e| Error(format!("where stacked is: {e}")))?;
        if exe.starts_with(&app) {
            return Err(Error(format!(
                "this stacked is the one in {}: make StackED.app with the stacked to copy into it",
                app.display()
            )));
        }
        if app.exists() {
            let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap_or_default();
            if !plist.contains(&format!("<string>{ID}</string>")) {
                return Err(Error(format!("{} is there, and is not StackED: nothing changed", app.display())));
            }
            std::fs::remove_dir_all(&app).map_err(|e| at(e, &app))?;
        }
        let contents = app.join("Contents");
        let pictures = contents.join("Resources/StackED.iconset");
        for d in [contents.join("MacOS"), pictures.clone()] {
            std::fs::create_dir_all(&d).map_err(|e| at(e, &d))?;
        }
        let plist = contents.join("Info.plist");
        std::fs::write(&plist, info_plist()).map_err(|e| at(e, &plist))?;
        let program = contents.join("MacOS/stacked");
        std::fs::copy(&exe, &program).map_err(|e| at(e, &program))?;
        // The icon, each size drawn once, then made one file.
        let mut drawn: HashMap<u32, Vec<u8>> = HashMap::new();
        for (name, size) in PICTURES {
            if let Entry::Vacant(e) = drawn.entry(size) {
                e.insert(crate::icon::data(size).to_png_bytes().map_err(|e| Error(format!("the icon: {e}")))?);
            }
            let file = pictures.join(format!("icon_{name}.png"));
            std::fs::write(&file, &drawn[&size]).map_err(|e| at(e, &file))?;
        }
        let icon = contents.join("Resources/StackED.icns");
        run("iconutil", &[OsStr::new("-c"), "icns".as_ref(), pictures.as_os_str(), "-o".as_ref(), icon.as_os_str()])?;
        std::fs::remove_dir_all(&pictures).map_err(|e| at(e, &pictures))?;
        run("codesign", &[OsStr::new("--force"), "--sign".as_ref(), "-".as_ref(), app.as_os_str()])?;
        Ok(app)
    }

    /// `stacked app --bundle [DIR]` on macOS: StackED.app made in `dir`,
    /// else in ~/Applications, the folder made if need be.
    pub fn made(dir: Option<PathBuf>) -> Result<Made> {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let dir = dir.unwrap_or_else(|| home.join("Applications"));
        std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
        let app = make(&dir)?;
        let said = format!(
            "StackED.app is made. Drag it from the Finder to the Dock to keep it there (`open -R {}` shows it). Make it again once stacked is updated: it runs a copy of this stacked.",
            app.display()
        );
        Ok(Made { at: app, said })
    }

    /// Whether stacked was opened as StackED.app is, from the Dock or the
    /// Finder: `exe` in an app's Contents/MacOS, given nothing to do (but
    /// the `-psn_…` some versions of macOS give an app).
    pub fn opened(exe: &Path, args: &[String]) -> bool {
        let bundled = exe.parent().is_some_and(|d| d.ends_with("Contents/MacOS"))
            && exe.ancestors().nth(3).and_then(Path::extension).is_some_and(|e| e == "app");
        bundled && args.iter().skip(1).all(|a| a.starts_with("-psn_"))
    }

    /// Opens a window of Terminal's running `command`, for
    /// `bundle::edit_in_window`: `command` is written in a script (a
    /// .command file), which `open -a Terminal` has Terminal run in a window
    /// of its own, as it runs any `.command` file. No AppleScript, so macOS
    /// asks for no permission to control Terminal.
    pub fn terminal(command: &str) -> Result<()> {
        crate::bundle::in_terminal(command, "command", &|script| run("open", &[OsStr::new("-a"), OsStr::new("Terminal"), script]))
    }
}

/// StackED's launcher, as the desktops that follow freedesktop.org have
/// their applications: a .desktop file in `applications/` of the user's
/// data folder (`$XDG_DATA_HOME`, else ~/.local/share), running this
/// stacked, and StackED's icon in `icons/hicolor/`, at the sizes the icon
/// theme has.
pub mod desktop {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use eframe::icon_data::IconDataExt;

    use super::{ID, Made, write_whole};
    use crate::db::{Error, Result};

    /// What the launcher runs after stacked's path: StackED, told that the
    /// desktop opened it.
    pub const ARGS: [&str; 2] = ["app", "--launched"];

    /// Where `$EDITOR` runs, opened from the launcher.
    pub const IN_TERMINAL: &str = "in a terminal window";

    /// The icon's sizes, in pixels: those of the hicolor theme's folders
    /// for apps, each `NxN`.
    pub const SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];

    /// The user's data folder, as the environment `var` names it:
    /// `$XDG_DATA_HOME` if it is a path from the root, else
    /// ~/.local/share; none without a home.
    pub fn data_home(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
        let named = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        match named("XDG_DATA_HOME").filter(|p| p.is_absolute()) {
            Some(dir) => Some(dir),
            None => named("HOME").map(|home| home.join(".local/share")),
        }
    }

    /// The launcher's file, in the data folder `data`.
    pub fn entry_file(data: &Path) -> PathBuf {
        data.join("applications").join(format!("{ID}.desktop"))
    }

    /// The icon of `size` pixels, in the data folder `data`.
    pub fn icon_file(data: &Path, size: u32) -> PathBuf {
        data.join(format!("icons/hicolor/{size}x{size}/apps/{ID}.png"))
    }

    /// `path` as a word of the launcher's Exec line: quoted, its `"`, `` `
    /// ``, `$` and `\` escaped as the line's quoting has them, each `\` then
    /// doubled as the file's strings have them, and `%` doubled, as a field
    /// code starts with it. A path that is not UTF-8, or holds what a line
    /// cannot (its end, a tab), cannot be one.
    pub fn exec_word(path: &Path) -> Result<String> {
        let s = path_text(path)?;
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' | '`' | '$' => {
                    out.push_str("\\\\");
                    out.push(c);
                }
                '\\' => out.push_str("\\\\\\\\"),
                '%' => out.push_str("%%"),
                c => out.push(c),
            }
        }
        out.push('"');
        Ok(out)
    }

    /// `path` as a string of the launcher's: each `\` doubled.
    fn string(path: &Path) -> Result<String> {
        Ok(path_text(path)?.replace('\\', "\\\\"))
    }

    /// `path` as text, if a launcher can hold it.
    fn path_text(path: &Path) -> Result<&str> {
        let s = path.to_str().ok_or_else(|| Error(format!("{}: a launcher holds only paths of UTF-8", path.display())))?;
        if s.chars().any(char::is_control) {
            return Err(Error(format!("{s:?}: a launcher holds no path with a line's end or a tab in it")));
        }
        Ok(s)
    }

    /// The launcher's file, for the stacked at `exe`: StackED, its icon, and
    /// what it runs, `exe` and `ARGS`, not in a terminal.
    pub fn entry(exe: &Path) -> Result<String> {
        let lines = [
            "[Desktop Entry]".to_string(),
            "Type=Application".into(),
            "Version=1.5".into(),
            "Name=StackED".into(),
            "GenericName=Stack editor".into(),
            "Comment=Review a stack of patches, and have an agent edit it".into(),
            format!("TryExec={}", string(exe)?),
            format!("Exec={} {}", exec_word(exe)?, ARGS.join(" ")),
            format!("Icon={ID}"),
            "Terminal=false".into(),
            "Categories=Development;RevisionControl;".into(),
            "Keywords=git;patch;stack;review;".into(),
            "StartupNotify=true".into(),
            // As X11 names the windows: by the program's own name.
            "StartupWMClass=stacked".into(),
        ];
        Ok(lines.join("\n") + "\n")
    }

    /// Whether `text`, a launcher's file, is StackED's: it runs a stacked as
    /// the launcher does.
    fn ours(text: &str) -> bool {
        text.lines().any(|l| l.starts_with("Exec=") && l.ends_with(&format!(" {}", ARGS.join(" "))))
    }

    /// StackED's launcher made in the data folder `data`, for the stacked at
    /// `exe`, made anew if it is there already, the icon drawn at each of
    /// `SIZES`: where the launcher is. A file of its name that is not
    /// StackED's is left alone.
    pub fn make(data: &Path, exe: &Path) -> Result<PathBuf> {
        let file = entry_file(data);
        if let Ok(text) = std::fs::read_to_string(&file)
            && !ours(&text)
        {
            return Err(Error(format!("{} is there, and is not StackED's launcher: nothing changed", file.display())));
        }
        if std::fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) {
            return Err(Error(format!("{} is there, and is not StackED's launcher: nothing changed", file.display())));
        }
        let text = entry(exe)?;
        for size in SIZES {
            let png = crate::icon::data(size).to_png_bytes().map_err(|e| Error(format!("the icon: {e}")))?;
            write_whole(&icon_file(data, size), &png)?;
        }
        write_whole(&file, text.as_bytes())?;
        Ok(file)
    }

    /// `stacked app --bundle [DIR]` elsewhere than on macOS: StackED's
    /// launcher made in the data folder `dir`, else the user's.
    pub fn made(dir: Option<PathBuf>) -> Result<Made> {
        let data = match dir {
            Some(dir) => dir,
            None => data_home(|k| std::env::var_os(k))
                .ok_or_else(|| Error("neither $XDG_DATA_HOME nor $HOME is set, to say where your applications are: name the folder, as `stacked app --bundle ~/.local/share` does".into()))?,
        };
        let exe = std::env::current_exe().map_err(|e| Error(format!("where stacked is: {e}")))?;
        let at = make(&data, &exe)?;
        let said = format!(
            "StackED's launcher is made, and its icon is in {}. StackED is among your applications now. The launcher runs this stacked, {}: make the launcher again if this stacked moves.",
            data.join("icons/hicolor").display(),
            exe.display()
        );
        Ok(Made { at, said })
    }

    /// Whether stacked was opened by its launcher: given `ARGS`, and
    /// nothing else.
    pub fn opened(_exe: &Path, args: &[String]) -> bool {
        args.len() == ARGS.len() + 1 && args[1..].iter().zip(ARGS).all(|(a, b)| a == b)
    }

    /// A terminal, as one is told to run a program in a window of its own:
    /// its program, then `args`, then the program to run.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Terminal {
        pub program: PathBuf,
        pub args: Vec<String>,
    }

    /// The terminals tried when `$TERMINAL` names none: Debian's choice of
    /// the system's first, then the desktops' own, each with what goes
    /// before the program it runs.
    pub const TERMINALS: [(&str, &[&str]); 14] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("ptyxis", &["--"]),
        ("kgx", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("mate-terminal", &["-x"]),
        ("tilix", &["-e"]),
        ("terminator", &["-x"]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("foot", &[]),
        ("wezterm", &["start", "--"]),
        ("xterm", &["-e"]),
    ];

    /// What is said when no terminal is found to run `$EDITOR` in, after
    /// the editor's name.
    pub const NO_TERMINAL: &str =
        "did not run: no terminal was found to run it in (set $TERMINAL to your terminal's program, or name an editor with a window of its own in the settings); nothing changed";

    /// Whether `path` is a program to run.
    fn runs(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }

    /// The terminal to run a program in, as `terminal` (`$TERMINAL`) and
    /// `path` (`$PATH`) say: the one `$TERMINAL` names (its own words after
    /// its program, then `-e`, unless it is one of `TERMINALS`, which says
    /// what goes there); else the first of `TERMINALS` there is.
    pub fn find(terminal: Option<OsString>, path: Option<OsString>) -> Option<Terminal> {
        let dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        let find = |name: &str| match name.contains('/') {
            true => Some(PathBuf::from(name)).filter(|p| runs(p)),
            false => dirs.iter().map(|d| d.join(name)).find(|p| runs(p)),
        };
        let args = |name: &OsStr| TERMINALS.iter().find(|(n, _)| OsStr::new(n) == name).map_or(&["-e"][..], |(_, a)| *a);
        if let Some(named) = terminal.filter(|t| !t.is_empty()) {
            let named = named.to_string_lossy().into_owned();
            let mut words = named.split_whitespace();
            if let Some(program) = words.next().and_then(find) {
                let mut own: Vec<String> = words.map(String::from).collect();
                own.extend(args(program.file_name().unwrap_or_default()).iter().map(|a| a.to_string()));
                return Some(Terminal { program, args: own });
            }
        }
        TERMINALS.iter().find_map(|(name, args)| {
            find(name).map(|program| Terminal {
                program,
                args: args.iter().map(|a| a.to_string()).collect(),
            })
        })
    }

    impl Terminal {
        /// Runs the script `script` with sh in a window of the terminal's:
        /// started in a process group of its own, so that a ctrl-c where
        /// stacked was started does not stop it. An error if it fails at
        /// once, as a terminal that cannot open a window does; past a moment
        /// it runs, or has had its server open the window, and is waited for
        /// away from here.
        pub fn run(&self, script: &OsStr) -> Result<()> {
            let name = self.program.file_name().unwrap_or(self.program.as_os_str()).to_string_lossy().into_owned();
            let mut c = Command::new(&self.program);
            // sh reads the script, rather than the terminal running it: a
            // file just written may not be run while a process forked
            // meanwhile still holds it open.
            c.args(&self.args).arg("/bin/sh").arg(script).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            std::os::unix::process::CommandExt::process_group(&mut c, 0);
            let mut child = crate::bundle::spawn(&mut c).map_err(|e| Error(format!("did not run: {name} did not start: {e}; nothing changed")))?;
            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(s)) if s.success() => return Ok(()),
                    Ok(Some(s)) => {
                        let how = s.code().map_or_else(|| "was stopped".to_string(), |code| format!("failed, exiting with {code}"));
                        return Err(Error(format!("did not run: {name} {how}; nothing changed")));
                    }
                    Ok(None) if started.elapsed() < Duration::from_millis(500) => std::thread::sleep(Duration::from_millis(20)),
                    Ok(None) => {
                        std::thread::spawn(move || child.wait());
                        return Ok(());
                    }
                    Err(e) => return Err(Error(format!("did not run: {name}: {e}; nothing changed"))),
                }
            }
        }
    }

    /// Runs `command`, for sh, in a window of the terminal `terminal`
    /// (`$TERMINAL`) and `path` (`$PATH`) find: written in a script of its
    /// own, which removes itself as it runs (`bundle::in_terminal`). An
    /// error saying so if no terminal is found.
    pub fn terminal_in(command: &str, terminal: Option<OsString>, path: Option<OsString>) -> Result<()> {
        let found = find(terminal, path).ok_or_else(|| Error(NO_TERMINAL.into()))?;
        crate::bundle::in_terminal(command, "sh", &|script| found.run(script))
    }

    /// Runs `command`, for sh, in a window of this system's terminal: the
    /// one `$TERMINAL` names, else the first of `TERMINALS` on `$PATH`.
    pub fn terminal(command: &str) -> Result<()> {
        terminal_in(command, std::env::var_os("TERMINAL"), std::env::var_os("PATH"))
    }
}

/// The Edit menu of macOS's menu bar, there only while a panel of the
/// system's runs: AppKit gives ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V and ⌘A to a text field,
/// such as the name in the Save panel, only through items of the menu bar
/// that have them. StackED's windows have egui's menu bar, and AppKit's has
/// only the application menu winit makes, so the Edit menu is put there as
/// the panel opens, and taken off once it is closed.
#[cfg(target_os = "macos")]
mod edit_keys {
    use std::ffi::CStr;

    use objc2::MainThreadMarker;
    use objc2::MainThreadOnly;
    use objc2::rc::Retained;
    use objc2::runtime::Sel;
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::{NSString, ns_string};

    /// An item of the Edit menu: its title, the action it sends to what is
    /// in front (the panel's text field, or its window, which undoes), and
    /// the key of its shortcut, with ⌘, and with ⇧ too if it is a capital.
    pub(super) struct Edit {
        pub(super) title: &'static str,
        pub(super) action: &'static CStr,
        pub(super) key: &'static str,
    }

    /// The Edit menu: Undo and Redo, a separator (`None`), then Cut, Copy,
    /// Paste and Select All.
    pub(super) const EDIT: [Option<Edit>; 7] = [
        Some(Edit { title: "Undo", action: c"undo:", key: "z" }),
        Some(Edit { title: "Redo", action: c"redo:", key: "Z" }),
        None,
        Some(Edit { title: "Cut", action: c"cut:", key: "x" }),
        Some(Edit { title: "Copy", action: c"copy:", key: "c" }),
        Some(Edit { title: "Paste", action: c"paste:", key: "v" }),
        Some(Edit { title: "Select All", action: c"selectAll:", key: "a" }),
    ];

    /// The Edit menu on the menu bar, for as long as this is kept.
    struct OnBar {
        bar: Retained<NSMenu>,
        item: Retained<NSMenuItem>,
    }

    impl OnBar {
        /// The Edit menu put on the menu bar, last: none if there is no menu
        /// bar.
        fn put(mtm: MainThreadMarker) -> Option<OnBar> {
            let bar = NSApplication::sharedApplication(mtm).mainMenu()?;
            let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Edit"));
            for e in &EDIT {
                let it: Retained<NSMenuItem> = match e {
                    // SAFETY: each action is one of AppKit's own, which its
                    // text fields and their windows have, sent to what is in
                    // front (no target); each takes the item that sends it.
                    Some(e) => unsafe {
                        NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(mtm),
                            &NSString::from_str(e.title),
                            Some(Sel::register(e.action)),
                            &NSString::from_str(e.key),
                        )
                    },
                    None => NSMenuItem::separatorItem(mtm),
                };
                menu.addItem(&it);
            }
            // SAFETY: no action, no key: the menu's title on the bar.
            let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), ns_string!("Edit"), None, ns_string!("")) };
            item.setSubmenu(Some(&menu));
            bar.addItem(&item);
            Some(OnBar { bar, item })
        }
    }

    impl Drop for OnBar {
        fn drop(&mut self) {
            self.bar.removeItem(&self.item);
        }
    }

    /// `f` run on the main thread, with the Edit menu on the menu bar until
    /// it is done.
    pub(super) fn while_open<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        dispatch2::run_on_main(|mtm| {
            let _edit = OnBar::put(mtm);
            f()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_from_the_dock_is_the_bundle_s_stacked_given_nothing() {
        let exe = Path::new("/Users/me/Applications/StackED.app/Contents/MacOS/stacked");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(mac::opened(exe, &args(&["stacked"])));
        assert!(mac::opened(exe, &args(&["stacked", "-psn_0_1234"])), "as some versions of macOS open an app");
        assert!(!mac::opened(exe, &args(&["stacked", "comments"])), "a command, from a terminal");
        assert!(!mac::opened(Path::new("/Users/me/.cargo/bin/stacked"), &args(&["stacked"])));
        assert!(!mac::opened(Path::new("/tmp/Contents/MacOS/stacked"), &args(&["stacked"])), "not in an app");
    }

    #[test]
    fn opened_from_the_launcher_is_stacked_given_what_the_launcher_gives() {
        let exe = Path::new("/home/me/.cargo/bin/stacked");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(desktop::opened(exe, &args(&["stacked", "app", "--launched"])));
        assert!(!desktop::opened(exe, &args(&["stacked", "app"])), "from a terminal");
        assert!(!desktop::opened(exe, &args(&["stacked", "app", "--launched", "x.jsonl"])));
        assert!(!desktop::opened(exe, &args(&["stacked"])));
    }

    #[test]
    fn the_info_plist_names_the_bundle_and_its_program() {
        let plist = mac::info_plist();
        for part in [
            "<key>CFBundleExecutable</key>\n\t<string>stacked</string>",
            "<key>CFBundleIconFile</key>\n\t<string>StackED</string>",
            "<key>CFBundleIdentifier</key>\n\t<string>io.github.carverdamien.stacked</string>",
            "<key>CFBundleName</key>\n\t<string>StackED</string>",
            "<key>CFBundlePackageType</key>\n\t<string>APPL</string>",
        ] {
            assert!(plist.contains(part), "{part}\n{plist}");
        }
        assert!(plist.contains(&format!("<string>{}</string>", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn the_launcher_is_named_with_its_icon_and_runs_this_stacked_with_no_terminal() {
        let text = desktop::entry(Path::new("/home/me/.cargo/bin/stacked")).unwrap();
        assert_eq!(
            text,
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.5\n\
             Name=StackED\n\
             GenericName=Stack editor\n\
             Comment=Review a stack of patches, and have an agent edit it\n\
             TryExec=/home/me/.cargo/bin/stacked\n\
             Exec=\"/home/me/.cargo/bin/stacked\" app --launched\n\
             Icon=io.github.carverdamien.stacked\n\
             Terminal=false\n\
             Categories=Development;RevisionControl;\n\
             Keywords=git;patch;stack;review;\n\
             StartupNotify=true\n\
             StartupWMClass=stacked\n"
        );
        // What the Exec line's quoting escapes, then the file's strings:
        // a quote, a backtick, a dollar, a backslash, a percent.
        assert_eq!(desktop::exec_word(Path::new("/a b/\"q\"/`c`/$d/e\\f/50%")).unwrap(), r#""/a b/\\"q\\"/\\`c\\`/\\$d/e\\\\f/50%%""#);
        assert!(desktop::exec_word(Path::new("/a\nb")).is_err(), "a line's end");
    }

    #[test]
    fn the_launcher_and_its_icon_go_in_the_data_folder_and_are_made_again() {
        use eframe::icon_data::IconDataExt;
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("share");
        // $XDG_DATA_HOME, else ~/.local/share; a relative one is not one.
        let var = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into());
        assert_eq!(desktop::data_home(var(&[("XDG_DATA_HOME", "/x/data"), ("HOME", "/h")])), Some(PathBuf::from("/x/data")));
        assert_eq!(desktop::data_home(var(&[("XDG_DATA_HOME", "rel"), ("HOME", "/h")])), Some(PathBuf::from("/h/.local/share")));
        assert_eq!(desktop::data_home(var(&[("XDG_DATA_HOME", ""), ("HOME", "/h")])), Some(PathBuf::from("/h/.local/share")));
        assert_eq!(desktop::data_home(var(&[])), None);
        let exe = Path::new("/opt/stacked/bin/stacked");
        let at = desktop::make(&data, exe).unwrap();
        assert_eq!(at, data.join("applications/io.github.carverdamien.stacked.desktop"));
        assert_eq!(std::fs::read_to_string(&at).unwrap(), desktop::entry(exe).unwrap());
        // The icon at each size of the theme's, as the macOS one is drawn.
        for size in desktop::SIZES {
            let png = std::fs::read(data.join(format!("icons/hicolor/{size}x{size}/apps/io.github.carverdamien.stacked.png"))).unwrap();
            assert_eq!(png, crate::icon::data(size).to_png_bytes().unwrap(), "{size}");
            // A PNG of that size: its header says so.
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), size);
        }
        // Made again, for another stacked: anew; nothing left beside it.
        desktop::make(&data, Path::new("/elsewhere/stacked")).unwrap();
        assert!(std::fs::read_to_string(&at).unwrap().contains("Exec=\"/elsewhere/stacked\" app --launched"));
        let names: Vec<String> = std::fs::read_dir(data.join("applications")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["io.github.carverdamien.stacked.desktop"]);
        // A file of its name that is not StackED's: left alone.
        std::fs::write(&at, "[Desktop Entry]\nExec=other\n").unwrap();
        let e = desktop::make(&data, exe).unwrap_err();
        assert!(e.0.contains("is not StackED's launcher: nothing changed"), "{}", e.0);
        assert_eq!(std::fs::read_to_string(&at).unwrap(), "[Desktop Entry]\nExec=other\n");
    }

    #[test]
    fn the_terminal_is_the_one_terminal_names_else_the_first_found() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let program = |dir: &str, name: &str| {
            let d = tmp.path().join(dir);
            std::fs::create_dir_all(&d).unwrap();
            let p = d.join(name);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let konsole = program("a", "konsole");
        let xterm = program("b", "xterm");
        let kitty = program("b", "kitty");
        // Not a program: passed over.
        std::fs::write(tmp.path().join("a/x-terminal-emulator"), "").unwrap();
        let path = std::env::join_paths([tmp.path().join("a"), tmp.path().join("b")]).unwrap();
        let t = |program: &Path, args: &[&str]| desktop::Terminal {
            program: program.to_path_buf(),
            args: args.iter().map(|a| a.to_string()).collect(),
        };
        // None named: the first of the list there is.
        assert_eq!(desktop::find(None, Some(path.clone())), Some(t(&konsole, &["-e"])));
        // $TERMINAL: its own words, then what that terminal takes before
        // the program, else -e.
        assert_eq!(desktop::find(Some("kitty --single-instance".into()), Some(path.clone())), Some(t(&kitty, &["--single-instance"])));
        let other = program("c", "myterm");
        assert_eq!(desktop::find(Some(other.clone().into()), Some(path.clone())), Some(t(&other, &["-e"])));
        assert_eq!(desktop::find(Some("xterm".into()), Some(path.clone())), Some(t(&xterm, &["-e"])));
        // One $TERMINAL names that is not there: the list's.
        assert_eq!(desktop::find(Some("nowhere-term".into()), Some(path.clone())), Some(t(&konsole, &["-e"])));
        // None at all.
        let empty = tmp.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(desktop::find(Some("nowhere-term".into()), Some(empty.into())), None);
        assert_eq!(desktop::find(None, None), None);
    }

    #[test]
    fn the_editor_runs_in_a_terminal_window_and_none_found_is_said() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let file = tmp.path().join("note.txt");
        std::fs::write(&file, "before\n").unwrap();
        // No terminal: said, and nothing run.
        let e = crate::bundle::edit_in_window("printf 'after\\n' > \"$1\"", &file, tmp.path(), &|command| {
            desktop::terminal_in(command, None, Some(bin.clone().into()))
        })
        .unwrap_err();
        assert_eq!(e.0, desktop::NO_TERMINAL);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "before\n");
        // A stand-in for the system's terminal, as x-terminal-emulator is
        // run: `-e` and the program, sh and the script, run away from here,
        // its arguments noted.
        let log = tmp.path().join("ran.txt");
        let stand_in = bin.join("x-terminal-emulator");
        std::fs::write(&stand_in, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n[ \"$1\" = -e ] || exit 3\nshift\n\"$@\" &\n", log.display())).unwrap();
        std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755)).unwrap();
        crate::bundle::edit_in_window("printf 'after\\n' > \"$1\"", &file, tmp.path(), &|command| {
            desktop::terminal_in(command, None, Some(bin.clone().into()))
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "after\n");
        let ran = std::fs::read_to_string(&log).unwrap();
        let ran: Vec<&str> = ran.lines().collect();
        assert_eq!(ran[..2], ["-e", "/bin/sh"]);
        assert!(ran[2].ends_with(".sh") && !Path::new(ran[2]).exists(), "the script, gone once run: {ran:?}");
        // A terminal that fails at once: said.
        std::fs::write(&stand_in, "#!/bin/sh\nexit 4\n").unwrap();
        let e = crate::bundle::edit_in_window("true", &file, tmp.path(), &|command| desktop::terminal_in(command, None, Some(bin.clone().into()))).unwrap_err();
        assert_eq!(e.0, "did not run: x-terminal-emulator failed, exiting with 4; nothing changed");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_edit_menu_has_a_mac_app_s_items_shortcuts_and_actions() {
        use objc2::runtime::{AnyClass, Sel};
        // As the menu shows them while a panel runs: ⌘ and the key, and ⇧
        // too for a capital, which AppKit reads as ⇧ and the letter.
        let shown: Vec<String> = edit_keys::EDIT
            .iter()
            .map(|e| match e {
                Some(e) if e.key.chars().all(|c| c.is_ascii_uppercase()) => format!("{} ⇧⌘{}", e.title, e.key),
                Some(e) => format!("{} ⌘{}", e.title, e.key.to_uppercase()),
                None => "—".into(),
            })
            .collect();
        assert_eq!(shown, ["Undo ⌘Z", "Redo ⇧⌘Z", "—", "Cut ⌘X", "Copy ⌘C", "Paste ⌘V", "Select All ⌘A"]);
        // Each action is AppKit's: the text view that edits a field has it,
        // or the field's window, which undoes.
        let text = AnyClass::get(c"NSTextView").expect("AppKit's");
        let window = AnyClass::get(c"NSWindow").expect("AppKit's");
        for e in edit_keys::EDIT.iter().flatten() {
            let action = Sel::register(e.action);
            assert!(text.instance_method(action).is_some() || window.instance_method(action).is_some(), "{action:?}");
        }
    }
}
