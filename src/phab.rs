//! Reviewers' comments from Phabricator: each patch's revision (its
//! `Differential Revision:` trailer) read over Conduit with the token
//! moz-phab and arc keep in `~/.arcrc`, or the one the settings give, which
//! curl is given on its standard input (`request`).
//! Taken from self-review's, less what files them in its sessions:
//! `review::pull` files them in the database. Then what the editor shows
//! of a stack's revisions, read when asked, and stored in the database
//! (`fetch_latest`): their statuses, their latest diffs, their parents and
//! their comments.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::{Comments, Db, Seen, Stack, State};
use crate::submit::{Below, Parent};
use crate::view::{NoteLine, fold};

/// A Conduit endpoint and the token for it.
pub struct Conduit {
    pub api: String,
    token: String,
}

/// What a call of Conduit came to, when not to its result: Phabricator
/// turning away the requests of this machine for now (HTTP 429, too many
/// requests); or anything else that went wrong, said.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    TurnedAway,
    Said(String),
}

/// What a call that Phabricator turned away says.
pub const TURNED_AWAY: &str = "Phabricator is turning away requests from this machine for now (HTTP 429, too many requests)";

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::TurnedAway => f.write_str(TURNED_AWAY),
            Error::Said(said) => f.write_str(said),
        }
    }
}

