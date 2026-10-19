//! The settings, in `~/.stacked.toml` (or the file `$STACKED_CONFIG`
//! names): the database opened by default, the editor and how it is told a
//! line, the agent claude's asks run, where reviews come from, how the
//! editor starts, and the keys. Every one is optional, and what is not set
//! is as it would be with no file. A file that cannot be read is said, and
//! not used: the defaults are. `stacked config` prints every setting as it
//! is, in the file's form.
//!
//! A checkout's own settings, `.stacked/local.toml` at its top, override
//! those there: those of the checkout stacked runs in, and for a stack's
//! agent those of the stack's checkout (`of`). One that git tracks is not
//! read: it would be the repository's, and a setting can name a program to
//! run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ratatui::style::Style;
use serde::Deserialize;

use crate::keys::{Key, Keymap};
use crate::paint::{Paint, parse_color};

/// How the code is drawn at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Unified,
    Split,
    Compact,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Unified => "unified",
            Layout::Split => "side-by-side",
            Layout::Compact => "compact",
        }
    }

    fn parse(s: &str) -> Option<Layout> {
        match s {
            "unified" => Some(Layout::Unified),
            "side-by-side" | "split" => Some(Layout::Split),
            "compact" => Some(Layout::Compact),
            _ => None,
        }
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

/// Where an ask's agent works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    /// The stack's repository as it is checked out: as it is, for an
    /// answer; moved to the stack and back, for a change.
    Pwd,
    /// A worktree of the stack's own, made the first time an ask needs one,
    /// and reused while it holds nothing an agent left unfinished.
    OnDemand,
    /// The same, made when the editor opens the stack, and removed when it
    /// exits.
    Precreate,
}

