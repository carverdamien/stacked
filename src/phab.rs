//! Reviewers' comments from Phabricator: each patch's revision (its
//! `Differential Revision:` trailer) read over Conduit with the token
//! moz-phab and arc keep in `~/.arcrc`, or the one the settings give, which
//! curl is given on its standard input (`request`).
//! Taken from self-review's, less what files them in its sessions:
//! `review::pull` files them in the database.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::view::{NoteLine, fold};

/// A Conduit endpoint and the token for it.
pub struct Conduit {
    pub api: String,
    token: String,
}

/// What a call of Conduit came to, when not to its result: Phabricator
/// turning away the requests of this machine for now (HTTP 429, too many
/// requests), for as long as its `Retry-After` says, when it says a time to
/// come; or anything else that went wrong, said.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    TurnedAway(Option<Duration>),
    Said(String),
}

/// What a call that Phabricator turned away says.
pub const TURNED_AWAY: &str = "Phabricator is turning away requests from this machine for now (HTTP 429, too many requests)";

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::TurnedAway(_) => f.write_str(TURNED_AWAY),
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
/// as it says it; Phabricator turning this machine away (HTTP 429), for as
/// long as it says; a status other than 200, or a page that is not JSON,
/// said with the page's first line; or the error Conduit gives.
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
    let (status, retry_after, page) = answered(&out.stdout);
    let first = String::from_utf8_lossy(page).lines().map(str::trim).find(|l| !l.is_empty()).map(|l| match l.chars().count() > 80 {
        true => format!("{}…", l.chars().take(80).collect::<String>()),
        false => l.to_string(),
    });
    match (status, &first) {
        (Some(429), _) => return Err(Error::TurnedAway(retry_after.as_deref().and_then(|v| wait_asked(v, crate::db::now())))),
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
/// that only say to go on (`100 Continue`), its `Retry-After` if it has
/// one, and its page. No status where curl wrote no headers.
fn answered(out: &[u8]) -> (Option<u16>, Option<String>, &[u8]) {
    let (mut status, mut retry_after, mut rest) = (None, None, out);
    while rest.starts_with(b"HTTP/") {
        let end = rest
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|k| k + 4)
            .or_else(|| rest.windows(2).position(|w| w == b"\n\n").map(|k| k + 2))
            .unwrap_or(rest.len());
        let headers = String::from_utf8_lossy(&rest[..end]);
        status = headers.split_whitespace().nth(1).and_then(|s| s.parse().ok());
        retry_after = headers.lines().skip(1).find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim().eq_ignore_ascii_case("retry-after").then(|| value.trim().to_string())
        });
        rest = &rest[end..];
    }
    (status, retry_after, rest)
}

/// How long a `Retry-After` of `value` says to wait, at `now` (seconds since
/// 1970): its number of seconds, or until its date, as HTTP writes one
/// (`Wed, 21 Oct 2015 07:28:00 GMT`). None for no time to come.
fn wait_asked(value: &str, now: i64) -> Option<Duration> {
    if let Ok(secs) = value.trim().parse::<u64>() {
        return (secs > 0).then(|| Duration::from_secs(secs));
    }
    let [_, day, month, year, time, "GMT"] = value.split_whitespace().collect::<Vec<_>>()[..] else { return None };
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS.iter().position(|m| *m == month)? as i64 + 1;
    let (day, year): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let hms: Vec<i64> = time.split(':').map(|n| n.parse().ok()).collect::<Option<_>>()?;
    let [h, m, s] = hms[..] else { return None };
    // Days since 1970-01-01, as Howard Hinnant's days_from_civil counts
    // them: from March, so that February's last day ends a year.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let of_era = y - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468;
    let at = days * 86_400 + h * 3600 + m * 60 + s;
    (at > now).then(|| Duration::from_secs((at - now) as u64))
}

/// A comment as Phabricator has it, as it was last edited: its id is that
/// version's, each edit making one of its own.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Remote {
    pub phid: String,
    pub author: String,
    pub text: String,
    /// When it was first written.
    pub created: i64,
    /// None for a comment on the revision as a whole.
    pub path: Option<String>,
    pub line: usize,
    /// The line's text in the diff it was made on, when that is known.
    pub line_text: Option<String>,
    pub reply_to: Option<String>,
    /// What it was before its last edit, the version before it first: none
    /// if it was never edited.
    pub earlier: Vec<Version>,
}

/// A comment as it was before an edit: its id then, and what it said.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Version {
    pub phid: String,
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

/// New-side line number -> text, from a Phabricator diff's hunks.
fn diff_lines(conduit: &Conduit, diff: u64) -> HashMap<(String, usize), String> {
    let mut out = HashMap::new();
    let Ok(r) = conduit.call("differential.querydiffs", json!({"ids": [diff]})) else {
        return out;
    };
    let Some(d) = r.get(diff.to_string()) else {
        return out;
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
            let mut no = h
                .get("newOffset")
                .and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64()))
                .unwrap_or(1) as usize;
            let corpus = h.get("corpus").and_then(Value::as_str).unwrap_or("");
            for l in corpus.lines() {
                match l.chars().next() {
                    Some('-') => {}
                    Some('\\') => {}
                    _ => {
                        out.insert((path.clone(), no), l.chars().skip(1).collect());
                        no += 1;
                    }
                }
            }
        }
    }
    out
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