impl From<String> for Error {
    fn from(said: String) -> Error {
        Error::Said(said)
    }
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

fn arcrc(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

impl Conduit {
    /// The endpoint for `url` (a revision's link), with its token: the
    /// settings', for their Phabricator, else the arcrc's; there, the host
    /// `url` names, else the settings' Phabricator, else the only one.
    pub fn for_url(url: Option<&str>) -> Result<Conduit, String> {
        let cfg = &crate::config::get().reviews;
        let host_of = |u: &str| {
            u.split("://")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .map(String::from)
        };
        let mine = cfg.url.as_deref().and_then(host_of);
        let want = url.and_then(host_of).or(mine.clone());
        if let (Some(token), Some(base)) = (&cfg.token, &cfg.url)
            && want == mine
        {
            return Ok(Conduit {
                api: format!("{base}/api/"),
                token: token.clone(),
            });
        }
        let rc_path = cfg.arcrc.display().to_string();
        let rc = arcrc(&cfg.arcrc).ok_or(format!("no {rc_path}: log in with moz-phab (or arc) first"))?;
        let hosts = rc
            .get("hosts")
            .and_then(Value::as_object)
            .ok_or(format!("{rc_path} has no hosts"))?;
        let pick = hosts
            .iter()
            .find(|(api, _)| want.is_none() || host_of(api) == want)
            .or_else(|| (hosts.len() == 1).then(|| hosts.iter().next().unwrap()));
        let Some((api, v)) = pick else {
            return Err(format!(
                "no token for {} in {rc_path}",
                want.unwrap_or_default()
            ));
        };
        let token = v
            .get("token")
            .and_then(Value::as_str)
            .ok_or(format!("{rc_path} has no token for that host"))?;
        let mut api = api.clone();
        if !api.ends_with('/') {
            api.push('/');
        }
        Ok(Conduit {
            api,
            token: token.into(),
        })
    }

    /// Calls a Conduit method, by curl (`request`); its `result`.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, Error> {
        request(Command::new("curl"), &self.api, &self.token, method, params)
    }
}

/// Conduit's method `method` called at `api` with `params` and the token
/// `token`, by `curl`, the command curl's arguments are added to: curl, or
/// the tests' stand-in. The token and the parameters are given on curl's
/// standard input, never on its command line, where any process of this
/// machine could read them. Its `result`; or what went wrong: curl failing,
/// as it says it; Phabricator turning this machine away (HTTP 429); a
/// status other than 200, or a page that is not JSON, said with the page's
/// first line; or the error Conduit gives.
fn request(mut curl: Command, api: &str, token: &str, method: &str, params: Value) -> Result<Value, Error> {
    let mut params = params;
    params["__conduit__"] = json!({"token": token});
    // The headers of the answer first, then its page (`-D -`).
    curl.args(["-sS", "--max-time", "30", "-D", "-", "-X", "POST"])
        .arg(format!("{api}{method}"))
        .args(["--data-urlencode", "params@-", "--data", "output=json", "--data", "__conduit__=1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = curl.spawn().map_err(|e| Error::Said(format!("curl: {e}")))?;
    // curl reads it whole before it sends anything. Should it stop reading,
    // what it says of why is what is said.
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(params.to_string().as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| Error::Said(format!("curl: {e}")))?;
    if !out.status.success() {
        return Err(Error::Said(format!("{method}: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    let (status, page) = answered(&out.stdout);
    let first = String::from_utf8_lossy(page).lines().map(str::trim).find(|l| !l.is_empty()).map(|l| match l.chars().count() > 80 {
        true => format!("{}…", l.chars().take(80).collect::<String>()),
        false => l.to_string(),
    });
    match (status, &first) {
        (Some(429), _) => return Err(Error::TurnedAway),
        (Some(s), Some(l)) if s != 200 => return Err(Error::Said(format!("{method}: Phabricator answered HTTP {s} with a page starting \"{l}\""))),
        (Some(s), None) if s != 200 => return Err(Error::Said(format!("{method}: Phabricator answered HTTP {s} with an empty page"))),
        _ => {}
    }
    let v: Value = serde_json::from_slice(page).map_err(|_| match &first {
        Some(l) => Error::Said(format!("{method}: Phabricator answered with a page that is not JSON, starting \"{l}\"")),
        None => Error::Said(format!("{method}: Phabricator answered with an empty page")),
    })?;
    if let Some(e) = v.get("error_info").and_then(Value::as_str) {
        return Err(Error::Said(format!("{method}: {e}")));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// What curl wrote with `-D -`: the status of the last answer, past those
/// that only say to go on (`100 Continue`), and its page. No status where
/// curl wrote no headers.
fn answered(out: &[u8]) -> (Option<u16>, &[u8]) {
    let (mut status, mut rest) = (None, out);
    while rest.starts_with(b"HTTP/") {
        let end = rest
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|k| k + 4)
            .or_else(|| rest.windows(2).position(|w| w == b"\n\n").map(|k| k + 2))
            .unwrap_or(rest.len());
        let headers = String::from_utf8_lossy(&rest[..end]);
        status = headers.split_whitespace().nth(1).and_then(|s| s.parse().ok());
        rest = &rest[end..];
    }
    (status, rest)
}

/// A comment as Phabricator has it, as it was last edited: its id is that
/// version's, each edit making one of its own. Read with the suggested
/// edit Phabricator gives with it, it is a version of its own, its id that
/// version's and `+suggestion` (`SUGGESTED`), the version as read without
/// it one before it: so a comment filed without its suggested edit is held
/// once its suggested edit is read, to take the place of its turn as an
/// edit does, said for what it is, as no one edited it (`anew`). Kept so
/// in the run of a pull while it waits its turn (`review::Job`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Remote {
    pub phid: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub text: String,
    /// When it was first written.
    #[serde(default)]
    pub created: i64,
    /// None for a comment on the revision as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub line: usize,
    /// On the old side of the diff it was made on, the file before it, as
    /// Phabricator's `isNewFile` says: `line` that side's number, and
    /// `line_text` that side's text. Else on the new side, as every comment
    /// is where Phabricator does not say.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub old_side: bool,
    /// The line's text in the diff it was made on, on its side, when that
    /// is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// What it was before its last edit, the version before it first: none
    /// if it was never edited.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier: Vec<Version>,
}

/// A comment as it was before an edit: its id then, and what it said.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Version {
    pub phid: String,
    #[serde(default)]
    pub text: String,
}

/// One revision's comments, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionComments {
    pub revision: String,
    pub source: String,
    pub comments: Vec<Remote>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pulled {
    pub revisions: Vec<RevisionComments>,
}

/// A side of a diff: the new one, the file as the diff leaves it, its lines
/// of context and those it adds; or the old one, the file before it, its
/// lines of context and those it removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    New,
    Old,
}

/// A Phabricator diff's lines, from its hunks, Conduit called with `call`:
/// by the path of their file, their side and their number there, the text
/// of each, its origin left out. Each side is numbered from the hunk's own
/// offset (`newOffset`, `oldOffset`): a line of context is on both, one the
/// diff adds on the new side alone, one it removes on the old side alone,
/// and git's `\ No newline at end of file` on neither. The diff not in the
/// answer, none of its lines; the call failing, what went wrong, as every
/// read of a pull fails then.
fn diff_lines(call: &mut Call, diff: u64) -> Result<HashMap<(String, Side, usize), String>, String> {
    let mut out = HashMap::new();
    let r = call("differential.querydiffs", json!({"ids": [diff]}))?;
    let Some(d) = r.get(diff.to_string()) else {
        return Ok(out);
    };
    for ch in d
        .get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path = ch
            .get("currentPath")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        for h in ch
            .get("hunks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // Written out, as Conduit gives them, or as numbers.
            let offset = |k: &str| h.get(k).and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64())).unwrap_or(1) as usize;
            let (mut old, mut new) = (offset("oldOffset"), offset("newOffset"));
            let corpus = h.get("corpus").and_then(Value::as_str).unwrap_or("");
            for l in corpus.lines() {
                let text: String = l.chars().skip(1).collect();
                match l.chars().next() {
                    Some('-') => {
                        out.insert((path.clone(), Side::Old, old), text);
                        old += 1;
                    }
                    Some('+') => {
                        out.insert((path.clone(), Side::New, new), text);
                        new += 1;
                    }
                    Some('\\') => {}
                    _ => {
                        out.insert((path.clone(), Side::Old, old), text.clone());
                        out.insert((path.clone(), Side::New, new), text);
                        old += 1;
                        new += 1;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Every transaction of `revision` (`D123`), as transaction.search gives
/// them, page after page, Conduit called with `call`.
fn transactions(call: &mut Call, revision: &str) -> Result<Vec<Value>, String> {
    let mut txns: Vec<Value> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut params = json!({"objectIdentifier": revision, "limit": 100});
        if let Some(a) = &after {
            params["after"] = a.clone().into();
        }
        let r = call("transaction.search", params)?;
        txns.extend(
            r.get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        after = r
            .pointer("/cursor/after")
            .and_then(Value::as_str)
            .map(String::from);
        if after.is_none() {
            break;
        }
    }
    Ok(txns)
}

/// Every comment on `revision` (`D123`), at `link`, Conduit called with
/// `call`: as a pull reads them, each by its author's name, its line's
/// text, and the lines a suggested edit of it replaces, read from the diff
/// it was made on, on its side. Each diff an inline comment is on is read
/// once, first: a call that fails fails the read, saying why, as any call
/// of a pull does, so that no comment is filed without the lines a passing
/// failure kept from it. Lines the diff read does not hold are none.
fn comments_over(call: &mut Call, revision: &str, link: &str) -> Result<Vec<Remote>, String> {
    let txns = transactions(call, revision)?;
    let mut authors: Vec<String> = txns
        .iter()
        .filter_map(|t| {
            t.get("authorPHID")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect();
    authors.sort();
    authors.dedup();
    let mut names: HashMap<String, String> = HashMap::new();
    if !authors.is_empty() {
        let r = call("user.search", json!({"constraints": {"phids": authors}}))?;
        for u in r
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let phid = u.get("phid").and_then(Value::as_str).unwrap_or("");
            let name = u
                .pointer("/fields/username")
                .and_then(Value::as_str)
                .unwrap_or("someone");
            names.insert(phid.into(), name.into());
        }
    }
    // The diffs the inline comments are on, each once, as they come.
    let mut ids: Vec<u64> = Vec::new();
    for t in &txns {
        let on = t.get("type").and_then(Value::as_str) == Some("inline") && t.pointer("/fields/path").and_then(Value::as_str).is_some();
        if let Some(d) = t.pointer("/fields/diff/id").and_then(Value::as_u64).filter(|d| on && !ids.contains(d)) {
            ids.push(d);
        }
    }
    let mut diffs: HashMap<u64, HashMap<(String, Side, usize), String>> = HashMap::new();
    for d in ids {
        diffs.insert(d, diff_lines(call, d)?);
    }
    Ok(comments_of(&txns, link, &names, |path, diff, side, line| diffs.get(&diff)?.get(&(path.to_string(), side, line)).cloned()))
}

/// What ends the id of a comment's version read with the suggested edit
/// Phabricator gives with it (`Remote::phid`), after the version's own id.
const SUGGESTED: &str = "+suggestion";

/// Whether the version of a comment of id `id` is the one of id `was` read
/// again with the suggested edit Phabricator gives with it (`SUGGESTED`):
/// the same version on Phabricator, which no one edited, a comment filed
/// without its suggested edit read so once a pull reads that.
pub fn anew(id: &str, was: &str) -> bool {
    id.strip_suffix(SUGGESTED) == Some(was)
}

/// The `length` lines from `line` on (Conduit's `length`, how many lines a
/// comment is on), said: `line 2906`, `lines 2906–2909`; on the old side of
/// their diff, `old_side`, those of the file before the patch.
fn lines_said(line: usize, length: usize, old_side: bool) -> String {
    let lines = match length {
        0 | 1 => format!("line {line}"),
        n => format!("lines {line}–{}", line + n - 1),
    };
    match old_side {
        true => format!("{lines} of the file before the patch"),
        false => lines,
    }
}

/// What an inline comment of `who`'s that is a suggested edit alone says,
/// as a pull reads it from a Phabricator that gives no suggested edits, its
/// transactions saying nothing of them (`hasSuggestion`): there
/// transaction.search gives a comment's words alone, and such a comment has
/// none. So it says that `who` suggested an edit to the lines it is on, the
/// `length` lines from `line` on, which stacked cannot read, as this
/// Phabricator's API does not give it, and where Phabricator shows it, `at`
/// (`inline_at`).
fn suggested(who: &str, line: usize, length: usize, at: &str) -> String {
    let lines = lines_said(line, length, false);
    format!("{who} suggested an edit to {lines} here, which stacked cannot read, as this Phabricator's API does not give suggested edits: see it on Phabricator, {at}")
}

/// What a stacked of before, which read no suggested edit of any
/// Phabricator, said of an inline comment of `who`'s that is a suggested
/// edit alone: as `suggested` says it, but that Phabricator's API gives
/// none. What a thread it filed says, and so what such a comment said as
/// read without the suggested edit Phabricator gives with it (`comments_of`),
/// for the comment read with it to redact that turn.
fn suggested_before(who: &str, line: usize, length: usize, at: &str) -> String {
    let lines = lines_said(line, length, false);
    format!("{who} suggested an edit to {lines} here, which stacked cannot read, as Phabricator's API does not give suggested edits: see it on Phabricator, {at}")
}

/// What an inline comment of `who`'s says of the suggested edit Phabricator
/// gives with it, `suggestion` (its `suggestionText`), of the `length` lines
/// from `line` on, on the old side of its diff if `old_side`: a sentence
/// saying that `who` suggests it, then the edit as a unified diff writes
/// it, between the fences of a `diff` code block, so that its lines are
/// drawn and told as they are, none read as markdown (a `-` or a `+` would
/// be a list's bullet), nor folded between words. Each line it replaces, as
/// the diff it was made on has it on that side (`replaced`), after `-`;
/// then each line of the suggestion after `+`, a newline ending its last
/// line, not starting one more. Nothing of either is trimmed. With no line
/// of its own, it deletes those it is on. `replaced` none, as a line of
/// those could not be read: its own lines alone, saying so, never a guess.
fn suggestion_said(who: &str, line: usize, length: usize, old_side: bool, replaced: Option<&[String]>, suggestion: &str) -> String {
    let lines = lines_said(line, length, old_side);
    let added: Vec<&str> = suggestion.lines().collect();
    let said = match (replaced.is_some(), added.is_empty()) {
        (true, false) => format!("{who} suggests this edit of {lines}:"),
        (true, true) => format!("{who} suggests deleting {lines}:"),
        (false, false) => format!("{who} suggests this edit of {lines}, but the lines replaced could not be read from Phabricator, so only the new lines are shown:"),
        (false, true) => return format!("{who} suggests deleting {lines}, but those lines could not be read from Phabricator."),
    };
    let diff: Vec<String> = replaced.into_iter().flatten().map(|l| format!("-{l}")).chain(added.iter().map(|l| format!("+{l}"))).collect();
    format!("{said}\n```diff\n{}\n```", diff.join("\n"))
}

/// Where Phabricator shows the inline comment of id `id` (the `id`
/// transaction.search gives of it), made on diff `diff`, of the revision at
/// `link`: at its anchor, `#inline-` and its id, as Phabricator names each
/// inline comment it draws, on the revision's page showing that diff
/// (`?id=`), as the revision's history links an inline comment made on
/// another diff than the one it shows. It is drawn with the diff it was
/// made on, whatever diffs came after. Its diff not known, the anchor on the
/// revision's page; its id not known, that page.
fn inline_at(link: &str, diff: Option<u64>, id: Option<u64>) -> String {
    match (diff, id) {
        (Some(diff), Some(id)) => format!("{link}?id={diff}#inline-{id}"),
        (None, Some(id)) => format!("{link}#inline-{id}"),
        (_, None) => link.to_string(),
    }
}

/// The comments of a revision, the one at `link`, oldest first, from its
/// transactions as transaction.search gives them: each its author's name,
/// from `names` by their PHID, and the text of the lines it is on, as
/// `read_line` reads each from (path, diff, side, line), on the side of its
/// diff Phabricator says it is on (`isNewFile`), else on the new one. Each
/// is as it was last edited, what it was before kept with it; a comment
/// removed is none.
///
/// An inline comment's suggested edit, which Mozilla's Phabricator gives in
/// its transaction as the comment is now (`hasSuggestion`,
/// `suggestionText`), follows its words, as a diff of the lines it replaces
/// (`suggestion_said`). The comment so read is a version of its own, its id
/// the latest version's and `SUGGESTED`; that version as read without its
/// suggested edit is the one before it, saying what a stacked that read
/// none filed of it, so that a comment filed so is held to take the place
/// of its turn, as what it is, no edit (`anew`). Phabricator gives no
/// suggested edit of an older version: each older one
/// is known by both its ids, as either may be the one filed, and says what
/// it said read without one. Where this Phabricator says that a comment has
/// no suggested edit, an inline comment with no words says nothing, and is
/// none, as a comment on the whole revision with none is. Where it says
/// nothing of suggested edits, a version of an inline comment with no words
/// is a suggested edit alone, which this Phabricator does not give: it says
/// so, with where Phabricator shows it (`suggested`).
fn comments_of(txns: &[Value], link: &str, names: &HashMap<String, String>, mut read_line: impl FnMut(&str, u64, Side, usize) -> Option<String>) -> Vec<Remote> {
    let mut comments = Vec::new();
    for t in txns {
        let kind = t.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "inline" && kind != "comment" {
            continue;
        }
        let author = t
            .get("authorPHID")
            .and_then(Value::as_str)
            .and_then(|p| names.get(p))
            .cloned()
            .unwrap_or_else(|| "someone".into());
        let fields = t.get("fields").cloned().unwrap_or(Value::Null);
        let path = (kind == "inline")
            .then(|| fields.get("path").and_then(Value::as_str).map(String::from))
            .flatten();
        let line = fields.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
        // How many lines it is on, from `line`: Conduit's `length`, one at
        // least.
        let length = (fields.get("length").and_then(Value::as_u64).unwrap_or(1) as usize).max(1);
        let diff = fields.pointer("/diff/id").and_then(Value::as_u64);
        // The side of its diff it is on: the old one where Phabricator says
        // so, else the new one, as a Phabricator that does not say has them.
        let old_side = path.is_some() && fields.get("isNewFile").and_then(Value::as_bool) == Some(false);
        let side = if old_side { Side::Old } else { Side::New };
        let mut read = |at: usize| match (&path, diff) {
            (Some(p), Some(d)) => read_line(p, d, side, at),
            _ => None,
        };
        let line_text = read(line);
        let reply_to = fields
            .get("replyToCommentPHID")
            .and_then(Value::as_str)
            .map(String::from);
        // Whether it has a suggested edit, as it is now, where this
        // Phabricator says (`hasSuggestion`), as only an inline comment may.
        // Only that says it: `suggestionText` is none without one, but
        // empty on Phabricators of before, as for an edit that deletes.
        let suggests = (kind == "inline").then(|| fields.get("hasSuggestion").and_then(Value::as_bool)).flatten();
        // Each of its versions, the latest first: an edit on Phabricator
        // makes one, with an id of its own.
        let mut versions: Vec<&Value> = t
            .get("comments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        versions.sort_by_key(|c| std::cmp::Reverse(c.get("version").and_then(Value::as_i64).unwrap_or(0)));
        let removed = |c: &Value| c.get("removed").and_then(Value::as_bool).unwrap_or(false);
        // Removed, it is gone: its latest version says so, and what every
        // version of it said is hidden, an older version saying nothing,
        // though it is not removed itself.
        if versions.first().is_none_or(|c| removed(c)) {
            continue;
        }
        let words = |c: &Value| c.pointer("/content/raw").and_then(Value::as_str).unwrap_or("").trim().to_string();
        // What a version says as a stacked that read no suggested edit filed
        // it: its words; an inline comment's with none, a suggested edit
        // alone, said so, as this stacked says it where Phabricator gives
        // none, else as a stacked of before said it.
        let before = |c: &Value| {
            let words = words(c);
            let at = || inline_at(link, diff, c.get("id").and_then(Value::as_u64));
            match (words.is_empty() && kind == "inline", suggests) {
                (false, _) => words,
                (true, None) => suggested(&author, line, length, &at()),
                (true, Some(_)) => suggested_before(&author, line, length, &at()),
            }
        };
        let phid = |c: &Value| c.get("phid").and_then(Value::as_str).unwrap_or("").to_string();
        // The version it is read as, its id, and what it says: with a
        // suggested edit, the latest, its words then the edit; else as
        // before, but an inline comment's words alone where this
        // Phabricator says it has none, which may be none.
        let (at, id, text) = match suggests {
            Some(true) => {
                let replaced: Option<Vec<String>> = (line..line + length).map(&mut read).collect();
                let suggestion = fields.get("suggestionText").and_then(Value::as_str).unwrap_or("");
                let edit = suggestion_said(&author, line, length, old_side, replaced.as_deref(), suggestion);
                let text = match words(versions[0]) {
                    w if w.is_empty() => edit,
                    w => format!("{w}\n\n{edit}"),
                };
                (0, format!("{}{SUGGESTED}", phid(versions[0])), text)
            }
            _ => {
                let says = |c: &Value| if suggests.is_some() { words(c) } else { before(c) };
                let Some(at) = versions.iter().position(|c| !removed(c) && !says(c).is_empty()) else {
                    continue;
                };
                (at, phid(versions[at]), says(versions[at]))
            }
        };
        // The versions before it, the latest first: the one it is read from
        // with its suggested edit, as read without it; then each older one,
        // by both its ids where this Phabricator gives suggested edits.
        let mut earlier = Vec::new();
        if suggests == Some(true) {
            earlier.push(Version { phid: phid(versions[0]), text: before(versions[0]) });
        }
        for c in &versions[at + 1..] {
            if suggests.is_some() {
                earlier.push(Version { phid: format!("{}{SUGGESTED}", phid(c)), text: before(c) });
            }
            earlier.push(Version { phid: phid(c), text: before(c) });
        }
        comments.push(Remote {
            phid: id,
            author,
            text,
            created: versions.last().and_then(|c| c.get("dateCreated")).and_then(Value::as_i64).unwrap_or(0),
            path,
            line,
            old_side,
            line_text,
            reply_to,
            earlier,
        });
    }
    comments.sort_by_key(|c| c.created);
    comments
}

/// The revision `text` names, `D123` (or `123`) or its link: its `D123`,
/// and the link if one was given. None if it names none.
pub fn revision_named(text: &str) -> Option<(String, Option<String>)> {
    // A link as the browser gives it: its anchor, its query left out.
    let t = text.trim();
    let t = t.split(['#', '?']).next().unwrap_or(t).trim_end_matches('/');
    let last = t.rsplit('/').next()?;
    let link = t.contains('/');
    let digits = match last.strip_prefix('D') {
        Some(digits) => digits,
        // A number alone names one too; a link, only as D123 does.
        None if !link => last,
        None => return None,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((format!("D{digits}"), link.then(|| t.to_string())))
}

/// How Conduit is called: a method, its parameters; its `result`.
pub(crate) type Call<'a> = dyn FnMut(&str, Value) -> Result<Value, String> + 'a;

// -- a pull: the revisions, their parents and those stacked on them ------ //

/// How a pull came to read a revision: asked (a revision the stack names,
/// the patch's own, the one a new stack is pulled from); the parent on
/// Phabricator, open, of revision `D123` it read, which the stack does not
/// hold; or stacked there on revision `D123` it read. Where an added
/// revision goes is told by it: below the patch of the one it is the parent
/// of, above that of the one it is stacked on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Reached {
    #[default]
    Asked,
    Parent(String),
    Child(String),
}

/// A revision as a pull reads it (`read_pull`): its `D123`, its link and
/// its PHID; its status, as Phabricator writes its value (`needs-review`),
/// its title, summary and bug; its parents, each its PHID and its `D123`
/// (its PHID again where none was read), however open; how the pull came to
/// read it; the id of its latest diff (0 for none), stored raw as a
/// rawdiff record, and the older diffs whose raw diff it read too, the
/// newest first, to the first that is a version of its patch stacked knows;
/// the commit its latest diff is on, and the author and message of that
/// diff's commit, as uploaded, when it says; its comments, as a pull reads
/// them; when it was last modified, whether it is a draft, and its
/// repository, as its search said (none of whether it is a draft in a read
/// a stacked of before kept). Kept in the pull's run while it waits its
/// turn, and made from at its turn (`review::make`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Read {
    pub d: String,
    pub link: String,
    pub phid: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bug: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<(String, String)>,
    #[serde(default)]
    pub reached: Reached,
    #[serde(default)]
    pub diff: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub older: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Remote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
}

impl Read {
    /// Its status, as stacked knows it.
    pub fn status(&self) -> Status {
        Status::from_value(&self.status)
    }
}

/// What a pull asks of Phabricator: the revisions `asked`, each `D123` and
/// its link, bottom first; `held`, the links of the revisions the stack
/// holds, where the walks down and up stop; and `up`, the link of the
/// revision to walk up from, through the revisions stacked on it, if those
/// are pulled too.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wants {
    pub asked: Vec<(String, String)>,
    pub held: Vec<String>,
    pub up: Option<String>,
}

/// What a pull read: each revision, in the order it read them (the asked
/// first, bottom first, then their parents as the walk down found them,
/// then those stacked on top, as the walk up found them); the raw diffs it
/// read, by link and id, to store; and the asked revisions Phabricator does
/// not have, each `D123` and its link.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PullRead {
    pub revisions: Vec<Read>,
    pub raws: Vec<(String, u64, String)>,
    pub missing: Vec<(String, String)>,
}

/// Why a pull reads nothing more: revision `d` has more than one open
/// parent on Phabricator, `on`, or more than one open revision stacked on
/// it, `on`, the walk up going through it. A stack is one line of patches.
fn more_than_one(d: &str, on: &[String], parents: bool) -> String {
    let on = crate::submit::and(on);
    match parents {
        true => format!("{d} is stacked on more than one open revision on Phabricator, {on}: a stack is one line of patches, so nothing was pulled"),
        false => format!("More than one open revision is stacked on {d} on Phabricator, {on}: a stack is one line of patches, so nothing was pulled"),
    }
}

/// A revision as a search finds it, as a pull reads it: its `D123`, its
/// link on `base` (`https://phabricator.example.com`), and what the search
/// says, `reached` as the pull came to it.
fn read_of(d: &str, base: &str, s: &Searched, reached: Reached) -> Read {
    Read {
        d: d.to_string(),
        link: format!("{base}/{d}"),
        phid: s.phid.clone(),
        status: s.status.value().to_string(),
        title: s.title.clone(),
        summary: s.summary.clone(),
        bug: s.bug.clone(),
        reached,
        modified: s.modified,
        draft: Some(s.draft),
        repository: s.repository.clone(),
        ..Default::default()
    }
}

/// The revisions of PHIDs `phids`, as one search finds them, each with its
/// `D123`, by PHID.
fn searched_by_phid(call: &mut Call, phids: &[String]) -> Result<HashMap<String, (String, Searched)>, String> {
    if phids.is_empty() {
        return Ok(HashMap::new());
    }
    let found = call("differential.revision.search", json!({"constraints": {"phids": phids}}))?;
    Ok(found
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(searched_of)
        .map(|(d, s)| (s.phid.clone(), (d, s)))
        .collect())
}

/// Reads for a pull what `wants` asks (`Wants`), through the stand-in set
/// for the database at `db`, else over Conduit on the Phabricator the first
/// revision asked is on, never from a process that keeps nothing: first the
/// revisions asked, then, down from each, its open parent while the stack
/// does not hold it, then, up from `wants.up`, the open revision stacked on
/// it while there is one, the stack holding it or not; each with its
/// parents, named. More than one open parent, or more than one open
/// revision stacked on one where the walk up goes, and nothing more is read,
/// said why. Then each revision in turn, `say` told it as it starts (`D123`,
/// from 1, of how many): its diffs, its latest raw unless `stored(link,
/// id)` has it; while `known(link, raw)` says that diff is no version of
/// its patch stacked knows (`Some(false)`), the older ones, the newest
/// first, until it says one is; and its comments. What is wrong, said, when
/// a call fails, Phabricator turning this machine away as such: no call is
/// made after it.
pub fn read_pull(
    db: &Path,
    wants: &Wants,
    known: &dyn Fn(&str, &str) -> Option<bool>,
    stored: &dyn Fn(&str, u64) -> Option<String>,
    say: &mut dyn FnMut(&str, usize, usize),
) -> Result<PullRead, Error> {
    let Some((_, first)) = wants.asked.first() else { return Ok(PullRead::default()) };
    calling(db, first, |call| pull_over(call, wants, known, stored, say))
}

/// As `read_pull`, Conduit called with `call`.
fn pull_over(
    call: &mut Call,
    wants: &Wants,
    known: &dyn Fn(&str, &str) -> Option<bool>,
    stored: &dyn Fn(&str, u64) -> Option<String>,
    say: &mut dyn FnMut(&str, usize, usize),
) -> Result<PullRead, String> {
    let base = wants.asked.first().and_then(|(_, l)| l.rsplit_once('/')).map_or(String::new(), |(b, _)| b.to_string());
    let ids: Vec<String> = wants.asked.iter().map(|(d, _)| d.clone()).collect();
    let found = statuses_over(call, &ids)?;
    let mut out = PullRead::default();
    let mut reads: Vec<Read> = Vec::new();
    for (d, link) in &wants.asked {
        match found.get(d) {
            Some(s) => reads.push(Read { link: link.clone(), ..read_of(d, &base, s, Reached::Asked) }),
            None => out.missing.push((d.clone(), link.clone())),
        }
    }
    // What is known of each revision met, by PHID: its `D123`, and what its
    // search said.
    let mut met: HashMap<String, (String, Searched)> = HashMap::new();
    for r in &reads {
        if let Some(s) = found.get(&r.d) {
            met.insert(r.phid.clone(), (r.d.clone(), s.clone()));
        }
    }
    let held = |link: &str| wants.held.iter().any(|l| l == link);
    // Their parents, each named, then down: the open parent of each, while
    // the stack does not hold it and it is not read already, read too.
    let mut looked = 0;
    while looked < reads.len() {
        let these: Vec<String> = reads[looked..].iter().map(|r| r.phid.clone()).collect();
        let parents = parents_over(call, &these)?;
        let unknown: Vec<String> = {
            let mut u: Vec<String> = parents.values().flatten().filter(|p| !met.contains_key(*p)).cloned().collect();
            u.sort();
            u.dedup();
            u
        };
        met.extend(searched_by_phid(call, &unknown)?);
        let end = reads.len();
        for i in looked..end {
            let ps = parents.get(&reads[i].phid).cloned().unwrap_or_default();
            reads[i].parents = ps.iter().map(|p| (p.clone(), met.get(p).map_or_else(|| p.clone(), |(d, _)| d.clone()))).collect();
            let open: Vec<&(String, Searched)> = ps.iter().filter_map(|p| met.get(p)).filter(|(_, s)| s.status.open()).collect();
            if open.len() > 1 {
                let on: Vec<String> = open.iter().map(|(d, _)| d.clone()).collect();
                return Err(more_than_one(&reads[i].d, &on, true));
            }
            let [(d, s)] = open.as_slice() else { continue };
            if held(&format!("{base}/{d}")) || reads.iter().any(|r| r.phid == s.phid) {
                continue;
            }
            let at = reads[i].d.clone();
            reads.push(read_of(d, &base, s, Reached::Parent(at)));
        }
        looked = end;
    }
    // Up: the open revision stacked on each, while there is one, read too,
    // the stack holding it or not: it is pulled with the one under it.
    if let Some(mut at) = wants.up.as_ref().and_then(|up| reads.iter().position(|r| r.link == *up)) {
        loop {
            let on = reads[at].phid.clone();
            let kids = edges_over(call, std::slice::from_ref(&on), "revision.child")?.remove(&on).unwrap_or_default();
            let unknown: Vec<String> = kids.iter().filter(|p| !met.contains_key(*p)).cloned().collect();
            met.extend(searched_by_phid(call, &unknown)?);
            let open: Vec<(String, Searched)> = kids.iter().filter_map(|p| met.get(p)).filter(|(_, s)| s.status.open()).cloned().collect();
            if open.len() > 1 {
                let on: Vec<String> = open.iter().map(|(d, _)| d.clone()).collect();
                return Err(more_than_one(&reads[at].d, &on, false));
            }
            let [(d, s)] = open.as_slice() else { break };
            // Met again: the stack holds it below, or Phabricator goes round.
            if reads.iter().any(|r| r.phid == s.phid) {
                break;
            }
            let under = reads[at].d.clone();
            reads.push(read_of(d, &base, s, Reached::Child(under)));
            at = reads.len() - 1;
            // Its parents, named, and more than one refused.
            let parents = parents_over(call, std::slice::from_ref(&s.phid))?;
            let ps = parents.get(&s.phid).cloned().unwrap_or_default();
            let unknown: Vec<String> = ps.iter().filter(|p| !met.contains_key(*p)).cloned().collect();
            met.extend(searched_by_phid(call, &unknown)?);
            reads[at].parents = ps.iter().map(|p| (p.clone(), met.get(p).map_or_else(|| p.clone(), |(d, _)| d.clone()))).collect();
            let open: Vec<String> = ps.iter().filter_map(|p| met.get(p)).filter(|(_, s)| s.status.open()).map(|(d, _)| d.clone()).collect();
            if open.len() > 1 {
                return Err(more_than_one(d, &open, true));
            }
        }
    }
    // Then each in turn: its diffs, its comments.
    let n = reads.len();
    for (k, r) in reads.iter_mut().enumerate() {
        say(&r.d, k + 1, n);
        let num: u64 = r.d.trim_start_matches('D').parse().map_err(|_| format!("{} names no revision", r.d))?;
        let diffs = call("differential.querydiffs", json!({"revisionIDs": [num]}))?;
        let mut all: Vec<(u64, &Value)> = diffs.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.parse::<u64>().ok()?, v))).collect();
        all.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
        let mut raw_of = |id: u64, out: &mut PullRead| -> Result<String, String> {
            if let Some(raw) = stored(&r.link, id).or_else(|| out.raws.iter().find(|(l, i, _)| *l == r.link && *i == id).map(|(_, _, raw)| raw.clone())) {
                return Ok(raw);
            }
            let raw = call("differential.getrawdiff", json!({"diffID": id}))?.as_str().unwrap_or_default().to_string();
            out.raws.push((r.link.clone(), id, raw.clone()));
            Ok(raw)
        };
        if let Some((latest, d)) = all.first() {
            r.diff = *latest;
            r.base = d.get("sourceControlBaseRevision").and_then(Value::as_str).filter(|b| !b.is_empty()).map(String::from);
            let commit = d.pointer("/properties/local:commits").and_then(Value::as_object).and_then(|m| m.values().next());
            let said = |k: &str| commit.and_then(|c| c.get(k)).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
            r.author = said("author").map(|name| match said("authorEmail") {
                Some(e) => format!("{name} <{e}>"),
                None => name,
            });
            r.message = said("message");
            let raw = raw_of(*latest, &mut out)?;
            // Not a version of its patch stacked knows: the version it was
            // last, the older diffs read to the first that is one.
            if known(&r.link, &raw) == Some(false) {
                for (id, _) in all.iter().skip(1) {
                    let raw = raw_of(*id, &mut out)?;
                    r.older.push(*id);
                    if known(&r.link, &raw) == Some(true) {
                        break;
                    }
                }
            }
        }
        r.comments = comments_over(call, &r.d, &r.link)?;
    }
    out.revisions = reads;
    Ok(out)
}

/// Stores in `db` what a pull read, `read`, as a fetch stores what it reads
/// (`store`): of each revision, its state and its comments, with when the
/// revision was last modified as they were read, only where they differ
/// from what is stored; each raw diff it read, unless it is stored; the
/// revisions asked that Phabricator does not have, as such; then that
/// every one of them was read, now.
pub fn store_read(db: &mut Db, read: &PullRead) -> crate::db::Result<()> {
    for (link, id, raw) in &read.raws {
        db.store_raw_diff(link, *id, raw)?;
    }
    for r in &read.revisions {
        let state = State {
            missing: false,
            status: r.status.clone(),
            title: r.title.clone(),
            summary: r.summary.clone(),
            bug: r.bug.clone(),
            phid: r.phid.clone(),
            parents: parents_known(db, &r.parents),
            diff: r.diff,
            base: r.base.clone().unwrap_or_default(),
            draft: r.draft,
            repository: r.repository.clone(),
        };
        let comments = Comments { modified: r.modified, list: r.comments.iter().map(seen).collect() };
        db.store_revision(&r.link, &r.d, Some(state), Some(comments))?;
    }
    for (d, link) in &read.missing {
        db.store_revision(link, d, Some(State { missing: true, ..Default::default() }), None)?;
    }
    let mut links: Vec<String> = read.revisions.iter().map(|r| r.link.clone()).collect();
    links.extend(read.missing.iter().map(|(_, l)| l.clone()));
    if links.is_empty() {
        return Ok(());
    }
    db.store_fetched(links)
}

/// The key of the line a comment is on, among the lines of its side of the
/// patch: the line with its text near its number, else the line at its
/// number, else None. On the new side, the lines the patch leaves; on the
/// old side (`Remote::old_side`), the lines it found, those it removes
/// among them, which a comment is on as on any line.
pub fn anchor(lines: &[NoteLine], c: &Remote) -> Option<String> {
    let path = c.path.as_deref()?;
    // Its side's lines and their numbers there: those of context, and those
    // the patch adds on the new side, those it removes on the old one; not
    // git's `\ No newline at end of file`, which has the numbers of the
    // lines after it.
    let theirs = if c.old_side { '-' } else { '+' };
    let no = |l: &NoteLine| if c.old_side { l.old_no } else { l.new_no };
    let here: Vec<&NoteLine> = lines
        .iter()
        .filter(|l| l.file == path && (l.origin == ' ' || l.origin == theirs))
        .collect();
    if let Some(t) = &c.line_text {
        let want = fold(t);
        if let Some(l) = here
            .iter()
            .filter(|l| l.text == want)
            .min_by_key(|l| no(l).abs_diff(c.line))
        {
            return Some(l.key.clone());
        }
    }
    here.iter()
        .find(|l| no(l) == c.line)
        .map(|l| l.key.clone())
}

/// A patch's revision, `D123`, and the link its trailer gives: the
/// settings' trailer (`Differential Revision:`), a bare `D123` on the
/// settings' Phabricator.
pub fn revision_of(message: &str) -> Option<(String, String)> {
    let cfg = &crate::config::get().reviews;
    revision_in(message, &cfg.trailer, cfg.url.as_deref())
}

/// As `revision_of`, the trailer named `trailer`, a bare `D123` on `url`:
/// the last line that says one, trailers being last, the message above it
/// free to name another revision. Of one line, the revision it names, if it
/// does: what a line of a message opens in the browser (`links`).
pub(crate) fn revision_in(message: &str, trailer: &str, url: Option<&str>) -> Option<(String, String)> {
    let trailer = format!("{trailer}:");
    message.lines().rev().find_map(|line| {
        let rest = line.strip_prefix(&trailer)?.trim();
        let last = rest.rsplit('/').next()?;
        let digits = last.strip_prefix('D')?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let link = if rest.contains('/') {
            rest.to_string()
        } else {
            format!("{}/{last}", url?)
        };
        Some((last.to_string(), link))
    })
}

/// A revision's status on Phabricator, as the patches pane shows it after
/// each patch's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Draft,
    ChangesPlanned,
    NeedsReview,
    NeedsRevision,
    Accepted,
    /// Closed, as Phabricator names it: landed.
    Published,
    Abandoned,
    /// One stacked does not know.
    #[default]
    Unknown,
}

impl Status {
    /// The status of revision `r`, as differential.revision.search gives
    /// it: its value alone (`fields.status.value`). `isDraft` is not read:
    /// Phabricator sets it (as `!shouldBroadcast`) on every revision it has
    /// not sent to reviewers yet, whatever its status: a draft, but also a
    /// revision in Changes Planned that was never sent for review, as a
    /// submit in work makes one, and one abandoned before then.
    pub fn of(r: &Value) -> Status {
        Status::from_value(r.pointer("/fields/status/value").and_then(Value::as_str).unwrap_or_default())
    }

    /// The status whose value, as differential.revision.search writes it,
    /// is `value`: as the database stores it too (`db::State::status`).
    pub fn from_value(value: &str) -> Status {
        match value {
            "draft" => Status::Draft,
            "changes-planned" => Status::ChangesPlanned,
            "needs-review" => Status::NeedsReview,
            "needs-revision" => Status::NeedsRevision,
            "accepted" => Status::Accepted,
            "published" => Status::Published,
            "abandoned" => Status::Abandoned,
            _ => Status::Unknown,
        }
    }

    /// Its value, as differential.revision.search writes it, and the
    /// database stores it: `unknown` for one stacked does not know.
    pub fn value(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::ChangesPlanned => "changes-planned",
            Status::NeedsReview => "needs-review",
            Status::NeedsRevision => "needs-revision",
            Status::Accepted => "accepted",
            Status::Published => "published",
            Status::Abandoned => "abandoned",
            Status::Unknown => "unknown",
        }
    }

    /// The patches pane's status, after the patch's number: a letter, in
    /// brackets.
    pub fn letter(self) -> &'static str {
        match self {
            Status::Draft => "[D]",
            Status::ChangesPlanned => "[W]",
            Status::NeedsReview => "[R]",
            Status::NeedsRevision => "[C]",
            Status::Accepted => "[A]",
            Status::Published => "[L]",
            Status::Abandoned => "[X]",
            Status::Unknown => "[?]",
        }
    }

    /// Whether a revision of this status is open, as `open` reads one:
    /// neither landed nor abandoned.
    pub fn open(self) -> bool {
        !matches!(self, Status::Published | Status::Abandoned)
    }

    /// The status by its full name, as the Phabricator line over a patch's
    /// message says it: Closed, as Phabricator names it, Landed.
    pub fn name(self) -> &'static str {
        match self {
            Status::Draft => "Draft",
            Status::ChangesPlanned => "Changes Planned",
            Status::NeedsReview => "Needs Review",
            Status::NeedsRevision => "Needs Revision",
            Status::Accepted => "Accepted",
            Status::Published => "Landed",
            Status::Abandoned => "Abandoned",
            Status::Unknown => "status unknown",
        }
    }
}

