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

/// A patch's revision, `D123`, and the link its trailer gives: the
/// settings' trailer (`Differential Revision:`), a bare `D123` on the
/// settings' Phabricator.
pub fn revision_of(message: &str) -> Option<(String, String)> {
    let cfg = &crate::config::get().reviews;
    let trailer = format!("{}:", cfg.trailer);
    message.lines().find_map(|line| {
        let rest = line.strip_prefix(&trailer)?.trim();
        let last = rest.rsplit('/').next()?;
        let digits = last.strip_prefix('D')?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let link = if rest.contains('/') {
            rest.to_string()
        } else {
            format!("{}/{last}", cfg.url.as_deref()?)
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
}
