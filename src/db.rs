//! The database: one file of JSON lines, appended to, only its header ever
//! written over.
//!
//! A line is a record, named by its place among the records, its rev
//! (`@12`): the header, the first line, is @1. A checkpoint's line is no
//! record, and takes no rev.
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
//! The records are only ever appended, so a crash can only leave a last
//! line cut short. Such a line, or any line that cannot be read, is passed
//! over with a warning, and keeps its rev: the next write ends it with a
//! newline rather than cutting it off, and every rev after stays the same.
//! And a reader need only read what was appended since it last read.
//!
//! The header is 128 bytes long, its JSON padded with spaces, so that it
//! can be written over in place. A file an older stacked wrote is written
//! again once, the same lines under the new header, and renamed over the
//! old: so a process takes the lock, then sees that the file it locked is
//! still the one there, else opens it again.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 2: the agent's turns are fenced ```agent, its comments `by` "agent".
/// 3: the header is 128 bytes long, and checkpoints' lines take no rev.
pub const VERSION: u32 = 3;

/// How long the header is, its newline in.
const HEADER: usize = 128;

/// How a checkpoint's line starts: in a file of format 3 on, a line that
/// does is one, whether it reads or not, and takes no rev.
const CHECKPOINT: &[u8] = b"{\"t\":\"checkpoint\",";

/// The header of a new file: of the fixed length, its newline left out.
pub(crate) fn header_line() -> String {
    format!("{:<width$}", format!(r#"{{"t":"stacked","version":{VERSION},"checkpoint":null}}"#), width = HEADER - 1)
}

/// Where a record's line is in the file: its offset, and its length, the
/// newline left out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    at: u64,
    len: u64,
}

impl Pos {
    /// Not known: a record not yet written.
    const NONE: Pos = Pos { at: u64::MAX, len: 0 };
}

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
    /// A comment collapsed by hand, in the middle pane: one quiet line for
    /// it, until it is expanded.
    Collapse { change: String, id: String, collapsed: bool },
    /// The review of what run `run` brought back, as saved, until changed.
    Review { run: String, review: crate::held::Review },
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
    /// Written in the review of landed work, by its run: that work goes
    /// back to the agent with it, not accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
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
    /// Build and test, in the stack's build worktree; changes nothing in
    /// the stack.
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
/// stack's own comments, not only its patch; asked to answer only, it
/// answers with the whole stack in view.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Queued {
    pub way: Way,
    pub batch: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stack: bool,
    /// Batched, it waits for you to send the batch; ready, its turn.
    #[serde(default, skip_serializing_if = "Stage::is_ready")]
    pub stage: Stage,
    /// The landed work, by its run, whose stack the ask starts from: a
    /// round sent back from that work's review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// Asked again: the run before, whose landed work, dropped, the new
    /// agent is shown, to reuse what still holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
}

impl Queued {
    /// How it is said to the user: `answer only`, `may change the whole
    /// stack`.
    pub fn said(&self) -> &'static str {
        if self.stack && self.way == Way::Change { WHOLE } else { self.way.said() }
    }

    /// Batched: it waits for you to send the batch, not for its turn.
    pub fn batched(&self) -> bool {
        self.stage == Stage::Batch
    }
}

/// Where a comment sent stands, as in self-review: batched, until you send
/// the batch; or ready, to go as soon as nothing stands in its way.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Batch,
    #[default]
    Ready,
}