/// A revision as the statuses' search finds it: its status, its title and
/// summary, its bug, and its PHID, which a fetch reads the revision's
/// latest diff by; when it was last modified (`dateModified`, seconds since
/// 1970), when the search says, which a fetch reads the rest of the
/// revision again by; whether it is a draft (`isDraft`), which a submit
/// takes as in work, as moz-phab does; and the PHID of its repository
/// (`repositoryPHID`), empty for none, which a new diff of it is made in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Searched {
    pub status: Status,
    pub title: String,
    pub summary: String,
    pub bug: String,
    pub phid: String,
    pub modified: Option<i64>,
    pub draft: bool,
    pub repository: String,
}

/// The statuses of revisions `ids` (`D123`), Conduit called with `call`:
/// one search for all of them. By revision, those Phabricator has, each
/// with its title, summary, bug and PHID, and when it was last modified.
pub(crate) fn statuses_over(call: &mut Call, ids: &[String]) -> Result<HashMap<String, Searched>, String> {
    let numbers: Vec<u64> = ids.iter().filter_map(|d| d.strip_prefix('D')?.parse().ok()).collect();
    if numbers.is_empty() {
        return Ok(HashMap::new());
    }
    let found = call("differential.revision.search", json!({"constraints": {"ids": numbers}}))?;
    Ok(found.get("data").and_then(Value::as_array).into_iter().flatten().filter_map(searched_of).collect())
}

/// Revision `r`, as differential.revision.search gives it: its `D123`, and
/// what the search says of it (`Searched`). None without its number.
fn searched_of(r: &Value) -> Option<(String, Searched)> {
    // A number written out, as the bug's may be.
    let text = |k: &str| match r.pointer(&format!("/fields/{k}")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let searched = Searched {
        status: Status::of(r),
        title: text("title"),
        summary: text("summary"),
        bug: text("bugzilla.bug-id"),
        phid: r.get("phid").and_then(Value::as_str).unwrap_or_default().to_string(),
        modified: r.pointer("/fields/dateModified").and_then(Value::as_i64),
        draft: r.pointer("/fields/isDraft").and_then(Value::as_bool).unwrap_or(false),
        repository: r.pointer("/fields/repositoryPHID").and_then(Value::as_str).unwrap_or_default().to_string(),
    };
    Some((format!("D{}", r.get("id")?.as_u64()?), searched))
}

/// What a fetch reads of a revision beyond what the statuses' search says
/// (`fetch_latest`): the id of its latest diff, and that diff, raw; its
/// comments, each as it was last edited, what it was before kept with it,
/// as a pull reads them (`comments_of`), but for their authors and the
/// lines they are on, which are not read; its parents, each its PHID and
/// its `D123`, by PHID (`parents_named`); and when the revision was last
/// modified as its diff and comments were read, as the statuses' search
/// said it (`Searched::modified`): none when that is not known.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Latest {
    pub diff: u64,
    /// The commit `diff` is on, as Phabricator names it: empty where it
    /// says none.
    pub base: String,
    pub raw: String,
    pub comments: Vec<Remote>,
    pub parents: Vec<(String, String)>,
    pub modified: Option<i64>,
}

/// A revision whose latest diff, comments and parents a fetch reads
/// (`latests`): its `D123`, its link and its PHID, and when it was last
/// modified, as the statuses' search found it; what is stored of it, if
/// anything, which is not read again while the revision was not modified
/// since, its parents aside, and its raw diff not while it is the latest;
/// and whether the parents it has are to be named, as a submit would set it
/// one (not at the stack's bottom, say), so that the view of what differs
/// names them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wanted {
    pub d: String,
    pub link: String,
    pub phid: String,
    pub modified: Option<i64>,
    pub had: Option<Latest>,
    pub name_parents: bool,
}

/// Where the edges of kind `revision.parent` from each of revisions `phids`
/// go, by revision: their PHIDs, sorted, none for one with no parent. One
/// edge.search for all of them, page after page.
pub(crate) fn parents_over(call: &mut Call, phids: &[String]) -> Result<HashMap<String, Vec<String>>, String> {
    edges_over(call, phids, "revision.parent")
}

/// Where the edges of kind `kind` (`revision.parent`, `revision.child`)
/// from each of revisions `phids` go, by revision: their PHIDs, sorted,
/// none for one with no such edge. One edge.search for all of them, page
/// after page.
fn edges_over(call: &mut Call, phids: &[String], kind: &str) -> Result<HashMap<String, Vec<String>>, String> {
    let mut out: HashMap<String, Vec<String>> = phids.iter().map(|p| (p.clone(), Vec::new())).collect();
    let mut after: Option<String> = None;
    loop {
        let mut params = json!({"sourcePHIDs": phids, "types": [kind]});
        if let Some(a) = &after {
            params["after"] = a.clone().into();
        }
        let r = call("edge.search", params)?;
        for e in r.get("data").and_then(Value::as_array).into_iter().flatten() {
            let (Some(from), Some(to)) = (e.get("sourcePHID").and_then(Value::as_str), e.get("destinationPHID").and_then(Value::as_str)) else { continue };
            if let Some(these) = out.get_mut(from) {
                these.push(to.to_string());
            }
        }
        after = r.pointer("/cursor/after").and_then(Value::as_str).map(String::from);
        if after.is_none() {
            break;
        }
    }
    for these in out.values_mut() {
        these.sort();
        these.dedup();
    }
    Ok(out)
}

/// The id of the latest diff of each of revisions `phids`, by revision (its
/// `fields.revisionPHID`), with the commit it is on as Phabricator names it
/// (its `base` ref), empty where it says none: differential.diff.search for
/// all of them, the newest first, page after page until each has its
/// newest, or no page is left. None for a revision with no diff.
pub(crate) fn newest_diffs(call: &mut Call, phids: &[String]) -> Result<HashMap<String, (u64, String)>, String> {
    let mut out: HashMap<String, (u64, String)> = HashMap::new();
    let mut after: Option<String> = None;
    loop {
        let mut params = json!({"constraints": {"revisionPHIDs": phids}, "order": "newest"});
        if let Some(a) = &after {
            params["after"] = a.clone().into();
        }
        let r = call("differential.diff.search", params)?;
        for d in r.get("data").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), Some(rev)) = (d.get("id").and_then(Value::as_u64), d.pointer("/fields/revisionPHID").and_then(Value::as_str)) else { continue };
            let base = d
                .pointer("/fields/refs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|r| r.get("type").and_then(Value::as_str) == Some("base"))
                .and_then(|r| r.get("identifier").and_then(Value::as_str))
                .unwrap_or_default();
            let newest = out.entry(rev.to_string()).or_insert((id, base.to_string()));
            if id > newest.0 {
                *newest = (id, base.to_string());
            }
        }
        after = r.pointer("/cursor/after").and_then(Value::as_str).map(String::from);
        // The newest first: once each revision has one, the rest are older.
        if after.is_none() || phids.iter().all(|p| out.contains_key(p)) {
            break;
        }
    }
    Ok(out)
}

/// The parents of revisions `wanted`, `parents` by revision, named, `D123`,
/// by PHID: those that are revisions of `wanted`, as their search found
/// them, or that what is stored of them named, as a PHID's revision never
/// changes; the others, of those whose parents are to be named, in one
/// search, if there are any. One it does not find, or a search that fails,
/// leaves a parent unnamed.
fn parents_named(call: &mut Call, wanted: &[&Wanted], parents: &HashMap<String, Vec<String>>) -> HashMap<String, String> {
    let mut names: HashMap<String, String> = wanted.iter().map(|w| (w.phid.clone(), w.d.clone())).collect();
    for (phid, d) in wanted.iter().flat_map(|w| w.had.iter().flat_map(|h| &h.parents)) {
        if phid != d {
            names.entry(phid.clone()).or_insert_with(|| d.clone());
        }
    }
    let mut others: Vec<String> = wanted
        .iter()
        .filter(|w| w.name_parents)
        .flat_map(|w| parents.get(&w.phid).into_iter().flatten())
        .filter(|p| !names.contains_key(*p))
        .cloned()
        .collect();
    others.sort();
    others.dedup();
    if others.is_empty() {
        return names;
    }
    let found = call("differential.revision.search", json!({"constraints": {"phids": others}})).unwrap_or(Value::Null);
    for r in found.get("data").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(phid), Some(n)) = (r.get("phid").and_then(Value::as_str), r.get("id").and_then(Value::as_u64)) {
            names.insert(phid.to_string(), format!("D{n}"));
        }
    }
    names
}

/// As `latests`, Conduit called with `call`, for revisions `wanted` of one
/// Phabricator, in turn: the parents of all of them (`parents_over`), and
/// their latest diffs (`newest_diffs`), first; those parents named
/// (`parents_named`); then, for each modified since it was read, or never
/// read, its latest diff, raw, unless it is the one stored, and every
/// transaction of the revision, for its comments, `got` told it as soon as
/// it is read; for each other, what is stored of it, its parents as they
/// are now. A revision with no diff is passed over. Up to the
/// first call that fails, and what is wrong with it.
pub(crate) fn latests_over(call: &mut Call, wanted: &[&Wanted], got: &mut dyn FnMut(&str, Latest)) -> Result<(), String> {
    let phids: Vec<String> = wanted.iter().map(|w| w.phid.clone()).collect();
    let parents = parents_over(call, &phids)?;
    let diffs = newest_diffs(call, &phids)?;
    let names = parents_named(call, wanted, &parents);
    for w in wanted {
        let Some((diff, base)) = diffs.get(&w.phid).cloned() else { continue };
        let named = |p: &String| (p.clone(), names.get(p).cloned().unwrap_or_else(|| p.clone()));
        let parents = parents.get(&w.phid).into_iter().flatten().map(named).collect();
        // Not modified since it was read, its latest diff the same: as it
        // was read, but for its parents, which change from the other side
        // too, when another revision is edited to take it as its child.
        let unchanged = |h: &&Latest| h.diff == diff && h.modified.is_some() && h.modified == w.modified;
        if let Some(had) = w.had.as_ref().filter(unchanged) {
            got(&w.link, Latest { parents, base, ..had.clone() });
            continue;
        }
        let raw = match &w.had {
            Some(h) if h.diff == diff => h.raw.clone(),
            _ => call("differential.getrawdiff", json!({"diffID": diff}))?.as_str().unwrap_or_default().to_string(),
        };
        let txns = transactions(call, &w.d)?;
        let comments = comments_of(&txns, &w.link, &HashMap::new(), |_, _, _, _| None);
        got(&w.link, Latest { diff, base, raw, comments, parents, modified: w.modified });
    }
    Ok(())
}

/// Items `all`, each of the Phabricator its link (`link`) names, by
/// Phabricator, in the order each first comes.
fn by_host<T>(all: &[T], link: impl Fn(&T) -> &str) -> Vec<Vec<&T>> {
    let mut out: Vec<(String, Vec<&T>)> = Vec::new();
    for t in all {
        let host = link(t).split('/').take(3).collect::<Vec<_>>().join("/");
        match out.iter_mut().find(|(h, _)| *h == host) {
            Some((_, these)) => these.push(t),
            None => out.push((host, vec![t])),
        }
    }
    out.into_iter().map(|(_, these)| these).collect()
}

/// What answers Conduit's calls in place of Phabricator for the fetches of
/// a database (`statuses_stand_in`): the tests', which never reach it. Each
/// call, a method and its parameters; its `result`, or what went wrong, as
/// `Conduit::call` says it, Phabricator turning this machine away too.
pub type StandIn = Box<dyn FnMut(&str, Value) -> Result<Value, Error> + Send>;

/// A stand-in for Phabricator, shared with the read under way.
type StatusStandIn = std::sync::Arc<std::sync::Mutex<StandIn>>;

/// What answers the fetches of a database in place of Phabricator, by the
/// database's path: the tests', which never reach it. For that database
/// alone, not for the whole process as a submit's stand-in is: one for the
/// process would answer the editors of every test at once.
static STATUS_STAND_INS: std::sync::Mutex<Vec<(std::path::PathBuf, StatusStandIn)>> = std::sync::Mutex::new(Vec::new());

/// Has `p` answer the fetches of the database at `db` (`fetch_latest`)
/// rather than Phabricator; with none, Phabricator again.
pub fn statuses_stand_in(db: &Path, p: Option<StandIn>) {
    let mut all = STATUS_STAND_INS.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|(at, _)| at != db);
    if let Some(p) = p {
        all.push((db.to_path_buf(), std::sync::Arc::new(std::sync::Mutex::new(p))));
    }
}

/// The stand-in answering for the database at `db`, if one does.
fn status_stand_in(db: &Path) -> Option<StatusStandIn> {
    let all = STATUS_STAND_INS.lock().unwrap_or_else(|e| e.into_inner());
    all.iter().find(|(at, _)| at == db).map(|(_, p)| p.clone())
}

/// Whether the editors of the database at `db` read Phabricator, and show
/// what is stored of it: from a stand-in set for it, else from Phabricator,
/// but never from a process that keeps nothing, as the tests' own.
pub fn statuses_read(db: &Path) -> bool {
    status_stand_in(db).is_some() || crate::home::get().dir != Path::new(crate::home::NOWHERE)
}

/// What a process that keeps nothing, as the tests' own, says it reads of
/// Phabricator without a stand-in.
const NOT_REACHED: &str = "Phabricator is not reached from a process that keeps nothing, as the tests' own";

