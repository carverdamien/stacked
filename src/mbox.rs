//! Patches as mail: what `git format-patch --stdout` writes is read, and
//! what is written is what `git am` reads. A patch being edited is the same
//! mail without its `From <id>` line.

use crate::db::{Error, Patch, Result, err};

/// A patch read from mail, not yet given a change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Draft {
    /// The commit it was made from, as its `From <sha>` line says.
    pub commit: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub message: String,
    pub diff: String,
}

impl Draft {
    pub fn title(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }
}

impl From<&Patch> for Draft {
    fn from(p: &Patch) -> Draft {
        Draft {
            commit: None,
            author: p.author.clone(),
            date: p.date.clone(),
            message: p.message.clone(),
            diff: p.diff.clone(),
        }
    }
}

/// Is `line` the line `git format-patch` starts each mail with?
fn is_separator(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("From ") else {
        return false;
    };
    let hash = rest.split(' ').next().unwrap_or("");
    hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Every mail in `text`, in order. Text with no `From <id>` line is one
/// mail, or none if it is blank.
pub fn parse_mbox(text: &str) -> Result<Vec<Draft>> {
    let mut mails: Vec<(Option<String>, Vec<&str>)> = Vec::new();
    let mut lines = text.split_inclusive('\n').peekable();
    if lines.peek().is_some_and(|l| !is_separator(l.trim_end_matches('\n'))) {
        mails.push((None, Vec::new()));
    }
    for line in lines {
        if is_separator(line.trim_end_matches('\n')) {
            mails.push((Some(line[5..45].to_string()), Vec::new()));
        } else {
            mails.last_mut().unwrap().1.push(line);
        }
    }
    mails
        .into_iter()
        .filter(|(_, m)| m.iter().any(|l| !l.trim().is_empty()))
        .enumerate()
        .map(|(i, (commit, m))| {
            let mut d =
                parse_mail(&m.concat()).map_err(|e| Error(format!("mail {}: {e}", i + 1)))?;
            d.commit = commit;
            Ok(d)
        })
        .collect()
}

/// The commit `format-patch --base` says the mail applies to.
pub fn base_commit(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("base-commit: "))
        .map(|s| s.trim().to_string())
        .find(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// One mail: headers, if it starts with any, then the message, then the
/// diff from its first `diff --git` line.
pub fn parse_mail(text: &str) -> Result<Draft> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut draft = Draft::default();
    let mut subject = None;
    let mut at = 0;
    if lines.first().is_some_and(|l| is_header(l)) {
        let mut headers: Vec<String> = Vec::new();
        while at < lines.len() {
            let line = lines[at].trim_end_matches(['\n', '\r']);
            at += 1;
            if line.is_empty() {
                break;
            }
            match headers.last_mut() {
                Some(h) if line.starts_with([' ', '\t']) => {
                    h.push(' ');
                    h.push_str(line.trim_start());
                }
                _ => headers.push(line.to_string()),
            }
        }
        for h in headers {
            let Some((key, value)) = h.split_once(':') else {
                return err(format!("not a header: {h}"));
            };
            let value = decode_words(value.trim());
            match key.to_ascii_lowercase().as_str() {
                "from" => draft.author = Some(value),
                "date" => draft.date = Some(value),
                "subject" => subject = Some(strip_patch_prefix(&value).to_string()),
                _ => {}
            }
        }
    }
    let rest = &lines[at..];
    let start = rest
        .iter()
        .enumerate()
        .position(|(i, l)| {
            l.starts_with("diff ")
                || l.starts_with("Index: ")
                || (l.starts_with("--- ") && rest.get(i + 1).is_some_and(|n| n.starts_with("+++ ")))
        })
        .unwrap_or(rest.len());
    // As `git am`, the message ends at the first `---`: what follows it is
    // the diffstat.
    let message: Vec<&str> = rest[..start]
        .iter()
        .take_while(|l| l.trim_end() != "---")
        .copied()
        .collect();
    let body = message.concat();
    let body = body.trim_matches('\n');
    draft.message = match subject {
        Some(s) if body.is_empty() => s,
        Some(s) => format!("{s}\n\n{body}"),
        None => body.to_string(),
    };
    draft.message = draft.message.trim_end().to_string();
    let diff: Vec<&str> = rest[start..]
        .iter()
        .filter(|l| !(l.starts_with("base-commit: ") || l.starts_with("prerequisite-patch-id: ")))
        .copied()
        .collect();
    draft.diff = strip_signature(&diff.concat());
    if draft.message.is_empty() {
        return err("no title");
    }
    Ok(draft)
}

fn is_header(line: &str) -> bool {
    let line = line.trim_end();
    match line.split_once(": ") {
        Some((key, _)) => {
            !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }
        None => false,
    }
}

/// `[PATCH 2/3] Title` is `Title`.
fn strip_patch_prefix(subject: &str) -> &str {
    let mut s = subject.trim();
    while s.starts_with('[')
        && let Some(end) = s.find(']')
        && s[1..end].to_ascii_uppercase().starts_with("PATCH")
    {
        s = s[end + 1..].trim_start();
    }
    s
}

/// The diff without the `-- ` line and the git version `format-patch`
/// signs it with.
fn strip_signature(diff: &str) -> String {
    if let Some(at) = diff.rfind("\n-- \n") {
        let after = &diff[at + 5..];
        let signed = after.lines().filter(|l| !l.trim().is_empty()).count();
        if (1..=2).contains(&signed) && !after.starts_with([' ', '+', '-', '@', '\\']) {
            return diff[..at + 1].to_string();
        }
    }
    let mut d = diff.trim_end_matches('\n').to_string();
    if !d.is_empty() {
        d.push('\n');
    }
    d
}

/// Undoes RFC 2047's `=?UTF-8?q?...?=` and `=?UTF-8?b?...?=`, which
/// `format-patch` writes for names and titles that are not ASCII.
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let Some(word) = encoded_word(&rest[start..]) else {
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            last_was_word = false;
            continue;
        };
        let (decoded, len) = word;
        let between = &rest[..start];
        // Whitespace between two encoded words is not part of the text.
        if !(last_was_word && between.trim().is_empty()) {
            out.push_str(between);
        }
        out.push_str(&decoded);
        rest = &rest[start + len..];
        last_was_word = true;
    }
    out.push_str(rest);
    out
}

