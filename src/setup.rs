//! `stacked setup agent`: what each kind of ask may use besides what
//! stacked always gives it, set up with you. Answer-only asks have a list
//! of their own, agent.answer_tools; asks that change the stack have
//! theirs, agent.change_tools; agent.tools, which every ask may use, goes on
//! as it was. The setup shows them as one list, a tool a row, with a box
//! for each kind of ask: `read only` for answers, `change` for changes.
//! Each column starts ticked as the settings say, else as stacked has it
//! (`Start`): nothing for changes, curl and jq for answers. The list holds
//! what the agent says it has (`ask::agent_tools`: its own tools, and its
//! MCP servers'); what the checkout's own claude settings allow
//! (`allowed`), ticked for answers, as you allow it yourself; what the
//! checkout's skills let run (`skilled`), and the programs its instructions
//! to the agent name (`named`), unticked, as stacked cannot know what they
//! do; and claude's tool for the skills themselves, with what it lets an
//! answer do. Rules that mean the same (`same`) are one row, which says
//! where each comes from and is written one way: a command's the newer way,
//! `Bash(PROGRAM *)`, however it was written (`shown`). What the checkout's
//! claude settings of yours deny (`denied`) is said, and refused to every
//! ask. A command or a tool may be added too, as claude writes a permission
//! rule (`rule`). Saving is the setup done: both lists are written, with
//! agent.setup, the version of the setup that was done, without which no
//! ask goes. On a screen of its own (`Setup`, for `--tui`, and in the
//! editor before its first ask), or asked line by line (`ask_lines`).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::ask::{CHANGING, GIVEN, GIVEN_SAID};
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

/// What `rule` lets use, written the same way for every rule that means
/// the same to claude, so that two such rules are told to be one. A
/// command's older prefix form, `Bash(PREFIX:*)`, which lets PREFIX run
/// alone or followed by a space and more, is written `Bash(PREFIX *)`, the
/// newer form, whose one `*`, at its end after a space, claude matches
/// alike; but not when PREFIX holds a `*` or a `\`, which the older form
/// reads as they are written and the newer does not, nor when it starts or
/// ends with a space. All of an MCP server's tools, `mcp__SERVER`, are
/// written `mcp__SERVER__*`. Any other rule is only the same as itself: a
/// wider rule (`Bash(./mach *)`) is never the same as a narrower one
/// (`Bash(./mach build *)`).
fn same(rule: &str) -> String {
    if let Some(prefix) = rule.strip_prefix("Bash(").and_then(|r| r.strip_suffix(":*)"))
        && !prefix.is_empty()
        && prefix.trim() == prefix
        && !prefix.contains(['*', '\\'])
    {
        return format!("Bash({prefix} *)");
    }
    match rule.strip_prefix("mcp__") {
        Some(server) if !server.contains("__") => format!("mcp__{server}__*"),
        _ => rule.to_string(),
    }
}

/// How the setup shows and writes `rule`, however its source writes it: a
/// command's older prefix form the newer way (`same`), which reads better
/// and which claude reads alike; anything else as it is written. All of an
/// MCP server's tools are written as their row has it (`Setup::new`).
fn shown(rule: &str) -> String {
    match rule.starts_with("mcp__") {
        true => rule.to_string(),
        false => same(rule),
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
/// worktrees do not have, then in .claude/settings.json. Each once for each
/// file that allows it, as written there. One that claude could not be
/// given in a list (a comma in it) is left out; what the settings deny is
/// left alone.
pub fn allowed(top: &Path) -> Vec<Allowed> {
    let mut out: Vec<Allowed> = Vec::new();
    for file in [LOCAL_SETTINGS, ".claude/settings.json"] {
        for r in rules_in(top, file, "allow") {
            if !gives(&r) && !out.iter().any(|a| a.rule == r && a.file == file) {
                out.push(Allowed { rule: r, file: file.to_string() });
            }
        }
    }
    out
}

/// The checkout's own claude settings that are yours, and untracked: the
/// agents' worktrees, made of what git tracks, do not have them.
pub const LOCAL_SETTINGS: &str = ".claude/settings.local.json";

/// What the checkout `top`'s own claude settings, yours, deny
/// (`permissions.deny` of .claude/settings.local.json): what claude refuses
/// when it runs in the checkout, but not in the agents' worktrees, which do
/// not have the file. stacked refuses them to every ask, wherever it runs.
/// Each once, as written; what .claude/settings.json denies, tracked, is
/// in the worktrees already.
pub fn denied(top: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in rules_in(top, LOCAL_SETTINGS, "deny") {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// The rules the checkout `top`'s claude settings `file` holds under
/// `permissions.WHAT`, if it is a file of the checkout: each that reads as
/// a rule (`rule`), as written; one claude could not be given in a list
/// (a comma in it) left out.
fn rules_in(top: &Path, file: &str, what: &str) -> Vec<String> {
    let Some(real) = in_checkout(top, Path::new(file)) else { return vec![] };
    let Some(v) = std::fs::read_to_string(&real).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return vec![] };
    let rules = v.pointer(&format!("/permissions/{what}")).and_then(|a| a.as_array()).into_iter().flatten();
    rules.filter_map(|r| rule(r.as_str()?).ok()).collect()
}

/// A rule the checkout's skills let run, which the setup lists, unticked:
/// the rule as a skill's header writes it, and the skills that allow it, by
/// name.
#[derive(Debug, Clone, PartialEq)]
pub struct Skilled {
    pub rule: String,
    pub skills: Vec<String>,
}

/// What the checkout `top`'s skills let run: the `allowed-tools` of the
/// header of each `.claude/skills/NAME/SKILL.md`, by the skill's name (the
/// header's, else its folder's). Each entry as written, quotes around it and
/// a stray comma after it taken off; one that is not a rule (`[Agent]`),
/// and what stacked gives or refuses (`gives`), left out; a rule several
/// skills allow, once, with each. Only files of the checkout are read: a
/// folder a link takes out of it is not.
pub fn skilled(top: &Path) -> Vec<Skilled> {
    let Some(root) = top.canonicalize().ok() else { return vec![] };
    let Some(dir) = root.join(".claude/skills").canonicalize().ok().filter(|d| d.starts_with(&root) && d.is_dir()) else { return vec![] };
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    folders.sort();
    let mut out: Vec<Skilled> = Vec::new();
    for folder in folders {
        let Some(text) = in_checkout(&root, &folder.join("SKILL.md")).and_then(|f| std::fs::read_to_string(f).ok()) else { continue };
        let (name, tools) = header(&text);
        let Some(name) = name.or_else(|| folder.file_name().map(|n| n.to_string_lossy().into_owned())) else { continue };
        for r in tools.iter().filter_map(|t| rule(t).ok()).filter(|r| !gives(r)) {
            match out.iter_mut().find(|s| s.rule == r) {
                Some(s) if !s.skills.contains(&name) => s.skills.push(name.clone()),
                Some(_) => {}
                None => out.push(Skilled { rule: r, skills: vec![name.clone()] }),
            }
        }
    }
    out
}

/// The name and the `allowed-tools` the header of a skill, `text`, gives
/// (its front matter, between `---` lines at its top): those on the line of
/// `allowed-tools:`, a space or a comma between each, or listed under it,
/// `- RULE` a line.
fn header(text: &str) -> (Option<String>, Vec<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, vec![]);
    }
    let (mut name, mut tools, mut listed) = (None, Vec::new(), false);
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if listed {
            match line.trim_start().strip_prefix('-') {
                Some(item) => {
                    tools.extend(entries(item));
                    continue;
                }
                None => listed = false,
            }
        }
        // A key of the header's own, not one under another.
        let Some((key, value)) = line.split_once(':').filter(|_| !line.starts_with(char::is_whitespace)) else { continue };
        match key.trim() {
            "name" => name = Some(value.trim().trim_matches(['"', '\'']).to_string()).filter(|n| !n.is_empty()),
            "allowed-tools" if value.trim().is_empty() => listed = true,
            "allowed-tools" => tools.extend(entries(value)),
            _ => {}
        }
    }
    (name, tools)
}

/// The entries of an `allowed-tools` value: a space or a comma between
/// each, none within parentheses or quotes; the quotes taken off.
fn entries(value: &str) -> Vec<String> {
    let (mut out, mut at) = (Vec::new(), String::new());
    let (mut depth, mut quote) = (0i32, None);
    for c in value.chars() {
        match (c, quote) {
            ('"' | '\'', None) if at.is_empty() => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (',' | ' ' | '\t', None) if depth <= 0 => {
                if !at.is_empty() {
                    out.push(std::mem::take(&mut at));
                }
            }
            _ => {
                depth += match c {
                    '(' => 1,
                    ')' => -1,
                    _ => 0,
                };
                at.push(c);
            }
        }
    }
    if !at.is_empty() {
        out.push(at);
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
/// the setup lists, unticked: the rule that lets it run; a file that names
/// it, by its path from the top of the checkout; and whether it is there to
/// run, on PATH or as a file of the checkout.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub rule: String,
    pub file: String,
    pub found: bool,
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
/// unmarked block. Kept, each once for each file that names it: those that
/// are there to run, on `path` or as a file of the checkout
/// (`./tools/lint`); and a block of shell's that are not on `path`, said
/// so. Left out: what stacked gives already
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
            if out.iter().any(|n| n.rule == rule && n.file == file) {
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

/// Which asks a list is for, and so which column of the setup ticks what
/// they may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Asks that change the stack: the column `change`.
    Change,
    /// Asks to answer only: the column `read only`.
    Answer,
}

impl Kind {
    /// The columns, from the left.
    pub const BOTH: [Kind; 2] = [Kind::Answer, Kind::Change];

    /// Its column, from the left: where its box is among a row's.
    fn col(self) -> usize {
        match self {
            Kind::Answer => 0,
            Kind::Change => 1,
        }
    }

    /// The other column's.
    fn other(self) -> Kind {
        match self {
            Kind::Answer => Kind::Change,
            Kind::Change => Kind::Answer,
        }
    }

    /// Its column's heading.
    fn label(self) -> &'static str {
        match self {
            Kind::Answer => "read only",
            Kind::Change => "change",
        }
    }

    /// Its asks, said within a sentence.
    fn named(self) -> &'static str {
        match self {
            Kind::Answer => "read-only asks",
            Kind::Change => "changes",
        }
    }

    /// Its list, as the settings name it.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Change => "agent.change_tools",
            Kind::Answer => "agent.answer_tools",
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

/// Why what `rule` lets use is to think twice about, for each kind of ask
/// it would be for (`Kind::warn`): an answer's, then a change's.
fn warnings(rule: &str) -> Vec<&'static str> {
    Kind::BOTH.iter().filter_map(|k| k.warn(rule)).collect()
}