/// What `f` comes to, Conduit called with what it is given for the fetches
/// of the database at `db`, on the Phabricator of revision link `link`: the
/// stand-in set for that database, if one is; else Conduit, with the token
/// for that Phabricator, never from a process that keeps nothing. Should
/// `f` fail once a call was turned away, that is what went wrong, of its
/// own kind (`Error::TurnedAway`), whatever `f` makes of it.
fn calling<T>(db: &Path, link: &str, f: impl FnOnce(&mut Call) -> Result<T, String>) -> Result<T, Error> {
    let mut away: Option<Error> = None;
    let done = {
        // What went wrong, as `f` takes it; a call turned away, kept.
        let mut told = |e: Error| {
            if away.is_none() && e == Error::TurnedAway {
                away = Some(e.clone());
            }
            e.to_string()
        };
        match status_stand_in(db) {
            Some(p) => {
                let mut p = p.lock().unwrap_or_else(|e| e.into_inner());
                f(&mut |m, v| p(m, v).map_err(&mut told))
            }
            None if crate::home::get().dir == Path::new(crate::home::NOWHERE) => return Err(Error::Said(NOT_REACHED.into())),
            None => {
                let conduit = Conduit::for_url(Some(link))?;
                f(&mut |m, v| conduit.call(m, v).map_err(&mut told))
            }
        }
    };
    done.map_err(|e| away.unwrap_or(Error::Said(e)))
}

/// How a fetch reaches Phabricator: given a revision's link and what to do
/// there with the calls of Conduit, what that came to, the calls made on
/// the Phabricator the link names, as the fetches of a database make them
/// (`calling`), or as a submit makes its own (`fetch_with`).
type Reach<'a> = dyn FnMut(&str, &mut dyn FnMut(&mut Call) -> Result<(), String>) -> Result<(), Error> + 'a;

/// The statuses of revisions `revisions` (`D123`, and its link) on
/// Phabricator, by link, each with what else the search says of it: one
/// search for those of each Phabricator, as their links name it
/// (`statuses_over`). Through the stand-in set for the database at `db`, if
/// one is; else over Conduit, with the token for that Phabricator, never
/// from a process that keeps nothing. What is wrong, said, when a search
/// fails: Phabricator turning this machine away, as such.
pub fn statuses(db: &Path, revisions: &[(String, String)]) -> Result<HashMap<String, Searched>, Error> {
    if status_stand_in(db).is_none() && crate::home::get().dir == Path::new(crate::home::NOWHERE) {
        return Err(Error::Said(NOT_REACHED.into()));
    }
    statuses_by(&mut |link, f| calling(db, link, f), revisions)
}

/// As `statuses`, each Phabricator reached with `reach`.
fn statuses_by(reach: &mut Reach, revisions: &[(String, String)]) -> Result<HashMap<String, Searched>, Error> {
    // By Phabricator, as `pull` reads the comments.
    let mut out = HashMap::new();
    for these in by_host(revisions, |(_, link)| link.as_str()) {
        let ids: Vec<String> = these.iter().map(|(d, _)| d.clone()).collect();
        let mut found = HashMap::new();
        reach(&these[0].1, &mut |call| {
            found = statuses_over(call, &ids)?;
            Ok(())
        })?;
        for (d, link) in these {
            if let Some(s) = found.get(d) {
                out.insert(link.clone(), s.clone());
            }
        }
    }
    Ok(out)
}

/// What a fetch reads of revisions `wanted`, beyond their search: the
/// latest diff of each, raw, but when it is the one stored; its comments;
/// and its parents, named; of a revision not modified since it was read,
/// its parents alone (`latests_over`): `got` told each revision, by its
/// link, as soon as it is read, in the order given. The calls of each
/// Phabricator, as the revisions' links name it, one after another.
/// Through the stand-in set for the database at `db`, as the statuses are
/// read, else over Conduit; never from a process that keeps nothing. What
/// is wrong, said, when a call fails, Phabricator turning this machine away
/// as such: no call is made after it.
pub fn latests(db: &Path, wanted: &[Wanted], got: &mut dyn FnMut(&str, Latest)) -> Result<(), Error> {
    latests_by(&mut |link, f| calling(db, link, f), wanted, got)
}

/// As `latests`, each Phabricator reached with `reach`.
fn latests_by(reach: &mut Reach, wanted: &[Wanted], got: &mut dyn FnMut(&str, Latest)) -> Result<(), Error> {
    for these in by_host(wanted, |w| w.link.as_str()) {
        reach(&these[0].link, &mut |call| latests_over(call, &these, got))?;
    }
    Ok(())
}

// -- what is read of Phabricator, stored ----------------------------------- //

/// A comment of a revision as the database stores it (`db::Seen`): the id
/// of its version as it was last edited, those of its versions before, and
/// the comment it replies to, as the count of those not pulled yet reads
/// them (`review::not_pulled`).
pub fn seen(c: &Remote) -> Seen {
    Seen {
        id: c.phid.clone(),
        earlier: c.earlier.iter().map(|v| v.phid.clone()).collect(),
        reply_to: c.reply_to.clone(),
    }
}

/// A comment as the database stores it (`seen`), as a comment of
/// Phabricator's that the count of those not pulled yet reads: its ids and
/// the comment it replies to, nothing else.
pub fn remote_of(s: &Seen) -> Remote {
    Remote {
        phid: s.id.clone(),
        reply_to: s.reply_to.clone(),
        earlier: s.earlier.iter().map(|id| Version { phid: id.clone(), text: String::new() }).collect(),
        ..Default::default()
    }
}

/// The revisions the patches of stack `s` name, `D123` and its link
/// (`revision_of`): each once, by link.
pub fn named_by(db: &Db, s: &Stack) -> Vec<(String, String)> {
    let mut named: Vec<(String, String)> = s.patches.iter().filter_map(|id| revision_of(&db.patch(id).message)).collect();
    named.sort_by(|a, b| a.1.cmp(&b.1));
    named.dedup_by(|a, b| a.1 == b.1);
    named
}

/// What is stored of the revision at `link` in `db`, as a fetch takes it so
/// as to read again only what changed (`Wanted::had`): the id of its latest
/// diff, with that diff raw, when the diff is stored, else no id, so that
/// it is read again; its comments, and when the revision was last modified
/// as they were read; its parents. None for one never read, or that
/// Phabricator did not have.
fn had_of(db: &Db, link: &str) -> Option<Latest> {
    let (state, _) = db.revision_state(link).filter(|(s, _)| !s.missing)?;
    let raw = db.raw_diff(link, state.diff);
    let comments = db.revision_comments(link).map(|(c, _)| c);
    Some(Latest {
        diff: if raw.is_some() { state.diff } else { 0 },
        base: state.base.clone(),
        raw: raw.unwrap_or_default().to_string(),
        comments: comments.map_or_else(Vec::new, |c| c.list.iter().map(remote_of).collect()),
        parents: state.parents.clone(),
        modified: comments.and_then(|c| c.modified),
    })
}

/// When what is stored in `db` of the revision at `link` was fetched, in
/// seconds since 1970, as the Phabricator line and the review of a submit
/// say how long ago that was: of its state and its comments, of those
/// stored, the one read longest ago, each as of the read that stored it, or
/// of the latest fetch that found it as it was stored. None while its state
/// is not stored.
pub fn fetched_at(db: &Db, link: &str) -> Option<i64> {
    let (_, state) = db.revision_state(link)?;
    let fetched = db.fetched(link);
    let at = |t: i64| fetched.map_or(t, |f| f.max(t));
    let comments = db.revision_comments(link).map(|(_, t)| at(t));
    Some(comments.map_or(at(state), |c| c.min(at(state))))
}

/// The revisions of stack `s` a fetch reads beyond their search
/// (`Wanted`): those of `named` that the search found, `found` by link,
/// bottom first, each once; each with when it was last modified, as its
/// search says, what is stored of it (`had_of`), and whether the parents it
/// has are to be named, as a submit of a patch naming it would set it one,
/// whatever parents it has (`submit::parent_with`), the revision of the
/// patch below as the search found it, else as it is stored
/// (`Below::stored`).
fn wanted_of(db: &Db, s: &Stack, named: &[(String, String)], found: &HashMap<String, Searched>) -> Vec<Wanted> {
    let none: &[String] = &[];
    let below = |link: &str| -> Result<Below, ()> {
        if let Some(b) = found.get(link) {
            return Ok(Below::Found { phid: b.phid.clone(), open: b.status.open(), status: b.status.name().to_string() });
        }
        if named.iter().any(|(_, l)| l == link) {
            return Ok(Below::Missing);
        }
        Below::stored(db, link).ok_or(())
    };
    let mut out: Vec<Wanted> = Vec::new();
    for at in 0..s.patches.len() {
        let Some((d, link)) = revision_of(&db.patch(&s.patches[at]).message) else { continue };
        let Some(f) = found.get(&link).filter(|f| !f.phid.is_empty()) else { continue };
        let sets = crate::submit::parent_with(db, s, at, Some((d.as_str(), none)), |_, b| below(b));
        let sets = matches!(sets, Ok(Parent::Set { .. }));
        match out.iter_mut().find(|w| w.link == link) {
            Some(w) => w.name_parents |= sets,
            None => {
                let had = had_of(db, &link);
                out.push(Wanted { d, link, phid: f.phid.clone(), modified: f.modified, had, name_parents: sets });
            }
        }
    }
    out
}

/// Parents `parents`, each its PHID and its `D123`, or its PHID again where
/// the read that found them did not name it: named so, where what is
/// stored in `db` names it (`Db::revision_name`), as a PHID's revision
/// never changes; so that a parent once named stays named, whichever read
/// stores it next.
pub fn parents_known(db: &Db, parents: &[(String, String)]) -> Vec<(String, String)> {
    parents
        .iter()
        .map(|(phid, d)| match d == phid {
            true => (phid.clone(), db.revision_name(phid).unwrap_or(phid.as_str()).to_string()),
            false => (phid.clone(), d.clone()),
        })
        .collect()
}

/// Stores in `db` what a fetch read of revisions `named`: of each, its
/// state, as its search found it, `found` by link, with its parents and
/// its latest diff as `read` has them, or that Phabricator does not have
/// it; its comments, read again or as they were stored, and when the
/// revision was last modified as they were read; its latest diff, raw,
/// unless that is stored. Each only where it is not what is stored already
/// (`Db::store_revision`), as another read may have stored it meanwhile.
/// Then that the fetch read every one of them, now.
fn store(db: &mut Db, named: &[(String, String)], found: &HashMap<String, Searched>, read: &HashMap<String, Latest>) -> crate::db::Result<()> {
    for (d, link) in named {
        let Some(f) = found.get(link) else {
            db.store_revision(link, d, Some(State { missing: true, ..Default::default() }), None)?;
            continue;
        };
        let l = read.get(link);
        let state = State {
            missing: false,
            status: f.status.value().into(),
            title: f.title.clone(),
            summary: f.summary.clone(),
            bug: f.bug.clone(),
            phid: f.phid.clone(),
            parents: l.map_or_else(Vec::new, |l| parents_known(db, &l.parents)),
            diff: l.map_or(0, |l| l.diff),
            base: l.map_or(String::new(), |l| l.base.clone()),
            draft: Some(f.draft),
            repository: f.repository.clone(),
        };
        let comments = l.map(|l| Comments { modified: l.modified, list: l.comments.iter().map(seen).collect() });
        if let Some(l) = l {
            db.store_raw_diff(link, l.diff, &l.raw)?;
        }
        db.store_revision(link, d, Some(state), comments)?;
    }
    db.store_fetched(named.iter().map(|(_, link)| link.clone()).collect())
}

/// Fetch the latest from Phabricator: every revision the patches of stack
/// `name` in the database at `db` name, or those at the links `only` gives
/// alone; read in a few calls for all of them, their search, their parents
/// and their latest diffs (`statuses`, `latests`), then the comments and
/// the raw latest diff of each modified since it was stored, or never
/// stored; then stored, under the database's lock, only what differs from
/// what is stored (`store`), and when they were read. Through the stand-in
/// set for the database, else over Conduit; never from a process that keeps
/// nothing, nor while reviews come from nowhere. What it read, said; or,
/// nothing stored, what went wrong, Phabricator turning this machine away
/// as such.
pub fn fetch_latest(db: &Path, name: &str, only: Option<&[String]>) -> Result<String, Error> {
    fetch_by(db, name, only, &mut |link, f| calling(db, link, f))
}

/// As `fetch_latest`, the revisions at the links `only` gives alone, every
/// call made with `call`, as a submit makes its own, on the Phabricator of
/// the revision it sends to: what a submit fetches first of what the
/// database lacks for it (`submit::lacking`), so that every call of a
/// submit goes one way, its stand-in's in the tests.
pub(crate) fn fetch_with(db: &Path, name: &str, only: &[String], call: &mut Call) -> Result<String, Error> {
    fetch_by(db, name, Some(only), &mut |_, f| f(call).map_err(Error::Said))
}

