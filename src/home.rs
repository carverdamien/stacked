//! Where stacked keeps what is yours rather than a repository's, in one
//! folder: `~/.stacked/`, or the folder `$STACKED_HOME` names. It holds the
//! settings, `config.toml` (or the file `$STACKED_CONFIG` names), which
//! `config` reads and writes; and the state, `state.json` (or the file
//! `$STACKED_APP_STATE` names), which `state` keeps: the databases opened
//! last, by StackED and by stacked's editor, StackED's windows open, the
//! searches made in the editor's middle, and the effort last picked in its
//! menus. A checkout's own settings stay in the checkout, in
//! `.stacked/local.toml`.
//!
//! The binary alone works out where the folder is, from its environment,
//! once as it starts (`resolve`), and sets it here for the whole process
//! (`set`). Nothing else reads `$HOME` for these files: where nothing set
//! them, as in the tests' own processes, each is the null device, which
//! reads as empty and keeps nothing. A stacked that stacked runs itself (an
//! ask's `prompt`, `record` and `land`) is told the same files
//! (`Home::pass_on`), so that it works out none of its own.
//!
//! The settings were once in `~/.stacked.toml`, and StackED's state in
//! ~/Library/Application Support/StackED/state.json. A run of yours that
//! finds one of them, and nothing in its place in the folder, moves it
//! there and says so; one that finds both reads the folder's, and says that
//! the other is no longer read. Neither is moved when an override names
//! where it is to be.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, OnceLock};

/// Nowhere: the null device, which reads as empty and keeps nothing.
pub const NOWHERE: &str = "/dev/null";

/// The settings' file of before, in the home folder.
const OLD_CONFIG: &str = ".stacked.toml";

/// StackED's state of before, in the home folder.
const OLD_STATE: &str = "Library/Application Support/StackED/state.json";

/// Where stacked keeps what is yours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    /// The folder: `~/.stacked`, or `$STACKED_HOME`.
    pub dir: PathBuf,
    /// The settings: `config.toml` in the folder, or `$STACKED_CONFIG`.
    pub config: PathBuf,
    /// The state: `state.json` in the folder, or `$STACKED_APP_STATE`.
    pub state: PathBuf,
}

impl Home {
    /// The folder `dir`, with the settings and the state in it.
    pub fn at(dir: &Path) -> Home {
        Home {
            dir: dir.to_path_buf(),
            config: dir.join("config.toml"),
            state: dir.join("state.json"),
        }
    }

    /// Nowhere: no settings read but the defaults, and nothing kept.
    pub fn none() -> Home {
        let nowhere = PathBuf::from(NOWHERE);
        Home {
            dir: nowhere.clone(),
            config: nowhere.clone(),
            state: nowhere,
        }
    }

    /// Has the stacked that `c` runs keep what is yours where this one
    /// does: each place named in its environment, so that it works out
    /// none of its own, and moves nothing.
    pub fn pass_on(&self, c: &mut Command) {
        c.env("STACKED_HOME", &self.dir).env("STACKED_CONFIG", &self.config).env("STACKED_APP_STATE", &self.state);
    }
}

static SET: OnceLock<Home> = OnceLock::new();
static UNSET: LazyLock<Home> = LazyLock::new(Home::none);

/// From now on, `home` is where stacked keeps what is yours, unless that
/// was set already: by the binary, as it starts, before anything reads it.
/// Whether it is now.
pub fn set(home: Home) -> bool {
    SET.set(home).is_ok()
}

/// Where stacked keeps what is yours: as `set` set it, else nowhere.
pub fn get() -> &'static Home {
    SET.get().unwrap_or(&UNSET)
}

