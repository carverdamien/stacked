//! The tools the agent could not use, as items of the review of its work.
//! claude's result lists each call it refused (`permission_denials`): each
//! is read as the rule that would have let it run, one item a rule, so
//! that five calls of `searchfox-cli` are one item, `Bash(searchfox-cli
//! *)`, with the calls it tried. A command's rule is `Bash(PROGRAM *)` for
//! a plain program, and narrower for a broad one (`./mach`, `git`, `sh`,
//! `bash`, `python`, `npx`, `cargo`): a refused `./mach try fuzzy --no-push
//! …` is `Bash(./mach try fuzzy *)`, which may be widened to `Bash(./mach
//! try *)`, then `Bash(./mach *)`; `npx eslint --fix` is `Bash(npx eslint
//! *)`, then `Bash(npx *)`. The narrowest is the rule a change is given the
//! checkout's linter by (`narrowest`), so that no call of the linter is an
//! item of a change's. Each command of a pipe is its own, as claude
//! matches each apart. Another tool's rule is its name, an MCP server's
//! tool widened to all of the server's (`mcp__SERVER__*`). The agent says
//! why it needed each, and how it worked around it (`"refused"` in its
//! answer): that is its comment on the item.
//!
//! Each item is refused, the default; allowed for this work, every round of
//! it until its review closes, sent back or asked again, never saved; or
//! allowed from now on, saved in the checkout's own settings
//! (`.stacked/local.toml`), in its kind of ask's list of its own
//! (`agent.answer_accepted`…), once the work is sent back or its review
//! closes. What claude would refuse whatever is allowed is noted, not
//! offered: what the checkout's claude settings deny, what stacked refuses,
//! and, to an ask that only reads, what changes things. Nor is an ask that
//! only reads ever offered a rule that can write files or run programs,
//! whatever the call that needed it (`can_write`): `Bash(sed *)` of `sed -n
//! 1p`, `Bash(git *)` of `git -C x log`; that is noted too, and no item
//! offered to it widens to such a rule.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{Db, Run, Way};

/// A tool the agent was refused, held with its work for review.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Refusal {
    /// The rules that would let it run, the narrowest first: the one
    /// offered, then those it may be widened to, each as claude writes it.
    pub rules: Vec<String>,
    /// The calls the agent tried that were refused, as said: a command, or
    /// what a tool was given.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,
    /// Why the agent needed it, and how it worked around it, as it said:
    /// empty when it said nothing of it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// Why it is not offered, when it is not: noted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noted: Option<String>,
    /// Your answer to the agent's word on it, written in the review: the
    /// work goes back to the agent with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

impl Refusal {
    /// The rule offered at first: the narrowest.
    pub fn rule(&self) -> &str {
        self.rules.first().map_or("", String::as_str)
    }

    /// Whether it may be allowed: not noted.
    pub fn offered(&self) -> bool {
        self.noted.is_none()
    }
}

/// Where an item of what the agent could not use stands, in the review:
/// refused, the default; allowed for this work; or allowed from now on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allow {
    Refused,
    Work,
    Always,
}

impl Allow {
    /// It, said: `allowed for this work`.
    pub fn said(self) -> &'static str {
        match self {
            Allow::Refused => "refused",
            Allow::Work => "allowed for this work",
            Allow::Always => "allowed from now on",
        }
    }
}

/// What the review's section of the tools the agent could not use is
/// called, as its heading says it.
pub const HEADING: &str = "tools the agent could not use";

/// The mark, in a review, of item `k`: true, allowed from now on; false,
/// allowed for this work; none, refused.
pub fn key(k: usize) -> String {
    format!("tool {k}")
}

/// The mark, in a review, saying item `k` is allowed with its rule `j`, a
/// wider one: none, the narrowest.
fn rule_key(k: usize, j: usize) -> String {
    format!("tool {k} rule {j}")
}

/// Whether `key` is a review's mark of an item of what the agent could not
/// use, or of the rule it is allowed with.
pub fn is_key(key: &str) -> bool {
    key.starts_with("tool ")
}

/// Where item `k` stands, as `marks` say.
pub fn state(marks: &BTreeMap<String, bool>, k: usize) -> Allow {
    match marks.get(&key(k)) {
        Some(true) => Allow::Always,
        Some(false) => Allow::Work,
        None => Allow::Refused,
    }
}

/// Item `k` marked `a` in `marks`.
pub fn set_state(marks: &mut BTreeMap<String, bool>, k: usize, a: Allow) {
    match a {
        Allow::Refused => {
            marks.remove(&key(k));
        }
        Allow::Work => {
            marks.insert(key(k), false);
        }
        Allow::Always => {
            marks.insert(key(k), true);
        }
    }
}

/// Which of item `r`'s rules, at `k`, `marks` pick: its place among them.
pub fn picked(marks: &BTreeMap<String, bool>, k: usize, r: &Refusal) -> usize {
    (1..r.rules.len()).find(|&j| marks.get(&rule_key(k, j)) == Some(&true)).unwrap_or(0)
}

/// Item `r`, at `k`, allowed with its rule `j` from now on, in `marks`.
pub fn set_picked(marks: &mut BTreeMap<String, bool>, k: usize, r: &Refusal, j: usize) {
    for i in 1..r.rules.len() {
        marks.remove(&rule_key(k, i));
    }
    if j > 0 && j < r.rules.len() {
        marks.insert(rule_key(k, j), true);
    }
}

/// The rule item `r`, at `k`, is allowed with, as `marks` pick it.
pub fn rule_of<'a>(r: &'a Refusal, marks: &BTreeMap<String, bool>, k: usize) -> &'a str {
    r.rules.get(picked(marks, k, r)).map_or(r.rule(), String::as_str)
}

/// `rule` added to `rules`, unless a rule there means the same.
fn push_unique(rules: &mut Vec<String>, rule: String) {
    let same = crate::setup::same(&rule);
    if !rules.iter().any(|r| crate::setup::same(r) == same) {
        rules.push(rule);
    }
}

/// The rules `marks` allow of `items`, each once, in their order: those
/// for this work, then those from now on.
pub fn allowed(items: &[Refusal], marks: &BTreeMap<String, bool>) -> (Vec<String>, Vec<String>) {
    let (mut work, mut always) = (Vec::new(), Vec::new());
    for (k, r) in items.iter().enumerate().filter(|(_, r)| r.offered()) {
        let rule = rule_of(r, marks, k).to_string();
        match state(marks, k) {
            Allow::Work => push_unique(&mut work, rule),
            Allow::Always => push_unique(&mut always, rule),
            Allow::Refused => {}
        }
    }
    (work, always)
}

/// What the work of run `id` may use for this work: what it was given for
/// this work as it started, and what its review allows, for this work, of
/// what it was refused. Given the round sent back from it, and the ask
/// asked again in its place.
pub fn carried(db: &Db, id: &str) -> Vec<String> {
    let Some(r) = db.run(id) else { return vec![] };
    let mut out = crate::ask::info(r).map(|i| i.allowed).unwrap_or_default();
    if let Some(h) = &r.held {
        let marks = db.review(id).map(|v| v.marks.clone()).unwrap_or_default();
        for rule in allowed(&h.tools, &marks).0 {
            push_unique(&mut out, rule);
        }
    }
    out
}

/// `rules`, each in backquotes, said in a sentence: `` `a`, `b` ``.
pub fn listed(rules: &[String]) -> String {
    rules.iter().map(|r| format!("`{r}`")).collect::<Vec<_>>().join(", ")
}

// -- the commands of a call --------------------------------------------- //

/// The commands of shell command `line`, as claude matches each against the
/// rules apart: cut at `&&`, `||`, `;`, `|`, `|&`, `&`, new lines and the
/// parentheses of a subshell, none of them in quotes nor in `$(…)`; each
/// trimmed, none empty. An `&` of a redirection (`2>&1`, `&>`) cuts
/// nothing; the lines of a here-document (`<<EOF`, `<<-'EOF'`) are what it
/// reads, no command.
pub fn commands(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t = cur.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
        cur.clear();
    };
    let (mut quote, mut depth, mut i) = (None::<char>, 0usize, 0);
    // The here-documents a command line opened, each its word and whether
    // its lines may start with tabs (`<<-`): read once the line ends.
    let mut docs: Vec<(String, bool)> = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        // A here-document: its word kept with the command, its lines passed
        // over at the end of this one. Not a here-string (`<<<`).
        if quote.is_none() && depth == 0 && c == '<' && chars.get(i + 1) == Some(&'<') && chars.get(i + 2) != Some(&'<') {
            let mut j = i + 2;
            let dash = chars.get(j) == Some(&'-');
            j += usize::from(dash);
            while chars.get(j).is_some_and(|c| *c == ' ' || *c == '\t') {
                j += 1;
            }
            let mut word = String::new();
            match chars.get(j) {
                Some(&q) if q == '\'' || q == '"' => {
                    j += 1;
                    while let Some(&c) = chars.get(j) {
                        j += 1;
                        if c == q {
                            break;
                        }
                        word.push(c);
                    }
                }
                _ => {
                    while let Some(&c) = chars.get(j).filter(|c| !c.is_whitespace() && !";|&<>()".contains(**c)) {
                        if c != '\\' {
                            word.push(c);
                        }
                        j += 1;
                    }
                }
            }
            cur.extend(&chars[i..j]);
            if !word.is_empty() {
                docs.push((word, dash));
            }
            i = j;
            continue;
        }
        // The end of a line that opened here-documents: their lines passed
        // over, each to the line that is its word.
        if quote.is_none() && depth == 0 && c == '\n' && !docs.is_empty() {
            flush(&mut cur, &mut out);
            let mut j = i + 1;
            for (word, dash) in docs.drain(..) {
                while j < chars.len() {
                    let end = (j..chars.len()).find(|&k| chars[k] == '\n').unwrap_or(chars.len());
                    let text: String = chars[j..end].iter().collect();
                    j = end + 1;
                    let text = if dash { text.trim_start_matches('\t') } else { text.as_str() };
                    if text.trim_end() == word {
                        break;
                    }
                }
            }
            i = j;
            continue;
        }
        match quote {
            Some(q) => {
                cur.push(c);
                if c == '\\' && q != '\'' && i + 1 < chars.len() {
                    cur.push(chars[i + 1]);
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\\' if i + 1 < chars.len() => {
                    cur.push(c);
                    cur.push(chars[i + 1]);
                    i += 1;
                }
                '\'' | '"' | '`' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '(' if cur.ends_with('$') || depth > 0 => {
                    depth += 1;
                    cur.push(c);
                }
                ')' if depth > 0 => {
                    depth -= 1;
                    cur.push(c);
                }
                _ if depth > 0 => cur.push(c),
                '(' | ')' | ';' | '\n' => flush(&mut cur, &mut out),
                '&' if cur.ends_with(['>', '<']) || chars.get(i + 1) == Some(&'>') => cur.push(c),
                '&' | '|' => {
                    flush(&mut cur, &mut out);
                    if matches!(chars.get(i + 1), Some('&' | '|')) {
                        i += 1;
                    }
                }
                _ => cur.push(c),
            },
        }
        i += 1;
    }
    flush(&mut cur, &mut out);
    out
}