impl Workspace {
    pub fn name(self) -> &'static str {
        match self {
            Workspace::Pwd => "pwd",
            Workspace::OnDemand => "ondemand",
            Workspace::Precreate => "precreate",
        }
    }

    fn parse(s: &str) -> Option<Workspace> {
        match s {
            "pwd" => Some(Workspace::Pwd),
            "ondemand" => Some(Workspace::OnDemand),
            "precreate" => Some(Workspace::Precreate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// The program run as claude; else `$STACKED_CLAUDE`'s, else `claude`.
    pub command: Option<String>,
    /// Arguments added to every run.
    pub args: Vec<String>,
    /// `--model` for every run.
    pub model: Option<String>,
    /// Tools every ask may use besides its own, as claude names them for
    /// `--allowedTools`: an MCP server's, a command.
    pub tools: Vec<String>,
    /// Where a change works.
    pub worktree: Workspace,
    /// Where an answer works.
    pub answer_only_worktree: Workspace,
    /// Where the agents' worktrees go; else `.stacked/` at the top of the
    /// stack's repository.
    pub worktrees: Option<PathBuf>,
}

impl Default for Agent {
    fn default() -> Agent {
        Agent {
            command: None,
            args: Vec::new(),
            model: None,
            tools: Vec::new(),
            worktree: Workspace::OnDemand,
            answer_only_worktree: Workspace::Pwd,
            worktrees: None,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub layout: Layout,
    /// Reading rather than commenting, at the start.
    pub reading: bool,
    /// The patches shown on the left, at the start.
    pub patches: bool,
    /// The comments shown on the right when there are any, at the start.
    pub comments: bool,
    /// The lines of claude's stream under a comment it works on.
    pub stream_lines: usize,
    /// The columns the middle needs for two sides.
    pub split_width: usize,
}

impl Default for View {
    fn default() -> View {
        View {
            layout: Layout::Unified,
            reading: false,
            patches: true,
            comments: true,
            stream_lines: 3,
            split_width: 60,
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

/// The tour, `stacked demo`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Demo {
    /// It was opened before: it starts on the list of its sections.
    pub seen: bool,
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
            },
            view: View::default(),
            colors: Colors::default(),
            keys: Keymap::default(),
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
    demo: WrittenDemo,
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
    worktree: Option<String>,
    answer_only_worktree: Option<String>,
    worktrees: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenReviews {
    source: Option<String>,
    url: Option<String>,
    arcrc: Option<String>,
    token: Option<String>,
    trailer: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenView {
    layout: Option<String>,
    reading: Option<bool>,
    patches: Option<bool>,
    comments: Option<bool>,
    stream_lines: Option<usize>,
    split_width: Option<usize>,
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

/// `~/x` as a path in $HOME.
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
        if let Some(d) = a.worktrees {
            c.agent.worktrees = Some(expand(&d));
        }
        for (key, written, at) in [
            ("agent.worktree", &a.worktree, &mut c.agent.worktree),
            ("agent.answer_only_worktree", &a.answer_only_worktree, &mut c.agent.answer_only_worktree),
        ] {
            if let Some(s) = written {
                match Workspace::parse(s) {
                    Some(p) => *at = p,
                    None => wrong.push(format!("{key}: {s:?} is not pwd, ondemand or precreate")),
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
        let v = w.view;
        if let Some(l) = v.layout {
            match Layout::parse(&l) {
                Some(l) => c.view.layout = l,
                None => wrong.push(format!("view.layout: {l:?} is not unified, side-by-side or compact")),
            }
        }
        c.view.reading = v.reading.unwrap_or(c.view.reading);
        c.view.patches = v.patches.unwrap_or(c.view.patches);
        c.view.comments = v.comments.unwrap_or(c.view.comments);
        c.view.stream_lines = v.stream_lines.unwrap_or(c.view.stream_lines);
        if let Some(n) = v.split_width {
            if n < 20 {
                wrong.push(format!("view.split_width: {n} is too narrow for two sides"));
            } else {
                c.view.split_width = n;
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
            "# Tools every ask may use besides its own, as claude names them for --allowedTools:\n\
             # an MCP server's (\"mcp__bugzilla__*\"), one of its tools (\"mcp__bugzilla__get_bug\"),\n\
             # a command (\"Bash(jq *)\"). An answer may use them too: give only tools that read.\n",
        );
        out.push_str(&format!("tools = {}\n", list(&self.agent.tools)));
        out.push_str(
            "# Where a change works: \"ondemand\", a worktree of the stack's own (.stacked/NAME/agent)\n\
             # made the first time and reused while it holds nothing unfinished; \"precreate\", the\n\
             # same made when the editor opens the stack, removed when it exits; \"pwd\", the\n\
             # stack's repository as it is checked out, moved to the stack and back (refused\n\
             # while it has changes).\n",
        );
        out.push_str(&format!("worktree = {}\n", q(self.agent.worktree.name())));
        out.push_str(
            "# Where an answer works: \"pwd\", the stack's repository as it is checked out; or\n\
             # \"ondemand\" or \"precreate\", as for a change (.stacked/NAME/answer-only-agent).\n",
        );
        out.push_str(&format!("answer_only_worktree = {}\n", q(self.agent.answer_only_worktree.name())));
        out.push_str("# Where the agents' worktrees go.\n");
        out.push_str(&match &self.agent.worktrees {
            Some(w) => format!("worktrees = {}\n", path(w)),
            None => "# worktrees = ...   (.stacked/ at the top of the stack's repository)\n".into(),
        });
        out.push_str("\n[reviews]\n# Where `pull` reads reviews from: \"phabricator\", or \"none\".\n");
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
        let v = &self.view;
        out.push_str("\n[view]\n# How code is drawn at the start: \"unified\", \"side-by-side\" or \"compact\" (1 cycles them).\n");
        out.push_str(&format!("layout = {}\n", q(v.layout.name())));
        out.push_str("# Reading rather than commenting, at the start (2 switches).\n");
        out.push_str(&format!("reading = {}\n", v.reading));
        out.push_str("# The patches on the left, and the comments on the right, shown at the start.\n");
        out.push_str(&format!("patches = {}\ncomments = {}\n", v.patches, v.comments));
        out.push_str("# The lines of claude's stream under a comment it works on.\n");
        out.push_str(&format!("stream_lines = {}\n", v.stream_lines));
        out.push_str("# The columns the middle needs for two sides.\n");
        out.push_str(&format!("split_width = {}\n", v.split_width));
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
             # space, esc, tab, f1..f24, a character, any after ctrl- or alt-. Nothing else\n\
             # is a key.\n",
        );
        for k in Key::ALL {
            out.push_str(&format!("{} = {}\n", k.name(), list(&self.keys.names(k))));
        }
        out.push_str("\n[demo]\n# The tour, `stacked demo`, was opened before: it starts on the list of its sections.\n");
        out.push_str(&format!("seen = {}\n", self.demo.seen));
        out
    }
}

/// Writes in the settings' file at `path` that the tour was opened: `seen =
/// true` under `[demo]`, the rest of the file as it was. A file that is not
/// there is made; one stacked cannot read is left as it is, and said.
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
    std::fs::write(path, new).map_err(|e| at(&e))?;
    Ok(true)
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
}

/// Every setting there is, as the file names it, and what it holds; the
/// keys', `keys.NAME`, are lists.
const SETTINGS: [(&str, Kind); 21] = [
    ("file", Kind::Text),
    ("editor", Kind::Text),
    ("agent.command", Kind::Text),
    ("agent.args", Kind::List),
    ("agent.model", Kind::Text),
    ("agent.tools", Kind::List),
    ("agent.worktree", Kind::Text),
    ("agent.answer_only_worktree", Kind::Text),
    ("agent.worktrees", Kind::Text),
    ("reviews.source", Kind::Text),
    ("reviews.url", Kind::Text),
    ("reviews.arcrc", Kind::Text),
    ("reviews.token", Kind::Text),
    ("reviews.trailer", Kind::Text),
    ("view.layout", Kind::Text),
    ("view.reading", Kind::Flag),
    ("view.patches", Kind::Flag),
    ("view.comments", Kind::Flag),
    ("view.stream_lines", Kind::Count),
    ("view.split_width", Kind::Count),
    ("demo.seen", Kind::Flag),
];

fn kind_of(key: &str) -> Result<Kind, String> {
    if let Some((_, k)) = SETTINGS.iter().find(|(name, _)| *name == key) {
        return Ok(*k);
    }
    if key.strip_prefix("colors.").and_then(Paint::named).is_some() {
        return Ok(Kind::Color);
    }
    match key.strip_prefix("keys.") {
        Some(k) if Key::ALL.iter().any(|key| key.name() == k) => Ok(Kind::List),
        _ => Err(format!("{key} is not a setting: `stacked config` prints them all")),
    }
}

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
        "agent.worktree" => vec![c.agent.worktree.name().into()],
        "agent.answer_only_worktree" => vec![c.agent.answer_only_worktree.name().into()],
        "agent.worktrees" => path(&c.agent.worktrees),
        "reviews.source" => vec![match c.reviews.source {
            Source::Phabricator => "phabricator".into(),
            Source::None => "none".into(),
        }],
        "reviews.url" => opt(&c.reviews.url),
        "reviews.arcrc" => vec![c.reviews.arcrc.display().to_string()],
        "reviews.token" => opt(&c.reviews.token),
        "reviews.trailer" => vec![c.reviews.trailer.clone()],
        "view.layout" => vec![c.view.layout.name().into()],
        "view.reading" => vec![c.view.reading.to_string()],
        "view.patches" => vec![c.view.patches.to_string()],
        "view.comments" => vec![c.view.comments.to_string()],
        "view.stream_lines" => vec![c.view.stream_lines.to_string()],
        "view.split_width" => vec![c.view.split_width.to_string()],
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
/// line as it was; a file there is not, made. Refused, with why, for a
/// setting there is not, a value of the wrong kind, or one stacked would
/// not take; a file stacked cannot read is left as it is.
pub fn set(path: &Path, key: &str, values: &[String]) -> Result<(), String> {
    let lit = literal(key, kind_of(key)?, values)?;
    edit_settings(path, |text| with_setting(text, key, Some(&lit)))
}

/// Takes `key` out of the settings' file at `path`: it is then as it would
/// be with no file.
pub fn unset(path: &Path, key: &str) -> Result<(), String> {
    kind_of(key)?;
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
    std::fs::write(path, new).map_err(|e| at(&e))
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

/// Where the settings are read from: `$STACKED_CONFIG`, else
/// `~/.stacked.toml`.
pub fn path() -> PathBuf {
    match std::env::var_os("STACKED_CONFIG") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home().join(".stacked.toml"),
    }
}

fn loaded() -> &'static Loaded {
    LOADED.get_or_init(|| {
        let (global, mut wrong) = Config::load(&path());
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
    let (global, mut wrong) = if l.given { (l.config.clone(), vec![]) } else { Config::load(&path()) };
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
/// is one and it is yours; and what was wrong with it.
fn with_local(base: &Config, dir: &Path) -> (Config, Vec<String>) {
    match local_path(dir) {
        Some(p) if p.exists() => match not_yours(&p) {
            Some(why) => (base.clone(), vec![format!("{why}; it is not read")]),
            None => base.load_over(&p),
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

/// The settings the local file of the checkout `dir` is in sets, as
/// `stacked config` names them, if it is read.
pub fn local_keys(dir: &Path) -> Vec<String> {
    let Some(t) = local_text(dir).and_then(|(_, text)| toml::from_str::<toml::Table>(&text).ok()) else {
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

/// The file whose `key` is the one in use in the checkout `dir` is in: its
/// local file, if it is read and sets it; else the settings' file, if that
/// does. None if neither does.
pub fn set_in(dir: &Path, key: &str) -> Option<PathBuf> {
    if let Some((p, text)) = local_text(dir)
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
/// `value_of` says it; None if it does not give it any.
pub fn local_value(dir: &Path, key: &str) -> Result<Option<Vec<String>>, String> {
    kind_of(key)?;
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
    let lit = literal(key, kind_of(key)?, values)?;
    let p = local_to_write(dir)?;
    let folder = p.parent().unwrap_or(Path::new("."));
    if let Some(top) = folder.parent() {
        crate::git::exclude(top, "/.stacked/").map_err(|e| e.0)?;
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    edit_settings(&p, |text| with_setting(text, key, Some(&lit)))?;
    Ok(p)
}

/// Takes `key` out of the local file of the checkout `dir` is in: there, it
/// is then as the settings' file has it. Where it was.
pub fn unset_local(dir: &Path, key: &str) -> Result<PathBuf, String> {
    kind_of(key)?;
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
    fn colours_are_set_as_git_writes_them() {
        use ratatui::style::{Color, Modifier};
        let (c, wrong) = Config::parse("[colors]\nyou = \"208 bold\"\nagent = \"purpleish\"\n").unwrap();
        assert_eq!(c.colors.style(Paint::You), Style::new().fg(Color::Indexed(208)).add_modifier(Modifier::BOLD));
        assert_eq!(c.colors.spec(Paint::Agent), "134", "what is wrong keeps its default");
        assert_eq!(c.colors.spec(Paint::Added), "green", "what is not set too");
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
}