/// Where stacked keeps what is yours, as the environment `var` names it:
/// `$STACKED_CONFIG` and `$STACKED_APP_STATE` each a file, `$STACKED_HOME`
/// the folder of the others, else `.stacked` in `$HOME`; with neither
/// folder, those not named are nowhere. One set to nothing is not set, and
/// each is taken from where stacked runs. And what is to be said, by a run
/// of yours (`by_you`), of the files of before in `$HOME`: each moved into
/// the folder, unless an override names where it is to be or a file is in
/// its place already, which is never written over. A run that is not yours
/// moves nothing and says nothing: it reads a file of before where it is,
/// until a run of yours moves it.
pub fn resolve(var: impl Fn(&str) -> Option<OsString>, by_you: bool) -> (Home, Vec<String>) {
    let named = |k: &str| var(k).filter(|v| !v.is_empty()).map(|v| absolute(Path::new(&v)));
    let (folder, home) = (named("STACKED_HOME"), named("HOME"));
    let mut h = match (&folder, &home) {
        (Some(dir), _) => Home::at(dir),
        (None, Some(home)) => Home::at(&home.join(".stacked")),
        (None, None) => Home::none(),
    };
    let mut said = Vec::new();
    if folder.is_none()
        && let Some(home) = &home
    {
        if named("STACKED_CONFIG").is_none() {
            settle(&home.join(OLD_CONFIG), &mut h.config, by_you, Old::Settings, &mut said);
        }
        if named("STACKED_APP_STATE").is_none() {
            settle(&home.join(OLD_STATE), &mut h.state, by_you, Old::State, &mut said);
        }
    }
    if let Some(file) = named("STACKED_CONFIG") {
        h.config = file;
    }
    if let Some(file) = named("STACKED_APP_STATE") {
        h.state = file;
    }
    (h, said)
}

/// `p` from where stacked runs, if it is not from the root already.
fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// A file of before.
#[derive(Clone, Copy)]
enum Old {
    Settings,
    State,
}

impl Old {
    /// What is said once it is moved from `old` to `new`.
    fn moved(self, old: &Path, new: &Path) -> String {
        let (old, new) = (old.display(), new.display());
        match self {
            Old::Settings => format!("your settings were moved from {old} to {new}, where stacked reads them from now on"),
            Old::State => format!(
                "StackED's list of the databases opened last was moved from {old} to {new}, where StackED and stacked's editor keep it from now on"
            ),
        }
    }

    /// What is said while `old` is there, but `new` is read.
    fn unread(self, old: &Path, new: &Path) -> String {
        let (old, new) = (old.display(), new.display());
        match self {
            Old::Settings => format!(
                "{old} is no longer read, as stacked reads its settings from {new}: copy there what you still want from {old}, then delete it"
            ),
            Old::State => format!("{old} is no longer read, as StackED keeps the databases opened last in {new}: it can be deleted"),
        }
    }
}

/// `old`, a file of before, and `new`, its place in the folder now: if
/// `old` is there and nothing is in its place, moved there by a run of
/// yours, and said; read where it is by another run until then, `new` made
/// `old`. Once something is in its place, said to be read no longer, by a
/// run of yours.
fn settle(old: &Path, new: &mut PathBuf, by_you: bool, what: Old, said: &mut Vec<String>) {
    if !old.exists() {
        return;
    }
    if std::fs::symlink_metadata(&*new).is_ok() {
        if by_you {
            said.push(what.unread(old, new));
        }
        return;
    }
    if !by_you {
        *new = old.to_path_buf();
        return;
    }
    match move_file(old, new) {
        Ok(()) => {
            said.push(what.moved(old, new));
            // StackED's folder there, which held the state alone: removed,
            // if nothing else is in it.
            if let (Old::State, Some(dir)) = (what, old.parent()) {
                let _ = std::fs::remove_dir(dir);
            }
        }
        // Made meanwhile, by another run: never written over.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => said.push(what.unread(old, new)),
        Err(e) => {
            said.push(format!("{} cannot be moved to {} ({e}), so it is read where it is", old.display(), new.display()));
            *new = old.to_path_buf();
        }
    }
}