/// Reads every comment on `revision`.
pub fn fetch(conduit: &Conduit, revision: &str, source: &str) -> Result<RevisionComments, String> {
    let txns = transactions(&mut |m, v| conduit.call(m, v).map_err(String::from), revision)?;
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
        let r = conduit.call("user.search", json!({"constraints": {"phids": authors}}))?;
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
    let mut diffs: HashMap<u64, HashMap<(String, usize), String>> = HashMap::new();
    let comments = comments_of(&txns, &names, |path, diff, line| {
        diffs.entry(diff).or_insert_with(|| diff_lines(conduit, diff)).get(&(path.to_string(), line)).cloned()
    });
    Ok(RevisionComments {
        revision: revision.into(),
        source: source.into(),
        comments,
    })
}

/// The comments of a revision, oldest first, from its transactions as
/// transaction.search gives them: each its author's name, from `names` by
/// their PHID, and its line's text, as `read_line` reads it from (path,
/// diff, line). Each is as it was last edited, what it was before kept with
/// it; a comment removed is none.
fn comments_of(txns: &[Value], names: &HashMap<String, String>, mut read_line: impl FnMut(&str, u64, usize) -> Option<String>) -> Vec<Remote> {
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
        let line_text = match (&path, fields.pointer("/diff/id").and_then(Value::as_u64)) {
            (Some(p), Some(d)) => read_line(p, d, line),
            _ => None,
        };
        let reply_to = fields
            .get("replyToCommentPHID")
            .and_then(Value::as_str)
            .map(String::from);
        // Each of its versions, the latest first: an edit on Phabricator
        // makes one, with an id of its own.
        let mut versions: Vec<&Value> = t
            .get("comments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        versions.sort_by_key(|c| std::cmp::Reverse(c.get("version").and_then(Value::as_i64).unwrap_or(0)));
        let text = |c: &Value| c.pointer("/content/raw").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let phid = |c: &Value| c.get("phid").and_then(Value::as_str).unwrap_or("").to_string();
        // Removed, it is gone: what every version of it said is hidden.
        let Some(at) = versions
            .iter()
            .position(|c| !c.get("removed").and_then(Value::as_bool).unwrap_or(false) && !text(c).is_empty())
        else {
            continue;
        };
        comments.push(Remote {
            phid: phid(versions[at]),
            author,
            text: text(versions[at]),
            created: versions.last().and_then(|c| c.get("dateCreated")).and_then(Value::as_i64).unwrap_or(0),
            path,
            line,
            line_text,
            reply_to,
            earlier: versions[at + 1..].iter().map(|c| Version { phid: phid(c), text: text(c) }).collect(),
        });
    }
    comments.sort_by_key(|c| c.created);
    comments
}

/// What reads one revision's comments, from `(source key, D123, link)`.
pub type Fetch = fn(&str, &str, &str) -> Result<RevisionComments, String>;

/// What reads the revisions in place of Conduit, when one is set: the
/// tests', which never reach Phabricator, as `ask::stand_in` is for the
/// agent.
static STAND_IN: std::sync::Mutex<Option<Fetch>> = std::sync::Mutex::new(None);

/// Reads the revisions with `fetch` rather than over Conduit; with none,
/// over Conduit again.
pub fn stand_in(fetch: Option<Fetch>) {
    *STAND_IN.lock().unwrap() = fetch;
}

/// Reads the comments of every commit's revision: `revisions` is
/// `(source key, D123, link)`. `progress` is told of each, by its place
/// from 0, as it starts reading it.
pub fn pull(revisions: &[(String, String, String)], progress: &mut dyn FnMut(usize)) -> Result<Pulled, String> {
    let fake = *STAND_IN.lock().unwrap();
    if let Some(fetch) = fake {
        return read_each(revisions, progress, fetch);
    }
    let mut conduits: HashMap<String, Conduit> = HashMap::new();
    read_each(revisions, progress, |source, rev, url| {
        let host = url.split('/').take(3).collect::<Vec<_>>().join("/");
        if !conduits.contains_key(&host) {
            conduits.insert(host.clone(), Conduit::for_url(Some(url))?);
        }
        fetch(&conduits[&host], rev, source)
    })
}

/// Reads each of `revisions` with `fetch`, `progress` told of each, by its
/// place from 0, as it starts reading it: up to the first that fails, and
/// what is wrong with it.
fn read_each(
    revisions: &[(String, String, String)],
    progress: &mut dyn FnMut(usize),
    mut fetch: impl FnMut(&str, &str, &str) -> Result<RevisionComments, String>,
) -> Result<Pulled, String> {
    let mut out = Pulled::default();
    for (k, (source, rev, url)) in revisions.iter().enumerate() {
        progress(k);
        out.revisions.push(fetch(source, rev, url)?);
    }
    Ok(out)
}

