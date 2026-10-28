//! `stacked setup agent`: what each kind of ask may use besides what
//! stacked always gives it, set up with you. Asks that change the stack
//! have a list of their own, agent.change_tools; answer-only asks have
//! theirs, agent.answer_tools; agent.tools, which every ask may use, goes on
//! as it was. Each list starts ticked as the settings say, else as stacked
//! has it (`Start`): nothing for changes, curl and jq for answers. It lists
//! what the agent says it has (`ask::agent_tools`: its own tools, and its
//! MCP servers'); what the checkout's own claude settings allow
//! (`allowed`), ticked for answers, as you allow it yourself; and the
//! programs the checkout's instructions to the agent name (`named`),
//! unticked, as stacked cannot know what they do. A command or a tool may
//! be added to either list too, as claude writes a permission rule
//! (`rule`). Saving is the setup done: both lists are written, with
//! agent.setup, the version of the setup that was done, without which no ask
//! goes. On a screen of its own (`Setup`, for `--tui`, and in the editor
//! before its first ask), or asked line by line (`ask_lines`).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::ask::{ANSWER_GIVEN, CHANGE_GIVEN, CHANGING, GIVEN};
use crate::config::{ANSWER_TOOLS, Agent};
use crate::db::{Error, Result};
use crate::editor::Editor;
use crate::paint::{color, fit, on_cursor, put, window};

/// The version of the setup this stacked asks for: a setup done with an
/// earlier one (`agent.setup`) is asked again, as what the setup shows may
/// have changed since.
pub const VERSION: usize = 1;

/// The agent's tools that read the web, of what a change may be let use,
/// with Bash at large: said to reach the network, as nothing a change is
/// given does.
const WEB: [&str; 2] = ["WebFetch", "WebSearch"];

/// The tool `rule` lets use: what comes before its parentheses.
fn tool_of(rule: &str) -> &str {
    rule.split('(').next().unwrap_or(rule)
}

/// Whether `program` is on `path` (PATH's folders) as the shell would find
/// it: a file there that may be run.
pub fn on_path(program: &str, path: &OsStr) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .any(|d| d.join(program).metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// `text` as claude writes a permission rule, for `--allowedTools`, if it
/// reads as one: a tool's name (`WebFetch`); the same with what it is let do
/// between parentheses (`Bash(make check *)`); or an MCP server's tools,
/// all of them (`mcp__SERVER__*`, `mcp__SERVER`) or one
/// (`mcp__SERVER__TOOL`). Else why not.
pub fn rule(text: &str) -> std::result::Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("type a tool, as WebFetch, or a command, as Bash(PROGRAM *)".into());
    }
    // The tools are given claude as a list, a comma between each.
    if t.contains(',') {
        return Err(format!("{t} holds a comma, which would cut it in two in the list claude is given"));
    }
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if let Some(rest) = t.strip_prefix("mcp__") {
        let (server, tool) = match rest.split_once("__") {
            Some((s, tool)) => (s, Some(tool)),
            None => (rest, None),
        };
        return match tool {
            _ if !word(server) => Err(format!("{t} names no MCP server: a server's tool is mcp__SERVER__TOOL, all its tools mcp__SERVER__*")),
            None | Some("*") => Ok(t.to_string()),
            Some(tool) if word(tool) => Ok(t.to_string()),
            Some(_) => Err(format!("{t} names no tool of {server}: a server's tool is mcp__SERVER__TOOL, all its tools mcp__SERVER__*")),
        };
    }
    let (name, pattern) = match t.split_once('(') {
        Some((name, rest)) => (name, Some(rest)),
        None => (t, None),
    };
    let tool = name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_alphanumeric());
    if !tool && t.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Err(format!("{t} is not a tool's name, as Bash or WebFetch, then what it may do between parentheses"));
    }
    if !tool {
        let command = if t.contains(' ') { format!("Bash({t})") } else { format!("Bash({t} *)") };
        return Err(format!("{t} is not a tool's name: a command is let run as {command}"));
    }
    match pattern.map(|p| p.strip_suffix(')')) {
        None => Ok(t.to_string()),
        Some(None) => Err(format!("{t} has no ) to close what {name} may do")),
        Some(Some(inner)) if inner.trim().is_empty() => Err(format!("{t} says nothing between its parentheses: {name}(PROGRAM *) lets a program run")),
        Some(Some(_)) => Ok(t.to_string()),
    }
}

/// A file of the checkout `top`, `name` from its top: where it really is,
/// if it is a file and in the checkout. One a link takes out of it (to a
/// home's `~/.claude`) is none.
fn in_checkout(top: &Path, name: &Path) -> Option<PathBuf> {
    let root = top.canonicalize().ok()?;
    root.join(name).canonicalize().ok().filter(|r| r.starts_with(&root) && r.is_file())
}

/// A rule the checkout's own claude settings allow, which the setup lists:
/// what claude lets you run there, as written, and the file it is in, by
/// its path from the top of the checkout.
#[derive(Debug, Clone, PartialEq)]
pub struct Allowed {
    pub rule: String,
    pub file: String,
}

/// What the checkout `top`'s own claude settings allow (`permissions.allow`)
/// that stacked does not give, nor refuse (`gives`): in
/// .claude/settings.local.json first, yours and untracked, which the agents'
/// worktrees do not have, then in .claude/settings.json. Each once, as
/// written. One that claude could not be given in a list (a comma in it) is
/// left out; what the settings deny is left alone.
pub fn allowed(top: &Path) -> Vec<Allowed> {
    let mut out: Vec<Allowed> = Vec::new();
    for file in [".claude/settings.local.json", ".claude/settings.json"] {
        let Some(real) = in_checkout(top, Path::new(file)) else { continue };
        let Some(v) = std::fs::read_to_string(&real).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        let rules = v.pointer("/permissions/allow").and_then(|a| a.as_array()).into_iter().flatten().filter_map(|r| r.as_str());
        for r in rules {
            let Ok(r) = rule(r) else { continue };
            if !gives(&r) && !out.iter().any(|a| a.rule == r) {
                out.push(Allowed { rule: r, file: file.to_string() });
            }
        }
    }
    out
}

/// Whether stacked gives what `rule` lets use, as it wants it given, or
/// would not let it: a tool every ask is given (Read, Grep…), or a command
/// of a program `gives_program` says.
fn gives(rule: &str) -> bool {
    let (tool, inner) = match rule.split_once('(') {
        Some((tool, rest)) => (tool, rest.strip_suffix(')')),
        None => (rule, None),
    };
    GIVEN.contains(&tool) || (tool == "Bash" && inner.and_then(|i| i.split([' ', ':']).next()).is_some_and(gives_program))
}

/// Whether stacked gives program `w` already, as it wants it given, or would
/// not let it run as one of its own: git, stacked itself, the commands that
/// read, a shell or its builtins, what stacked refuses
/// (`claude::refused_tools`).
fn gives_program(w: &str) -> bool {
    let refused = crate::claude::refused_tools();
    w == "git"
        || w == "stacked"
        || crate::claude::READ_COMMANDS.contains(&w)
        || SHELL.contains(&w)
        || refused.split(',').any(|r| r.strip_prefix("Bash(").and_then(|r| r.split_whitespace().next()) == Some(w))
}

/// A program the checkout's instructions to the agent name in code, which
/// the setup lists, unticked: the rule that lets it run; the file that
/// names it first, by its path from the top of the checkout; and whether it
/// is there to run, on PATH or as a file of the checkout.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub rule: String,
    pub file: String,
    pub found: bool,
}

impl Named {
    /// What the setup says of it: where it is named, and that it is not on
    /// PATH, if it is not.
    fn said(&self) -> String {
        match self.found {
            true => format!("named in {}", self.file),
            false => format!("named in {}, not found on PATH", self.file),
        }
    }
}

/// The files at the top of the checkout `top` that tell the agent how to
/// work there, which claude reads into every ask: CLAUDE.md,
/// .claude/CLAUDE.md, AGENTS.md and AGENT.md, and what a CLAUDE.md imports
/// (`@path`, from its folder), one level down. Only files of the checkout
/// are read: one an import or a link takes out of it (a home's, as
/// `@~/.claude/…`) is not. Each by its path from the top, with what it holds.
fn instructions(top: &Path) -> Vec<(String, String)> {
    let Ok(root) = top.canonicalize() else { return vec![] };
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    let mut read = |real: PathBuf, out: &mut Vec<(String, String)>| {
        if seen.contains(&real) {
            return None;
        }
        let text = std::fs::read_to_string(&real).ok()?;
        seen.push(real.clone());
        out.push((real.strip_prefix(&root).unwrap_or(&real).display().to_string(), text.clone()));
        Some(text)
    };
    for name in ["CLAUDE.md", ".claude/CLAUDE.md", "AGENTS.md", "AGENT.md"] {
        let Some(real) = in_checkout(&root, Path::new(name)) else { continue };
        let folder = real.parent().map_or(root.clone(), Path::to_path_buf);
        let Some(text) = read(real, &mut out) else { continue };
        if !name.ends_with("CLAUDE.md") {
            continue;
        }
        for path in imports(&text).iter().filter(|p| !p.starts_with('~')) {
            if let Some(real) = in_checkout(&root, &folder.join(path)) {
                read(real, &mut out);
            }
        }
    }
    out
}

