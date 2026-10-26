//! References an agent attaches to what it writes: lines of a file, as
//! ranges, or the whole file, as a patch of the stack leaves the file, as
//! the stack's base has it, or as the whole stack leaves it, which reaches
//! the files the stack does not touch too; or another comment of the stack.
//! Kept in the turn they come with, as a `refs` block, a reference a line:
//! `path:120-140,200-210 @3` (`path @3` the whole file; `@0` the base; no
//! `@`, the whole stack), or a comment's change and id as JSON. Under the
//! comment, a line each; Enter previews a reference's lines, and Enter
//! again goes there; Enter on a comment goes to it.
//!
//! The references you attach to a comment are the comment's own, kept with
//! it whatever its turns become: each numbered once and for all, `r1`,
//! `r2`, which what you write names: `@r1`, `@r{1,3-4}` several, `@r*` all
//! of them. The agent reads them with the comment, the comments as they
//! stand, open or closed.

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
    /// `path: @3`), the whole file.
    pub fn parse(line: &str) -> Option<Ref> {
        let line = line.trim();
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
    pub fn from_json(v: &Value) -> Option<Ref> {
        let file = v.get("file")?.as_str()?.trim();
        if file.is_empty() {
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

/// What a line of a `refs` block points at: lines of a file, or a comment
/// of the stack, by its change and id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pointer {
    Lines(Ref),
    Comment { change: String, id: String },
}

impl Pointer {
    /// As a line of a `refs` block: a comment's as JSON, which no path is.
    pub fn line(&self) -> String {
        match self {
            Pointer::Lines(r) => r.line(),
            Pointer::Comment { change, id } => serde_json::json!({"change": change, "id": id}).to_string(),
        }
    }

    /// A line of a `refs` block, if it is one.
    pub fn parse(line: &str) -> Option<Pointer> {
        let line = line.trim();
        if line.starts_with('{') {
            let v: Value = serde_json::from_str(line).ok()?;
            return Some(Pointer::Comment {
                change: v.get("change")?.as_str()?.to_string(),
                id: v.get("id")?.as_str()?.to_string(),
            });
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
/// as `stacked comments` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Given {
    Lines(Ref),
    Comment(String),
    /// One that does not read as either: it keeps its place, and its number
    /// with it.
    Unread,
}

/// The references the agent gave, `v` (a list), each in its place, those
/// it cannot read too, so that what names the others still does.
pub fn of_json(v: Option<&Value>) -> Vec<Given> {
    let given = |r: &Value| match r.get("comment") {
        Some(c) => c.as_str().map(|c| Given::Comment(c.trim().to_string())).filter(|g| *g != Given::Comment(String::new())),
        None => Ref::from_json(r).map(Given::Lines),
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
/// not numbered: lines only, as no comment can be found here.
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
/// the stack, or lines of a file, as a `refs` line says them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum To {
    Comment { change: String, id: String },
    Lines(String),
}

/// A reference you attach to a comment: its number, which `@r1` names, and
/// what it points at. A number is never given twice in a comment.
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
            To::Comment { .. } => None,
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
        }
    }
}

/// The number the next reference attached to `attached` takes.
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
/// it, then those its turns point at.
pub fn pointed(n: &Note) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = n
        .attached
        .iter()
        .filter_map(|a| match &a.to {
            To::Comment { change, id } => Some((change.clone(), id.clone())),
            To::Lines(_) => None,
        })
        .collect();
    for t in crate::thread::all_turns(&n.text) {
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
/// turns attach (the agent's), each with whether the agent attached it.
pub fn references(n: &Note) -> Vec<(usize, Pointer, bool)> {
    let mut out: Vec<(usize, Pointer, bool)> = n.attached.iter().filter_map(|a| Some((a.n, a.pointer()?, false))).collect();
    for t in crate::thread::all_turns(&n.text) {
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

/// The number comment `n`'s next reference takes: one past every one it
/// has.
pub fn next_of(n: &Note) -> usize {
    numbers(n).last().map_or(1, |k| k + 1)
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
}
