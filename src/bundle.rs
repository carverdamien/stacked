//! StackED.app: StackED as macOS opens an app, from the Dock or the Finder.
//! `stacked app --bundle` makes it: this stacked copied in, StackED's icon
//! drawn into its icon file, the whole signed as this machine's own (ad
//! hoc). Opened so, stacked has no terminal, nor what a terminal's shell
//! sets: it takes what the user's login shell sets (PATH, for git and
//! claude; EDITOR), then opens StackED on the database `$STACKED_FILE` or
//! the settings name, else on one picked in the system's Open dialog.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::icon_data::IconDataExt;

use crate::db::{Error, Result};

/// The bundle's name.
pub const NAME: &str = "StackED.app";

/// What macOS knows the bundle by, its Info.plist saying so.
pub const ID: &str = "io.github.carverdamien.stacked";

/// The pictures of the icon file, as `iconutil` names them, and the size of
/// each, in pixels.
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

/// The bundle's Info.plist: its name, what macOS knows it by, the program
/// it runs (stacked), its icon, and its version, this stacked's.
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

/// StackED.app made in `dir`, made anew if it is there already: where it
/// is. Anything else of its name there is left alone.
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

/// Whether stacked was opened as StackED.app is, from the Dock or the
/// Finder: `exe` in an app's Contents/MacOS, given nothing to do (but the
/// `-psn_…` some versions of macOS give an app).
pub fn opened(exe: &Path, args: &[String]) -> bool {
    let bundled = exe.parent().is_some_and(|d| d.ends_with("Contents/MacOS"))
        && exe.ancestors().nth(3).and_then(Path::extension).is_some_and(|e| e == "app");
    bundled && args.iter().skip(1).all(|a| a.starts_with("-psn_"))
}

/// Printed around what `env -0` prints, by the login shell `login_env`
/// runs: what is between them is the environment, whatever the shell's
/// files print before.
const START: &str = "STACKED-ENV";
const END: &str = "STACKED-END";

/// What the user's login shell `shell` sets, as a terminal's would have it
/// (PATH, for git and claude; EDITOR; STACKED_FILE): none if the shell
/// fails, or takes longer than `wait`, as broken files of its might.
pub fn login_env(shell: &Path, wait: Duration) -> Option<Vec<(OsString, OsString)>> {
    let script = format!("printf '\\0%s\\0' {START}; /usr/bin/env -0; printf '%s\\0' {END}");
    let mut child = Command::new(shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    // Read away from the wait: what the shell started may keep its output
    // open after it exits.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let until = Instant::now() + wait;
    let mut all = Vec::new();
    let env = loop {
        if let Some(env) = parse_env(&all) {
            break Some(env);
        }
        match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(more) => all.extend(more),
            Err(_) => break None,
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    env
}

/// The variables `out` holds between `START` and `END`: none until `END`
/// is there.
fn parse_env(out: &[u8]) -> Option<Vec<(OsString, OsString)>> {
    let start = format!("\0{START}\0");
    let at = out.windows(start.len()).position(|w| w == start.as_bytes())?;
    let mut env = Vec::new();
    for entry in out[at + start.len()..].split(|b| *b == 0) {
        if entry == END.as_bytes() {
            return Some(env);
        }
        if let Some(eq) = entry.iter().position(|b| *b == b'=') {
            let (k, v) = (&entry[..eq], &entry[eq + 1..]);
            env.push((OsStr::from_bytes(k).to_os_string(), OsStr::from_bytes(v).to_os_string()));
        }
    }
    None
}

/// `env` set in this process's environment, over what it has: but where
/// the shell was (PWD and the like), which is nothing here. To be called
/// before stacked starts a thread of its own (`login_env`'s reader reads
/// its pipe, and nothing of the environment).
pub fn adopt(env: Vec<(OsString, OsString)>) {
    for (k, v) in env {
        if ["PWD", "OLDPWD", "SHLVL", "_"].iter().any(|s| k == *s) {
            continue;
        }
        // SAFETY: no other thread reads or writes the environment yet.
        unsafe { std::env::set_var(k, v) };
    }
}

/// A database picked in the system's Open dialog: none if it is cancelled.
pub fn pick() -> Option<PathBuf> {
    let choose = "POSIX path of (choose file with prompt \"Pick the stacked database for StackED to open (a .jsonl file).\")";
    let out = Command::new("osascript")
        .args(["-e", "activate", "-e", choose])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim_end_matches('\n');
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// `text` said in an alert: opened from the Dock, stacked has no terminal
/// to say it in.
pub fn alert(text: &str) {
    let quoted = text.replace('\\', "\\\\").replace('"', "\\\"");
    let _ = Command::new("osascript")
        .args(["-e", "activate", "-e", &format!("display alert \"StackED\" message \"{quoted}\" as critical")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_from_the_dock_is_the_bundle_s_stacked_given_nothing() {
        let exe = Path::new("/Users/me/Applications/StackED.app/Contents/MacOS/stacked");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(opened(exe, &args(&["stacked"])));
        assert!(opened(exe, &args(&["stacked", "-psn_0_1234"])), "as some versions of macOS open an app");
        assert!(!opened(exe, &args(&["stacked", "comments"])), "a command, from a terminal");
        assert!(!opened(Path::new("/Users/me/.cargo/bin/stacked"), &args(&["stacked"])));
        assert!(!opened(Path::new("/tmp/Contents/MacOS/stacked"), &args(&["stacked"])), "not in an app");
    }

    #[test]
    fn the_login_shell_s_environment_is_what_env_prints_between_the_marks() {
        let out = b"Welcome!\n\0STACKED-ENV\0PATH=/opt/bin:/usr/bin\0EDITOR=hx\0NOTE=a\nb=c\0STACKED-END\0";
        let env = parse_env(out).unwrap();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap())).collect();
        assert_eq!(env, [("PATH", "/opt/bin:/usr/bin"), ("EDITOR", "hx"), ("NOTE", "a\nb=c")]);
        // Not all of it yet: nothing.
        assert!(parse_env(b"\0STACKED-ENV\0PATH=/usr/bin\0").is_none());
        assert!(parse_env(b"PATH=/usr/bin\0STACKED-END\0").is_none());
    }

    #[test]
    fn the_login_shell_is_run_for_its_environment_and_not_waited_for_long() {
        let tmp = tempfile::tempdir().unwrap();
        let shell = |name: &str, script: &str| {
            let p = tmp.path().join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            p
        };
        // As a login shell is run: `-ilc` and the script, run.
        let sh = shell("sh", "[ \"$1\" = -ilc ] || exit 1\nexport FROM_LOGIN=yes\neval \"$2\"");
        let env = login_env(&sh, Duration::from_secs(10)).unwrap();
        assert!(env.iter().any(|(k, v)| k == "FROM_LOGIN" && v == "yes"), "{env:?}");
        // One that never ends, nor prints the end: given up.
        let stuck = shell("stuck", "exec sleep 30");
        let started = Instant::now();
        assert!(login_env(&stuck, Duration::from_millis(300)).is_none());
        assert!(started.elapsed() < Duration::from_secs(5));
        // One that fails: nothing.
        assert!(login_env(&shell("fails", "exit 3"), Duration::from_secs(10)).is_none());
    }

    #[test]
    fn the_info_plist_names_the_bundle_and_its_program() {
        let plist = info_plist();
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
}
