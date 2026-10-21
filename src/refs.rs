//! References an agent attaches to what it writes: lines of a file, as
//! ranges, as a patch of the stack leaves the file, as the stack's base has
//! it, or as the whole stack leaves it, which reaches the files the stack
//! does not touch too. Kept in the turn they come with, as a `refs` block, a
//! reference a line: `path:120-140,200-210 @3` (`@0` the base; no `@`, the
//! whole stack). Under the comment, a line each; Enter previews its lines,
//! and Enter again goes there.

use serde_json::Value;

/// A reference: `file`'s lines `ranges` (from 1, both ends in, sorted,
/// apart), as `at` has the file.
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
    /// As a line of a `refs` block: `path:120-140,200-210 @3`.
    pub fn line(&self) -> String {
        let at = match self.at {
            At::Top => String::new(),
            At::Base => " @0".into(),
            At::Patch(n) => format!(" @{n}"),
        };
        format!("{}:{}{at}", self.file, ranges_said(&self.ranges, ","))
    }

    /// A line of a `refs` block, if it is one.
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
        let (file, lines) = spec.rsplit_once(':')?;
        let file = file.trim();
        (!file.is_empty()).then_some(())?;
        Some(Ref {
            file: file.to_string(),
            ranges: ranges(lines)?,
            at,
        })
    }

    /// One the agent gave: `{"file", "lines", "patch"}`, `"patch"` 0 for the
    /// base, none for the whole stack.
    pub fn from_json(v: &Value) -> Option<Ref> {
        let file = v.get("file")?.as_str()?.trim();
        if file.is_empty() {
            return None;
        }
        let lines = match v.get("lines")? {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Array(a) => a
                .iter()
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            _ => return None,
        };
        let at = match v.get("patch").and_then(|p| p.as_u64().or_else(|| p.as_str()?.trim().parse().ok())) {
            None => At::Top,
            Some(0) => At::Base,
            Some(n) => At::Patch(n as usize),
        };
        Some(Ref {
            file: file.to_string(),
            ranges: ranges(&lines)?,
            at,
        })
    }

    /// As the comment shows it: `a.txt:2-3, 10 · as patch 1 leaves it`.
    pub fn said(&self) -> String {
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

/// The references the agent gave, `v` (a list), those it can read.
pub fn of_json(v: Option<&Value>) -> Vec<Ref> {
    v.and_then(Value::as_array).map_or(vec![], |a| a.iter().filter_map(Ref::from_json).collect())
}

/// What an agent wrote, `v`: its text and the references it attaches; a
/// string attaches none, an object has its `text` and its `refs`.
pub fn text_and_refs(v: &Value) -> (String, Vec<Ref>) {
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

/// What an agent wrote, `v`, its references in a `refs` block at its end.
pub fn written(v: &Value) -> String {
    let (text, refs) = text_and_refs(v);
    attach(&text, &refs)
}

/// `text`, with `refs` in a `refs` block at its end; as it is without any.
pub fn attach(text: &str, refs: &[Ref]) -> String {
    if refs.is_empty() {
        return text.to_string();
    }
    let lines: Vec<String> = refs.iter().map(Ref::line).collect();
    format!("{}\n\n```{TAG}\n{}\n```", text.trim_end(), lines.join("\n"))
}

/// `body`, a turn's text, without its `refs` blocks; and the references
/// they hold, in order.
pub fn split(body: &str) -> (String, Vec<Ref>) {
    let (mut kept, mut refs) = (Vec::new(), Vec::new());
    let mut lines = body.lines();
    while let Some(l) = lines.next() {
        if l.trim() == format!("```{TAG}") {
            for r in lines.by_ref() {
                if r.trim() == "```" {
                    break;
                }
                refs.extend(Ref::parse(r));
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
        assert_eq!(Ref::parse("a.txt"), None);
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
        assert_eq!(refs.iter().map(Ref::line).collect::<Vec<_>>(), ["a.txt:1-2,5 @1", "b.txt:3,7-8"]);
        assert_eq!(written(&serde_json::json!("Plain.")), "Plain.");
        assert_eq!(attach("Plain.", &[]), "Plain.");
    }
}
