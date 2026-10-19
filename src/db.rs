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
//! And a reader need only read what was appended since it last read.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 2: the agent's turns are fenced ```agent, its comments `by` "agent".
pub const VERSION: u32 = 2;

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
    /// What a stack's comments pane shows (a filter), until changed.
    View { stack: String, show: String },
    /// A choice of a stack's menus, picked with ← →, until changed: `of`
    /// names it (`edit`: where a file is edited by hand), `pick` the one
    /// picked.
    Choice { stack: String, of: String, pick: String },
    /// Lines of a patch marked read, in the reading mode; or a mark taken
    /// back.
    Read(Mark),
    /// A line of what a run's claude wrote, as it wrote it.
    Event {
        run: String,
        event: serde_json::Value,
    },
    /// A line that could not be read.
    #[serde(skip)]
    Broken,
}

/// Lines of a patch marked read at once, in the reading mode: a block of
/// code, as self-review marks them. The lines are keyed by their content, so
/// a mark holds for every version of the patch that has them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Mark {
    pub change: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    /// The mark, by its rev, this one takes back: then `keys` is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<usize>,
    pub time: i64,
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
    /// Sent to claude, and waiting for its turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<Queued>,
    /// Why the last ask about it failed, until it is sent again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// Where it was before it was promoted to the stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved: Option<Moved>,
    /// For a comment an answer opened, the comment that asked: its change
    /// and id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<(String, String)>,
    /// For a comment on the stack's history, the record (its rev) it is
    /// about: the agent is asked about it to answer only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<usize>,
    pub time: i64,
}

impl Note {
    /// Whether it is on the stack itself, not a patch.
    pub fn on_stack(&self) -> bool {
        self.change.starts_with(STACK)
    }
}

/// What claude may do with a comment sent to it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    /// Answer, and change the code within the comment's scope.
    Change,
    /// Answer only: read-only.
    Answer,
    /// Build and test, in a workspace of claude's choosing; changes
    /// nothing in the stack.
    Test,
}

impl Way {
    pub fn name(self) -> &'static str {
        match self {
            Way::Change => "change",
            Way::Answer => "answer",
            Way::Test => "test",
        }
    }

    /// How it is said to the user: `answer only`.
    pub fn said(self) -> &'static str {
        match self {
            Way::Change => "may change the code",
            Way::Answer => "answer only",
            Way::Test => "build and test",
        }
    }
}

/// A comment sent and waiting: how, and the batch it goes in. Comments
/// sent together go in one ask. A patch's comment may be sent to the whole
/// stack (`stack`): the agent may then change any patch, as for the
/// stack's own comments, not only its patch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Queued {
    pub way: Way,
    pub batch: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stack: bool,
}

impl Queued {
    /// How it is said to the user: `answer only`, `may change the whole
    /// stack`.
    pub fn said(&self) -> &'static str {
        if self.stack { WHOLE } else { self.way.said() }
    }
}

/// How a change asked of the whole stack, from a patch's comment, is said.
pub const WHOLE: &str = "may change the whole stack";

/// Where a promoted comment was.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Moved {
    pub change: String,
    pub id: String,
    pub line: String,
    pub file: String,
    pub preview: String,
}

/// The change a stack's own comments are on.
const STACK: &str = "stack:";

pub fn stack_change(name: &str) -> String {
    format!("{STACK}{name}")
}

