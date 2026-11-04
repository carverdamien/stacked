//! The settings, in `~/.stacked/config.toml` (or the file `$STACKED_CONFIG`
//! names, or `config.toml` in the folder `$STACKED_HOME` names: `home`): the
//! database opened by default, the editor and how it is told a line, the
//! agent claude's asks run, where reviews come from, how the editor starts,
//! the keys, and the port `stacked web` listens on. Every one is optional,
//! and what is not set is as it would be with no file. A file that cannot
//! be read is said, and not used: the defaults are. `stacked config` prints
//! every setting as it is, in the file's form.
//!
//! A checkout's own settings, `.stacked/local.toml` at its top, override
//! those there: those of the checkout stacked runs in, and for a stack's
//! agent those of the stack's checkout (`of`); but for `stacked web`'s,
//! which are the settings' file's alone, as `"fixed-random"` keeps its port
//! there. One that git tracks is not read: it would be the repository's,
//! and a setting can name a program to run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ratatui::style::Style;
use serde::Deserialize;

use crate::keys::{Context, Key, Keymap};
use crate::paint::{Paint, parse_color};

/// How the code is drawn at the start: unified, side by side, compact, or
/// unified in each half of the middle split in two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Unified,
    Split,
    Compact,
    Halves,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Unified => "unified",
            Layout::Split => "side-by-side",
            Layout::Compact => "compact",
            Layout::Halves => "unified-split",
        }
    }

    /// The layout `s` names. `split`, as side by side was named before, is
    /// read as it still.
    fn parse(s: &str) -> Option<Layout> {
        match s {
            "unified" => Some(Layout::Unified),
            "side-by-side" | "split" => Some(Layout::Split),
            "compact" => Some(Layout::Compact),
            "unified-split" => Some(Layout::Halves),
            _ => None,
        }
    }
}

/// How the interdiffs are drawn at the start, on every screen that shows
/// one: the interdiff, as an interdiff, each line a sign saying which
/// version has it before that version's own sign; in a column per version,
/// what each version's patch does with each line, as git's combined diff
/// draws a merge; or side by side, the old version on the left and the new
/// one on the right. Or the change from the old version to the new one as
/// a patch of its own, each file as the one leaves it on its base, then as
/// the other leaves it on its own, in the views of the code: unified, side
/// by side, or side by side and compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterLayout {
    #[default]
    Interdiff,
    Columns,
    Sides,
    ChangeUnified,
    ChangeSides,
    ChangeCompact,
}

impl InterLayout {
    pub fn name(self) -> &'static str {
        match self {
            InterLayout::Interdiff => "interdiff",
            InterLayout::Columns => "columns",
            InterLayout::Sides => "side-by-side",
            InterLayout::ChangeUnified => "change-unified",
            InterLayout::ChangeSides => "change-side-by-side",
            InterLayout::ChangeCompact => "change-compact",
        }
    }

    /// The view `s` names.
    fn parse(s: &str) -> Option<InterLayout> {
        match s {
            "interdiff" => Some(InterLayout::Interdiff),
            "columns" => Some(InterLayout::Columns),
            "side-by-side" => Some(InterLayout::Sides),
            "change-unified" => Some(InterLayout::ChangeUnified),
            "change-side-by-side" => Some(InterLayout::ChangeSides),
            "change-compact" => Some(InterLayout::ChangeCompact),
            _ => None,
        }
    }

    /// Whether it draws the change as a patch of its own, rather than the
    /// interdiff.
    pub fn change(self) -> bool {
        matches!(self, InterLayout::ChangeUnified | InterLayout::ChangeSides | InterLayout::ChangeCompact)
    }

    /// Whether it needs two sides, and the room for them (`split_width`).
    pub fn sides(self) -> bool {
        matches!(self, InterLayout::Sides | InterLayout::ChangeSides | InterLayout::ChangeCompact)
    }
}

/// What `view.interdiff` is told it is not, when it names no view.
const NOT_AN_INTERDIFF_VIEW: &str = "interdiff, columns, side-by-side, change-unified, change-side-by-side or change-compact";

/// Which lines the interdiffs are drawn with at the start, on every screen
/// that shows one, in every view of them: every line, as the two versions'
/// diffs have them; or only what the patches changed, the lines both
/// versions have alike, the context lines of their diffs, their index
/// lines and the numbers of a hunk's header that a change of base moved
/// left out, and of the change from the old version to the new one, the
/// lines the change of base made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterLines {
    #[default]
    Every,
    Changed,
}

impl InterLines {
    pub fn name(self) -> &'static str {
        match self {
            InterLines::Every => "every",
            InterLines::Changed => "changed",
        }
    }

    /// The lines `s` names.
    fn parse(s: &str) -> Option<InterLines> {
        match s {
            "every" => Some(InterLines::Every),
            "changed" => Some(InterLines::Changed),
            _ => None,
        }
    }
}

/// How the patches pane is shown: not at all; listing the files of the
/// patch picked under it, a line each, to jump to; or expanded, each patch
/// its title whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatchesPane {
    Hidden,
    Files,
    #[default]
    Expanded,
}

impl PatchesPane {
    pub fn name(self) -> &'static str {
        match self {
            PatchesPane::Hidden => "hidden",
            PatchesPane::Files => "files",
            PatchesPane::Expanded => "expanded",
        }
    }

    /// The state `s` names. `compact`, the patches' state before the files
    /// took its place, is read as the files.
    fn parse(s: &str) -> Option<PatchesPane> {
        match s {
            "hidden" => Some(PatchesPane::Hidden),
            "files" | "compact" => Some(PatchesPane::Files),
            "expanded" => Some(PatchesPane::Expanded),
            _ => None,
        }
    }

    /// The next, as ← on the patches cycles them: expanded, files, hidden,
    /// then expanded again.
    pub fn next(self) -> PatchesPane {
        match self {
            PatchesPane::Expanded => PatchesPane::Files,
            PatchesPane::Files => PatchesPane::Hidden,
            PatchesPane::Hidden => PatchesPane::Expanded,
        }
    }

    pub fn shown(self) -> bool {
        self != PatchesPane::Hidden
    }
}

/// How the comments pane is shown: not at all; compact, a line for each
/// comment, as narrow as those need; or expanded, the first line of what
/// was said last under each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    Hidden,
    Compact,
    #[default]
    Expanded,
}

impl Pane {
    pub fn name(self) -> &'static str {
        match self {
            Pane::Hidden => "hidden",
            Pane::Compact => "compact",
            Pane::Expanded => "expanded",
        }
    }

    fn parse(s: &str) -> Option<Pane> {
        match s {
            "hidden" => Some(Pane::Hidden),
            "compact" => Some(Pane::Compact),
            "expanded" => Some(Pane::Expanded),
            _ => None,
        }
    }

    /// The next, as → on the comments cycles them: expanded, compact,
    /// hidden, then expanded again.
    pub fn next(self) -> Pane {
        match self {
            Pane::Expanded => Pane::Compact,
            Pane::Compact => Pane::Hidden,
            Pane::Hidden => Pane::Expanded,
        }
    }

    pub fn shown(self) -> bool {
        self != Pane::Hidden
    }
}

/// Where reviews are pulled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Phabricator,
    /// Nowhere: no pulling.
    None,
}

/// Where an ask's agent works: always a worktree of the stack's own, never
/// your checkout. The setting says when the worktree is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    /// Made the first time an ask needs one, and reused while it holds
    /// nothing an agent left unfinished.
    OnDemand,
    /// The same, made when the editor opens the stack, and removed when it
    /// exits.
    Precreate,
}

impl Workspace {
    pub fn name(self) -> &'static str {
        match self {
            Workspace::OnDemand => "ondemand",
            Workspace::Precreate => "precreate",
        }
    }

    fn parse(s: &str) -> Option<Workspace> {
        match s {
            "ondemand" => Some(Workspace::OnDemand),
            "precreate" => Some(Workspace::Precreate),
            _ => None,
        }
    }
}

/// What a stacked of before took, in `agent.worktree` and
/// `agent.answer_only_worktree`, for the checkout itself: the agent then
/// worked in your checkout. No ask does now.
const PWD: &str = "pwd";

/// Why `key` no longer takes `"pwd"`.
fn no_pwd(key: &str) -> String {
    format!("{key} = \"pwd\" is no longer supported: asks run in a worktree of their own")
}

/// What answer-only asks may use unless the settings say otherwise: the
/// web, read with curl, and what curl reads, through jq.
pub const ANSWER_TOOLS: [&str; 2] = ["Bash(curl *)", "Bash(jq *)"];

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// The program run as claude; else `$STACKED_CLAUDE`'s, else `claude`.
    pub command: Option<String>,
    /// Arguments added to every run.
    pub args: Vec<String>,
    /// `--model` for every run.
    pub model: Option<String>,
    /// Tools every ask may use besides its own, build and test asks too,
    /// as claude names them for `--allowedTools`: an MCP server's, a
    /// command.
    pub tools: Vec<String>,
    /// Tools asks that change the stack may use, besides what stacked gives
    /// them: none unless set, as nothing they are given reaches the network
    /// but the checkout's linter, yours.
    pub change_tools: Vec<String>,
    /// Tools answer-only asks may use, besides what stacked gives them:
    /// unless set, curl and jq, to read the web.
    pub answer_tools: Vec<String>,
    /// Tools the asks that select a try push's tasks may use, besides what
    /// stacked gives them, which is what it gives answers: unless set, curl
    /// and jq, to read the web, as answers.
    #[cfg(feature = "try-push")]
    pub try_tools: Vec<String>,
    /// What the reviews of the agent's work allowed from now on, of what it
    /// was refused, for each kind of ask: a list of its own each, added to
    /// that kind's tools, so that it never hides the lists the setup
    /// writes. Written in a checkout's own settings alone, by its reviews
    /// (`refusals::save`).
    pub change_accepted: Vec<String>,
    pub answer_accepted: Vec<String>,
    pub test_accepted: Vec<String>,
    #[cfg(feature = "try-push")]
    pub try_accepted: Vec<String>,
    /// When a change's worktree is made.
    pub worktree: Workspace,
    /// When an answer's worktree is made, a try push's selection's too.
    pub answer_only_worktree: Workspace,
    /// Where the agents' worktrees go; else `.stacked/` at the top of the
    /// stack's repository.
    pub worktrees: Option<PathBuf>,
    /// Whether a change or an answer starts while the change before it waits
    /// for your review, from the stack that change makes, as if accepted:
    /// rejected, sent back or edited by hand, what went on from it starts
    /// again.
    pub chain: bool,
    /// Whether the agent gives its answer as structured output, checked
    /// against what the ask asks for (`--json-schema`): off for an agent
    /// that does not take it, its answer then read from what it writes.
    pub schema: bool,
    /// Whether the ask dialog checks that the network reaches the host
    /// claude talks to before it offers Ask Now (`reach`): off for a setup
    /// where that check is wrong though claude works.
    pub reach_check: bool,
}

impl Agent {
    /// What the reviews of the agent's work allowed from now on, for the
    /// asks made in `way`.
    pub fn accepted(&self, way: crate::db::Way) -> &[String] {
        use crate::db::Way;
        match way {
            Way::Change => &self.change_accepted,
            Way::Answer => &self.answer_accepted,
            Way::Test => &self.test_accepted,
            #[cfg(feature = "try-push")]
            Way::Try => &self.try_accepted,
        }
    }
}

/// The setting that holds what the reviews of the agent's work allowed from
/// now on, for the asks made in `way`: `agent.answer_accepted`.
pub fn accepted_key(way: crate::db::Way) -> &'static str {
    use crate::db::Way;
    match way {
        Way::Change => "agent.change_accepted",
        Way::Answer => "agent.answer_accepted",
        Way::Test => "agent.test_accepted",
        #[cfg(feature = "try-push")]
        Way::Try => "agent.try_accepted",
    }
}

impl Default for Agent {
    fn default() -> Agent {
        Agent {
            command: None,
            args: Vec::new(),
            model: None,
            tools: Vec::new(),
            change_tools: Vec::new(),
            answer_tools: ANSWER_TOOLS.map(String::from).to_vec(),
            #[cfg(feature = "try-push")]
            try_tools: ANSWER_TOOLS.map(String::from).to_vec(),
            change_accepted: Vec::new(),
            answer_accepted: Vec::new(),
            test_accepted: Vec::new(),
            #[cfg(feature = "try-push")]
            try_accepted: Vec::new(),
            worktree: Workspace::OnDemand,
            answer_only_worktree: Workspace::OnDemand,
            worktrees: None,
            chain: true,
            schema: true,
            reach_check: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reviews {
    pub source: Source,
    /// The Phabricator a bare `D123` is on, and whose token is used.
    pub url: Option<String>,
    /// Where tokens are: moz-phab's and arc's file.
    pub arcrc: PathBuf,
    /// A token to use rather than the file's.
    pub token: Option<String>,
    /// The trailer naming a patch's revision.
    pub trailer: String,
    /// The Bugzilla that a bug a commit message names (`Bug 123`) is
    /// opened on, in the browser.
    pub bugzilla: String,
}

/// The Bugzilla a bug is opened on unless the settings say otherwise.
pub const BUGZILLA: &str = "https://bugzilla.mozilla.org";

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub layout: Layout,
    /// How the interdiffs are drawn at the start, on every screen that
    /// shows one: as interdiffs, in a column per version, or side by side;
    /// or as the change from one version to the other, a patch of its own,
    /// unified, side by side, or compact.
    pub interdiff: InterLayout,
    /// The lines the interdiffs are drawn with at the start, in every view
    /// of them: every line, or only what the patches changed.
    pub interdiff_lines: InterLines,
    /// Reading rather than commenting, at the start.
    pub reading: bool,
    /// Whether changed lines that are empty or white space alone need
    /// review, in the reading mode: when not, they need no mark, and no
    /// count of lines to read holds them.
    pub whitespace_needs_review: bool,
    /// The patches on the left, at the start: expanded, files or hidden.
    pub patches: PatchesPane,
    /// The comments on the right, when there are any, at the start.
    pub comments: Pane,
    /// The lines of claude's stream under a comment it works on.
    pub stream_lines: usize,
    /// The columns the middle needs for two sides, or for two halves; and
    /// a pane, for its interdiffs side by side, or the change of a patch's
    /// version side by side.
    pub split_width: usize,
    /// The columns a commit message's lines are wrapped at, when a menu
    /// offers to wrap its long lines (`wrap`).
    pub message_width: usize,
}

impl Default for View {
    fn default() -> View {
        View {
            layout: Layout::Unified,
            interdiff: InterLayout::Interdiff,
            interdiff_lines: InterLines::Every,
            reading: false,
            whitespace_needs_review: false,
            patches: PatchesPane::Expanded,
            comments: Pane::Expanded,
            stream_lines: 3,
            split_width: 60,
            message_width: 72,
        }
    }
}

/// What paints each part of the screen: for each, the colour as git writes
/// one, and the style it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    specs: Vec<String>,
    styles: Vec<Style>,
}

impl Default for Colors {
    fn default() -> Colors {
        let specs: Vec<String> = Paint::ALL.iter().map(|p| p.default_spec().to_string()).collect();
        let styles = specs.iter().map(|s| parse_color(s).unwrap_or_default()).collect();
        Colors { specs, styles }
    }
}

impl Colors {
    pub fn style(&self, p: Paint) -> Style {
        self.styles[p as usize]
    }

