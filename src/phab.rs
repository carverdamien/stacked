//! Reviewers' comments from Phabricator: each patch's revision (its
//! `Differential Revision:` trailer) read over Conduit with the token
//! moz-phab and arc keep in `~/.arcrc`, or the one the settings give.
//! Taken from self-review's, less what files them in its sessions:
//! `review::pull` files them in the database.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use crate::view::{NoteLine, fold};

/// A Conduit endpoint and the token for it.
pub struct Conduit {
    pub api: String,
    token: String,
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

    /// Calls a Conduit method; its `result`.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut params = params;
        params["__conduit__"] = json!({"token": self.token});
        let out = Command::new("curl")
            .args(["-sS", "--max-time", "30", "-X", "POST"])
            .arg(format!("{}{method}", self.api))
            .args(["--data-urlencode", &format!("params={params}")])
            .args(["--data", "output=json", "--data", "__conduit__=1"])
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{method}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let v: Value = serde_json::from_slice(&out.stdout)
            .map_err(|_| format!("{method}: not JSON from {}", self.api))?;
        if let Some(e) = v.get("error_info").and_then(Value::as_str) {
            return Err(format!("{method}: {e}"));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
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

/// Reads every comment on `revision`.
pub fn fetch(conduit: &Conduit, revision: &str, source: &str) -> Result<RevisionComments, String> {
    let mut txns: Vec<Value> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut params = json!({"objectIdentifier": revision, "limit": 100});
        if let Some(a) = &after {
            params["after"] = a.clone().into();
        }
        let r = conduit.call("transaction.search", params)?;
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
    stack_over(&mut |method, params| conduit.call(method, params), &conduit.api, &trailer, rev)
}

/// How Conduit is called: a method, its parameters; its `result`.
type Call<'a> = dyn FnMut(&str, Value) -> Result<Value, String> + 'a;

/// Whether revision `r`, as differential.revision.search gives it, is
/// open: neither landed nor abandoned.
fn open(r: &Value) -> bool {
    !matches!(r.pointer("/fields/status/value").and_then(Value::as_str), Some("published" | "abandoned"))
}

/// The first revision `constraints` find, as differential.revision.search
/// gives it.
fn revision_by(call: &mut Call, constraints: Value) -> Result<Option<Value>, String> {
    let r = call("differential.revision.search", json!({ "constraints": constraints }))?;
    Ok(r.pointer("/data/0").cloned())
}

/// Where the edges of kind `kind` (`revision.parent`, `revision.child`)
/// from revision `phid` go: their PHIDs.
fn edges(call: &mut Call, phid: &str, kind: &str) -> Result<Vec<String>, String> {
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
/// free to name another revision.
fn revision_in(message: &str, trailer: &str, url: Option<&str>) -> Option<(String, String)> {
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
}
