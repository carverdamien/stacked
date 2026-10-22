//! The editor: the current stack's patches on the left, under a header that
//! is the stack's own place; in the middle, the selected patch (unified or
//! side by side, comments under their lines) or, from the header, the stack
//! as a whole; on the right, the comments of what the middle shows, under a
//! header of their own, when there are any. Every change is appended to the
//! database; claude's asks run on their own and what they did shows up when
//! it lands.
//!
//! The keys are few: the arrows move (← on the patches makes them compact,
//! then hides them; → on the comments too), enter does what is to be done
//! where the cursor is, space is the menu of all that can be done there,
//! `?` says what that is, and esc goes back. Nothing else is a shortcut:
//! what the menu does, only the menu does. Comments are written in an
//! editor inline where they will show, and sent to claude from their menu.

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
use crate::paint::{color, fit, put, put_parts, window, wrap};
use crate::panes::{Compact, Panes, Scroll, pane_streams, spans};
use crate::reading::{Blocks, Finder, Reading, Target as Block};
use crate::review;
use crate::rows::{BAR, By, Deco, Kind, Row, by_of, patch_rows, split_lines, split_row, stack_rows};
use crate::thread::{
    REWIND_SAYS, Role, Turn, all_turns, append_turn, edit_turn, last_mine, last_mine_at, last_reply_way, last_role, replace_last_turn,
    rewind_from, rewound_lines, thread_turns, without_written,
};
use crate::tree;
use crate::view::{MSG_PATH, note_lines, title_key};

pub mod banner;
pub use banner::BannerLine;
pub mod places;
pub mod reviewing;
pub use reviewing::Reviewing;
pub mod searching;
pub use searching::Searching;

fn by_style(by: Option<&By>) -> Style {
    match by {
        Some(By::Agent) => color::agent(),
        Some(By::Reviewer) => color::reviewer(),
        _ => color::you(),
    }
}

/// Row `row` as the parts it is drawn in, each in its style: a turn of a
/// comment's thread has its bar in its author's colour, their name bold in
/// it, and what they said in the terminal's own colour; any other row is
/// one part.
pub(crate) fn row_parts(row: &Row, selected: bool) -> Vec<(String, Style)> {
    let st = row_style(row, selected);
    let bar = row.text.find(BAR).filter(|_| row.kind == Kind::Comment && row.turn.is_some());
    let Some(at) = bar else {
        return vec![(row.text.clone(), st)];
    };
    let (left, said) = row.text.split_at(at + BAR.len_utf8());
    let said_st = if row.who {
        st.add_modifier(Modifier::BOLD)
    } else {
        let mut plain = Style::new();
        if row.read {
            plain = plain.add_modifier(Modifier::DIM);
        }
        if selected {
            plain = plain.add_modifier(Modifier::REVERSED);
        }
        plain
    };
    // Read as markdown: the bar's indent, then each piece as it looks.
    if !row.who && !row.pieces.is_empty() {
        let indent = said.len().saturating_sub(row.pieces.iter().map(|(s, _)| s.len()).sum());
        let mut parts = vec![(left.to_string(), st), (said[..indent].to_string(), said_st)];
        parts.extend(row.pieces.iter().map(|(s, m)| (s.clone(), mark_style(*m, said_st))));
        return parts;
    }
    vec![(left.to_string(), st), (said.to_string(), said_st)]
}

/// How a piece of a comment's markdown looks, given how what was said
/// looks: emphasis by the terminal's own means, as what was said is in its
/// own colour, so that no comment reads as code; but code itself, in the
/// colour of the settings for it.
fn mark_style(m: crate::markdown::Mark, plain: Style) -> Style {
    use crate::markdown::Mark;
    match m {
        Mark::Code | Mark::Block => plain.patch(color::code()),
        Mark::Plain | Mark::Bullet => plain,
        Mark::Strong | Mark::Heading => plain.add_modifier(Modifier::BOLD),
        Mark::Emph => plain.add_modifier(Modifier::ITALIC),
        Mark::Link => plain.add_modifier(Modifier::UNDERLINED),
        Mark::Url | Mark::Quote | Mark::Rule => plain.add_modifier(Modifier::DIM),
        Mark::Mention => plain.add_modifier(Modifier::UNDERLINED),
        Mark::Broken => plain.patch(color::warn()).add_modifier(Modifier::UNDERLINED),
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
    if row.read || row.free {
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

/// A stack diff's lines, coloured as they say: added, dropped, a new
/// version and what changed in it, moved; the patches as they were, dim.
fn diff_lines(diff: &[Diffed]) -> Vec<(String, Style)> {
    diff.iter()
        .map(|d| {
            let st = match d {
                Diffed::Same => color::dim(),
                Diffed::Made | Diffed::Deleted => Style::new(),
                Diffed::Base { .. } => color::hunk(),
                Diffed::Patch { added: true, .. } => color::add(),
                Diffed::Patch { changed: true, .. } => color::hunk(),
                Diffed::Patch { was: Some(_), .. } => Style::new(),
                Diffed::Patch { .. } => color::dim(),
                Diffed::Dropped { .. } => color::del(),
                Diffed::Code(l) => diff_style(l),
            };
            (format!(" {}", d.text()), st)
        })
        .collect()
}

/// `line` cut into lines of at most `w` characters, between words where it
/// can be, those after the first indented by `indent`: a long line of a
/// stack diff, kept in its pane.
fn fold(line: &str, w: usize, indent: usize) -> Vec<String> {
    let mut rest: Vec<char> = line.chars().collect();
    if rest.len() <= w || w <= indent + 10 {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    while rest.len() > w {
        // The last space in the line's second half, else where it ends.
        let cut = match rest[..=w].iter().rposition(|c| *c == ' ') {
            Some(at) if at > w / 2 => at,
            _ => w,
        };
        out.push(rest[..cut].iter().collect::<String>().trim_end().to_string());
        rest.drain(..cut);
        while rest.first() == Some(&' ') {
            rest.remove(0);
        }
        if rest.is_empty() {
            return out;
        }
        rest.splice(0..0, std::iter::repeat_n(' ', indent));
    }
    out.push(rest.into_iter().collect());
    out
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

/// The panes of History…: the history, and the stack diff of the record
/// picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistFocus {
    #[default]
    List,
    Diff,
}

/// What a cell of the screen shows, for a click there: the stack's header,
/// or a patch of the list by its place; a row of the middle; the comments
/// pane's header, or a comment; an item of a menu, or of a list screen; a
/// line of History…'s stack diff; or a box over the rest (a menu, an editor,
/// help), where a click is on nothing but what is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Patch(Option<usize>),
    Row(usize),
    NotesHead,
    Note(usize),
    Item(usize),
    /// Choice `k` of menu item `i`.
    Choice(usize, usize),
    Pick(usize),
    HistRow(usize),
    Box,
    /// The banner saying what the agent's work on the stack is; line `i`
    /// of it, open.
    Banner,
    BannerLine(usize),
    /// Line `i` of a review's list.
    ReviewLine(usize),
    /// Match `i` of the search's; where it looks, its button.
    Found(usize),
    SearchScope,
}

/// That `spot` is drawn at `(x, y)` of `area`, `w` by `h`: what a click
/// there is on.
fn spot_in(spots: &std::cell::RefCell<Vec<(Rect, Spot)>>, area: Rect, (x, y, w, h): (usize, usize, usize, usize), spot: Spot) {
    let w = w.min((area.width as usize).saturating_sub(x));
    let h = h.min((area.height as usize).saturating_sub(y));
    if w > 0 && h > 0 {
        spots
            .borrow_mut()
            .push((Rect::new(area.x + x as u16, area.y + y as u16, w as u16, h as u16), spot));
    }
}

/// How the code is drawn, as self-review draws it: one column, the two
/// sides line by line, or the two sides each a pane of its own (compact).
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
    /// The review of what the agent brought back.
    Review,
    /// The search: what it looks for, and what it finds.
    Search,
}

/// What the comments pane shows, as self-review's filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    All,
    ToSend,
    /// Their reply from the agent waits for your review.
    ToReview,
    ToRead,
    ToReadAnswer,
    ToReadChange,
    InProgress,
    Failed,
    Gone,
    /// The comments closed, each as it was: none of the others.
    Closed,
    Nothing,
}

impl Show {
    pub const ALL: [Show; 11] = [
        Show::All,
        Show::ToSend,
        Show::ToReview,
        Show::ToRead,
        Show::ToReadAnswer,
        Show::ToReadChange,
        Show::InProgress,
        Show::Failed,
        Show::Gone,
        Show::Closed,
        Show::Nothing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Show::All => "all",
            Show::ToSend => "to send",
            Show::ToReview => "to review",
            Show::ToRead => "to read",
            Show::ToReadAnswer => "to read: read-only",
            Show::ToReadChange => "to read: write",
            Show::InProgress => "in progress",
            Show::Failed => "failed or moved",
            Show::Gone => "gone",
            Show::Closed => "closed",
            Show::Nothing => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Show> {
        // As a database kept it before comments were closed, not deleted.
        if s == "deleted" {
            return Some(Show::Closed);
        }
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
    /// Your last turn of a comment, written again.
    Edit { change: String, id: String },
    /// Another turn of a comment's thread, written again, whoever said it:
    /// `turn` of all its turns, those deleted too.
    Turn { change: String, id: String, turn: usize },
    /// A comment's conversation rewound from turn `from` of all its turns,
    /// yours: what is written takes the place of that turn and of every
    /// turn after it.
    Rewind { change: String, id: String, from: usize },
    /// A comment on the stack's history: on its record `rev`.
    About(usize),
    /// Your answer to reply `k` of the work run `run` holds, in its review:
    /// it goes back to the agent with it.
    Answer { run: String, k: usize },
}

/// A comment being written, in the pane it is written in.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub ed: Editor,
    pub target: Target,
    pub pane: Focus,
    /// The comment whose last turn is edited, taken off the agent's queue
    /// meanwhile: how it was on it. It goes back on if it is left as it
    /// was.
    pub held: Option<crate::db::Queued>,
    /// The references attached to the comment, as it is saved: those it
    /// had, as they are attached and detached meanwhile.
    pub attached: Vec<crate::refs::Attached>,
}

/// Lines of a file being named, to attach to the comment being written,
/// `back`: as a `refs` line says them, `a.txt:120-140,200-210 @2`.
#[derive(Debug, Clone, PartialEq)]
pub struct Lines {
    pub ed: Editor,
    pub back: Edit,
}

/// Where the keys go, over the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    /// All that can be done: ↑↓ to choose, enter to do it.
    Menu(Menu),
    /// A comment being written, inline.
    Edit(Box<Edit>),
    /// The patch at `from` being moved, now at `to`: ↑↓ to place it.
    Moving { from: usize, to: usize },
    /// HEAD of the checkout stacked runs in being moved, shown with `to` of
    /// the stack's patches: ↑↓ to place it, the checkout moved once Enter
    /// leaves the mode.
    MovingHead { to: usize },
    /// What can be done where the cursor is, said. The next key goes back
    /// to `back`, and does there what it does.
    Help { lines: Vec<String>, back: Box<Mode> },
    /// The stream of the comment claude is working on, opened in place;
    /// `up` lines up from its end.
    Stream { change: String, id: String, up: usize },
    /// The lines a reference points at, previewed: Enter goes there.
    Preview(Box<Preview>),
    /// Lines of a file being named, to attach to the comment being
    /// written: Enter attaches them, Esc goes back to the comment.
    Lines(Box<Lines>),
}

/// A reference's lines, previewed: `title` says which; `lines` are the
/// lines, numbered, `top` the first shown; `jump`, where Enter goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub title: String,
    pub lines: Vec<(String, Style)>,
    pub top: usize,
    pub jump: Jump,
}

/// Where a reference's preview goes: the line in its patch, when the
/// patch's diff has it; else the file whole, at the line, as it reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Jump {
    Line { change: String, key: String },
    File { title: String, lines: Vec<(String, Style)>, at: usize },
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
    /// Its choices, as radio buttons, one of them picked: ← → pick
    /// another, and Enter does the item with it.
    pub choices: Option<Choices>,
    /// Its label when the agent is asked now, and when later: an ask,
    /// under the row that picks when.
    pub labels: Option<(String, String)>,
    /// A box before its label, checked or not: Enter checks or unchecks it.
    pub check: Option<bool>,
}

/// An item's choices, drawn as radio buttons after its label, `[x]` on the
/// one picked. What is picked is kept per stack, in the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Choices {
    /// What they are a choice of, as kept: `edit`, `ask`, `write`.
    pub of: &'static str,
    /// What is picked, as the bar and `?` say it: `where`, `how`.
    pub what: &'static str,
    pub list: Vec<Choice>,
    pub picked: usize,
}

/// One of an item's choices.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// Its name, as kept.
    pub name: &'static str,
    /// As drawn.
    pub shown: String,
    /// What it is, as `?` says it.
    pub said: String,
    /// What Enter does with it picked, if not the item's own action.
    pub action: Option<Action>,
}

impl Choice {
    fn new(name: &'static str, shown: impl Into<String>, said: impl Into<String>, action: Option<Action>) -> Choice {
        Choice {
            name,
            shown: shown.into(),
            said: said.into(),
            action,
        }
    }
}

impl Choices {
    /// `list`, the stack's pick of `of` picked if there is one of it, else
    /// the first.
    fn new(of: &'static str, what: &'static str, list: Vec<Choice>, pick: Option<&str>) -> Choices {
        let picked = list.iter().position(|c| Some(c.name) == pick).unwrap_or(0);
        Choices { of, what, list, picked }
    }

    /// The one picked: its name as kept.
    pub fn pick(&self) -> &'static str {
        self.list[self.picked].name
    }
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
    /// HEAD of the checkout stacked runs in picked up to move, shown after
    /// the patch at `0`, else where it is.
    MoveHead(Option<usize>),
    /// The message of the checkout's commit that is the patch at `0` (each
    /// one that differs, if none) taken as the patch's own; or the patch's
    /// put on the commit.
    MessagesToStack(Option<usize>),
    MessagesToCheckout(Option<usize>),
    EditPatch,
    EditCode,
    EditAgain,
    Resolve,
    ThrowEdit,
    AskDrop,
    Drop,
    CommentStack,
    CommentPatch,
    CommentLine,
    Reply,
    CommentInEditor,
    /// Your last turn of the comment, written again: here, or in $EDITOR.
    EditMine,
    EditMineInEditor,
    /// The turn of the thread the cursor is on, of all its turns: written
    /// again, here or in $EDITOR.
    EditTurn(usize),
    EditTurnInEditor(usize),
    /// The conversation rewound from the turn the cursor is on, if it is
    /// yours, else from yours above it: here, or in $EDITOR.
    Rewind(usize),
    RewindInEditor(usize),
    /// The comment shown as one quiet line in the middle, or whole again.
    Collapse,
    Expand,
    /// The agent's reply that waits for review taken into its thread at
    /// once: of run `0`, its reply `1`.
    TakeReply(String, usize),
    /// The review of what run `0` brought back opened, on part `1`.
    Review(String, Option<crate::held::Part>),
    /// In the review: the part the cursor is on marked accepted (true) or
    /// rejected; every comment the work opened so, or every reply.
    MarkPart(bool),
    MarkOpened(bool),
    MarkReplies(bool),
    /// The review's marks saved; those made since it opened thrown away;
    /// the review closed, the parts not marked yet accepted or rejected
    /// if said.
    ReviewSave,
    ReviewDiscard,
    ReviewClose(Option<bool>),
    /// Asked first: the review closed, the parts not marked yet accepted
    /// (true), or rejected.
    AskReviewClose(bool),
    /// In the review: the comment the ask was about at place `0` marked to
    /// close with the review (`1`), or to keep.
    CloseAsking(usize, bool),
    /// In the review: the work sent back to the agent, with what you wrote
    /// in it; your answer to its reply `0` written.
    SendBack,
    Answer(usize),
    /// In the review: what you wrote there, on the list's line the cursor
    /// is on, written again, or taken back.
    EditYours,
    TakeYoursBack,
    /// In the review: closing it, or sending the work back, asked first when
    /// it asks something again; closing it and stopping the build and test of
    /// the stack as it was.
    AskClose,
    AskSendBack,
    ReviewCloseStop(Option<bool>),
    /// In the review's menu: the row picking whether the comments that
    /// asked are kept open or closed when the review closes.
    AskingAll,
    /// In the review's menu: the row picking whether HEAD of your checkout
    /// follows the stack when the review closes, its change accepted.
    HeadSync,
    /// A reference a comment attached: its lines previewed.
    ShowRef(Box<crate::refs::Ref>),
    /// A comment a reference points at: there, or, closed, as it was.
    GoToPointed(String, String),
    /// A line of a comment naming its references: the one it names, or a
    /// menu of them.
    Named,
    /// In the editor's menu: what may be attached to the comment, to pick.
    AttachRef,
    /// Comment (change, id) attached to the comment being written.
    AttachComment(String, String),
    /// Lines of a file, to name, attached to the comment being written.
    AttachLines,
    /// In the editor's menu: the references attached, to pick one to
    /// detach.
    DetachRef,
    /// Reference `r{0}` detached from the comment being written.
    Detach(usize),
    Send(Way),
    /// A patch's comment sent to the whole stack: the agent may change any
    /// patch.
    SendWhole,
    SendAll(Way),
    Unsend,
    /// The row above an ask, picking when it asks: now, or later, in the
    /// batch. Enter on it does the ask.
    When,
    /// In the review of landed work, an edit by hand of the stack it makes:
    /// as a list, the patch at a place as mail, or a file of it.
    EditWorkList,
    EditWorkPatch(usize),
    EditWorkFile(usize, String),
    /// A batched comment asked now, as it was batched; or taken out of the
    /// batch.
    SendNow,
    Unbatch,
    /// The batch sent: what is asked in way `0`, together, in one ask; or,
    /// `1`, each in an ask of its own.
    SendBatch(Way, bool),
    /// Comment (change, id) put in the batch, or taken out, from the
    /// banner's menu.
    Toggle(String, String),
    OpenStream,
    AskStop(String),
    Stop(String),
    GoToOpened,
    AskCloseOpened,
    CloseOpened,
    GoToOrigin,
    MoveBack,
    NoteHistory,
    AskCloseComment,
    CloseComment,
    /// A closed comment open again, as it was.
    Reopen,
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
    /// Comment (change, id), from the banner's menu.
    GoToComment(String, String),
    /// The history, at its record `rev`.
    GoToHistory(usize),
    Whole(bool),
    MarkRead,
    MarkUnread,
    Pull,
    AskApply,
    Apply,
    Runs,
    Search,
    Stacks,
    AskDeleteStack,
    DeleteStack,
    Transcript(String),
    /// What to do with a comment written: save it, and send it if `way`.
    Save(Option<Way>),
    /// Save a patch's comment, and send it to the whole stack.
    SaveWhole,
    /// A comment written again but left as it was: sent as it is (a
    /// patch's, to the whole stack), or the editor closed on it.
    SendAsItIs(Way),
    SendWholeAsItIs,
    LeaveAsItIs,
    ThrowAway,
    /// What was changed of a comment written again thrown away.
    ThrowChanges,
    KeepWriting,
    Quit,
}

/// What saving a comment and asking about it, `a`, is for one left as it
/// was: asking about it as it is.
fn as_it_is(a: Action) -> Action {
    match a {
        Action::Save(Some(way)) => Action::SendAsItIs(way),
        Action::SaveWhole => Action::SendWholeAsItIs,
        a => a,
    }
}