/// A revision of Phabricator as an import takes it: its latest diff, as a
/// patch, and the commit that diff is on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Revision {
    /// `D123`.
    pub id: String,
    /// The patch's message: its commit's, as uploaded, else the revision's
    /// title and summary; with its trailer (`Differential Revision:`).
    pub message: String,
    /// Who wrote it, `Name <email>`, as its commit says, if it does.
    pub author: Option<String>,
    /// The diff, as git writes one.
    pub diff: String,
    /// The commit its diff is on, as it says, if it does.
    pub base: Option<String>,
}

/// What reads a stack of revisions in place of Conduit, from (`D123`, its
/// link if given): the tests', which never reach Phabricator.
pub type ReadStack = Box<dyn Fn(&str, Option<&str>) -> Result<Vec<Revision>, String> + Send + Sync>;

static STACK_STAND_IN: std::sync::Mutex<Option<ReadStack>> = std::sync::Mutex::new(None);

/// Reads stacks of revisions with `read` rather than over Conduit; with
/// none, over Conduit again.
pub fn stack_stand_in(read: Option<ReadStack>) {
    *STACK_STAND_IN.lock().unwrap() = read;
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

/// Revision `rev` (`D123`), at `link` if given, read with the revisions it
/// stacks on, down to the first that is not open, and those that stack on
/// it while one is open: bottom first, each as an import takes it.
pub fn read_stack(rev: &str, link: Option<&str>) -> Result<Vec<Revision>, String> {
    if let Some(read) = STACK_STAND_IN.lock().unwrap().as_ref() {
        return read(rev, link);
    }
    let conduit = Conduit::for_url(link)?;
    let trailer = crate::config::get().reviews.trailer.clone();
    stack_over(&mut |method, params| conduit.call(method, params).map_err(String::from), &conduit.api, &trailer, rev)
}

/// How Conduit is called: a method, its parameters; its `result`.
pub(crate) type Call<'a> = dyn FnMut(&str, Value) -> Result<Value, String> + 'a;

/// Whether revision `r`, as differential.revision.search gives it, is
/// open: neither landed nor abandoned.
pub(crate) fn open(r: &Value) -> bool {
    !matches!(r.pointer("/fields/status/value").and_then(Value::as_str), Some("published" | "abandoned"))
}

/// The first revision `constraints` find, as differential.revision.search
/// gives it.
pub(crate) fn revision_by(call: &mut Call, constraints: Value) -> Result<Option<Value>, String> {
    let r = call("differential.revision.search", json!({ "constraints": constraints }))?;
    Ok(r.pointer("/data/0").cloned())
}

/// Where the edges of kind `kind` (`revision.parent`, `revision.child`)
/// from revision `phid` go: their PHIDs.
pub(crate) fn edges(call: &mut Call, phid: &str, kind: &str) -> Result<Vec<String>, String> {
    let r = call("edge.search", json!({"sourcePHIDs": [phid], "types": [kind]}))?;
    Ok(r.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("destinationPHID").and_then(Value::as_str).map(String::from))
        .collect())
}

/// As `read_stack`, Conduit called with `call` at `api`, the trailer named
/// `trailer`: down while a revision stacks on one other, open; up while one
/// open revision stacks on it.
fn stack_over(call: &mut Call, api: &str, trailer: &str, rev: &str) -> Result<Vec<Revision>, String> {
    let n: u64 = rev.trim_start_matches('D').parse().map_err(|_| format!("{rev} names no revision"))?;
    let first = revision_by(call, json!({ "ids": [n] }))?.ok_or(format!("no revision {rev}"))?;
    let phid = |r: &Value| r.get("phid").and_then(Value::as_str).unwrap_or("").to_string();
    let mut chain = vec![first.clone()];
    for (kind, down) in [("revision.parent", true), ("revision.child", false)] {
        let mut at = first.clone();
        while chain.len() < 100 {
            let mut next = Vec::new();
            for p in edges(call, &phid(&at), kind)? {
                if let Some(r) = revision_by(call, json!({ "phids": [p] }))?.filter(open) {
                    next.push(r);
                }
            }
            let [r] = next.as_slice() else { break };
            if down {
                chain.insert(0, r.clone());
            } else {
                chain.push(r.clone());
            }
            at = r.clone();
        }
    }
    chain.iter().map(|r| revision_patch(call, api, trailer, r)).collect()
}

/// Revision `r`, as differential.revision.search gives it, as an import
/// takes it: its latest diff, raw; the commit that diff is on; and the
/// message and author of its commit as uploaded.
fn revision_patch(call: &mut Call, api: &str, trailer: &str, r: &Value) -> Result<Revision, String> {
    let n = r.get("id").and_then(Value::as_u64).ok_or("a revision without its id")?;
    let field = |k: &str| r.pointer(&format!("/fields/{k}")).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let diffs = call("differential.querydiffs", json!({ "revisionIDs": [n] }))?;
    let (id, d) = diffs
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.parse::<u64>().ok()?, v)))
        .max_by_key(|(k, _)| *k)
        .ok_or(format!("D{n} has no diff"))?;
    let commit = d.pointer("/properties/local:commits").and_then(Value::as_object).and_then(|m| m.values().next());
    let said = |k: &str| commit.and_then(|c| c.get(k)).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let author = said("author").map(|name| match said("authorEmail") {
        Some(e) => format!("{name} <{e}>"),
        None => name.to_string(),
    });
    let mut message = said("message")
        .map(String::from)
        .unwrap_or_else(|| format!("{}\n\n{}", field("title"), field("summary")).trim().to_string());
    let trailer = format!("{trailer}:");
    if !message.lines().any(|l| l.starts_with(&trailer)) {
        message = format!("{message}\n\n{trailer} {}D{n}", api.trim_end_matches("api/"));
    }
    let diff = call("differential.getrawdiff", json!({ "diffID": id }))?;
    Ok(Revision {
        id: format!("D{n}"),
        message,
        author,
        diff: diff.as_str().unwrap_or("").to_string(),
        base: d.get("sourceControlBaseRevision").and_then(Value::as_str).filter(|b| !b.is_empty()).map(String::from),
    })
}

