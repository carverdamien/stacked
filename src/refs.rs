//! References an agent attaches to what it writes: lines of a file, as
//! ranges, or the whole file, as a patch of the stack leaves the file, as
//! the stack's base has it, or as the whole stack leaves it, which reaches
//! the files the stack does not touch too; another comment of the stack;
//! or a page of the web, by its link, for what nothing in the stack says.
//! Kept in the turn they come with, as a `refs` block, a reference a line:
//! `path:120-140,200-210 @3` (`path @3` the whole file; `@0` the base; no
//! `@`, the whole stack), a comment's change and id as JSON, or a link as
//! it is, `https://…`; neither is ever read as a path. Under the comment, a
//! line each; Enter previews a reference's lines, and Enter again goes
//! there; Enter on a comment goes to it; Enter on a link shows it whole,
//! and Enter again opens it in the browser (`open_link`).
//!
//! The references you attach to a comment are the comment's own: each
//! numbered once and for all, `r1`, `r2`, which what you write names:
//! `@r1`, `@r{1,3-4}` several, `@r*` all of them. The agent reads them with
//! the comment, the comments as they stand, open or closed.
//!
//! What names a reference is a mention in a turn that stands, whoever wrote
//! it, or in the agent's reply waiting for review, which goes in once
//! accepted. A rewind or an edit that takes away the last mention of one
//! detaches it, as Detach does: one you attached from the comment, one a
//! turn attaches out of its block. One that nothing named, attached for
//! what it says, stays; one a turn attaches without naming it in its text
//! counts as named by its turn, and goes only with it. A turn written
//! again keeps the references it attaches, but those whose last mention
//! the edit took away. A turn deleted the first way counts nowhere: its
//! references are not the comment's, its mentions name nothing. A number
//! is never given again in a comment, its reference gone or not.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{Db, Note};

/// A reference: `file`'s lines `ranges` (from 1, both ends in, sorted,
/// apart), as `at` has the file; with none, the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ref {
    pub file: String,
    pub ranges: Vec<(usize, usize)>,
    pub at: At,
}

/// Which version of the file a reference's lines are of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum At {
    /// As the whole stack leaves it.
    Top,
    /// As the stack's base has it.
    Base,
    /// As patch `n` (from 1) leaves it.
    Patch(usize),
}

/// The tag of a `refs` block's fence.
const TAG: &str = "refs";

/// Ranges of lines as written, `120-140,200-210` or `7`: from 1, sorted,
/// those that touch merged. None if one is not a range.
pub fn ranges(s: &str) -> Option<Vec<(usize, usize)>> {
    let mut all: Vec<(usize, usize)> = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse().ok()?, b.trim().parse().ok()?),
            None => {
                let n = part.parse().ok()?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return None;
        }
        all.push((a, b));
    }
    all.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in all {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Ranges as `sep` joins them: `120-140,200-210`.
fn ranges_said(ranges: &[(usize, usize)], sep: &str) -> String {
    ranges
        .iter()
        .map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") })
        .collect::<Vec<_>>()
        .join(sep)
}

/// Whether `s` starts as an address does, a scheme then `://` (`https://`,
/// `ftp://`): no path of a file does, and none is read as one.
pub fn addressed(s: &str) -> bool {
    let s = s.trim_start();
    s.find("://").is_some_and(|k| {
        let scheme = &s[..k];
        scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// The web link `text` writes, if it is one: an address starting with
/// `https://` or `http://` (in any case), naming a site, with no space in
/// it, kept whole. Else why it is not, said.
pub fn link(text: &str) -> std::result::Result<String, String> {
    let url = text.trim();
    let whole = "write it whole, as https://example.com/a/b";
    if url.is_empty() {
        return Err(format!("write the web link to attach: {whole}"));
    }
    let lower = url.to_ascii_lowercase();
    let Some(scheme) = ["https://", "http://"].into_iter().find(|s| lower.starts_with(s)) else {
        return Err(format!("a web link starts with https:// or http://: {whole}"));
    };
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("a web link has no spaces in it: write each one as %20".into());
    }
    // Its site: after who logs in to it, before its port.
    let site = url[scheme.len()..].split(['/', '?', '#']).next().unwrap_or_default();
    let host = site.rsplit_once('@').map_or(site, |(_, h)| h);
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() {
        return Err(format!("the link names no site after {}: {whole}", &url[..scheme.len()]));
    }
    Ok(url.to_string())
}

impl Ref {
    /// Whether it points at the whole file, not at lines of it.
    pub fn whole(&self) -> bool {
        self.ranges.is_empty()
    }

    /// As a line of a `refs` block: `path:120-140,200-210 @3`; the whole
    /// file, `path @3`.
    pub fn line(&self) -> String {
        let at = match self.at {
            At::Top => String::new(),
            At::Base => " @0".into(),
            At::Patch(n) => format!(" @{n}"),
        };
        if self.whole() {
            return format!("{}{at}", self.file);
        }
        format!("{}:{}{at}", self.file, ranges_said(&self.ranges, ","))
    }

    /// A line of a `refs` block, if it is one: with no lines (`path @3`,
    /// `path: @3`), the whole file. Not a web link, nor any address.
    pub fn parse(line: &str) -> Option<Ref> {
        let line = line.trim();
        if addressed(line) {
            return None;
        }
        let (spec, at) = match line.rsplit_once(" @") {
            Some((spec, n)) => (
                spec,
                match n.trim().parse::<usize>().ok()? {
                    0 => At::Base,
                    n => At::Patch(n),
                },
            ),
            None => (line, At::Top),
        };
        let (file, lines) = match spec.rsplit_once(':') {
            Some((file, lines)) if lines.trim().is_empty() => (file, vec![]),
            Some((file, lines)) => (file, ranges(lines)?),
            None => (spec, vec![]),
        };
        let file = file.trim();
        (!file.is_empty()).then_some(())?;
        Some(Ref {
            file: file.to_string(),
            ranges: lines,
            at,
        })
    }

    /// One the agent gave: `{"file", "lines", "patch"}`, no `"lines"` for
    /// the whole file, `"patch"` 0 for the base, none for the whole stack.
    /// A file that is an address is none.
    pub fn from_json(v: &Value) -> Option<Ref> {
        let file = v.get("file")?.as_str()?.trim();
        if file.is_empty() || addressed(file) {
            return None;
        }
        let lines = match v.get("lines") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Some(_) => return None,
        };
        let at = match v.get("patch").and_then(|p| p.as_u64().or_else(|| p.as_str()?.trim().parse().ok())) {
            None => At::Top,
            Some(0) => At::Base,
            Some(n) => At::Patch(n as usize),
        };
        Some(Ref {
            file: file.to_string(),
            ranges: if lines.trim().is_empty() { vec![] } else { ranges(&lines)? },
            at,
        })
    }

    /// As the comment shows it: `a.txt:2-3, 10 · as patch 1 leaves it`;
    /// the whole file, `a.txt · as patch 1 leaves it`.
    pub fn said(&self) -> String {
        if self.whole() {
            return format!("{} · {}", self.file, self.at_said());
        }
        format!("{}:{} · {}", self.file, ranges_said(&self.ranges, ", "), self.at_said())
    }

    /// Which version of the file it is of, said: `as patch 1 leaves it`.
    pub fn at_said(&self) -> String {
        match self.at {
            At::Top => "as the stack leaves it".into(),
            At::Base => "as the stack's base has it".into(),
            At::Patch(n) => format!("as patch {n} leaves it"),
        }
    }
}

/// What a line of a `refs` block points at: lines of a file, a comment of
/// the stack, by its change and id, or a page of the web, by its link.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pointer {
    Lines(Ref),
    Comment { change: String, id: String },
    Link(String),
}

impl Pointer {
    /// As a line of a `refs` block: a comment's as JSON, a link as it is,
    /// which no path is.
    pub fn line(&self) -> String {
        match self {
            Pointer::Lines(r) => r.line(),
            Pointer::Comment { change, id } => serde_json::json!({"change": change, "id": id}).to_string(),
            Pointer::Link(url) => url.clone(),
        }
    }

    /// A line of a `refs` block, if it is one: an address is a link, if it
    /// is one, and never lines.
    pub fn parse(line: &str) -> Option<Pointer> {
        let line = line.trim();
        if line.starts_with('{') {
            let v: Value = serde_json::from_str(line).ok()?;
            return Some(Pointer::Comment {
                change: v.get("change")?.as_str()?.to_string(),
                id: v.get("id")?.as_str()?.to_string(),
            });
        }
        if addressed(line) {
            return link(line).ok().map(Pointer::Link);
        }
        Ref::parse(line).map(Pointer::Lines)
    }
}

/// A line of a `refs` block: what it points at, and its number, which
/// `@r4` names; references written before they were numbered have none.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pointing {
    pub n: Option<usize>,
    pub to: Pointer,
}

