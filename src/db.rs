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
//! - No stack goes in, nor does a head move, that would change a patch
//!   that should not change, its revision Accepted or Landed on Phabricator
//!   (`protected`), unless the edit was confirmed for that patch
//!   (`Db::confirm`): refused, as an error of its own kind
//!   (`Db::edit_confirmed`), nothing written.
//! - A comment is on a line of a change, kept as the change gets new
//!   versions; the latest record for a line is the comment.
//! - What a read of Phabricator learnt of a revision, its state and its
//!   comments, is a record of its own, written only where it differs from
//!   what is stored; the latest record with each is what stands. A
//!   revision's raw latest diff is stored once for each diff id, and a
//!   fetch that found a revision as stored writes only a small record of
//!   when it read it. A repository on Phabricator a submit makes a new
//!   diff in is read once, and stored once, as none of it changes. A
//!   submit that changed a revision writes a small record of when: until
//!   a read of the revision is written after it, what is stored of the
//!   revision was read before that submit.
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
//!
//! The header points at the last checkpoint: a line that takes no rev,
//! saying where the records are that the state stands on (each stack's
//! head and the stacks one undo and one redo go to, the comments open, the
//! runs under way and what they name, what is chosen and marked read, where
//! stacked last left HEAD of each checkout, what stands of each revision
//! read from Phabricator and the raw diff its state names, the latest
//! submit that changed it, the repositories read from it) and how the
//! indexes stood. A reader reads the header, the checkpoint, those
//! records and the lines after it, not the whole file; the records it did
//! not read are `Unloaded`. What needs the history (an older stack, the
//! log, versions of a patch, a comment's history, the comments closed, the
//! runs done with) reads the whole file the first time it is asked, and
//! anything that changes the database takes that in first: an accessor
//! never answers from part of the history. A checkpoint is written, then
//! the header pointed at it, once the file has grown four times the last
//! one's length past it (and at least 256 KB), and when a head moves. A
//! crash in between leaves the header pointing at the one before; a header
//! or checkpoint that does not check out is passed over, the file read
//! whole.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 2: the agent's turns are fenced ```agent, its comments `by` "agent".
/// 3: the header is 128 bytes long, and checkpoints' lines take no rev.
pub const VERSION: u32 = 3;

/// How long the header is, its newline in.
const HEADER: usize = 128;

/// How a checkpoint's line starts: a line that does is one, whether it
/// reads or not, and takes no rev. No record's line starts so.
const CHECKPOINT: &[u8] = b"{\"t\":\"checkpoint\",";

/// The layout of a checkpoint: one of another is passed over.
const STATE: u32 = 1;

/// The least the file grows past the last checkpoint before a new one.
const FLOOR: u64 = 256 * 1024;

/// The header, of the fixed length, its newline left out, pointing at
/// checkpoint `at` if any: none when the pointer does not fit.
fn header_with(at: Option<&Pointer>) -> Option<String> {
    let checkpoint = at.map_or("null".to_string(), |p| serde_json::to_string(p).unwrap());
    let line = format!(r#"{{"t":"stacked","version":{VERSION},"checkpoint":{checkpoint}}}"#);
    (line.len() < HEADER).then(|| format!("{line:<width$}", width = HEADER - 1))
}

/// The header of a new file.
pub(crate) fn header_line() -> String {
    header_with(None).expect("an empty header fits")
}

/// The header as read: the format, and where the last checkpoint is.
#[derive(Deserialize)]
struct Header {
    version: u32,
    #[serde(default)]
    checkpoint: Option<Pointer>,
}

/// Where the last checkpoint is: its line's offset and length (its newline
/// left out), the records before it, and a sum of those and of its line,
/// which a header cut short, or pointing elsewhere, does not match.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Pointer {
    at: u64,
    rev: usize,
    len: u64,
    sum: String,
}

impl Pointer {
    fn new(at: u64, rev: usize, line: &[u8]) -> Pointer {
        let len = line.len() as u64;
        Pointer {
            at,
            rev,
            len,
            sum: sum(at, rev, len, line),
        }
    }
}

/// The line `bytes` hold, between the newline before it and its own: none
/// if they are not so.
fn whole_line(bytes: &[u8]) -> Option<&[u8]> {
    let line = bytes.strip_prefix(b"\n")?.strip_suffix(b"\n")?;
    (!line.contains(&b'\n')).then_some(line)
}

fn sum(at: u64, rev: usize, len: u64, line: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("{at} {rev} {len}\n").as_bytes());
    h.update(line);
    h.digest().to_string()[..16].to_string()
}

/// A checkpoint: where the records are that the state stood on after its
/// `rev` records, and how the indexes stood. Revs, not indices.
#[derive(Serialize, Deserialize, Default, Debug)]
struct Checkpoint {
    /// "checkpoint", first, so that its line starts as a checkpoint's.
    t: String,
    state: u32,
    /// The records before it.
    rev: usize,
    /// The lines before it, the header's in.
    lines: usize,
    changes: usize,
    /// The records read with it: rev, offset and length.
    load: Vec<(usize, u64, u64)>,
    heads: Vec<(String, Option<usize>)>,
    patches: Vec<(String, usize)>,
    latest: Vec<(String, usize)>,
    /// Every stack: its rev, name and parent.
    stacks: Vec<(usize, String, Option<usize>)>,
    on: Vec<(String, Vec<usize>)>,
    /// Every comment: change, id, the rev of its latest record and of its
    /// first.
    notes: Vec<(String, String, usize, usize)>,
    queued: Vec<(String, String, Queued)>,
    note_lines: Vec<(String, String, Vec<String>)>,
    views: Vec<(String, usize)>,
    choices: Vec<(String, String, usize)>,
    /// Where stacked last left HEAD: the stack, the checkout, and the rev
    /// of its latest record. None in a checkpoint a stacked of before
    /// wrote, as such a stacked wrote none of them.
    #[serde(default)]
    moved: Vec<(String, String, usize)>,
    /// What was read of Phabricator's revisions, each by its link: the rev
    /// of its latest record with its state, and of its latest with its
    /// comments; the rev of each raw diff stored, by link and diff id; and
    /// the rev of the latest fetch that read each. None in a checkpoint a
    /// stacked of before wrote, as such a stacked wrote none of them.
    #[serde(default)]
    revisions: Vec<(String, usize)>,
    #[serde(default)]
    revision_comments: Vec<(String, usize)>,
    #[serde(default)]
    raw_diffs: Vec<(String, u64, usize)>,
    #[serde(default)]
    fetched: Vec<(String, usize)>,
    /// The rev of the latest record of each repository read from
    /// Phabricator, by its PHID. None in a checkpoint a stacked of before
    /// wrote, as such a stacked wrote none of them.
    #[serde(default)]
    repositories: Vec<(String, usize)>,
    /// The rev of the latest submit that changed each revision on
    /// Phabricator, by its link. None in a checkpoint a stacked of before
    /// wrote, as such a stacked wrote none of them.
    #[serde(default)]
    submitted: Vec<(String, usize)>,
    collapsed: Vec<(String, String)>,
    reviews: Vec<(String, usize)>,
    marks: Vec<(String, Vec<usize>)>,
    /// Every run, oldest started first, and the rev of its latest record.
    runs: Vec<(String, usize)>,
    /// The runs whose events are read with it, and those events' revs.
    events: Vec<(String, Vec<usize>)>,
    /// The runs with events not read with it.
    unread: Vec<String>,
    warnings: Vec<String>,
}

/// What a database read from a checkpoint did not read.
#[derive(Debug, Default)]
struct Partial {
    /// The runs with events not read: the others' are all there.
    unread: HashSet<String>,
    /// A review written since of a run not read: what is under way is then
    /// known only from the whole history.
    stale: bool,
}

/// The file a database was read from, to read more of it: the file as it
/// was opened (another may have been renamed over it since), or one kept
/// in memory.
enum Src {
    File(File),
    #[cfg(feature = "demo")]
    Memory(std::sync::Arc<std::sync::Mutex<Vec<u8>>>),
}

impl Src {
    /// The `len` bytes from `at`.
    fn read(&self, at: u64, len: u64) -> std::io::Result<Vec<u8>> {
        match self {
            Src::File(f) => {
                let mut buf = vec![0; len as usize];
                f.read_exact_at(&mut buf, at)?;
                Ok(buf)
            }
            #[cfg(feature = "demo")]
            Src::Memory(m) => {
                let bytes = m.lock().unwrap();
                let end = at.checked_add(len).filter(|&e| e <= bytes.len() as u64);
                end.map(|e| bytes[at as usize..e as usize].to_vec()).ok_or_else(|| std::io::ErrorKind::UnexpectedEof.into())
            }
        }
    }

    /// The shared lock for a read of it, the file at `path` (`reading`):
    /// none for a file kept in memory, each write of which is whole as it
    /// is made, under the lock of its bytes.
    fn reading(&self, path: &Path) -> Result<Option<Reading>> {
        match self {
            Src::File(f) => reading(f, path).map(Some),
            #[cfg(feature = "demo")]
            Src::Memory(_) => Ok(None),
        }
    }

    /// Every byte from `at` on.
    fn rest(&self, at: u64) -> std::io::Result<Vec<u8>> {
        match self {
            Src::File(f) => {
                let (mut out, mut buf, mut pos) = (Vec::new(), vec![0; 1 << 20], at);
                loop {
                    let n = f.read_at(&mut buf, pos)?;
                    if n == 0 {
                        return Ok(out);
                    }
                    out.extend_from_slice(&buf[..n]);
                    pos += n as u64;
                }
            }
            #[cfg(feature = "demo")]
            Src::Memory(m) => Ok(m.lock().unwrap().get(at as usize..).map_or(vec![], <[u8]>::to_vec)),
        }
    }
}

/// Under test: a checkpoint at every write, and each database read from one
/// read whole too, the two the same or a panic. The whole suite runs so
/// with `STACKED_TEST_CHECKPOINTS` set.
fn test_checkpoints() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STACKED_TEST_CHECKPOINTS").is_some())
}

/// What run `r` names: the stacks (by rev), the patches, the runs.
fn named_by(r: &Run) -> (Vec<usize>, Vec<String>, Vec<String>) {
    let (mut revs, mut patches, mut runs): (Vec<usize>, Vec<String>, Vec<String>) = (r.rev.into_iter().collect(), vec![], vec![]);
    if let Some(c) = r.held.as_ref().and_then(|h| h.stack.as_ref()) {
        revs.extend(c.from);
        patches.extend(c.patches.iter().cloned());
    }
    // The follow-ups that came into its review: a try push's of them is
    // what closing the review pushes as.
    if let Some(h) = &r.held {
        runs.extend(h.follow_ups.iter().map(|f| f.run.clone()));
    }
    if let Some(a) = &r.ask {
        revs.extend(a.get("from").and_then(|f| f.as_u64()).map(|f| f as usize));
        patches.extend(a.get("patch").and_then(|p| p.get(0)).and_then(|p| p.as_str()).map(str::to_string));
        for c in a.get("commits").and_then(|c| c.as_array()).into_iter().flatten() {
            patches.extend(c.get(1).and_then(|p| p.as_str()).map(str::to_string));
        }
        for k in ["on", "after", "was"] {
            runs.extend(a.get(k).and_then(|v| v.as_str()).map(str::to_string));
        }
    }
    // A try push: the stack it pushes, which it reads as it was.
    #[cfg(feature = "try-push")]
    if let Some(p) = &r.push {
        revs.push(p.rev);
    }
    // A lint job: the stack it lints, once it runs.
    if let Some(j) = &r.lint {
        revs.extend(j.rev);
    }
    // A reconcile: the work it went on from.
    if let Some(j) = &r.reconcile {
        runs.extend(j.after.clone());
    }
    (revs, patches, runs)
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

/// Why an edit of the database did not go (`Db::edit_confirmed`): what
/// went wrong, said; or, an error of its own kind, a write of a stack
/// refused as it would change patches that should not change, none of them
/// confirmed for it (`protected::Refused`), which names them.
#[derive(Debug)]
pub enum Failed {
    Error(Error),
    Protected(Box<crate::protected::Refused>),
}

impl From<Failed> for Error {
    fn from(f: Failed) -> Error {
        match f {
            Failed::Error(e) => e,
            Failed::Protected(r) => Error(r.said()),
        }
    }
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
    /// The commit HEAD of the checkout at `checkout` (its top) was at right
    /// after stacked last moved it on stack `stack`, by Move HEAD or
    /// following a review: each move replaces it. A database a stacked of
    /// before wrote has none.
    Moved { stack: String, checkout: String, commit: String },
    /// Lines of a patch marked read, in the reading mode; or a mark taken
    /// back.
    Read(Mark),
    /// What a read of Phabricator learnt of a revision (Fetch the latest
    /// from Phabricator, a submit, a pull): its state, its comments, or
    /// both, each only when it differs from what is stored. The latest
    /// record with each is what stands. A database a stacked of before
    /// wrote has none.
    Revision(Revision),
    /// The raw latest diff of the revision at `link` on Phabricator, of id
    /// `diff`: stored once for each id, as a diff never changes.
    RawDiff {
        #[serde(default)]
        link: String,
        #[serde(default)]
        diff: u64,
        #[serde(default)]
        raw: String,
    },
    /// When Fetch the latest from Phabricator read the revisions at
    /// `links`, whether it stored anything of them or found each as it was
    /// stored.
    Fetched {
        #[serde(default)]
        links: Vec<String>,
        #[serde(default)]
        time: i64,
    },
    /// A repository on Phabricator, as a submit read it to make a new diff
    /// in it: stored once, as none of it changes. A database a stacked of
    /// before wrote has none.
    Repository(Repository),
    /// That a submit changed the revision at `link` on Phabricator, at
    /// `time`: its edit made, or its edit back to Changes Planned, or the
    /// revision made by it. The latest of a revision is what stands; while
    /// no read of the revision is stored after it, what is stored of the
    /// revision was read before that submit (`Db::submitted_unread`). A
    /// database a stacked of before wrote has none.
    Submitted {
        #[serde(default)]
        link: String,
        #[serde(default)]
        time: i64,
    },
    /// A line of what a run's claude wrote, as it wrote it.
    Event {
        run: String,
        event: serde_json::Value,
    },
    /// A line that could not be read.
    #[serde(skip)]
    Broken,
    /// A record not read: the file was read from a checkpoint, and the
    /// state did not need it.
    #[serde(skip)]
    Unloaded,
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

/// What a read of Phabricator learnt of one revision, as a record of its
/// own (`Record::Revision`): its state, its comments, or both; none of a
/// part the read did not read, or found as it was stored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Revision {
    /// The revision's link, as a patch's trailer names it: what it is
    /// stored by.
    #[serde(default)]
    pub link: String,
    /// `D123`.
    #[serde(default)]
    pub d: String,
    /// What its search, its edges and its diffs said of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
    /// Its comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<Comments>,
    /// When it was read, in seconds since 1970.
    #[serde(default)]
    pub time: i64,
}

/// A revision's state on Phabricator, as a read stored it (`Revision`):
/// its status, title, summary and bug, its PHID, its parents, the id of
/// its latest diff, whether it is a draft, and its repository. Or that
/// Phabricator does not have it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct State {
    /// Phabricator has no such revision: the rest is empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    /// Its status, as differential.revision.search writes its value:
    /// `needs-review`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Its bug, empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bug: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub phid: String,
    /// Its parents, each its PHID and its `D123`, or its PHID again where
    /// it was not named.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<(String, String)>,
    /// The id of its latest diff: 0 for none, or none read.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub diff: u64,
    /// The commit its latest diff is on, as Phabricator names it
    /// (`sourceControlBaseRevision`): empty where it said none, or what
    /// read it did not read that.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base: String,
    /// Whether it is a draft, as its search said (`isDraft`): Phabricator
    /// says so of every revision it has not sent to reviewers yet, whatever
    /// its status. None in a state stored before stacked read that, which a
    /// submit has fetched again first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    /// The PHID of its repository, as its search said (`repositoryPHID`):
    /// empty where it said none, and in a state stored before stacked read
    /// that.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
}

/// A repository on Phabricator, as a submit read it once
/// (diffusion.repository.search), to make a new diff in it: its PHID; its
/// callsign, as the search gave it, or the one it was asked by, empty for
/// none; what holds it, `git` or `hg`; and when it was read, in seconds
/// since 1970. None of it changes, so that it is never read again.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Repository {
    #[serde(default)]
    pub phid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub callsign: String,
    #[serde(default)]
    pub vcs: String,
    #[serde(default)]
    pub time: i64,
}

/// A revision's comments, as a read stored them (`Revision`): what the
/// count of those not pulled yet needs, and when the revision was last
/// modified as they were read.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Comments {
    /// When the revision was last modified as they were read, as its
    /// search said (`dateModified`, seconds since 1970): none when the read
    /// did not say, as a pull does not. While it stays the same, a fetch
    /// does not read them again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list: Vec<Seen>,
}

/// A comment of a revision, as a read stored it (`Comments`): the id of
/// its version as it was last edited, the ids of its versions before, the
/// newest first, and the comment it replies to, if it does.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Seen {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
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