/// A stack of revisions as an import makes it: where it is from (`D1..D3`),
/// its patches, bottom first, the commit the bottom one is on, and the
/// repository.
pub struct Plan {
    pub from: String,
    pub drafts: Vec<crate::mbox::Draft>,
    pub base: String,
    pub repo: String,
}

/// Revisions `revs`, read for `rev`, bottom first, as an import makes them
/// in the checkout at `at`: on the commit the bottom one is on, which the
/// checkout must have. What is wrong, said, if they cannot be.
pub fn plan(at: &Path, rev: &str, revs: Vec<Revision>) -> Result<Plan, String> {
    let Some(bottom) = revs.first() else {
        return Err(format!("{rev}: nothing to import"));
    };
    let Some(base) = bottom.base.clone() else {
        return Err(format!("{} does not say what commit it is on", bottom.id));
    };
    let Ok(repo) = crate::git::toplevel(at) else {
        return Err("not in a git repository: a stack is imported into the checkout stacked runs in".into());
    };
    let Ok(base) = crate::git::rev_parse(at, &base) else {
        return Err(format!("{} is on {}, which this repository does not have: fetch it first", bottom.id, &base[..12.min(base.len())]));
    };
    let from = match revs.as_slice() {
        [a, .., b] => format!("{}..{}", a.id, b.id),
        _ => rev.to_string(),
    };
    let drafts = revs
        .into_iter()
        .map(|r| crate::mbox::Draft {
            author: r.author,
            message: r.message,
            diff: r.diff,
            ..Default::default()
        })
        .collect();
    Ok(Plan { from, drafts, base, repo })
}

/// `stacked moz-phab import`: the revision `text` names (`D123`, `123`, or
/// its link) read with those it stacks on, and those that stack on it while
/// one is open, imported as a new stack of the database at `db`, in the
/// checkout at `at`, named `name` (else the revision's, or one free): what
/// was imported, said. Not while the settings say reviews come from
/// nowhere, as the editor offers no import then.
pub fn import(db: &Path, at: &Path, text: &str, name: Option<&str>) -> crate::db::Result<String> {
    use crate::db::{Db, err};
    if crate::config::get().reviews.source == crate::config::Source::None {
        return err(format!(
            "revisions come from nowhere: reviews.source is \"none\" in {}",
            crate::config::path().display()
        ));
    }
    let Some((rev, link)) = revision_named(text) else {
        return err(format!("{text:?} names no revision: give D123, 123, or its link"));
    };
    if let Some(n) = name
        && let Some(why) = crate::import::name_wrong(&Db::load(db)?, n)
    {
        return err(why);
    }
    let revs = read_stack(&rev, link.as_deref()).map_err(crate::db::Error)?;
    let p = plan(at, &rev, revs).map_err(crate::db::Error)?;
    let n = p.drafts.len();
    let name = Db::edit(db, |d| {
        let name = match name {
            Some(n) => n.trim().to_string(),
            None => crate::import::free_name(d, &rev),
        };
        if let Some(why) = crate::import::name_wrong(d, &name) {
            return err(why);
        }
        crate::ops::import(d, &name, p.drafts, false, Some((p.base, p.repo)))?;
        Ok(name)
    })?;
    Ok(format!("imported {n} patch{} from {} as {name}", if n == 1 { "" } else { "es" }, p.from))
}