impl Pointing {
    /// As a line of a `refs` block: its number first, `r4 a.txt:2 @1`.
    pub fn line(&self) -> String {
        match self.n {
            Some(n) => format!("r{n} {}", self.to.line()),
            None => self.to.line(),
        }
    }

    /// A line of a `refs` block, if it is one.
    pub fn parse(line: &str) -> Option<Pointing> {
        let line = line.trim();
        if let Some((name, rest)) = line.split_once(' ')
            && let Some(n) = name.strip_prefix('r').and_then(|n| n.parse::<usize>().ok())
            && let Some(to) = Pointer::parse(rest)
        {
            return Some(Pointing { n: Some(n), to });
        }
        Pointer::parse(line).map(|to| Pointing { n: None, to })
    }
}

/// What the agent pointed at in a `refs` list: lines, as it gave them; a
/// comment, by the name it gave it, a label of the ask's (`c1`) or an id
/// as `stacked comments` lists it; a page of the web, by its link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Given {
    Lines(Ref),
    Comment(String),
    Link(String),
    /// One that does not read as any of them: it keeps its place, and its
    /// number with it.
    Unread,
}

/// The references the agent gave, `v` (a list), each in its place, those
/// it cannot read too, so that what names the others still does: a link
/// that is not of the web (`{"url": "ftp://…"}`) is one of those.
pub fn of_json(v: Option<&Value>) -> Vec<Given> {
    let given = |r: &Value| match (r.get("url"), r.get("comment")) {
        (Some(u), _) => u.as_str().and_then(|u| link(u).ok()).map(Given::Link),
        (None, Some(c)) => c.as_str().map(|c| Given::Comment(c.trim().to_string())).filter(|g| *g != Given::Comment(String::new())),
        (None, None) => Ref::from_json(r).map(Given::Lines),
    };
    v.and_then(Value::as_array).map_or(vec![], |a| a.iter().map(|r| given(r).unwrap_or(Given::Unread)).collect())
}

/// What an agent wrote, `v`: its text and the references it attaches; a
/// string attaches none, an object has its `text` and its `refs`.
pub fn text_and_refs(v: &Value) -> (String, Vec<Given>) {
    match v {
        Value::String(s) => (s.clone(), vec![]),
        Value::Object(o) if o.contains_key("text") => {
            let text = match o.get("text") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => String::new(),
                Some(t) => t.to_string(),
            };
            (text, of_json(o.get("refs")))
        }
        Value::Null => (String::new(), vec![]),
        other => (other.to_string(), vec![]),
    }
}

/// The references the agent gave, the comments it named found by
/// `comment` (their change and id): those it names that are none, and
/// those that do not read, left out.
pub fn resolved(given: Vec<Given>, first: Option<usize>, comment: &dyn Fn(&str) -> Option<(String, String)>) -> Vec<Pointing> {
    let mut out: Vec<Pointing> = Vec::new();
    for (k, g) in given.into_iter().enumerate() {
        let to = match g {
            Given::Lines(r) => Some(Pointer::Lines(r)),
            Given::Comment(name) => comment(&name).map(|(change, id)| Pointer::Comment { change, id }),
            Given::Link(url) => Some(Pointer::Link(url)),
            Given::Unread => None,
        };
        let Some(to) = to else { continue };
        match first {
            // Numbered by where it was given: one left out, its number with
            // it, so that what names the others still does.
            Some(f) => out.push(Pointing { n: Some(f + k), to }),
            None if !out.iter().any(|p| p.to == to) => out.push(Pointing { n: None, to }),
            None => {}
        }
    }
    out
}

/// What an agent wrote, `v`, its references in a `refs` block at its end,
/// numbered from `first` in the order it gave them, the comments it names
/// found by `comment`.
pub fn written_with(v: &Value, first: usize, comment: &dyn Fn(&str) -> Option<(String, String)>) -> String {
    let (text, given) = text_and_refs(v);
    attach(&text, &resolved(given, Some(first), comment))
}

/// What an agent wrote, `v`, its references in a `refs` block at its end,
/// not numbered: lines and links only, as no comment can be found here.
pub fn written(v: &Value) -> String {
    let (text, given) = text_and_refs(v);
    attach(&text, &resolved(given, None, &|_| None))
}

/// `text`, with `refs` in a `refs` block at its end; as it is without any.
pub fn attach(text: &str, refs: &[Pointing]) -> String {
    if refs.is_empty() {
        return text.to_string();
    }
    let lines: Vec<String> = refs.iter().map(Pointing::line).collect();
    format!("{}\n\n```{TAG}\n{}\n```", text.trim_end(), lines.join("\n"))
}

/// `body`, a turn's text, without its `refs` blocks; and the references
/// they hold, in order.
pub fn split(body: &str) -> (String, Vec<Pointing>) {
    let (mut kept, mut refs) = (Vec::new(), Vec::new());
    let mut lines = body.lines();
    while let Some(l) = lines.next() {
        if l.trim() == format!("```{TAG}") {
            for r in lines.by_ref() {
                if r.trim() == "```" {
                    break;
                }
                refs.extend(Pointing::parse(r));
            }
            continue;
        }
        kept.push(l);
    }
    // The blank line before a block taken out goes with it.
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    (kept.join("\n"), refs)
}

/// What a reference you attach to a comment points at: another comment of
/// the stack, lines of a file, as a `refs` line says them, or a page of the
/// web, by its link, whole. Kept as `"comment"`, `"lines"` or `"link"`:
/// what comments saved before links were kept still reads.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum To {
    Comment { change: String, id: String },
    Lines(String),
    Link(String),
}

/// A reference you attach to a comment: its number, which `@r1` names, and
/// what it points at. A number is never given twice in a comment, not even
/// once its reference is detached (`next_of`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Attached {
    pub n: usize,
    #[serde(flatten)]
    pub to: To,
}

impl Attached {
    /// Its name, as a mention names it: `r1`.
    pub fn name(&self) -> String {
        format!("r{}", self.n)
    }

    /// The lines it points at, if it does.
    pub fn lines(&self) -> Option<Ref> {
        match &self.to {
            To::Lines(l) => Ref::parse(l),
            To::Comment { .. } | To::Link(_) => None,
        }
    }