    /// As the settings write it.
    pub fn spec(&self, p: Paint) -> &str {
        &self.specs[p as usize]
    }

    /// `p` made `spec`, if it is a colour.
    fn set(&mut self, p: Paint, spec: &str) -> Result<(), String> {
        self.styles[p as usize] = parse_color(spec)?;
        self.specs[p as usize] = spec.to_string();
        Ok(())
    }
}

/// A checkout's linter, which `Lint the stack` runs (`stacked setup
/// lint`): a checkout's own setting alone, read only from its local file,
/// never from the settings' file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lint {
    /// The command, as the local file writes it: a program and its
    /// arguments, the files to lint after them. Empty, lint is off; unset,
    /// a checkout whose top has `./mach` lints with `./mach lint --fix`.
    pub command: Option<String>,
}

/// The tour, `stacked demo`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Demo {
    /// It was opened before: it starts on the list of its sections.
    pub seen: bool,
}

/// The port of 127.0.0.1 `stacked web` listens on, unless `--port` names
/// one: any free one each time; any free one the first time, which stacked
/// then writes in the settings' file (`web.kept_port`) and listens on each
/// time after; or the one named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Port {
    #[default]
    Random,
    FixedRandom,
    Fixed(u16),
}

impl Port {
    /// As `stacked config` says it: `random`, `fixed-random`, or the port.
    pub fn name(self) -> String {
        match self {
            Port::Random => "random".into(),
            Port::FixedRandom => "fixed-random".into(),
            Port::Fixed(p) => p.to_string(),
        }
    }

    /// The port `s` names, as `stacked config` is given it: `random`,
    /// `fixed-random`, or a port from 1 to 65535.
    fn parse(s: &str) -> Option<Port> {
        match s {
            "random" => Some(Port::Random),
            "fixed-random" => Some(Port::FixedRandom),
            _ => port_number(s).map(Port::Fixed),
        }
    }

    /// The port `v` names, as the file writes it: "random", "fixed-random",
    /// or a port, a number (or one in quotes).
    fn read(v: &toml::Value) -> Option<Port> {
        match v {
            toml::Value::String(s) => Port::parse(s),
            v => port_of(v).map(Port::Fixed),
        }
    }

    /// As the file writes it: a name in quotes, a port as a number.
    fn literal(self) -> String {
        match self {
            Port::Fixed(p) => p.to_string(),
            named => toml::Value::String(named.name()).to_string(),
        }
    }
}

/// The port `s` is, if it is one a setting may name: from 1 to 65535.
fn port_number(s: &str) -> Option<u16> {
    s.parse::<u16>().ok().filter(|p| *p > 0)
}

/// The port `v` is, as the file writes it, if it is one a setting may name:
/// a number from 1 to 65535, or one in quotes.
fn port_of(v: &toml::Value) -> Option<u16> {
    match v {
        toml::Value::Integer(n) => u16::try_from(*n).ok().filter(|p| *p > 0),
        toml::Value::String(s) => port_number(s),
        _ => None,
    }
}

/// What `web.port` is told it is not, when it names no port.
const NOT_A_PORT: &str = "\"random\", \"fixed-random\" or a port from 1 to 65535";

/// `stacked web`: settings of the settings' file alone, never read from a
/// checkout's own, as `"fixed-random"` keeps its port there, for every
/// checkout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Web {
    /// The port it listens on, unless `--port` names one.
    pub port: Port,
    /// The port `"fixed-random"` took the first time, which stacked wrote in
    /// the settings' file: listened on each time `port` is `"fixed-random"`,
    /// and passed over, but kept, while it is another.
    pub kept_port: Option<u16>,
}

/// Every setting, defaults filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The database opened when none is named, nor `$STACKED_FILE`.
    pub file: Option<PathBuf>,
    /// The editor; else `$VISUAL`, else `$EDITOR`, else vi.
    pub editor: Option<String>,
    pub agent: Agent,
    pub reviews: Reviews,
    pub view: View,
    pub colors: Colors,
    pub keys: Keymap,
    pub web: Web,
    pub lint: Lint,
    pub demo: Demo,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            file: None,
            editor: None,
            agent: Agent::default(),
            reviews: Reviews {
                source: Source::Phabricator,
                url: None,
                arcrc: home().join(".arcrc"),
                token: None,
                trailer: "Differential Revision".into(),
                bugzilla: BUGZILLA.into(),
            },
            view: View::default(),
            colors: Colors::default(),
            keys: Keymap::default(),
            web: Web::default(),
            lint: Lint::default(),
            demo: Demo::default(),
        }
    }
}

// -- the file as written ------------------------------------------------- //

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Written {
    file: Option<String>,
    editor: Option<String>,
    agent: WrittenAgent,
    reviews: WrittenReviews,
    view: WrittenView,
    colors: BTreeMap<String, String>,
    keys: BTreeMap<String, OneOrMore>,
    web: WrittenWeb,
    lint: WrittenLint,
    demo: WrittenDemo,
}