/// Where a state of a stack stands from the state the stack is on now
/// (`Db::since`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Since {
    /// The state it is on was edited from it, this many edits ago: none
    /// when it is that state.
    Edits(usize),
    /// The state it is on was not edited from it: it is another version of
    /// the stack, an edit undone and another made in its place, say.
    Elsewhere,
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
    /// The references you attached to it, which what you write names
    /// (`@r1`): other comments of the stack, lines of files, web links. The
    /// agent reads them with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attached: Vec<crate::refs::Attached>,
    /// The highest number one of its references was given, once that
    /// reference is gone (detached, or gone with its turn): no reference
    /// takes it again, nor one below it. None while a reference it has
    /// holds the highest, as in a comment saved before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<usize>,
    pub time: i64,
}

impl Note {
    /// Whether it is on the stack itself, not a patch.
    pub fn on_stack(&self) -> bool {
        self.change.starts_with(STACK)
    }

    /// Where on its patch it is, said: `a.txt: TWO`, its file and line; on
    /// a file's name, `a.txt, as a whole`.
    pub fn where_said(&self) -> String {
        where_said(&self.line, &self.file, &self.preview)
    }
}

/// Where a comment on `line` (its key), in `file`, showing `preview`, is
/// on its patch, said.
fn where_said(line: &str, file: &str, preview: &str) -> String {
    match crate::diff::is_file_key(line) {
        true => format!("{file}, as a whole"),
        false => format!("{file}: {preview}"),
    }
}

/// What claude may do with a comment sent to it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    /// Answer, and change the code within the comment's scope.
    Change,
    /// Answer only: read-only. Built without the `try-push` feature, an ask
    /// that a stacked built with it made a try push's is read as one of
    /// these, which changes nothing either: the database reads all the same.
    /// Built without the `moz-phab` feature, an ask that drafted a bug is
    /// read as one too.
    #[cfg_attr(not(feature = "try-push"), serde(alias = "try"))]
    #[cfg_attr(not(feature = "moz-phab"), serde(alias = "bug"))]
    Answer,
    /// Build and test, in the stack's build worktree; changes nothing in
    /// the stack.
    Test,
    /// The CI tasks the comment asks for, selected for a try push: it
    /// changes nothing, and the tasks wait for your review, to be pushed
    /// once you accept them.
    #[cfg(feature = "try-push")]
    Try,
    /// A bug drafted for a patch whose title names none, built with the
    /// `moz-phab` feature (`bug`): it changes nothing, and the draft waits
    /// for your review; closing the review opens Bugzilla's form, filled
    /// in with it.
    #[cfg(feature = "moz-phab")]
    Bug,
}

impl Way {
    /// Every way of asking, as each kind of ask is said: a change, an
    /// answer, a build and test, and, with the `try-push` feature, a try
    /// push's selection. Not a bug's draft, which is let use what an
    /// answer is, from the answers' lists (`Agent::accepted`).
    pub const ALL: &'static [Way] = &[
        Way::Change,
        Way::Answer,
        Way::Test,
        #[cfg(feature = "try-push")]
        Way::Try,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Way::Change => "change",
            Way::Answer => "answer",
            Way::Test => "test",
            #[cfg(feature = "try-push")]
            Way::Try => "try",
            #[cfg(feature = "moz-phab")]
            Way::Bug => "bug",
        }
    }

    /// How it is said to the user: `answer only`.
    pub fn said(self) -> &'static str {
        match self {
            Way::Change => "may change the code",
            Way::Answer => "answer only",
            Way::Test => "build and test",
            #[cfg(feature = "try-push")]
            Way::Try => "try push",
            #[cfg(feature = "moz-phab")]
            Way::Bug => "file a bug",
        }
    }
}

/// How hard the agent works on an ask, as claude's `--effort` takes it,
/// the least first. Unless picked, `high`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    #[default]
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh, Effort::Max];

    /// Each level's name, as claude takes it.
    pub const NAMES: [&'static str; 5] = ["low", "medium", "high", "xhigh", "max"];

    pub fn name(self) -> &'static str {
        Effort::NAMES[self as usize]
    }

    /// The level named `name`, if claude takes it.
    pub fn parse(name: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.name() == name)
    }

    /// The levels, said: `low, medium, high, xhigh or max`.
    pub fn listed() -> String {
        let (last, rest) = Effort::NAMES.split_last().unwrap();
        format!("{} or {last}", rest.join(", "))
    }
}

/// The model the agent works on an ask with, by its full id, as claude's
/// `--model` takes it, in the ask dialog's order: weakest to strongest, as
/// the efforts run low to max (the variants in `ALL`'s order, `IDS`'s too,
/// each one's place in them its own). There is no default: the dialog's
/// model is picked. A record keeps the id itself
/// (`Queued::model`), not one of these, so that one of a model since taken
/// out of the list still loads, and goes as it was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Model {
    Haiku55,
    Sonnet55,
    Opus55,
    Fable51,
}

impl Model {
    pub const ALL: [Model; 4] = [Model::Haiku55, Model::Sonnet55, Model::Opus55, Model::Fable51];

    /// The strongest model: the list's last, as it runs weakest to
    /// strongest.
    pub const STRONGEST: Model = Model::ALL[Model::ALL.len() - 1];

    /// Each model's id, as claude takes it.
    pub const IDS: [&'static str; 4] = ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"];

    pub fn id(self) -> &'static str {
        Model::IDS[self as usize]
    }

    /// The model of id `id`, if the list has it.
    pub fn parse(id: &str) -> Option<Model> {
        Model::ALL.into_iter().find(|m| m.id() == id)
    }

    /// The ids, said: `claude-haiku-5-5, claude-sonnet-5-5, claude-opus-5-5
    /// or claude-fable-5-1`.
    pub fn listed() -> String {
        let (last, rest) = Model::IDS.split_last().unwrap();
        format!("{} or {last}", rest.join(", "))
    }