/// The words of command `cmd`, as written: cut at white space that no quote
/// holds, the quotes kept.
fn words(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None::<char>;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == '\\' && q != '\'' {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                } else if c == q {
                    quote = None;
                }
            }
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => {
                if matches!(c, '\'' | '"' | '`') {
                    quote = Some(c);
                }
                cur.push(c);
                if c == '\\'
                    && let Some(n) = chars.next()
                {
                    cur.push(n);
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether `w` sets a variable for the command it starts (`A=b`).
fn assigns(w: &str) -> bool {
    w.split_once('=')
        .is_some_and(|(k, _)| k.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// `words` past what claude passes over before it matches a command:
/// `time`, `nohup`, `command`, `builtin` and `exec`; `timeout`, `nice` and
/// `stdbuf` with what they are given; and `xargs` alone.
fn unwrapped(words: &[String]) -> &[String] {
    let mut at = 0;
    loop {
        let w = |k: usize| words.get(k).map(String::as_str);
        let option = |k: usize| w(k).is_some_and(|x| x.starts_with('-'));
        let next = match w(at) {
            Some("time") => at + 1 + usize::from(w(at + 1) == Some("-p")),
            Some("nohup" | "builtin" | "exec") => at + 1,
            Some("command" | "xargs") if !option(at + 1) => at + 1,
            Some("timeout") => {
                let mut k = at + 1;
                while option(k) {
                    k += if matches!(w(k), Some("-k" | "-s" | "--kill-after" | "--signal")) { 2 } else { 1 };
                }
                // Its duration.
                if w(k).is_some_and(|d| d.starts_with(|c: char| c.is_ascii_digit())) { k + 1 } else { k }
            }
            Some("nice") => match w(at + 1) {
                Some("-n") => at + 3,
                Some(x) if x.starts_with("--adjustment") || (x.starts_with('-') && x[1..].chars().all(|c| c.is_ascii_digit())) => at + 2,
                _ => at + 1,
            },
            Some("stdbuf") => {
                let mut k = at + 1;
                while option(k) {
                    k += if matches!(w(k), Some("-i" | "-o" | "-e")) { 2 } else { 1 };
                }
                k
            }
            _ => return &words[at.min(words.len())..],
        };
        if next >= words.len() {
            return &[];
        }
        at = next;
    }
}

/// What a shell does itself, or what no rule lets run as a program: no
/// rule is offered for them.
const BUILTIN: [&str; 37] = [
    "cd", "echo", "export", "source", ".", "set", "unset", "alias", "pwd", "pushd", "popd", "true", "false", "test", "[", "[[", "printf",
    "read", "type", "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "{", "}", "!", "local",
    "eval", "exit",
];

/// Whether `w` may be part of a rule as it is: no white space, quote,
/// pattern or list's comma in it.
fn plain(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "._/@:+=-~".contains(c))
}

/// Whether `w` reads as a program's subcommand: a word of small letters,
/// digits and dashes, starting with a letter (`try`, `file-info`).
fn subcommand(w: &str) -> bool {
    w.starts_with(|c: char| c.is_ascii_lowercase()) && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The options npx takes with no value before the package it runs, as npm
/// reads them from its version 7 on.
const NPX_FLAGS: [&str; 4] = ["-y", "--yes", "-q", "--quiet"];

/// The option npx takes with a value, `-p PKG` (`--package`): a package to
/// install. With it, the word after the options is the command to run, not
/// a package.
const NPX_VALUED: [&str; 2] = ["-p", "--package"];

/// cargo's global options that take no value, given before its subcommand.
const CARGO_FLAGS: [&str; 8] = ["-q", "--quiet", "-v", "-vv", "--verbose", "--locked", "--offline", "--frozen"];

/// cargo's global options that take a value.
const CARGO_VALUED: [&str; 4] = ["--color", "--config", "-C", "-Z"];

/// What npx or cargo is asked to run, as one step of its rules
/// (`rules_of`): the words of `args` up to the one that names what runs,
/// which is `named`, with the options before it as written, so that the
/// rule lets the call (`-y eslint` of `npx -y eslint .`, `+nightly --locked
/// clippy` of `cargo +nightly --locked clippy`). The options start at
/// `from`; the words before it are kept as they are (cargo's toolchain).
/// An option is one of `flags`, alone; one of `valued`, with the word after
/// it, or a long one with its value after an `=` (`--color=always`); `--`
/// ends them. Nothing for an option that is none of those, as its value
/// could be the word taken for what runs; for no word `named` after the
/// options; or for a word that no rule holds as it is written.
fn runs(args: &[String], from: usize, flags: &[&str], valued: &[&str], named: fn(&str) -> bool) -> Vec<String> {
    let mut k = from;
    while let Some(w) = args.get(k).map(String::as_str).filter(|w| w.starts_with('-')) {
        if w == "--" {
            k += 1;
            break;
        }
        let joined = valued.iter().any(|v| v.starts_with("--") && w.strip_prefix(v).is_some_and(|value| value.starts_with('=')));
        k += if flags.contains(&w) || joined {
            1
        } else if valued.contains(&w) {
            2
        } else {
            return vec![];
        };
    }
    match args.get(k) {
        Some(w) if named(w) && args[..=k].iter().all(|w| plain(w)) => vec![args[..=k].join(" ")],
        _ => vec![],
    }
}

/// The rules that would let command `cmd` run, the narrowest first, each
/// as claude writes it: `Bash(PREFIX *)`. Its program alone, for a plain
/// program; for a broad one, with what it is asked to do, then wider, a
/// word at a time: `./mach try fuzzy`, then `./mach try`, then `./mach`;
/// `git log`, then `git`; `bash ./run.sh`, then `bash`; `python3 -m pytest`,
/// then `python3`; `npx eslint`, then `npx`; `cargo clippy`, then `cargo`.
/// The options npx or cargo is given before what it runs stay in the
/// narrowest rule as written, so that the rule lets the call: `npx -y
/// eslint`, `cargo +nightly clippy`. What it sets first (`A=b`) stays
/// before the program. None for a shell's own (`cd`, `echo`), nor for what
/// no rule can hold as it is written (a quote in its program, a comma,
/// which would cut in two the list claude is given).
pub fn command_rules(cmd: &str) -> Vec<String> {
    rules_of(cmd).0
}

/// The narrowest rule that would let command `cmd` run, the one a refusal
/// of it is offered with (`command_rules`), if it is narrow: none for a
/// broad program that `cmd` asks nothing narrower of, its rule letting it
/// run anything (`Bash(./mach *)` of `./mach --verbose lint`, `Bash(sh *)`
/// of `sh -c …`, `Bash(npx *)` of `npx` alone), nor for a command no rule
/// holds. `Bash(./mach lint *)` of `./mach lint --fix`, `Bash(npx eslint
/// *)` of `npx eslint --fix`, `Bash(cargo clippy *)` of `cargo clippy
/// --fix`; a plain program's own, `Bash(eslint *)` of `eslint --fix`.
pub fn narrowest(cmd: &str) -> Option<String> {
    let (rules, broad) = rules_of(cmd);
    if broad && rules.len() < 2 {
        return None;
    }
    rules.into_iter().next()
}

/// The rules of `command_rules`, and whether the program of `cmd` is a
/// broad one, whose rule is narrowed by what it is asked to do.
fn rules_of(cmd: &str) -> (Vec<String>, bool) {
    let all = words(cmd);
    let ws = unwrapped(&all);
    let at = ws.iter().position(|w| !assigns(w)).unwrap_or(ws.len());
    let (sets, rest) = ws.split_at(at);
    let Some((program, args)) = rest.split_first() else { return (vec![], false) };
    if BUILTIN.contains(&program.as_str()) || !plain(program) || sets.iter().any(|s| !plain(s)) {
        return (vec![], false);
    }
    let base = program.rsplit('/').next().unwrap_or(program);
    // What a broad program is asked to do, in steps that its wider rules
    // drop one at a time, the last first: `try` and `fuzzy` of `./mach try
    // fuzzy`. Words that go together are one step: `-m` with its module,
    // the options of npx or cargo with what they run.
    let more: Option<Vec<String>> = match base {
        "mach" => Some(args.iter().take(2).take_while(|w| subcommand(w)).cloned().collect()),
        "git" => Some(args.iter().take(1).take_while(|w| subcommand(w)).cloned().collect()),
        "sh" | "bash" | "zsh" => Some(args.iter().take(1).take_while(|w| !w.starts_with('-') && plain(w)).cloned().collect()),
        "python" | "python3" => Some(match args.first().map(String::as_str) {
            Some("-m") => args.get(1).filter(|m| plain(m)).map_or(vec![], |m| vec![format!("-m {m}")]),
            Some(w) if !w.starts_with('-') && plain(w) => vec![w.to_string()],
            _ => vec![],
        }),
        // The package npx runs, a scope or a version part of its word
        // (`@scope/pkg`, `eslint@9`); after `-p PKG`, the command it runs.
        "npx" => Some(runs(args, 0, &NPX_FLAGS, &NPX_VALUED, |w| !w.starts_with('-'))),
        // cargo's subcommand, after the toolchain rustup reads first
        // (`+nightly`) and cargo's global options.
        "cargo" => {
            let toolchain = args.first().is_some_and(|w| w.len() > 1 && w.starts_with('+'));
            Some(runs(args, usize::from(toolchain), &CARGO_FLAGS, &CARGO_VALUED, subcommand))
        }
        _ => None,
    };
    let broad = more.is_some();
    let more = more.unwrap_or_default();
    let head: Vec<&str> = sets.iter().chain([program]).map(String::as_str).collect();
    let rules = (0..=more.len())
        .rev()
        .map(|n| format!("Bash({} *)", head.iter().copied().chain(more[..n].iter().map(String::as_str)).collect::<Vec<_>>().join(" ")))
        .filter(|r| crate::setup::rule(r).is_ok())
        .collect();
    (rules, broad)
}

/// The command `cmd` as claude matches it: past what it passes over
/// (`unwrapped`), its words one space apart.
fn matched(cmd: &str) -> String {
    unwrapped(&words(cmd)).join(" ")
}

/// Whether glob `pattern` matches all of `text`, each `*` matching any
/// run of characters.
fn glob(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|k| glob(rest, &text[k..])),
        Some((c, rest)) => text.first() == Some(c) && glob(rest, &text[1..]),
    }
}

/// Whether what a command rule holds between its parentheses, `inner`,
/// lets command `cmd` run, as claude matches it: the older prefix form,
/// `PREFIX:*`, the prefix alone or followed by a space and more; else a
/// pattern whose `*` matches anything, one at its end after a space
/// matching the command alone too.
fn command_allowed(inner: &str, cmd: &str) -> bool {
    if let Some(prefix) = inner.strip_suffix(":*") {
        return cmd == prefix || cmd.strip_prefix(prefix).is_some_and(|r| r.starts_with(' '));
    }
    if let Some(alone) = inner.strip_suffix(" *")
        && cmd == alone
    {
        return true;
    }
    glob(&inner.chars().collect::<Vec<_>>(), &cmd.chars().collect::<Vec<_>>())
}

/// The host of web link `url`.
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().map(|h| h.rsplit('@').next().unwrap_or(h).split(':').next().unwrap_or(h)).filter(|h| !h.is_empty())
}

/// Whether rule `rule` lets tool `tool` run as it was called, given
/// `input`, or, a command, as `cmd`, one of the commands of its call; as
/// claude matches them, enough of it for what stacked asks: a tool by its
/// name; a command by what its parentheses hold (`command_allowed`); all of
/// an MCP server's tools, or one; what WebFetch fetches, by its domain. A
/// rule of another tool with what it may do (`Read(./secret)`) lets
/// nothing here, as stacked cannot read it.
pub fn lets(rule: &str, tool: &str, input: &Value, cmd: Option<&str>) -> bool {
    let (name, inner) = match rule.split_once('(') {
        Some((n, rest)) => (n, rest.strip_suffix(')')),
        None => (rule, None),
    };
    if let Some(server) = name.strip_prefix("mcp__") {
        let Some(of) = tool.strip_prefix("mcp__") else { return false };
        return match server.split_once("__") {
            None => of.split_once("__").is_some_and(|(s, _)| s == server),
            Some((s, "*")) => of.split_once("__").is_some_and(|(o, _)| o == s),
            Some(_) => glob(&name.chars().collect::<Vec<_>>(), &tool.chars().collect::<Vec<_>>()),
        };
    }
    if name != tool {
        return false;
    }
    match (inner, tool) {
        (None, _) => true,
        (Some(inner), "Bash") => cmd.is_some_and(|c| command_allowed(inner, &matched(c))),
        (Some(inner), "WebFetch") => match (inner.strip_prefix("domain:"), input.get("url").and_then(Value::as_str).and_then(host_of)) {
            (Some(d), Some(h)) => h == d || h.ends_with(&format!(".{d}")),
            _ => false,
        },
        _ => false,
    }
}

// -- what changes things ------------------------------------------------- //

/// Programs that change files.
const WRITERS: [&str; 18] =
    ["rm", "rmdir", "mv", "cp", "mkdir", "touch", "chmod", "chown", "chgrp", "ln", "tee", "dd", "truncate", "install", "patch", "rsync", "unlink", "shred"];

/// What git does that changes the checkout, its history or a remote.
const GIT_WRITES: [&str; 27] = [
    "add", "am", "apply", "branch", "checkout", "cherry-pick", "clean", "clone", "commit", "config", "fetch", "gc", "init", "merge",
    "mv", "notes", "pull", "push", "rebase", "reset", "restore", "revert", "rm", "stash", "submodule", "switch", "worktree",
];

/// Whether command `cmd` writes a file with a redirection: `>` or `>>` to
/// anything but `/dev/null` or another of its outputs (`2>&1`).
fn redirects(cmd: &str) -> bool {
    let ws = words(cmd);
    ws.iter().enumerate().any(|(k, w)| {
        let t = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        let Some(rest) = t.strip_prefix(">>").or_else(|| t.strip_prefix('>')) else { return false };
        let target = if rest.is_empty() { ws.get(k + 1).map_or("", String::as_str) } else { rest };
        !(target.starts_with('&') || target == "/dev/null")
    })
}

/// Whether a call of `tool`, command `cmd` of it if it is Bash, changes
/// things: a tool that writes (`ask::CHANGING`); a program that writes
/// files, git that changes the checkout or a remote, `sed -i`, or a
/// redirection to a file.
fn writes(tool: &str, cmd: Option<&str>) -> bool {
    let Some(cmd) = cmd.filter(|_| tool == "Bash") else { return crate::ask::CHANGING.contains(&tool) };
    let all = words(cmd);
    let ws: Vec<&String> = unwrapped(&all).iter().skip_while(|w| assigns(w)).collect();
    let Some((program, args)) = ws.split_first() else { return false };
    let base = program.rsplit('/').next().unwrap_or(program);
    WRITERS.contains(&base)
        || (base == "git" && args.iter().find(|a| !a.starts_with('-')).is_some_and(|s| GIT_WRITES.contains(&s.as_str())))
        || (matches!(base, "sed" | "perl") && args.iter().any(|a| a.starts_with("-i") || a.starts_with("--in-place")))
        || redirects(cmd)
}

/// Programs that can write files or run another program, whatever the
/// call, by what they are given: a script (sed's `w`, awk's `system`, a
/// language's), the command they run (`xargs`, `env`, `command`), a file to
/// write (`uniq IN OUT`, `tree -o`), or as a shell.
const RUNS: [&str; 18] = [
    "sed", "awk", "gawk", "mawk", "nawk", "uniq", "tree", "xargs", "env", "command", "perl", "python", "python3", "ruby", "node", "sh", "bash", "zsh",
];

/// Whether the rule of command `cmd`, the narrowest, would let it write
/// files or run programs, whatever the call: its program one that writes
/// files (`WRITERS`), or one of `RUNS`; or git, but for what it does that
/// only reads (`claude::GIT_READS`), git asked nothing narrower too, its
/// rule letting it do anything (`Bash(git *)` of `git -C x log`). A rule's
/// prefix is such a command too: `git` of `Bash(git *)`.
fn can_write(cmd: &str) -> bool {
    let all = words(cmd);
    let ws: Vec<&String> = unwrapped(&all).iter().skip_while(|w| assigns(w)).collect();
    let Some((program, args)) = ws.split_first() else { return false };
    let base = program.rsplit('/').next().unwrap_or(program);
    WRITERS.contains(&base)
        || RUNS.contains(&base)
        || (base == "git" && !args.first().is_some_and(|s| crate::claude::GIT_READS.contains(&s.as_str())))
}

// -- the items ----------------------------------------------------------- //

/// Why an item is not offered, when the checkout's claude settings `file`
/// deny it.
fn denied_said(file: &str) -> String {
    format!("the checkout's {file} denies it, and a deny wins over any allow")
}

/// Why an item is not offered, when stacked refuses it.
pub const STACKED_REFUSES: &str =
    "stacked refuses it to every ask but builds and tests, as it could run another program or delete files, and a deny wins over any allow";

/// Why an item is not offered, when stacked refuses it with `rule`, of
/// those it refuses asks in `way`: to every ask but builds and tests
/// (`STACKED_REFUSES`); or, as it would write a file or run a program of
/// what they are given, to the asks that only read (`claude::read_refused`).
fn refused_said(rule: &str, way: Way) -> String {
    match crate::claude::read_refused().split(',').any(|r| r == rule) {
        true => format!("stacked refuses it to {}, as it could write a file or run another program, and a deny wins over any allow", kind_said(way)),
        false => STACKED_REFUSES.to_string(),
    }
}

/// Why an item is not offered, when it changes things and asks in `way`
/// only read.
fn writes_said(way: Way) -> String {
    format!("it changes things, and {} only read", kind_said(way))
}

/// Why an item is not offered, when its rule can write files or run
/// programs, whatever the call (`can_write`), and asks in `way` only read.
fn can_write_said(way: Way) -> String {
    format!("it can write files or run programs, and {} only read", kind_said(way))
}

/// Why an item is not offered, when what it needs was allowed already.
pub const ALREADY: &str = "it is allowed already: claude refused this call for a reason of its own, which no rule changes";

/// Why an item is not offered, when its commands are the shell's own
/// (`cd` elsewhere), which no rule lets run.
pub const SHELLS_OWN: &str = "the shell runs it itself, and claude refused this call for a reason of its own, which no rule changes";

/// The asks made in `way`, said within a sentence: `answer-only asks`.
pub fn kind_said(way: Way) -> &'static str {
    match way {
        Way::Change => "the asks that change the stack",
        Way::Answer => "answer-only asks",
        Way::Test => "builds and tests",
        #[cfg(feature = "try-push")]
        Way::Try => "the asks that select a try push's tasks",
    }
}