/// What a CLAUDE.md, `text`, imports, as claude reads `@path` in it: each
/// path as it is written, a word of the prose starting with @; none in code.
fn imports(text: &str) -> Vec<String> {
    let (prose, _) = scan(text);
    prose
        .iter()
        .flat_map(|l| l.split_whitespace())
        .filter_map(|w| w.strip_prefix('@'))
        .map(|p| p.trim_end_matches(['.', ',', ';', ':', ')']).to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// What markdown `text` says: its prose, a line each, its code spans taken
/// out; and its code, in order: each span (none for its language), each
/// line of a block, with the language its fence names (empty for none).
/// A line that goes on from the one before it (`\`) is left out.
fn scan(text: &str) -> (Vec<String>, Vec<(Option<String>, String)>) {
    let (mut prose, mut code) = (Vec::new(), Vec::new());
    // The block open: its fence's character and length, its language, and
    // whether its last line went on to the next.
    let mut fence: Option<(char, usize, String, bool)> = None;
    for line in text.lines() {
        let t = line.trim_start();
        let indented = line.len() - t.len() >= 4;
        let run = |c: char| t.chars().take_while(|&x| x == c).count();
        if let Some((c, n, lang, cont)) = &mut fence {
            if !indented && run(*c) >= *n && t.trim_start_matches(*c).trim().is_empty() {
                fence = None;
            } else if !std::mem::replace(cont, t.trim_end().ends_with('\\')) {
                code.push((Some(lang.clone()), line.to_string()));
            }
            continue;
        }
        let (ticks, tildes) = (run('`'), run('~'));
        if !indented && (ticks >= 3 || tildes >= 3) {
            let (c, n) = if ticks >= 3 { ('`', ticks) } else { ('~', tildes) };
            let lang = t[n..].split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            fence = Some((c, n, lang, false));
            continue;
        }
        let (said, spans) = spans_of(line);
        prose.push(said);
        code.extend(spans.into_iter().map(|s| (None, s)));
    }
    (prose, code)
}

/// `line` of prose, its code spans (each between runs of as many
/// backticks) taken out; and those spans.
fn spans_of(line: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let (mut said, mut spans) = (String::new(), Vec::new());
    let ticks = |at: usize| chars[at..].iter().take_while(|&&c| c == '`').count();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            said.push(chars[i]);
            i += 1;
            continue;
        }
        let n = ticks(i);
        let mut j = i + n;
        let close = loop {
            match chars.get(j) {
                None => break None,
                Some('`') if ticks(j) == n => break Some(j),
                Some('`') => j += ticks(j),
                Some(_) => j += 1,
            }
        };
        match close {
            Some(j) => {
                spans.push(chars[i + n..j].iter().collect::<String>().trim().to_string());
                i = j + n;
            }
            None => {
                said.extend(&chars[i..i + n]);
                i += n;
            }
        }
    }
    (said, spans)
}

/// The program a command line runs: its first word, past a prompt (`$ `)
/// and what the line sets first (`A=b`). None if it has none.
fn program_of(line: &str) -> Option<&str> {
    let t = line.trim();
    let t = t.strip_prefix("$ ").or_else(|| t.strip_prefix("% ")).unwrap_or(t);
    let sets = |w: &str| {
        w.split_once('=')
            .is_some_and(|(k, _)| k.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    };
    t.split_whitespace().find(|w| !sets(w))
}

/// Shells and their builtins, and what runs another program: none a program
/// of its own to let run.
const SHELL: [&str; 44] = [
    "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "cd", "echo", "export", "source", ".", "set", "unset", "alias", "eval",
    "exec", "exit", "pwd", "pushd", "popd", "true", "false", "test", "printf", "read", "type", "command", "builtin", "env", "time",
    "nohup", "xargs", "sudo", "if", "then", "else", "fi", "for", "while", "do", "done", "case",
];

/// The programs the agent's instruction files in the checkout `top`
/// (`instructions`) name in code, for the setup to list: the first word of
/// a code span, or of a command of a block of shell (a line of it, past its
/// prompt and what it sets first, a pipe's later parts not read), or of an
/// unmarked block. Kept, each once: those that are there to run, on `path`
/// or as a file of the checkout (`./tools/lint`); and a block of shell's
/// that are not on `path`, said so. Left out: what stacked gives already
/// (git, grep, rg, cat…, and itself), shells and their builtins (cd, echo,
/// export…), what stacked refuses (`claude::refused_tools`), and a path that
/// names no file of the checkout that may be run.
pub fn named(top: &Path, path: &OsStr) -> Vec<Named> {
    let word = |w: &str| {
        !w.starts_with('-')
            && !w.ends_with('/')
            && w.chars().any(|c| c.is_ascii_alphabetic())
            && w.chars().all(|c| c.is_ascii_alphanumeric() || "._+-/".contains(c))
    };
    // Whether `w` is there to run: none if it is a path naming no file of
    // the checkout that may be run.
    let root = top.canonicalize().ok();
    let found = |w: &str| -> Option<bool> {
        use std::os::unix::fs::PermissionsExt;
        if !w.contains('/') {
            return Some(on_path(w, path));
        }
        let (root, real) = (root.as_ref()?, top.join(w).canonicalize().ok()?);
        let runs = real.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
        (real.starts_with(root) && runs).then_some(true)
    };
    let mut out: Vec<Named> = Vec::new();
    for (file, text) in instructions(top) {
        let (_, code) = scan(&text);
        for (lang, line) in code {
            // A block of shell is commands, a console's only past their
            // prompt; a span, or an unmarked block, may be anything.
            let (sure, prompted) = match lang.as_deref() {
                Some("sh" | "bash" | "zsh" | "shell") => (true, false),
                Some("console" | "shell-session" | "sh-session" | "terminal") => (true, true),
                Some("") | None => (false, false),
                Some(_) => continue,
            };
            let t = line.trim();
            if t.starts_with('#') || (prompted && !(t.starts_with("$ ") || t.starts_with("% "))) {
                continue;
            }
            let Some(w) = program_of(t).filter(|w| word(w) && !gives_program(w)) else { continue };
            let rule = format!("Bash({w} *)");
            if out.iter().any(|n| n.rule == rule) {
                continue;
            }
            match found(w) {
                Some(found) if found || sure => out.push(Named { rule, file: file.clone(), found }),
                _ => {}
            }
        }
    }
    out
}

/// Which asks a list is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Asks that change the stack.
    Change,
    /// Asks to answer only.
    Answer,
}

impl Kind {
    pub const BOTH: [Kind; 2] = [Kind::Change, Kind::Answer];

    /// Its asks, said at the start of a sentence.
    pub fn asks(self) -> &'static str {
        match self {
            Kind::Change => "Asks that change the stack",
            Kind::Answer => "Answer-only asks",
        }
    }

    /// Its asks, said within a sentence.
    fn named(self) -> &'static str {
        match self {
            Kind::Change => "asks that change the stack",
            Kind::Answer => "answer-only asks",
        }
    }

    /// Its list, as the settings name it.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Change => "agent.change_tools",
            Kind::Answer => "agent.answer_tools",
        }
    }

    /// What stacked always gives its asks, said a line each.
    fn given(self) -> &'static [&'static str] {
        match self {
            Kind::Change => &CHANGE_GIVEN,
            Kind::Answer => &ANSWER_GIVEN,
        }
    }

    /// Why what `rule` lets use is to think twice about for its asks, if it
    /// is: what reaches the network (Bash at large, the web's tools), for a
    /// change; what changes things (Bash at large, Edit, Write…), for an
    /// answer.
    fn warn(self, rule: &str) -> Option<&'static str> {
        let (tool, any) = (tool_of(rule), rule == "Bash");
        match self {
            Kind::Change if any || WEB.contains(&tool) => Some("it can reach the network: a change could touch a remote"),
            Kind::Answer if any || (tool != "Bash" && CHANGING.contains(&tool)) => Some("it changes things: an answer could too"),
            _ => None,
        }
    }
}

/// What the setup starts from: the tools ticked for each kind of ask; what
/// the settings let every ask use (agent.tools), ticked for both; what the
/// checkout's own claude settings allow, and the programs its instructions
/// name; and the version of the setup done, if one was.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Start {
    pub change: Vec<String>,
    pub answer: Vec<String>,
    pub every: Vec<String>,
    pub allowed: Vec<Allowed>,
    pub named: Vec<Named>,
    pub done: Option<usize>,
}

