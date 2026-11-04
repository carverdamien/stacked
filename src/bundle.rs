//! StackED as the system opens an app of its own, with no terminal: from
//! the Dock or the Finder on macOS, from the desktop's applications
//! elsewhere (`launcher` makes either, `stacked app --bundle`). Opened so,
//! stacked has no terminal, nor what a terminal's shell sets: it takes what
//! the user's login shell sets (PATH, for git and claude; EDITOR), then
//! opens StackED's windows again (`app::dock`), and runs `$EDITOR` in a
//! terminal window of its own (`edit_in_window`, the window the
//! launcher's), unless the editor opens a window of its own.

use std::ffi::{CStr, OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::db::{Error, Result};

/// The user's login shell: `$SHELL`, else the one the user database names
/// for the user, else sh.
pub fn login_shell() -> PathBuf {
    if let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) {
        return PathBuf::from(shell);
    }
    // SAFETY: the entry getpwuid gives back is read at once, before
    // anything else could ask for one.
    let named = unsafe {
        let entry = libc::getpwuid(libc::getuid());
        match entry.is_null() || (*entry).pw_shell.is_null() {
            true => None,
            false => Some(OsStr::from_bytes(CStr::from_ptr((*entry).pw_shell).to_bytes()).to_os_string()),
        }
    };
    named.filter(|s| !s.is_empty()).map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from)
}

/// `cmd` started. A program written a moment ago may not be run while a
/// process another thread forked meanwhile still holds it open (Linux says
/// so, "Text file busy"): it is tried again, for a second at most.
pub fn spawn(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    let mut tries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            started => return started,
        }
    }
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
    let mut child = spawn(Command::new(shell).args(["-ilc", &script]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())).ok()?;
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

/// Opened as an app: no terminal for `$EDITOR` to run in, a terminal
/// window opened for it instead (`edit_in_window`).
static NO_TERMINAL: AtomicBool = AtomicBool::new(false);

/// From now on, stacked has no terminal: the system opened it as an app.
pub fn set_no_terminal() {
    NO_TERMINAL.store(true, Ordering::Relaxed);
}

/// Whether stacked has no terminal for `$EDITOR`, opened as an app: it then
/// runs in a terminal window of its own, unless it opens a window of its
/// own (`config::own_window`).
pub fn no_terminal() -> bool {
    NO_TERMINAL.load(Ordering::Relaxed)
}

/// `s` quoted for sh.
fn sh_quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The edits in windows started so far: each its own files to wait on.
static EDITS: AtomicUsize = AtomicUsize::new(0);