    /// What it points at, as a `refs` line says it.
    pub fn pointer(&self) -> Option<Pointer> {
        match &self.to {
            To::Comment { change, id } => Some(Pointer::Comment {
                change: change.clone(),
                id: id.clone(),
            }),
            To::Lines(_) => self.lines().map(Pointer::Lines),
            To::Link(url) => link(url).ok().map(Pointer::Link),
        }
    }
}

/// The number past those of `attached`; a comment's next reference takes
/// `next_of`'s, past those it had too.
pub fn next_n(attached: &[Attached]) -> usize {
    attached.iter().map(|a| a.n).max().unwrap_or(0) + 1
}

/// What a mention names: every reference attached (`@r*`), some of them
/// (`@r2`, `@r{1,3-4}`), or nothing it reads as (`@r{1,`, `@r0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Names {
    All,
    Some(Vec<usize>),
    Bad,
}

/// A mention of attached references in a text: where it is, in bytes, and
/// what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub at: std::ops::Range<usize>,
    pub names: Names,
}

/// Whether `c` is of a word: a mention is not one inside it, as an
/// address's `@`.
fn wordy(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// The numbers a mention's braces hold, `1,3-4`: none if one is not a
/// number or a range of them, from 1.
fn listed(inner: &str) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for part in inner.split(',').map(str::trim) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>().ok()?, b.trim().parse::<usize>().ok()?),
            None => {
                let n = part.parse::<usize>().ok()?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return None;
        }
        out.extend(a..=b);
    }
    Some(out)
}

/// The mentions of attached references in `text`: `@r1`, `@r{1,2,3}`,
/// `@r{2-4}`, `@r*`. One that does not read is a mention too, to say so:
/// `@r{1,`, `@r0`. Not `@return`, nor an address's `x@r1`.
pub fn mentions(text: &str) -> Vec<Mention> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = text[from..].find("@r") {
        let at = from + k;
        from = at + 2;
        if wordy(text[..at].chars().next_back()) {
            continue;
        }
        let rest = &text[at + 2..];
        if rest.starts_with('*') {
            out.push(Mention {
                at: at..at + 3,
                names: Names::All,
            });
            from = at + 3;
            continue;
        }
        if let Some(inner) = rest.strip_prefix('{') {
            // Its braces close before the line ends and the next mention
            // starts, else they are not closed.
            let own = &inner[..inner.find(['\n', '@']).unwrap_or(inner.len())];
            let (len, names) = match own.find('}') {
                Some(close) => (close + 2, listed(&own[..close]).map_or(Names::Bad, Names::Some)),
                // Not closed: as far as a list could go.
                None => {
                    let list = own.find(|c: char| !(c.is_ascii_digit() || matches!(c, ',' | '-' | ' '))).map_or(own, |k| &own[..k]);
                    (1 + list.trim_end().len(), Names::Bad)
                }
            };
            out.push(Mention { at: at..at + 2 + len, names });
            from = at + 2 + len;
            continue;
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || wordy(rest[digits..].chars().next()) {
            continue;
        }
        let n: usize = rest[..digits].parse().unwrap_or(0);
        out.push(Mention {
            at: at..at + 2 + digits,
            names: if n == 0 { Names::Bad } else { Names::Some(vec![n]) },
        });
        from = at + 2 + digits;
    }
    out
}

/// Whether mention `m` names only references of those numbered `ns`.
pub fn holds(m: &Mention, ns: &[usize]) -> bool {
    match &m.names {
        Names::All => !ns.is_empty(),
        Names::Some(named) => named.iter().all(|n| ns.contains(n)),
        Names::Bad => false,
    }
}

/// The mentions in `text` that name a reference not attached, of those
/// numbered `ns`, as they are written.
pub fn broken<'a>(text: &'a str, ns: &[usize]) -> Vec<&'a str> {
    mentions(text).into_iter().filter(|m| !holds(m, ns)).map(|m| &text[m.at]).collect()
}

/// The mentions in what you wrote in comment `n` that name a reference not
/// attached to it, as they are written.
pub fn broken_in(n: &Note) -> Vec<String> {
    let ns = numbers(n);
    crate::thread::thread_turns(&n.text)
        .iter()
        .filter(|t| t.role == crate::thread::Role::You)
        .flat_map(|t| broken(&t.body, &ns).into_iter().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

/// The comments comment `n` points at, by change and id: those attached to
/// it, then those its turns that stand point at.
pub fn pointed(n: &Note) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = n
        .attached
        .iter()
        .filter_map(|a| match &a.to {
            To::Comment { change, id } => Some((change.clone(), id.clone())),
            To::Lines(_) | To::Link(_) => None,
        })
        .collect();
    for t in crate::thread::thread_turns(&n.text) {
        for p in split(&t.body).1 {
            if let Pointer::Comment { change, id } = p.to
                && !out.contains(&(change.clone(), id.clone()))
            {
                out.push((change, id));
            }
        }
    }
    out
}

/// Comment `n`'s references by number: those attached to it, and those its
/// turns that stand attach (the agent's), each with whether the agent
/// attached it.
pub fn references(n: &Note) -> Vec<(usize, Pointer, bool)> {
    let mut out: Vec<(usize, Pointer, bool)> = n.attached.iter().filter_map(|a| Some((a.n, a.pointer()?, false))).collect();
    for t in crate::thread::thread_turns(&n.text) {
        for p in split(&t.body).1 {
            if let Some(k) = p.n {
                out.push((k, p.to, t.role != crate::thread::Role::You));
            }
        }
    }
    out.sort_by_key(|(k, ..)| *k);
    out
}

/// The numbers of comment `n`'s references, which mentions name.
pub fn numbers(n: &Note) -> Vec<usize> {
    let mut out: Vec<usize> = references(n).into_iter().map(|(k, ..)| k).collect();
    out.dedup();
    out
}

/// The highest number comment `n` gave a reference: of those attached to
/// it, of those its turns attach (deleted ones too), and of one gone since
/// (`Note::given`).
pub fn highest(n: &Note) -> usize {
    let turns = crate::thread::all_turns(&n.text).into_iter().flat_map(|t| split(&t.body).1).filter_map(|p| p.n);
    n.attached.iter().map(|a| a.n).chain(turns).chain(n.given).max().unwrap_or(0)
}

/// The number comment `n`'s next reference takes: one past every one it
/// has or had, so that none is given twice.
pub fn next_of(n: &Note) -> usize {
    highest(n) + 1
}

/// The numbers the mentions in `text` name, of `ns`, a comment's
/// references' numbers, added to `out`: `@r*` names all of them; a mention
/// in a `refs` block is none.
fn named_in(text: &str, ns: &[usize], out: &mut BTreeSet<usize>) {
    for m in mentions(&split(text).0) {
        match m.names {
            Names::All => out.extend(ns),
            Names::Some(named) => out.extend(named),
            Names::Bad => {}
        }
    }
}

/// The numbers the mentions of comment `n` name: those in its turns that
/// stand, whoever wrote them, and in `waiting`, the agent's reply waiting
/// for review, which goes in once accepted.
pub fn named(n: &Note, waiting: Option<&str>) -> BTreeSet<usize> {
    let ns = numbers(n);
    let mut out = BTreeSet::new();
    for t in crate::thread::thread_turns(&n.text) {
        named_in(&t.body, &ns, &mut out);
    }
    if let Some(w) = waiting {
        named_in(w, &ns, &mut out);
    }
    out
}

/// The references the turns of comment `n` that stand attach without
/// naming them in their text, by number: the agent's, for what they say,
/// which count as named by their turn as long as it stands.
fn context(n: &Note) -> BTreeSet<usize> {
    let ns = numbers(n);
    let mut out = BTreeSet::new();
    for t in crate::thread::thread_turns(&n.text) {
        let mut said = BTreeSet::new();
        named_in(&t.body, &ns, &mut said);
        out.extend(split(&t.body).1.into_iter().filter_map(|p| p.n).filter(|k| !said.contains(k)));
    }
    out
}

/// Comment `now`, saved in the place of `was` by a rewind, an edit or any
/// save: each reference of `was` whose last mention the save took away is
/// detached, as Detach does, one you attached from the comment, one a turn
/// attaches out of its `refs` block. One that nothing named (attached for
/// what it says, or named by nothing yet) stays; so does one a turn
/// attaches without its text naming it, as its turn names it (a rewind
/// that takes the turn takes it with it). `waiting`, the agent's reply
/// waiting for review, names them too. The highest number `was` gave is
/// kept, so that no reference takes it again. The numbers detached, in
/// order.
pub fn settle(was: &Note, now: &mut Note, waiting: Option<&str>) -> Vec<usize> {
    let had = numbers(was);
    let own = context(was);
    let after = named(now, waiting);
    let lost: Vec<usize> = named(was, waiting).into_iter().filter(|k| had.contains(k) && !own.contains(k) && !after.contains(k)).collect();
    let mut gone: Vec<usize> = Vec::new();
    if !lost.is_empty() {
        now.attached.retain(|a| {
            let kept = !lost.contains(&a.n);
            if !kept {
                gone.push(a.n);
            }
            kept
        });
        let mut turns = crate::thread::all_turns(&now.text);
        let mut changed = false;
        for t in turns.iter_mut().filter(|t| !t.deleted) {
            let (body, refs) = split(&t.body);
            let (out, kept): (Vec<Pointing>, Vec<Pointing>) = refs.into_iter().partition(|p| p.n.is_some_and(|k| lost.contains(&k)));
            if !out.is_empty() {
                gone.extend(out.iter().filter_map(|p| p.n));
                t.body = attach(&body, &kept);
                changed = true;
            }
        }
        if changed {
            now.text = crate::thread::render_thread(&turns);
        }
    }
    let top = highest(was);
    if highest(now) < top {
        now.given = Some(top);
    }
    gone.sort_unstable();
    gone.dedup();
    gone
}

/// What a turn written again as `body` holds: `body`, and the references
/// the turn attached as it was, `was`, unless `body` attaches its own; the
/// editor shows a turn's text without them. Those nothing names any more
/// go once it is saved (`settle`).
pub fn kept(was: &str, body: &str) -> String {
    let (_, refs) = split(was);
    match refs.is_empty() || body.trim().is_empty() || !split(body).1.is_empty() {
        true => body.to_string(),
        false => attach(body, &refs),
    }
}

/// References `ns`, detached as nothing names them any more, said: `r2 is
/// no longer named: it is detached`.
pub fn detached_said(ns: &[usize]) -> String {
    let names: Vec<String> = ns.iter().map(|k| format!("r{k}")).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => format!("{one} is no longer named: it is detached"),
        [rest @ .., last] => format!("{} and {last} are no longer named: they are detached", rest.join(", ")),
    }
}

