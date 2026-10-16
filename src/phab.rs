//! Reviewers' comments from Phabricator: each patch's revision (its
//! `Differential Revision:` trailer) read over Conduit with the token
//! moz-phab and arc keep in `~/.arcrc`. Taken from self-review's, less what
//! files them in its sessions: `review::pull` files them in the database.

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

fn arcrc() -> Option<Value> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(Path::new(&home).join(".arcrc")).ok()?;
    serde_json::from_str(&text).ok()
}

impl Conduit {
    /// The endpoint for `url` (a revision's link), with its token from
    /// ~/.arcrc; the only host there when `url` names none.
    pub fn for_url(url: Option<&str>) -> Result<Conduit, String> {
        let rc = arcrc().ok_or("no ~/.arcrc: log in with moz-phab (or arc) first")?;
        let hosts = rc
            .get("hosts")
            .and_then(Value::as_object)
            .ok_or("~/.arcrc has no hosts")?;
        let host_of = |u: &str| {
            u.split("://")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .map(String::from)
        };
        let want = url.and_then(host_of);
        let pick = hosts
            .iter()
            .find(|(api, _)| want.is_none() || host_of(api) == want)
            .or_else(|| (hosts.len() == 1).then(|| hosts.iter().next().unwrap()));
        let Some((api, v)) = pick else {
            return Err(format!(
                "no token for {} in ~/.arcrc",
                want.unwrap_or_default()
            ));
        };
        let token = v
            .get("token")
            .and_then(Value::as_str)
            .ok_or("~/.arcrc has no token for that host")?;
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

/// A comment as Phabricator has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    pub phid: String,
    pub author: String,
    pub text: String,
    pub created: i64,
    /// None for a comment on the revision as a whole.
    pub path: Option<String>,
    pub line: usize,
    /// The line's text in the diff it was made on, when that is known.
    pub line_text: Option<String>,
    pub reply_to: Option<String>,
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
    let mut comments = Vec::new();
    for t in &txns {
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
            (Some(p), Some(d)) => diffs
                .entry(d)
                .or_insert_with(|| diff_lines(conduit, d))
                .get(&(p.clone(), line))
                .cloned(),
            _ => None,
        };
        let reply_to = fields
            .get("replyToCommentPHID")
            .and_then(Value::as_str)
            .map(String::from);
        for c in t
            .get("comments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if c.get("removed").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let text = c
                .pointer("/content/raw")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            comments.push(Remote {
                phid: c.get("phid").and_then(Value::as_str).unwrap_or("").into(),
                author: author.clone(),
                text,
                created: c.get("dateCreated").and_then(Value::as_i64).unwrap_or(0),
                path: path.clone(),
                line,
                line_text: line_text.clone(),
                reply_to: reply_to.clone(),
            });
            break;
        }
    }
    comments.sort_by_key(|c| c.created);
    Ok(RevisionComments {
        revision: revision.into(),
        source: source.into(),
        comments,
    })
}

/// Reads the comments of every commit's revision: `revisions` is
/// `(source key, D123, link)`.
pub fn pull(revisions: &[(String, String, String)]) -> Result<Pulled, String> {
    let mut out = Pulled::default();
    let mut conduits: HashMap<String, Conduit> = HashMap::new();
    for (source, rev, url) in revisions {
        let host = url.split('/').take(3).collect::<Vec<_>>().join("/");
        if !conduits.contains_key(&host) {
            conduits.insert(host.clone(), Conduit::for_url(Some(url))?);
        }
        out.revisions.push(fetch(&conduits[&host], rev, source)?);
    }
    Ok(out)
}

/// The key of the line a comment is on: the line with its text near its
/// number, else the line at its number, else None.
pub fn anchor(lines: &[NoteLine], c: &Remote) -> Option<String> {
    let path = c.path.as_deref()?;
    let here: Vec<&NoteLine> = lines
        .iter()
        .filter(|l| l.file == path && l.origin != '-')
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

/// A patch's revision, `D123`, and the link its trailer gives.
pub fn revision_of(message: &str) -> Option<(String, String)> {
    message.lines().find_map(|line| {
        let rest = line.strip_prefix("Differential Revision:")?.trim();
        let (_, last) = rest.rsplit_once('/')?;
        let digits = last.strip_prefix('D')?;
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| (last.to_string(), rest.to_string()))
    })
}
