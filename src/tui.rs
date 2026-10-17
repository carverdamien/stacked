//! The editor: the current stack's patches on the left, under a header that
//! is the stack's own place; in the middle, the selected patch (unified or
//! side by side, comments under their lines) or, from the header, the stack
//! as a whole; on the right, the comments of what the middle shows, under a
//! header of their own, when there are any. Every change is appended to the
//! database; claude's asks run on their own and what they did shows up when
//! it lands.
//!
//! The keys are few: the arrows move (← on the patches hides them, → on the
//! comments too), enter does what is to be done where the cursor is, space
//! is the menu of all that can be done there, `?` says what that is, and esc
//! goes back. Nothing else is a shortcut: what the menu does, only the menu
//! does. Comments are written in an editor inline where they will show, and
//! sent to claude from their menu.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::ask::{self, Scope};
use crate::db::{Db, Note, Patch, Result, RunState, Stack, Way, err, local_time, short_id, stack_change};
use crate::editor::{Editor, Op};
use crate::ops::{self, edit_text, strip_comments};
use crate::paint::{color, fit, put, window, wrap};
use crate::panes::{Compact, Panes, Scroll, pane_streams, spans};
use crate::reading::{Blocks, Finder, Reading, Target as Block};
use crate::review;
use crate::rows::{By, Deco, Kind, Row, by_of, patch_rows, split_lines, split_row, stack_rows};
use crate::thread::{Role, append_turn, last_reply_way, last_role, thread_turns};
use crate::tree;
use crate::view::{MSG_PATH, note_lines, title_key};

fn by_style(by: Option<&By>) -> Style {
    match by {
        Some(By::Agent) => color::add(),
        Some(By::Reviewer) => color::reviewer(),
        _ => color::comment(),
    }
}

fn row_style(row: &Row, selected: bool) -> Style {
    let mut st = match row.kind {
        Kind::Add => color::add(),
        Kind::Del => color::del(),
        Kind::Hunk => color::hunk(),
        Kind::File => color::file(),
        Kind::Meta => color::meta(),
        Kind::Comment => by_style(row.by.as_ref()),
        Kind::Stream => color::dim(),
        _ => Style::new(),
    };
    if row.read {
        st = st.add_modifier(Modifier::DIM);
    }
    if selected {
        st = st.add_modifier(Modifier::REVERSED);
    }
    st
}

/// The reading mode's margin: a changed line's check when it is read, or
/// what Space (Enter) would make of it; three columns, and a space.
const MARGIN_W: usize = 4;

/// What the margin's marks say: what Space would read, or Enter unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tick {
    ToRead,
    ToUnread,
}

/// A line of an interdiff or a mail, coloured as a diff's.
fn diff_style(line: &str) -> Style {
    match line.chars().next() {
        Some('+') if !line.starts_with("+++") => color::add(),
        Some('-') if !line.starts_with("---") => color::del(),
        Some('@') => color::hunk(),
        _ => Style::new(),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn short(text: &str, n: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= n {
        text
    } else {
        format!("{}…", text.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The patches, on the left.
    List,
    /// The patch, or the stack, in the middle.
    Diff,
    /// The comments, on the right.
    Notes,
}

/// How the code is drawn, as self-review draws it: one column, the two
/// sides line by line, or the two sides each a pane of its own (compact).
/// The panes of History…: the history, the stack as it was at the record
/// picked, and its patch as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistFocus {
    #[default]
    List,
    Stack,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Unified,
    Split,
    Compact,
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Layout::Unified => "unified",
            Layout::Split => "side-by-side",
            Layout::Compact => "side-by-side, compact",
        }
    }

    fn next(self) -> Layout {
        match self {
            Layout::Unified => Layout::Split,
            Layout::Split => Layout::Compact,
            Layout::Compact => Layout::Unified,
        }
    }
}

/// What the screen is showing, over the panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    /// The current stack's tree of edits, one to pick.
    Tree,
    /// The stacks, one to switch to.
    Stacks,
    /// The runs, newest first.
    Runs,
    /// What a run's claude wrote.
    Transcript,
    /// A patch's versions, and what changed from one to the next.
    Versions,
    /// The stack's history, newest first.
    History,
    /// A text to read: what an edit changed, a comment's history.
    Text,
}

/// What the comments pane shows, as self-review's filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    All,
    ToSend,
    ToRead,
    ToReadAnswer,
    ToReadChange,
    InProgress,
    Failed,
    Gone,
    Nothing,
}

impl Show {
    pub const ALL: [Show; 9] = [
        Show::All,
        Show::ToSend,
        Show::ToRead,
        Show::ToReadAnswer,
        Show::ToReadChange,
        Show::InProgress,
        Show::Failed,
        Show::Gone,
        Show::Nothing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Show::All => "all",
            Show::ToSend => "to send",
            Show::ToRead => "to read",
            Show::ToReadAnswer => "to read: read-only",
            Show::ToReadChange => "to read: write",
            Show::InProgress => "in progress",
            Show::Failed => "failed or moved",
            Show::Gone => "gone",
            Show::Nothing => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Show> {
        Show::ALL.into_iter().find(|w| w.name() == s)
    }
}

/// Where a comment being written goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A new comment on the stack itself.
    Stack,
    /// A new comment on a patch as a whole, its title: the change.
    Patch(String),
    /// A comment on a line of a patch.
    Line {
        change: String,
        key: String,
        file: String,
        preview: String,
    },
    /// A reply to a comment.
    Reply { change: String, id: String },
    /// A comment on the stack's history: on its record `rev`.
    About(usize),
}

/// A comment being written, in the pane it is written in.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub ed: Editor,
    pub target: Target,
    pub pane: Focus,
}

/// Where the keys go, over the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    /// All that can be done: ↑↓ to choose, enter to do it.
    Menu(Menu),
    /// A comment being written, inline.
    Edit(Edit),
    /// The patch at `from` being moved, now at `to`: ↑↓ to place it.
    Moving { from: usize, to: usize },
    /// What can be done where the cursor is, said. The next key goes back
    /// to `back`, and does there what it does.
    Help { lines: Vec<String>, back: Box<Mode> },
    /// The stream of the comment claude is working on, opened in place;
    /// `up` lines up from its end.
    Stream { change: String, id: String, up: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub title: String,
    pub items: Vec<Item>,
    pub sel: usize,
    /// Why some items cannot be chosen, when some cannot.
    pub note: Option<String>,
    top: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    pub action: Action,
    pub enabled: bool,
}

/// What a menu item, or enter, does. An `Ask...` asks first: a menu of its
/// own.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A line between groups of items.
    Line,
    Cancel,
    Open,
    GoToNote,
    MovePatch,
    EditPatch,
    EditCode,
    EditAgain,
    ThrowEdit,
    AskDrop,
    Drop,
    CommentStack,
    CommentPatch,
    CommentLine,
    Reply,
    CommentInEditor,
    Send(Way),
    SendAll(Way),
    Unsend,
    OpenStream,
    AskStop(String),
    Stop(String),
    GoToOpened,
    AskDeleteOpened,
    DeleteOpened,
    GoToOrigin,
    MoveBack,
    NoteHistory,
    AskDeleteComment,
    DeleteComment,
    ShowMenu,
    SetShow(Show),
    Undo,
    Redo,
    EditList,
    Tree,
    EditFrom(usize),
    WhatChanged(usize),
    History,
    Versions,
    UseVersion(String),
    /// A new database, the history before this rev left out.
    AskTrim(usize),
    Trim(usize),
    /// A question on the history's record `rev`, for the agent to answer.
    AskAbout(usize),
    /// The comments on the history's record `rev`.
    GoToAbout(usize),
    /// The history, at its record `rev`.
    GoToHistory(usize),
    Whole(bool),
    MarkRead,
    MarkUnread,
    Pull,
    AskApply,
    Apply,
    Runs,
    Stacks,
    AskDeleteStack,
    DeleteStack,
    Transcript(String),
    /// What to do with a comment written: save it, and send it if `way`.
    Save(Option<Way>),
    ThrowAway,
    KeepWriting,
    Quit,
}

/// What `action` does, to follow "Press Enter to".
fn about(action: &Action) -> String {
    match action {
        Action::Line | Action::Cancel => "change nothing".into(),
        Action::Open => "read this patch".into(),
        Action::GoToNote => "go to this comment".into(),
        Action::MovePatch => "pick this patch up and put it elsewhere in the stack".into(),
        Action::EditPatch => "edit this patch in $EDITOR, as mail, opened where the cursor is".into(),
        Action::EditCode => "edit this file as the patch leaves it, in $EDITOR, opened at this line".into(),
        Action::EditAgain => "edit it again, as you left it".into(),
        Action::ThrowEdit => "throw your edit away".into(),
        Action::AskDrop => "drop this patch from the stack (undo brings it back)".into(),
        Action::Drop => "drop it".into(),
        Action::CommentStack => "create a comment on the stack, for the agent to work on the whole stack".into(),
        Action::CommentPatch => "create a comment on this patch as a whole".into(),
        Action::CommentLine => "create a comment on this line".into(),
        Action::Reply => "reply to this comment".into(),
        Action::CommentInEditor => "write it in $EDITOR".into(),
        Action::Send(Way::Change) => "ask the agent, which may change the code within this comment's place".into(),
        Action::Send(Way::Answer) => "ask the agent, to answer only: nothing changes".into(),
        Action::Send(Way::Test) => "ask the agent to build and test, in a workspace of its choosing".into(),
        Action::SendAll(Way::Change) => "ask the agent about every comment listed; it may change the code".into(),
        Action::SendAll(Way::Answer) => "ask the agent about every comment listed, to answer only".into(),
        Action::SendAll(Way::Test) => "ask the agent to build and test, for every comment listed".into(),
        Action::Unsend => "take it back from the agent's queue".into(),
        Action::OpenStream => "read what the agent is writing".into(),
        Action::AskStop(_) | Action::Stop(_) => "stop the agent: the ask lands as failed".into(),
        Action::GoToOpened => "go to the comments its answer opened".into(),
        Action::AskDeleteOpened | Action::DeleteOpened => "delete it and the comments its answer opened".into(),
        Action::GoToOrigin => "go to where it was before it moved to the stack".into(),
        Action::MoveBack => "move it back where it came from".into(),
        Action::NoteHistory => "read every state this comment has been in".into(),
        Action::AskDeleteComment | Action::DeleteComment => "delete this comment".into(),
        Action::ShowMenu => "pick which comments the panes show".into(),
        Action::SetShow(s) => format!("show the comments {}", s.name()),
        Action::Undo => "go back to the stack before this edit".into(),
        Action::Redo => "make that edit again".into(),
        Action::EditList => "edit the stack as `git rebase -i` does, in $EDITOR".into(),
        Action::Tree => "see every state of the stack, and edit from any".into(),
        Action::EditFrom(rev) => format!("edit from @{rev}"),
        Action::WhatChanged(rev) => format!("read what the edit that made @{rev} changed"),
        Action::History => "read everything that happened to the stack, newest first".into(),
        Action::Versions => "see every version of this patch, and what changed from one to the next".into(),
        Action::UseVersion(_) => "put this version back in the stack".into(),
        Action::AskAbout(rev) => format!("write a question on @{rev}, for the agent to answer: it may open comments, it changes nothing"),
        Action::GoToAbout(rev) => format!("go to the comments on @{rev}, among the stack's"),
        Action::GoToHistory(rev) => format!("go to @{rev} in the stack's history"),
        Action::AskTrim(rev) | Action::Trim(rev) => {
            format!("write a new database, its history from @{rev} on, before it its start; this one is not changed")
        }
        Action::Whole(true) => "read this version whole".into(),
        Action::Whole(false) => "read what changed from the version before".into(),
        Action::MarkRead => "mark this block as read".into(),
        Action::MarkUnread => "mark this block as unread".into(),
        Action::Pull => "pull the reviewers' comments from Phabricator".into(),
        Action::AskApply | Action::Apply => "commit the stack's patches in the worktree, on top of its base".into(),
        Action::Runs => "see the agent's asks, the applies and the pulls, and what came of them".into(),
        Action::Stacks => "switch to another stack".into(),
        Action::AskDeleteStack | Action::DeleteStack => "delete this stack (undo brings it back)".into(),
        Action::Transcript(_) => "see what the agent wrote".into(),
        Action::Save(None) => "save the comment".into(),
        Action::Save(Some(w)) => format!("save the comment and ask the agent ({})", w.said()),
        Action::ThrowAway => "throw what you wrote away".into(),
        Action::KeepWriting => "go back to writing".into(),
        Action::Quit => "quit: the agent's asks carry on".into(),
    }
}

impl Item {
    fn new(label: impl Into<String>, action: Action) -> Item {
        Item {
            label: label.into(),
            action,
            enabled: true,
        }
    }

    /// Offered, but only to be chosen if `ok`.
    fn when(mut self, ok: bool) -> Item {
        self.enabled = ok;
        self
    }

    fn line() -> Item {
        Item {
            label: String::new(),
            action: Action::Line,
            enabled: false,
        }
    }
}

impl Menu {
    /// `items`, with lines only between groups, the first that can be
    /// chosen selected. `note` is shown if some cannot be chosen.
    fn new(title: impl Into<String>, items: Vec<Item>, note: Option<String>) -> Menu {
        let mut out: Vec<Item> = Vec::new();
        for item in items {
            if item.action == Action::Line && out.last().is_none_or(|l| l.action == Action::Line) {
                continue;
            }
            out.push(item);
        }
        while out.last().is_some_and(|l| l.action == Action::Line) {
            out.pop();
        }
        let blocked = out.iter().any(|i| !i.enabled && i.action != Action::Line);
        Menu {
            title: title.into(),
            sel: out.iter().position(|i| i.enabled).unwrap_or(0),
            items: out,
            note: note.filter(|_| blocked),
            top: 0,
        }
    }

    /// A question: `yes` or `no`, with `no` selected if `careful`.
    fn ask(title: impl Into<String>, yes: Item, no: &str, careful: bool) -> Menu {
        let mut m = Menu::new(title, vec![yes, Item::new(no, Action::Cancel)], None);
        if careful {
            m.sel = 1;
        }
        m
    }

    /// Selects the next item that can be chosen, down or up, around the
    /// ends.
    fn step(&mut self, down: bool) {
        let n = self.items.len();
        for k in 1..=n {
            let i = if down { (self.sel + k) % n } else { (self.sel + n - k) % n };
            if self.items[i].enabled {
                self.sel = i;
                return;
            }
        }
    }

    pub fn chosen(&self) -> Option<&Item> {
        self.items.get(self.sel).filter(|i| i.enabled)
    }
}

/// What a key asks for that the event loop must do: run $EDITOR, which
/// needs the terminal back, or quit.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    None,
    Quit,
    /// The patch as mail, opened at `line` if one is known.
    EditPatch {
        line: Option<usize>,
    },
    /// `file` as the patch leaves it, at `line`; from `text`, an edit
    /// refused before, if given.
    EditCode {
        file: String,
        line: usize,
        text: Option<String>,
    },
    EditTodo,
    EditComment(Target),
}

/// Where the cursor is to go once the rows are built: to a comment, or to
/// a line.
#[derive(Debug, Clone, PartialEq)]
enum Want {
    Note(String),
    Line(String),
}

/// A text to read, on a screen of its own.
#[derive(Debug, Clone)]
pub struct TextView {
    pub title: String,
    pub lines: Vec<(String, Style)>,
    pub scroll: usize,
    back: Screen,
}

pub use crate::history::{Entry, Goto};
use crate::history::{note_change, way_said};

pub struct App {
    pub path: PathBuf,
    pub db: Db,
    /// The settings it runs with.
    pub cfg: crate::config::Config,
    pub stack: String,
    pub sel: usize,
    /// The left pane's cursor is on its header, the stack's place.
    pub on_stack: bool,
    pub focus: Focus,
    /// The patches are shown, on the left.
    pub list: bool,
    /// The comments are shown, on the right, when there are any.
    pub comments: bool,
    /// The comment selected in the comments pane, of `notes()`.
    pub note_sel: usize,
    /// The comments pane's cursor is on its header.
    pub notes_head: bool,
    pub layout: Layout,
    /// The compact layout's panes.
    pub compact: Compact,
    panes: Arc<Panes>,
    /// Reading rather than commenting: in the middle, Space marks the
    /// block of code at the cursor read, Enter takes the mark back.
    pub reading: bool,
    /// The patches' code blocks, for reading.
    finder: Finder,
    /// Each patch's changed lines, by id: what is read of it, counted.
    changed: HashMap<String, Arc<Vec<String>>>,
    /// How wide the middle was last drawn.
    mid_w: usize,
    pub screen: Screen,
    pub mode: Mode,
    pub rows: Vec<Row>,
    /// What `rows` were built from; None to build them again.
    built: Option<String>,
    /// What they show: the stack, or a patch. When it changes, the cursor
    /// goes back to the top.
    subject: Option<String>,
    /// Where the cursor goes once they are built again.
    want: Option<Want>,
    pub cursor: usize,
    pub top: usize,
    /// The selected line of a list screen, and its top.
    pub pick: usize,
    pick_top: usize,
    /// The record the history's cursor is on: it stays on it as records
    /// come, newest first, above it.
    hist_rev: Option<usize>,
    /// History…'s pane the cursor is in; the patch picked in the stack as
    /// it was; the cursor in that patch, and its top.
    pub hist_focus: HistFocus,
    pub hist_patch: usize,
    pub hist_cursor: usize,
    hist_top: usize,
    /// The rows of that patch as it was, and what they were built from.
    pub hist_rows: Vec<Row>,
    hist_built: Option<String>,
    /// The run a transcript is of, and how far it is scrolled up.
    transcript: Option<String>,
    scroll: usize,
    /// The patch whose versions are listed (its change), and whether a
    /// version is read whole rather than as what changed.
    versions: Option<String>,
    whole: bool,
    pub text: Option<TextView>,
    /// What the comments pane shows: the stack's filter, kept in the file.
    pub show: Show,
    /// A comment written and not yet saved, while what to do with it is
    /// asked.
    pending: Option<Edit>,
    /// An edit of a patch's code the stack did not take: its file, line and
    /// text, to edit again.
    refused: Option<(String, usize, String)>,
    /// What the last key did, when that needs saying: gone with the next.
    pub status: String,
    body_h: usize,
    /// The runs still going when the database was last read: when one
    /// lands, the status line says what came of it.
    going: Vec<String>,
    judged: Option<Judged>,
    /// The work done away from the screen.
    bg: Background,
    /// The comments the agent works on, as last read, and how many records
    /// the database had then.
    busy_memo: std::cell::RefCell<Option<(usize, std::rc::Rc<Busy>)>>,
}