/// As `fetch_latest`, each Phabricator reached with `reach`.
fn fetch_by(db: &Path, name: &str, only: Option<&[String]>, reach: &mut Reach) -> Result<String, Error> {
    if crate::config::get().reviews.source == crate::config::Source::None {
        return Err(Error::Said(format!("reviews come from nowhere: reviews.source is \"none\" in {}", crate::config::path().display())));
    }
    let d = Db::load(db).map_err(|e| Error::Said(e.0))?;
    let s = d.stack(name).map_err(|e| Error::Said(e.0))?.clone();
    let mut named = named_by(&d, &s);
    if let Some(only) = only {
        named.retain(|(_, link)| only.contains(link));
    }
    if named.is_empty() {
        return Err(Error::Said("no patch of the stack names a revision".into()));
    }
    let found = statuses_by(reach, &named)?;
    let wanted = wanted_of(&d, &s, &named, &found);
    let mut read: HashMap<String, Latest> = HashMap::new();
    latests_by(reach, &wanted, &mut |link, l| {
        read.insert(link.to_string(), l);
    })?;
    Db::edit(db, |d| store(d, &named, &found, &read)).map_err(|e| Error::Said(e.0))?;
    Ok(match named.as_slice() {
        [(d, _)] => format!("fetched the latest of {d} from Phabricator"),
        all => format!("fetched the latest of {} revisions from Phabricator", all.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_is_read_as_it_was_last_edited_what_it_was_kept_with_it() {
        let names: HashMap<String, String> = [("PHID-USER-ann".to_string(), "ann".to_string())].into();
        let version = |phid: &str, n: i64, at: i64, text: &str, removed: bool| {
            json!({"phid": phid, "version": n, "dateCreated": at, "removed": removed, "content": {"raw": text}})
        };
        let txns = vec![
            // On a line, never edited, and a reply to it.
            json!({"type": "inline", "authorPHID": "PHID-USER-bob", "fields": {"path": "a.txt", "line": 2, "diff": {"id": 7}}, "comments": [version("i1", 1, 150, " Why? ", false)]}),
            json!({"type": "inline", "authorPHID": "PHID-USER-ann", "fields": {"path": "a.txt", "line": 2, "diff": {"id": 7}, "replyToCommentPHID": "i1"}, "comments": [version("i2", 1, 160, "Because.", false)]}),
            // Edited twice, the last time after the reply: its versions,
            // given the latest first.
            json!({"type": "comment", "authorPHID": "PHID-USER-ann", "fields": {}, "comments": [
                version("v3", 3, 300, "Fine, but the title.\r\nAnd the date.", false),
                version("v2", 2, 200, "Fine, but the title.", false),
                version("v1", 1, 100, "Fine.", false),
            ]}),
            // Removed: every version's text hidden, the latest removed.
            json!({"type": "comment", "authorPHID": "PHID-USER-ann", "fields": {}, "comments": [version("r2", 2, 400, "", true), version("r1", 1, 120, "", false)]}),
            json!({"type": "accept", "authorPHID": "PHID-USER-ann", "fields": {}}),
        ];
        let got = comments_of(&txns, "https://p.example/D12", &names, |path, diff, _, line| Some(format!("{path}@{diff}:{line}")));
        let said: Vec<(&str, &str, &str, i64)> = got.iter().map(|c| (c.phid.as_str(), c.author.as_str(), c.text.as_str(), c.created)).collect();
        // By when each was first written, edited later or not.
        assert_eq!(said, [("v3", "ann", "Fine, but the title.\r\nAnd the date.", 100), ("i1", "someone", "Why?", 150), ("i2", "ann", "Because.", 160)]);
        let v = |phid: &str, text: &str| Version { phid: phid.into(), text: text.into() };
        assert_eq!(got[0].earlier, [v("v2", "Fine, but the title."), v("v1", "Fine.")]);
        assert!(got[1].earlier.is_empty());
        assert_eq!((got[1].path.as_deref(), got[1].line, got[1].line_text.as_deref()), (Some("a.txt"), 2, Some("a.txt@7:2")));
        assert_eq!((got[2].reply_to.as_deref(), got[0].path.as_deref()), (Some("i1"), None));
        // Its versions out of order, the latest is still the one read.
        let txns = vec![json!({"type": "comment", "authorPHID": "PHID-USER-ann", "fields": {}, "comments": [version("v1", 1, 100, "Fine.", false), version("v2", 2, 200, "Fine!", false)]})];
        let got = comments_of(&txns, "https://p.example/D12", &names, |_, _, _, _| None);
        assert_eq!((got[0].phid.as_str(), got[0].text.as_str(), got[0].earlier.clone()), ("v2", "Fine!", vec![v("v1", "Fine.")]));
    }

    #[test]
    fn where_phabricator_gives_no_suggested_edits_an_inline_comment_with_no_words_says_so_with_its_lines_and_its_link() {
        let names: HashMap<String, String> = [("PHID-USER-ann".to_string(), "ann".to_string())].into();
        let link = "https://p.example/D12";
        // A version as transaction.search gives it, its id with it.
        let version = |id: u64, phid: &str, n: i64, at: i64, text: &str, removed: bool| {
            json!({"id": id, "phid": phid, "version": n, "dateCreated": at, "removed": removed, "content": {"raw": text}})
        };
        // An inline comment of `author`'s on a.txt, from `line` on `length`
        // lines, as Conduit counts them, made on diff 7 unless it says, as a
        // Phabricator that gives no suggested edits gives it: saying nothing
        // of them, nor of the side it is on.
        let inline = |author: &str, line: u64, length: u64, diff: Option<u64>, reply: Option<&str>, versions: Vec<Value>| {
            let mut fields = json!({"path": "a.txt", "line": line, "length": length});
            if let Some(d) = diff {
                fields["diff"] = json!({"id": d});
            }
            if let Some(r) = reply {
                fields["replyToCommentPHID"] = r.into();
            }
            json!({"type": "inline", "authorPHID": author, "fields": fields, "comments": versions})
        };
        let ann = "PHID-USER-ann";
        let txns = vec![
            // ann's suggested edit alone, on four lines: no words, nor
            // removed.
            inline(ann, 2906, 4, Some(7), None, vec![version(501, "s1", 1, 100, "", false)]),
            // One on a line, answering it, of an author not known; one whose
            // diff is not said.
            inline("PHID-USER-bob", 2906, 1, Some(7), Some("s1"), vec![version(502, "s2", 1, 110, " \n", false)]),
            inline(ann, 40, 1, None, None, vec![version(503, "s3", 1, 120, "", false)]),
            // One with words: its words, as ever.
            inline(ann, 41, 2, Some(7), None, vec![version(508, "w1", 1, 122, " Use a constant. ", false)]),
            // Removed: none, every version's text hidden, the older one's
            // not removed itself.
            inline(ann, 30, 2, Some(7), None, vec![version(505, "r1v2", 2, 130, "", true), version(504, "r1", 1, 125, "", false)]),
            inline(ann, 31, 1, Some(7), None, vec![version(506, "r2", 1, 135, "", true)]),
            // On the whole revision, with no words: none, as before.
            json!({"type": "comment", "authorPHID": ann, "fields": {}, "comments": [version(507, "g1", 1, 140, "", false)]}),
        ];
        let got = comments_of(&txns, link, &names, |path, diff, side, line| Some(format!("{path}@{diff}:{side:?}:{line}")));
        let said: Vec<(&str, &str)> = got.iter().map(|c| (c.phid.as_str(), c.text.as_str())).collect();
        let sentence = |who: &str, lines: &str, at: &str| {
            format!("{who} suggested an edit to {lines} here, which stacked cannot read, as this Phabricator's API does not give suggested edits: see it on Phabricator, {at}")
        };
        assert_eq!(
            said,
            [
                ("s1", sentence("ann", "lines 2906–2909", "https://p.example/D12?id=7#inline-501").as_str()),
                ("s2", sentence("someone", "line 2906", "https://p.example/D12?id=7#inline-502").as_str()),
                ("s3", sentence("ann", "line 40", "https://p.example/D12#inline-503").as_str()),
                ("w1", "Use a constant."),
            ]
        );
        // On its lines, on the new side, as any comment is, a reply in its
        // thread.
        assert_eq!((got[0].path.as_deref(), got[0].line, got[0].line_text.as_deref(), got[0].created), (Some("a.txt"), 2906, Some("a.txt@7:New:2906"), 100));
        assert!(got.iter().all(|c| !c.old_side));
        assert_eq!((got[1].reply_to.as_deref(), got[0].reply_to.as_deref(), got[0].earlier.len()), (Some("s1"), None, 0));
        // Edited since to have words: a comment as any, as it was before
        // kept with it, by its one id, saying what it said then.
        let txns = vec![inline(ann, 2906, 4, Some(7), None, vec![version(601, "s1v2", 2, 200, "Use a constant.", false), version(501, "s1", 1, 100, "", false)])];
        let got = comments_of(&txns, link, &names, |_, _, _, _| None);
        assert_eq!((got[0].phid.as_str(), got[0].text.as_str(), got[0].created), ("s1v2", "Use a constant.", 100));
        let was = sentence("ann", "lines 2906–2909", "https://p.example/D12?id=7#inline-501");
        assert_eq!(got[0].earlier, [Version { phid: "s1".into(), text: was }]);
        // And removed later: none.
        let txns = vec![inline(ann, 2906, 4, Some(7), None, vec![version(602, "s1v3", 3, 300, "", true), version(601, "s1v2", 2, 200, "", false), version(501, "s1", 1, 100, "", false)])];
        assert!(comments_of(&txns, link, &names, |_, _, _, _| None).is_empty());
    }

    /// A diff as the tests of suggested edits have it: its id, the path of
    /// its file, and its hunks, each its offsets on the old side and on the
    /// new one, and its corpus.
    type Diff<'a> = (u64, &'a str, Vec<(u64, u64, String)>);

    /// Conduit as the tests of suggested edits have it: the transactions
    /// `txns` of the revision (transaction.search); each author's name, as
    /// their PHID ends (user.search); and diffs `diffs`, as
    /// differential.querydiffs gives them, the offsets written out. Another
    /// diff asked for is not in its answer, as Phabricator answers for a
    /// diff it does not have.
    fn conduit<'a>(txns: &'a [Value], diffs: &'a [Diff<'a>]) -> impl FnMut(&str, Value) -> Result<Value, String> + 'a {
        move |method: &str, p: Value| -> Result<Value, String> {
            Ok(match method {
                "transaction.search" => json!({"data": txns, "cursor": {"after": null}}),
                "user.search" => {
                    let users: Vec<Value> = p["constraints"]["phids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(|u| json!({"phid": u, "fields": {"username": u.trim_start_matches("PHID-USER-")}}))
                        .collect();
                    json!({"data": users})
                }
                "differential.querydiffs" => match diffs.iter().find(|(id, _, _)| p["ids"] == json!([id])) {
                    Some((id, path, hunks)) => {
                        let hunks: Vec<Value> = hunks.iter().map(|(old, new, corpus)| json!({"oldOffset": old.to_string(), "newOffset": new.to_string(), "corpus": corpus})).collect();
                        json!({ id.to_string(): {"id": id.to_string(), "changes": [{"oldPath": path, "currentPath": path, "hunks": hunks}]} })
                    }
                    None => json!({}),
                },
                m => return Err(format!("not as Phabricator is asked: {m}")),
            })
        }
    }

    /// An inline comment of `author`'s as Mozilla's Phabricator gives it:
    /// its fields `fields`, and its versions, the latest first, each its id,
    /// its PHID and its words, written when its id says, none removed.
    fn inline_of(author: &str, fields: Value, versions: &[(u64, &str, &str)]) -> Value {
        let n = versions.len();
        let versions: Vec<Value> = versions
            .iter()
            .enumerate()
            .map(|(k, (id, phid, raw))| json!({"id": id, "phid": phid, "version": n - k, "dateCreated": id, "removed": false, "content": {"raw": raw}}))
            .collect();
        json!({"type": "inline", "authorPHID": author, "fields": fields, "comments": versions})
    }

    #[test]
    fn a_suggested_edit_alone_is_read_as_a_diff_of_the_lines_it_replaces_after_what_a_stacked_of_before_said_of_it() {
        // D337641's: ochameau's inline comment on Console.cpp's lines 2906
        // to 2909 of diff 1422734, on its new side, a suggested edit alone
        // of three lines, the last ending with no newline.
        let path = "dom/console/Console.cpp";
        let was = [
            "  // MOZ_LOG already logs the module name, so only the prefix is added.",
            "  const nsString& label = aIsForMozLog ? mPrefix : mLogModuleAndPrefix;",
            "  nsString message = (aIsForMozLog ? u\"\"_ns : u\"console.\"_ns) + aMethodString +",
            "                     u\": \"_ns + label + (label.IsEmpty() ? u\"\"_ns : u\": \"_ns);",
        ];
        let is = [
            "  const nsString& prefix = aIsForMozLog ? mMozLogPrefix : mConsolePrefix;",
            "  nsString message = (aIsForMozLog ? u\"\"_ns : u\"console.\"_ns) + aMethodString +",
            "                     u\": \"_ns + prefix + (prefix.IsEmpty() ? u\"\"_ns : u\": \"_ns);",
        ];
        // The diff's hunk there, its new side from 2903 on: three lines of
        // context, the empty one written as a space, the four it adds, then
        // one more.
        let added: Vec<String> = was.iter().map(|l| format!("+{l}")).collect();
        let corpus = format!(" void Console::Dump() {{\n   bool dumped = false;\n \n{}\n }}\n", added.join("\n"));
        let diffs = [(1422734, path, vec![(2901, 2903, corpus)])];
        let fields = json!({"path": path, "line": 2906, "length": 4, "diff": {"id": 1422734}, "isNewFile": true, "hasSuggestion": true, "suggestionText": is.join("\n")});
        let txns = [inline_of("PHID-USER-ochameau", fields, &[(1234567, "PHID-XCMT-a", "")])];
        let link = "https://phabricator.services.mozilla.com/D337641";
        let got = comments_over(&mut conduit(&txns, &diffs), "D337641", link).unwrap();
        // Who suggests it and where, then the diff, fenced: each line it
        // replaces, then each of its own, as they are.
        let mut want = vec!["ochameau suggests this edit of lines 2906–2909:".to_string(), "```diff".into()];
        want.extend(was.iter().map(|l| format!("-{l}")));
        want.extend(is.iter().map(|l| format!("+{l}")));
        want.push("```".into());
        assert_eq!(got[0].text, want.join("\n"));
        // A version of its own, the one it is read from before it, saying
        // what a stacked of before, which read no suggested edit, filed of
        // it: so that a thread saying that is held as an edit of it.
        assert_eq!(got[0].phid, "PHID-XCMT-a+suggestion");
        let before = format!("ochameau suggested an edit to lines 2906–2909 here, which stacked cannot read, as Phabricator's API does not give suggested edits: see it on Phabricator, {link}?id=1422734#inline-1234567");
        assert_eq!(got[0].earlier, [Version { phid: "PHID-XCMT-a".into(), text: before }]);
        // On its first line, by its text on the new side.
        assert_eq!((got[0].line, got[0].old_side, got[0].line_text.as_deref()), (2906, false, Some(was[0])));
    }

    #[test]
    fn a_suggested_edit_with_words_follows_them_its_lines_as_they_are() {
        // a.txt's diff 7: three and four made THREE and FOUR.
        let diffs = [(7, "a.txt", vec![(1, 1, " one\n two\n-three\n-four\n+THREE\n+FOUR\n five\n".to_string())])];
        // ann's words, white space around them, then her edit of lines 3 and
        // 4, its first line indented, and white space ending it.
        let fields = json!({"path": "a.txt", "line": 3, "length": 2, "diff": {"id": 7}, "isNewFile": true, "hasSuggestion": true, "suggestionText": "    three  \nfour\n"});
        let txns = [inline_of("PHID-USER-ann", fields.clone(), &[(501, "s1", "\n  Lower case, please.  \n")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        // Its words trimmed, then the edit, its lines as they are.
        assert_eq!(got[0].text, "Lower case, please.\n\nann suggests this edit of lines 3–4:\n```diff\n-THREE\n-FOUR\n+    three  \n+four\n```");
        // Before it, as read without it: its words alone.
        let v = |phid: &str, text: &str| Version { phid: phid.into(), text: text.into() };
        assert_eq!((got[0].phid.as_str(), got[0].earlier.clone()), ("s1+suggestion", vec![v("s1", "Lower case, please.")]));
        // Edited on Phabricator since: the version before known by both its
        // ids, as either may be the one filed, saying its words, as
        // Phabricator gives no suggested edit of it.
        let txns = [inline_of("PHID-USER-ann", fields, &[(601, "s1v2", "Lower case, please."), (501, "s1", "Shout less.")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        assert_eq!(got[0].phid, "s1v2+suggestion");
        assert_eq!(got[0].earlier, [v("s1v2", "Lower case, please."), v("s1+suggestion", "Shout less."), v("s1", "Shout less.")]);
    }

    #[test]
    fn a_suggested_edit_of_no_lines_deletes_those_it_is_on() {
        let diffs = [(7, "a.txt", vec![(1, 1, " one\n two\n-three\n-four\n+THREE\n+FOUR\n five\n".to_string())])];
        // Its text empty, Phabricator saying it has one: it deletes lines 3
        // and 4 of diff `diff`.
        let on = |diff: u64| json!({"path": "a.txt", "line": 3, "length": 2, "diff": {"id": diff}, "isNewFile": true, "hasSuggestion": true, "suggestionText": ""});
        let txns = [inline_of("PHID-USER-ann", on(7), &[(501, "s1", "")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        assert_eq!(got[0].text, "ann suggests deleting lines 3–4:\n```diff\n-THREE\n-FOUR\n```");
        // Its diff not in Phabricator's answer: said so, no line guessed.
        let txns = [inline_of("PHID-USER-ann", on(8), &[(501, "s1", "")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        assert_eq!(got[0].text, "ann suggests deleting lines 3–4, but those lines could not be read from Phabricator.");
    }

    #[test]
    fn a_newline_ending_a_suggested_edit_ends_its_last_line_rather_than_adding_one() {
        // Console.cpp's diff 1422734, its lines from 150 and from 915 on,
        // each saying its number.
        let path = "dom/console/Console.cpp";
        let lines = |from: u64, to: u64| (from..=to).map(|n| format!(" // line {n}\n")).collect::<String>();
        let diffs = [(1422734, path, vec![(150, 150, lines(150, 160)), (915, 915, lines(915, 930))])];
        // ochameau's suggested edit of the `length` lines from `line`.
        let edit = |line: u64, length: u64, text: &str| {
            let fields = json!({"path": path, "line": line, "length": length, "diff": {"id": 1422734}, "isNewFile": true, "hasSuggestion": true, "suggestionText": text});
            let txns = [inline_of("PHID-USER-ochameau", fields, &[(1, "c1", "")])];
            comments_over(&mut conduit(&txns, &diffs), "D337641", "https://phabricator.services.mozilla.com/D337641").unwrap().remove(0).text
        };
        // As on D337641: one line in the place of lines 155 and 156, its
        // newline ending it; six in the place of lines 919 to 925, with none.
        assert_eq!(
            edit(155, 2, "  // It is printed after the log module in stdout and in the console events.\n"),
            "ochameau suggests this edit of lines 155–156:\n```diff\n-// line 155\n-// line 156\n+  // It is printed after the log module in stdout and in the console events.\n```"
        );
        let six = ["  if (aIsForMozLog) {", "    MOZ_LOG(mLogModule, LogLevel::Debug, (\"%s\", message.get()));", "    return;", "  }", "", "  printf_stderr(\"%s\", message.get());"];
        let mut want = vec!["ochameau suggests this edit of lines 919–925:".to_string(), "```diff".into()];
        want.extend((919..=925).map(|n| format!("-// line {n}")));
        want.extend(six.iter().map(|l| format!("+{l}")));
        want.push("```".into());
        assert_eq!(edit(919, 7, &six.join("\n")), want.join("\n"));
        // A line left empty last, before the newline that ends it, is a
        // line of the edit.
        assert_eq!(edit(157, 1, "  x\n\n"), "ochameau suggests this edit of line 157:\n```diff\n-// line 157\n+  x\n+\n```");
    }

    #[test]
    fn a_suggested_edit_on_the_old_side_reads_the_lines_there_and_sits_on_those_the_patch_removes() {
        // a.txt's diff 7: two and three made TWO and THREE.
        let diffs = [(7, "a.txt", vec![(1, 1, " one\n-two\n-three\n+TWO\n+THREE\n four\n".to_string())])];
        // ann's suggested edit of lines 2 and 3, on the side `new` says.
        let fields = |new: bool| json!({"path": "a.txt", "line": 2, "length": 2, "diff": {"id": 7}, "isNewFile": new, "hasSuggestion": true, "suggestionText": "Two\nThree\n"});
        let txns = [inline_of("PHID-USER-ann", fields(false), &[(501, "s1", "")]), inline_of("PHID-USER-ann", fields(true), &[(502, "s2", "")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        // On the old side, the lines there and its text there, said to be of
        // the file before the patch; on the new side, those there.
        assert_eq!(got[0].text, "ann suggests this edit of lines 2–3 of the file before the patch:\n```diff\n-two\n-three\n+Two\n+Three\n```");
        assert_eq!((got[0].old_side, got[0].line_text.as_deref()), (true, Some("two")));
        assert_eq!(got[1].text, "ann suggests this edit of lines 2–3:\n```diff\n-TWO\n-THREE\n+Two\n+Three\n```");
        assert_eq!((got[1].old_side, got[1].line_text.as_deref()), (false, Some("TWO")));
        // On the patch, one on the old side sits on the line the patch
        // removes, by its text there or by its number there; one on the new
        // side on the line the patch adds; a line of context is on both.
        let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,4 +1,4 @@\n one\n-two\n-three\n+TWO\n+THREE\n four\n";
        let p = crate::db::Patch::new("c".into(), None, None, "Bug 1 - shout".into(), diff.into());
        let lines = crate::view::note_lines(&p);
        let key = |origin: char, text: &str| lines.iter().find(|l| l.origin == origin && l.text == text).map(|l| l.key.clone());
        assert_eq!((anchor(&lines, &got[0]), anchor(&lines, &got[1])), (key('-', "two"), key('+', "TWO")));
        let on = |line: usize, old_side: bool, text: Option<&str>| {
            let c = Remote { path: Some("a.txt".into()), line, old_side, line_text: text.map(String::from), ..Default::default() };
            anchor(&lines, &c)
        };
        assert_eq!((on(3, true, Some("gone since")), on(3, false, None)), (key('-', "three"), key('+', "THREE")));
        assert_eq!((on(4, true, None), on(4, false, None)), (key(' ', "four"), key(' ', "four")));
    }

    #[test]
    fn a_removed_comment_is_none_whatever_it_suggested_and_one_said_to_suggest_nothing_says_its_words() {
        // ann's inline comment on a.txt's line 3 of diff 7, as Mozilla's
        // Phabricator gives it, saying whether it has a suggested edit, and
        // its versions, the latest first, each its id, PHID, words, and
        // whether it is removed.
        let inline = |has: bool, versions: &[(u64, &str, &str, bool)]| {
            let text = if has { json!("Three") } else { Value::Null };
            let n = versions.len();
            let versions: Vec<Value> = versions
                .iter()
                .enumerate()
                .map(|(k, (id, phid, raw, removed))| json!({"id": id, "phid": phid, "version": n - k, "dateCreated": id, "removed": removed, "content": {"raw": raw}}))
                .collect();
            let fields = json!({"path": "a.txt", "line": 3, "length": 1, "diff": {"id": 7}, "isNewFile": true, "hasSuggestion": has, "suggestionText": text});
            json!({"type": "inline", "authorPHID": "PHID-USER-ann", "fields": fields, "comments": versions})
        };
        let names: HashMap<String, String> = [("PHID-USER-ann".to_string(), "ann".to_string())].into();
        let read = |t: Value| comments_of(&[t], "https://p.example/D12", &names, |_, _, _, _| Some("THREE".into()));
        // Removed: none, said to have no suggested edit, as Mozilla's
        // Phabricator says it then, or to have one all the same.
        for has in [false, true] {
            assert!(read(inline(has, &[(502, "s1v2", "", true), (501, "s1", "", false)])).is_empty(), "{has}");
        }
        // Said to have none: its words alone, by its own id; with none, it
        // says nothing, and is none.
        let got = read(inline(false, &[(501, "s1", "Why shout?", false)]));
        assert_eq!((got[0].phid.as_str(), got[0].text.as_str(), got[0].earlier.len()), ("s1", "Why shout?", 0));
        assert!(read(inline(false, &[(501, "s1", " \n", false)])).is_empty());
        // Edited to have words, the version before with none: known by both
        // its ids, saying what a stacked of before filed of it.
        let got = read(inline(false, &[(601, "s1v2", "Why shout?", false), (501, "s1", "", false)]));
        let was = "ann suggested an edit to line 3 here, which stacked cannot read, as Phabricator's API does not give suggested edits: see it on Phabricator, https://p.example/D12?id=7#inline-501";
        let v = |phid: &str| Version { phid: phid.into(), text: was.into() };
        assert_eq!((got[0].phid.as_str(), got[0].earlier.clone()), ("s1v2", vec![v("s1+suggestion"), v("s1")]));
    }

    #[test]
    fn a_suggested_edit_whose_lines_cannot_all_be_read_shows_its_own_alone_saying_so() {
        // a.txt's diff 7, its hunk to line 3 alone.
        let diffs = [(7, "a.txt", vec![(1, 1, " one\n two\n-three\n+THREE\n".to_string())])];
        let on = |diff: u64| json!({"path": "a.txt", "line": 3, "length": 2, "diff": {"id": diff}, "isNewFile": true, "hasSuggestion": true, "suggestionText": "three\nfour\n"});
        let said = "ann suggests this edit of lines 3–4, but the lines replaced could not be read from Phabricator, so only the new lines are shown:\n```diff\n+three\n+four\n```";
        // Line 4 not in its diff: none of the lines it replaces shown, not
        // even the one read.
        let txns = [inline_of("PHID-USER-ann", on(7), &[(501, "s1", "")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        assert_eq!((got[0].text.as_str(), got[0].line_text.as_deref()), (said, Some("THREE")));
        // Its diff not in Phabricator's answer at all.
        let txns = [inline_of("PHID-USER-ann", on(8), &[(501, "s1", "")])];
        let got = comments_over(&mut conduit(&txns, &diffs), "D12", "https://p.example/D12").unwrap();
        assert_eq!((got[0].text.as_str(), got[0].line_text.as_deref()), (said, None));
    }

    #[test]
    fn a_read_of_a_diff_s_lines_that_fails_fails_the_read_saying_why() {
        // ann's suggested edit of a.txt's line 3, on diff 7.
        let diffs = [(7, "a.txt", vec![(1, 1, " one\n two\n-three\n+THREE\n".to_string())])];
        let fields = json!({"path": "a.txt", "line": 3, "length": 1, "diff": {"id": 7}, "isNewFile": true, "hasSuggestion": true, "suggestionText": "Three\n"});
        let txns = [inline_of("PHID-USER-ann", fields, &[(501, "s1", "")])];
        // The diff's lines refused: no comment read without them, the read
        // failing, saying why.
        let mut inner = conduit(&txns, &diffs);
        let mut refused = |m: &str, p: Value| match m {
            "differential.querydiffs" => Err(format!("{m}: Phabricator is down")),
            _ => inner(m, p),
        };
        assert_eq!(comments_over(&mut refused, "D12", "https://p.example/D12"), Err("differential.querydiffs: Phabricator is down".to_string()));
        // So a pull's read fails, Phabricator turning this machine away
        // there said as such, as at any of its calls.
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("db.jsonl");
        let (mut rest, inline) = (pulled_from(&[]), txns[0].clone());
        statuses_stand_in(
            &db,
            Some(Box::new(move |m, p: Value| {
                if m == "transaction.search" && p["objectIdentifier"] == "D12" {
                    return Ok(json!({"data": [inline.clone()], "cursor": {"after": null}}));
                }
                if m == "differential.querydiffs" && p["ids"].is_array() {
                    return Err(Error::TurnedAway);
                }
                rest(m, p).map_err(Error::Said)
            })),
        );
        let wants = Wants { asked: vec![("D12".into(), "https://p.example/D12".into())], held: vec!["https://p.example/D11".into()], up: None };
        assert_eq!(read_pull(&db, &wants, &|_, _| None, &|_, _| None, &mut |_, _, _| {}), Err(Error::TurnedAway));
        statuses_stand_in(&db, None);
    }

    /// Phabricator as the tests of a pull's reading have it: revisions by
    /// number, each its status, its parents and its diffs (their ids, the
    /// latest last); D11's latest diff with its commit as uploaded; ann's
    /// comment on D12. `extra` parents, by revision, to make more than one.
    fn pulled_from(extra: &[(u64, u64)]) -> impl FnMut(&str, Value) -> Result<Value, String> + use<'_> {
        let status = |n: u64| if n == 10 { "published" } else { "needs-review" };
        let parents = move |n: u64| -> Vec<u64> {
            let mut out: Vec<u64> = if (11..=14).contains(&n) { vec![n - 1] } else { vec![] };
            out.extend(extra.iter().filter(|(of, _)| *of == n).map(|(_, p)| *p));
            out
        };
        let num = |v: &Value| v.as_str().map_or(v.as_u64(), |s| s.trim_start_matches("PHID-DREV-").trim_start_matches('D').parse().ok());
        move |method: &str, p: Value| -> Result<Value, String> {
            let rev = |n: u64| json!({"id": n, "phid": format!("PHID-DREV-{n}"), "fields": {"title": format!("Bug {n}"), "summary": "Why.", "status": {"value": status(n)}}});
            Ok(match method {
                "differential.revision.search" => {
                    let c = &p["constraints"];
                    let ns: Vec<u64> = c["ids"].as_array().or(c["phids"].as_array()).into_iter().flatten().filter_map(num).filter(|n| (9..=20).contains(n)).collect();
                    json!({"data": ns.into_iter().map(rev).collect::<Vec<_>>()})
                }
                "edge.search" => {
                    let from: Vec<u64> = p["sourcePHIDs"].as_array().into_iter().flatten().filter_map(num).collect();
                    let up = p["types"][0] == "revision.child";
                    let edges: Vec<Value> = from
                        .iter()
                        .flat_map(|&n| {
                            let to: Vec<u64> = match up {
                                true => (9..=20).filter(|c| parents(*c).contains(&n)).collect(),
                                false => parents(n),
                            };
                            to.into_iter().map(move |t| json!({"sourcePHID": format!("PHID-DREV-{n}"), "destinationPHID": format!("PHID-DREV-{t}")}))
                        })
                        .collect();
                    json!({"data": edges, "cursor": {"after": null}})
                }
                // D11's latest diff as uploaded; D12's two diffs.
                "differential.querydiffs" => match num(&p["revisionIDs"][0]) {
                    Some(11) => json!({"111": {"sourceControlBaseRevision": "bbb", "properties": {"local:commits": {"c1": {
                        "author": "Ann", "authorEmail": "ann@example.com", "message": "Bug 11 - as committed"
                    }}}}}),
                    Some(12) => json!({"121": {}, "122": {"sourceControlBaseRevision": "ccc"}}),
                    Some(n) => json!({ (n * 10 + 1).to_string(): {} }),
                    None => json!({}),
                },
                "differential.getrawdiff" => json!(format!("raw {}", p["diffID"])),
                "transaction.search" => match p["objectIdentifier"].as_str() {
                    Some("D12") => json!({"data": [{"type": "comment", "authorPHID": "PHID-USER-ann", "fields": {}, "comments": [{"phid": "c1", "version": 1, "content": {"raw": "Why?"}}]}]}),
                    _ => json!({"data": []}),
                },
                "user.search" => json!({"data": [{"phid": "PHID-USER-ann", "fields": {"username": "ann"}}]}),
                m => return Err(format!("not as Phabricator is asked: {m}")),
            })
        }
    }

    #[test]
    fn a_pull_reads_the_revisions_asked_down_their_open_parents_and_up_through_those_stacked_on_them() {
        // D10 landed; D11 on it, D12 on D11, D13 on D12, D14 on D13. D12
        // asked, with those stacked on it: D11 below it, then D13 and D14.
        let link = |n: u64| format!("https://p.example/D{n}");
        let wants = Wants { asked: vec![("D12".into(), link(12))], held: vec![], up: Some(link(12)) };
        // D12's latest diff no version of its patch stacked knows, the one
        // before it one.
        let known = |l: &str, raw: &str| (l == link(12)).then(|| raw == "raw 121");
        let mut told = Vec::new();
        let mut call = pulled_from(&[]);
        let read = pull_over(&mut call, &wants, &known, &|_, _| None, &mut |d, at, of| told.push(format!("{d} {at}/{of}"))).unwrap();
        let said: Vec<(&str, &Reached)> = read.revisions.iter().map(|r| (r.d.as_str(), &r.reached)).collect();
        assert_eq!(
            said,
            [("D12", &Reached::Asked), ("D11", &Reached::Parent("D12".into())), ("D13", &Reached::Child("D12".into())), ("D14", &Reached::Child("D13".into()))]
        );
        assert_eq!(told, ["D12 1/4", "D11 2/4", "D13 3/4", "D14 4/4"]);
        // Each its parents, named; its latest diff, raw, and the commit it
        // is on; as uploaded, its commit's author and message.
        let d11 = &read.revisions[1];
        assert_eq!((d11.parents.clone(), d11.diff, d11.base.as_deref()), (vec![("PHID-DREV-10".to_string(), "D10".to_string())], 111, Some("bbb")));
        assert_eq!((d11.author.as_deref(), d11.message.as_deref(), d11.status.as_str()), (Some("Ann <ann@example.com>"), Some("Bug 11 - as committed"), "needs-review"));
        let d12 = &read.revisions[0];
        assert_eq!((d12.diff, d12.older.clone(), d12.link.as_str(), d12.title.as_str()), (122, vec![121], "https://p.example/D12", "Bug 12"));
        assert_eq!(d12.comments.iter().map(|c| (c.author.as_str(), c.text.as_str())).collect::<Vec<_>>(), [("ann", "Why?")]);
        assert_eq!(read.raws.iter().map(|(l, id, _)| (l.as_str(), *id)).collect::<Vec<_>>()[..2], [("https://p.example/D12", 122), ("https://p.example/D12", 121)]);
        // The stack holding D11: not read, the walk down stops there; a raw
        // diff stored, not read again; only the asked, with nothing above.
        let wants = Wants { asked: vec![("D12".into(), link(12))], held: vec![link(11)], up: None };
        let stored = |l: &str, id: u64| (l == link(12) && id == 122).then(|| "raw 122".to_string());
        let mut call = pulled_from(&[]);
        let read = pull_over(&mut call, &wants, &|_, _| None, &stored, &mut |_, _, _| {}).unwrap();
        assert_eq!((read.revisions.len(), read.raws.len(), read.revisions[0].older.len()), (1, 0, 0));
        // One Phabricator does not have: said apart, nothing read of it.
        let wants = Wants { asked: vec![("D99".into(), link(99)), ("D12".into(), link(12))], held: vec![link(11)], up: None };
        let mut call = pulled_from(&[]);
        let read = pull_over(&mut call, &wants, &|_, _| None, &|_, _| None, &mut |_, _, _| {}).unwrap();
        assert_eq!((read.missing.clone(), read.revisions.len()), (vec![("D99".to_string(), link(99))], 1));
    }

    #[test]
    fn more_than_one_open_parent_or_one_stacked_on_where_the_walk_up_goes_reads_nothing_more_saying_which() {
        let link = |n: u64| format!("https://p.example/D{n}");
        // D12 on D11 and on D20, both open.
        let wants = Wants { asked: vec![("D12".into(), link(12))], held: vec![], up: None };
        let mut call = pulled_from(&[(12, 20)]);
        let e = pull_over(&mut call, &wants, &|_, _| None, &|_, _| None, &mut |_, _, _| panic!("nothing read")).unwrap_err();
        assert_eq!(e, "D12 is stacked on more than one open revision on Phabricator, D11 and D20: a stack is one line of patches, so nothing was pulled");
        // D13 and D20 both on D12: refused going up, not without it.
        let wants = Wants { asked: vec![("D12".into(), link(12))], held: vec![link(11)], up: Some(link(12)) };
        let mut call = pulled_from(&[(20, 12)]);
        let e = pull_over(&mut call, &wants, &|_, _| None, &|_, _| None, &mut |_, _, _| panic!("nothing read")).unwrap_err();
        assert_eq!(e, "More than one open revision is stacked on D12 on Phabricator, D13 and D20: a stack is one line of patches, so nothing was pulled");
        let wants = Wants { up: None, ..wants };
        let mut call = pulled_from(&[(20, 12)]);
        assert_eq!(pull_over(&mut call, &wants, &|_, _| None, &|_, _| None, &mut |_, _, _| {}).unwrap().revisions.len(), 1);
        // D10, landed, under D11 with D9: one open parent, read.
        let wants = Wants { asked: vec![("D11".into(), link(11))], held: vec![], up: None };
        let mut call = pulled_from(&[(11, 9)]);
        let read = pull_over(&mut call, &wants, &|_, _| None, &|_, _| None, &mut |_, _, _| {}).unwrap();
        assert_eq!(read.revisions.iter().map(|r| r.d.as_str()).collect::<Vec<_>>(), ["D11", "D9"]);
    }

    #[test]
    fn a_revision_is_named_by_its_number_or_its_link() {
        assert_eq!(revision_named(" D123 "), Some(("D123".into(), None)));
        assert_eq!(revision_named("123"), Some(("D123".into(), None)), "a number alone");
        let link = "https://phabricator.example.com/D123";
        assert_eq!(revision_named(&format!("{link}/")), Some(("D123".into(), Some(link.into()))));
        for wrong in ["", "D", "12x", "D12x", "https://phabricator.example.com/", "https://phabricator.example.com/123"] {
            assert_eq!(revision_named(wrong), None, "{wrong:?}");
        }
        // As the browser gives it, at an inline comment, or asked for a diff.
        for given in [format!("{link}#inline-456"), format!("{link}?id=789"), format!("{link}/#change-x")] {
            assert_eq!(revision_named(&given), Some(("D123".into(), Some(link.into()))), "{given:?}");
        }
    }

    #[test]
    fn a_patchs_revision_is_its_last_trailer_the_message_naming_others() {
        let msg = "Bug 2 - back out\n\nThis backs out the change of\nDifferential Revision: https://p.example/D100\nwhich broke the build.\n\nDifferential Revision: https://p.example/D200";
        assert_eq!(revision_in(msg, "Differential Revision", None), Some(("D200".into(), "https://p.example/D200".into())));
        let bare = "Bug 3 - x\n\nDifferential Revision: D7";
        assert_eq!(revision_in(bare, "Differential Revision", Some("https://p.example")), Some(("D7".into(), "https://p.example/D7".into())));
        assert_eq!(revision_in(bare, "Differential Revision", None), None, "no Phabricator to name it on");
        assert_eq!(revision_in("Bug 4 - y", "Differential Revision", None), None);
    }

    #[test]
    fn each_revision_is_told_as_it_starts_being_read_up_to_a_call_that_fails() {
        let link = |n: u64| format!("https://p.example/D{n}");
        let wants = Wants { asked: vec![("D12".into(), link(12)), ("D13".into(), link(13))], held: vec![link(11)], up: None };
        let mut inner = pulled_from(&[]);
        // D12's comments refused: D13 never read.
        let mut call = |m: &str, p: Value| -> Result<Value, String> {
            if m == "transaction.search" && p["objectIdentifier"] == "D12" {
                return Err("transaction.search: D12 is gone".into());
            }
            inner(m, p)
        };
        let mut told = Vec::new();
        let read = pull_over(&mut call, &wants, &|_, _| None, &|_, _| None, &mut |d, _, _| told.push(d.to_string()));
        assert_eq!((told, read), (vec!["D12".to_string()], Err("transaction.search: D12 is gone".to_string())));
    }

    /// A revision as differential.revision.search gives it: its number, its
    /// status's value, if it has one, and whether Phabricator says it is a
    /// draft (`isDraft`).
    fn revision(n: u64, status: Option<&str>, draft: bool) -> Value {
        let mut fields = json!({"isDraft": draft});
        if let Some(s) = status {
            fields["status"] = json!({"value": s});
        }
        json!({"id": n, "phid": format!("PHID-DREV-{n}"), "fields": fields})
    }

    #[test]
    fn a_revision_s_status_is_its_value_whatever_phabricator_says_of_drafts() {
        let all = [
            ("draft", Status::Draft, "[D]", "Draft"),
            ("changes-planned", Status::ChangesPlanned, "[W]", "Changes Planned"),
            ("needs-review", Status::NeedsReview, "[R]", "Needs Review"),
            ("needs-revision", Status::NeedsRevision, "[C]", "Needs Revision"),
            ("accepted", Status::Accepted, "[A]", "Accepted"),
            ("published", Status::Published, "[L]", "Landed"),
            ("abandoned", Status::Abandoned, "[X]", "Abandoned"),
            ("something-new", Status::Unknown, "[?]", "status unknown"),
        ];
        for (value, status, letter, name) in all {
            assert_eq!(Status::of(&revision(1, Some(value), false)), status, "{value}");
            assert_eq!((status.letter(), status.name()), (letter, name), "{value}");
        }
        // Never sent for review: Phabricator says each is a draft, Changes
        // Planned as a submit in work leaves it, and abandoned so. Its
        // status is its value all the same.
        assert_eq!(Status::of(&revision(1, Some("changes-planned"), true)), Status::ChangesPlanned);
        assert_eq!(Status::of(&revision(1, Some("abandoned"), true)), Status::Abandoned);
        assert_eq!(Status::of(&revision(1, Some("draft"), true)), Status::Draft);
        assert_eq!(Status::of(&revision(1, None, true)), Status::Unknown, "no value");
    }

    #[test]
    fn the_statuses_of_revisions_are_read_in_one_search_those_it_finds_by_revision() {
        let mut asked = Vec::new();
        let mut call = |method: &str, params: Value| -> Result<Value, String> {
            asked.push((method.to_string(), params.clone()));
            let mut d12 = revision(12, Some("accepted"), false);
            d12["fields"]["title"] = json!("Bug 12 - twelve");
            d12["fields"]["summary"] = json!("Why.");
            d12["fields"]["dateModified"] = json!(1_700_000_012);
            d12["fields"]["repositoryPHID"] = json!("PHID-REPO-1");
            Ok(json!({"data": [d12, revision(11, Some("changes-planned"), true)]}))
        };
        let ids = ["D11", "D12", "D13"].map(String::from);
        let found = statuses_over(&mut call, &ids).unwrap();
        assert_eq!(asked, [("differential.revision.search".to_string(), json!({"constraints": {"ids": [11, 12, 13]}}))]);
        // D13, not given: not known. Each with its title, summary and PHID,
        // when it was last modified, none when the search gives none, whether
        // it is a draft, and its repository, none when the search gives none.
        let searched = |status: Status, title: &str, summary: &str, n: u64, modified: Option<i64>, draft: bool, repository: &str| Searched {
            status,
            title: title.into(),
            summary: summary.into(),
            bug: String::new(),
            phid: format!("PHID-DREV-{n}"),
            modified,
            draft,
            repository: repository.into(),
        };
        let want: HashMap<String, Searched> = [
            ("D11".to_string(), searched(Status::ChangesPlanned, "", "", 11, None, true, "")),
            ("D12".to_string(), searched(Status::Accepted, "Bug 12 - twelve", "Why.", 12, Some(1_700_000_012), false, "PHID-REPO-1")),
        ]
        .into();
        assert_eq!(found, want);
        // No revision, no search.
        let mut none = |_: &str, _: Value| -> Result<Value, String> { panic!("nothing to search") };
        assert_eq!(statuses_over(&mut none, &[]), Ok(HashMap::new()));
        // A search refused: what is wrong, said.
        let mut refused = |m: &str, _: Value| -> Result<Value, String> { Err(format!("{m}: refused")) };
        assert_eq!(statuses_over(&mut refused, &ids), Err("differential.revision.search: refused".to_string()));
    }

    #[test]
    fn the_statuses_of_an_editor_s_revisions_are_read_by_its_database_s_stand_in_one_search_for_each_phabricator() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, other) = (tmp.path().join("db.jsonl"), tmp.path().join("other.jsonl"));
        let revisions: Vec<(String, String)> = [("D11", "https://a.example/D11"), ("D12", "https://a.example/D12"), ("D11", "https://b.example/D11")]
            .map(|(d, l)| (d.to_string(), l.to_string()))
            .to_vec();
        // The tests' own process keeps nothing: with no stand-in for the
        // database, Phabricator is not reached, and the editor reads none.
        assert!(!statuses_read(&db));
        assert_eq!(statuses(&db, &revisions), Err(Error::Said("Phabricator is not reached from a process that keeps nothing, as the tests' own".to_string())));
        let searched = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = searched.clone();
        statuses_stand_in(
            &db,
            Some(Box::new(move |_, params: Value| {
                let ids = params["constraints"]["ids"].clone();
                seen.lock().unwrap().push(ids.clone());
                // Each Phabricator its own: D11 needs review on the first,
                // is abandoned on the second.
                let status = if ids == json!([11]) { "abandoned" } else { "needs-review" };
                Ok(json!({"data": ids.as_array().unwrap().iter().map(|n| revision(n.as_u64().unwrap(), Some(status), false)).collect::<Vec<_>>()}))
            })),
        );
        assert!(statuses_read(&db) && !statuses_read(&other), "for that database alone");
        let found = statuses(&db, &revisions).unwrap();
        assert_eq!(*searched.lock().unwrap(), [json!([11, 12]), json!([11])], "one search for each Phabricator");
        let by_link = |l: &str| found.get(l).map(|s| s.status);
        assert_eq!(
            (by_link("https://a.example/D11"), by_link("https://a.example/D12"), by_link("https://b.example/D11")),
            (Some(Status::NeedsReview), Some(Status::NeedsReview), Some(Status::Abandoned))
        );
        statuses_stand_in(&db, None);
        assert!(!statuses_read(&db));
    }

    /// A revision the Phabricator line reads (`Wanted`): D`n`, of PHID
    /// `PHID-DREV-n`, its parents named or not, its latest diff as read
    /// before, and nothing else of it; when it was last modified not known.
    fn wanted(n: u64, name_parents: bool, had: Option<(u64, &str)>) -> Wanted {
        Wanted {
            d: format!("D{n}"),
            link: format!("https://p.example/D{n}"),
            phid: format!("PHID-DREV-{n}"),
            had: had.map(|(id, raw)| Latest { diff: id, raw: raw.to_string(), ..Default::default() }),
            name_parents,
            ..Default::default()
        }
    }

    #[test]
    fn the_revisions_are_read_in_few_calls_their_parents_and_latest_diffs_all_at_once_each_raw_diff_once() {
        let comment = |phid: &str, text: &str| json!({"type": "comment", "authorPHID": "PHID-USER-ann", "fields": {}, "comments": [{"phid": phid, "version": 1, "dateCreated": 5, "content": {"raw": text}}]});
        let edge = |from: u64, to: u64| json!({"sourcePHID": format!("PHID-DREV-{from}"), "edgeType": "revision.parent", "destinationPHID": format!("PHID-DREV-{to}")});
        // The commit each diff is on, among its refs: none for D11's.
        let refs = |id: u64| match id {
            9 => json!([{"type": "branch", "name": "main"}, {"type": "base", "identifier": "b9"}]),
            7 | 8 => json!([{"type": "base", "identifier": format!("b{id}")}]),
            _ => json!([]),
        };
        let diff = |id: u64, of: u64| json!({"id": id, "phid": format!("PHID-DIFF-{id}"), "fields": {"revisionPHID": format!("PHID-DREV-{of}"), "refs": refs(id)}});
        let asked = std::cell::RefCell::new(Vec::new());
        // D12 on D11; D13 on D12 and on D9, which none of them is. D13's
        // diffs 7 and 9, its latest; D12's 8; D11's 5; another of D11's, 3,
        // older than all of them, on a page not read.
        let mut call = |method: &str, params: Value| -> Result<Value, String> {
            asked.borrow_mut().push((method.to_string(), params.clone()));
            Ok(match method {
                // Two pages of edges, then of diffs, the newest first.
                "edge.search" => match params.get("after") {
                    None => json!({"data": [edge(12, 11)], "cursor": {"after": "e1"}}),
                    Some(_) => json!({"data": [edge(13, 12), edge(13, 9)], "cursor": {"after": null}}),
                },
                "differential.diff.search" => match params.get("after").and_then(Value::as_str) {
                    None => json!({"data": [diff(9, 13), diff(8, 12)], "cursor": {"after": "8"}}),
                    Some("8") => json!({"data": [diff(7, 13), diff(5, 11)], "cursor": {"after": "5"}}),
                    Some(_) => json!({"data": [diff(3, 11)], "cursor": {"after": null}}),
                },
                "differential.revision.search" => json!({"data": [{"id": 9, "phid": "PHID-DREV-9", "fields": {}}]}),
                "differential.getrawdiff" => json!(format!("raw {}", params["diffID"])),
                // D13's comments on two pages; none on the others.
                "transaction.search" => match (params["objectIdentifier"].as_str(), params.get("after")) {
                    (Some("D13"), None) => json!({"data": [comment("c1", "Why?")], "cursor": {"after": "1"}}),
                    (Some("D13"), Some(_)) => json!({"data": [comment("c2", "Because."), {"type": "accept", "fields": {}}], "cursor": {"after": null}}),
                    _ => json!({"data": [], "cursor": {"after": null}}),
                },
                m => return Err(format!("not as Phabricator is asked: {m}")),
            })
        };
        // D13 first, as the patch shown names it; D12's latest diff read
        // before; D11 at the bottom, its parents not named.
        let all = [wanted(13, true, None), wanted(11, false, None), wanted(12, true, Some((8, "as before")))];
        let mut got = Vec::new();
        latests_over(&mut call, &all.iter().collect::<Vec<_>>(), &mut |link, l| got.push((link.to_string(), l))).unwrap();
        let phids = json!(["PHID-DREV-13", "PHID-DREV-11", "PHID-DREV-12"]);
        let asked = asked.into_inner();
        let methods: Vec<&str> = asked.iter().map(|(m, _)| m.as_str()).collect();
        // The parents and the latest diffs of all three at once, page after
        // page, the diffs until each has its newest; the parent that is none
        // of them named in one search; then each in turn, D12's raw diff not
        // read again, its comments read all the same. Their authors and
        // lines are not read: no user.search, no diff's lines.
        assert_eq!(
            methods,
            [
                "edge.search",
                "edge.search",
                "differential.diff.search",
                "differential.diff.search",
                "differential.revision.search",
                "differential.getrawdiff",
                "transaction.search",
                "transaction.search",
                "differential.getrawdiff",
                "transaction.search",
                "transaction.search",
            ]
        );
        assert_eq!(asked[0].1, json!({"sourcePHIDs": phids, "types": ["revision.parent"]}));
        assert_eq!(asked[1].1, json!({"sourcePHIDs": phids, "types": ["revision.parent"], "after": "e1"}));
        assert_eq!(asked[2].1, json!({"constraints": {"revisionPHIDs": phids}, "order": "newest"}));
        assert_eq!(asked[3].1, json!({"constraints": {"revisionPHIDs": phids}, "order": "newest", "after": "8"}));
        assert_eq!(asked[4].1, json!({"constraints": {"phids": ["PHID-DREV-9"]}}));
        assert_eq!((asked[5].1.clone(), asked[8].1.clone()), (json!({"diffID": 9}), json!({"diffID": 5})));
        // Each as soon as it is read, in the order given: its latest diff,
        // its comments, and its parents, each named.
        assert_eq!(
            got.iter().map(|(link, l)| (link.as_str(), l.diff, l.raw.as_str())).collect::<Vec<_>>(),
            [("https://p.example/D13", 9, "raw 9"), ("https://p.example/D11", 5, "raw 5"), ("https://p.example/D12", 8, "as before")]
        );
        // The commit each latest diff is on, as its refs name it.
        assert_eq!(got.iter().map(|(_, l)| l.base.as_str()).collect::<Vec<_>>(), ["b9", "", "b8"]);
        assert_eq!(got[0].1.comments.iter().map(|c| (c.phid.as_str(), c.text.as_str())).collect::<Vec<_>>(), [("c1", "Why?"), ("c2", "Because.")]);
        let named = |ps: &[(&str, &str)]| ps.iter().map(|(p, d)| (p.to_string(), d.to_string())).collect::<Vec<_>>();
        assert_eq!(got[0].1.parents, named(&[("PHID-DREV-12", "D12"), ("PHID-DREV-9", "D9")]));
        assert_eq!((got[1].1.parents.clone(), got[2].1.parents.clone()), (vec![], named(&[("PHID-DREV-11", "D11")])));
    }

    #[test]
    fn a_revision_s_parents_are_named_only_where_a_submit_sets_one_and_left_unnamed_when_not_found() {
        let mut searched = Vec::new();
        let mut call = |method: &str, params: Value| -> Result<Value, String> {
            assert_eq!(method, "differential.revision.search");
            searched.push(params["constraints"]["phids"].clone());
            Ok(json!({"data": []}))
        };
        let parents: HashMap<String, Vec<String>> = [("PHID-DREV-11", vec!["PHID-DREV-9"]), ("PHID-DREV-12", vec!["PHID-DREV-11", "PHID-DREV-7"])]
            .map(|(p, to)| (p.to_string(), to.into_iter().map(String::from).collect::<Vec<String>>()))
            .into();
        // D11's parents are not to be named: none searched; D12's other
        // parent, searched, is not found: unnamed.
        let all = [wanted(11, false, None), wanted(12, true, None)];
        let names = parents_named(&mut call, &all.iter().collect::<Vec<_>>(), &parents);
        assert_eq!(searched, [json!(["PHID-DREV-7"])]);
        assert_eq!((names.get("PHID-DREV-11").map(String::as_str), names.get("PHID-DREV-7"), names.get("PHID-DREV-9")), (Some("D11"), None, None));
        // Every parent one of theirs: no search at all.
        let mut none = |_: &str, _: Value| -> Result<Value, String> { panic!("nothing to search") };
        let all = [wanted(11, true, None), wanted(12, true, None), wanted(9, false, None), wanted(7, false, None)];
        assert_eq!(parents_named(&mut none, &all.iter().collect::<Vec<_>>(), &parents).len(), 4);
    }

    #[test]
    fn a_read_of_the_revisions_passes_over_one_with_no_diff_and_stops_at_a_call_that_fails() {
        let mut call = |method: &str, params: Value| -> Result<Value, String> {
            Ok(match method {
                "edge.search" => json!({"data": []}),
                // D11 and D13 have no diff.
                "differential.diff.search" => json!({"data": [{"id": 4, "fields": {"revisionPHID": "PHID-DREV-12"}}]}),
                "differential.getrawdiff" => json!("raw 4"),
                "transaction.search" => json!({"data": []}),
                m => return Err(format!("not as Phabricator is asked: {m}: {params}")),
            })
        };
        let all = [wanted(11, true, None), wanted(12, true, None), wanted(13, true, None)];
        let mut got = Vec::new();
        let read = latests_over(&mut call, &all.iter().collect::<Vec<_>>(), &mut |link, l| got.push((link.to_string(), l.diff)));
        assert_eq!((read, got), (Ok(()), vec![("https://p.example/D12".to_string(), 4)]));
        // A call refused: what is wrong said, nothing more read.
        let mut refused = |m: &str, _: Value| -> Result<Value, String> { Err(format!("{m}: refused")) };
        let read = latests_over(&mut refused, &all.iter().collect::<Vec<_>>(), &mut |_, _| panic!("nothing read"));
        assert_eq!(read, Err("edge.search: refused".to_string()));
    }

    #[test]
    fn a_fetch_stores_what_it_reads_and_one_finding_all_as_stored_writes_nothing_of_it_but_the_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("db.jsonl");
        // Stack s: D11, D12 on it, then a patch naming D13, which
        // Phabricator does not have.
        let patches = [(1, 11), (2, 12), (3, 13)]
            .map(|(k, n)| crate::db::Patch::new(format!("c{k}"), None, None, format!("Bug {k}\n\nDifferential Revision: https://p.example/D{n}"), format!("diff --git a/f{k} b/f{k}\n")));
        let stack = Stack { name: "s".into(), patches: patches.iter().map(|p| p.id.clone()).collect(), base: None, repo: None, parent: None, op: "import".into(), time: 1 };
        let mut records: Vec<crate::db::Record> = patches.into_iter().map(crate::db::Record::Patch).collect();
        records.push(crate::db::Record::Stack(stack));
        Db::append(&db, &records).unwrap();
        // The tests' own process keeps nothing: with no stand-in, nothing
        // read, nothing stored.
        assert_eq!(fetch_latest(&db, "s", None), Err(Error::Said(NOT_REACHED.into())));
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let modified = std::sync::Arc::new(std::sync::Mutex::new(100i64));
        let (seen, at) = (calls.clone(), modified.clone());
        statuses_stand_in(
            &db,
            Some(Box::new(move |method, p: Value| {
                seen.lock().unwrap().push(method.to_string());
                let at = *at.lock().unwrap();
                Ok(match method {
                    "differential.revision.search" => json!({"data": [
                        {"id": 11, "phid": "PHID-DREV-11", "fields": {"status": {"value": "needs-review"}, "title": "Bug 1", "bugzilla.bug-id": 1, "dateModified": 100}},
                        {"id": 12, "phid": "PHID-DREV-12", "fields": {"status": {"value": "changes-planned"}, "title": "WIP: Bug 2", "summary": "Why.", "dateModified": at,
                            "isDraft": true, "repositoryPHID": "PHID-REPO-1"}},
                    ]}),
                    "edge.search" => json!({"data": [{"sourcePHID": "PHID-DREV-12", "destinationPHID": "PHID-DREV-11"}]}),
                    "differential.diff.search" => json!({"data": [{"id": 6, "fields": {"revisionPHID": "PHID-DREV-12"}}, {"id": 5, "fields": {"revisionPHID": "PHID-DREV-11"}}]}),
                    "differential.getrawdiff" => json!(format!("raw {}", p["diffID"])),
                    "transaction.search" => match p["objectIdentifier"].as_str() {
                        Some("D12") => json!({"data": [{"type": "comment", "fields": {}, "comments": [{"phid": "c1", "version": 1, "content": {"raw": "Why?"}}]}]}),
                        _ => json!({"data": []}),
                    },
                    m => return Err(format!("not as Phabricator is asked: {m}").into()),
                })
            })),
        );
        let link = |n: u64| format!("https://p.example/D{n}");
        let count = |db: &Path| {
            Db::load(db).unwrap().records().iter().fold((0, 0, 0), |(r, w, f), rec| match rec {
                crate::db::Record::Revision(_) => (r + 1, w, f),
                crate::db::Record::RawDiff { .. } => (r, w + 1, f),
                crate::db::Record::Fetched { .. } => (r, w, f + 1),
                _ => (r, w, f),
            })
        };
        // Stored: each revision's state, its comments, its raw latest diff;
        // D13 as not found; and the fetch, of all three. Each state says
        // whether the revision is a draft, and its repository, as the
        // search said, none when it said none.
        assert_eq!(fetch_latest(&db, "s", None), Ok("fetched the latest of 3 revisions from Phabricator".into()));
        let d = Db::load(&db).unwrap();
        let state = State { status: "needs-review".into(), title: "Bug 1".into(), bug: "1".into(), phid: "PHID-DREV-11".into(), diff: 5, draft: Some(false), ..Default::default() };
        assert_eq!(d.revision_state(&link(11)).map(|(s, _)| s.clone()), Some(state));
        let (s12, _) = d.revision_state(&link(12)).unwrap();
        assert_eq!((s12.status.as_str(), s12.summary.as_str(), s12.parents.clone(), s12.diff), ("changes-planned", "Why.", vec![("PHID-DREV-11".to_string(), "D11".to_string())], 6));
        assert_eq!((s12.draft, s12.repository.as_str()), (Some(true), "PHID-REPO-1"));
        assert_eq!(d.revision_comments(&link(12)).map(|(c, _)| c.clone()), Some(Comments { modified: Some(100), list: vec![Seen { id: "c1".into(), ..Default::default() }] }));
        assert_eq!((d.raw_diff(&link(11), 5), d.raw_diff(&link(12), 6)), (Some("raw 5"), Some("raw 6")));
        assert!(d.revision_state(&link(13)).is_some_and(|(s, _)| s.missing));
        assert!([11, 12, 13].iter().all(|n| d.fetched(&link(*n)).is_some()));
        assert_eq!(count(&db), (3, 2, 1));
        // Again, nothing modified: the search, the parents and the latest
        // diffs read, nothing more; nothing written but the fetch.
        calls.lock().unwrap().clear();
        fetch_latest(&db, "s", None).unwrap();
        assert_eq!(*calls.lock().unwrap(), ["differential.revision.search", "edge.search", "differential.diff.search"]);
        assert_eq!(count(&db), (3, 2, 2));
        // D12 modified since: its comments read again, alone, and written
        // again, its state as it was.
        *modified.lock().unwrap() = 200;
        calls.lock().unwrap().clear();
        fetch_latest(&db, "s", None).unwrap();
        assert_eq!(*calls.lock().unwrap(), ["differential.revision.search", "edge.search", "differential.diff.search", "transaction.search"]);
        assert_eq!(count(&db), (4, 2, 3));
        assert_eq!(Db::load(&db).unwrap().revision_comments(&link(12)).map(|(c, _)| c.modified), Some(Some(200)));
        // D12 alone, as a submit fetches what it lacks: read as any fetch
        // reads it, nothing more than its search, its parents and its latest
        // diff, as it was not modified since; its parent named as it was
        // stored, though D11 is not read with it.
        calls.lock().unwrap().clear();
        assert_eq!(fetch_latest(&db, "s", Some(&[link(12)])), Ok("fetched the latest of D12 from Phabricator".into()));
        assert_eq!(*calls.lock().unwrap(), ["differential.revision.search", "edge.search", "differential.diff.search"]);
        assert_eq!(count(&db), (4, 2, 4));
        assert_eq!(Db::load(&db).unwrap().revision_state(&link(12)).map(|(s, _)| s.parents.clone()), Some(vec![("PHID-DREV-11".to_string(), "D11".to_string())]));
        // Its calls made as a submit makes its own: the same, the database's
        // stand-in asked nothing.
        calls.lock().unwrap().clear();
        let mut made = Vec::new();
        let seen = calls.clone();
        let mut call = |m: &str, p: Value| -> Result<Value, String> {
            made.push(m.to_string());
            let at = 200;
            Ok(match m {
                "differential.revision.search" => json!({"data": [{"id": 12, "phid": "PHID-DREV-12", "fields": {"status": {"value": "changes-planned"}, "title": "WIP: Bug 2", "summary": "Why.", "dateModified": at, "isDraft": true, "repositoryPHID": "PHID-REPO-1"}}]}),
                "edge.search" => json!({"data": [{"sourcePHID": "PHID-DREV-12", "destinationPHID": "PHID-DREV-11"}]}),
                "differential.diff.search" => json!({"data": [{"id": 6, "fields": {"revisionPHID": "PHID-DREV-12"}}]}),
                _ => return Err(format!("not as Phabricator is asked: {m} {p}")),
            })
        };
        assert_eq!(fetch_with(&db, "s", &[link(12)], &mut call), Ok("fetched the latest of D12 from Phabricator".into()));
        assert_eq!((made, seen.lock().unwrap().len()), (vec!["differential.revision.search".to_string(), "edge.search".into(), "differential.diff.search".into()], 0));
        assert_eq!(count(&db), (4, 2, 5));
        // Turned away: said so, of its kind, and nothing written.
        let before = std::fs::read(&db).unwrap();
        statuses_stand_in(&db, Some(Box::new(|_, _| Err(Error::TurnedAway))));
        assert_eq!(fetch_latest(&db, "s", None), Err(Error::TurnedAway));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        statuses_stand_in(&db, None);
    }

    #[test]
    fn the_revisions_of_an_editor_are_read_by_its_database_s_stand_in_each_phabricator_on_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("db.jsonl");
        let mut b = wanted(21, false, None);
        b.link = "https://b.example/D21".into();
        let all = [wanted(11, false, None), b, wanted(12, false, None)];
        // The tests' own process keeps nothing: with no stand-in, none read.
        let nowhere = "Phabricator is not reached from a process that keeps nothing, as the tests' own";
        assert_eq!(latests(&db, &all, &mut |_, _| panic!("nothing read")), Err(Error::Said(nowhere.to_string())));
        let edges = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = edges.clone();
        statuses_stand_in(
            &db,
            Some(Box::new(move |m, params: Value| {
                Ok(match m {
                    "edge.search" => {
                        seen.lock().unwrap().push(params["sourcePHIDs"].clone());
                        json!({"data": []})
                    }
                    "differential.diff.search" => {
                        let phids = params["constraints"]["revisionPHIDs"].as_array().cloned().unwrap_or_default();
                        json!({"data": phids.iter().map(|p| json!({"id": 1, "fields": {"revisionPHID": p}})).collect::<Vec<_>>()})
                    }
                    "differential.getrawdiff" => json!("raw"),
                    _ => json!({"data": []}),
                })
            })),
        );
        let mut got = Vec::new();
        latests(&db, &all, &mut |link, _| got.push(link.to_string())).unwrap();
        assert_eq!(*edges.lock().unwrap(), [json!(["PHID-DREV-11", "PHID-DREV-12"]), json!(["PHID-DREV-21"])], "one read for each Phabricator");
        assert_eq!(got, ["https://p.example/D11", "https://p.example/D12", "https://b.example/D21"]);
        statuses_stand_in(&db, None);
    }

    #[test]
    fn a_comment_on_a_line_after_no_newline_at_end_of_file_is_on_that_line() {
        // f.txt ended with no newline: the patch gives it one, and a line
        // more, after git's `\ No newline at end of file`.
        let diff = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,3 @@\n one\n-two\n\\ No newline at end of file\n+two\n+three\n";
        let p = crate::db::Patch::new("c".into(), None, None, "Bug 1 - three".into(), diff.into());
        let lines = crate::view::note_lines(&p);
        let added = |text: &str| lines.iter().find(|l| l.origin == '+' && l.text == text).map(|l| l.key.clone());
        let on = |line: usize, text: Option<&str>| {
            let c = Remote { path: Some("f.txt".into()), line, line_text: text.map(String::from), ..Default::default() };
            anchor(&lines, &c)
        };
        // By its number, the line's text not found, and by its text.
        assert_eq!(on(2, None), added("two"));
        assert_eq!(on(3, None), added("three"));
        assert_eq!(on(3, Some("three")), added("three"));
        assert_eq!(on(2, Some("gone since")), added("two"));
    }

    #[test]
    fn a_revision_not_modified_since_it_was_read_is_not_read_again_but_for_its_parents() {
        let asked = std::cell::RefCell::new(Vec::new());
        let mut call = |method: &str, params: Value| -> Result<Value, String> {
            asked.borrow_mut().push(format!("{method} {}", params.get("objectIdentifier").or(params.get("diffID")).map(Value::to_string).unwrap_or_default()));
            Ok(match method {
                // D12 on D11 now, set from D11's side.
                "edge.search" => json!({"data": [{"sourcePHID": "PHID-DREV-12", "destinationPHID": "PHID-DREV-11"}]}),
                // D14's latest diff a new one, 9; the others' as read.
                "differential.diff.search" => {
                    json!({"data": (11..=15).map(|n| json!({"id": if n == 14 { 9 } else { n - 10 }, "fields": {"revisionPHID": format!("PHID-DREV-{n}")}})).collect::<Vec<_>>()})
                }
                "differential.getrawdiff" => json!(format!("raw {}", params["diffID"])),
                "transaction.search" => json!({"data": [{"type": "comment", "fields": {}, "comments": [{"phid": "new", "version": 1, "content": {"raw": "New."}}]}]}),
                m => return Err(format!("not as Phabricator is asked: {m}")),
            })
        };
        let old = vec![Remote { phid: "old".into(), text: "Old.".into(), ..Default::default() }];
        let read = |n: u64, modified: Option<i64>| Latest { diff: n - 10, raw: format!("raw of D{n}"), comments: old.clone(), modified, ..Default::default() };
        let at = |n: u64, modified: Option<i64>, had: Option<Latest>| Wanted { modified, had, ..wanted(n, false, None) };
        let all = [
            // D11 and D12 not modified since they were read; D13 modified
            // since; D14 neither, but given a new diff; D15 never read.
            at(11, Some(100), Some(read(11, Some(100)))),
            at(12, Some(100), Some(read(12, Some(100)))),
            at(13, Some(200), Some(read(13, Some(100)))),
            at(14, Some(100), Some(read(14, Some(100)))),
            at(15, Some(100), None),
        ];
        let mut got = Vec::new();
        latests_over(&mut call, &all.iter().collect::<Vec<_>>(), &mut |link, l| got.push((link.to_string(), l))).unwrap();
        // The parents and the latest diffs of all at once, as ever; then
        // only those modified since, given a new diff, or never read: their
        // comments, and a raw diff not read before.
        assert_eq!(
            asked.into_inner(),
            [
                "edge.search ",
                "differential.diff.search ",
                "transaction.search \"D13\"",
                "differential.getrawdiff 9",
                "transaction.search \"D14\"",
                "differential.getrawdiff 5",
                "transaction.search \"D15\"",
            ]
        );
        // Each told all the same: as it was read, its parents as they are
        // now; those read again, their comments as they are, and when
        // they were last modified as the search says.
        let each: Vec<(&str, &str, &str, Option<i64>)> = got.iter().map(|(link, l)| (link.as_str(), l.raw.as_str(), l.comments[0].text.as_str(), l.modified)).collect();
        assert_eq!(
            each,
            [
                ("https://p.example/D11", "raw of D11", "Old.", Some(100)),
                ("https://p.example/D12", "raw of D12", "Old.", Some(100)),
                ("https://p.example/D13", "raw of D13", "New.", Some(200)),
                ("https://p.example/D14", "raw 9", "New.", Some(100)),
                ("https://p.example/D15", "raw 5", "New.", Some(100)),
            ]
        );
        assert_eq!(got[1].1.parents, [("PHID-DREV-11".to_string(), "D11".to_string())]);
        // When it was last modified not known, or not kept with what was
        // read (a submit or a pull forgot it): read again.
        for (modified, had) in [(None, Some(100)), (Some(100), None), (None, None)] {
            let asked = std::cell::RefCell::new(0);
            let mut call = |method: &str, _: Value| -> Result<Value, String> {
                *asked.borrow_mut() += usize::from(method == "transaction.search");
                Ok(match method {
                    "differential.diff.search" => json!({"data": [{"id": 1, "fields": {"revisionPHID": "PHID-DREV-11"}}]}),
                    _ => json!({"data": []}),
                })
            };
            let one = at(11, modified, Some(read(11, had)));
            latests_over(&mut call, &[&one], &mut |_, _| {}).unwrap();
            assert_eq!(asked.into_inner(), 1, "{modified:?}, {had:?}");
        }
    }

    /// The token of the tests' calls by curl: no one's.
    const TOKEN: &str = "api-scratchtokenofthetests";

    /// A stand-in for curl, a script that `sh` runs, so that it is never
    /// run as a program just written, which another test's fork might
    /// still hold: it writes its arguments to `args` beside it, one a line,
    /// and what it reads to `read`; then, as curl with `-D -`, the headers
    /// and the page of `answer`. Or, the answer `curl:` and more, it says
    /// that alone, as curl says it cannot connect.
    fn curl_answering(dir: &Path, answer: &str) -> Command {
        std::fs::write(dir.join("answer"), answer).unwrap();
        let here = "\"$(dirname \"$0\")\"";
        let script = dir.join("curl.sh");
        std::fs::write(
            &script,
            format!("printf '%s\\n' \"$@\" > {here}/args\ncat > {here}/read\nif grep -q '^curl:' {here}/answer; then cat {here}/answer >&2; exit 7; fi\ncat {here}/answer\n"),
        )
        .unwrap();
        let mut c = Command::new("sh");
        c.arg(script);
        c
    }

    /// What a call of `method` by the stand-in for curl, answering
    /// `answer`, comes to.
    fn answer_of(dir: &Path, method: &str, answer: &str) -> Result<Value, Error> {
        request(curl_answering(dir, answer), "https://p.example/api/", TOKEN, method, json!({"constraints": {"ids": [11]}}))
    }

    #[test]
    fn curl_is_given_the_token_and_the_parameters_on_its_standard_input_never_on_its_command_line() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"result\": {\"data\": []}, \"error_code\": null, \"error_info\": null}";
        assert_eq!(answer_of(tmp.path(), "differential.revision.search", ok), Ok(json!({"data": []})));
        let args = std::fs::read_to_string(tmp.path().join("args")).unwrap();
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "-sS",
                "--max-time",
                "30",
                "-D",
                "-",
                "-X",
                "POST",
                "https://p.example/api/differential.revision.search",
                "--data-urlencode",
                "params@-",
                "--data",
                "output=json",
                "--data",
                "__conduit__=1",
            ]
        );
        assert!(!args.contains(TOKEN) && !args.contains("constraints"), "{args}");
        let read: Value = serde_json::from_str(&std::fs::read_to_string(tmp.path().join("read")).unwrap()).unwrap();
        assert_eq!(read, json!({"constraints": {"ids": [11]}, "__conduit__": {"token": TOKEN}}));
    }

    #[test]
    fn a_call_turned_away_says_http_429_plainly_whatever_phabricator_asks_to_wait() {
        let tmp = tempfile::tempdir().unwrap();
        let away = |retry_after: &str| format!("HTTP/1.1 429 Too Many Requests\r\n{retry_after}Content-Length: 0\r\n\r\n");
        // Two minutes asked, the page empty: its own kind of error, which
        // says HTTP 429 plainly.
        let e = answer_of(tmp.path(), "conduit.ping", &away("Retry-After: 120\r\n")).unwrap_err();
        assert_eq!(e, Error::TurnedAway);
        assert_eq!(e.to_string(), "Phabricator is turning away requests from this machine for now (HTTP 429, too many requests)");
        assert_eq!(String::from(e), TURNED_AWAY);
        // Nothing asked, or no time to come: turned away all the same.
        assert_eq!(answer_of(tmp.path(), "conduit.ping", &away("")), Err(Error::TurnedAway));
        assert_eq!(answer_of(tmp.path(), "conduit.ping", &away("retry-after: 0\r\n")), Err(Error::TurnedAway));
        // As HTTP/2 writes it, after an answer that says to go on.
        let continued = "HTTP/1.1 100 Continue\r\n\r\nHTTP/2 429 \r\nretry-after: 30\r\n\r\n";
        assert_eq!(answer_of(tmp.path(), "conduit.ping", continued), Err(Error::TurnedAway));
    }

    #[test]
    fn another_status_or_a_page_that_is_not_json_is_said_with_the_page_s_first_line() {
        let tmp = tempfile::tempdir().unwrap();
        let method = "differential.revision.search";
        // A gateway's error page: the status, and the page's first line.
        let page = "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/html\r\n\r\n<html><head><title>502 Bad Gateway</title></head>\r\n<body><center><h1>502 Bad Gateway</h1></center></body>\r\n</html>\r\n";
        assert_eq!(
            answer_of(tmp.path(), method, page),
            Err(Error::Said("differential.revision.search: Phabricator answered HTTP 502 with a page starting \"<html><head><title>502 Bad Gateway</title></head>\"".into()))
        );
        // Its first line long: cut short. None: said so.
        let long = format!("HTTP/1.1 503 Service Unavailable\r\n\r\n\r\n  {}  \r\nmore", "x".repeat(100));
        assert_eq!(
            answer_of(tmp.path(), method, &long),
            Err(Error::Said(format!("differential.revision.search: Phabricator answered HTTP 503 with a page starting \"{}…\"", "x".repeat(80))))
        );
        assert_eq!(
            answer_of(tmp.path(), method, "HTTP/1.1 500 Internal Server Error\r\n\r\n"),
            Err(Error::Said("differential.revision.search: Phabricator answered HTTP 500 with an empty page".into()))
        );
        // Another status that is not 429 is no refusal of this machine.
        assert!(matches!(answer_of(tmp.path(), method, "HTTP/1.1 403 Forbidden\r\n\r\nNo."), Err(Error::Said(_))));
        // HTTP 200, but a page that is not JSON, or none.
        assert_eq!(
            answer_of(tmp.path(), method, "HTTP/1.1 200 OK\r\n\r\n<!DOCTYPE html>\n<p>Log in.</p>"),
            Err(Error::Said("differential.revision.search: Phabricator answered with a page that is not JSON, starting \"<!DOCTYPE html>\"".into()))
        );
        assert_eq!(
            answer_of(tmp.path(), method, "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"),
            Err(Error::Said("differential.revision.search: Phabricator answered with an empty page".into()))
        );
        // Conduit's own error, and curl's, as before.
        let refused = "HTTP/1.1 200 OK\r\n\r\n{\"result\": null, \"error_code\": \"ERR-INVALID-AUTH\", \"error_info\": \"API token is invalid.\"}";
        assert_eq!(answer_of(tmp.path(), method, refused), Err(Error::Said("differential.revision.search: API token is invalid.".into())));
        assert_eq!(
            answer_of(tmp.path(), method, "curl: (7) Failed to connect to p.example port 443"),
            Err(Error::Said("differential.revision.search: curl: (7) Failed to connect to p.example port 443".into()))
        );
    }

    /// The request a server on the loopback was sent, read from `c`: its
    /// headers, then its body, as long as its `Content-Length` says.
    fn request_read(c: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        let body_at = loop {
            if let Some(k) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                break k + 4;
            }
            let n = c.read(&mut buf).unwrap();
            assert!(n > 0, "the request ended before its headers");
            got.extend_from_slice(&buf[..n]);
        };
        let head = String::from_utf8_lossy(&got[..body_at]).to_lowercase();
        let length: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).map_or(0, |n| n.trim().parse().unwrap());
        while got.len() < body_at + length {
            let n = c.read(&mut buf).unwrap();
            assert!(n > 0, "the request ended before its body");
            got.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&got).into_owned()
    }

    /// Form field `name` of `body`, as a form encodes it, decoded.
    fn form_field(body: &str, name: &str) -> Option<String> {
        let value = body.split('&').find_map(|f| f.strip_prefix(&format!("{name}=")))?;
        let bytes = value.as_bytes();
        let mut out = Vec::new();
        let mut k = 0;
        while k < bytes.len() {
            match bytes[k] {
                b'+' => out.push(b' '),
                b'%' => {
                    out.push(u8::from_str_radix(std::str::from_utf8(bytes.get(k + 1..k + 3)?).ok()?, 16).ok()?);
                    k += 2;
                }
                b => out.push(b),
            }
            k += 1;
        }
        String::from_utf8(out).ok()
    }

    #[test]
    fn curl_itself_sends_the_token_in_its_request_and_says_what_a_server_on_the_loopback_answered() {
        // On the loopback alone, never the network: a 429 asking for two
        // minutes, a gateway's error page, then Conduit's answer.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://127.0.0.1:{}/api/", server.local_addr().unwrap().port());
        let page = "<html><head><title>502 Bad Gateway</title></head>\r\n</html>\r\n";
        let json = "{\"result\": \"pong\", \"error_code\": null, \"error_info\": null}";
        let answers = [
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            format!("HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len()),
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len()),
        ];
        let served = std::thread::spawn(move || {
            answers.map(|a| {
                let (mut c, _) = server.accept().unwrap();
                let request = request_read(&mut c);
                c.write_all(a.as_bytes()).unwrap();
                request
            })
        });
        // curl itself, without the settings of anyone's ~/.curlrc (`-q`),
        // nor a proxy.
        let curl = || {
            let mut c = Command::new("curl");
            c.args(["-q", "--noproxy", "*"]);
            c
        };
        let ping = || request(curl(), &api, TOKEN, "conduit.ping", json!({"x": "a b&c=d"}));
        assert_eq!(ping(), Err(Error::TurnedAway));
        assert_eq!(ping(), Err(Error::Said("conduit.ping: Phabricator answered HTTP 502 with a page starting \"<html><head><title>502 Bad Gateway</title></head>\"".into())));
        assert_eq!(ping(), Ok(json!("pong")));
        // Each a POST of the form Conduit reads, the token in its
        // parameters, which curl read on its standard input.
        for request in served.join().unwrap() {
            assert!(request.starts_with("POST /api/conduit.ping HTTP/1.1\r\n"), "{request}");
            let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
            let params: Value = serde_json::from_str(&form_field(body, "params").unwrap()).unwrap();
            assert_eq!(params, json!({"x": "a b&c=d", "__conduit__": {"token": TOKEN}}));
            assert_eq!((form_field(body, "output").as_deref(), form_field(body, "__conduit__").as_deref()), (Some("json"), Some("1")));
        }
    }

    #[test]
    fn a_read_turned_away_says_so_of_its_kind_whatever_the_calls_make_of_it() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("db.jsonl");
        // Phabricator turning this machine away.
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        statuses_stand_in(
            &db,
            Some(Box::new(move |m, _| {
                seen.lock().unwrap().push(m.to_string());
                Err(Error::TurnedAway)
            })),
        );
        let revisions = [("D11".to_string(), "https://p.example/D11".to_string())];
        assert_eq!(statuses(&db, &revisions), Err(Error::TurnedAway));
        // The lines' read, though its calls take what went wrong as text,
        // stopped at its first call.
        let all = [wanted(11, false, None)];
        assert_eq!(latests(&db, &all, &mut |_, _| panic!("nothing read")), Err(Error::TurnedAway));
        assert_eq!(*asked.lock().unwrap(), ["differential.revision.search", "edge.search"]);
        // Another error is said as it was.
        statuses_stand_in(&db, Some(Box::new(|m, _| Err(format!("{m}: Phabricator is down").into()))));
        assert_eq!(statuses(&db, &revisions), Err(Error::Said("differential.revision.search: Phabricator is down".into())));
        statuses_stand_in(&db, None);
    }
}