/// The most calls an item keeps, as said.
const CALLS: usize = 20;

/// The longest a call is kept, as said.
const CALL_CHARS: usize = 300;

/// What run in `way` was refused, as its stream's result says
/// (`permission_denials`), as items: one a rule, in the order first
/// refused, each with the calls that needed it, and why it is noted, not
/// offered, if it is. Noted: what the checkout's claude settings deny
/// (`denied`, each rule with its file), what stacked refuses (`refused`),
/// what it was `given` already, and, to an ask that only reads, what
/// changes things, and a call whose rule can write files or run programs,
/// however it was called (`can_write`): to such an ask, no wider rule that
/// can is offered either. An item that some call needs offered is offered.
/// The
/// agent's word on each, as it said it in its answer (`said`, its tool and
/// why: `said_of`), is the item's comment: each goes to the items whose
/// rule matches most of the tool it names.
pub fn items(events: &[&Value], way: Way, given: &[String], refused: &[String], denied: &[(String, String)], said: &[(String, String)]) -> Vec<Refusal> {
    let result = events.iter().rev().find(|v| v.get("type").and_then(Value::as_str) == Some("result"));
    let denials = result.and_then(|r| r.get("permission_denials")).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out: Vec<Refusal> = Vec::new();
    for d in &denials {
        let tool = d.get("tool_name").and_then(Value::as_str).unwrap_or("").to_string();
        let input = d.get("tool_input").cloned().unwrap_or(Value::Null);
        if tool.is_empty() {
            continue;
        }
        // A Bash call with no command line holds nothing to allow: passed
        // over, rather than offering the whole of Bash.
        if tool == "Bash" && input.get("command").and_then(Value::as_str).is_none() {
            continue;
        }
        // Why rule `rules`'s call `cmd` is not offered, if it is not; and
        // whether it was given already.
        let noted = |cmd: Option<&str>| -> (Option<String>, bool) {
            if let Some((_, file)) = denied.iter().find(|(r, _)| lets(r, &tool, &input, cmd)) {
                return (Some(denied_said(file)), false);
            }
            if let Some(r) = refused.iter().find(|r| lets(r, &tool, &input, cmd)) {
                return (Some(refused_said(r, way)), false);
            }
            if given.iter().any(|r| lets(r, &tool, &input, cmd)) {
                return (Some(ALREADY.to_string()), true);
            }
            if crate::ask::reads(way) && writes(&tool, cmd) {
                return (Some(writes_said(way)), false);
            }
            if crate::ask::reads(way) && cmd.is_some_and(can_write) {
                return (Some(can_write_said(way)), false);
            }
            (None, false)
        };
        let mut found: Vec<(Vec<String>, String, Option<String>)> = Vec::new();
        match input.get("command").and_then(Value::as_str).filter(|_| tool == "Bash") {
            Some(line) => {
                let call: String = line.trim().chars().take(CALL_CHARS).collect();
                let mut first_given = None;
                for cmd in commands(line) {
                    // To an ask that only reads, no wider rule that can
                    // write files or run programs: `git grep`'s is
                    // `Bash(git grep *)` alone, never widened to `Bash(git
                    // *)`.
                    let rules: Vec<String> = command_rules(&cmd)
                        .into_iter()
                        .enumerate()
                        .filter(|(k, r)| *k == 0 || !crate::ask::reads(way) || !prefix_of(r).is_some_and(can_write))
                        .map(|(_, r)| r)
                        .collect();
                    if rules.is_empty() {
                        continue;
                    }
                    match noted(Some(&cmd)) {
                        (why, true) => {
                            first_given.get_or_insert((rules, why));
                        }
                        (why, false) => found.push((rules, call.clone(), why)),
                    }
                }
                // Each of its commands given already: refused all the same,
                // said so. Each the shell's own: said so too.
                if found.is_empty()
                    && let Some((rules, why)) = first_given
                {
                    found.push((rules, call.clone(), why));
                }
                if found.is_empty()
                    && let Some(word) = commands(line).first().and_then(|c| unwrapped(&words(c)).iter().find(|w| !assigns(w)).cloned()).filter(|w| plain(w))
                {
                    found.push((vec![format!("Bash({word} *)")], call, Some(SHELLS_OWN.to_string())));
                }
            }
            None => {
                let mut rules = vec![tool.clone()];
                if let Some((server, _)) = tool.strip_prefix("mcp__").and_then(|t| t.split_once("__")) {
                    rules.push(format!("mcp__{server}__*"));
                }
                let call = match crate::claude::tool_summary(Some(&input)) {
                    s if s.is_empty() => tool.clone(),
                    s => format!("{tool}: {s}"),
                };
                found.push((rules, call, noted(None).0));
            }
        }
        for (rules, call, why) in found {
            match out.iter_mut().find(|r| r.rules[0] == rules[0]) {
                Some(r) => {
                    if !r.calls.contains(&call) && r.calls.len() < CALLS {
                        r.calls.push(call);
                    }
                    // Offered as soon as one call needs it offered.
                    if why.is_none() {
                        r.noted = None;
                    }
                }
                None => out.push(Refusal { rules, calls: vec![call], noted: why, ..Default::default() }),
            }
        }
    }
    for (tool, why) in said {
        let scores: Vec<usize> = out.iter().map(|r| score(r, tool)).collect();
        let best = scores.iter().copied().max().unwrap_or(0);
        if best == 0 {
            continue;
        }
        for (r, s) in out.iter_mut().zip(scores) {
            if s == best && !r.why.split("\n\n").any(|w| w == why) {
                r.why = if r.why.is_empty() { why.clone() } else { format!("{}\n\n{why}", r.why) };
            }
        }
    }
    out
}

