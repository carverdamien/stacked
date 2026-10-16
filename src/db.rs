//! The database: one file of JSON lines, appended to and never rewritten.
//!
//! A line is a record, named by its line number, its rev (`@12`).
//!
//! - A patch is stored once and never changes. An edited patch is a new
//!   patch sharing its `change`.
//! - A stack is the whole stack as an edit left it, with the stack it was
//!   edited from as its parent: the stacks of a name make a tree, the tree
//!   of its edits.
//! - A head moves a name to another stack of its tree: an undo, a redo, a
//!   stack picked from the tree, or (to none) a delete. An edit made from
//!   there starts a new branch, and the old one stays.
//! - A comment is on a line of a change, kept as the change gets new
//!   versions; the latest record for a line is the comment.
//!
//! The file is only ever appended to, so a crash can only leave a last line
//! cut short. Such a line, or any line that cannot be read, is passed over
//! with a warning, and keeps its number: the next write ends it with a
//! newline rather than cutting it off, and every rev after stays the same.

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
    Head(Head),
    Comment(Note),
    Run(Run),
    /// A line of what a run's claude wrote, as it wrote it.
    Event {
        run: String,
        event: serde_json::Value,
    },
    /// A line that could not be read.
    #[serde(skip)]
    Broken,
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Stack {
    pub name: String,
    /// Patch ids, bottom first: each applies on top of the ones before it.
    pub patches: Vec<String>,
    /// The commit the bottom patch applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The repository `base` is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The stack this one was edited from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// What the edit was.
    pub op: String,
    /// Seconds since the epoch.
    pub time: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Head {
    pub name: String,
    /// The stack `name` is now; none when it is deleted.
    pub rev: Option<usize>,
    pub op: String,
    pub time: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Note {
    pub change: String,
    /// Which comment: the line's key for yours, `claude:<ask>:<n>` for a
    /// review's note, `phab:<id>` for a reviewer's thread.
    pub id: String,
    /// The line it is on, as self-review keys lines: by their text, so it
    /// stays on a line kept by the next version of the patch. Empty when it
    /// is on no line.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub line: String,
    /// The thread: yours, then claude's replies in ```claude fences and
    /// reviewers' in ```reviewer ones. Empty when the comment is deleted.
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    /// The line's text, for showing it when the line is gone.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preview: String,
    /// Who opened it, when not you: `claude`, for a review's note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The Phabricator comments it holds, so pulling again adds only new ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote: Vec<String>,
    pub time: i64,
}

/// An operation's start or result: an ask of claude's, putting a stack in
/// the worktree, a pull from Phabricator. The latest record of an id is
/// where it stands.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Run {
    pub id: String,
    /// `edit`, `rebase`, `review`, `test`, `apply`, `pull`.
    pub what: String,
    pub stack: String,
    pub state: RunState,
    /// What was asked, for an ask.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request: String,
    /// What came of it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Claude's last reply, for an ask.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
    /// The stack it made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<usize>,
    /// For an ask: all it was started with (`ask::Info`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<serde_json::Value>,
    /// The process running it, while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub time: i64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    #[default]
    Started,
    Done,
    Failed,
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
    let hex = sha1_smol::Sha1::from(format!("{nanos} {} {n} {seed}", std::process::id()))
        .digest()
        .to_string();
    hex.chars()
        .take(12)
        .map(|c: char| (b'k' + c.to_digit(16).unwrap() as u8) as char)
        .collect()
}

pub struct Db {
    pub path: PathBuf,
    /// `records[i]` is rev `i + 1`.
    pub records: Vec<Record>,
    patches: HashMap<String, usize>,
    /// Every name ever used, to the index of the stack it is on, or none
    /// when it is deleted.
    heads: BTreeMap<String, Option<usize>>,
    /// (change, comment id) to the index of its latest comment record.
    notes: BTreeMap<(String, String), usize>,
    /// Run id to the index of its latest record.
    runs: BTreeMap<String, usize>,
    /// Run id to the indices of its events.
    events: HashMap<String, Vec<usize>>,
    /// The bytes read.
    len: u64,
    /// The last line read has no newline: the next write starts with one.
    torn: bool,
    /// Records from here on are not yet on disk.
    saved: usize,
    /// The lines passed over, and why.
    pub warnings: Vec<String>,
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
            notes: BTreeMap::new(),
            runs: BTreeMap::new(),
            events: HashMap::new(),
            len: bytes.len() as u64,
            torn: !bytes.is_empty() && !bytes.ends_with(b"\n"),
            saved: 0,
            warnings: Vec::new(),
        };
        let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
        for (i, &line) in lines.iter().enumerate() {
            let last = i + 1 == lines.len();
            if last && line.is_empty() {
                break;
            }
            let at = |e: &str| format!("{}:{}: {e}", path.display(), i + 1);
            let rec = std::str::from_utf8(line)
                .map_err(|e| e.to_string())
                .and_then(|l| serde_json::from_str::<Record>(l).map_err(|e| e.to_string()));
            let rec = match rec {
                Ok(r) => r,
                Err(e) => {
                    let why = if last { "cut short, passed over" } else { "passed over" };
                    db.warnings.push(at(&format!("{why}: {e}")));
                    db.records.push(Record::Broken);
                    continue;
                }
            };
            match (&rec, i) {
                (Record::Stacked { version }, 0) if *version > VERSION => {
                    return err(at(&format!(
                        "written by a newer stacked (format {version}, this one reads {VERSION})"
                    )));
                }
                (Record::Stacked { .. }, 0) => {}
                (_, 0) => return err(format!("{}: not a stacked database", path.display())),
                (Record::Stacked { .. }, _) => {
                    db.warnings.push(at("a second header, passed over"));
                    db.records.push(Record::Broken);
                    continue;
                }
                _ => {}
            }
            if let Err(e) = db.index(rec) {
                db.warnings.push(at(&format!("passed over: {e}")));
                db.records.push(Record::Broken);
            }
        }
        db.saved = db.records.len();
        Ok(db)
    }

    fn index(&mut self, rec: Record) -> Result<()> {
        let at = self.records.len();
        match &rec {
            Record::Stacked { .. } | Record::Broken => {}
            Record::Patch(p) => {
                self.patches.entry(p.id.clone()).or_insert(at);
            }
            Record::Stack(s) => {
                if let Some(id) = s.patches.iter().find(|id| !self.patches.contains_key(*id)) {
                    return err(format!("stack {} names patch {id}, which is not stored", s.name));
                }
                self.heads.insert(s.name.clone(), Some(at));
            }
            Record::Head(h) => {
                if let Some(rev) = h.rev {
                    match self.rev(rev) {
                        Some(Record::Stack(s)) if s.name == h.name => {}
                        _ => return err(format!("@{rev} is not a stack of {}", h.name)),
                    }
                }
                self.heads.insert(h.name.clone(), h.rev.map(|r| r - 1));
            }
            Record::Comment(n) => {
                self.notes.insert((n.change.clone(), n.id.clone()), at);
            }
            Record::Run(r) => {
                self.runs.insert(r.id.clone(), at);
            }
            Record::Event { run, .. } => {
                self.events.entry(run.clone()).or_default().push(at);
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
        if self.torn {
            out.push('\n');
        }
        for rec in &self.records[self.saved..] {
            out.push_str(&serde_json::to_string(rec).unwrap());
            out.push('\n');
        }
        let io = |e: std::io::Error| Error(format!("{}: {e}", self.path.display()));
        lock.0.write_all(out.as_bytes()).map_err(io)?;
        lock.0.sync_data().map_err(io)?;
        self.len += out.len() as u64;
        self.torn = false;
        self.saved = self.records.len();
        Ok(())
    }

    /// The bytes read: when the file is longer, someone else wrote to it.
    pub fn len(&self) -> u64 {
        self.len
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

    /// The stack at `rev`.
    pub fn node(&self, rev: usize) -> Result<&Stack> {
        match self.rev(rev) {
            Some(Record::Stack(s)) => Ok(s),
            _ => err(format!("@{rev} is not a stack")),
        }
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

    /// The stack `name` is on, and its rev.
    pub fn head(&self, name: &str) -> Option<(usize, &Stack)> {
        let at = (*self.heads.get(name)?)?;
        match &self.records[at] {
            Record::Stack(s) => Some((at + 1, s)),
            _ => unreachable!(),
        }
    }

    pub fn stack(&self, name: &str) -> Result<&Stack> {
        match (self.head(name), self.heads.contains_key(name)) {
            (Some((_, s)), _) => Ok(s),
            (None, true) => err(format!("stack {name} is deleted")),
            (None, false) => err(format!("no stack {name}")),
        }
    }

    /// The names not deleted.
    pub fn stacks(&self) -> Vec<&Stack> {
        self.heads.keys().filter_map(|n| self.head(n)).map(|(_, s)| s).collect()
    }

    /// Every name ever used, deleted ones too.
    pub fn names(&self) -> Vec<&str> {
        self.heads.keys().map(String::as_str).collect()
    }

    /// The edits to `name`, or to every name, oldest first: each stack
    /// made, and each move of a head.
    pub fn log(&self, name: Option<&str>) -> Vec<(usize, &Record)> {
        self.records
            .iter()
            .enumerate()
            .filter(|(_, r)| match r {
                Record::Stack(s) => name.is_none_or(|n| n == s.name),
                Record::Head(h) => name.is_none_or(|n| n == h.name),
                _ => false,
            })
            .map(|(i, r)| (i + 1, r))
            .collect()
    }

    /// Every stack of `name`'s tree, oldest first.
    pub fn tree(&self, name: &str) -> Vec<(usize, &Stack)> {
        self.records
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                Record::Stack(s) if s.name == name => Some((i + 1, s)),
                _ => None,
            })
            .collect()
    }

    /// Makes `name` hold `patches`, on `base`, edited from the stack it is
    /// on. Nothing is written if that is what it holds already; the rev of
    /// the stack it is on either way.
    pub fn set_stack_on(
        &mut self,
        name: &str,
        patches: Vec<String>,
        base: Option<String>,
        repo: Option<String>,
        op: &str,
    ) -> Result<usize> {
        let parent = self.head(name);
        if let Some((rev, s)) = parent
            && s.patches == patches
            && s.base == base
            && s.repo == repo
        {
            return Ok(rev);
        }
        check_name(name)?;
        self.push(Record::Stack(Stack {
            name: name.into(),
            patches,
            base,
            repo,
            parent: parent.map(|(rev, _)| rev),
            op: op.into(),
            time: now(),
        }))
    }

    /// As `set_stack_on`, the base kept.
    pub fn set_stack(&mut self, name: &str, patches: Vec<String>, op: &str) -> Result<usize> {
        let (base, repo) = match self.head(name) {
            Some((_, s)) => (s.base.clone(), s.repo.clone()),
            None => (None, None),
        };
        self.set_stack_on(name, patches, base, repo, op)
    }

    pub fn new_stack(&mut self, name: &str) -> Result<usize> {
        if self.head(name).is_some() {
            return err(format!("stack {name} already exists"));
        }
        check_name(name)?;
        self.push(Record::Stack(Stack {
            name: name.into(),
            patches: vec![],
            base: None,
            repo: None,
            parent: None,
            op: "new".into(),
            time: now(),
        }))
    }

    fn move_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        self.push(Record::Head(Head {
            name: name.into(),
            rev,
            op,
            time: now(),
        }))
    }

    pub fn delete_stack(&mut self, name: &str) -> Result<usize> {
        self.stack(name)?;
        self.move_head(name, None, "delete".into())
    }

    /// Makes the stack at `rev` the one its name is on.
    pub fn select(&mut self, rev: usize) -> Result<usize> {
        let name = self.node(rev)?.name.clone();
        if self.head(&name).is_some_and(|(r, _)| r == rev) {
            return Ok(rev);
        }
        self.move_head(&name, Some(rev), format!("select @{rev}"))
    }

    /// Moves `name` to the stack its stack was edited from; a deleted
    /// stack comes back as it was.
    pub fn undo(&mut self, name: &str) -> Result<usize> {
        let Some((rev, s)) = self.head(name) else {
            let Some(back) = self.last_head(name) else {
                return err(format!("no stack {name}"));
            };
            return self.move_head(name, Some(back), "undo delete".into());
        };
        let Some(parent) = s.parent else {
            return err(format!("nothing to undo: {name} was made at @{rev}"));
        };
        let op = format!("undo {}", s.op);
        self.move_head(name, Some(parent), op)
    }

    /// The stack `name` was on before it was deleted.
    fn last_head(&self, name: &str) -> Option<usize> {
        self.records.iter().enumerate().rev().find_map(|(i, r)| match r {
            Record::Stack(s) if s.name == name => Some(i + 1),
            Record::Head(h) if h.name == name && h.rev.is_some() => h.rev,
            _ => None,
        })
    }

    /// Moves `name` to the stack edited from its own that it was last on:
    /// what the undos before took it back from.
    pub fn redo(&mut self, name: &str) -> Result<usize> {
        let Some((rev, _)) = self.head(name) else {
            return err(format!("no stack {name}"));
        };
        // The stacks it has been on, oldest first.
        let mut on = Vec::new();
        for (i, r) in self.records.iter().enumerate() {
            match r {
                Record::Stack(s) if s.name == name => on.push(i + 1),
                Record::Head(h) if h.name == name => on.extend(h.rev),
                _ => {}
            }
        }
        for &later in on.iter().rev() {
            if let Some(child) = self.child_towards(rev, later) {
                let op = format!("redo {}", self.node(child)?.op);
                return self.move_head(name, Some(child), op);
            }
        }
        err(format!("nothing to redo: no stack was edited from @{rev}"))
    }

    /// The child of `from` on the way to `to`, if `to` descends from it.
    fn child_towards(&self, from: usize, to: usize) -> Option<usize> {
        let mut at = to;
        loop {
            let parent = self.node(at).ok()?.parent?;
            if parent == from {
                return Some(at);
            }
            at = parent;
        }
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

    /// The comments on `change`, by id.
    pub fn notes(&self, change: &str) -> Vec<&Note> {
        self.notes
            .range((change.to_string(), String::new())..)
            .take_while(|((c, _), _)| c == change)
            .filter_map(|(_, &at)| match &self.records[at] {
                Record::Comment(n) if !n.text.is_empty() => Some(n),
                _ => None,
            })
            .collect()
    }

    /// Where run `id` stands.
    pub fn run(&self, id: &str) -> Option<&Run> {
        match &self.records[*self.runs.get(id)?] {
            Record::Run(r) => Some(r),
            _ => None,
        }
    }

    /// Every run as it stands, oldest started first.
    pub fn runs(&self) -> Vec<&Run> {
        let mut ids: Vec<(&String, &usize)> = self.runs.iter().collect();
        // By where each was first written: ids are not in time order.
        let first: HashMap<&str, usize> = self
            .records
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(i, r)| match r {
                Record::Run(r) => Some((r.id.as_str(), i)),
                _ => None,
            })
            .collect();
        ids.sort_by_key(|(id, _)| first[id.as_str()]);
        ids.into_iter().filter_map(|(id, _)| self.run(id)).collect()
    }

    /// What run `id`'s claude wrote, in order.
    pub fn events(&self, id: &str) -> Vec<&serde_json::Value> {
        self.events
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|&at| match &self.records[at] {
                Record::Event { event, .. } => Some(event),
                _ => None,
            })
            .collect()
    }

    pub fn add_event(&mut self, run: &str, event: serde_json::Value) -> Result<usize> {
        self.push(Record::Event {
            run: run.into(),
            event,
        })
    }

    /// Appends where run `r.id` stands now.
    pub fn add_run(&mut self, r: Run) -> Result<usize> {
        self.push(Record::Run(Run { time: now(), ..r }))
    }

    /// Appends a stack made from somewhere else than its name's head (a
    /// landed ask's): its name moves to it.
    pub fn add_node(&mut self, s: Stack) -> Result<usize> {
        if let Some(p) = s.parent {
            self.node(p)?;
        }
        check_name(&s.name)?;
        self.push(Record::Stack(s))
    }

    /// Moves `name` to `rev`, or to none.
    pub fn move_head_to(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        self.move_head(name, rev, op)
    }

    /// The comment `id` on `change`, if it is not deleted.
    pub fn note(&self, change: &str, id: &str) -> Option<&Note> {
        match &self.records[*self.notes.get(&(change.to_string(), id.to_string()))?] {
            Record::Comment(n) if !n.text.is_empty() => Some(n),
            _ => None,
        }
    }

    /// Writes a comment; empty text deletes it.
    pub fn set_note(&mut self, note: Note) -> Result<()> {
        let same = self
            .notes
            .get(&(note.change.clone(), note.id.clone()))
            .map(|&at| &self.records[at])
            .is_some_and(|r| matches!(r, Record::Comment(n) if n.text == note.text && n.remote == note.remote));
        if !same {
            self.push(Record::Comment(Note { time: now(), ..note }))?;
        }
        Ok(())
    }
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains(char::is_whitespace) || name.starts_with('-') {
        return err(format!("{name:?} cannot name a stack"));
    }
    Ok(())
}