/// Runs the editor as `editing` says (for sh, its file `"$1"`) on `file`, in
/// `dir`, in a window `open` opens on the sh command it is given, as a
/// terminal's are; once it exits, what it says: an error if it failed. The
/// window closed before the editor exited, an error too, nothing changed.
pub fn edit_in_window(editing: &str, file: &Path, dir: &Path, open: &dyn Fn(&str) -> Result<()>) -> Result<()> {
    let n = EDITS.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("stacked-edit-{}-{n}", std::process::id()));
    let (pid, status) = (base.with_extension("pid"), base.with_extension("status"));
    let gone = || {
        for f in [&pid, &status] {
            let _ = std::fs::remove_file(f);
        }
    };
    gone();
    let path = |p: &Path| sh_quoted(&p.to_string_lossy());
    // Its status written whole, once the editor exits.
    let script = format!(
        "echo $$ > {pid}; cd {dir} && sh -c {editing} editor {file}; echo $? > {status}.part && mv {status}.part {status}",
        pid = path(&pid),
        dir = path(dir),
        editing = sh_quoted(editing),
        file = path(file),
        status = path(&status),
    );
    open(&format!("exec sh -c {}", sh_quoted(&script)))?;
    let started = Instant::now();
    loop {
        if let Ok(said) = std::fs::read_to_string(&status) {
            gone();
            return match said.trim() {
                "0" => Ok(()),
                _ => Err(Error("failed; nothing changed".into())),
            };
        }
        let running = std::fs::read_to_string(&pid).ok().and_then(|p| p.trim().parse::<i32>().ok());
        match running {
            // Gone, with no status written: its window was closed.
            Some(p) if unsafe { libc::kill(p, 0) } != 0 && !status.exists() => {
                gone();
                return Err(Error("its window was closed before it exited; nothing changed".into()));
            }
            None if started.elapsed() > Duration::from_secs(30) => {
                gone();
                return Err(Error("no window opened for it; nothing changed".into()));
            }
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Has `open` run `command`, written in a script of its own (`script_file`,
/// its name ending in `.extension`), as a terminal runs a program given to
/// it. The script is removed if `open` fails; run, it removes itself.
pub fn in_terminal(command: &str, extension: &str, open: &dyn Fn(&OsStr) -> Result<()>) -> Result<()> {
    let script = script_file(command, extension)?;
    open(script.as_os_str()).inspect_err(|_| {
        let _ = std::fs::remove_file(&script);
    })
}

/// The scripts written for terminals so far: each a file of its own.
static SCRIPTS: AtomicUsize = AtomicUsize::new(0);

/// `command` written in a script (`script`) in the temporary folder, which
/// only its owner can read or run, its name ending in `.extension`: where
/// it is.
fn script_file(command: &str, extension: &str) -> Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let n = SCRIPTS.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("stacked-terminal-{}-{n}.{extension}", std::process::id()));
    let at = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
    let _ = std::fs::remove_file(&path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700).open(&path).map_err(at)?;
    f.write_all(script(command).as_bytes()).map_err(at)?;
    Ok(path)
}

/// A script that removes itself, then runs `command` with sh.
fn script(command: &str) -> String {
    format!("#!/bin/sh\nrm -f \"$0\"\n{command}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_shell_s_environment_is_what_env_prints_between_the_marks() {
        let out = b"Welcome!\n\0STACKED-ENV\0PATH=/opt/bin:/usr/bin\0EDITOR=hx\0NOTE=a\nb=c\0STACKED-END\0";
        let env = parse_env(out).unwrap();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap())).collect();
        assert_eq!(env, [("PATH", "/opt/bin:/usr/bin"), ("EDITOR", "hx"), ("NOTE", "a\nb=c")]);
        // Not all of it yet: nothing.
        assert!(parse_env(b"\0STACKED-ENV\0PATH=/usr/bin\0").is_none());
        assert!(parse_env(b"PATH=/usr/bin\0STACKED-END\0").is_none());
        // The shell is $SHELL's, else the user database's: one of the
        // system's, from the root.
        assert!(login_shell().is_absolute(), "{}", login_shell().display());
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
    fn an_edit_in_a_window_is_waited_for_and_says_how_it_went() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a dir's");
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("it's here.txt");
        std::fs::write(&file, "before\n").unwrap();
        // The window, as a terminal's runs what it is given: sh, away from
        // here; the child, to close the window with.
        let opened: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);
        let window = |command: &str| {
            let child = Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| Error(e.to_string()))?;
            *opened.lock().unwrap() = Some(child);
            Ok(())
        };
        // Edited, in the folder given: waited for, a quote and a space in
        // the names.
        edit_in_window("test -e \"its-here\" || printf 'after\\n' > \"$1\"", &file, &dir, &window).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "after\n");
        std::fs::write(dir.join("its-here"), "").unwrap();
        // The editor failing: said.
        let e = edit_in_window("test ! -e its-here", &file, &dir, &window).unwrap_err();
        assert_eq!(e.0, "failed; nothing changed");
        // The window closed while the editor runs: said, not waited for.
        let closing = |command: &str| {
            window(command)?;
            let mut child = opened.lock().unwrap().take().unwrap();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let _ = child.kill();
                let _ = child.wait();
            });
            Ok(())
        };
        let e = edit_in_window("exec sleep 30", &file, &dir, &closing).unwrap_err();
        assert!(e.0.contains("window was closed"), "{}", e.0);
    }

    #[test]
    fn a_terminal_is_given_a_script_that_removes_itself_then_runs_the_edit() {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(script("exec sh -c 'vi x'"), "#!/bin/sh\nrm -f \"$0\"\nexec sh -c 'vi x'\n");
        // For its owner alone, to read and run, named as the terminal takes
        // it.
        let path = script_file("true", "command").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), script("true"));
        assert!(path.extension().is_some_and(|e| e == "command"));
        std::fs::remove_file(&path).unwrap();
        // An edit in a window of a terminal's, the terminal played by running
        // the script with sh, as a terminal runs what it is given: waited
        // for, and the script gone.
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("note.txt");
        std::fs::write(&file, "before\n").unwrap();
        let given: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
        let terminal = |script: &OsStr| {
            *given.lock().unwrap() = Some(PathBuf::from(script));
            let mut child = Command::new("sh")
                .arg(script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| Error(e.to_string()))?;
            std::thread::spawn(move || child.wait());
            Ok(())
        };
        let open = |command: &str| in_terminal(command, "command", &terminal);
        edit_in_window("printf 'after\\n' > \"$1\"", &file, tmp.path(), &open).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "after\n");
        let ran = given.lock().unwrap().take().unwrap();
        assert!(ran.extension().is_some_and(|e| e == "command") && !ran.exists(), "{}", ran.display());
        // The terminal not opened: said, and the script removed.
        let fails = |script: &OsStr| {
            *given.lock().unwrap() = Some(PathBuf::from(script));
            Err(Error("no Terminal".into()))
        };
        assert_eq!(in_terminal("true", "sh", &fails).unwrap_err().0, "no Terminal");
        assert!(!given.lock().unwrap().take().unwrap().exists());
    }
}