/// What command rule `rule` lets run, its prefix: `./mach try fuzzy` of
/// `Bash(./mach try fuzzy *)`. None for another rule.
fn prefix_of(rule: &str) -> Option<&str> {
    rule.strip_prefix("Bash(")?.strip_suffix(" *)")
}

/// How well what the agent named, `said`, a tool as it called it, names
/// item `r`: for a command, how many of the words of the rule's prefix, its
/// program first, one of the commands `said` names starts with (`Bash:
/// ./mach try fuzzy --no-push`, `Bash(./mach try *)`, `./mach`); for
/// another tool, its name in what is said, else all of its MCP server's
/// tools named. None at all is 0.
fn score(r: &Refusal, said: &str) -> usize {
    let said = said.trim();
    let Some(prefix) = prefix_of(r.rule()) else {
        let tool = r.rule();
        if said.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|w| w == tool) {
            return 100;
        }
        let server = tool.strip_prefix("mcp__").and_then(|t| t.split_once("__")).map(|(s, _)| s);
        return match server {
            Some(s) if said == format!("mcp__{s}__*") || said == format!("mcp__{s}") => 50,
            _ => 0,
        };
    };
    let said = said.strip_prefix("Bash").map_or(said, |s| s.trim_start_matches([':', '(', ' ']));
    let said = said.trim_end_matches([')', ' ']).trim_end_matches(":*").trim_end_matches(" *");
    let ours: Vec<&str> = prefix.split_whitespace().filter(|w| !assigns(w)).collect();
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_string();
    commands(said)
        .iter()
        .map(|cmd| {
            let all = words(cmd);
            let theirs: Vec<&String> = unwrapped(&all).iter().filter(|w| !assigns(w)).collect();
            ours.iter()
                .zip(&theirs)
                .enumerate()
                .take_while(|(k, (o, t))| if *k == 0 { base(o) == base(t) } else { **o == t.as_str() })
                .count()
        })
        .max()
        .unwrap_or(0)
}

