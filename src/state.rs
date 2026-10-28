//! What stacked keeps between runs, besides the settings: the databases
//! opened last, for StackED's File → Open Recent, which stacked's editor
//! adds to as well (in a terminal, in a browser); those whose windows
//! StackED.app has open, to open again when it next starts; and the
//! searches made in the editor's middle with `/`, for its next runs. A
//! file of JSON, `state.json` in stacked's folder (`home`); never the
//! settings' file. A database no longer there is taken off both lists
//! whenever the file is read.
//!
//! StackED and a terminal's stacked may change the file at once. Each
//! change is made under a lock, on the file `state.json.lock` beside it,
//! and to the file as it is then, so that neither loses what the other did;
//! the file is written whole beside it, then renamed over it, so that it is
//! never seen half written. A file of another kind (the null device, which
//! the tests' own processes have) keeps nothing.

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many databases File → Open Recent lists.
pub const RECENT: usize = 8;

/// How many searches of the middle are kept, as self-review keeps.
pub const SEARCHES: usize = 20;

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
    /// The searches made in the editor's middle with `/`, the latest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    searches: Vec<String>,
}

impl State {
    /// The state kept in `file`: none if there is no file, or it cannot be
    /// read.
    fn load(file: &Path) -> State {
        std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    /// Kept in `file`, its folder made if need be: written whole beside
    /// it, then renamed over it, so that the file is never seen half
    /// written. Not kept if it cannot be: stacked runs on all the same.
    fn save(&self, file: &Path) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        let mut part = file.as_os_str().to_owned();
        part.push(format!(".{}", std::process::id()));
        if std::fs::write(&part, text + "\n").and_then(|_| std::fs::rename(&part, file)).is_err() {
            let _ = std::fs::remove_file(&part);
        }
    }
}

/// The lock on the state kept in a file, held until dropped.
struct Lock(std::fs::File);

impl Lock {
    /// The lock on the state kept in `file`: the file `file.lock` beside it
    /// locked, made if it is not there, as is its folder; waited for while
    /// another holds it. None if it cannot be had: the state is then
    /// changed unlocked, as an older stacked changed it.
    fn take(file: &Path) -> Option<Lock> {
        let mut name = file.as_os_str().to_owned();
        name.push(".lock");
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let f = std::fs::File::options().create(true).truncate(false).write(true).open(PathBuf::from(name)).ok()?;
        (unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0).then_some(Lock(f))
    }
}