/// `[web]` as the file writes it: each value read whatever it is, so that
/// one of the wrong kind is said, its default kept, rather than the file
/// not used.
#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenWeb {
    port: Option<toml::Value>,
    kept_port: Option<toml::Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenLint {
    command: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenDemo {
    seen: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenAgent {
    command: Option<String>,
    args: Option<Vec<String>>,
    model: Option<String>,
    tools: Option<Vec<String>>,
    change_tools: Option<Vec<String>>,
    answer_tools: Option<Vec<String>>,
    /// Read built without the `try-push` feature too, to be passed over
    /// there: a file a stacked built with it wrote reads in any.
    #[cfg_attr(not(feature = "try-push"), allow(dead_code))]
    try_tools: Option<Vec<String>>,
    change_accepted: Option<Vec<String>>,
    answer_accepted: Option<Vec<String>>,
    test_accepted: Option<Vec<String>>,
    #[cfg_attr(not(feature = "try-push"), allow(dead_code))]
    try_accepted: Option<Vec<String>>,
    /// The setup's version, as a stacked of before wrote it once the setup
    /// was saved, when no ask went before it: read, and passed over.
    #[allow(dead_code)]
    setup: Option<usize>,
    worktree: Option<String>,
    answer_only_worktree: Option<String>,
    worktrees: Option<String>,
    chain: Option<bool>,
    schema: Option<bool>,
    reach_check: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenReviews {
    source: Option<String>,
    url: Option<String>,
    arcrc: Option<String>,
    token: Option<String>,
    trailer: Option<String>,
    bugzilla: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenView {
    layout: Option<String>,
    interdiff: Option<String>,
    interdiff_lines: Option<String>,
    reading: Option<bool>,
    whitespace_needs_review: Option<bool>,
    patches: Option<WrittenPane>,
    comments: Option<WrittenPane>,
    stream_lines: Option<usize>,
    split_width: Option<usize>,
    message_width: Option<usize>,
}

/// A side pane as the file writes it: its mode, or, as before there were
/// modes, shown (expanded) or not.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WrittenPane {
    Flag(bool),
    Mode(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMore {
    One(String),
    More(Vec<String>),
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// `~/x`, as a setting names a path, as a path in $HOME.
pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

impl Config {
    /// The settings `text` says, and what it got wrong: settings it got
    /// wrong keep their defaults. Unreadable as a whole, an error.
    pub fn parse(text: &str) -> Result<(Config, Vec<String>), String> {
        Config::default().over(text)
    }

    /// These settings with those `text` says over them, and what it got
    /// wrong: a setting it does not set, or sets wrong, stays as it is
    /// here; a list it sets is its own, a table's settings each its own.
    /// Unreadable as a whole, an error.
    pub fn over(&self, text: &str) -> Result<(Config, Vec<String>), String> {
        let w: Written = toml::from_str(text).map_err(|e| e.to_string().trim().to_string())?;
        let mut c = self.clone();
        let mut wrong = Vec::new();
        if let Some(f) = w.file {
            c.file = Some(expand(&f));
        }
        // An empty one is none: the editor of $VISUAL, the agent of
        // $STACKED_CLAUDE, as they would be without.
        if let Some(e) = w.editor {
            c.editor = Some(e).filter(|e| !e.trim().is_empty());
        }
        let a = w.agent;
        if let Some(command) = a.command {
            c.agent.command = Some(command).filter(|e| !e.trim().is_empty());
        }
        if let Some(args) = a.args {
            c.agent.args = args;
        }
        if let Some(m) = a.model {
            c.agent.model = Some(m);
        }
        if let Some(tools) = a.tools {
            c.agent.tools = tools;
        }
        if let Some(tools) = a.change_tools {
            c.agent.change_tools = tools;
        }
        if let Some(tools) = a.answer_tools {
            c.agent.answer_tools = tools;
        }
        #[cfg(feature = "try-push")]
        if let Some(tools) = a.try_tools {
            c.agent.try_tools = tools;
        }
        for (written, at) in [
            (a.change_accepted, &mut c.agent.change_accepted),
            (a.answer_accepted, &mut c.agent.answer_accepted),
            (a.test_accepted, &mut c.agent.test_accepted),
        ] {
            if let Some(tools) = written {
                *at = tools;
            }
        }
        #[cfg(feature = "try-push")]
        if let Some(tools) = a.try_accepted {
            c.agent.try_accepted = tools;
        }
        if let Some(d) = a.worktrees {
            c.agent.worktrees = Some(expand(&d));
        }
        c.agent.chain = a.chain.unwrap_or(c.agent.chain);
        c.agent.schema = a.schema.unwrap_or(c.agent.schema);
        c.agent.reach_check = a.reach_check.unwrap_or(c.agent.reach_check);
        for (key, written, at) in [
            ("agent.worktree", &a.worktree, &mut c.agent.worktree),
            ("agent.answer_only_worktree", &a.answer_only_worktree, &mut c.agent.answer_only_worktree),
        ] {
            if let Some(s) = written {
                match Workspace::parse(s) {
                    Some(p) => *at = p,
                    // Your checkout, as a stacked of before let an agent
                    // work in: a worktree of its own instead, made when an
                    // ask needs it; and said.
                    None if s == PWD => {
                        *at = Workspace::OnDemand;
                        wrong.push(format!("{}; read as \"ondemand\"", no_pwd(key)));
                    }
                    None => wrong.push(format!("{key}: {s:?} is not ondemand or precreate")),
                }
            }
        }
        if let Some(s) = w.reviews.source {
            match s.as_str() {
                "phabricator" => c.reviews.source = Source::Phabricator,
                "none" => c.reviews.source = Source::None,
                _ => wrong.push(format!("reviews.source: {s:?} is neither \"phabricator\" nor \"none\"")),
            }
        }
        if let Some(u) = w.reviews.url {
            c.reviews.url = Some(u.trim_end_matches('/').to_string());
        }
        if let Some(a) = w.reviews.arcrc {
            c.reviews.arcrc = expand(&a);
        }
        if let Some(t) = w.reviews.token {
            c.reviews.token = Some(t);
        }
        if c.reviews.token.is_some() && c.reviews.url.is_none() {
            wrong.push("reviews.token: it needs reviews.url, the Phabricator it is for".into());
            c.reviews.token = None;
        }
        if let Some(t) = w.reviews.trailer {
            c.reviews.trailer = t.trim_end_matches(':').to_string();
        }
        if let Some(b) = w.reviews.bugzilla {
            let b = b.trim().trim_end_matches('/').to_string();
            match crate::refs::link(&b) {
                Ok(_) => c.reviews.bugzilla = b,
                Err(_) => wrong.push(format!("reviews.bugzilla: {b:?} is not a web link to a site, https://… or http://…")),
            }
        }
        let v = w.view;
        if let Some(l) = v.layout {
            match Layout::parse(&l) {
                Some(l) => c.view.layout = l,
                None => wrong.push(format!("view.layout: {l:?} is not unified, side-by-side, compact or unified-split")),
            }
        }
        if let Some(i) = v.interdiff {
            match InterLayout::parse(&i) {
                Some(i) => c.view.interdiff = i,
                None => wrong.push(format!("view.interdiff: {i:?} is not {NOT_AN_INTERDIFF_VIEW}")),
            }
        }
        if let Some(l) = v.interdiff_lines {
            match InterLines::parse(&l) {
                Some(l) => c.view.interdiff_lines = l,
                None => wrong.push(format!("view.interdiff_lines: {l:?} is not every or changed")),
            }
        }
        c.view.reading = v.reading.unwrap_or(c.view.reading);
        c.view.whitespace_needs_review = v.whitespace_needs_review.unwrap_or(c.view.whitespace_needs_review);
        match v.patches {
            Some(WrittenPane::Flag(shown)) => c.view.patches = if shown { PatchesPane::Expanded } else { PatchesPane::Hidden },
            Some(WrittenPane::Mode(m)) => match PatchesPane::parse(&m) {
                Some(p) => c.view.patches = p,
                None => wrong.push(format!("view.patches: {m:?} is not expanded, files or hidden")),
            },
            None => {}
        }
        match v.comments {
            Some(WrittenPane::Flag(shown)) => c.view.comments = if shown { Pane::Expanded } else { Pane::Hidden },
            Some(WrittenPane::Mode(m)) => match Pane::parse(&m) {
                Some(p) => c.view.comments = p,
                None => wrong.push(format!("view.comments: {m:?} is not expanded, compact or hidden")),
            },
            None => {}
        }
        c.view.stream_lines = v.stream_lines.unwrap_or(c.view.stream_lines);
        if let Some(n) = v.split_width {
            if n < 20 {
                wrong.push(format!("view.split_width: {n} is too narrow for two sides"));
            } else {
                c.view.split_width = n;
            }
        }
        if let Some(n) = v.message_width {
            if n < 20 {
                wrong.push(format!("view.message_width: {n} is too narrow for the lines of a message"));
            } else {
                c.view.message_width = n;
            }
        }
        for (name, spec) in &w.colors {
            // A colour there is not, as any setting there is not: the file
            // is not used.
            let Some(p) = Paint::named(name) else {
                let all: Vec<&str> = Paint::ALL.iter().map(|p| p.name()).collect();
                return Err(format!("unknown colour `{name}` in [colors]: the colours are {}", all.join(", ")));
            };
            let was = c.colors.spec(p).to_string();
            if let Err(e) = c.colors.set(p, spec) {
                wrong.push(format!("colors.{name}: {e}; it stays {was:?}"));
            }
        }
        let named: Vec<(String, Vec<String>)> = w
            .keys
            .into_iter()
            .map(|(k, v)| {
                let v = match v {
                    OneOrMore::One(s) => vec![s],
                    OneOrMore::More(v) => v,
                };
                (k, v)
            })
            .collect();
        let (keys, bad) = c.keys.with_names(&named);
        c.keys = keys;
        wrong.extend(bad);
        if let Some(v) = w.web.port {
            match Port::read(&v) {
                Some(p) => c.web.port = p,
                None => wrong.push(format!("web.port: {v} is not {NOT_A_PORT}")),
            }
        }
        if let Some(v) = w.web.kept_port {
            match port_of(&v) {
                Some(p) => c.web.kept_port = Some(p),
                None => wrong.push(format!("web.kept_port: {v} is not a port from 1 to 65535")),
            }
        }
        if let Some(command) = w.lint.command {
            c.lint.command = Some(command.trim().to_string());
        }
        if let Some(seen) = w.demo.seen {
            c.demo.seen = seen;
        }
        Ok((c, wrong))
    }

    /// The settings in the file at `path`, if there is one; and what was
    /// wrong with it, each said with where.
    pub fn load(path: &Path) -> (Config, Vec<String>) {
        Config::default().load_over(path)
    }

    /// These settings with those of the file at `path` over them, if there
    /// is one; and what was wrong with it, each said with where. A file
    /// that cannot be read leaves them as they are.
    pub fn load_over(&self, path: &Path) -> (Config, Vec<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (self.clone(), vec![]),
            Err(e) => return (self.clone(), vec![format!("{}: {e}", path.display())]),
        };
        match self.over(&text) {
            Ok((c, wrong)) => (c, wrong.into_iter().map(|w| format!("{}: {w}", path.display())).collect()),
            Err(e) => (self.clone(), vec![format!("{}: {e}; none of it is used", path.display())]),
        }
    }

    /// The editor: the setting's, else $VISUAL's, $EDITOR's, or vi.
    pub fn editor(&self) -> String {
        self.editor
            .clone()
            .or_else(|| std::env::var("VISUAL").ok().filter(|e| !e.trim().is_empty()))
            .or_else(|| std::env::var("EDITOR").ok().filter(|e| !e.trim().is_empty()))
            .unwrap_or_else(|| "vi".into())
    }

    /// The shell command that edits the file `"$1"`, at `line` if there is
    /// one: the editor, with `{file}` and `{line}` put where it says; else
    /// told the line as it takes it, when it is an editor known to take
    /// one.
    pub fn editing(&self, line: Option<usize>) -> String {
        let editor = self.editor();
        let at = line.unwrap_or(1).max(1);
        if editor.contains("{file}") || editor.contains("{line}") {
            let cmd = editor.replace("{line}", &at.to_string());
            return if cmd.contains("{file}") {
                cmd.replace("{file}", "\"$1\"")
            } else {
                format!("{cmd} \"$1\"")
            };
        }
        let Some(line) = line else {
            return format!("{editor} \"$1\"");
        };
        let program = editor
            .split_whitespace()
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("");
        match program {
            "vi" | "vim" | "nvim" | "view" | "gvim" | "mvim" | "nano" | "pico" | "emacs" | "emacsclient" | "kak"
            | "joe" | "jed" | "mg" | "ne" => format!("{editor} +{line} \"$1\""),
            "hx" | "helix" | "subl" | "sublime_text" | "zed" | "zeditor" => format!("{editor} \"$1\":{line}"),
            "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => format!("{editor} --goto \"$1\":{line}"),
            _ => format!("{editor} \"$1\""),
        }
    }

    /// The program run as claude: `$STACKED_CLAUDE`'s, else the setting's,
    /// else `claude`.
    pub fn agent_command(&self) -> String {
        std::env::var("STACKED_CLAUDE")
            .ok()
            .filter(|c| !c.is_empty())
            .or_else(|| self.agent.command.clone())
            .unwrap_or_else(|| "claude".into())
    }

    /// The arguments added to every run of claude.
    pub fn agent_args(&self) -> Vec<String> {
        let mut out = self.agent.args.clone();
        if let Some(m) = &self.agent.model {
            out.extend(["--model".to_string(), m.clone()]);
        }
        out
    }

    /// Every setting as it is, in the file's form, each said: what `stacked
    /// config` prints, to start a file from.
    pub fn render(&self) -> String {
        let q = |s: &str| format!("{s:?}");
        let opt = |name: &str, v: &Option<String>, unset: &str| match v {
            Some(v) => format!("{name} = {}\n", q(v)),
            None => format!("# {name} = ...   ({unset})\n"),
        };
        let path = |p: &Path| q(&p.to_string_lossy());
        let list = |v: &[String]| format!("[{}]", v.iter().map(|s| q(s)).collect::<Vec<_>>().join(", "));
        let mut out = String::new();
        out.push_str("# stacked's settings. Each one is optional: what is not set is as below.\n\n");
        out.push_str("# The database opened when none is named, nor $STACKED_FILE.\n");
        out.push_str(&match &self.file {
            Some(f) => format!("file = {}\n", path(f)),
            None => "# file = \"stacked.jsonl\"   (in the directory it is run in)\n".into(),
        });
        out.push_str(
            "\n# The editor, for comments, patches, the stack as a list and the code. {file} and\n\
             # {line} go where it says; without them, the file goes last, after the line as the\n\
             # editor takes it: +LINE for vi, vim, nvim, nano, emacs, kak; FILE:LINE for hx,\n\
             # zed, subl; --goto FILE:LINE for code.\n",
        );
        out.push_str(&opt("editor", &self.editor, "$VISUAL, else $EDITOR, else vi"));
        out.push_str("\n[agent]\n# The program asks run as claude: it is given claude's arguments.\n");
        out.push_str(&opt("command", &self.agent.command, "$STACKED_CLAUDE, else claude"));
        out.push_str("# Arguments added to every run.\n");
        out.push_str(&format!("args = {}\n", list(&self.agent.args)));
        out.push_str(&opt("model", &self.agent.model, "claude's own"));
        out.push_str(
            "# Tools every ask may use besides its own, build and test asks too, as claude names\n\
             # them for --allowedTools: an MCP server's (\"mcp__bugzilla__*\"), one of its tools\n\
             # (\"mcp__bugzilla__get_bug\"), a command (\"Bash(jq *)\"). An answer may use them\n\
             # too: give only tools that read.\n",
        );
        out.push_str(&format!("tools = {}\n", list(&self.agent.tools)));
        out.push_str(
            "# Tools asks that change the stack may use, besides git's history commands and the\n\
             # commands that read, which stacked gives them. Nothing they are given reaches the\n\
             # network, so that a change cannot touch a remote: only what is added here could.\n",
        );
        out.push_str(&format!("change_tools = {}\n", list(&self.agent.change_tools)));
        out.push_str(
            "# Tools answer-only asks may use, besides what reads the code and the stack, which\n\
             # stacked gives them: unless this is set, curl and jq, to read the web.\n",
        );
        out.push_str(&format!("answer_tools = {}\n", list(&self.agent.answer_tools)));
        #[cfg(feature = "try-push")]
        {
            out.push_str(
                "# Tools the asks that select a try push's tasks may use, besides what reads the code\n\
                 # and the stack, which stacked gives them as it gives answers: unless this is set,\n\
                 # curl and jq, to read the web.\n",
            );
            out.push_str(&format!("try_tools = {}\n", list(&self.agent.try_tools)));
        }
        out.push_str(
            "# What the reviews of the agent's work allowed from now on, of the tools it could not\n\
             # use, for each kind of ask: added to its tools above, never in their place. A\n\
             # review writes them in its checkout's own .stacked/local.toml, never here, and\n\
             # `stacked setup agent` lists them, to take out those you no longer allow.\n",
        );
        out.push_str(&format!("change_accepted = {}\n", list(&self.agent.change_accepted)));
        out.push_str(&format!("answer_accepted = {}\n", list(&self.agent.answer_accepted)));
        out.push_str(&format!("test_accepted = {}\n", list(&self.agent.test_accepted)));
        #[cfg(feature = "try-push")]
        out.push_str(&format!("try_accepted = {}\n", list(&self.agent.try_accepted)));
        out.push_str(
            "# Every ask runs in a worktree of its own, never in your checkout. A change works in\n\
             # a worktree of the stack's own, .stacked/NAME/agent: \"ondemand\", made the first time\n\
             # an ask needs it and reused while it holds nothing unfinished; \"precreate\", the\n\
             # same, made when the editor opens the stack and removed when it exits.\n",
        );
        out.push_str(&format!("worktree = {}\n", q(self.agent.worktree.name())));
        out.push_str(
            "# An answer works in a worktree of the stack's own too, .stacked/NAME/answer-only-agent:\n\
             # \"ondemand\" or \"precreate\", as for a change.\n",
        );
        out.push_str(&format!("answer_only_worktree = {}\n", q(self.agent.answer_only_worktree.name())));
        out.push_str("# Where the agents' worktrees go.\n");
        out.push_str(&match &self.agent.worktrees {
            Some(w) => format!("worktrees = {}\n", path(w)),
            None => "# worktrees = ...   (.stacked/ at the top of the stack's repository)\n".into(),
        });
        out.push_str(
            "# Whether a change or an answer starts while the change before it waits for your\n\
             # review, from the stack that change makes, as if you accept it: rejected, sent\n\
             # back or edited by hand, what went on from it is dropped and starts again.\n",
        );
        out.push_str(&format!("chain = {}\n", self.agent.chain));
        out.push_str(
            "# Whether the agent gives its answer as structured output, checked against what the\n\
             # ask asks for (claude's --json-schema): off for an agent that does not take it,\n\
             # its answer then read from what it writes.\n",
        );
        out.push_str(&format!("schema = {}\n", self.agent.schema));
        out.push_str(
            "# Whether the dialog that asks the agent checks, as it opens, that the network\n\
             # reaches the host claude talks to (the proxy of $HTTPS_PROXY, else the host of\n\
             # $ANTHROPIC_BASE_URL, else api.anthropic.com), and grays Ask Now while it does\n\
             # not: false turns the check off, where it is wrong though claude works.\n",
        );
        out.push_str(&format!("reach_check = {}\n", self.agent.reach_check));
        // Phabricator's, built with it (the moz-phab feature).
        if cfg!(feature = "moz-phab") {
            out.push_str("\n[reviews]\n# Where `moz-phab pull` reads reviews from, and `moz-phab import` revisions: \"phabricator\", or \"none\".\n");
            out.push_str(&format!(
                "source = {}\n",
                q(match self.reviews.source {
                    Source::Phabricator => "phabricator",
                    Source::None => "none",
                })
            ));
            out.push_str("# The Phabricator a trailer's bare D123 is on, and whose token is used.\n");
            out.push_str(&opt("url", &self.reviews.url, "the host each trailer's link names"));
            out.push_str("# Where the tokens are, as moz-phab and arc keep them.\n");
            out.push_str(&format!("arcrc = {}\n", path(&self.reviews.arcrc)));
            out.push_str("# A token for url, rather than the file's.\n");
            out.push_str(&match &self.reviews.token {
                Some(_) => "token = \"...\"   # set\n".to_string(),
                None => "# token = \"api-...\"\n".into(),
            });
            out.push_str("# The trailer naming a patch's revision.\n");
            out.push_str(&format!("trailer = {}\n", q(&self.reviews.trailer)));
            out.push_str("# The Bugzilla that a bug a commit message names (Bug 123) is opened on, in your browser.\n");
            out.push_str(&format!("bugzilla = {}\n", q(&self.reviews.bugzilla)));
        }
        let v = &self.view;
        out.push_str(
            "\n[view]\n# How code is drawn at the start: \"unified\", \"side-by-side\", \"compact\" or\n\
             # \"unified-split\", the patch unified in each half of the middle (1 picks one).\n",
        );
        out.push_str(&format!("layout = {}\n", q(v.layout.name())));
        out.push_str(
            "# How every interdiff is drawn at the start: \"interdiff\", a sign for which version\n\
             # has a line before that version's own sign; \"columns\", a column per version, what\n\
             # each one's patch does with the line; or \"side-by-side\", the old version on the left\n\
             # and the new one on the right. Or the change from the old version to the new one as\n\
             # a patch of its own: \"change-unified\", \"change-side-by-side\" or \"change-compact\"\n\
             # (1 picks one there).\n",
        );
        out.push_str(&format!("interdiff = {}\n", q(v.interdiff.name())));
        out.push_str(
            "# The lines every interdiff is drawn with at the start, in every view: \"every\" line, or\n\
             # only what the patches \"changed\": no line both versions have alike, no context line,\n\
             # no index line, a hunk's header once though a change of base moved its numbers, and\n\
             # in the change, no line the change of base made (1 picks one there).\n",
        );
        out.push_str(&format!("interdiff_lines = {}\n", q(v.interdiff_lines.name())));
        out.push_str("# Reading rather than commenting, at the start (2 switches).\n");
        out.push_str(&format!("reading = {}\n", v.reading));
        out.push_str(
            "# Whether changed lines that are empty or white space alone need review, in the\n\
             # reading mode: when not, they need no mark, and no count of lines to read holds them.\n",
        );
        out.push_str(&format!("whitespace_needs_review = {}\n", v.whitespace_needs_review));
        out.push_str(
            "# The patches on the left, at the start: \"expanded\" (their titles whole), \"files\" (the\n\
             # files of the patch picked listed under it, to jump to) or \"hidden\". The comments on\n\
             # the right: \"expanded\", \"compact\" (a line for each) or \"hidden\". The arrow toward a\n\
             # pane's edge cycles them.\n",
        );
        out.push_str(&format!("patches = {}\ncomments = {}\n", q(v.patches.name()), q(v.comments.name())));
        out.push_str("# The lines of claude's stream under a comment it works on.\n");
        out.push_str(&format!("stream_lines = {}\n", v.stream_lines));
        out.push_str("# The columns the middle needs for two sides, or for two halves; and a pane, for its\n# interdiffs side by side, or the change from one version to another.\n");
        out.push_str(&format!("split_width = {}\n", v.split_width));
        out.push_str(
            "# The columns a commit message's lines are wrapped at, when a menu offers to wrap\n\
             # its long lines: its paragraphs of prose, not its title, code, quoted lines or trailers.\n",
        );
        out.push_str(&format!("message_width = {}\n", v.message_width));
        out.push_str(
            "\n[colors]\n# What paints each part of the screen, as git writes a colour: a foreground,\n\
             # then a background, each a name (black, red, green, yellow, blue, magenta,\n\
             # cyan, white, any of these after bright, default, normal), a number from 0 to\n\
             # 255 or #rrggbb; and any of bold, dim, italic, ul, blink, reverse, strike.\n",
        );
        for p in Paint::ALL {
            out.push_str(&format!("# {}\n{} = {}\n", p.what(), p.name(), q(self.colors.spec(p))));
        }
        out.push_str(
            "\n[keys]\n# The keys for each thing a key does, outside the comment editor (which reads\n\
             # what is typed): up, down, left, right, pageup, pagedown, home, end, enter,\n\
             # space, esc, tab, shift-tab, backspace, delete, f1..f24, a character, any\n\
             # after ctrl- or alt-. Nothing else is a key. Each is read in its context,\n\
             # below: two things of one context on one key clash, and the first does it; of\n\
             # two, each does its own.\n",
        );
        for c in Context::ALL {
            out.push_str(&format!("# {}: {}.\n", c.name(), c.said()));
            for k in Key::ALL.into_iter().filter(|k| k.context() == c) {
                out.push_str(&format!("{} = {}\n", k.name(), list(&self.keys.names(k))));
            }
        }
        // `stacked web`'s, built with it (the web feature).
        if cfg!(feature = "web") {
            out.push_str(
                "\n[web]\n# The port of 127.0.0.1 `stacked web` listens on, unless --port N names one for that\n\
                 # run: \"random\", any free one each time; \"fixed-random\", any free one the first time,\n\
                 # which stacked then writes below as kept_port and listens on each time after; or a\n\
                 # port from 1 to 65535. A port in use is said, and `stacked web` does not start. Read\n\
                 # from the settings' file alone, never from a checkout's own .stacked/local.toml.\n",
            );
            out.push_str(&format!("port = {}\n", self.web.port.literal()));
            out.push_str(
                "# The port \"fixed-random\" took the first time, which stacked wrote here: listened on\n\
                 # while port is \"fixed-random\", and passed over, but kept, while it is another. Taken\n\
                 # out, the next launch with \"fixed-random\" takes any free port again, and writes it.\n",
            );
            out.push_str(&match self.web.kept_port {
                Some(p) => format!("kept_port = {p}\n"),
                None => "# kept_port = ...   (none taken yet)\n".into(),
            });
        }
        // A checkout's own setting alone: said, never written as one, so
        // that this, kept as the settings' file, sets no linter there.
        out.push_str(
            "\n[lint]\n# The linter of a checkout, which the editor's `Lint the stack` runs: a program and\n\
             # its arguments, the files of each patch after them. A checkout's own setting alone,\n\
             # read only from its .stacked/local.toml, which `stacked setup lint COMMAND…` writes;\n\
             # `stacked setup lint --off` writes command = \"\", lint turned off. Unset, a checkout\n\
             # whose top has ./mach lints with ./mach lint --fix.\n",
        );
        out.push_str(&match &self.lint.command {
            Some(c) if c.is_empty() => "# command = \"\"   (this checkout's own: lint is off)\n".to_string(),
            Some(c) => format!("# command = {}   (this checkout's own, as `stacked setup lint` wrote it)\n", q(c)),
            None => "# command = ...   (none set: ./mach lint --fix where ./mach is at the checkout's top)\n".into(),
        });
        out.push_str("\n[demo]\n# The tour, `stacked demo`, was opened before: it starts on the list of its sections.\n");
        out.push_str(&format!("seen = {}\n", self.demo.seen));
        out
    }
}

/// Writes in the settings' file at `path` that the tour was opened: `seen =
/// true` under `[demo]`, the rest of the file as it was. A file that is not
/// there is made, and its folder; one stacked cannot read is left as it is,
/// and said.
/// Whether it wrote: not when the file said so already.
pub fn remember_demo(path: &Path) -> Result<bool, String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(at(&e)),
    };
    let (c, _) = Config::parse(&text).map_err(|e| at(&e))?;
    if c.demo.seen {
        return Ok(false);
    }
    let new = with_setting(&text, "demo.seen", Some("true"));
    match Config::parse(&new) {
        Ok((c, _)) if c.demo.seen => {}
        _ => return Err(at(&"[demo] seen = true cannot be written in it as it is")),
    }
    write_settings(path, &new)?;
    Ok(true)
}

/// `text` written as the settings' file at `path`, its folder made if it
/// is not there (stacked's own, `~/.stacked`, the first time).
fn write_settings(path: &Path, text: &str) -> Result<(), String> {
    let at = |e: std::io::Error, p: &Path| format!("{}: {e}", p.display());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| at(e, dir))?;
    }
    std::fs::write(path, text).map_err(|e| at(e, path))
}