/// What the agent said of each tool it was refused, in its answer `text`
/// (its last JSON object): `"refused": [{"tool", "why"}]`, each the tool as
/// it called it, and why it needed it and how it worked around it, what it
/// wrote with references their `refs` block at its end. One with nothing
/// said of it is left out.
pub fn said_of(text: &str) -> Vec<(String, String)> {
    let Some(o) = crate::claude::last_object(text) else { return vec![] };
    o.get("refused")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let o = v.as_object()?;
            let tool = o.get("tool").and_then(Value::as_str)?.trim().to_string();
            let why = o.get("why").map(crate::refs::written).unwrap_or_default().trim().to_string();
            (!tool.is_empty() && !why.is_empty()).then_some((tool, why))
        })
        .collect()
}

/// The checkout `top`'s claude settings' denials, each rule with its file:
/// `.claude/settings.local.json`'s, which stacked passes on to every ask,
/// then `.claude/settings.json`'s, which claude reads in a worktree.
pub fn denied_in(top: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for file in [crate::setup::LOCAL_SETTINGS, ".claude/settings.json"] {
        for r in crate::setup::rules_in(top, file, "deny") {
            if !out.iter().any(|(o, _)| *o == r) {
                out.push((r, file.to_string()));
            }
        }
    }
    out
}

// -- allowed from now on ------------------------------------------------- //

/// Saves `rules`, allowed from now on in a review of the work of an ask in
/// `way`, in the checkout `dir`'s own settings, `.stacked/local.toml`:
/// added to that kind of ask's list of its own (`config::accepted_key`),
/// each once, however it is written, the newer way (`setup::shown`). Never
/// in the settings' file, nor in any other file of the checkout. Where they
/// are saved; else why not: outside a checkout, or git tracking that file.
pub fn save(dir: &Path, way: Way, rules: &[String]) -> std::result::Result<PathBuf, String> {
    let key = crate::config::accepted_key(way);
    let mut list = crate::config::local_value(dir, key)?.unwrap_or_default();
    for r in rules {
        push_unique(&mut list, crate::setup::shown(r));
    }
    crate::config::set_local(dir, key, &list).map_err(|e| why_not(&e))
}

/// Why a checkout's own settings are not written, `e`, said as the review
/// says it: what it says, without what it says of nothing written, which is
/// said otherwise there.
fn why_not(e: &str) -> String {
    e.trim_end_matches("; nothing written").to_string()
}

/// Where the review of the work of run `r`, on database `db`, saves what it
/// allows from now on: its stack's checkout's own settings; or why it
/// cannot: outside a checkout, or git tracking that file.
pub fn local_of(db: &Db, r: &Run) -> std::result::Result<PathBuf, String> {
    let repo = db.stack(&r.stack).ok().and_then(|s| s.repo.clone()).ok_or_else(|| format!("{} has no checkout", r.stack))?;
    crate::config::local_to_write(Path::new(&repo)).map_err(|e| why_not(&e))
}

/// What a review saved of what it allows from now on, once its work is
/// sent back or its review closed: the rules saved, and where; the rules
/// it could not save, and why; and, sent back, those of them allowed for
/// this work instead.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settled {
    pub saved: Vec<String>,
    pub at: Option<PathBuf>,
    pub failed: Vec<String>,
    pub why: Option<String>,
    pub instead: bool,
}

impl Settled {
    /// What it did, said after what came of the review, each part after
    /// `; `: nothing when it did nothing.
    pub fn said(&self, way: Way) -> String {
        let mut out = String::new();
        if let Some(at) = self.at.as_ref().filter(|_| !self.saved.is_empty()) {
            out += &format!("; saved for good in {}, for {}: {}", at.display(), kind_said(way), listed(&self.saved));
        }
        if let Some(why) = self.why.as_ref().filter(|_| !self.failed.is_empty()) {
            let then = if self.instead { "; allowed for this work instead" } else { "" };
            out += &format!("; {} could not be saved: {why}{then}", listed(&self.failed));
        }
        out
    }
}

/// What the review of run `r` allows from now on, as `marks` mark it,
/// saved in its stack's checkout's own settings (`save`), for its kind of
/// ask. With `instead` (the work sent back), a rule that could not be saved
/// is marked allowed for this work in `marks` instead, so that the next
/// round may use it all the same.
pub fn settle(db: &Db, r: &Run, marks: &mut BTreeMap<String, bool>, instead: bool) -> Settled {
    let Some(h) = &r.held else { return Settled::default() };
    let (_, always) = allowed(&h.tools, marks);
    let Some(way) = crate::ask::info(r).map(|i| i.way).filter(|_| !always.is_empty()) else { return Settled::default() };
    let to = match db.stack(&r.stack).ok().and_then(|s| s.repo.clone()) {
        Some(repo) => save(Path::new(&repo), way, &always),
        None => Err(format!("{} has no checkout", r.stack)),
    };
    match to {
        Ok(at) => Settled { saved: always, at: Some(at), ..Default::default() },
        Err(why) => {
            if instead {
                let these: Vec<usize> = (0..h.tools.len()).filter(|&k| state(marks, k) == Allow::Always).collect();
                for k in these {
                    set_state(marks, k, Allow::Work);
                }
            }
            Settled { failed: always, why: Some(why), instead, ..Default::default() }
        }
    }
}

