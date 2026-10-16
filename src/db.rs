//! The database: one file of JSON lines, appended to and never rewritten.
//!
//! A line is a record. A patch record is stored once and never changes; an
//! edited patch is a new record sharing its `change`. A stack record is the
//! whole stack as it stood after an edit, so a stack's history is its
//! records, oldest first, and undoing an edit appends a record too. A record
//! is named by its line number, its rev (`@12`).

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Record {
    /// The first line of every file.
    Stacked { version: u32 },
    Patch(Patch),
    Stack(Stack),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Patch {
    /// The sha1 of everything below: the same patch is only stored once.
    pub id: String,
    /// Shared by every version of a patch.
    pub change: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// The title, a blank line and the body.
    pub message: String,
    /// A unified diff, as `git diff` writes it.
    pub diff: String,
}

impl Patch {
    pub fn new(
        change: String,
        author: Option<String>,
        date: Option<String>,
        message: String,
        diff: String,
    ) -> Patch {
        let content = serde_json::to_string(&(&change, &author, &date, &message, &diff)).unwrap();
        Patch {
            id: sha1_smol::Sha1::from(content).digest().to_string(),
            change,
            author,
            date,
            message,
            diff,
        }
    }

    pub fn title(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }

    /// What follows the title, without the blank line between them.
    pub fn body(&self) -> &str {
        match self.message.split_once('\n') {
            Some((_, rest)) => rest.trim_matches('\n'),
            None => "",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Stack {
    pub name: String,
    /// Patch ids, bottom first: each applies on top of the ones before it.
    pub patches: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    /// The rev this edit was made from: the stack's head at the time, or,
    /// for an undo, what that head was made from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// What the edit was, for the log.
    pub op: String,
    /// Seconds since the epoch.
    pub time: i64,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A new change id: twelve letters from k to z, so it is never taken for a
/// patch id, which is hex.
pub fn new_change(seed: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let hex = sha1_smol::Sha1::from(format!("{nanos} {} {n} {seed}", std::process::id())).digest().to_string();
    hex.chars()
        .take(12)
        .map(|c| (b'k' + c.to_digit(16).unwrap() as u8) as char)
        .collect()
}

pub struct Db {
    pub path: PathBuf,
    /// `records[i]` is rev `i + 1`.
    pub records: Vec<Record>,
    patches: HashMap<String, usize>,
    /// Every stack ever named, deleted or not, to its latest record.
    heads: BTreeMap<String, usize>,
    /// The bytes of complete lines read.
    len: u64,
    /// Records from here on are not yet on disk.
    saved: usize,
}

/// The database's lock, held until dropped.
pub struct Lock(File);

impl Db {
    /// Reads `path`; a missing file is an empty database.
    pub fn load(path: &Path) -> Result<Db> {
        let mut bytes = Vec::new();
        match File::open(path) {
            Ok(mut f) => {
                f.read_to_end(&mut bytes)
                    .map_err(|e| Error(format!("{}: {e}", path.display())))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return err(format!("{}: {e}", path.display())),
        }
        Db::parse(path, &bytes)
    }

    fn parse(path: &Path, bytes: &[u8]) -> Result<Db> {
        let mut db = Db {
            path: path.to_path_buf(),
            records: Vec::new(),
            patches: HashMap::new(),
            heads: BTreeMap::new(),
            len: 0,
            saved: 0,
        };
        // A last line with no newline is a write cut short: it is not read,
        // and the next write drops it.
        let complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        db.len = complete as u64;
        let text = std::str::from_utf8(&bytes[..complete])
            .map_err(|e| Error(format!("{}: not UTF-8: {e}", path.display())))?;
        for (i, line) in text.lines().enumerate() {
            let rec: Record = serde_json::from_str(line)
                .map_err(|e| Error(format!("{}:{}: {e}", path.display(), i + 1)))?;
            match (&rec, i) {
                (Record::Stacked { version }, 0) if *version <= VERSION => {}
                (Record::Stacked { version }, 0) => {
                    return err(format!(
                        "{}: written by a newer stacked (format {version}, this one reads {VERSION})",
                        path.display()
                    ));
                }
                (_, 0) => return err(format!("{}: not a stacked database", path.display())),
                (Record::Stacked { .. }, _) => {
                    return err(format!("{}:{}: a second header", path.display(), i + 1));
                }
                _ => {}
            }
            db.index(rec)
                .map_err(|e| Error(format!("{}:{}: {e}", path.display(), i + 1)))?;
        }
        db.saved = db.records.len();
        Ok(db)
    }

    fn index(&mut self, rec: Record) -> Result<()> {
        let at = self.records.len();
        match &rec {
            Record::Stacked { .. } => {}
            Record::Patch(p) => {
                self.patches.entry(p.id.clone()).or_insert(at);
            }
            Record::Stack(s) => {
                if let Some(id) = s.patches.iter().find(|id| !self.patches.contains_key(*id)) {
                    return err(format!("stack {} names patch {id}, which is not stored", s.name));
                }
                self.heads.insert(s.name.clone(), at);
            }
        }
        self.records.push(rec);
        Ok(())
    }

    /// Takes the lock on `path`, then reads it: what is read stays true
    /// until the lock is dropped, so an edit made from it loses no one
    /// else's.
    pub fn lock(path: &Path) -> Result<(Db, Lock)> {
        let f = File::options()
            .create(true)
            .append(true)
            .read(true)
            .open(path)
            .map_err(|e| Error(format!("{}: {e}", path.display())))?;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return err(format!(
                "{}: cannot lock: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        let mut lock = Lock(f);
        let mut bytes = Vec::new();
        lock.0
            .read_to_end(&mut bytes)
            .map_err(|e| Error(format!("{}: {e}", path.display())))?;
        Ok((Db::parse(path, &bytes)?, lock))
    }

    /// Runs `edit` on the database as it is on disk, under its lock, and
    /// appends the records it added.
    pub fn edit<T>(path: &Path, edit: impl FnOnce(&mut Db) -> Result<T>) -> Result<T> {
        let (mut db, mut lock) = Db::lock(path)?;
        let out = edit(&mut db)?;
        db.save(&mut lock)?;
        Ok(out)
    }

    /// Appends the records added since the database was read.
    pub fn save(&mut self, lock: &mut Lock) -> Result<()> {
        if self.saved == self.records.len() {
            return Ok(());
        }
        let mut out = String::new();
        if self.len == 0 && self.saved == 0 {
            out.push_str(&serde_json::to_string(&Record::Stacked { version: VERSION }).unwrap());
            out.push('\n');
        }
        for rec in &self.records[self.saved..] {
            if matches!(rec, Record::Stacked { .. }) {
                continue;
            }
            out.push_str(&serde_json::to_string(rec).unwrap());
            out.push('\n');
        }
        let io = |e: std::io::Error| Error(format!("{}: {e}", self.path.display()));
        lock.0.set_len(self.len).map_err(io)?;
        lock.0.write_all(out.as_bytes()).map_err(io)?;
        lock.0.sync_data().map_err(io)?;
        self.len += out.len() as u64;
        self.saved = self.records.len();
        Ok(())
    }

    fn push(&mut self, rec: Record) -> Result<usize> {
        if self.records.is_empty() {
            self.records.push(Record::Stacked { version: VERSION });
        }
        self.index(rec)?;
        Ok(self.records.len())
    }

    /// The record at `rev`.
    pub fn rev(&self, rev: usize) -> Option<&Record> {
        rev.checked_sub(1).and_then(|i| self.records.get(i))
    }

    pub fn patch(&self, id: &str) -> &Patch {
        match &self.records[self.patches[id]] {
            Record::Patch(p) => p,
            _ => unreachable!(),
        }
    }

    /// Stores `p` unless it already is; either way, its id.
    pub fn add_patch(&mut self, p: Patch) -> Result<String> {
        let id = p.id.clone();
        if !self.patches.contains_key(&id) {
            self.push(Record::Patch(p))?;
        }
        Ok(id)
    }

    /// The patch whose id starts with `prefix`.
    pub fn find_patch(&self, prefix: &str) -> Result<&Patch> {
        let mut found = self.patches.keys().filter(|id| id.starts_with(prefix));
        match (found.next(), found.next()) {
            (Some(id), None) if !prefix.is_empty() => Ok(self.patch(id)),
            (Some(_), Some(_)) => err(format!("patch {prefix} is ambiguous")),
            _ => err(format!("no patch {prefix}")),
        }
    }

    /// Every version of `change`, oldest first.
    pub fn versions(&self, change: &str) -> Vec<&Patch> {
        self.records
            .iter()
            .filter_map(|r| match r {
                Record::Patch(p) if p.change == change => Some(p),
                _ => None,
            })
            .collect()
    }

    /// The stack's latest record and its rev, deleted or not.
    pub fn head(&self, name: &str) -> Option<(usize, &Stack)> {
        let at = *self.heads.get(name)?;
        match &self.records[at] {
            Record::Stack(s) => Some((at + 1, s)),
            _ => unreachable!(),
        }
    }

    pub fn stack(&self, name: &str) -> Result<&Stack> {
        match self.head(name) {
            Some((_, s)) if !s.deleted => Ok(s),
            Some(_) => err(format!("stack {name} is deleted")),
            None => err(format!("no stack {name}")),
        }
    }

    /// The stacks not deleted, by name.
    pub fn stacks(&self) -> Vec<&Stack> {
        self.heads
            .keys()
            .filter_map(|name| self.head(name))
            .filter(|(_, s)| !s.deleted)
            .map(|(_, s)| s)
            .collect()
    }

    /// Every stack record, or `name`'s, with its rev, oldest first.
    pub fn log(&self, name: Option<&str>) -> Vec<(usize, &Stack)> {
        self.records
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                Record::Stack(s) if name.is_none_or(|n| n == s.name) => Some((i + 1, s)),
                _ => None,
            })
            .collect()
    }

    /// Records `name` as holding `patches` now. Nothing is written if that
    /// is what it holds already; the rev of its head either way.
    pub fn set_stack(&mut self, name: &str, patches: Vec<String>, op: &str) -> Result<usize> {
        let parent = self.head(name);
        if let Some((rev, s)) = parent
            && !s.deleted
            && s.patches == patches
        {
            return Ok(rev);
        }
        check_name(name)?;
        self.push(Record::Stack(Stack {
            name: name.into(),
            patches,
            deleted: false,
            parent: parent.map(|(rev, _)| rev),
            op: op.into(),
            time: now(),
        }))
    }

    pub fn new_stack(&mut self, name: &str) -> Result<usize> {
        if self.stack(name).is_ok() {
            return err(format!("stack {name} already exists"));
        }
        check_name(name)?;
        let parent = self.head(name).map(|(rev, _)| rev);
        self.push(Record::Stack(Stack {
            name: name.into(),
            patches: vec![],
            deleted: false,
            parent,
            op: "new".into(),
            time: now(),
        }))
    }

    pub fn delete_stack(&mut self, name: &str) -> Result<usize> {
        self.stack(name)?;
        let (rev, _) = self.head(name).unwrap();
        self.push(Record::Stack(Stack {
            name: name.into(),
            patches: vec![],
            deleted: true,
            parent: Some(rev),
            op: "delete".into(),
            time: now(),
        }))
    }

    /// Puts `name` back as it was before its last edit. Undoing again goes
    /// further back: the record made is a copy of the one before, made from
    /// what that one was made from.
    pub fn undo(&mut self, name: &str) -> Result<usize> {
        let Some((rev, head)) = self.head(name) else {
            return err(format!("no stack {name}"));
        };
        let Some(before) = head.parent else {
            return err(format!("nothing to undo: {name} was made at @{rev}"));
        };
        let Some(Record::Stack(was)) = self.rev(before) else {
            unreachable!()
        };
        let rec = Stack {
            op: format!("undo {}", head.op),
            parent: was.parent,
            time: now(),
            ..was.clone()
        };
        self.push(Record::Stack(rec))
    }

    /// Puts `name` back as it was at `rev`.
    pub fn restore(&mut self, name: &str, rev: usize) -> Result<usize> {
        let Some(Record::Stack(was)) = self.rev(rev) else {
            return err(format!("@{rev} is not a stack"));
        };
        if was.name != name {
            return err(format!("@{rev} is stack {}, not {name}", was.name));
        }
        let rec = Stack {
            op: format!("restore @{rev}"),
            parent: self.head(name).map(|(rev, _)| rev),
            time: now(),
            ..was.clone()
        };
        self.push(Record::Stack(rec))
    }

    /// The patch `spec` names in `stack`: its position, counting from 1 at
    /// the bottom, or a prefix of its id or of its change.
    pub fn resolve(&self, stack: &Stack, spec: &str) -> Result<usize> {
        if let Ok(n) = spec.parse::<usize>()
            && spec.len() < 4
        {
            return match n {
                1.. if n <= stack.patches.len() => Ok(n - 1),
                _ => err(format!("{} has {} patches, not {n}", stack.name, stack.patches.len())),
            };
        }
        let found: Vec<usize> = (0..stack.patches.len())
            .filter(|&i| {
                let p = self.patch(&stack.patches[i]);
                p.id.starts_with(spec) || p.change.starts_with(spec)
            })
            .collect();
        match found[..] {
            [i] if !spec.is_empty() => Ok(i),
            [] | [_] => err(format!("no patch {spec} in {}", stack.name)),
            _ => err(format!("patch {spec} is ambiguous in {}", stack.name)),
        }
    }
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains(char::is_whitespace) || name.starts_with('-') {
        return err(format!("{name:?} cannot name a stack"));
    }
    Ok(())
}