/// What the setup starts from: the tools ticked for each kind of ask, and
/// whether the settings set each list (change_tools, then answer_tools)
/// rather than stacked's defaults; what the settings let every ask use
/// (agent.tools), ticked for both; what the checkout's own claude settings
/// allow, what its skills let run, and the programs its instructions name;
/// what its claude settings of yours deny, refused to every ask; and the
/// version of the setup done, if one was.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Start {
    pub change: Vec<String>,
    pub answer: Vec<String>,
    pub set: (bool, bool),
    pub every: Vec<String>,
    pub allowed: Vec<Allowed>,
    pub skilled: Vec<Skilled>,
    pub named: Vec<Named>,
    pub denied: Vec<String>,
    pub done: Option<usize>,
}

impl Start {
    /// What the setup starts from, the settings being `agent`, `set` saying
    /// which of their lists a file sets (change_tools's, then
    /// answer_tools's), for the checkout `top`, with PATH `path`. A list a
    /// file sets is ticked as it says. One no file sets is ticked as stacked
    /// has it: for changes, nothing; for answers, curl and jq, and what the
    /// checkout's own claude settings allow, as you allow it yourself, but
    /// what changes things. What its skills let run, the skills themselves,
    /// and the programs its instructions name are ticked only as the
    /// settings say.
    pub fn new(agent: &Agent, set: (bool, bool), top: Option<&Path>, path: &OsStr) -> Start {
        let allowed = top.map(allowed).unwrap_or_default();
        let answer = match set.1 {
            true => agent.answer_tools.clone(),
            false => {
                let mut out: Vec<String> = ANSWER_TOOLS.iter().map(|t| t.to_string()).collect();
                for a in allowed.iter().filter(|a| Kind::Answer.warn(&a.rule).is_none()) {
                    if !out.contains(&a.rule) {
                        out.push(a.rule.clone());
                    }
                }
                out
            }
        };
        Start {
            change: if set.0 { agent.change_tools.clone() } else { Vec::new() },
            answer,
            set,
            every: agent.tools.clone(),
            skilled: top.map(skilled).unwrap_or_default(),
            named: top.map(|t| named(t, path)).unwrap_or_default(),
            denied: top.map(denied).unwrap_or_default(),
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

/// A row of the screen. A row's boxes, and the marks of what stacked always
/// gives, are read only's, then change's (`Kind::BOTH`).
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Heading(String),
    /// What the settings give every ask, or what every ask is refused,
    /// said: nothing to let or not.
    Given(String),
    /// The columns' headings, and what each is for.
    Columns,
    /// What stacked always gives, said: `always` in the column of each kind
    /// of ask that has it, nothing to let or not.
    Always { said: &'static str, given: [bool; 2] },
    /// An MCP server: all its tools let, now and to come, or not, in each
    /// column; that written as the settings write it (`mcp__NAME__*`,
    /// `mcp__NAME`); where else it comes from.
    Server { name: String, form: String, all: [bool; 2], from: Vec<Source> },
    /// A tool, or a command, as it is written: let or not in each column;
    /// of a server, or not; where it comes from.
    Tool { name: String, server: Option<String>, on: [bool; 2], from: Vec<Source> },
    /// Where a command or a tool is added, typed.
    Add,
    /// A warning of the row above it, to read: nothing to let or not.
    Note(&'static str),
}

/// Where a rule the setup lists comes from.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    /// stacked itself: its default for answers, claude's tool for skills.
    Stacked(&'static str),
    /// A file of the checkout's claude settings that allows it.
    Allowed(String),
    /// The skills of the checkout that let it run.
    Skills(Vec<String>),
    /// A file of the checkout's instructions that names it, and whether it
    /// is there to run.
    Named(String, bool),
    /// The agent, which has it.
    Agent,
    /// The settings, which let it, none of the rest listing it.
    Settings,
    /// You, on the screen.
    Added,
}

/// What is said of where a rule comes from, `from`: each kind of source
/// once, with all it names. The files of the claude settings that allow
/// it; the skills that let it run; the files of the instructions that name
/// it, and that it is not on PATH, if it is not.
fn said(from: &[Source]) -> Vec<String> {
    let (mut files, mut skills, mut named, mut found): (Vec<&str>, Vec<&str>, Vec<&str>, bool) = Default::default();
    for f in from {
        match f {
            Source::Allowed(file) if !files.contains(&file.as_str()) => files.push(file),
            Source::Skills(all) => {
                for k in all {
                    if !skills.contains(&k.as_str()) {
                        skills.push(k);
                    }
                }
            }
            Source::Named(file, there) => {
                if !named.contains(&file.as_str()) {
                    named.push(file);
                }
                found |= there;
            }
            _ => {}
        }
    }
    let mut out: Vec<String> = Vec::new();
    for f in from {
        let one = match f {
            Source::Stacked(what) => what.to_string(),
            Source::Allowed(_) => format!("allowed in {}", files.join(", ")),
            Source::Skills(_) if skills.len() == 1 => format!("allowed by the skill {}", skills[0]),
            Source::Skills(_) => format!("allowed by the skills {}", skills.join(", ")),
            Source::Named(..) if found => format!("named in {}", named.join(", ")),
            Source::Named(..) => format!("named in {}, not found on PATH", named.join(", ")),
            Source::Agent => "one of the agent's tools".to_string(),
            Source::Settings => "in the settings".to_string(),
            Source::Added => "added here".to_string(),
        };
        if !out.contains(&one) {
            out.push(one);
        }
    }
    out
}

/// The rule that lets an ask use skills: claude's tool for them.
pub const SKILL: &str = "Skill";

/// What the setup says of letting answers use skills, a line each, under
/// the row of claude's tool for them.
const SKILLS_SAID: [&str; 3] = [
    "a skill may run what its own allowed-tools let it, commands that change things too",
    "(rewriting history, filing a bug), and an answer runs in your checkout itself,",
    "unless agent.answer_only_worktree says otherwise",
];

/// What the columns' heading says each is for.
const COLUMNS_SAID: &str = "read only is for answer-only asks; change is for asks that change the stack";

/// Where each column starts on a line, and how wide it is, as wide as its
/// heading: read only's, then change's. What a row says starts at `TEXT`.
const COLUMNS: [(usize, usize); 2] = [(1, 9), (12, 6)];
const TEXT: usize = 21;

impl Row {
    /// What it lets use, written as for every rule that means the same
    /// (`same`), if it lets use something.
    fn key(&self) -> Option<String> {
        match self {
            Row::Server { form, .. } => Some(same(form)),
            Row::Tool { name, .. } => Some(same(name)),
            _ => None,
        }
    }

    /// Whether it has a box in each column.
    fn boxes(&self) -> bool {
        matches!(self, Row::Server { .. } | Row::Tool { .. })
    }

    /// Whether the cursor can be on it.
    fn picks(&self) -> bool {
        self.boxes() || *self == Row::Add
    }
}

/// A box, ticked or not.
fn mark(on: bool) -> &'static str {
    if on { "[x]" } else { "[ ]" }
}

/// `text` in the middle of `w` columns, cut to them, spaces on each side.
fn centred(text: &str, w: usize) -> String {
    let text: String = text.chars().take(w).collect();
    let left = (w - text.chars().count()) / 2;
    format!("{}{text}{}", " ".repeat(left), " ".repeat(w - text.chars().count() - left))
}

/// A row being made: what it lets use (`same`); each way its sources write
/// that; where it comes from; and whether it is an MCP server's, all its
/// tools, or a tool's, of a server or not.
struct Item {
    key: String,
    forms: Vec<String>,
    from: Vec<Source>,
    server: Option<String>,
    whole: bool,
}

impl Item {
    /// The row for `form`, from `from` if a source gives it, of `server` if
    /// it is a server's, and its whole if `whole`.
    fn new(form: &str, from: Option<Source>, server: Option<String>, whole: bool) -> Item {
        Item { key: same(form), forms: vec![form.to_string()], from: from.into_iter().collect(), server, whole }
    }
}

/// The rows being made, in their order: those of the sources read first;
/// then each MCP server's, with its tools'; then the rest. Whether a row
/// made now goes after the servers'.
#[derive(Default)]
struct Items {
    before: Vec<Item>,
    served: Vec<Item>,
    after: Vec<Item>,
    past: bool,
}

impl Items {
    /// Each row made so far.
    fn all(&mut self) -> impl Iterator<Item = &mut Item> {
        self.before.iter_mut().chain(&mut self.served).chain(&mut self.after)
    }

    /// `form`, from `source`: on the row of what it lets use, if there is
    /// one, said with where else it comes from; else on a row of its own.
    fn put(&mut self, form: &str, source: Source) {
        let key = same(form);
        if let Some(i) = self.all().find(|i| i.key == key) {
            i.forms.push(form.to_string());
            if !i.from.contains(&source) {
                i.from.push(source);
            }
            return;
        }
        let item = Item::new(form, Some(source), None, false);
        if self.past { self.after.push(item) } else { self.before.push(item) }
    }
}

/// A command or a tool being typed, to add a row for: why it cannot be
/// added as it is, if it cannot.
#[derive(Debug, Clone, PartialEq)]
pub struct Adding {
    pub ed: Editor,
    pub wrong: Option<String>,
}

/// The screen: its rows, the one picked, and the column the cursor is in;
/// what agent.tools lets every ask use; what was chosen as it opened; the
/// file it is saved in; where the agent was asked, and what the setup is
/// now, said; and what is open over it.
#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
    rows: Vec<Row>,
    pub sel: usize,
    pub col: Kind,
    /// The first row drawn; how many lines there was room for; whether the
    /// columns' heading, its own row above the first drawn, was drawn on
    /// the first of them.
    top: usize,
    room: usize,
    pinned: bool,
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
    /// The screen for the agent's `tools` and MCP `servers`, the boxes as
    /// `start` ticks them, to be saved in the file `to`; `said`, where the
    /// agent was asked. First what is refused to every ask, the checkout's
    /// claude settings of yours denying it, and what the settings let every
    /// ask use, to read. Then one list, a box in each row for each kind of
    /// ask: under the columns' headings, what stacked always gives, marked
    /// in the column of each kind that has it; curl and jq, which stacked
    /// let answers run before there were lists; the skills, with what they
    /// may run; what the checkout's own claude settings allow; what its
    /// skills let run; the programs its instructions name; each MCP server
    /// and its tools; the agent's other tools; what the settings let that is
    /// none of those (a command, a server not there now), to keep or take
    /// out; and a row to add one more. Rules that mean the same (`same`) are
    /// one row, which says where each comes from. Its boxes are ticked as
    /// for any row: as the settings say, else as stacked has it for any of
    /// those sources. A command's rule is shown and written the newer way
    /// (`shown`), however they write it; all of a server's tools as the
    /// settings write them, if they do, else as all its sources do, if they
    /// agree, else as `same` writes them; any other rule as it is written.
    pub fn new(tools: &[String], servers: &[String], start: &Start, to: &str, said: &str) -> Setup {
        let mut names: Vec<String> = servers.to_vec();
        for t in tools {
            if let Some(s) = server_of(t)
                && !names.contains(&s)
            {
                names.push(s);
            }
        }
        // The servers' rows, and their tools', made first: a rule a source
        // gives for one of them goes on its row.
        let mut items = Items::default();
        for s in &names {
            items.served.push(Item::new(&format!("mcp__{s}__*"), None, Some(s.clone()), true));
            for t in tools.iter().filter(|t| server_of(t).as_deref() == Some(s.as_str())) {
                items.served.push(Item::new(t, None, Some(s.clone()), false));
            }
        }
        for t in ANSWER_TOOLS {
            items.put(t, Source::Stacked("stacked's default for answers"));
        }
        items.put(SKILL, Source::Stacked("claude's tool for the checkout's skills"));
        for a in &start.allowed {
            items.put(&a.rule, Source::Allowed(a.file.clone()));
        }
        for s in &start.skilled {
            items.put(&s.rule, Source::Skills(s.skills.clone()));
        }
        for n in &start.named {
            items.put(&n.rule, Source::Named(n.file.clone(), n.found));
        }
        items.past = true;
        for t in tools.iter().filter(|t| server_of(t).is_none() && !GIVEN.contains(&t.as_str())) {
            items.put(t, Source::Agent);
        }
        // What the settings let that is none of those: kept, unless taken
        // out.
        for t in start.answer.iter().chain(&start.change).chain(&start.every) {
            let key = same(t);
            if !items.all().any(|i| i.key == key) {
                items.after.push(Item::new(t, Some(Source::Settings), None, false));
            }
        }
        // How the settings write what they let: agent.tools first, then each
        // list they set. A row's rule but an MCP server's is its key, as
        // `shown` writes it.
        let written: Vec<&String> = start
            .every
            .iter()
            .chain(start.answer.iter().filter(|_| start.set.1))
            .chain(start.change.iter().filter(|_| start.set.0))
            .collect();
        let form = |i: &Item| match written.iter().find(|w| same(w) == i.key) {
            _ if !i.key.starts_with("mcp__") => i.key.clone(),
            Some(w) => w.to_string(),
            None if i.forms.iter().all(|f| *f == i.forms[0]) => i.forms[0].clone(),
            None => i.key.clone(),
        };
        let ticked = |key: &str| {
            Kind::BOTH.map(|kind| {
                let list = match kind {
                    Kind::Change => &start.change,
                    Kind::Answer => &start.answer,
                };
                list.iter().chain(&start.every).any(|t| same(t) == key)
            })
        };
        // What every ask is refused, said once however it is written.
        let mut denied: Vec<&String> = Vec::new();
        for d in &start.denied {
            if !denied.iter().any(|x| same(x) == same(d)) {
                denied.push(d);
            }
        }
        let mut rows = Vec::new();
        if !denied.is_empty() {
            rows.push(Row::Heading("Refused to every ask, whatever the lists say".into()));
            rows.extend(denied.iter().map(|r| Row::Given(format!("{r}: denied in {LOCAL_SETTINGS}, refused to every ask"))));
        }
        if !start.every.is_empty() {
            rows.push(Row::Heading("Every ask, builds and tests too: agent.tools".into()));
            rows.push(Row::Given(format!(
                "{}: ticked in both columns below; taken out of one, a tool is kept in the other's list alone",
                start.every.join(", ")
            )));
        }
        rows.push(Row::Columns);
        rows.extend(GIVEN_SAID.iter().map(|&(said, answer, change)| Row::Always { said, given: [answer, change] }));
        // The boxes of the server last listed, all its tools let.
        let mut whole = [false; 2];
        for i in items.before.iter().chain(&items.served).chain(&items.after) {
            let mut on = ticked(&i.key);
            match &i.server {
                Some(name) if i.whole => {
                    whole = on;
                    rows.push(Row::Server { name: name.clone(), form: form(i), all: on, from: i.from.clone() });
                    continue;
                }
                Some(_) => on = [on[0] || whole[0], on[1] || whole[1]],
                None => {}
            }
            rows.push(Row::Tool { name: form(i), server: i.server.clone(), on, from: i.from.clone() });
            if i.key == SKILL {
                rows.extend(SKILLS_SAID.map(Row::Note));
            }
        }
        rows.push(Row::Add);
        let sel = rows.iter().position(Row::picks).unwrap_or(0);
        let now = match start.done.unwrap_or(0) {
            n if n >= VERSION => "Ticked as the settings say now. Saving writes both lists again.",
            0 => "Not set up yet: ticked as stacked has it, and as the settings say. Saving it lets the asks go.",
            _ => "An earlier stacked's setup was done: ticked as the settings say. Saving it lets the asks go.",
        };
        let mut s = Setup {
            rows,
            sel,
            col: Kind::Answer,
            top: 0,
            room: 0,
            pinned: false,
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
        self.rows.iter().any(|r| matches!(r, Row::Server { name, all, .. } if name == server && all[kind.col()]))
    }

    /// What `kind`'s asks are let use, as ticked in its column: each server
    /// let whole, as its row writes it, and each tool let.
    fn ticked(&self, kind: Kind) -> Vec<String> {
        let c = kind.col();
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for r in &self.rows {
            let t = match r {
                Row::Server { form, all, .. } if all[c] => form.clone(),
                Row::Tool { name, server, on, .. } if on[c] && !server.as_deref().is_some_and(|s| self.all(kind, s)) => name.clone(),
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
    /// of agent.tools, if a tool of it is no longer let one kind's asks, or
    /// if one of its rules is to be written as its row writes it, not as
    /// agent.tools does now (a command's older prefix form, `shown`).
    pub fn chosen(&self) -> Chosen {
        let (change, answer) = (self.ticked(Kind::Change), self.ticked(Kind::Answer));
        let mut kept: Vec<String> = Vec::new();
        for t in &self.every {
            let key = same(t);
            let written = self.rows.iter().find_map(|r| match r {
                Row::Server { form: rule, .. } | Row::Tool { name: rule, .. } if same(rule) == key => Some(rule.clone()),
                _ => None,
            });
            let t = written.unwrap_or_else(|| t.clone());
            if change.contains(&t) && answer.contains(&t) && !kept.contains(&t) {
                kept.push(t);
            }
        }
        Chosen {
            change: change.into_iter().filter(|t| !kept.contains(t)).collect(),
            answer: answer.into_iter().filter(|t| !kept.contains(t)).collect(),
            every: (kept != self.every).then_some(kept),
        }
    }

    /// The box of the row picked in the cursor's column, ticked or not. A
    /// server's: all its tools let, or none. One tool of a server let
    /// whole: the server no longer is, its other tools still are. The row
    /// to add one: the line to type it in, open.
    fn toggle(&mut self) {
        let c = self.col.col();
        match self.rows.get(self.sel).cloned() {
            Some(Row::Server { name, all, .. }) => {
                for r in &mut self.rows {
                    match r {
                        Row::Server { name: n, all: a, .. } if *n == name => a[c] = !all[c],
                        Row::Tool { server: Some(s), on, .. } if *s == name => on[c] = !all[c],
                        _ => {}
                    }
                }
            }
            Some(Row::Tool { server, on, .. }) => {
                if let Some(s) = server {
                    for r in &mut self.rows {
                        if let Row::Server { name, all, .. } = r
                            && *name == s
                        {
                            all[c] = false;
                        }
                    }
                }
                if let Some(Row::Tool { on: o, .. }) = self.rows.get_mut(self.sel) {
                    o[c] = !on[c];
                }
            }
            Some(Row::Add) => self.open_adding(),
            _ => {}
        }
    }

    /// Row `k`'s boxes ticked as `to` says, read only's then change's, each
    /// as Space would tick it.
    fn set(&mut self, k: usize, to: [bool; 2]) {
        let (sel, col) = (self.sel, self.col);
        self.sel = k;
        for kind in Kind::BOTH {
            let now = match &self.rows[k] {
                Row::Server { all: on, .. } | Row::Tool { on, .. } => on[kind.col()],
                _ => continue,
            };
            if now != to[kind.col()] {
                self.col = kind;
                self.toggle();
            }
        }
        (self.sel, self.col) = (sel, col);
    }

    /// The line to type a command or a tool in, open.
    fn open_adding(&mut self) {
        self.adding = Some(Adding { ed: Editor::new("").single(), wrong: rule("").err() });
    }

    /// `rule`'s row, the cursor on it: one of its own, last, neither box
    /// ticked, the rule as the setup writes it (`shown`); or, if a row lets
    /// use the same already (`same`), that one, as it is. Whether it is new.
    pub fn add(&mut self, rule: &str) -> bool {
        let key = same(rule);
        if let Some(at) = self.rows.iter().position(|r| r.key().as_deref() == Some(key.as_str())) {
            self.sel = at;
            return false;
        }
        let at = self.rows.iter().position(|r| *r == Row::Add).unwrap_or(self.rows.len());
        self.rows.insert(at, Row::Tool { name: shown(rule), server: None, on: [false; 2], from: vec![Source::Added] });
        self.sel = at;
        true
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

    /// A key. ↑↓ (and PageUp, PageDown, Home, End) pick a row; ← and →, or
    /// Tab and shift-Tab, the column, read only or change; Space (or Enter)
    /// ticks the box of the row picked in that column, or not. Esc (or q),
    /// as in the comment editor: with something changed, a menu, to save it,
    /// throw it away or keep choosing; with nothing, out. `a` (or Enter on
    /// the row for it) opens a line to type a command or a tool in: Enter
    /// adds a row for it, neither box ticked, once it reads as claude's
    /// permission rules do; Esc goes back. `s` saves, changed or not: saving
    /// is the setup done. Ctrl-c is out, whatever is open.
    pub fn key(&mut self, k: KeyEvent) -> Step {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Step::Leave;
        }
        if let Some(mut a) = self.adding.take() {
            match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => match rule(&a.ed.text()) {
                    Ok(r) => {
                        self.add(&r);
                    }
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
            KeyCode::Left => self.col = Kind::BOTH[0],
            KeyCode::Right => self.col = Kind::BOTH[1],
            KeyCode::Tab | KeyCode::BackTab => self.col = self.col.other(),
            KeyCode::Char(' ') | KeyCode::Enter => self.toggle(),
            KeyCode::Char('a') => self.open_adding(),
            KeyCode::Char('s') => return Step::Save(self.chosen()),
            KeyCode::Esc | KeyCode::Char('q') if self.changed() => self.menu = Some(0),
            KeyCode::Esc | KeyCode::Char('q') => return Step::Leave,
            _ => {}
        }
        Step::Stay
    }

    /// What is drawn at `(x, y)` of the screen, as `draw` drew it last: the
    /// row there, if one that can be picked is; and the column whose box is
    /// at `x`, if the row has one there.
    pub fn at(&self, x: usize, y: usize) -> Option<(usize, Option<Kind>)> {
        let first = 2 + usize::from(self.pinned);
        let k = y.checked_sub(first).filter(|&k| k + usize::from(self.pinned) < self.room)? + self.top;
        let row = self.rows.get(k).filter(|r| r.picks())?;
        let col = Kind::BOTH.into_iter().find(|kind| {
            let (at, w) = COLUMNS[kind.col()];
            row.boxes() && (at..at + w).contains(&x)
        });
        Some((k, col))
    }

    /// The mouse over `(x, y)`: the row there picked, as the arrows would,
    /// and, over a box, its column.
    pub fn hover(&mut self, x: usize, y: usize) {
        if let Some((k, col)) = self.at(x, y) {
            self.sel = k;
            self.col = col.unwrap_or(self.col);
        }
    }

    /// A click at `(x, y)`: on a box, that box ticked or not, the cursor
    /// on it; on the row to add a command or a tool, the line to type it in,
    /// open; on another row, that row picked.
    pub fn click(&mut self, x: usize, y: usize) {
        let Some((k, col)) = self.at(x, y) else { return };
        self.sel = k;
        match col {
            Some(c) => {
                self.col = c;
                self.toggle();
            }
            None if self.rows[k] == Row::Add => self.open_adding(),
            None => {}
        }
    }

    /// Row `k` picked, as the arrows would, if it can be.
    pub fn pick(&mut self, k: usize) {
        if self.rows.get(k).is_some_and(Row::picks) {
            self.sel = k;
        }
    }

    /// Row `k` as drawn: what it shows in each column, read only's then
    /// change's, if it has columns; and what it says after them, or across
    /// the line if it has none.
    fn cells(&self, k: usize) -> (Option<[String; 2]>, String) {
        let empty = || Some([String::new(), String::new()]);
        let boxes = |on: &[bool; 2]| Some(on.map(|o| mark(o).to_string()));
        let after = |all: Vec<String>| if all.is_empty() { String::new() } else { format!("   {}", all.join("; ")) };
        match &self.rows[k] {
            Row::Heading(t) => (None, format!(" {t}")),
            Row::Given(t) => (None, format!("   {t}")),
            Row::Columns => (Some(Kind::BOTH.map(|kind| kind.label().to_string())), COLUMNS_SAID.to_string()),
            Row::Always { said, given } => (Some(given.map(|g| if g { "always".to_string() } else { String::new() })), said.to_string()),
            Row::Server { name, form, all, from } => {
                (boxes(all), format!("all of the MCP server {name}'s tools, those to come too ({form}){}", after(said(from))))
            }
            Row::Tool { name, server: Some(s), on, from } => {
                let short = name.strip_prefix(&format!("mcp__{s}__")).unwrap_or(name);
                (boxes(on), format!("  {short}{}", after(said(from))))
            }
            Row::Tool { name, on, from, .. } => {
                let all = said(from).into_iter().chain(warnings(name).into_iter().map(String::from)).collect();
                (boxes(on), format!("{name}{}", after(all)))
            }
            Row::Add => (empty(), "+ add a command or a tool…".to_string()),
            Row::Note(t) => (empty(), format!("  {t}")),
        }
    }

    /// Row `k` as drawn, without its colours: as the setup asked line by
    /// line says it too.
    fn line(&self, k: usize) -> String {
        let (cells, text) = self.cells(k);
        let Some(cells) = cells else { return text };
        let mut out = String::new();
        for (kind, cell) in Kind::BOTH.into_iter().zip(&cells) {
            let (at, w) = COLUMNS[kind.col()];
            out += &" ".repeat(at - out.chars().count());
            out += &centred(cell, w);
        }
        out += &" ".repeat(TEXT - out.chars().count());
        out += &text;
        out.trim_end().to_string()
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
        // The first row drawn: the one picked always shows, and so do the
        // rows above it when none of them can be picked. Once the columns'
        // heading is above the first row drawn, it is drawn on the first
        // line, over the rest, to say which box is which.
        let room = h - 4;
        let heading = self.rows.iter().position(|r| *r == Row::Columns).unwrap_or(0);
        let mut top = self.top.min(self.sel);
        if !self.rows[..self.sel].iter().any(Row::picks) && self.sel < room {
            top = 0;
        }
        if self.sel >= top + room {
            top = self.sel + 1 - room;
        }
        let pinned = top > heading && room > 1;
        if pinned && self.sel + 1 >= top + room {
            top = self.sel + 2 - room;
        }
        (self.top, self.room, self.pinned) = (top, room, pinned);
        let first = 2 + usize::from(pinned);
        for (y, k) in (top..self.rows.len()).take(room - usize::from(pinned)).enumerate() {
            self.draw_row(buf, area, k, first + y, w);
        }
        if pinned {
            self.draw_row(buf, area, heading, 2, w);
        }
        // What the keys do, as the comment editor says it; typing a command,
        // why Enter does not take it, if it does not.
        let esc = if self.changed() { "esc: save or throw away" } else { "esc: close" };
        let keys = match (&self.adding, self.menu, self.rows.get(self.sel)) {
            (Some(Adding { wrong: Some(why), .. }), ..) => format!(" {why} · esc: back"),
            (Some(_), ..) => " enter: add it · esc: back".to_string(),
            (None, Some(_), _) => " ↑↓ choose · enter: do it · esc: keep choosing".to_string(),
            (None, None, Some(Row::Add)) => format!(" ↑↓: pick · enter: add a command or a tool · s: save · {esc}"),
            (None, None, _) => format!(" ↑↓←→: pick · space: let {} use it or not · a: add a command or a tool · s: save · {esc}", self.col.named()),
        };
        put(buf, area, 0, h - 1, w, &fit(&keys, w), color::status());
        if let Some(at) = self.menu {
            self.draw_menu(buf, area, at);
        }
        self.adding.as_ref().map(|a| {
            let bw = (w * 9 / 10).clamp(30.min(w), 100.min(w));
            let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(3) / 2);
            let title = "a command or a tool, as claude writes it: Bash(PROGRAM *), WebFetch, mcp__SERVER__TOOL";
            let foot = if a.wrong.is_some() { "esc: back" } else { "enter: add it · esc: back" };
            a.ed.draw(buf, area, x, y, bw, 3, title, foot)
        })
    }

    /// Row `k`, on line `y`, `w` wide, in its colours: a server's tool's box
    /// dim in the column where the server is let whole. Picked, what it says
    /// and its box in the cursor's column are under the cursor.
    fn draw_row(&self, buf: &mut Buffer, area: Rect, k: usize, y: usize, w: usize) {
        let row = &self.rows[k];
        let st = match row {
            Row::Heading(_) | Row::Columns | Row::Server { .. } => color::file(),
            Row::Given(_) | Row::Always { .. } | Row::Add => color::dim(),
            Row::Tool { name, server: None, .. } if !warnings(name).is_empty() => color::note(),
            Row::Tool { .. } => Style::new(),
            Row::Note(_) => color::note(),
        };
        let line = self.line(k);
        put(buf, area, 0, y, w, &fit(&line, w), st);
        let picked = k == self.sel;
        if let (Some(cells), true) = (self.cells(k).0, row.boxes()) {
            for kind in Kind::BOTH {
                let (at, cw) = COLUMNS[kind.col()];
                let dim = matches!(row, Row::Tool { server: Some(s), .. } if self.all(kind, s));
                let bst = if dim { color::dim() } else { Style::new() };
                let bst = if picked && kind == self.col { on_cursor(bst) } else { bst };
                put(buf, area, at, y, cw, &centred(&cells[kind.col()], cw), bst);
            }
        }
        if picked {
            let text: String = line.chars().skip(TEXT).collect();
            let room = w.saturating_sub(TEXT);
            put(buf, area, TEXT, y, room, &fit(&text, room), on_cursor(st));
        }
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

    /// The name of row `k`'s tool, short of its server's prefix if it is a
    /// server's.
    fn short(&self, k: usize) -> String {
        match &self.rows[k] {
            Row::Tool { name, server: Some(s), .. } => name.strip_prefix(&format!("mcp__{s}__")).unwrap_or(name).to_string(),
            Row::Tool { name, .. } => name.clone(),
            _ => String::new(),
        }
    }

    /// Row `k`, a server's or a tool's, asked line by line: which kinds of
    /// ask may use what it names, both at once, Enter keeping it as it is;
    /// for a server, all its tools at once, or each.
    fn ask_row(&mut self, k: usize, ask: &mut dyn FnMut(&str) -> String, say: &mut dyn FnMut(&str)) {
        match self.rows[k].clone() {
            Row::Server { name, form, all, .. } => {
                let its: Vec<usize> = (0..self.rows.len()).filter(|&i| matches!(&self.rows[i], Row::Tool { server: Some(s), .. } if *s == name)).collect();
                let on = |s: &Setup, i: usize| match &s.rows[i] {
                    Row::Tool { on, .. } => *on,
                    _ => [false; 2],
                };
                let short: Vec<String> = its.iter().map(|&i| self.short(i)).collect();
                say(&match short.is_empty() {
                    true => format!("  The MCP server {name} has no tools now."),
                    false => format!("  The MCP server {name} has these tools: {}.", short.join(", ")),
                });
                // What Enter keeps: in each column, all of its tools, or
                // none; else each as it is.
                let now = Kind::BOTH.map(|kind| match all[kind.col()] {
                    true => Some(true),
                    false => (!its.iter().any(|&i| on(self, i)[kind.col()])).then_some(false),
                });
                let now = match now {
                    [Some(a), Some(c)] => Some([a, c]),
                    _ => None,
                };
                let a = ask(&format!("  Which asks may use all of the MCP server {name}'s tools, those to come too ({form})? {}", options(now, true)));
                match (chosen_of(&a), a.trim().to_ascii_lowercase().as_str()) {
                    (Some(to), _) => self.set(k, to),
                    (None, "e" | "each") => {
                        for i in its {
                            let was = on(self, i);
                            let a = ask(&format!("    Which asks may use {}? {}", self.short(i), options(Some(was), false)));
                            self.set(i, chosen_of(&a).unwrap_or(was));
                        }
                    }
                    _ => {}
                }
            }
            Row::Tool { name, on, from, .. } => {
                // Where it comes from, said with what it names; a warning,
                // and the notes under it, a sentence after the question.
                let places: String = said(&from).iter().map(|s| format!(", {s}")).collect();
                let notes: Vec<&str> = self.rows[k + 1..]
                    .iter()
                    .map_while(|r| match r {
                        Row::Note(t) => Some(*t),
                        _ => None,
                    })
                    .collect();
                let mut warned: String = warnings(&name).into_iter().map(sentence).collect();
                if !notes.is_empty() {
                    warned += &sentence(&notes.join(" "));
                }
                let a = ask(&format!("  Which asks may use {name}{places}?{warned} {}", options(Some(on), false)));
                if let Some(to) = chosen_of(&a) {
                    self.set(k, to);
                }
            }
            _ => {}
        }
    }
}

/// `text` as a sentence of its own, after a space: its first letter a
/// capital, a full stop at its end.
fn sentence(text: &str) -> String {
    let mut c = text.chars();
    c.next().map_or(String::new(), |f| format!(" {}{}.", f.to_uppercase(), c.as_str().trim_end_matches(['.', ','])))
}

/// The answers to a row's question asked line by line, and the boxes each
/// ticks, read only's then change's.
const CHOICES: [(char, &str, [bool; 2]); 4] =
    [('r', "read only", [true, false]), ('c', "change", [false, true]), ('b', "both", [true, true]), ('n', "neither", [false, false])];

/// The answers a row's question takes, said for it, the one Enter gives in
/// capitals: the one ticking the boxes as they are `now`. With `each`, a
/// server's question, the answer that asks of each of its tools too, Enter's
/// when its tools are as no other answer ticks them (`now` being none).
fn options(now: Option<[bool; 2]>, each: bool) -> String {
    let mut out: Vec<String> = CHOICES
        .iter()
        .map(|&(letter, label, to)| match now == Some(to) {
            true => format!("{}: {label}", letter.to_ascii_uppercase()),
            false => format!("{letter}: {label}"),
        })
        .collect();
    if each {
        out.push(if now.is_none() { "E: each" } else { "e: each" }.to_string());
    }
    format!("[{}]", out.join(", "))
}

/// The boxes answer `a` ticks, if it is one of `CHOICES`, by its letter or
/// in full.
fn chosen_of(a: &str) -> Option<[bool; 2]> {
    let a = a.trim().to_ascii_lowercase();
    CHOICES.iter().find(|(letter, label, _)| a == letter.to_string() || a == *label).map(|&(_, _, to)| to)
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
/// answer (a line, as typed), `say` says the rest. First the list, a line a
/// row as the screen shows it, to keep as it is or to go through a row at a
/// time, each asked for both kinds of ask in one question, Enter keeping it
/// as it was; then any command or tool to add, until a line is left empty,
/// each asked so in turn. What is to be saved.
pub fn ask_lines(s: &mut Setup, ask: &mut dyn FnMut(&str) -> String, say: &mut dyn FnMut(&str)) -> Chosen {
    for k in 0..s.rows.len() {
        if s.rows[k] != Row::Add {
            say(&s.line(k));
        }
    }
    if matches!(ask("Keep them as they are? [Y/n: one by one]").to_ascii_lowercase().as_str(), "n" | "no") {
        for k in 0..s.rows.len() {
            if matches!(s.rows[k], Row::Server { .. } | Row::Tool { server: None, .. }) {
                s.ask_row(k, ask, say);
            }
        }
    }
    loop {
        let a = ask("Add a command or a tool, as claude writes a permission rule (Bash(PROGRAM *), WebFetch, mcp__SERVER__TOOL), or press Enter for none:");
        if a.trim().is_empty() {
            break;
        }
        match rule(&a) {
            Ok(r) => {
                let new = s.add(&r);
                let was = match &s.rows[s.sel] {
                    Row::Tool { name, .. } | Row::Server { form: name, .. } => name.clone(),
                    _ => r.clone(),
                };
                match (new, was == r) {
                    (true, true) => say(&format!("  {r}: added.")),
                    (true, false) => say(&format!("  {r}: added, as {was}.")),
                    (false, true) => say(&format!("  {r}: listed already.")),
                    (false, false) => say(&format!("  {r}: listed already, as {was}.")),
                }
                s.ask_row(s.sel, ask, say);
            }
            Err(why) => say(&format!("  {why}.")),
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
        let area = Rect::new(0, 0, 160, h);
        let mut buf = Buffer::empty(area);
        s.draw(&mut buf, area);
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// How many lines of `shown` are `line`.
    fn times(shown: &str, line: &str) -> usize {
        shown.lines().filter(|l| *l == line).count()
    }

    /// The line of `shown` that is `line`.
    fn line_of(shown: &str, line: &str) -> usize {
        shown.lines().position(|l| l == line).unwrap_or_else(|| panic!("{line:?} in:\n{shown}"))
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

    /// Where the row for `name` is, a tool's or a server's.
    fn row_of(s: &Setup, name: &str) -> usize {
        s.rows
            .iter()
            .position(|r| matches!(r, Row::Tool { name: n, .. } if n == name) || matches!(r, Row::Server { name: n, .. } if n == name))
            .unwrap()
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
    fn rules_that_mean_the_same_are_told_from_those_that_do_not() {
        // The older prefix form and the newer, a program's or a command's;
        // all of a server's tools, either way.
        for (a, b) in [
            ("Bash(searchfox-cli:*)", "Bash(searchfox-cli *)"),
            ("Bash(./mach:*)", "Bash(./mach *)"),
            ("Bash(git push:*)", "Bash(git push *)"),
            ("mcp__moz", "mcp__moz__*"),
        ] {
            assert_eq!(same(a), same(b), "{a} and {b}");
            assert_eq!(same(a), b, "written the newer way");
        }
        // A narrower command and a wider one; the command alone and with
        // more; a program and every word it starts; a star the older form
        // reads as it is written; one tool of a server and all of them.
        for (a, b) in [
            ("Bash(./mach file-info:*)", "Bash(./mach *)"),
            ("Bash(./mach file-info *)", "Bash(./mach *)"),
            ("Bash(make)", "Bash(make *)"),
            ("Bash(make*)", "Bash(make *)"),
            ("Bash(git * push:*)", "Bash(git * push *)"),
            ("Bash(a\\ b:*)", "Bash(a\\ b *)"),
            ("Bash( make:*)", "Bash(make *)"),
            ("mcp__moz__get_bug", "mcp__moz__*"),
            ("Bash", "Bash(make *)"),
        ] {
            assert_ne!(same(a), same(b), "{a} and {b}");
        }
        assert_eq!(same("Bash(git * push:*)"), "Bash(git * push:*)", "as written");
    }

    #[test]
    fn the_lists_start_as_stacked_has_them_the_settings_winning() {
        // Nothing set: no tool for changes; curl and jq for answers.
        let s = fresh();
        assert_eq!((s.change, s.answer, s.set, s.done), (vec![], strings(&["Bash(curl *)", "Bash(jq *)"]), (false, false), None));
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
            (s.change, s.answer, s.every, s.set, s.done),
            (strings(&["Bash(make check *)"]), strings(&["Bash(curl *)"]), strings(&["mcp__moz__*"]), (true, true), Some(VERSION))
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
    fn one_list_a_box_for_each_kind_what_stacked_gives_marked_in_its_column() {
        let tools = strings(&["Read", "Bash", "WebFetch", "mcp__moz__get_bug", "mcp__moz__get_revision", "mcp__notes__write"]);
        let servers = strings(&["moz", "notes"]);
        // As the settings were: notes' write let every ask; a command of the
        // change list's own, kept as it is.
        let start = Start {
            change: strings(&["Bash(make check *)"]),
            set: (true, false),
            every: strings(&["mcp__notes__write"]),
            ..fresh()
        };
        let mut s = Setup::new(&tools, &servers, &start, "local.toml", "the agent's tools, in repo");
        let shown = screen(&mut s);
        let lines = [
            " What each kind of ask may use, besides what stacked always gives it: the agent's tools, in repo",
            " Not set up yet: ticked as stacked has it, and as the settings say. Saving it lets the asks go.",
            " Every ask, builds and tests too: agent.tools",
            "   mcp__notes__write: ticked in both columns below; taken out of one, a tool is kept in the other's list alone",
            " read only  change   read only is for answer-only asks; change is for asks that change the stack",
            "  always    always   Read, Grep, Glob, LS, and git show, log and diff",
            "            always   commands that read: grep, rg, find, cat, sed, sort…",
            "            always   git's other history commands (status, add, commit, reset, rebase -i…), and git config to read",
            "  always             stacked's commands that read the stack: export, show, log, tree, versions, interdiff, comments",
            "            always   stacked's fits and export; nothing a change is given reaches the network",
            "    [x]      [ ]     Bash(curl *)   stacked's default for answers",
            "    [x]      [ ]     Bash(jq *)   stacked's default for answers",
            "    [ ]      [ ]     Skill   claude's tool for the checkout's skills",
            "    [ ]      [ ]     all of the MCP server moz's tools, those to come too (mcp__moz__*)",
            "    [ ]      [ ]       get_bug",
            "    [ ]      [ ]       get_revision",
            "    [ ]      [ ]     all of the MCP server notes's tools, those to come too (mcp__notes__*)",
            "    [x]      [x]       write",
            "    [ ]      [ ]     Bash   one of the agent's tools; it changes things: an answer could too; it can reach the network: a change could touch a remote",
            "    [ ]      [ ]     WebFetch   one of the agent's tools; it can reach the network: a change could touch a remote",
            "    [ ]      [x]     Bash(make check *)   in the settings",
            "                     + add a command or a tool…",
            " To be, in local.toml: change_tools: Bash(make check *); answer_tools: Bash(curl *), Bash(jq *)",
        ];
        // Each once, in that order: one list.
        let at: Vec<usize> = lines.iter().map(|l| line_of(&shown, l)).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?} in:\n{shown}");
        for l in lines {
            assert_eq!(times(&shown, l), 1, "{l:?} in:\n{shown}");
        }
        assert!(!shown.contains("Read   one of"), "given already: {shown}");
        assert_eq!(shown.matches("Bash(curl *)").count(), 2, "on its row, and in what saving writes: {shown}");
        // The cursor starts on the first box: read only's, of curl.
        assert_eq!((s.sel, s.col), (row_of(&s, "Bash(curl *)"), Kind::Answer));
        // moz let changes, all of it at once: its change box.
        s.pick(row_of(&s, "moz"));
        press(&mut s, KeyCode::Right);
        assert_eq!(press(&mut s, KeyCode::Char(' ')), Step::Stay);
        assert_eq!(s.chosen().change, ["mcp__moz__*", "Bash(make check *)"]);
        assert!(screen(&mut s).contains("\n    [ ]      [x]     all of the MCP server moz's tools, those to come too (mcp__moz__*)\n"));
        // One of its tools taken back: moz is no longer let whole.
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.chosen().change, ["mcp__moz__get_revision", "Bash(make check *)"]);
        // notes' write taken out of the change column: it leaves
        // agent.tools, and goes on for answers alone.
        s.pick(row_of(&s, "mcp__notes__write"));
        press(&mut s, KeyCode::Enter);
        let c = s.chosen();
        assert_eq!(c.every, Some(vec![]));
        assert_eq!(c.answer, ["Bash(curl *)", "Bash(jq *)", "mcp__notes__write"]);
        assert!(c.said().ends_with("; tools: none"), "{}", c.said());
        // make check taken out of the changes, curl of the answers; saved,
        // the setup done.
        s.pick(row_of(&s, "Bash(make check *)"));
        press(&mut s, KeyCode::Char(' '));
        s.pick(row_of(&s, "Bash(curl *)"));
        press(&mut s, KeyCode::Left);
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
    fn space_ticks_the_box_of_the_cursor_s_column_and_a_click_any_box() {
        let mut s = Setup::new(&strings(&["WebFetch"]), &[], &fresh(), "local.toml", "here");
        s.pick(row_of(&s, "WebFetch"));
        // In the read-only column: Space lets answers use it.
        assert_eq!(s.col, Kind::Answer);
        assert!(screen(&mut s).ends_with("\n ↑↓←→: pick · space: let read-only asks use it or not · a: add a command or a tool · s: save · esc: close"));
        press(&mut s, KeyCode::Char(' '));
        assert_eq!((s.chosen().answer, s.chosen().change), (strings(&["Bash(curl *)", "Bash(jq *)", "WebFetch"]), vec![]));
        // → to the change column: Space lets changes use it too.
        press(&mut s, KeyCode::Right);
        assert_eq!(s.col, Kind::Change);
        assert!(screen(&mut s).ends_with("\n ↑↓←→: pick · space: let changes use it or not · a: add a command or a tool · s: save · esc: save or throw away"));
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.chosen().change, ["WebFetch"]);
        assert!(screen(&mut s).contains("\n    [x]      [x]     WebFetch   one of the agent's tools; it can reach the network: a change could touch a remote\n"));
        // ← back, Space takes it out of the answers' list; → at the right
        // stays there; Tab and shift-Tab, each the other column.
        press(&mut s, KeyCode::Left);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!((s.chosen().answer, s.chosen().change), (strings(&["Bash(curl *)", "Bash(jq *)"]), strings(&["WebFetch"])));
        press(&mut s, KeyCode::Left);
        assert_eq!(s.col, Kind::Answer);
        for (code, then) in [(KeyCode::Right, Kind::Change), (KeyCode::Right, Kind::Change), (KeyCode::Tab, Kind::Answer), (KeyCode::BackTab, Kind::Change)] {
            press(&mut s, code);
            assert_eq!(s.col, then, "{code:?}");
        }
        // ↑↓ keep to the column.
        press(&mut s, KeyCode::Up);
        assert_eq!((s.sel, s.col), (row_of(&s, "Skill"), Kind::Change));
        // A click on a box ticks it or not, the cursor going there; one on
        // what a row says picks the row; one between the boxes, the same.
        let shown = screen(&mut s);
        let jq = line_of(&shown, "    [x]      [ ]     Bash(jq *)   stacked's default for answers");
        s.click(5, jq);
        assert_eq!((s.sel, s.col), (row_of(&s, "Bash(jq *)"), Kind::Answer));
        assert_eq!(s.chosen().answer, ["Bash(curl *)"]);
        s.click(14, jq);
        assert_eq!((s.chosen().change, s.col), (strings(&["Bash(jq *)", "WebFetch"]), Kind::Change));
        let before = s.chosen();
        let curl = line_of(&shown, "    [x]      [ ]     Bash(curl *)   stacked's default for answers");
        for x in [30, 10] {
            s.click(x, curl);
            assert_eq!((s.sel, s.chosen()), (row_of(&s, "Bash(curl *)"), before.clone()), "{x}");
        }
        // The mouse over a box picks its row and its column, ticking nothing.
        s.hover(4, jq);
        assert_eq!((s.sel, s.col, s.chosen()), (row_of(&s, "Bash(jq *)"), Kind::Answer, before.clone()));
        // What stacked always gives has no box, and no row of it is picked;
        // nor is the top line, nor the keys'.
        let given = line_of(&shown, "  always    always   Read, Grep, Glob, LS, and git show, log and diff");
        for (x, y) in [(3, given), (13, given), (5, 0), (5, 39)] {
            assert_eq!(s.at(x, y), None, "{x}, {y}");
        }
        // A click on the row to add one opens the line to type it in.
        s.click(25, line_of(&shown, "                     + add a command or a tool…"));
        assert!(s.adding.is_some());
    }

    #[test]
    fn rules_that_mean_the_same_are_one_row_saying_each_source() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("repo");
        executable(&tmp.path().join("bin/searchfox-cli"));
        let path = tmp.path().join("bin").into_os_string();
        // Your claude settings allow it the older way; the instructions name
        // it, which is the newer; one skill allows it the newer way, another
        // the older, and a narrower command of it too.
        std::fs::create_dir_all(top.join(".claude")).unwrap();
        std::fs::write(top.join(".claude/settings.local.json"), r#"{"permissions": {"allow": ["Bash(searchfox-cli:*)"]}}"#).unwrap();
        std::fs::write(top.join("AGENTS.md"), "```sh\nsearchfox-cli --define Foo\n```\n").unwrap();
        let skill = |folder: &str, text: &str| {
            let at = top.join(".claude/skills").join(folder);
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join("SKILL.md"), text).unwrap();
        };
        skill("spec-trace", "---\nname: spec-trace\nallowed-tools: Bash(searchfox-cli *)\n---\n");
        skill("xref", "---\nallowed-tools: Bash(searchfox-cli:*), Bash(searchfox-cli query *)\n---\n");
        // One row, saying each source; ticked for answers, as what your
        // claude settings allow is; written the newer way, as no setting
        // writes it.
        let start = Start::new(&Agent::default(), (false, false), Some(&top), &path);
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        let row = "    [x]      [ ]     Bash(searchfox-cli *)   allowed in .claude/settings.local.json; allowed by the skills spec-trace, xref; named in AGENTS.md";
        assert_eq!(times(&shown, row), 1, "{shown}");
        assert_eq!(times(&shown, "    [ ]      [ ]     Bash(searchfox-cli query *)   allowed by the skill xref"), 1, "narrower, apart: {shown}");
        assert_eq!(shown.matches("searchfox-cli").count(), 3, "and in what saving writes: {shown}");
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(jq *)", "Bash(searchfox-cli *)"]);
        // Let changes use it too: written the same way in both lists.
        s.pick(row_of(&s, "Bash(searchfox-cli *)"));
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Char(' '));
        let Step::Save(c) = press(&mut s, KeyCode::Char('s')) else { panic!() };
        assert_eq!((c.change, c.answer), (strings(&["Bash(searchfox-cli *)"]), strings(&["Bash(curl *)", "Bash(jq *)", "Bash(searchfox-cli *)"])));
        // The settings writing it the older way: the row the same, ticked as
        // they say; saved the newer way, in each list it is ticked for.
        let agent = Agent { answer_tools: strings(&["Bash(curl *)", "Bash(searchfox-cli:*)"]), ..Agent::default() };
        let start = Start::new(&agent, (false, true), Some(&top), &path);
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        assert_eq!(times(&shown, row), 1, "{shown}");
        assert!(!shown.contains("Bash(searchfox-cli:*)"), "{shown}");
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(searchfox-cli *)"]);
        s.pick(row_of(&s, "Bash(searchfox-cli *)"));
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Char(' '));
        let c = s.chosen();
        assert_eq!((c.change, c.answer), (strings(&["Bash(searchfox-cli *)"]), strings(&["Bash(curl *)", "Bash(searchfox-cli *)"])));
        // agent.tools writing it the older way: the row ticked in both
        // columns; agent.tools saved with it written the newer way.
        let agent = Agent { tools: strings(&["mcp__moz", "Bash(searchfox-cli:*)"]), ..Agent::default() };
        let mut s = Setup::new(&[], &[], &Start::new(&agent, (false, false), Some(&top), &path), "local.toml", "here");
        assert!(screen(&mut s).contains("\n    [x]      [x]     Bash(searchfox-cli *)   allowed in"), "{}", screen(&mut s));
        let c = s.chosen();
        assert_eq!((c.change, c.answer, c.every), (vec![], strings(&["Bash(curl *)", "Bash(jq *)"]), Some(strings(&["mcp__moz", "Bash(searchfox-cli *)"]))));
        // What is denied twice, each way: said once.
        let start = Start { denied: strings(&["Bash(rm:*)", "Bash(rm *)", "Bash(rm -rf:*)"]), ..fresh() };
        let shown = screen(&mut Setup::new(&[], &[], &start, "local.toml", "here"));
        assert_eq!(shown.matches("Bash(rm").count(), 2, "{shown}");
        assert_eq!(times(&shown, "   Bash(rm -rf:*): denied in .claude/settings.local.json, refused to every ask"), 1, "{shown}");
    }

    #[test]
    fn what_means_otherwise_stays_a_row_of_its_own() {
        let allowed = |rule: &str| Allowed { rule: rule.into(), file: LOCAL_SETTINGS.into() };
        let start = Start {
            allowed: ["Bash(./mach file-info:*)", "Bash(./mach *)", "Bash(make)", "Bash(git * push:*)"].map(allowed).to_vec(),
            skilled: vec![Skilled { rule: "Bash(git * push *)".into(), skills: strings(&["push"]) }],
            named: vec![Named { rule: "Bash(make *)".into(), file: "AGENTS.md".into(), found: true }],
            ..fresh()
        };
        let start = Start { answer: strings(&["mcp__moz"]), set: (false, true), ..start };
        let mut s = Setup::new(&strings(&["mcp__moz__get_bug"]), &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        // A row each: the narrower command written the newer way, as every
        // command is; the rest, which no other way of writing means, as
        // written.
        for rule in ["Bash(./mach file-info *)", "Bash(./mach *)", "Bash(make)", "Bash(make *)", "Bash(git * push:*)", "Bash(git * push *)"] {
            assert_eq!(shown.lines().filter(|l| l.contains(&format!("     {rule}   "))).count(), 1, "{rule} in:\n{shown}");
        }
        assert!(!shown.contains("file-info:*"), "{shown}");
        // All of moz's tools, as the settings write them, on the server's row.
        assert_eq!(times(&shown, "    [x]      [ ]     all of the MCP server moz's tools, those to come too (mcp__moz)"), 1, "{shown}");
        assert_eq!(times(&shown, "    [x]      [ ]       get_bug"), 1, "{shown}");
        assert_eq!(s.chosen().answer, ["mcp__moz"]);
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
        // Listed once each, unticked, where they are named said; the
        // settings ticking one win.
        let agent = Agent { change_tools: strings(&["Bash(./tools/run.sh *)"]), ..Agent::default() };
        let start = Start::new(&agent, (true, false), Some(&top), &path);
        assert_eq!(start.named, found);
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        for line in [
            "    [ ]      [ ]     Bash(searchfox-cli *)   named in AGENTS.md",
            "    [ ]      [ ]     Bash(notthere *)   named in AGENTS.md, not found on PATH",
            "    [ ]      [x]     Bash(./tools/run.sh *)   named in AGENTS.md",
        ] {
            assert_eq!(times(&shown, line), 1, "{line:?} in:\n{shown}");
        }
        assert_eq!(s.chosen().change, ["Bash(./tools/run.sh *)"]);
        assert_eq!(s.chosen().answer, ["Bash(curl *)", "Bash(jq *)"]);
        // Named by two files: once, each file said.
        std::fs::write(top.join("CLAUDE.md"), "Build with `./tools/run.sh`.\n").unwrap();
        let start = Start::new(&Agent::default(), (false, false), Some(&top), &path);
        assert_eq!(start.named.iter().filter(|n| n.rule == "Bash(./tools/run.sh *)").count(), 2, "{:?}", start.named);
        let shown = screen(&mut Setup::new(&[], &[], &start, "local.toml", "here"));
        assert_eq!(times(&shown, "    [ ]      [ ]     Bash(./tools/run.sh *)   named in CLAUDE.md, AGENTS.md"), 1, "{shown}");
        // A checkout with none of those files names nothing.
        assert!(named(tmp.path(), &path).is_empty());
    }

    #[test]
    fn what_the_checkout_s_own_claude_settings_allow_is_suggested() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("repo");
        std::fs::create_dir_all(top.join(".claude")).unwrap();
        // Yours, untracked, then the checkout's: a rule each, as written; a
        // tool stacked gives, and one it gives as it wants (git's), left out.
        // What yours denies, every ask is refused, said; what the tracked
        // one denies, the worktrees have already.
        std::fs::write(
            top.join(".claude/settings.local.json"),
            r#"{"permissions": {"allow": ["Bash(searchfox-cli:*)", "Bash(grep:*)", "Bash(git push:*)"], "deny": ["Bash(rm:*)", "Bash(rm:*)", "Bash(a,b)"]}}"#,
        )
        .unwrap();
        std::fs::write(
            top.join(".claude/settings.json"),
            r#"{"permissions": {"allow": ["WebFetch(domain:example.com)", "Read", "Bash(searchfox-cli:*)", "Bash(a,b)"], "deny": ["Bash(tracked:*)"]}}"#,
        )
        .unwrap();
        assert_eq!(denied(&top), ["Bash(rm:*)"]);
        // The instructions name the same program.
        std::fs::write(top.join("AGENTS.md"), "```sh\nsearchfox-cli --define Foo\n```\n").unwrap();
        let found = allowed(&top);
        let one = |rule: &str, file: &str| Allowed { rule: rule.into(), file: file.into() };
        assert_eq!(
            found,
            [
                one("Bash(searchfox-cli:*)", ".claude/settings.local.json"),
                one("WebFetch(domain:example.com)", ".claude/settings.json"),
                one("Bash(searchfox-cli:*)", ".claude/settings.json"),
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
            " Refused to every ask, whatever the lists say",
            "   Bash(rm:*): denied in .claude/settings.local.json, refused to every ask",
            "    [x]      [ ]     Bash(searchfox-cli *)   allowed in .claude/settings.local.json, .claude/settings.json; named in AGENTS.md, not found on PATH",
            "    [x]      [ ]     WebFetch(domain:example.com)   allowed in .claude/settings.json; it can reach the network: a change could touch a remote",
        ] {
            assert_eq!(times(&shown, line), 1, "{line:?} in:\n{shown}");
        }
        assert!(!shown.contains("grep:*") && !shown.contains("tracked"), "{shown}");
        // What is denied is said, not let or not: no box of its own; and it
        // is said above the list.
        assert_eq!(shown.matches("rm:*").count(), 1, "{shown}");
        assert!(line_of(&shown, "   Bash(rm:*): denied in .claude/settings.local.json, refused to every ask") < shown.lines().position(|l| l.starts_with(" read only  change")).unwrap());
        assert!(!s.chosen().answer.iter().chain(&s.chosen().change).any(|t| t.contains("rm:*")));
        // What changes things is not ticked for answers, allowed or not.
        std::fs::write(top.join(".claude/settings.json"), r#"{"permissions": {"allow": ["Edit(src/**)", "Bash"]}}"#).unwrap();
        let start = Start::new(&Agent::default(), (false, false), Some(&top), OsStr::new(""));
        assert_eq!(start.answer, ["Bash(curl *)", "Bash(jq *)", "Bash(searchfox-cli:*)"]);
        // The settings setting the answers' list win.
        let agent = Agent { answer_tools: strings(&["Bash(curl *)"]), ..Agent::default() };
        assert_eq!(Start::new(&agent, (false, true), Some(&top), OsStr::new("")).answer, ["Bash(curl *)"]);
        // A link out of the checkout is not read.
        std::fs::write(tmp.path().join("theirs.json"), r#"{"permissions": {"allow": ["Bash(theirs:*)"], "deny": ["Bash(theirs:*)"]}}"#).unwrap();
        std::fs::remove_file(top.join(".claude/settings.local.json")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("theirs.json"), top.join(".claude/settings.local.json")).unwrap();
        assert!(!allowed(&top).iter().any(|a| a.rule.contains("theirs")), "{:?}", allowed(&top));
        assert!(denied(&top).is_empty(), "{:?}", denied(&top));
        // What stacked gives, as it wants it given: none to suggest.
        for given in ["Read", "Grep(src/**)", "Bash(rg:*)", "Bash(git log *)", "Bash(git push:*)", "Bash(cd *)"] {
            assert!(gives(given), "{given}");
        }
        for not in ["WebFetch", "Bash", "Bash(make:*)", "mcp__moz__get_bug"] {
            assert!(!gives(not), "{not}");
        }
    }

    #[test]
    fn what_the_checkout_s_skills_let_run_is_listed_unticked() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("repo");
        let skill = |folder: &str, text: &str| {
            let at = top.join(".claude/skills").join(folder);
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join("SKILL.md"), text).unwrap();
        };
        // On the line, a comma between each, the skill named in its header.
        skill("commit", "---\nname: jj-commit\ndescription: Commits.\nallowed-tools: Bash(jj commit:*), Bash(jj log:*)\n---\nHow to commit.\n");
        // Listed under it, quoted, a stray comma after one; one no rule;
        // one another skill allows too.
        skill(
            "file-bug",
            "---\nname: file-bug\nallowed-tools:\n  - Bash(./tools/file-bug.py *),\n  - \"Bash(jj log:*)\"\n  - [Agent]\nmodel: inherit\n---\n",
        );
        // A space between each, no name but its folder's; what stacked gives
        // left out.
        skill("lint", "---\ndescription: Lints.\nallowed-tools: Read Grep Bash(./tools/lint *)\n---\n");
        // No header: nothing it says counts.
        skill("plain", "Just text.\nallowed-tools: Bash(nope *)\n");
        // A skill a link takes out of the checkout is not read.
        std::fs::write(tmp.path().join("theirs.md"), "---\nallowed-tools: Bash(theirs *)\n---\n").unwrap();
        std::fs::create_dir_all(top.join(".claude/skills/linked")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("theirs.md"), top.join(".claude/skills/linked/SKILL.md")).unwrap();
        let found = skilled(&top);
        let one = |rule: &str, skills: &[&str]| Skilled { rule: rule.into(), skills: strings(skills) };
        assert_eq!(
            found,
            [
                one("Bash(jj commit:*)", &["jj-commit"]),
                one("Bash(jj log:*)", &["jj-commit", "file-bug"]),
                one("Bash(./tools/file-bug.py *)", &["file-bug"]),
                one("Bash(./tools/lint *)", &["lint"]),
            ]
        );
        // Listed once, unticked in both columns, the skills that allow it
        // said; a command the newer way, however a skill writes it.
        let start = Start::new(&Agent::default(), (false, false), Some(&top), OsStr::new(""));
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        let shown = screen(&mut s);
        for line in [
            "    [ ]      [ ]     Bash(jj commit *)   allowed by the skill jj-commit",
            "    [ ]      [ ]     Bash(jj log *)   allowed by the skills jj-commit, file-bug",
            "    [ ]      [ ]     Bash(./tools/file-bug.py *)   allowed by the skill file-bug",
            "    [ ]      [ ]     Bash(./tools/lint *)   allowed by the skill lint",
        ] {
            assert_eq!(times(&shown, line), 1, "{line:?} in:\n{shown}");
        }
        assert!(!shown.contains("theirs") && !shown.contains("nope") && !shown.contains("Agent]"), "{shown}");
        assert_eq!((s.chosen().change, s.chosen().answer), (vec![], strings(&["Bash(curl *)", "Bash(jq *)"])));
        // The settings ticking one win.
        let agent = Agent { answer_tools: strings(&["Bash(jj log:*)"]), ..Agent::default() };
        let mut s = Setup::new(&[], &[], &Start::new(&agent, (false, true), Some(&top), OsStr::new("")), "local.toml", "here");
        assert!(screen(&mut s).contains("\n    [x]      [ ]     Bash(jj log *)   allowed by the skills jj-commit, file-bug\n"), "{}", screen(&mut s));
        // The skills' folder a link takes out of the checkout: none read.
        let other = tmp.path().join("other");
        std::fs::create_dir_all(other.join(".claude")).unwrap();
        std::os::unix::fs::symlink(top.join(".claude/skills"), other.join(".claude/skills")).unwrap();
        assert!(skilled(&other).is_empty());
    }

    #[test]
    fn the_skill_row_s_read_only_box_keeps_what_it_lets_an_answer_do() {
        // The agent has claude's tool for skills: one row, its warning under
        // it, for its read-only box.
        let mut s = Setup::new(&strings(&["Skill"]), &[], &fresh(), "local.toml", "here");
        let shown = screen(&mut s);
        let lines = [
            "    [ ]      [ ]     Skill   claude's tool for the checkout's skills; one of the agent's tools",
            "                       a skill may run what its own allowed-tools let it, commands that change things too",
            "                       (rewriting history, filing a bug), and an answer runs in your checkout itself,",
            "                       unless agent.answer_only_worktree says otherwise",
        ];
        let at = line_of(&shown, lines[0]);
        for (k, line) in lines.iter().enumerate() {
            assert_eq!(line_of(&shown, line), at + k, "{shown}");
        }
        assert_eq!(shown.matches("Skill").count(), 1, "{shown}");
        // Its read-only box: answers use skills, claude's Skill in their
        // list; its change box, the same for changes.
        s.pick(row_of(&s, "Skill"));
        press(&mut s, KeyCode::Char(' '));
        let c = s.chosen();
        assert_eq!((c.change, c.answer), (vec![], strings(&["Bash(curl *)", "Bash(jq *)", "Skill"])));
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.chosen().change, ["Skill"]);
        // There for answers, the agent saying nothing of it; as the settings
        // tick it, unticked else.
        let start = Start { answer: strings(&["Skill"]), ..fresh() };
        let mut s = Setup::new(&[], &[], &start, "local.toml", "here");
        assert!(screen(&mut s).contains("\n    [x]      [ ]     Skill   claude's tool for the checkout's skills\n"), "{}", screen(&mut s));
        // Added by hand: its row, as it is.
        let mut s = Setup::new(&[], &[], &fresh(), "local.toml", "here");
        assert!(!s.add("Skill"));
        assert_eq!((s.sel, s.chosen().answer), (row_of(&s, "Skill"), strings(&["Bash(curl *)", "Bash(jq *)"])));
    }

    #[test]
    fn a_command_is_added_as_a_row_once_it_reads_as_a_rule() {
        let mut s = Setup::new(&strings(&["Bash"]), &[], &fresh(), "local.toml", "here");
        // `a` opens the line.
        press(&mut s, KeyCode::Char('a'));
        let shown = screen(&mut s);
        assert!(shown.contains("a command or a tool, as claude writes it: Bash(PROGRAM *), WebFetch, mcp__SERVER__TOOL"), "{shown}");
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
        // Added: a row of its own, last, neither box ticked, the cursor on
        // it; nothing to be written for it until a box is.
        press(&mut s, KeyCode::Enter);
        assert!(s.adding.is_none());
        assert_eq!((s.sel, s.chosen().change, s.chosen().answer), (row_of(&s, "Bash(make check *)"), vec![], strings(&["Bash(curl *)", "Bash(jq *)"])));
        let shown = screen(&mut s);
        assert!(shown.contains("\n    [ ]      [ ]     Bash(make check *)   added here\n                     + add a command or a tool…\n"), "{shown}");
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.chosen().change, ["Bash(make check *)"]);
        // On the row for it, Enter opens it. One a row lets use already,
        // however written: that row, as it is, the cursor on it.
        press(&mut s, KeyCode::End);
        assert_eq!(s.rows[s.sel], Row::Add);
        assert!(screen(&mut s).ends_with(" ↑↓: pick · enter: add a command or a tool · s: save · esc: save or throw away"), "{}", screen(&mut s));
        press(&mut s, KeyCode::Enter);
        typed(&mut s, "Bash(jq:*)");
        press(&mut s, KeyCode::Enter);
        assert_eq!((s.sel, s.chosen().answer), (row_of(&s, "Bash(jq *)"), strings(&["Bash(curl *)", "Bash(jq *)"])));
        assert_eq!(screen(&mut s).matches("jq").count(), 2, "its row, and what saving writes: {}", screen(&mut s));
        // Esc goes back, nothing added.
        press(&mut s, KeyCode::Char('a'));
        typed(&mut s, "WebFetch");
        press(&mut s, KeyCode::Esc);
        assert!(s.adding.is_none());
        assert!(!screen(&mut s).contains("WebFetch   "));
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
        assert_eq!((s.menu, s.chosen().answer), (None, strings(&["Bash(jq *)"])));
        press(&mut s, KeyCode::Char('q'));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Stay);
        assert_eq!(s.menu, None);
        // Saved from it; thrown away from it, each the screen's end.
        press(&mut s, KeyCode::Esc);
        let Step::Save(c) = press(&mut s, KeyCode::Enter) else { panic!() };
        assert_eq!(c.answer, ["Bash(jq *)"]);
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
    fn a_long_list_keeps_the_columns_heading_on_its_first_line() {
        let tools: Vec<String> = (1..=30).map(|k| format!("mcp__moz__tool_{k}")).collect();
        let start = Start { answer: strings(&["mcp__moz__*"]), denied: strings(&["Bash(rm:*)"]), done: Some(VERSION), ..fresh() };
        let mut s = Setup::new(&tools, &[], &start, "settings.toml", "here");
        let shown = screen_of(&mut s, 20);
        assert!(shown.contains(" Ticked as the settings say now. Saving writes both lists again."), "{shown}");
        assert!(shown.contains("\n    [x]      [ ]     all of the MCP server moz's tools, those to come too (mcp__moz__*)\n"), "{shown}");
        // Scrolled down: the heading of the columns on the first line of the
        // list, the row picked shown, and the mouse on the rows as drawn.
        press(&mut s, KeyCode::End);
        press(&mut s, KeyCode::Up);
        let shown = screen_of(&mut s, 20);
        let lines: Vec<&str> = shown.lines().collect();
        assert!(lines[2].starts_with(" read only  change   "), "{shown}");
        assert_eq!(lines[17], "    [x]      [ ]       tool_30", "{shown}");
        assert_eq!(s.at(5, 17), Some((s.sel, Some(Kind::Answer))));
        assert_eq!(s.at(5, 2), None, "the heading");
        assert_eq!(s.at(5, 3).map(|(k, _)| s.short(k)), Some("tool_16".to_string()), "{shown}");
        // Back up to the top: what is above the list shows again.
        press(&mut s, KeyCode::Home);
        let shown = screen_of(&mut s, 20);
        assert!(shown.lines().nth(2) == Some(" Refused to every ask, whatever the lists say"), "{shown}");
    }

    #[test]
    fn the_setup_is_asked_line_by_line_too() {
        let tools = strings(&["Bash", "mcp__moz__get_bug", "mcp__moz__get_revision"]);
        let start = Start {
            every: strings(&["mcp__moz__get_bug"]),
            named: vec![Named { rule: "Bash(lint *)".into(), file: "AGENTS.md".into(), found: false }],
            denied: strings(&["Bash(rm:*)", "Bash(rm *)"]),
            ..fresh()
        };
        let mut s = Setup::new(&tools, &strings(&["moz"]), &start, "local.toml", "here");
        // Row by row: curl kept, jq neither, the skills for answers, lint
        // both, moz's tools each, get_bug kept and get_revision for changes,
        // Bash kept. Then a command added after a wrong one, typed the older
        // way, for changes; jq again, written otherwise, for answers again.
        let answers = ["n", "", "n", "r", "b", "e", "", "c", "", "make check", "Bash(make check:*)", "c", "Bash(jq:*)", "r", ""];
        let mut answers = answers.into_iter();
        let (mut asked, mut said) = (Vec::new(), Vec::new());
        let c = ask_lines(
            &mut s,
            &mut |q| {
                asked.push(q.to_string());
                answers.next().unwrap_or_default().to_string()
            },
            &mut |l| said.push(l.to_string()),
        );
        // The list first, as the screen shows it, what is denied above it,
        // said once.
        for (k, line) in [
            " Refused to every ask, whatever the lists say",
            "   Bash(rm:*): denied in .claude/settings.local.json, refused to every ask",
            " Every ask, builds and tests too: agent.tools",
            "   mcp__moz__get_bug: ticked in both columns below; taken out of one, a tool is kept in the other's list alone",
            " read only  change   read only is for answer-only asks; change is for asks that change the stack",
            "  always    always   Read, Grep, Glob, LS, and git show, log and diff",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(said[k], *line, "{said:#?}");
        }
        for line in [
            "    [x]      [ ]     Bash(curl *)   stacked's default for answers",
            "    [ ]      [ ]     Bash(lint *)   named in AGENTS.md, not found on PATH",
            "    [x]      [x]       get_bug",
            "  The MCP server moz has these tools: get_bug, get_revision.",
            "  make check is not a tool's name: a command is let run as Bash(make check).",
            "  Bash(make check:*): added, as Bash(make check *).",
            "  Bash(jq:*): listed already, as Bash(jq *).",
        ] {
            assert!(said.contains(&line.to_string()), "{line:?} in {said:#?}");
        }
        assert!(!said.iter().any(|l| l.contains("rm *")), "{said:#?}");
        assert_eq!(asked[0], "Keep them as they are? [Y/n: one by one]");
        for q in [
            "  Which asks may use Bash(jq *), stacked's default for answers? [R: read only, c: change, b: both, n: neither]",
            "  Which asks may use Skill, claude's tool for the checkout's skills? A skill may run what its own allowed-tools let it, commands that change things too (rewriting history, filing a bug), and an answer runs in your checkout itself, unless agent.answer_only_worktree says otherwise. [r: read only, c: change, b: both, N: neither]",
            "  Which asks may use Bash(lint *), named in AGENTS.md, not found on PATH? [r: read only, c: change, b: both, N: neither]",
            "  Which asks may use all of the MCP server moz's tools, those to come too (mcp__moz__*)? [r: read only, c: change, b: both, n: neither, E: each]",
            "    Which asks may use get_bug? [r: read only, c: change, B: both, n: neither]",
            "    Which asks may use get_revision? [r: read only, c: change, b: both, N: neither]",
            "  Which asks may use Bash, one of the agent's tools? It changes things: an answer could too. It can reach the network: a change could touch a remote. [r: read only, c: change, b: both, N: neither]",
            "Add a command or a tool, as claude writes a permission rule (Bash(PROGRAM *), WebFetch, mcp__SERVER__TOOL), or press Enter for none:",
            "  Which asks may use Bash(make check *), added here? [r: read only, c: change, b: both, N: neither]",
            "  Which asks may use Bash(jq *), stacked's default for answers? [r: read only, c: change, b: both, N: neither]",
        ] {
            assert!(asked.contains(&q.to_string()), "{q:?} in {asked:#?}");
        }
        assert_eq!(c.change, ["Bash(lint *)", "mcp__moz__get_revision", "Bash(make check *)"]);
        assert_eq!(c.answer, ["Bash(curl *)", "Bash(jq *)", "Skill", "Bash(lint *)"]);
        assert_eq!(c.every, None, "moz's get_bug is let both, as it was");
        // Kept as they are, Enter: nothing asked of the rows.
        let mut s = Setup::new(&tools, &strings(&["moz"]), &start, "local.toml", "here");
        let mut asked = Vec::new();
        let c = ask_lines(
            &mut s,
            &mut |q| {
                asked.push(q.to_string());
                String::new()
            },
            &mut |_| {},
        );
        assert_eq!(asked.len(), 2, "{asked:#?}");
        assert_eq!((c.change, c.answer, c.every), (vec![], strings(&["Bash(curl *)", "Bash(jq *)"]), None));
    }
}