/// What the agent's reply to comment (change, id) that waits for your
/// review says: it goes in the thread once accepted.
pub fn waiting(db: &Db, change: &str, id: &str) -> Option<String> {
    let (run, k) = crate::held::reply_waiting(db, change, id)?;
    db.run(&run)?.held.as_ref()?.replies.get(k).map(|rp| rp.text.clone())
}

/// Comment (change, id) of stack `name` as it stands: open; open on the
/// stack, the agent having moved it there; or closed, as it was last.
/// With whether it is open.
pub fn find_comment(db: &Db, name: &str, change: &str, id: &str) -> Option<(Note, bool)> {
    if let Some(n) = db.note(change, id) {
        return Some((n.clone(), true));
    }
    let moved = |n: &&Note| n.moved.as_ref().is_some_and(|m| m.change == change && m.id == id);
    if let Some(n) = db.stack_notes(name).into_iter().find(moved) {
        return Some((n.clone(), true));
    }
    db.deleted_notes(name, change).into_iter().find(|(_, n)| n.id == id).map(|(_, n)| (n, false))
}

/// The program that opens web links, when not `$STACKED_OPEN`'s or the
/// system's: the editor's tests set a stand-in here, as they share their
/// process and its environment, so that none of them opens a browser.
static OPENER: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Opens web links with `program` from now on, run as `program URL`.
pub fn open_links_with(program: &Path) {
    *OPENER.lock().unwrap() = Some(program.to_path_buf());
}

/// The program that opens a web link, run as `program URL`, when one is
/// named: the tests' stand-in, else the one `$STACKED_OPEN` names. None for
/// the system's own way, which the `open` crate knows (`opened_by_system`).
pub fn opener() -> Option<PathBuf> {
    if let Some(p) = OPENER.lock().unwrap().clone() {
        return Some(p);
    }
    std::env::var_os("STACKED_OPEN").filter(|p| !p.is_empty()).map(PathBuf::from)
}

/// How long the program opening a link is waited for, to say whether it
/// could: past that, it runs on (the browser it started, perhaps), and the
/// link is taken as opened.
const OPENING: Duration = Duration::from_secs(5);

/// Opens web link `url` with `program`, which reads nothing and writes
/// nowhere, as the screen is the editor's: what came of it, said for the
/// status line.
pub fn open_with(program: &Path, url: &str) -> String {
    said(opened_with(program, url))
}

/// The link opened, said; else why not.
fn said(opened: std::result::Result<String, String>) -> String {
    match opened {
        Ok(url) => format!("opened {url} in your browser"),
        Err(why) => why,
    }
}

/// Web link `url` opened with `program`: the link, as it was opened; else
/// why not, said for the status line.
fn opened_with(program: &Path, url: &str) -> std::result::Result<String, String> {
    let url = link(url)?;
    let name = program.display();
    let mut c = Command::new(program);
    c.arg(&url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // In a process group of its own: a ctrl-c where stacked was started
    // does not stop the browser it starts.
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("the link could not be opened: there is no {name} here to open it ($STACKED_OPEN may name a program that does)"));
        }
        Err(e) => return Err(format!("the link could not be opened: {name} did not run: {e}")),
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(url),
            Ok(Some(s)) => {
                return Err(match s.code() {
                    Some(code) => format!("the link could not be opened: {name} failed, exiting with {code}"),
                    None => format!("the link could not be opened: {name} was stopped"),
                });
            }
            Ok(None) if started.elapsed() < OPENING => std::thread::sleep(Duration::from_millis(10)),
            // Running on: waited for away from here, so that it is not left
            // a zombie once it is done.
            Ok(None) => {
                std::thread::spawn(move || child.wait());
                return Ok(url);
            }
            Err(e) => return Err(format!("the link could not be opened: {e}")),
        }
    }
}

/// Web link `url` opened as this system opens one, as the `open` crate
/// knows it: `open` on macOS; elsewhere `xdg-open`, else gio's and the
/// desktops' own, each tried in turn, each waited for as long as
/// `OPENING`, in a session of its own (a ctrl-c where stacked was started
/// does not stop the browser it starts). The link, as it was opened; else
/// why not, said for the status line.
fn opened_by_system(url: &str) -> std::result::Result<String, String> {
    let url = link(url)?;
    match open::that_detached_with_timeout(&url, OPENING) {
        Ok(()) => Ok(url),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("the link could not be opened: this system has no program to open it with ($STACKED_OPEN may name one)".into())
        }
        Err(_) => Err("the link could not be opened: the program this system opens links with failed ($STACKED_OPEN may name another)".into()),
    }
}