impl Start {
    /// What the setup starts from, the settings being `agent`, `set` saying
    /// which of their lists a file sets (change_tools's, then
    /// answer_tools's), for the checkout `top`, with PATH `path`. A list a
    /// file sets is ticked as it says. One no file sets is ticked as stacked
    /// has it: for changes, nothing; for answers, curl and jq, and what the
    /// checkout's own claude settings allow, as you allow it yourself, but
    /// what changes things. The programs the checkout's instructions name
    /// are ticked only as the settings say.
    pub fn new(agent: &Agent, set: (bool, bool), top: Option<&Path>, path: &OsStr) -> Start {
        let allowed = top.map(allowed).unwrap_or_default();
        let answer = match set.1 {
            true => agent.answer_tools.clone(),
            false => ANSWER_TOOLS
                .iter()
                .map(|t| t.to_string())
                .chain(allowed.iter().filter(|a| Kind::Answer.warn(&a.rule).is_none()).map(|a| a.rule.clone()))
                .collect(),
        };
        Start {
            change: if set.0 { agent.change_tools.clone() } else { Vec::new() },
            answer,
            every: agent.tools.clone(),
            named: top.map(|t| named(t, path)).unwrap_or_default(),
            allowed,
            done: agent.setup,
        }
    }
}

/// What saving the setup writes: each kind's list, and what is left of
/// agent.tools if that is to change (a tool of it taken out of one kind's
/// list is kept in the other's alone).
#[derive(Debug, Clone, PartialEq)]
pub struct Chosen {
    pub change: Vec<String>,
    pub answer: Vec<String>,
    pub every: Option<Vec<String>>,
}

impl Chosen {
    /// The settings saving writes, as `config::set_all` takes them: both
    /// lists, agent.tools if it changes, and the setup done, as this
    /// stacked's version of it.
    pub fn settings(&self) -> Vec<(&'static str, Vec<String>)> {
        let mut out = vec![(Kind::Change.key(), self.change.clone()), (Kind::Answer.key(), self.answer.clone())];
        if let Some(every) = &self.every {
            out.push(("agent.tools", every.clone()));
        }
        out.push(("agent.setup", vec![VERSION.to_string()]));
        out
    }

    /// What saving writes, said: each list as it is to be.
    pub fn said(&self) -> String {
        let mut out = format!("change_tools: {}; answer_tools: {}", listed(&self.change), listed(&self.answer));
        if let Some(every) = &self.every {
            out += &format!("; tools: {}", listed(every));
        }
        out
    }
}

/// What `tools` are said as: each, or none.
fn listed(tools: &[String]) -> String {
    if tools.is_empty() { "none".to_string() } else { tools.join(", ") }
}

/// A row of the screen.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Heading(String),
    /// What stacked gives a kind of ask, or what the settings give every
    /// ask, said: nothing to let or not.
    Given(String),
    /// An MCP server: all its tools let, now and to come, or not.
    Server { kind: Kind, name: String, all: bool },
    /// A tool, or a command: let or not; of a server, or not; what is said
    /// of it: a warning, or where it comes from.
    Tool {
        kind: Kind,
        name: String,
        server: Option<String>,
        on: bool,
        warn: Option<&'static str>,
        about: Option<String>,
    },
    /// Where a command or a tool is added to the kind's list, typed.
    Add(Kind),
}

impl Row {
    /// The kind of ask it lets use something, if it does.
    fn kind(&self) -> Option<Kind> {
        match self {
            Row::Server { kind, .. } | Row::Tool { kind, .. } | Row::Add(kind) => Some(*kind),
            Row::Heading(_) | Row::Given(_) => None,
        }
    }

    /// Whether the cursor can be on it.
    fn picks(&self) -> bool {
        self.kind().is_some()
    }
}

/// A command or a tool being typed, to add to `kind`'s list: why it cannot
/// be added as it is, if it cannot.
#[derive(Debug, Clone, PartialEq)]
pub struct Adding {
    pub kind: Kind,
    pub ed: Editor,
    pub wrong: Option<String>,
}

/// The screen: its rows, the one picked; what agent.tools lets every ask
/// use; what was chosen as it opened; the file it is saved in; where the
/// agent was asked, and what the setup is now, said; and what is open over
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
    rows: Vec<Row>,
    pub sel: usize,
    /// The first row drawn, and how many there was room for.
    top: usize,
    room: usize,
    every: Vec<String>,
    start: Chosen,
    to: String,
    said: String,
    now: String,
    /// Esc's menu, open, its item picked: as the comment editor's, once
    /// something changed.
    pub menu: Option<usize>,
    pub adding: Option<Adding>,
}

/// What a key comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Stay,
    /// Saved: what is to be written.
    Save(Chosen),
    /// Left, nothing written.
    Leave,
}

/// What an item of Esc's menu does: what is chosen saved, or thrown away,
/// or the boxes picked on; a line between them.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Choice {
    Save,
    Throw,
    Keep,
    Line,
}

/// Esc's menu, as the comment editor's: saving first, throwing away under
/// a line, then going on.
const MENU: [(&str, Choice); 4] = [("Save it", Choice::Save), ("", Choice::Line), ("Throw it away", Choice::Throw), ("Keep choosing", Choice::Keep)];

/// The MCP server tool `t` is of, if it is one's (`mcp__NAME__TOOL`).
fn server_of(t: &str) -> Option<String> {
    t.strip_prefix("mcp__").and_then(|r| r.split_once("__")).map(|(s, _)| s.to_string())
}

impl Setup {
    /// The screen for the agent's `tools` and MCP `servers`, each kind's
    /// boxes as `start` ticks them, to be saved in the file `to`; `said`,
    /// where the agent was asked. Each kind has its own rows: what stacked
    /// gives it, said; for answers, curl and jq, which stacked let them run
    /// before there were lists; what the checkout's own claude settings
    /// allow; the programs its instructions name (but one a rule allowed
    /// lets run whole already); each MCP server and its tools; the agent's
    /// other tools; what the settings let that is none of those (a command,
    /// a server not there now), to keep or take out; and a row to add one
    /// more.
    pub fn new(tools: &[String], servers: &[String], start: &Start, to: &str, said: &str) -> Setup {
        let mut names: Vec<String> = servers.to_vec();
        for t in tools {
            if let Some(s) = server_of(t)
                && !names.contains(&s)
            {
                names.push(s);
            }
        }
        let mut rows = Vec::new();
        if !start.every.is_empty() {
            rows.push(Row::Heading("Every ask, builds and tests too: agent.tools".into()));
            rows.push(Row::Given(format!(
                "{}: ticked in both lists below; taken out of one, a tool is kept in the other's alone",
                start.every.join(", ")
            )));
        }
        for kind in Kind::BOTH {
            let ticked = match kind {
                Kind::Change => &start.change,
                Kind::Answer => &start.answer,
            };
            let on = |t: &str| ticked.iter().chain(&start.every).any(|x| x == t);
            let from = rows.len();
            // Whether the kind's rows have one for `t` already.
            let known = |rows: &[Row], t: &str| {
                rows[from..].iter().any(|r| match r {
                    Row::Server { name, .. } => t == format!("mcp__{name}__*"),
                    Row::Tool { name, .. } => name == t,
                    _ => false,
                })
            };
            rows.push(Row::Heading(format!("{}: {}", kind.asks(), kind.key())));
            rows.extend(kind.given().iter().map(|g| Row::Given(format!("always given: {g}"))));
            if kind == Kind::Answer {
                for t in ANSWER_TOOLS {
                    rows.push(Row::Tool {
                        kind,
                        name: t.to_string(),
                        server: None,
                        on: on(t),
                        warn: None,
                        about: Some("stacked's default for answers".into()),
                    });
                }
            }
            for a in &start.allowed {
                if !known(&rows, &a.rule) {
                    rows.push(Row::Tool {
                        kind,
                        name: a.rule.clone(),
                        server: None,
                        on: on(&a.rule),
                        warn: kind.warn(&a.rule),
                        about: Some(format!("allowed in {}", a.file)),
                    });
                }
            }
            for n in &start.named {
                let program = n.rule.strip_prefix("Bash(").and_then(|r| r.strip_suffix(" *)")).unwrap_or_default();
                let whole = start.allowed.iter().any(|a| a.rule == format!("Bash({program}:*)"));
                if !whole && !known(&rows, &n.rule) {
                    rows.push(Row::Tool {
                        kind,
                        name: n.rule.clone(),
                        server: None,
                        on: on(&n.rule),
                        warn: None,
                        about: Some(n.said()),
                    });
                }
            }
            for s in &names {
                let all = on(&format!("mcp__{s}__*"));
                rows.push(Row::Server { kind, name: s.clone(), all });
                for t in tools.iter().filter(|t| server_of(t).as_deref() == Some(s.as_str())) {
                    rows.push(Row::Tool {
                        kind,
                        name: t.clone(),
                        server: Some(s.clone()),
                        on: all || on(t),
                        warn: None,
                        about: None,
                    });
                }
            }
            for t in tools.iter().filter(|t| server_of(t).is_none() && !GIVEN.contains(&t.as_str())) {
                rows.push(Row::Tool {
                    kind,
                    name: t.clone(),
                    server: None,
                    on: on(t),
                    warn: kind.warn(t),
                    about: None,
                });
            }
            // What the settings let that is none of those: kept, unless taken
            // out.
            for t in ticked.iter().chain(&start.every) {
                if !known(&rows, t) {
                    rows.push(Row::Tool {
                        kind,
                        warn: kind.warn(t),
                        name: t.clone(),
                        server: None,
                        on: true,
                        about: None,
                    });
                }
            }
            rows.push(Row::Add(kind));
        }
        let sel = rows.iter().position(Row::picks).unwrap_or(0);
        let now = match start.done.unwrap_or(0) {
            n if n >= VERSION => "Ticked as the settings say now. Saving writes both lists again.",
            0 => "Not set up yet: ticked as stacked has it, and as the settings say. Saving it lets the asks go.",
            _ => "An earlier stacked's setup was done: ticked as the settings say. Saving it lets the asks go.",
        };
        let mut s = Setup {
            rows,
            sel,
            top: 0,
            room: 0,
            every: start.every.clone(),
            start: Chosen { change: vec![], answer: vec![], every: None },
            to: to.to_string(),
            said: said.to_string(),
            now: now.to_string(),
            menu: None,
            adding: None,
        };
        s.start = s.chosen();
        s
    }