/// What your review of `items`, as `marks` mark them, says to the agent in
/// the thread of the comment that asked, as a turn of yours, once the work
/// goes back to it: each item allowed, and each you answered, by its rule,
/// with your answer. None for none.
pub fn turn(items: &[Refusal], marks: &BTreeMap<String, bool>) -> Option<String> {
    let mut lines = Vec::new();
    for (k, r) in items.iter().enumerate() {
        let state = if r.offered() { state(marks, k) } else { Allow::Refused };
        let now = match state {
            Allow::Refused if r.answer.is_none() => continue,
            Allow::Refused => "still refused.",
            Allow::Work => "you may use it now, for this work.",
            Allow::Always => "you may use it now, and from now on.",
        };
        let answer = r.answer.as_deref().map_or(String::new(), |a| format!(" {}", a.trim()));
        lines.push(format!("- `{}`: {now}{answer}", rule_of(r, marks, k)));
    }
    (!lines.is_empty()).then(|| format!("On the tools you could not use:\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_command_line_is_cut_into_the_commands_claude_matches_apart() {
        assert_eq!(commands("curl -s 'https://a/b?x=1&y=2' | jq '.a | .b'"), ["curl -s 'https://a/b?x=1&y=2'", "jq '.a | .b'"]);
        assert_eq!(commands("cd dom && ./mach build; echo done || true"), ["cd dom", "./mach build", "echo done", "true"]);
        assert_eq!(commands("make 2>&1 | tee log &> /dev/null"), ["make 2>&1", "tee log &> /dev/null"]);
        assert_eq!(commands("echo \"$(git log -1 | head)\" |& wc"), ["echo \"$(git log -1 | head)\"", "wc"]);
        assert_eq!(commands("(cd a && ls)\nrg x"), ["cd a", "ls", "rg x"]);
        assert!(commands("  ;; ").is_empty());
        // A here-document's lines are what it reads, no command.
        assert_eq!(commands("python3 - <<'EOF'\nimport json\nprint(1) | x\nEOF\nrg x"), ["python3 - <<'EOF'", "rg x"]);
        assert_eq!(commands("cat <<-END > f && wc f\n\tbody; more\n\tEND"), ["cat <<-END > f", "wc f"]);
        assert_eq!(commands("grep x <<< \"$y\""), ["grep x <<< \"$y\""]);
    }

    #[test]
    fn a_command_s_rule_is_its_program_narrower_for_a_broad_one_with_wider_choices() {
        assert_eq!(command_rules("searchfox-cli --define Foo"), ["Bash(searchfox-cli *)"]);
        assert_eq!(command_rules("./mach try fuzzy --no-push -q 'test-linux'"), ["Bash(./mach try fuzzy *)", "Bash(./mach try *)", "Bash(./mach *)"]);
        assert_eq!(command_rules("./mach test dom/a/test.html"), ["Bash(./mach test *)", "Bash(./mach *)"]);
        assert_eq!(command_rules("git log --oneline main"), ["Bash(git log *)", "Bash(git *)"]);
        assert_eq!(command_rules("git -C x status"), ["Bash(git *)"]);
        assert_eq!(command_rules("bash ./tools/run.sh a"), ["Bash(bash ./tools/run.sh *)", "Bash(bash *)"]);
        assert_eq!(command_rules("sh -c 'ls'"), ["Bash(sh *)"]);
        assert_eq!(command_rules("python3 -m json.tool"), ["Bash(python3 -m json.tool *)", "Bash(python3 *)"]);
        assert_eq!(command_rules("python3 tools/x.py --all"), ["Bash(python3 tools/x.py *)", "Bash(python3 *)"]);
        // What claude passes over first, passed over; what sets a variable
        // first kept before the program.
        assert_eq!(command_rules("timeout 30 nice -n 5 searchfox-cli x"), ["Bash(searchfox-cli *)"]);
        assert_eq!(command_rules("MOZ_LOG=1 ./mach run"), ["Bash(MOZ_LOG=1 ./mach run *)", "Bash(MOZ_LOG=1 ./mach *)"]);
        // A shell's own, or what no rule holds as written: none.
        for none in ["cd ..", "echo hi", "'./mach' build", "a,b x", "X=$(pwd) make"] {
            assert!(command_rules(none).is_empty(), "{none}");
        }
    }

    #[test]
    fn the_narrowest_rule_is_none_for_a_broad_program_asked_nothing_narrower() {
        // The rule a refusal of each is offered with first.
        for (cmd, rule) in [
            ("./mach lint --fix", "Bash(./mach lint *)"),
            ("./mach try fuzzy -q x", "Bash(./mach try fuzzy *)"),
            ("git clang-format --diff", "Bash(git clang-format *)"),
            ("python3 -m black", "Bash(python3 -m black *)"),
            ("bash ./tools/lint.sh", "Bash(bash ./tools/lint.sh *)"),
            ("eslint --fix", "Bash(eslint *)"),
            ("npx eslint --fix", "Bash(npx eslint *)"),
            ("cargo clippy --fix", "Bash(cargo clippy *)"),
            ("ruff check --fix", "Bash(ruff *)"),
        ] {
            assert_eq!(narrowest(cmd).as_deref(), Some(rule), "{cmd}");
            assert_eq!(command_rules(cmd)[0], rule, "{cmd}");
        }
        // A broad program it asks nothing narrower of, which its rule would
        // let do anything; or what no rule holds: none.
        for none in ["./mach --verbose lint", "./mach", "sh -c 'eslint --fix'", "git -C x status", "python3 -c 'x'", "npx", "cargo", "'./my lint' --fix", "cd x", ""] {
            assert_eq!(narrowest(none), None, "{none}");
        }
    }

    #[test]
    fn npx_s_rule_names_the_package_it_runs_and_cargo_s_its_subcommand() {
        let bash = |rule: &str, cmd: &str| lets(rule, "Bash", &Value::Null, Some(cmd));
        // The narrowest rule, then the wide one. The options before what
        // runs stay as written, so that the rule lets the call.
        for (cmd, rule, wide) in [
            ("npx eslint --fix", "Bash(npx eslint *)", "Bash(npx *)"),
            ("npx -y eslint .", "Bash(npx -y eslint *)", "Bash(npx *)"),
            ("npx --yes -q eslint .", "Bash(npx --yes -q eslint *)", "Bash(npx *)"),
            ("npx -- eslint --fix", "Bash(npx -- eslint *)", "Bash(npx *)"),
            // With -p, what npx installs, then the command it runs.
            ("npx -p typescript tsc --noEmit", "Bash(npx -p typescript tsc *)", "Bash(npx *)"),
            ("npx --package=typescript tsc", "Bash(npx --package=typescript tsc *)", "Bash(npx *)"),
            // A scope or a version is part of the package's word.
            ("npx @scope/pkg run", "Bash(npx @scope/pkg *)", "Bash(npx *)"),
            ("npx eslint@9 .", "Bash(npx eslint@9 *)", "Bash(npx *)"),
            ("npx @scope/pkg@1.2.3", "Bash(npx @scope/pkg@1.2.3 *)", "Bash(npx *)"),
            // cargo's toolchain and global options, then its subcommand.
            ("cargo clippy --fix", "Bash(cargo clippy *)", "Bash(cargo *)"),
            ("cargo +nightly clippy", "Bash(cargo +nightly clippy *)", "Bash(cargo *)"),
            ("cargo --locked test", "Bash(cargo --locked test *)", "Bash(cargo *)"),
            ("cargo -q --color always build", "Bash(cargo -q --color always build *)", "Bash(cargo *)"),
            ("cargo +1.80 -vv --config=build.jobs=2 nextest run", "Bash(cargo +1.80 -vv --config=build.jobs=2 nextest *)", "Bash(cargo *)"),
            ("~/.cargo/bin/cargo fmt", "Bash(~/.cargo/bin/cargo fmt *)", "Bash(~/.cargo/bin/cargo *)"),
        ] {
            assert_eq!(command_rules(cmd), [rule, wide], "{cmd}");
            assert_eq!(narrowest(cmd).as_deref(), Some(rule), "{cmd}");
            assert!(bash(rule, cmd) && bash(wide, cmd), "{rule} and {wide} let {cmd}");
        }
        // Another package, or another of cargo's subcommands: not let.
        assert!(!bash("Bash(npx eslint *)", "npx prettier --write .") && !bash("Bash(npx eslint *)", "npx eslint-x"));
        assert!(!bash("Bash(cargo clippy *)", "cargo run") && !bash("Bash(cargo +nightly clippy *)", "cargo +nightly install x"));
        // Alone; its options with nothing after them; an option it is not
        // known to read alone or with a value, whose value could be the
        // word after it; or what no rule holds as written: the wide rule
        // alone, and no narrowest.
        for (cmd, wide) in [
            ("npx", "Bash(npx *)"),
            ("npx -y", "Bash(npx *)"),
            ("npx -p typescript", "Bash(npx *)"),
            ("npx --registry https://r.example eslint", "Bash(npx *)"),
            ("npx -c 'eslint --fix'", "Bash(npx *)"),
            ("npx 'eslint' --fix", "Bash(npx *)"),
            ("cargo", "Bash(cargo *)"),
            ("cargo +nightly", "Bash(cargo *)"),
            ("cargo --version", "Bash(cargo *)"),
            ("cargo --color clippy", "Bash(cargo *)"),
            ("cargo --frobnicate x clippy", "Bash(cargo *)"),
            ("cargo Clippy", "Bash(cargo *)"),
        ] {
            assert_eq!(command_rules(cmd), [wide], "{cmd}");
            assert_eq!(narrowest(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn the_linter_a_change_is_given_is_the_rule_a_refusal_of_it_is_offered_with() {
        use crate::lint::Linter;
        // The calls an agent makes of the linter, on the files it changed,
        // with or without --fix: the rule given lets each run, and is the
        // one a refusal of it would be offered with first.
        for (linter, calls) in [
            (Linter::Mach, vec!["./mach lint --fix dom/a.js dom/b.js", "./mach lint dom/a.js", "./mach lint -l eslint dom/a.js"]),
            (Linter::Set("eslint --fix".into()), vec!["eslint --fix src/a.js", "eslint src/a.js"]),
            (Linter::Set("npx eslint --fix".into()), vec!["npx eslint --fix src/a.js", "npx eslint src/a.js"]),
            (Linter::Set("npx -y eslint --fix".into()), vec!["npx -y eslint --fix src/a.js", "npx -y eslint src/a.js"]),
            (Linter::Set("cargo clippy --fix".into()), vec!["cargo clippy --fix --allow-dirty", "cargo clippy"]),
            (Linter::Set("cargo +nightly clippy --fix".into()), vec!["cargo +nightly clippy --fix --allow-dirty", "cargo +nightly clippy"]),
            (Linter::Set("ruff check --fix".into()), vec!["ruff check --fix a.py", "ruff check a.py"]),
        ] {
            let rule = linter.rule().unwrap();
            for call in calls {
                assert!(lets(&rule, "Bash", &Value::Null, Some(call)), "{rule} lets {call}");
                assert_eq!(command_rules(call)[0], rule, "{call}");
            }
        }
        // npx's package and cargo's subcommand, not npx nor cargo at large:
        // the rule given lets no other package run, nor `cargo run`, nor
        // `cargo install`.
        let rule = |command: &str| Linter::Set(command.into()).rule();
        assert_eq!(rule("npx eslint --fix").as_deref(), Some("Bash(npx eslint *)"));
        assert_eq!(rule("cargo clippy --fix").as_deref(), Some("Bash(cargo clippy *)"));
        for (linter, other) in [("npx eslint --fix", "npx prettier --write src/a.js"), ("cargo clippy --fix", "cargo run"), ("cargo clippy --fix", "cargo install x")] {
            assert!(!lets(&rule(linter).unwrap(), "Bash", &Value::Null, Some(other)), "{linter}: {other}");
        }
        // npx or cargo with nothing narrower to name, as another broad
        // program asked nothing narrower: none given.
        for none in ["npx", "npx -y", "cargo", "cargo +nightly --locked"] {
            assert_eq!(rule(none), None, "{none}");
        }
        assert_eq!((Linter::Off.rule(), Linter::None.rule()), (None, None));
        // A run of before, not given it: its refused call an item, offered
        // with that rule, to widen. One given it that claude refused all the
        // same: noted, allowed already, as no rule would change it.
        let result = refusing(&[("Bash", command("./mach lint --fix dom/a.js"))]);
        let before = items(&[&result], Way::Change, &strings(&["Read"]), &[], &[], &[]);
        assert_eq!(before.iter().map(|r| (r.rules.clone(), r.offered())).collect::<Vec<_>>(), [(strings(&["Bash(./mach lint *)", "Bash(./mach *)"]), true)]);
        let given = strings(&["Read", Linter::Mach.rule().unwrap().as_str()]);
        let now = items(&[&result], Way::Change, &given, &[], &[], &[]);
        assert_eq!(now.iter().map(|r| (r.rule(), r.noted.as_deref())).collect::<Vec<_>>(), [("Bash(./mach lint *)", Some(ALREADY))]);
        // npx's and cargo's linters likewise, to widen to npx or cargo at
        // large; npx or cargo alone, offered with the wide rule, its only
        // choice.
        let result = refusing(&[
            ("Bash", command("npx eslint --fix src/a.js")),
            ("Bash", command("cargo clippy --fix --allow-dirty")),
            ("Bash", command("npx")),
            ("Bash", command("cargo")),
        ]);
        let before = items(&[&result], Way::Change, &strings(&["Read"]), &[], &[], &[]);
        assert_eq!(
            before.iter().map(|r| (r.rules.clone(), r.offered())).collect::<Vec<_>>(),
            [
                (strings(&["Bash(npx eslint *)", "Bash(npx *)"]), true),
                (strings(&["Bash(cargo clippy *)", "Bash(cargo *)"]), true),
                (strings(&["Bash(npx *)"]), true),
                (strings(&["Bash(cargo *)"]), true),
            ]
        );
        let given = strings(&["Read", &rule("npx eslint --fix").unwrap(), &rule("cargo clippy --fix").unwrap()]);
        let now = items(&[&result], Way::Change, &given, &[], &[], &[]);
        let noted: Vec<(&str, Option<&str>)> = now.iter().map(|r| (r.rule(), r.noted.as_deref())).collect();
        assert_eq!(noted, [("Bash(npx eslint *)", Some(ALREADY)), ("Bash(cargo clippy *)", Some(ALREADY)), ("Bash(npx *)", None), ("Bash(cargo *)", None)]);
    }

    #[test]
    fn a_rule_lets_a_call_as_claude_matches_it() {
        let none = Value::Null;
        let bash = |rule: &str, cmd: &str| lets(rule, "Bash", &none, Some(cmd));
        assert!(bash("Bash(curl *)", "curl -s x") && bash("Bash(curl *)", "curl") && !bash("Bash(curl *)", "curly x"));
        assert!(bash("Bash(rm:*)", "rm -rf x") && bash("Bash(rm:*)", "rm") && !bash("Bash(rm:*)", "rmdir x"));
        assert!(bash("Bash(find *-exec*)", "find . -exec rm {} ;") && !bash("Bash(find *-exec*)", "find . -name x"));
        assert!(bash("Bash", "anything at all") && bash("Bash(searchfox-cli *)", "timeout 5 searchfox-cli x"));
        assert!(!lets("Bash(curl *)", "WebFetch", &none, None));
        let mcp = |rule: &str, tool: &str| lets(rule, tool, &none, None);
        assert!(mcp("mcp__moz__*", "mcp__moz__get_bug") && mcp("mcp__moz", "mcp__moz__get_bug") && mcp("mcp__moz__get_bug", "mcp__moz__get_bug"));
        assert!(!mcp("mcp__moz__*", "mcp__mozz__get_bug") && !mcp("mcp__moz__get_bug", "mcp__moz__put_bug") && mcp("mcp__moz__get_*", "mcp__moz__get_bug"));
        let url = serde_json::json!({"url": "https://bugzilla.mozilla.org/rest/bug/1"});
        assert!(lets("WebFetch(domain:mozilla.org)", "WebFetch", &url, None) && !lets("WebFetch(domain:example.com)", "WebFetch", &url, None));
        assert!(lets("WebFetch", "WebFetch", &url, None) && !lets("Read(./secret)", "Read", &none, None));
    }

    /// A result refusing each of `calls`: a tool, and what it was given.
    fn refusing(calls: &[(&str, Value)]) -> Value {
        let denials: Vec<Value> = calls.iter().map(|(t, i)| serde_json::json!({"tool_name": t, "tool_input": i})).collect();
        serde_json::json!({"type": "result", "result": "{}", "permission_denials": denials})
    }

    fn command(c: &str) -> Value {
        serde_json::json!({"command": c})
    }

    #[test]
    fn refusals_are_one_item_a_rule_with_the_calls_tried_and_the_agent_s_word() {
        let result = refusing(&[
            ("Bash", command("searchfox-cli --define Foo")),
            ("Bash", command("searchfox-cli --define Bar")),
            ("Bash", command("searchfox-cli --define Foo")),
            ("Bash", command("./mach try fuzzy --no-push -q x")),
            ("Bash", command("curl -s https://b.example/1 | jq .id")),
            ("mcp__moz__get_bug", serde_json::json!({"id": 1})),
            // No command line: no item, rather than the whole of Bash.
            ("Bash", serde_json::json!({})),
        ]);
        let said = vec![
            ("Bash: searchfox-cli --define Foo".to_string(), "To find Foo; I used rg instead.".to_string()),
            ("mcp__moz__get_bug".to_string(), "To read bug 1; I read the patch instead.".to_string()),
            ("Bash(./mach try *)".to_string(), "To check the labels.".to_string()),
        ];
        let given = strings(&["Read", "Bash(curl *)"]);
        let items = items(&[&result], Way::Change, &given, &[], &[], &said);
        let rules: Vec<&str> = items.iter().map(Refusal::rule).collect();
        assert_eq!(rules, ["Bash(searchfox-cli *)", "Bash(./mach try fuzzy *)", "Bash(jq *)", "mcp__moz__get_bug"]);
        assert_eq!(items[0].calls, ["searchfox-cli --define Foo", "searchfox-cli --define Bar"]);
        assert_eq!(items[0].why, "To find Foo; I used rg instead.");
        assert_eq!(items[1].rules, ["Bash(./mach try fuzzy *)", "Bash(./mach try *)", "Bash(./mach *)"]);
        assert_eq!(items[1].why, "To check the labels.");
        // A pipe: what it was given already (curl) is no item; the call is
        // listed under the rest.
        assert_eq!(items[2].calls, ["curl -s https://b.example/1 | jq .id"]);
        assert_eq!((items[2].why.as_str(), items[2].noted.as_deref()), ("", None), "not explained: an item all the same");
        assert_eq!(items[3].rules, ["mcp__moz__get_bug", "mcp__moz__*"]);
        assert_eq!(items[3].calls, ["mcp__moz__get_bug: {\"id\": 1}"]);
        assert_eq!(items[3].why, "To read bug 1; I read the patch instead.");
    }

    #[test]
    fn what_claude_would_refuse_whatever_is_allowed_is_noted() {
        let result = refusing(&[
            ("Bash", command("rm -rf build")),
            ("Bash", command("find . -name x -delete")),
            ("Edit", serde_json::json!({"file_path": "a.txt"})),
            ("Bash", command("git commit -am x")),
            ("Bash", command("sed -i s/a/b/ a.txt")),
            ("Bash", command("rg x > out.txt")),
            ("Bash", command("rg y 2>/dev/null")),
            ("Bash", command("ls -la")),
            ("Bash", command("cd /elsewhere && pwd")),
        ]);
        let denied = vec![("Bash(rm:*)".to_string(), ".claude/settings.local.json".to_string())];
        let refused: Vec<String> = crate::claude::refused_tools().split(',').map(String::from).collect();
        let given = strings(&["Bash(ls *)"]);
        let items = items(&[&result], Way::Answer, &given, &refused, &denied, &[]);
        let noted: Vec<(&str, Option<&str>)> = items.iter().map(|r| (r.rule(), r.noted.as_deref())).collect();
        let writes = "it changes things, and answer-only asks only read";
        assert_eq!(
            noted,
            [
                ("Bash(rm *)", Some("the checkout's .claude/settings.local.json denies it, and a deny wins over any allow")),
                ("Bash(find *)", Some(STACKED_REFUSES)),
                ("Edit", Some(writes)),
                ("Bash(git commit *)", Some(writes)),
                ("Bash(sed *)", Some(writes)),
                ("Bash(rg *)", None),
                ("Bash(ls *)", Some(ALREADY)),
                ("Bash(cd *)", Some(SHELLS_OWN)),
            ]
        );
        // rg wrote a file once, and is needed as it reads once: offered.
        assert_eq!(items[5].calls, ["rg x > out.txt", "rg y 2>/dev/null"]);
        // A change may write: offered.
        let change = super::items(&[&result], Way::Change, &given, &[], &[], &[]);
        assert!(change.iter().find(|r| r.rule() == "Edit").is_some_and(Refusal::offered));
    }

    #[test]
    fn a_rule_that_can_write_or_run_programs_is_never_offered_an_ask_that_only_reads() {
        // Calls that only read, of programs that can write files or run
        // programs whatever they are given; git at large, and what git does
        // that changes things, or does not only read; what stacked refuses
        // an ask that only reads; and a program of no such kind.
        let result = refusing(&[
            ("Bash", command("sed -n 1,5p f")),
            ("Bash", command("awk '{print $1}' f")),
            ("Bash", command("sort f | uniq -c")),
            ("Bash", command("git -C x log")),
            ("Bash", command("git commit -m x")),
            ("Bash", command("git reflog")),
            ("Bash", command("env LC_ALL=C grep x f")),
            ("Bash", command("xargs -0 grep x")),
            ("Bash", command("python3 -m json.tool f")),
            ("Bash", command("rg -n Foo src | sed -n 1,5p")),
            ("Bash", command("sort -o out f")),
            ("Bash", command("git grep -O less Foo")),
            ("Bash", command("searchfox-cli --define Foo")),
        ]);
        // An answer, given and refused what stacked gives and refuses one.
        let agent = crate::config::Agent::default();
        let given: Vec<String> = crate::ask::tools_for(Way::Answer, "stacked -f db", &agent, &crate::lint::Linter::None).split(',').map(String::from).collect();
        let refused = crate::ask::refused_of(Way::Answer);
        let answer = items(&[&result], Way::Answer, &given, &refused, &[], &[]);
        let noted: Vec<(&str, Option<&str>)> = answer.iter().map(|r| (r.rule(), r.noted.as_deref())).collect();
        let can = "it can write files or run programs, and answer-only asks only read";
        let writes = "it changes things, and answer-only asks only read";
        let refuses = "stacked refuses it to answer-only asks, as it could write a file or run another program, and a deny wins over any allow";
        assert_eq!(
            noted,
            [
                ("Bash(sed *)", Some(can)),
                ("Bash(awk *)", Some(can)),
                ("Bash(uniq *)", Some(can)),
                ("Bash(git *)", Some(can)),
                ("Bash(git commit *)", Some(writes)),
                ("Bash(git reflog *)", Some(can)),
                ("Bash(env *)", Some(can)),
                ("Bash(xargs *)", Some(can)),
                ("Bash(python3 -m json.tool *)", Some(can)),
                ("Bash(sort *)", Some(refuses)),
                ("Bash(git grep *)", Some(refuses)),
                ("Bash(searchfox-cli *)", None),
            ]
        );
        // rg, sort and grep are given: no item of theirs where another
        // command of the pipe needs one.
        assert_eq!(answer[0].calls, ["sed -n 1,5p f", "rg -n Foo src | sed -n 1,5p"]);
        assert_eq!(answer[2].calls, ["sort f | uniq -c"]);
        // No rule wider than one that can write files or run programs.
        assert_eq!((answer[5].rules.clone(), answer[8].rules.clone(), answer[10].rules.clone()), (strings(&["Bash(git reflog *)"]), strings(&["Bash(python3 -m json.tool *)"]), strings(&["Bash(git grep *)"])));
        // rg alone, given though refused: noted as allowed already.
        let alone = items(&[&refusing(&[("Bash", command("rg -n Foo src"))])], Way::Answer, &given, &refused, &[], &[]);
        assert_eq!(alone.iter().map(|r| (r.rule(), r.noted.as_deref())).collect::<Vec<_>>(), [("Bash(rg *)", Some(ALREADY))]);
        // A run of before, given what an answer was then: git grep and rg
        // offered, as they only read, git grep's rule never widened to the
        // whole of git.
        let before = strings(&["Read", "Grep", "Glob", "LS", "Bash(git show *)", "Bash(git log *)", "Bash(git diff *)"]);
        let then = items(&[&refusing(&[("Bash", command("git grep -n Foo")), ("Bash", command("rg Foo"))])], Way::Answer, &before, &refused, &[], &[]);
        assert_eq!(then.iter().map(|r| (r.rules.clone(), r.offered())).collect::<Vec<_>>(), [(strings(&["Bash(git grep *)"]), true), (strings(&["Bash(rg *)"]), true)]);
        // A change may write and run programs: each offered, as before,
        // with its wider rules.
        let change = items(&[&result], Way::Change, &strings(&["Read"]), &crate::ask::refused_of(Way::Change), &[], &[]);
        assert!(change.iter().all(Refusal::offered), "{change:?}");
        assert_eq!(
            change.iter().map(Refusal::rule).collect::<Vec<_>>(),
            [
                "Bash(sed *)",
                "Bash(awk *)",
                "Bash(sort *)",
                "Bash(uniq *)",
                "Bash(git *)",
                "Bash(git commit *)",
                "Bash(git reflog *)",
                "Bash(env *)",
                "Bash(xargs *)",
                "Bash(python3 -m json.tool *)",
                "Bash(rg *)",
                "Bash(git grep *)",
                "Bash(searchfox-cli *)",
            ]
        );
        assert_eq!(change[6].rules, ["Bash(git reflog *)", "Bash(git *)"]);
        assert_eq!(change[9].rules, ["Bash(python3 -m json.tool *)", "Bash(python3 *)"]);
    }

    #[test]
    fn the_three_states_and_the_rule_picked_are_kept_in_the_review_s_marks() {
        let r = Refusal { rules: strings(&["Bash(./mach try fuzzy *)", "Bash(./mach try *)", "Bash(./mach *)"]), ..Default::default() };
        let other = Refusal { rules: strings(&["WebFetch"]), ..Default::default() };
        let noted = Refusal { rules: strings(&["Edit"]), noted: Some("no".into()), ..Default::default() };
        let mut marks = BTreeMap::new();
        assert_eq!(state(&marks, 0), Allow::Refused);
        set_state(&mut marks, 0, Allow::Work);
        set_picked(&mut marks, 0, &r, 1);
        set_state(&mut marks, 1, Allow::Always);
        set_state(&mut marks, 2, Allow::Always);
        assert_eq!((state(&marks, 0), rule_of(&r, &marks, 0)), (Allow::Work, "Bash(./mach try *)"));
        let items = [r.clone(), other, noted];
        assert_eq!(allowed(&items, &marks), (strings(&["Bash(./mach try *)"]), strings(&["WebFetch"])), "the noted one never");
        set_picked(&mut marks, 0, &r, 0);
        set_state(&mut marks, 1, Allow::Refused);
        assert_eq!(marks.keys().collect::<Vec<_>>(), ["tool 0", "tool 2"]);
        assert!(marks.keys().all(|k| is_key(k)));
        // What the agent is told, the work going back to it.
        let mut items = items;
        items[1].answer = Some("Use the API.".into());
        assert_eq!(
            turn(&items, &marks).as_deref(),
            Some("On the tools you could not use:\n- `Bash(./mach try fuzzy *)`: you may use it now, for this work.\n- `WebFetch`: still refused. Use the API.")
        );
        assert_eq!(turn(&items[2..], &marks), None);
    }

    #[cfg(feature = "try-push")]
    #[test]
    fn what_changes_things_is_noted_for_a_try_push_s_selection_too() {
        let result = refusing(&[("Write", serde_json::json!({"file_path": "x"})), ("Bash", command("rm -f x")), ("Bash", command("jq -r .x graph.json"))]);
        let items = items(&[&result], Way::Try, &[], &[], &[], &[]);
        let writes = "it changes things, and the asks that select a try push's tasks only read";
        let noted: Vec<Option<&str>> = items.iter().map(|r| r.noted.as_deref()).collect();
        assert_eq!(noted, [Some(writes), Some(writes), None]);
        // What can write files or run programs, and what stacked refuses it
        // as an answer, likewise.
        let result = refusing(&[("Bash", command("sed -n 1p graph.json")), ("Bash", command("sort -o out labels.txt"))]);
        let items = super::items(&[&result], Way::Try, &[], &crate::ask::refused_of(Way::Try), &[], &[]);
        let noted: Vec<Option<&str>> = items.iter().map(|r| r.noted.as_deref()).collect();
        assert_eq!(
            noted,
            [
                Some("it can write files or run programs, and the asks that select a try push's tasks only read"),
                Some("stacked refuses it to the asks that select a try push's tasks, as it could write a file or run another program, and a deny wins over any allow"),
            ]
        );
    }

    #[test]
    fn from_now_on_is_saved_only_where_the_checkout_s_own_settings_may_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, plain) = (dir.path().join("db.jsonl"), dir.path().join("plain"));
        // A stack of a checkout that is one no more.
        std::fs::create_dir(&plain).unwrap();
        let git = |args: &[&str]| crate::git::git(&plain, args).unwrap();
        git(&["init", "-q"]);
        git(&["-c", "user.name=A", "-c", "user.email=a@x", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "base"]);
        let base = git(&["rev-parse", "HEAD"]).trim().to_string();
        let run = |stack: &str| Run { id: format!("r-{stack}"), stack: stack.into(), ..Default::default() };
        Db::edit(&db_path, |d| {
            d.new_stack("plain", &base, &plain.display().to_string())?;
            d.add_run(run("plain"))?;
            d.add_run(run("gone"))
        })
        .unwrap();
        std::fs::remove_dir_all(plain.join(".git")).unwrap();
        let d = Db::load(&db_path).unwrap();
        // Of a repository that is no checkout, or of a stack there is not:
        // why said, nothing saved.
        let e = local_of(&d, d.run("r-plain").unwrap()).unwrap_err();
        assert!(e.contains("is not in a git checkout"), "{e}");
        assert_eq!(local_of(&d, d.run("r-gone").unwrap()), Err("gone has no checkout".to_string()));
        let e = save(&plain, Way::Answer, &strings(&["WebFetch"])).unwrap_err();
        assert!(e.contains("is not in a git checkout"), "{e}");
        assert!(!plain.join(".stacked").exists());
        // In a checkout: its own settings, each rule once, the newer way.
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        crate::git::git(&repo, &["init", "-q"]).unwrap();
        let at = save(&repo, Way::Test, &strings(&["Bash(make:*)", "WebFetch"])).unwrap();
        save(&repo, Way::Test, &strings(&["Bash(make *)", "mcp__moz__*"])).unwrap();
        assert_eq!(at, repo.canonicalize().unwrap().join(".stacked/local.toml"));
        assert_eq!(std::fs::read_to_string(&at).unwrap(), "[agent]\ntest_accepted = [\"Bash(make *)\", \"WebFetch\", \"mcp__moz__*\"]\n");
    }

    #[test]
    fn the_agent_s_word_on_each_tool_is_read_from_its_answer() {
        let text = r#"Done. {"replies": {"c1": "x"}, "refused": [{"tool": "Bash: searchfox-cli --define Foo", "why": "To find Foo."}, {"tool": "WebFetch", "why": {"text": "To read @r1.", "refs": [{"url": "https://example.com/a"}]}}, {"tool": "Edit", "why": ""}, "x"]}"#;
        let said = said_of(text);
        assert_eq!(said[0], ("Bash: searchfox-cli --define Foo".to_string(), "To find Foo.".to_string()));
        assert!(said[1].0 == "WebFetch" && said[1].1.starts_with("To read @r1.") && said[1].1.contains("https://example.com/a"), "{said:?}");
        assert_eq!(said.len(), 2);
        assert!(said_of("no answer").is_empty());
    }
}