/// The key of the line a comment is on: the line with its text near its
/// number, else the line at its number, else None.
pub fn anchor(lines: &[NoteLine], c: &Remote) -> Option<String> {
    let path = c.path.as_deref()?;
    // The new side's lines: not those removed, nor git's `\ No newline at
    // end of file`, which has the number of the line after it.
    let here: Vec<&NoteLine> = lines
        .iter()
        .filter(|l| l.file == path && matches!(l.origin, ' ' | '+'))
        .collect();
    if let Some(t) = &c.line_text {
        let want = fold(t);
        if let Some(l) = here
            .iter()
            .filter(|l| l.text == want)
            .min_by_key(|l| l.new_no.abs_diff(c.line))
        {
            return Some(l.key.clone());
        }
    }
    here.iter()
        .find(|l| l.new_no == c.line)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        match r.pointer("/fields/status/value").and_then(Value::as_str) {
            Some("draft") => Status::Draft,
            Some("changes-planned") => Status::ChangesPlanned,
            Some("needs-review") => Status::NeedsReview,
            Some("needs-revision") => Status::NeedsRevision,
            Some("accepted") => Status::Accepted,
            Some("published") => Status::Published,
            Some("abandoned") => Status::Abandoned,
            _ => Status::Unknown,
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
/// summary, and its PHID, which the Phabricator line over a patch's message
/// reads the revision's latest diff by; and when it was last modified
/// (`dateModified`, seconds since 1970), when the search says, which the
/// line reads the rest of the revision again by.
#[derive(Debug, Clone, PartialEq)]
pub struct Searched {
    pub status: Status,
    pub title: String,
    pub summary: String,
    pub phid: String,
    pub modified: Option<i64>,
}

/// The statuses of revisions `ids` (`D123`), Conduit called with `call`:
/// one search for all of them. By revision, those Phabricator has, each
/// with its title, summary and PHID, and when it was last modified.
pub(crate) fn statuses_over(call: &mut Call, ids: &[String]) -> Result<HashMap<String, Searched>, String> {
    let numbers: Vec<u64> = ids.iter().filter_map(|d| d.strip_prefix('D')?.parse().ok()).collect();
    if numbers.is_empty() {
        return Ok(HashMap::new());
    }
    let found = call("differential.revision.search", json!({"constraints": {"ids": numbers}}))?;
    let text = |r: &Value, k: &str| r.pointer(&format!("/fields/{k}")).and_then(Value::as_str).unwrap_or_default().to_string();
    Ok(found
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let searched = Searched {
                status: Status::of(r),
                title: text(r, "title"),
                summary: text(r, "summary"),
                phid: r.get("phid").and_then(Value::as_str).unwrap_or_default().to_string(),
                modified: r.pointer("/fields/dateModified").and_then(Value::as_i64),
            };
            Some((format!("D{}", r.get("id")?.as_u64()?), searched))
        })
        .collect())
}

/// What the Phabricator line over a patch's message reads of its revision
/// beyond what the statuses' search says: the id of its latest diff, and
/// that diff, raw; its comments, each as it was last edited, what it was
/// before kept with it, as a pull reads them (`comments_of`), but for their
/// authors and the lines they are on, which are not read; its parents,
/// each its PHID and its `D123`, by PHID (`parents_named`); and when the
/// revision was last modified as its diff and comments were read, as the
/// statuses' search said it (`Searched::modified`): none when that is not
/// known, or what was read is not to be taken as the revision's since.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Latest {
    pub diff: u64,
    pub raw: String,
    pub comments: Vec<Remote>,
    pub parents: Vec<(String, String)>,
    pub modified: Option<i64>,
}