    /// Whether what is chosen is not what was as the screen opened.
    fn changed(&self) -> bool {
        self.chosen() != self.start
    }

    /// Whether all of `server`'s tools are let `kind`'s asks.
    fn all(&self, kind: Kind, server: &str) -> bool {
        self.rows.iter().any(|r| matches!(r, Row::Server { kind: k, name, all: true } if *k == kind && name == server))
    }

    /// What `kind`'s asks are let use, as ticked: each server let whole, as
    /// `mcp__NAME__*`, and each tool let.
    fn ticked(&self, kind: Kind) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for r in &self.rows {
            let t = match r {
                Row::Server { kind: k, name, all: true } if *k == kind => format!("mcp__{name}__*"),
                Row::Tool { kind: k, name, server, on: true, .. } if *k == kind && !server.as_deref().is_some_and(|s| self.all(kind, s)) => name.clone(),
                _ => continue,
            };
            if seen.insert(t.clone()) {
                out.push(t);
            }
        }
        out
    }

    /// What saving is to write: each kind's list, what agent.tools lets
    /// every ask use left out of both while both still let it; what is left
    /// of agent.tools, if a tool of it is no longer let one kind's asks.
    pub fn chosen(&self) -> Chosen {
        let (change, answer) = (self.ticked(Kind::Change), self.ticked(Kind::Answer));
        let kept: Vec<String> = self.every.iter().filter(|t| change.contains(t) && answer.contains(t)).cloned().collect();
        Chosen {
            change: change.into_iter().filter(|t| !kept.contains(t)).collect(),
            answer: answer.into_iter().filter(|t| !kept.contains(t)).collect(),
            every: (kept != self.every).then_some(kept),
        }
    }

    /// The row picked let, or not. A server: all its tools, or none. One
    /// tool of a server let whole: the server no longer is, its other tools
    /// still are. The row to add one: the line to type it in, open.
    fn toggle(&mut self) {
        match self.rows.get(self.sel).cloned() {
            Some(Row::Server { kind, name, all }) => {
                for r in &mut self.rows {
                    match r {
                        Row::Server { kind: k, name: n, all: a } if *k == kind && *n == name => *a = !all,
                        Row::Tool { kind: k, server: Some(s), on, .. } if *k == kind && *s == name => *on = !all,
                        _ => {}
                    }
                }
            }
            Some(Row::Tool { kind, server, on, .. }) => {
                if let Some(s) = server {
                    for r in &mut self.rows {
                        if let Row::Server { kind: k, name, all } = r
                            && *k == kind
                            && *name == s
                        {
                            *all = false;
                        }
                    }
                }
                if let Some(Row::Tool { on: o, .. }) = self.rows.get_mut(self.sel) {
                    *o = !on;
                }
            }
            Some(Row::Add(kind)) => self.open_adding(kind),
            _ => {}
        }
    }

    /// The line to type a command or a tool in, for `kind`'s list, open.
    fn open_adding(&mut self, kind: Kind) {
        self.adding = Some(Adding { kind, ed: Editor::new("").single(), wrong: rule("").err() });
    }

    /// `rule` let `kind`'s asks, the cursor on its row: ticked, if the list
    /// has it already; else a row of its own, last.
    pub fn add(&mut self, kind: Kind, rule: &str) {
        let at = self.rows.iter().position(|r| match r {
            Row::Tool { kind: k, name, .. } => *k == kind && name == rule,
            Row::Server { kind: k, name, .. } => *k == kind && rule == format!("mcp__{name}__*"),
            _ => false,
        });
        let at = match at {
            Some(at) => at,
            None => {
                let at = self.rows.iter().position(|r| *r == Row::Add(kind)).unwrap_or(self.rows.len());
                let row = Row::Tool { kind, name: rule.to_string(), server: None, on: false, warn: kind.warn(rule), about: None };
                self.rows.insert(at, row);
                at
            }
        };
        self.sel = at;
        let off = match &self.rows[at] {
            Row::Server { all, .. } => !all,
            Row::Tool { on, .. } => !on,
            _ => false,
        };
        if off {
            self.toggle();
        }
    }

    /// The next row that can be picked, down or up.
    fn step(&mut self, down: bool, by: usize) {
        for _ in 0..by {
            let mut k = self.sel;
            loop {
                let next = if down { k + 1 } else { k.wrapping_sub(1) };
                match self.rows.get(next) {
                    Some(r) if !r.picks() => k = next,
                    Some(_) => {
                        self.sel = next;
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    /// The kind of ask of the row picked.
    fn kind_here(&self) -> Kind {
        self.rows.get(self.sel).and_then(Row::kind).unwrap_or(Kind::Change)
    }

    /// A key. Esc (or q), as in the comment editor: with something changed,
    /// a menu, to save it, throw it away or keep choosing; with nothing,
    /// out. `a` (or Enter on the row for it) opens a line to type a command
    /// or a tool in, for the list of the row picked: Enter adds it, once it
    /// reads as claude's permission rules do; Esc goes back. `s` saves,
    /// changed or not: saving is the setup done. Ctrl-c is out, whatever is
    /// open.
    pub fn key(&mut self, k: KeyEvent) -> Step {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Step::Leave;
        }
        if let Some(mut a) = self.adding.take() {
            match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => match rule(&a.ed.text()) {
                    Ok(r) => self.add(a.kind, &r),
                    Err(why) => {
                        a.wrong = Some(why);
                        self.adding = Some(a);
                    }
                },
                _ => {
                    a.ed.key(k);
                    a.wrong = rule(&a.ed.text()).err();
                    self.adding = Some(a);
                }
            }
            return Step::Stay;
        }
        if let Some(at) = self.menu {
            // ↑↓ past the line between its items.
            let picks: Vec<usize> = (0..MENU.len()).filter(|&k| MENU[k].1 != Choice::Line).collect();
            let here = picks.iter().position(|&k| k == at).unwrap_or(0);
            match k.code {
                KeyCode::Up => self.menu = Some(picks[here.saturating_sub(1)]),
                KeyCode::Down => self.menu = Some(picks[(here + 1).min(picks.len() - 1)]),
                KeyCode::Esc => self.menu = None,
                KeyCode::Enter => match MENU[at].1 {
                    Choice::Save => return Step::Save(self.chosen()),
                    Choice::Throw => return Step::Leave,
                    Choice::Keep => self.menu = None,
                    Choice::Line => {}
                },
                _ => {}
            }
            return Step::Stay;
        }
        match k.code {
            KeyCode::Up => self.step(false, 1),
            KeyCode::Down => self.step(true, 1),
            KeyCode::PageUp => self.step(false, 10),
            KeyCode::PageDown => self.step(true, 10),
            KeyCode::Home => self.step(false, self.rows.len()),
            KeyCode::End => self.step(true, self.rows.len()),
            KeyCode::Char(' ') | KeyCode::Enter => self.toggle(),
            KeyCode::Char('a') => self.open_adding(self.kind_here()),
            KeyCode::Char('s') => return Step::Save(self.chosen()),
            KeyCode::Esc | KeyCode::Char('q') if self.changed() => self.menu = Some(0),
            KeyCode::Esc | KeyCode::Char('q') => return Step::Leave,
            _ => {}
        }
        Step::Stay
    }

    /// The row drawn on line `y` of the screen, as `draw` drew it last, if
    /// one that can be picked is there.
    pub fn row_at(&self, y: usize) -> Option<usize> {
        let k = y.checked_sub(2).filter(|&k| k < self.room)? + self.top;
        self.rows.get(k).filter(|r| r.picks()).map(|_| k)
    }

    /// Row `k` picked, as the arrows would, if it can be.
    pub fn pick(&mut self, k: usize) {
        if self.rows.get(k).is_some_and(Row::picks) {
            self.sel = k;
        }
    }

    /// The screen, in `area`: what it is for, and where the agent was asked;
    /// what the setup is now; a row each; what saving writes, and what the
    /// keys do; over it, Esc's menu, or the line a command is typed in.
    /// Where the terminal's cursor goes, while a command is typed.
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let (w, h) = (area.width as usize, area.height as usize);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].reset();
            }
        }
        if h < 5 {
            return None;
        }
        let head = format!(" What each kind of ask may use, besides what stacked always gives it: {}", self.said);
        put(buf, area, 0, 0, w, &fit(&head, w), color::header());
        put(buf, area, 0, 1, w, &fit(&format!(" {}", self.now), w), color::dim());
        let st = if self.changed() { color::note() } else { color::dim() };
        put(buf, area, 0, h - 2, w, &fit(&format!(" To be, in {}: {}", self.to, self.chosen().said()), w), st);
        let room = h - 4;
        self.room = room;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + room {
            self.top = self.sel + 1 - room;
        }
        for (y, (i, r)) in self.rows.iter().enumerate().skip(self.top).take(room).enumerate() {
            let mark = |on: bool| if on { "[x]" } else { "[ ]" };
            let (text, mut st) = match r {
                Row::Heading(t) => (format!(" {t}"), color::file()),
                Row::Given(t) => (format!("   {t}"), color::dim()),
                Row::Server { name, all, .. } => (
                    format!("   {} all of the MCP server {name}'s tools, those to come too (mcp__{name}__*)", mark(*all)),
                    color::file(),
                ),
                Row::Tool { kind, name, server: Some(s), on, .. } => {
                    let short = name.strip_prefix(&format!("mcp__{s}__")).unwrap_or(name);
                    let st = if self.all(*kind, s) { color::dim() } else { Style::new() };
                    (format!("       {} {short}", mark(*on)), st)
                }
                Row::Tool { name, on, warn, about, .. } => {
                    let said: Vec<&str> = about.as_deref().into_iter().chain(*warn).collect();
                    let said = if said.is_empty() { String::new() } else { format!("   {}", said.join("; ")) };
                    let st = if warn.is_some() { color::note() } else { Style::new() };
                    (format!("   {} {name}{said}", mark(*on)), st)
                }
                Row::Add(_) => ("   + add a command or a tool…".to_string(), color::dim()),
            };
            if i == self.sel {
                st = on_cursor(st);
            }
            put(buf, area, 0, y + 2, w, &fit(&text, w), st);
        }
        // What the keys do, as the comment editor says it; typing a command,
        // why Enter does not take it, if it does not.
        let esc = if self.changed() { "esc: save or throw away" } else { "esc: close" };
        let keys = match (&self.adding, self.menu, self.rows.get(self.sel)) {
            (Some(Adding { wrong: Some(why), .. }), ..) => format!(" {why} · esc: back"),
            (Some(_), ..) => " enter: add it · esc: back".to_string(),
            (None, Some(_), _) => " ↑↓ choose · enter: do it · esc: keep choosing".to_string(),
            (None, None, Some(Row::Add(_))) => format!(" ↑↓: pick · enter: add a command or a tool · s: save · {esc}"),
            (None, None, _) => format!(" ↑↓: pick · space: let it or not · a: add a command or a tool · s: save · {esc}"),
        };
        put(buf, area, 0, h - 1, w, &fit(&keys, w), color::status());
        if let Some(at) = self.menu {
            self.draw_menu(buf, area, at);
        }
        self.adding.as_ref().map(|a| {
            let bw = (w * 9 / 10).clamp(30.min(w), 100.min(w));
            let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(3) / 2);
            let title = format!("a command or a tool for {}, as claude writes it: Bash(PROGRAM *), WebFetch, mcp__SERVER__TOOL", a.kind.named());
            let foot = if a.wrong.is_some() { "esc: back" } else { "enter: add it · esc: back" };
            a.ed.draw(buf, area, x, y, bw, 3, &title, foot)
        })
    }

    /// Esc's menu, in a box over the screen, item `at` picked.
    fn draw_menu(&self, buf: &mut Buffer, area: Rect, at: usize) {
        let (w, h) = (area.width as usize, area.height as usize);
        let title = format!("the setup, in {}", self.to);
        let widest = MENU.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        let bw = (widest.max(title.chars().count() + 2).max(24) + 4).min(w.saturating_sub(2));
        let bh = MENU.len() + 2;
        let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(bh) / 2);
        window(buf, area, x, y, bw, bh, &title, color::meta());
        for (r, (label, what)) in MENU.iter().enumerate() {
            if *what == Choice::Line {
                put(buf, area, x + 1, y + 1 + r, bw - 2, &"─".repeat(bw - 2), color::meta());
                continue;
            }
            let st = if r == at { on_cursor(Style::new()) } else { Style::new() };
            put(buf, area, x + 1, y + 1 + r, bw - 2, &fit(&format!(" {label}"), bw - 2), st);
        }
    }
}