/// What a setting holds, as the file writes it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Flag,
    Count,
    List,
    /// A colour, as git writes one.
    Color,
    /// The patches pane's state, or true or false.
    Patches,
    /// The comments pane's state, or true or false.
    Pane,
    /// When an ask's worktree is made: ondemand or precreate.
    Workspace,
    /// The port `stacked web` listens on: random, fixed-random, or a port.
    Port,
    /// A port, from 1 to 65535.
    PortNumber,
}

/// Every setting there is, as the file names it, and what it holds; the
/// keys', `keys.NAME`, are lists.
const SETTINGS: &[(&str, Kind)] = &[
    ("file", Kind::Text),
    ("editor", Kind::Text),
    ("agent.command", Kind::Text),
    ("agent.args", Kind::List),
    ("agent.model", Kind::Text),
    ("agent.tools", Kind::List),
    ("agent.change_tools", Kind::List),
    ("agent.answer_tools", Kind::List),
    #[cfg(feature = "try-push")]
    ("agent.try_tools", Kind::List),
    ("agent.change_accepted", Kind::List),
    ("agent.answer_accepted", Kind::List),
    ("agent.test_accepted", Kind::List),
    #[cfg(feature = "try-push")]
    ("agent.try_accepted", Kind::List),
    ("agent.worktree", Kind::Workspace),
    ("agent.answer_only_worktree", Kind::Workspace),
    ("agent.worktrees", Kind::Text),
    ("agent.chain", Kind::Flag),
    ("agent.schema", Kind::Flag),
    ("agent.reach_check", Kind::Flag),
    ("reviews.source", Kind::Text),
    ("reviews.url", Kind::Text),
    ("reviews.arcrc", Kind::Text),
    ("reviews.token", Kind::Text),
    ("reviews.trailer", Kind::Text),
    ("reviews.bugzilla", Kind::Text),
    ("view.layout", Kind::Text),
    ("view.interdiff", Kind::Text),
    ("view.interdiff_lines", Kind::Text),
    ("view.reading", Kind::Flag),
    ("view.whitespace_needs_review", Kind::Flag),
    ("view.patches", Kind::Patches),
    ("view.comments", Kind::Pane),
    ("view.stream_lines", Kind::Count),
    ("view.split_width", Kind::Count),
    ("view.message_width", Kind::Count),
    ("web.port", Kind::Port),
    ("web.kept_port", Kind::PortNumber),
    ("lint.command", Kind::Text),
    ("demo.seen", Kind::Flag),
];

fn kind_of(key: &str) -> Result<Kind, String> {
    if let Some((_, k)) = SETTINGS.iter().find(|(name, _)| *name == key) {
        return Ok(*k);
    }
    if let Some(why) = gone_key(key) {
        return Err(why);
    }
    if key.strip_prefix("colors.").and_then(Paint::named).is_some() {
        return Ok(Kind::Color);
    }
    match key.strip_prefix("keys.") {
        Some(k) if Key::ALL.iter().any(|key| key.name() == k) => Ok(Kind::List),
        _ => Err(format!("{key} is not a setting: `stacked config` prints them all")),
    }
}

/// Why `key` is no longer a setting, if it is a key that is gone
/// (`keys::GONE`): a file that still sets it is told so, and taking it out
/// of the file is all `stacked config` does with it.
fn gone_key(key: &str) -> Option<String> {
    if key == "agent.setup" {
        return Some(SETUP_GONE.into());
    }
    let name = key.strip_prefix("keys.")?;
    crate::keys::GONE.iter().find(|(n, _)| *n == name).map(|(n, why)| crate::keys::gone(n, why))
}

/// Why `agent.setup` is no longer a setting: a file that still sets it is
/// read all the same, the setting passed over, as it says nothing now.
const SETUP_GONE: &str = "agent.setup is no longer a setting: no ask waits for the setup of the agent's tools any more";

/// The value of `key` as `values` say it, in the file's form.
fn literal(key: &str, kind: Kind, values: &[String]) -> Result<String, String> {
    let one = || match values {
        [v] => Ok(v.as_str()),
        _ => Err(format!("{key} takes one value, not {}", values.len())),
    };
    Ok(match kind {
        Kind::Text => toml::Value::String(one()?.to_string()).to_string(),
        Kind::Flag => match one()? {
            v @ ("true" | "false") => v.to_string(),
            v => return Err(format!("{key} is true or false, not {v:?}")),
        },
        Kind::Count => match one()?.parse::<usize>() {
            Ok(n) => n.to_string(),
            Err(_) => return Err(format!("{key} is a number, not {:?}", values[0])),
        },
        Kind::List => toml::Value::Array(values.iter().map(|v| toml::Value::String(v.clone())).collect()).to_string(),
        // `compact`, read as the files from a file written before them, is
        // not written.
        Kind::Patches => match one()? {
            v @ ("true" | "false") => v.to_string(),
            v if PatchesPane::parse(v).is_some_and(|p| p.name() == v) => toml::Value::String(v.to_string()).to_string(),
            v => return Err(format!("{key} is expanded, files or hidden, not {v:?}")),
        },
        Kind::Pane => match one()? {
            v @ ("true" | "false") => v.to_string(),
            v if Pane::parse(v).is_some() => toml::Value::String(v.to_string()).to_string(),
            v => return Err(format!("{key} is expanded, compact or hidden, not {v:?}")),
        },
        // Your checkout, which no ask works in now, refused, as a stacked
        // of before took it.
        Kind::Workspace => match one()? {
            v if Workspace::parse(v).is_some() => toml::Value::String(v.to_string()).to_string(),
            PWD => return Err(format!("{}; nothing written", no_pwd(key))),
            v => return Err(format!("{key} is ondemand or precreate, not {v:?}")),
        },
        Kind::Port => match Port::parse(one()?) {
            Some(p) => p.literal(),
            None => return Err(format!("{key} is {NOT_A_PORT}, not {:?}", values[0])),
        },
        Kind::PortNumber => match port_number(one()?) {
            Some(p) => p.to_string(),
            None => return Err(format!("{key} is a port from 1 to 65535, not {:?}", values[0])),
        },
        Kind::Color => {
            let v = one()?;
            parse_color(v).map_err(|e| format!("{key}: {e}"))?;
            toml::Value::String(v.to_string()).to_string()
        }
    })
}

/// The value `c` has for `key`, as the file would write it: one string per
/// value, none for a setting not set.
pub fn value_of(c: &Config, key: &str) -> Result<Vec<String>, String> {
    kind_of(key)?;
    let opt = |v: &Option<String>| v.iter().cloned().collect();
    let path = |p: &Option<PathBuf>| p.iter().map(|p| p.display().to_string()).collect();
    Ok(match key {
        "file" => path(&c.file),
        "editor" => opt(&c.editor),
        "agent.command" => opt(&c.agent.command),
        "agent.args" => c.agent.args.clone(),
        "agent.model" => opt(&c.agent.model),
        "agent.tools" => c.agent.tools.clone(),
        "agent.change_tools" => c.agent.change_tools.clone(),
        "agent.answer_tools" => c.agent.answer_tools.clone(),
        #[cfg(feature = "try-push")]
        "agent.try_tools" => c.agent.try_tools.clone(),
        "agent.change_accepted" => c.agent.change_accepted.clone(),
        "agent.answer_accepted" => c.agent.answer_accepted.clone(),
        "agent.test_accepted" => c.agent.test_accepted.clone(),
        #[cfg(feature = "try-push")]
        "agent.try_accepted" => c.agent.try_accepted.clone(),
        "agent.worktree" => vec![c.agent.worktree.name().into()],
        "agent.answer_only_worktree" => vec![c.agent.answer_only_worktree.name().into()],
        "agent.worktrees" => path(&c.agent.worktrees),
        "agent.chain" => vec![c.agent.chain.to_string()],
        "agent.schema" => vec![c.agent.schema.to_string()],
        "agent.reach_check" => vec![c.agent.reach_check.to_string()],
        "reviews.source" => vec![match c.reviews.source {
            Source::Phabricator => "phabricator".into(),
            Source::None => "none".into(),
        }],
        "reviews.url" => opt(&c.reviews.url),
        "reviews.arcrc" => vec![c.reviews.arcrc.display().to_string()],
        "reviews.token" => opt(&c.reviews.token),
        "reviews.trailer" => vec![c.reviews.trailer.clone()],
        "reviews.bugzilla" => vec![c.reviews.bugzilla.clone()],
        "view.layout" => vec![c.view.layout.name().into()],
        "view.interdiff" => vec![c.view.interdiff.name().into()],
        "view.interdiff_lines" => vec![c.view.interdiff_lines.name().into()],
        "view.reading" => vec![c.view.reading.to_string()],
        "view.whitespace_needs_review" => vec![c.view.whitespace_needs_review.to_string()],
        "view.patches" => vec![c.view.patches.name().into()],
        "view.comments" => vec![c.view.comments.name().into()],
        "view.stream_lines" => vec![c.view.stream_lines.to_string()],
        "view.split_width" => vec![c.view.split_width.to_string()],
        "view.message_width" => vec![c.view.message_width.to_string()],
        "web.port" => vec![c.web.port.name()],
        "web.kept_port" => c.web.kept_port.iter().map(|p| p.to_string()).collect(),
        "lint.command" => opt(&c.lint.command),
        "demo.seen" => vec![c.demo.seen.to_string()],
        k if k.starts_with("colors.") => match Paint::named(&k["colors.".len()..]) {
            Some(p) => vec![c.colors.spec(p).to_string()],
            None => vec![],
        },
        _ => {
            let name = key.trim_start_matches("keys.");
            let k = Key::ALL.iter().find(|k| k.name() == name).copied().unwrap_or(Key::Quit);
            c.keys.names(k)
        }
    })
}

/// Writes `key` = `values` in the settings' file at `path`, every other
/// line as it was; a file there is not, made, and its folder. Refused,
/// with why, for a setting there is not, a value of the wrong kind, or one
/// stacked would not take; a file stacked cannot read is left as it is.
pub fn set(path: &Path, key: &str, values: &[String]) -> Result<(), String> {
    set_all(path, &[(key, values.to_vec())])
}

/// Writes each of `settings`, a key and its values, in the settings' file
/// at `path`, as `set` writes one: all in one go, or none of them.
pub fn set_all(path: &Path, settings: &[(&str, Vec<String>)]) -> Result<(), String> {
    if let Some((key, _)) = settings.iter().find(|(key, _)| local_only(key)) {
        return Err(only_local(key));
    }
    let lits = literals(settings)?;
    edit_settings(path, |text| with_settings(text, &lits))
}

/// Whether `key` is a checkout's own setting alone, never read from the
/// settings' file: the linter's.
pub fn local_only(key: &str) -> bool {
    key.starts_with("lint.")
}

/// Why the settings' file does not take `key`, a checkout's own setting
/// alone.
fn only_local(key: &str) -> String {
    format!("{key} is a checkout's own setting, read only from its .stacked/local.toml: `stacked setup lint` writes it there; nothing written")
}

/// Whether `key` is a setting of the settings' file alone, never read from
/// a checkout's own: `stacked web`'s, as `"fixed-random"` keeps its port
/// there, for every checkout.
pub fn global_only(key: &str) -> bool {
    key.starts_with("web.")
}

/// Why a checkout's own settings do not take `key`, a setting of the
/// settings' file alone.
fn only_global(key: &str) -> String {
    format!("{key} is read only from the settings' file, never from a checkout's own .stacked/local.toml")
}