/// What `action` does, to follow "Press Enter to".
fn about(action: &Action) -> String {
    match action {
        Action::Line | Action::Cancel => "change nothing".into(),
        Action::Open => "read this patch".into(),
        Action::GoToNote => "go to this comment".into(),
        Action::MovePatch => "pick this patch up and put it elsewhere in the stack".into(),
        Action::MoveHead(_) => {
            "pick HEAD of your checkout up: ↑↓ place it, Enter moves the checkout there, detached; your branch as it is".into()
        }
        Action::MessagesToStack(_) => {
            "keep your workspace's message and write it over the patch's: a new version of the patch, its diff as it is, for your review".into()
        }
        Action::MessagesToCheckout(_) => {
            "keep the stack's message and write it over your workspace's commit's: that commit and the ones above it are made again, their files as they are, and the branch moves to them".into()
        }
        Action::EditPatch => "edit this patch in $EDITOR, as mail, opened where the cursor is".into(),
        Action::EditCode => "edit this file as the patch leaves it, in $EDITOR, opened at this line".into(),
        Action::EditAgain => "edit it again, as you left it; conflicts you resolved since are thrown away".into(),
        Action::Resolve => "open the file of the patch that no longer applies in $EDITOR, as that patch would leave it on your edit: conflict markers show where the two disagree. Once it is resolved and saved without them, your edit and what you resolved go in the stack together".into(),
        Action::ThrowEdit => "throw your edit away, and the conflicts you resolved since".into(),
        Action::AskDrop => "drop this patch from the stack (undo brings it back)".into(),
        Action::Drop => "drop it".into(),
        Action::CommentStack => "create a comment on the stack, for the agent to work on the whole stack".into(),
        Action::CommentPatch => "create a comment on this patch as a whole".into(),
        Action::CommentLine => "create a comment on this line".into(),
        Action::Reply => "reply to this comment".into(),
        Action::CommentInEditor => "write it in $EDITOR".into(),
        Action::EditMine => "edit what you said last, or add to it".into(),
        Action::EditMineInEditor => "write what you said last again, in $EDITOR".into(),
        Action::EditTurn(_) => "edit the turn of the thread the cursor is on, whoever wrote it: the agent is told it as you leave it".into(),
        Action::EditTurnInEditor(_) => "edit the turn of the thread the cursor is on, in $EDITOR".into(),
        Action::Rewind(_) => {
            "rewind the conversation from your turn: what you write takes the place of that turn and of every turn after it".into()
        }
        Action::RewindInEditor(_) => "rewind the conversation from your turn, in $EDITOR".into(),
        Action::Collapse => "show this comment in the middle as one quiet line, until you expand it".into(),
        Action::Expand => "show this comment whole again".into(),
        Action::TakeReply(..) => "accept this reply: it goes into its thread now, out of the review of what it landed with".into(),
        Action::Review(..) => "review what landed, the agent's work or the comments pulled from Phabricator: accept or reject each part of it".into(),
        Action::MarkPart(true) => "mark this part accepted: it is applied when the review closes".into(),
        Action::MarkPart(false) => "mark this part rejected: it is dropped when the review closes".into(),
        Action::MarkOpened(true) => "mark every comment this work opened accepted".into(),
        Action::MarkOpened(false) => "mark every comment this work opened rejected".into(),
        Action::MarkReplies(true) => "mark every reply of this work accepted".into(),
        Action::MarkReplies(false) => "mark every reply of this work rejected".into(),
        Action::ReviewSave => "save your marks and leave: the review goes on from them when you open it again".into(),
        Action::ReviewDiscard => "leave, throwing away the marks you made since you opened the review".into(),
        Action::ReviewClose(None) => "close the review: what you accepted is applied, what you rejected is dropped for good".into(),
        Action::ReviewClose(Some(true)) => "accept every part not marked yet, then close the review".into(),
        Action::ReviewClose(Some(false)) => "reject every part not marked yet, then close the review".into(),
        Action::AskReviewClose(true) => "accept every part not marked yet, then close the review, once you say so".into(),
        Action::AskReviewClose(false) => "reject every part not marked yet, then close the review, once you say so".into(),
        Action::CloseAsking(_, true) => {
            "close this comment as the review closes, done with: its replies, which go with it, are accepted (Show…, closed, lists it, to reopen)".into()
        }
        Action::CloseAsking(_, false) => "keep this comment open when the review closes; its replies as they were".into(),
        Action::SendBack => {
            "send the work back to the agent with what you wrote in this review: the work is not accepted, and the agent is asked again, starting from this work; the agent's new work lands for review in place of this one".into()
        }
        Action::Answer(_) => "answer the agent's reply: the work goes back to the agent with your answer after the reply".into(),
        Action::EditYours => "write it again: what you said last in your comment, or your answer to the agent's reply".into(),
        Action::TakeYoursBack => "take it back: it does not go back to the agent with the work".into(),
        Action::AskClose => "close the review, once you say so: what you accepted is applied, and what that asks again is said first".into(),
        Action::AskSendBack => "send the work back to the agent, once you say so: what went on from it is asked again".into(),
        Action::ReviewCloseStop(_) => "close the review, and stop the build and test of the stack as it was".into(),
        Action::AskingAll => "pick whether the comments that asked are kept open or closed when the review closes: ← → or Enter".into(),
        Action::HeadSync => {
            "pick whether HEAD of your checkout moves to the top of the stack as the review leaves it, once its change is accepted: ← → or Enter".into()
        }
        Action::ShowRef(_) => "preview the lines the agent points at: Enter there goes to them".into(),
        Action::GoToPointed(..) => "go to the comment this points at; closed, read it as it was".into(),
        Action::Named => "go to the reference this line names (@r1): a comment, or lines previewed; naming several, pick one".into(),
        Action::AttachRef => {
            "attach a reference to the comment: another comment of the stack, or lines of a file; what you write names it @r1, several @r{1,2}, all @r*".into()
        }
        Action::AttachComment(..) => "attach this comment: the agent reads it with yours, open or closed".into(),
        Action::AttachLines => "attach lines of a file, as a patch leaves it, as the base has it, or as the whole stack leaves it".into(),
        Action::DetachRef => "detach a reference from the comment".into(),
        Action::Detach(_) => "detach this reference: a mention of it then names nothing".into(),
        Action::Send(Way::Change) => "ask the agent, which may change the code within this comment's place".into(),
        Action::Send(Way::Answer) => "ask the agent, to answer only: nothing changes".into(),
        Action::Send(Way::Test) => "ask the agent to build and test, in the stack's build worktree, .stacked/NAME/build".into(),
        Action::SendWhole => {
            "ask the agent, which may change the whole stack, as for the stack's own comments: any patch, new patches anywhere, their order".into()
        }
        Action::SendAll(Way::Change) => "ask the agent about every comment listed; it may change the code".into(),
        Action::SendAll(Way::Answer) => "ask the agent about every comment listed, to answer only".into(),
        Action::SendAll(Way::Test) => "ask the agent to build and test, for every comment listed".into(),
        Action::Unsend => "take it back from the agent's queue".into(),
        Action::When => {
            "pick when the agent is asked: now, or later, the comment waiting in the batch, which the banner sends with the rest; enter asks".into()
        }
        Action::EditWorkList => {
            "edit the stack this work makes as a list, as `git rebase -i` does, in $EDITOR: saved, it lands in place of this work, which is rejected, its replies and comments going with it".into()
        }
        Action::EditWorkPatch(at) => format!(
            "edit patch {} as this work leaves it, as mail in $EDITOR: saved, it lands in place of this work, which is rejected, its replies and comments going with it",
            at + 1
        ),
        Action::EditWorkFile(at, file) => format!(
            "edit {file} as patch {} of this work leaves it, in $EDITOR, where the item picks: saved, it lands in place of this work, which is rejected, its replies and comments going with it",
            at + 1
        ),
        Action::SendNow => "ask the agent about it now, as it was batched, without waiting for the rest of the batch".into(),
        Action::Unbatch => "take it out of the batch: it is not sent".into(),
        Action::SendBatch(Way::Change, _) => {
            "send the batched edits to a single agent, in one ask: from more than one patch, it may change the whole stack".into()
        }
        Action::SendBatch(Way::Answer, false) => "send the batched questions to a single agent, in one ask, to answer only".into(),
        Action::SendBatch(Way::Answer, true) => "send each batched question to an agent of its own, all at once, to answer only".into(),
        Action::SendBatch(Way::Test, _) => "send each batched build and test, each an ask of its own, one after the other".into(),
        Action::Toggle(..) => "put this comment in the batch, to be sent with it, or take it out".into(),
        Action::OpenStream => "read what the agent is writing".into(),
        Action::AskStop(_) | Action::Stop(_) => "stop the agent: the ask lands as failed".into(),
        Action::GoToOpened => "go to the comments its answer opened".into(),
        Action::AskCloseOpened | Action::CloseOpened => "close it and the comments its answer opened".into(),
        Action::GoToOrigin => "go to where it was before it moved to the stack".into(),
        Action::MoveBack => "move it back where it came from".into(),
        Action::NoteHistory => "read every state this comment has been in".into(),
        Action::AskCloseComment | Action::CloseComment => "close this comment, done with: it leaves the comments shown (Show…, closed, lists it, to reopen)".into(),
        Action::Reopen => "reopen this comment, as it was when it was closed".into(),
        Action::ShowMenu => "pick which comments the panes show".into(),
        Action::SetShow(s) => format!("show the comments {}", s.name()),
        Action::Undo => "go back to the stack before this edit".into(),
        Action::Redo => "make that edit again".into(),
        Action::EditList => "edit the stack as `git rebase -i` does, in $EDITOR".into(),
        Action::Tree => "see every state of the stack, and edit from any".into(),
        Action::EditFrom(rev) => format!("edit from @{rev}"),
        Action::WhatChanged(rev) => format!("read what the edit that made @{rev} changed"),
        Action::History => "read what changed the stack, newest first, each with its stack diff".into(),
        Action::Versions => "see every version of this patch, and what changed from one to the next".into(),
        Action::UseVersion(_) => "put this version back in the stack".into(),
        Action::AskAbout(rev) => format!("write a question on @{rev}, for the agent to answer: it may open comments, it changes nothing"),
        Action::GoToAbout(rev) => format!("go to the comments on @{rev}, among the stack's"),
        Action::GoToComment(..) => "go to this comment".into(),
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
        Action::Search => "search the comments, the patches and the files they touch, and the history: what is found is listed as it is found, and enter goes to it".into(),
        Action::Stacks => "switch to another stack".into(),
        Action::AskDeleteStack | Action::DeleteStack => "delete this stack (undo brings it back)".into(),
        Action::Transcript(_) => "see what the agent wrote".into(),
        Action::Save(None) => "save the comment".into(),
        Action::Save(Some(w)) => format!("save the comment and ask the agent ({})", w.said()),
        Action::SaveWhole => format!("save the comment and ask the agent ({})", crate::db::WHOLE),
        Action::SendAsItIs(w) => format!("ask the agent about the comment as it is ({})", w.said()),
        Action::SendWholeAsItIs => format!("ask the agent about the comment as it is ({})", crate::db::WHOLE),
        Action::LeaveAsItIs => "close the editor: the comment stays as it is".into(),
        Action::ThrowAway => "throw what you wrote away".into(),
        Action::ThrowChanges => "throw your changes away: the comment stays as it was".into(),
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
            choices: None,
            labels: None,
            check: None,
        }
    }

    /// With a box before its label, checked if `on`.
    fn checked(mut self, on: bool) -> Item {
        self.check = Some(on);
        self
    }

    /// Offered, but only to be chosen if `ok`.
    fn when(mut self, ok: bool) -> Item {
        self.enabled = ok;
        self
    }

    /// With `choices`, as radio buttons.
    fn with(mut self, choices: Choices) -> Item {
        self.choices = Some(choices);
        self
    }

    fn line() -> Item {
        Item {
            label: String::new(),
            action: Action::Line,
            enabled: false,
            choices: None,
            labels: None,
            check: None,
        }
    }

    /// What its row says: its label, then its choices, which start past
    /// `pad` columns (the menu's choices in one column); and each choice,
    /// where it starts in the row and how wide it is.
    pub fn row(&self, pad: usize) -> (String, Vec<(usize, usize)>) {
        let mut text = match self.check {
            Some(on) => format!("{} {}", if on { "[x]" } else { "[ ]" }, self.label),
            None => self.label.clone(),
        };
        let mut at = Vec::new();
        if let Some(c) = &self.choices {
            let width = text.chars().count();
            text.extend(std::iter::repeat_n(' ', pad.saturating_sub(width)));
            text.push_str("  ");
            for (k, ch) in c.list.iter().enumerate() {
                text.push_str("  ");
                let one = format!("{} {}", if k == c.picked { "[x]" } else { "[ ]" }, ch.shown);
                at.push((text.chars().count(), one.chars().count()));
                text.push_str(&one);
            }
        }
        (text, at)
    }

    /// What Enter on it does: its pick's action, if the pick has one, else
    /// its own.
    pub fn doing(&self) -> Action {
        self.choices
            .as_ref()
            .and_then(|c| c.list[c.picked].action.clone())
            .unwrap_or_else(|| self.action.clone())
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
        // Not on the row picking when an ask goes: on the ask.
        let first = out.iter().position(|i| i.enabled && i.action != Action::When);
        Menu {
            title: title.into(),
            sel: first.or_else(|| out.iter().position(|i| i.enabled)).unwrap_or(0),
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

    /// Its asks labelled as they ask: `later`, or now.
    fn relabel(&mut self, later: bool) {
        for i in &mut self.items {
            if let Some((now, then)) = &i.labels {
                i.label = if later { then } else { now }.clone();
            }
        }
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
    /// `file` as the patch leaves it, at `line`, where the stack's menus
    /// say it is edited; from `again`, an edit not taken before, if given.
    EditCode {
        file: String,
        line: usize,
        again: Option<ops::Kept>,
    },
    EditTodo,
    EditComment(Target),
    /// The conflict a later patch has with an edit not taken, resolved by
    /// hand in $EDITOR.
    Resolve,
    /// An edit by hand of what landed work `run` makes of the stack, from
    /// its review: `edit` says what of it.
    EditWork { run: String, edit: WorkEdit },
}

/// What an edit by hand in the review of landed work edits, of the stack
/// the work makes: the stack as a list, the patch at a place as mail, or
/// a file of it at a line (from `again`, an edit not taken before).
#[derive(Debug, Clone, PartialEq)]
pub enum WorkEdit {
    List,
    Mail(usize),
    Code { at: usize, file: String, line: usize, again: Option<ops::Kept> },
}

/// Where the cursor is to go once the rows are built: to a comment, or to
/// a line.
#[derive(Debug, Clone, PartialEq)]
enum Want {
    Note(String),
    Line(String),
    /// A line of the patch's message, of its diff, from 0.
    Msg(usize),
    Diff(usize),
    /// The row of file `path` nearest its line, from 1.
    Near(String, usize),
}

/// A text to read, on a screen of its own.
#[derive(Debug, Clone)]
pub struct TextView {
    pub title: String,
    pub lines: Vec<(String, Style)>,
    pub scroll: usize,
    back: Screen,
}

pub use crate::config::Pane;
pub use crate::history::{Diffed, Entry};
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
    /// The cursor is on the banner saying what the agent's work on the
    /// stack is, above the panes: open, a line for each, the line
    /// `banner_sel` picked, from `banner_top` on shown. Where the cursor was
    /// is as it was, to go back to.
    pub on_banner: bool,
    pub banner_sel: usize,
    banner_top: usize,
    /// Where the cursor was in each pane, by what it showed: to come back
    /// to.
    places: places::Places,
    /// The review of what the agent brought back, being made.
    pub reviewing: Option<Reviewing>,
    /// The search open, and the searches made before, newest first: kept
    /// for as long as the editor runs.
    pub searching: Option<Searching>,
    pub searches: Vec<String>,
    pub focus: Focus,
    /// How the patches are shown, on the left: expanded, compact or
    /// hidden.
    pub list: Pane,
    /// How the comments are shown, on the right, when there are any.
    pub comments: Pane,
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
    changed: crate::progress::Memo,
    /// How far the review of the stack is, and what that was worked out
    /// from: the stack's name, and how many changes the database had.
    progress: Option<((String, usize), crate::progress::Progress)>,
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
    /// History…'s pane the cursor is in; the line of the stack diff the
    /// cursor is on, and its top.
    pub hist_focus: HistFocus,
    pub hist_cursor: usize,
    hist_top: usize,
    /// The lines of the stack diff of the record picked, and what they were
    /// made of: the stacks diffed, the width.
    pub hist_rows: Vec<(String, Style)>,
    hist_built: Option<(Option<usize>, Option<usize>, usize)>,
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
    /// the edit as it was left, to edit again.
    refused: Option<(String, usize, ops::Kept)>,
    /// When that edit was of landed work, in its review: the work's run,
    /// and the patch's place in the stack it makes.
    refused_work: Option<(String, usize)>,
    /// The conflict a later patch has with it, to resolve by hand.
    conflict: Option<Box<ops::Conflict>>,
    /// What the last key did, when that needs saying: gone with the next.
    pub status: String,
    body_h: usize,
    body_w: usize,
    /// The runs still going when the database was last read: when one
    /// lands, the status line says what came of it.
    going: Vec<String>,
    judged: Option<Judged>,
    /// The checkout stacked runs in, whose HEAD the patches pane shows.
    pub checkout: PathBuf,
    /// Where its HEAD was against a stack, when last looked at.
    head: Option<HeadSeen>,
    /// The work done away from the screen.
    bg: Background,
    /// The comments the agent works on, as last read, and how many records
    /// the database had then.
    busy_memo: std::cell::RefCell<Option<(usize, std::rc::Rc<Busy>)>>,
    /// The lines each patch adds and removes, by its id: the compact
    /// patches pane's stats.
    stats_memo: std::cell::RefCell<HashMap<String, (usize, usize)>>,
    /// The comments whose reply waits for review, as last read, and how
    /// many changes the database had then.
    held_memo: std::cell::RefCell<Option<(usize, std::rc::Rc<Held>)>>,
    hist_memo: std::cell::RefCell<HistMemo>,
    /// What each part of the screen showed when it was last drawn, for a
    /// click: drawn later, on top.
    spots: std::cell::RefCell<Vec<(Rect, Spot)>>,
    /// Where the editor box was last drawn, on the screen: the menu Esc
    /// opens goes beside it, not over it.
    edit_box: std::cell::Cell<Option<Rect>>,
}

/// The stack's history, as last made: of which stack, at how many records
/// (an agent's stream aside).
type HistMemo = Option<((String, usize), std::rc::Rc<Vec<Entry>>)>;

/// What the patches pane shows, its lines: a patch, by its place shown and
/// in the stack; or, none, HEAD.
type ListEntry = (Option<(usize, usize)>, Vec<String>);

/// What work done away from the screen came to.
enum Done {
    /// Asks started, and asks whose agent is gone landed: what went wrong.
    Launched(Option<String>),
    /// A pull or an apply (`what` it was): what came of it, said.
    Said(&'static str, String),
    /// Where the pull under way is.
    Pulling(review::Pulling),
    /// Whether stack `name` at `rev` applies: why not, if not.
    Judged {
        name: String,
        rev: Option<usize>,
        why: Option<String>,
    },
    /// A stack's agents' worktrees made ahead, if its checkout's settings
    /// say so, or why they could not be; and what was wrong with those
    /// settings.
    Precreated(std::result::Result<Option<ask::Ahead>, String>, Vec<String>),
    /// Where HEAD of the checkout stacked runs in is, against stack `name`
    /// at `rev`: none if the checkout is not of its repository. And, looked
    /// at in a review with HEAD in sync with the stack, whether nothing is
    /// uncommitted there.
    Head {
        name: String,
        rev: Option<usize>,
        head: Option<crate::head::Head>,
        clean: Option<bool>,
    },
}

/// Where HEAD of the checkout stacked runs in was, against stack `name` at
/// `rev`, when it was last looked at; whether to look again at once.
struct HeadSeen {
    name: String,
    rev: Option<usize>,
    head: Option<crate::head::Head>,
    /// Whether nothing is uncommitted in the checkout: looked at only in a
    /// review, with HEAD in sync with the stack, for HEAD to follow it.
    clean: Option<bool>,
    when: Instant,
    stale: bool,
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
    /// The pull under way: its stack, and where it is, which the banner
    /// says.
    pull: Option<(String, review::Pulling)>,
    /// The stack being checked, and its rev.
    judging: Option<(String, Option<usize>)>,
    /// The stacks whose agents' worktrees were made ahead, and where: their
    /// repository, folder and kinds, to remove them once the editor exits.
    precreating: Vec<String>,
    precreated: Vec<ask::Ahead>,
    /// Where HEAD is is being looked at.
    heading: bool,
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
            pull: None,
            judging: None,
            precreating: vec![],
            precreated: vec![],
            heading: false,
        }
    }
}