/// An id no other process, and no other call of this one, makes: the time
/// in milliseconds, the process, and a count. Ids of runs, batches and
/// comments; those made earlier sort before.
pub fn unique() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{ms}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// A comment's short id: six hex characters, for the command line.
pub fn short_id(change: &str, id: &str) -> String {
    sha1_smol::Sha1::from(format!("{change}\0{id}")).digest().to_string()[..6].to_string()
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
    /// The process starting it (its worktree to make), until `pid` is
    /// known: while it lives, the run is not left behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starter: Option<u32>,
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

/// `t` as local time, `2026-10-16 14:03`.
pub fn local_time(t: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = t as libc::time_t;
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
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
    /// (change, comment id) to the index of each of its records, oldest
    /// first: its history.
    note_records: HashMap<(String, String), Vec<usize>>,
    /// Stack name to the index of its latest view record.
    views: HashMap<String, usize>,
    /// (stack name, what the choice is of) to the index of its latest
    /// choice record.
    choices: BTreeMap<(String, String), usize>,
    /// Change to the indices of its read marks not taken back, oldest
    /// first.
    marks: HashMap<String, Vec<usize>>,
    /// Run id to the index of its latest record.
    runs: BTreeMap<String, usize>,
    /// Run ids, as each was first written.
    run_order: Vec<String>,
    /// How many records are not lines of an agent's stream: what changes
    /// what the editor shows.
    changes: usize,
    /// Run id to the indices of its events.
    events: HashMap<String, Vec<usize>>,
    /// The bytes read.
    len: u64,
    /// The last line read has no newline: the next write starts with one.
    torn: bool,
    /// Where that line starts.
    tail: u64,
    /// Records from here on are not yet on disk.
    saved: usize,
    /// The lines passed over, and why.
    pub warnings: Vec<String>,
}

/// The database's lock, held until dropped.
pub struct Lock(Held);

/// What a lock holds: its file, locked; or a file kept in memory, the
/// lock of those, and its bytes.
enum Held {
    File(File),
    #[cfg(feature = "demo")]
    Memory {
        _held: std::sync::MutexGuard<'static, ()>,
        bytes: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    },
}

thread_local! {
    /// This thread holds a database's lock: taking one again would wait on
    /// itself for ever, so it is an error instead.
    static LOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Takes the lock on `f` for this thread: an error if it holds one already.
fn take_lock(f: &File, path: &Path) -> Result<()> {
    if LOCKED.with(|l| l.get()) {
        return err(format!(
            "{}: locked already by this thread: taking it again would wait for ever",
            path.display()
        ));
    }
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return err(format!("{}: cannot lock: {}", path.display(), std::io::Error::last_os_error()));
    }
    LOCKED.with(|l| l.set(true));
    Ok(())
}

/// Gives the lock on `f` back, then: not when the last copy of its file is
/// closed, which a child forked meanwhile could hold.
fn give_lock(f: &File) {
    unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_UN) };
    LOCKED.with(|l| l.set(false));
}

impl Lock {
    /// The file locked, when it is on disk.
    fn file(&mut self) -> &mut File {
        match &mut self.0 {
            Held::File(f) => f,
            #[cfg(feature = "demo")]
            Held::Memory { .. } => unreachable!("a file kept in memory has no file"),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        match &self.0 {
            Held::File(f) => give_lock(f),
            #[cfg(feature = "demo")]
            Held::Memory { .. } => LOCKED.with(|l| l.set(false)),
        }
    }
}

impl Db {
    /// Reads `path`; a missing file is an empty database.
    pub fn load(path: &Path) -> Result<Db> {
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(path) {
            let bytes = m.lock().unwrap().clone();
            return Db::parse(path, &bytes);
        }
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

    /// The database `bytes` hold, as if read from `path`.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Db> {
        Db::parse(path, bytes)
    }

    fn parse(path: &Path, bytes: &[u8]) -> Result<Db> {
        let mut db = Db {
            path: path.to_path_buf(),
            records: Vec::new(),
            patches: HashMap::new(),
            heads: BTreeMap::new(),
            notes: BTreeMap::new(),
            note_records: HashMap::new(),
            views: HashMap::new(),
            choices: BTreeMap::new(),
            marks: HashMap::new(),
            runs: BTreeMap::new(),
            run_order: Vec::new(),
            changes: 0,
            events: HashMap::new(),
            len: 0,
            torn: false,
            tail: 0,
            saved: 0,
            warnings: Vec::new(),
        };
        db.read(bytes)?;
        Ok(db)
    }

    /// Reads `bytes`, what the file holds from byte `self.len` on.
    fn read(&mut self, bytes: &[u8]) -> Result<()> {
        let mut offset = self.len;
        let path = self.path.display().to_string();
        let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
        for (i, &line) in lines.iter().enumerate() {
            let last = i + 1 == lines.len();
            if last && line.is_empty() {
                break;
            }
            let no = self.records.len() + 1;
            let at = |e: &str| format!("{path}:{no}: {e}");
            // A last line with no newline is not read: a write in progress,
            // or one a crash cut short. It holds its line's number.
            if last {
                self.warnings.push(at("cut short, passed over"));
                self.records.push(Record::Broken);
                self.torn = true;
                self.tail = offset;
                break;
            }
            offset += line.len() as u64 + 1;
            let rec = std::str::from_utf8(line)
                .map_err(|e| e.to_string())
                .and_then(|l| serde_json::from_str::<Record>(l).map_err(|e| e.to_string()));
            let rec = match rec {
                Ok(r) => r,
                Err(e) => {
                    self.warnings.push(at(&format!("passed over: {e}")));
                    self.records.push(Record::Broken);
                    continue;
                }
            };
            match (&rec, no) {
                (Record::Stacked { version }, 1) if *version > VERSION => {
                    return err(at(&format!(
                        "written by a newer stacked (format {version}, this one reads {VERSION})"
                    )));
                }
                (Record::Stacked { .. }, 1) => {}
                (_, 1) => return err(format!("{}: not a stacked database", self.path.display())),
                (Record::Stacked { .. }, _) => {
                    self.warnings.push(at("a second header, passed over"));
                    self.records.push(Record::Broken);
                    continue;
                }
                _ => {}
            }
            if let Err(e) = self.index(rec) {
                self.warnings.push(at(&format!("passed over: {e}")));
                self.records.push(Record::Broken);
            }
        }
        self.len += bytes.len() as u64;
        self.saved = self.records.len();
        Ok(())
    }