/// The screen, run on the terminal until it is saved or left: what to
/// write, if it is saved.
pub fn run(mut s: Setup) -> Result<Option<Chosen>> {
    let io = |e: std::io::Error| Error(e.to_string());
    let mut terminal = ratatui::init();
    let out = (|| loop {
        terminal
            .draw(|f| {
                let area = f.area();
                if let Some(at) = s.draw(f.buffer_mut(), area) {
                    f.set_cursor_position(at);
                }
            })
            .map_err(io)?;
        let Event::Key(k) = event::read().map_err(io)? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        match s.key(k) {
            Step::Stay => {}
            Step::Save(c) => return Ok(Some(c)),
            Step::Leave => return Ok(None),
        }
    })();
    ratatui::restore();
    out
}

/// The setup asked line by line: `ask` says what it asks and reads the
/// answer (a line, as typed), `say` says the rest. For each kind of ask:
/// what stacked always gives it, and what is ticked, to keep as it is or to
/// go through one by one, Enter keeping each as it was; then any command or
/// tool to add, until a line is left empty. What is to be saved.
pub fn ask_lines(s: &mut Setup, ask: &mut dyn FnMut(&str) -> String, say: &mut dyn FnMut(&str)) -> Chosen {
    let yes = |a: &str, was: bool| match a.to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => was,
    };
    let yn = |on: bool| if on { "[Y/n]" } else { "[y/N]" };
    for kind in Kind::BOTH {
        say(&format!("{}, {}:", kind.asks(), kind.key()));
        for g in kind.given() {
            say(&format!("  Always given: {g}."));
        }
        say(&format!("  Ticked: {}.", listed(&s.ticked(kind))));
        if matches!(ask("Keep them as they are? [Y/n: one by one]").to_ascii_lowercase().as_str(), "n" | "no") {
            for k in 0..s.rows.len() {
                match s.rows[k].clone() {
                    Row::Server { kind: rk, name, all } if rk == kind => {
                        let its: Vec<(usize, String, bool)> = s
                            .rows
                            .iter()
                            .enumerate()
                            .filter_map(|(i, r)| match r {
                                Row::Tool { kind: tk, server: Some(sv), name: t, on, .. } if *tk == kind && *sv == name => {
                                    Some((i, t.strip_prefix(&format!("mcp__{name}__")).unwrap_or(t).to_string(), *on))
                                }
                                _ => None,
                            })
                            .collect();
                        let short: Vec<&str> = its.iter().map(|(_, t, _)| t.as_str()).collect();
                        say(&format!("  The MCP server {name}: {}.", if short.is_empty() { "no tools".into() } else { short.join(", ") }));
                        let options = match (all, its.iter().any(|(_, _, on)| *on)) {
                            (true, _) => "[Y/n/e: each]",
                            (false, true) => "[y/n/E: each]",
                            (false, false) => "[y/N/e: each]",
                        };
                        let a = ask(&format!("  Let {} use all of its tools, those to come too (mcp__{name}__*)? {options}", kind.named()));
                        let set_all = |s: &mut Setup, to: bool| {
                            for r in &mut s.rows {
                                match r {
                                    Row::Server { kind: sk, name: n, all } if *sk == kind && *n == name => *all = to,
                                    Row::Tool { kind: tk, server: Some(sv), on, .. } if *tk == kind && *sv == name => *on = to,
                                    _ => {}
                                }
                            }
                        };
                        match a.to_ascii_lowercase().as_str() {
                            "y" | "yes" => set_all(s, true),
                            "n" | "no" => set_all(s, false),
                            "e" | "each" => {
                                set_all(s, false);
                                for (i, t, on) in its {
                                    let a = ask(&format!("    {t}? {}", yn(on || all)));
                                    if let Row::Tool { on: o, .. } = &mut s.rows[i] {
                                        *o = yes(&a, on || all);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Row::Tool { kind: rk, server: None, name, on, warn, about } if rk == kind => {
                        // Where it comes from, said with what it names; a
                        // warning, a sentence after the question.
                        let from = about.map_or(String::new(), |a| format!(", {a}"));
                        let warned = warn.map_or(String::new(), |w| {
                            let mut c = w.chars();
                            c.next().map_or(String::new(), |f| format!(" {}{}.", f.to_uppercase(), c.as_str()))
                        });
                        let a = ask(&format!("  Let {} use {name}{from}?{warned} {}", kind.named(), yn(on)));
                        if let Row::Tool { on: o, .. } = &mut s.rows[k] {
                            *o = yes(&a, on);
                        }
                    }
                    _ => {}
                }
            }
        }
        loop {
            let a = ask("  Add a command or a tool, as claude writes a permission rule (Bash(PROGRAM *), WebFetch, mcp__SERVER__TOOL), or press Enter for none:");
            if a.trim().is_empty() {
                break;
            }
            match rule(&a) {
                Ok(r) => {
                    s.add(kind, &r);
                    say(&format!("  {r}: added."));
                }
                Err(why) => say(&format!("  {why}.")),
            }
        }
    }
    s.chosen()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(s: &mut Setup, code: KeyCode) -> Step {
        s.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn typed(s: &mut Setup, text: &str) {
        for c in text.chars() {
            press(s, KeyCode::Char(c));
        }
    }

    fn screen(s: &mut Setup) -> String {
        screen_of(s, 40)
    }

    fn screen_of(s: &mut Setup, h: u16) -> String {
        let area = Rect::new(0, 0, 130, h);
        let mut buf = Buffer::empty(area);
        s.draw(&mut buf, area);
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// An executable file at `at`, made with its folder.
    fn executable(at: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// What nothing set starts from, no checkout read.
    fn fresh() -> Start {
        Start::new(&Agent::default(), (false, false), None, OsStr::new(""))
    }

    /// Where `kind`'s row for `name` is.
    fn row_of(s: &Setup, kind: Kind, name: &str) -> usize {
        s.rows.iter().position(|r| matches!(r, Row::Tool { kind: k, name: n, .. } if *k == kind && n == name)).unwrap()
    }

    #[test]
    fn a_rule_reads_as_claude_writes_them() {
        for ok in ["WebFetch", "Bash(make check *)", "WebFetch(domain:example.com)", "mcp__moz__get_bug", "mcp__moz__*", "mcp__moz", "mcp__my-notes__read_it"] {
            assert_eq!(rule(ok).as_deref(), Ok(ok), "{ok}");
        }
        assert_eq!(rule("  Bash(jq *) ").as_deref(), Ok("Bash(jq *)"), "trimmed");
        let wrong = |t: &str| rule(t).unwrap_err();
        assert_eq!(wrong(""), "type a tool, as WebFetch, or a command, as Bash(PROGRAM *)");
        assert_eq!(wrong("lint"), "lint is not a tool's name: a command is let run as Bash(lint *)");
        assert_eq!(wrong("make check"), "make check is not a tool's name: a command is let run as Bash(make check)");
        assert_eq!(wrong("Bash(make check *"), "Bash(make check * has no ) to close what Bash may do");
        assert_eq!(wrong("Bash()"), "Bash() says nothing between its parentheses: Bash(PROGRAM *) lets a program run");
        assert_eq!(wrong("Bash (jq *)"), "Bash (jq *) is not a tool's name, as Bash or WebFetch, then what it may do between parentheses");
        assert!(wrong("Bash(jq *),Read").contains("holds a comma"), "{}", wrong("Bash(jq *),Read"));
        assert!(wrong("mcp__").contains("names no MCP server"), "{}", wrong("mcp__"));
        assert!(wrong("mcp__moz__get bug").starts_with("mcp__moz__get bug names no tool of moz"), "{}", wrong("mcp__moz__get bug"));
    }

    #[test]
    fn the_lists_start_as_stacked_has_them_the_settings_winning() {
        // Nothing set: no tool for changes; curl and jq for answers.
        let s = fresh();
        assert_eq!((s.change, s.answer, s.done), (vec![], strings(&["Bash(curl *)", "Bash(jq *)"]), None));
        // A list a file sets is as it says; agent.tools, every ask's, kept
        // apart.
        let agent = Agent {
            change_tools: strings(&["Bash(make check *)"]),
            answer_tools: strings(&["Bash(curl *)"]),
            tools: strings(&["mcp__moz__*"]),
            setup: Some(VERSION),
            ..Agent::default()
        };
        let s = Start::new(&agent, (true, true), None, OsStr::new(""));
        assert_eq!(
            (s.change, s.answer, s.every, s.done),
            (strings(&["Bash(make check *)"]), strings(&["Bash(curl *)"]), strings(&["mcp__moz__*"]), Some(VERSION))
        );
        // One a file does not set, as stacked has it: the change list, here.
        assert!(Start::new(&agent, (false, true), None, OsStr::new("")).change.is_empty());
        // What is on PATH: a file there that may be run.
        let tmp = tempfile::tempdir().unwrap();
        executable(&tmp.path().join("bin/lint"));
        std::fs::write(tmp.path().join("bin/notes"), "").unwrap();
        std::fs::create_dir(tmp.path().join("bin/dir")).unwrap();
        let path = std::env::join_paths([tmp.path().join("nowhere"), tmp.path().join("bin")]).unwrap();
        assert!(on_path("lint", &path));
        assert!(!on_path("notes", &path) && !on_path("dir", &path) && !on_path("lint", OsStr::new("")));
    }

    #[test]
    fn the_checkout_s_instructions_name_programs_to_suggest() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("repo");
        // A folder of the test's own for PATH, with one of the programs.
        executable(&tmp.path().join("bin/searchfox-cli"));
        let path = tmp.path().join("bin").into_os_string();
        // What is outside the checkout is never read: a file beside it, and
        // a home's.
        std::fs::write(tmp.path().join("outside.md"), "```sh\noutsider --all\n```\n").unwrap();
        std::fs::create_dir_all(top.join(".claude")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("outside.md"), top.join(".claude/CLAUDE.md")).unwrap();
        std::fs::write(
            top.join("CLAUDE.md"),
            "Read @AGENTS.md first, then @../outside.md and @~/.claude/notes.md.\n\nList with `ls -la`; never `@nothing` in code.\n",
        )
        .unwrap();
        executable(&top.join("tools/run.sh"));
        std::fs::write(top.join("src.rs"), "").unwrap();
        std::fs::write(
            top.join("AGENTS.md"),
            "# Working here\n\nSearch with `searchfox-cli -q 'fn main'`; a type, `Option<T>`, and a file, `src.rs`, name no program.\n\n\
             ```sh\n$ searchfox-cli --define Foo\nnotthere build\ncd src\necho done\ngrep -r TODO .\n# a comment\n\
             FOO=1 ./tools/run.sh --quick \\\n  missing --more\n```\n\n```rust\nfn main() { inrust(); }\n```\n\n\
             ```console\n$ ./tools/run.sh again\nprinted out\n```\n",
        )
        .unwrap();
        let found = named(&top, &path);
        assert_eq!(
            found,
            [
                Named { rule: "Bash(searchfox-cli *)".into(), file: "AGENTS.md".into(), found: true },
                Named { rule: "Bash(notthere *)".into(), file: "AGENTS.md".into(), found: false },
                Named { rule: "Bash(./tools/run.sh *)".into(), file: "AGENTS.md".into(), found: true },
            ]
        );
        // Each kind lists them, unticked, where they are named said; the
        // settings ticking one win.
        let agent = Agent { change_tools: strings(&["Bash(./tools/run.sh *)"]), ..Agent::default() };
        let start = Start::new(&agent, (true, false), Some(&top), &path);
        assert_eq!(start.named, found);
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        for (line, times) in [
            ("   [ ] Bash(searchfox-cli *)   named in AGENTS.md", 2),
            ("   [ ] Bash(notthere *)   named in AGENTS.md, not found on PATH", 2),
            ("   [x] Bash(./tools/run.sh *)   named in AGENTS.md", 1),
            ("   [ ] Bash(./tools/run.sh *)   named in AGENTS.md", 1),
        ] {
            assert_eq!(shown.lines().filter(|l| *l == line).count(), times, "{line:?} in:\n{shown}");
        }
        assert_eq!(s.chosen().change, ["Bash(./tools/run.sh *)"]);
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(jq *)"]);
        // A checkout with none of those files names nothing.
        assert!(named(tmp.path(), &path).is_empty());
    }

    #[test]
    fn what_the_checkout_s_own_claude_settings_allow_is_suggested() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("repo");
        std::fs::create_dir_all(top.join(".claude")).unwrap();
        // Yours, untracked, then the checkout's: a rule each, as written; a
        // tool stacked gives, and one it gives as it wants (git's), left out;
        // what they deny, left alone.
        std::fs::write(
            top.join(".claude/settings.local.json"),
            r#"{"permissions": {"allow": ["Bash(searchfox-cli:*)", "Bash(grep:*)", "Bash(git push:*)"], "deny": ["Bash(rm:*)"]}}"#,
        )
        .unwrap();
        std::fs::write(
            top.join(".claude/settings.json"),
            r#"{"permissions": {"allow": ["WebFetch(domain:example.com)", "Read", "Bash(searchfox-cli:*)", "Bash(a,b)"]}}"#,
        )
        .unwrap();
        // The instructions name the same program: once, as allowed.
        std::fs::write(top.join("AGENTS.md"), "```sh\nsearchfox-cli --define Foo\n```\n").unwrap();
        let found = allowed(&top);
        assert_eq!(
            found,
            [
                Allowed { rule: "Bash(searchfox-cli:*)".into(), file: ".claude/settings.local.json".into() },
                Allowed { rule: "WebFetch(domain:example.com)".into(), file: ".claude/settings.json".into() },
            ]
        );
        // Ticked for answers, as you allow them yourself; not for changes,
        // which reach no network unless ticked.
        let start = Start::new(&Agent::default(), (false, false), Some(&top), OsStr::new(""));
        assert_eq!(start.answer, ["Bash(curl *)", "Bash(jq *)", "Bash(searchfox-cli:*)", "WebFetch(domain:example.com)"]);
        assert!(start.change.is_empty());
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        for line in [
            "   [ ] Bash(searchfox-cli:*)   allowed in .claude/settings.local.json",
            "   [x] Bash(searchfox-cli:*)   allowed in .claude/settings.local.json",
            "   [ ] WebFetch(domain:example.com)   allowed in .claude/settings.json; it can reach the network: a change could touch a remote",
            "   [x] WebFetch(domain:example.com)   allowed in .claude/settings.json",
        ] {
            assert_eq!(shown.lines().filter(|l| *l == line).count(), 1, "{line:?} in:\n{shown}");
        }
        assert!(!shown.contains("Bash(searchfox-cli *)") && !shown.contains("grep:*") && !shown.contains("rm:*"), "{shown}");
        // What changes things is not ticked for answers, allowed or not.
        std::fs::write(top.join(".claude/settings.json"), r#"{"permissions": {"allow": ["Edit(src/**)", "Bash"]}}"#).unwrap();
        let start = Start::new(&Agent::default(), (false, false), Some(&top), OsStr::new(""));
        assert_eq!(start.answer, ["Bash(curl *)", "Bash(jq *)", "Bash(searchfox-cli:*)"]);
        // The settings setting the answers' list win.
        let agent = Agent { answer_tools: strings(&["Bash(curl *)"]), ..Agent::default() };
        assert_eq!(Start::new(&agent, (false, true), Some(&top), OsStr::new("")).answer, ["Bash(curl *)"]);
        // A link out of the checkout is not read.
        std::fs::write(tmp.path().join("theirs.json"), r#"{"permissions": {"allow": ["Bash(theirs:*)"]}}"#).unwrap();
        std::fs::remove_file(top.join(".claude/settings.local.json")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("theirs.json"), top.join(".claude/settings.local.json")).unwrap();
        assert!(!allowed(&top).iter().any(|a| a.rule.contains("theirs")), "{:?}", allowed(&top));
        // What stacked gives, as it wants it given: none to suggest.
        for given in ["Read", "Grep(src/**)", "Bash(rg:*)", "Bash(git log *)", "Bash(git push:*)", "Bash(cd *)"] {
            assert!(gives(given), "{given}");
        }
        for not in ["WebFetch", "Bash", "Bash(make:*)", "mcp__moz__get_bug"] {
            assert!(!gives(not), "{not}");
        }
    }

    #[test]
    fn each_kind_s_list_is_its_own_what_stacked_gives_it_said() {
        let tools = strings(&["Read", "Bash", "WebFetch", "mcp__moz__get_bug", "mcp__moz__get_revision", "mcp__notes__write"]);
        let servers = strings(&["moz", "notes"]);
        // As the settings were: notes' write let every ask; a command of the
        // change list's own, kept as it is.
        let start = Start {
            change: strings(&["Bash(make check *)"]),
            every: strings(&["mcp__notes__write"]),
            ..fresh()
        };
        let mut s = Setup::new(&tools, &servers, &start, "local.toml", "the agent's tools, in repo");
        let shown = screen(&mut s);
        for line in [
            " What each kind of ask may use, besides what stacked always gives it: the agent's tools, in repo",
            " Not set up yet: ticked as stacked has it, and as the settings say. Saving it lets the asks go.",
            " Every ask, builds and tests too: agent.tools",
            "   mcp__notes__write: ticked in both lists below; taken out of one, a tool is kept in the other's alone",
            " Asks that change the stack: agent.change_tools",
            "   always given: git's history commands (status, log, show, diff, add, commit, reset, rebase -i…), and git config to read",
            "   [ ] Bash   it can reach the network: a change could touch a remote",
            "   [x] Bash(make check *)",
            " Answer-only asks: agent.answer_tools",
            "   always given: Read, Grep, Glob, LS, and git show, log and diff",
            "   [x] Bash(curl *)   stacked's default for answers",
            "   [x] Bash(jq *)   stacked's default for answers",
            "   [ ] Bash   it changes things: an answer could too",
            "   [ ] all of the MCP server moz's tools, those to come too (mcp__moz__*)",
            "       [x] write",
            "   + add a command or a tool…",
            " To be, in local.toml: change_tools: Bash(make check *); answer_tools: Bash(curl *), Bash(jq *)",
        ] {
            assert!(shown.lines().any(|l| l == line), "{line:?} in:\n{shown}");
        }
        assert!(!shown.contains("[ ] Read"), "given already: {shown}");
        assert_eq!(shown.matches("Bash(curl *)").count(), 2, "for answers alone, and in what saving writes: {shown}");
        // The first box is the change list's moz, all of it at once.
        assert_eq!(press(&mut s, KeyCode::Char(' ')), Step::Stay);
        assert_eq!(s.chosen().change, ["mcp__moz__*", "Bash(make check *)"]);
        // One of them taken back: moz is no longer let whole.
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.chosen().change, ["mcp__moz__get_revision", "Bash(make check *)"]);
        // notes' write taken out of the change list: it leaves agent.tools,
        // and goes on for answers alone.
        s.pick(row_of(&s, Kind::Change, "mcp__notes__write"));
        press(&mut s, KeyCode::Enter);
        let c = s.chosen();
        assert_eq!(c.every, Some(vec![]));
        assert_eq!(c.answer, ["Bash(curl *)", "Bash(jq *)", "mcp__notes__write"]);
        assert!(c.said().ends_with("; tools: none"), "{}", c.said());
        // The make check of the changes taken out too, curl of the answers;
        // saved, the setup done.
        s.pick(row_of(&s, Kind::Change, "Bash(make check *)"));
        press(&mut s, KeyCode::Char(' '));
        s.pick(row_of(&s, Kind::Answer, "Bash(curl *)"));
        press(&mut s, KeyCode::Char(' '));
        let Step::Save(c) = press(&mut s, KeyCode::Char('s')) else { panic!() };
        assert_eq!(
            c.settings(),
            [
                ("agent.change_tools", strings(&["mcp__moz__get_revision"])),
                ("agent.answer_tools", strings(&["Bash(jq *)", "mcp__notes__write"])),
                ("agent.tools", vec![]),
                ("agent.setup", vec![VERSION.to_string()]),
            ]
        );
        assert_eq!(c.said(), "change_tools: mcp__moz__get_revision; answer_tools: Bash(jq *), mcp__notes__write; tools: none");
    }

    #[test]
    fn a_command_is_added_to_either_list_once_it_reads_as_a_rule() {
        let mut s = Setup::new(&strings(&["Bash"]), &[], &fresh(), "local.toml", "here");
        // On the change list: `a` opens the line, for it.
        assert_eq!(s.kind_here(), Kind::Change);
        press(&mut s, KeyCode::Char('a'));
        let shown = screen(&mut s);
        assert!(shown.contains("a command or a tool for asks that change the stack, as claude writes it"), "{shown}");
        assert!(shown.ends_with("\n type a tool, as WebFetch, or a command, as Bash(PROGRAM *) · esc: back"), "{shown}");
        // Why Enter does not take what is typed, said in full under it.
        typed(&mut s, "make check");
        let shown = screen(&mut s);
        assert!(shown.ends_with("\n make check is not a tool's name: a command is let run as Bash(make check) · esc: back"), "{shown}");
        press(&mut s, KeyCode::Enter);
        assert!(s.adding.is_some());
        for _ in 0.."make check".len() {
            press(&mut s, KeyCode::Backspace);
        }
        typed(&mut s, "Bash(make check *)");
        assert!(screen(&mut s).ends_with("\n enter: add it · esc: back"), "{}", screen(&mut s));
        press(&mut s, KeyCode::Enter);
        assert!(s.adding.is_none());
        assert_eq!(s.chosen().change, ["Bash(make check *)"]);
        assert!(screen(&mut s).contains("   [x] Bash(make check *)\n   + add a command or a tool…"), "{}", screen(&mut s));
        // On the answers' row for it, Enter opens it; one the list has
        // already is ticked, not listed twice.
        press(&mut s, KeyCode::End);
        assert_eq!(s.rows[s.sel], Row::Add(Kind::Answer));
        assert!(screen(&mut s).ends_with(" ↑↓: pick · enter: add a command or a tool · s: save · esc: save or throw away"), "{}", screen(&mut s));
        press(&mut s, KeyCode::Enter);
        typed(&mut s, "Bash(jq *)");
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(jq *)"]);
        press(&mut s, KeyCode::Char(' '));
        press(&mut s, KeyCode::Char('a'));
        typed(&mut s, "Bash(jq *)");
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(jq *)"], "ticked again");
        // Esc goes back, nothing added.
        press(&mut s, KeyCode::Char('a'));
        typed(&mut s, "WebFetch");
        press(&mut s, KeyCode::Esc);
        assert!(s.adding.is_none());
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(jq *)"]);
        // The cursor goes where it is drawn, in the line.
        press(&mut s, KeyCode::Char('a'));
        let area = Rect::new(0, 0, 130, 40);
        let mut buf = Buffer::empty(area);
        assert!(s.draw(&mut buf, area).is_some());
    }

    #[test]
    fn esc_is_a_menu_once_something_changed_as_in_the_comment_editor() {
        let tools = strings(&["mcp__moz__get_bug", "WebFetch"]);
        let open = || Setup::new(&tools, &[], &fresh(), "local.toml", "here");
        // Nothing changed: out, nothing to write.
        let mut s = open();
        assert!(screen(&mut s).ends_with("esc: close"), "{}", screen(&mut s));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Leave);
        // Saved as it is: the setup done all the same.
        let mut s = open();
        let Step::Save(c) = press(&mut s, KeyCode::Char('s')) else { panic!() };
        assert_eq!(c, Chosen { change: vec![], answer: strings(&["Bash(curl *)", "Bash(jq *)"]), every: None });
        assert_eq!(c.settings().last(), Some(&("agent.setup", vec![VERSION.to_string()])));
        // Changed: a menu, on saving it; ↑↓ past the line between its items.
        let mut s = open();
        press(&mut s, KeyCode::Char(' '));
        assert!(screen(&mut s).ends_with("esc: save or throw away"), "{}", screen(&mut s));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Stay);
        let shown = screen(&mut s);
        for item in ["the setup, in local.toml", " Save it", " Throw it away", " Keep choosing"] {
            assert!(shown.contains(item), "{item:?} in:\n{shown}");
        }
        assert!(shown.ends_with("↑↓ choose · enter: do it · esc: keep choosing"), "{shown}");
        assert_eq!(s.menu, Some(0));
        press(&mut s, KeyCode::Down);
        assert_eq!(s.menu, Some(2), "past the line");
        // Keep choosing, or Esc: back to the boxes, what was chosen kept.
        press(&mut s, KeyCode::Down);
        assert_eq!(press(&mut s, KeyCode::Enter), Step::Stay);
        assert_eq!((s.menu, s.chosen().change), (None, strings(&["mcp__moz__*"])));
        press(&mut s, KeyCode::Char('q'));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Stay);
        assert_eq!(s.menu, None);
        // Saved from it; thrown away from it, each the screen's end.
        press(&mut s, KeyCode::Esc);
        let Step::Save(c) = press(&mut s, KeyCode::Enter) else { panic!() };
        assert_eq!(c.change, ["mcp__moz__*"]);
        let changed = || {
            let mut s = open();
            press(&mut s, KeyCode::Char(' '));
            press(&mut s, KeyCode::Esc);
            s
        };
        let mut s = changed();
        press(&mut s, KeyCode::Down);
        assert_eq!(press(&mut s, KeyCode::Enter), Step::Leave);
        // Ctrl-c, out whatever is open.
        let mut s = changed();
        assert_eq!(s.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), Step::Leave);
    }

    #[test]
    fn a_server_let_whole_is_said_so_and_a_click_picks_a_row() {
        let tools = strings(&["mcp__moz__get_bug"]);
        let start = Start { answer: strings(&["mcp__moz__*"]), done: Some(VERSION), ..fresh() };
        let mut s = Setup::new(&tools, &[], &start, "settings.toml", "here");
        let shown = screen(&mut s);
        assert!(shown.contains(" Ticked as the settings say now. Saving writes both lists again."), "{shown}");
        let at = shown.lines().position(|l| l == "   [x] all of the MCP server moz's tools, those to come too (mcp__moz__*)").unwrap();
        assert_eq!(s.chosen().answer, ["mcp__moz__*"]);
        // The row drawn there, picked.
        let k = s.row_at(at).unwrap();
        s.pick(k);
        assert_eq!(s.sel, k);
        assert_eq!(s.row_at(0), None, "the top line is none");
        assert_eq!(s.row_at(39), None, "nor the keys' line");
        // Long, scrolled: the row picked always shows.
        press(&mut s, KeyCode::End);
        assert!(screen_of(&mut s, 8).contains("+ add a command or a tool…"));
    }

    #[test]
    fn the_setup_is_asked_line_by_line_too() {
        let tools = strings(&["Bash", "mcp__moz__get_bug", "mcp__moz__get_revision"]);
        let start = Start {
            every: strings(&["mcp__moz__get_bug"]),
            named: vec![Named { rule: "Bash(lint *)".into(), file: "AGENTS.md".into(), found: false }],
            ..fresh()
        };
        let mut s = Setup::new(&tools, &strings(&["moz"]), &start, "local.toml", "here");
        // Changes kept as they are, with a command added after a wrong one;
        // answers one by one: jq no longer, lint let, moz's each, its
        // revisions too, Bash not; Enter keeping the rest.
        let mut answers = vec!["", "make check", "Bash(make check *)", "", "n", "", "n", "y", "e", "", "y", "n", ""].into_iter();
        let (mut asked, mut said) = (Vec::new(), Vec::new());
        let c = ask_lines(
            &mut s,
            &mut |q| {
                asked.push(q.to_string());
                answers.next().unwrap_or_default().to_string()
            },
            &mut |l| said.push(l.to_string()),
        );
        assert!(said.contains(&"Asks that change the stack, agent.change_tools:".to_string()), "{said:?}");
        assert!(said.contains(&"  Ticked: mcp__moz__get_bug.".to_string()), "{said:?}");
        assert!(said.iter().any(|l| l == "  Always given: Read, Grep, Glob, LS, and git show, log and diff."), "{said:?}");
        assert!(said.iter().any(|l| l.starts_with("  make check is not a tool's name")), "{said:?}");
        assert!(said.contains(&"  Bash(make check *): added.".to_string()), "{said:?}");
        for q in [
            "  Let answer-only asks use Bash(jq *), stacked's default for answers? [Y/n]",
            "  Let answer-only asks use Bash(lint *), named in AGENTS.md, not found on PATH? [y/N]",
            "  Let answer-only asks use all of its tools, those to come too (mcp__moz__*)? [y/n/E: each]",
            "  Let answer-only asks use Bash? It changes things: an answer could too. [y/N]",
        ] {
            assert!(asked.contains(&q.to_string()), "{q:?} in {asked:#?}");
        }
        assert_eq!(c.change, ["Bash(make check *)"]);
        assert_eq!(c.answer, ["Bash(curl *)", "Bash(lint *)", "mcp__moz__get_revision"]);
        assert_eq!(c.every, None, "moz's get_bug is let both, as it was");
    }
}