/// The settings' file's settings, and what is wrong with it: a checkout's
/// own setting it sets passed over, and said.
fn global() -> (Config, Vec<String>) {
    let p = path();
    let (mut c, mut wrong) = Config::load(&p);
    if c.lint.command.take().is_some() {
        wrong.push(format!(
            "{}: lint.command is a checkout's own setting, read only from its .stacked/local.toml (`stacked setup lint` writes it there): here it is passed over",
            p.display()
        ));
    }
    (c, wrong)
}

/// Each of `settings`, a key and its values, in the file's form: refused,
/// with why, for one there is not, or a value of the wrong kind.
fn literals<'a>(settings: &[(&'a str, Vec<String>)]) -> Result<Vec<(&'a str, String)>, String> {
    settings
        .iter()
        .map(|(key, values)| match gone_key(key) {
            Some(why) => Err(format!("{why}; nothing written")),
            None => Ok((*key, literal(key, kind_of(key)?, values)?)),
        })
        .collect()
}

/// `text`, a settings' file, with each of `lits` (a key, and its value in
/// the file's form) written as `with_setting` writes one.
fn with_settings(text: &str, lits: &[(&str, String)]) -> String {
    lits.iter().fold(text.to_string(), |text, (key, lit)| with_setting(&text, key, Some(lit)))
}

/// Takes `key` out of the settings' file at `path`: it is then as it would
/// be with no file.
pub fn unset(path: &Path, key: &str) -> Result<(), String> {
    if gone_key(key).is_none() {
        kind_of(key)?;
    }
    edit_settings(path, |text| with_setting(text, key, None))
}

/// The settings' file at `path` made what `edit` makes of it, once what it
/// makes is seen to read back with nothing more wrong than before.
fn edit_settings(path: &Path, edit: impl FnOnce(&str) -> String) -> Result<(), String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(at(&e)),
    };
    let (_, was) = Config::parse(&text).map_err(|e| at(&format!("{e}; nothing written in it")))?;
    let new = edit(&text);
    if new == text {
        return Ok(());
    }
    let (_, wrong) = Config::parse(&new).map_err(|e| at(&format!("{e}; nothing written")))?;
    let more: Vec<&String> = wrong.iter().filter(|w| !was.contains(w)).collect();
    if !more.is_empty() {
        return Err(more.iter().map(|w| w.to_string()).collect::<Vec<_>>().join("; ") + "; nothing written");
    }
    write_settings(path, &new)
}

/// `text`, a settings' file, with `key` (`table.name`, or `name` at the
/// top) given the value `value`, in the file's form, or taken out if None;
/// every other line as it was. A line of it already there is replaced (all
/// of it, a list over several lines too); else a top-level one goes before
/// the first table, and one of a table after its header, the table made at
/// the end if there is none.
fn with_setting(text: &str, key: &str, value: Option<&str>) -> String {
    let (table, name) = match key.split_once('.') {
        Some((t, n)) => (Some(t), n),
        None => (None, key),
    };
    let lines: Vec<&str> = text.lines().collect();
    let header = |l: &str| l.trim_start().starts_with('[');
    let is = |l: &str, t: &str| l.split('#').next().unwrap_or("").trim() == format!("[{t}]");
    // Where the table's lines are: after its header (or the top), to the
    // next header; and whether it is there at all.
    let first_header = lines.iter().position(|l| header(l)).unwrap_or(lines.len());
    let (start, end, there) = match table {
        None => (0, first_header, true),
        Some(t) => match lines.iter().position(|l| is(l, t)) {
            Some(at) => (at + 1, lines[at + 1..].iter().position(|l| header(l)).map_or(lines.len(), |e| at + 1 + e), true),
            None => (lines.len(), lines.len(), false),
        },
    };
    let assigns = |l: &str| l.trim_start().strip_prefix(name).is_some_and(|rest| rest.trim_start().starts_with('='));
    let found = (start..end).find(|&i| assigns(lines[i]));
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let line = value.map(|v| format!("{name} = {v}"));
    match (found, line) {
        (Some(i), line) => {
            let span = value_lines(&lines[i..end]);
            out.splice(i..i + span, line);
        }
        (None, None) => {}
        // A table's last, after its last line; at the top, after the last
        // setting there, before any comment heading the first table.
        (None, Some(line)) if there => {
            let at = match table {
                Some(_) => (start..end).rev().find(|&i| !lines[i].trim().is_empty()).map_or(start, |i| i + 1),
                None => (0..end)
                    .rev()
                    .find(|&i| {
                        let t = lines[i].trim();
                        !t.is_empty() && !t.starts_with('#')
                    })
                    .map_or(0, |i| i + 1),
            };
            out.insert(at, line);
        }
        (None, Some(line)) => {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("[{}]", table.unwrap_or_default()));
            out.push(line);
        }
    }
    if out.is_empty() {
        return String::new();
    }
    out.join("\n") + "\n"
}

/// How many of `lines` the setting on the first holds: one, or a list or a
/// string in triple quotes that goes on to later lines.
fn value_lines(lines: &[&str]) -> usize {
    let mut depth = 0i32;
    let mut triple = false;
    for (k, line) in lines.iter().enumerate() {
        let mut quote: Option<char> = None;
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if triple {
                if line[line.char_indices().nth(i).map_or(0, |(b, _)| b)..].starts_with("\"\"\"") {
                    triple = false;
                    i += 3;
                    continue;
                }
            } else if let Some(q) = quote {
                if c == '\\' && q == '"' {
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            } else if line[line.char_indices().nth(i).map_or(0, |(b, _)| b)..].starts_with("\"\"\"") {
                triple = true;
                i += 3;
                continue;
            } else {
                match c {
                    '"' | '\'' => quote = Some(c),
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    '#' => break,
                    _ => {}
                }
            }
            i += 1;
        }
        if depth <= 0 && !triple {
            return k + 1;
        }
    }
    lines.len().max(1)
}

/// The settings in use, read once: the settings' file's (or `init`'s), with
/// the local file of the checkout stacked runs in over them; and what was
/// wrong with either file.
struct Loaded {
    config: Config,
    wrong: Vec<String>,
    /// `init`'s: no file was read for them.
    given: bool,
}

static LOADED: OnceLock<Loaded> = OnceLock::new();

/// Where the settings are read from, as the binary worked it out as it
/// started (`home`): `config.toml` in stacked's folder, or the file
/// `$STACKED_CONFIG` names. The null device where nothing worked it out, as
/// in the tests' own processes: no file of the user's is read.
pub fn path() -> PathBuf {
    crate::home::get().config.clone()
}

fn loaded() -> &'static Loaded {
    LOADED.get_or_init(|| {
        let (global, mut wrong) = global();
        let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (config, more) = with_local(&global, &here);
        wrong.extend(more);
        Loaded { config, wrong, given: false }
    })
}

/// The settings in use: the settings' file's, with the local file of the
/// checkout stacked runs in over them.
pub fn get() -> &'static Config {
    &loaded().config
}

/// What was wrong with the settings' file, or the local file, if anything.
pub fn warnings() -> &'static [String] {
    &loaded().wrong
}

/// Uses `c` rather than any file, unless the settings are read already:
/// for the tests, which share one process. A stack's checkout's own local
/// file is still read over it, by `of`.
pub fn init(c: Config) {
    let _ = LOADED.set(Loaded { config: c, wrong: vec![], given: true });
}

/// The settings of the checkout `dir` is in, as the files say them now:
/// the settings' file's (or `init`'s), with that checkout's local file
/// over them; and what was wrong with either. What a stack's agent does is
/// read so, from the stack's checkout, whichever directory stacked runs in:
/// a change to either file counts from the next ask.
pub fn of(dir: &Path) -> (Config, Vec<String>) {
    let l = loaded();
    let (global, mut wrong) = if l.given { (l.config.clone(), vec![]) } else { global() };
    let (config, more) = with_local(&global, dir);
    wrong.extend(more);
    (config, wrong)
}

// -- a checkout's own settings ------------------------------------------- //

/// Where the checkout `dir` is in keeps its own settings, which override
/// the settings' file there: `.stacked/local.toml` at its top, beside the
/// stacks' worktrees. None outside a checkout.
pub fn local_path(dir: &Path) -> Option<PathBuf> {
    let top = crate::git::toplevel(dir).ok()?;
    Some(Path::new(&top).join(".stacked").join("local.toml"))
}

fn not_in_checkout(dir: &Path) -> String {
    format!(
        "{} is not in a git checkout: a checkout's own settings are in .stacked/local.toml at its top",
        dir.display()
    )
}

/// As `local_path`, outside a checkout said.
pub fn local_file(dir: &Path) -> Result<PathBuf, String> {
    local_path(dir).ok_or_else(|| not_in_checkout(dir))
}

/// Why the local file at `path` is not to be read or written, if it is not:
/// git tracks it, so it came with the repository, as a command it would
/// have stacked run could; or git cannot tell whether it does (`.stacked` a
/// link of the repository's).
fn not_yours(path: &Path) -> Option<String> {
    let top = path.parent().and_then(Path::parent)?;
    match crate::git::git(top, &["ls-files", "--", ".stacked/local.toml"]) {
        Ok(out) if out.trim().is_empty() => None,
        Ok(_) => Some(format!(
            "{}: git tracks it, and a checkout's own settings are yours, not the repository's",
            path.display()
        )),
        Err(e) => Some(format!("{}: git cannot tell whether it tracks it ({})", path.display(), e.0.trim())),
    }
}

/// `base` with the local file of the checkout `dir` is in over it, if there
/// is one and it is yours; and what was wrong with it. What it says of
/// `stacked web`'s settings, the settings' file's alone, is passed over,
/// and said so.
fn with_local(base: &Config, dir: &Path) -> (Config, Vec<String>) {
    match local_path(dir) {
        Some(p) if p.exists() => match not_yours(&p) {
            Some(why) => (base.clone(), vec![format!("{why}; it is not read")]),
            None => {
                let (mut c, mut wrong) = base.load_over(&p);
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                let passed: Vec<String> = keys_in(&text).into_iter().filter(|k| global_only(k) && kind_of(k).is_ok()).collect();
                if !passed.is_empty() {
                    // As the settings' file has them; what is wrong with them
                    // here is not said, as they are not read.
                    c.web = base.web.clone();
                    let at = format!("{}: web.", p.display());
                    wrong.retain(|w| !w.starts_with(&at));
                    wrong.extend(passed.iter().map(|k| format!("{}: {}: here it is passed over", p.display(), only_global(k))));
                }
                (c, wrong)
            }
        },
        _ => (base.clone(), vec![]),
    }
}

/// Whether there is a local file at `path` that is read: yours.
pub fn read_local(path: &Path) -> bool {
    path.exists() && not_yours(path).is_none()
}

/// The local file of the checkout `dir` is in, read: what it says, if there
/// is one, it is yours, and stacked can read it.
fn local_text(dir: &Path) -> Option<(PathBuf, String)> {
    let p = local_path(dir).filter(|p| p.exists() && not_yours(p).is_none())?;
    let text = std::fs::read_to_string(&p).ok().filter(|t| Config::parse(t).is_ok())?;
    Some((p, text))
}

/// Whether the settings' file `text` sets `key`.
fn sets(text: &str, key: &str) -> bool {
    let Ok(t) = toml::from_str::<toml::Table>(text) else {
        return false;
    };
    match key.split_once('.') {
        Some((table, name)) => t.get(table).and_then(|v| v.as_table()).is_some_and(|t| t.contains_key(name)),
        None => t.contains_key(key),
    }
}

/// The settings the settings' file `text` sets, as `stacked config` names
/// them.
fn keys_in(text: &str) -> Vec<String> {
    let Ok(t) = toml::from_str::<toml::Table>(text) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (k, v) in &t {
        match v.as_table() {
            Some(inner) => out.extend(inner.keys().map(|n| format!("{k}.{n}"))),
            None => out.push(k.clone()),
        }
    }
    out
}

/// The settings the local file of the checkout `dir` is in sets, as
/// `stacked config` names them, if it is read: but `stacked web`'s, which
/// are passed over there.
pub fn local_keys(dir: &Path) -> Vec<String> {
    let Some((_, text)) = local_text(dir) else {
        return vec![];
    };
    keys_in(&text).into_iter().filter(|k| !global_only(k)).collect()
}

/// The file whose `key` is the one in use in the checkout `dir` is in: its
/// local file, if it is read and sets it (never for `stacked web`'s, the
/// settings' file's alone); else the settings' file, if that does. None if
/// neither does.
pub fn set_in(dir: &Path, key: &str) -> Option<PathBuf> {
    if !global_only(key)
        && let Some((p, text)) = local_text(dir)
        && sets(&text, key)
    {
        return Some(p);
    }
    let p = path();
    file_sets(&p, key).then_some(p)
}

/// Whether the settings' file at `path` sets `key`, and stacked reads it.
pub fn file_sets(path: &Path, key: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| Config::parse(&text).is_ok() && sets(&text, key))
}