    /// Of the ids `ids`, the one of the strongest model, as an ask of
    /// comments queued with more than one goes with it, as it goes at the
    /// highest of their efforts; an id the list does not have only when
    /// none is one it has, the first such then.
    pub fn strongest_of<'a>(ids: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
        let rank = |id: &str| Model::parse(id).map(|m| m as usize);
        ids.into_iter().fold(None, |best, id| match best {
            Some(b) if rank(b) >= rank(id) => Some(b),
            _ => Some(id),
        })
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
    /// round sent back from that work's review, or a follow-up asked from
    /// it (`follow`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// Asked again: the run before, whose landed work, dropped, the new
    /// agent is shown, to reuse what still holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// The effort the agent is asked at, as picked when the comment was put
    /// on the queue or in the batch: it goes at that effort whenever its
    /// turn comes. None for one queued before efforts were picked, which
    /// goes at the agent's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// The model the agent is asked with, by its id, as picked when the
    /// comment was put on the queue or in the batch. None for one queued
    /// before models were picked, which goes with the agent's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// With `on`: a follow-up that only reads (an answer, a build and test,
    /// a try push's selection), asked from the review of that work, which
    /// stays waiting for your review: what it brings back comes into that
    /// review, rather than landing in its place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub follow: bool,
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

impl Moved {
    /// Where on its patch it was, said, as `Note::where_said` says it.
    pub fn where_said(&self) -> String {
        where_said(&self.line, &self.file, &self.preview)
    }
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

/// What a try push's run is (`Run::what`): one a stacked built with the
/// `try-push` feature makes. Known by any, which says nothing of it as the
/// agent's.
pub const TRY_PUSH: &str = "try-push";

/// What a lint job's run is (`Run::what`): the linter of the stack's
/// checkout, run by stacked on the stack, patch by patch (`lint`).
pub const LINT: &str = "lint";

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
    /// For a try push (`try-push`), built with the `try-push` feature: what
    /// it pushes, and where it stands. Built without it, the run reads all
    /// the same, this passed over.
    #[cfg(feature = "try-push")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<Box<crate::try_push::Push>>,
    /// For a lint job (`lint`): the linter it runs, and where it stands,
    /// waiting its turn or running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint: Option<Box<crate::lint::Job>>,
    /// For a reconcile with HEAD (`reconcile`): the commits' messages it
    /// takes, and the work it went on from, if it is chained on some.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconcile: Option<Box<crate::reconcile::Job>>,
    /// For a pull from Phabricator (`pull`), built with the `moz-phab`
    /// feature: what it was asked, what it read of Phabricator, and the work
    /// it went on from, if it is chained on some. A pull of before, which
    /// held the comments it read at once, has none; built without the
    /// feature, the run reads all the same, this passed over.
    #[cfg(feature = "moz-phab")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<Box<crate::review::Job>>,
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
    /// (stack name, checkout) to the index of its latest record of where
    /// stacked left HEAD.
    moved: BTreeMap<(String, String), usize>,
    /// A revision's link to the index of its latest record that has its
    /// state on Phabricator; and of its latest that has its comments.
    revisions: BTreeMap<String, usize>,
    revision_comments: BTreeMap<String, usize>,
    /// (A revision's link, a diff's id) to the index of the record of that
    /// raw diff.
    raw_diffs: BTreeMap<(String, u64), usize>,
    /// A revision's link to the index of the latest fetch that read it.
    fetched: BTreeMap<String, usize>,
    /// A repository's PHID on Phabricator to the index of its latest
    /// record.
    repositories: BTreeMap<String, usize>,
    /// A revision's link to the index of the latest submit that changed it.
    submitted: BTreeMap<String, usize>,
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
    /// The file read, to read more of it.
    src: Option<Src>,
    /// Read from a checkpoint: what was not read.
    cp: Option<Box<Partial>>,
    /// Read from a checkpoint: the whole history, read the first time it
    /// is asked, and taken in before anything changes.
    shadow: std::sync::OnceLock<Box<Db>>,
    /// The last checkpoint's line: its offset and length.
    last_cp: Option<(u64, u64)>,
    /// A line cut short was ended: it is read again before a checkpoint.
    mended: bool,
    /// The lines passed over, and why.
    pub warnings: Vec<String>,
    /// The patches that should not change that the edit made of it may
    /// change all the same, by the versions the stack holds: confirmed for
    /// the one operation it makes (`confirm`). Never read from the file, and
    /// kept as the whole history is taken in.
    confirmed: BTreeSet<String>,
    /// The write of a stack that edit refused last, as it would change
    /// patches that should not change, none of them confirmed: what
    /// `edit_confirmed` gives back, as the error of its own kind it is.
    refused: Option<Box<crate::protected::Refused>>,
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

/// Whether this thread holds a database's lock: what must be taken before
/// it is not asked for meanwhile (`ask::launch_lock`).
pub(crate) fn holds_lock() -> bool {
    LOCKED.with(|l| l.get())
}

/// flock `how` on `f`, asked again when a signal cut the wait short.
fn flock(f: &File, how: libc::c_int) -> std::io::Result<()> {
    loop {
        if unsafe { libc::flock(f.as_raw_fd(), how) } == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// A reader's hold on the file it reads: the database's lock, shared, so
/// that no writer appends while it reads, and no line is read half
/// written; given back once dropped. Held through a copy of the file's
/// descriptor, which gives back the lock of that opening of the file
/// whatever becomes of the file read. None is taken by a thread that holds
/// the database's lock already, which keeps every writer out too: asked
/// for then, on another opening of the file, the shared lock would wait on
/// that thread for ever.
struct Reading(Option<File>);

impl Drop for Reading {
    fn drop(&mut self) {
        if let Some(f) = &self.0 {
            let _ = flock(f, libc::LOCK_UN);
        }
    }
}

/// The shared lock on `f`, the file at `path`, for this thread's read of
/// it (`Reading`).
fn reading(f: &File, path: &Path) -> Result<Reading> {
    if holds_lock() {
        return Ok(Reading(None));
    }
    let io = |e: std::io::Error| Error(format!("{}: cannot lock to read: {e}", path.display()));
    let copy = f.try_clone().map_err(io)?;
    flock(&copy, libc::LOCK_SH).map_err(io)?;
    Ok(Reading(Some(copy)))
}

/// Opens the file at `path` to read it, and takes its shared lock
/// (`reading`); then sees that it is still the file at `path`, else
/// (another renamed over it meanwhile, an older file written again) opens
/// that one, as `open_locked` does. None when there is no file.
fn open_read(path: &Path) -> Result<Option<(File, Reading)>> {
    let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
    for _ in 0..1000 {
        let f = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        let held = reading(&f, path)?;
        let meta = f.metadata().map_err(io)?;
        if std::fs::metadata(path).is_ok_and(|m| (m.dev(), m.ino()) == (meta.dev(), meta.ino())) {
            return Ok(Some((f, held)));
        }
    }
    err(format!("{}: renamed over again and again while it was read", path.display()))
}

/// Writes `bytes`, the lines of records, to `f`, under the database's lock.
/// Under test, a thread may have them written in two halves, saying so
/// once the first is, then pausing before the second (`HALVES`): as a
/// writer slowed down midway, whose half line no reader must read.
fn write_lines(f: &mut File, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(test)]
    if let Some((said, pause)) = HALVES.with(|h| h.borrow().clone()) {
        let (first, second) = bytes.split_at(bytes.len() / 2);
        f.write_all(first)?;
        let _ = said.send(());
        std::thread::sleep(pause);
        return f.write_all(second);
    }
    f.write_all(bytes)
}

#[cfg(test)]
thread_local! {
    /// Under test: this thread's writes of records made in two halves, the
    /// first said on the channel, then the pause before the second.
    static HALVES: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>, std::time::Duration)>> = const { std::cell::RefCell::new(None) };
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
    /// Makes an empty database at `path`, as a first write starts one: its
    /// header alone. Only where nothing is: a file there, even one made
    /// meanwhile, is an error, and is not written over.
    pub fn create(path: &Path) -> Result<()> {
        let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
        let mut f = File::options().write(true).create_new(true).open(path).map_err(io)?;
        // Written under its lock, as every write is: a reader reads the
        // header whole, or nothing.
        take_lock(&f, path)?;
        let written = f.write_all(format!("{}\n", header_line()).as_bytes()).and_then(|_| f.sync_data()).map_err(io);
        give_lock(&f);
        written
    }

    /// Reads `path`, under its lock, shared (`open_read`): no record is read
    /// half written. A missing file is an empty database.
    pub fn load(path: &Path) -> Result<Db> {
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(path) {
            return Db::read_src(path, Src::Memory(m), None);
        }
        let Some((f, held)) = open_read(path)? else { return Db::parse(path, &[]) };
        let ino = f.metadata().ok().map(|m| (m.dev(), m.ino()));
        let db = Db::read_src(path, Src::File(f), ino);
        drop(held);
        db
    }

    /// The file at `path` whole, as it is: read under its lock, shared, so
    /// that its last record is not half written. Nothing when there is no
    /// file.
    pub fn bytes(path: &Path) -> Result<Vec<u8>> {
        #[cfg(feature = "demo")]
        if let Some(m) = crate::memory::file(path) {
            return Ok(m.lock().unwrap().clone());
        }
        let Some((f, held)) = open_read(path)? else { return Ok(vec![]) };
        let bytes = Src::File(f).rest(0).map_err(|e| Error(format!("{}: {e}", path.display())));
        drop(held);
        bytes
    }

    /// The database `src` holds: from the checkpoint its header points at
    /// and what follows it, if that checks out; else read whole.
    fn read_src(path: &Path, src: Src, ino: Option<(u64, u64)>) -> Result<Db> {
        let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
        let mut db = match Db::from_checkpoint(path, &src) {
            Some(mut db) => {
                db.read(&src.rest(db.len).map_err(io)?)?;
                db
            }
            None => Db::parse(path, &src.rest(0).map_err(io)?)?,
        };
        if test_checkpoints() && db.cp.is_some() {
            let whole = Db::parse(path, &src.read(0, db.len).map_err(io)?)?;
            if let Some(e) = db.differs(&whole) {
                panic!("{}: read from its checkpoint, not as read whole: {e}", path.display());
            }
        }
        (db.src, db.ino) = (Some(src), ino);
        Ok(db)
    }

    /// The database as the checkpoint the header of `src` points at left
    /// it: the records it needs read, the others not. None if there is no
    /// checkpoint, or anything of it does not check out.
    fn from_checkpoint(path: &Path, src: &Src) -> Option<Db> {
        let head = src.read(0, HEADER as u64).ok()?;
        let line = head.strip_suffix(b"\n")?;
        let header: Header = serde_json::from_slice(line).ok()?;
        let ptr = header.checkpoint.filter(|_| header.version == VERSION)?;
        if ptr.at < HEADER as u64 || ptr.rev == 0 {
            return None;
        }
        // Its line whole, and the sum of it.
        let bytes = src.read(ptr.at - 1, ptr.len.checked_add(2)?).ok()?;
        let at = whole_line(&bytes)?;
        if sum(ptr.at, ptr.rev, ptr.len, at) != ptr.sum {
            return None;
        }
        let cp: Checkpoint = serde_json::from_slice(at).ok()?;
        if cp.t != "checkpoint" || cp.state != STATE || cp.rev != ptr.rev {
            return None;
        }
        let mut db = Db::empty(path);
        db.records = vec![Record::Unloaded; cp.rev];
        db.pos = vec![Pos::NONE; cp.rev];
        db.records[0] = serde_json::from_slice(line).ok()?;
        db.pos[0] = Pos {
            at: 0,
            len: line.len() as u64,
        };
        (db.version, db.fixed) = (VERSION, true);
        for &(rev, at, len) in &cp.load {
            if !(2..=cp.rev).contains(&rev) || at < HEADER as u64 {
                return None;
            }
            let bytes = src.read(at - 1, len.checked_add(2)?).ok()?;
            let rec: Record = serde_json::from_slice(whole_line(&bytes)?).ok()?;
            if matches!(rec, Record::Stacked { .. }) {
                return None;
            }
            db.records[rev - 1] = rec;
            db.pos[rev - 1] = Pos { at, len };
        }
        db.restore(cp)?;
        db.len = ptr.at + ptr.len + 1;
        db.last_cp = Some((ptr.at, ptr.len));
        Some(db)
    }

    /// The indexes as checkpoint `cp` says they stood, seen to hold with
    /// the records read: none if they do not.
    fn restore(&mut self, cp: Checkpoint) -> Option<()> {
        let n = cp.rev;
        let ix = |rev: usize| (1..=n).contains(&rev).then(|| rev - 1);
        let read = |db: &Db, i: usize| !matches!(db.records[i], Record::Unloaded);
        for (name, head) in cp.heads {
            let h = match head {
                Some(r) => Some(ix(r)?),
                None => None,
            };
            if h.is_some_and(|i| !matches!(&self.records[i], Record::Stack(s) if s.name == name)) {
                return None;
            }
            self.heads.insert(name, h);
        }
        for (id, rev) in cp.patches {
            let i = ix(rev)?;
            if read(self, i) && !matches!(&self.records[i], Record::Patch(p) if p.id == id) {
                return None;
            }
            self.patches.insert(id, i);
        }
        for (change, rev) in cp.latest {
            let i = ix(rev)?;
            if read(self, i) && !matches!(&self.records[i], Record::Patch(p) if p.change == change) {
                return None;
            }
            self.latest.insert(change, i);
        }
        for (rev, name, parent) in cp.stacks {
            let i = ix(rev)?;
            if read(self, i) && !matches!(&self.records[i], Record::Stack(s) if s.name == name && s.parent == parent) {
                return None;
            }
            self.stacks.insert(i, (name, parent));
        }
        self.on = cp.on.into_iter().collect();
        for (change, id, rev, first) in cp.notes {
            let (i, f) = (ix(rev)?, ix(first)?);
            if read(self, i) && !matches!(&self.records[i], Record::Comment(c) if c.change == change && c.id == id) {
                return None;
            }
            self.note_first.insert((change.clone(), id.clone()), f);
            self.notes.insert((change, id), i);
        }
        self.last_queued = cp.queued.into_iter().map(|(c, i, q)| ((c, i), q)).collect();
        self.note_lines = cp.note_lines.into_iter().map(|(c, i, l)| ((c, i), l)).collect();
        for (stack, rev) in cp.views {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::View { stack: s, .. } if *s == stack) {
                return None;
            }
            self.views.insert(stack, i);
        }
        for (stack, of, rev) in cp.choices {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Choice { stack: s, of: o, .. } if *s == stack && *o == of) {
                return None;
            }
            self.choices.insert((stack, of), i);
        }
        for (stack, checkout, rev) in cp.moved {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Moved { stack: s, checkout: c, .. } if *s == stack && *c == checkout) {
                return None;
            }
            self.moved.insert((stack, checkout), i);
        }
        // What stands of each revision read from Phabricator, read with it;
        // its raw diffs, read with it only for the latest each state names.
        for (link, rev) in cp.revisions {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Revision(r) if r.link == link && r.state.is_some()) {
                return None;
            }
            self.revisions.insert(link, i);
        }
        for (link, rev) in cp.revision_comments {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Revision(r) if r.link == link && r.comments.is_some()) {
                return None;
            }
            self.revision_comments.insert(link, i);
        }
        for (link, diff, rev) in cp.raw_diffs {
            let i = ix(rev)?;
            if read(self, i) && !matches!(&self.records[i], Record::RawDiff { link: l, diff: d, .. } if *l == link && *d == diff) {
                return None;
            }
            self.raw_diffs.insert((link, diff), i);
        }
        for (link, rev) in cp.fetched {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Fetched { links, .. } if links.contains(&link)) {
                return None;
            }
            self.fetched.insert(link, i);
        }
        // Each repository read from Phabricator, read with it.
        for (phid, rev) in cp.repositories {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Repository(r) if r.phid == phid) {
                return None;
            }
            self.repositories.insert(phid, i);
        }
        // The latest submit that changed each revision, read with it.
        for (link, rev) in cp.submitted {
            let i = ix(rev)?;
            if !matches!(&self.records[i], Record::Submitted { link: l, .. } if *l == link) {
                return None;
            }
            self.submitted.insert(link, i);
        }
        self.collapsed = cp.collapsed.into_iter().collect();
        for (run, rev) in cp.reviews {
            let i = ix(rev)?;
            if read(self, i) && !matches!(&self.records[i], Record::Review { run: r, .. } if *r == run) {
                return None;
            }
            self.reviews.insert(run, i);
        }
        for (change, revs) in cp.marks {
            let mut at = Vec::new();
            for rev in revs {
                let i = ix(rev)?;
                if !matches!(&self.records[i], Record::Read(m) if m.change == change) {
                    return None;
                }
                at.push(i);
            }
            self.marks.insert(change, at);
        }
        for (id, rev) in cp.runs {
            let i = ix(rev)?;
            if read(self, i) && !matches!(&self.records[i], Record::Run(r) if r.id == id) {
                return None;
            }
            if self.runs.insert(id.clone(), i).is_some() {
                return None;
            }
            self.run_order.push(id);
        }
        for (run, revs) in cp.events {
            let mut at = Vec::new();
            for rev in revs {
                let i = ix(rev)?;
                if !matches!(&self.records[i], Record::Event { run: r, .. } if *r == run) {
                    return None;
                }
                at.push(i);
            }
            self.events.insert(run, at);
        }
        // Its own line too.
        (self.changes, self.lines, self.saved) = (cp.changes, cp.lines + 1, n);
        self.warnings = cp.warnings;
        self.cp = Some(Box::new(Partial {
            unread: cp.unread.into_iter().collect(),
            stale: false,
        }));
        Some(())
    }

    /// How this database, read from a checkpoint, differs from `whole`, the
    /// same bytes read whole: under test, there is no difference.
    fn differs(&self, whole: &Db) -> Option<String> {
        macro_rules! same {
            ($($f:ident),*) => {$(
                if self.$f != whole.$f {
                    return Some(format!("{}: {:?}, read whole {:?}", stringify!($f), self.$f, whole.$f));
                }
            )*};
        }
        same!(len, lines, torn, torn_slot, tail, saved, version, fixed, changes, warnings, last_cp);
        same!(patches, latest, heads, stacks, on, notes, note_first, last_queued, note_lines);
        same!(views, choices, moved, collapsed, reviews, marks, runs, run_order);
        same!(revisions, revision_comments, raw_diffs, fetched, repositories, submitted);
        if self.records.len() != whole.records.len() {
            return Some(format!("{} records, read whole {}", self.records.len(), whole.records.len()));
        }
        // The raw diffs the stored states name: read with it.
        let named: HashSet<usize> = self.phab_needed();
        for (i, r) in self.records.iter().enumerate() {
            match r {
                // Not read: a comment's latest only when it is closed; never
                // what stands of a revision read from Phabricator.
                Record::Unloaded => {
                    if matches!(&whole.records[i], Record::Comment(n) if !n.text.is_empty()) && self.notes.values().any(|&k| k == i) {
                        return Some(format!("@{}: an open comment not read", i + 1));
                    }
                    if named.contains(&i) {
                        return Some(format!("@{}: what stands of a revision read from Phabricator not read", i + 1));
                    }
                }
                r if *r != whole.records[i] => return Some(format!("@{}: {r:?}, read whole {:?}", i + 1, whole.records[i])),
                _ if self.pos[i] != whole.pos[i] => return Some(format!("@{}: at {:?}, read whole {:?}", i + 1, self.pos[i], whole.pos[i])),
                _ => {}
            }
        }
        let ids = |db: &Db| db.live_runs().iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        if ids(self) != ids(whole) {
            return Some(format!("under way {:?}, read whole {:?}", ids(self), ids(whole)));
        }
        let unread = self.cp.as_ref().map(|c| &c.unread);
        for (run, at) in &whole.events {
            if !unread.is_some_and(|u| u.contains(run)) && self.events.get(run) != Some(at) {
                return Some(format!("the events of {run}: {:?}, read whole {at:?}", self.events.get(run)));
            }
        }
        None
    }

    /// The database `bytes` hold, as if read from `path`.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Db> {
        Db::parse(path, bytes)
    }

    fn parse(path: &Path, bytes: &[u8]) -> Result<Db> {
        let mut db = Db::empty(path);
        db.read(bytes)?;
        Ok(db)
    }

    fn empty(path: &Path) -> Db {
        Db {
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
            moved: BTreeMap::new(),
            revisions: BTreeMap::new(),
            revision_comments: BTreeMap::new(),
            raw_diffs: BTreeMap::new(),
            fetched: BTreeMap::new(),
            repositories: BTreeMap::new(),
            submitted: BTreeMap::new(),
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
            src: None,
            cp: None,
            shadow: std::sync::OnceLock::new(),
            last_cp: None,
            mended: false,
            warnings: Vec::new(),
            confirmed: BTreeSet::new(),
            refused: None,
        }
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
            // A checkpoint's line: no record. Known by how it starts, as no
            // record's does, whatever the header says: one half written
            // says nothing.
            let checkpoint = line.starts_with(CHECKPOINT);
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
                self.last_cp = Some((pos.at, pos.len));
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
        self.promote();
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
        // Read under its lock, shared: what was appended is read whole.
        let Some((mut f, held)) = open_read(&self.path)? else {
            // Gone: an empty database.
            if self.len == 0 {
                return Ok(false);
            }
            *self = Db::load(&self.path)?;
            return Ok(true);
        };
        let meta = f.metadata().map_err(io)?;
        let ino = (meta.dev(), meta.ino());
        // Another file renamed over it (an older one written again), or
        // the file cut, which is not ours to do: read it whole, the lock
        // given back first, as reading it whole takes it again.
        if self.ino.is_some_and(|i| i != ino) || meta.len() < self.len {
            drop(held);
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
        drop(held);
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
        write_lines(f, out.as_bytes()).map_err(io)?;
        f.sync_data().map_err(io)
    }

    fn index(&mut self, rec: Record, pos: Pos) -> Result<()> {
        self.promote();
        let at = self.records.len();
        if !matches!(rec, Record::Event { .. }) {
            self.changes += 1;
        }
        match &rec {
            Record::Stacked { .. } | Record::Broken | Record::Unloaded => {}
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
                    // By the stacks' index: the stack may be one not read.
                    if rev.checked_sub(1).and_then(|i| self.stacks.get(&i)).is_none_or(|(name, _)| *name != h.name) {
                        return err(format!("@{rev} is not a stack of {}", h.name));
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
            Record::Moved { stack, checkout, .. } => {
                self.moved.insert((stack.clone(), checkout.clone()), at);
            }
            Record::Revision(r) => {
                if r.state.is_some() {
                    self.revisions.insert(r.link.clone(), at);
                }
                if r.comments.is_some() {
                    self.revision_comments.insert(r.link.clone(), at);
                }
            }
            // Once for each id: the first, should two have come.
            Record::RawDiff { link, diff, .. } => {
                self.raw_diffs.entry((link.clone(), *diff)).or_insert(at);
            }
            Record::Fetched { links, .. } => {
                for link in links {
                    self.fetched.insert(link.clone(), at);
                }
            }
            Record::Repository(r) => {
                self.repositories.insert(r.phid.clone(), at);
            }
            Record::Submitted { link, .. } => {
                self.submitted.insert(link.clone(), at);
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
                // Of a run not read: whether it is under way now is known
                // only from the whole history.
                if let Some(cp) = &mut self.cp
                    && self.runs.get(run).is_some_and(|&i| matches!(self.records[i], Record::Unloaded))
                {
                    cp.stale = true;
                }
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
            let lock = Lock(Held::Memory {
                _held: guard,
                bytes: m.clone(),
            });
            return Ok((Db::read_src(path, Src::Memory(m), None)?, lock));
        }
        // Given back if reading fails: the lock first.
        let mut lock = open_locked(path)?;
        let ino = lock.file().metadata().ok().map(|m| (m.dev(), m.ino()));
        // Read by another opening of the file: the lock's appends.
        let src = File::open(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let mut db = Db::read_src(path, Src::File(src), ino)?;
        // The header of the fixed length, but not read: a crash as it was
        // written over. Written again, pointing at no checkpoint.
        if matches!(db.records.first(), Some(Record::Broken)) && db.pos[0] == (Pos { at: 0, len: HEADER as u64 - 1 }) {
            let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
            let f = File::options().write(true).open(path).map_err(io)?;
            f.write_all_at(header_line().as_bytes(), 0).and_then(|_| f.sync_data()).map_err(io)?;
            db = Db::read_src(path, Src::File(File::open(path).map_err(io)?), ino)?;
        }
        if db.needs_upgrade() {
            match db.upgrade(&mut lock) {
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
    fn upgrade(&self, lock: &mut Lock) -> Result<Option<Db>> {
        if self.live_runs().iter().any(|r| crate::ask::busy(r)) {
            return Ok(None);
        }
        let path = &self.path;
        let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
        let bytes = self.src.as_ref().map_or(Ok(vec![]), |s| s.rest(0)).map_err(io)?;
        let Some(nl) = bytes.iter().position(|&b| b == b'\n') else { return Ok(None) };
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
        match std::mem::replace(&mut lock.0, Held::File(f)) {
            Held::File(old) => {
                unsafe { libc::flock(old.as_raw_fd(), libc::LOCK_UN) };
            }
            #[cfg(feature = "demo")]
            Held::Memory { .. } => {}
        }
        let mut db = Db::parse(path, &new)?;
        db.ino = lock.file().metadata().ok().map(|m| (m.dev(), m.ino()));
        db.src = Some(Src::File(File::open(path).map_err(io)?));
        Ok(Some(db))
    }

    /// Runs `edit` on the database as it is on disk, under its lock, and
    /// appends the records it added. A write of a stack it refused for
    /// patches that should not change is what went wrong, said
    /// (`protected::Refused::said`).
    pub fn edit<T>(path: &Path, edit: impl FnOnce(&mut Db) -> Result<T>) -> Result<T> {
        let (mut db, mut lock) = Db::lock(path)?;
        let out = edit(&mut db)?;
        db.save(&mut lock)?;
        Ok(out)
    }

    /// As `edit`, the patches `confirmed`, which should not change,
    /// confirmed for the one operation it makes (`confirm`): a write of a
    /// stack refused as it would change others comes back as the error of
    /// its own kind (`Failed::Protected`), nothing written, for the
    /// operation to be asked about, then made again with them confirmed.
    pub fn edit_confirmed<T>(path: &Path, confirmed: &[String], edit: impl FnOnce(&mut Db) -> Result<T>) -> std::result::Result<T, Failed> {
        let (mut db, mut lock) = Db::lock(path).map_err(Failed::Error)?;
        db.confirm(confirmed.iter().cloned());
        match edit(&mut db) {
            Ok(out) => {
                db.save(&mut lock).map_err(Failed::Error)?;
                Ok(out)
            }
            Err(e) => Err(match db.refused.take() {
                Some(r) => Failed::Protected(r),
                None => Failed::Error(e),
            }),
        }
    }

    /// Appends the records added since the database was read.
    pub fn save(&mut self, lock: &mut Lock) -> Result<()> {
        self.promote();
        if self.saved == self.records.len() {
            return Ok(());
        }
        let from = self.saved;
        let mut out = String::new();
        if self.torn {
            out.push('\n');
            // Ended, a line cut short reads as whatever it holds: a
            // checkpoint's is one, a record's is read again before a
            // checkpoint is made.
            if self.torn_slot {
                self.mended = true;
            } else {
                self.last_cp = Some((self.tail, self.len - self.tail));
            }
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
                write_lines(f, out.as_bytes()).map_err(io)?;
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
        if self.wants_checkpoint(from) {
            self.write_checkpoint(lock)?;
        }
        Ok(())
    }

    /// Whether to write a checkpoint after the records from `from` on:
    /// once the file has grown four times the last one's length past it,
    /// and at least `FLOOR`; or when a head moved, which may be to a stack
    /// the last one did not read.
    fn wants_checkpoint(&self, from: usize) -> bool {
        if !self.fixed {
            return false;
        }
        let (end, last) = self.last_cp.map_or((HEADER as u64, 0), |(at, len)| (at + len + 1, len + 1));
        test_checkpoints()
            || self.records[from..].iter().any(|r| matches!(r, Record::Head(_)))
            || self.len.saturating_sub(end) >= FLOOR.max(4 * last)
    }

    /// Appends a checkpoint, then points the header at it. A crash in
    /// between leaves the header pointing at the one before; a reader
    /// catching the header half written finds its sum wrong, and reads the
    /// file whole.
    fn write_checkpoint(&mut self, lock: &mut Lock) -> Result<()> {
        if self.mended || self.cp.as_ref().is_some_and(|c| c.stale) {
            self.reread()?;
        }
        let need = match self.needed() {
            Some(need) => need,
            // Something it needs was not read: the file read whole first.
            None => {
                self.reread()?;
                match self.needed() {
                    Some(need) => need,
                    None => return Ok(()),
                }
            }
        };
        let line = serde_json::to_string(&self.checkpoint(&need)).map_err(|e| Error(e.to_string()))?;
        let at = self.len;
        let Some(header) = header_with(Some(&Pointer::new(at, self.records.len(), line.as_bytes()))) else {
            return Ok(());
        };
        let io = |e: std::io::Error| Error(format!("{}: {e}", self.path.display()));
        match &mut lock.0 {
            Held::File(f) => {
                f.write_all(format!("{line}\n").as_bytes()).map_err(io)?;
                // Written over in place, by another opening of the file:
                // the lock's appends.
                let h = File::options().write(true).open(&self.path).map_err(io)?;
                if h.metadata().ok().map(|m| (m.dev(), m.ino())) == self.ino {
                    h.write_all_at(header.as_bytes(), 0).map_err(io)?;
                }
            }
            #[cfg(feature = "demo")]
            Held::Memory { bytes, .. } => {
                let mut b = bytes.lock().unwrap();
                b.extend_from_slice(format!("{line}\n").as_bytes());
                b[..HEADER - 1].copy_from_slice(header.as_bytes());
            }
        }
        self.len += line.len() as u64 + 1;
        self.lines += 1;
        self.last_cp = Some((at, line.len() as u64));
        Ok(())
    }

    /// The file read again whole, as far as this one read it: what a
    /// reader reads now.
    fn reread(&mut self) -> Result<()> {
        let Some(src) = self.src.take() else { return Ok(()) };
        // Under its lock, shared, as every read; none taken under the
        // database's own, which a checkpoint's writer holds.
        let read = src.reading(&self.path).and_then(|held| {
            let bytes = src.read(0, self.len).map_err(|e| Error(format!("{}: {e}", self.path.display())));
            drop(held);
            bytes
        });
        let bytes = match read {
            Ok(b) => b,
            Err(e) => {
                self.src = Some(src);
                return Err(e);
            }
        };
        let (path, ino) = (self.path.clone(), self.ino);
        *self = Db::parse(&path, &bytes)?;
        (self.src, self.ino) = (Some(src), ino);
        Ok(())
    }

    /// The records a checkpoint reads with it, by index: what the state
    /// stands on. None when one of them was not read here.
    fn needed(&self) -> Option<BTreeSet<usize>> {
        let mut need = BTreeSet::new();
        let (mut stacks, mut patches, mut runs) = (Vec::new(), Vec::new(), BTreeSet::new());
        // Each head, and where one undo and one redo go; a name deleted,
        // the stack undoing that brings back.
        for (name, head) in &self.heads {
            match head {
                Some(h) => {
                    stacks.push(*h);
                    stacks.extend(self.stacks.get(h).and_then(|(_, p)| *p).and_then(|p| p.checked_sub(1)));
                    stacks.extend(self.redo_child(name).and_then(|c| c.checked_sub(1)));
                }
                None => stacks.extend(self.last_head(name).and_then(|r| r.checked_sub(1))),
            }
        }
        // The runs under way, the runs they name, and what the agent of
        // those going wrote.
        let unread = |run: &str| self.cp.as_ref().is_some_and(|c| c.unread.contains(run));
        for r in self.live_runs() {
            runs.insert(r.id.clone());
            runs.extend(named_by(r).2);
            if r.state == RunState::Started && !unread(&r.id) {
                need.extend(self.events.get(&r.id).into_iter().flatten().copied());
            }
        }
        // The comments open, the runs their sending names, and the latest
        // version of their patch.
        for &at in self.notes.values() {
            let Record::Comment(n) = &self.records[at] else { continue };
            if n.text.is_empty() {
                continue;
            }
            need.insert(at);
            if let Some(q) = &n.queued {
                runs.extend(q.on.iter().chain(&q.was).cloned());
            }
            need.extend(self.latest.get(&n.change).copied());
        }
        // Each of those runs, its review, and the stacks and patches it
        // names: an ask starts from the stack the work it goes on from makes.
        for id in &runs {
            let Some(&at) = self.runs.get(id) else { continue };
            let Record::Run(r) = &self.records[at] else { return None };
            need.insert(at);
            need.extend(self.reviews.get(id).copied());
            let (revs, ps, _) = named_by(r);
            stacks.extend(revs.into_iter().filter_map(|r| r.checked_sub(1)));
            patches.extend(ps);
        }
        need.extend(self.views.values().copied());
        need.extend(self.choices.values().copied());
        need.extend(self.moved.values().copied());
        need.extend(self.marks.values().flatten().copied());
        need.extend(self.phab_needed());
        for s in stacks {
            let Some(Record::Stack(st)) = self.records.get(s) else { return None };
            need.insert(s);
            patches.extend(st.patches.iter().cloned());
        }
        need.extend(patches.iter().filter_map(|id| self.patches.get(id).copied()));
        need.iter()
            .all(|&i| !matches!(self.records[i], Record::Unloaded | Record::Broken) && self.pos[i] != Pos::NONE)
            .then_some(need)
    }

    /// The records that what stands of the revisions read from Phabricator
    /// is in, by index: each one's latest with its state, its latest with
    /// its comments, the latest fetch that read it, the latest submit that
    /// changed it, and the raw diff its state names, when that is stored;
    /// and the latest record of each repository read from it.
    fn phab_needed(&self) -> HashSet<usize> {
        let mut need: HashSet<usize> = self
            .revisions
            .values()
            .chain(self.revision_comments.values())
            .chain(self.fetched.values())
            .chain(self.repositories.values())
            .chain(self.submitted.values())
            .copied()
            .collect();
        for (link, &at) in &self.revisions {
            if let Record::Revision(Revision { state: Some(s), .. }) = &self.records[at]
                && let Some(&k) = self.raw_diffs.get(&(link.clone(), s.diff))
            {
                need.insert(k);
            }
        }
        need
    }

    /// The checkpoint of the state as it is, reading `need` with it.
    fn checkpoint(&self, need: &BTreeSet<usize>) -> Checkpoint {
        fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
            v.sort();
            v
        }
        let revs = |at: &[usize]| at.iter().map(|i| i + 1).collect::<Vec<_>>();
        let mut queued: Vec<(String, String, Queued)> = self.last_queued.iter().map(|((c, i), q)| (c.clone(), i.clone(), q.clone())).collect();
        queued.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        // The events read with it: those of the runs going.
        let mut unread: BTreeSet<String> = self.cp.as_ref().map(|c| c.unread.iter().cloned().collect()).unwrap_or_default();
        let going: HashSet<&str> = self.live_runs().iter().filter(|r| r.state == RunState::Started).map(|r| r.id.as_str()).collect();
        let mut events = Vec::new();
        for (run, at) in &self.events {
            if going.contains(run.as_str()) && !unread.contains(run) {
                events.push((run.clone(), revs(at)));
            } else {
                unread.insert(run.clone());
            }
        }
        Checkpoint {
            t: "checkpoint".into(),
            state: STATE,
            rev: self.records.len(),
            lines: self.lines,
            changes: self.changes,
            load: need.iter().map(|&i| (i + 1, self.pos[i].at, self.pos[i].len)).collect(),
            heads: self.heads.iter().map(|(n, h)| (n.clone(), h.map(|i| i + 1))).collect(),
            patches: sorted(self.patches.iter().map(|(id, i)| (id.clone(), i + 1)).collect()),
            latest: sorted(self.latest.iter().map(|(c, i)| (c.clone(), i + 1)).collect()),
            stacks: self.stacks.iter().map(|(i, (n, p))| (i + 1, n.clone(), *p)).collect(),
            on: sorted(self.on.iter().map(|(n, r)| (n.clone(), r.clone())).collect()),
            notes: self.notes.iter().map(|((c, id), i)| (c.clone(), id.clone(), i + 1, self.note_first[&(c.clone(), id.clone())] + 1)).collect(),
            queued,
            note_lines: sorted(self.note_lines.iter().map(|((c, i), l)| (c.clone(), i.clone(), l.clone())).collect()),
            views: sorted(self.views.iter().map(|(s, i)| (s.clone(), i + 1)).collect()),
            choices: self.choices.iter().map(|((s, o), i)| (s.clone(), o.clone(), i + 1)).collect(),
            moved: self.moved.iter().map(|((s, c), i)| (s.clone(), c.clone(), i + 1)).collect(),
            revisions: self.revisions.iter().map(|(l, i)| (l.clone(), i + 1)).collect(),
            revision_comments: self.revision_comments.iter().map(|(l, i)| (l.clone(), i + 1)).collect(),
            raw_diffs: self.raw_diffs.iter().map(|((l, d), i)| (l.clone(), *d, i + 1)).collect(),
            fetched: self.fetched.iter().map(|(l, i)| (l.clone(), i + 1)).collect(),
            repositories: self.repositories.iter().map(|(p, i)| (p.clone(), i + 1)).collect(),
            submitted: self.submitted.iter().map(|(l, i)| (l.clone(), i + 1)).collect(),
            collapsed: sorted(self.collapsed.iter().cloned().collect()),
            reviews: sorted(self.reviews.iter().map(|(r, i)| (r.clone(), i + 1)).collect()),
            marks: sorted(self.marks.iter().map(|(c, at)| (c.clone(), revs(at))).collect()),
            runs: self.run_order.iter().map(|id| (id.clone(), self.runs[id] + 1)).collect(),
            events: sorted(events),
            unread: unread.into_iter().collect(),
            warnings: self.warnings.clone(),
        }
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

    /// The rev of the last record, the header's @1 in: where the records
    /// end, a position in the file, and how many there are. It is no sign
    /// that anything changed: a line read cut short holds a rev of its own,
    /// which the same line read whole keeps. What says that something the
    /// editor shows may have changed is `changes`.
    pub fn end_rev(&self) -> usize {
        self.records.len()
    }

    /// Every record, `records()[i]` rev `i + 1`: the whole history.
    pub fn records(&self) -> &[Record] {
        &self.full().records
    }

    /// The record at `rev`.
    pub fn rev(&self, rev: usize) -> Option<&Record> {
        rev.checked_sub(1).filter(|&i| i < self.records.len()).map(|i| self.at(i))
    }

    /// The record at index `i`: read from the whole history if it was not.
    fn at(&self, i: usize) -> &Record {
        match &self.records[i] {
            Record::Unloaded => &self.full().records[i],
            r => r,
        }
    }

    /// The database with its whole history: this one, unless it was read
    /// from a checkpoint; then the file read whole as this one read it, the
    /// first time it is asked.
    fn full(&self) -> &Db {
        if self.cp.is_none() {
            return self;
        }
        self.shadow.get_or_init(|| {
            let whole = self.read_whole();
            Box::new(whole.unwrap_or_else(|e| panic!("{}: reading the whole history: {}", self.path.display(), e.0)))
        })
    }

    /// The file read whole, up to where this one read it, and what this
    /// one has added since that is not written yet.
    fn read_whole(&self) -> Result<Db> {
        let src = self.src.as_ref().ok_or_else(|| Error("the file it was read from is not kept".into()))?;
        // Under its lock, shared, as every read: its header is written over
        // as a checkpoint is pointed at.
        let held = src.reading(&self.path)?;
        let bytes = src.read(0, self.len).map_err(|e| Error(e.to_string()))?;
        drop(held);
        let mut db = Db::parse(&self.path, &bytes)?;
        for rec in &self.records[self.saved..] {
            db.index(rec.clone(), Pos::NONE)?;
        }
        Ok(db)
    }

    /// Whether it was read from a checkpoint, and whether something read its
    /// whole history since: for the tests of what reads none.
    #[cfg(test)]
    pub(crate) fn read_from_checkpoint(&self) -> (bool, bool) {
        (self.cp.is_some(), self.shadow.get().is_some())
    }

    /// The whole history taken in, if it was read: before anything changes.
    /// What the edit made of it was confirmed for, and the write it refused,
    /// are kept.
    fn promote(&mut self) {
        if let Some(whole) = self.shadow.take() {
            let (src, ino) = (self.src.take(), self.ino);
            let (confirmed, refused) = (std::mem::take(&mut self.confirmed), self.refused.take());
            *self = *whole;
            (self.src, self.ino, self.confirmed, self.refused) = (src, ino, confirmed, refused);
        }
    }

    /// The stack at `rev`.
    pub fn node(&self, rev: usize) -> Result<&Stack> {
        match self.rev(rev) {
            Some(Record::Stack(s)) => Ok(s),
            _ => err(format!("@{rev} is not a stack")),
        }
    }

    pub fn patch(&self, id: &str) -> &Patch {
        match self.at(self.patches[id]) {
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
        match self.at(*self.latest.get(change)?) {
            Record::Patch(p) => Some(p),
            _ => None,
        }
    }

    /// Every version of `change`, oldest first.
    pub fn versions(&self, change: &str) -> Vec<&Patch> {
        if self.cp.is_some() {
            return self.full().versions(change);
        }
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
        if self.cp.is_some() {
            return self.full().first_use(id);
        }
        self.records.iter().enumerate().find_map(|(i, r)| match r {
            Record::Stack(s) if s.patches.iter().any(|p| p == id) => Some((i + 1, s)),
            _ => None,
        })
    }

    /// The stack `name` is on, and its rev.
    pub fn head(&self, name: &str) -> Option<(usize, &Stack)> {
        let at = (*self.heads.get(name)?)?;
        match self.at(at) {
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
        if self.cp.is_some() {
            return self.full().log(name);
        }
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
        if self.cp.is_some() {
            return self.full().tree(name);
        }
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
    /// the stack it is on either way. Refused, as every write of a stack,
    /// when it changes a patch that should not change that this edit was
    /// not confirmed for (`guard`).
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

    /// Appends `s`, once it is seen to apply, and to change no patch that
    /// should not change, but those confirmed (`guard`).
    fn push_stack(&mut self, s: Stack) -> Result<usize> {
        self.verify(&s)?;
        self.guard(&s.name, &s.patches)?;
        self.push(Record::Stack(s))
    }

    /// Confirms `ids`, patches that should not change, by their versions,
    /// for the one operation this edit makes: a write of a stack that
    /// changes them is not refused for them (`guard`). As you typed
    /// `accept` for them; and as a pull takes what Phabricator has.
    pub fn confirm(&mut self, ids: impl IntoIterator<Item = String>) {
        self.confirmed.extend(ids);
    }

    /// That the stack `name` is on may become one holding `patches`, as
    /// every write of a stack checks once it is seen to apply: refused when
    /// that changes patches that should not change (`protected::changes`),
    /// but those confirmed for this edit (`confirm`). The refusal is kept,
    /// the error of its own kind that `edit_confirmed` gives back, and its
    /// message names the patches. Nothing is looked at but the patches the
    /// write replaces.
    fn guard(&mut self, name: &str, patches: &[String]) -> Result<()> {
        let Some((_, s)) = self.head(name) else { return Ok(()) };
        let changed: Vec<crate::protected::Changed> =
            crate::protected::changes(self, &s.patches, patches).into_iter().filter(|c| !self.confirmed.contains(&c.id)).collect();
        if changed.is_empty() {
            return Ok(());
        }
        let refused = crate::protected::Refused { stack: name.into(), patches: changed };
        let said = refused.said();
        self.refused = Some(Box::new(refused));
        err(said)
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
    /// delete it, to none. Refused, as every write of a stack, when it
    /// changes a patch that should not change that this edit was not
    /// confirmed for (`push_head`): an undo, a redo and a stack picked from
    /// the tree as any.
    fn move_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        match rev {
            Some(r) => self.verify(self.node(r)?)?,
            None => self.writable(name)?,
        }
        self.push_head(name, rev, op)
    }

    /// Moves `name` to the stack at `rev`, or to none, once it is seen to
    /// change no patch that should not change, but those confirmed
    /// (`guard`): to none, deleted, it drops them all, which changes none.
    fn push_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        let patches = match rev {
            Some(r) => self.node(r)?.patches.clone(),
            None => vec![],
        };
        self.guard(name, &patches)?;
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
        let child = self.redo_child(name)?;
        Some((child, self.node(child).ok()?.op.clone()))
    }

    /// Where redo would take `name`, by the indexes alone.
    fn redo_child(&self, name: &str) -> Option<usize> {
        let rev = (*self.heads.get(name)?)? + 1;
        self.on.get(name)?.iter().rev().find_map(|&later| self.child_towards(rev, later))
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

    /// Where the stack at `rev`, a state of stack `name`, stands from the
    /// state `name` is on now, along the tree of edits: how many edits were
    /// made since, when the state it is on was edited from it, each a step
    /// from a stack to its parent; else another version. By the index of
    /// the stacks alone, no record read: a parent is older than its child,
    /// so the steps stop once they are past `rev`. None when `rev` is no
    /// state of `name`, or `name` is on none.
    pub fn since(&self, name: &str, rev: usize) -> Option<Since> {
        self.stacks.get(&rev.checked_sub(1)?).filter(|(n, _)| n == name)?;
        let mut at = (*self.heads.get(name)?)? + 1;
        let mut edits = 0;
        while at > rev {
            match self.stacks.get(&(at - 1)).and_then(|(_, parent)| *parent) {
                Some(parent) => at = parent,
                None => return Some(Since::Elsewhere),
            }
            edits += 1;
        }
        Some(if at == rev { Since::Edits(edits) } else { Since::Elsewhere })
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
            // A comment's latest record not read is one that closed it.
            .filter_map(|(_, &at)| match &self.records[at] {
                Record::Comment(n) if !n.text.is_empty() => Some(n),
                _ => None,
            })
            .collect()
    }

    /// Every comment open, whatever holds it, by change then id: on a stack
    /// or one of its patches, and on a patch no stack holds, as a comment
    /// written in the review of landed work on a patch that work adds is
    /// until the work is accepted.
    pub fn open_notes(&self) -> Vec<&Note> {
        self.notes
            .values()
            // A comment's latest record not read is one that closed it.
            .filter_map(|&at| match &self.records[at] {
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
        if self.cp.is_some() {
            return self.full().note_history(change, id);
        }
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
        match self.at(*self.views.get(name)?) {
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
        match self.at(*self.choices.get(&(name.to_string(), of.to_string()))?) {
            Record::Choice { pick, .. } => Some(pick),
            _ => None,
        }
    }

    /// Every choice stack `name` picked: what it is of, and the pick.
    pub fn choices(&self, name: &str) -> Vec<(&str, &str)> {
        self.choices
            .range((name.to_string(), String::new())..)
            .take_while(|((s, _), _)| s == name)
            .filter_map(|(_, &at)| match self.at(at) {
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

    /// The commit stacked last left HEAD at in the checkout whose top is
    /// `dir`, on stack `name`, if it ever moved HEAD there.
    pub fn moved(&self, name: &str, dir: &Path) -> Option<&str> {
        match self.at(*self.moved.get(&(name.to_string(), dir.to_string_lossy().into_owned()))?) {
            Record::Moved { commit, .. } => Some(commit),
            _ => None,
        }
    }

    /// Every checkout of stack `name` stacked left HEAD in, and the commit
    /// it left it at.
    pub fn moved_all(&self, name: &str) -> Vec<(&str, &str)> {
        self.moved
            .range((name.to_string(), String::new())..)
            .take_while(|((s, _), _)| s == name)
            .filter_map(|(_, &at)| match self.at(at) {
                Record::Moved { checkout, commit, .. } => Some((checkout.as_str(), commit.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Remembers that stacked left HEAD of the checkout whose top is `dir`
    /// at `commit`, on stack `name`, in place of where it left it before.
    pub fn set_moved(&mut self, name: &str, dir: &Path, commit: &str) -> Result<()> {
        if self.moved(name, dir) != Some(commit) {
            self.push(Record::Moved {
                stack: name.into(),
                checkout: dir.to_string_lossy().into_owned(),
                commit: commit.into(),
            })?;
        }
        Ok(())
    }

    /// What stands of the state on Phabricator of the revision at `link`, as
    /// the last read of it stored it, and when that read was: none before
    /// any read stored it.
    pub fn revision_state(&self, link: &str) -> Option<(&State, i64)> {
        match self.at(*self.revisions.get(link)?) {
            Record::Revision(r) => r.state.as_ref().map(|s| (s, r.time)),
            _ => None,
        }
    }

    /// What stands of the comments on Phabricator of the revision at
    /// `link`, as the last read of them stored them, and when that read
    /// was: none before any read stored them.
    pub fn revision_comments(&self, link: &str) -> Option<(&Comments, i64)> {
        match self.at(*self.revision_comments.get(link)?) {
            Record::Revision(r) => r.comments.as_ref().map(|c| (c, r.time)),
            _ => None,
        }
    }

    /// The raw diff of id `diff` of the revision at `link`, if it is
    /// stored.
    pub fn raw_diff(&self, link: &str, diff: u64) -> Option<&str> {
        match self.at(*self.raw_diffs.get(&(link.to_string(), diff))?) {
            Record::RawDiff { raw, .. } => Some(raw),
            _ => None,
        }
    }

    /// When Fetch the latest from Phabricator last read the revision at
    /// `link`, if it ever did.
    pub fn fetched(&self, link: &str) -> Option<i64> {
        match self.at(*self.fetched.get(link)?) {
            Record::Fetched { time, .. } => Some(*time),
            _ => None,
        }
    }

    /// When the latest submit that changed the revision at `link` on
    /// Phabricator was, in seconds since 1970, while no read of the
    /// revision is stored after it: what is stored of the revision, if
    /// anything, was read before that submit. None when no submit changed
    /// it, or once a read stored it after the latest one (Fetch the latest
    /// from Phabricator, the fetch a submit makes first, a pull), its
    /// state, its comments or that it read it. The file only appended to,
    /// the order of their records says which came last, to the record,
    /// however close their times.
    pub fn submitted_unread(&self, link: &str) -> Option<i64> {
        let at = *self.submitted.get(link)?;
        let read = [&self.revisions, &self.revision_comments, &self.fetched].into_iter().filter_map(|m| m.get(link)).max();
        if read.is_some_and(|&r| r > at) {
            return None;
        }
        match self.at(at) {
            Record::Submitted { time, .. } => Some(*time),
            _ => None,
        }
    }

    /// Every revision a submit changed since a read last stored it, each
    /// its link and when (`submitted_unread`), by link.
    pub fn submitted_all_unread(&self) -> Vec<(&str, i64)> {
        self.submitted.keys().filter_map(|link| Some((link.as_str(), self.submitted_unread(link)?))).collect()
    }

    /// The `D123` of the revision of PHID `phid`, as what is stored of the
    /// revisions read from Phabricator names it: one's own, or as the
    /// parent of another. A revision's PHID and number never change, so
    /// that a name once read holds.
    pub fn revision_name(&self, phid: &str) -> Option<&str> {
        let states = self.revisions.values().filter_map(|&i| match self.at(i) {
            Record::Revision(r) => r.state.as_ref().map(|s| (r, s)),
            _ => None,
        });
        let mut named = None;
        for (r, s) in states {
            if s.phid == phid {
                return Some(&r.d);
            }
            named = named.or_else(|| s.parents.iter().find(|(p, d)| p == phid && d != phid).map(|(_, d)| d.as_str()));
        }
        named
    }

    /// The links of the revisions something of which is stored: a state,
    /// comments, or a fetch that read them.
    pub fn stored_revisions(&self) -> Vec<&str> {
        let mut all: BTreeSet<&str> = self.revisions.keys().map(String::as_str).collect();
        all.extend(self.revision_comments.keys().map(String::as_str));
        all.extend(self.fetched.keys().map(String::as_str));
        all.into_iter().collect()
    }

    /// The records what stands of the revision at `link` is in: its latest
    /// with its state, and its latest with its comments, when that is
    /// another, the older first, each with the part it stands for alone.
    pub fn revision_records(&self, link: &str) -> Vec<Revision> {
        let at = |m: &BTreeMap<String, usize>| m.get(link).copied();
        let rec = |i: usize| match self.at(i) {
            Record::Revision(r) => Some(r.clone()),
            _ => None,
        };
        match (at(&self.revisions), at(&self.revision_comments)) {
            (Some(s), Some(c)) if s == c => rec(s).into_iter().collect(),
            (s, c) => {
                let mut out: Vec<(usize, Revision)> = Vec::new();
                out.extend(s.and_then(|i| rec(i).map(|r| (i, Revision { comments: None, ..r }))));
                out.extend(c.and_then(|i| rec(i).map(|r| (i, Revision { state: None, ..r }))));
                out.sort_by_key(|(i, _)| *i);
                out.into_iter().map(|(_, r)| r).collect()
            }
        }
    }

    /// Stores what a read of Phabricator learnt of revision `d` at `link`:
    /// its state and its comments, each only where it is not what is
    /// stored already, none of either written when both are. Whether it
    /// wrote a record.
    pub fn store_revision(&mut self, link: &str, d: &str, state: Option<State>, comments: Option<Comments>) -> Result<bool> {
        let state = state.filter(|s| self.revision_state(link).is_none_or(|(was, _)| was != s));
        let comments = comments.filter(|c| self.revision_comments(link).is_none_or(|(was, _)| was != c));
        if state.is_none() && comments.is_none() {
            return Ok(false);
        }
        self.push(Record::Revision(Revision {
            link: link.into(),
            d: d.into(),
            state,
            comments,
            time: now(),
        }))?;
        Ok(true)
    }

    /// Stores raw diff `raw`, of id `diff`, of the revision at `link`, unless
    /// it is stored already. Whether it wrote it.
    pub fn store_raw_diff(&mut self, link: &str, diff: u64, raw: &str) -> Result<bool> {
        if self.raw_diffs.contains_key(&(link.to_string(), diff)) {
            return Ok(false);
        }
        self.push(Record::RawDiff {
            link: link.into(),
            diff,
            raw: raw.into(),
        })?;
        Ok(true)
    }

    /// Remembers that Fetch the latest from Phabricator read the revisions
    /// at `links` now.
    pub fn store_fetched(&mut self, links: Vec<String>) -> Result<()> {
        self.push(Record::Fetched { links, time: now() }).map(|_| ())
    }

    /// Remembers that a submit changed the revision at `link` on
    /// Phabricator now (`Record::Submitted`).
    pub fn store_submitted(&mut self, link: &str) -> Result<()> {
        self.push(Record::Submitted { link: link.into(), time: now() }).map(|_| ())
    }

    /// The repository on Phabricator of PHID `phid`, as a submit read it:
    /// none before one did.
    pub fn repository(&self, phid: &str) -> Option<&Repository> {
        match self.at(*self.repositories.get(phid)?) {
            Record::Repository(r) => Some(r),
            _ => None,
        }
    }

    /// The repository on Phabricator of callsign `callsign`, as a submit
    /// read it, by that callsign or by its PHID: none before one did.
    pub fn repository_called(&self, callsign: &str) -> Option<&Repository> {
        self.repositories().into_iter().find(|r| !callsign.is_empty() && r.callsign == callsign)
    }

    /// Every repository on Phabricator a submit read, by PHID.
    pub fn repositories(&self) -> Vec<&Repository> {
        self.repositories
            .values()
            .filter_map(|&i| match self.at(i) {
                Record::Repository(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    /// Stores repository `r`, as a submit read it now, unless it is stored
    /// as it is, when it was read aside. Whether it wrote it.
    pub fn store_repository(&mut self, r: Repository) -> Result<bool> {
        let same = |was: &Repository| (&was.phid, &was.callsign, &was.vcs) == (&r.phid, &r.callsign, &r.vcs);
        if self.repository(&r.phid).is_some_and(same) {
            return Ok(false);
        }
        self.push(Record::Repository(Repository { time: now(), ..r }))?;
        Ok(true)
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
            .filter_map(|&i| match self.at(i) {
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
        match self.at(*self.runs.get(id)?) {
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
        if self.cp.as_ref().is_some_and(|c| c.stale) {
            return self.full().live_runs();
        }
        self.run_order
            .iter()
            // A run not read was done with at the checkpoint, and nothing
            // was written of it since.
            .filter_map(|id| match &self.records[self.runs[id]] {
                Record::Run(r) => Some(r),
                _ => None,
            })
            .filter(|r| r.state == RunState::Started || crate::held::waiting(self, r))
            .collect()
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
        if self.cp.as_ref().is_some_and(|c| c.unread.contains(id)) {
            return self.full().events(id);
        }
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
        if self.cp.as_ref().is_some_and(|c| c.unread.contains(id)) {
            return self.full().event_count(id);
        }
        self.events.get(id).map_or(0, Vec::len)
    }

    /// How many records are not lines of an agent's stream: when it moves,
    /// something the editor shows may have changed. The one sign of change
    /// that what the editor works out from the database is kept by: a line
    /// read cut short is counted only once it is read whole.
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
        match self.at(*self.reviews.get(id)?) {
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
    /// moved it: no check that it applies, as nothing new is chosen; the
    /// patches that should not change are guarded as by every write.
    pub fn restore_head(&mut self, name: &str, rev: Option<usize>, op: String) -> Result<usize> {
        self.push_head(name, rev, op)
    }

    /// The comments of stack `name` on `change` that were deleted, oldest
    /// first: each as it was last, with the rev of the record that deleted
    /// it. A comment the agent moved to the stack is not one of them, nor
    /// one moved back to where it came from.
    pub fn deleted_notes(&self, name: &str, change: &str) -> Vec<(usize, Note)> {
        if self.cp.is_some() {
            return self.full().deleted_notes(name, change);
        }
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

    /// The comment `id` on `change`, if it is not deleted: its latest
    /// record not read is one that deleted it.
    pub fn note(&self, change: &str, id: &str) -> Option<&Note> {
        match &self.records[*self.notes.get(&(change.to_string(), id.to_string()))?] {
            Record::Comment(n) if !n.text.is_empty() => Some(n),
            _ => None,
        }
    }

    /// Writes a comment; empty text deletes it.
    pub fn set_note(&mut self, note: Note) -> Result<()> {
        let same = self.notes.get(&(note.change.clone(), note.id.clone())).is_some_and(|&at| {
            // Not read, it deleted the comment: the same only if this does too.
            if matches!(self.records[at], Record::Unloaded) && !note.text.is_empty() {
                return false;
            }
            matches!(self.at(at), Record::Comment(n) if *n == Note { time: n.time, ..note.clone() })
        });
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
    fn a_comment_queued_before_models_were_picked_loads_with_none_and_one_of_a_model_keeps_its_id() {
        // Written before: no model, read as none, and written so again.
        let before: Queued = serde_json::from_str(r#"{"way": "answer", "batch": "b1", "effort": "max"}"#).unwrap();
        assert_eq!((before.model.as_deref(), before.effort), (None, Some(Effort::Max)));
        assert!(!serde_json::to_string(&before).unwrap().contains("model"));
        // With one: its id, kept as it is, one the list no longer has too.
        for id in ["claude-opus-5-5", "claude-gone-4"] {
            let q = Queued { model: Some(id.into()), ..before.clone() };
            let read: Queued = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
            assert_eq!(read.model.as_deref(), Some(id));
        }
        // An ask's own, the same.
        let info = |extra: &str| serde_json::from_str::<crate::ask::Info>(&format!(r#"{{"way": "answer", "scope": "stack", "from": 0, "notes": []{extra}}}"#)).unwrap();
        assert_eq!((info("").model, info(r#", "model": "claude-haiku-5-5""#).model.as_deref()), (None, Some("claude-haiku-5-5")));
        let run = |ask: serde_json::Value| Run { ask: Some(ask), ..Default::default() };
        assert_eq!(crate::ask::model_of(&run(serde_json::json!({"effort": "low"}))), None);
        assert_eq!(crate::ask::at_of(&run(serde_json::json!({"effort": "low", "model": "claude-opus-5-5"}))).bracketed().as_deref(), Some("claude-opus-5-5, low"));
    }

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
            crate::ask::promote(db, "s", &note("m", "needs the stack"), "it needs more", &crate::thread::At::default())
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
        assert_fixed(&bytes);
    }

    #[test]
    fn a_database_made_empty_is_its_header_and_never_a_file_written_over() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("new.jsonl");
        Db::create(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), format!("{}\n", header_line()).as_bytes());
        let db = Db::load(&path).unwrap();
        assert!(db.fixed && db.warnings.is_empty() && db.stacks().is_empty(), "{:?}", db.warnings);
        // Written to as any database is.
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        assert_eq!(Db::load(&path).unwrap().view("s"), Some("all"));
        assert_fixed(&std::fs::read(&path).unwrap());
        // A file there already: an error, its bytes as they were.
        let before = std::fs::read(&path).unwrap();
        assert!(Db::create(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(tmp.path().join("notes.txt"), "mine\n").unwrap();
        assert!(Db::create(&tmp.path().join("notes.txt")).is_err());
        assert_eq!(std::fs::read_to_string(tmp.path().join("notes.txt")).unwrap(), "mine\n");
    }

    /// That `bytes` start with the header of the fixed length: pointing at
    /// no checkpoint, unless one is written at every write.
    fn assert_fixed(bytes: &[u8]) {
        assert_eq!(bytes[HEADER - 1], b'\n');
        let h: Header = serde_json::from_slice(&bytes[..HEADER - 1]).unwrap();
        assert_eq!(h.version, VERSION);
        if !test_checkpoints() {
            assert_eq!(&bytes[..HEADER - 1], header_line().as_bytes());
        }
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
        assert_eq!(db.end_rev(), 3, "the header and two views");
        assert!(matches!(db.rev(3), Some(Record::View { show, .. }) if show == "none"));
        // The next write ends the line cut short; its record is the next
        // rev.
        let rev = Db::edit(&path, |db| {
            db.set_view("s", "all")?;
            Ok(db.end_rev())
        })
        .unwrap();
        assert_eq!(rev, 4);
        let db = Db::load(&path).unwrap();
        assert_eq!((db.end_rev(), db.view("s")), (4, Some("all")));
        assert!(db.warnings.is_empty(), "{:?}", db.warnings);
        // A line that does not read keeps its rev, and is said by its line.
        let no = std::fs::read(&path).unwrap().iter().filter(|&&b| b == b'\n').count() + 1;
        add(b"garbage\n");
        let db = Db::load(&path).unwrap();
        assert_eq!(db.end_rev(), 5);
        assert!(db.warnings[0].contains(&format!(":{no}: passed over")), "{:?}", db.warnings);
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
        assert_fixed(&bytes);
        assert!(bytes[HEADER..].starts_with(&old.as_bytes()[old.find('\n').unwrap() + 1..]), "every line after it as it was");
        assert_ne!(ino(&path), was, "a new file, renamed over the old");
        let db = Db::load(&path).unwrap();
        assert_eq!(db.end_rev(), 5);
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
        assert_eq!((db.end_rev(), db.view("s")), (5, Some("none")));
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

    /// A database with a history, its revs said:
    /// - @2-@5 two versions of a patch and the stacks of each, the second
    ///   edited from the first; @6 an undo back to the first;
    /// - @7-@10 a run done with, and the lines its agent wrote; @11 a run
    ///   going, @12 a line its agent wrote;
    /// - @13 a comment open; @14 one opened, @15 closed;
    /// - @16 a view, @17 a choice, @18 a read mark, @19 a collapse.
    fn history(path: &Path) {
        let p1 = Patch::new("c".into(), None, None, "Bug 1 - one".into(), "diff --git a/a b/a\n".into());
        let p2 = Patch::new("c".into(), None, None, "Bug 1 - one again".into(), "diff --git a/a b/a\n".into());
        let stack = |p: &Patch, parent: Option<usize>, op: &str| {
            Record::Stack(Stack {
                name: "s".into(),
                patches: vec![p.id.clone()],
                base: None,
                repo: None,
                parent,
                op: op.into(),
                time: 1,
            })
        };
        let run = |id: &str, state: RunState| {
            Record::Run(Run {
                id: id.into(),
                what: "edit".into(),
                stack: "s".into(),
                state,
                pid: Some(std::process::id()),
                ask: Some(serde_json::json!({"from": 3})),
                time: 3,
                ..Default::default()
            })
        };
        let event = |run: &str, n: usize| Record::Event {
            run: run.into(),
            event: serde_json::json!({"type": "x", "n": n}),
        };
        let undo = Record::Head(Head {
            name: "s".into(),
            rev: Some(3),
            op: "undo edit".into(),
            time: 2,
        });
        Db::append(
            path,
            &[
                Record::Patch(p1.clone()),
                stack(&p1, None, "import"),
                Record::Patch(p2.clone()),
                stack(&p2, Some(3), "edit"),
                undo,
                run("r1", RunState::Started),
                event("r1", 1),
                event("r1", 2),
                run("r1", RunState::Done),
                run("r2", RunState::Started),
                event("r2", 1),
            ],
        )
        .unwrap();
        let note = |id: &str, text: &str| Note {
            change: "c".into(),
            id: id.into(),
            line: format!("+{id}"),
            text: text.into(),
            ..Default::default()
        };
        Db::edit(path, |db| {
            db.set_note(note("a", "why?"))?;
            db.set_note(note("b", "and?"))?;
            db.set_note(note("b", ""))?;
            db.set_view("s", "all")?;
            db.set_choice("s", "ask", "whole")?;
            db.mark_read("c", vec!["+x".into()])?;
            db.set_collapsed("c", "a", true)
        })
        .unwrap();
    }

    /// A checkpoint written now, as a write past the threshold does.
    fn checkpoint(path: &Path) {
        let (mut db, mut lock) = Db::lock(path).unwrap();
        db.write_checkpoint(&mut lock).unwrap();
    }

    #[test]
    fn a_database_read_from_its_checkpoint_is_the_one_read_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        checkpoint(&path);
        // And a line after it.
        Db::append(&path, &[Record::Event { run: "r2".into(), event: serde_json::json!({"type": "y"}) }]).unwrap();
        let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        let mut db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!(db.differs(&whole), None);
        // Read: the stacks undo and redo go to, their patches, the run
        // going and what its agent wrote, what is open, chosen and marked.
        let read = |rev: usize| !matches!(db.records[rev - 1], Record::Unloaded);
        for rev in [2, 3, 4, 5, 11, 12, 13, 16, 17, 18, 20] {
            assert!(read(rev), "@{rev} read");
        }
        // Not: the undo, the run done with, the comment closed, the
        // collapse (the checkpoint holds what is collapsed).
        for rev in [6, 7, 8, 9, 10, 14, 15, 19] {
            assert!(!read(rev), "@{rev} not read");
        }
        assert_eq!(db.live_runs().iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2"]);
        assert_eq!(db.events("r2").len(), 2);
        assert_eq!(db.note("c", "a").map(|n| n.text.as_str()), Some("why?"));
        assert!(db.note("c", "b").is_none() && db.collapsed("c", "a"));
        assert_eq!((db.undo_target("s"), db.redo_target("s")), (whole.undo_target("s"), whole.redo_target("s")));
        assert_eq!(db.redo_target("s"), Some((5, "edit".into())));
        assert!(db.shadow.get().is_none(), "nothing has read the whole history");
        // The history, read the first time it is asked.
        assert_eq!(db.versions("c"), whole.versions("c"));
        assert_eq!(db.note_history("c", "b"), whole.note_history("c", "b"));
        assert_eq!(db.events("r1"), whole.events("r1"));
        assert_eq!(db.runs(), whole.runs());
        assert_eq!(db.deleted_notes("s", "c"), whole.deleted_notes("s", "c"));
        assert!(db.shadow.get().is_some());
        // Taken in before anything changes.
        db.catch_up().unwrap();
        assert!(db.cp.is_none() && db.records == whole.records);
    }

    #[test]
    fn the_edits_since_a_state_of_a_stack_are_counted_along_its_tree_by_the_index_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let patch = |n: usize| Patch::new("c".into(), None, None, format!("Bug 1 - take {n}"), "diff --git a/a b/a\n".into());
        let stack = |p: &Patch, parent: Option<usize>| {
            Record::Stack(Stack {
                name: "s".into(),
                patches: vec![p.id.clone()],
                base: None,
                repo: None,
                parent,
                op: "edit".into(),
                time: 1,
            })
        };
        let (p1, p2, p3, p4) = (patch(1), patch(2), patch(3), patch(4));
        // Made at @3, edited at @5, then at @7.
        Db::append(&path, &[Record::Patch(p1.clone()), stack(&p1, None), Record::Patch(p2.clone()), stack(&p2, Some(3)), Record::Patch(p3.clone()), stack(&p3, Some(5))]).unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!([3, 5, 7].map(|rev| db.since("s", rev)), [Some(Since::Edits(2)), Some(Since::Edits(1)), Some(Since::Edits(0))]);
        // Of no state of the stack's, nothing: a patch's rev, another name.
        assert_eq!((db.since("s", 2), db.since("t", 3)), (None, None));
        drop(db);
        // The last edit undone, and another made in its place, at @10: @7 is
        // another version; the rest counted along the way to @10. Read from
        // a checkpoint, as read whole, the history not read for it.
        let undo = Record::Head(Head {
            name: "s".into(),
            rev: Some(5),
            op: "undo edit".into(),
            time: 2,
        });
        Db::append(&path, &[undo, Record::Patch(p4.clone()), stack(&p4, Some(5))]).unwrap();
        checkpoint(&path);
        let db = Db::load(&path).unwrap();
        let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        let since = [3, 5, 7, 10].map(|rev| db.since("s", rev));
        assert_eq!(since, [Some(Since::Edits(2)), Some(Since::Edits(1)), Some(Since::Elsewhere), Some(Since::Edits(0))]);
        assert_eq!(since, [3, 5, 7, 10].map(|rev| whole.since("s", rev)));
        assert!(db.cp.is_some() && db.shadow.get().is_none(), "read from its checkpoint, nothing read of the whole history");
    }

    #[test]
    fn a_checkpoint_that_does_not_check_out_is_passed_over_the_file_read_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        checkpoint(&path);
        let good = std::fs::read(&path).unwrap();
        let whole = Db::from_bytes(&path, &good).unwrap();
        let ptr = serde_json::from_slice::<Header>(&good[..HEADER - 1]).unwrap().checkpoint.unwrap();
        let line = good[ptr.at as usize..(ptr.at + ptr.len) as usize].to_vec();
        // The header pointed at checkpoint line `line`, appended.
        let pointing = |line: &[u8]| {
            let mut b = good.clone();
            let at = b.len() as u64;
            b.extend_from_slice(line);
            b.push(b'\n');
            b[..HEADER - 1].copy_from_slice(header_with(Some(&Pointer::new(at, ptr.rev, line))).unwrap().as_bytes());
            b
        };
        let edited = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut v: serde_json::Value = serde_json::from_slice(&line).unwrap();
            edit(&mut v);
            pointing(serde_json::to_string(&v).unwrap().as_bytes())
        };
        let header = |p: Pointer| {
            let mut b = good.clone();
            b[..HEADER - 1].copy_from_slice(header_with(Some(&p)).unwrap().as_bytes());
            b
        };
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("the header half written", {
                let mut b = good.clone();
                b[30..90].fill(b'#');
                b
            }),
            (
                "a sum that does not match",
                header(Pointer {
                    sum: "0000000000000000".into(),
                    ..ptr.clone()
                }),
            ),
            (
                "a pointer past the end",
                header(Pointer {
                    at: good.len() as u64 + 100,
                    ..ptr.clone()
                }),
            ),
            ("a checkpoint's line cut short", pointing(&line[..line.len() / 2])),
            ("a checkpoint of another layout", edited(&|v| v["state"] = 99.into())),
            ("a record not where it says", edited(&|v| v["load"][0][1] = (v["load"][0][1].as_u64().unwrap() + 1).into())),
            ("a head on a stack not read", edited(&|v| v["load"] = serde_json::json!([]))),
        ];
        for (what, bytes) in cases {
            std::fs::write(&path, &bytes).unwrap();
            let db = Db::load(&path).unwrap();
            assert!(db.cp.is_none(), "{what}: read whole");
            assert_eq!(db.records[1..], whole.records[1..], "{what}");
            assert_eq!(db.head("s").map(|(r, _)| r), Some(3), "{what}");
        }
        // The header half written, it is written again under the lock.
        let mut b = good.clone();
        b[30..90].fill(b'#');
        std::fs::write(&path, &b).unwrap();
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.warnings.is_empty() && db.fixed, "{:?}", db.warnings);
        assert_eq!((db.end_rev(), db.view("s")), (20, Some("none")));
    }

    #[test]
    fn a_reader_reads_while_checkpoints_are_written_and_pointed_at() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                for k in 0..60 {
                    Db::edit(&path, |db| db.set_view("s", if k % 2 == 0 { "none" } else { "all" })).unwrap();
                    checkpoint(&path);
                }
            })
        };
        let mut read = 0;
        while !writer.is_finished() {
            let db = Db::load(&path).unwrap();
            // Each read as the file read whole up to where it read it.
            let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()[..db.len as usize]).unwrap();
            assert_eq!(db.records.len(), whole.records.len());
            assert_eq!(db.view("s"), whole.view("s"));
            assert_eq!(db.head("s").map(|(r, _)| r), Some(3));
            read += usize::from(db.cp.is_some());
        }
        writer.join().unwrap();
        assert!(read > 0, "some read from a checkpoint");
    }

    #[test]
    fn a_checkpoint_is_written_once_the_file_has_grown_enough() {
        if test_checkpoints() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        let pointer = || serde_json::from_slice::<Header>(&std::fs::read(&path).unwrap()[..HEADER - 1]).unwrap().checkpoint;
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        assert_eq!(pointer(), None, "the file is short");
        // The agent of the run going writes on, past `FLOOR`.
        let big = "x".repeat(4096);
        let lines: Vec<Record> = (0..70).map(|n| Record::Event { run: "r2".into(), event: serde_json::json!({"n": n, "text": big}) }).collect();
        Db::append(&path, &lines).unwrap();
        assert_eq!(pointer(), None, "appending alone writes none");
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        let first = pointer().expect("the next write writes one");
        assert!(Db::load(&path).unwrap().cp.is_some());
        // Not again until the file grows past it: four times its length,
        // and `FLOOR`.
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        assert_eq!(pointer(), Some(first.clone()));
        // A head moved: at once.
        Db::append(&path, &[Record::Head(Head { name: "s".into(), rev: Some(5), op: "redo edit".into(), time: 4 })]).unwrap();
        Db::edit(&path, |db| db.restore_head("s", Some(3), "undo edit".into()).map(|_| ())).unwrap();
        let db = Db::load(&path).unwrap();
        assert_ne!(pointer(), Some(first));
        assert!(db.cp.is_some());
        assert_eq!(db.head("s").map(|(r, _)| r), Some(3));
    }

    #[test]
    fn where_stacked_left_head_is_kept_each_move_replacing_it_and_read_from_a_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        let (co, other) = (Path::new("/checkout"), Path::new("/other"));
        // Each move in place of the one before, per stack and checkout; the
        // same again, nothing written.
        Db::edit(&path, |db| {
            db.set_moved("s", co, "a1")?;
            db.set_moved("s", co, "b2")?;
            db.set_moved("s", other, "c3")?;
            db.set_moved("t", co, "d4")
        })
        .unwrap();
        let n = Db::load(&path).unwrap().end_rev();
        Db::edit(&path, |db| db.set_moved("s", co, "b2")).unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!(db.end_rev(), n, "nothing written");
        assert_eq!((db.moved("s", co), db.moved("s", other), db.moved("t", co), db.moved("t", other)), (Some("b2"), Some("c3"), Some("d4"), None));
        assert_eq!(db.moved_all("s"), [("/checkout", "b2"), ("/other", "c3")]);
        // Read from a checkpoint: the same, the records it stands on read.
        checkpoint(&path);
        let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!(db.differs(&whole), None);
        assert_eq!((db.moved("s", co), db.moved("s", other), db.moved("t", co)), (Some("b2"), Some("c3"), Some("d4")));
        assert!(db.shadow.get().is_none(), "nothing has read the whole history");
        // Written as a record of its own.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"{"t":"moved","stack":"s","checkout":"/checkout","commit":"b2"}"#), "{text}");
    }

    #[test]
    fn a_checkpoint_of_before_where_stacked_left_head_reads_as_none() {
        // A database a stacked of before wrote: no record of where stacked
        // left HEAD, and a checkpoint without their index.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        checkpoint(&path);
        let good = std::fs::read(&path).unwrap();
        let ptr = serde_json::from_slice::<Header>(&good[..HEADER - 1]).unwrap().checkpoint.unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&good[ptr.at as usize..(ptr.at + ptr.len) as usize]).unwrap();
        assert!(v.as_object_mut().unwrap().remove("moved").is_some());
        // In its place, the file's last line.
        let line = serde_json::to_string(&v).unwrap();
        assert_eq!(good.len() as u64, ptr.at + ptr.len + 1);
        let mut b = good[..ptr.at as usize].to_vec();
        b.extend_from_slice(line.as_bytes());
        b.push(b'\n');
        b[..HEADER - 1].copy_from_slice(header_with(Some(&Pointer::new(ptr.at, ptr.rev, line.as_bytes()))).unwrap().as_bytes());
        std::fs::write(&path, &b).unwrap();
        // Read from it all the same, as read whole; then a move kept.
        let whole = Db::from_bytes(&path, &b).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!((db.differs(&whole), db.moved("s", Path::new("/checkout"))), (None, None));
        Db::edit(&path, |db| db.set_moved("s", Path::new("/checkout"), "a1")).unwrap();
        assert_eq!(Db::load(&path).unwrap().moved("s", Path::new("/checkout")), Some("a1"));
    }

    /// What a read of Phabricator learnt of D`n`: its state, its latest
    /// diff `diff`, and its comments, of ids `ids`, as of when it was
    /// modified, `modified`.
    fn revision_read(n: u64, diff: u64, modified: Option<i64>, ids: &[&str]) -> (State, Comments) {
        let state = State { status: "needs-review".into(), title: format!("Bug {n}"), phid: format!("PHID-DREV-{n}"), diff, ..Default::default() };
        let comments = Comments { modified, list: ids.iter().map(|id| Seen { id: id.to_string(), ..Default::default() }).collect() };
        (state, comments)
    }

    #[test]
    fn what_is_read_of_phabricator_is_stored_once_and_read_from_a_checkpoint_as_read_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        let link = |n: u64| format!("https://p.example/D{n}");
        // D11 and D12 read, with their raw diffs; D13 not found; the fetch.
        Db::edit(&path, |db| {
            for (n, diff) in [(11, 5), (12, 6)] {
                let (s, c) = revision_read(n, diff, Some(100), &["c1"]);
                assert!(db.store_revision(&link(n), &format!("D{n}"), Some(s), Some(c))?);
                assert!(db.store_raw_diff(&link(n), diff, &format!("raw {diff}"))?);
            }
            db.store_revision(&link(13), "D13", Some(State { missing: true, ..Default::default() }), None)?;
            db.store_fetched(vec![link(11), link(12), link(13)])
        })
        .unwrap();
        let n = Db::load(&path).unwrap().end_rev();
        // The same again: nothing written of it, but that it was fetched.
        Db::edit(&path, |db| {
            let (s, c) = revision_read(11, 5, Some(100), &["c1"]);
            assert!(!db.store_revision(&link(11), "D11", Some(s), Some(c))?);
            assert!(!db.store_raw_diff(&link(11), 5, "raw 5")?);
            db.store_fetched(vec![link(11)])
        })
        .unwrap();
        assert_eq!(Db::load(&path).unwrap().end_rev(), n + 1);
        // D12 commented on since: its comments alone written again, its
        // state as it was; then a new diff of it: its state alone, and that
        // raw diff.
        Db::edit(&path, |db| {
            let (s, c) = revision_read(12, 6, Some(200), &["c1", "c2"]);
            db.store_revision(&link(12), "D12", Some(s), Some(c))?;
            let (s, c) = revision_read(12, 7, Some(200), &["c1", "c2"]);
            db.store_revision(&link(12), "D12", Some(s), Some(c))?;
            db.store_raw_diff(&link(12), 7, "raw 7").map(|_| ())
        })
        .unwrap();
        let db = Db::load(&path).unwrap();
        let of_d12: Vec<(Option<u64>, Option<usize>)> = db
            .records()
            .iter()
            .filter_map(|r| match r {
                Record::Revision(r) if r.link == link(12) => Some((r.state.as_ref().map(|s| s.diff), r.comments.as_ref().map(|c| c.list.len()))),
                _ => None,
            })
            .collect();
        assert_eq!(of_d12, [(Some(6), Some(1)), (None, Some(2)), (Some(7), None)]);
        // What stands: each part as the latest record with it has it.
        assert_eq!(db.revision_state(&link(12)).map(|(s, _)| s.diff), Some(7));
        assert_eq!(db.revision_comments(&link(12)).map(|(c, _)| (c.modified, c.list.len())), Some((Some(200), 2)));
        assert_eq!((db.raw_diff(&link(12), 6), db.raw_diff(&link(12), 7), db.raw_diff(&link(12), 8)), (Some("raw 6"), Some("raw 7"), None));
        assert!(db.revision_state(&link(13)).is_some_and(|(s, _)| s.missing) && db.revision_comments(&link(13)).is_none());
        assert_eq!(db.stored_revisions(), [link(11), link(12), link(13)]);
        assert_eq!((db.revision_name("PHID-DREV-12"), db.revision_name("PHID-DREV-9")), (Some("D12"), None));
        drop(db);
        // Read from a checkpoint: the same, what stands read with it, the
        // raw diff no state names not.
        checkpoint(&path);
        let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!(db.differs(&whole), None);
        let read = |i: usize| !matches!(db.records[i], Record::Unloaded);
        assert!(read(db.revisions[&link(12)]) && read(db.revision_comments[&link(12)]) && read(db.raw_diffs[&(link(12), 7)]) && read(db.fetched[&link(11)]));
        assert!(!read(db.raw_diffs[&(link(12), 6)]), "the raw diff of D12's diff before");
        assert_eq!(db.revision_state(&link(12)).map(|(s, _)| s.diff), Some(7));
        assert_eq!(db.revision_comments(&link(12)).map(|(c, _)| c.list.len()), Some(2));
        assert_eq!((db.raw_diff(&link(12), 7), db.fetched(&link(13))), (Some("raw 7"), whole.fetched(&link(13))));
        assert!(db.shadow.get().is_none(), "nothing has read the whole history");
        // Written as records of their own.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"{"t":"rawdiff","link":"https://p.example/D12","diff":7,"raw":"raw 7"}"#), "{text}");
        assert!(text.contains(r#"{"t":"revision","link":"https://p.example/D13","d":"D13","state":{"missing":true},"time":"#), "{text}");
        assert!(text.contains(r#"{"t":"fetched","links":["https://p.example/D11"],"time":"#), "{text}");
    }

    #[test]
    fn a_checkpoint_of_before_what_is_read_of_phabricator_was_stored_reads_as_one_with_none() {
        // A checkpoint a stacked of before wrote: no index of what was read
        // of Phabricator, as it stored none.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        checkpoint(&path);
        let good = std::fs::read(&path).unwrap();
        let ptr = serde_json::from_slice::<Header>(&good[..HEADER - 1]).unwrap().checkpoint.unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&good[ptr.at as usize..(ptr.at + ptr.len) as usize]).unwrap();
        for k in ["revisions", "revision_comments", "raw_diffs", "fetched", "repositories", "submitted"] {
            assert!(v.as_object_mut().unwrap().remove(k).is_some(), "{k}");
        }
        let line = serde_json::to_string(&v).unwrap();
        let mut b = good[..ptr.at as usize].to_vec();
        b.extend_from_slice(line.as_bytes());
        b.push(b'\n');
        b[..HEADER - 1].copy_from_slice(header_with(Some(&Pointer::new(ptr.at, ptr.rev, line.as_bytes()))).unwrap().as_bytes());
        std::fs::write(&path, &b).unwrap();
        // Read from it all the same, as read whole; then a revision stored,
        // and a submit of it.
        let whole = Db::from_bytes(&path, &b).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!((db.differs(&whole), db.revision_state("https://p.example/D11")), (None, None));
        assert!(db.repositories().is_empty() && db.submitted_all_unread().is_empty());
        let (s, c) = revision_read(11, 5, None, &[]);
        Db::edit(&path, |db| db.store_revision("https://p.example/D11", "D11", Some(s), Some(c)).map(|_| ())).unwrap();
        assert_eq!(Db::load(&path).unwrap().revision_state("https://p.example/D11").map(|(s, _)| s.diff), Some(5));
        Db::edit(&path, |db| db.store_submitted("https://p.example/D11")).unwrap();
        assert!(Db::load(&path).unwrap().submitted_unread("https://p.example/D11").is_some());
    }

    #[test]
    fn a_submit_stands_until_a_read_of_its_revision_is_stored_after_it_read_from_a_checkpoint_as_read_whole_and_kept_by_a_trim() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        let link = |n: u64| format!("https://p.example/D{n}");
        // D13 read, then submitted; D14 made by a submit, never read; D12
        // submitted, then read, as a fetch, a submit's fetch first or a pull
        // stores what it read, then that it read it.
        Db::edit(&path, |db| {
            let (s, c) = revision_read(13, 5, Some(100), &[]);
            db.store_revision(&link(13), "D13", Some(s), Some(c))?;
            db.store_fetched(vec![link(13)])?;
            db.store_submitted(&link(13))?;
            db.store_submitted(&link(14))?;
            db.store_submitted(&link(12))?;
            db.store_fetched(vec![link(12)])
        })
        .unwrap();
        // Those submitted since a read: D13 and D14, each with when, however
        // close that is to the read, the order of the records deciding.
        let unread = |db: &Db| [12, 13, 14, 15].map(|n| db.submitted_unread(&link(n)).is_some());
        let db = Db::load(&path).unwrap();
        assert_eq!(unread(&db), [false, true, true, false]);
        let all = db.submitted_all_unread().into_iter().map(|(l, t)| (l.to_string(), t)).collect::<Vec<_>>();
        assert_eq!(all.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(), [link(13), link(14)]);
        assert!(all.iter().all(|(_, t)| *t > 0 && *t <= now()), "{all:?}");
        drop(db);
        // A record of its own.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"{"t":"submitted","link":"https://p.example/D13","time":"#), "{text}");
        // Read from a checkpoint: the same, each read with it.
        checkpoint(&path);
        let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!(db.differs(&whole), None);
        assert!([12, 13, 14].iter().all(|n| !matches!(db.records[db.submitted[&link(*n)]], Record::Unloaded)));
        assert_eq!(unread(&db), [false, true, true, false]);
        assert!(db.shadow.get().is_none(), "nothing has read the whole history");
        drop(db);
        // D13's state read again: its submit stands no more, no record taken
        // out; D14's stands. Then a trim's cut, and D14 fetched after it.
        Db::edit(&path, |db| {
            let (s, c) = revision_read(13, 6, Some(200), &[]);
            db.store_revision(&link(13), "D13", Some(s), Some(c)).map(|_| ())
        })
        .unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!(unread(&db), [false, false, true, false]);
        assert_eq!(db.records().iter().filter(|r| matches!(r, Record::Submitted { .. })).count(), 3);
        let cut = db.end_rev() + 1;
        let kept = db.submitted_all_unread().into_iter().map(|(l, t)| (l.to_string(), t)).collect::<Vec<_>>();
        drop(db);
        Db::edit(&path, |db| db.store_fetched(vec![link(14)])).unwrap();
        // Trimmed to what there is now: no submit stands, and none is kept.
        let now_only = tmp.path().join("now.jsonl");
        crate::trim::trim(&path, &now_only, None).unwrap();
        let made = Db::load(&now_only).unwrap();
        assert_eq!(unread(&made), [false; 4]);
        assert!(!made.records().iter().any(|r| matches!(r, Record::Submitted { .. })));
        // Trimmed from the cut: D14's submit kept as it stood there, with
        // when, before the fetch after it, which it says nothing after.
        let from_cut = tmp.path().join("from-cut.jsonl");
        crate::trim::trim(&path, &from_cut, Some(cut)).unwrap();
        let made = Db::load(&from_cut).unwrap();
        assert_eq!(unread(&made), [false; 4]);
        let submits: Vec<(String, i64)> = made
            .records()
            .iter()
            .filter_map(|r| match r {
                Record::Submitted { link, time } => Some((link.clone(), *time)),
                _ => None,
            })
            .collect();
        assert_eq!(submits, kept);
        // Submitted again there: it stands, as of then.
        Db::edit(&from_cut, |db| db.store_submitted(&link(14))).unwrap();
        assert_eq!(unread(&Db::load(&from_cut).unwrap()), [false, false, true, false]);
    }

    #[test]
    fn a_repository_read_of_phabricator_is_stored_once_found_by_its_phid_or_its_callsign_and_read_from_a_checkpoint_as_read_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        history(&path);
        let repo = |phid: &str, callsign: &str| Repository { phid: phid.into(), callsign: callsign.into(), vcs: "hg".into(), ..Default::default() };
        // Read by its callsign, then the same again: written once; another,
        // with no callsign, found by its PHID alone.
        Db::edit(&path, |db| {
            assert!(db.store_repository(repo("PHID-REPO-1", "MOZILLACENTRAL"))?);
            assert!(!db.store_repository(repo("PHID-REPO-1", "MOZILLACENTRAL"))?);
            assert!(db.store_repository(repo("PHID-REPO-2", ""))?);
            Ok(())
        })
        .unwrap();
        let db = Db::load(&path).unwrap();
        let found = |r: Option<&Repository>| r.map(|r| (r.phid.clone(), r.vcs.clone()));
        assert_eq!(found(db.repository("PHID-REPO-1")), Some(("PHID-REPO-1".into(), "hg".into())));
        assert_eq!(found(db.repository_called("MOZILLACENTRAL")), Some(("PHID-REPO-1".into(), "hg".into())));
        assert_eq!((found(db.repository("PHID-REPO-3")), found(db.repository_called(""))), (None, None));
        assert_eq!(db.repositories().len(), 2);
        assert!(db.repository("PHID-REPO-2").is_some_and(|r| r.time > 0));
        drop(db);
        // Read from a checkpoint: the same, each read with it.
        checkpoint(&path);
        let whole = Db::from_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.cp.is_some(), "read from its checkpoint");
        assert_eq!(db.differs(&whole), None);
        assert!(!matches!(db.records[db.repositories["PHID-REPO-1"]], Record::Unloaded));
        assert_eq!(found(db.repository_called("MOZILLACENTRAL")), Some(("PHID-REPO-1".into(), "hg".into())));
        assert!(db.shadow.get().is_none(), "nothing has read the whole history");
        // A record of its own.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"{"t":"repository","phid":"PHID-REPO-1","callsign":"MOZILLACENTRAL","vcs":"hg","time":"#), "{text}");
    }

    #[test]
    fn a_state_says_whether_the_revision_is_a_draft_and_its_repository_and_one_stored_before_says_neither() {
        // As a stacked of before stored one: neither said.
        let before: State = serde_json::from_str(r#"{"status":"changes-planned","phid":"PHID-DREV-1","diff":5}"#).unwrap();
        assert_eq!((before.draft, before.repository.as_str()), (None, ""));
        // As a read says them now; not a draft written as such, an empty
        // repository not written.
        let now = State { draft: Some(false), repository: "PHID-REPO-1".into(), ..before.clone() };
        let text = serde_json::to_string(&now).unwrap();
        assert_eq!(text, r#"{"status":"changes-planned","phid":"PHID-DREV-1","diff":5,"draft":false,"repository":"PHID-REPO-1"}"#);
        assert_eq!(serde_json::from_str::<State>(&text).unwrap(), now);
        let none = State { draft: Some(true), ..before };
        assert_eq!(serde_json::to_string(&none).unwrap(), r#"{"status":"changes-planned","phid":"PHID-DREV-1","diff":5,"draft":true}"#);
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
        // Two threads take it in turn: the second waits for the first, which
        // says once it holds it.
        let started = std::time::Instant::now();
        let (holding, holds) = std::sync::mpsc::channel();
        let held = {
            let path = path.clone();
            std::thread::spawn(move || {
                Db::edit(&path, |_| {
                    holding.send(()).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    Ok(())
                })
            })
        };
        holds.recv().unwrap();
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        assert!(started.elapsed() >= std::time::Duration::from_millis(300), "it waited its turn");
        held.join().unwrap().unwrap();
    }

    #[test]
    fn a_reader_never_reads_a_record_half_written() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        let mut reader = Db::load(&path).unwrap();
        // A writer that writes its record in two halves, and pauses between
        // them, under its lock: as one slowed down midway.
        let halves = |k: usize, edit: bool| {
            let (said, half) = std::sync::mpsc::channel();
            let path = path.clone();
            let writer = std::thread::spawn(move || {
                HALVES.with(|h| *h.borrow_mut() = Some((said, std::time::Duration::from_millis(300))));
                let event = serde_json::json!({"type": "text", "text": format!("line {k}")});
                match edit {
                    true => Db::edit(&path, |db| db.add_event("r", event).map(|_| ())),
                    false => Db::append(&path, &[Record::Event { run: "r".into(), event }]),
                }
            });
            half.recv().unwrap();
            writer
        };
        for (k, edit) in [(0, false), (1, true)] {
            // Its first half is in the file: a reader catching up, or reading
            // the file anew, waits for the whole, and reads it whole.
            let writer = halves(k, edit);
            assert!(reader.catch_up().unwrap());
            assert_eq!((reader.events("r").len(), reader.torn), (2 * k + 1, false));
            assert!(reader.warnings.is_empty(), "{:?}", reader.warnings);
            writer.join().unwrap().unwrap();
            let writer = halves(k + 10, edit);
            let fresh = Db::load(&path).unwrap();
            assert_eq!((fresh.events("r").len(), fresh.torn), (2 * k + 2, false));
            assert!(fresh.warnings.is_empty(), "{:?}", fresh.warnings);
            writer.join().unwrap().unwrap();
            assert!(reader.catch_up().unwrap());
        }
        // The whole file too, as a trim reads it.
        let writer = halves(20, false);
        let whole = Db::from_bytes(&path, &Db::bytes(&path).unwrap()).unwrap();
        assert!(whole.warnings.is_empty() && !whole.torn, "{:?}", whole.warnings);
        assert_eq!(whole.events("r").len(), 5);
        writer.join().unwrap().unwrap();
    }

    #[test]
    fn a_thread_holding_the_lock_reads_the_file_under_it() {
        // Under its own lock, a thread reads the file again, as a load inside
        // an edit does, with no shared lock asked for: that would wait on
        // the thread for ever.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        let read = Db::edit(&path, |db| {
            db.set_view("s", "none")?;
            let mut again = Db::load(&path)?;
            again.catch_up()?;
            Ok((again.view("s").map(String::from), Db::bytes(&path)?.len() as u64 == again.len))
        })
        .unwrap();
        assert_eq!(read, (Some("all".to_string()), true));
        assert_eq!(Db::load(&path).unwrap().view("s"), Some("none"));
    }

    /// The trailer naming revision `D{n}` on the tests' Phabricator, and its
    /// link.
    #[cfg(feature = "moz-phab")]
    fn named(n: u64) -> String {
        format!("Differential Revision: {}", link(n))
    }

    #[cfg(feature = "moz-phab")]
    fn link(n: u64) -> String {
        format!("https://phabricator.example.com/D{n}")
    }

    /// A checkout, and a database holding stack `s` of three of its commits,
    /// with messages `messages`: a.txt's line 2 changed; b.txt added; a.txt's
    /// line 20 changed.
    #[cfg(feature = "moz-phab")]
    fn protected_stack(messages: [&str; 3]) -> (tempfile::TempDir, PathBuf) {
        let git = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "a")
                .env("GIT_AUTHOR_EMAIL", "a@b")
                .env("GIT_COMMITTER_NAME", "a")
                .env("GIT_COMMITTER_EMAIL", "a@b")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path().join("src");
        std::fs::create_dir(&r).unwrap();
        git(&r, &["init", "-q", "-b", "main"]);
        git(&r, &["config", "commit.gpgsign", "false"]);
        let lines: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        std::fs::write(r.join("a.txt"), &lines).unwrap();
        git(&r, &["add", "."]);
        git(&r, &["commit", "-qm", "base"]);
        std::fs::write(r.join("a.txt"), lines.replace("line 2\n", "LINE 2\n")).unwrap();
        git(&r, &["commit", "-qam", messages[0]]);
        std::fs::write(r.join("b.txt"), "bee\n").unwrap();
        git(&r, &["add", "."]);
        git(&r, &["commit", "-qm", messages[1]]);
        std::fs::write(r.join("a.txt"), lines.replace("line 2\n", "LINE 2\n").replace("line 20\n", "LINE 20\n")).unwrap();
        git(&r, &["commit", "-qam", messages[2]]);
        let path = tmp.path().join("db.jsonl");
        let read = crate::import::read_range(&r, "HEAD~3..HEAD", false, false).unwrap();
        Db::edit(&path, |d| crate::ops::import(d, "s", read.drafts, false, Some((read.base, r.display().to_string())))).unwrap();
        (tmp, path)
    }

    /// The status of revision `D{n}` stored as a fetch stores it, now.
    #[cfg(feature = "moz-phab")]
    fn fetched_as(path: &Path, n: u64, status: &str) {
        let state = State { status: status.into(), phid: format!("PHID-DREV-{n}"), ..Default::default() };
        Db::edit(path, |db| db.store_revision(&link(n), &format!("D{n}"), Some(state), None).map(|_| ())).unwrap();
    }

    /// Patch `at` of stack `s` given a new version, its diff with `from`
    /// written `to`: an edit to make with `Db::edit`.
    #[cfg(feature = "moz-phab")]
    fn changing(at: usize, from: &'static str, to: &'static str) -> impl Fn(&mut Db) -> Result<usize> {
        move |db: &mut Db| {
            let old = db.patch(&db.stack("s")?.patches[at]).clone();
            let draft = crate::mbox::Draft { diff: old.diff.replace(from, to), ..crate::mbox::Draft::from(&old) };
            crate::ops::replace(db, "s", at, draft, "edit")
        }
    }

    /// The patches of stack `s`, by their ids.
    #[cfg(feature = "moz-phab")]
    fn patches_of(path: &Path) -> Vec<String> {
        Db::load(path).unwrap().stack("s").unwrap().patches.clone()
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_write_that_changes_a_patch_that_should_not_change_is_refused_naming_it_and_writes_nothing_unless_it_was_confirmed() {
        let (_tmp, path) = protected_stack([&format!("Bug 1 - one\n\n{}", named(11)), &format!("Bug 2 - two\n\n{}", named(12)), "Bug 3 - three"]);
        fetched_as(&path, 11, "accepted");
        fetched_as(&path, 12, "published");
        let was = patches_of(&path);
        let bytes = std::fs::read(&path).unwrap();
        // Patch 1's diff changed: refused, as an error of its own kind
        // naming it, its revision, its status and what that means; nothing
        // written.
        let one = changing(0, "+LINE 2", "+Line two");
        match Db::edit_confirmed(&path, &[], &one) {
            Err(Failed::Protected(r)) => {
                assert_eq!((r.stack.as_str(), r.ids()), ("s", vec![was[0].clone()]));
                assert_eq!(r.lines(), ["Patch 1, D11, is Accepted on Phabricator: a change needs review again."]);
                assert_eq!(r.fetched(now()), "That status was fetched from Phabricator just now.");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        // An edit that does not ask for the refusal says it as its error.
        let e = Db::edit(&path, &one).unwrap_err();
        assert_eq!(e.0, "patch 1 should not change, so nothing changed: D11 is Accepted on Phabricator, and a change needs review again");
        // Confirmed for another patch, refused all the same.
        assert!(matches!(Db::edit_confirmed(&path, &[was[1].clone()], &one), Err(Failed::Protected(_))));
        // Patch 2's message alone changed, as it Landed: refused too.
        let reword = |db: &mut Db| {
            let old = db.patch(&db.stack("s")?.patches[1]).clone();
            let draft = crate::mbox::Draft { message: old.message.replacen("two", "TWO", 1), ..crate::mbox::Draft::from(&old) };
            crate::ops::replace(db, "s", 1, draft, "reword")
        };
        match Db::edit_confirmed(&path, &[], reword) {
            Err(Failed::Protected(r)) => assert_eq!(r.lines(), ["Patch 2, D12, is Landed: it is in the tree already."]),
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        // Both in one write: both named, in their order.
        let both = |db: &mut Db| {
            changing(0, "+LINE 2", "+Line two")(db)?;
            reword(db)
        };
        match Db::edit_confirmed(&path, &[], both) {
            // The first write refused: the second is not made.
            Err(Failed::Protected(r)) => assert_eq!(r.ids(), [was[0].clone()]),
            other => panic!("{other:?}"),
        }
        let whole = |db: &mut Db| {
            let s = db.stack("s")?.clone();
            let p1 = db.patch(&s.patches[0]).clone();
            let p2 = db.patch(&s.patches[1]).clone();
            let a = db.add_patch(crate::ops::revise(&p1, crate::mbox::Draft { diff: p1.diff.replace("+LINE 2", "+Line two"), ..crate::mbox::Draft::from(&p1) }))?;
            let b = db.add_patch(crate::ops::revise(&p2, crate::mbox::Draft { message: format!("{}, again", p2.message), ..crate::mbox::Draft::from(&p2) }))?;
            db.set_stack("s", vec![a, b, s.patches[2].clone()], "edit both")
        };
        match Db::edit_confirmed(&path, &[], whole) {
            Err(Failed::Protected(r)) => {
                assert_eq!(r.ids(), [was[0].clone(), was[1].clone()]);
                assert_eq!(r.title(false), "this changes 2 patches that should not change");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        // Confirmed for exactly those: written.
        Db::edit_confirmed(&path, &was[..2], whole).unwrap();
        let now_on = patches_of(&path);
        assert!(now_on[0] != was[0] && now_on[1] != was[1] && now_on[2] == was[2]);
        // Confirmed for patch 1, the version the stack held: written, for
        // that edit alone.
        let (_tmp, path) = protected_stack([&format!("Bug 1 - one\n\n{}", named(11)), "Bug 2 - two", "Bug 3 - three"]);
        fetched_as(&path, 11, "accepted");
        let was = patches_of(&path);
        Db::edit_confirmed(&path, &[was[0].clone()], &one).unwrap();
        let now_on = patches_of(&path);
        assert!(Db::load(&path).unwrap().patch(&now_on[0]).diff.contains("+Line two"));
        // The next edit of it, of its new version, asks again.
        assert!(matches!(Db::edit_confirmed(&path, &[], changing(0, "+Line two", "+Line 2")), Err(Failed::Protected(_))));
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_patch_that_should_not_change_dropped_moved_or_left_as_it_is_while_others_change_asks_nothing() {
        let (_tmp, path) = protected_stack([&format!("Bug 1 - one\n\n{}", named(11)), &format!("Bug 2 - two\n\n{}", named(12)), &format!("Bug 3 - three\n\n{}", named(13))]);
        fetched_as(&path, 11, "accepted");
        fetched_as(&path, 12, "published");
        fetched_as(&path, 13, "changes-planned");
        // Patch 3, Changes Planned, changed: nothing asked; so in Needs
        // Review.
        Db::edit_confirmed(&path, &[], changing(2, "+LINE 20", "+Line twenty")).unwrap();
        fetched_as(&path, 13, "needs-review");
        Db::edit_confirmed(&path, &[], changing(2, "+Line twenty", "+LINE 20")).unwrap();
        // Patch 2, Landed, moved to the top, its version as it was; patch 1,
        // Accepted, dropped.
        let was = patches_of(&path);
        Db::edit_confirmed(&path, &[], |db| crate::ops::move_patch(db, "s", 1, 2)).unwrap();
        assert_eq!(patches_of(&path), [was[0].clone(), was[2].clone(), was[1].clone()]);
        Db::edit_confirmed(&path, &[], |db| crate::ops::drop_patches(db, "s", &[0])).unwrap();
        assert_eq!(patches_of(&path), [was[2].clone(), was[1].clone()]);
        // A version of the same diff and message, its author alone another:
        // no change.
        Db::edit_confirmed(&path, &[], |db| {
            let old = db.patch(&db.stack("s")?.patches[1]).clone();
            crate::ops::replace(db, "s", 1, crate::mbox::Draft { author: Some("Someone Else <else@example.com>".into()), ..crate::mbox::Draft::from(&old) }, "edit")
        })
        .unwrap();
        assert_ne!(patches_of(&path)[1], was[1]);
        // Deleted, the stack drops them all: no change either.
        Db::edit_confirmed(&path, &[], |db| db.delete_stack("s")).unwrap();
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_patch_that_should_not_change_rebased_unchanged_onto_a_changed_patch_under_it_asks_nothing_and_one_whose_lines_change_is_refused() {
        // Patch 1 and patch 3 given new versions in one write, `under` and
        // `over` making their diffs of the ones they have, and `message`
        // patch 3's message: what the write came to.
        let write = |path: &Path, under: &dyn Fn(&str) -> String, over: &dyn Fn(&str) -> String, message: Option<&str>| {
            Db::edit_confirmed(path, &[], |db| {
                let s = db.stack("s")?.clone();
                let (p1, p3) = (db.patch(&s.patches[0]).clone(), db.patch(&s.patches[2]).clone());
                let a = db.add_patch(crate::ops::revise(&p1, crate::mbox::Draft { diff: under(&p1.diff), ..crate::mbox::Draft::from(&p1) }))?;
                let draft = crate::mbox::Draft { diff: over(&p3.diff), message: message.unwrap_or(&p3.message).into(), ..crate::mbox::Draft::from(&p3) };
                let c = db.add_patch(crate::ops::revise(&p3, draft))?;
                db.set_stack("s", vec![a, s.patches[1].clone(), c], "edit")
            })
        };
        // `from` written `to` in diff `d`, which holds it.
        let swap = |d: &str, from: &str, to: &str| {
            assert!(d.contains(from), "{from:?} in {d}");
            d.replacen(from, to, 1)
        };
        // Patch 1 with a line put in after its line 2, which moves patch 3's
        // lines down one.
        let line_in = |d: &str| swap(&swap(d, "+LINE 2\n", "+LINE 2\n+line 2.5\n"), "@@ -1,5 +1,5 @@", "@@ -1,5 +1,6 @@");
        // Patch 3 as a rebase onto that writes it: its hunk a line down, the
        // blobs its index line names others.
        let down = |d: &str| {
            let d = swap(d, "@@ -17,7 +17,7 @@", "@@ -18,7 +18,7 @@");
            d.lines().map(|l| if l.starts_with("index ") { "index 1234567..89abcde 100644\n".to_string() } else { format!("{l}\n") }).collect::<String>()
        };
        for status in ["accepted", "published"] {
            let (_tmp, path) = protected_stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{}", named(13))]);
            fetched_as(&path, 13, status);
            let was = patches_of(&path);
            // Rebased so, changing nothing of its own: written, no one asked,
            // its new version in the stack.
            write(&path, &line_in, &down, None).unwrap();
            let now_on = patches_of(&path);
            assert!(now_on[0] != was[0] && now_on[2] != was[2], "{status}");
            assert!(Db::load(&path).unwrap().patch(&now_on[2]).diff.contains("@@ -18,7 +18,7 @@"), "{status}");
            // A line of its own written otherwise as it is rebased: refused,
            // nothing written.
            let (_tmp, path) = protected_stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{}", named(13))]);
            fetched_as(&path, 13, status);
            let bytes = std::fs::read(&path).unwrap();
            let otherwise = |d: &str| swap(&down(d), "+LINE 20", "+LINE twenty");
            match write(&path, &line_in, &otherwise, None) {
                Err(Failed::Protected(r)) => assert_eq!(r.ids(), [patches_of(&path)[2].clone()], "{status}"),
                other => panic!("{status}: {other:?}"),
            }
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{status}: nothing written");
            // A line of its context changed by the patch under it: its diff
            // changes, refused too.
            let context = |d: &str| format!("{d}@@ -17,3 +17,3 @@\n line 17\n-line 18\n+LINE 18\n line 19\n");
            let around = |d: &str| swap(d, " line 18\n", " LINE 18\n");
            assert!(matches!(write(&path, &context, &around, None), Err(Failed::Protected(_))), "{status}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{status}: nothing written");
            // Its message written otherwise as it is rebased: refused.
            let reworded = format!("Bug 3 - THREE\n\n{}", named(13));
            assert!(matches!(write(&path, &line_in, &down, Some(&reworded)), Err(Failed::Protected(_))), "{status}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{status}: nothing written");
        }
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn undo_redo_and_a_stack_picked_from_the_tree_are_refused_as_any_write_that_changes_a_patch_that_should_not_change() {
        let (_tmp, path) = protected_stack([&format!("Bug 1 - one\n\n{}", named(11)), "Bug 2 - two", "Bug 3 - three"]);
        // Patch 1 changed, then Accepted as it is now.
        let first = Db::load(&path).unwrap().head("s").unwrap().0;
        Db::edit(&path, changing(0, "+LINE 2", "+Line two")).unwrap();
        fetched_as(&path, 11, "accepted");
        let accepted = patches_of(&path)[0].clone();
        let bytes = std::fs::read(&path).unwrap();
        // Undone, the version before would be back: refused, nothing written.
        match Db::edit_confirmed(&path, &[], |db| db.undo("s")) {
            Err(Failed::Protected(r)) => assert_eq!(r.ids(), std::slice::from_ref(&accepted)),
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        // So is that stack picked from the tree.
        assert!(matches!(Db::edit_confirmed(&path, &[], |db| db.select(first)), Err(Failed::Protected(_))));
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        // Confirmed: undone.
        Db::edit_confirmed(&path, std::slice::from_ref(&accepted), |db| db.undo("s")).unwrap();
        let before = patches_of(&path)[0].clone();
        assert_ne!(before, accepted);
        // Redone, the version it held would be back: refused, then confirmed.
        let bytes = std::fs::read(&path).unwrap();
        match Db::edit_confirmed(&path, &[], |db| db.redo("s")) {
            Err(Failed::Protected(r)) => assert_eq!(r.ids(), std::slice::from_ref(&before)),
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "nothing written");
        Db::edit_confirmed(&path, &[before], |db| db.redo("s")).unwrap();
        assert_eq!(patches_of(&path)[0], accepted);
        // Deleted, then brought back by an undo: from no stack, nothing
        // changes.
        Db::edit_confirmed(&path, &[], |db| db.delete_stack("s")).unwrap();
        Db::edit_confirmed(&path, &[], |db| db.undo("s")).unwrap();
        assert_eq!(patches_of(&path)[0], accepted);
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_patch_brought_to_what_phabricator_has_of_it_is_told_from_one_changed_otherwise() {
        let (_tmp, path) = protected_stack([&format!("Bug 1 - one\n\n{}", named(11)), "Bug 2 - two", "Bug 3 - three"]);
        let was = patches_of(&path);
        let db = Db::load(&path).unwrap();
        let p1 = db.patch(&was[0]).clone();
        drop(db);
        // D11's latest diff on Phabricator, stored raw: patch 1's as it is
        // now, its line written otherwise.
        let raw = p1.diff.replace("+LINE 2", "+Line two");
        let state = State {
            status: "accepted".into(),
            title: "Bug 1 - one, as titled there".into(),
            summary: "Its summary.".into(),
            phid: "PHID-DREV-11".into(),
            diff: 1000,
            ..Default::default()
        };
        Db::edit(&path, |db| {
            db.store_revision(&link(11), "D11", Some(state), None)?;
            db.store_raw_diff(&link(11), 1000, &raw).map(|_| ())
        })
        .unwrap();
        let mut db = Db::load(&path).unwrap();
        let version = |db: &mut Db, diff: &str, message: &str| -> Vec<String> {
            let id = db.add_patch(crate::ops::revise(&p1, crate::mbox::Draft { diff: diff.into(), message: message.into(), ..crate::mbox::Draft::from(&p1) })).unwrap();
            let mut now = was.clone();
            now[0] = id;
            now
        };
        // Its latest diff, the patch's message: Phabricator's own.
        let now = version(&mut db, &raw, &p1.message);
        assert_eq!(crate::protected::from_phabricator(&db, &was, &now), [was[0].clone()]);
        // Its title and summary as its message, the patch's diff: so too.
        let titled = format!("Bug 1 - one, as titled there\n\nIts summary.\n\n{}", named(11));
        let now = version(&mut db, &p1.diff, &titled);
        assert_eq!(crate::protected::from_phabricator(&db, &was, &now), [was[0].clone()]);
        // Both: so too.
        let now = version(&mut db, &raw, &titled);
        assert_eq!(crate::protected::from_phabricator(&db, &was, &now), [was[0].clone()]);
        // Another diff, or another message: a change of the pull's own, not
        // Phabricator's.
        let now = version(&mut db, &p1.diff.replace("+LINE 2", "+Line 2"), &p1.message);
        assert!(crate::protected::from_phabricator(&db, &was, &now).is_empty());
        assert_eq!(crate::protected::changes(&db, &was, &now).len(), 1);
        let now = version(&mut db, &raw, &format!("Bug 1 - one, reworded\n\n{}", named(11)));
        assert!(crate::protected::from_phabricator(&db, &was, &now).is_empty());
    }
}