impl Stage {
    fn is_ready(&self) -> bool {
        *self == Stage::Ready
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
    /// For an ask landed: what it brought back, held until it is reviewed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<crate::held::Held>,
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
    records: Vec<Record>,
    patches: HashMap<String, usize>,
    /// Change to the index of its latest version.
    latest: HashMap<String, usize>,
    /// Every name ever used, to the index of the stack it is on, or none
    /// when it is deleted.
    heads: BTreeMap<String, Option<usize>>,
    /// Every stack, by its index: its name and its parent.
    stacks: BTreeMap<usize, (String, Option<usize>)>,
    /// Name to the revs of the stacks it has been on, oldest first: each
    /// made, each a head moved to.
    on: HashMap<String, Vec<usize>>,
    /// (change, comment id) to the index of its latest comment record.
    notes: BTreeMap<(String, String), usize>,
    /// (change, comment id) to the index of each of its records, oldest
    /// first: its history.
    note_records: HashMap<(String, String), Vec<usize>>,
    /// (change, comment id) to the index of its first record.
    note_first: HashMap<(String, String), usize>,
    /// (change, comment id) to how it was last sent, if it ever was.
    last_queued: HashMap<(String, String), Queued>,
    /// (change, comment id) to every line it was on.
    note_lines: HashMap<(String, String), Vec<String>>,
    /// Stack name to the index of its latest view record.
    views: HashMap<String, usize>,
    /// (stack name, what the choice is of) to the index of its latest
    /// choice record.
    choices: BTreeMap<(String, String), usize>,
    /// The comments collapsed: (change, comment id).
    collapsed: HashSet<(String, String)>,
    /// Run id to the index of its latest review record.
    reviews: HashMap<String, usize>,
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
    /// Where each record's line is: `pos[i]` rev `i + 1`'s.
    pos: Vec<Pos>,
    /// The bytes read.
    len: u64,
    /// The lines read, records or not.
    lines: usize,
    /// The last line read has no newline: the next write starts with one.
    torn: bool,
    /// Whether that line holds a record's place: a checkpoint's holds none.
    torn_slot: bool,
    /// Where that line starts.
    tail: u64,
    /// Records from here on are not yet on disk.
    saved: usize,
    /// The format the header says.
    version: u32,
    /// Whether the header is of the fixed length: else an older stacked
    /// wrote the file, and it is written again once, under a new one.
    fixed: bool,
    /// The file read, by device and inode: another renamed over it, it is
    /// read again whole.
    ino: Option<(u64, u64)>,
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

/// Opens the file at `path`, made if it is not there, and takes its lock;
/// then sees that it is still the file at `path`, else (another renamed
/// over it meanwhile, an older file written again) opens that one.
fn open_locked(path: &Path) -> Result<Lock> {
    let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
    for _ in 0..1000 {
        let f = File::options().create(true).append(true).read(true).open(path).map_err(io)?;
        take_lock(&f, path)?;
        let held = f.metadata();
        // Dropped unless kept: the lock given back.
        let lock = Lock(Held::File(f));
        let held = held.map_err(io)?;
        if std::fs::metadata(path).is_ok_and(|m| (m.dev(), m.ino()) == (held.dev(), held.ino())) {
            return Ok(lock);
        }
    }
    err(format!("{}: renamed over again and again while it was locked", path.display()))
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
        let mut ino = None;
        match File::open(path) {
            Ok(mut f) => {
                ino = f.metadata().ok().map(|m| (m.dev(), m.ino()));
                f.read_to_end(&mut bytes)
                    .map_err(|e| Error(format!("{}: {e}", path.display())))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return err(format!("{}: {e}", path.display())),
        }
        let mut db = Db::parse(path, &bytes)?;
        db.ino = ino;
        Ok(db)
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
            latest: HashMap::new(),
            heads: BTreeMap::new(),
            stacks: BTreeMap::new(),
            on: HashMap::new(),
            notes: BTreeMap::new(),
            note_records: HashMap::new(),
            note_first: HashMap::new(),
            last_queued: HashMap::new(),
            note_lines: HashMap::new(),
            views: HashMap::new(),
            choices: BTreeMap::new(),
            collapsed: HashSet::new(),
            reviews: HashMap::new(),
            marks: HashMap::new(),
            runs: BTreeMap::new(),
            run_order: Vec::new(),
            changes: 0,
            events: HashMap::new(),
            pos: Vec::new(),
            len: 0,
            lines: 0,
            torn: false,
            torn_slot: false,
            tail: 0,
            saved: 0,
            version: 0,
            fixed: false,
            ino: None,
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
            self.lines += 1;
            let no = self.lines;
            let at = |e: &str| format!("{path}:{no}: {e}");
            let pos = Pos {
                at: offset,
                len: line.len() as u64,
            };
            // A checkpoint's line: no record.
            let checkpoint = self.version >= 3 && line.starts_with(CHECKPOINT);
            // A last line with no newline is not read: a write in progress,
            // or one a crash cut short. It holds its record's place.
            if last {
                self.torn = true;
                self.torn_slot = !checkpoint;
                self.tail = offset;
                if !checkpoint {
                    self.warnings.push(at("cut short, passed over"));
                    self.push_broken(pos);
                }
                break;
            }
            offset += line.len() as u64 + 1;
            if checkpoint {
                continue;
            }
            let rec = std::str::from_utf8(line)
                .map_err(|e| e.to_string())
                .and_then(|l| serde_json::from_str::<Record>(l).map_err(|e| e.to_string()));
            let rec = match rec {
                Ok(r) => r,
                Err(e) => {
                    self.warnings.push(at(&format!("passed over: {e}")));
                    self.push_broken(pos);
                    continue;
                }
            };
            match (&rec, self.records.len()) {
                (Record::Stacked { version }, 0) if *version > VERSION => {
                    return err(at(&format!(
                        "written by a newer stacked (format {version}, this one reads {VERSION})"
                    )));
                }
                (Record::Stacked { version }, 0) => {
                    self.version = *version;
                    self.fixed = *version >= 3 && line.len() == HEADER - 1;
                }
                (_, 0) => return err(format!("{}: not a stacked database", self.path.display())),
                (Record::Stacked { .. }, _) => {
                    self.warnings.push(at("a second header, passed over"));
                    self.push_broken(pos);
                    continue;
                }
                _ => {}
            }
            if let Err(e) = self.index(rec, pos) {
                self.warnings.push(at(&format!("passed over: {e}")));
                self.push_broken(pos);
            }
        }
        self.len += bytes.len() as u64;
        self.saved = self.records.len();
        Ok(())
    }

    fn push_broken(&mut self, pos: Pos) {
        self.records.push(Record::Broken);
        self.pos.push(pos);
    }

    /// Forgets the line cut short, to read it again once it is whole.
    fn untear(&mut self) {
        if self.torn_slot {
            self.records.pop();
            self.pos.pop();
            self.warnings.pop();
        }
        self.lines -= 1;
        self.len = self.tail;
        self.torn = false;
        self.torn_slot = false;
    }

    /// Reads what was appended since: the file only grows, so only the new
    /// bytes, and the line that was cut short again. Whether there was any.
    pub fn catch_up(&mut self) -> Result<bool> {
        let path = self.path.clone();
        let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(&self.path) {
            let size = m.lock().unwrap().len() as u64;
            if size == self.len {
                return Ok(false);
            }
            if size < self.len {
                *self = Db::load(&self.path)?;
                return Ok(true);
            }
            if self.torn {
                self.untear();
            }
            let bytes = m.lock().unwrap()[self.len as usize..].to_vec();
            self.read(&bytes)?;
            return Ok(true);
        }
        let mut f = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Gone: an empty database.
                if self.len == 0 {
                    return Ok(false);
                }
                *self = Db::load(&self.path)?;
                return Ok(true);
            }
            Err(e) => return Err(io(e)),
        };
        let meta = f.metadata().map_err(io)?;
        let ino = (meta.dev(), meta.ino());
        // Another file renamed over it (an older one written again), or
        // the file cut, which is not ours to do: read it whole.
        if self.ino.is_some_and(|i| i != ino) || meta.len() < self.len {
            *self = Db::load(&self.path)?;
            return Ok(true);
        }
        self.ino = Some(ino);
        if meta.len() == self.len {
            return Ok(false);
        }
        if self.torn {
            self.untear();
        }
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(self.len)).map_err(io)?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).map_err(io)?;
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
        let mut lock = open_locked(path)?;
        let f = lock.file();
        let size = f.metadata().map_err(io)?.len();
        let mut out = String::new();
        if size == 0 {
            out.push_str(&header_line());
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

    fn index(&mut self, rec: Record, pos: Pos) -> Result<()> {
        let at = self.records.len();
        if !matches!(rec, Record::Event { .. }) {
            self.changes += 1;
        }
        match &rec {
            Record::Stacked { .. } | Record::Broken => {}
            Record::Patch(p) => {
                self.patches.entry(p.id.clone()).or_insert(at);
                self.latest.insert(p.change.clone(), at);
            }
            Record::Stack(s) => {
                if let Some(id) = s.patches.iter().find(|id| !self.patches.contains_key(*id)) {
                    return err(format!("stack {} names patch {id}, which is not stored", s.name));
                }
                self.heads.insert(s.name.clone(), Some(at));
                self.stacks.insert(at, (s.name.clone(), s.parent));
                self.on.entry(s.name.clone()).or_default().push(at + 1);
            }
            Record::Head(h) => {
                if let Some(rev) = h.rev {
                    match self.rev(rev) {
                        Some(Record::Stack(s)) if s.name == h.name => {}
                        _ => return err(format!("@{rev} is not a stack of {}", h.name)),
                    }
                    self.on.entry(h.name.clone()).or_default().push(rev);
                }
                self.heads.insert(h.name.clone(), h.rev.map(|r| r - 1));
            }
            Record::Comment(n) => {
                let key = (n.change.clone(), n.id.clone());
                self.note_records.entry(key.clone()).or_default().push(at);
                self.note_first.entry(key.clone()).or_insert(at);
                if let Some(q) = &n.queued {
                    self.last_queued.insert(key.clone(), q.clone());
                }
                let lines = self.note_lines.entry(key.clone()).or_default();
                if !lines.contains(&n.line) {
                    lines.push(n.line.clone());
                }
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
            Record::Collapse { change, id, collapsed } => {
                let key = (change.clone(), id.clone());
                if *collapsed {
                    self.collapsed.insert(key);
                } else {
                    self.collapsed.remove(&key);
                }
            }
            Record::Review { run, .. } => {
                self.reviews.insert(run.clone(), at);
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
        self.pos.push(pos);
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
        // Given back if reading fails: the lock first.
        let mut lock = open_locked(path)?;
        let mut bytes = Vec::new();
        lock.file()
            .read_to_end(&mut bytes)
            .map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let mut db = Db::parse(path, &bytes)?;
        db.ino = lock.file().metadata().ok().map(|m| (m.dev(), m.ino()));
        if db.needs_upgrade() {
            match db.upgrade(&bytes, &mut lock) {
                Ok(Some(new)) => db = new,
                Ok(None) => {}
                Err(e) => db.warnings.push(format!("{}: not written again under a new header: {}", path.display(), e.0)),
            }
        }
        Ok((db, lock))
    }

    /// Whether an older stacked wrote the file, its header not of the
    /// fixed length: it is written again under a new one.
    fn needs_upgrade(&self) -> bool {
        !self.fixed && matches!(self.records.first(), Some(Record::Stacked { .. }))
    }

    /// Writes the file again, the same lines under the fixed header, and
    /// renames it over the old one, `lock` then holding the new one's lock;
    /// the database read from it. Not while an ask is going: its process
    /// could be an older stacked's, which reads no file of a newer format.
    fn upgrade(&self, bytes: &[u8], lock: &mut Lock) -> Result<Option<Db>> {
        if self.live_runs().iter().any(|r| crate::ask::busy(r)) {
            return Ok(None);
        }
        let Some(nl) = bytes.iter().position(|&b| b == b'\n') else { return Ok(None) };
        let path = &self.path;
        let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = path.with_file_name(format!("{name}.upgrade-{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut f = File::options().create_new(true).read(true).append(true).open(&tmp).map_err(io)?;
        // Locked before it is there to open: whoever opens it next waits.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(std::io::Error::last_os_error()));
        }
        let mut new = header_line().into_bytes();
        new.push(b'\n');
        new.extend_from_slice(&bytes[nl + 1..]);
        if let Err(e) = f.write_all(&new).and_then(|_| f.sync_all()).and_then(|_| std::fs::rename(&tmp, path)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(e));
        }
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        // The old file's lock given back: whoever waits on it sees it is
        // not the file there, and opens this one.
        if let Held::File(old) = std::mem::replace(&mut lock.0, Held::File(f)) {
            unsafe { libc::flock(old.as_raw_fd(), libc::LOCK_UN) };
        }
        let mut db = Db::parse(path, &new)?;
        db.ino = lock.file().metadata().ok().map(|m| (m.dev(), m.ino()));
        Ok(Some(db))
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
        for i in self.saved..self.records.len() {
            let line = match &self.records[i] {
                // A new file's header: of the fixed length.
                Record::Stacked { .. } if i == 0 => header_line(),
                rec => serde_json::to_string(rec).unwrap(),
            };
            self.pos[i] = Pos {
                at: self.len + out.len() as u64,
                len: line.len() as u64,
            };
            out.push_str(&line);
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
        self.lines += self.records.len() - self.saved;
        self.torn = false;
        self.torn_slot = false;
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
            self.index(Record::Stacked { version: VERSION }, Pos::NONE)?;
            (self.version, self.fixed) = (VERSION, true);
        }
        self.index(rec, Pos::NONE)?;
        Ok(self.records.len())
    }

    /// Where the line of the record at `rev` ends, its newline in: what a
    /// file holding the records up to `rev` would be cut at.
    pub fn end_of(&self, rev: usize) -> Option<u64> {
        let p = self.pos.get(rev.checked_sub(1)?).filter(|p| **p != Pos::NONE)?;
        Some(p.at + p.len + 1)
    }

    /// The last rev: how many records there are.
    pub fn last_rev(&self) -> usize {
        self.records.len()
    }

    /// Every record, `records()[i]` rev `i + 1`.
    pub fn records(&self) -> &[Record] {
        &self.records
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

    /// The latest version of `change`.
    pub fn latest_version(&self, change: &str) -> Option<&Patch> {
        match &self.records[*self.latest.get(change)?] {
            Record::Patch(p) => Some(p),
            _ => None,
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
        self.on.get(name)?.last().copied()
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
        self.on
            .get(name)?
            .iter()
            .rev()
            .find_map(|&later| self.child_towards(rev, later))
            .and_then(|child| Some((child, self.node(child).ok()?.op.clone())))
    }

    /// The child of `from` on the way to `to`, if `to` descends from it.
    fn child_towards(&self, from: usize, to: usize) -> Option<usize> {
        let mut at = to;
        loop {
            let parent = self.stacks.get(&at.checked_sub(1)?)?.1?;
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

    /// The rev of comment `id` on `change`'s first record: when it was
    /// opened.
    pub fn note_first(&self, change: &str, id: &str) -> Option<usize> {
        self.note_first.get(&(change.to_string(), id.to_string())).map(|&at| at + 1)
    }

    /// The rev of comment `id` on `change`'s latest record: when it was
    /// last written.
    pub fn note_rev(&self, change: &str, id: &str) -> Option<usize> {
        self.notes.get(&(change.to_string(), id.to_string())).map(|&at| at + 1)
    }

    /// How comment `id` on `change` was last sent, if it ever was.
    pub fn last_queued(&self, change: &str, id: &str) -> Option<&Queued> {
        self.last_queued.get(&(change.to_string(), id.to_string()))
    }

    /// Whether a comment, open or closed, was ever on line `line` of
    /// `change`.
    pub fn ever_on(&self, change: &str, line: &str) -> bool {
        self.notes
            .range((change.to_string(), String::new())..)
            .take_while(|((c, _), _)| c == change)
            .any(|(key, _)| self.note_lines.get(key).is_some_and(|ls| ls.iter().any(|l| l == line)))
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

    /// Whether comment (change, id) is collapsed, in the middle pane.
    pub fn collapsed(&self, change: &str, id: &str) -> bool {
        self.collapsed.contains(&(change.to_string(), id.to_string()))
    }

    pub fn set_collapsed(&mut self, change: &str, id: &str, collapsed: bool) -> Result<()> {
        if self.collapsed(change, id) != collapsed {
            self.push(Record::Collapse {
                change: change.into(),
                id: id.into(),
                collapsed,
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

    /// The runs whose agent is going, or whose work waits for your review,
    /// oldest started first: those under way, not those done with.
    pub fn live_runs(&self) -> Vec<&Run> {
        self.runs().into_iter().filter(|r| r.state == RunState::Started || crate::held::waiting(self, r)).collect()
    }

    /// Whether a run was ever written.
    pub fn has_runs(&self) -> bool {
        !self.run_order.is_empty()
    }

    /// Every run's id, oldest started first.
    pub fn run_ids(&self) -> impl Iterator<Item = &str> {
        self.run_order.iter().map(String::as_str)
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

    /// The review of what run `id` brought back, as last saved.
    pub fn review(&self, id: &str) -> Option<&crate::held::Review> {
        match &self.records[*self.reviews.get(id)?] {
            Record::Review { review, .. } => Some(review),
            _ => None,
        }
    }

    pub fn set_review(&mut self, id: &str, review: crate::held::Review) -> Result<usize> {
        self.push(Record::Review { run: id.into(), review })
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

    /// The comments of stack `name` on `change` that were deleted, oldest
    /// first: each as it was last, with the rev of the record that deleted
    /// it. A comment the agent moved to the stack is not one of them, nor
    /// one moved back to where it came from.
    pub fn deleted_notes(&self, name: &str, change: &str) -> Vec<(usize, Note)> {
        // Where the comments on the stack were moved from.
        let origins: std::collections::HashSet<(String, String)> = self
            .stack_notes(name)
            .iter()
            .filter_map(|n| n.moved.as_ref().map(|m| (m.change.clone(), m.id.clone())))
            .collect();
        let mut out = Vec::new();
        for ((c, id), &at) in self.notes.range((change.to_string(), String::new())..).take_while(|((c, _), _)| c == change) {
            match &self.records[at] {
                Record::Comment(n) if n.text.is_empty() => {}
                _ => continue,
            }
            if origins.contains(&(c.clone(), id.clone())) {
                continue;
            }
            let Some((_, was)) = self.note_history(c, id).into_iter().rev().find(|(_, n)| !n.text.is_empty()) else {
                continue;
            };
            // Moved back: where it came from has it again.
            if was.moved.as_ref().is_some_and(|m| self.note(&m.change, &m.id).is_some()) {
                continue;
            }
            out.push((at + 1, was.clone()));
        }
        out.sort_by_key(|(rev, _)| *rev);
        out
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
    fn a_deleted_comment_is_kept_as_it_was_and_a_moved_one_is_not_one() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let note = |id: &str, text: &str| Note {
            change: "c".into(),
            id: id.into(),
            line: id.into(),
            text: text.into(),
            ..Default::default()
        };
        Db::edit(&path, |db| {
            db.set_note(note("a", "first"))?;
            db.set_note(note("a", "first, then more"))?;
            db.set_note(note("a", ""))?;
            db.set_note(note("b", "kept"))?;
            // Moved to the stack, as the agent moves one: not deleted.
            db.set_note(note("m", "needs the stack"))?;
            crate::ask::promote(db, "s", &note("m", "needs the stack"), "it needs more")
        })
        .unwrap();
        let db = Db::load(&path).unwrap();
        let deleted = db.deleted_notes("s", "c");
        assert_eq!(deleted.len(), 1, "{deleted:?}");
        let (rev, was) = deleted[0].clone();
        assert_eq!((was.id.as_str(), was.text.as_str()), ("a", "first, then more"));
        assert_eq!(db.note_history("c", "a").last().map(|(r, _)| *r), Some(rev), "the rev that deleted it");
        // Moved back: neither it nor the stack's copy of it is deleted.
        Db::edit(&path, |db| {
            let moved = db.stack_notes("s").into_iter().find(|n| n.moved.is_some()).cloned().unwrap();
            crate::ask::move_back(db, &moved)
        })
        .unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!(db.deleted_notes("s", "c").len(), 1);
        assert!(db.deleted_notes("s", &stack_change("s")).is_empty());
        // Brought back: not deleted.
        Db::edit(&path, |db| db.set_note(was.clone())).unwrap();
        assert!(Db::load(&path).unwrap().deleted_notes("s", "c").is_empty());
    }

    #[test]
    fn the_header_is_of_the_fixed_length() {
        assert_eq!(header_line().len(), HEADER - 1);
        let db = Db::from_bytes(Path::new("db.jsonl"), format!("{}\n", header_line()).as_bytes()).unwrap();
        assert!(db.fixed && db.version == VERSION && db.warnings.is_empty(), "{:?}", db.warnings);
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..HEADER], format!("{}\n", header_line()).as_bytes());
    }

    #[test]
    fn a_checkpoints_line_takes_no_rev() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let add = |bytes: &[u8]| File::options().append(true).open(&path).unwrap().write_all(bytes).unwrap();
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        // A checkpoint's line, whatever it holds; a record; another cut
        // short.
        add(b"{\"t\":\"checkpoint\",\"state\":99}\n");
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        add(b"{\"t\":\"checkpoint\",\"sta");
        let db = Db::load(&path).unwrap();
        assert!(db.warnings.is_empty(), "{:?}", db.warnings);
        assert_eq!(db.last_rev(), 3, "the header and two views");
        assert!(matches!(db.rev(3), Some(Record::View { show, .. }) if show == "none"));
        // The next write ends the line cut short; its record is the next
        // rev.
        let rev = Db::edit(&path, |db| {
            db.set_view("s", "all")?;
            Ok(db.last_rev())
        })
        .unwrap();
        assert_eq!(rev, 4);
        let db = Db::load(&path).unwrap();
        assert_eq!((db.last_rev(), db.view("s")), (4, Some("all")));
        assert!(db.warnings.is_empty(), "{:?}", db.warnings);
        // A line that does not read keeps its rev, and is said by its line.
        add(b"garbage\n");
        let db = Db::load(&path).unwrap();
        assert_eq!(db.last_rev(), 5);
        assert!(db.warnings[0].contains(":7: passed over"), "{:?}", db.warnings);
    }

    /// The file as stacked 2 wrote it, of `lines` after its header.
    fn old_file(path: &Path, lines: &[String]) -> String {
        let text = format!("{{\"t\":\"stacked\",\"version\":2}}\n{}\n", lines.join("\n"));
        std::fs::write(path, &text).unwrap();
        text
    }

    fn view(show: &str) -> String {
        serde_json::to_string(&Record::View {
            stack: "s".into(),
            show: show.into(),
        })
        .unwrap()
    }

    #[test]
    fn an_older_file_is_written_again_under_the_fixed_header_its_revs_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let old = old_file(&path, &[view("all"), "garbage".into(), view("none")]);
        let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
        let was = ino(&path);
        let mut reader = Db::load(&path).unwrap();
        assert!(!reader.fixed);
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..HEADER], format!("{}\n", header_line()).as_bytes());
        assert!(bytes[HEADER..].starts_with(&old.as_bytes()[old.find('\n').unwrap() + 1..]), "every line after it as it was");
        assert_ne!(ino(&path), was, "a new file, renamed over the old");
        let db = Db::load(&path).unwrap();
        assert_eq!(db.last_rev(), 5);
        assert!(matches!(db.rev(3), Some(Record::Broken)));
        assert_eq!(db.view("s"), Some("all"));
        assert_eq!(db.warnings.len(), 1, "{:?}", db.warnings);
        // A reader of the old file reads the new one whole.
        assert!(reader.catch_up().unwrap());
        assert_eq!(reader.records(), db.records());
        assert!(reader.fixed);
        // Once only, and nothing left behind.
        let now = ino(&path);
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        assert_eq!(ino(&path), now);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn an_older_file_is_written_again_only_once_no_ask_is_going() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        // An ask going: its process could be an older stacked's.
        let run = Run {
            id: "r".into(),
            what: "edit".into(),
            stack: "s".into(),
            state: RunState::Started,
            pid: Some(std::process::id()),
            time: 1,
            ..Default::default()
        };
        let old = old_file(&path, &[serde_json::to_string(&Record::Run(run.clone())).unwrap()]);
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(old.as_bytes()), "not written again");
        Db::edit(&path, |db| {
            db.add_run(Run {
                state: RunState::Done,
                pid: None,
                ..run
            })
            .map(|_| ())
        })
        .unwrap();
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.fixed);
        assert_eq!((db.last_rev(), db.view("s")), (5, Some("none")));
    }

    #[test]
    fn a_process_waiting_on_the_lock_follows_the_file_renamed_over_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        // Another process holds the lock...
        let held = File::open(&path).unwrap();
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
        let edit = {
            let path = path.clone();
            std::thread::spawn(move || Db::edit(&path, |db| db.set_view("s", "none")))
        };
        let append = {
            let path = path.clone();
            std::thread::spawn(move || {
                Db::append(
                    &path,
                    &[Record::Event {
                        run: "r".into(),
                        event: serde_json::json!({"type": "x"}),
                    }],
                )
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        // ...renames another file over it, as one written again is, and
        // gives the lock back.
        let other = tmp.path().join("other");
        std::fs::copy(&path, &other).unwrap();
        std::fs::rename(&other, &path).unwrap();
        drop(held);
        edit.join().unwrap().unwrap();
        append.join().unwrap().unwrap();
        // What waited went in the file there.
        let db = Db::load(&path).unwrap();
        assert_eq!(db.view("s"), Some("none"));
        assert_eq!(db.events("r").len(), 1);
        assert!(db.warnings.is_empty(), "{:?}", db.warnings);
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