/// Web link `url` opened with the program that opens links (`opener`),
/// else as the system opens one: the link, as it was opened; else why not,
/// said for the status line.
pub fn open_url(url: &str) -> std::result::Result<String, String> {
    match opener() {
        Some(program) => opened_with(&program, url),
        None => opened_by_system(url),
    }
}

/// Opens web link `url` with the program that opens links (`opener`), else
/// as the system opens one: what came of it, said.
pub fn open_link(url: &str) -> String {
    said(open_url(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_sorted_and_merged_where_they_touch() {
        assert_eq!(ranges("200-210, 120-140,7"), Some(vec![(7, 7), (120, 140), (200, 210)]));
        assert_eq!(ranges("1-3,4,6-8,7-9"), Some(vec![(1, 4), (6, 9)]));
        for wrong in ["", "0", "3-1", "a", "1-b", ","] {
            assert_eq!(ranges(wrong), None, "{wrong:?}");
        }
    }

    #[test]
    fn a_reference_is_a_line_of_its_block_and_back() {
        let r = Ref {
            file: "dom/base/Foo.cpp".into(),
            ranges: vec![(120, 140), (200, 200)],
            at: At::Patch(3),
        };
        assert_eq!(r.line(), "dom/base/Foo.cpp:120-140,200 @3");
        assert_eq!(Ref::parse(&r.line()), Some(r.clone()));
        assert_eq!(r.said(), "dom/base/Foo.cpp:120-140, 200 · as patch 3 leaves it");
        assert_eq!(Ref::parse("a.txt:2 @0").map(|r| r.at), Some(At::Base));
        assert_eq!(Ref::parse("a.txt:2").map(|r| r.at), Some(At::Top));
        // No lines: the whole file, as the whole stack leaves it or a patch
        // does; lines that are none, no reference.
        let whole = Ref {
            file: "lib/rule.mjs".into(),
            ranges: vec![],
            at: At::Patch(15),
        };
        assert_eq!(whole.line(), "lib/rule.mjs @15");
        assert_eq!(Ref::parse(&whole.line()), Some(whole.clone()));
        assert_eq!(Ref::parse("lib/rule.mjs: @15"), Some(whole.clone()), "as the editor starts it");
        assert_eq!(whole.said(), "lib/rule.mjs · as patch 15 leaves it");
        assert!(whole.whole() && !r.whole());
        assert_eq!(Ref::parse("a.txt").map(|r| (r.whole(), r.at)), Some((true, At::Top)));
        assert_eq!(Ref::parse("a.txt:x @1"), None);
    }

    #[test]
    fn what_the_agent_points_at_keeps_its_number_a_whole_file_too() {
        // As an agent gave them: lines, two whole files, one that is no
        // reference, and lines again.
        let v: Value = serde_json::json!({
            "text": "See @r1, @r{2,3} and @r5.",
            "refs": [
                {"file": "dom/console/Console.cpp", "lines": "890-925", "patch": 1},
                {"file": "lib/rule.mjs", "patch": 15},
                {"file": "lib/rule.mjs", "lines": null, "patch": 19},
                {"file": "x.txt", "lines": "nope"},
                {"file": "lib/AdsClient.sys.mjs", "lines": "55-70", "patch": 18}
            ]
        });
        let text = written_with(&v, 1, &|_| None);
        assert!(
            text.ends_with("```refs\nr1 dom/console/Console.cpp:890-925 @1\nr2 lib/rule.mjs @15\nr3 lib/rule.mjs @19\nr5 lib/AdsClient.sys.mjs:55-70 @18\n```"),
            "{text}"
        );
        // What it names is each what it meant: r4 left out, its number too.
        let (_, refs) = split(&text);
        assert_eq!(refs.iter().map(|p| p.n.unwrap()).collect::<Vec<_>>(), [1, 2, 3, 5]);
        assert!(broken("See @r1, @r{2,3} and @r5.", &[1, 2, 3, 5]).is_empty());
        assert_eq!(of_json(v.get("refs")).len(), 5, "each in its place");
        assert_eq!(of_json(v.get("refs"))[3], Given::Unread);
    }

    #[test]
    fn what_the_agent_gave_is_its_text_its_references_in_a_block() {
        let v: Value = serde_json::json!({
            "text": "Look there.",
            "refs": [{"file": "a.txt", "lines": "5,1-2", "patch": 1}, {"file": "b.txt", "lines": [3, "7-8"]}, {"lines": "1"}]
        });
        let text = written(&v);
        assert_eq!(text, "Look there.\n\n```refs\na.txt:1-2,5 @1\nb.txt:3,7-8\n```");
        let (body, refs) = split(&text);
        assert_eq!(body, "Look there.");
        assert_eq!(refs.iter().map(Pointing::line).collect::<Vec<_>>(), ["a.txt:1-2,5 @1", "b.txt:3,7-8"]);
        assert_eq!(written(&serde_json::json!("Plain.")), "Plain.");
        assert_eq!(attach("Plain.", &[]), "Plain.");
    }

    #[test]
    fn the_agent_points_at_comments_by_a_label_or_an_id_those_found_kept() {
        let v: Value = serde_json::json!({
            "text": "As said there.",
            "refs": [{"comment": "c1"}, {"comment": "abc123"}, {"comment": "nowhere"}, {"comment": "c1"}, {"file": "a.txt", "lines": "2"}]
        });
        let found = |name: &str| match name {
            "c1" => Some(("c".to_string(), "+x".to_string())),
            "abc123" => Some(("stack:s".to_string(), "s1".to_string())),
            _ => None,
        };
        // Numbered from r3, by where each was given: the one that is no
        // comment leaves its number out.
        let text = written_with(&v, 3, &found);
        let (body, refs) = split(&text);
        assert_eq!(body, "As said there.");
        let numbered = |n: usize, to: Pointer| Pointing { n: Some(n), to };
        let c1 = Pointer::Comment {
            change: "c".into(),
            id: "+x".into(),
        };
        assert_eq!(
            refs,
            [
                numbered(3, c1.clone()),
                numbered(
                    4,
                    Pointer::Comment {
                        change: "stack:s".into(),
                        id: "s1".into()
                    }
                ),
                numbered(6, c1),
                numbered(7, Pointer::Lines(Ref::parse("a.txt:2").unwrap())),
            ],
            "{text}"
        );
        assert!(text.contains("\nr3 {\"change\":\"c\",\"id\":\"+x\"}\n") && text.contains("\nr7 a.txt:2\n"), "{text}");
        // A comment's line, back: its change and id, whatever they hold.
        let odd = Pointer::Comment {
            change: "c".into(),
            id: "+if (a: b) { \"x\" }".into(),
        };
        assert_eq!(Pointer::parse(&odd.line()), Some(odd));
    }

    #[test]
    fn a_comments_references_are_numbered_those_attached_and_the_agents() {
        let a = |n: usize, l: &str| Attached {
            n,
            to: To::Lines(l.into()),
        };
        let mut n = Note {
            text: "Which one, @r1 or @r2?".into(),
            attached: vec![a(1, "a.txt:1"), a(2, "a.txt:2")],
            ..Default::default()
        };
        assert_eq!((numbers(&n), next_of(&n)), (vec![1, 2], 3));
        // The agent's reply attaches r3; and an older one, not numbered.
        n.text += "\n````agent answer\nAs @r3 shows.\n\n```refs\nr3 b.txt:4 @2\nc.txt:1\n```\n````";
        assert_eq!((numbers(&n), next_of(&n)), (vec![1, 2, 3], 4));
        let refs = references(&n);
        assert_eq!(refs.iter().map(|(k, _, agent)| (*k, *agent)).collect::<Vec<_>>(), [(1, false), (2, false), (3, true)]);
        assert_eq!(refs[2].1, Pointer::Lines(Ref::parse("b.txt:4 @2").unwrap()));
        // A reply of yours naming the agent's is not broken.
        n.text += "\nAnd @r3?";
        assert!(broken_in(&n).is_empty(), "{:?}", broken_in(&n));
        n.text += " Or @r4?";
        assert_eq!(broken_in(&n), ["@r4"]);
        // A refs line's number, back.
        let p = Pointing {
            n: Some(12),
            to: Pointer::Lines(Ref::parse("a.txt:3-4 @1").unwrap()),
        };
        assert_eq!(p.line(), "r12 a.txt:3-4 @1");
        assert_eq!(Pointing::parse(&p.line()), Some(p));
        assert_eq!(Pointing::parse("a.txt:3").map(|p| p.n), Some(None));
    }

    #[test]
    fn mentions_name_references_one_several_or_all() {
        let said = |text: &str| mentions(text).into_iter().map(|m| (text[m.at].to_string(), m.names)).collect::<Vec<_>>();
        assert_eq!(
            said("Which is better, @r1 or @r2?"),
            [("@r1".into(), Names::Some(vec![1])), ("@r2".into(), Names::Some(vec![2]))]
        );
        assert_eq!(said("Do @r{1,2,3}, not @r4."), [("@r{1,2,3}".into(), Names::Some(vec![1, 2, 3])), ("@r4".into(), Names::Some(vec![4]))]);
        assert_eq!(said("@r{2-4, 7}"), [("@r{2-4, 7}".into(), Names::Some(vec![2, 3, 4, 7]))]);
        assert_eq!(said("Recap @r*"), [("@r*".into(), Names::All)]);
        // Not mentions: a word, an address, a number that goes on in one.
        assert!(said("@return a@r1.org @r2d2 @r alone").is_empty());
        // Mentions that do not read.
        assert_eq!(said("@r{1, and @r0 and @r{}"), [("@r{1,".into(), Names::Bad), ("@r0".into(), Names::Bad), ("@r{}".into(), Names::Bad)]);
        // Broken: naming what is not attached.
        assert_eq!(broken("@r1 @r{1,3} @r* @r{x}", &[1, 2]), ["@r{1,3}", "@r{x}"]);
        assert_eq!(broken("Recap @r*", &[]), ["@r*"]);
        assert!(broken("No mention.", &[]).is_empty());
    }

    #[test]
    fn an_attached_reference_is_kept_by_its_number_and_what_it_points_at() {
        let a = Attached {
            n: 2,
            to: To::Comment {
                change: "c".into(),
                id: "+x".into(),
            },
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r#"{"n":2,"comment":{"change":"c","id":"+x"}}"#);
        assert_eq!(serde_json::from_str::<Attached>(&json).unwrap(), a);
        let b = Attached {
            n: 3,
            to: To::Lines("a.txt:2-3 @1".into()),
        };
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"n":3,"lines":"a.txt:2-3 @1"}"#);
        assert_eq!((b.name(), b.lines().map(|r| r.said())), ("r3".into(), Some("a.txt:2-3 · as patch 1 leaves it".into())));
        assert_eq!(next_n(&[a, b]), 4);
        assert_eq!(next_n(&[]), 1);
    }

    /// Lines of a file, attached as reference `n`.
    fn lines(n: usize, l: &str) -> Attached {
        Attached {
            n,
            to: To::Lines(l.into()),
        }
    }

    #[test]
    fn a_rewind_detaches_what_only_the_turns_it_takes_named() {
        use crate::thread::{Role, append_turn, rewind};
        // r2 named by your comment, r3 the agent's, named by its reply, r1 by
        // your reply; r4 attached for what it says, named by none.
        let text = append_turn("Is @r2 right?", Role::Agent, "No: @r3 shows why.\n\n```refs\nr3 b.txt:4 @1\n```", Some("answer"));
        let was = Note {
            text: append_turn(&text, Role::You, "And @r1?", None),
            attached: vec![lines(1, "a.txt:1"), lines(2, "a.txt:2"), lines(4, "a.txt:4")],
            ..Default::default()
        };
        // Rewound from your reply: r1, named there only, is detached.
        let mut now = Note {
            text: rewind(&was.text, 2, "Thanks.").unwrap(),
            ..was.clone()
        };
        assert_eq!(settle(&was, &mut now, None), [1]);
        assert_eq!(now.attached.iter().map(Attached::name).collect::<Vec<_>>(), ["r2", "r4"]);
        assert_eq!(numbers(&now), [2, 3, 4]);
        // Rewound from your comment: r2 is detached, r3 goes with the
        // agent's reply, and r4, never named, stays.
        let was = now;
        let mut now = Note {
            text: rewind(&was.text, 0, "Start again.").unwrap(),
            ..was.clone()
        };
        assert_eq!(settle(&was, &mut now, None), [2]);
        assert_eq!(references(&now).into_iter().map(|(k, ..)| k).collect::<Vec<_>>(), [4]);
        assert_eq!((now.given, next_of(&now)), (None, 5));
    }

    #[test]
    fn an_edit_detaches_what_it_no_longer_names() {
        use crate::thread::{Role, append_turn, replace_last_turn};
        let text = append_turn(&append_turn("Look here.", Role::Agent, "Seen.", Some("answer")), Role::You, "Compare @r1 and @r{2,3}.", None);
        let was = Note {
            text,
            attached: vec![lines(1, "a.txt:1"), lines(2, "a.txt:2"), lines(3, "a.txt:3")],
            ..Default::default()
        };
        // Your reply written again without r2 and r3: both are detached.
        let mut now = Note {
            text: replace_last_turn(&was.text, "Compare @r1.").unwrap(),
            ..was.clone()
        };
        assert_eq!(settle(&was, &mut now, None), [2, 3]);
        assert_eq!(numbers(&now), [1]);
        assert_eq!(detached_said(&[2, 3]), "r2 and r3 are no longer named: they are detached");
        assert_eq!(detached_said(&[2]), "r2 is no longer named: it is detached");
        assert_eq!(detached_said(&[1, 2, 4]), "r1, r2 and r4 are no longer named: they are detached");
        // Nothing taken away, nothing detached.
        let mut same = was.clone();
        assert!(settle(&was, &mut same, None).is_empty());
        assert_eq!(same, was);
    }

    #[test]
    fn a_reference_still_named_stays_by_another_turn_the_agent_all_or_the_reply_waiting() {
        use crate::thread::{Role, append_turn, rewind};
        // Your reply names r1; rewound from it, that mention goes.
        let thread = |first: &str, agent: &str| Note {
            text: append_turn(&append_turn(first, Role::Agent, agent, Some("answer")), Role::You, "Is @r1 it?", None),
            attached: vec![lines(1, "a.txt:1")],
            ..Default::default()
        };
        let rewound = |was: &Note| Note {
            text: rewind(&was.text, 2, "Thanks.").unwrap(),
            ..was.clone()
        };
        // Named still by your comment, by the agent, by @r*: it stays.
        for (first, agent) in [("See @r1.", "Seen."), ("See.", "@r1 it is."), ("Recap @r*.", "Seen.")] {
            let was = thread(first, agent);
            let mut now = rewound(&was);
            assert!(settle(&was, &mut now, None).is_empty(), "{first:?} {agent:?}");
            assert_eq!(now.attached, was.attached);
        }
        // Named by nothing else, it goes; but for the agent's reply waiting
        // for review that names it, which goes in once accepted.
        let was = thread("See.", "Seen.");
        let mut now = rewound(&was);
        assert!(settle(&was, &mut now, Some("As @r1 says.\n\n```refs\nr2 b.txt:1\n```")).is_empty());
        assert_eq!(now.attached, was.attached);
        let mut now = rewound(&was);
        assert_eq!(settle(&was, &mut now, None), [1]);
        assert!(now.attached.is_empty());
        // Named only in a refs block, it is not named.
        assert!(named(&Note { text: "```refs\n@r1 a.txt:1\n```".into(), ..was.clone() }, None).is_empty());
    }

    #[test]
    fn a_turn_written_again_keeps_its_references_but_those_it_no_longer_names() {
        use crate::thread::{Role, all_turns, append_turn, edit_turn, rewind};
        // The agent names r1 and r2, and attaches r3 for what it says.
        let said = "See @r1 and @r2.\n\n```refs\nr1 a.txt:1 @1\nr2 a.txt:2 @1\nr3 a.txt:3 @1\n```";
        let was = Note {
            text: append_turn("Why?", Role::Agent, said, Some("answer")),
            ..Default::default()
        };
        // Written again as the editor has it, its text alone: its block is
        // kept, but r2, which nothing names any more; r3, never named,
        // stays.
        let old = all_turns(&was.text)[1].body.clone();
        assert_eq!(kept(&old, "See @r1."), format!("See @r1.\n\n{}", said.split_once("\n\n").unwrap().1));
        let mut now = Note {
            text: edit_turn(&was.text, 1, &kept(&old, "See @r1.")).unwrap(),
            ..was.clone()
        };
        assert_eq!(settle(&was, &mut now, None), [2]);
        assert_eq!(all_turns(&now.text)[1].body, "See @r1.\n\n```refs\nr1 a.txt:1 @1\nr3 a.txt:3 @1\n```");
        assert_eq!(all_turns(&now.text)[1].tag.as_deref(), Some("answer"));
        // What is written with references of its own keeps those; with
        // nothing written, nothing is kept.
        assert_eq!(kept(&old, "Mine.\n\n```refs\nr9 c.txt:1\n```"), "Mine.\n\n```refs\nr9 c.txt:1\n```");
        assert_eq!((kept(&old, " "), kept("Plain.", "Plainer.")), (" ".to_string(), "Plainer.".to_string()));
        // One the agent attached without naming it counts as named by its
        // turn: named by you, then no longer, it stays while that turn does.
        let was = Note {
            text: append_turn(&now.text, Role::You, "Why @r3?", None),
            ..now.clone()
        };
        let mut again = Note {
            text: rewind(&was.text, 2, "Thanks.").unwrap(),
            ..was.clone()
        };
        assert!(settle(&was, &mut again, None).is_empty());
        assert_eq!(numbers(&again), [1, 3]);
    }

    #[test]
    fn a_turn_deleted_the_first_way_counts_nowhere() {
        use crate::thread::{Role, Turn, render_thread, rewind};
        let turn = |role: Role, body: &str, deleted: bool| Turn {
            role,
            body: body.into(),
            tag: (role != Role::You).then(|| "answer".to_string()),
            deleted,
            edit: None,
            redacted: false,
            effort: None,
            stack: None,
        };
        // The agent's reply, deleted the first way, names r1 and points at
        // a comment, r2.
        let text = render_thread(&[
            turn(Role::You, "Look.", false),
            turn(Role::Agent, "As @r1 says.\n\n```refs\nr2 {\"change\":\"c\",\"id\":\"+x\"}\n```", true),
            turn(Role::You, "Well?", false),
        ]);
        let was = Note {
            text,
            attached: vec![lines(1, "a.txt:1")],
            ..Default::default()
        };
        // Its reference is not the comment's, nor pointed at; its mention
        // names nothing.
        assert_eq!(numbers(&was), [1]);
        assert!(pointed(&was).is_empty());
        assert!(named(&was, None).is_empty());
        // So r1 was never named: a rewind leaves it attached. The deleted
        // turn's number is not given again.
        let mut now = Note {
            text: rewind(&was.text, 2, "Thanks.").unwrap(),
            ..was.clone()
        };
        assert!(settle(&was, &mut now, None).is_empty());
        assert_eq!((now.attached.clone(), next_of(&now)), (was.attached.clone(), 3));
    }

    #[test]
    fn a_number_is_never_given_again_its_reference_detached_or_gone() {
        use crate::thread::{Role, append_turn, rewind};
        // r3, the highest, detached by hand: not as no longer named, but its
        // number is kept.
        let was = Note {
            text: "See @r1.".into(),
            attached: vec![lines(1, "a.txt:1"), lines(2, "a.txt:2"), lines(3, "a.txt:3")],
            ..Default::default()
        };
        let mut now = Note {
            attached: was.attached[..2].to_vec(),
            ..was.clone()
        };
        assert!(settle(&was, &mut now, None).is_empty());
        assert_eq!((now.given, next_of(&now)), (Some(3), 4));
        // Kept in the database, and read back; a comment saved before it was
        // kept reads as having none.
        let json = serde_json::to_value(&now).unwrap();
        assert_eq!(json["given"], 3);
        assert_eq!(serde_json::from_value::<Note>(json).unwrap().given, Some(3));
        assert!(serde_json::to_value(&was).unwrap().get("given").is_none());
        // The agent's r2 gone with its reply, rewound: 2 is not given again,
        // and r1, never named, stays.
        let was = Note {
            text: append_turn("Why?", Role::Agent, "@r2 shows it.\n\n```refs\nr2 b.txt:1\n```", Some("answer")),
            attached: vec![lines(1, "a.txt:1")],
            ..Default::default()
        };
        let mut now = Note {
            text: rewind(&was.text, 0, "Why, really?").unwrap(),
            ..was.clone()
        };
        assert!(settle(&was, &mut now, None).is_empty());
        assert_eq!((numbers(&now), now.given, next_of(&now)), (vec![1], Some(2), 3));
    }

    #[test]
    fn a_web_link_is_a_line_of_its_block_and_never_a_path() {
        // As a refs line: the link as it is, after its number; and back.
        let p = Pointing {
            n: Some(3),
            to: Pointer::Link("https://example.com/a:12?b=c#d".into()),
        };
        assert_eq!(p.line(), "r3 https://example.com/a:12?b=c#d");
        assert_eq!(Pointing::parse(&p.line()), Some(p.clone()));
        assert_eq!(Pointer::parse(" HTTP://Example.com/A "), Some(Pointer::Link("HTTP://Example.com/A".into())));
        // Never lines of a file, nor a file whole, though a colon and a
        // number end it; an address that is no web link is nothing.
        for url in ["https://example.com/a:12", "http://example.com", "https://example.com/a.txt @2", "ftp://example.com/a:1"] {
            assert_eq!(Ref::parse(url), None, "{url}");
        }
        for wrong in ["https://", "https:///a", "ftp://example.com/a:1", "https://exa mple.com"] {
            assert_eq!(Pointer::parse(wrong), None, "{wrong}");
        }
        assert_eq!(Pointer::parse("a.txt:12"), Some(Pointer::Lines(Ref::parse("a.txt:12").unwrap())));
        // In a block, beside lines.
        let lines = Pointing {
            n: Some(4),
            to: Pointer::Lines(Ref::parse("a.txt:2 @1").unwrap()),
        };
        let text = attach("See there.", &[p.clone(), lines.clone()]);
        assert_eq!(text, "See there.\n\n```refs\nr3 https://example.com/a:12?b=c#d\nr4 a.txt:2 @1\n```");
        assert_eq!(split(&text), ("See there.".to_string(), vec![p, lines]));
    }

    #[test]
    fn a_web_link_is_an_address_of_https_or_http_whole() {
        for url in [
            "https://example.com",
            "http://example.com/a/b?c=d&e#f",
            "HTTPS://Example.COM/A",
            "https://user:pw@host:8080/p",
            "http://[::1]:8080/x",
            "https://bugzilla.mozilla.org/show_bug.cgi?id=1234567",
        ] {
            assert_eq!(link(&format!("  {url} ")).as_deref(), Ok(url), "{url}");
        }
        let why = |text: &str| link(text).unwrap_err();
        assert_eq!(why(" "), "write the web link to attach: write it whole, as https://example.com/a/b");
        for not in ["example.com/a", "ftp://example.com/a", "file:///etc/passwd", "javascript:alert(1)", "mailto:a@b.org", "a.txt:2 @1"] {
            assert_eq!(why(not), "a web link starts with https:// or http://: write it whole, as https://example.com/a/b", "{not}");
        }
        assert_eq!(why("https://exa mple.com"), "a web link has no spaces in it: write each one as %20");
        for siteless in ["https://", "https:///a", "http://?q", "https://user@/a", "https://:80/a"] {
            assert!(why(siteless).starts_with("the link names no site after http"), "{siteless}: {}", why(siteless));
        }
        assert_eq!(why("HTTPS://"), "the link names no site after HTTPS://: write it whole, as https://example.com/a/b");
    }

    #[test]
    fn the_agent_gives_a_web_link_as_a_url_a_link_of_the_web_only() {
        let v: Value = serde_json::json!({
            "text": "As @r1 says, not @r2.",
            "refs": [
                {"url": "https://bugzilla.mozilla.org/show_bug.cgi?id=1234567"},
                {"url": "ftp://example.com/a"},
                {"url": "javascript:alert(1)"},
                {"url": 7},
                {"file": "https://example.com/a.txt", "lines": "1"},
                {"file": "a.txt", "lines": "2", "patch": 1}
            ]
        });
        let bug = "https://bugzilla.mozilla.org/show_bug.cgi?id=1234567";
        let given = of_json(v.get("refs"));
        assert_eq!(given[0], Given::Link(bug.into()));
        // Not of the web, or a file that is an address: none, each keeping
        // its place.
        assert_eq!(given[1..5], [Given::Unread, Given::Unread, Given::Unread, Given::Unread]);
        let text = written_with(&v, 1, &|_| None);
        assert!(text.ends_with(&format!("```refs\nr1 {bug}\nr6 a.txt:2 @1\n```")), "{text}");
        assert_eq!(split(&text).1[0].to, Pointer::Link(bug.into()));
        assert_eq!(written(&serde_json::json!({"text": "See.", "refs": [{"url": bug}]})), format!("See.\n\n```refs\n{bug}\n```"));
    }

    #[test]
    fn an_attached_web_link_is_kept_as_its_link_and_comments_saved_before_still_read() {
        let a = Attached {
            n: 3,
            to: To::Link("https://example.com/a/b".into()),
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r#"{"n":3,"link":"https://example.com/a/b"}"#);
        assert_eq!(serde_json::from_str::<Attached>(&json).unwrap(), a);
        assert_eq!((a.lines(), a.pointer()), (None, Some(Pointer::Link("https://example.com/a/b".into()))));
        // A comment saved before links were kept: what it had attached reads
        // as it was.
        let old = r#"{"change":"c","id":"+x","line":"+x","text":"See @r1 and @r2.","attached":[{"n":1,"comment":{"change":"c","id":"+y"}},{"n":2,"lines":"a.txt:2-3 @1"}],"time":1}"#;
        let n: Note = serde_json::from_str(old).unwrap();
        assert_eq!(n.attached.iter().map(|a| a.pointer().is_some()).collect::<Vec<_>>(), [true, true]);
        assert_eq!(numbers(&n), [1, 2]);
        // A link is one of the comment's references, not a comment pointed
        // at.
        let n = Note {
            text: "See @r3.".into(),
            attached: vec![a],
            ..Default::default()
        };
        assert!(pointed(&n).is_empty());
        assert_eq!(references(&n), [(3, Pointer::Link("https://example.com/a/b".into()), false)]);
        assert!(broken_in(&n).is_empty());
        assert_eq!(next_of(&n), 4);
    }

    #[test]
    fn a_web_link_is_named_and_detached_as_any_reference() {
        use crate::thread::{Role, append_turn, rewind};
        let link = |n: usize| Attached {
            n,
            to: To::Link(format!("https://example.com/{n}")),
        };
        // r1 and r2 attached, links; your reply names r2; the agent's r3 is
        // a link too, which its reply names.
        let text = append_turn("Why?", Role::Agent, "See @r3.\n\n```refs\nr3 https://example.com/3\n```", Some("answer"));
        let was = Note {
            text: append_turn(&text, Role::You, "And @r2?", None),
            attached: vec![link(1), link(2)],
            ..Default::default()
        };
        assert_eq!(numbers(&was), [1, 2, 3]);
        assert_eq!(named(&was, None).into_iter().collect::<Vec<_>>(), [2, 3]);
        // Rewound from your reply: r2, named there only, is detached; r1,
        // never named, stays, and so does the agent's r3, its turn standing.
        let mut now = Note {
            text: rewind(&was.text, 2, "Thanks.").unwrap(),
            ..was.clone()
        };
        assert_eq!(settle(&was, &mut now, None), [2]);
        assert_eq!((now.attached.clone(), numbers(&now), next_of(&now)), (vec![link(1)], vec![1, 3], 4));
    }

    #[test]
    fn a_web_link_is_opened_by_the_program_that_opens_links_or_said_why_not() {
        let url = "https://example.com/a/b";
        assert_eq!(open_with(Path::new("true"), url), "opened https://example.com/a/b in your browser");
        assert_eq!(open_with(Path::new("false"), url), "the link could not be opened: false failed, exiting with 1");
        assert_eq!(
            open_with(Path::new("/nowhere/at/all/open"), url),
            "the link could not be opened: there is no /nowhere/at/all/open here to open it ($STACKED_OPEN may name a program that does)"
        );
        // What is not a web link is not handed to it, nor to the system's
        // way of opening links.
        assert!(open_with(Path::new("true"), "file:///etc/passwd").starts_with("a web link starts with https://"));
        assert!(opened_by_system("file:///etc/passwd").unwrap_err().starts_with("a web link starts with https://"));
        // The program the tests set is the one that opens links: never the
        // system's, which would open a browser.
        open_links_with(Path::new("true"));
        assert_eq!(opener(), Some(PathBuf::from("true")));
        assert_eq!(open_link(url), "opened https://example.com/a/b in your browser");
        assert_eq!(open_url(&format!(" {url} ")), Ok(url.to_string()), "the link as it was opened");
    }
}