/// What the local file of the checkout `dir` is in gives `key`, as
/// `value_of` says it; None if it does not give it any. Refused for
/// `stacked web`'s settings, the settings' file's alone.
pub fn local_value(dir: &Path, key: &str) -> Result<Option<Vec<String>>, String> {
    kind_of(key)?;
    if global_only(key) {
        return Err(format!("{}: `stacked config {key}` reads it", only_global(key)));
    }
    let p = local_file(dir)?;
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    if !sets(&text, key) {
        return Ok(None);
    }
    let (c, _) = Config::parse(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    value_of(&c, key).map(Some)
}

/// The local file of the checkout `dir` is in, to write: refused outside a
/// checkout, and when it is not yours.
pub fn local_to_write(dir: &Path) -> Result<PathBuf, String> {
    let p = local_file(dir)?;
    match not_yours(&p) {
        Some(why) => Err(format!("{why}; nothing written")),
        None => Ok(p),
    }
}

/// Writes `key` = `values` in the local file of the checkout `dir` is in,
/// as `set` writes the settings' file: `.stacked/` made if it is not
/// there, and kept out of git's sight. Where it wrote.
pub fn set_local(dir: &Path, key: &str, values: &[String]) -> Result<PathBuf, String> {
    set_local_all(dir, &[(key, values.to_vec())])
}

/// Writes each of `settings` in the local file of the checkout `dir` is
/// in, as `set_local` writes one: all in one go, or none of them; none of
/// `stacked web`'s, the settings' file's alone. Where it wrote.
pub fn set_local_all(dir: &Path, settings: &[(&str, Vec<String>)]) -> Result<PathBuf, String> {
    if let Some((key, _)) = settings.iter().find(|(key, _)| global_only(key)) {
        return Err(format!("{}: `stacked config {key} …` writes it there; nothing written", only_global(key)));
    }
    let lits = literals(settings)?;
    let p = local_to_write(dir)?;
    let folder = p.parent().unwrap_or(Path::new("."));
    if let Some(top) = folder.parent() {
        crate::git::exclude(top, "/.stacked/").map_err(|e| e.0)?;
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    edit_settings(&p, |text| with_settings(text, &lits))?;
    Ok(p)
}

/// Takes `key` out of the local file of the checkout `dir` is in: there, it
/// is then as the settings' file has it. Where it was.
pub fn unset_local(dir: &Path, key: &str) -> Result<PathBuf, String> {
    if gone_key(key).is_none() {
        kind_of(key)?;
    }
    let p = local_to_write(dir)?;
    edit_settings(&p, |text| with_setting(text, key, None))?;
    Ok(p)
}

/// `text`, the settings as `render` writes them, with each line of a
/// setting in `local` said to be the local file's.
pub fn mark_local(text: &str, local: &[String]) -> String {
    let mut table = String::new();
    let mut out = String::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            table = name.to_string();
        }
        out.push_str(line);
        if let Some((name, _)) = t.split_once('=')
            && !t.starts_with('#')
        {
            let name = name.trim();
            let key = if table.is_empty() { name.to_string() } else { format!("{table}.{name}") };
            if local.contains(&key) {
                out.push_str("   # .stacked/local.toml");
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_sets_what_it_says_and_says_what_is_wrong() {
        let (c, wrong) = Config::parse(
            r#"
            file = "~/stacks.jsonl"
            editor = "hx {file}:{line}"
            [agent]
            command = "my-claude"
            model = "opus"
            tools = ["mcp__bugzilla__*"]
            [reviews]
            source = "nowhere"
            url = "https://phab.example.com/"
            [view]
            layout = "compact"
            reading = true
            [keys]
            up = ["up", "k"]
            menu = "m"
            "#,
        )
        .unwrap();
        assert_eq!(c.file, Some(home().join("stacks.jsonl")));
        assert_eq!(c.agent_args(), ["--model", "opus"]);
        assert_eq!(c.agent.tools, ["mcp__bugzilla__*"]);
        assert_eq!(c.reviews.url.as_deref(), Some("https://phab.example.com"));
        assert_eq!(c.reviews.source, Source::Phabricator, "what is wrong keeps its default");
        assert_eq!(wrong, ["reviews.source: \"nowhere\" is neither \"phabricator\" nor \"none\""]);
        assert_eq!((c.view.layout, c.view.reading), (Layout::Compact, true));
        assert_eq!(c.keys.label(Key::Menu), "m");
        assert_eq!(c.editing(Some(12)), "hx \"$1\":12");
        // What is not a setting at all: the file is not used.
        let e = Config::parse("colour = \"red\"\n").unwrap_err();
        assert!(e.contains("unknown field `colour`"), "{e}");
    }

    #[test]
    fn the_editor_is_told_the_line_as_it_takes_it() {
        let with = |e: &str| Config {
            editor: Some(e.into()),
            ..Config::default()
        };
        assert_eq!(with("vim").editing(Some(7)), "vim +7 \"$1\"");
        assert_eq!(with("/usr/bin/nvim -u NONE").editing(Some(7)), "/usr/bin/nvim -u NONE +7 \"$1\"");
        assert_eq!(with("hx").editing(Some(7)), "hx \"$1\":7");
        assert_eq!(with("code --wait").editing(Some(7)), "code --wait --goto \"$1\":7");
        assert_eq!(with("ed").editing(Some(7)), "ed \"$1\"", "one not known is given the file alone");
        assert_eq!(with("vim").editing(None), "vim \"$1\"");
        assert_eq!(with("kak +{line}").editing(Some(3)), "kak +3 \"$1\"");
    }

    #[test]
    fn the_tour_is_remembered_in_the_file_the_rest_as_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        // No file: one is made, with that alone.
        assert_eq!(remember_demo(&path), Ok(true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[demo]\nseen = true\n");
        assert_eq!(remember_demo(&path), Ok(false), "said already");
        // Settings of yours, comments among them: [demo] goes at the end.
        let mine = "# mine\neditor = \"hx\"\n\n[view]\nlayout = \"compact\" # as I like it\n";
        std::fs::write(&path, mine).unwrap();
        assert_eq!(remember_demo(&path), Ok(true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{mine}\n[demo]\nseen = true\n"));
        let (c, wrong) = Config::load(&path);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert!(c.demo.seen && c.view.layout == Layout::Compact && c.editor.as_deref() == Some("hx"));
        // [demo] there already, seen = false: that line alone changes.
        std::fs::write(&path, "[demo]\nseen = false # again\n[keys]\nmenu = \"m\"\n").unwrap();
        assert_eq!(remember_demo(&path), Ok(true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[demo]\nseen = true\n[keys]\nmenu = \"m\"\n");
        // A file stacked cannot read is left as it is.
        std::fs::write(&path, "colour = \"red\"\n").unwrap();
        assert!(remember_demo(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "colour = \"red\"\n");
    }

    #[test]
    fn one_setting_is_written_or_taken_out_the_rest_as_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let mine = "# mine\nfile = \"stacks.jsonl\"\n\n# how it looks\n[view]\nlayout = \"compact\" # as I like it\n\n[agent]\nargs = [\n  \"--verbose\",\n]\n";
        std::fs::write(&path, mine).unwrap();
        let read = || std::fs::read_to_string(&path).unwrap();
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (c, _) = Config::load(&path);
        assert_eq!(value_of(&c, "view.layout"), Ok(vec!["compact".to_string()]));
        assert_eq!(value_of(&c, "editor"), Ok(vec![]), "not set");
        // At the top, after the last setting there.
        set(&path, "editor", &vals(&["hx {file}:{line}"])).unwrap();
        assert_eq!(read(), mine.replace("file = \"stacks.jsonl\"\n", "file = \"stacks.jsonl\"\neditor = \"hx {file}:{line}\"\n"));
        // A setting there, replaced: a list over several lines whole.
        set(&path, "agent.args", &vals(&["--model", "opus"])).unwrap();
        assert!(read().ends_with("[agent]\nargs = [\"--model\", \"opus\"]\n"), "{}", read());
        set(&path, "view.layout", &vals(&["side-by-side"])).unwrap();
        assert!(read().contains("[view]\nlayout = \"side-by-side\"\n\n[agent]"), "{}", read());
        // A table not there, made at the end.
        set(&path, "keys.down", &vals(&["down", "j"])).unwrap();
        assert!(read().ends_with("\n\n[keys]\ndown = [\"down\", \"j\"]\n"), "{}", read());
        let (c, wrong) = Config::load(&path);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(value_of(&c, "keys.down"), Ok(vec!["down".to_string(), "j".to_string()]));
        assert_eq!(c.editor.as_deref(), Some("hx {file}:{line}"));
        // Taken out: as without it.
        unset(&path, "view.layout").unwrap();
        assert!(read().contains("[view]\n\n[agent]"), "{}", read());
        assert_eq!(value_of(&Config::load(&path).0, "view.layout"), Ok(vec!["unified".to_string()]));
        // Refused, the file as it was: no such setting, a value of the
        // wrong kind, one stacked would not take.
        let before = read();
        assert!(set(&path, "colour", &vals(&["red"])).unwrap_err().contains("not a setting"));
        assert!(set(&path, "view.reading", &vals(&["maybe"])).unwrap_err().contains("true or false"));
        assert!(set(&path, "view.split_width", &vals(&["wide"])).unwrap_err().contains("a number"));
        assert!(set(&path, "editor", &vals(&["a", "b"])).unwrap_err().contains("one value"));
        let e = set(&path, "view.layout", &vals(&["sideways"])).unwrap_err();
        assert!(e.contains("view.layout") && e.contains("nothing written"), "{e}");
        assert!(set(&path, "keys.menu", &vals(&["ctrl-nothing-at-all"])).unwrap_err().contains("keys.menu"));
        assert_eq!(read(), before);
    }

    #[test]
    fn the_bugzilla_a_bug_opens_on_is_a_web_link_to_a_site() {
        assert_eq!(Config::default().reviews.bugzilla, "https://bugzilla.mozilla.org");
        let (c, wrong) = Config::parse("[reviews]\nbugzilla = \"https://bugzilla.example.com/\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.reviews.bugzilla, "https://bugzilla.example.com", "its last slash taken off");
        assert_eq!(value_of(&c, "reviews.bugzilla"), Ok(vec!["https://bugzilla.example.com".to_string()]));
        // What is not one keeps the default, and is said.
        let (c, wrong) = Config::parse("[reviews]\nbugzilla = \"bugzilla.example.com\"\n").unwrap();
        assert_eq!(c.reviews.bugzilla, BUGZILLA);
        assert_eq!(wrong, ["reviews.bugzilla: \"bugzilla.example.com\" is not a web link to a site, https://… or http://…"]);
        // Printed with Phabricator's settings, when stacked is built with them.
        assert_eq!(Config::default().render().contains("\nbugzilla = \"https://bugzilla.mozilla.org\"\n"), cfg!(feature = "moz-phab"));
        // Written, or refused with the file as it was.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        set(&path, "reviews.bugzilla", &["https://bz.example.com".to_string()]).unwrap();
        assert_eq!(Config::load(&path).0.reviews.bugzilla, "https://bz.example.com");
        let before = std::fs::read_to_string(&path).unwrap();
        let e = set(&path, "reviews.bugzilla", &["nowhere".to_string()]).unwrap_err();
        assert!(e.contains("\"nowhere\" is not a web link") && e.ends_with("nothing written"), "{e}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn colours_are_set_as_git_writes_them() {
        use ratatui::style::{Color, Modifier};
        let (c, wrong) = Config::parse("[colors]\nyou = \"208 bold\"\nagent = \"purpleish\"\n").unwrap();
        assert_eq!(c.colors.style(Paint::You), Style::new().fg(Color::Indexed(208)).add_modifier(Modifier::BOLD));
        assert_eq!(c.colors.spec(Paint::Agent), "134", "what is wrong keeps its default");
        assert_eq!(c.colors.spec(Paint::Added), "green", "what is not set too");
        // A pane's pick while the cursor is elsewhere: on a grey background,
        // its text as it is.
        assert_eq!(c.colors.spec(Paint::Picked), "normal 238");
        assert_eq!(c.colors.style(Paint::Picked), Style::new().bg(Color::Indexed(238)));
        // Code in a comment's markdown: light grey on a dark grey.
        assert_eq!(c.colors.style(Paint::Code), Style::new().fg(Color::Indexed(252)).bg(Color::Indexed(236)));
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(wrong[0].starts_with("colors.agent: \"purpleish\" is not a colour") && wrong[0].ends_with("; it stays \"134\""), "{wrong:?}");
        // A colour there is not, as any setting there is not: the file is
        // not used.
        let e = Config::parse("[colors]\ncomment = \"red\"\n").unwrap_err();
        assert!(e.contains("unknown colour `comment`") && e.contains("you, agent, reviewer"), "{e}");
        // Written, read and refused as any setting is.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        set(&path, "colors.added", &vals(&["brightgreen"])).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[colors]\nadded = \"brightgreen\"\n");
        assert_eq!(value_of(&Config::load(&path).0, "colors.added"), Ok(vec!["brightgreen".to_string()]));
        assert_eq!(value_of(&Config::default(), "colors.you"), Ok(vec!["172".to_string()]));
        let e = set(&path, "colors.added", &vals(&["greenish"])).unwrap_err();
        assert!(e.contains("colors.added: \"greenish\" is not a colour"), "{e}");
        assert!(set(&path, "colors.comment", &vals(&["red"])).unwrap_err().contains("not a setting"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[colors]\nadded = \"brightgreen\"\n");
    }

    #[test]
    fn a_file_over_another_sets_only_what_it_says() {
        let (global, wrong) = Config::parse(
            "editor = \"vim\"\n[agent]\ntools = [\"mcp__bugzilla__*\"]\nargs = [\"--verbose\"]\n[view]\nlayout = \"compact\"\n[colors]\nyou = \"208\"\n[keys]\nmenu = \"m\"\nup = [\"up\", \"k\"]\n",
        )
        .unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        let (c, wrong) = global
            .over("[agent]\ntools = [\"mcp__moz__*\"]\n[view]\nlayout = \"sideways\"\nreading = true\n[colors]\nagent = \"99\"\nyou = \"purpleish\"\n[keys]\ndown = [\"down\", \"j\"]\n")
            .unwrap();
        // What it sets is its own: a list whole, a table's settings each.
        assert_eq!(c.agent.tools, ["mcp__moz__*"]);
        assert!(c.view.reading);
        assert_eq!(c.colors.spec(Paint::Agent), "99");
        assert_eq!(c.keys.names(Key::Down), ["down", "j"]);
        // What it does not set, or sets wrong, is as it was.
        assert_eq!((c.editor.as_deref(), c.agent.args.as_slice()), (Some("vim"), &["--verbose".to_string()][..]));
        assert_eq!(c.view.layout, Layout::Compact);
        assert_eq!(c.colors.spec(Paint::You), "208");
        assert_eq!((c.keys.label(Key::Menu), c.keys.names(Key::Up)), ("m".to_string(), vec!["up".to_string(), "k".to_string()]));
        assert_eq!(wrong.len(), 2, "{wrong:?}");
        assert!(wrong[0].starts_with("view.layout: \"sideways\"") && wrong[1].ends_with("; it stays \"208\""), "{wrong:?}");
        // An empty list is none; an empty editor, the one of $VISUAL.
        let (c, _) = global.over("editor = \"\"\n[agent]\ntools = []\n").unwrap();
        assert!(c.agent.tools.is_empty() && c.editor.is_none());
        // A key for two things is said by the file that gives it so.
        let (_, wrong) = global.over("[keys]\ndown = [\"k\"]\n").unwrap();
        assert_eq!(wrong, ["keys: k is up and down; it does the first"]);
        let (twice, _) = Config::parse("[keys]\nup = [\"k\"]\ndown = [\"k\"]\n").unwrap();
        assert_eq!(twice.over("[keys]\nmenu = \"m\"\n").unwrap().1, Vec::<String>::new());
    }

    #[test]
    fn a_checkout_s_own_settings_are_read_unless_git_tracks_them() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir_all(top.join("sub")).unwrap();
        let git = |args: &[&str]| crate::git::git(&top, args).unwrap();
        git(&["init", "-q"]);
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (base, _) = Config::parse("editor = \"vim\"\n[agent]\ntools = [\"mcp__bugzilla__*\"]\n").unwrap();
        // None yet: the settings as they are.
        assert_eq!(with_local(&base, &top.join("sub")), (base.clone(), vec![]));
        // Written from anywhere in the checkout, at its top, out of git's
        // sight; read from anywhere in it.
        let local = set_local(&top.join("sub"), "agent.tools", &vals(&["mcp__moz__*"])).unwrap();
        assert_eq!(local, top.join(".stacked/local.toml"));
        assert_eq!(git(&["status", "--porcelain", "--untracked-files=all"]), "");
        let (c, wrong) = with_local(&base, &top.join("sub"));
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!((c.agent.tools.clone(), c.editor.as_deref()), (vals(&["mcp__moz__*"]), Some("vim")));
        assert_eq!(local_value(&top, "agent.tools"), Ok(Some(vals(&["mcp__moz__*"]))));
        assert_eq!(local_value(&top, "editor"), Ok(None));
        assert_eq!(local_keys(&top), ["agent.tools"]);
        assert!(file_sets(&local, "agent.tools") && !file_sets(&local, "editor"));
        // As `stacked config` prints them, the checkout's said so.
        let shown = mark_local(&c.render(), &local_keys(&top));
        assert!(shown.contains("\ntools = [\"mcp__moz__*\"]   # .stacked/local.toml\n"), "{shown}");
        assert!(shown.contains("\neditor = \"vim\"\n"), "{shown}");
        // Taken out: the settings' own again.
        unset_local(&top, "agent.tools").unwrap();
        assert_eq!(with_local(&base, &top).0.agent.tools, ["mcp__bugzilla__*"]);
        // One git tracks came with the repository: not read, nor written.
        set_local(&top, "editor", &vals(&["hx"])).unwrap();
        assert_eq!(with_local(&base, &top).0.editor.as_deref(), Some("hx"));
        git(&["add", "-f", ".stacked/local.toml"]);
        let (c, wrong) = with_local(&base, &top);
        assert_eq!(c.editor.as_deref(), Some("vim"));
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(
            wrong[0].ends_with("local.toml: git tracks it, and a checkout's own settings are yours, not the repository's; it is not read"),
            "{wrong:?}"
        );
        let e = set_local(&top, "editor", &vals(&["nano"])).unwrap_err();
        assert!(e.ends_with("; nothing written"), "{e}");
        // Outside a checkout, there is none.
        let out = tmp.path().canonicalize().unwrap();
        assert_eq!(local_path(&out), None);
        assert!(set_local(&out, "editor", &vals(&["hx"])).unwrap_err().contains("is not in a git checkout"));
    }

    #[test]
    fn the_patches_pane_is_expanded_files_or_hidden() {
        let (c, wrong) = Config::parse("[view]\npatches = \"files\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.view.patches, PatchesPane::Files);
        // Compact, as versions before the files wrote it: the files.
        let (c, wrong) = Config::parse("[view]\npatches = \"compact\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.view.patches, PatchesPane::Files);
        let (c, _) = Config::parse("[view]\npatches = false\n").unwrap();
        assert_eq!(c.view.patches, PatchesPane::Hidden, "hidden, as before there were modes");
        let (c, wrong) = Config::parse("[view]\npatches = \"wide\"\n").unwrap();
        assert_eq!(c.view.patches, PatchesPane::Expanded, "what is wrong keeps its default");
        assert_eq!(wrong, ["view.patches: \"wide\" is not expanded, files or hidden"]);
        // Written, read and refused as any setting is; compact is read, and
        // not written.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        set(&path, "view.patches", &vals(&["files"])).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\npatches = \"files\"\n");
        assert_eq!(value_of(&Config::load(&path).0, "view.patches"), Ok(vals(&["files"])));
        assert!(set(&path, "view.patches", &vals(&["compact"])).unwrap_err().contains("expanded, files or hidden, not \"compact\""));
        std::fs::write(&path, "[view]\npatches = \"compact\"\n").unwrap();
        assert_eq!(value_of(&Config::load(&path).0, "view.patches"), Ok(vals(&["files"])));
        assert!(Config::default().render().contains("\npatches = \"expanded\"\n"));
        // ← on the patches cycles them.
        let all = [PatchesPane::Expanded, PatchesPane::Files, PatchesPane::Hidden];
        assert_eq!(all.map(PatchesPane::next), [PatchesPane::Files, PatchesPane::Hidden, PatchesPane::Expanded]);
    }

    #[test]
    fn the_comments_pane_is_expanded_compact_or_hidden() {
        let (c, wrong) = Config::parse("[view]\ncomments = \"compact\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.view.comments, Pane::Compact);
        let (c, _) = Config::parse("[view]\ncomments = false\n").unwrap();
        assert_eq!(c.view.comments, Pane::Hidden, "hidden, as before there were modes");
        let (c, wrong) = Config::parse("[view]\ncomments = \"files\"\n").unwrap();
        assert_eq!(c.view.comments, Pane::Expanded, "the files are the patches'");
        assert_eq!(wrong, ["view.comments: \"files\" is not expanded, compact or hidden"]);
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        set(&path, "view.comments", &vals(&["compact"])).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\ncomments = \"compact\"\n");
        assert_eq!(value_of(&Config::load(&path).0, "view.comments"), Ok(vals(&["compact"])));
        assert!(set(&path, "view.comments", &vals(&["files"])).unwrap_err().contains("expanded, compact or hidden"));
        // → on the comments cycles them.
        assert_eq!([Pane::Expanded.next(), Pane::Compact.next(), Pane::Hidden.next()], [Pane::Compact, Pane::Hidden, Pane::Expanded]);
    }

    #[test]
    fn the_layout_is_unified_side_by_side_compact_or_unified_split_in_two() {
        let (c, wrong) = Config::parse("[view]\nlayout = \"unified-split\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.view.layout, Layout::Halves);
        assert!(c.render().contains("\nlayout = \"unified-split\"\n"), "{}", c.render());
        // Side by side, as it was named before.
        let (c, wrong) = Config::parse("[view]\nlayout = \"split\"\n").unwrap();
        assert!(wrong.is_empty() && c.view.layout == Layout::Split, "{wrong:?}");
        let (c, wrong) = Config::parse("[view]\nlayout = \"halves\"\n").unwrap();
        assert_eq!(c.view.layout, Layout::Unified, "what is wrong keeps its default");
        assert_eq!(wrong, ["view.layout: \"halves\" is not unified, side-by-side, compact or unified-split"]);
        // Written and read as any setting is.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        set(&path, "view.layout", &["unified-split".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\nlayout = \"unified-split\"\n");
        assert_eq!(value_of(&Config::load(&path).0, "view.layout"), Ok(vec!["unified-split".to_string()]));
    }

    #[test]
    fn the_interdiffs_start_as_interdiffs_in_columns_side_by_side_or_as_the_change_in_the_views_of_the_code() {
        // As stacked always drew them, unless set.
        assert_eq!(Config::default().view.interdiff, InterLayout::Interdiff);
        assert!(Config::default().render().contains("\ninterdiff = \"interdiff\"\n"), "{}", Config::default().render());
        let (c, wrong) = Config::parse("[view]\ninterdiff = \"side-by-side\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.view.interdiff, InterLayout::Sides);
        assert_eq!(value_of(&c, "view.interdiff"), Ok(vec!["side-by-side".to_string()]));
        let (c, wrong) = Config::parse("[view]\ninterdiff = \"columns\"\n").unwrap();
        assert!(wrong.is_empty() && c.view.interdiff == InterLayout::Columns, "{wrong:?}");
        let (c, wrong) = Config::parse("[view]\ninterdiff = \"split\"\n").unwrap();
        assert_eq!(c.view.interdiff, InterLayout::Interdiff, "what is wrong keeps its default");
        assert_eq!(wrong, ["view.interdiff: \"split\" is not interdiff, columns, side-by-side, change-unified, change-side-by-side or change-compact"]);
        // The change as a patch of its own, in each of the views of the
        // code.
        for (name, v) in [("change-unified", InterLayout::ChangeUnified), ("change-side-by-side", InterLayout::ChangeSides), ("change-compact", InterLayout::ChangeCompact)] {
            let (c, wrong) = Config::parse(&format!("[view]\ninterdiff = \"{name}\"\n")).unwrap();
            assert!(wrong.is_empty() && c.view.interdiff == v && v.change(), "{name}: {wrong:?}");
            assert_eq!((value_of(&c, "view.interdiff"), v.name()), (Ok(vec![name.to_string()]), name));
        }
        assert!(Config::default().render().contains("\"change-unified\", \"change-side-by-side\" or \"change-compact\""), "{}", Config::default().render());
        // Written and read as any setting is; a view there is not, refused.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        set(&path, "view.interdiff", &["side-by-side".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\ninterdiff = \"side-by-side\"\n");
        assert_eq!(Config::load(&path).0.view.interdiff, InterLayout::Sides);
        let e = set(&path, "view.interdiff", &["compact".to_string()]).unwrap_err();
        assert!(e.contains("view.interdiff: \"compact\" is not interdiff, columns, side-by-side, change-unified, change-side-by-side or change-compact") && e.contains("nothing written"), "{e}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\ninterdiff = \"side-by-side\"\n");
    }

    #[test]
    fn the_interdiffs_start_with_every_line_unless_set_to_only_what_the_patches_changed() {
        // Every line, as stacked always drew them, unless set.
        assert_eq!(Config::default().view.interdiff_lines, InterLines::Every);
        assert!(Config::default().render().contains("\ninterdiff_lines = \"every\"\n"), "{}", Config::default().render());
        let (c, wrong) = Config::parse("[view]\ninterdiff_lines = \"changed\"\n").unwrap();
        assert!(wrong.is_empty() && c.view.interdiff_lines == InterLines::Changed, "{wrong:?}");
        assert_eq!(value_of(&c, "view.interdiff_lines"), Ok(vec!["changed".to_string()]));
        let (c, wrong) = Config::parse("[view]\ninterdiff_lines = \"some\"\n").unwrap();
        assert_eq!(c.view.interdiff_lines, InterLines::Every, "what is wrong keeps its default");
        assert_eq!(wrong, ["view.interdiff_lines: \"some\" is not every or changed"]);
        // Written and read as any setting is; lines there are not, refused.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        set(&path, "view.interdiff_lines", &["changed".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\ninterdiff_lines = \"changed\"\n");
        assert_eq!(Config::load(&path).0.view.interdiff_lines, InterLines::Changed);
        let e = set(&path, "view.interdiff_lines", &["all".to_string()]).unwrap_err();
        assert!(e.contains("view.interdiff_lines: \"all\" is not every or changed") && e.contains("nothing written"), "{e}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\ninterdiff_lines = \"changed\"\n");
    }

    #[test]
    fn lines_of_white_space_alone_need_review_only_when_set_so() {
        assert!(!Config::default().view.whitespace_needs_review);
        assert!(Config::default().render().contains("\nwhitespace_needs_review = false\n"));
        let (c, wrong) = Config::parse("[view]\nwhitespace_needs_review = true\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert!(c.view.whitespace_needs_review);
        assert_eq!(value_of(&c, "view.whitespace_needs_review"), Ok(vec!["true".to_string()]));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        std::fs::write(&path, "").unwrap();
        set(&path, "view.whitespace_needs_review", &["true".to_string()]).unwrap();
        assert!(Config::load(&path).0.view.whitespace_needs_review);
        assert!(set(&path, "view.whitespace_needs_review", &["yes".to_string()]).unwrap_err().contains("true or false"));
    }

    #[test]
    fn the_ask_dialog_checks_the_network_unless_set_not_to() {
        assert!(Config::default().agent.reach_check);
        let text = Config::default().render();
        assert!(text.contains("\n# Whether the dialog that asks the agent checks, as it opens, that the network\n"), "{text}");
        assert!(text.contains("\n# not: false turns the check off, where it is wrong though claude works.\nreach_check = true\n"), "{text}");
        let (c, wrong) = Config::parse("[agent]\nreach_check = false\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert!(!c.agent.reach_check);
        assert_eq!(value_of(&c, "agent.reach_check"), Ok(vec!["false".to_string()]));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        set(&path, "agent.reach_check", &["false".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[agent]\nreach_check = false\n");
        assert!(!Config::load(&path).0.agent.reach_check);
        assert!(set(&path, "agent.reach_check", &["no".to_string()]).unwrap_err().contains("true or false"));
    }

    #[test]
    fn messages_are_wrapped_at_72_columns_unless_set_otherwise() {
        assert_eq!(Config::default().view.message_width, 72);
        assert!(Config::default().render().contains("\nmessage_width = 72\n"));
        let (c, wrong) = Config::parse("[view]\nmessage_width = 80\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(c.view.message_width, 80);
        assert_eq!(value_of(&c, "view.message_width"), Ok(vec!["80".to_string()]));
        // Too narrow to wrap at: said, and the default kept.
        let (c, wrong) = Config::parse("[view]\nmessage_width = 10\n").unwrap();
        assert_eq!(c.view.message_width, 72, "what is wrong keeps its default");
        assert_eq!(wrong, ["view.message_width: 10 is too narrow for the lines of a message"]);
        // Written, read and refused as any setting is.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        set(&path, "view.message_width", &["100".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[view]\nmessage_width = 100\n");
        assert_eq!(Config::load(&path).0.view.message_width, 100);
        assert!(set(&path, "view.message_width", &["wide".to_string()]).unwrap_err().contains("a number"));
        let e = set(&path, "view.message_width", &["5".to_string()]).unwrap_err();
        assert!(e.contains("too narrow") && e.ends_with("nothing written"), "{e}");
        assert_eq!(Config::load(&path).0.view.message_width, 100);
    }

    #[test]
    fn the_setup_writes_each_list_in_one_go_and_its_old_mark_is_passed_over() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let read = || std::fs::read_to_string(&path).unwrap();
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Not set: nothing for changes, curl and jq for answers, as before
        // there were lists; nothing the reviews allowed.
        let d = Config::default();
        assert_eq!(value_of(&d, "agent.change_tools"), Ok(vec![]));
        assert_eq!(value_of(&d, "agent.answer_tools"), Ok(vals(&["Bash(curl *)", "Bash(jq *)"])));
        assert_eq!(value_of(&d, "agent.answer_accepted"), Ok(vec![]));
        for line in ["\nchange_tools = []\n", "\nanswer_tools = [\"Bash(curl *)\", \"Bash(jq *)\"]\n", "\nanswer_accepted = []\n", "\ntest_accepted = []\n"] {
            assert!(d.render().contains(line), "{line:?} in {}", d.render());
        }
        assert!(!d.render().contains("setup ="), "{}", d.render());
        // Saved: each list, the rest as it was.
        std::fs::write(&path, "# mine\n[agent]\ntools = [\"mcp__moz__*\"]\n").unwrap();
        let saved = crate::setup::Chosen {
            answer: vals(&["Bash(curl *)"]),
            #[cfg(feature = "try-push")]
            try_push: vals(&["Bash(jq *)"]),
            ..Default::default()
        };
        set_all(&path, &saved.settings()).unwrap();
        let tried = if cfg!(feature = "try-push") { "try_tools = [\"Bash(jq *)\"]\n" } else { "" };
        assert_eq!(read(), format!("# mine\n[agent]\ntools = [\"mcp__moz__*\"]\nchange_tools = []\nanswer_tools = [\"Bash(curl *)\"]\n{tried}"));
        let (c, wrong) = Config::load(&path);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!((c.agent.change_tools.clone(), c.agent.answer_tools.clone()), (vec![], vals(&["Bash(curl *)"])));
        #[cfg(feature = "try-push")]
        assert_eq!(value_of(&c, "agent.try_tools"), Ok(vals(&["Bash(jq *)"])));
        // A file a stacked built with the try push wrote reads in any.
        let (_, wrong) = Config::parse("[agent]\ntry_tools = [\"Bash(curl *)\"]\ntry_accepted = [\"WebFetch\"]\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        // What a review allowed from now on: a list of its own for each
        // kind of ask, added to its tools.
        let (c, _) = Config::parse("[agent]\nanswer_tools = [\"Bash(jq *)\"]\nanswer_accepted = [\"Bash(searchfox-cli *)\"]\ntest_accepted = [\"WebFetch\"]\n").unwrap();
        assert_eq!(c.agent.accepted(crate::db::Way::Answer), ["Bash(searchfox-cli *)"]);
        assert_eq!(c.agent.accepted(crate::db::Way::Test), ["WebFetch"]);
        assert!(c.agent.accepted(crate::db::Way::Change).is_empty());
        assert_eq!(value_of(&c, "agent.answer_tools"), Ok(vals(&["Bash(jq *)"])), "its own list as it was");
        // The setup's old mark, as a stacked of before wrote it: read, and
        // passed over, nothing said; not written again; taken out.
        std::fs::write(&path, "[agent]\nsetup = 2\nchange_tools = []\n").unwrap();
        let (_, wrong) = Config::load(&path);
        assert!(wrong.is_empty(), "{wrong:?}");
        let gone = "agent.setup is no longer a setting: no ask waits for the setup of the agent's tools any more";
        assert_eq!(set(&path, "agent.setup", &vals(&["2"])), Err(format!("{gone}; nothing written")));
        assert_eq!(value_of(&c, "agent.setup"), Err(gone.to_string()));
        unset(&path, "agent.setup").unwrap();
        assert_eq!(read(), "[agent]\nchange_tools = []\n");
        // One of them wrong: none written.
        let before = read();
        let e = set_all(&path, &[("agent.change_tools", vals(&["WebFetch"])), ("agent.answer_accepted", vals(&[]))]).map(|_| ());
        assert!(e.is_ok(), "{e:?}");
        let before_wrong = read();
        let e = set_all(&path, &[("agent.answer_tools", vals(&["Read"])), ("view.stream_lines", vals(&["many"]))]).unwrap_err();
        assert!(e.contains("view.stream_lines is a number"), "{e}");
        assert_eq!(read(), before_wrong);
        assert_ne!(before, before_wrong);
    }

    #[test]
    fn what_is_printed_reads_back_the_same() {
        let (c, wrong) = Config::parse(
            "editor = \"vim\"\n[agent]\nargs = [\"--verbose\"]\n[view]\nsplit_width = 80\n[colors]\nyou = \"208 bold\"\n[keys]\ndown = [\"down\", \"j\"]\n",
        )
        .unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        let (again, wrong) = Config::parse(&c.render()).unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(again, c);
        let (defaults, _) = Config::parse(&Config::default().render()).unwrap();
        assert_eq!(defaults, Config::default());
    }

    #[test]
    fn every_ask_works_in_a_worktree_of_its_own_and_pwd_is_read_as_ondemand() {
        // Unless set, an answer's worktree is made when an ask needs it, as
        // a change's is; the settings say every ask has its own.
        let d = Config::default();
        assert_eq!((d.agent.worktree, d.agent.answer_only_worktree), (Workspace::OnDemand, Workspace::OnDemand));
        let text = d.render();
        assert!(text.contains("\n# Every ask runs in a worktree of its own, never in your checkout. A change works in\n"), "{text}");
        assert!(text.contains("\nworktree = \"ondemand\"\n") && text.contains("\nanswer_only_worktree = \"ondemand\"\n"), "{text}");
        let agent = &text[text.find("\n[agent]\n").unwrap()..];
        let agent = &agent[..agent[1..].find("\n[").map_or(agent.len(), |e| e + 1)];
        assert!(!agent.contains("pwd"), "{agent}");
        // "pwd", your checkout, as a stacked of before took it: read as
        // "ondemand", and said.
        let (c, wrong) = Config::parse("[agent]\nworktree = \"pwd\"\nanswer_only_worktree = \"pwd\"\n").unwrap();
        assert_eq!((c.agent.worktree, c.agent.answer_only_worktree), (Workspace::OnDemand, Workspace::OnDemand));
        assert_eq!(
            wrong,
            [
                "agent.worktree = \"pwd\" is no longer supported: asks run in a worktree of their own; read as \"ondemand\"",
                "agent.answer_only_worktree = \"pwd\" is no longer supported: asks run in a worktree of their own; read as \"ondemand\"",
            ]
        );
        assert_eq!(value_of(&c, "agent.answer_only_worktree"), Ok(vec!["ondemand".to_string()]));
        // Over a file that says "precreate", too: "ondemand".
        let (base, _) = Config::parse("[agent]\nanswer_only_worktree = \"precreate\"\n").unwrap();
        let (c, wrong) = base.over("[agent]\nanswer_only_worktree = \"pwd\"\n").unwrap();
        assert_eq!((c.agent.answer_only_worktree, wrong.len()), (Workspace::OnDemand, 1), "{wrong:?}");
        // Neither: said, and the default kept.
        let (c, wrong) = Config::parse("[agent]\nworktree = \"sometimes\"\n").unwrap();
        assert_eq!(c.agent.worktree, Workspace::OnDemand);
        assert_eq!(wrong, ["agent.worktree: \"sometimes\" is not ondemand or precreate"]);
        // Written, or refused with why, the file as it was.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let read = || std::fs::read_to_string(&path).unwrap();
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        set(&path, "agent.answer_only_worktree", &vals(&["precreate"])).unwrap();
        assert_eq!(read(), "[agent]\nanswer_only_worktree = \"precreate\"\n");
        for key in ["agent.worktree", "agent.answer_only_worktree"] {
            let e = set(&path, key, &vals(&["pwd"])).unwrap_err();
            assert_eq!(e, format!("{key} = \"pwd\" is no longer supported: asks run in a worktree of their own; nothing written"));
        }
        let e = set(&path, "agent.worktree", &vals(&["sometimes"])).unwrap_err();
        assert_eq!(e, "agent.worktree is ondemand or precreate, not \"sometimes\"");
        assert_eq!(read(), "[agent]\nanswer_only_worktree = \"precreate\"\n");
        // A file that still says "pwd" takes the other settings written in
        // it, and loses what it says once "ondemand" is written over it.
        std::fs::write(&path, "[agent]\nworktree = \"pwd\"\n").unwrap();
        set(&path, "agent.chain", &vals(&["false"])).unwrap();
        assert_eq!(read(), "[agent]\nworktree = \"pwd\"\nchain = false\n");
        assert!(set(&path, "agent.worktree", &vals(&["pwd"])).is_err(), "refused, even as the file says it");
        set(&path, "agent.worktree", &vals(&["ondemand"])).unwrap();
        let (c, wrong) = Config::load(&path);
        assert!(wrong.is_empty() && c.agent.worktree == Workspace::OnDemand && !c.agent.chain, "{wrong:?}");
    }

    #[test]
    fn the_keys_are_listed_by_the_context_they_are_read_in() {
        let text = Config::default().render();
        let keys = &text[text.find("\n[keys]\n").unwrap()..];
        let at = |line: &str| keys.find(line).unwrap_or_else(|| panic!("no {line:?} in {keys}"));
        let (screens, search) = (at("\n# screens: on the screens"), at("\n# search: in the search of the middle"));
        assert!(screens < at("\nup = [\"up\"]\n") && at("\nquit = [\"ctrl-c\"]\n") < search, "{keys}");
        assert!(search < at("\nfind_next = [\"tab\"]\n") && search < at("\nfind_previous = [\"shift-tab\"]\n"), "{keys}");
        // The sync's keys, gone with it, are not listed.
        assert!(!keys.contains("sync") && !keys.contains("keep_stack") && !keys.contains("keep_workspace"), "{keys}");
        // One key in two contexts is no clash; two things of one are.
        let (c, wrong) = Config::parse("[keys]\nmenu = \"tab\"\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!((c.keys.label(Key::Menu), c.keys.label(Key::FindNext)), ("Tab".to_string(), "Tab".to_string()));
        let (_, wrong) = Config::parse("[keys]\nfind_next = \"f5\"\nfind_previous = [\"f5\"]\n").unwrap();
        assert_eq!(wrong, ["keys: F5 is find_next and find_previous; it does the first"]);
    }

    #[test]
    fn the_sync_s_keys_are_said_to_be_gone_read_refused_and_taken_out() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("stacked.toml");
        let gone = "the sync of the commit messages it picked in is gone";
        let vals = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Read: said, with why, the rest of the file used.
        std::fs::write(&path, "[keys]\nmenu = \"m\"\nkeep_stack = \"s\"\nkeep_workspace = [\"w\"]\n").unwrap();
        let (c, wrong) = Config::load(&path);
        let at = path.display();
        assert_eq!(
            wrong,
            [
                format!("{at}: keys.keep_stack is no longer supported: {gone}; it is passed over"),
                format!("{at}: keys.keep_workspace is no longer supported: {gone}; it is passed over"),
            ]
        );
        assert_eq!(c.keys.label(Key::Menu), "m");
        // Written, or read alone: refused, with why.
        assert_eq!(set(&path, "keys.keep_stack", &vals(&["s"])), Err(format!("keys.keep_stack is no longer supported: {gone}; nothing written")));
        assert_eq!(value_of(&c, "keys.keep_workspace"), Err(format!("keys.keep_workspace is no longer supported: {gone}")));
        // Taken out: gone from the file, and nothing more is said.
        unset(&path, "keys.keep_stack").unwrap();
        unset(&path, "keys.keep_workspace").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[keys]\nmenu = \"m\"\n");
        assert!(Config::load(&path).1.is_empty());
    }

    #[test]
    fn stacked_web_s_port_is_random_fixed_random_or_a_port_and_any_other_is_said() {
        // Random unless set, none kept.
        let d = Config::default();
        assert_eq!((d.web.port, d.web.kept_port), (Port::Random, None));
        assert_eq!(value_of(&d, "web.port"), Ok(vec!["random".to_string()]));
        assert_eq!(value_of(&d, "web.kept_port"), Ok(vec![]));
        // Each kind; a port in quotes, as a number.
        for (written, port) in [
            ("\"random\"", Port::Random),
            ("\"fixed-random\"", Port::FixedRandom),
            ("8080", Port::Fixed(8080)),
            ("\"8080\"", Port::Fixed(8080)),
            ("1", Port::Fixed(1)),
            ("65535", Port::Fixed(65535)),
        ] {
            let (c, wrong) = Config::parse(&format!("[web]\nport = {written}\n")).unwrap();
            assert!(wrong.is_empty(), "{written}: {wrong:?}");
            assert_eq!(c.web.port, port, "{written}");
        }
        // Any other value, of any kind: said, and random kept, the rest of
        // the file read.
        for written in ["\"sometimes\"", "0", "65536", "-1", "true", "80.5", "[8080]"] {
            let (c, wrong) = Config::parse(&format!("editor = \"hx\"\n[web]\nport = {written}\n")).unwrap();
            assert_eq!((c.web.port, c.editor.as_deref()), (Port::Random, Some("hx")), "{written}");
            assert_eq!(wrong, [format!("web.port: {written} is not \"random\", \"fixed-random\" or a port from 1 to 65535")]);
        }
        // The port fixed-random kept, beside it; one that is no port, said.
        let (c, wrong) = Config::parse("[web]\nport = \"fixed-random\"\nkept_port = 51234\n").unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!((c.web.port, c.web.kept_port), (Port::FixedRandom, Some(51234)));
        let (c, wrong) = Config::parse("[web]\nkept_port = 0\n").unwrap();
        assert_eq!(c.web.kept_port, None);
        assert_eq!(wrong, ["web.kept_port: 0 is not a port from 1 to 65535"]);
        // Written as any setting is, the rest of the file as it was: a name
        // in quotes, a port as a number, the kept port beside them.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let read = || std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, "# mine\n[web]\n").unwrap();
        set(&path, "web.port", &["fixed-random".to_string()]).unwrap();
        set(&path, "web.kept_port", &["51234".to_string()]).unwrap();
        assert_eq!(read(), "# mine\n[web]\nport = \"fixed-random\"\nkept_port = 51234\n");
        // Another value: the kept port passed over, and kept.
        set(&path, "web.port", &["8080".to_string()]).unwrap();
        assert_eq!(read(), "# mine\n[web]\nport = 8080\nkept_port = 51234\n");
        let (c, wrong) = Config::load(&path);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!((c.web.port, c.web.kept_port), (Port::Fixed(8080), Some(51234)));
        assert_eq!((value_of(&c, "web.port"), value_of(&c, "web.kept_port")), (Ok(vec!["8080".to_string()]), Ok(vec!["51234".to_string()])));
        // Refused, the file as it was.
        for v in ["sometimes", "0", "65536", "-80"] {
            assert_eq!(set(&path, "web.port", &[v.to_string()]), Err(format!("web.port is \"random\", \"fixed-random\" or a port from 1 to 65535, not {v:?}")));
        }
        assert_eq!(set(&path, "web.kept_port", &["random".to_string()]), Err("web.kept_port is a port from 1 to 65535, not \"random\"".to_string()));
        assert_eq!(read(), "# mine\n[web]\nport = 8080\nkept_port = 51234\n");
        // Printed, built with stacked web, and read back the same.
        let text = c.render();
        assert_eq!(text.contains("\n[web]\n"), cfg!(feature = "web"), "{text}");
        if cfg!(feature = "web") {
            assert!(text.contains("\nport = 8080\n# The port \"fixed-random\" took the first time") && text.contains("\nkept_port = 51234\n"), "{text}");
            assert_eq!(Config::parse(&text), Ok((c, vec![])));
            let d = Config::default().render();
            assert!(d.contains("\nport = \"random\"\n") && d.contains("\n# kept_port = ...   (none taken yet)\n"), "{d}");
        }
    }

    #[test]
    fn stacked_web_s_settings_are_the_settings_file_s_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir_all(top.join(".stacked")).unwrap();
        crate::git::git(&top, &["init", "-q"]).unwrap();
        let (base, _) = Config::parse("[web]\nport = \"fixed-random\"\nkept_port = 51234\n").unwrap();
        // A checkout's own that sets them, even wrong: as the settings' file
        // has them, each said to be passed over, the rest of it read.
        let local = top.join(".stacked/local.toml");
        std::fs::write(&local, "editor = \"hx\"\n[web]\nport = \"sometimes\"\nkept_port = 8080\n").unwrap();
        let (c, mut wrong) = with_local(&base, &top);
        assert_eq!((&c.web, c.editor.as_deref()), (&base.web, Some("hx")));
        wrong.sort();
        let at = local.display();
        let passed = "is read only from the settings' file, never from a checkout's own .stacked/local.toml: here it is passed over";
        assert_eq!(wrong, [format!("{at}: web.kept_port {passed}"), format!("{at}: web.port {passed}")]);
        // Not said to be the checkout's, nor the one in use there.
        assert_eq!(local_keys(&top), ["editor"]);
        assert_eq!(set_in(&top, "web.port"), None);
        // Neither written there, nor read from there; taken out all the same.
        let only = "web.port is read only from the settings' file, never from a checkout's own .stacked/local.toml";
        assert_eq!(set_local(&top, "web.port", &["8080".to_string()]), Err(format!("{only}: `stacked config web.port …` writes it there; nothing written")));
        assert_eq!(local_value(&top, "web.port"), Err(format!("{only}: `stacked config web.port` reads it")));
        unset_local(&top, "web.port").unwrap();
        unset_local(&top, "web.kept_port").unwrap();
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "editor = \"hx\"\n[web]\n");
        assert_eq!(with_local(&base, &top).1, Vec::<String>::new());
    }
}