    /// Reads what was appended since: the file only grows, so only the new
    /// bytes, and the line that was cut short again. Whether there was any.
    pub fn catch_up(&mut self) -> Result<bool> {
        let size = Db::file_len(&self.path);
        if size == self.len {
            return Ok(false);
        }
        if size < self.len {
            // Not ours to shrink: someone cut the file. Read it whole.
            *self = Db::load(&self.path)?;
            return Ok(true);
        }
        if self.torn {
            self.records.pop();
            self.warnings.pop();
            self.len = self.tail;
            self.torn = false;
        }
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(&self.path) {
            let bytes = m.lock().unwrap()[self.len as usize..].to_vec();
            self.read(&bytes)?;
            return Ok(true);
        }
        let mut f = File::open(&self.path).map_err(|e| Error(format!("{}: {e}", self.path.display())))?;
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(self.len))
            .map_err(|e| Error(e.to_string()))?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).map_err(|e| Error(e.to_string()))?;
        self.read(&bytes)?;
        Ok(true)
    }

    /// Appends `recs` to the file at `path` under its lock, without reading
    /// it: for what nothing else depends on, as the lines a run's claude
    /// writes, which come fast.
    pub fn append(path: &Path, recs: &[Record]) -> Result<()> {
        #[cfg(feature = "demo")]
        if crate::memory::is_file(path) {
            let (mut db, mut lock) = Db::lock(path)?;
            for rec in recs {
                db.push(rec.clone())?;
            }
            return db.save(&mut lock);
        }
        let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
        let f = File::options().create(true).append(true).read(true).open(path).map_err(io)?;
        take_lock(&f, path)?;
        let mut lock = Lock(Held::File(f));
        let f = lock.file();
        let size = f.metadata().map_err(io)?.len();
        let mut out = String::new();
        if size == 0 {
            out.push_str(&serde_json::to_string(&Record::Stacked { version: VERSION }).unwrap());
            out.push('\n');
        } else {
            use std::io::{Seek, SeekFrom};
            let mut last = [0u8];
            f.seek(SeekFrom::Start(size - 1)).map_err(io)?;
            f.read_exact(&mut last).map_err(io)?;
            if last[0] != b'\n' {
                out.push('\n');
            }
        }
        for rec in recs {
            out.push_str(&serde_json::to_string(rec).unwrap());
            out.push('\n');
        }
        f.write_all(out.as_bytes()).map_err(io)?;
        f.sync_data().map_err(io)
    }

    fn index(&mut self, rec: Record) -> Result<()> {
        let at = self.records.len();
        if !matches!(rec, Record::Event { .. }) {
            self.changes += 1;
        }
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
                let key = (n.change.clone(), n.id.clone());
                self.note_records.entry(key.clone()).or_default().push(at);
                self.notes.insert(key, at);
            }
            Record::Run(r) => {
                if self.runs.insert(r.id.clone(), at).is_none() {
                    self.run_order.push(r.id.clone());
                }
            }
            Record::Event { run, .. } => {
                self.events.entry(run.clone()).or_default().push(at);
            }
            Record::View { stack, .. } => {
                self.views.insert(stack.clone(), at);
            }
            Record::Choice { stack, of, .. } => {
                self.choices.insert((stack.clone(), of.clone()), at);
            }
            Record::Read(m) => {
                let marks = self.marks.entry(m.change.clone()).or_default();
                match m.undo {
                    None => marks.push(at),
                    Some(rev) => match marks.iter().position(|&i| i + 1 == rev) {
                        Some(k) => {
                            marks.remove(k);
                        }
                        None => return err(format!("@{rev} is no read mark of {} standing", m.change)),
                    },
                }
            }
        }
        self.records.push(rec);
        Ok(())
    }

    /// Takes the lock on `path`, then reads it: what is read stays true
    /// until the lock is dropped, so an edit made from it loses no one
    /// else's.
    pub fn lock(path: &Path) -> Result<(Db, Lock)> {
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(path) {
            if LOCKED.with(|l| l.get()) {
                return err(format!(
                    "{}: locked already by this thread: taking it again would wait for ever",
                    path.display()
                ));
            }
            let guard = crate::memory::lock();
            LOCKED.with(|l| l.set(true));
            let bytes = m.lock().unwrap().clone();
            let lock = Lock(Held::Memory { _held: guard, bytes: m });
            return Ok((Db::parse(path, &bytes)?, lock));
        }
        let f = File::options()
            .create(true)
            .append(true)
            .read(true)
            .open(path)
            .map_err(|e| Error(format!("{}: {e}", path.display())))?;
        take_lock(&f, path)?;
        // Given back if reading fails: the lock first.
        let mut lock = Lock(Held::File(f));
        let mut bytes = Vec::new();
        lock.file()
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
        match &mut lock.0 {
            Held::File(f) => {
                f.write_all(out.as_bytes()).map_err(io)?;
                f.sync_data().map_err(io)?;
            }
            #[cfg(feature = "demo")]
            Held::Memory { bytes, .. } => bytes.lock().unwrap().extend_from_slice(out.as_bytes()),
        }
        self.len += out.len() as u64;
        self.torn = false;
        self.saved = self.records.len();
        Ok(())
    }

    /// The bytes read: when the file is longer, someone else wrote to it.
    pub fn bytes_read(&self) -> u64 {
        self.len
    }

    /// Whether there is a database at `path`, wherever it is kept.
    pub fn exists(path: &Path) -> bool {
        #[cfg(feature = "demo")]
        if crate::memory::is_file(path) {
            return true;
        }
        path.exists()
    }

    /// How long the database at `path` is, wherever it is kept.
    pub fn file_len(path: &Path) -> u64 {
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(path) {
            return m.lock().unwrap().len() as u64;
        }
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
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

    /// The first stack to hold patch `id`, and its rev: when, and by what
    /// edit, that version came in.
    pub fn first_use(&self, id: &str) -> Option<(usize, &Stack)> {
        self.records.iter().enumerate().find_map(|(i, r)| match r {
            Record::Stack(s) if s.patches.iter().any(|p| p == id) => Some((i + 1, s)),
            _ => None,
        })
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
        self.push_stack(Stack {
            name: name.into(),
            patches,
            base,
            repo,
            parent: parent.map(|(rev, _)| rev),
            op: op.into(),
            time: now(),
        })
    }

    /// Whether `s` may go in the database, or be the stack its name is on:
    /// its patches apply, each on the ones before it, to its base. A stack
    /// with no base, or whose repository or base is not there, cannot be
    /// checked, and so cannot change: it is read-only, comments aside.
    pub fn verify(&self, s: &Stack) -> Result<()> {
        let patches: Vec<&Patch> = s.patches.iter().map(|id| self.patch(id)).collect();
        crate::check::stack(s, &patches)
    }

    /// Whether the stack `name` is on can be checked, and so changed: its
    /// repository and base are there.
    pub fn writable(&self, name: &str) -> Result<()> {
        crate::check::can_check(self.stack(name)?).map(|_| ())
    }

    /// Appends `s`, once it is seen to apply.
    fn push_stack(&mut self, s: Stack) -> Result<usize> {
        self.verify(&s)?;
        self.push(Record::Stack(s))
    }

    /// As `set_stack_on`, the base kept.
    pub fn set_stack(&mut self, name: &str, patches: Vec<String>, op: &str) -> Result<usize> {
        let (base, repo) = match self.head(name) {
            Some((_, s)) => (s.base.clone(), s.repo.clone()),
            None => (None, None),
        };
        self.set_stack_on(name, patches, base, repo, op)
    }

    /// Makes an empty stack on `base`, a commit of the repository `repo`.
    pub fn new_stack(&mut self, name: &str, base: &str, repo: &str) -> Result<usize> {
        if self.head(name).is_some() {
            return err(format!("stack {name} already exists"));
        }
        check_name(name)?;
        self.push_stack(Stack {
            name: name.into(),
            patches: vec![],
            base: Some(base.into()),
            repo: Some(repo.into()),
            parent: None,
            op: "new".into(),
            time: now(),
        })
    }

    /// Moves `name` to the stack at `rev`, once it is seen to apply; or, to
    /// delete it, to none.
    fn move_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        match rev {
            Some(r) => self.verify(self.node(r)?)?,
            None => self.writable(name)?,
        }
        self.push_head(name, rev, op)
    }

    fn push_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
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
        let Some((to, op)) = self.undo_target(name) else {
            return match self.head(name) {
                Some((rev, _)) => err(format!("nothing to undo: {name} was made at @{rev}")),
                None => err(format!("no stack {name}")),
            };
        };
        self.move_head(name, Some(to), format!("undo {op}"))
    }

    /// Where undo would take `name`, and the edit it would take back.
    pub fn undo_target(&self, name: &str) -> Option<(usize, String)> {
        match self.head(name) {
            Some((_, s)) => Some((s.parent?, s.op.clone())),
            None => Some((self.last_head(name)?, "delete".into())),
        }
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
        match self.redo_target(name) {
            Some((child, op)) => self.move_head(name, Some(child), format!("redo {op}")),
            None => err(format!("nothing to redo: no stack was edited from @{rev}")),
        }
    }

    /// Where redo would take `name`, and the edit it would make again.
    pub fn redo_target(&self, name: &str) -> Option<(usize, String)> {
        let (rev, _) = self.head(name)?;
        // The stacks it has been on, oldest first.
        let mut on = Vec::new();
        for (i, r) in self.records.iter().enumerate() {
            match r {
                Record::Stack(s) if s.name == name => on.push(i + 1),
                Record::Head(h) if h.name == name => on.extend(h.rev),
                _ => {}
            }
        }
        on.iter()
            .rev()
            .find_map(|&later| self.child_towards(rev, later))
            .and_then(|child| Some((child, self.node(child).ok()?.op.clone())))
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

    /// Every comment of stack `name`: its own, then its patches', bottom
    /// first.
    pub fn stack_notes(&self, name: &str) -> Vec<&Note> {
        let mut out = self.notes(&stack_change(name));
        if let Ok(s) = self.stack(name) {
            for id in &s.patches {
                out.extend(self.notes(&self.patch(id).change));
            }
        }
        out
    }

    /// The comment of stack `name` whose short id starts with `short`.
    pub fn find_note(&self, name: &str, short: &str) -> Result<&Note> {
        let found: Vec<&Note> = self
            .stack_notes(name)
            .into_iter()
            .filter(|n| !short.is_empty() && short_id(&n.change, &n.id).starts_with(short))
            .collect();
        match found[..] {
            [n] => Ok(n),
            [] => err(format!("no comment {short} on {name}")),
            _ => err(format!("comment {short} is ambiguous on {name}")),
        }
    }

    /// Every record of comment `id` on `change`, oldest first, with its rev.
    pub fn note_history(&self, change: &str, id: &str) -> Vec<(usize, &Note)> {
        self.note_records
            .get(&(change.to_string(), id.to_string()))
            .into_iter()
            .flatten()
            .filter_map(|&at| match &self.records[at] {
                Record::Comment(n) => Some((at + 1, n)),
                _ => None,
            })
            .collect()
    }

    /// The comments an answer opened from comment `id` on `change`.
    pub fn opened_from(&self, name: &str, change: &str, id: &str) -> Vec<&Note> {
        self.stack_notes(name)
            .into_iter()
            .filter(|n| n.opened_by.as_ref().is_some_and(|(c, i)| c == change && i == id))
            .collect()
    }

    /// What stack `name`'s comments pane shows, if it was ever picked.
    pub fn view(&self, name: &str) -> Option<&str> {
        match &self.records[*self.views.get(name)?] {
            Record::View { show, .. } => Some(show),
            _ => None,
        }
    }

    pub fn set_view(&mut self, name: &str, show: &str) -> Result<()> {
        if self.view(name) != Some(show) {
            self.push(Record::View {
                stack: name.into(),
                show: show.into(),
            })?;
        }
        Ok(())
    }

    /// What stack `name` picked for its menus' choice `of`, if it ever did.
    pub fn choice(&self, name: &str, of: &str) -> Option<&str> {
        match &self.records[*self.choices.get(&(name.to_string(), of.to_string()))?] {
            Record::Choice { pick, .. } => Some(pick),
            _ => None,
        }
    }

    /// Every choice stack `name` picked: what it is of, and the pick.
    pub fn choices(&self, name: &str) -> Vec<(&str, &str)> {
        self.choices
            .range((name.to_string(), String::new())..)
            .take_while(|((s, _), _)| s == name)
            .filter_map(|(_, &at)| match &self.records[at] {
                Record::Choice { of, pick, .. } => Some((of.as_str(), pick.as_str())),
                _ => None,
            })
            .collect()
    }

    pub fn set_choice(&mut self, name: &str, of: &str, pick: &str) -> Result<()> {
        if self.choice(name, of) != Some(pick) {
            self.push(Record::Choice {
                stack: name.into(),
                of: of.into(),
                pick: pick.into(),
            })?;
        }
        Ok(())
    }

    /// The read marks on `change` not taken back, oldest first, each with
    /// its rev.
    pub fn marks(&self, change: &str) -> Vec<(usize, &Mark)> {
        self.marks
            .get(change)
            .into_iter()
            .flatten()
            .filter_map(|&i| match &self.records[i] {
                Record::Read(m) => Some((i + 1, m)),
                _ => None,
            })
            .collect()
    }

    /// The lines of `change` marked read, by key.
    pub fn read_keys(&self, change: &str) -> std::collections::HashSet<String> {
        self.marks(change).into_iter().flat_map(|(_, m)| m.keys.iter().cloned()).collect()
    }

    /// Marks lines of `change` read, as one mark: its rev.
    pub fn mark_read(&mut self, change: &str, mut keys: Vec<String>) -> Result<usize> {
        keys.sort();
        keys.dedup();
        self.push(Record::Read(Mark {
            change: change.into(),
            keys,
            undo: None,
            time: now(),
        }))
    }

    /// Takes back the mark at `rev` on `change`.
    pub fn take_back(&mut self, change: &str, rev: usize) -> Result<()> {
        self.push(Record::Read(Mark {
            change: change.into(),
            keys: vec![],
            undo: Some(rev),
            time: now(),
        }))
        .map(|_| ())
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
        // By where each was first written: ids are not in time order.
        self.run_order.iter().filter_map(|id| self.run(id)).collect()
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

    /// How many lines run `id`'s agent wrote.
    pub fn event_count(&self, id: &str) -> usize {
        self.events.get(id).map_or(0, Vec::len)
    }

    /// How many records are not lines of an agent's stream: when it moves,
    /// something the editor shows may have changed.
    pub fn changes(&self) -> usize {
        self.changes
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
    /// landed ask's), once it is seen to apply: its name moves to it.
    pub fn add_node(&mut self, s: Stack) -> Result<usize> {
        if let Some(p) = s.parent {
            self.node(p)?;
        }
        check_name(&s.name)?;
        self.push_stack(s)
    }

    /// Puts `name` back on `rev` (or none), where it was before `add_node`
    /// moved it: no check, as nothing new is chosen.
    pub fn restore_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        self.push_head(name, rev, op)
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
            .is_some_and(|r| matches!(r, Record::Comment(n) if *n == Note { time: n.time, ..note.clone() }));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_never_made_twice() {
        // Many at once, in the same millisecond, from threads of one
        // process: as asks are launched away from the screen.
        let all: Vec<String> = (0..8)
            .map(|_| std::thread::spawn(|| (0..500).map(|_| unique()).collect::<Vec<_>>()))
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|t| t.join().unwrap())
            .collect();
        let distinct: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(distinct.len(), all.len());
    }

    #[test]
    fn a_thread_never_waits_on_its_own_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        // Locked again from within: said at once, rather than waited on for
        // ever, the outer edit going on.
        let inner = Db::edit(&path, |db| {
            db.set_view("s", "all")?;
            let again = Db::edit(&path, |_| Ok(()));
            let append = Db::append(&path, &[]);
            Ok((again.err().map(|e| e.0), append.err().map(|e| e.0)))
        })
        .unwrap();
        for e in [inner.0, inner.1] {
            assert!(e.is_some_and(|e| e.contains("locked already by this thread")));
        }
        // Given back after: the next takes it.
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        assert_eq!(Db::load(&path).unwrap().view("s"), Some("none"));
        // Two threads take it in turn: the second waits for the first.
        let started = std::time::Instant::now();
        let held = {
            let path = path.clone();
            std::thread::spawn(move || {
                Db::edit(&path, |_| {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    Ok(())
                })
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        assert!(started.elapsed() >= std::time::Duration::from_millis(300), "it waited its turn");
        held.join().unwrap().unwrap();
    }
}