/// What work done away from the screen came to.
enum Done {
    /// Asks started, and asks whose agent is gone landed: what went wrong.
    Launched(Option<String>),
    /// A pull or an apply (`what` it was): what came of it, said.
    Said(&'static str, String),
    /// Whether stack `name` at `rev` applies: why not, if not.
    Judged {
        name: String,
        rev: Option<usize>,
        why: Option<String>,
    },
}

/// The work done away from the screen, each in a thread of its own, so the
/// screen never waits on it: starting asks (a worktree to make), landing
/// those whose agent is gone, pulls, applies, checking a stack someone
/// else made. What it came to is read when the screen is next drawn.
struct Background {
    tx: std::sync::mpsc::Sender<Done>,
    rx: std::sync::mpsc::Receiver<Done>,
    /// Asks are being started; and to look again when that is done.
    launching: bool,
    again: bool,
    /// What is being done that the bar says: pulling, applying.
    doing: Vec<&'static str>,
    /// The stack being checked, and its rev.
    judging: Option<(String, Option<usize>)>,
}

impl Background {
    fn new() -> Background {
        let (tx, rx) = std::sync::mpsc::channel();
        Background {
            tx,
            rx,
            launching: false,
            again: false,
            doing: vec![],
            judging: None,
        }
    }
}

/// Why stack checking said no, as the bar says it.
fn why_not(e: crate::db::Error) -> String {
    let why = e.0.trim_end_matches("; nothing changed").to_string();
    match why.split_once(" is read-only: ") {
        Some((_, w)) => format!("read-only: {w}"),
        None => format!("does not apply: {why}"),
    }
}

/// The stack last checked, and why it cannot be changed or does not apply,
/// if so.
struct Judged {
    name: String,
    rev: Option<usize>,
    why: Option<String>,
}

/// The runs not landed yet.
fn going(db: &Db) -> Vec<String> {
    db.runs()
        .iter()
        .filter(|r| r.state == RunState::Started)
        .map(|r| r.id.clone())
        .collect()
}

impl App {
    /// The editor on the database at `path`, with the settings in use.
    pub fn new(path: &Path) -> Result<App> {
        App::with_config(path, crate::config::get().clone())
    }

    /// The editor on the database at `path`, with settings `cfg`.
    pub fn with_config(path: &Path, cfg: crate::config::Config) -> Result<App> {
        let db = Db::load(path)?;
        let stack = db
            .stacks()
            .iter()
            .max_by_key(|s| db.head(&s.name).map(|(r, _)| r))
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let status = match (crate::config::warnings(), db.warnings.len()) {
            ([w, ..], _) => w.clone(),
            ([], 0) => String::new(),
            ([], n) => format!("{n} lines of the database passed over: `stacked check` lists them"),
        };
        let layout = match cfg.view.layout {
            crate::config::Layout::Unified => Layout::Unified,
            crate::config::Layout::Split => Layout::Split,
            crate::config::Layout::Compact => Layout::Compact,
        };
        let show = db.view(&stack).and_then(Show::parse).unwrap_or(Show::All);
        let mut app = App {
            path: path.to_path_buf(),
            stack,
            sel: 0,
            on_stack: false,
            focus: Focus::List,
            list: cfg.view.patches,
            comments: cfg.view.comments,
            note_sel: 0,
            notes_head: false,
            layout,
            compact: Compact::default(),
            panes: Arc::new(Panes::default()),
            reading: cfg.view.reading,
            finder: Finder::default(),
            changed: HashMap::new(),
            mid_w: 0,
            screen: Screen::Main,
            mode: Mode::Normal,
            rows: vec![],
            built: None,
            subject: None,
            want: None,
            cursor: 0,
            top: 0,
            pick: 0,
            pick_top: 0,
            hist_rev: None,
            hist_focus: HistFocus::List,
            hist_patch: 0,
            hist_cursor: 0,
            hist_top: 0,
            hist_rows: vec![],
            hist_built: None,
            transcript: None,
            scroll: 0,
            versions: None,
            whole: false,
            text: None,
            show,
            pending: None,
            refused: None,
            status,
            body_h: 20,
            going: going(&db),
            judged: None,
            bg: Background::new(),
            busy_memo: std::cell::RefCell::new(None),
            db,
            cfg,
        };
        if app.patch_count() == 0 {
            app.on_stack = true;
        }
        app.judge();
        Ok(app)
    }

    /// Checks the stack the current name is on, once per stack: why it
    /// cannot be changed, or why it does not apply, if it cannot or does not.
    /// The first is checked before the screen is drawn; any after, away
    /// from it (a stack the agent made takes a moment to check).
    fn judge(&mut self) {
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        let name = self.stack.clone();
        if self.judged.as_ref().is_some_and(|j| j.name == name && j.rev == rev) {
            return;
        }
        let Some((_, s)) = self.db.head(&name) else {
            self.judged = Some(Judged { name, rev, why: None });
            return;
        };
        let s = s.clone();
        let patches: Vec<Patch> = s.patches.iter().map(|id| self.db.patch(id).clone()).collect();
        let check = move || crate::check::stack(&s, &patches.iter().collect::<Vec<_>>()).err().map(why_not);
        if self.judged.is_none() {
            self.judged = Some(Judged { name, rev, why: check() });
            return;
        }
        if self.bg.judging.as_ref().is_some_and(|(n, r)| *n == name && *r == rev) {
            return;
        }
        self.bg.judging = Some((name.clone(), rev));
        self.spawn(move || Done::Judged { name, rev, why: check() });
    }

    /// Why the current stack cannot be changed, if it cannot.
    pub fn read_only(&self) -> Option<&str> {
        self.judged
            .as_ref()
            .filter(|j| j.name == self.stack)
            .and_then(|j| j.why.as_deref())
            .filter(|why| why.starts_with("read-only"))
    }

    /// Runs `work` in a thread of its own; what it comes to is read when
    /// the screen is next drawn.
    fn spawn(&self, work: impl FnOnce() -> Done + Send + 'static) {
        let tx = self.bg.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
    }

    /// Starts what asks may go, and lands those whose agent is gone, away
    /// from the screen.
    pub fn launch(&mut self) {
        if self.bg.launching {
            self.bg.again = true;
            return;
        }
        self.bg.launching = true;
        let path = self.path.clone();
        self.spawn(move || {
            if let Ok(db) = Db::load(&path) {
                ask::land_orphans(&path, &db);
            }
            Done::Launched(ask::launch(&path).err().map(|e| e.0))
        });
    }

    /// What some work away from the screen came to.
    fn done(&mut self, d: Done) {
        match d {
            Done::Launched(e) => {
                self.bg.launching = false;
                if let Some(e) = e {
                    self.status = e;
                }
                if std::mem::take(&mut self.bg.again) {
                    self.launch();
                }
            }
            Done::Said(what, said) => {
                self.bg.doing.retain(|d| *d != what);
                self.status = said;
            }
            Done::Judged { name, rev, why } => {
                if self.bg.judging.as_ref().is_some_and(|(n, r)| *n == name && *r == rev) {
                    self.bg.judging = None;
                }
                self.judged = Some(Judged { name, rev, why });
            }
        }
    }

    /// Reads what the work away from the screen came to; whether it came
    /// to anything.
    fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(d) = self.bg.rx.try_recv() {
            self.done(d);
            any = true;
        }
        any
    }

    /// Whether work is being done away from the screen: starting asks,
    /// pulling, applying, checking.
    pub fn working(&self) -> bool {
        self.bg.launching || !self.bg.doing.is_empty() || self.bg.judging.is_some()
    }

    /// Waits for the work away from the screen to be done.
    pub fn settle(&mut self) {
        while self.bg.launching || !self.bg.doing.is_empty() || self.bg.judging.is_some() {
            match self.bg.rx.recv_timeout(Duration::from_secs(60)) {
                Ok(d) => self.done(d),
                Err(_) => break,
            }
        }
        self.reload();
    }

    /// The comments the agent is working on: read again only when the
    /// database has.
    fn busy(&self) -> std::rc::Rc<Busy> {
        let key = self.db.records.len();
        if let Some((k, b)) = &*self.busy_memo.borrow()
            && *k == key
        {
            return b.clone();
        }
        let b = std::rc::Rc::new(ask::with_agent(&self.db));
        *self.busy_memo.borrow_mut() = Some((key, b.clone()));
        b
    }

    pub fn current(&self) -> Option<&Stack> {
        self.db.stack(&self.stack).ok()
    }

    fn patch_id(&self) -> Option<String> {
        self.current()?.patches.get(self.sel).cloned()
    }

    fn patch_count(&self) -> usize {
        self.current().map_or(0, |s| s.patches.len())
    }

    fn patch_title(&self) -> String {
        self.patch_id()
            .map(|id| self.db.patch(&id).title().to_string())
            .unwrap_or_default()
    }

    /// The change the middle pane's comments are on: the stack's own when
    /// the cursor is on the stack, else the selected patch's.
    pub fn subject_change(&self) -> Option<String> {
        if self.on_stack {
            self.current().map(|_| stack_change(&self.stack))
        } else {
            self.patch_id().map(|id| self.db.patch(&id).change.clone())
        }
    }

    /// Every comment the middle pane could show, before the filter: the
    /// stack's own oldest first, or the patch's.
    fn subject_notes(&self) -> Vec<Note> {
        let Some(change) = self.subject_change() else { return vec![] };
        let mut notes: Vec<Note> = self.db.notes(&change).into_iter().cloned().collect();
        if self.on_stack {
            notes.sort_by_key(|n| self.db.note_history(&n.change, &n.id).first().map_or(0, |r| r.0));
        }
        notes
    }

    /// Whether `n` is on a line its patch's version in the stack lacks.
    fn gone(&self, n: &Note, lines: &HashSet<String>) -> bool {
        !n.on_stack() && !lines.contains(&n.line)
    }

    fn line_keys(&self) -> HashSet<String> {
        match (self.on_stack, self.patch_id()) {
            (false, Some(id)) => note_lines(self.db.patch(&id)).into_iter().map(|l| l.key).collect(),
            _ => HashSet::new(),
        }
    }

    /// Whether the filter `show` lets `n` be shown.
    fn shows(&self, show: Show, n: &Note, busy: &Busy, gone: bool) -> bool {
        shows(show, n, busy, gone)
    }

    /// The comments the filter lets the middle pane show.
    fn shown_notes(&self) -> Vec<Note> {
        let busy = self.busy();
        let lines = self.line_keys();
        self.subject_notes()
            .into_iter()
            .filter(|n| self.shows(self.show, n, &busy, self.gone(n, &lines)))
            .collect()
    }

    /// How many of the middle pane's comments each filter would show.
    fn show_counts(&self) -> Vec<(Show, usize)> {
        let busy = self.busy();
        let lines = self.line_keys();
        let notes = self.subject_notes();
        Show::ALL
            .into_iter()
            .map(|s| (s, notes.iter().filter(|n| self.shows(s, n, &busy, self.gone(n, &lines))).count()))
            .collect()
    }

    fn where_was(&self, m: &crate::db::Moved) -> String {
        where_was(&self.db, &self.stack, m)
    }

    fn tag(&self, n: &Note, busy: &Busy) -> String {
        note_state(&self.db, &self.stack, n, busy)
    }

    /// The tags and streams of `notes`.
    fn deco(&self, notes: &[Note]) -> Deco {
        let busy = self.busy();
        let mut deco = Deco::default();
        for n in notes {
            let tag = self.tag(n, &busy);
            if !tag.is_empty() {
                deco.tags.insert(n.id.clone(), tag);
            }
            if let Some((_, run)) = busy.get(&(n.change.clone(), n.id.clone())) {
                let lines = match &self.mode {
                    Mode::Stream { change, id, up } if *change == n.change && *id == n.id => {
                        let all = ask::tail(&self.db, run, usize::MAX);
                        let room = (self.body_h / 2).max(5);
                        let end = all.len().saturating_sub(*up);
                        all[end.saturating_sub(room)..end].to_vec()
                    }
                    _ => {
                        let lines = ask::tail(&self.db, run, self.cfg.view.stream_lines);
                        if lines.is_empty() { vec!["the agent is starting…".into()] } else { lines }
                    }
                };
                deco.streams.insert(n.id.clone(), lines);
            }
        }
        deco
    }

