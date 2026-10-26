//! What StackED keeps between runs: the databases opened last, for File →
//! Open Recent, and those whose windows StackED.app has open, to open again
//! when it next starts. A file of JSON, `state.json` in ~/Library/Application
//! Support/StackED, or the file `$STACKED_APP_STATE` names (the tests'
//! own); never the settings' file. A database no longer there is taken off
//! both lists whenever the file is read.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many databases File → Open Recent lists.
pub const RECENT: usize = 8;

/// What the file holds.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct State {
    /// The databases opened, the most recent first.
    #[serde(default)]
    recent: Vec<PathBuf>,
    /// The databases of the windows StackED.app has open, kept as they open
    /// and close, and as it is quit.
    #[serde(default)]
    open: Vec<PathBuf>,
}

/// Where the state is kept: `$STACKED_APP_STATE`, else state.json in
/// ~/Library/Application Support/StackED.
pub fn path() -> PathBuf {
    path_of(std::env::var_os("STACKED_APP_STATE"), std::env::var_os("HOME"))
}

/// Where the state is kept, `$STACKED_APP_STATE` being `named` and `$HOME`
/// being `home`.
fn path_of(named: Option<OsString>, home: Option<OsString>) -> PathBuf {
    match named {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(home.unwrap_or_default()).join("Library/Application Support/StackED/state.json"),
    }
}

impl State {
    /// The state kept in `file`: none if there is no file, or it cannot be
    /// read.
    fn load(file: &Path) -> State {
        std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    /// Kept in `file`, its folder made if need be: written whole beside
    /// it, then renamed over it, so that the file is never seen half
    /// written. Not kept if it cannot be: StackED runs on all the same.
    fn save(&self, file: &Path) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        let mut part = file.as_os_str().to_owned();
        part.push(format!(".{}", std::process::id()));
        if std::fs::write(&part, text + "\n").is_ok() && std::fs::rename(&part, file).is_err() {
            let _ = std::fs::remove_file(&part);
        }
    }
}

/// The state in `file` changed by `change`, the databases no longer there
/// taken off both lists first, and kept if any of it changed: what
/// `change` gives back.
fn change<T>(file: &Path, change: impl FnOnce(&mut State) -> T) -> T {
    let was = State::load(file);
    let mut state = was.clone();
    state.recent.retain(|p| p.is_file());
    state.open.retain(|p| p.is_file());
    let out = change(&mut state);
    if state != was {
        state.save(file);
    }
    out
}

/// The databases opened most recently, the most recent first, for File →
/// Open Recent: those no longer there are left out, and taken off the list.
pub fn recent(file: &Path) -> Vec<PathBuf> {
    change(file, |s| s.recent.clone())
}

/// `db` opened: first on the list of recent databases, and there once; the
/// list no longer than `RECENT`.
pub fn opened(file: &Path, db: &Path) {
    change(file, |s| {
        s.recent.retain(|p| p != db);
        s.recent.insert(0, db.to_path_buf());
        s.recent.truncate(RECENT);
    })
}

/// File → Open Recent → Clear Menu: no database is recent.
pub fn clear(file: &Path) {
    change(file, |s| s.recent.clear())
}

/// StackED.app's windows are now on `dbs`: kept, to open them again when
/// it next starts.
pub fn keep_open(file: &Path, dbs: &[PathBuf]) {
    change(file, |s| s.open = dbs.to_vec())
}

/// The databases of the windows StackED.app had open when it was last
/// quit, to open again: those no longer there are left out, and taken off
/// both lists.
pub fn reopen(file: &Path) -> Vec<PathBuf> {
    change(file, |s| s.open.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Databases named `names` in `dir`, made there.
    fn dbs(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| {
                let p = dir.join(n);
                std::fs::write(&p, "").unwrap();
                p
            })
            .collect()
    }

    /// What the file says, read as it is.
    fn kept(file: &Path) -> State {
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    #[test]
    fn the_state_is_kept_where_the_tests_say_else_in_application_support() {
        assert_eq!(path_of(Some("/tmp/x.json".into()), Some("/Users/me".into())), Path::new("/tmp/x.json"));
        let default = Path::new("/Users/me/Library/Application Support/StackED/state.json");
        assert_eq!(path_of(None, Some("/Users/me".into())), default);
        assert_eq!(path_of(Some("".into()), Some("/Users/me".into())), default, "set to nothing");
    }

    #[test]
    fn the_recent_databases_are_the_latest_first_once_each_and_eight_at_most() {
        let tmp = tempfile::tempdir().unwrap();
        // In folders not yet there: made.
        let file = tmp.path().join("Application Support/StackED/state.json");
        assert_eq!(recent(&file), Vec::<PathBuf>::new());
        assert!(!file.exists(), "nothing written for nothing changed");
        let d = dbs(tmp.path(), &["a.jsonl", "b.jsonl", "c.jsonl"]);
        for db in &d {
            opened(&file, db);
        }
        assert_eq!(recent(&file), [d[2].clone(), d[1].clone(), d[0].clone()]);
        // Opened again: first, and once.
        opened(&file, &d[0]);
        assert_eq!(recent(&file), [d[0].clone(), d[2].clone(), d[1].clone()]);
        // Ten opened: the latest eight.
        let more = dbs(tmp.path(), &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]);
        for db in &more {
            opened(&file, db);
        }
        let mut latest: Vec<PathBuf> = more[2..].to_vec();
        latest.reverse();
        assert_eq!(recent(&file), latest);
        assert_eq!(kept(&file).recent.len(), RECENT);
        // Cleared: none, in the file too.
        clear(&file);
        assert_eq!(recent(&file), Vec::<PathBuf>::new());
        assert!(kept(&file).recent.is_empty());
    }

    #[test]
    fn a_database_no_longer_there_is_taken_off_the_lists_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("state.json");
        let d = dbs(tmp.path(), &["a.jsonl", "b.jsonl", "c.jsonl"]);
        for db in &d {
            opened(&file, db);
        }
        keep_open(&file, &d);
        std::fs::remove_file(&d[1]).unwrap();
        // Left out of what is read, and of the file.
        assert_eq!(recent(&file), [d[2].clone(), d[0].clone()]);
        assert_eq!(kept(&file).recent, [d[2].clone(), d[0].clone()]);
        assert_eq!(kept(&file).open, [d[0].clone(), d[2].clone()], "from the windows to open again too");
        // As StackED.app starts: the windows open again but the one gone,
        // which is kept on neither list.
        std::fs::remove_file(&d[2]).unwrap();
        assert_eq!(reopen(&file), [d[0].clone()]);
        assert_eq!(kept(&file), State { recent: vec![d[0].clone()], open: vec![d[0].clone()] });
        // The windows closed: none to open again.
        keep_open(&file, &[]);
        assert_eq!(reopen(&file), Vec::<PathBuf>::new());
        assert_eq!(recent(&file), [d[0].clone()], "still recent");
    }

    #[test]
    fn a_state_file_that_cannot_be_read_is_none_and_written_anew() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("state.json");
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(recent(&file), Vec::<PathBuf>::new());
        let d = dbs(tmp.path(), &["a.jsonl"]);
        opened(&file, &d[0]);
        assert_eq!(kept(&file).recent, d);
        // Nothing left beside it.
        let names: Vec<String> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }
}