/// A revision whose latest diff, comments and parents the Phabricator line
/// reads (`latests`): its `D123`, its link and its PHID, and when it was
/// last modified, as the statuses' search found it; what was read of it
/// before, if anything, which is not read again while the revision was not
/// modified since, its parents aside, and its raw diff not while it is the
/// latest; and whether the parents it has are to be named, as a submit
/// would set it one (not at the stack's bottom, say), so that the view of
/// what differs names them.
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
    let mut out: HashMap<String, Vec<String>> = phids.iter().map(|p| (p.clone(), Vec::new())).collect();
    let mut after: Option<String> = None;
    loop {
        let mut params = json!({"sourcePHIDs": phids, "types": ["revision.parent"]});
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
/// `fields.revisionPHID`): differential.diff.search for all of them, the
/// newest first, page after page until each has its newest, or no page is
/// left. None for a revision with no diff.
pub(crate) fn newest_diffs(call: &mut Call, phids: &[String]) -> Result<HashMap<String, u64>, String> {
    let mut out: HashMap<String, u64> = HashMap::new();
    let mut after: Option<String> = None;
    loop {
        let mut params = json!({"constraints": {"revisionPHIDs": phids}, "order": "newest"});
        if let Some(a) = &after {
            params["after"] = a.clone().into();
        }
        let r = call("differential.diff.search", params)?;
        for d in r.get("data").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), Some(rev)) = (d.get("id").and_then(Value::as_u64), d.pointer("/fields/revisionPHID").and_then(Value::as_str)) else { continue };
            let newest = out.entry(rev.to_string()).or_insert(id);
            *newest = (*newest).max(id);
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
/// them; the others, of those whose parents are to be named, in one search,
/// if there are any. One it does not find, or a search that fails, leaves
/// a parent unnamed.
fn parents_named(call: &mut Call, wanted: &[&Wanted], parents: &HashMap<String, Vec<String>>) -> HashMap<String, String> {
    let mut names: HashMap<String, String> = wanted.iter().map(|w| (w.phid.clone(), w.d.clone())).collect();
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
/// read, its latest diff, raw, unless it is the one read before, and every
/// transaction of the revision, for its comments, `got` told it as soon as
/// it is read; for each other, what was read of it before, its parents
/// as they are now. A revision with no diff is passed over. Up to the
/// first call that fails, and what is wrong with it.
pub(crate) fn latests_over(call: &mut Call, wanted: &[&Wanted], got: &mut dyn FnMut(&str, Latest)) -> Result<(), String> {
    let phids: Vec<String> = wanted.iter().map(|w| w.phid.clone()).collect();
    let parents = parents_over(call, &phids)?;
    let diffs = newest_diffs(call, &phids)?;
    let names = parents_named(call, wanted, &parents);
    for w in wanted {
        let Some(&diff) = diffs.get(&w.phid) else { continue };
        let named = |p: &String| (p.clone(), names.get(p).cloned().unwrap_or_else(|| p.clone()));
        let parents = parents.get(&w.phid).into_iter().flatten().map(named).collect();
        // Not modified since it was read, its latest diff the same: as it
        // was read, but for its parents, which change from the other side
        // too, when another revision is edited to take it as its child.
        let unchanged = |h: &&Latest| h.diff == diff && h.modified.is_some() && h.modified == w.modified;
        if let Some(had) = w.had.as_ref().filter(unchanged) {
            got(&w.link, Latest { parents, ..had.clone() });
            continue;
        }
        let raw = match &w.had {
            Some(h) if h.diff == diff => h.raw.clone(),
            _ => call("differential.getrawdiff", json!({"diffID": diff}))?.as_str().unwrap_or_default().to_string(),
        };
        let txns = transactions(call, &w.d)?;
        let comments = comments_of(&txns, &HashMap::new(), |_, _, _| None);
        got(&w.link, Latest { diff, raw, comments, parents, modified: w.modified });
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

/// What answers Conduit's calls in place of Phabricator for the editors of a
/// database (`statuses_stand_in`): the tests', which never reach it. Each
/// call, a method and its parameters; its `result`, or what went wrong, as
/// `Conduit::call` says it, Phabricator turning this machine away too.
pub type StandIn = Box<dyn FnMut(&str, Value) -> Result<Value, Error> + Send>;

/// A stand-in for Phabricator, shared with the read under way.
type StatusStandIn = std::sync::Arc<std::sync::Mutex<StandIn>>;

/// What answers the statuses' searches of the editors of a database in
/// place of Phabricator, by the database's path: the tests', which never
/// reach it. For the editors of that database alone, not for the whole
/// process as a submit's stand-in is: every editor reads the statuses of
/// the stack it shows, so one for the process would answer the editors of
/// every test at once.
static STATUS_STAND_INS: std::sync::Mutex<Vec<(std::path::PathBuf, StatusStandIn)>> = std::sync::Mutex::new(Vec::new());

/// Has `p` answer the statuses' searches of the editors of the database at
/// `db` rather than Phabricator; with none, Phabricator again.
pub fn statuses_stand_in(db: &Path, p: Option<StandIn>) {
    let mut all = STATUS_STAND_INS.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|(at, _)| at != db);
    if let Some(p) = p {
        all.push((db.to_path_buf(), std::sync::Arc::new(std::sync::Mutex::new(p))));
    }
}

/// The stand-in answering for the editors of the database at `db`, if one
/// does.
fn status_stand_in(db: &Path) -> Option<StatusStandIn> {
    let all = STATUS_STAND_INS.lock().unwrap_or_else(|e| e.into_inner());
    all.iter().find(|(at, _)| at == db).map(|(_, p)| p.clone())
}

/// Whether the editors of the database at `db` read the statuses of their
/// revisions: from a stand-in set for it, else from Phabricator, but never
/// from a process that keeps nothing, as the tests' own.
pub fn statuses_read(db: &Path) -> bool {
    status_stand_in(db).is_some() || crate::home::get().dir != Path::new(crate::home::NOWHERE)
}

/// What a process that keeps nothing, as the tests' own, says it reads of
/// Phabricator without a stand-in.
const NOT_REACHED: &str = "Phabricator is not reached from a process that keeps nothing, as the tests' own";

/// What `f` comes to, Conduit called with what it is given for the editors
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
            if away.is_none() && matches!(e, Error::TurnedAway(_)) {
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
    // By Phabricator, as `pull` reads the comments.
    let mut out = HashMap::new();
    for these in by_host(revisions, |(_, link)| link.as_str()) {
        let ids: Vec<String> = these.iter().map(|(d, _)| d.clone()).collect();
        let found = calling(db, &these[0].1, |call| statuses_over(call, &ids))?;
        for (d, link) in these {
            if let Some(s) = found.get(d) {
                out.insert(link.clone(), s.clone());
            }
        }
    }
    Ok(out)
}

/// What the Phabricator lines over the patches' messages read of revisions
/// `wanted`, beyond their search: the latest diff of each, raw, but when it
/// is the one read before; its comments; and its parents, named; of a
/// revision not modified since it was read, its parents alone
/// (`latests_over`): `got` told each revision, by its link, as soon as it
/// is read, in the order given. The calls of each Phabricator, as the
/// revisions' links name it, one after another. Through the stand-in set
/// for the database at `db`, as the statuses are read, else over Conduit;
/// never from a process that keeps nothing. What is wrong, said, when a
/// call fails, Phabricator turning this machine away as such: no call is
/// made after it.
pub fn latests(db: &Path, wanted: &[Wanted], got: &mut dyn FnMut(&str, Latest)) -> Result<(), Error> {
    for these in by_host(wanted, |w| w.link.as_str()) {
        calling(db, &these[0].link, |call| latests_over(call, &these, got))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revisions() -> Vec<(String, String, String)> {
        ["D1", "D2", "D3"].iter().map(|d| (format!("c{d}"), d.to_string(), format!("https://p.example/{d}"))).collect()
    }

    fn none_said(source: &str, rev: &str) -> Result<RevisionComments, String> {
        Ok(RevisionComments {
            revision: rev.into(),
            source: source.into(),
            comments: vec![],
        })
    }

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
        let got = comments_of(&txns, &names, |path, diff, line| Some(format!("{path}@{diff}:{line}")));
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
        let got = comments_of(&txns, &names, |_, _, _| None);
        assert_eq!((got[0].phid.as_str(), got[0].text.as_str(), got[0].earlier.clone()), ("v2", "Fine!", vec![v("v1", "Fine.")]));
    }

    #[test]
    fn a_stack_of_revisions_is_read_down_while_open_and_up_while_one_child_is() {
        // D10 landed; D11 on it, D12 on D11, D13 on D12: read from D12.
        let rev = |n: u64, status: &str, title: &str| {
            json!({"id": n, "phid": format!("PHID-DREV-{n}"), "fields": {"title": title, "summary": "Why.", "status": {"value": status}}})
        };
        let diff = |n: u64| format!("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-{n}\n+{}\n", n + 1);
        let mut calls = Vec::new();
        let mut call = |method: &str, params: Value| -> Result<Value, String> {
            calls.push(method.to_string());
            let first = |k: &str| params.pointer(k).and_then(|v| v.get(0)).cloned().unwrap_or(Value::Null);
            Ok(match method {
                "differential.revision.search" => {
                    let n = match first("/constraints/ids").as_u64() {
                        Some(n) => n,
                        None => first("/constraints/phids").as_str().unwrap().trim_start_matches("PHID-DREV-").parse().unwrap(),
                    };
                    let status = if n == 10 { "published" } else { "needs-review" };
                    json!({"data": [rev(n, status, &format!("Bug {n}"))]})
                }
                "edge.search" => {
                    let n: u64 = first("/sourcePHIDs").as_str().unwrap().trim_start_matches("PHID-DREV-").parse().unwrap();
                    let to = match first("/types").as_str().unwrap() {
                        "revision.parent" => n - 1,
                        _ if n < 13 => n + 1,
                        _ => return Ok(json!({"data": []})),
                    };
                    json!({"data": [{"destinationPHID": format!("PHID-DREV-{to}")}]})
                }
                // Two diffs of D11, the latest read; D11's commit as uploaded,
                // the others' none.
                "differential.querydiffs" => match first("/revisionIDs").as_u64().unwrap() {
                    11 => json!({
                        "101": {"sourceControlBaseRevision": "aaa", "properties": {}},
                        "111": {"sourceControlBaseRevision": "bbb", "properties": {"local:commits": {"c1": {
                            "author": "Ann", "authorEmail": "ann@example.com",
                            "message": "Bug 11 - as committed\n\nDifferential Revision: https://p.example/D11"
                        }}}}
                    }),
                    n => json!({ (n * 10 + 2).to_string(): {"sourceControlBaseRevision": "ccc", "properties": []} }),
                },
                "differential.getrawdiff" => Value::String(diff(params["diffID"].as_u64().unwrap())),
                m => return Err(format!("not as Phabricator is asked: {m}")),
            })
        };
        let revs = stack_over(&mut call, "https://p.example/api/", "Differential Revision", "D12").unwrap();
        assert_eq!(revs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["D11", "D12", "D13"]);
        assert_eq!(
            revs[0],
            Revision {
                id: "D11".into(),
                message: "Bug 11 - as committed\n\nDifferential Revision: https://p.example/D11".into(),
                author: Some("Ann <ann@example.com>".into()),
                diff: diff(111),
                base: Some("bbb".into()),
            }
        );
        // No commit as uploaded: the revision's title and summary, and its
        // trailer.
        assert_eq!(revs[1].message, "Bug 12\n\nWhy.\n\nDifferential Revision: https://p.example/D12");
        assert_eq!((revs[1].author.clone(), revs[1].diff.clone()), (None, diff(122)));
        assert!(calls.contains(&"edge.search".to_string()));
        // A revision that is not there: said.
        let mut none = |_: &str, _: Value| -> Result<Value, String> { Ok(json!({"data": []})) };
        assert_eq!(stack_over(&mut none, "https://p.example/api/", "Differential Revision", "D9"), Err("no revision D9".to_string()));
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
    fn each_revision_is_told_as_it_starts_being_read_up_to_one_that_fails() {
        let mut told = Vec::new();
        let pulled = read_each(&revisions(), &mut |k| told.push(k), |source, rev, _| none_said(source, rev)).unwrap();
        assert_eq!(told, [0, 1, 2]);
        assert_eq!(pulled.revisions.iter().map(|r| r.revision.as_str()).collect::<Vec<_>>(), ["D1", "D2", "D3"]);
        // One fails: none is read after it, and what is wrong is said.
        let mut told = Vec::new();
        let pulled = read_each(&revisions(), &mut |k| told.push(k), |source, rev, _| {
            if rev == "D2" { Err("transaction.search: D2 is gone".into()) } else { none_said(source, rev) }
        });
        assert_eq!(told, [0, 1]);
        assert_eq!(pulled, Err("transaction.search: D2 is gone".to_string()));
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
            Ok(json!({"data": [d12, revision(11, Some("changes-planned"), true)]}))
        };
        let ids = ["D11", "D12", "D13"].map(String::from);
        let found = statuses_over(&mut call, &ids).unwrap();
        assert_eq!(asked, [("differential.revision.search".to_string(), json!({"constraints": {"ids": [11, 12, 13]}}))]);
        // D13, not given: not known. Each with its title, summary and PHID,
        // and when it was last modified, none when the search gives none.
        let searched = |status: Status, title: &str, summary: &str, n: u64, modified: Option<i64>| Searched {
            status,
            title: title.into(),
            summary: summary.into(),
            phid: format!("PHID-DREV-{n}"),
            modified,
        };
        let want: HashMap<String, Searched> = [
            ("D11".to_string(), searched(Status::ChangesPlanned, "", "", 11, None)),
            ("D12".to_string(), searched(Status::Accepted, "Bug 12 - twelve", "Why.", 12, Some(1_700_000_012))),
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
        let diff = |id: u64, of: u64| json!({"id": id, "phid": format!("PHID-DIFF-{id}"), "fields": {"revisionPHID": format!("PHID-DREV-{of}")}});
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
    fn a_call_turned_away_says_http_429_and_how_long_phabricator_asks_to_wait() {
        let tmp = tempfile::tempdir().unwrap();
        let away = |retry_after: &str| format!("HTTP/1.1 429 Too Many Requests\r\n{retry_after}Content-Length: 0\r\n\r\n");
        // Two minutes asked, the page empty: its own kind of error, which
        // says HTTP 429 plainly.
        let e = answer_of(tmp.path(), "conduit.ping", &away("Retry-After: 120\r\n")).unwrap_err();
        assert_eq!(e, Error::TurnedAway(Some(Duration::from_secs(120))));
        assert_eq!(e.to_string(), "Phabricator is turning away requests from this machine for now (HTTP 429, too many requests)");
        assert_eq!(String::from(e), TURNED_AWAY);
        // Nothing asked, or no time to come: turned away all the same.
        assert_eq!(answer_of(tmp.path(), "conduit.ping", &away("")), Err(Error::TurnedAway(None)));
        assert_eq!(answer_of(tmp.path(), "conduit.ping", &away("retry-after: 0\r\n")), Err(Error::TurnedAway(None)));
        // As HTTP/2 writes it, after an answer that says to go on.
        let continued = "HTTP/1.1 100 Continue\r\n\r\nHTTP/2 429 \r\nretry-after: 30\r\n\r\n";
        assert_eq!(answer_of(tmp.path(), "conduit.ping", continued), Err(Error::TurnedAway(Some(Duration::from_secs(30)))));
        // A Retry-After that is a date: until then.
        let at = 1_445_412_480;
        assert_eq!(wait_asked("Wed, 21 Oct 2015 07:28:00 GMT", at - 90), Some(Duration::from_secs(90)));
        assert_eq!(wait_asked("Tue, 29 Feb 2000 23:59:59 GMT", 951_868_799 - 1), Some(Duration::from_secs(1)));
        assert_eq!(wait_asked("Sun, 01 Mar 2026 00:00:00 GMT", 1_772_323_200 - 3600), Some(Duration::from_secs(3600)));
        for gone in ["Wed, 21 Oct 2015 07:28:00 GMT", "Thu, 01 Jan 1970 00:00:00 GMT"] {
            assert_eq!(wait_asked(gone, at), None, "{gone}");
        }
        for wrong in ["soon", "", "-5", "Wed, 21 Oct 2015 07:28 GMT", "Wed, 21 Foo 2015 07:28:00 GMT", "Wed, 21 Oct 2015 07:28:00 CET"] {
            assert_eq!(wait_asked(wrong, 0), None, "{wrong:?}");
        }
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
        assert_eq!(ping(), Err(Error::TurnedAway(Some(Duration::from_secs(120)))));
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
        // Phabricator turning this machine away, asking for two minutes.
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        statuses_stand_in(
            &db,
            Some(Box::new(move |m, _| {
                seen.lock().unwrap().push(m.to_string());
                Err(Error::TurnedAway(Some(Duration::from_secs(120))))
            })),
        );
        let revisions = [("D11".to_string(), "https://p.example/D11".to_string())];
        assert_eq!(statuses(&db, &revisions), Err(Error::TurnedAway(Some(Duration::from_secs(120)))));
        // The lines' read, though its calls take what went wrong as text,
        // stopped at its first call.
        let all = [wanted(11, false, None)];
        assert_eq!(latests(&db, &all, &mut |_, _| panic!("nothing read")), Err(Error::TurnedAway(Some(Duration::from_secs(120)))));
        assert_eq!(*asked.lock().unwrap(), ["differential.revision.search", "edge.search"]);
        // Another error is said as it was.
        statuses_stand_in(&db, Some(Box::new(|m, _| Err(format!("{m}: Phabricator is down").into()))));
        assert_eq!(statuses(&db, &revisions), Err(Error::Said("differential.revision.search: Phabricator is down".into())));
        statuses_stand_in(&db, None);
    }
}