    fn rebuild(&mut self, width: usize) {
        let notes = self.shown_notes();
        let stream = match &self.mode {
            Mode::Stream { id, up, .. } => format!("{id}:{up}"),
            _ => String::new(),
        };
        let subject = if self.on_stack {
            format!("stack:{}", self.stack)
        } else {
            self.patch_id().unwrap_or_default()
        };
        // Built again when what they show changed: a record that is not a
        // line of an agent's stream, or the stream of a comment shown, not
        // each line any agent writes.
        let busy = self.busy();
        let streams: String = notes
            .iter()
            .filter_map(|n| busy.get(&(n.change.clone(), n.id.clone())))
            .map(|(_, run)| format!("{}.", self.db.event_count(run)))
            .collect();
        let want = format!(
            "{subject}\0{width}\0{}\0{streams}\0{:?}\0{stream}\0{}\0{}",
            self.db.changes(),
            self.show,
            self.body_h,
            self.reading
        );
        if self.built.as_deref() == Some(&want) {
            return;
        }
        let same = self.subject.as_deref() == Some(subject.as_str());
        let refs: Vec<&Note> = notes.iter().collect();
        let mut deco = self.deco(&notes);
        if self.reading && !self.on_stack {
            deco.read = self.subject_change().map(|c| self.db.read_keys(&c));
        }
        self.rows = if self.on_stack {
            match self.current() {
                Some(s) => {
                    let base = s.base.as_deref().map_or(String::new(), |b| format!(" on {}", &b[..12.min(b.len())]));
                    let about = format!("the stack {} · {} patch{}{base}", self.stack, s.patches.len(), if s.patches.len() == 1 { "" } else { "es" });
                    let titles: Vec<String> = s.patches.iter().map(|id| self.db.patch(id).title().to_string()).collect();
                    stack_rows(&about, &titles, &refs, width, &deco)
                }
                None => vec![],
            }
        } else {
            match self.patch_id() {
                Some(id) => patch_rows(self.db.patch(&id), &refs, width, &deco),
                None => vec![],
            }
        };
        self.built = Some(want);
        self.subject = Some(subject);
        self.panes = Arc::new(pane_streams(&self.rows));
        if !same {
            self.cursor = 0;
            self.top = 0;
            self.note_sel = 0;
            self.compact.reset();
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        self.note_sel = self.note_sel.min(self.notes().len().saturating_sub(1));
        if let Some(w) = self.want.take() {
            let at = match &w {
                Want::Note(id) => self.rows.iter().position(|r| r.head && r.note.as_deref() == Some(id)),
                Want::Line(key) => self.rows.iter().position(|r| r.key.as_deref() == Some(key) && r.note.is_none()),
            };
            match at {
                Some(i) => {
                    self.cursor = i;
                    self.top = i.saturating_sub(self.body_h / 3);
                    self.compact.cursor = None;
                }
                // Not shown as filtered: all of them are, for it.
                None if matches!(w, Want::Note(_)) && self.show != Show::All => {
                    self.show = Show::All;
                    self.status = "showing all the comments, for that one".into();
                    self.want = Some(w);
                    self.built = None;
                    self.rebuild(width);
                }
                None => {}
            }
        }
    }

    /// Whether the comments pane is there: there are comments shown, and the
    /// pane was not put away.
    pub fn notes_shown(&self) -> bool {
        self.screen == Screen::Main && self.comments && !self.subject_notes().is_empty()
    }

    /// The comments the middle pane shows, in its order: each its first row
    /// and its id.
    pub fn notes(&self) -> Vec<(usize, String)> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.head)
            .filter_map(|(i, r)| Some((i, r.note.clone()?)))
            .collect()
    }

    /// The comment the middle pane's cursor is in, of `notes()`.
    fn note_at_cursor(&self) -> Option<usize> {
        let id = self.rows.get(self.cursor)?.note.as_ref()?;
        self.notes().iter().position(|(_, n)| n == id)
    }

    /// Goes to the selected comment in the middle pane: its first row a
    /// third of the way down, the cursor on it.
    fn go_to_note(&mut self) {
        if let Some(&(row, _)) = self.notes().get(self.note_sel) {
            self.cursor = row;
            self.top = row.saturating_sub(self.body_h / 3);
            self.focus = Focus::Diff;
        }
    }

    /// The row what is done is done to: the selected comment's first row in
    /// the comments pane, else the middle pane's cursor.
    fn target_row(&self) -> usize {
        match self.focus {
            Focus::Notes => self.notes().get(self.note_sel).map_or(self.cursor, |n| n.0),
            _ => self.cursor,
        }
    }

    /// The comment what is done is done to, if the cursor is on one.
    pub fn picked(&self) -> Option<Note> {
        if self.focus == Focus::List || (self.focus == Focus::Notes && self.notes_head) {
            return None;
        }
        let change = self.subject_change()?;
        let id = self.rows.get(self.target_row())?.note.clone()?;
        self.db.note(&change, &id).cloned()
    }

    /// The line the middle pane's cursor is on, if a comment can go on it:
    /// (change, key, file, preview).
    fn commentable(&self) -> Option<(String, String, String, String)> {
        if self.on_stack || self.focus != Focus::Diff {
            return None;
        }
        let row = self.rows.get(self.cursor)?;
        if row.note.is_some() || !row.kind.commentable() {
            return None;
        }
        let change = self.subject_change()?;
        Some((
            change,
            row.key.clone()?,
            row.path.clone().unwrap_or_default(),
            row.preview.clone().unwrap_or_default(),
        ))
    }

    /// Reads what someone appended to the database, if anything, and what
    /// the work away from the screen came to.
    pub fn refresh(&mut self) {
        let polled = self.poll();
        let len = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if len != self.db.bytes_read() || polled {
            self.reload();
        }
    }

    /// Reads what was appended: what an edit just wrote, or another
    /// process.
    fn reload(&mut self) {
        if let Err(e) = self.db.catch_up() {
            self.status = e.0;
        }
        for id in std::mem::take(&mut self.going) {
            if let Some(r) = self.db.run(&id)
                && r.state != RunState::Started
            {
                let state = format!("{:?}", r.state).to_lowercase();
                self.status = format!("the agent's {} {state}: {}", r.what, r.summary);
            }
        }
        self.going = going(&self.db);
        self.clamp();
        self.judge();
    }

    fn clamp(&mut self) {
        let n = self.patch_count();
        self.sel = self.sel.min(n.saturating_sub(1));
        if n == 0 {
            self.on_stack = true;
            self.list = true;
            if self.focus == Focus::List {
                self.on_stack = true;
            }
        }
        if self.focus == Focus::Notes && !self.notes_shown() {
            self.focus = Focus::Diff;
        }
    }

    /// Runs an edit under the database's lock, then reads what it appended.
    fn edit<T>(&mut self, f: impl FnOnce(&mut Db) -> Result<T>) -> Option<T> {
        let out = Db::edit(&self.path, f);
        self.reload();
        match out {
            Ok(v) => Some(v),
            Err(e) => {
                self.status = e.0;
                None
            }
        }
    }

    fn running(&self) -> usize {
        self.db.runs().iter().filter(|r| ask::running(r)).count()
    }

    fn ready(&self) -> usize {
        self.db.stack_notes(&self.stack).iter().filter(|n| n.queued.is_some()).count()
    }

    // -- keys ------------------------------------------------------------- //

    /// Handles a key; what the loop must do for it.
    pub fn key(&mut self, k: KeyEvent) -> Effect {
        if k.kind == KeyEventKind::Release {
            return Effect::None;
        }
        // The keys as the settings map them, but in the comment editor,
        // which reads what is typed. A key that does nothing still closes
        // what `?` said.
        let k = if matches!(self.mode, Mode::Edit(_)) {
            k
        } else {
            match self.cfg.keys.translate(&k) {
                Some(t) => t,
                None => {
                    if let Mode::Help { back, .. } = &self.mode {
                        self.mode = (**back).clone();
                    }
                    return Effect::None;
                }
            }
        };
        // ctrl-c is the terminal's, as in every program: out.
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::Quit;
        }
        // What can be done here, said: the next key goes back, and does
        // what it does there.
        if let Mode::Help { back, .. } = &self.mode {
            self.mode = (**back).clone();
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('?')) {
                return Effect::None;
            }
        } else if k.code == KeyCode::Char('?') && !matches!(self.mode, Mode::Edit(_)) {
            self.mode = Mode::Help {
                lines: self.help(),
                back: Box::new(self.mode.clone()),
            };
            return Effect::None;
        }
        self.status.clear();
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal | Mode::Help { .. } => {}
            Mode::Menu(m) => return self.menu_key(m, k),
            Mode::Edit(e) => return self.edit_key(e, k),
            Mode::Moving { from, to } => {
                self.moving_key(from, to, k);
                return Effect::None;
            }
            Mode::Stream { change, id, up } => {
                self.stream_key(change, id, up, k);
                return Effect::None;
            }
        }
        // self-review's two keys: 1 for another view of the code, 2 to read
        // rather than comment, or comment rather than read.
        if self.screen == Screen::Main {
            match k.code {
                KeyCode::Char('1') => {
                    self.cycle_layout();
                    return Effect::None;
                }
                KeyCode::Char('2') => {
                    self.toggle_reading();
                    return Effect::None;
                }
                KeyCode::Char(' ') if self.reading_here() => return self.perform(Action::MarkRead),
                _ => {}
            }
        }
        if k.code == KeyCode::Char(' ') {
            self.mode = Mode::Menu(self.main_menu());
            return Effect::None;
        }
        match self.screen {
            Screen::Main => self.main_key(k),
            Screen::History => {
                self.history_key(k);
                Effect::None
            }
            Screen::Transcript => {
                self.transcript_key(k);
                Effect::None
            }
            Screen::Text => {
                self.text_key(k);
                Effect::None
            }
            _ => {
                self.list_key(k);
                Effect::None
            }
        }
    }

    fn page(&self) -> usize {
        self.body_h.saturating_sub(1).max(1)
    }

    fn main_key(&mut self, k: KeyEvent) -> Effect {
        if self.focus == Focus::Diff
            && self.compact_drawn()
            && let Some(e) = self.compact_key(k)
        {
            return e;
        }
        let n = self.patch_count();
        let last = n.saturating_sub(1);
        match (self.focus, k.code) {
            (_, KeyCode::Enter) => {
                return match self.enter() {
                    Some(action) => self.perform(action),
                    None => {
                        self.mode = Mode::Menu(self.main_menu());
                        Effect::None
                    }
                };
            }
            (Focus::List, KeyCode::Down) if self.on_stack => {
                if n > 0 {
                    self.on_stack = false;
                    self.sel = 0;
                }
            }
            (Focus::List, KeyCode::Down) => self.sel = (self.sel + 1).min(last),
            (Focus::List, KeyCode::Up) if self.sel == 0 => self.on_stack = true,
            (Focus::List, KeyCode::Up) => self.sel = self.sel.saturating_sub(1),
            (Focus::List, KeyCode::PageDown) if n > 0 => {
                self.sel = if self.on_stack { 0 } else { (self.sel + self.page()).min(last) };
                self.on_stack = false;
            }
            (Focus::List, KeyCode::PageUp) => {
                if self.sel == 0 {
                    self.on_stack = true;
                }
                self.sel = self.sel.saturating_sub(self.page());
            }
            (Focus::List, KeyCode::Home) => self.on_stack = true,
            (Focus::List, KeyCode::End) if n > 0 => {
                self.on_stack = false;
                self.sel = last;
            }
            (Focus::List, KeyCode::Right) if self.current().is_some() => self.focus = Focus::Diff,
            // Further left than the patches: they make way for the middle.
            (Focus::List, KeyCode::Left) if self.current().is_some() => {
                self.list = false;
                self.focus = Focus::Diff;
            }
            (Focus::Diff, KeyCode::Left | KeyCode::Esc) => {
                self.list = true;
                self.focus = Focus::List;
            }
            (Focus::Diff, KeyCode::Right) if !self.subject_notes().is_empty() => {
                self.comments = true;
                // The comment the cursor is in, else the next one down.
                let at = self.note_at_cursor().unwrap_or_else(|| {
                    let notes = self.notes();
                    notes.iter().position(|(row, _)| *row >= self.cursor).unwrap_or(notes.len().saturating_sub(1))
                });
                self.focus = Focus::Notes;
                self.notes_head = self.notes().is_empty();
                self.note_sel = at;
            }
            (Focus::Diff, code) => {
                let last = self.rows.len().saturating_sub(1);
                self.cursor = match code {
                    KeyCode::Down => (self.cursor + 1).min(last),
                    KeyCode::Up => self.cursor.saturating_sub(1),
                    KeyCode::PageDown => (self.cursor + self.page()).min(last),
                    KeyCode::PageUp => self.cursor.saturating_sub(self.page()),
                    KeyCode::Home => 0,
                    KeyCode::End => last,
                    _ => self.cursor,
                };
            }
            (Focus::Notes, KeyCode::Left | KeyCode::Esc) => self.focus = Focus::Diff,
            // Further right than the comments: they make way for the middle,
            // which is where it was.
            (Focus::Notes, KeyCode::Right) => {
                self.comments = false;
                self.focus = Focus::Diff;
            }
            (Focus::Notes, KeyCode::Up) if self.note_sel == 0 => self.notes_head = true,
            (Focus::Notes, KeyCode::Down) if self.notes_head => {
                if !self.notes().is_empty() {
                    self.notes_head = false;
                    self.note_sel = 0;
                }
            }
            (Focus::Notes, KeyCode::Home) => self.notes_head = true,
            (Focus::Notes, code) => {
                let last = self.notes().len().saturating_sub(1);
                self.note_sel = match code {
                    KeyCode::Down => (self.note_sel + 1).min(last),
                    KeyCode::Up => self.note_sel.saturating_sub(1),
                    KeyCode::PageDown => (self.note_sel + self.page() / 2).min(last),
                    KeyCode::PageUp => self.note_sel.saturating_sub(self.page() / 2),
                    KeyCode::End => last,
                    _ => self.note_sel,
                };
            }
            _ => {}
        }
        Effect::None
    }

    /// The arrows in the compact layout: ↑↓ move down the panes, ← → turn
    /// which of them scrolls, the old side, both, or the new side, and past
    /// those go on to the pane beyond.
    fn compact_key(&mut self, k: KeyEvent) -> Option<Effect> {
        let (p, h, page) = (self.panes.clone(), self.body_h, self.page() as isize);
        match k.code {
            KeyCode::Down => self.compact.step(&p, 1, h),
            KeyCode::Up => self.compact.step(&p, -1, h),
            KeyCode::PageDown => self.compact.step(&p, page, h),
            KeyCode::PageUp => self.compact.step(&p, -page, h),
            KeyCode::Home => self.compact.to_start(),
            KeyCode::End => self.compact.to_end(&p, h),
            KeyCode::Left | KeyCode::Right => {
                if !self.compact.turn(k.code == KeyCode::Right) {
                    return None;
                }
                self.status = self.compact.scroll.name().into();
            }
            _ => return None,
        }
        if let Some(row) = self.compact.settle(&p, h) {
            self.cursor = row;
        }
        Some(Effect::None)
    }

    /// The layout the middle is drawn in, `w` wide: a stack has no sides.
    fn drawn_layout(&self, w: usize) -> Layout {
        if self.on_stack || (self.layout != Layout::Unified && w < self.cfg.view.split_width) {
            Layout::Unified
        } else {
            self.layout
        }
    }

    fn compact_drawn(&self) -> bool {
        self.drawn_layout(self.mid_w) == Layout::Compact
    }

    /// 1: unified, side-by-side, compact, in turn; two sides when the
    /// middle is wide enough for them.
    fn cycle_layout(&mut self) {
        let next = self.layout.next();
        let need = self.cfg.view.split_width;
        if next != Layout::Unified && self.mid_w > 0 && self.mid_w < need {
            self.status = self.say_keys(format!(
                "side-by-side needs {need} columns: ← on the patches hides them, → on the comments hides them"
            ));
            return;
        }
        self.layout = next;
        if next == Layout::Compact {
            self.compact.scroll = Scroll::Both;
            self.compact.cursor = None;
        }
        self.built = None;
        self.status = format!("{} view", next.label());
    }

    /// 2: reading, or commenting.
    fn toggle_reading(&mut self) {
        self.reading = !self.reading;
        self.built = None;
        self.status = if self.reading {
            "reading: in the code, Space marks a block as read, Enter as unread".into()
        } else {
            "commenting: in the code, Enter comments, Space is the menu".into()
        };
    }

    /// Whether the reading mode has the middle's keys: a patch is there,
    /// and the cursor is in it.
    pub fn reading_here(&self) -> bool {
        self.reading && self.screen == Screen::Main && self.focus == Focus::Diff && !self.on_stack && self.patch_id().is_some()
    }

    /// The current patch's key for its blocks.
    fn blocks_key(&self) -> Option<String> {
        let s = self.current()?;
        Some(crate::reading::key(s.base.as_deref(), s.patches.get(..=self.sel)?))
    }

    /// The current patch's code blocks, once found; they are looked for
    /// meanwhile.
    fn blocks(&mut self) -> Option<Arc<Blocks>> {
        let key = self.blocks_key()?;
        if let Some(b) = self.finder.peek(&key) {
            return Some(b);
        }
        let s = self.current()?.clone();
        let job = match (&s.repo, &s.base) {
            (Some(repo), Some(base)) => Some((
                PathBuf::from(repo),
                base.clone(),
                s.patches[..=self.sel].iter().map(|id| self.db.patch(id).clone()).collect(),
            )),
            _ => None,
        };
        self.finder.get(&key, || job)
    }

    /// Waits for the code blocks being looked for.
    pub fn wait_blocks(&mut self) {
        self.blocks();
        self.finder.wait();
    }

    /// What Space and Enter would do at the cursor, in the reading mode:
    /// None while the patch's blocks are looked for.
    #[allow(clippy::type_complexity)]
    fn reading_at(&self) -> Option<(Option<Block>, Option<(usize, HashSet<String>)>)> {
        let blocks = self.finder.peek(&self.blocks_key()?)?;
        let change = self.subject_change()?;
        let read = self.db.read_keys(&change);
        let marks = self.db.marks(&change);
        let r = Reading {
            rows: &self.rows,
            blocks: &blocks,
            read: &read,
            marks: &marks,
        };
        Some((r.to_mark(self.cursor), r.to_unmark(self.cursor)))
    }

    /// Space, reading: marks the block at the cursor read, then goes on to
    /// the next line not read; with all read there, goes on.
    fn mark_read(&mut self) {
        let Some(change) = self.subject_change() else { return };
        if self.blocks().is_none() {
            self.status = "still finding this patch's code blocks".into();
            return;
        }
        let at = self.cursor;
        let next = |app: &App| {
            let blocks = app.finder.peek(&app.blocks_key()?)?;
            let read = app.db.read_keys(&change);
            let r = Reading {
                rows: &app.rows,
                blocks: &blocks,
                read: &read,
                marks: &[],
            };
            r.next_unread(at, true)
        };
        match self.reading_at().and_then(|(mark, _)| mark) {
            Some(t) => {
                let n = t.new.len();
                let keys: Vec<String> = t.keys.into_iter().collect();
                if self.edit(|db| db.mark_read(&change, keys)).is_none() {
                    return;
                }
                self.rebuild_now();
                self.status = format!("read: {}, {n} line{}", t.what, plural(n));
                match next(self) {
                    Some(j) => self.cursor = j,
                    None => self.status += " · nothing left to read below",
                }
            }
            None => match next(self) {
                Some(j) => {
                    self.cursor = j;
                    self.status = "all read there: on to the next line not read".into();
                }
                None => self.status = "nothing left to read below".into(),
            },
        }
    }

    /// Enter, reading: takes back the latest mark around the cursor.
    fn mark_unread(&mut self) {
        let Some(change) = self.subject_change() else { return };
        if self.blocks().is_none() {
            self.status = "still finding this patch's code blocks".into();
            return;
        }
        match self.reading_at().and_then(|(_, unmark)| unmark) {
            Some((rev, gone)) => {
                if self.edit(|db| db.take_back(&change, rev)).is_some() {
                    let n = gone.len();
                    self.status = format!("unread: {n} line{}", plural(n));
                }
            }
            None => self.status = "nothing here is marked read".into(),
        }
    }

    /// The rows, built again now rather than when next drawn.
    fn rebuild_now(&mut self) {
        let w = self.built.as_deref().and_then(|b| b.split('\0').nth(1)?.parse().ok()).unwrap_or(80);
        self.built = None;
        self.rebuild(w);
    }

    /// The changed lines of patch `id`.
    fn changed_lines(&mut self, id: &str) -> Arc<Vec<String>> {
        if let Some(c) = self.changed.get(id) {
            return c.clone();
        }
        let p = self.db.patch(id);
        let keys: Vec<String> = crate::diff::parse_diff(&p.diff, &crate::view::mode(&p.change))
            .iter()
            .flat_map(|f| f.hunks.iter())
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.origin != ' ')
            .map(|l| l.key.clone())
            .collect();
        let keys = Arc::new(keys);
        self.changed.insert(id.into(), keys.clone());
        keys
    }

    /// How many of patch `id`'s changed lines are read, of how many.
    fn read_of(&mut self, id: &str) -> (usize, usize) {
        let keys = self.changed_lines(id);
        let read = self.db.read_keys(&self.db.patch(id).change.clone());
        (keys.iter().filter(|k| read.contains(*k)).count(), keys.len())
    }

    /// The file and line (from 1) of the code the middle's cursor is on, as
    /// the patch leaves it: a line of a diff, a hunk's header, a file's.
    fn code_place(&self) -> Option<(String, usize)> {
        if self.on_stack {
            return None;
        }
        let row = self.rows.get(self.cursor)?;
        let file = row.path.clone().filter(|p| p != MSG_PATH)?;
        Some((file, row.new_no?))
    }

    /// The line (from 1) of the patch's mail the middle's cursor is on.
    fn mail_place(&self) -> Option<usize> {
        if self.on_stack {
            return None;
        }
        let p = self.db.patch(&self.patch_id()?);
        let row = self.rows.get(self.cursor)?;
        (row.msg_at.is_some() || row.diff_at.is_some()).then(|| ops::mail_line(p, row))
    }

    /// What enter does where the cursor is, when that is not the menu.
    pub fn enter(&self) -> Option<Action> {
        if self.screen != Screen::Main {
            return None;
        }
        match self.focus {
            Focus::List if self.on_stack => None,
            Focus::List => (self.patch_count() > 0).then_some(Action::Open),
            Focus::Notes if self.notes_head => None,
            Focus::Notes => (!self.notes().is_empty()).then_some(Action::GoToNote),
            Focus::Diff if self.reading_here() => Some(Action::MarkUnread),
            Focus::Diff => {
                if let Some(n) = self.picked() {
                    let busy = self.busy();
                    return Some(if busy.contains_key(&(n.change.clone(), n.id.clone())) {
                        Action::OpenStream
                    } else {
                        Action::Reply
                    });
                }
                self.commentable().map(|_| Action::CommentLine)
            }
        }
    }

    /// What can be done where the cursor is, said: what enter does first,
    /// then the menu, then where the arrows go.
    pub fn help(&self) -> Vec<String> {
        let mut out = Vec::new();
        match &self.mode {
            Mode::Menu(m) => {
                if let Some(item) = m.chosen() {
                    out.push(format!("Press Enter to {}.", about(&item.action)));
                }
                out.push("Press ↑↓ for another item, Esc to close the menu.".into());
                return out;
            }
            Mode::Moving { to, .. } => {
                out.push(format!("Press Enter to put {} there, at {}.", self.patch_title(), to + 1));
                out.push("Press ↑↓ to place it elsewhere, Esc to leave it where it was.".into());
                return out;
            }
            Mode::Stream { .. } => {
                out.push("Press ↑↓ (pgup pgdn home end) to scroll what the agent wrote, Esc to fold it.".into());
                return out;
            }
            _ => {}
        }
        match self.screen {
            Screen::Main if self.reading_here() => {
                match self.reading_at() {
                    None => out.push(
                        "Press Space to mark this block as read, once this patch's code blocks are found: they are being looked for."
                            .into(),
                    ),
                    Some((mark, unmark)) => {
                        out.push(match mark {
                            Some(t) => format!(
                                "Press Space to mark this block as read: {}, {} line{} not read yet.",
                                t.what,
                                t.new.len(),
                                plural(t.new.len())
                            ),
                            None => "Press Space to go on to the next line not read: all is read here.".into(),
                        });
                        out.push(match unmark {
                            Some((_, gone)) => format!(
                                "Press Enter to mark this block as unread: the latest mark here, {} line{}.",
                                gone.len(),
                                plural(gone.len())
                            ),
                            None => "Enter marks a block as unread: nothing here is marked read.".into(),
                        });
                    }
                }
                out.push("Press 2 to go back to commenting, where Enter comments and Space is the menu; 1 for another view of the code.".into());
                out.push(self.arrows_help());
            }
            Screen::Main => {
                match self.enter() {
                    Some(a) => out.push(format!("Press Enter to {}.", about(&a))),
                    None if self.current().is_none() => {
                        out.push("There is no stack here: `stacked import NAME main..HEAD` makes one.".into())
                    }
                    None => {}
                }
                let here = match self.focus {
                    Focus::List if self.on_stack => "the stack: its comments, sending them, and the stack's own menu",
                    Focus::Notes if self.notes_head => "the comments listed: sending them all, and what is listed",
                    _ => "here",
                };
                out.push(format!("Press Space for all that can be done {here}."));
                match self.focus {
                    Focus::List => out.push(
                        "Press ↑↓ for another patch, or the stack at the top; → to read it; ← to hide the patches."
                            .into(),
                    ),
                    Focus::Diff => out.push(self.arrows_help()),
                    Focus::Notes => out.push(
                        "Press ↑↓ for another comment, or the list at the top (the middle stays where it is, and shows it at its top or bottom); ← to go back; → to hide the comments."
                            .into(),
                    ),
                }
                let next = self.layout.next().label();
                out.push(if self.reading {
                    format!("Press 1 for the {next} view of the code; 2 to comment rather than read.")
                } else {
                    format!("Press 1 for the {next} view of the code; 2 to read: to mark the code read, a block at a time.")
                });
                if let Some(why) = self.read_only() {
                    out.push(format!("This stack is {why}. Comments can still be added."));
                }
            }
            Screen::Tree => {
                if let Some(l) = tree::graph(&self.db, &self.stack).get(self.pick) {
                    let on = self.db.head(&self.stack).map(|(r, _)| r) == Some(l.rev);
                    if on {
                        out.push(format!("@{} is the stack you are editing.", l.rev));
                    } else {
                        let op = self.db.node(l.rev).map(|s| s.op.clone()).unwrap_or_default();
                        out.push(format!("Press Enter to edit from @{}: {op}.", l.rev));
                    }
                }
                out.push("Press Space for what this edit changed, and more.".into());
                out.push("Press ↑↓ for another stack of the tree, Esc to go back.".into());
            }
            Screen::Stacks => {
                if let Some(s) = self.db.stacks().get(self.pick) {
                    out.push(format!("Press Enter to switch to {}.", s.name));
                }
                out.push("Press ↑↓ for another stack, Esc to go back.".into());
            }
            Screen::Runs => {
                match self.runs_newest().get(self.pick) {
                    Some(r) if ask::running(r) => out.push("Press Enter to see what the agent is writing, or to stop it.".into()),
                    Some(_) => out.push("Press Enter to see how it went.".into()),
                    None => out.push("Nothing has run yet.".into()),
                }
                out.push("Press ↑↓ for another run, Esc to go back.".into());
            }
            Screen::Versions => {
                out.push("Press Enter, or Space, to put this version back in the stack, or read it whole.".into());
                out.push("Press ↑↓ for another version, Esc to go back.".into());
            }
            Screen::History => match self.hist_focus {
                HistFocus::List => {
                    out.push("Press Enter to go to it: the stack it made, the comment, the run.".into());
                    out.push("Press Space to ask the agent about it, answer only, or to start a new database from there.".into());
                    out.push("Press ↑↓ for another, → for the stack as it was then, and its patches; Esc to leave the history.".into());
                }
                HistFocus::Stack => {
                    out.push("Press Enter, or →, to read the patch as it was.".into());
                    out.push("Press ↑↓ for another of its patches, ← for the history. Only to look at: Esc leaves the history, to edit or comment.".into());
                }
                HistFocus::Patch => {
                    out.push("Press ↑↓ (pgup pgdn home end) to scroll, ← for the stack. The patch as it was, its comments as they stood: Esc leaves the history, to edit or comment.".into());
                }
            },
            Screen::Transcript | Screen::Text => {
                out.push("Press ↑↓ (pgup pgdn home end) to scroll, Esc to go back.".into())
            }
        }
        if self.screen != Screen::Main {
            out.push("Press Space for the menu.".into());
        }
        out.into_iter().map(|l| self.say_keys(l)).collect()
    }

    /// `text`, which names the keys as they are by default, naming them as
    /// the settings have them.
    pub fn say_keys(&self, text: String) -> String {
        use crate::keys::Key;
        let keys = &self.cfg.keys;
        if *keys == crate::keys::Keymap::default() {
            return text;
        }
        let l = |k: Key| keys.label(k);
        let lower = |k: Key| keys.label(k).to_lowercase();
        let arrows = format!("{}{}", l(Key::Up), l(Key::Down));
        let pages = format!("{} {} {} {}", lower(Key::PageUp), lower(Key::PageDown), lower(Key::Home), lower(Key::End));
        [
            ("pgup pgdn home end", pages),
            ("↑↓", arrows),
            ("←", l(Key::Left)),
            ("→", l(Key::Right)),
            ("Enter", l(Key::Enter)),
            ("Space", l(Key::Menu)),
            ("Esc", l(Key::Back)),
            ("enter:", format!("{}:", lower(Key::Enter))),
            ("space:", format!("{}:", lower(Key::Menu))),
            ("esc:", format!("{}:", lower(Key::Back))),
            ("?:", format!("{}:", l(Key::Help))),
            ("Press 1 ", format!("Press {} ", l(Key::View))),
            ("; 1 for", format!("; {} for", l(Key::View))),
            ("Press 2 ", format!("Press {} ", l(Key::Read))),
            ("; 2 to", format!("; {} to", l(Key::Read))),
            ("1: view", format!("{}: view", l(Key::View))),
            ("2: read", format!("{}: read", l(Key::Read))),
            ("2: comment", format!("{}: comment", l(Key::Read))),
        ]
        .into_iter()
        .fold(text, |t, (from, to)| t.replace(from, &to))
    }

    /// Where the arrows go from the middle.
    fn arrows_help(&self) -> String {
        if self.compact_drawn() {
            let mut keys = format!(
                "Press ↑↓ (pgup pgdn home end) to move; ← → to scroll the old side, both or the new side (now {})",
                self.compact.scroll.name()
            );
            keys += ", and past them, ← for the patches";
            if !self.subject_notes().is_empty() {
                keys += ", → for the comments";
            }
            return format!("{keys}.");
        }
        let mut keys = "Press ↑↓ (pgup pgdn home end) to move".to_string();
        keys += if self.list { ", ← for the patches" } else { ", ← to show the patches" };
        if !self.subject_notes().is_empty() {
            keys += if self.comments { ", → for the comments" } else { ", → to show the comments" };
        }
        format!("{keys}.")
    }

    /// History…'s keys: ↑↓ in the pane the cursor is in, ← → from pane to
    /// pane, enter on a record goes to it; esc leaves. The stack and its
    /// patch as they were are only to look at: nothing is edited, nor
    /// asked, from them.
    fn history_key(&mut self, k: KeyEvent) {
        let patches = self.hist_state().map_or(0, |(_, s)| s.patches.len());
        match (self.hist_focus, k.code) {
            (_, KeyCode::Esc) => self.screen = Screen::Main,
            (HistFocus::List, KeyCode::Right) => {
                if self.hist_state().is_some() {
                    self.hist_focus = HistFocus::Stack;
                }
            }
            (HistFocus::List, _) => {
                let was = self.pick;
                self.list_key(k);
                if self.pick != was {
                    self.hist_patch = 0;
                    self.hist_cursor = 0;
                    self.hist_top = 0;
                }
            }
            (HistFocus::Stack, KeyCode::Left) => self.hist_focus = HistFocus::List,
            (HistFocus::Stack, KeyCode::Right | KeyCode::Enter) => {
                if patches > 0 {
                    self.hist_focus = HistFocus::Patch;
                }
            }
            (HistFocus::Stack, code) => {
                let last = patches.saturating_sub(1);
                let was = self.hist_patch;
                self.hist_patch = match code {
                    KeyCode::Down => (self.hist_patch + 1).min(last),
                    KeyCode::Up => self.hist_patch.saturating_sub(1),
                    KeyCode::Home => 0,
                    KeyCode::End => last,
                    KeyCode::PageDown => (self.hist_patch + self.page()).min(last),
                    KeyCode::PageUp => self.hist_patch.saturating_sub(self.page()),
                    _ => self.hist_patch,
                };
                if self.hist_patch != was {
                    self.hist_cursor = 0;
                    self.hist_top = 0;
                }
            }
            (HistFocus::Patch, KeyCode::Left) => self.hist_focus = HistFocus::Stack,
            (HistFocus::Patch, KeyCode::Enter) => {
                self.status = "a preview, as it was: esc leaves the history, to edit or comment".into();
            }
            (HistFocus::Patch, code) => {
                let last = self.hist_rows.len().saturating_sub(1);
                self.hist_cursor = match code {
                    KeyCode::Down => (self.hist_cursor + 1).min(last),
                    KeyCode::Up => self.hist_cursor.saturating_sub(1),
                    KeyCode::PageDown => (self.hist_cursor + self.page()).min(last),
                    KeyCode::PageUp => self.hist_cursor.saturating_sub(self.page()),
                    KeyCode::Home => 0,
                    KeyCode::End => last,
                    _ => self.hist_cursor,
                };
            }
        }
    }

    /// The stack as it was just after the record History…'s cursor is on:
    /// its rev and itself, if it was on one.
    pub fn hist_state(&self) -> Option<(usize, Stack)> {
        let e = self.history().into_iter().nth(self.pick)?;
        let at = crate::history::stack_at(&self.db, &self.stack, e.rev)?;
        Some((at, self.db.node(at).ok()?.clone()))
    }

    /// The lists: the tree, the stacks, the runs, the versions, the
    /// history.
    fn list_key(&mut self, k: KeyEvent) {
        let last = self.picks().saturating_sub(1);
        match k.code {
            KeyCode::Down => self.pick = (self.pick + 1).min(last),
            KeyCode::Up => self.pick = self.pick.saturating_sub(1),
            KeyCode::PageDown => self.pick = (self.pick + self.page()).min(last),
            KeyCode::PageUp => self.pick = self.pick.saturating_sub(self.page()),
            KeyCode::Home => self.pick = 0,
            KeyCode::End => self.pick = last,
            KeyCode::Enter => self.choose(),
            KeyCode::Esc | KeyCode::Left => self.screen = Screen::Main,
            _ => {}
        }
        if self.screen == Screen::History {
            self.hist_rev = self.history().get(self.pick).map(|e| e.rev);
        }
    }

    fn transcript_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Up => self.scroll += 1,
            KeyCode::Down => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageUp => self.scroll += self.page(),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(self.page()),
            KeyCode::Home => self.scroll = usize::MAX / 2,
            KeyCode::End => self.scroll = 0,
            KeyCode::Esc | KeyCode::Left => self.screen = Screen::Runs,
            _ => {}
        }
    }

    fn text_key(&mut self, k: KeyEvent) {
        let page = self.page();
        let Some(t) = self.text.as_mut() else {
            self.screen = Screen::Main;
            return;
        };
        let last = t.lines.len().saturating_sub(1);
        match k.code {
            KeyCode::Down => t.scroll = (t.scroll + 1).min(last),
            KeyCode::Up => t.scroll = t.scroll.saturating_sub(1),
            KeyCode::PageDown => t.scroll = (t.scroll + page).min(last),
            KeyCode::PageUp => t.scroll = t.scroll.saturating_sub(page),
            KeyCode::Home => t.scroll = 0,
            KeyCode::End => t.scroll = last,
            KeyCode::Esc | KeyCode::Left => self.screen = t.back,
            _ => {}
        }
    }

    fn menu_key(&mut self, mut m: Menu, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Down => m.step(true),
            KeyCode::Up => m.step(false),
            KeyCode::Home => {
                m.sel = m.items.len() - 1;
                m.step(true);
            }
            KeyCode::End => {
                m.sel = 0;
                m.step(false);
            }
            KeyCode::Esc | KeyCode::Char(' ') => {
                // A comment written stays being written.
                if let Some(e) = self.pending.take() {
                    self.mode = Mode::Edit(e);
                }
                return Effect::None;
            }
            KeyCode::Enter => {
                return match m.chosen() {
                    Some(item) => self.perform(item.action.clone()),
                    None => Effect::None,
                };
            }
            _ => {}
        }
        self.mode = Mode::Menu(m);
        Effect::None
    }

    fn edit_key(&mut self, mut e: Edit, k: KeyEvent) -> Effect {
        match e.ed.key(k) {
            Op::Stay => self.mode = Mode::Edit(e),
            Op::Close => {}
            Op::Done(_) if matches!(e.target, Target::About(_)) => {
                let w = self.read_only().is_none();
                let items = vec![
                    Item::new("Save it and ask agent, answer only", Action::Save(Some(Way::Answer))).when(w),
                    Item::new("Save it", Action::Save(None)),
                    Item::line(),
                    Item::new("Throw it away", Action::ThrowAway),
                    Item::new("Keep writing", Action::KeepWriting),
                ];
                self.pending = Some(e);
                self.mode = Mode::Menu(Menu::new("the question", items, self.read_only().map(String::from)));
            }
            Op::Done(_) => {
                let send = self.can_send_target(&e.target);
                let mut items = vec![Item::new("Save it", Action::Save(None))];
                if send {
                    items.push(Item::new("Save it and ask agent", Action::Save(Some(Way::Change))).when(self.read_only().is_none()));
                    items.push(Item::new("Save it and ask agent, answer only", Action::Save(Some(Way::Answer))).when(self.read_only().is_none()));
                    if matches!(e.target, Target::Stack | Target::Patch(_)) {
                        items.push(Item::new("Save it and ask agent to build and test", Action::Save(Some(Way::Test))).when(self.read_only().is_none()));
                    }
                }
                items.push(Item::line());
                items.push(Item::new("Throw it away", Action::ThrowAway));
                items.push(Item::new("Keep writing", Action::KeepWriting));
                self.pending = Some(e);
                self.mode = Mode::Menu(Menu::new("the comment", items, self.read_only().map(String::from)));
            }
        }
        Effect::None
    }

    /// Whether a comment written for `t` can be sent once saved: not while
    /// claude works on the thread.
    fn can_send_target(&self, t: &Target) -> bool {
        match t {
            Target::Reply { change, id } => !self.busy().contains_key(&(change.clone(), id.clone())),
            _ => true,
        }
    }

    fn moving_key(&mut self, from: usize, mut to: usize, k: KeyEvent) {
        let last = self.patch_count().saturating_sub(1);
        match k.code {
            KeyCode::Down => to = (to + 1).min(last),
            KeyCode::Up => to = to.saturating_sub(1),
            KeyCode::Home => to = 0,
            KeyCode::End => to = last,
            KeyCode::Esc => return,
            KeyCode::Enter if to == from => return,
            KeyCode::Enter => {
                let name = self.stack.clone();
                if self.edit(|db| ops::move_patch(db, &name, from, to)).is_some() {
                    self.sel = to;
                    return;
                }
                // Refused, and the status line says why: it can go elsewhere.
            }
            _ => {}
        }
        self.mode = Mode::Moving { from, to };
    }

    fn stream_key(&mut self, change: String, id: String, mut up: usize, k: KeyEvent) {
        let page = (self.body_h / 2).max(5);
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Up => up += 1,
            KeyCode::Down => up = up.saturating_sub(1),
            KeyCode::PageUp => up += page,
            KeyCode::PageDown => up = up.saturating_sub(page),
            KeyCode::Home => up = usize::MAX / 4,
            KeyCode::End => up = 0,
            _ => {}
        }
        // No further up than its start.
        if let Some((_, run)) = self.busy().get(&(change.clone(), id.clone())) {
            let all = ask::tail(&self.db, run, usize::MAX).len();
            up = up.min(all.saturating_sub(page));
        }
        self.mode = Mode::Stream { change, id, up };
    }

    /// The comments listed in the comments pane, of the stack's or the
    /// patch's: what Send all sends, the sendable ones.
    fn listed_sendable(&self, way: Way) -> Vec<(String, String)> {
        let busy = self.busy();
        self.notes()
            .iter()
            .filter_map(|(_, id)| self.db.note(&self.subject_change()?, id))
            .filter(|n| ask::sendable(n) && !busy.contains_key(&(n.change.clone(), n.id.clone())))
            .filter(|n| way != Way::Test || ask::can_test(&self.db, n))
            // On the history: asked about to answer only.
            .filter(|n| way == Way::Answer || n.about.is_none())
            .map(|n| (n.change.clone(), n.id.clone()))
            .collect()
    }

    /// The items for the menu's end, everywhere: undo, redo, the view, the
    /// runs, quit.
    fn common(&self, items: &mut Vec<Item>) {
        let w = self.read_only().is_none();
        items.push(Item::line());
        // Nothing is edited from History…, only looked at.
        let edits = self.screen != Screen::History;
        if edits && (self.current().is_some() || self.db.names().contains(&self.stack.as_str())) {
            if let Some((_, op)) = self.db.undo_target(&self.stack) {
                items.push(Item::new(format!("Undo: {op}"), Action::Undo).when(w));
            }
            if let Some((_, op)) = self.db.redo_target(&self.stack) {
                items.push(Item::new(format!("Redo: {op}"), Action::Redo).when(w));
            }
        }
        if !self.db.runs().is_empty() {
            let label = match self.running() {
                0 => "Runs…".to_string(),
                n => format!("Runs… (agent ●{n})"),
            };
            items.push(Item::new(label, Action::Runs));
        }
        items.push(Item::line());
        items.push(Item::new("Quit", Action::Quit));
    }

    /// All that can be done where the cursor is, then the menu's end.
    pub fn main_menu(&self) -> Menu {
        let ro = self.read_only().map(String::from);
        let w = ro.is_none();
        let n = self.patch_count();
        let mut title = self.stack.clone();
        let mut items = Vec::new();
        match (self.screen, self.focus) {
            (Screen::Main, Focus::List) if self.on_stack => {
                title = format!("the stack {}", self.stack);
                if self.current().is_some() {
                    items.push(Item::new("Comment on the stack…", Action::CommentStack));
                    self.send_all_items(&mut items, true);
                    items.push(Item::new("Show…", Action::ShowMenu));
                    items.push(Item::line());
                    items.push(Item::new("Edit the stack as a list in $EDITOR", Action::EditList).when(w && n > 0));
                    items.push(Item::new("Tree of edits…", Action::Tree));
                    items.push(Item::new("History…", Action::History));
                    if self.cfg.reviews.source != crate::config::Source::None {
                        items.push(Item::new("Pull reviews from Phabricator", Action::Pull));
                    }
                    items.push(Item::new("Apply the stack to the worktree…", Action::AskApply).when(w));
                    items.push(Item::line());
                }
                if self.db.stacks().iter().any(|s| s.name != self.stack) {
                    items.push(Item::new("Switch stack…", Action::Stacks));
                }
                if self.current().is_some() {
                    items.push(Item::new("Delete this stack…", Action::AskDeleteStack).when(w));
                }
            }
            (Screen::Main, Focus::List) if n > 0 => {
                title = self.patch_title();
                items.push(Item::new("Read this patch", Action::Open));
                items.push(Item::new("Comment on this patch…", Action::CommentPatch));
                items.push(Item::new("Move this patch…", Action::MovePatch).when(w && n > 1));
                items.push(Item::new("Edit this patch in $EDITOR", Action::EditPatch).when(w));
                items.push(Item::new("Drop this patch…", Action::AskDrop).when(w));
                items.push(Item::new("Versions of this patch…", Action::Versions));
            }
            (Screen::Main, Focus::Notes) if self.notes_head => {
                title = "the comments listed".into();
                self.send_all_items(&mut items, !self.on_stack);
                items.push(Item::new("Show…", Action::ShowMenu));
            }
            (Screen::Main, Focus::Diff | Focus::Notes) => {
                if let Some(c) = self.picked() {
                    title = match by_of(&c) {
                        By::You => "your comment".into(),
                        By::Agent => "agent's comment".into(),
                        By::Reviewer => format!("{}'s comment", c.by.clone().unwrap_or_default()),
                    };
                    self.comment_items(&c, &mut items, w);
                } else {
                    if self.commentable().is_some() {
                        title = format!("line: {}", self.rows.get(self.cursor).and_then(|r| r.preview.clone()).unwrap_or_default());
                        items.push(Item::new("Comment on this line…", Action::CommentLine));
                        items.push(Item::new("Comment in $EDITOR", Action::CommentInEditor));
                    }
                    // Edited by hand, where the cursor is.
                    if self.focus == Focus::Diff && !self.on_stack && self.patch_id().is_some() {
                        items.push(Item::line());
                        if let Some((file, _)) = self.code_place() {
                            let short = file.rsplit('/').next().unwrap_or(&file).to_string();
                            items.push(Item::new(format!("Edit {short} here in $EDITOR"), Action::EditCode).when(w));
                        }
                        items.push(Item::new("Edit this patch in $EDITOR", Action::EditPatch).when(w));
                    }
                }
            }
            (Screen::Tree, _) => {
                if let Some(l) = tree::graph(&self.db, &self.stack).get(self.pick) {
                    title = format!("@{}", l.rev);
                    items.push(Item::new("Edit from this one", Action::EditFrom(l.rev)).when(w));
                    items.push(Item::new("What this edit changed…", Action::WhatChanged(l.rev)));
                }
            }
            (Screen::History, _) => {
                if let Some(e) = self.history().get(self.pick) {
                    title = format!("@{}", e.rev);
                    items.push(Item::new("Ask agent about this, answer only…", Action::AskAbout(e.rev)));
                    if !self.about(e.rev).is_empty() {
                        items.push(Item::new("Go to its comments", Action::GoToAbout(e.rev)));
                    }
                    items.push(Item::line());
                    items.push(Item::new("Start a new database here…", Action::AskTrim(e.rev)));
                }
            }
            (Screen::Versions, _) => {
                if let Some((p, _)) = self.versions_list().get(self.pick) {
                    title = "this version".into();
                    let current = self.current().is_some_and(|s| s.patches.contains(&p.id));
                    items.push(Item::new("Put this version back in the stack", Action::UseVersion(p.id.clone())).when(w && !current));
                    if self.whole {
                        items.push(Item::new("Read what changed from the version before", Action::Whole(false)));
                    } else {
                        items.push(Item::new("Read it whole", Action::Whole(true)));
                    }
                }
            }
            _ => {}
        }
        self.common(&mut items);
        Menu::new(title, items, ro)
    }

    /// The Send all items of a header: the comments listed that can be sent.
    fn send_all_items(&self, items: &mut Vec<Item>, test: bool) {
        let w = self.read_only().is_none();
        let n = self.listed_sendable(Way::Change).len();
        let all = |t: &str| format!("Ask agent about all {n}{t}");
        items.push(Item::new(all(""), Action::SendAll(Way::Change)).when(w && n > 0));
        items.push(Item::new(all(", answer only"), Action::SendAll(Way::Answer)).when(w && n > 0));
        if test {
            let t = self.listed_sendable(Way::Test).len();
            items.push(Item::new(format!("Ask agent to build and test, for all {t}"), Action::SendAll(Way::Test)).when(w && t > 0));
        }
    }

    fn comment_items(&self, c: &Note, items: &mut Vec<Item>, w: bool) {
        let busy = self.busy();
        let with = busy.get(&(c.change.clone(), c.id.clone()));
        items.push(Item::new("Reply…", Action::Reply));
        items.push(Item::new("Reply in $EDITOR", Action::CommentInEditor));
        items.push(Item::line());
        match with {
            Some((_, run)) => {
                items.push(Item::new("Read what the agent is writing", Action::OpenStream));
                items.push(Item::new("Stop the agent…", Action::AskStop(run.clone())));
            }
            None if c.queued.is_some() => items.push(Item::new("Take it back from the agent's queue", Action::Unsend)),
            None if c.about.is_some() => {
                items.push(Item::new("Ask agent, answer only", Action::Send(Way::Answer)).when(w && ask::sendable(c)));
            }
            None => {
                let s = ask::sendable(c);
                items.push(Item::new("Ask agent", Action::Send(Way::Change)).when(w && s));
                items.push(Item::new("Ask agent, answer only", Action::Send(Way::Answer)).when(w && s));
                if ask::can_test(&self.db, c) {
                    items.push(Item::new("Ask agent to build and test", Action::Send(Way::Test)).when(w && s));
                }
            }
        }
        if let Some(rev) = c.about {
            items.push(Item::line());
            items.push(Item::new(format!("Go to @{rev} in the history"), Action::GoToHistory(rev)));
        }
        items.push(Item::line());
        if self.focus == Focus::Notes {
            items.push(Item::new("Go to it", Action::GoToNote));
        }
        if !self.db.opened_from(&self.stack, &c.change, &c.id).is_empty() {
            items.push(Item::new("Go to the comments it opened", Action::GoToOpened));
            items.push(Item::new("Delete it and the comments it opened…", Action::AskDeleteOpened));
        }
        if let Some(m) = &c.moved {
            items.push(Item::new("Go to where it came from", Action::GoToOrigin));
            let back = self.where_was(m).split(" · ").next().unwrap_or("its patch").to_string();
            items.push(Item::new(format!("Move it back to {back}"), Action::MoveBack));
        }
        items.push(Item::new("History of this comment…", Action::NoteHistory));
        items.push(Item::new("Delete this comment…", Action::AskDeleteComment));
    }

    fn perform(&mut self, action: Action) -> Effect {
        let name = self.stack.clone();
        match action {
            Action::Line | Action::Cancel => {}
            Action::Open => {
                self.screen = Screen::Main;
                self.focus = Focus::Diff;
            }
            Action::GoToNote => self.go_to_note(),
            Action::MovePatch => {
                self.mode = Mode::Moving {
                    from: self.sel,
                    to: self.sel,
                }
            }
            Action::EditPatch => return Effect::EditPatch { line: self.mail_place() },
            Action::EditCode => {
                if let Some((file, line)) = self.code_place() {
                    return Effect::EditCode { file, line, text: None };
                }
            }
            Action::EditAgain => {
                if let Some((file, line, text)) = self.refused.take() {
                    return Effect::EditCode {
                        file,
                        line,
                        text: Some(text),
                    };
                }
            }
            Action::ThrowEdit => {
                self.refused = None;
                self.status = "your edit is thrown away".into();
            }
            Action::AskDrop => {
                let title = format!("drop {}?", self.patch_title());
                self.mode = Mode::Menu(Menu::ask(title, Item::new("Drop it", Action::Drop), "Keep it", false));
            }
            Action::Drop => {
                let at = self.sel;
                self.edit(|db| ops::drop_patches(db, &name, &[at]));
            }
            Action::CommentStack => self.write(Target::Stack, Focus::List, ""),
            Action::CommentPatch => {
                if let Some(id) = self.patch_id() {
                    let change = self.db.patch(&id).change.clone();
                    self.write(Target::Patch(change), Focus::List, "");
                }
            }
            Action::CommentLine => {
                if let Some((change, key, file, preview)) = self.commentable() {
                    // Yours on the line already: what is written is a reply.
                    let target = if self.db.note(&change, &key).is_some() {
                        Target::Reply { change, id: key }
                    } else {
                        Target::Line { change, key, file, preview }
                    };
                    self.write(target, Focus::Diff, "");
                }
            }
            Action::Reply => {
                if let Some(c) = self.picked() {
                    self.write(Target::Reply { change: c.change, id: c.id }, self.focus, "");
                }
            }
            Action::CommentInEditor => {
                let target = match self.picked() {
                    Some(c) => Target::Reply { change: c.change, id: c.id },
                    None => match self.commentable() {
                        Some((change, key, file, preview)) => Target::Line { change, key, file, preview },
                        None => return Effect::None,
                    },
                };
                return Effect::EditComment(target);
            }
            Action::Send(way) => {
                if let Some(c) = self.picked() {
                    self.send(vec![(c.change, c.id)], way);
                }
            }
            Action::SendAll(way) => {
                let notes = self.listed_sendable(way);
                self.send(notes, way);
            }
            Action::Unsend => {
                if let Some(c) = self.picked() {
                    if let Err(e) = ask::unsend(&self.path, &[(c.change, c.id)]) {
                        self.status = e.0;
                    }
                    self.reload();
                }
            }
            Action::OpenStream => {
                if let Some(c) = self.picked() {
                    self.mode = Mode::Stream {
                        change: c.change,
                        id: c.id,
                        up: 0,
                    };
                }
            }
            Action::AskStop(id) => {
                let yes = Item::new("Stop it", Action::Stop(id));
                self.mode = Mode::Menu(Menu::ask("stop the agent? the ask lands as failed", yes, "Let it run", true));
            }
            Action::Stop(id) => {
                if let Some(r) = self.db.run(&id) {
                    ask::stop(r);
                    self.status = "stopped: it lands as failed".into();
                }
            }
            Action::GoToOpened => {
                if let Some(c) = self.picked()
                    && let Some(o) = self.db.opened_from(&name, &c.change, &c.id).first().map(|o| (o.change.clone(), o.id.clone()))
                {
                    self.go_to(&o.0, &o.1);
                }
            }
            Action::AskDeleteOpened => {
                if let Some(c) = self.picked() {
                    let n = self.db.opened_from(&name, &c.change, &c.id).len();
                    let title = format!("delete this comment and the {n} it opened? it cannot be undone");
                    self.mode = Mode::Menu(Menu::ask(title, Item::new("Delete them", Action::DeleteOpened), "Keep them", true));
                }
            }
            Action::DeleteOpened => {
                if let Some(c) = self.picked() {
                    self.edit(|db| {
                        let mut all: Vec<Note> = db.opened_from(&name, &c.change, &c.id).into_iter().cloned().collect();
                        all.push(c.clone());
                        for n in all {
                            db.set_note(Note { text: String::new(), ..n })?;
                        }
                        Ok(())
                    });
                }
            }
            Action::GoToOrigin => {
                if let Some(m) = self.picked().and_then(|c| c.moved) {
                    self.go_to_patch_line(&m.change, &m.line);
                }
            }
            Action::MoveBack => {
                if let Some(c) = self.picked() {
                    self.edit(|db| ask::move_back(db, &c));
                }
            }
            Action::NoteHistory => {
                if let Some(c) = self.picked() {
                    self.open_text(format!("history of a comment · {}", short(&c.preview, 40)), self.note_history_lines(&c.change, &c.id));
                }
            }
            Action::AskDeleteComment => {
                self.mode = Mode::Menu(Menu::ask(
                    "delete this comment? it cannot be undone",
                    Item::new("Delete it", Action::DeleteComment),
                    "Keep it",
                    true,
                ));
            }
            Action::DeleteComment => {
                if let Some(c) = self.picked() {
                    self.edit(|db| db.set_note(Note { text: String::new(), ..c }));
                }
            }
            Action::ShowMenu => {
                let items = self
                    .show_counts()
                    .into_iter()
                    .map(|(s, n)| {
                        let mark = if s == self.show { "● " } else { "  " };
                        Item::new(format!("{mark}{} ({n})", s.name()), Action::SetShow(s))
                    })
                    .collect();
                let mut m = Menu::new("show the comments", items, None);
                m.sel = Show::ALL.iter().position(|s| *s == self.show).unwrap_or(0);
                self.mode = Mode::Menu(m);
            }
            Action::SetShow(s) => {
                self.show = s;
                self.built = None;
                let show = s.name();
                self.edit(|db| db.set_view(&name, show));
            }
            Action::Undo => {
                self.edit(|db| db.undo(&name));
            }
            Action::Redo => {
                self.edit(|db| db.redo(&name));
            }
            Action::EditList => return Effect::EditTodo,
            Action::Tree => self.open_list(Screen::Tree),
            Action::EditFrom(rev) => {
                if self.edit(|db| db.select(rev)).is_some() {
                    self.status = format!("editing @{rev}");
                    self.screen = Screen::Main;
                }
            }
            Action::WhatChanged(rev) => {
                let lines = self.what_changed(rev);
                self.open_text(format!("what the edit that made @{rev} changed"), lines);
            }
            Action::History => self.open_list(Screen::History),
            Action::Versions => {
                if let Some(id) = self.patch_id() {
                    self.versions = Some(self.db.patch(&id).change.clone());
                    self.whole = false;
                    self.open_list(Screen::Versions);
                    self.pick = self.versions_list().iter().position(|(p, _)| p.id == id).unwrap_or(0);
                }
            }
            Action::UseVersion(id) => {
                let change = self.db.patch(&id).change.clone();
                let done = self.edit(|db| {
                    let s = db.stack(&name)?.clone();
                    let Some(at) = s.patches.iter().position(|p| db.patch(p).change == change) else {
                        return err("its patch is no longer in the stack");
                    };
                    let mut patches = s.patches.clone();
                    patches[at] = id.clone();
                    db.set_stack(&name, patches, &format!("back to version {}", &id[..8]))
                });
                if done.is_some() {
                    self.status = "that version is back in the stack".into();
                }
            }
            Action::Whole(w) => self.whole = w,
            Action::MarkRead => self.mark_read(),
            Action::MarkUnread => self.mark_unread(),
            Action::Pull => {
                if self.bg.doing.contains(&"pulling") {
                    self.status = "already pulling the reviews".into();
                } else {
                    self.bg.doing.push("pulling");
                    self.status = "pulling the reviews from Phabricator…".into();
                    let path = self.path.clone();
                    self.spawn(move || {
                        Done::Said(
                            "pulling",
                            match review::pull(&path, &name) {
                                Ok(r) => r.summary,
                                Err(e) => e.0,
                            },
                        )
                    });
                }
            }
            Action::AskApply => {
                let (n, repo) = match self.current() {
                    Some(s) => (s.patches.len(), s.repo.clone().unwrap_or_default()),
                    None => return Effect::None,
                };
                let title = format!("commit the stack's {n} patch{} in {repo}, on top of its base, on no branch?", if n == 1 { "" } else { "es" });
                self.mode = Mode::Menu(Menu::ask(title, Item::new("Apply it", Action::Apply), "Cancel", true));
            }
            Action::Apply => {
                if self.bg.doing.contains(&"applying") {
                    self.status = "already applying".into();
                } else {
                    self.bg.doing.push("applying");
                    self.status = "applying the stack to the worktree…".into();
                    let path = self.path.clone();
                    self.spawn(move || {
                        Done::Said(
                            "applying",
                            match ops::apply_stack(&path, &name, None) {
                                Ok(r) => r.summary,
                                Err(e) => e.0,
                            },
                        )
                    });
                }
            }
            Action::AskAbout(rev) => self.write(Target::About(rev), Focus::Diff, ""),
            Action::GoToAbout(rev) => {
                if let Some(n) = self.about(rev).first() {
                    let (change, id) = (n.change.clone(), n.id.clone());
                    self.go_to(&change, &id);
                }
            }
            Action::GoToHistory(rev) => {
                self.open_list(Screen::History);
                self.pick = self.history().iter().position(|e| e.rev == rev).unwrap_or(0);
                self.hist_rev = Some(rev);
            }
            Action::AskTrim(rev) => {
                let to = self.trimmed_path(rev);
                let title = format!(
                    "write {}: the history from @{rev} on, what was before it its start? {} is not changed",
                    to.file_name().unwrap_or_default().to_string_lossy(),
                    self.path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.mode = Mode::Menu(Menu::ask(title, Item::new("Write it", Action::Trim(rev)), "Cancel", false));
            }
            Action::Trim(rev) => {
                if self.bg.doing.contains(&"trimming") {
                    self.status = "already writing a new database".into();
                } else {
                    self.bg.doing.push("trimming");
                    let (from, to) = (self.path.clone(), self.trimmed_path(rev));
                    self.status = format!("writing {}…", to.display());
                    self.spawn(move || {
                        Done::Said(
                            "trimming",
                            match crate::trim::trim(&from, &to, Some(rev)) {
                                Ok(t) => t.said(&to),
                                Err(e) => e.0,
                            },
                        )
                    });
                }
            }
            Action::Runs => self.open_list(Screen::Runs),
            Action::Stacks => self.open_list(Screen::Stacks),
            Action::AskDeleteStack => {
                let title = format!("delete stack {name}? undo brings it back");
                self.mode = Mode::Menu(Menu::ask(title, Item::new("Delete it", Action::DeleteStack), "Keep it", false));
            }
            Action::DeleteStack => {
                self.edit(|db| db.delete_stack(&name));
            }
            Action::Transcript(id) => {
                self.transcript = Some(id);
                self.scroll = 0;
                self.screen = Screen::Transcript;
            }
            Action::Save(way) => {
                if let Some(e) = self.pending.take() {
                    let text = e.ed.text();
                    if let Some(saved) = self.save(&e.target, &text)
                        && let Some(way) = way
                    {
                        self.send(vec![saved], way);
                    }
                }
            }
            Action::ThrowAway => {
                self.pending = None;
                self.status = "thrown away".into();
            }
            Action::KeepWriting => {
                if let Some(e) = self.pending.take() {
                    self.mode = Mode::Edit(e);
                }
            }
            Action::Quit => return Effect::Quit,
        }
        Effect::None
    }

    /// Opens the editor for `target`, in `pane`.
    fn write(&mut self, target: Target, pane: Focus, text: &str) {
        self.mode = Mode::Edit(Edit {
            ed: Editor::new(text),
            target,
            pane,
        });
    }

    /// Saves `text` where `target` says: a new comment, or your turn of the
    /// thread. The comment, (change, id).
    pub fn save(&mut self, target: &Target, text: &str) -> Option<(String, String)> {
        if text.trim().is_empty() {
            return None;
        }
        let name = self.stack.clone();
        let text = text.trim().to_string();
        let stamp = crate::db::unique();
        self.edit(|db| {
            let note = match target {
                Target::Stack => Note {
                    change: stack_change(&name),
                    id: format!("s{stamp}"),
                    text,
                    ..Default::default()
                },
                Target::Patch(change) => {
                    let p = db
                        .stack(&name)?
                        .patches
                        .iter()
                        .map(|id| db.patch(id))
                        .find(|p| p.change == *change)
                        .cloned()
                        .ok_or_else(|| crate::db::Error("its patch is no longer in the stack".into()))?;
                    Note {
                        change: change.clone(),
                        id: format!("p{stamp}"),
                        line: title_key(&p),
                        text,
                        file: MSG_PATH.into(),
                        preview: p.title().chars().take(80).collect(),
                        ..Default::default()
                    }
                }
                Target::Line { change, key, file, preview } => match db.note(change, key).cloned() {
                    Some(n) => Note {
                        text: append_turn(&n.text, Role::You, &text, None),
                        failed: None,
                        ..n
                    },
                    None => Note {
                        change: change.clone(),
                        id: key.clone(),
                        line: key.clone(),
                        text,
                        file: file.clone(),
                        preview: preview.clone(),
                        ..Default::default()
                    },
                },
                Target::Reply { change, id } => {
                    let n = db.note(change, id).cloned().ok_or_else(|| crate::db::Error("the comment is gone".into()))?;
                    Note {
                        text: append_turn(&n.text, Role::You, &text, None),
                        failed: None,
                        ..n
                    }
                }
                Target::About(rev) => Note {
                    change: stack_change(&name),
                    id: format!("h{stamp}"),
                    text,
                    preview: crate::history::entries(db, &name)
                        .into_iter()
                        .find(|e| e.rev == *rev)
                        .map_or(format!("@{rev}"), |e| e.text.chars().take(80).collect()),
                    about: Some(*rev),
                    ..Default::default()
                },
            };
            let at = (note.change.clone(), note.id.clone());
            db.set_note(note)?;
            Ok(at)
        })
    }

    /// Puts `notes` on the agent's queue; they are started away from the
    /// screen.
    fn send(&mut self, notes: Vec<(String, String)>, way: Way) {
        let n = notes.len();
        if let Err(e) = ask::queue(&self.path, &self.stack, &notes, way) {
            self.status = e.0;
            self.reload();
            return;
        }
        self.reload();
        self.status = match ask::waits_on(&self.db, &self.stack, way) {
            None => format!("asked the agent about {n} comment{} ({}): it is on it", plural(n), way.said()),
            Some(w) => format!("asked the agent about {n} comment{} ({}): ready, it goes {w}", plural(n), way.said()),
        };
        self.launch();
    }

    /// Goes to comment `id` on `change`: its patch, or the stack, and its
    /// first row.
    fn go_to(&mut self, change: &str, id: &str) {
        self.screen = Screen::Main;
        if change == stack_change(&self.stack) {
            self.on_stack = true;
        } else if let Some(at) = self.current().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == change)) {
            self.on_stack = false;
            self.sel = at;
        } else {
            self.status = "that comment's patch is no longer in the stack".into();
            return;
        }
        self.built = None;
        self.want = Some(Want::Note(id.into()));
        self.focus = Focus::Diff;
    }

    /// Goes to the line `key` of the patch of `change`.
    fn go_to_patch_line(&mut self, change: &str, key: &str) {
        let Some(at) = self.current().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == change)) else {
            self.status = "its patch is no longer in the stack".into();
            return;
        };
        self.screen = Screen::Main;
        self.on_stack = false;
        self.sel = at;
        self.built = None;
        self.want = Some(Want::Line(key.into()));
        self.focus = Focus::Diff;
    }

    /// Where a database trimmed from `rev` on is written: by this one, a
    /// name not taken.
    fn trimmed_path(&self, rev: usize) -> PathBuf {
        let dir = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let ext = self.path.extension().map_or("jsonl".into(), |e| e.to_string_lossy().into_owned());
        (1..)
            .map(|n| {
                let n = if n == 1 { String::new() } else { format!("-{n}") };
                dir.join(format!("{stem}-from-{rev}{n}.{ext}"))
            })
            .find(|p| !p.exists())
            .unwrap()
    }

    fn open_list(&mut self, screen: Screen) {
        self.screen = screen;
        self.pick = 0;
        self.pick_top = 0;
        self.hist_rev = None;
        self.hist_focus = HistFocus::List;
        self.hist_patch = 0;
        self.hist_cursor = 0;
        self.hist_top = 0;
        if screen == Screen::Tree {
            let head = self.db.head(&self.stack).map(|(r, _)| r);
            self.pick = tree::graph(&self.db, &self.stack)
                .iter()
                .position(|l| Some(l.rev) == head)
                .unwrap_or(0);
        }
        if screen == Screen::Stacks {
            self.pick = self.db.stacks().iter().position(|s| s.name == self.stack).unwrap_or(0);
        }
    }

    fn open_text(&mut self, title: String, lines: Vec<(String, Style)>) {
        self.text = Some(TextView {
            title,
            lines,
            scroll: 0,
            back: self.screen,
        });
        self.screen = Screen::Text;
    }

    fn runs_newest(&self) -> Vec<&crate::db::Run> {
        let mut runs = self.db.runs();
        runs.reverse();
        runs
    }

    fn picks(&self) -> usize {
        match self.screen {
            Screen::Tree => tree::graph(&self.db, &self.stack).len(),
            Screen::Stacks => self.db.stacks().len(),
            Screen::Runs => self.db.runs().len(),
            Screen::Versions => self.versions_list().len(),
            Screen::History => self.history().len(),
            _ => 0,
        }
    }

    /// Enter on a list: what there is to do with the line, or, when there
    /// is more than one thing, a menu of them.
    fn choose(&mut self) {
        match self.screen {
            Screen::Tree => {
                if let Some(l) = tree::graph(&self.db, &self.stack).get(self.pick) {
                    self.perform(Action::EditFrom(l.rev));
                }
            }
            Screen::Stacks => {
                if let Some(s) = self.db.stacks().get(self.pick) {
                    self.stack = s.name.clone();
                    self.sel = 0;
                    self.on_stack = self.patch_count() == 0;
                    self.built = None;
                    self.show = self.db.view(&self.stack).and_then(Show::parse).unwrap_or(Show::All);
                    self.judge();
                }
                self.screen = Screen::Main;
            }
            Screen::Runs => {
                let Some(r) = self.runs_newest().get(self.pick).map(|r| (*r).clone()) else {
                    return;
                };
                if ask::running(&r) {
                    let items = vec![
                        Item::new("Show what the agent wrote", Action::Transcript(r.id.clone())),
                        Item::new("Stop the agent…", Action::AskStop(r.id.clone())),
                    ];
                    self.mode = Mode::Menu(Menu::new(format!("{} {}", r.what, r.stack), items, None));
                } else {
                    self.perform(Action::Transcript(r.id));
                }
            }
            Screen::Versions => self.mode = Mode::Menu(self.main_menu()),
            Screen::History => {
                let Some(e) = self.history().get(self.pick).cloned() else { return };
                match e.to {
                    Goto::Stack(rev) => {
                        self.open_list(Screen::Tree);
                        self.pick = tree::graph(&self.db, &self.stack).iter().position(|l| l.rev == rev).unwrap_or(0);
                    }
                    Goto::Note { change, id } => {
                        if self.db.note(&change, &id).is_some() {
                            self.go_to(&change, &id);
                        } else {
                            let lines = self.note_history_lines(&change, &id);
                            self.open_text("history of a deleted comment".into(), lines);
                        }
                    }
                    Goto::Run(id) => {
                        self.perform(Action::Transcript(id));
                    }
                }
            }
            _ => {}
        }
    }

    /// Runs what needs $EDITOR; the terminal is the editor's meanwhile.
    pub fn effect(&mut self, e: Effect) -> Result<()> {
        let name = self.stack.clone();
        match e {
            Effect::EditPatch { line } if self.patch_id().is_some() => {
                self.status = ops::edit_patch(&self.path, &name, self.sel, line)?
            }
            Effect::EditCode { file, line, text } if self.patch_id().is_some() => {
                match ops::edit_code(&self.path, &name, self.sel, &file, line, text)? {
                    ops::Edited::Done(said) => self.status = said,
                    ops::Edited::Unchanged => self.status = "saved as it was: nothing changed".into(),
                    ops::Edited::Refused { why, text } => {
                        self.refused = Some((file, line, text));
                        let items = vec![
                            Item::new("Edit it again", Action::EditAgain),
                            Item::new("Throw the edit away", Action::ThrowEdit),
                        ];
                        let mut m = Menu::new("your edit is not taken", items, None);
                        m.note = Some(why);
                        self.reload();
                        self.mode = Mode::Menu(m);
                        return Ok(());
                    }
                }
            }
            Effect::EditTodo => self.status = ops::edit_stack(&self.path, &name)?,
            Effect::EditComment(target) => {
                let text = strip_comments(&edit_text(
                    "\n# Your comment. Lines starting with # are dropped.\n",
                    ".txt",
                )?);
                self.save(&target, &text);
            }
            _ => {}
        }
        self.reload();
        Ok(())
    }

    // -- history ------------------------------------------------------------ //

    /// The versions of the patch whose versions are listed, oldest first,
    /// each with the first stack to hold it.
    fn versions_list(&self) -> Vec<(Patch, Option<(usize, Stack)>)> {
        let Some(change) = &self.versions else { return vec![] };
        self.db
            .versions(change)
            .into_iter()
            .map(|p| (p.clone(), self.db.first_use(&p.id).map(|(r, s)| (r, s.clone()))))
            .collect()
    }

    /// What the edit that made stack `rev` changed, as the history says it,
    /// coloured as its lines are.
    fn what_changed(&self, rev: usize) -> Vec<(String, Style)> {
        crate::history::what_changed(&self.db, rev)
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                let st = if i == 0 {
                    color::file()
                } else if let Some(d) = l.strip_prefix("    ") {
                    diff_style(d)
                } else if l.starts_with("+ ") {
                    color::add()
                } else if l.starts_with("− ") {
                    color::del()
                } else if l.starts_with("~ ") {
                    color::hunk()
                } else if l == "nothing changed" {
                    color::dim()
                } else {
                    Style::new()
                };
                (l, st)
            })
            .collect()
    }

    /// Every record of a comment, oldest first, and what each was.
    fn note_history_lines(&self, change: &str, id: &str) -> Vec<(String, Style)> {
        let mut out = Vec::new();
        let mut before: Option<&Note> = None;
        for (rev, n) in self.db.note_history(change, id) {
            out.push((format!("@{rev} · {} · {}", local_time(n.time), note_change(before, n)), color::file()));
            for l in n.text.lines() {
                out.push((format!("    {l}"), Style::new()));
            }
            before = Some(n);
        }
        if out.is_empty() {
            out.push(("no record of it".into(), color::dim()));
        }
        out
    }

    /// Everything that happened to the stack, newest first: its edits, its
    /// head's moves, its comments, its runs.
    pub fn history(&self) -> Vec<Entry> {
        crate::history::entries(&self.db, &self.stack)
    }

    /// The comments on the history's record `rev`, oldest first.
    fn about(&self, rev: usize) -> Vec<Note> {
        let mut out: Vec<Note> = self
            .db
            .notes(&stack_change(&self.stack))
            .into_iter()
            .filter(|n| n.about == Some(rev))
            .cloned()
            .collect();
        out.sort_by_key(|n| self.db.note_history(&n.change, &n.id).first().map_or(0, |r| r.0));
        out
    }

    // -- drawing ------------------------------------------------------------ //

    /// Draws the whole screen into `buf`; where the terminal's cursor goes,
    /// when a comment is being written.
    pub fn draw(&mut self, buf: &mut Buffer) -> Option<(u16, u16)> {
        let area = buf.area;
        let (w, h) = (area.width as usize, area.height as usize);
        if w < 20 || h < 4 {
            return None;
        }
        self.body_h = h - 2;
        let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
        let mut cursor = None;
        match self.screen {
            Screen::Runs => self.draw_runs(buf, body),
            Screen::Transcript => self.draw_transcript(buf, body),
            Screen::History => {
                let w = body.width as usize;
                if w < 60 {
                    cursor = self.draw_history(buf, body);
                } else {
                    let list_w = (w * 2 / 5).max(30);
                    let stack_w = (w / 5).clamp(18, 32);
                    let (x1, x2) = (list_w, list_w + 1 + stack_w);
                    for y in 0..body.height {
                        buf.set_string(body.x + x1 as u16, body.y + y, "│", color::meta());
                        buf.set_string(body.x + x2 as u16, body.y + y, "│", color::meta());
                    }
                    let list = Rect::new(body.x, body.y, list_w as u16, body.height);
                    let stack = Rect::new(body.x + (x1 + 1) as u16, body.y, stack_w as u16, body.height);
                    let patch = Rect::new(body.x + (x2 + 1) as u16, body.y, (w - x2 - 1) as u16, body.height);
                    cursor = self.draw_history(buf, list);
                    self.draw_history_stack(buf, stack);
                    self.draw_history_patch(buf, patch);
                }
            }
            Screen::Text => self.draw_text(buf, body),
            Screen::Tree | Screen::Stacks | Screen::Versions => {
                let (left, (right, _)) = self.columns(buf, body, true, false, None);
                let left = left.unwrap();
                match self.screen {
                    Screen::Tree => self.draw_tree(buf, left),
                    Screen::Stacks => self.draw_stacks(buf, left),
                    _ => self.draw_versions(buf, left),
                }
                match self.screen {
                    Screen::Versions => self.draw_version(buf, right),
                    _ => self.draw_node(buf, right),
                }
            }
            Screen::Main => {
                let notes = self.notes_shown();
                let wide = match &self.mode {
                    Mode::Edit(e) => Some(e.pane),
                    _ => None,
                };
                let (left, (middle, right)) = self.columns(buf, body, self.list, notes, wide);
                // The middle first: the comments pane lists its rows.
                cursor = self.draw_patch(buf, middle).or(cursor);
                if let Some(left) = left {
                    cursor = self.draw_list(buf, left).or(cursor);
                }
                if let Some(right) = right {
                    cursor = self.draw_notes(buf, right).or(cursor);
                }
            }
        }
        self.draw_bar(buf, area);
        self.draw_menu(buf, body);
        self.draw_help(buf, body);
        self.draw_status(buf, area);
        cursor
    }

    /// The body cut into the patches (if `left`), the middle, and the
    /// comments (if `right`), with lines between. The pane a comment is
    /// written in, `wide`, takes half the width.
    fn columns(&self, buf: &mut Buffer, body: Rect, left: bool, right: bool, wide: Option<Focus>) -> (Option<Rect>, (Rect, Option<Rect>)) {
        let w = body.width as usize;
        let mut left_w = if left { (w / 4).clamp(20, 40) } else { 0 };
        let mut right_w = if right { (w / 4).clamp(24, 44) } else { 0 };
        match wide {
            Some(Focus::List) if left => left_w = left_w.max(w / 2),
            Some(Focus::Notes) if right => right_w = right_w.max(w / 2),
            _ => {}
        }
        let line = |buf: &mut Buffer, x: usize| {
            for y in 0..body.height {
                buf.set_string(body.x + x as u16, body.y + y, "│", color::meta());
            }
        };
        let mut x = 0;
        let l = left.then(|| {
            line(buf, left_w);
            x = left_w + 1;
            Rect::new(body.x, body.y, left_w as u16, body.height)
        });
        let mid_w = w.saturating_sub(x + if right { right_w + 1 } else { 0 });
        let mid = Rect::new(body.x + x as u16, body.y, mid_w as u16, body.height);
        let r = right.then(|| {
            line(buf, x + mid_w);
            Rect::new(body.x + (x + mid_w + 1) as u16, body.y, right_w as u16, body.height)
        });
        (l, (mid, r))
    }

    fn draw_bar(&self, buf: &mut Buffer, area: Rect) {
        let w = area.width as usize;
        let file = self.path.file_name().unwrap_or_default().to_string_lossy();
        let mut text = format!(" stacked · {file}");
        match self.db.head(&self.stack) {
            Some((rev, s)) => {
                text += &format!(" · {} @{rev} · {} patches", self.stack, s.patches.len());
                if let Some(b) = &s.base {
                    text += &format!(" on {}", &b[..12.min(b.len())]);
                }
                // With the patches out of sight, which one this is; with the
                // comments, how many there are.
                if !self.list && self.screen == Screen::Main && !s.patches.is_empty() && !self.on_stack {
                    text += &format!(" · patch {} of {}", self.sel + 1, s.patches.len());
                }
                let notes = self.subject_notes().len();
                if !self.comments && self.screen == Screen::Main && notes > 0 {
                    text += &format!(" · {notes} comment{}", plural(notes));
                }
                if self.screen == Screen::Main {
                    if self.reading {
                        text += " · reading";
                        let changed = self.rows.iter().filter(|r| matches!(r.kind, Kind::Add | Kind::Del)).count();
                        if !self.on_stack && changed > 0 {
                            let read = self.rows.iter().filter(|r| r.read).count();
                            text += &format!(" {read}/{changed}");
                        }
                    }
                    if self.layout != Layout::Unified && !self.on_stack {
                        text += &format!(" · {}", self.layout.label());
                        if self.compact_drawn() {
                            text += &format!(" {}", self.compact.scroll.mark());
                        }
                    }
                }
            }
            None => text += " · no stack",
        }
        let running = self.running();
        if running > 0 {
            text += &format!(" · agent ●{running}");
        }
        let ready = self.ready();
        if ready > 0 {
            text += &format!(" · ready:{ready}");
        }
        for d in &self.bg.doing {
            text += &format!(" · {d}…");
        }
        if self.bg.judging.as_ref().is_some_and(|(n, _)| *n == self.stack) {
            text += " · checking…";
        }
        let mut st = color::bar();
        if let Some(why) = self.judged.as_ref().filter(|j| j.name == self.stack).and_then(|j| j.why.as_ref()) {
            text += &format!(" · {why}");
            st = color::warn();
        }
        put(buf, area, 0, 0, w, &fit(&text, w), st);
    }

    /// The keys there are, where the keys go now.
    pub fn hint(&self) -> String {
        match &self.mode {
            Mode::Menu(_) => "↑↓ choose · enter: do it · esc: close · ?: what it does".into(),
            Mode::Moving { .. } => "↑↓ to place it · enter: put it there · esc: leave it where it was".into(),
            Mode::Edit(_) => "enter: a new line · esc: save, send or throw away".into(),
            Mode::Help { .. } => "any key: back, and that key".into(),
            Mode::Stream { .. } => "↑↓: scroll · esc: fold it".into(),
            Mode::Normal if self.reading_here() => {
                "space: mark this block read · enter: unread · 1: view · 2: comment · ?: what can I do here".into()
            }
            Mode::Normal => {
                let enter = match (self.screen, self.enter()) {
                    (Screen::Main, Some(Action::Open)) => "enter: read it · ",
                    (Screen::Main, Some(Action::GoToNote)) => "enter: go to it · ",
                    (Screen::Main, Some(Action::Reply)) => "enter: reply · ",
                    (Screen::Main, Some(Action::OpenStream)) => "enter: what the agent writes · ",
                    (Screen::Main, Some(_)) => "enter: comment · ",
                    (Screen::Main, None) => "enter or ",
                    (Screen::Transcript | Screen::Text, _) => "↑↓: scroll · ",
                    (Screen::History, _) => match self.hist_focus {
                        HistFocus::List => "enter: go to it · →: the stack then · ",
                        HistFocus::Stack => "enter: read it · ←: the history · ",
                        HistFocus::Patch => "↑↓: scroll · ←: the stack · ",
                    },
                    _ => "enter: pick · ",
                };
                let keys = match (self.screen, self.reading) {
                    (Screen::Main, false) => "1: view · 2: read · ",
                    (Screen::Main, true) => "1: view · 2: comment · ",
                    _ => "",
                };
                format!("{enter}space: menu · {keys}?: what can I do here")
            }
        }
    }

    fn draw_status(&self, buf: &mut Buffer, area: Rect) {
        let w = area.width as usize;
        let y = area.height as usize - 1;
        let text = if self.status.is_empty() {
            format!(" {}", self.say_keys(self.hint()))
        } else {
            format!(" {}", self.status)
        };
        put(buf, area, 0, y, w, &fit(&text, w), color::status());
    }

    fn draw_menu(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Mode::Menu(m) = &mut self.mode else { return };
        let widest = m.items.iter().map(|i| i.label.chars().count()).max().unwrap_or(0);
        let bw = (widest.max(m.title.chars().count() + 2).max(24) + 4).min(w.saturating_sub(2));
        let note = m.note.as_deref().map(|n| wrap(n, bw.saturating_sub(4))).unwrap_or_default();
        let below = if note.is_empty() { 0 } else { note.len() + 1 };
        let rows = m.items.len().min(h.saturating_sub(2 + below).max(1));
        if m.sel < m.top {
            m.top = m.sel;
        } else if m.sel >= m.top + rows {
            m.top = m.sel + 1 - rows;
        }
        let bh = rows + 2 + below;
        let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(bh) / 2);
        window(buf, area, x, y, bw, bh, &m.title, color::meta());
        for (r, (i, item)) in m.items.iter().enumerate().skip(m.top).take(rows).enumerate() {
            if item.action == Action::Line {
                put(buf, area, x + 1, y + 1 + r, bw - 2, &"─".repeat(bw - 2), color::meta());
                continue;
            }
            let mut st = if item.enabled { Style::new() } else { color::dim() };
            if i == m.sel {
                st = st.add_modifier(Modifier::REVERSED);
            }
            put(buf, area, x + 1, y + 1 + r, bw - 2, &fit(&format!(" {}", item.label), bw - 2), st);
        }
        for (k, line) in note.iter().enumerate() {
            put(buf, area, x + 2, y + 2 + rows + k, bw - 4, line, color::comment());
        }
    }

    fn draw_help(&self, buf: &mut Buffer, area: Rect) {
        let Mode::Help { lines, .. } = &self.mode else { return };
        let (w, h) = (area.width as usize, area.height as usize);
        let bw = (w * 2 / 3).clamp(30.min(w), 72.min(w));
        let mut text: Vec<(String, Style)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            // What enter does is what is asked about: it comes first.
            let st = if i == 0 { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
            text.extend(wrap(l, bw.saturating_sub(4)).into_iter().map(|l| (l, st)));
        }
        let bh = (text.len() + 2).min(h);
        let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(bh) / 2);
        window(buf, area, x, y, bw, bh, "what can I do here", color::meta());
        for (k, (l, st)) in text.iter().take(bh.saturating_sub(2)).enumerate() {
            put(buf, area, x + 2, y + 1 + k, bw - 4, l, *st);
        }
    }

    /// How many lines the editor box takes in `area`, if a comment is
    /// being written in `pane`.
    fn editor_height(&self, area: Rect, pane: Focus) -> usize {
        match &self.mode {
            Mode::Edit(e) if e.pane == pane => {
                let (w, h) = (area.width as usize, area.height as usize);
                (e.ed.height(w.saturating_sub(2)) + 2).clamp(4, (h / 2).max(4))
            }
            _ => 0,
        }
    }

    /// The editor box for the comment being written in `pane`, if one is,
    /// at `y` of `area`: drawn, and where the terminal's cursor goes.
    fn draw_editor(&self, buf: &mut Buffer, area: Rect, y: usize, pane: Focus) -> Option<(u16, u16)> {
        let Mode::Edit(e) = &self.mode else { return None };
        if e.pane != pane {
            return None;
        }
        let (w, h) = (area.width as usize, area.height as usize);
        let bh = self.editor_height(area, pane);
        let y = y.min(h.saturating_sub(bh));
        let title = match &e.target {
            Target::Stack => "a comment on the stack".to_string(),
            Target::Patch(_) => "a comment on the patch".to_string(),
            Target::Line { preview, .. } => format!("a comment on: {}", short(preview, 30)),
            Target::Reply { .. } => "a reply".to_string(),
            Target::About(rev) => format!("a question on @{rev}, for the agent to answer"),
        };
        Some(e.ed.draw(buf, area, 0, y, w, bh, &title))
    }

    fn draw_list(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let w = area.width as usize;
        // The header: the stack's own place.
        let own = self.db.notes(&stack_change(&self.stack)).len();
        let header = match self.current() {
            Some(s) => {
                let mut t = format!(" ≡ {} · {} patch{}", self.stack, s.patches.len(), if s.patches.len() == 1 { "" } else { "es" });
                if own > 0 {
                    t += &format!(" · {own} comment{}", plural(own));
                }
                t
            }
            None => format!(" ≡ {}", if self.stack.is_empty() { "no stack" } else { &self.stack }),
        };
        let mut st = color::header();
        if self.on_stack {
            st = if self.focus == Focus::List { st.add_modifier(Modifier::REVERSED) } else { st.add_modifier(Modifier::UNDERLINED) };
        }
        put(buf, area, 0, 0, w, &fit(&header, w), st);
        let Some(s) = self.current().cloned() else {
            let say = if self.db.names().contains(&self.stack.as_str()) {
                format!("{} is deleted: Undo, in the menu, brings it back", self.stack)
            } else {
                "no stack yet: `stacked import NAME main..HEAD` makes one".to_string()
            };
            for (y, line) in wrap(&say, w.saturating_sub(2)).iter().enumerate() {
                put(buf, area, 1, y + 1, w, line, color::dim());
            }
            return None;
        };
        let editing = match &self.mode {
            Mode::Edit(e) if e.pane == Focus::List => Some(e.target.clone()),
            _ => None,
        };
        // A comment being written goes under what it is on, the patches
        // below it further down.
        let edit_h = self.editor_height(area, Focus::List);
        let mut cursor = None;
        let mut y = 1;
        if editing == Some(Target::Stack) {
            cursor = self.draw_editor(buf, area, y, Focus::List);
            y += edit_h;
        }
        if s.patches.is_empty() {
            put(buf, area, 1, y, w, "an empty stack", color::dim());
        }
        // Being moved, the patch shows where it would go.
        let mut order: Vec<usize> = (0..s.patches.len()).collect();
        let (on, moving) = match self.mode {
            Mode::Moving { from, to } => {
                let p = order.remove(from);
                order.insert(to, p);
                (to, true)
            }
            _ => (self.sel, false),
        };
        let h = area.height as usize;
        let top = on.saturating_sub(h.saturating_sub(2 + edit_h));
        for (at, &i) in order.iter().enumerate().skip(top) {
            if y >= h {
                break;
            }
            // Reading, what is read of it: ✓ when all of it is.
            let mut tag = match self.reading.then(|| self.read_of(&s.patches[i])) {
                Some((n, m)) if m > 0 && n == m => " ✓".to_string(),
                Some((n, m)) if m > 0 => format!(" {n}/{m}"),
                _ => String::new(),
            };
            let p = self.db.patch(&s.patches[i]);
            let open = self
                .db
                .notes(&p.change)
                .iter()
                .filter(|n| last_role(&n.text) != Some(Role::Agent))
                .count();
            if open > 0 {
                tag += &format!(" [{open}]");
            }
            let mark = if moving && at == on { " ↕".to_string() } else { format!("{:>2}", at + 1) };
            let text = format!(" {mark} {}", p.title());
            let room = w.saturating_sub(tag.chars().count());
            let line = format!("{}{tag}", fit(&text, room));
            let mut st = Style::new();
            if at == on && !self.on_stack {
                st = if self.focus == Focus::List || moving {
                    st.add_modifier(Modifier::REVERSED)
                } else {
                    st.add_modifier(Modifier::BOLD)
                };
            }
            put(buf, area, 0, y, w, &line, st);
            y += 1;
            if editing == Some(Target::Patch(p.change.clone())) {
                cursor = self.draw_editor(buf, area, y, Focus::List);
                y += edit_h;
            }
        }
        cursor
    }

    fn draw_patch(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let (w, h) = (area.width as usize, area.height as usize);
        self.mid_w = w;
        let layout = self.drawn_layout(w);
        let half = w.saturating_sub(3) / 2;
        let margin = if self.reading && !self.on_stack { MARGIN_W } else { 0 };
        let fold = match layout {
            Layout::Unified => w,
            _ => half,
        };
        self.rebuild(fold.saturating_sub(margin + 1));
        if self.rows.is_empty() {
            return None;
        }
        // Reading: what Space would mark read at the cursor, or else what
        // Enter would unmark, marked in the margin.
        let ticks = (margin > 0).then(|| {
            self.blocks();
            match self.reading_at() {
                Some((Some(t), _)) => (Tick::ToRead, t.new),
                Some((None, Some((_, gone)))) => (Tick::ToUnread, gone),
                _ => (Tick::ToRead, HashSet::new()),
            }
        });
        let ticks = ticks.as_ref();
        // Where a comment being written here goes: under its line, or the
        // comment it answers, which then shows.
        let anchor = self.editor_anchor();
        let lit = |i: usize, app: &App| i == app.cursor && app.focus != Focus::List;
        // Each row drawn: the line of the pane it is on, and where it starts.
        let mut drawn: Vec<(usize, usize, usize)> = Vec::new();
        match layout {
            Layout::Unified => {
                if let Some(a) = anchor
                    && (a < self.top || a + 1 >= self.top + h)
                {
                    self.top = a.saturating_sub(h / 3);
                }
                if anchor.is_none() {
                    if self.cursor < self.top {
                        self.top = self.cursor;
                    } else if self.cursor >= self.top + h {
                        self.top = self.cursor + 1 - h;
                    }
                }
                for y in 0..h {
                    let i = self.top + y;
                    if i >= self.rows.len() {
                        break;
                    }
                    self.draw_row(buf, area, (0, y, w), i, lit(i, self), ticks);
                    drawn.push((i, y, 0));
                }
            }
            Layout::Split => {
                let (lines, index) = split_lines(&self.rows);
                let cur = *index.get(&anchor.unwrap_or(self.cursor)).unwrap_or(&0);
                let mut top = *index.get(&self.top).unwrap_or(&0);
                if cur < top {
                    top = cur;
                } else if cur >= top + h {
                    top = cur + 1 - h;
                }
                self.top = lines.get(top).map_or(0, split_row);
                for y in 0..h {
                    let Some(line) = lines.get(top + y) else { break };
                    if let Some(i) = line.span {
                        self.draw_row(buf, area, (0, y, w), i, lit(i, self), ticks);
                        drawn.push((i, y, 0));
                        continue;
                    }
                    put(buf, area, half + 1, y, 1, "│", color::meta());
                    for (x, idx) in [(0, line.left), (half + 2, line.right)] {
                        match idx {
                            None => put(buf, area, x, y, half, &fit("~", half), color::dim()),
                            Some(i) => {
                                self.draw_row(buf, area, (x, y, half), i, lit(i, self), ticks);
                                drawn.push((i, y, x));
                            }
                        }
                    }
                }
            }
            Layout::Compact => {
                let p = self.panes.clone();
                // Moved by row (to a comment, to a comment being written):
                // the panes go there.
                if let Some(a) = anchor {
                    self.compact.resync(&p, a, h);
                } else {
                    self.compact.resync(&p, self.cursor, h);
                }
                if anchor.is_none()
                    && let Some(row) = self.compact.settle(&p, h)
                {
                    self.cursor = row;
                }
                let scroll = self.compact.scroll;
                for (y, (l, r)) in self.compact.lines(&p, h).into_iter().enumerate() {
                    let on = y == self.compact.y && self.focus != Focus::List && anchor.is_none();
                    if l.is_none() && r.is_none() {
                        continue;
                    }
                    if l == r && l.is_some_and(|i| spans(self.rows[i].kind)) {
                        let i = l.unwrap();
                        self.draw_row(buf, area, (0, y, w), i, on, ticks);
                        drawn.push((i, y, 0));
                        continue;
                    }
                    put(buf, area, half + 1, y, 1, "│", color::meta());
                    // Both scrolling, the line is lit across; one alone, its
                    // side, or wherever the cursor's row is.
                    let (mut lit_l, mut lit_r) = (on, on);
                    if on && scroll != Scroll::Both {
                        let left = scroll == Scroll::Left;
                        if (if left { l } else { r }) == Some(self.cursor) {
                            (lit_l, lit_r) = (left, !left);
                        } else {
                            (lit_l, lit_r) = (l == Some(self.cursor), r == Some(self.cursor));
                        }
                    }
                    for (x, idx, sel) in [(0, l, lit_l), (half + 2, r, lit_r)] {
                        if let Some(i) = idx {
                            self.draw_row(buf, area, (x, y, half), i, sel, ticks);
                            drawn.push((i, y, x));
                        }
                    }
                }
            }
        }
        if let Some(a) = anchor {
            let y = drawn.iter().find(|d| d.0 == a).map_or(h / 3, |d| d.1);
            return self.draw_editor(buf, area, y + 1, Focus::Diff);
        }
        self.draw_preview(buf, area, &drawn);
        None
    }

    /// Row `i` at `(x, y)`, `w` wide: in the reading mode after its margin,
    /// a check if it is read, or what Space or Enter would make of it.
    fn draw_row(
        &self,
        buf: &mut Buffer,
        area: Rect,
        (x, y, w): (usize, usize, usize),
        i: usize,
        selected: bool,
        ticks: Option<&(Tick, HashSet<String>)>,
    ) {
        let row = &self.rows[i];
        let mut x0 = x;
        if let Some((tick, keys)) = ticks {
            let changed = matches!(row.kind, Kind::Add | Kind::Del);
            let (text, st) = match row.key.as_ref() {
                Some(k) if changed && keys.contains(k) => match tick {
                    Tick::ToRead => ("○→✓ ", color::add().add_modifier(Modifier::BOLD)),
                    Tick::ToUnread => ("✓→○ ", color::del().add_modifier(Modifier::BOLD)),
                },
                _ if row.read => ("  ✓ ", color::add()),
                _ => ("    ", Style::new()),
            };
            put(buf, area, x, y, MARGIN_W.min(w), text, st);
            x0 += MARGIN_W;
        }
        let tw = w.saturating_sub(x0 - x);
        put(buf, area, x0, y, tw, &fit(&row.text, tw), row_style(row, selected));
    }

    /// The last row of what a comment being written in the middle goes
    /// under: its line and the comments on it, or the comment it answers.
    fn editor_anchor(&self) -> Option<usize> {
        let Mode::Edit(e) = &self.mode else { return None };
        if e.pane != Focus::Diff {
            return None;
        }
        let first = match &e.target {
            Target::Line { key, .. } => self.rows.iter().position(|r| r.key.as_deref() == Some(key) && r.note.is_none())?,
            Target::Reply { id, .. } => self.rows.iter().position(|r| r.note.as_deref() == Some(id))?,
            _ => return None,
        };
        let mut last = first;
        while self.rows.get(last + 1).is_some_and(|r| matches!(r.kind, Kind::Comment | Kind::Stream)) {
            if let Target::Reply { id, .. } = &e.target
                && self.rows[last + 1].note.as_deref() != Some(id)
            {
                break;
            }
            last += 1;
        }
        Some(last)
    }

    /// With the cursor in the comments pane, its comment in the middle:
    /// marked where it is if the middle shows it; else its thread over the
    /// top or the bottom, as it is above or below what the middle shows.
    /// The middle itself does not move.
    fn draw_preview(&self, buf: &mut Buffer, area: Rect, drawn: &[(usize, usize, usize)]) {
        if self.focus != Focus::Notes || self.notes_head {
            return;
        }
        let (w, h) = (area.width as usize, area.height as usize);
        let notes = self.notes();
        let Some((head, id)) = notes.get(self.note_sel) else { return };
        let head = *head;
        let end = (head..self.rows.len())
            .take_while(|&i| self.rows[i].note.as_ref() == Some(id))
            .last()
            .unwrap_or(head);
        let Some(first) = drawn.iter().map(|d| d.0).min() else {
            return;
        };
        if drawn.iter().any(|d| (head..=end).contains(&d.0)) {
            for &(i, y, x) in drawn {
                if (head..=end).contains(&i) {
                    put(buf, area, x, y, 1, "▶", by_style(self.rows[i].by.as_ref()).add_modifier(Modifier::BOLD));
                }
            }
            return;
        }
        let above = end < first;
        // The line it is on, then its thread, as much as a third of the pane.
        let mut rows: Vec<&Row> = Vec::new();
        if let Some(a) = head.checked_sub(1).filter(|&a| self.rows[a].note.is_none() && self.rows[a].key.is_some()) {
            rows.push(&self.rows[a]);
        }
        rows.extend(&self.rows[head..=end]);
        let room = (h / 3).max(3).min(h).saturating_sub(1);
        rows.truncate(room);
        let bh = rows.len() + 1;
        let y0 = if above { 0 } else { h.saturating_sub(bh) };
        let n = notes.len();
        let rule = self.say_keys(if above {
            format!("▲ comment {} of {n}, above · enter: go to it ", self.note_sel + 1)
        } else {
            format!("▼ comment {} of {n}, below · enter: go to it ", self.note_sel + 1)
        });
        let rule = format!("{rule}{}", "─".repeat(w.saturating_sub(rule.chars().count())));
        let (rule_y, rows_y) = if above { (y0 + rows.len(), y0) } else { (y0, y0 + 1) };
        put(buf, area, 0, rule_y, w, &rule, color::meta());
        for (k, row) in rows.iter().enumerate() {
            put(buf, area, 0, rows_y + k, w, &fit(&row.text, w), row_style(row, false));
        }
    }

    /// The comments of what the middle shows, under a header: two lines
    /// each, a mark for where it stands, who and where, then its last word.
    fn draw_notes(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let (w, h) = (area.width as usize, area.height as usize);
        let change = self.subject_change()?;
        let notes = self.notes();
        let busy = self.busy();
        let all: Vec<Note> = notes.iter().filter_map(|(_, id)| self.db.note(&change, id).cloned()).collect();
        let with = all.iter().filter(|n| busy.contains_key(&(n.change.clone(), n.id.clone()))).count();
        let to_read = all.iter().filter(|n| self.shows(Show::ToRead, n, &busy, false)).count();
        let total = self.subject_notes().len();
        let mut header = if self.show == Show::All {
            format!(" {} comment{}", all.len(), plural(all.len()))
        } else {
            format!(" {} · {} of {total}", self.show.name(), all.len())
        };
        if with > 0 {
            header += &format!(" · {with} with the agent");
        }
        if to_read > 0 {
            header += &format!(" · {to_read} to read");
        }
        let mut st = color::header();
        if self.focus == Focus::Notes && self.notes_head {
            st = st.add_modifier(Modifier::REVERSED);
        }
        put(buf, area, 0, 0, w, &fit(&header, w), st);
        // The one the middle's cursor is in, when the cursor is there.
        let lit = match self.focus {
            Focus::Notes if !self.notes_head => Some(self.note_sel),
            Focus::Notes => None,
            _ => self.note_at_cursor(),
        };
        let editing = match &self.mode {
            Mode::Edit(e) if e.pane == Focus::Notes => Some(e.target.clone()),
            _ => None,
        };
        let per = 2;
        let edit_h = self.editor_height(area, Focus::Notes);
        let fits = ((h.saturating_sub(1 + edit_h)) / per).max(1);
        let on = lit.unwrap_or(0);
        let first = on.saturating_sub(fits.saturating_sub(1));
        let mut cursor = None;
        let mut y = 1;
        for (i, n) in all.iter().enumerate().skip(first) {
            if y + 1 >= h {
                break;
            }
            let who = n.by.as_deref().unwrap_or("you");
            let mark = if busy.contains_key(&(n.change.clone(), n.id.clone())) {
                "●"
            } else if n.queued.is_some() {
                "◌"
            } else if n.failed.is_some() {
                "✗"
            } else if n.moved.is_some() {
                "↑"
            } else if last_role(&n.text) == Some(Role::Agent) {
                "✓"
            } else {
                " "
            };
            let last = thread_turns(&n.text)
                .last()
                .map(|t| t.body.lines().next().unwrap_or("").to_string())
                .unwrap_or_default();
            let mut head = by_style(Some(&by_of(n)));
            let mut tail = color::dim();
            if lit == Some(i) {
                let m = if self.focus == Focus::Notes { Modifier::REVERSED } else { Modifier::BOLD };
                head = head.add_modifier(m);
                tail = Style::new().add_modifier(m);
            }
            let at = match n.about {
                Some(rev) => format!("history @{rev}"),
                None if n.on_stack() => "the stack".to_string(),
                None => format!("{}: {}", n.file, n.preview),
            };
            put(buf, area, 0, y, w, &fit(&format!("{mark} {who} · {at}"), w), head);
            put(buf, area, 0, y + 1, w, &fit(&format!("   {last}"), w), tail);
            y += per;
            if editing == Some(Target::Reply { change: n.change.clone(), id: n.id.clone() }) {
                cursor = self.draw_editor(buf, area, y, Focus::Notes);
                y += edit_h;
            }
        }
        cursor
    }

    fn list_top(&mut self, h: usize) -> usize {
        if self.pick < self.pick_top {
            self.pick_top = self.pick;
        } else if self.pick >= self.pick_top + h {
            self.pick_top = self.pick + 1 - h;
        }
        self.pick_top
    }

    fn draw_tree(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let lines = tree::graph(&self.db, &self.stack);
        let top = self.list_top(h);
        for (y, (i, l)) in lines.iter().enumerate().skip(top).take(h).enumerate() {
            let s = self.db.node(l.rev).unwrap();
            let text = format!(" {}@{} {}", l.lanes, l.rev, s.op);
            let st = if i == self.pick { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
            put(buf, area, 0, y, w, &fit(&text, w), st);
        }
    }

    fn draw_stacks(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let top = self.list_top(h);
        let stacks: Vec<(String, usize)> = self.db.stacks().iter().map(|s| (s.name.clone(), s.patches.len())).collect();
        for (y, (i, (name, n))) in stacks.iter().enumerate().skip(top).take(h).enumerate() {
            let text = format!(" {name} ({n} patches)");
            let st = if i == self.pick { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
            put(buf, area, 0, y, w, &fit(&text, w), st);
        }
    }

    /// The stack the tree or the stack list is on: what it holds, how it
    /// came to be, and what its edit changed.
    fn draw_node(&self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let node = match self.screen {
            Screen::Tree => tree::graph(&self.db, &self.stack).get(self.pick).map(|l| l.rev),
            _ => self
                .db
                .stacks()
                .get(self.pick)
                .and_then(|s| self.db.head(&s.name))
                .map(|(r, _)| r),
        };
        let Some(rev) = node else { return };
        let Ok(s) = self.db.node(rev) else { return };
        let mut lines: Vec<(String, Style)> = vec![
            (format!(" @{rev} {} · {}", s.name, s.op), color::file()),
            (
                format!(
                    " {}{}",
                    local_time(s.time),
                    s.parent.map_or(String::new(), |p| format!(" · edited from @{p}"))
                ),
                color::dim(),
            ),
            (String::new(), Style::new()),
        ];
        for (i, id) in s.patches.iter().enumerate() {
            lines.push((format!(" {:>2} {}", i + 1, self.db.patch(id).title()), Style::new()));
        }
        if self.screen == Screen::Tree {
            lines.push((String::new(), Style::new()));
            lines.push((self.say_keys(" what this edit changed (space: in full)".into()), color::meta()));
            for (l, st) in self.what_changed(rev).into_iter().skip(1) {
                if !l.starts_with("    ") {
                    lines.push((format!(" {l}"), st));
                }
            }
        }
        for (y, (l, st)) in lines.iter().take(h).enumerate() {
            put(buf, area, 0, y, w, &fit(l, w), *st);
        }
    }

    fn draw_versions(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let list = self.versions_list();
        let top = self.list_top(h);
        let current = self.current().map(|s| s.patches.clone()).unwrap_or_default();
        for (y, (i, (p, first))) in list.iter().enumerate().skip(top).take(h).enumerate() {
            // The one in the stack, marked; each with the edit that made it.
            let here = if current.contains(&p.id) { "●" } else { " " };
            let made = first.as_ref().map_or("never in the stack".to_string(), |(r, s)| format!("@{r} {}", s.op));
            let text = format!(" v{}{here} {made}", i + 1);
            let st = if i == self.pick { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
            put(buf, area, 0, y, w, &fit(&text, w), st);
        }
    }

    /// The version picked: what changed from the version before, or the
    /// whole of it.
    fn draw_version(&self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let list = self.versions_list();
        let Some((p, first)) = list.get(self.pick) else { return };
        let text = match (self.whole, self.pick.checked_sub(1).and_then(|i| list.get(i))) {
            (false, Some((before, _))) => ops::interdiff(before, p),
            _ => ops::render(p),
        };
        let say = if self.whole || self.pick == 0 { "the version whole" } else { "what changed from the version before" };
        let made = first
            .as_ref()
            .map_or("never in the stack".to_string(), |(r, s)| format!("@{r} {} · {}", local_time(s.time), s.op));
        let here = if self.current().is_some_and(|s| s.patches.contains(&p.id)) { ", in the stack" } else { "" };
        put(buf, area, 0, 0, w, &fit(&format!(" v{}{here} · {say} · {made}", self.pick + 1), w), color::file());
        for (y, l) in text.lines().take(h.saturating_sub(1)).enumerate() {
            put(buf, area, 0, y + 1, w, &fit(l, w), diff_style(l));
        }
    }

    fn draw_history(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let (w, h) = (area.width as usize, area.height as usize);
        let events = self.history();
        // On the record it was on, wherever it now is.
        if let Some(p) = self.hist_rev.and_then(|r| events.iter().position(|e| e.rev == r)) {
            self.pick = p;
        }
        self.pick = self.pick.min(events.len().saturating_sub(1));
        self.hist_rev = events.get(self.pick).map(|e| e.rev);
        put(buf, area, 0, 0, w, &fit(&format!(" the history of {}, newest first", self.stack), w), color::file());
        let busy = self.busy();
        // Each record, then the comments on it, each its last word and where
        // it stands.
        let mut lines: Vec<(Option<usize>, String, Style)> = Vec::new();
        for (i, e) in events.iter().enumerate() {
            lines.push((Some(i), format!(" {}{} {}", e.lanes, local_time(e.time), e.text), Style::new()));
            for n in self.about(e.rev) {
                let last = thread_turns(&n.text)
                    .last()
                    .map(|t| {
                        let who = match t.role {
                            Role::You => "you",
                            Role::Agent => "agent",
                            Role::Reviewer => "reviewer",
                        };
                        format!("{who}: {}", t.body.lines().next().unwrap_or(""))
                    })
                    .unwrap_or_default();
                let state = self.tag(&n, &busy);
                let state = if state.is_empty() { String::new() } else { format!(" · {state}") };
                lines.push((None, format!(" {}   └ {last}{state}", e.under), by_style(Some(&by_of(&n)))));
            }
        }
        let editing = match &self.mode {
            Mode::Edit(e) => matches!(e.target, Target::About(_)),
            _ => false,
        };
        let edit_h = if editing { self.editor_height(area, Focus::Diff) } else { 0 };
        // The picked record in sight, with what is written under it.
        let h = h.saturating_sub(1);
        let at = lines.iter().position(|l| l.0 == Some(self.pick)).unwrap_or(0);
        let below = lines[at + 1..].iter().take_while(|l| l.0.is_none()).count();
        if at < self.pick_top {
            self.pick_top = at;
        } else if at + below + edit_h + 1 > self.pick_top + h {
            self.pick_top = (at + below + edit_h + 1).saturating_sub(h);
        }
        let mut cursor = None;
        let mut y = 1;
        for (k, (i, text, st)) in lines.iter().enumerate().skip(self.pick_top) {
            if y > h {
                break;
            }
            let st = if *i == Some(self.pick) { st.add_modifier(Modifier::REVERSED) } else { *st };
            put(buf, area, 0, y, w, &fit(text, w), st);
            y += 1;
            if editing && k == at + below {
                cursor = self.draw_editor(buf, area, y, Focus::Diff);
                y += edit_h;
            }
        }
        cursor
    }

    /// The stack as it was at the record picked: which, and its patches.
    fn draw_history_stack(&self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Some((rev, s)) = self.hist_state() else {
            put(buf, area, 1, 0, w, "no stack then", color::dim());
            return;
        };
        let mut head = color::header();
        if self.hist_focus == HistFocus::Stack {
            head = head.add_modifier(Modifier::UNDERLINED);
        }
        put(buf, area, 0, 0, w, &fit(&format!(" @{rev} · {}", s.op), w), head);
        put(buf, area, 0, 1, w, &fit(&format!(" as it was, {}", local_time(s.time)), w), color::dim());
        if s.patches.is_empty() {
            put(buf, area, 1, 2, w, "an empty stack", color::dim());
        }
        let rows = h.saturating_sub(2);
        let top = self.hist_patch.saturating_sub(rows.saturating_sub(1));
        for (y, (i, id)) in s.patches.iter().enumerate().skip(top).take(rows).enumerate() {
            let mut st = Style::new();
            if i == self.hist_patch {
                st = st.add_modifier(if self.hist_focus == HistFocus::Stack { Modifier::REVERSED } else { Modifier::BOLD });
            }
            put(buf, area, 0, y + 2, w, &fit(&format!(" {:>2} {}", i + 1, self.db.patch(id).title()), w), st);
        }
    }

    /// The patch picked, as it was, its comments as they stood: only to
    /// look at.
    fn draw_history_patch(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Some(e) = self.history().into_iter().nth(self.pick) else { return };
        let Some((_, s)) = self.hist_state() else { return };
        let Some(id) = s.patches.get(self.hist_patch.min(s.patches.len().saturating_sub(1))).cloned() else {
            return;
        };
        let want = format!("{id}\0{}\0{w}\0{}", e.rev, self.db.changes());
        if self.hist_built.as_deref() != Some(&want) {
            let p = self.db.patch(&id).clone();
            let notes = crate::history::notes_at(&self.db, &p.change, e.rev);
            let refs: Vec<&Note> = notes.iter().collect();
            self.hist_rows = patch_rows(&p, &refs, w.saturating_sub(1), &Deco::default());
            self.hist_built = Some(want);
            self.hist_cursor = self.hist_cursor.min(self.hist_rows.len().saturating_sub(1));
        }
        let focus = self.hist_focus == HistFocus::Patch;
        let rows = h;
        if self.hist_cursor < self.hist_top {
            self.hist_top = self.hist_cursor;
        } else if self.hist_cursor >= self.hist_top + rows {
            self.hist_top = self.hist_cursor + 1 - rows;
        }
        for y in 0..rows {
            let i = self.hist_top + y;
            let Some(row) = self.hist_rows.get(i) else { break };
            put(buf, area, 0, y, w, &fit(&row.text, w), row_style(row, focus && i == self.hist_cursor));
        }
    }

    fn draw_text(&self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Some(t) = &self.text else { return };
        put(buf, area, 0, 0, w, &fit(&format!(" {}", t.title), w), color::file());
        for (y, (l, st)) in t.lines.iter().skip(t.scroll).take(h.saturating_sub(1)).enumerate() {
            put(buf, area, 0, y + 1, w, &fit(l, w), *st);
        }
    }

    fn draw_runs(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let top = self.list_top(h);
        let lines: Vec<(String, Style)> = self
            .runs_newest()
            .iter()
            .map(|r| {
                let state = if ask::running(r) {
                    "running".to_string()
                } else {
                    format!("{:?}", r.state).to_lowercase()
                };
                let req = if r.request.is_empty() { String::new() } else { format!(" \"{}\"", r.request) };
                let st = match r.state {
                    RunState::Failed => color::del(),
                    RunState::Done => Style::new(),
                    RunState::Started => color::comment(),
                };
                (
                    format!(
                        " {} {:<7} {} {}{req} · {}",
                        local_time(r.time),
                        state,
                        r.what,
                        r.stack,
                        r.summary
                    ),
                    st,
                )
            })
            .collect();
        if lines.is_empty() {
            put(buf, area, 1, 0, w, "no runs yet", color::dim());
        }
        for (y, (i, (text, st))) in lines.iter().enumerate().skip(top).take(h).enumerate() {
            let st = if i == self.pick { st.add_modifier(Modifier::REVERSED) } else { *st };
            put(buf, area, 0, y, w, &fit(text, w), st);
        }
    }

    fn draw_transcript(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Some(id) = &self.transcript else { return };
        let mut lines = ask::tail(&self.db, id, usize::MAX);
        if let Some(r) = self.db.run(id) {
            if !r.output.is_empty() {
                lines.push(String::new());
                lines.extend(r.output.lines().map(String::from));
            }
            if !r.summary.is_empty() {
                lines.push(String::new());
                lines.push(format!("→ {}", r.summary));
            }
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(h));
        let end = lines.len() - self.scroll;
        let start = end.saturating_sub(h);
        for (y, l) in lines[start..end].iter().enumerate() {
            put(buf, area, 1, y, w - 1, l, Style::new());
        }
    }
}

/// The comments claude is working on, and the way it was asked and the run.
pub type Busy = HashMap<(String, String), (Way, String)>;

/// Whether the filter `show` lets `n` be shown: `gone` when its line is
/// not in its patch's version in the stack.
pub fn shows(show: Show, n: &Note, busy: &Busy, gone: bool) -> bool {
    let with = busy.contains_key(&(n.change.clone(), n.id.clone()));
    let last = last_role(&n.text);
    let to_read = last == Some(Role::Agent) && !with && n.moved.is_none() && n.queued.is_none();
    match show {
        Show::All => true,
        Show::Nothing => false,
        Show::ToSend => !with && ask::sendable(n),
        Show::ToRead => to_read,
        Show::ToReadAnswer => to_read && last_reply_way(&n.text).as_deref() == Some("answer"),
        Show::ToReadChange => to_read && last_reply_way(&n.text).as_deref() == Some("change"),
        Show::InProgress => with || n.queued.is_some(),
        Show::Failed => n.failed.is_some() || n.moved.is_some(),
        Show::Gone => gone,
    }
}

/// Where a comment moved to stack `name` came from, said.
pub fn where_was(db: &Db, name: &str, m: &crate::db::Moved) -> String {
    let at = db
        .stack(name)
        .ok()
        .and_then(|s| s.patches.iter().position(|id| db.patch(id).change == m.change))
        .map_or("a patch".to_string(), |i| format!("patch {}", i + 1));
    format!("{at} · {}: {}", m.file, m.preview)
}

/// What a comment's first line says of where it stands: with claude,
/// ready, failed, replied, moved, and what its answer opened.
pub fn note_state(db: &Db, name: &str, n: &Note, busy: &Busy) -> String {
    let mut parts = Vec::new();
    if let Some((way, _)) = busy.get(&(n.change.clone(), n.id.clone())) {
        parts.push(format!("with the agent ({})", way.said()));
    } else if let Some(q) = &n.queued {
        let on = ask::waits_on(db, name, q.way).unwrap_or_else(|| "next".into());
        parts.push(format!("ready ({}), {on}", q.way.said()));
    } else if let Some(f) = &n.failed {
        parts.push(format!("ask failed: {}", f.split_whitespace().collect::<Vec<_>>().join(" ")));
    } else if last_role(&n.text) == Some(Role::Agent) && n.moved.is_none() {
        let way = last_reply_way(&n.text).unwrap_or_default();
        parts.push(format!("agent replied ({})", way_said(&way)));
    }
    if let Some(m) = &n.moved {
        parts.push(format!("moved from {}", where_was(db, name, m)));
    }
    let opened = db.opened_from(name, &n.change, &n.id);
    if !opened.is_empty() {
        let replied = opened.iter().filter(|o| last_role(&o.text) == Some(Role::You)).count();
        let mut t = format!("opened {} comment{}", opened.len(), plural(opened.len()));
        if replied > 0 {
            t += &format!(", {replied} with your replies");
        }
        parts.push(t);
    }
    parts.join(" · ")
}

/// Where a run's scope is, said: for the transcript's title.
pub fn scope_said(db: &Db, name: &str, s: &Scope) -> String {
    match s {
        Scope::Stack => "the stack".into(),
        Scope::Patch(c) => db
            .stack(name)
            .ok()
            .and_then(|st| st.patches.iter().position(|id| db.patch(id).change == *c))
            .map_or("a patch".into(), |i| format!("patch {}", i + 1)),
    }
}

pub fn run(path: &Path) -> Result<()> {
    let mut app = App::new(path)?;
    app.launch();
    let mut terminal = ratatui::init();
    let res = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    let running = app.running();
    if running > 0 {
        eprintln!("stacked: {running} of the agent's asks still running; what they do lands on its own");
    }
    res
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    let io = |e: std::io::Error| crate::db::Error(e.to_string());
    let mut launched = Instant::now();
    loop {
        app.refresh();
        // What waits goes, and what an agent left lands, away from the
        // screen.
        if launched.elapsed() > Duration::from_secs(5) {
            app.launch();
            launched = Instant::now();
        }
        terminal
            .draw(|f| {
                if let Some((x, y)) = app.draw(f.buffer_mut()) {
                    f.set_cursor_position((x, y));
                }
            })
            .map_err(io)?;
        if !event::poll(Duration::from_millis(300)).map_err(io)? {
            continue;
        }
        let Event::Key(k) = event::read().map_err(io)? else {
            continue;
        };
        match app.key(k) {
            Effect::Quit => return Ok(()),
            Effect::None => {}
            e => {
                ratatui::restore();
                let out = app.effect(e);
                *terminal = ratatui::init();
                terminal.clear().map_err(io)?;
                if let Err(e) = out {
                    app.status = e.0;
                }
            }
        }
    }
}

#[allow(dead_code)]
fn _short_id(c: &str, i: &str) -> String {
    short_id(c, i)
}