fn encoded_word(s: &str) -> Option<(String, usize)> {
    let inner = &s[2..];
    let (charset, inner) = inner.split_once('?')?;
    let (enc, inner) = inner.split_once('?')?;
    let end = inner.find("?=")?;
    let text = &inner[..end];
    let len = 2 + charset.len() + 1 + enc.len() + 1 + end + 2;
    let bytes = match enc {
        "q" | "Q" => {
            let mut out = Vec::new();
            let b = text.as_bytes();
            let mut i = 0;
            while i < b.len() {
                match b[i] {
                    b'_' => out.push(b' '),
                    b'=' => {
                        let hex = text.get(i + 1..i + 3)?;
                        out.push(u8::from_str_radix(hex, 16).ok()?);
                        i += 2;
                    }
                    c => out.push(c),
                }
                i += 1;
            }
            out
        }
        "b" | "B" => base64(text)?,
        _ => return None,
    };
    Some((String::from_utf8_lossy(&bytes).into_owned(), len))
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The mail headers and message of a patch, then its diff: what
/// `parse_mail` reads back.
pub fn format_mail(d: &Draft, subject_prefix: &str) -> String {
    let mut out = String::new();
    if let Some(a) = &d.author {
        out.push_str(&format!("From: {a}\n"));
    }
    if let Some(date) = &d.date {
        out.push_str(&format!("Date: {date}\n"));
    }
    let (title, body) = match d.message.split_once('\n') {
        Some((t, b)) => (t, b.trim_matches('\n')),
        None => (d.message.as_str(), ""),
    };
    out.push_str(&format!("Subject: {subject_prefix}{title}\n\n"));
    if !body.is_empty() {
        out.push_str(body);
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(&d.diff);
    out
}

/// The patches as one mbox, for `git am`.
pub fn format_mbox(patches: &[&Patch]) -> String {
    let n = patches.len();
    let mut out = String::new();
    for (i, p) in patches.iter().enumerate() {
        let prefix = if n > 1 {
            format!("[PATCH {}/{n}] ", i + 1)
        } else {
            "[PATCH] ".to_string()
        };
        out.push_str(&format!("From {} Mon Sep 17 00:00:00 2001\n", p.id));
        let mail = format_mail(&Draft::from(*p), &prefix);
        // Headers end at the first blank line; the charset goes with them.
        let (head, rest) = mail.split_once("\n\n").unwrap();
        out.push_str(head);
        out.push_str(
            "\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n\n",
        );
        out.push_str(rest);
        out.push_str("-- \nstacked\n\n");
    }
    out
}