/// What the bar says while HEAD of the checkout is being moved, or its
/// commits made again with the stack's messages.
const MOVING_HEAD: &str = "moving HEAD";
const REWORDING: &str = "rewording";

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
    db.live_runs()
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
            on_banner: false,
            banner_sel: 0,
            banner_top: 0,
            places: places::Places::default(),
            reviewing: None,
            searching: None,
            searches: Vec::new(),
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
            progress: None,
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
            refused_work: None,
            conflict: None,
            status,
            body_h: 20,
            body_w: 80,
            going: going(&db),
            judged: None,
            checkout: std::env::current_dir().unwrap_or_default(),
            head: None,
            bg: Background::new(),
            busy_memo: std::cell::RefCell::new(None),
            stats_memo: std::cell::RefCell::new(HashMap::new()),
            held_memo: std::cell::RefCell::new(None),
            hist_memo: std::cell::RefCell::new(None),
            spots: std::cell::RefCell::new(Vec::new()),
            edit_box: std::cell::Cell::new(None),
            db,
            cfg,
        };
        if app.patch_count() == 0 {
            app.on_stack = true;
        }
        app.judge();
        app.precreate();
        Ok(app)
    }

    /// The current stack's agents' worktrees made ahead, away from the
    /// screen, as the settings of its checkout say (`precreate`): once per
    /// stack. Those settings are read there too, and what is wrong with them
    /// said.
    fn precreate(&mut self) {
        if self.bg.precreating.contains(&self.stack) {
            return;
        }
        let Some(repo) = self.db.stack(&self.stack).ok().and_then(|s| s.repo.clone()) else {
            return;
        };
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path) {
            return;
        }
        self.bg.precreating.push(self.stack.clone());
        let (path, name) = (self.path.clone(), self.stack.clone());
        self.spawn(move || {
            let (cfg, wrong) = crate::config::of(Path::new(&repo));
            let kinds = ask::precreated(&cfg.agent);
            let made = match kinds.is_empty() {
                true => Ok(None),
                false => ask::precreate(&path, &name, &kinds, cfg.agent.worktrees.as_deref())
                    .map(|(repo, dir)| Some((repo, dir, kinds)))
                    .map_err(|e| e.0),
            };
            Done::Precreated(made, wrong)
        });
    }

    /// Once the editor exits: the agents' worktrees it made ahead removed,
    /// away from it, but those an agent works in or left work in.
    pub fn leave(&self) {
        ask::forget(&self.path, &self.bg.precreated);
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

    /// Why the stack cannot be edited by hand now, if it cannot: it is
    /// read-only, or the agent is changing it.
    pub fn by_hand(&self) -> Option<String> {
        self.read_only().map(String::from).or_else(|| ask::blocked(&self.db, &self.stack))
    }

    /// Where HEAD of the checkout stacked runs in is, against the current
    /// stack: looked at away from the screen, again as the stack changes,
    /// and every few seconds, as HEAD moves outside stacked too.
    fn check_head(&mut self) {
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        // In a review, HEAD in sync with the stack seen without whether the
        // checkout is clean is seen again.
        let unsure = |h: &HeadSeen| self.reviewing.is_some() && h.clean.is_none() && h.head.as_ref().is_some_and(|hd| hd.in_sync(self.patch_count()));
        let seen = self
            .head
            .as_ref()
            .is_some_and(|h| h.name == self.stack && h.rev == rev && !h.stale && h.when.elapsed() < Duration::from_secs(3) && !unsure(h));
        if self.bg.heading || seen {
            return;
        }
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path) {
            return;
        }
        let Some((_, s)) = self.db.head(&self.stack) else { return };
        let s = s.clone();
        let patches: Vec<Patch> = s.patches.iter().map(|id| self.db.patch(id).clone()).collect();
        let (name, here) = (self.stack.clone(), self.checkout.clone());
        // In a review, whether the checkout is clean too: HEAD in sync with
        // the stack may follow it as the review closes.
        let (review, n) = (self.reviewing.is_some(), s.patches.len());
        self.bg.heading = true;
        self.spawn(move || {
            let head = crate::head::of_stack(&s, &patches.iter().collect::<Vec<_>>(), &here).ok().flatten();
            // Not known to be clean, it is not.
            let clean = head
                .as_ref()
                .filter(|h| review && h.in_sync(n))
                .map(|h| !crate::git::unfinished_tracked(&h.dir).unwrap_or(true));
            Done::Head { name, rev, head, clean }
        });
    }

    /// HEAD looked at again, at once: it may have moved. Where it was is
    /// shown meanwhile.
    pub fn look_at_head(&mut self) {
        if let Some(h) = &mut self.head {
            h.stale = true;
        }
        self.check_head();
    }

    /// Where HEAD of the checkout stacked runs in is against the current
    /// stack, as last looked at: if it is a checkout of its repository.
    pub fn head_here(&self) -> Option<&crate::head::Head> {
        self.head.as_ref().filter(|h| h.name == self.stack).and_then(|h| h.head.as_ref())
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
            Done::Pulling(p) => {
                if let Some((_, at)) = &mut self.bg.pull {
                    *at = p;
                }
            }
            Done::Said(what, said) => {
                self.bg.doing.retain(|d| *d != what);
                if what == "pulling" {
                    self.bg.pull = None;
                }
                self.status = said;
                // Moved, or its commits made again: where HEAD is, looked at
                // again.
                if what == MOVING_HEAD || what == REWORDING {
                    self.look_at_head();
                }
            }
            Done::Judged { name, rev, why } => {
                if self.bg.judging.as_ref().is_some_and(|(n, r)| *n == name && *r == rev) {
                    self.bg.judging = None;
                }
                self.judged = Some(Judged { name, rev, why });
            }
            Done::Precreated(made, wrong) => {
                // Unless the bar said it at the start: the stack's checkout
                // is the one stacked runs in.
                if let Some(w) = wrong.iter().find(|w| !crate::config::warnings().contains(w)) {
                    self.status = w.clone();
                }
                match made {
                    Ok(Some(made)) if !self.bg.precreated.contains(&made) => self.bg.precreated.push(made),
                    Ok(_) => {}
                    Err(e) => self.status = format!("the agents' worktrees could not be made ahead: {e}"),
                }
            }
            Done::Head { name, rev, head, clean } => {
                self.bg.heading = false;
                self.head = Some(HeadSeen {
                    name,
                    rev,
                    head,
                    clean,
                    when: Instant::now(),
                    stale: false,
                });
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
        while self.bg.launching || !self.bg.doing.is_empty() || self.bg.judging.is_some() || self.bg.heading {
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
        let key = self.db.last_rev();
        if let Some((k, b)) = &*self.busy_memo.borrow()
            && *k == key
        {
            return b.clone();
        }
        let b = std::rc::Rc::new(ask::with_agent(&self.db));
        *self.busy_memo.borrow_mut() = Some((key, b.clone()));
        b
    }

    /// The comments whose reply from the agent waits for review: to its
    /// run, and the reply's place in what the run holds.
    fn held_replies(&self) -> std::rc::Rc<Held> {
        let key = self.db.changes();
        if let Some((k, h)) = &*self.held_memo.borrow()
            && *k == key
        {
            return h.clone();
        }
        let h = std::rc::Rc::new(crate::held::replies_waiting(&self.db));
        *self.held_memo.borrow_mut() = Some((key, h.clone()));
        h
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
            notes.sort_by_key(|n| self.db.note_first(&n.change, &n.id).unwrap_or(0));
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
        shows(show, n, busy, &self.held_replies(), gone)
    }

    /// The comments the filter lets the middle pane show: the closed ones,
    /// each as it was, when the filter is those.
    fn shown_notes(&self) -> Vec<Note> {
        if self.show == Show::Closed {
            return self.closed().into_iter().map(|(_, n)| n).collect();
        }
        let busy = self.busy();
        let lines = self.line_keys();
        self.subject_notes()
            .into_iter()
            .filter(|n| self.shows(self.show, n, &busy, self.gone(n, &lines)))
            .collect()
    }

    /// The comments the middle pane would show that were closed: each as it
    /// was last, with the rev that deleted it.
    fn closed(&self) -> Vec<(usize, Note)> {
        self.subject_change().map(|c| self.db.deleted_notes(&self.stack, &c)).unwrap_or_default()
    }

    /// How many of the middle pane's comments each filter would show.
    pub fn show_counts(&self) -> Vec<(Show, usize)> {
        let busy = self.busy();
        let lines = self.line_keys();
        let notes = self.subject_notes();
        Show::ALL
            .into_iter()
            .map(|s| match s {
                Show::Closed => (s, self.closed().len()),
                _ => (s, notes.iter().filter(|n| self.shows(s, n, &busy, self.gone(n, &lines))).count()),
            })
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
        if let Some((id, turns)) = self.written_turns() {
            deco.writing.insert(id, turns);
        }
        // A closed comment says when it was, on its first line.
        let deleted: HashMap<String, usize> = match self.show {
            Show::Closed => self.closed().into_iter().map(|(rev, n)| (n.id, rev)).collect(),
            _ => HashMap::new(),
        };
        deco.pointed = self.pointed_of(notes);
        for n in notes {
            let tag = match deleted.get(&n.id) {
                Some(rev) => format!("closed @{rev}"),
                None => self.tag(n, &busy),
            };
            if !tag.is_empty() {
                deco.tags.insert(n.id.clone(), tag);
            }
            if self.show != Show::Closed && self.db.collapsed(&n.change, &n.id) {
                deco.collapsed.insert(n.id.clone());
            }
            if let Some((_, run)) = busy.get(&(n.change.clone(), n.id.clone())) {
                let lines = match &self.mode {
                    Mode::Stream { change, id, up } if *change == n.change && *id == n.id => {
                        let all = ask::tail(&self.db, run, usize::MAX);
                        let room = (self.body_h / 2).max(5);
                        let end = all.len().saturating_sub(*up);
                        all[end.saturating_sub(room)..end].to_vec()
                    }
                    // Starting, one line says what it runs: the command
                    // first, cut where the line ends.
                    _ => match ask::starting(&self.db, run) {
                        Some(doing) => vec![format!("starting: {doing}")],
                        None => {
                            let lines = ask::tail(&self.db, run, self.cfg.view.stream_lines);
                            if lines.is_empty() { vec!["the agent is starting…".into()] } else { lines }
                        }
                    },
                };
                deco.streams.insert(n.id.clone(), lines);
            }
        }
        deco
    }

    fn rebuild(&mut self, width: usize) {
        let mut notes = self.shown_notes();
        // The comment written on in the middle: its references as the editor
        // has them, under it.
        let attaching = self
            .writing()
            .filter(|e| self.refs_in_rows(e) && self.attachable(&e.target))
            .and_then(|e| Some((self.comment_of(&e.target)?, e.attached.clone())));
        if let Some(((change, id), attached)) = &attaching
            && let Some(n) = notes.iter_mut().find(|n| n.change == *change && n.id == *id)
        {
            n.attached = attached.clone();
        }
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
            "{subject}\0{width}\0{}\0{streams}\0{:?}\0{stream}\0{}\0{}\0{:?}\0{:?}",
            self.db.changes(),
            self.show,
            self.body_h,
            self.reading,
            self.written_turns(),
            attaching
        );
        if self.built.as_deref() == Some(&want) {
            return;
        }
        let same = self.subject.as_deref() == Some(subject.as_str());
        // Leaving what the panes showed: where their cursors were, kept.
        if !same && let Some(was) = self.subject.clone() {
            self.keep_panes(&was);
        }
        let refs: Vec<&Note> = notes.iter().collect();
        let mut deco = self.deco(&notes);
        if self.reading && !self.on_stack {
            deco.read = self.subject_change().map(|c| self.db.read_keys(&c));
            deco.whitespace = self.cfg.view.whitespace_needs_review;
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
        self.subject = Some(subject.clone());
        self.panes = Arc::new(pane_streams(&self.rows));
        if !same {
            self.cursor = 0;
            self.top = 0;
            self.note_sel = 0;
            self.compact.reset();
            // Back on what they showed before: where they were.
            self.back_to_panes(&subject);
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        self.note_sel = self.note_sel.min(self.notes().len().saturating_sub(1));
        if let Some(w) = self.want.take() {
            let at = match &w {
                Want::Note(id) => self.rows.iter().position(|r| r.head && r.note.as_deref() == Some(id)),
                Want::Line(key) => self.rows.iter().position(|r| r.key.as_deref() == Some(key) && r.note.is_none()),
                Want::Msg(k) => self.rows.iter().position(|r| r.msg_at == Some(*k) && r.note.is_none()),
                Want::Diff(k) => self.rows.iter().position(|r| r.diff_at == Some(*k) && r.note.is_none()),
                Want::Near(path, line) => self
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.note.is_none() && r.path.as_deref() == Some(path.as_str()))
                    .min_by_key(|(_, r)| r.new_no.map_or(usize::MAX, |n| n.abs_diff(*line)))
                    .map(|(i, _)| i),
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
        let any = match self.show {
            Show::Closed => !self.closed().is_empty(),
            _ => !self.subject_notes().is_empty(),
        };
        self.screen == Screen::Main && self.comments.shown() && any
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
        self.db.note(&change, &id).cloned().or_else(|| match self.show {
            // Closed, as it was, shown so.
            Show::Closed => self.closed().into_iter().map(|(_, n)| n).find(|n| n.id == id),
            _ => None,
        })
    }

    /// The turn of comment `c`'s thread the cursor is on, in the middle:
    /// where it is among all its turns, and what it is.
    fn turn_here(&self, c: &Note) -> Option<(usize, Turn)> {
        let row = self.rows.get(self.target_row())?;
        let i = row.turn.filter(|_| row.note.as_deref() == Some(c.id.as_str()))?;
        all_turns(&c.text).into_iter().nth(i).map(|t| (i, t))
    }

    /// Whether `n` is closed: shown as it was, only to reopen.
    fn is_closed(&self, n: &Note) -> bool {
        self.db.note(&n.change, &n.id).is_none()
    }

    /// That `spot` is drawn on line `y` of `area`, from `x`, `w` wide.
    fn spot(&self, area: Rect, (x, y, w): (usize, usize, usize), spot: Spot) {
        spot_in(&self.spots, area, (x, y, w, 1), spot);
    }

    /// What was drawn at `(x, y)` of the screen when it was last drawn: on
    /// top, the last drawn there.
    pub fn spot_at(&self, x: u16, y: u16) -> Option<Spot> {
        self.spots
            .borrow()
            .iter()
            .rev()
            .find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(_, s)| *s)
    }

    /// Whether the cursor is on `spot` already.
    fn on(&self, spot: Spot) -> bool {
        // On the banner, the cursor is in none of the panes.
        let panes = !self.on_banner;
        match spot {
            Spot::Patch(None) => panes && self.focus == Focus::List && self.on_stack,
            Spot::Patch(Some(i)) => panes && self.focus == Focus::List && !self.on_stack && self.sel == i,
            Spot::Row(i) => panes && self.focus == Focus::Diff && self.cursor == i,
            Spot::NotesHead => panes && self.focus == Focus::Notes && self.notes_head,
            Spot::Note(i) => panes && self.focus == Focus::Notes && !self.notes_head && self.note_sel == i,
            Spot::Pick(i) => self.pick == i && (self.screen != Screen::History || self.hist_focus == HistFocus::List),
            Spot::HistRow(i) => self.hist_focus == HistFocus::Diff && self.hist_cursor == i,
            // Its own line: what it does is its lines'.
            Spot::Banner => false,
            Spot::BannerLine(i) => self.on_banner && self.banner_sel == i,
            Spot::ReviewLine(i) => self.reviewing.as_ref().is_some_and(|r| r.sel == i && !r.in_main),
            Spot::Found(i) => self.searching.as_ref().is_some_and(|s| s.sel == i),
            Spot::Item(_) | Spot::Choice(..) | Spot::Box | Spot::SearchScope => false,
        }
    }

    /// A click at `(x, y)` of the screen, with the right button if `right`:
    /// in a menu, the item done (and outside it, the menu closed); else
    /// what is there picked, as the arrows would, and if it was picked
    /// already, done as Enter does; or, a right click, its menu, as Space
    /// opens it. Keys as they are, whatever the settings map: Enter is
    /// Enter.
    pub fn click(&mut self, x: u16, y: u16, right: bool) -> Effect {
        let spot = self.spot_at(x, y);
        let press = |app: &mut App, code: KeyCode| app.pressed(KeyEvent::new(code, KeyModifiers::NONE));
        match &mut self.mode {
            Mode::Menu(m) => {
                return match spot {
                    Some(Spot::Item(i)) => {
                        m.sel = i;
                        press(self, KeyCode::Enter)
                    }
                    // A choice picked, the item not done yet.
                    Some(Spot::Choice(i, k)) => {
                        m.sel = i;
                        let picked = m
                            .items
                            .get_mut(i)
                            .filter(|i| i.enabled)
                            .and_then(|i| i.choices.as_mut())
                            .filter(|c| k < c.list.len())
                            .map(|c| {
                                c.picked = k;
                                (c.of, c.pick())
                            });
                        if let Some((of, pick)) = picked {
                            self.picked_choice(of, pick);
                            if of == "when"
                                && let Mode::Menu(m) = &mut self.mode
                            {
                                m.relabel(pick == "later");
                            }
                        }
                        Effect::None
                    }
                    Some(Spot::Box) => Effect::None,
                    _ => {
                        self.mode = Mode::Normal;
                        Effect::None
                    }
                };
            }
            Mode::Help { back, .. } => {
                self.mode = (**back).clone();
                return Effect::None;
            }
            // A reference's preview: a click in it goes there, out of it
            // closes it.
            Mode::Preview(_) => {
                if matches!(spot, Some(Spot::Box)) {
                    return press(self, KeyCode::Enter);
                }
                self.mode = Mode::Normal;
                return Effect::None;
            }
            // Lines being named: a click out of their box goes back to the
            // comment.
            Mode::Lines(_) => {
                if !matches!(spot, Some(Spot::Box)) {
                    return press(self, KeyCode::Esc);
                }
                return Effect::None;
            }
            // What the agent writes, read in place: folded.
            Mode::Stream { .. } => {
                self.mode = Mode::Normal;
                return Effect::None;
            }
            // A patch being moved goes where clicked, and is put down there
            // with a click on it.
            Mode::Moving { to, .. } => {
                if let Some(Spot::Patch(Some(at))) = spot {
                    if *to == at {
                        return press(self, KeyCode::Enter);
                    }
                    *to = at;
                }
                return Effect::None;
            }
            // HEAD being moved goes under the patch clicked (under the
            // header, at the base), and is put down there with a click on it.
            Mode::MovingHead { to } => {
                if let Some(Spot::Patch(at)) = spot {
                    let there = at.map_or(0, |at| at + 1);
                    if *to == there {
                        return press(self, KeyCode::Enter);
                    }
                    *to = there;
                }
                return Effect::None;
            }
            Mode::Edit(_) => return Effect::None,
            Mode::Normal => {}
        }
        let Some(spot) = spot else { return Effect::None };
        let was = self.on(spot);
        if !self.pick_spot(spot) {
            return Effect::None;
        }
        if right {
            return press(self, KeyCode::Char(' '));
        }
        if was {
            return press(self, KeyCode::Enter);
        }
        Effect::None
    }

    /// The mouse over `(x, y)` of the screen: what is there picked, as the
    /// arrows would, so that a click does on it what Enter does. In a menu,
    /// its item; a patch being moved goes there. Not while a comment is
    /// written, nor over what `?` says.
    pub fn hover(&mut self, x: u16, y: u16) {
        let Some(spot) = self.spot_at(x, y) else { return };
        match &mut self.mode {
            Mode::Menu(m) => {
                if let Spot::Item(i) | Spot::Choice(i, _) = spot {
                    m.sel = i;
                }
            }
            Mode::Moving { to, .. } => {
                if let Spot::Patch(Some(at)) = spot {
                    *to = at;
                }
            }
            Mode::MovingHead { to } => {
                if let Spot::Patch(at) = spot {
                    *to = at.map_or(0, |at| at + 1);
                }
            }
            // Not onto the banner, nor off it: it opens and closes under a
            // key or a click, not under the mouse going by, which would move
            // what is under the mouse.
            Mode::Normal if matches!(spot, Spot::Banner | Spot::BannerLine(_)) == self.on_banner => {
                self.pick_spot(spot);
            }
            _ => {}
        }
    }

    /// `spot` picked, as the arrows would: whether it is one to pick.
    fn pick_spot(&mut self, spot: Spot) -> bool {
        if !matches!(spot, Spot::Item(_) | Spot::Choice(..) | Spot::Box) {
            let banner = matches!(spot, Spot::Banner | Spot::BannerLine(_));
            if banner && !self.on_banner {
                self.open_banner(0);
            }
            self.on_banner = banner;
        }
        match spot {
            Spot::Banner => {}
            Spot::BannerLine(i) => self.banner_sel = i,
            Spot::ReviewLine(i) => {
                if let Some(r) = &mut self.reviewing {
                    r.sel = i;
                    r.in_main = false;
                }
            }
            Spot::Found(i) => {
                if let Some(s) = &mut self.searching {
                    s.sel = i;
                }
            }
            Spot::SearchScope => self.step_scope(),
            Spot::Patch(None) => {
                self.focus = Focus::List;
                self.on_stack = true;
            }
            Spot::Patch(Some(i)) => {
                self.focus = Focus::List;
                self.on_stack = false;
                self.sel = i;
            }
            Spot::Row(i) => {
                self.focus = Focus::Diff;
                self.cursor = i;
            }
            Spot::NotesHead => {
                self.focus = Focus::Notes;
                self.notes_head = true;
            }
            Spot::Note(i) => {
                self.focus = Focus::Notes;
                self.notes_head = false;
                self.note_sel = i;
            }
            Spot::Pick(i) => {
                self.pick = i;
                if self.screen == Screen::History {
                    self.hist_focus = HistFocus::List;
                    self.hist_rev = self.history().get(i).map(|e| e.rev);
                }
            }
            Spot::HistRow(i) => {
                self.hist_focus = HistFocus::Diff;
                self.hist_cursor = i;
            }
            Spot::Item(_) | Spot::Choice(..) | Spot::Box => return false,
        }
        true
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
        self.poll_search();
        let polled = self.poll();
        let len = Db::file_len(&self.path);
        if len != self.db.bytes_read() || polled {
            self.reload();
        }
        self.check_head();
    }

    /// Reads what was appended: what an edit just wrote, or another
    /// process.
    fn reload(&mut self) {
        // The comment the cursor is on, should the agent move it away.
        let on = self.picked().map(|n| (n.change, n.id));
        if let Err(e) = self.db.catch_up() {
            self.status = e.0;
        }
        self.fold_done();
        // What landed says so, but what waits for review: the banner says
        // that, not to interrupt.
        for id in std::mem::take(&mut self.going) {
            if let Some(r) = self.db.run(&id)
                && r.state != RunState::Started
                && r.held.is_none()
            {
                let state = format!("{:?}", r.state).to_lowercase();
                self.status = format!("the agent's {} {state}: {}", r.what, r.summary);
            }
        }
        self.going = going(&self.db);
        if let Some((change, id)) = on {
            self.follow(&change, &id);
        }
        self.clamp();
        self.judge();
    }

    /// The comment the cursor was on, (change, id), moved to the stack's
    /// comments (it needs more than its patch, the agent says): the cursor
    /// goes with it, to ask again there.
    fn follow(&mut self, change: &str, id: &str) {
        if change == stack_change(&self.stack) || self.db.note(change, id).is_some() {
            return;
        }
        let moved = self
            .db
            .stack_notes(&self.stack)
            .into_iter()
            .find(|n| n.moved.as_ref().is_some_and(|m| m.change == change && m.id == id))
            .cloned();
        let Some(n) = moved else { return };
        self.go_to(&n.change, &n.id);
        let why = thread_turns(&n.text).pop().map(|t| t.body).unwrap_or_default();
        let why = why.strip_prefix("This needs more than the patch: ").unwrap_or(&why);
        self.status = format!(
            "moved to the stack's comments, the cursor with it: there, Ask agent may change the whole stack · {}",
            short(why, 200)
        );
    }

    fn clamp(&mut self) {
        let n = self.patch_count();
        self.sel = self.sel.min(n.saturating_sub(1));
        if n == 0 {
            self.on_stack = true;
            if !self.list.shown() {
                self.list = Pane::Expanded;
            }
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
        self.db.live_runs().iter().filter(|r| ask::running(r)).count()
    }

    fn ready(&self) -> usize {
        self.db.stack_notes(&self.stack).iter().filter(|n| n.queued.as_ref().is_some_and(|q| !q.batched())).count()
    }

    /// How many of the stack's comments are batched.
    fn batched_count(&self) -> usize {
        self.db.stack_notes(&self.stack).iter().filter(|n| n.queued.as_ref().is_some_and(|q| q.batched())).count()
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
        // The search reads what is typed too.
        let typed = self.screen == Screen::Search && matches!(self.mode, Mode::Normal);
        let k = if matches!(self.mode, Mode::Edit(_) | Mode::Lines(_)) || typed {
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
        self.pressed(k)
    }

    /// What `key` does, whatever keys the settings give it: for what is
    /// done for you, as the tour does.
    pub fn press(&mut self, key: crate::keys::Key) -> Effect {
        self.pressed(key.event())
    }

    /// Key `k`, as the settings map it: done.
    fn pressed(&mut self, k: KeyEvent) -> Effect {
        // ctrl-c is the terminal's, as in every program: out.
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::Quit;
        }
        // The search reads every key: ? and space are typed.
        if self.screen == Screen::Search && matches!(self.mode, Mode::Normal) {
            self.status.clear();
            return self.search_key(k);
        }
        // What can be done here, said: the next key goes back, and does
        // what it does there.
        if let Mode::Help { back, .. } = &self.mode {
            self.mode = (**back).clone();
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('?')) {
                return Effect::None;
            }
        } else if k.code == KeyCode::Char('?') && !matches!(self.mode, Mode::Edit(_) | Mode::Lines(_)) && (self.helps() || self.mode != Mode::Normal) {
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
            Mode::Preview(p) => return self.preview_key(*p, k),
            Mode::Edit(e) => return self.edit_key(*e, k),
            Mode::Lines(l) => return self.lines_key(*l, k),
            Mode::Moving { from, to } => {
                self.moving_key(from, to, k);
                return Effect::None;
            }
            Mode::MovingHead { to } => {
                self.head_key(to, k);
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
            Screen::Review => self.review_key(k),
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
        // On the banner, open: its keys, whatever pane the cursor was in.
        if self.on_banner {
            return self.banner_key(k);
        }
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
            // Esc goes to the banner first, from any pane, when there is one:
            // the agent's work, open; Esc there comes back here. Without one,
            // it goes back up: from the comments to the middle, from there to
            // the patches, from a patch to the stack's header.
            (_, KeyCode::Esc) if !self.banner_lines().is_empty() => self.open_banner(0),
            (Focus::List, KeyCode::Esc) if !self.on_stack => self.on_stack = true,
            // Up from the stack's header, the banner's last line, just above.
            (Focus::List, KeyCode::Up) if self.on_stack && !self.banner_lines().is_empty() => {
                let last = self.banner_lines().len() - 1;
                self.open_banner(last);
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
            // Further left than the patches: they say less, compact, then
            // make way for the middle, hidden.
            (Focus::List, KeyCode::Left) if self.current().is_some() => {
                self.list = self.list.next();
                if !self.list.shown() {
                    self.focus = Focus::Diff;
                }
            }
            // To the patches; hidden, they are shown again, expanded.
            (Focus::Diff, KeyCode::Left | KeyCode::Esc) => {
                if !self.list.shown() {
                    self.list = self.list.next();
                }
                self.focus = Focus::List;
            }
            (Focus::Diff, KeyCode::Right) if !self.subject_notes().is_empty() => {
                if !self.comments.shown() {
                    self.comments = self.comments.next();
                }
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
            // Further right than the comments: they say less, a line each,
            // then make way for the middle, which is where it was.
            (Focus::Notes, KeyCode::Right) => {
                self.comments = self.comments.next();
                if !self.comments.shown() {
                    self.focus = Focus::Diff;
                }
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

    /// Whether the middle, as last drawn, is wide enough for two sides.
    pub fn sides_fit(&self) -> bool {
        self.mid_w == 0 || self.mid_w >= self.cfg.view.split_width
    }

    /// 1: unified, side-by-side, compact, in turn; two sides when the
    /// middle is wide enough for them.
    fn cycle_layout(&mut self) {
        let next = self.layout.next();
        if next != Layout::Unified && !self.sides_fit() {
            let need = self.cfg.view.split_width;
            self.status = self.say_keys(format!(
                "side-by-side needs {need} columns: ← on the patches makes them compact, then hides them; → on the comments too"
            ));
            return;
        }
        self.set_layout(next);
        self.status = format!("{} view", next.label());
    }

    /// The code drawn as `l` from now on; two sides once the middle has room
    /// for them.
    pub fn set_layout(&mut self, l: Layout) {
        self.layout = l;
        if l == Layout::Compact {
            self.compact.scroll = Scroll::Both;
            self.compact.cursor = None;
        }
        self.built = None;
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
        self.reading
            && self.screen == Screen::Main
            && self.focus == Focus::Diff
            && !self.on_stack
            && !self.on_banner
            && self.patch_id().is_some()
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
            whitespace: self.cfg.view.whitespace_needs_review,
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
                whitespace: app.cfg.view.whitespace_needs_review,
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

    /// The changed lines of patch `id`: each one's key, and whether it is
    /// white space alone.
    fn changed_lines(&mut self, id: &str) -> Arc<Vec<(String, bool)>> {
        if let Some(c) = self.changed.get(id) {
            return c.clone();
        }
        let keys = Arc::new(crate::progress::changed_lines(self.db.patch(id)));
        self.changed.insert(id.into(), keys.clone());
        keys
    }

    /// How far the review of the stack is, worked out again once the
    /// database changed.
    fn keep_progress(&mut self) {
        let at = (self.stack.clone(), self.db.changes());
        if self.progress.as_ref().is_none_or(|(was, _)| *was != at) {
            let p = crate::progress::progress(&self.db, &self.stack, &mut self.changed, self.cfg.view.whitespace_needs_review);
            self.progress = Some((at, p));
        }
    }

    /// How many of patch `id`'s changed lines that need reading are read,
    /// of how many.
    fn read_of(&mut self, id: &str) -> (usize, usize) {
        let whitespace = self.cfg.view.whitespace_needs_review;
        let lines = self.changed_lines(id);
        let read = self.db.read_keys(&self.db.patch(id).change.clone());
        let keys: Vec<&String> = lines.iter().filter(|(_, blank)| whitespace || !blank).map(|(k, _)| k).collect();
        (keys.iter().filter(|k| read.contains(**k)).count(), keys.len())
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
                // A reference a comment attached: its lines, previewed.
                if let Some(r) = self.rows.get(self.target_row()).and_then(|row| row.reference.clone()) {
                    return Some(Action::ShowRef(Box::new(r)));
                }
                // A comment a reference points at: there.
                if let Some((change, id)) = self.rows.get(self.target_row()).and_then(|row| row.points.clone()) {
                    return Some(Action::GoToPointed(change, id));
                }
                // A line naming references: the one it names, or which.
                if self.rows.get(self.target_row()).is_some_and(|row| !row.names.is_empty()) {
                    return Some(Action::Named);
                }
                if let Some(n) = self.picked() {
                    // Closed: its menu, to reopen it.
                    if self.is_closed(&n) {
                        return None;
                    }
                    if self.db.collapsed(&n.change, &n.id) {
                        return Some(Action::Expand);
                    }
                    let busy = self.busy();
                    let held = self.held_replies().get(&(n.change.clone(), n.id.clone())).cloned();
                    return Some(if busy.contains_key(&(n.change.clone(), n.id.clone())) {
                        Action::OpenStream
                    } else if let Some((run, k)) = held {
                        // Its reply waiting for review: taken at once; or,
                        // going with a change of the stack, reviewed with it.
                        let alone = self.db.run(&run).and_then(|r| r.held.as_ref()).is_some_and(|h| h.stack.is_none());
                        if alone { Action::TakeReply(run, k) } else { Action::Review(run, Some(crate::held::Part::Reply(k))) }
                    } else if last_mine(&n.text).is_some() {
                        // The last word yours: a reply would only add to it.
                        Action::EditMine
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
                    // On the row picking when an ask goes, Enter does the ask
                    // under it; later, in the batch.
                    let (ask, does) = match (&item.action, m.items.get(m.sel + 1)) {
                        (Action::When, Some(next)) => (next, next.doing()),
                        _ => (item, item.doing()),
                    };
                    let later = if ask.labels.is_some() && self.later() { ", later: it waits in the batch, which the banner sends" } else { "" };
                    out.push(format!("Press Enter to {}{later}.", about(&does)));
                    if let Some(c) = &item.choices {
                        let each: Vec<String> = c.list.iter().map(|ch| format!("{}, {}", ch.shown, ch.said)).collect();
                        out.push(format!("Press ← → to pick {}: {}. What is picked is kept for this stack.", c.what, each.join("; ")));
                    }
                }
                out.push("Press ↑↓ for another item, Esc to close the menu.".into());
                return out;
            }
            Mode::Moving { to, .. } => {
                out.push(format!("Press Enter to put {} there, at {}.", self.patch_title(), to + 1));
                out.push("Press ↑↓ to place it elsewhere, Esc to leave it where it was.".into());
                return out;
            }
            Mode::MovingHead { to } => {
                out.push(format!(
                    "Press Enter to move HEAD of your checkout to {}: detached there, your branch as it is. The checkout does not change before.",
                    crate::head::place(&self.stack, *to)
                ));
                out.push("Press ↑↓ to place it elsewhere, Esc to leave it where it was.".into());
                return out;
            }
            Mode::Stream { .. } => {
                out.push("Press ↑↓ (pgup pgdn home end) to scroll what the agent wrote, Esc to fold it. Once the agent is done, the stream folds on its own.".into());
                return out;
            }
            Mode::Preview(_) => {
                out.push("These are the lines the agent points at, numbered.".into());
                out.push("Press Enter to go there: to the line in its patch when the patch's diff has it, else to the file whole, at that line; ↑↓ to scroll; Esc to go back.".into());
                return out;
            }
            _ => {}
        }
        match self.screen {
            Screen::Main if self.on_banner => {
                out.push(
                    "The banner says what waits on you on this stack, a line for each, in three sections. Read only: what landed and changes nothing, the agent's answers and the comments pulled from Phabricator, then a pull from Phabricator under way, with the revision it reads. Edits: what changes the stack, the one you review now first, then those going on from it, as if you accept it, which you review after it. Nothing of it is applied before you review it. Then the batch, the comments you asked the agent about later, to send together, and every comment open, the most recently updated first, marked as the comments pane marks it, with where it stands."
                        .into(),
                );
                out.push(match self.banner_lines().get(self.banner_sel) {
                    Some(BannerLine::Landed(_)) => "Press Enter to review this one.".to_string(),
                    Some(BannerLine::Pulling) => format!("The pull is under way: {}.", banner::PULL_BRINGS),
                    Some(BannerLine::Batch) => {
                        "Press Enter to pick what to send: the batch's edits to a single agent, its answers to a single agent or fanned out, an agent each; and which comments are in it.".into()
                    }
                    _ => "Press Enter to go to this comment.".into(),
                });
                out.push("Press ↑↓ for another line, and ↓ past the last for the stack; Space to pick one, to review, send or go to; Esc to go back where you were.".into());
            }
            Screen::Review => {
                out.push(
                    "The review of what landed, the agent's work or the comments pulled from Phabricator: the stack operations of a change, the comments opened, the replies. Each is accepted or rejected; a change of the stack is one part, and rejecting it rejects the comments too."
                        .into(),
                );
                if self.reviewing.as_ref().is_some_and(|rv| rv.in_main && rv.in_patch) && self.review_patch().is_some() {
                    out.push("This is the patch as the work leaves it. Press ↑↓ for a line, and Enter to write a comment on it, or on the comment under it; ← to go back to the stack diff.".into());
                } else if self.reviewing.as_ref().is_some_and(|rv| rv.in_main) && self.review_on_diff() {
                    out.push("This is the stack diff of the change, each new version's interdiff under it. Press ↑↓ for a line, and Enter to go to that line in its patch as the work leaves it, to write a comment there.".into());
                } else {
                    out.push("Press Enter to accept the part the cursor is on, or, accepted, to put it back to review; Space for its menu, to reject it, or to reply to the agent's reply; → to scroll what it looks like, → again to see it wide, the list out of sight, and ← to come back. On the stack operations, → goes into the stack diff, where Enter on a line goes to it in its patch as the work leaves it, to comment on it.".into());
                }
                out.push("What you write in the review of the agent's work sends that work back to the agent: the work is not accepted, and the agent is asked again, starting from this work.".into());
                if self.review_yours() > (0, 0) {
                    out.push("On a line of what you wrote, under \"what you wrote, to send back\", press Enter to write it again, and Space to take it back.".into());
                }
                out.push(if self.review_yours() > (0, 0) {
                    "Press Esc to send the work back to the agent with what you wrote, to save your marks, or to throw them away.".into()
                } else {
                    "Press Esc to save your marks, to throw them away, or to close the review: what you accepted is applied then.".into()
                });
            }
            Screen::Search => {
                out.push(
                    "The search: on the left, where it looks (all, patch, comment, history) and what it looks for, then what it finds, as it finds it; on the right, the match picked, in its context."
                        .into(),
                );
                out.push("Type what to look for; ← → change where it looks; Tab takes the search before that is suggested; ↑↓ pick a match; Enter goes there; Esc goes back.".into());
            }
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
                if let Some(a) = self.enter() {
                    out.push(format!("Press Enter to {}.", about(&a)));
                }
                let here = match self.focus {
                    Focus::List if self.on_stack => "on the stack: its comments, sending them, and the stack's own menu",
                    Focus::Notes if self.notes_head => "on the comments listed: sending them all, and what is listed",
                    _ => "here",
                };
                out.push(format!("Press Space for all that can be done {here}."));
                if !self.banner_lines().is_empty() {
                    out.push("Press Esc for the banner, open: the agent's work on this stack; Esc there comes back here.".into());
                }
                match self.focus {
                    Focus::List => out.push(
                        "Press ↑↓ for another patch, or the stack at the top; → to read it; ← for less of the patches: compact, then hidden."
                            .into(),
                    ),
                    Focus::Diff => out.push(self.arrows_help()),
                    Focus::Notes => out.push(
                        "Press ↑↓ for another comment, or the list at the top (the middle stays where it is, and shows it at its top or bottom); ← to go back; → for less of the comments: a line each, then hidden."
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
            Screen::History => {
                out.push("The history lists what changed the stack: its edits, and the moves of its head (undo, redo, select, delete); beside each, the stack diff from the stack before it to the one after: + added, − dropped, ~ a new version (what changed in it under it), ↕ moved.".into());
                match self.hist_focus {
                    HistFocus::List => {
                        out.push("Press Enter to go to the stack it made, in the tree of edits.".into());
                        out.push("Press Space to ask the agent about it, answer only, or to start a new database from there.".into());
                        out.push("Press ↑↓ for another, → to scroll its stack diff; Esc to leave the history.".into());
                    }
                    HistFocus::Diff => {
                        out.push("Press ↑↓ (pgup pgdn home end) to scroll, ← for the history. Only to look at: Esc leaves the history, to edit or comment.".into());
                    }
                }
            }
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
            ("↑", l(Key::Up)),
            ("↓", l(Key::Down)),
            ("←", l(Key::Left)),
            ("→", l(Key::Right)),
            ("Enter", l(Key::Enter)),
            ("Space", l(Key::Menu)),
            ("Esc", l(Key::Back)),
            ("enter:", format!("{}:", lower(Key::Enter))),
            ("space:", format!("{}:", lower(Key::Menu))),
            ("esc:", format!("{}:", lower(Key::Back))),
            ("?:", format!("{}:", l(Key::Help))),
            ("Press ?", format!("Press {}", l(Key::Help))),
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
        keys += if self.list.shown() { ", ← for the patches" } else { ", ← to show the patches" };
        if !self.subject_notes().is_empty() {
            keys += if self.comments.shown() { ", → for the comments" } else { ", → to show the comments" };
        }
        format!("{keys}.")
    }

    /// History…'s keys: ↑↓ in the pane the cursor is in, ← → from one to
    /// the other, enter on a record goes to it; esc leaves. The stack diff
    /// is only to look at: nothing is edited, nor asked, from it.
    fn history_key(&mut self, k: KeyEvent) {
        match (self.hist_focus, k.code) {
            (_, KeyCode::Esc) => self.screen = Screen::Main,
            (HistFocus::List, KeyCode::Right) => {
                if self.hist_entry().is_some() {
                    self.hist_focus = HistFocus::Diff;
                }
            }
            (HistFocus::List, _) => {
                let was = self.pick;
                self.list_key(k);
                if self.pick != was {
                    self.hist_cursor = 0;
                    self.hist_top = 0;
                }
            }
            (HistFocus::Diff, KeyCode::Left) => self.hist_focus = HistFocus::List,
            (HistFocus::Diff, KeyCode::Enter) => {
                self.status = "the stack diff is only to look at: esc leaves the history, to edit or comment".into();
            }
            (HistFocus::Diff, code) => {
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

    /// The record History…'s cursor is on.
    pub fn hist_entry(&self) -> Option<Entry> {
        self.history().get(self.pick).cloned()
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
        self.keep_transcript();
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
            // On an item with choices, another one picked, and kept.
            KeyCode::Left | KeyCode::Right => {
                let right = k.code == KeyCode::Right;
                let picked = m.items.get_mut(m.sel).filter(|i| i.enabled).and_then(|i| i.choices.as_mut()).map(|c| {
                    let n = c.list.len();
                    c.picked = if right { (c.picked + 1) % n } else { (c.picked + n - 1) % n };
                    (c.of, c.pick())
                });
                if let Some((of, pick)) = picked {
                    self.picked_choice(of, pick);
                    if of == "when" {
                        m.relabel(pick == "later");
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char(' ') => {
                // A comment written stays being written.
                if let Some(e) = self.pending.take() {
                    self.mode = Mode::Edit(Box::new(e));
                }
                return Effect::None;
            }
            // On the review's rows picking whether the comments that asked
            // close, and whether HEAD follows the stack: the other pick, the
            // menu staying.
            KeyCode::Enter if m.chosen().is_some_and(|i| matches!(i.action, Action::AskingAll | Action::HeadSync)) => {
                if let Some(c) = m.items.get_mut(m.sel).and_then(|i| i.choices.as_mut()) {
                    c.picked = (c.picked + 1) % c.list.len();
                    let (of, pick) = (c.of, c.pick());
                    self.picked_choice(of, pick);
                }
            }
            KeyCode::Enter => {
                // On the row picking when an ask goes, the ask.
                let item = match m.chosen() {
                    Some(i) if i.action == Action::When => m.items.get(m.sel + 1).filter(|i| i.enabled),
                    chosen => chosen,
                };
                return match item {
                    Some(item) => self.perform(item.doing()),
                    None => Effect::None,
                };
            }
            _ => {}
        }
        self.mode = Mode::Menu(m);
        Effect::None
    }

    fn edit_key(&mut self, mut e: Edit, k: KeyEvent) -> Effect {
        let changed = match e.ed.key(k) {
            Op::Stay => {
                self.mode = Mode::Edit(Box::new(e));
                return Effect::None;
            }
            Op::Close => false,
            Op::Done(_) => true,
        };
        // A reference attached or detached: changed too.
        let changed = changed || (self.attachable(&e.target) && e.attached != self.attached_of(&e.target));
        // Esc asks what to do with what is written, changed or not: it may
        // be saved and sent as it is. Not with nothing written, as nothing
        // would be saved; nor with nothing changed, if the agent cannot be
        // asked from it.
        let send = self.can_send_target(&e.target);
        let w = self.read_only().is_none();
        let ask = send && w;
        // A mention naming no reference attached: the agent is not asked
        // until it does.
        let broken = e.ed.broken();
        let named = broken.is_empty();
        let note = self.read_only().map(String::from).or_else(|| {
            (!named).then(|| format!("{} names no reference attached: attach one, or write it otherwise, to ask the agent", broken.join(", ")))
        });
        let refs: Vec<Item> = match self.attachable(&e.target) {
            true if e.attached.is_empty() => vec![Item::new("Attach a reference…", Action::AttachRef)],
            true => vec![Item::new("Attach a reference…", Action::AttachRef), Item::new("Detach a reference…", Action::DetachRef)],
            false => vec![],
        };
        if self.nothing_written(&e) || !(changed || ask) {
            self.settle_edit(&e, false);
            return Effect::None;
        }
        // Left as it was, nothing is saved nor thrown away: it is asked as
        // it is, or left so. Changed, a comment written again throws away
        // the changes alone.
        let again = matches!(e.target, Target::Edit { .. } | Target::Turn { .. });
        let rest = [
            Item::line(),
            match (changed, again) {
                (false, _) => Item::new("Leave it as it is", Action::LeaveAsItIs),
                (true, true) => Item::new("Throw the changes away", Action::ThrowChanges),
                (true, false) => Item::new("Throw it away", Action::ThrowAway),
            },
            Item::new("Keep writing", Action::KeepWriting),
        ];
        if self.answer_only(&e.target) {
            let mut items = vec![];
            if send {
                let (ask, later) = match changed {
                    true => (
                        Item::new("Save it and ask agent, answer only", Action::Save(Some(Way::Answer))).when(w && named),
                        "Save it and ask agent later, answer only",
                    ),
                    false => (
                        Item::new("Ask agent, answer only", Action::SendAsItIs(Way::Answer)).when(named),
                        "Ask agent later, answer only",
                    ),
                };
                self.push_ask(&mut items, ask, later);
            }
            if changed {
                items.push(Item::new("Save it", Action::Save(None)));
            }
            items.extend(refs);
            items.extend(rest);
            self.pending = Some(e);
            self.mode = Mode::Menu(Menu::new("the question", items, note));
            return Effect::None;
        }
        let mut items = vec![];
        if changed {
            items.push(Item::new("Save it", Action::Save(None)));
        }
        if send {
            let a_patch_s = match &e.target {
                Target::Patch(_) | Target::Line { .. } => true,
                Target::Reply { change, .. } | Target::Edit { change, .. } | Target::Turn { change, .. } | Target::Rewind { change, .. } => {
                    *change != stack_change(&self.stack)
                }
                _ => false,
            };
            let test = matches!(e.target, Target::Stack | Target::Patch(_));
            let mut choices = self.ask_choices(a_patch_s, test, true);
            if changed {
                let ask = Item::new("Save it and ask agent", Action::Save(Some(Way::Change))).when(w && named).with(choices);
                self.push_ask(&mut items, ask, "Save it and ask agent later");
            } else {
                for c in &mut choices.list {
                    c.action = c.action.take().map(as_it_is);
                }
                self.push_ask(&mut items, Item::new("Ask agent", Action::SendAsItIs(Way::Change)).when(named).with(choices), "Ask agent later");
            }
        }
        items.extend(refs);
        items.extend(rest);
        self.pending = Some(e);
        self.mode = Mode::Menu(Menu::new("the comment", items, note));
        Effect::None
    }

    /// What Esc does in the editor `e`, as `edit_key` decides it: changed,
    /// it asks whether to save, send or throw away; left as it was, whether
    /// to send it as it is; else it closes the editor.
    fn esc_does(&self, e: &Edit) -> &'static str {
        let ask = self.can_send_target(&e.target) && self.read_only().is_none();
        match (self.nothing_written(e), e.ed.modified(), ask) {
            (false, true, true) => "esc: save, send or throw away",
            (false, true, false) => "esc: save or throw away",
            (false, false, true) => "esc: send as it is, or leave it",
            _ => "esc: close",
        }
    }

    /// Whether the editor `e` holds nothing that saving it would keep: no
    /// text, or, as the conversation is rewound, only the lines stacked
    /// wrote.
    fn nothing_written(&self, e: &Edit) -> bool {
        let text = e.ed.text();
        if let Target::Rewind { change, id, from } = &e.target
            && let Some(n) = self.db.note(change, id)
        {
            return without_written(&text, &rewind_written(&n.text, *from)).is_empty();
        }
        text.trim().is_empty()
    }

    /// The turns of a comment that the editor writes again, so that its
    /// rows do not show them too: the comment's id, then from which turn,
    /// to the one after the last.
    fn written_turns(&self) -> Option<(String, (usize, usize))> {
        match &self.writing()?.target {
            Target::Edit { change, id } => {
                let k = last_mine_at(&self.db.note(change, id)?.text)?;
                Some((id.clone(), (k, k + 1)))
            }
            Target::Turn { id, turn, .. } => Some((id.clone(), (*turn, *turn + 1))),
            Target::Rewind { id, from, .. } => Some((id.clone(), (*from, usize::MAX))),
            _ => None,
        }
    }

    /// Whether a comment written for `t` can be sent once saved: not while
    /// claude works on the thread; a turn written again, if the last word
    /// is yours, as it is for asking. Not in a review, as what is written
    /// there goes back to the agent with the work.
    fn can_send_target(&self, t: &Target) -> bool {
        if self.screen == Screen::Review {
            return false;
        }
        match t {
            // Rewound, the last word is yours.
            Target::Reply { change, id } | Target::Edit { change, id } | Target::Rewind { change, id, .. } => {
                !self.busy().contains_key(&(change.clone(), id.clone()))
            }
            Target::Turn { change, id, .. } => {
                !self.busy().contains_key(&(change.clone(), id.clone())) && self.db.note(change, id).is_some_and(ask::sendable)
            }
            _ => true,
        }
    }

    /// Whether what is written for `t` may only be asked to answer: a
    /// question on the history, or a turn of one.
    fn answer_only(&self, t: &Target) -> bool {
        match t {
            Target::About(_) => true,
            Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. } => {
                self.db.note(change, id).is_some_and(|n| n.about.is_some())
            }
            _ => false,
        }
    }

    /// What the turn `t` writes again says now: what you said last, or the
    /// turn it is.
    fn mine_of(&self, t: &Target) -> Option<String> {
        match t {
            Target::Edit { change, id } => last_mine(&self.db.note(change, id)?.text),
            Target::Turn { change, id, turn } => {
                all_turns(&self.db.note(change, id)?.text).into_iter().nth(*turn).filter(|t| !t.deleted).map(|t| t.body)
            }
            _ => None,
        }
    }

    /// Opens what you said last on comment (change, id) in an editor in
    /// `pane`, to write it again or add to it.
    fn edit_mine(&mut self, change: String, id: String, pane: Focus) {
        self.write_again(Target::Edit { change, id }, pane);
    }

    /// Opens the turn of a comment `t` writes again in an editor in `pane`,
    /// as `write` does: not while the agent works on it, off its queue
    /// meanwhile.
    fn write_again(&mut self, t: Target, pane: Focus) {
        let Some(text) = self.mine_of(&t) else { return };
        self.write(t, pane, &text);
    }

    /// Once a comment is written on, or not: taken off the agent's queue
    /// for it, it goes back on as it was, or with a reply `changed` it; a
    /// turn written again waits to be asked again.
    fn settle_edit(&mut self, e: &Edit, changed: bool) {
        let (
            Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. },
            Some(q),
        ) = (&e.target, &e.held)
        else {
            return;
        };
        let reply = matches!(e.target, Target::Reply { .. });
        if changed && !reply {
            self.status = "edited: off the agent's queue, until you ask again".into();
            return;
        }
        match ask::requeue(&self.path, change, id, q.clone()) {
            Ok(()) if changed => self.status = format!("your reply is added: back on the agent's queue ({})", q.said()),
            Ok(()) => self.status = format!("as it was: back on the agent's queue ({})", q.said()),
            Err(err) => self.status = err.0,
        }
        self.reload();
        self.launch();
    }

    /// A key while HEAD is being moved: ↑↓ place it, and only Enter moves
    /// the checkout, once the mode is left; Esc leaves it where it was.
    fn head_key(&mut self, to: usize, k: KeyEvent) {
        let last = self.patch_count();
        let to = match k.code {
            KeyCode::Down => (to + 1).min(last),
            KeyCode::Up => to.saturating_sub(1),
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::Esc => return,
            KeyCode::Enter => return self.move_head(to),
            _ => to,
        };
        self.mode = Mode::MovingHead { to };
    }

    /// HEAD of the checkout stacked runs in moved to the current stack with
    /// its first `n` patches, away from the screen: detached there, the
    /// status line saying what came of it.
    fn move_head(&mut self, n: usize) {
        self.move_head_after(n, String::new());
    }

    /// As `move_head`, the status line saying `said` first.
    pub(super) fn move_head_after(&mut self, n: usize, said: String) {
        if self.bg.doing.contains(&MOVING_HEAD) {
            self.status = format!("{said}HEAD is being moved already");
            return;
        }
        self.bg.doing.push(MOVING_HEAD);
        self.status = format!("{said}moving HEAD to {}…", crate::head::place(&self.stack, n));
        let (path, name, here) = (self.path.clone(), self.stack.clone(), self.checkout.clone());
        self.spawn(move || Done::Said(MOVING_HEAD, format!("{said}{}", crate::head::move_to(&path, &name, &here, n).unwrap_or_else(|e| e.0))));
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
                // Taken by the agent meanwhile: no place would do.
                if let Some(why) = self.by_hand() {
                    self.status = why;
                    return;
                }
                let name = self.stack.clone();
                if self
                    .edit(|db| {
                        ops::by_hand(db, &name)?;
                        ops::move_patch(db, &name, from, to)
                    })
                    .is_some()
                {
                    self.sel = to;
                    return;
                }
                // Refused, and the status line says why: it can go elsewhere.
            }
            _ => {}
        }
        self.mode = Mode::Moving { from, to };
    }

    /// What the agent writes, read in place, folded once the agent is done
    /// with its comment, for the keys to be the screen's again; and said,
    /// what it brought too when that waits for your review (what landed
    /// says so after). Read from the help, it is folded there, for the
    /// help to go back to the screen.
    fn fold_done(&mut self) {
        let busy = self.busy();
        let done = |m: &Mode| match m {
            Mode::Stream { change, id, .. } if !busy.contains_key(&(change.clone(), id.clone())) => Some((change.clone(), id.clone())),
            _ => None,
        };
        let at = match &mut self.mode {
            Mode::Help { back, .. } => {
                let at = done(back);
                if at.is_some() {
                    **back = Mode::Normal;
                }
                at
            }
            m => {
                let at = done(m);
                if at.is_some() {
                    *m = Mode::Normal;
                }
                at
            }
        };
        let Some((change, id)) = at else { return };
        self.status = match crate::held::reply_waiting(&self.db, &change, &id) {
            Some(_) => "the agent is done: what it brought waits for your review".into(),
            None => "the agent is done: what it wrote is folded".into(),
        };
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

    /// Reply `k` that run `run` holds, said: `the agent's reply`, `ann's
    /// edit`.
    fn reply_said(&self, run: &str, k: usize) -> String {
        self.db.run(run).and_then(|r| r.held.as_ref()).and_then(|h| h.replies.get(k)).map_or("the agent's reply".into(), |rp| rp.said())
    }

    /// Whether `?` says what can be done where the cursor is: not on the
    /// main screen without a stack, where there is nothing to do but what
    /// the menu has.
    fn helps(&self) -> bool {
        !(self.screen == Screen::Main && self.current().is_none())
    }

    /// The comments listed in the comments pane, of the stack's or the
    /// patch's: what Send all sends, the sendable ones.
    fn listed_sendable(&self, way: Way) -> Vec<(String, String)> {
        let busy = self.busy();
        let held = self.held_replies();
        self.notes()
            .iter()
            .filter_map(|(_, id)| self.db.note(&self.subject_change()?, id))
            .filter(|n| ask::sendable(n) && !busy.contains_key(&(n.change.clone(), n.id.clone())))
            .filter(|n| !held.contains_key(&(n.change.clone(), n.id.clone())))
            .filter(|n| way != Way::Test || ask::can_test(&self.db, n))
            // On the history: asked about to answer only.
            .filter(|n| way == Way::Answer || n.about.is_none())
            .map(|n| (n.change.clone(), n.id.clone()))
            .collect()
    }

    /// The items for the menu's end, everywhere: undo, redo, the view, the
    /// runs, quit.
    fn common(&self, items: &mut Vec<Item>) {
        let h = self.by_hand().is_none();
        items.push(Item::line());
        // Nothing is edited from History…, nor from a review: only looked at.
        let edits = !matches!(self.screen, Screen::History | Screen::Review);
        if edits && (self.current().is_some() || self.db.names().contains(&self.stack.as_str())) {
            if let Some((_, op)) = self.db.undo_target(&self.stack) {
                items.push(Item::new(format!("Undo: {op}"), Action::Undo).when(h));
            }
            if let Some((_, op)) = self.db.redo_target(&self.stack) {
                items.push(Item::new(format!("Redo: {op}"), Action::Redo).when(h));
            }
        }
        if self.current().is_some() && self.screen != Screen::Search {
            items.push(Item::new("Search…", Action::Search));
        }
        if self.db.has_runs() {
            let label = match self.running() {
                0 => "Runs…".to_string(),
                n => format!("Runs… (agent ●{n})"),
            };
            items.push(Item::new(label, Action::Runs));
        }
        items.push(Item::line());
        items.push(Item::new("Quit", Action::Quit));
    }

    /// Whose message a patch keeps when its commit in the workspace says
    /// another, as radio buttons: the stack's, put on the commit; or the
    /// workspace's, taken into the stack. Of the patch at `at`, else of
    /// every one whose message differs.
    fn sync_choices(&self, at: Option<usize>) -> Choices {
        let list = vec![
            Choice::new(
                "stacked",
                "stacked → workspace",
                "the stack's message is kept, and written over your workspace's commit's",
                Some(Action::MessagesToCheckout(at)),
            ),
            Choice::new(
                "workspace",
                "workspace → stacked",
                "your workspace's message is kept, and written over the patch's in the stack, for your review",
                Some(Action::MessagesToStack(at)),
            ),
        ];
        Choices::new("sync", "which way the message goes", list, self.db.choice(&self.stack, "sync"))
    }

    /// What the menu says of syncing commit messages, where it has the item
    /// to: on a patch whose message is not its commit's in the workspace,
    /// both, from the first line they differ by, and what each way does; on
    /// the stack's header, which patches differ.
    fn sync_said(&self) -> Option<String> {
        if self.screen != Screen::Main || self.focus != Focus::List || self.on_banner {
            return None;
        }
        let h = self.head_here()?;
        let crate::head::At::Stack(on) = &h.at else { return None };
        let ways = "The message on the left of the arrow is kept, and it is written over the one on the right. \
                    stacked → workspace makes your workspace's commit again with the stack's message. \
                    workspace → stacked makes a new version of the patch with your workspace's message, for you to review.";
        if self.on_stack {
            let differ = h.differ();
            return (!differ.is_empty()).then(|| {
                format!("The commit messages of {} are not the same in the stack and in your workspace.\n{ways}", crate::head::patches_said(&differ))
            });
        }
        let (_, theirs) = on.messages.iter().find(|(k, _)| *k == self.sel)?;
        let ours = self.db.patch(self.current()?.patches.get(self.sel)?).message.clone();
        let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
        let k = (0..a.len().max(b.len())).find(|&k| a.get(k).map(|l| l.trim_end()) != b.get(k).map(|l| l.trim_end())).unwrap_or(0);
        let from = if k == 0 { String::new() } else { format!(", from its line {}", k + 1) };
        let line = |l: Option<&&str>| l.map_or("(no such line)".to_string(), |l| l.trim_end().to_string());
        Some(format!(
            "Its commit message is not the same in the stack and in your workspace{from}.\nstacked: {}\nworkspace: {}\n{ways}",
            line(a.get(k)),
            line(b.get(k))
        ))
    }

    /// Where a file of a patch is edited by hand, as radio buttons: a copy
    /// of it alone, this checkout, or the stack's own worktree.
    fn edit_choices(&self) -> Choices {
        let list = vec![
            Choice::new("tmp", "tmp/file", "a copy of the file alone, in a temporary folder", None),
            Choice::new("pwd", "pwd", "this checkout, moved to the patch for the edit, then back where it was", None),
            Choice::new(
                "worktree",
                format!(".stacked/{}/user", self.stack),
                format!("the stack's own worktree, .stacked/{}/user in its repository, kept between edits", self.stack),
                None,
            ),
        ];
        Choices::new("edit", "where", list, self.db.choice(&self.stack, "edit"))
    }

    /// How the agent is asked about a comment, as radio buttons: to change
    /// the code (and `whole`, for a patch's: the whole stack), to answer
    /// only, or (`test`) to build and test. Each asks, or, `saving` a
    /// comment being written, saves it and asks.
    fn ask_choices(&self, whole: bool, test: bool, saving: bool) -> Choices {
        let act = |way: Way| Some(if saving { Action::Save(Some(way)) } else { Action::Send(way) });
        let mut list = vec![Choice::new("change", "change", "it may change the code within the comment's place", act(Way::Change))];
        if whole {
            let a = if saving { Action::SaveWhole } else { Action::SendWhole };
            list.push(Choice::new(
                "whole",
                "the whole stack",
                "a change with the whole stack in its worktree: it may change any patch, add or reorder them",
                Some(a),
            ));
        }
        list.push(Choice::new("answer", "answer only", "it answers, and changes nothing", act(Way::Answer)));
        if test {
            list.push(Choice::new(
                "test",
                "build and test",
                "it builds and tests in the stack's build worktree, and changes nothing",
                act(Way::Test),
            ));
        }
        Choices::new("ask", "how", list, self.db.choice(&self.stack, "ask"))
    }

    /// When the agent is asked, as radio buttons: now, or later, the comment
    /// waiting in the stack's batch until the banner sends it.
    fn when_choices(&self) -> Choices {
        let list = vec![
            Choice::new("now", "now", "the agent is asked at once", None),
            Choice::new("later", "later", "the comment waits in the batch, which the banner sends with the rest", None),
        ];
        Choices::new("when", "when", list, self.db.choice(&self.stack, "when"))
    }

    /// Whether the stack's asks are made later, in its batch, as its menus
    /// picked.
    fn later(&self) -> bool {
        self.db.choice(&self.stack, "when") == Some("later")
    }

    /// Ask `ask`, under the row that picks when it asks, labelled `later`
    /// while it asks later.
    fn push_ask(&self, items: &mut Vec<Item>, mut ask: Item, later: &str) {
        items.push(Item::new("Send it", Action::When).when(ask.enabled).with(self.when_choices()));
        let now = std::mem::take(&mut ask.label);
        ask.label = if self.later() { later.to_string() } else { now.clone() };
        ask.labels = Some((now, later.to_string()));
        items.push(ask);
    }

    /// Where a comment is written, as radio buttons: here, in an editor in
    /// place (`here`), or in $EDITOR (`editor`).
    fn write_choices(&self, here: Action, editor: Action) -> Choices {
        let list = vec![
            Choice::new("here", "here", "in an editor in place, where it will show", Some(here)),
            Choice::new("editor", "in $EDITOR", "in $EDITOR, a file of its own", Some(editor)),
        ];
        Choices::new("write", "where", list, self.db.choice(&self.stack, "write"))
    }

    /// Where this stack's files are edited by hand, as its menus picked.
    fn edit_place(&self) -> ops::Place {
        ops::Place::named(self.db.choice(&self.stack, "edit").unwrap_or("tmp"))
    }

    /// `pick`, kept for this stack as its choice of `of`.
    fn pick_choice(&mut self, of: &str, pick: &str) {
        let name = self.stack.clone();
        self.edit(|db| db.set_choice(&name, of, pick));
    }

    /// A choice picked in a menu: kept for the stack; but the review's own
    /// rows, not kept for the stack: the comments that asked marked so in
    /// the review, and whether HEAD follows the stack kept for the review.
    fn picked_choice(&mut self, of: &str, pick: &str) {
        match of {
            "asking" => self.mark_all_asking(pick == "close"),
            "head" => self.head_stays(pick == "stay"),
            _ => self.pick_choice(of, pick),
        }
    }

    /// All that can be done where the cursor is, then the menu's end.
    pub fn main_menu(&self) -> Menu {
        let w = self.read_only().is_none();
        // Edited by hand: not while the agent changes the stack.
        let hands = self.by_hand();
        let h = hands.is_none();
        let n = self.patch_count();
        let mut title = self.stack.clone();
        let mut items = Vec::new();
        match (self.screen, self.focus) {
            // What landed, to review; the comments open, to go to.
            (Screen::Main, _) if self.on_banner => {
                title = "what waits on you".into();
                self.banner_items(&mut items);
            }
            (Screen::Review, _) => {
                title = "this part of what landed".into();
                self.review_items(&mut items);
            }
            (Screen::Main, Focus::List) if self.on_stack => {
                title = format!("the stack {}", self.stack);
                if self.current().is_some() {
                    items.push(Item::new("Comment on the stack…", Action::CommentStack));
                    self.send_all_items(&mut items, true);
                    items.push(Item::new("Show…", Action::ShowMenu));
                    items.push(Item::line());
                    items.push(Item::new("Edit the stack as a list in $EDITOR", Action::EditList).when(h && n > 0));
                    items.push(Item::new("Tree of edits…", Action::Tree));
                    items.push(Item::new("History…", Action::History));
                    if self.cfg.reviews.source != crate::config::Source::None {
                        items.push(Item::new("Pull reviews from Phabricator", Action::Pull));
                    }
                    items.push(Item::new("Apply the stack to the worktree…", Action::AskApply).when(w));
                    if self.head_here().is_some() {
                        items.push(Item::new("Move HEAD…", Action::MoveHead(None)));
                    }
                    // The patches whose message differs from their commit's.
                    if self.head_here().is_some_and(|hd| !hd.differ().is_empty()) {
                        items.push(Item::new("Sync commit messages", Action::MessagesToCheckout(None)).with(self.sync_choices(None)));
                    }
                    items.push(Item::line());
                }
                if self.db.stacks().iter().any(|s| s.name != self.stack) {
                    items.push(Item::new("Switch stack…", Action::Stacks));
                }
                if self.current().is_some() {
                    items.push(Item::new("Delete this stack…", Action::AskDeleteStack).when(h));
                }
            }
            (Screen::Main, Focus::List) if n > 0 => {
                title = self.patch_title();
                items.push(Item::new("Read this patch", Action::Open));
                items.push(Item::new("Comment on this patch…", Action::CommentPatch));
                items.push(Item::new("Move this patch…", Action::MovePatch).when(h && n > 1));
                if self.head_here().is_some() {
                    items.push(Item::new("Move HEAD…", Action::MoveHead(Some(self.sel))));
                }
                // Its message differs from its commit's in the checkout.
                if self.head_here().is_some_and(|hd| hd.differ().contains(&self.sel)) {
                    let at = Some(self.sel);
                    items.push(Item::new("Sync commit message", Action::MessagesToCheckout(at)).with(self.sync_choices(at)));
                }
                items.push(Item::new("Edit this patch in $EDITOR", Action::EditPatch).when(h));
                items.push(Item::new("Drop this patch…", Action::AskDrop).when(h));
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
                    if self.is_closed(&c) {
                        title = format!("{title}, closed");
                    }
                    self.comment_items(&c, &mut items, w);
                } else {
                    if self.commentable().is_some() {
                        title = format!("line: {}", self.rows.get(self.cursor).and_then(|r| r.preview.clone()).unwrap_or_default());
                        let choices = self.write_choices(Action::CommentLine, Action::CommentInEditor);
                        items.push(Item::new("Comment on this line", Action::CommentLine).with(choices));
                    }
                    // Edited by hand, where the cursor is.
                    if self.focus == Focus::Diff && !self.on_stack && self.patch_id().is_some() {
                        items.push(Item::line());
                        if let Some((file, _)) = self.code_place() {
                            let short = file.rsplit('/').next().unwrap_or(&file).to_string();
                            items.push(Item::new(format!("Edit {short} here in $EDITOR"), Action::EditCode).when(h).with(self.edit_choices()));
                        }
                        items.push(Item::new("Edit this patch in $EDITOR", Action::EditPatch).when(h));
                    }
                }
            }
            (Screen::Tree, _) => {
                if let Some(l) = tree::graph(&self.db, &self.stack).get(self.pick) {
                    title = format!("@{}", l.rev);
                    items.push(Item::new("Edit from this one", Action::EditFrom(l.rev)).when(h));
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
                    items.push(Item::new("Put this version back in the stack", Action::UseVersion(p.id.clone())).when(h && !current));
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
        // In a review, what waits is what is reviewed, and edited by hand
        // from there: nothing it holds back is said.
        let mut m = Menu::new(title, items, hands.filter(|_| self.screen != Screen::Review));
        // Its commit message to sync: both said, and which way does what.
        if let Some(said) = self.sync_said() {
            m.note = Some(match m.note.take() {
                Some(n) => format!("{n}\n{said}"),
                None => said,
            });
        }
        m
    }

    /// The Send all items of a header: the comments listed that can be sent.
    fn send_all_items(&self, items: &mut Vec<Item>, test: bool) {
        let w = self.read_only().is_none();
        let n = self.listed_sendable(Way::Change).len();
        let t = if test { self.listed_sendable(Way::Test).len() } else { 0 };
        let mut list = vec![
            Choice::new("change", format!("change ({n})"), "each may change the code within its place", Some(Action::SendAll(Way::Change))),
            Choice::new("answer", format!("answer only ({n})"), "each is answered, and nothing changes", Some(Action::SendAll(Way::Answer))),
        ];
        if test {
            list.push(Choice::new(
                "test",
                format!("build and test ({t})"),
                "the agent builds and tests, for each, and changes nothing",
                Some(Action::SendAll(Way::Test)),
            ));
        }
        let choices = Choices::new("ask", "how", list, self.db.choice(&self.stack, "ask"));
        items.push(Item::new("Ask agent about all", Action::SendAll(Way::Change)).when(w && (n > 0 || t > 0)).with(choices));
    }

    fn comment_items(&self, c: &Note, items: &mut Vec<Item>, w: bool) {
        // Closed: to reopen as it was, or to see what it went through.
        if self.is_closed(c) {
            items.push(Item::new("Reopen it", Action::Reopen));
            items.push(Item::new("History of this comment…", Action::NoteHistory));
            return;
        }
        let busy = self.busy();
        let with = busy.get(&(c.change.clone(), c.id.clone()));
        // Its reply from the agent waiting for review: nothing is written on
        // it before.
        let held = self.held_replies().get(&(c.change.clone(), c.id.clone())).cloned();
        let open = with.is_none() && held.is_none();
        // Collapsed: expanded first, as enter does.
        let collapsed = self.db.collapsed(&c.change, &c.id);
        if collapsed {
            items.push(Item::new("Expand this comment", Action::Expand));
        }
        // The turn the cursor is on, in the middle, may be written again,
        // whoever said it; elsewhere on the comment, what you said last, if
        // the last word is yours, as enter does. Not while the agent works
        // on it, or its reply waits.
        let here = self.turn_here(c);
        let last = last_mine_at(&c.text);
        match &here {
            Some((i, t)) if Some(*i) != last => {
                let choices = self.write_choices(Action::EditTurn(*i), Action::EditTurnInEditor(*i));
                items.push(Item::new(format!("Edit {}", t.name(*i)), Action::EditTurn(*i)).when(open).with(choices));
            }
            _ if last.is_some() => {
                let what = if thread_turns(&c.text).len() > 1 { "reply" } else { "comment" };
                let choices = self.write_choices(Action::EditMine, Action::EditMineInEditor);
                items.push(Item::new(format!("Edit your {what}"), Action::EditMine).when(open).with(choices));
            }
            _ => {}
        }
        items.push(
            Item::new("Reply", Action::Reply)
                .when(open)
                .with(self.write_choices(Action::Reply, Action::CommentInEditor)),
        );
        // Rewound from your turn there, or from yours above it: what comes
        // after it goes. Not from your last word, which Edit writes again.
        if let Some((i, _)) = &here
            && let Some(from) = rewind_from(&c.text, *i)
            && Some(from) != last
        {
            let what = all_turns(&c.text)[from].name(from);
            let choices = self.write_choices(Action::Rewind(*i), Action::RewindInEditor(*i));
            items.push(Item::new(format!("Rewind the conversation from {what}"), Action::Rewind(*i)).when(open).with(choices));
        }
        items.push(Item::line());
        match with {
            Some((_, run)) => {
                items.push(Item::new("Read what the agent is writing", Action::OpenStream));
                items.push(Item::new("Stop the agent…", Action::AskStop(run.clone())));
            }
            // Taken at once into the thread, as enter does; not a reply going
            // with a change of the stack, reviewed with it.
            None if held.is_some() => {
                let (run, k) = held.clone().unwrap();
                let r = self.db.run(&run);
                let alone = r.and_then(|r| r.held.as_ref()).is_some_and(|h| h.stack.is_none());
                if alone {
                    items.push(Item::new(format!("Accept {}", self.reply_said(&run, k)), Action::TakeReply(run.clone(), k)));
                }
                let review = if r.is_some_and(crate::held::pulled) { "Review the comments pulled…" } else { "Review the agent's work…" };
                items.push(Item::new(review, Action::Review(run, Some(crate::held::Part::Reply(k)))));
            }
            // In the batch: sent now, on its own, or taken out.
            None if c.queued.as_ref().is_some_and(|q| q.batched()) => {
                let said = c.queued.as_ref().map_or("", |q| q.said());
                items.push(Item::new(format!("Send it now ({said})"), Action::SendNow).when(w));
                items.push(Item::new("Remove it from the batch", Action::Unbatch));
            }
            None if c.queued.is_some() => items.push(Item::new("Take it back from the agent's queue", Action::Unsend)),
            None if c.about.is_some() => {
                let ask = Item::new("Ask agent, answer only", Action::Send(Way::Answer)).when(w && ask::sendable(c));
                self.push_ask(items, ask, "Ask agent later, answer only");
            }
            None => {
                let s = ask::sendable(c);
                let choices = self.ask_choices(!c.on_stack(), ask::can_test(&self.db, c), false);
                self.push_ask(items, Item::new("Ask agent", Action::Send(Way::Change)).when(w && s).with(choices), "Ask agent later");
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
            items.push(Item::new("Close it and the comments it opened…", Action::AskCloseOpened).when(open));
        }
        if let Some(m) = &c.moved {
            items.push(Item::new("Go to where it came from", Action::GoToOrigin));
            let back = self.where_was(m).split(" · ").next().unwrap_or("its patch").to_string();
            items.push(Item::new(format!("Move it back to {back}"), Action::MoveBack).when(open));
        }
        if !collapsed {
            items.push(Item::new("Collapse this comment", Action::Collapse));
        }
        items.push(Item::new("History of this comment…", Action::NoteHistory));
        items.push(Item::new("Close this comment…", Action::AskCloseComment).when(open));
    }

    fn perform(&mut self, action: Action) -> Effect {
        let name = self.stack.clone();
        // Kept in memory, as the demo's database: nothing that writes on
        // disk, or reaches Phabricator.
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path)
            && matches!(action, Action::Pull | Action::AskApply | Action::Apply | Action::AskTrim(_) | Action::Trim(_))
        {
            self.status = "not in the demo: this would write on disk, or reach Phabricator".into();
            return Effect::None;
        }
        match action {
            Action::Line | Action::Cancel => {}
            Action::Open => {
                self.screen = Screen::Main;
                self.focus = Focus::Diff;
            }
            Action::GoToNote => self.go_to_note(),
            Action::MessagesToStack(at) => {
                let places: Vec<usize> = at.into_iter().collect();
                match crate::head::messages_to_stack(&self.path, &self.stack, &self.checkout, &places) {
                    Ok(done) => self.edited(done, None, None),
                    Err(e) => self.status = e.0,
                }
                self.look_at_head();
            }
            Action::MessagesToCheckout(at) => {
                if self.bg.doing.contains(&REWORDING) {
                    self.status = "the checkout's commits are being made again already".into();
                } else {
                    let places: Vec<usize> = at.into_iter().collect();
                    self.bg.doing.push(REWORDING);
                    self.status = "putting the stack's messages on your checkout…".into();
                    let (path, name, here) = (self.path.clone(), self.stack.clone(), self.checkout.clone());
                    self.spawn(move || {
                        Done::Said(REWORDING, crate::head::messages_to_checkout(&path, &name, &here, &places).unwrap_or_else(|e| e.0))
                    });
                }
            }
            Action::MoveHead(at) => {
                let to = at.map(|at| at + 1).or_else(|| self.head_here().and_then(|h| h.patches()));
                self.mode = Mode::MovingHead {
                    to: to.unwrap_or(self.patch_count()),
                };
            }
            Action::MovePatch => match self.by_hand() {
                Some(why) => self.status = why,
                None => {
                    self.mode = Mode::Moving {
                        from: self.sel,
                        to: self.sel,
                    }
                }
            },
            Action::EditPatch => return Effect::EditPatch { line: self.mail_place() },
            Action::EditCode => {
                if let Some((file, line)) = self.code_place() {
                    return Effect::EditCode { file, line, again: None };
                }
            }
            Action::EditAgain => {
                self.conflict = None;
                if let Some((file, line, kept)) = self.refused.take() {
                    // Of landed work, in its review: there again.
                    if let Some((run, at)) = self.refused_work.take() {
                        let again = Some(kept);
                        return Effect::EditWork {
                            run,
                            edit: WorkEdit::Code { at, file, line, again },
                        };
                    }
                    return Effect::EditCode {
                        file,
                        line,
                        again: Some(kept),
                    };
                }
            }
            Action::EditWorkList | Action::EditWorkPatch(_) | Action::EditWorkFile(..) => {
                if let Some(e) = self.work_edit(&action) {
                    return e;
                }
            }
            Action::Resolve => return Effect::Resolve,
            Action::ThrowEdit => {
                self.refused = None;
                self.refused_work = None;
                self.conflict = None;
                self.status = "your edit is thrown away".into();
            }
            Action::AskDrop => {
                let title = format!("drop {}?", self.patch_title());
                self.mode = Mode::Menu(Menu::ask(title, Item::new("Drop it", Action::Drop), "Keep it", false));
            }
            Action::Drop => {
                let at = self.sel;
                self.edit(|db| {
                    ops::by_hand(db, &name)?;
                    ops::drop_patches(db, &name, &[at])
                });
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
                    // Yours on the line already: what you said last written
                    // again, or added to, if the last word is yours; else a
                    // reply.
                    match self.db.note(&change, &key).map(|n| last_mine(&n.text).is_some()) {
                        Some(true) => self.edit_mine(change, key, Focus::Diff),
                        Some(false) => self.write(Target::Reply { change, id: key }, Focus::Diff, ""),
                        None => self.write(Target::Line { change, key, file, preview }, Focus::Diff, ""),
                    }
                }
            }
            Action::Reply => {
                if let Some(c) = self.picked() {
                    self.write(Target::Reply { change: c.change, id: c.id }, self.focus, "");
                }
            }
            Action::EditMine => {
                if let Some(c) = self.picked() {
                    self.edit_mine(c.change, c.id, self.focus);
                }
            }
            Action::EditMineInEditor => {
                if let Some(c) = self.picked()
                    && last_mine(&c.text).is_some()
                {
                    return Effect::EditComment(Target::Edit { change: c.change, id: c.id });
                }
            }
            Action::EditTurn(turn) => {
                if let Some(c) = self.picked() {
                    self.write_again(Target::Turn { change: c.change, id: c.id, turn }, self.focus);
                }
            }
            Action::EditTurnInEditor(turn) => {
                if let Some(c) = self.picked() {
                    return Effect::EditComment(Target::Turn { change: c.change, id: c.id, turn });
                }
            }
            // The editor empty for the new text, at its top; under it, as #
            // lines, what goes once it is saved.
            Action::Rewind(turn) => {
                if let Some(c) = self.picked()
                    && let Some(from) = rewind_from(&c.text, turn)
                {
                    let goes = rewound_lines(&c.text, from).join("\n");
                    self.write(Target::Rewind { change: c.change, id: c.id, from }, self.focus, &format!("\n{goes}"));
                    if let Mode::Edit(e) = &mut self.mode {
                        e.ed.to_start();
                    }
                }
            }
            Action::RewindInEditor(turn) => {
                if let Some(c) = self.picked()
                    && let Some(from) = rewind_from(&c.text, turn)
                {
                    return Effect::EditComment(Target::Rewind { change: c.change, id: c.id, from });
                }
            }
            Action::TakeReply(run, k) => {
                let said = self.reply_said(&run, k);
                if self.edit(|db| crate::held::take(db, &run, k)).is_some() {
                    self.status = format!("you accepted {said}: it is in the thread now");
                    self.launch();
                }
            }
            Action::Review(run, part) => self.open_review(&run, part),
            Action::MarkPart(accept) => {
                if let Some(p) = self.review_part() {
                    self.mark(p, accept);
                }
            }
            Action::MarkOpened(accept) => self.mark_all(false, accept),
            Action::MarkReplies(accept) => self.mark_all(true, accept),
            Action::ReviewSave => self.review_save(),
            Action::ReviewDiscard => {
                self.leave_review();
                self.status = "the marks you made since you opened the review are thrown away".into();
            }
            Action::AskReviewClose(accept) => self.mode = Mode::Menu(self.review_close_ask(accept)),
            Action::ReviewClose(rest) => self.review_close(rest),
            Action::CloseAsking(k, close) => self.mark_asking(k, close),
            Action::SendBack => self.review_send_back(),
            Action::AskClose => {
                if let Some(m) = self.review_close_menu() {
                    self.mode = Mode::Menu(m);
                }
            }
            Action::AskSendBack => self.mode = Mode::Menu(self.review_send_back_ask()),
            // Their rows pick in the menu itself.
            Action::AskingAll | Action::HeadSync => {}
            Action::ShowRef(r) => self.show_ref(&r),
            Action::GoToPointed(change, id) => match crate::refs::find_comment(&self.db, &self.stack, &change, &id) {
                Some((n, true)) => self.go_to(&n.change, &n.id),
                Some((n, false)) => {
                    let lines = crate::thread::said_lines(&n.text).into_iter().map(|l| (l, Style::new())).collect();
                    self.open_text(format!("a closed comment · {}", where_on(&self.db, &self.stack, &n)), lines);
                }
                None => self.status = "that comment is gone".into(),
            },
            Action::Named => {
                if let Some(a) = self.named() {
                    return self.perform(a);
                }
            }
            Action::AttachRef => self.attach_menu(),
            Action::DetachRef => self.detach_menu(),
            Action::AttachComment(change, id) => {
                if let Some(mut e) = self.pending.take() {
                    let n = self.next_number(&e);
                    e.attached.push(crate::refs::Attached {
                        n,
                        to: crate::refs::To::Comment { change, id },
                    });
                    self.status = format!("attached as r{n}: write @r{n} where you mean it");
                    self.show_attached(&mut e);
                    self.mode = Mode::Edit(Box::new(e));
                }
            }
            Action::AttachLines => {
                if let Some(e) = self.pending.take() {
                    let (text, col) = self.lines_prefill(&e.target);
                    let mut ed = Editor::new(&text).single();
                    ed.set_col(col);
                    self.mode = Mode::Lines(Box::new(Lines { ed, back: e }));
                }
            }
            Action::Detach(n) => {
                if let Some(mut e) = self.pending.take() {
                    if self.named_elsewhere(&e.target, n) {
                        self.status = format!("r{n} is named in another turn of the comment: it stays attached");
                    } else {
                        e.attached.retain(|a| a.n != n);
                        self.status = format!("r{n} is detached");
                    }
                    self.show_attached(&mut e);
                    self.mode = Mode::Edit(Box::new(e));
                }
            }
            Action::ReviewCloseStop(rest) => {
                let builds = ask::builds_going(&self.db, &self.stack);
                self.review_close(rest);
                if self.reviewing.is_none() && !builds.is_empty() {
                    for r in &builds {
                        ask::stop(r);
                    }
                    self.status += "; the build and test of the stack as it was is stopped";
                }
            }
            Action::Answer(k) => self.review_answer(k),
            Action::EditYours => {
                if let Some(y) = self.review_yours_line() {
                    self.review_write_yours(y);
                }
            }
            Action::TakeYoursBack => {
                if let Some(y) = self.review_yours_line() {
                    self.review_take_back(y);
                }
            }
            Action::Collapse | Action::Expand => {
                let collapse = matches!(action, Action::Collapse);
                if let Some(c) = self.picked()
                    && self.edit(|db| db.set_collapsed(&c.change, &c.id, collapse)).is_some()
                {
                    // The cursor stays on it, shorter or longer.
                    self.want = Some(Want::Note(c.id));
                    if collapse {
                        self.status = "the comment is collapsed: press Enter on its line to expand it".into();
                    }
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
                    self.ask_agent(vec![(c.change, c.id)], way, false);
                }
            }
            Action::SendWhole => {
                if let Some(c) = self.picked() {
                    self.ask_agent(vec![(c.change, c.id)], Way::Change, true);
                }
            }
            // Done by the ask under it.
            Action::When => {}
            Action::SendNow => {
                if let Some(c) = self.picked()
                    && let Some(q) = c.queued.clone()
                {
                    self.send(vec![(c.change, c.id)], q.way, q.stack);
                }
            }
            Action::Unbatch => {
                if let Some(c) = self.picked() {
                    self.status = match ask::unsend(&self.path, &[(c.change, c.id)]) {
                        Ok(()) => "taken out of the batch: it is not sent".into(),
                        Err(e) => e.0,
                    };
                    self.reload();
                }
            }
            Action::SendBatch(way, fan) => self.send_batch(way, fan),
            Action::Toggle(change, id) => self.toggle_batched(&change, &id),
            Action::SendAll(way) => {
                let notes = self.listed_sendable(way);
                self.send(notes, way, false);
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
            Action::AskCloseOpened => {
                if let Some(c) = self.picked() {
                    let n = self.db.opened_from(&name, &c.change, &c.id).len();
                    let title = format!("close this comment and the {n} it opened? Show…, closed, can reopen each");
                    self.mode = Mode::Menu(Menu::ask(title, Item::new("Close them", Action::CloseOpened), "Keep them open", true));
                }
            }
            Action::CloseOpened => {
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
            Action::AskCloseComment => {
                self.mode = Mode::Menu(Menu::ask(
                    "close this comment? Show…, closed, can reopen it",
                    Item::new("Close it", Action::CloseComment),
                    "Keep it open",
                    true,
                ));
            }
            Action::CloseComment => {
                if let Some(c) = self.picked()
                    && self.edit(|db| db.set_note(Note { text: String::new(), ..c })).is_some()
                {
                    self.status = "closed: Show…, closed, on a header, lists it to reopen".into();
                }
            }
            Action::Reopen => {
                // Not asked again, as it may have been when closed.
                if let Some(c) = self.picked().filter(|c| self.is_closed(c))
                    && self.edit(|db| db.set_note(Note { queued: None, ..c })).is_some()
                {
                    self.status = "reopened, as it was".into();
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
                self.edit(|db| {
                    ops::by_hand(db, &name)?;
                    db.undo(&name)
                });
            }
            Action::Redo => {
                self.edit(|db| {
                    ops::by_hand(db, &name)?;
                    db.redo(&name)
                });
            }
            Action::EditList => return Effect::EditTodo,
            Action::Tree => self.open_list(Screen::Tree),
            Action::EditFrom(rev) => {
                if self
                    .edit(|db| {
                        ops::by_hand(db, &name)?;
                        db.select(rev)
                    })
                    .is_some()
                {
                    self.status = format!("editing @{rev}");
                    self.screen = Screen::Main;
                }
            }
            Action::WhatChanged(rev) => {
                let Ok(s) = self.db.node(rev) else { return Effect::None };
                let mut lines = vec![(format!("@{rev} · {} · {}", local_time(s.time), s.op), color::file())];
                lines.extend(diff_lines(&crate::history::what_changed(&self.db, rev)));
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
                    ops::by_hand(db, &name)?;
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
                    self.bg.pull = Some((name.clone(), review::Pulling::Starting));
                    self.status = "pulling the reviews from Phabricator…".into();
                    let (path, tx) = (self.path.clone(), self.bg.tx.clone());
                    self.spawn(move || {
                        // Where it is, as it goes: the banner says.
                        let mut say = |p| {
                            let _ = tx.send(Done::Pulling(p));
                        };
                        Done::Said(
                            "pulling",
                            match review::pull_saying(&path, &name, &mut say) {
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
            Action::GoToComment(change, id) => {
                self.on_banner = false;
                self.go_to(&change, &id);
            }
            Action::GoToHistory(rev) => {
                // On it, or what changed the stack last before it.
                self.open_list(Screen::History);
                let all = self.history();
                self.pick = all.iter().position(|e| e.rev <= rev).unwrap_or(0);
                self.hist_rev = all.get(self.pick).map(|e| e.rev);
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
            Action::Search => self.open_search(),
            Action::Stacks => self.open_list(Screen::Stacks),
            Action::AskDeleteStack => {
                let title = format!("delete stack {name}? undo brings it back");
                self.mode = Mode::Menu(Menu::ask(title, Item::new("Delete it", Action::DeleteStack), "Keep it", false));
            }
            Action::DeleteStack => {
                self.edit(|db| {
                    ops::by_hand(db, &name)?;
                    db.delete_stack(&name)
                });
            }
            Action::Transcript(id) => {
                self.scroll = self.transcript_was(&id);
                self.transcript = Some(id);
                self.screen = Screen::Transcript;
            }
            Action::Save(way) => {
                if let Some(e) = self.pending.take() {
                    // Taken by the agent meanwhile: what you wrote stays in
                    // the editor.
                    if let Some(why) = self.locked_target(&e.target) {
                        self.status = why;
                        self.mode = Mode::Edit(Box::new(e));
                        return Effect::None;
                    }
                    let text = e.ed.text();
                    let was = self.mine_of(&e.target);
                    match (self.save_attached(&e.target, &text, &e.attached), way) {
                        (Some(saved), Some(way)) => self.ask_agent(vec![saved], way, false),
                        (saved, _) => self.settle_edit(&e, saved.is_some() && was.as_deref() != Some(text.trim())),
                    }
                }
            }
            Action::SaveWhole => {
                if let Some(e) = self.pending.take() {
                    if let Some(why) = self.locked_target(&e.target) {
                        self.status = why;
                        self.mode = Mode::Edit(Box::new(e));
                        return Effect::None;
                    }
                    let text = e.ed.text();
                    if let Some(saved) = self.save_attached(&e.target, &text, &e.attached) {
                        self.ask_agent(vec![saved], Way::Change, true);
                    }
                }
            }
            // Saving what did not change writes nothing: it is asked as it is.
            Action::SendAsItIs(way) => return self.perform(Action::Save(Some(way))),
            Action::SendWholeAsItIs => return self.perform(Action::SaveWhole),
            Action::ThrowAway | Action::ThrowChanges | Action::LeaveAsItIs => {
                self.status = match action {
                    Action::ThrowAway => "thrown away",
                    Action::ThrowChanges => "your changes are thrown away: the comment is as it was",
                    _ => "left as it is",
                }
                .into();
                if let Some(e) = self.pending.take() {
                    self.settle_edit(&e, false);
                }
            }
            Action::KeepWriting => {
                if let Some(e) = self.pending.take() {
                    self.mode = Mode::Edit(Box::new(e));
                }
            }
            Action::Quit => return Effect::Quit,
        }
        Effect::None
    }

    /// The menu of an edit by hand the stack did not take, `why` said:
    /// resolve the conflict a later patch has with it, if there is one;
    /// edit it again; or throw it away.
    /// What an edit by hand came to: landed, its review opens at once, to
    /// accept or reject it (of landed work `work`, in its place: the marks
    /// made of that work go with it); nothing changed, said; refused, the
    /// edit kept to take up again (`file`, the file and line of a code
    /// edit's).
    pub fn edited(&mut self, done: ops::Edited, file: Option<(String, usize)>, work: Option<String>) {
        let marks = work.as_deref().map(|w| self.carried_marks(w));
        self.reload();
        match done {
            ops::Edited::Landed { said, run } => {
                self.refused = None;
                self.refused_work = None;
                self.conflict = None;
                match marks {
                    Some(marks) => {
                        if !marks.is_empty() {
                            self.edit(|db| crate::held::save(db, &run, &marks));
                        }
                        self.status = format!("{said}: it lands in place of the work you were reviewing, which is rejected");
                        self.open_review(&run, Some(crate::held::Part::Stack));
                    }
                    None => {
                        self.status = format!("{said}: it waits for your review");
                        self.open_review(&run, None);
                    }
                }
            }
            ops::Edited::Unchanged(why) => self.status = why,
            ops::Edited::Refused { why, kept, conflict } => {
                match file {
                    Some((file, line)) => self.refused = Some((file, line, kept)),
                    None => self.refused_work = None,
                }
                self.conflict = conflict;
                self.not_taken(why);
            }
        }
    }

    fn not_taken(&mut self, why: String) {
        let mut items = Vec::new();
        let mut note = why;
        if let Some(c) = &self.conflict {
            let n = c.at + 1;
            if !c.left {
                note.push_str(&format!(
                    "\nPatch {n}'s change to {} can be merged with your edit by hand: the file opens as patch {n} would leave it, with conflict markers where the two disagree.",
                    c.file
                ));
            }
            items.push(Item::new(format!("Resolve the conflict in patch {n} by hand"), Action::Resolve));
        }
        items.push(Item::new("Edit it again", Action::EditAgain));
        items.push(Item::new("Throw the edit away", Action::ThrowEdit));
        let mut m = Menu::new("your edit is not taken", items, None);
        m.note = Some(note);
        self.mode = Mode::Menu(m);
    }

    /// Whether references may be attached to what is written on `target`:
    /// a turn of yours, not your answer in a review, nor the agent's turn
    /// written again.
    fn attachable(&self, target: &Target) -> bool {
        match target {
            Target::Answer { .. } => false,
            Target::Turn { change, id, turn } => {
                self.db.note(change, id).and_then(|n| all_turns(&n.text).into_iter().nth(*turn)).is_some_and(|t| t.role == Role::You)
            }
            _ => true,
        }
    }

    /// The comment `target` writes on, if it is one already there: its
    /// change and id. A line's in a review that does not show it is not.
    fn comment_of(&self, target: &Target) -> Option<(String, String)> {
        match target {
            Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. } => {
                Some((change.clone(), id.clone()))
            }
            Target::Line { change, key, .. } => self
                .db
                .note(change, key)
                .filter(|n| self.screen != Screen::Review || self.review_shows(n))
                .map(|n| (n.change.clone(), n.id.clone())),
            _ => None,
        }
    }

    /// The references attached to the comment `target` writes on, as they
    /// are.
    fn attached_of(&self, target: &Target) -> Vec<crate::refs::Attached> {
        self.comment_of(target).and_then(|(c, i)| self.db.note(&c, &i).map(|n| n.attached.clone())).unwrap_or_default()
    }

    /// The number the next reference attached to the comment `e` writes
    /// takes: one past every number it has or had.
    fn next_number(&self, e: &Edit) -> usize {
        let mut used: Vec<usize> = self.editing_refs(e).into_iter().map(|(k, ..)| k).collect();
        used.extend(self.attached_of(&e.target).iter().map(|a| a.n));
        // Those of the agent's reply to it waiting for your review, which
        // goes in once it is accepted.
        if let Some((c, i)) = self.comment_of(&e.target)
            && let Some((run, k)) = crate::held::reply_waiting(&self.db, &c, &i)
            && let Some(rp) = self.db.run(&run).and_then(|r| r.held.as_ref()?.replies.get(k).cloned())
        {
            used.extend(crate::refs::split(&rp.text).1.into_iter().filter_map(|p| p.n));
        }
        used.into_iter().max().map_or(1, |k| k + 1)
    }

    /// The references of the comment `e` writes, as they will be: those
    /// attached as `e` has them, and those its turns that stay attach (the
    /// agent's); each by its number, with whether the agent attached it.
    fn editing_refs(&self, e: &Edit) -> Vec<(usize, crate::refs::Pointer, bool)> {
        let mut out: Vec<(usize, crate::refs::Pointer, bool)> = e.attached.iter().filter_map(|a| Some((a.n, a.pointer()?, false))).collect();
        if let Some(note) = self.comment_of(&e.target).and_then(|(c, i)| self.db.note(&c, &i).cloned()) {
            let upto = match &e.target {
                Target::Rewind { from, .. } => *from,
                _ => usize::MAX,
            };
            for t in all_turns(&note.text).into_iter().take(upto) {
                for p in crate::refs::split(&t.body).1 {
                    if let Some(k) = p.n {
                        out.push((k, p.to, t.role != Role::You));
                    }
                }
            }
        }
        out.sort_by_key(|(k, ..)| *k);
        out
    }

    /// What Enter on a line naming references does: go to the one it
    /// names; naming several, a menu of them, to pick one.
    fn named(&mut self) -> Option<Action> {
        let names = self.rows.get(self.target_row())?.names.clone();
        let n = self.picked()?;
        let go = |p: &crate::refs::Pointer| match p {
            crate::refs::Pointer::Lines(r) => Action::ShowRef(Box::new(r.clone())),
            crate::refs::Pointer::Comment { change, id } => Action::GoToPointed(change.clone(), id.clone()),
        };
        let named: Vec<(usize, crate::refs::Pointer)> = crate::refs::references(&n).into_iter().filter(|(k, ..)| names.contains(k)).map(|(k, p, _)| (k, p)).collect();
        if let [(_, p)] = named.as_slice() {
            return Some(go(p));
        }
        let items = named.iter().map(|(k, p)| Item::new(self.pointer_said(*k, p), go(p))).collect();
        self.mode = Mode::Menu(Menu::new("the references it names", items, None));
        None
    }

    /// The references of comment `n`, a line each as the editor says them:
    /// for $EDITOR, under what you write.
    fn references_said(&self, n: &Note) -> Vec<String> {
        crate::refs::references(n).iter().map(|(k, p, agent)| self.reference_said(*k, p, *agent)).collect()
    }

    /// Reference `r{k}` of a comment, pointing at `p`, as the editor says
    /// it: by whom, when the agent attached it.
    fn reference_said(&self, k: usize, p: &crate::refs::Pointer, agent: bool) -> String {
        match agent {
            true => format!("{} (the agent's)", self.pointer_said(k, p)),
            false => self.pointer_said(k, p),
        }
    }

    /// Reference `r{k}` of a comment, pointing at `p`, as its line says it.
    fn pointer_said(&self, k: usize, p: &crate::refs::Pointer) -> String {
        match p {
            crate::refs::Pointer::Comment { change, id } => format!("r{k} ↗ {}", self.pointed_said(change, id)),
            crate::refs::Pointer::Lines(r) => format!("r{k} ⇢ {}", r.said()),
        }
    }

    /// Comment (change, id), pointed at, as its line says it: where it is,
    /// how it starts, and whether it is closed.
    fn pointed_said(&self, change: &str, id: &str) -> String {
        match crate::refs::find_comment(&self.db, &self.stack, change, id) {
            Some((n, open)) => {
                let first = thread_turns(&n.text).first().map(|t| crate::markdown::first_line(&crate::refs::split(&t.body).0)).unwrap_or_default();
                let closed = if open { "" } else { " · closed" };
                format!("{} · \"{}\"{closed}", where_on(&self.db, &self.stack, &n), short(&first, 60))
            }
            None => "a comment that is gone".into(),
        }
    }

    /// The comments `notes` point at, each as its line under them says
    /// it.
    pub(super) fn pointed_of<'a>(&self, notes: impl IntoIterator<Item = &'a Note>) -> HashMap<(String, String), String> {
        let mut out = HashMap::new();
        for n in notes {
            for (change, id) in crate::refs::pointed(n) {
                if !out.contains_key(&(change.clone(), id.clone())) {
                    let said = self.pointed_said(&change, &id);
                    out.insert((change, id), said);
                }
            }
        }
        out
    }

    /// Reference `a`, attached, as its line says it.
    fn attached_said(&self, a: &crate::refs::Attached) -> String {
        match &a.to {
            crate::refs::To::Comment { change, id } => format!("{} ↗ {}", a.name(), self.pointed_said(change, id)),
            crate::refs::To::Lines(l) => format!("{} ⇢ {}", a.name(), a.lines().map_or(l.clone(), |r| r.said())),
        }
    }

    /// The editor of `e` told of the references attached to its comment:
    /// their numbers, which mentions name, and their lines under the text.
    fn show_attached(&self, e: &mut Edit) {
        if self.attachable(&e.target) {
            let refs = self.editing_refs(e);
            // Written on a comment in the middle, they are its lines under
            // the editor.
            let lines = match self.refs_in_rows(e) {
                true => vec![],
                false => refs.iter().map(|(k, p, agent)| self.reference_said(*k, p, *agent)).collect(),
            };
            e.ed.set_attached(refs.iter().map(|(k, ..)| *k).collect(), lines);
        }
    }

    /// Whether the references of the comment `e` writes on show under the
    /// editor, as that comment's lines, rather than in it: written in the
    /// middle on a comment there, the editor goes between its turns and
    /// them, for the last reply and the references to be in sight both.
    fn refs_in_rows(&self, e: &Edit) -> bool {
        e.pane == Focus::Diff && matches!(e.target, Target::Reply { .. } | Target::Edit { .. } | Target::Turn { .. } | Target::Rewind { .. })
    }

    /// The menu of what may be attached to the comment being written: lines
    /// of a file, or a comment of the stack, open or closed, but itself and
    /// those attached already.
    fn attach_menu(&mut self) {
        let Some(e) = &self.pending else { return };
        let mut skip: Vec<(String, String)> = self.comment_of(&e.target).into_iter().collect();
        skip.extend(e.attached.iter().filter_map(|a| match &a.to {
            crate::refs::To::Comment { change, id } => Some((change.clone(), id.clone())),
            crate::refs::To::Lines(_) => None,
        }));
        let free = |n: &Note| !skip.iter().any(|(c, i)| *c == n.change && *i == n.id);
        let first = |n: &Note| thread_turns(&n.text).first().map(|t| crate::markdown::first_line(&crate::refs::split(&t.body).0)).unwrap_or_default();
        let mut comments: Vec<Item> = Vec::new();
        for n in self.db.stack_notes(&self.stack).into_iter().filter(|n| free(n)) {
            let label = format!("{} · \"{}\"", where_on(&self.db, &self.stack, n), short(&first(n), 60));
            comments.push(Item::new(label, Action::AttachComment(n.change.clone(), n.id.clone())));
        }
        let changes: Vec<String> = std::iter::once(stack_change(&self.stack))
            .chain(self.current().into_iter().flat_map(|s| s.patches.iter().map(|id| self.db.patch(id).change.clone())))
            .collect();
        for (_, n) in changes.iter().flat_map(|c| self.db.deleted_notes(&self.stack, c)).filter(|(_, n)| free(n)) {
            let label = format!("{} · \"{}\" · closed", where_on(&self.db, &self.stack, &n), short(&first(&n), 60));
            comments.push(Item::new(label, Action::AttachComment(n.change.clone(), n.id.clone())));
        }
        let mut items = vec![Item::new("Lines of a file…", Action::AttachLines)];
        if !comments.is_empty() {
            items.push(Item::line());
            items.extend(comments);
        }
        self.mode = Mode::Menu(Menu::new("a reference to attach", items, None));
    }

    /// The menu of the references attached to the comment being written,
    /// to pick one to detach.
    fn detach_menu(&mut self) {
        let Some(e) = &self.pending else { return };
        let items = e.attached.iter().map(|a| Item::new(self.attached_said(a), Action::Detach(a.n))).collect();
        self.mode = Mode::Menu(Menu::new("a reference to detach", items, None));
    }

    /// Whether a turn of the comment `target` writes on, but the one being
    /// written, names reference `r{n}`: then it stays attached.
    fn named_elsewhere(&self, target: &Target, n: usize) -> bool {
        let Some(note) = self.comment_of(target).and_then(|(c, i)| self.db.note(&c, &i).cloned()) else { return false };
        let turns = all_turns(&note.text);
        let last = turns.len().saturating_sub(1);
        let kept = |k: usize| match target {
            Target::Edit { .. } => k != last || turns[k].role != Role::You,
            Target::Turn { turn, .. } => k != *turn,
            Target::Rewind { from, .. } => k < *from,
            _ => true,
        };
        turns.iter().enumerate().filter(|(k, t)| kept(*k) && t.role == Role::You).any(|(_, t)| {
            crate::refs::mentions(&t.body).iter().any(|m| matches!(&m.names, crate::refs::Names::Some(ns) if ns.contains(&n)))
        })
    }

    /// What the lines to attach start as, and where the cursor goes in
    /// them: for a comment on a line, its file, as its patch leaves it.
    fn lines_prefill(&self, target: &Target) -> (String, usize) {
        if let Target::Line { change, file, .. } = target
            && let Some(k) = self.current().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == *change))
            && file != MSG_PATH
        {
            return (format!("{file}: @{}", k + 1), file.chars().count() + 1);
        }
        (String::new(), 0)
    }

    /// The lines `text` names, as a `refs` line says them, if the file has
    /// them as that version leaves it; else why not.
    fn lines_to_attach(&self, text: &str) -> std::result::Result<crate::refs::Ref, String> {
        let how = "write the file, its lines and which version: a.txt:120-140,200-210 @2 (@0 the base; no @, the whole stack)";
        let r = crate::refs::Ref::parse(text).ok_or_else(|| how.to_string())?;
        let s = self.current().cloned().ok_or_else(|| "there is no stack to read the file of".to_string())?;
        match self.ref_file(&s, &r) {
            Ok(Some(f)) => {
                let n = f.lines().count();
                match r.ranges.last() {
                    Some(&(_, last)) if last > n => Err(format!("{} has {n} lines {}", r.file, r.at_said())),
                    _ => Ok(r),
                }
            }
            Ok(None) => Err(format!("{} is not there {}", r.file, r.at_said())),
            Err(e) => Err(e.0),
        }
    }

    /// A key while lines of a file are named: Enter attaches them to the
    /// comment, if the file has them; Esc goes back to the comment.
    fn lines_key(&mut self, mut l: Lines, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Edit(Box::new(l.back)),
            KeyCode::Enter => match self.lines_to_attach(&l.ed.text()) {
                Ok(r) => {
                    let mut e = l.back;
                    let n = self.next_number(&e);
                    e.attached.push(crate::refs::Attached {
                        n,
                        to: crate::refs::To::Lines(r.line()),
                    });
                    self.status = format!("attached as r{n}: write @r{n} where you mean it");
                    self.show_attached(&mut e);
                    self.mode = Mode::Edit(Box::new(e));
                }
                Err(why) => {
                    self.status = why;
                    self.mode = Mode::Lines(Box::new(l));
                }
            },
            _ => {
                l.ed.key(k);
                self.mode = Mode::Lines(Box::new(l));
            }
        }
        Effect::None
    }

    /// The lines being named, in a box over the screen; where the cursor
    /// goes.
    fn draw_lines(&self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let Mode::Lines(l) = &self.mode else { return None };
        let (w, h) = (area.width as usize, area.height as usize);
        let bw = (w * 9 / 10).clamp(30.min(w), 100.min(w));
        let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(3) / 2);
        spot_in(&self.spots, area, (x, y, bw, 3), Spot::Box);
        Some(l.ed.draw(buf, area, x, y, bw, 3, "lines of a file, to attach: a.txt:120-140,200-210 @2", "enter: attach · esc: back"))
    }

    /// Opens the editor for `target`, in `pane`.
    fn write(&mut self, target: Target, pane: Focus, text: &str) {
        let mut held = None;
        if let Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. } =
            &target
        {
            let (change, id) = (change.clone(), id.clone());
            if let Some(why) = self.locked(&change, &id) {
                self.status = why;
                return;
            }
            // A comment collapsed is expanded to be written on: what it says
            // is in sight.
            if self.db.collapsed(&change, &id) {
                self.edit(|db| db.set_collapsed(&change, &id, false));
            }
            // Off the agent's queue meanwhile, so that the agent does not
            // take it as it was.
            held = self.db.note(&change, &id).and_then(|c| c.queued.clone()).filter(|q| !q.batched());
            if held.is_some() {
                if let Err(e) = ask::unsend(&self.path, &[(change, id)]) {
                    self.status = e.0;
                    return;
                }
                self.reload();
            }
        }
        let mut ed = Editor::new(text);
        // Nothing open on the line, and nothing written yet: the comment
        // last closed there shows, dimmed, until you write.
        if text.is_empty()
            && let Some(n) = self.closed_here(&target)
        {
            ed = ed.with_ghost(crate::thread::said_lines(&n.text));
        }
        let attached = self.attached_of(&target);
        let mut e = Edit {
            ed,
            target,
            pane,
            held,
            attached,
        };
        self.show_attached(&mut e);
        self.mode = Mode::Edit(Box::new(e));
    }

    /// The comment last closed on the line a new comment goes on (a line,
    /// or a patch's title), as it was, when none is open there.
    fn closed_here(&self, target: &Target) -> Option<Note> {
        let (change, line) = match target {
            Target::Line { change, key, .. } => (change.clone(), key.clone()),
            Target::Patch(change) => {
                let p = self.current()?.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == *change)?;
                (change.clone(), title_key(p))
            }
            _ => return None,
        };
        // No comment was ever on the line: none was closed there.
        if !self.db.ever_on(&change, &line) || self.db.notes(&change).iter().any(|n| n.line == line) {
            return None;
        }
        self.db.deleted_notes(&self.stack, &change).into_iter().filter(|(_, n)| n.line == line).max_by_key(|(rev, _)| *rev).map(|(_, n)| n)
    }

    /// Why comment (change, id) cannot be written on now, if it cannot:
    /// the agent is working on it, and its reply is to what it was asked.
    fn locked(&self, change: &str, id: &str) -> Option<String> {
        let key = (change.to_string(), id.to_string());
        if self.busy().contains_key(&key) {
            return Some("the agent is working on this comment: wait for its reply to land".into());
        }
        self.held_replies()
            .contains_key(&key)
            .then(|| "the agent's reply to this comment waits for your review: review it first".to_string())
    }

    /// Why what is written for `t` cannot be saved now, if it cannot.
    fn locked_target(&self, t: &Target) -> Option<String> {
        match t {
            Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. } => {
                self.locked(change, id)
            }
            _ => None,
        }
    }

    /// Saves `text` where `target` says: a new comment, or your turn of the
    /// thread. The comment, (change, id).
    pub fn save(&mut self, target: &Target, text: &str) -> Option<(String, String)> {
        self.save_with(target, text, None)
    }

    /// As `save`, the comment's references attached as `attached` says.
    pub fn save_attached(&mut self, target: &Target, text: &str, attached: &[crate::refs::Attached]) -> Option<(String, String)> {
        self.save_with(target, text, self.attachable(target).then_some(attached))
    }

    /// As `save`, with the references `attached` to the comment, if it
    /// says: else those it has.
    fn save_with(&mut self, target: &Target, text: &str, attached: Option<&[crate::refs::Attached]>) -> Option<(String, String)> {
        if text.trim().is_empty() {
            return None;
        }
        let name = self.stack.clone();
        let text = text.trim().to_string();
        let stamp = crate::db::unique();
        // Written in the review of landed work: it goes back to the agent
        // with that work.
        let review = (self.screen == Screen::Review).then(|| self.reviewing.as_ref().map(|rv| rv.run.clone())).flatten();
        // There, a comment of yours on the line that is not of the work is
        // not written on: what you write is a comment of its own.
        let apart = match target {
            Target::Line { change, key, .. } if review.is_some() => self.db.note(change, key).is_some_and(|n| !self.review_shows(n)),
            _ => false,
        };
        let saved = self.edit(|db| {
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
                Target::Line { change, key, file, preview } => match db.note(change, key).cloned().filter(|_| !apart) {
                    Some(n) => Note {
                        text: append_turn(&n.text, Role::You, &text, None),
                        failed: None,
                        ..n
                    },
                    None => Note {
                        change: change.clone(),
                        id: if apart { format!("l{stamp}") } else { key.clone() },
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
                Target::Edit { change, id } => {
                    let n = db.note(change, id).cloned().ok_or_else(|| crate::db::Error("the comment is gone".into()))?;
                    // Answered meanwhile, it is a reply.
                    let text = replace_last_turn(&n.text, &text).unwrap_or_else(|| append_turn(&n.text, Role::You, &text, None));
                    if text == n.text && attached.is_none_or(|a| a == n.attached) {
                        return Ok((change.clone(), id.clone()));
                    }
                    Note { text, failed: None, ..n }
                }
                Target::Rewind { change, id, from } => {
                    let n = db.note(change, id).cloned().ok_or_else(|| crate::db::Error("the comment is gone".into()))?;
                    let body = without_written(&text, &rewind_written(&n.text, *from));
                    let text = crate::thread::rewind(&n.text, *from, &body)
                        .ok_or_else(|| crate::db::Error("nothing was written: the conversation is as it was".into()))?;
                    Note { text, failed: None, ..n }
                }
                Target::Turn { change, id, turn } => {
                    let n = db.note(change, id).cloned().ok_or_else(|| crate::db::Error("the comment is gone".into()))?;
                    let text = edit_turn(&n.text, *turn, &text).ok_or_else(|| crate::db::Error("the turn you edited was deleted meanwhile".into()))?;
                    if text == n.text && attached.is_none_or(|a| a == n.attached) {
                        return Ok((change.clone(), id.clone()));
                    }
                    Note { text, failed: None, ..n }
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
                // Your answer to a reply, in its review: held with the reply.
                Target::Answer { run, k } => {
                    crate::held::answer(db, run, *k, &text)?;
                    let rp = db.run(run).and_then(|r| r.held.as_ref()?.replies.get(*k).cloned());
                    return rp.map(|rp| (rp.change, rp.id)).ok_or_else(|| crate::db::Error("the reply is gone".into()));
                }
            };
            let note = Note {
                review: review.clone().or(note.review),
                attached: attached.map_or(note.attached, <[_]>::to_vec),
                ..note
            };
            let at = (note.change.clone(), note.id.clone());
            db.set_note(note)?;
            Ok(at)
        });
        if saved.is_some() && review.is_some() {
            self.status = "saved in the review: Esc sends the work back to the agent with what you wrote".into();
        }
        saved
    }

    /// Asks the agent about `notes`, to the whole stack if `whole`: now, or
    /// later, in the batch, as the stack's menus picked.
    fn ask_agent(&mut self, notes: Vec<(String, String)>, way: Way, whole: bool) {
        if self.later() {
            self.batch(notes, way, whole);
        } else {
            self.send(notes, way, whole);
        }
    }

    /// Puts `notes` in the stack's batch, to the whole stack if `whole`:
    /// they wait there until the banner sends the batch.
    fn batch(&mut self, notes: Vec<(String, String)>, way: Way, whole: bool) {
        let done = ask::batch(&self.path, &self.stack, &notes, way, whole);
        self.reload();
        if let Err(e) = done {
            self.status = e.0;
            return;
        }
        let said = if whole { crate::db::WHOLE } else { way.said() };
        let n = self.batched_count();
        self.status = format!("batched ({said}): {n} ask{} in the batch, which the banner sends", plural(n));
    }

    /// Sends the batch of what is asked in `way`: together, or with `fan`
    /// each on its own. They are started away from the screen.
    fn send_batch(&mut self, way: Way, fan: bool) {
        let sent = ask::batched(&self.db, &self.stack, way);
        let done = ask::release_batch(&self.path, &self.stack, way, fan);
        self.reload();
        let n = match done {
            Ok(n) => n,
            Err(e) => {
                self.status = e.0;
                return;
            }
        };
        // As they went: the whole stack, if any of them did.
        let whole = sent.iter().any(|((c, i), _)| self.db.note(c, i).and_then(|n| n.queued.as_ref()).is_some_and(|q| q.stack));
        let said = if whole && way == Way::Change { crate::db::WHOLE } else { way.said() };
        let how = match (way, fan) {
            (Way::Test, _) => format!("{n} build{} and test{}, each on its own", plural(n), plural(n)),
            (_, true) => format!("{n} question{}, an agent each", plural(n)),
            _ => format!("{n} comment{} to a single agent", plural(n)),
        };
        self.status = match ask::waits_on(&self.db, &self.stack, way) {
            None => format!("sent the batch ({said}): {how}"),
            Some(w) => format!("sent the batch ({said}): {how}, ready, it goes {w}"),
        };
        self.launch();
    }

    /// From the banner's menu: comment (change, id) put in the batch, as the
    /// stack's asks are made, or taken out; the menu open again on it.
    fn toggle_batched(&mut self, change: &str, id: &str) {
        let Some(n) = self.db.note(change, id).cloned() else { return };
        let at = [(change.to_string(), id.to_string())];
        let done = match &n.queued {
            Some(q) if q.batched() => ask::unsend(&self.path, &at),
            _ => {
                let (way, whole) = self.way_for(&n);
                ask::batch(&self.path, &self.stack, &at, way, whole)
            }
        };
        self.reload();
        if let Err(e) = done {
            self.status = e.0;
        }
        let mut m = self.main_menu();
        if let Some(k) = m.items.iter().position(|i| i.action == Action::Toggle(change.to_string(), id.to_string())) {
            m.sel = k;
        }
        self.mode = Mode::Menu(m);
    }

    /// How comment `n` is asked once checked in the banner's menu: as it was
    /// last batched or asked, if it was; else as the stack's menus last
    /// asked, if that fits it (the whole stack, for a patch's; build and
    /// test, for the stack's or a patch's as a whole), else to change the
    /// code; on the history, to answer only.
    fn way_for(&self, n: &Note) -> (Way, bool) {
        if n.about.is_some() {
            return (Way::Answer, false);
        }
        let fits = |way: Way, whole: bool| (way != Way::Test || ask::can_test(&self.db, n)) && !(whole && n.on_stack());
        if let Some(q) = self.db.last_queued(&n.change, &n.id).cloned()
            && fits(q.way, q.stack)
        {
            return (q.way, q.stack);
        }
        match self.db.choice(&self.stack, "ask") {
            Some("whole") if !n.on_stack() => (Way::Change, true),
            Some("answer") => (Way::Answer, false),
            Some("test") if ask::can_test(&self.db, n) => (Way::Test, false),
            _ => (Way::Change, false),
        }
    }

    /// Puts `notes` on the agent's queue, to the whole stack if `whole`;
    /// they are started away from the screen.
    fn send(&mut self, notes: Vec<(String, String)>, way: Way, whole: bool) {
        let n = notes.len();
        if let Err(e) = ask::queue(&self.path, &self.stack, &notes, way, whole) {
            self.status = e.0;
            self.reload();
            return;
        }
        self.reload();
        let said = if whole { crate::db::WHOLE } else { way.said() };
        self.status = match ask::waits_on(&self.db, &self.stack, way) {
            None => format!("asked the agent about {n} comment{} ({said}): it is on it", plural(n)),
            Some(w) => format!("asked the agent about {n} comment{} ({said}): ready, it goes {w}", plural(n)),
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
        self.go_to_patch_want(change, Want::Line(key.into()));
    }

    /// Goes to the patch of `change`, the cursor where `want` says.
    fn go_to_patch_want(&mut self, change: &str, want: Want) {
        let Some(at) = self.current().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == change)) else {
            self.status = "its patch is no longer in the stack".into();
            return;
        };
        self.screen = Screen::Main;
        self.on_stack = false;
        self.sel = at;
        self.built = None;
        self.want = Some(want);
        self.focus = Focus::Diff;
    }

    /// Stack `name` the current one: its patches pane where it was, else on
    /// its first patch.
    fn switch_stack(&mut self, name: &str) {
        self.keep_list_of_patches();
        self.stack = name.to_string();
        self.back_to_list_of_patches();
        self.built = None;
        self.show = self.db.view(&self.stack).and_then(Show::parse).unwrap_or(Show::All);
        self.judge();
        self.precreate();
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
        self.back_to_list();
    }

    /// Reference `r`, previewed: its lines numbered, `⋯` between its
    /// ranges; Enter there goes to them.
    fn show_ref(&mut self, r: &crate::refs::Ref) {
        let Some(s) = self.current().cloned() else { return };
        let text = match self.ref_file(&s, r) {
            Ok(Some(text)) => text,
            Ok(None) => {
                self.status = format!("{} is not there, or is not text, {}", r.file, r.at_said());
                return;
            }
            Err(e) => {
                self.status = e.0;
                return;
            }
        };
        let all: Vec<&str> = text.lines().collect();
        let mut lines = Vec::new();
        for &(a, b) in &r.ranges {
            if !lines.is_empty() {
                lines.push(("      ⋯".to_string(), color::dim()));
            }
            for n in a..=b.min(all.len()) {
                lines.push((format!("{n:>5} │ {}", all[n - 1]), Style::new()));
            }
        }
        if lines.is_empty() {
            self.status = format!("{} has {} lines, {}: none of those", r.file, all.len(), r.at_said());
            return;
        }
        let jump = self.ref_jump(&s, r, &all);
        self.mode = Mode::Preview(Box::new(Preview {
            title: r.said(),
            lines,
            top: 0,
            jump,
        }));
    }

    /// The file reference `r` is of, as it says: as a patch of stack `s`
    /// leaves it, as the stack's base has it, or as the whole stack leaves it.
    fn ref_file(&self, s: &crate::db::Stack, r: &crate::refs::Ref) -> crate::db::Result<Option<String>> {
        use crate::refs::At;
        match r.at {
            At::Patch(n) if (1..=s.patches.len()).contains(&n) => ops::file_at(&self.db, s, n - 1, &r.file),
            At::Patch(n) => crate::db::err(format!("{} has no patch {n}", s.name)),
            At::Top if !s.patches.is_empty() => ops::file_at(&self.db, s, s.patches.len() - 1, &r.file),
            _ => {
                let (repo, base) = ops::base_of(s)?;
                Ok(crate::git::git(&repo, &["show", &format!("{base}:{}", r.file)]).ok())
            }
        }
    }

    /// Where Enter in reference `r`'s preview goes: its first line in the
    /// patch whose diff has it, as that patch leaves the file (for the whole
    /// stack, the last patch that touches it); else the file whole, `all`.
    fn ref_jump(&self, s: &crate::db::Stack, r: &crate::refs::Ref, all: &[&str]) -> Jump {
        use crate::refs::At;
        let first = r.ranges[0].0;
        let touches = |id: &String| {
            let d = &self.db.patch(id).diff;
            d.lines().any(|l| l.strip_prefix("+++ b/").or_else(|| l.strip_prefix("--- a/")) == Some(r.file.as_str()))
        };
        let at = match r.at {
            At::Patch(n) => n.checked_sub(1),
            At::Top => s.patches.iter().rposition(touches),
            At::Base => None,
        };
        if let Some(p) = at.and_then(|k| s.patches.get(k)).map(|id| self.db.patch(id)) {
            let row = patch_rows(p, &[], 80, &Deco::default()).into_iter().find(|row| {
                row.path.as_deref() == Some(r.file.as_str()) && row.new_no == Some(first) && matches!(row.kind, Kind::Add | Kind::Ctx)
            });
            if let Some(key) = row.and_then(|row| row.key) {
                return Jump::Line { change: p.change.clone(), key };
            }
        }
        let lines = all.iter().enumerate().map(|(i, l)| (format!("{:>5} │ {l}", i + 1), Style::new())).collect();
        Jump::File {
            title: format!("{}, {}", r.file, r.at_said()),
            lines,
            at: first,
        }
    }

    /// A key in a reference's preview: Enter goes there, ↑↓ scroll, Esc
    /// goes back.
    fn preview_key(&mut self, mut p: Preview, k: KeyEvent) -> Effect {
        let page = (self.body_h / 2).max(1);
        let last = p.lines.len().saturating_sub(1);
        match k.code {
            KeyCode::Esc => return Effect::None,
            KeyCode::Enter => {
                match p.jump {
                    Jump::Line { change, key } => self.go_to_patch_line(&change, &key),
                    Jump::File { title, lines, at } => {
                        self.open_text(title, lines);
                        if let Some(t) = &mut self.text {
                            t.scroll = at.saturating_sub(4);
                        }
                    }
                }
                return Effect::None;
            }
            KeyCode::Down => p.top = (p.top + 1).min(last),
            KeyCode::Up => p.top = p.top.saturating_sub(1),
            KeyCode::PageDown => p.top = (p.top + page).min(last),
            KeyCode::PageUp => p.top = p.top.saturating_sub(page),
            _ => {}
        }
        self.mode = Mode::Preview(Box::new(p));
        Effect::None
    }

    /// A reference's preview, in a box over the screen: its lines from the
    /// one shown first, as many as it holds.
    fn draw_ref_preview(&self, buf: &mut Buffer, area: Rect) {
        let Mode::Preview(p) = &self.mode else { return };
        let (w, h) = (area.width as usize, area.height as usize);
        let bw = (w * 9 / 10).clamp(30.min(w), 120.min(w));
        let bh = (p.lines.len() + 2).min((h * 2 / 3).max(5)).min(h);
        let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(bh) / 2);
        window(buf, area, x, y, bw, bh, &p.title, color::meta());
        spot_in(&self.spots, area, (x, y, bw, bh), Spot::Box);
        for (k, (l, st)) in p.lines.iter().skip(p.top).take(bh.saturating_sub(2)).enumerate() {
            put(buf, area, x + 2, y + 1 + k, bw.saturating_sub(4), &fit(l, bw.saturating_sub(4)), *st);
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
                if let Some(name) = self.db.stacks().get(self.pick).map(|s| s.name.clone()) {
                    self.switch_stack(&name);
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
                // The stack it made, or, deleted, the one it was.
                let Some(rev) = self.hist_entry().and_then(|e| e.to.or(e.from)) else { return };
                self.open_list(Screen::Tree);
                self.pick = tree::graph(&self.db, &self.stack).iter().position(|l| l.rev == rev).unwrap_or(0);
            }
            _ => {}
        }
    }

    /// Runs what needs $EDITOR; the terminal is the editor's meanwhile.
    pub fn effect(&mut self, e: Effect) -> Result<()> {
        let name = self.stack.clone();
        // $EDITOR edits a file on disk: not the demo's.
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path) {
            self.status = self.say_keys("not in the demo: $EDITOR would edit a file on disk; press Enter on a line to write a comment here instead".into());
            return Ok(());
        }
        match e {
            Effect::EditPatch { line } if self.patch_id().is_some() => {
                let done = ops::edit_patch(&self.path, &name, self.sel, line, None)?;
                self.edited(done, None, None);
                return Ok(());
            }
            Effect::EditCode { file, line, again } if self.patch_id().is_some() => {
                let place = self.edit_place();
                let done = ops::edit_code(&self.path, &name, self.sel, &file, line, &place, again, None)?;
                self.refused_work = None;
                self.edited(done, Some((file, line)), None);
                return Ok(());
            }
            Effect::EditWork { run, edit } => {
                let place = self.edit_place();
                let (done, file) = match edit {
                    WorkEdit::List => (ops::edit_stack(&self.path, &name, Some(&run))?, None),
                    WorkEdit::Mail(at) => (ops::edit_patch(&self.path, &name, at, None, Some(&run))?, None),
                    WorkEdit::Code { at, file, line, again } => {
                        let done = ops::edit_code(&self.path, &name, at, &file, line, &place, again, Some(&run))?;
                        self.refused_work = Some((run.clone(), at));
                        (done, Some((file, line)))
                    }
                };
                self.edited(done, file, Some(run));
                return Ok(());
            }
            Effect::Resolve => {
                let (Some(c), Some((_, _, kept))) = (self.conflict.take(), self.refused.clone()) else {
                    return Ok(());
                };
                match ops::resolve(&self.path, &name, &c, kept) {
                    Ok(done) => self.edited(done, None, c.work.clone()),
                    // Not resolved (the editor failed, say): the conflict is
                    // as it was, to resolve again.
                    Err(e) => {
                        self.reload();
                        self.conflict = Some(c);
                        self.not_taken(e.0);
                    }
                }
                return Ok(());
            }
            Effect::EditTodo => {
                let done = ops::edit_stack(&self.path, &name, None)?;
                self.edited(done, None, None);
                return Ok(());
            }
            Effect::EditComment(target @ (Target::Edit { .. } | Target::Turn { .. })) => {
                let (Target::Edit { change, id } | Target::Turn { change, id, .. }) = &target else { return Ok(()) };
                if let Some(why) = self.locked(change, id) {
                    self.status = why;
                    return Ok(());
                }
                let Some(n) = self.db.note(change, id).cloned() else { return Ok(()) };
                let Some(mine) = self.mine_of(&target) else { return Ok(()) };
                // In its place in the conversation, as a reply opens, the
                // comment's references under it.
                let at = match &target {
                    Target::Turn { turn, .. } => Some(*turn),
                    _ => last_mine_at(&n.text),
                };
                let refs = self.references_said(&n);
                let Some((file, wrote, line)) = at.and_then(|at| rewrite_file(&n.text, at, &refs)) else { return Ok(()) };
                // Off the agent's queue meanwhile; only the lines stacked
                // wrote are dropped from what you wrote, a line starting
                // with # kept as any.
                if n.queued.as_ref().is_some_and(|q| !q.batched()) {
                    ask::unsend(&self.path, &[(change.clone(), id.clone())])?;
                }
                let e = Edit {
                    ed: Editor::new(""),
                    target,
                    pane: self.focus,
                    held: n.queued.clone().filter(|q| !q.batched()),
                    attached: vec![],
                };
                let text = match ops::edit_file(&file, "edit.txt", Some(line)) {
                    Ok(t) => without_written(&t, &wrote),
                    Err(err) => {
                        self.settle_edit(&e, false);
                        return Err(err);
                    }
                };
                let changed = !text.is_empty() && text != mine;
                if changed {
                    self.save(&e.target, &text);
                }
                self.settle_edit(&e, changed);
            }
            Effect::EditComment(Target::Rewind { change, id, from }) => {
                if let Some(why) = self.locked(&change, &id) {
                    self.status = why;
                    return Ok(());
                }
                let Some(n) = self.db.note(&change, &id).cloned() else { return Ok(()) };
                if n.queued.as_ref().is_some_and(|q| !q.batched()) {
                    ask::unsend(&self.path, &[(change.clone(), id.clone())])?;
                }
                let e = Edit {
                    ed: Editor::new(""),
                    target: Target::Rewind { change, id, from },
                    pane: self.focus,
                    held: n.queued.clone().filter(|q| !q.batched()),
                    attached: vec![],
                };
                let wrote = rewind_written(&n.text, from);
                let text = match ops::edit_file(&format!("\n{}\n", wrote.join("\n")), "rewind.txt", Some(1)) {
                    Ok(t) => t,
                    Err(err) => {
                        self.settle_edit(&e, false);
                        return Err(err);
                    }
                };
                let changed = !without_written(&text, &wrote).is_empty();
                if changed {
                    self.save(&e.target, &text);
                } else {
                    self.status = "nothing was written: the conversation is as it was".into();
                }
                self.settle_edit(&e, changed);
            }
            Effect::EditComment(target) => {
                // A reply: not while the agent works on the comment, which
                // is off its queue meanwhile.
                let mut held = None;
                if let Target::Reply { change, id } = &target {
                    if let Some(why) = self.locked(change, id) {
                        self.status = why;
                        return Ok(());
                    }
                    held = self.db.note(change, id).and_then(|n| n.queued.clone()).filter(|q| !q.batched());
                    if held.is_some() {
                        ask::unsend(&self.path, &[(change.clone(), id.clone())])?;
                    }
                }
                // Written on a comment there already, it opens under the
                // conversation, at the line after it, the comment's
                // references under it.
                let on = self.comment_of(&target).and_then(|(c, i)| self.db.note(&c, &i).cloned());
                let e = Edit {
                    ed: Editor::new(""),
                    target,
                    pane: self.focus,
                    held,
                    attached: vec![],
                };
                let opened = match &on {
                    Some(n) => {
                        let (file, at) = reply_file(&n.text, &self.references_said(n));
                        ops::edit_file(&file, "reply.txt", Some(at))
                    }
                    None => edit_text("\n# Your comment. Lines starting with # are dropped.\n", ".txt"),
                };
                let text = match opened {
                    Ok(t) => strip_comments(&t),
                    Err(err) => {
                        self.settle_edit(&e, false);
                        return Err(err);
                    }
                };
                let saved = self.save(&e.target, &text);
                self.settle_edit(&e, saved.is_some());
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

    /// What changed the stack, newest first: its edits, and the moves of
    /// its head. Made again only when the database has more than an agent's
    /// stream.
    pub fn history(&self) -> std::rc::Rc<Vec<Entry>> {
        let key = (self.stack.clone(), self.db.changes());
        if let Some((k, all)) = &*self.hist_memo.borrow()
            && *k == key
        {
            return all.clone();
        }
        let all = std::rc::Rc::new(crate::history::entries(&self.db, &self.stack));
        *self.hist_memo.borrow_mut() = Some((key, all.clone()));
        all
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
        out.sort_by_key(|n| self.db.note_first(&n.change, &n.id).unwrap_or(0));
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
        self.edit_box.set(None);
        self.spots.borrow_mut().clear();
        // Where the cursor is in a list of its own screen, kept as it goes.
        self.keep_list();
        self.keep_progress();
        // The agent's work on the stack, under the top bar, on the main
        // screen: open while the cursor is on it, a line for each, in as
        // much as half the screen; the panes under it.
        let banner = (self.screen == Screen::Main && h > 4).then(|| self.banner()).flatten();
        if banner.is_none() {
            self.on_banner = false;
        }
        let lines = if self.on_banner { self.banner_lines() } else { Vec::new() };
        let room = self.banner_rows(&lines).len().min((h.saturating_sub(4) / 2).max(1));
        let top = 1 + banner.is_some() as usize + room;
        self.body_h = h - top - 1;
        self.body_w = w;
        let body = Rect::new(area.x, area.y + top as u16, area.width, (h - top - 1) as u16);
        if let Some(text) = &banner {
            self.draw_banner(buf, area, text, &lines, room);
        }
        let mut cursor = None;
        match self.screen {
            Screen::Runs => self.draw_runs(buf, body),
            Screen::Transcript => self.draw_transcript(buf, body),
            Screen::History => {
                let w = body.width as usize;
                // Too narrow for both: the one the cursor is in.
                if w < 60 && self.hist_focus == HistFocus::Diff {
                    self.draw_history_diff(buf, body);
                } else if w < 60 {
                    cursor = self.draw_history(buf, body);
                } else {
                    let list_w = (w * 2 / 5).max(30);
                    for y in 0..body.height {
                        buf.set_string(body.x + list_w as u16, body.y + y, "│", color::meta());
                    }
                    let list = Rect::new(body.x, body.y, list_w as u16, body.height);
                    let diff = Rect::new(body.x + (list_w + 1) as u16, body.y, (w - list_w - 1) as u16, body.height);
                    cursor = self.draw_history(buf, list);
                    self.draw_history_diff(buf, diff);
                }
            }
            Screen::Text => self.draw_text(buf, body),
            Screen::Review => cursor = self.draw_review(buf, body),
            Screen::Search => cursor = self.draw_search(buf, body),
            Screen::Tree | Screen::Stacks | Screen::Versions => {
                let (left, (right, _)) = self.columns(buf, body, (body.width as usize / 4).clamp(20, 40), 0);
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
                let right = if self.notes_shown() { self.comments } else { Pane::Hidden };
                let wide = self.writing().map(|e| e.pane);
                let (left_w, right_w) = self.side_widths(body.width as usize, self.list, right, wide);
                let (left, (middle, right)) = self.columns(buf, body, left_w, right_w);
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
        self.draw_ref_preview(buf, body);
        let cursor = self.draw_lines(buf, body).or(cursor);
        self.draw_status(buf, area);
        cursor
    }

    /// How wide the side panes are, of `w`, as `left` and `right` show
    /// them: expanded, a quarter of the width (what does not fit in it
    /// folds: the patches' titles whole); compact, what they show needs, the
    /// patches a quarter at most, the comments a fifth; hidden, not at all.
    /// The pane a comment is written in, `wide`, takes half the width.
    fn side_widths(&mut self, w: usize, left: Pane, right: Pane, wide: Option<Focus>) -> (usize, usize) {
        let mut left_w = match left {
            Pane::Hidden => 0,
            Pane::Compact => self.list_wants().clamp(10, (w / 4).max(10)),
            Pane::Expanded => (w / 4).clamp(20, 40),
        };
        let mut right_w = match right {
            Pane::Hidden => 0,
            Pane::Compact => self.notes_wants().clamp(12, (w / 5).max(12)),
            Pane::Expanded => (w / 4).clamp(24, 44),
        };
        match wide {
            Some(Focus::List) if left.shown() => left_w = left_w.max(w / 2),
            Some(Focus::Notes) if right.shown() => right_w = right_w.max(w / 2),
            _ => {}
        }
        (left_w, right_w)
    }

    /// The body cut into a pane `left_w` wide on the left and one `right_w`
    /// wide on the right (none, 0 wide), the middle between, with lines
    /// between them.
    fn columns(&self, buf: &mut Buffer, body: Rect, left_w: usize, right_w: usize) -> (Option<Rect>, (Rect, Option<Rect>)) {
        let w = body.width as usize;
        let (left, right) = (left_w > 0, right_w > 0);
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

    /// The patches pane's header: the stack's own place.
    fn list_header(&self) -> String {
        let own = self.db.notes(&stack_change(&self.stack)).len();
        match self.current() {
            Some(s) => {
                let mut t = format!(" ≡ {} · {} patch{}", self.stack, s.patches.len(), if s.patches.len() == 1 { "" } else { "es" });
                if own > 0 {
                    t += &format!(" · {own} comment{}", plural(own));
                }
                t
            }
            None => format!(" ≡ {}", if self.stack.is_empty() { "no stack" } else { &self.stack }),
        }
    }

    /// What the patches pane says of patch `id`, as `mode` shows it: its
    /// title (expanded) or the lines it adds and removes (compact); and its
    /// tags: what is read of it, reading; its comments the agent has not
    /// answered last.
    fn patch_said(&mut self, id: &str, mode: Pane) -> (String, String) {
        let mut tag = match self.reading.then(|| self.read_of(id)) {
            Some((n, m)) if m > 0 && n == m => " ✓".to_string(),
            Some((n, m)) if m > 0 => format!(" {n}/{m}"),
            _ => String::new(),
        };
        let p = self.db.patch(id);
        let open = self.db.notes(&p.change).iter().filter(|n| last_role(&n.text) != Some(Role::Agent)).count();
        if open > 0 {
            tag += &format!(" [{open}]");
        }
        let said = match mode {
            Pane::Compact => {
                let (added, removed) = self.stats(id);
                format!("+{added} −{removed}")
            }
            _ => p.title().to_string(),
        };
        (said, tag)
    }

    /// The lines patch `id` adds and removes, counted once: its versions do
    /// not change.
    fn stats(&self, id: &str) -> (usize, usize) {
        if let Some(s) = self.stats_memo.borrow().get(id) {
            return *s;
        }
        let mut s = (0, 0);
        for f in crate::diff::parse_diff(&self.db.patch(id).diff, "") {
            for l in f.hunks.iter().flat_map(|h| h.lines.iter()) {
                match l.origin {
                    '+' => s.0 += 1,
                    '-' => s.1 += 1,
                    _ => {}
                }
            }
        }
        self.stats_memo.borrow_mut().insert(id.to_string(), s);
        s
    }

    /// How wide the patches pane is to be, compact: its widest line.
    fn list_wants(&mut self) -> usize {
        let Some(ids) = self.current().map(|s| s.patches.clone()) else { return 0 };
        let mut most = 0;
        for id in &ids {
            let (said, tag) = self.patch_said(id, Pane::Compact);
            most = most.max(4 + said.chars().count() + tag.chars().count());
        }
        most + 1
    }

    /// How comment `n` is marked where it is listed: with the agent (●),
    /// batched (◇), waiting its turn (◌), its reply waiting for review (◆),
    /// failed (✗), moved (↑), answered (✓), else not.
    pub(super) fn note_mark(&self, n: &Note, busy: &Busy, held: &Held) -> &'static str {
        let key = (n.change.clone(), n.id.clone());
        if busy.contains_key(&key) {
            "●"
        } else if n.queued.as_ref().is_some_and(|q| q.batched()) {
            "◇"
        } else if n.queued.is_some() {
            "◌"
        } else if held.contains_key(&key) {
            "◆"
        } else if n.failed.is_some() {
            "✗"
        } else if n.moved.is_some() {
            "↑"
        } else if last_role(&n.text) == Some(Role::Agent) {
            "✓"
        } else {
            " "
        }
    }

    /// What the comments pane says of comment `n`: its mark (with the agent,
    /// waiting its turn, its reply waiting for review, failed, moved,
    /// answered), who opened it and where it is; then the first line of
    /// what was said last.
    fn note_said(&self, n: &Note, busy: &Busy, held: &Held) -> (String, String) {
        let mark = self.note_mark(n, busy, held);
        let who = n.by.as_deref().unwrap_or("you");
        let at = match n.about {
            Some(rev) => format!("history @{rev}"),
            None if n.on_stack() => "the stack".to_string(),
            None => format!("{}: {}", n.file, n.preview),
        };
        let last = thread_turns(&n.text).last().map(|t| crate::markdown::first_line(&t.body)).unwrap_or_default();
        (format!("{mark} {who} · {at}"), format!("   {last}"))
    }

    /// How wide the comments pane is to be, compact: its widest line, of
    /// the comments on what the middle shows.
    fn notes_wants(&self) -> usize {
        let (busy, held) = (self.busy(), self.held_replies());
        let lines = self.subject_notes().into_iter().map(|n| self.note_said(&n, &busy, &held).0.chars().count());
        lines.max().unwrap_or(0) + 1
    }

    fn draw_bar(&self, buf: &mut Buffer, area: Rect) {
        let w = area.width as usize;
        let file = self.path.file_name().unwrap_or_default().to_string_lossy();
        let mut text = format!(" stacked · {file}");
        match self.db.head(&self.stack) {
            Some((rev, s)) => {
                let n = s.patches.len();
                text += &format!(" · {} @{rev} · {n} patch{}", self.stack, if n == 1 { "" } else { "es" });
                if let Some(b) = &s.base {
                    text += &format!(" on {}", &b[..12.min(b.len())]);
                }
                // With the patches out of sight, which one this is; with the
                // comments, how many there are.
                if !self.list.shown() && self.screen == Screen::Main && !s.patches.is_empty() && !self.on_stack {
                    text += &format!(" · patch {} of {}", self.sel + 1, s.patches.len());
                }
                let notes = self.subject_notes().len();
                if !self.comments.shown() && self.screen == Screen::Main && notes > 0 {
                    text += &format!(" · {notes} comment{}", plural(notes));
                }
                if self.screen == Screen::Main {
                    if self.reading {
                        text += " · reading";
                        // Of the changed lines that need reading.
                        let changed = self.rows.iter().filter(|r| matches!(r.kind, Kind::Add | Kind::Del) && !r.free).count();
                        if !self.on_stack && changed > 0 {
                            let read = self.rows.iter().filter(|r| r.read && !r.free).count();
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
        let batched = self.batched_count();
        if batched > 0 {
            text += &format!(" · batch:{batched}");
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
        // On the right, how far the review of the stack is: as much of it
        // as the rest leaves room for.
        let used = text.chars().count();
        let right = match (&self.progress, self.db.head(&self.stack)) {
            (Some(((name, _), p)), Some(_)) if *name == self.stack => {
                p.said().into_iter().find(|said| used + 3 + said.chars().count() < w)
            }
            _ => None,
        };
        if let Some(said) = right {
            let n = said.chars().count();
            put(buf, area, w - n - 1, 0, n, &said, st);
        }
    }

    /// The keys there are, where the keys go now.
    pub fn hint(&self) -> String {
        match &self.mode {
            Mode::Menu(m) => match m.chosen() {
                Some(i) if i.check.is_some() => "↑↓ choose · enter: check or uncheck it · esc: close · ?: what it does".into(),
                Some(Item { choices: Some(c), .. }) => format!("↑↓ choose · ←→: {} · enter: do it · esc: close · ?: what it does", c.what),
                _ => "↑↓ choose · enter: do it · esc: close · ?: what it does".into(),
            },
            Mode::Moving { .. } => "↑↓ to place it · enter: put it there · esc: leave it where it was".into(),
            Mode::MovingHead { .. } => "↑↓ to place HEAD · enter: move your checkout there · esc: leave it where it was".into(),
            Mode::Edit(e) if e.ed.ghosted() => format!("↑↓: read the last comment closed here · enter: a new line · {}", self.esc_does(e)),
            Mode::Edit(e) => format!("enter: a new line · {}", self.esc_does(e)),
            Mode::Help { .. } => "any key: back, and that key".into(),
            Mode::Preview(_) => "↑↓: scroll · enter: go there · esc: back".into(),
            Mode::Lines(_) => "enter: attach these lines · esc: back to the comment".into(),
            Mode::Stream { .. } => "↑↓: scroll · esc: fold it".into(),
            Mode::Normal if self.screen == Screen::Main && self.on_banner => {
                let enter = match self.banner_lines().get(self.banner_sel) {
                    Some(BannerLine::Landed(_)) => "review it",
                    Some(BannerLine::Pulling) => "say what the pull brings",
                    Some(BannerLine::Batch) => "pick what to send",
                    _ => "go to it",
                };
                format!("↑↓ choose · enter: {enter} · space: pick one · esc: back where you were · ?: what can I do here")
            }
            Mode::Normal if self.screen == Screen::Search => {
                "type what to look for · ←→: where · tab: the search before · ↑↓: pick · enter: go there · esc: back".into()
            }
            Mode::Normal if self.screen == Screen::Review => {
                self.review_hint()
            }
            Mode::Normal if self.reading_here() => {
                "space: mark this block read · enter: unread · 1: view · 2: comment · ?: what can I do here".into()
            }
            Mode::Normal => {
                let action = self.enter();
                let taking = match &action {
                    Some(Action::TakeReply(run, k)) => format!("enter: accept {} · ", self.reply_said(run, *k)),
                    _ => String::new(),
                };
                let enter = match (self.screen, action) {
                    (Screen::Main, Some(Action::Open)) => "enter: read it · ",
                    (Screen::Main, Some(Action::GoToNote)) => "enter: go to it · ",
                    (Screen::Main, Some(Action::Reply)) => "enter: reply · ",
                    (Screen::Main, Some(Action::EditMine)) => "enter: edit it · ",
                    (Screen::Main, Some(Action::OpenStream)) => "enter: what the agent writes · ",
                    (Screen::Main, Some(Action::Expand)) => "enter: expand it · ",
                    (Screen::Main, Some(Action::TakeReply(..))) => taking.as_str(),
                    (Screen::Main, Some(_)) => "enter: comment · ",
                    (Screen::Main, None) => "enter or ",
                    (Screen::Transcript | Screen::Text, _) => "↑↓: scroll · ",
                    (Screen::History, _) => match self.hist_focus {
                        HistFocus::List => "enter: go to it · →: its stack diff · ",
                        HistFocus::Diff => "↑↓: scroll · ←: the history · ",
                    },
                    _ => "enter: pick · ",
                };
                let keys = match (self.screen, self.reading) {
                    (Screen::Main, false) => "1: view · 2: read · ",
                    (Screen::Main, true) => "1: view · 2: comment · ",
                    _ => "",
                };
                let ask = if self.helps() { "?: what can I do here" } else { "" };
                format!("{enter}space: menu · {keys}{ask}").trim_end_matches(" · ").to_string()
            }
        }
    }

    fn draw_status(&self, buf: &mut Buffer, area: Rect) {
        let w = area.width as usize;
        let y = area.height as usize - 1;
        let text = if self.status.is_empty() {
            format!(" {}", self.say_keys(self.hint()))
        } else if matches!(self.mode, Mode::Moving { .. }) {
            // Refused, and still moving: why, cut short if it must be, and
            // the keys that go on from here, never cut.
            let on = self.say_keys(" · ↑↓: elsewhere · esc: leave it where it was".into());
            let room = w.saturating_sub(1 + on.chars().count());
            let why = if self.status.chars().count() > room {
                format!("{}…", self.status.chars().take(room.saturating_sub(1)).collect::<String>())
            } else {
                self.status.clone()
            };
            format!(" {why}{on}")
        } else {
            format!(" {}", self.status)
        };
        put(buf, area, 0, y, w, &fit(&text, w), color::status());
    }

    fn draw_menu(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Mode::Menu(m) = &mut self.mode else { return };
        // The choices in one column, past the longest label that has them.
        let pad = m.items.iter().filter(|i| i.choices.is_some()).map(|i| i.label.chars().count()).max().unwrap_or(0);
        let widest = m.items.iter().map(|i| i.row(pad).0.chars().count()).max().unwrap_or(0);
        // A note of short lines, as a review's summary, each line in full.
        let note_w = m.note.as_deref().map_or(0, |n| n.lines().map(|l| l.chars().count()).max().unwrap_or(0).min(60));
        let bw = (widest.max(note_w).max(m.title.chars().count() + 2).max(24) + 4).min(w.saturating_sub(2));
        // A line of its own for each line of the note.
        let note: Vec<String> = m.note.as_deref().map_or(vec![], |n| n.lines().flat_map(|l| wrap(l, bw.saturating_sub(4))).collect());
        let below = if note.is_empty() { 0 } else { note.len() + 1 };
        let rows = m.items.len().min(h.saturating_sub(2 + below).max(1));
        if m.sel < m.top {
            m.top = m.sel;
        } else if m.sel >= m.top + rows {
            m.top = m.sel + 1 - rows;
        }
        let bh = rows + 2 + below;
        // Beside the comment being written, not over it: under it where
        // there is room, else above it.
        let centered = h.saturating_sub(bh) / 2;
        let y = match (self.pending.is_some(), self.edit_box.get()) {
            (true, Some(b)) => {
                let top = (b.y as usize).saturating_sub(area.y as usize);
                let bottom = top + b.height as usize;
                if bottom + bh <= h {
                    bottom
                } else if top >= bh {
                    top - bh
                } else {
                    centered
                }
            }
            _ => centered,
        };
        let x = w.saturating_sub(bw) / 2;
        window(buf, area, x, y, bw, bh, &m.title, color::meta());
        spot_in(&self.spots, area, (x, y, bw, bh), Spot::Box);
        for (r, (i, item)) in m.items.iter().enumerate().skip(m.top).take(rows).enumerate() {
            if item.action == Action::Line {
                put(buf, area, x + 1, y + 1 + r, bw - 2, &"─".repeat(bw - 2), color::meta());
                continue;
            }
            spot_in(&self.spots, area, (x + 1, y + 1 + r, bw - 2, 1), Spot::Item(i));
            let mut st = if item.enabled { Style::new() } else { color::dim() };
            if i == m.sel {
                st = st.add_modifier(Modifier::REVERSED);
            }
            let (text, choices) = item.row(pad);
            put(buf, area, x + 1, y + 1 + r, bw - 2, &fit(&format!(" {text}"), bw - 2), st);
            // Each choice a spot of its own, over the item's: a click picks it.
            for (k, (start, width)) in choices.into_iter().enumerate() {
                if 1 + start < bw - 2 {
                    spot_in(&self.spots, area, (x + 2 + start, y + 1 + r, width.min(bw - 3 - start), 1), Spot::Choice(i, k));
                }
            }
        }
        for (k, line) in note.iter().enumerate() {
            put(buf, area, x + 2, y + 2 + rows + k, bw - 4, line, color::note());
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
        spot_in(&self.spots, area, (x, y, bw, bh), Spot::Box);
        for (k, (l, st)) in text.iter().take(bh.saturating_sub(2)).enumerate() {
            put(buf, area, x + 2, y + 1 + k, bw - 4, l, *st);
        }
    }

    /// The comment being written: in the editor, or waiting on what the
    /// menu Esc opened says to do with it, still where it is, to be read
    /// again before choosing.
    fn writing(&self) -> Option<&Edit> {
        match &self.mode {
            Mode::Edit(e) => Some(e),
            Mode::Menu(_) => self.pending.as_ref(),
            Mode::Lines(l) => Some(&l.back),
            _ => None,
        }
    }

    /// How many lines the editor box takes in `area`, if a comment is
    /// being written in `pane`.
    fn editor_height(&self, area: Rect, pane: Focus) -> usize {
        match self.writing() {
            Some(e) if e.pane == pane => {
                let (w, h) = (area.width as usize, area.height as usize);
                (e.ed.height(w.saturating_sub(2)) + 2).clamp(4, (h / 2).max(4))
            }
            _ => 0,
        }
    }

    /// The editor box for the comment being written in `pane`, if one is,
    /// at `y` of `area`: drawn, and where the terminal's cursor goes, but
    /// while a menu says what to do with it.
    fn draw_editor(&self, buf: &mut Buffer, area: Rect, y: usize, pane: Focus) -> Option<(u16, u16)> {
        let e = self.writing()?;
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
            Target::Edit { change, id } => {
                let reply = self.db.note(change, id).is_some_and(|n| thread_turns(&n.text).len() > 1);
                format!("your {}: edit it, or add to it", if reply { "reply" } else { "comment" })
            }
            Target::Turn { change, id, turn } => {
                let t = self.db.note(change, id).and_then(|n| all_turns(&n.text).into_iter().nth(*turn));
                format!("{}: edit it", t.map_or("the turn".into(), |t| t.name(*turn)))
            }
            Target::Rewind { change, id, from } => {
                let t = self.db.note(change, id).and_then(|n| all_turns(&n.text).into_iter().nth(*from));
                format!("rewind from {}: write what replaces it", t.map_or("your turn".into(), |t| t.name(*from)))
            }
            Target::About(rev) => format!("a question on @{rev}, for the agent to answer"),
            Target::Answer { .. } => "your answer to the agent's reply".to_string(),
        };
        spot_in(&self.spots, area, (0, y, w, bh), Spot::Box);
        self.edit_box.set(Some(Rect::new(area.x, area.y + y as u16, w as u16, bh as u16)));
        let cursor = e.ed.draw(buf, area, 0, y, w, bh, &title, self.esc_does(e));
        matches!(self.mode, Mode::Edit(_)).then_some(cursor)
    }

    fn draw_list(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let w = area.width as usize;
        // The header: the stack's own place.
        let header = self.list_header();
        let mut st = color::header();
        if self.on_stack {
            st = if self.focus == Focus::List && !self.on_banner {
                st.add_modifier(Modifier::REVERSED)
            } else {
                st.add_modifier(Modifier::UNDERLINED)
            };
        }
        put(buf, area, 0, 0, w, &fit(&header, w), st);
        self.spot(area, (0, 0, w), Spot::Patch(None));
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
        let editing = self.writing().filter(|e| e.pane == Focus::List).map(|e| e.target.clone());
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
            y += 1;
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
        // Each patch's lines: its number and, compact, its stats; expanded,
        // its title whole, folded under itself where the pane is narrower;
        // its tags at the end of its first.
        // ✉: its message differs from its commit's in the checkout.
        let differ = self.head_here().map(|h| h.differ()).unwrap_or_default();
        let lines: Vec<Vec<String>> = order
            .iter()
            .enumerate()
            .map(|(at, &i)| {
                let (said, mut tag) = self.patch_said(&s.patches[i], self.list);
                if differ.contains(&i) {
                    tag += " ✉";
                }
                let mark = if moving && at == on { " ↕".to_string() } else { format!("{:>2}", at + 1) };
                let head = format!(" {mark} ");
                let (head_w, tag_w) = (head.chars().count(), tag.chars().count());
                let mut folded = vec![said.clone()];
                if self.list == Pane::Expanded && head_w + said.chars().count() + tag_w > w {
                    folded = wrap(&said, w.saturating_sub(head_w + tag_w).max(1));
                }
                if folded.is_empty() {
                    folded.push(String::new());
                }
                folded
                    .iter()
                    .enumerate()
                    .map(|(k, part)| match k {
                        0 => format!("{}{tag}", fit(&format!("{head}{part}"), w.saturating_sub(tag_w))),
                        _ => format!("{}{part}", " ".repeat(head_w)),
                    })
                    .collect()
            })
            .collect();
        // HEAD of the checkout stacked runs in: after the patches it has as
        // commits, under the header at the base or elsewhere.
        let moving_head = matches!(self.mode, Mode::MovingHead { .. });
        let head = self.head_line();
        let mut entries: Vec<ListEntry> =
            order.iter().zip(lines).enumerate().map(|(at, (&i, its))| (Some((at, i)), its)).collect();
        if let Some((n, line)) = &head {
            entries.insert((*n).min(entries.len()), (None, vec![line.clone()]));
        }
        let on_entry = match moving_head {
            true => entries.iter().position(|(e, _)| e.is_none()),
            false => entries.iter().position(|(e, _)| e.is_some_and(|(at, _)| at == on)),
        }
        .unwrap_or(0);
        // The first shown: the one the cursor is on in sight, all its lines.
        let h = area.height as usize;
        let room = h.saturating_sub(1 + edit_h);
        let mut top = on_entry.min(entries.len().saturating_sub(1));
        let mut used = entries.get(top).map_or(1, |(_, its)| its.len());
        while top > 0 && used + entries[top - 1].1.len() <= room {
            top -= 1;
            used += entries[top].1.len();
        }
        for (entry, its) in entries.iter().skip(top) {
            if y >= h {
                break;
            }
            let Some((at, i)) = *entry else {
                let st = if moving_head { color::meta().add_modifier(Modifier::REVERSED) } else { color::meta() };
                put(buf, area, 0, y, w, &fit(&its[0], w), st);
                y += 1;
                continue;
            };
            let mut st = Style::new();
            if at == on && !self.on_stack && !moving_head {
                st = if self.focus == Focus::List || moving {
                    st.add_modifier(Modifier::REVERSED)
                } else {
                    st.patch(color::picked())
                };
            }
            for line in its {
                if y >= h {
                    break;
                }
                put(buf, area, 0, y, w, &fit(line, w), st);
                self.spot(area, (0, y, w), Spot::Patch(Some(at)));
                y += 1;
            }
            let change = self.db.patch(&s.patches[i]).change.clone();
            if editing == Some(Target::Patch(change)) {
                cursor = self.draw_editor(buf, area, y, Focus::List);
                y += edit_h;
            }
        }
        cursor
    }

    /// HEAD's line in the patches pane, and how many patches go above it:
    /// where HEAD of the checkout stacked runs in is, if it is a checkout of
    /// the stack's repository; `+2`, the commits above them that are not
    /// the stack's.
    fn head_line(&self) -> Option<(usize, String)> {
        use crate::head::At;
        let h = self.head_here()?;
        if let Mode::MovingHead { to } = self.mode {
            return Some((to, format!(" ↕ HEAD {}", h.on())));
        }
        let sync = match h.differ().len() {
            0 => String::new(),
            n => format!(" · {n} to sync"),
        };
        Some(match &h.at {
            At::Stack(on) if !on.above.is_empty() => (on.commits.len(), format!(" ▶ HEAD {} +{}{sync}", h.on(), on.above.len())),
            At::Stack(on) => (on.commits.len(), format!(" ▶ HEAD {}{sync}", h.on())),
            At::Elsewhere => (0, format!(" ▶ HEAD {}: off the stack", h.on())),
        })
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
        // comment it answers, which then shows. Its box takes `gap` lines
        // there, and what it would hide comes after it.
        let anchor = self.editor_anchor();
        let gap = if anchor.is_some() { self.editor_height(area, Focus::Diff) } else { 0 };
        let mut editor_y: Option<usize> = None;
        // Not while the cursor is on the banner: it comes back there.
        let lit = |i: usize, app: &App| i == app.cursor && app.focus != Focus::List && !app.on_banner;
        // A patch's title, its message's first line: on the first line of
        // the middle once it is scrolled past, the rows under it.
        let title = if self.on_stack { None } else { self.rows.iter().position(|r| r.msg_at == Some(0) && r.note.is_none()) };
        // Each row drawn: the line of the pane it is on, and where it starts.
        let mut drawn: Vec<(usize, usize, usize)> = Vec::new();
        match layout {
            Layout::Unified => {
                // The anchor, a third down, and the box under it, in sight.
                if let Some(a) = anchor
                    && (a < self.top || a + 1 + gap > self.top + h)
                {
                    self.top = a.saturating_sub(h / 3).max((a + 1 + gap).saturating_sub(h)).min(a);
                }
                if anchor.is_none() {
                    if self.cursor < self.top {
                        self.top = self.cursor;
                    } else if self.cursor >= self.top + h {
                        self.top = self.cursor + 1 - h;
                    }
                }
                // Pinned, the title leaves the rows a line less: the cursor,
                // or the anchor and its box, in it.
                let pinned = |top: usize| h > 2 && title.is_some_and(|t| top > t);
                if pinned(self.top) {
                    match anchor {
                        Some(a) if a + 1 + gap > self.top + h - 1 => self.top = (a + 2 + gap).saturating_sub(h).min(a),
                        None if self.cursor + 1 >= self.top + h => self.top = self.cursor + 2 - h,
                        _ => {}
                    }
                }
                let mut y = 0;
                if pinned(self.top)
                    && let Some(t) = title
                {
                    self.draw_title(buf, area, w, t);
                    y = 1;
                }
                for i in self.top..self.rows.len() {
                    if y >= h {
                        break;
                    }
                    self.draw_row(buf, area, (0, y, w), i, lit(i, self), ticks);
                    drawn.push((i, y, 0));
                    y += 1;
                    if Some(i) == anchor {
                        editor_y = Some(y);
                        y += gap;
                    }
                }
            }
            Layout::Split => {
                let (lines, index) = split_lines(&self.rows);
                let cur = *index.get(&anchor.unwrap_or(self.cursor)).unwrap_or(&0);
                let at = anchor.and_then(|a| index.get(&a).copied());
                let mut top = *index.get(&self.top).unwrap_or(&0);
                if cur < top {
                    top = cur;
                } else if cur + 1 + gap > top + h {
                    top = (cur + 1 + gap).saturating_sub(h).min(cur);
                }
                let pinned = |top: usize| h > 2 && title.and_then(|t| index.get(&t)).is_some_and(|&l| top > l);
                if pinned(top) && cur + 2 + gap > top + h {
                    top = (cur + 2 + gap).saturating_sub(h).min(cur);
                }
                self.top = lines.get(top).map_or(0, split_row);
                let mut y = 0;
                if pinned(top)
                    && let Some(t) = title
                {
                    self.draw_title(buf, area, w, t);
                    y = 1;
                }
                for (k, line) in lines.iter().enumerate().skip(top) {
                    if y >= h {
                        break;
                    }
                    if let Some(i) = line.span {
                        self.draw_row(buf, area, (0, y, w), i, lit(i, self), ticks);
                        drawn.push((i, y, 0));
                    } else {
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
                    y += 1;
                    if Some(k) == at {
                        editor_y = Some(y);
                        y += gap;
                    }
                }
            }
            Layout::Compact => {
                let p = self.panes.clone();
                // The title above both panes: pinned, the panes under it.
                let above = |pos: &HashMap<usize, usize>, top: usize| title.and_then(|t| pos.get(&t)).is_some_and(|&k| k < top);
                let pin = h > 2 && above(&p.lpos, self.compact.top[0]) && above(&p.rpos, self.compact.top[1]);
                let panes_h = h - pin as usize;
                // Moved by row (to a comment, to a comment being written):
                // the panes go there; with a box, in what it leaves of them.
                let lines_h = panes_h.saturating_sub(gap).max(1);
                if let Some(a) = anchor {
                    self.compact.resync(&p, a, lines_h);
                } else {
                    self.compact.resync(&p, self.cursor, panes_h);
                }
                if anchor.is_none()
                    && let Some(row) = self.compact.settle(&p, panes_h)
                {
                    self.cursor = row;
                }
                if pin
                    && let Some(t) = title
                {
                    self.draw_title(buf, area, w, t);
                }
                let scroll = self.compact.scroll;
                // The lines after the anchor's, under the box.
                let mut shift = pin as usize;
                for (k, (l, r)) in self.compact.lines(&p, lines_h).into_iter().enumerate() {
                    let y = k + shift;
                    if anchor.is_some() && editor_y.is_none() && (l == anchor || r == anchor) {
                        editor_y = Some(y + 1);
                        shift += gap;
                    }
                    if y >= h {
                        break;
                    }
                    let on = k == self.compact.y && self.focus != Focus::List && !self.on_banner && anchor.is_none();
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
        if anchor.is_some() {
            return self.draw_editor(buf, area, editor_y.unwrap_or(h / 3), Focus::Diff);
        }
        self.draw_preview(buf, area, &drawn);
        None
    }

    /// The patch's title, row `t`, on the first line of the middle at
    /// `area`, `w` wide: its row is scrolled past. A click on it goes to it.
    fn draw_title(&self, buf: &mut Buffer, area: Rect, w: usize, t: usize) {
        put(buf, area, 0, 0, w, &fit(&self.rows[t].text, w), color::header());
        self.spot(area, (0, 0, w), Spot::Row(t));
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
                // A line that needs no reading has no check, marked or not.
                _ if row.read && !row.free => ("  ✓ ", color::add()),
                _ => ("    ", Style::new()),
            };
            put(buf, area, x, y, MARGIN_W.min(w), text, st);
            x0 += MARGIN_W;
        }
        let tw = w.saturating_sub(x0 - x);
        put_parts(buf, area, x0, y, tw, &row_parts(row, selected));
        self.spot(area, (x, y, w), Spot::Row(i));
    }

    /// The last row of what a comment being written in the middle goes
    /// under: its line and the comments on it, or the turns of the comment
    /// it answers, its references under the editor.
    fn editor_anchor(&self) -> Option<usize> {
        self.editor_anchor_in(&self.rows)
    }

    /// The last of `rows` that a comment being written in the middle goes
    /// under, as `editor_anchor` says.
    fn editor_anchor_in(&self, rows: &[Row]) -> Option<usize> {
        let e = self.writing()?;
        if e.pane != Focus::Diff {
            return None;
        }
        let first = match &e.target {
            Target::Line { key, .. } => rows.iter().position(|r| r.key.as_deref() == Some(key) && r.note.is_none())?,
            Target::Reply { id, .. } | Target::Edit { id, .. } => rows.iter().position(|r| r.note.as_deref() == Some(id))?,
            // Where the turn written again was, or the turns rewound: under
            // what comes before it, over what comes after.
            Target::Turn { id, turn, .. } | Target::Rewind { id, from: turn, .. } => {
                return rows.iter().rposition(|r| r.note.as_deref() == Some(id) && !r.is_reference() && r.turn.is_none_or(|t| t < *turn));
            }
            // Your answer to a reply, in its review: under the thread.
            Target::Answer { run, k } => {
                let id = &self.db.run(run)?.held.as_ref()?.replies.get(*k)?.id;
                return rows.iter().rposition(|r| r.note.as_deref() == Some(id));
            }
            _ => return None,
        };
        let mut last = first;
        while rows.get(last + 1).is_some_and(|r| matches!(r.kind, Kind::Comment | Kind::Stream)) {
            if let Target::Reply { id, .. } | Target::Edit { id, .. } = &e.target
                && (rows[last + 1].note.as_deref() != Some(id) || rows[last + 1].is_reference())
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
        if self.focus != Focus::Notes || self.notes_head || self.on_banner {
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
            put_parts(buf, area, 0, rows_y + k, w, &row_parts(row, false));
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
        let to_review = all.iter().filter(|n| self.shows(Show::ToReview, n, &busy, false)).count();
        let total = self.subject_notes().len();
        let mut header = if self.show == Show::All {
            format!(" {} comment{}", all.len(), plural(all.len()))
        } else {
            format!(" {} · {} of {total}", self.show.name(), all.len())
        };
        if with > 0 {
            header += &format!(" · {with} with the agent");
        }
        if to_review > 0 {
            header += &format!(" · {to_review} to review");
        }
        if to_read > 0 {
            header += &format!(" · {to_read} to read");
        }
        let mut st = color::header();
        if self.focus == Focus::Notes && self.notes_head && !self.on_banner {
            st = st.add_modifier(Modifier::REVERSED);
        }
        put(buf, area, 0, 0, w, &fit(&header, w), st);
        self.spot(area, (0, 0, w), Spot::NotesHead);
        // The one the middle's cursor is in, when the cursor is there.
        let lit = match self.focus {
            _ if self.on_banner => None,
            Focus::Notes if !self.notes_head => Some(self.note_sel),
            Focus::Notes => None,
            _ => self.note_at_cursor(),
        };
        let editing = self.writing().filter(|e| e.pane == Focus::Notes).map(|e| e.target.clone());
        // A comment's lines: compact, its mark, who and where; expanded, the
        // first line of what was said last too.
        let per = if self.comments == Pane::Compact { 1 } else { 2 };
        let edit_h = self.editor_height(area, Focus::Notes);
        let fits = ((h.saturating_sub(1 + edit_h)) / per).max(1);
        let on = lit.unwrap_or(0);
        let first = on.saturating_sub(fits.saturating_sub(1));
        let held = self.held_replies();
        let mut cursor = None;
        let mut y = 1;
        for (i, n) in all.iter().enumerate().skip(first) {
            if y + per > h {
                break;
            }
            let (said, last) = self.note_said(n, &busy, &held);
            let mut head = by_style(Some(&by_of(n)));
            let mut tail = color::dim();
            if lit == Some(i) {
                (head, tail) = if self.focus == Focus::Notes {
                    (head.add_modifier(Modifier::REVERSED), Style::new().add_modifier(Modifier::REVERSED))
                } else {
                    // Its author's colour, on what the cursor's pane is not.
                    (head.patch(color::picked()), color::picked())
                };
            }
            put(buf, area, 0, y, w, &fit(&said, w), head);
            if per == 2 {
                put(buf, area, 0, y + 1, w, &fit(&last, w), tail);
            }
            spot_in(&self.spots, area, (0, y, w, per), Spot::Note(i));
            y += per;
            if matches!(&editing, Some(Target::Reply { change, id } | Target::Edit { change, id } | Target::Turn { change, id, .. } | Target::Rewind { change, id, .. }) if *change == n.change && *id == n.id) {
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
            self.spot(area, (0, y, w), Spot::Pick(i));
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
            self.spot(area, (0, y, w), Spot::Pick(i));
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
            let changed: Vec<Diffed> = crate::history::what_changed(&self.db, rev)
                .into_iter()
                .filter(|d| d.is_change() && !matches!(d, Diffed::Code(_)))
                .collect();
            lines.extend(diff_lines(&changed));
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
            self.spot(area, (0, y, w), Spot::Pick(i));
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
                        format!("{who}: {}", crate::markdown::first_line(&t.body))
                    })
                    .unwrap_or_default();
                let state = self.tag(&n, &busy);
                let state = if state.is_empty() { String::new() } else { format!(" · {state}") };
                lines.push((None, format!(" {}   └ {last}{state}", e.under), by_style(Some(&by_of(&n)))));
            }
        }
        let editing = self.writing().is_some_and(|e| matches!(e.target, Target::About(_)));
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
        let mut owner = 0;
        for (k, (i, text, st)) in lines.iter().enumerate() {
            owner = i.unwrap_or(owner);
            if k < self.pick_top {
                continue;
            }
            if y > h {
                break;
            }
            let st = if *i == Some(self.pick) { st.add_modifier(Modifier::REVERSED) } else { *st };
            put(buf, area, 0, y, w, &fit(text, w), st);
            self.spot(area, (0, y, w), Spot::Pick(owner));
            y += 1;
            if editing && k == at + below {
                cursor = self.draw_editor(buf, area, y, Focus::Diff);
                y += edit_h;
            }
        }
        cursor
    }

    /// The stack diff of the record picked: from the stack before it to the
    /// one after, each patch marked, what changed in each new version under
    /// it. Only to look at.
    fn draw_history_diff(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        let Some(e) = self.hist_entry() else {
            put(buf, area, 1, 0, w, "nothing changed the stack yet", color::dim());
            return;
        };
        let want = (e.from, e.to, w);
        if self.hist_built != Some(want) {
            // Made, or deleted, its header says.
            let mut diff = crate::history::stack_diff(&self.db, e.from, e.to, true);
            diff.retain(|d| !matches!(d, Diffed::Made | Diffed::Deleted));
            self.hist_rows = diff_lines(&diff)
                .into_iter()
                .flat_map(|(l, st)| fold(&l, w.saturating_sub(1), 6).into_iter().map(move |l| (l, st)))
                .collect();
            self.hist_built = Some(want);
            self.hist_cursor = self.hist_cursor.min(self.hist_rows.len().saturating_sub(1));
        }
        let focus = self.hist_focus == HistFocus::Diff;
        let mut head = color::header();
        if focus {
            head = head.add_modifier(Modifier::UNDERLINED);
        }
        put(buf, area, 0, 0, w, &fit(&format!(" {}", e.span()), w), head);
        put(buf, area, 0, 1, w, &fit(&format!(" {} · {}", e.op, local_time(e.time)), w), color::dim());
        let rows = h.saturating_sub(2);
        if self.hist_cursor < self.hist_top {
            self.hist_top = self.hist_cursor;
        } else if self.hist_cursor >= self.hist_top + rows {
            self.hist_top = self.hist_cursor + 1 - rows;
        }
        for y in 0..rows {
            let i = self.hist_top + y;
            let Some((l, st)) = self.hist_rows.get(i) else { break };
            let st = if focus && i == self.hist_cursor { st.add_modifier(Modifier::REVERSED) } else { *st };
            put(buf, area, 0, y + 2, w, &fit(l, w), st);
            spot_in(&self.spots, area, (0, y + 2, w, 1), Spot::HistRow(i));
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
                    RunState::Started => color::note(),
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
            self.spot(area, (0, y, w), Spot::Pick(i));
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

/// What a reply written in $EDITOR opens on, and its line the cursor is
/// put on, from 1: the conversation `text` it answers, each turn under who
/// said it, the references it attaches left out; the empty line after it,
/// where you write; then `refs`, the lines of the comment's references, to
/// name in what you write, and the rules. Every line stacked writes there
/// but the empty ones starts with `#`, and is dropped as any such line is.
fn reply_file(text: &str, refs: &[String]) -> (String, usize) {
    let above = said_turns(&thread_turns(text));
    let mut below = refs_said(refs);
    below.push("# Write your reply above these lines, under the conversation. Lines".into());
    below.push("# starting with # are dropped; with nothing written, nothing is saved.".into());
    (format!("{}\n\n\n{}\n", above.join("\n"), below.join("\n")), above.len() + 1)
}

/// What turn `at` of all the turns of `text`, written again in $EDITOR,
/// opens on, as a reply does: the turns before it, then what it says, the
/// cursor on its first line, under who said it when that is not you; then
/// the turns after it, `refs`, the lines of the comment's references, and
/// the rules. And the lines stacked wrote there, every one starting with
/// `#`: only they are dropped once it is saved, any other line yours, `#`
/// or not. And the line of the cursor, from 1. None if there is no such
/// turn.
fn rewrite_file(text: &str, at: usize, refs: &[String]) -> Option<(String, Vec<String>, usize)> {
    let turns = all_turns(text);
    let t = turns.get(at).filter(|t| !t.deleted)?;
    let mut above = said_turns(turns[..at].iter().filter(|t| !t.deleted));
    if t.role != Role::You {
        above.push(format!("# {}:", who_said(t)));
    }
    let mut below = Vec::new();
    let after: Vec<&Turn> = turns[at + 1..].iter().filter(|t| !t.deleted).collect();
    if !after.is_empty() {
        below.push("# The conversation goes on after it:".to_string());
        below.extend(said_turns(after));
        below.push("#".into());
    }
    below.extend(refs_said(refs));
    below.push(format!("# Write {} again above these lines. The lines stacked wrote here,", t.name(at)));
    below.push("# starting with #, are dropped; any other line is yours, # or not. If you".into());
    below.push("# write nothing, or quit without saving, it stays as it was.".into());
    let line = above.len() + 1;
    let mut lines = above.clone();
    lines.push(t.body.clone());
    lines.push(String::new());
    lines.extend(below.iter().cloned());
    let mut wrote = above;
    wrote.extend(below);
    Some((lines.join("\n") + "\n", wrote, line))
}

/// The lines $EDITOR shows of `turns` of a conversation, each starting with
/// `#`: each turn under who said it, the references it attaches left out.
fn said_turns<'a>(turns: impl IntoIterator<Item = &'a Turn>) -> Vec<String> {
    let mut out = Vec::new();
    for t in turns {
        out.push(format!("# {}:", who_said(t)));
        let (body, _) = crate::refs::split(&t.body);
        out.extend(body.lines().map(|l| if l.trim().is_empty() { "#".to_string() } else { format!("#   {l}") }));
    }
    out
}

/// Who said turn `t`, as $EDITOR's lines say it: `you`, `agent`, `ann`.
fn who_said(t: &Turn) -> &str {
    match t.role {
        Role::You => "you",
        Role::Agent => "agent",
        Role::Reviewer => t.tag.as_deref().filter(|w| !w.is_empty()).unwrap_or("reviewer"),
    }
}

/// The lines $EDITOR shows of a comment's references, `refs`, under what
/// you write: none, without any.
fn refs_said(refs: &[String]) -> Vec<String> {
    if refs.is_empty() {
        return vec![];
    }
    let mut out = vec!["# The comment's references, to name as @r1, @r{1,2}, @r{2-4} or @r*:".to_string()];
    out.extend(refs.iter().map(|r| format!("#   {r}")));
    out.push("#".into());
    out
}

/// Every line a rewind from turn `from` writes in its editor, all of them
/// dropped once saved: its rules, then what goes.
fn rewind_written(text: &str, from: usize) -> Vec<String> {
    let mut out: Vec<String> = REWIND_SAYS.iter().map(|l| l.to_string()).collect();
    out.extend(rewound_lines(text, from));
    out
}

/// The comments whose reply waits for review: (change, id) to the run, and
/// the reply's place in what it holds.
pub type Held = HashMap<(String, String), (String, usize)>;

/// Whether the filter `show` lets `n` be shown: `gone` when its line is
/// not in its patch's version in the stack.
pub fn shows(show: Show, n: &Note, busy: &Busy, held: &Held, gone: bool) -> bool {
    let with = busy.contains_key(&(n.change.clone(), n.id.clone()));
    let waits = held.contains_key(&(n.change.clone(), n.id.clone()));
    let last = last_role(&n.text);
    let to_read = last == Some(Role::Agent) && !with && n.moved.is_none() && n.queued.is_none();
    match show {
        Show::All => true,
        Show::Nothing => false,
        Show::ToSend => !with && !waits && ask::sendable(n),
        Show::ToReview => waits,
        Show::ToRead => to_read,
        Show::ToReadAnswer => to_read && last_reply_way(&n.text).as_deref() == Some("answer"),
        Show::ToReadChange => to_read && last_reply_way(&n.text).as_deref() == Some("change"),
        Show::InProgress => with || n.queued.is_some(),
        Show::Failed => n.failed.is_some() || n.moved.is_some(),
        Show::Gone => gone,
        // The comments there are are open: the closed ones are shown apart.
        Show::Closed => false,
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

/// Where comment `n` of stack `name` is, in a line: on the history, the
/// stack, a patch as a whole, or a line of a file of it.
pub fn where_on(db: &Db, name: &str, n: &Note) -> String {
    if n.about.is_some() {
        return format!("the history, {}", n.preview);
    }
    if n.on_stack() {
        return "the stack".into();
    }
    let at = db
        .stack(name)
        .ok()
        .and_then(|s| s.patches.iter().position(|id| db.patch(id).change == n.change))
        .map_or("a patch no longer in the stack".to_string(), |i| format!("patch {}", i + 1));
    let p = db.latest_version(&n.change);
    if n.file == MSG_PATH && p.is_some_and(|p| title_key(p) == n.line) {
        format!("{at}, as a whole")
    } else {
        format!("{at} · {}: {}", n.file, n.preview)
    }
}

/// What a comment's first line says of where it stands: with claude,
/// ready, failed, replied, moved, and what its answer opened.
pub fn note_state(db: &Db, name: &str, n: &Note, busy: &Busy) -> String {
    let mut parts = Vec::new();
    if let Some((way, run)) = busy.get(&(n.change.clone(), n.id.clone())) {
        // A patch's comment, asked of the whole stack.
        let whole = *way == Way::Change && !n.on_stack() && ask::run_scope(db, run) == Some(Scope::Stack);
        parts.push(format!("with the agent ({})", if whole { crate::db::WHOLE } else { way.said() }));
    } else if let Some(q) = n.queued.as_ref().filter(|q| q.batched()) {
        parts.push(format!("batched ({})", q.said()));
    } else if let Some(q) = &n.queued {
        let on = ask::waits_on(db, name, q.way).unwrap_or_else(|| "next".into());
        parts.push(format!("ready ({}), {on}", q.said()));
    } else if let Some((run, k)) = crate::held::reply_waiting(db, &n.change, &n.id) {
        let said = db.run(&run).and_then(|r| r.held.as_ref()).and_then(|h| h.replies.get(k)).map_or("the agent's reply".into(), |rp| rp.said());
        parts.push(format!("{said} waits for your review"));
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
    // The asks still running, each where its agent works: read again, for
    // one started a moment ago.
    let now = Db::load(path);
    let running = ask::still_running(now.as_ref().unwrap_or(&app.db));
    if !running.is_empty() {
        eprintln!("stacked: {} of the agent's asks still running; what they do lands on its own:", running.len());
        for l in running {
            eprintln!("  {l}");
        }
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
            Effect::Quit => {
                app.leave();
                return Ok(());
            }
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_reply_in_editor_opens_under_the_conversation_its_references_under_it() {
        let text = [
            "Why so loud?".to_string(),
            crate::thread::fenced("agent answer", "As @r2 shows.\n\nIt is a shout.\n\n```refs\nr2 a.txt:2 @1\n```"),
            crate::thread::fenced("reviewer ann", "Agreed."),
        ]
        .join("\n");
        let refs = ["r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\"".to_string(), "r2 ⇢ a.txt:2 · as patch 1 leaves it (the agent's)".to_string()];
        let (file, at) = super::reply_file(&text, &refs);
        let lines: Vec<&str> = file.lines().collect();
        assert_eq!(
            lines,
            [
                "# you:",
                "#   Why so loud?",
                "# agent:",
                "#   As @r2 shows.",
                "#",
                "#   It is a shout.",
                "# ann:",
                "#   Agreed.",
                "",
                "",
                "# The comment's references, to name as @r1, @r{1,2}, @r{2-4} or @r*:",
                "#   r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\"",
                "#   r2 ⇢ a.txt:2 · as patch 1 leaves it (the agent's)",
                "#",
                "# Write your reply above these lines, under the conversation. Lines",
                "# starting with # are dropped; with nothing written, nothing is saved.",
            ]
        );
        // The cursor on the line after the last reply; what is written there
        // is all that is kept.
        assert_eq!((at, lines[at - 2], lines[at - 1]), (9, "#   Agreed.", ""));
        let mut written = lines.clone();
        written[at - 1] = "So be it.";
        assert_eq!(crate::ops::strip_comments(&written.join("\n")), "So be it.");
        // No references: the rules alone under it.
        let (file, at) = super::reply_file("Why?", &[]);
        assert_eq!((file.as_str(), at), ("# you:\n#   Why?\n\n\n# Write your reply above these lines, under the conversation. Lines\n# starting with # are dropped; with nothing written, nothing is saved.\n", 3));
    }

    #[test]
    fn a_turn_written_again_in_editor_is_in_its_place_in_the_conversation() {
        let text = [
            "Why so loud?".to_string(),
            crate::thread::fenced("agent answer", "As @r2 shows.\n\n```refs\nr2 a.txt:2 @1\n```"),
            "Then @r2 it is.\n# Mine, kept".to_string(),
        ]
        .join("\n");
        let refs = ["r2 ⇢ a.txt:2 · as patch 1 leaves it (the agent's)".to_string()];
        let rules = |name: &str| {
            [
                format!("# Write {name} again above these lines. The lines stacked wrote here,"),
                "# starting with #, are dropped; any other line is yours, # or not. If you".into(),
                "# write nothing, or quit without saving, it stays as it was.".into(),
            ]
        };
        let refs_lines = ["# The comment's references, to name as @r1, @r{1,2}, @r{2-4} or @r*:", "#   r2 ⇢ a.txt:2 · as patch 1 leaves it (the agent's)", "#"];
        // Your last reply: the conversation above it, the cursor on it, a #
        // line of yours kept.
        let (file, wrote, line) = super::rewrite_file(&text, 2, &refs).unwrap();
        let mut want: Vec<String> = ["# you:", "#   Why so loud?", "# agent:", "#   As @r2 shows.", "Then @r2 it is.", "# Mine, kept", ""].map(String::from).to_vec();
        want.extend(refs_lines.map(String::from));
        want.extend(rules("your reply"));
        assert_eq!(file.lines().collect::<Vec<_>>(), want);
        assert_eq!(line, 5);
        assert_eq!(crate::thread::without_written(&file, &wrote), "Then @r2 it is.\n# Mine, kept");
        // The agent's, in the middle: under who said it, the turns after it
        // under it.
        let (file, wrote, line) = super::rewrite_file(&text, 1, &refs).unwrap();
        let mut want: Vec<String> = ["# you:", "#   Why so loud?", "# agent:", "As @r2 shows.", "", "```refs", "r2 a.txt:2 @1", "```", ""].map(String::from).to_vec();
        want.extend(["# The conversation goes on after it:", "# you:", "#   Then @r2 it is.", "#   # Mine, kept", "#"].map(String::from));
        want.extend(refs_lines.map(String::from));
        want.extend(rules("the agent's reply"));
        assert_eq!(file.lines().collect::<Vec<_>>(), want);
        assert_eq!(line, 4);
        assert_eq!(crate::thread::without_written(&file, &wrote), "As @r2 shows.\n\n```refs\nr2 a.txt:2 @1\n```");
        // Your comment, first: nothing above it. No such turn: none.
        let (file, _, line) = super::rewrite_file(&text, 0, &[]).unwrap();
        assert!(file.starts_with("Why so loud?\n\n# The conversation goes on after it:\n# agent:\n"), "{file}");
        assert_eq!(line, 1);
        assert!(super::rewrite_file(&text, 3, &refs).is_none());
    }

    #[test]
    fn a_long_line_of_a_stack_diff_folds_between_words() {
        let line = " 1 ~ Bug 1 - parse skips blank lines, total does not overflow (reworded)";
        assert_eq!(
            super::fold(line, 40, 6),
            [" 1 ~ Bug 1 - parse skips blank lines,", "      total does not overflow (reworded)"]
        );
        // A word longer than the line is cut where the line ends.
        assert_eq!(super::fold("     ++averyveryverylongwordindeedsolong", 20, 6), ["     ++averyveryvery", "      longwordindeed", "      solong"]);
        assert_eq!(super::fold("short", 20, 6), ["short"]);
    }
}