/// `old` moved to `new`, its folder made if need be, but never over a file
/// there (an error, `AlreadyExists`). A file keeps its permissions, as what
/// it holds may be secret (a token); a link stays a link, to the same file.
fn move_file(old: &Path, new: &Path) -> std::io::Result<()> {
    if let Some(dir) = new.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::symlink_metadata(old)?.file_type().is_symlink() {
        // Taken from the link's own folder, as the link was read there.
        let to = std::fs::read_link(old)?;
        let to = old.parent().map_or_else(|| to.clone(), |dir| dir.join(&to));
        std::os::unix::fs::symlink(&to, new)?;
    } else {
        std::fs::hard_link(old, new)?;
    }
    std::fs::remove_file(old)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An environment of `pairs` alone.
    fn env(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, OsString)> = pairs.iter().map(|(k, v)| (k.to_string(), v.as_os_str().to_owned())).collect();
        move |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    /// The names under `dir`, each with what its file holds (a link, where
    /// it goes): all there is, to see that nothing else was made.
    fn listing(dir: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                let name = p.strip_prefix(dir).unwrap().display().to_string();
                let kind = e.file_type().unwrap();
                if kind.is_symlink() {
                    out.push((name, format!("-> {}", std::fs::read_link(&p).unwrap().display())));
                } else if kind.is_dir() {
                    out.push((name, "/".into()));
                    todo.push(p);
                } else {
                    out.push((name, std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    /// A home of `files` (path, then what it holds), under `tmp`.
    fn home_of(tmp: &Path, files: &[(&str, &str)]) -> PathBuf {
        let home = tmp.join("home");
        std::fs::create_dir_all(&home).unwrap();
        for (path, text) in files {
            let p = home.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        home
    }

    #[test]
    fn the_folder_is_the_home_s_unless_one_is_named_and_a_file_named_is_that_one() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home_of(tmp.path(), &[]);
        let at = |pairs: &[(&str, &Path)]| {
            let (h, said) = resolve(env(pairs), true);
            assert!(said.is_empty(), "{said:?}");
            h
        };
        // ~/.stacked, its files in it; nothing made for it.
        assert_eq!(at(&[("HOME", &home)]), Home::at(&home.join(".stacked")));
        assert!(listing(&home).is_empty(), "{:?}", listing(&home));
        // The folder $STACKED_HOME names, from where stacked runs.
        let elsewhere = tmp.path().join("elsewhere");
        assert_eq!(at(&[("HOME", &home), ("STACKED_HOME", &elsewhere)]), Home::at(&elsewhere));
        let here = std::env::current_dir().unwrap();
        assert_eq!(at(&[("HOME", &home), ("STACKED_HOME", Path::new("mine"))]), Home::at(&here.join("mine")));
        // A file named is that one, the other in the folder.
        let (settings, state) = (tmp.path().join("settings.toml"), tmp.path().join("kept.json"));
        let h = at(&[("HOME", &home), ("STACKED_CONFIG", &settings)]);
        assert_eq!((h.config, h.state), (settings.clone(), home.join(".stacked/state.json")));
        let h = at(&[("HOME", &home), ("STACKED_HOME", &elsewhere), ("STACKED_APP_STATE", &state)]);
        assert_eq!((h.dir, h.config, h.state), (elsewhere.clone(), elsewhere.join("config.toml"), state.clone()));
        // Set to nothing: not set.
        let empty = Path::new("");
        let h = at(&[("HOME", &home), ("STACKED_HOME", empty), ("STACKED_CONFIG", empty), ("STACKED_APP_STATE", empty)]);
        assert_eq!(h, Home::at(&home.join(".stacked")));
        // No folder at all: nowhere, but what is named.
        assert_eq!(at(&[]), Home::none());
        let h = at(&[("HOME", empty), ("STACKED_CONFIG", &settings)]);
        assert_eq!((h.config, h.state), (settings, PathBuf::from(NOWHERE)));
    }

    #[test]
    fn the_files_of_before_are_moved_into_the_folder_once_by_a_run_of_yours_and_said() {
        let tmp = tempfile::tempdir().unwrap();
        let state = r#"{"recent": ["/work/a.jsonl"]}"#;
        let home = home_of(tmp.path(), &[(OLD_CONFIG, "editor = \"hx\"\n"), (OLD_STATE, state)]);
        let new = Home::at(&home.join(".stacked"));
        // As a stacked that stacked runs: read where they are, nothing
        // moved, nothing said.
        let before = listing(&home);
        let (h, said) = resolve(env(&[("HOME", &home)]), false);
        assert_eq!((h.config, h.state), (home.join(OLD_CONFIG), home.join(OLD_STATE)));
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(listing(&home), before);
        // A run of yours: each moved, as it was, and said.
        let (h, said) = resolve(env(&[("HOME", &home)]), true);
        assert_eq!(h, new);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].starts_with(&format!("your settings were moved from {} to {}", home.join(OLD_CONFIG).display(), new.config.display())), "{said:?}");
        assert!(said[1].contains(&format!("databases opened last was moved from {} to {}", home.join(OLD_STATE).display(), new.state.display())), "{said:?}");
        assert_eq!(std::fs::read_to_string(&new.config).unwrap(), "editor = \"hx\"\n");
        assert_eq!(std::fs::read_to_string(&new.state).unwrap(), state);
        // Nothing left where they were: StackED's folder, empty, gone too.
        let now = listing(&home);
        let names: Vec<&str> = now.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, [".stacked", ".stacked/config.toml", ".stacked/state.json", "Library", "Library/Application Support"]);
        // Done once: the next run says nothing, and changes nothing.
        let (h, said) = resolve(env(&[("HOME", &home)]), true);
        assert_eq!(h, new);
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(listing(&home), now);
    }

    #[test]
    fn a_file_in_the_folder_already_is_never_written_over_the_other_said_to_be_read_no_longer() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home_of(
            tmp.path(),
            &[
                (OLD_CONFIG, "editor = \"old\"\n"),
                (OLD_STATE, "{}"),
                (".stacked/config.toml", "editor = \"new\"\n"),
                (".stacked/state.json", r#"{"recent": []}"#),
            ],
        );
        let before = listing(&home);
        let (h, said) = resolve(env(&[("HOME", &home)]), true);
        assert_eq!(h, Home::at(&home.join(".stacked")), "the folder's read");
        assert_eq!(listing(&home), before, "neither written over, nor moved");
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].starts_with(&format!("{} is no longer read, as stacked reads its settings from {}", home.join(OLD_CONFIG).display(), h.config.display())), "{said:?}");
        assert!(said[1].starts_with(&format!("{} is no longer read", home.join(OLD_STATE).display())), "{said:?}");
        // Said each time, while both are there; by a run of yours alone.
        assert_eq!(resolve(env(&[("HOME", &home)]), true).1, said);
        assert!(resolve(env(&[("HOME", &home)]), false).1.is_empty());
    }

    #[test]
    fn nothing_is_moved_that_an_override_names_the_place_of() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home_of(tmp.path(), &[(OLD_CONFIG, "editor = \"hx\"\n"), (OLD_STATE, "{}")]);
        let before = listing(&home);
        // The folder named: nothing of the home's moved, nothing said.
        let elsewhere = tmp.path().join("elsewhere");
        let (h, said) = resolve(env(&[("HOME", &home), ("STACKED_HOME", &elsewhere)]), true);
        assert_eq!(h, Home::at(&elsewhere));
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(listing(&home), before);
        assert!(!elsewhere.exists());
        // The settings named: those of before stay; the state moves.
        let settings = tmp.path().join("settings.toml");
        let (h, said) = resolve(env(&[("HOME", &home), ("STACKED_CONFIG", &settings)]), true);
        assert_eq!((h.config, h.state.clone()), (settings, home.join(".stacked/state.json")));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(home.join(OLD_CONFIG).exists() && h.state.exists() && !home.join(OLD_STATE).exists());
    }

    #[test]
    fn a_link_of_before_is_moved_as_a_link_to_the_same_file() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home_of(tmp.path(), &[("dotfiles/stacked.toml", "editor = \"hx\"\n")]);
        // As the home's own folder reads it.
        std::os::unix::fs::symlink("dotfiles/stacked.toml", home.join(OLD_CONFIG)).unwrap();
        let (h, said) = resolve(env(&[("HOME", &home)]), true);
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(std::fs::read_link(&h.config).unwrap(), home.join("dotfiles/stacked.toml"));
        assert_eq!(std::fs::read_to_string(&h.config).unwrap(), "editor = \"hx\"\n");
        assert!(std::fs::symlink_metadata(home.join(OLD_CONFIG)).is_err(), "the link of before gone");
    }

    #[test]
    fn what_stacked_runs_is_told_the_places_and_works_out_none_of_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        // A home with files of before, which the run told would move were
        // it told nothing.
        let home = home_of(tmp.path(), &[(OLD_CONFIG, "editor = \"hx\"\n")]);
        let before = listing(&home);
        for given in [Home::at(&tmp.path().join("folder")), Home::none()] {
            let mut c = Command::new("true");
            given.pass_on(&mut c);
            let told: Vec<(String, OsString)> =
                c.get_envs().filter_map(|(k, v)| Some((k.to_string_lossy().into_owned(), v?.to_owned()))).collect();
            let var = |k: &str| match k {
                "HOME" => Some(home.clone().into_os_string()),
                _ => told.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
            };
            let (h, said) = resolve(var, true);
            assert_eq!(h, given);
            assert!(said.is_empty(), "{said:?}");
            assert_eq!(listing(&home), before);
        }
    }

    #[test]
    fn set_by_no_one_the_places_are_nowhere_and_no_settings_are_read() {
        // As in every test's own process: nothing under $HOME, whatever it
        // holds.
        assert_eq!(*get(), Home::none());
        assert_eq!(crate::config::path(), Path::new(NOWHERE));
        let (c, wrong) = crate::config::Config::load(&crate::config::path());
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.render(), crate::config::Config::default().render());
    }
}