impl Drop for Lock {
    /// Given back at once, rather than when the last copy of the file is
    /// closed, which a child forked meanwhile could hold.
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// The state in `file` as it is, the databases no longer there taken off
/// both lists, then changed by `change`: what it was, what it is now, and
/// what `change` gives back.
fn read<T>(file: &Path, change: &mut impl FnMut(&mut State) -> T) -> (State, State, T) {
    let was = State::load(file);
    let mut state = was.clone();
    state.recent.retain(|p| p.is_file());
    state.open.retain(|p| p.is_file());
    let out = change(&mut state);
    (was, state, out)
}

/// The state in `file` changed by `change`, the databases no longer there
/// taken off both lists first, and kept if any of it changed: what `change`
/// gives back. A change is made again under the lock, to the state as it is
/// then, and kept; what changes nothing is neither locked nor written. A
/// file of another kind (the null device) is read as none, and keeps
/// nothing.
fn change<T>(file: &Path, mut change: impl FnMut(&mut State) -> T) -> T {
    if std::fs::metadata(file).is_ok_and(|m| !m.is_file()) {
        return change(&mut State::default());
    }
    let (was, state, out) = read(file, &mut change);
    if state == was {
        return out;
    }
    let _lock = Lock::take(file);
    let (was, state, out) = read(file, &mut change);
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

/// `db` opened: first on the list of recent databases, from the root, and
/// there once; the list no longer than `RECENT`.
pub fn opened(file: &Path, db: &Path) {
    let db = std::path::absolute(db).unwrap_or_else(|_| db.to_path_buf());
    change(file, |s| {
        s.recent.retain(|p| *p != db);
        s.recent.insert(0, db.clone());
        s.recent.truncate(RECENT);
    })
}

/// `db` opened by stacked's editor, in a terminal or a browser: first on
/// the list of recent databases kept where stacked keeps its state
/// (`home`), for StackED's File → Open Recent, if it is a file on disk.
/// One not there yet (the editor makes it on its first write) is not, nor
/// one kept in memory (the tour's).
pub fn opened_in_editor(db: &Path) {
    #[cfg(feature = "demo")]
    if crate::memory::is_file(db) {
        return;
    }
    if db.is_file() {
        opened(&crate::home::get().state, db);
    }
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

/// The searches made in the editor's middle with `/`, the latest first.
pub fn searches(file: &Path) -> Vec<String> {
    change(file, |s| s.searches.clone())
}

/// `query` searched in the middle: first among the searches kept, and
/// there once; no more than `SEARCHES` of them.
pub fn searched(file: &Path, query: &str) {
    change(file, |s| {
        s.searches.retain(|q| q != query);
        s.searches.insert(0, query.to_string());
        s.searches.truncate(SEARCHES);
    })
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

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        out.sort();
        out
    }

    #[test]
    fn the_recent_databases_are_the_latest_first_once_each_and_eight_at_most() {
        let tmp = tempfile::tempdir().unwrap();
        // In folders not yet there: made.
        let file = tmp.path().join("Application Support/StackED/state.json");
        assert_eq!(recent(&file), Vec::<PathBuf>::new());
        assert!(!file.exists() && !file.parent().unwrap().exists(), "nothing written, nor locked, for nothing changed");
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
    fn the_searches_of_the_middle_are_the_latest_first_once_each_and_twenty_at_most() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("state.json");
        assert!(searches(&file).is_empty());
        let d = dbs(tmp.path(), &["a.jsonl"]);
        opened(&file, &d[0]);
        for q in ["two", "line 3", "two"] {
            searched(&file, q);
        }
        assert_eq!(searches(&file), ["two", "line 3"]);
        // Kept beside the databases opened, each as it was.
        assert_eq!(recent(&file), d);
        opened(&file, &d[0]);
        assert_eq!(kept(&file).searches, ["two", "line 3"]);
        // Thirty searched: the latest twenty.
        for n in 0..30 {
            searched(&file, &n.to_string());
        }
        let latest: Vec<String> = (10..30).rev().map(|n| n.to_string()).collect();
        assert_eq!(searches(&file), latest);
        assert_eq!(kept(&file).searches.len(), SEARCHES);
        // A file of another kind keeps none.
        let null = Path::new(crate::home::NOWHERE);
        searched(null, "two");
        assert!(searches(null).is_empty());
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
        assert_eq!(kept(&file), State { recent: vec![d[0].clone()], open: vec![d[0].clone()], ..Default::default() });
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
        // Nothing left beside it but its lock.
        assert_eq!(names(tmp.path()), ["a.jsonl", "state.json", "state.json.lock"]);
    }

    #[test]
    fn two_writing_at_once_keep_what_each_wrote() {
        // Each writer takes the lock anew for each change, by a file of its
        // own opened, as another process does: StackED, and a terminal's
        // stacked. All start at once, on a list that is not there yet, again
        // and again.
        for _ in 0..20 {
            let tmp = tempfile::tempdir().unwrap();
            let file = tmp.path().join("state.json");
            let d = dbs(tmp.path(), &["a.jsonl", "b.jsonl", "c.jsonl", "d.jsonl", "e.jsonl", "f.jsonl"]);
            let start = std::sync::Arc::new(std::sync::Barrier::new(d.len()));
            let writers: Vec<_> = d
                .iter()
                .map(|db| {
                    let (file, start, db) = (file.clone(), start.clone(), db.clone());
                    std::thread::spawn(move || {
                        start.wait();
                        opened(&file, &db);
                    })
                })
                .collect();
            for w in writers {
                w.join().unwrap();
            }
            let mut kept = recent(&file);
            kept.sort();
            assert_eq!(kept, d, "each database opened is on the list");
            assert_eq!(names(tmp.path()).iter().filter(|n| n.starts_with("state.json.")).count(), 1, "no part left, the lock alone");
        }
    }

    #[test]
    fn a_state_file_of_another_kind_keeps_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dbs(tmp.path(), &["a.jsonl"]);
        let null = Path::new(crate::home::NOWHERE);
        opened(null, &d[0]);
        keep_open(null, &d);
        assert!(recent(null).is_empty() && reopen(null).is_empty());
        assert!(!Path::new("/dev/null.lock").exists());
        // A folder where the file would be: the same.
        opened(tmp.path(), &d[0]);
        assert_eq!(names(tmp.path()), ["a.jsonl"]);
    }

    #[test]
    fn the_editor_adds_its_database_where_stacked_keeps_its_state() {
        // Where nothing set where that is, as here: nowhere, and nothing
        // made beside the database.
        let tmp = tempfile::tempdir().unwrap();
        let d = dbs(tmp.path(), &["a.jsonl"]);
        opened_in_editor(&d[0]);
        opened_in_editor(&tmp.path().join("not-yet.jsonl"));
        assert_eq!(crate::home::get().state, Path::new(crate::home::NOWHERE));
        assert_eq!(names(tmp.path()), ["a.jsonl"]);
    }
}
