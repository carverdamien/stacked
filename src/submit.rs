//! A patch of a stack sent to Phabricator, one patch at a time, as
//! moz-phab's `submit --wip` sends a single commit: as a new revision, or
//! as a new diff of the revision its trailer (`Differential Revision:`)
//! names, in Changes Planned, the status of work in progress. Built with
//! the `moz-phab` feature.
//!
//! Nothing is sent before what will be is read and checked (`prepare`): a
//! revision that is not in work is refused, and so is one whose latest diff
//! is no version of the patch the database holds, as it came from
//! elsewhere. Then only what changed is sent (`send`): a new diff and its
//! commit's author and message, the title, the summary and the bug, the
//! status kept or put in Changes Planned; after that, each in a call of its
//! own, so that a refusal undoes none of the rest, the parent set to the
//! revision of the patch below, and the reviewers to those the title names.
//! A new revision's link goes into the patch, as a new version of it, at
//! once. The workspace's commits are not touched: the sync of commit
//! messages takes the link there.

use std::path::Path;
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::check::Left;
use crate::db::{Db, Patch, Stack};
use crate::phab::Call;

/// What answers the calls a submit makes in place of Phabricator: the
/// tests', which never reach it. Each call, a method and its parameters;
/// its `result`.
pub type Phabricator = Box<dyn FnMut(&str, Value) -> Result<Value, String> + Send>;

static STAND_IN: Mutex<Option<Phabricator>> = Mutex::new(None);

/// Has `p` answer the calls a submit makes rather than Phabricator; with
/// none, Phabricator again.
pub fn stand_in(p: Option<Phabricator>) {
    *STAND_IN.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

/// Phabricator as a submit calls it: the stand-in, when one is set; else
/// over Conduit.
enum Via {
    StandIn,
    Conduit(crate::phab::Conduit),
}

impl Via {
    /// Phabricator at `url` (a revision's link, or the checkout's
    /// `phabricator.uri`), else the settings'. Never from a process that
    /// keeps nothing, as the tests' own: there, only a stand-in answers.
    fn to(url: Option<&str>) -> Result<Via, String> {
        if STAND_IN.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            return Ok(Via::StandIn);
        }
        if crate::home::get().dir == Path::new(crate::home::NOWHERE) {
            return Err("Phabricator is not reached from a process that keeps nothing, as the tests' own".into());
        }
        crate::phab::Conduit::for_url(url).map(Via::Conduit)
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        match self {
            Via::StandIn => match STAND_IN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                Some(p) => p(method, params),
                None => Err(format!("{method}: Phabricator's stand-in is gone")),
            },
            Via::Conduit(c) => c.call(method, params),
        }
    }

    /// Where its API is, when it is Conduit's.
    fn api(&self) -> Option<&str> {
        match self {
            Via::StandIn => None,
            Via::Conduit(c) => Some(&c.api),
        }
    }
}

// -- the title, as moz-phab reads it -------------------------------------- //

/// A word character, as Python's `re` reads `\w` in a pattern of text.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// moz-phab's IRC_NICK_CHARS: `a-zA-Z0-9\-\_!`.
fn nick_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '!')
}

/// moz-phab's IRC_NICK at `i` of `s`, `#?[nick.]*[nick]+`, as far as it
/// goes: where it ends.
fn nick_end(s: &[char], i: usize) -> Option<usize> {
    let start = if s.get(i) == Some(&'#') { i + 1 } else { i };
    let mut end = start;
    while end < s.len() && (nick_char(s[end]) || s[end] == '.') {
        end += 1;
    }
    while end > start && s[end - 1] == '.' {
        end -= 1;
    }
    (end > start).then_some(end)
}

/// moz-phab's LIST at `i` of `s`, `[;,\/\\]\s*`: where it ends.
fn list_end(s: &[char], i: usize) -> Option<usize> {
    if !matches!(s.get(i), Some(';' | ',' | '/' | '\\')) {
        return None;
    }
    let mut end = i + 1;
    while end < s.len() && s[end].is_whitespace() {
        end += 1;
    }
    Some(end)
}

/// Whether another specifier starts at `i` of `s`, as `sr=` does: what
/// moz-phab's list of names looks ahead for, `[a-z0-9.\-]+[=?]`, to stop
/// before it.
fn specifier_at(s: &[char], i: usize) -> bool {
    let mut end = i;
    while end < s.len() && (s[end].is_ascii_lowercase() || s[end].is_ascii_digit() || matches!(s[end], '.' | '-')) {
        end += 1;
    }
    end > i && matches!(s.get(end), Some('=' | '?'))
}

/// A match of moz-phab's REVIEWERS_RE in a title: where it starts (on the
/// character before `r`), where it ends, and where its names are, if it
/// has any.
struct Spec {
    start: usize,
    end: usize,
    names: Option<(usize, usize)>,
}

/// The matches of moz-phab's REVIEWERS_RE for the specifiers `kinds` (`?`
/// asks for a review, `=` says it was given) in `s`, as Python's
/// `finditer` finds them: `([\s(.\[;,])(r[kinds])(NICK(?:LIST(?!...)NICK)*)?`.
fn specs(s: &[char], kinds: &[char]) -> Vec<Spec> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < s.len() {
        let lead = s[i].is_whitespace() || matches!(s[i], '(' | '.' | '[' | ';' | ',');
        if !(lead && s[i + 1] == 'r' && kinds.contains(&s[i + 2])) {
            i += 1;
            continue;
        }
        let from = i + 3;
        let names = nick_end(s, from).map(|mut end| {
            while let Some(l) = list_end(s, end)
                && !specifier_at(s, l)
                && let Some(e) = nick_end(s, l)
            {
                end = e;
            }
            (from, end)
        });
        let end = names.map_or(from, |(_, e)| e);
        out.push(Spec { start: i, end, names });
        i = end;
    }
    out
}

/// The names of a match, `s[from..to]`, split as moz-phab splits them, at
/// each LIST.
fn split_names(s: &[char], (from, to): (usize, usize)) -> Vec<String> {
    let names = &s[from..to];
    let mut out = Vec::new();
    let mut name = String::new();
    let mut i = 0;
    while i < names.len() {
        match list_end(names, i) {
            Some(e) => {
                out.push(std::mem::take(&mut name));
                i = e;
            }
            None => {
                name.push(names[i]);
                i += 1;
            }
        }
    }
    out.push(name);
    out
}

/// moz-phab's fix of a common typo, before it reads a title: `r!a,b` read
/// as `r=a!,b!`, each blocking (`morph_blocking_reviewers`).
fn morph_blocking(t: &str) -> String {
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if (i == 0 || !word(c[i - 1])) && c[i] == 'r' && c.get(i + 1) == Some(&'!') {
            let mut j = i + 2;
            while j < c.len() && (nick_char(c[j]) || matches!(c[j], '.' | ',')) {
                j += 1;
            }
            if j > i + 2 {
                let mut nick: String = c[i + 2..j].iter().collect();
                let suffix = match nick.ends_with([',', '.']) {
                    true => nick.pop().map(String::from).unwrap_or_default(),
                    false => String::new(),
                };
                let names: Vec<String> = nick.split(',').map(|n| format!("{}!", n.trim_end_matches('!'))).collect();
                out.push_str(&format!("r={}{suffix}", names.join(",")));
                i = j;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// `t` with its marks taken out, as moz-phab takes out the matches it
/// replaced after the first: each `\0`, the separator before it with it
/// (`LIST + "\0"`).
fn without_marks(t: &str) -> String {
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if let Some(e) = list_end(&c, i)
            && c.get(e) == Some(&'\0')
        {
            i = e + 1;
            continue;
        }
        if c[i] != '\0' {
            out.push(c[i]);
        }
        i += 1;
    }
    out
}

/// Title `t` with its reviewers written again as moz-phab writes them
/// (`replace_reviewers`, from vct's commitparser): `r?` those asked, `r=`
/// those who gave it, in the place of the first specifier, the others taken
/// out; at its end, when it has none.
fn replace_reviewers(t: &str, request: &[String], granted: &[String]) -> String {
    let mut lists = Vec::new();
    if !request.is_empty() {
        lists.push(format!("r?{}", request.join(",")));
    }
    if !granted.is_empty() {
        lists.push(format!("r={}", granted.join(",")));
    }
    let said = lists.join(" ");
    let c: Vec<char> = t.chars().collect();
    let specified = (0..c.len().saturating_sub(1)).any(|j| c[j] == 'r' && matches!(c[j + 1], '=' | '?') && (j == 0 || !word(c[j - 1])));
    if !specified {
        return format!("{t} {said}").trim().to_string();
    }
    let mut out = String::new();
    let mut at = 0;
    for (k, m) in specs(&c, &['=', '?']).into_iter().enumerate() {
        out.extend(&c[at..m.start]);
        if k == 0 {
            out.push(c[m.start]);
            out.push_str(&said);
        } else {
            out.push('\0');
        }
        at = m.end;
    }
    out.extend(&c[at..]);
    without_marks(&out).trim().to_string()
}

/// moz-phab's BUG_ID_RE, `(?:(?:bug|b=)(?:\s*)(\d+)(?=\b))` in any case: its
/// first match in `s`, where it starts and ends, and the bug's number.
fn bug_in(s: &[char]) -> Option<(usize, usize, String)> {
    for i in 0..s.len() {
        let low = |k: usize| s.get(i + k).map(char::to_ascii_lowercase);
        let after = if (low(0), low(1), low(2)) == (Some('b'), Some('u'), Some('g')) {
            i + 3
        } else if low(0) == Some('b') && s.get(i + 1) == Some(&'=') {
            i + 2
        } else {
            continue;
        };
        let mut j = after;
        while j < s.len() && s[j].is_whitespace() {
            j += 1;
        }
        let digits = j;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > digits && s.get(j).is_none_or(|c| !word(*c)) {
            return Some((i, j, s[digits..j].iter().collect()));
        }
    }
    None
}

/// What moz-phab makes of a commit's title (its message's first line) to
/// submit it as work in progress: the revision's title, the bug, the
/// reviewers it names.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Titled {
    /// `WIP: ` and the title, its reviewers and its bug written as
    /// moz-phab writes them (`revision_title`).
    pub title: String,
    /// From `Bug 123` (or `b=123`): the first.
    pub bug: Option<String>,
    /// As written after `r?` then `r=`: `alice`, `bob!` (blocking),
    /// `#build` (a group).
    pub reviewers: Vec<String>,
}

/// Title `first` as moz-phab's `submit --wip` reads it, and what it sends
/// of it: the typo `r!` fixed, the reviewers asked (`r?`) and given (`r=`)
/// read, then written again as it writes them, the first bug said `Bug N`,
/// and `WIP: ` before it all, in place of one written already.
pub fn wip_title(first: &str) -> Titled {
    let t = morph_blocking(first);
    let c: Vec<char> = t.chars().collect();
    let names = |kind: char| -> Vec<String> {
        specs(&c, &[kind])
            .iter()
            .filter_map(|m| m.names)
            .flat_map(|r| split_names(&c, r))
            .collect()
    };
    let (request, granted) = (names('?'), names('='));
    let bug = bug_in(&c).map(|(_, _, b)| b);
    let preview = replace_reviewers(&t, &request, &granted);
    let preview = match &bug {
        Some(b) => {
            let p: Vec<char> = preview.chars().collect();
            match bug_in(&p) {
                Some((i, j, _)) => format!("{}Bug {b}{}", p[..i].iter().collect::<String>(), p[j..].iter().collect::<String>()),
                None => format!("Bug {b} - {t}"),
            }
        }
        None => preview,
    };
    // `^(?:WIP[: ]|WIP$)`, in any case, taken off.
    let p: Vec<char> = preview.chars().collect();
    let wip = p.len() >= 3 && p[..3].iter().collect::<String>().eq_ignore_ascii_case("wip");
    let rest: String = match (wip, p.get(3)) {
        (true, Some(':' | ' ')) => p[4..].iter().collect(),
        (true, None) => String::new(),
        _ => preview.clone(),
    };
    Titled {
        title: format!("WIP: {}", rest.trim_start()),
        bug,
        reviewers: request.into_iter().chain(granted).collect(),
    }
}

/// Whether `line` names a revision: as moz-phab's ARC_DIFF_REV_RE has it
/// (`Differential Revision: https://host/D123`), or as `revision_of` reads
/// the settings' trailer, `trailer`.
fn names_revision(line: &str, trailer: &str) -> bool {
    let l = line.trim();
    let link = |rest: &str| {
        let last = rest.rsplit('/').next().unwrap_or_default();
        last.strip_prefix('D').is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
    };
    let arc = l.strip_prefix("Differential Revision:").map(str::trim).is_some_and(|r| {
        let host = r.strip_prefix("https://").or_else(|| r.strip_prefix("http://"));
        host.is_some_and(|h| h.split('/').count() == 2 && !h.starts_with('/')) && link(r)
    });
    let theirs = l.strip_prefix(&format!("{trailer}:")).map(str::trim).is_some_and(link);
    arc || theirs
}

/// The summary moz-phab sends of a commit's message, `message`: what
/// follows its title, without the lines naming a revision, which
/// Phabricator refuses in a summary, nor the empty lines around them, one
/// left between what was before and after them; trimmed.
pub fn summary_of(message: &str, trailer: &str) -> String {
    let lines: Vec<&str> = message.split('\n').skip(1).collect();
    let blank = |l: &str| l.trim().is_empty();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !names_revision(lines[i], trailer) {
            out.push(lines[i]);
            i += 1;
            continue;
        }
        while out.last().is_some_and(|l| blank(l)) {
            out.pop();
        }
        i += 1;
        while i < lines.len() && blank(lines[i]) {
            i += 1;
        }
        if !out.is_empty() && i < lines.len() {
            out.push("");
        }
    }
    out.join("\n").trim().to_string()
}

// -- what Phabricator holds ----------------------------------------------- //

/// A revision as a submit reads it, before it sends anything, and again
/// before it does: if any of it but `raw` changed meanwhile, nothing is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Remote {
    /// `D123`.
    pub id: String,
    pub phid: String,
    /// `changes-planned`, as differential.revision.search says it; and as
    /// it names it, `Changes Planned`.
    pub status: String,
    pub status_name: String,
    /// A draft, not yet sent for review.
    pub draft: bool,
    pub title: String,
    pub summary: String,
    /// Its bug, empty for none.
    pub bug: String,
    pub repository: Option<String>,
    /// Its reviewers: each one's PHID, and whether it is blocking; sorted.
    pub reviewers: Vec<(String, bool)>,
    /// Its parents' PHIDs, sorted.
    pub parents: Vec<String>,
    /// Its latest diff: its id, and the diff, raw.
    pub diff: u64,
    pub raw: String,
}

impl Remote {
    /// Whether it is in work, as moz-phab takes it: Changes Planned, or a
    /// draft.
    pub fn in_work(&self) -> bool {
        self.status == "changes-planned" || self.draft
    }

    /// Whether it is as `other` was, its raw diff aside.
    fn same_as(&self, other: &Remote) -> bool {
        Remote { raw: String::new(), ..self.clone() } == Remote { raw: String::new(), ..other.clone() }
    }
}

/// A field of revision `r`, as differential.revision.search gives it, as
/// text: a number written out, none empty.
fn field(r: &Value, k: &str) -> String {
    match r.pointer(&format!("/fields/{k}")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Revision `id` (`D123`) as it is now, its latest diff raw too if `raw`.
fn read_remote(call: &mut Call, id: &str, raw: bool) -> Result<Remote, String> {
    let n: u64 = id.trim_start_matches('D').parse().map_err(|_| format!("{id} names no revision"))?;
    let found = call("differential.revision.search", json!({"constraints": {"ids": [n]}, "attachments": {"reviewers": true}}))?;
    let r = found.pointer("/data/0").cloned().ok_or(format!("Phabricator has no revision {id}"))?;
    let phid = r.get("phid").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut reviewers: Vec<(String, bool)> = r
        .pointer("/attachments/reviewers/reviewers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let p = v.get("reviewerPHID").and_then(Value::as_str)?;
            Some((p.to_string(), v.get("isBlocking").and_then(Value::as_bool).unwrap_or(false)))
        })
        .collect();
    reviewers.sort();
    let mut parents = crate::phab::edges(call, &phid, "revision.parent")?;
    parents.sort();
    let diffs = call("differential.querydiffs", json!({"revisionIDs": [n]}))?;
    let diff = diffs
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, _)| k.parse::<u64>().ok())
        .max()
        .ok_or(format!("{id} has no diff"))?;
    let raw = match raw {
        true => call("differential.getrawdiff", json!({"diffID": diff}))?.as_str().unwrap_or_default().to_string(),
        false => String::new(),
    };
    let repository = r.pointer("/fields/repositoryPHID").and_then(Value::as_str).filter(|p| !p.is_empty()).map(String::from);
    Ok(Remote {
        id: format!("D{n}"),
        phid,
        status: r.pointer("/fields/status/value").and_then(Value::as_str).unwrap_or_default().to_string(),
        status_name: r.pointer("/fields/status/name").and_then(Value::as_str).unwrap_or_default().to_string(),
        draft: r.pointer("/fields/isDraft").and_then(Value::as_bool).unwrap_or(false),
        title: field(&r, "title"),
        summary: field(&r, "summary"),
        bug: field(&r, "bugzilla.bug-id"),
        repository,
        reviewers,
        parents,
        diff,
        raw,
    })
}

/// A status as Phabricator names it: its name, else its value written out.
fn status_said(r: &Remote) -> String {
    if !r.status_name.is_empty() {
        return r.status_name.clone();
    }
    r.status
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| w[..1].to_uppercase() + &w[1..])
        .collect::<Vec<_>>()
        .join(" ")
}

/// The checkout's `.arcconfig`, as moz-phab reads it: the Phabricator it
/// names, and the callsign of its repository there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arc {
    pub uri: Option<String>,
    pub callsign: Option<String>,
}

/// The `.arcconfig` of the repository at `repo`: in its git folder, else
/// at its top, as moz-phab looks; each setting from the first that has it.
pub fn arcconfig(repo: &Path) -> Arc {
    let git_dir = crate::git::git(repo, &["rev-parse", "--absolute-git-dir"]).ok().map(|d| Path::new(d.trim()).join(".arcconfig"));
    let files: Vec<Value> = git_dir
        .into_iter()
        .chain([repo.join(".arcconfig")])
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .filter_map(|t| serde_json::from_str(&t).ok())
        .collect();
    let get = |k: &str| {
        files
            .iter()
            .find_map(|v| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from))
    };
    Arc {
        uri: get("phabricator.uri").or_else(|| get("conduit_uri")),
        callsign: get("repository.callsign"),
    }
}

/// The repository a diff is made in on Phabricator, and what holds it
/// there (`git`, `hg`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Repository {
    pub phid: Option<String>,
    pub vcs: String,
}

/// The repository by `constraint` (`phids`, `callsigns`), as
/// diffusion.repository.search finds it.
fn repository_by(call: &mut Call, constraint: &str, value: &str) -> Result<Repository, String> {
    let r = call("diffusion.repository.search", json!({"constraints": {constraint: [value]}, "limit": 1}))?;
    let d = r.pointer("/data/0").ok_or(format!("Phabricator has no repository {value}"))?;
    Ok(Repository {
        phid: d.get("phid").and_then(Value::as_str).map(String::from),
        vcs: d.pointer("/fields/vcs").and_then(Value::as_str).unwrap_or("git").to_string(),
    })
}

// -- the plan ------------------------------------------------------------- //

/// How the latest diff of the revision is a version of the patch: which,
/// from 1, oldest first; whether it is the version in the stack; its diff,
/// as the database holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub version: usize,
    pub current: bool,
    pub diff: String,
}

/// The parent a submit gives the revision: the revision of the patch
/// below.
#[derive(Debug, Clone, PartialEq)]
pub enum Parent {
    /// Revision `id`, of PHID `phid`, the revision of patch `below` (from
    /// 1), set as its parent.
    Set { id: String, phid: String, below: usize },
    /// It is its parent already.
    Already { id: String, below: usize },
    /// None is set, and its parents are left as they are: why, said.
    Nothing(String),
}

/// The reviewers a submit sets: those the title names that Phabricator
/// has, each as the title names it, without its `!`, its PHID, and whether
/// it is blocking; those it has not. Or why they could not be looked up.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reviewers {
    pub found: Vec<(String, String, bool)>,
    pub missing: Vec<String>,
    pub failed: Option<String>,
}

impl Reviewers {
    /// The reviewers as `reviewers.set` takes them: the users, those
    /// blocking, the groups, those blocking, each blocking one written
    /// `blocking(PHID)`, as moz-phab orders them.
    fn value(&self) -> Vec<String> {
        let group = |n: &str| n.starts_with('#');
        let mut out = Vec::new();
        for (g, b) in [(false, false), (false, true), (true, false), (true, true)] {
            for (n, p, blocking) in &self.found {
                if group(n) == g && *blocking == b {
                    let v = if b { format!("blocking({p})") } else { p.clone() };
                    if !out.contains(&v) {
                        out.push(v);
                    }
                }
            }
        }
        out
    }

    /// As (PHID, blocking), sorted, as `Remote::reviewers` holds them.
    fn set(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = self.found.iter().map(|(_, p, b)| (p.clone(), *b)).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Those found, by name: `alice, bob (blocking) and #build`.
    pub fn said(&self) -> String {
        and(&self.found.iter().map(|(n, _, b)| if *b { format!("{n} (blocking)") } else { n.clone() }).collect::<Vec<_>>())
    }
}

/// `a`, `a and b`, `a, b and c`.
pub fn and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What a submit of one patch sends, read and checked before anything is.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub stack: String,
    pub change: String,
    /// The version sent: its id, and its place in the stack, from 0.
    pub patch: String,
    pub at: usize,
    /// Phabricator's address, `https://phabricator.example.com`, and where
    /// it was reached from (a revision's link, the checkout's
    /// `phabricator.uri`), to reach it again.
    pub url: String,
    pub from: Option<String>,
    /// The revision updated, as it was read; none for a new one.
    pub revision: Option<Remote>,
    /// How its latest diff is a version of the patch.
    pub known: Option<Known>,
    /// The title, the summary and the bug sent.
    pub title: String,
    pub summary: String,
    pub bug: Option<String>,
    /// A new diff is made: the code changed, or the revision is new.
    pub new_diff: bool,
    pub repository: Repository,
    pub parent: Parent,
    /// None when the title names no reviewer.
    pub reviewers: Option<Reviewers>,
    /// The revision's reviewers as they are, by name, and its parents, by
    /// revision: what the review says they were.
    pub reviewers_now: Vec<String>,
    pub parents_now: Vec<String>,
    /// The code from the revision's latest diff to the one sent, each file
    /// as both leave it on what is below the patch, as a diff; none when it
    /// is the same. And what that diff is, when it is not that.
    pub code: Option<String>,
    pub code_said: Option<String>,
}

impl Prepared {
    /// The patch's number in the stack, from 1.
    pub fn number(&self) -> usize {
        self.at + 1
    }

    /// The revision, as the sentences name it: `D123`, or `the new
    /// revision`.
    fn name(&self) -> String {
        self.revision.as_ref().map_or("the new revision".into(), |r| r.id.clone())
    }

    /// The revision's title and summary, as Phabricator has them; none for
    /// a new one.
    pub fn message_before(&self) -> Option<String> {
        let r = self.revision.as_ref()?;
        Some(format!("{}\n\n{}", r.title, summary_of(&format!("\n{}", r.summary), &trailer())).trim_end().to_string())
    }

    /// The title and summary sent.
    pub fn message_after(&self) -> String {
        format!("{}\n\n{}", self.title, self.summary).trim_end().to_string()
    }

    /// Whether the title, the summary or the bug change.
    pub fn message_changes(&self) -> bool {
        self.text_changes() || self.bug_changes()
    }

    /// Whether the title or the summary change.
    fn text_changes(&self) -> bool {
        self.message_before().as_deref() != Some(self.message_after().as_str())
    }

    /// Whether the bug changes: the title names one, and the revision has
    /// another, or none.
    fn bug_changes(&self) -> bool {
        self.revision.as_ref().is_none_or(|r| self.bug.as_ref().is_some_and(|b| *b != r.bug))
    }

    /// Whether Changes Planned is asked for: in the edit that sends the
    /// rest, for a new revision or one that is a draft; in an edit after
    /// it, for a revision in Changes Planned that takes a new diff, which
    /// puts it back in Needs Review, as moz-phab says.
    fn plan_changes(&self) -> (bool, bool) {
        match &self.revision {
            None => (true, false),
            Some(r) if r.status == "changes-planned" => (false, self.new_diff),
            Some(_) => (true, false),
        }
    }

    /// The reviewers to set, unless they are those already.
    fn reviewers_to_set(&self) -> Option<&Reviewers> {
        let r = self.reviewers.as_ref().filter(|r| !r.found.is_empty())?;
        match &self.revision {
            Some(now) if now.reviewers == r.set() => None,
            _ => Some(r),
        }
    }

    /// Whether the edit that sends the rest is made.
    fn main_edit(&self) -> bool {
        self.new_diff || self.message_changes() || self.plan_changes().0
    }

    /// Whether nothing at all is to be sent.
    pub fn nothing(&self) -> bool {
        !self.main_edit() && !matches!(self.parent, Parent::Set { .. }) && self.reviewers_to_set().is_none()
    }

    /// Whether the reviewers change, and the parent: what the review lists
    /// under what else changes.
    pub fn reviewers_change(&self) -> bool {
        self.reviewers_to_set().is_some()
    }

    pub fn parent_changes(&self) -> bool {
        matches!(self.parent, Parent::Set { .. })
    }

    /// The reviewers as they will be, by name.
    pub fn reviewers_after(&self) -> String {
        self.reviewers_to_set().map_or(String::new(), Reviewers::said)
    }

    /// The parent as it will be, said: `D122, the revision of patch 2`.
    pub fn parent_after(&self) -> String {
        match &self.parent {
            Parent::Set { id, below, .. } | Parent::Already { id, below } => format!("{id}, the revision of patch {below}"),
            Parent::Nothing(_) => String::new(),
        }
    }

    /// The revision's status as it is, as Phabricator names it.
    pub fn status_was(&self) -> String {
        self.revision.as_ref().map_or(String::new(), status_said)
    }

    /// What becomes of the status, said: `stays Changes Planned`.
    pub fn status_change(&self) -> String {
        match &self.revision {
            Some(r) if r.status != "changes-planned" => format!("goes from {} to Changes Planned", status_said(r)),
            Some(_) => "stays Changes Planned".into(),
            None => "is Changes Planned".into(),
        }
    }

    /// What is sent, said: what to where, and its status; then its parent
    /// and its reviewers. For the dialog, and over the review.
    pub fn said(&self) -> Vec<String> {
        let n = self.number();
        let mut out = vec![match &self.revision {
            None => format!(
                "Patch {n} is sent to Phabricator, at {}, as a new revision, with the status Changes Planned. Its title is \"{}\"{}.",
                self.url,
                self.title,
                self.bug.as_ref().map_or(", and it names no bug".into(), |b| format!(", and its bug is {b}"))
            ),
            Some(r) => {
                let same = format!("and no new diff, as its latest diff is patch {n} as it is now");
                let what = match (self.new_diff, self.text_changes(), self.bug_changes()) {
                    (true, ..) => format!("gets a new diff of patch {n}"),
                    (false, true, _) => format!("gets the title and summary of patch {n}, {same}"),
                    (false, false, true) => format!("gets the bug of patch {n}, {same}"),
                    (false, false, false) => format!("keeps its diff, title and summary, as they are those of patch {n} already"),
                };
                format!("{}, at {}/{}, {what}, and {}.", r.id, self.url, r.id, self.status_change())
            }
        }];
        out.push(self.parent_said());
        out.push(self.reviewers_said(false));
        out
    }

    /// What is done of the parent, said.
    fn parent_said(&self) -> String {
        match &self.parent {
            Parent::Set { id, below, .. } => format!("Its parent is set to {id}, the revision of patch {below}."),
            Parent::Already { id, below } => format!("Its parent is {id}, the revision of patch {below}, already."),
            Parent::Nothing(why) => why.clone(),
        }
    }

    /// What is done of the reviewers, said: before it is (`done` false), or
    /// once it is.
    fn reviewers_said(&self, done: bool) -> String {
        let name = self.name();
        let Some(r) = &self.reviewers else {
            return match &self.revision {
                None => "The title names no reviewers, so the new revision has none.".into(),
                Some(_) => format!("The title names no reviewers, so the reviewers of {name} are left as they are."),
            };
        };
        if let Some(e) = &r.failed {
            return format!("The reviewers the title names could not be looked up, so they are not set: {e}.");
        }
        let missing = match r.missing.len() {
            0 => None,
            1 => Some(format!("No reviewer named {} was found", r.missing[0])),
            _ => Some(format!("No reviewers named {} were found", and(&r.missing))),
        };
        let are = match done {
            true => format!("the reviewers of {name} are {}", r.said()),
            false => format!("the reviewers of {name} are set to {}", r.said()),
        };
        match (missing, r.found.is_empty(), self.reviewers_to_set().is_none()) {
            (Some(m), true, _) => format!("{m}, so the reviewers of {name} are left as they are."),
            (Some(m), false, _) if done => format!("{m}; {are}."),
            (Some(m), false, _) => format!("{m}, so {are}."),
            (None, _, true) if self.revision.is_some() => format!("The reviewers of {name} are {} already.", r.said()),
            (None, _, _) => format!("{}{}.", are[..1].to_uppercase(), &are[1..]),
        }
    }

    /// What the status line says when nothing is to be sent: and why no
    /// parent is set, or some reviewers are not, if that is so.
    pub fn nothing_said(&self) -> String {
        let r = self.revision.as_ref().map_or("The revision".into(), |r| r.id.clone());
        let mut out = vec![format!("{r} is patch {} as it is already, with its title and summary, so nothing was sent.", self.number())];
        if let Parent::Nothing(why) = &self.parent {
            out.push(why.clone());
        }
        if self.reviewers.as_ref().is_some_and(|rv| !rv.missing.is_empty() || rv.failed.is_some()) {
            out.push(self.reviewers_said(false));
        }
        out.join(" ")
    }
}

/// The settings' trailer, `Differential Revision`.
fn trailer() -> String {
    crate::config::get().reviews.trailer.clone()
}

// -- reading and checking, before anything is sent ------------------------ //

/// How `raw`, the latest diff of the revision of the patch at `at` of
/// stack `s`, is a version of that patch, if it is one: the version in the
/// stack first, then the others, newest first; each of the same effect
/// (`ops::same_effect`), else leaving the files the same as it once both
/// are applied on what is below the patch (`check::same_files`), so that a
/// diff with another context than the database's is that version still.
fn known(db: &Db, s: &Stack, at: usize, raw: &str) -> Option<Known> {
    let p = db.patch(&s.patches[at]);
    let versions = db.versions(&p.change);
    let mut list: Vec<(usize, &Patch)> = versions.iter().enumerate().map(|(k, v)| (k + 1, *v)).collect();
    list.sort_by_key(|(k, v)| (v.id != p.id, std::cmp::Reverse(*k)));
    let below: Vec<&str> = s.patches[..at].iter().map(|id| db.patch(id).diff.as_str()).collect();
    let pad = s.repo.as_deref().zip(s.base.as_deref());
    let files = |c: &[(usize, &Patch)]| -> Option<usize> {
        let (repo, base) = pad?;
        let diffs: Vec<&str> = c.iter().map(|(_, v)| v.diff.as_str()).collect();
        crate::check::same_files(Path::new(repo), base, &below, raw, &diffs).ok().flatten()
    };
    let effect = |c: &[(usize, &Patch)]| c.iter().position(|(_, v)| crate::ops::same_effect(raw, &v.diff));
    let (current, earlier) = list.split_at(list.iter().take_while(|(_, v)| v.id == p.id).count());
    let (version, v) = effect(current)
        .or_else(|| files(current))
        .map(|k| current[k])
        .or_else(|| effect(earlier).or_else(|| files(earlier)).map(|k| earlier[k]))?;
    Some(Known {
        version,
        current: v.id == p.id,
        diff: v.diff.clone(),
    })
}

/// The parent the revision of the patch at `at` of stack `s` is given: the
/// revision of the patch below, read with `call`, when it is open. `now`,
/// the revision as it is; none for a new one.
fn parent(db: &Db, s: &Stack, at: usize, call: &mut Call, now: Option<&Remote>) -> Parent {
    let me = now.map_or("the new revision".to_string(), |r| r.id.clone());
    let left = match now {
        Some(r) if !r.parents.is_empty() => format!("so the parents of {me} are left as they are"),
        _ => format!("so {me} gets no parent"),
    };
    if at == 0 {
        return Parent::Nothing(format!("Patch 1 is at the bottom of the stack, {left}."));
    }
    let below = db.patch(&s.patches[at - 1]);
    let Some((d, _)) = crate::phab::revision_of(&below.message) else {
        return Parent::Nothing(format!("Patch {at} has no revision, {left}."));
    };
    if now.is_some_and(|r| r.id == d) {
        return Parent::Nothing(format!("Patch {at} names {d} too, {left}."));
    }
    let n: u64 = d.trim_start_matches('D').parse().unwrap_or_default();
    match crate::phab::revision_by(call, json!({ "ids": [n] })) {
        Err(e) => Parent::Nothing(format!("The revision of patch {at}, {d}, could not be read, {left}: {e}.")),
        Ok(None) => Parent::Nothing(format!("Phabricator has no {d}, the revision of patch {at}, {left}.")),
        Ok(Some(r)) if !crate::phab::open(&r) => {
            let status = r.pointer("/fields/status/name").and_then(Value::as_str).unwrap_or("closed");
            Parent::Nothing(format!("{d}, the revision of patch {at}, is {status}, {left}."))
        }
        Ok(Some(r)) => {
            let phid = r.get("phid").and_then(Value::as_str).unwrap_or_default().to_string();
            match now {
                Some(me) if me.parents == [phid.clone()] => Parent::Already { id: d, below: at },
                _ => Parent::Set { id: d, phid, below: at },
            }
        }
    }
}

/// The reviewers `names` (as the title names them: `alice`, `bob!`,
/// `#build`), looked up with `call`: users by their usernames, groups by
/// their slugs, as moz-phab looks them up (`project.search`, the active
/// ones), names in any case.
fn look_up(call: &mut Call, names: &[String]) -> Reviewers {
    let mut out = Reviewers::default();
    // Each once, blocking if it is so once.
    let mut wanted: Vec<(String, bool)> = Vec::new();
    for n in names {
        let name = n.trim_end_matches('!').to_string();
        if name.trim_start_matches('#').is_empty() {
            continue;
        }
        match wanted.iter_mut().find(|(w, _)| w.eq_ignore_ascii_case(&name)) {
            Some(w) => w.1 |= n.ends_with('!'),
            None => wanted.push((name, n.ends_with('!'))),
        }
    }
    let mut phid_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let users: Vec<&str> = wanted.iter().map(|(n, _)| n.as_str()).filter(|n| !n.starts_with('#')).collect();
    let groups: Vec<&str> = wanted.iter().map(|(n, _)| n.as_str()).filter(|n| n.starts_with('#')).collect();
    let found = |v: &Value| v.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    if !users.is_empty() {
        match call("user.search", json!({"constraints": {"usernames": users}})) {
            Ok(v) => {
                for u in found(&v) {
                    if let (Some(name), Some(phid)) = (u.pointer("/fields/username").and_then(Value::as_str), u.get("phid").and_then(Value::as_str)) {
                        phid_of.insert(name.to_lowercase(), phid.into());
                    }
                }
            }
            Err(e) => out.failed = Some(e),
        }
    }
    if !groups.is_empty() && out.failed.is_none() {
        match call("project.search", json!({"queryKey": "active", "constraints": {"slugs": groups}})) {
            Ok(v) => {
                for g in found(&v) {
                    if let (Some(slug), Some(phid)) = (g.pointer("/fields/slug").and_then(Value::as_str), g.get("phid").and_then(Value::as_str)) {
                        phid_of.insert(format!("#{}", slug.to_lowercase()), phid.into());
                    }
                }
                // Found by another of their slugs.
                for (slug, m) in v.pointer("/maps/slugMap").and_then(Value::as_object).into_iter().flatten() {
                    if let Some(phid) = m.get("projectPHID").and_then(Value::as_str) {
                        phid_of.entry(format!("#{}", slug.trim_start_matches('#').to_lowercase())).or_insert(phid.into());
                    }
                }
            }
            Err(e) => out.failed = Some(e),
        }
    }
    if out.failed.is_some() {
        return out;
    }
    for (name, blocking) in wanted {
        match phid_of.get(&name.to_lowercase()) {
            Some(phid) => out.found.push((name, phid.clone(), blocking)),
            None => out.missing.push(name),
        }
    }
    out
}

/// The names of reviewers `phids` (each with whether it is blocking), read
/// with `call`: a user's username, a group's `#slug`, else the PHID.
fn reviewer_names(call: &mut Call, phids: &[(String, bool)]) -> Vec<String> {
    let mut name_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (method, prefix, mark) in [("user.search", "PHID-USER-", ""), ("project.search", "PHID-PROJ-", "#")] {
        let these: Vec<&str> = phids.iter().map(|(p, _)| p.as_str()).filter(|p| p.starts_with(prefix)).collect();
        if these.is_empty() {
            continue;
        }
        let Ok(v) = call(method, json!({"constraints": {"phids": these}})) else { continue };
        for d in v.get("data").and_then(Value::as_array).into_iter().flatten() {
            let name = d.pointer("/fields/username").or_else(|| d.pointer("/fields/slug")).and_then(Value::as_str);
            if let (Some(name), Some(phid)) = (name, d.get("phid").and_then(Value::as_str)) {
                name_of.insert(phid.into(), format!("{mark}{name}"));
            }
        }
    }
    phids
        .iter()
        .map(|(p, blocking)| {
            let name = name_of.get(p).cloned().unwrap_or_else(|| p.clone());
            if *blocking { format!("{name} (blocking)") } else { name }
        })
        .collect()
}

/// The revisions `phids`, read with `call`, as `D123`; each not found, its
/// PHID.
fn revision_names(call: &mut Call, phids: &[String]) -> Vec<String> {
    if phids.is_empty() {
        return Vec::new();
    }
    let found = call("differential.revision.search", json!({"constraints": {"phids": phids}})).unwrap_or(Value::Null);
    let id_of = |p: &String| {
        found
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|r| r.get("phid").and_then(Value::as_str) == Some(p.as_str()))
            .and_then(|r| r.get("id").and_then(Value::as_u64))
    };
    phids.iter().map(|p| id_of(p).map_or(p.clone(), |n| format!("D{n}"))).collect()
}

/// A path as git writes it in a diff's header, on side `side` (`a/`,
/// `b/`): quoted as C quotes it when it holds what git quotes.
fn git_path(side: &str, path: &str) -> String {
    let plain = path.bytes().all(|b| (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\');
    if plain {
        return format!("{side}{path}");
    }
    let mut out = String::from("\"");
    for b in format!("{side}{path}").bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            0x20..0x7f => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push('"');
    out
}

/// File `path` from `a` to `b`, as git would write that diff: none when
/// they are the same.
fn file_diff(path: &str, a: &Left, b: &Left) -> Option<String> {
    if a == b {
        return None;
    }
    let mut out = format!("diff --git {} {}\n", git_path("a/", path), git_path("b/", path));
    match (a, b) {
        (None, Some((m, _))) => out.push_str(&format!("new file mode {m}\n")),
        (Some((m, _)), None) => out.push_str(&format!("deleted file mode {m}\n")),
        (Some((ma, _)), Some((mb, _))) if ma != mb => out.push_str(&format!("old mode {ma}\nnew mode {mb}\n")),
        _ => {}
    }
    let text = |l: &Left| l.as_ref().map(|(_, t)| t.clone());
    match (text(a), text(b)) {
        (Some(None), _) | (_, Some(None)) => {
            out.push_str(&format!("Binary files {} and {} differ\n", git_path("a/", path), git_path("b/", path)));
        }
        (old, new) => {
            let (o, n) = (old.flatten().unwrap_or_default(), new.flatten().unwrap_or_default());
            if o != n {
                let from = if a.is_some() { git_path("a/", path) } else { "/dev/null".into() };
                let to = if b.is_some() { git_path("b/", path) } else { "/dev/null".into() };
                let d = similar::TextDiff::from_lines(o.as_str(), n.as_str());
                out.push_str(&d.unified_diff().context_radius(3).header(&from, &to).to_string());
            }
        }
    }
    Some(out)
}

/// The code from `theirs` to `mine`, two diffs of the patch at `at` of
/// stack `s`: each file either touches, as `theirs` leaves it and as `mine`
/// does, both applied on what is below the patch, as a diff; none when
/// none differs. None of it when either does not apply there, or the files
/// cannot be read.
fn code_diff(db: &Db, s: &Stack, at: usize, theirs: &str, mine: &str) -> Option<Option<String>> {
    let (repo, base) = (Path::new(s.repo.as_deref()?), s.base.as_deref()?);
    let below: Vec<&str> = s.patches[..at].iter().map(|id| db.patch(id).diff.as_str()).collect();
    let paths: Vec<String> = crate::check::paths_of(repo, &[theirs, mine]).into_iter().collect();
    let a = crate::check::left_by(repo, base, &below, theirs, &paths).ok()??;
    let b = crate::check::left_by(repo, base, &below, mine, &paths).ok()??;
    let out: String = paths.iter().zip(a.iter().zip(&b)).filter_map(|(p, (a, b))| file_diff(p, a, b)).collect();
    Some((!out.is_empty()).then_some(out))
}

/// The code the review shows, from the revision's latest diff, `raw`,
/// known as `k`, to the patch at `at` of stack `s`, `mine`: from the
/// database's version it is, else from `raw`, each file as it leaves it;
/// else, when neither applies on what is below the patch, the two diffs
/// diffed, as text. And what that is, said, when it is not the files.
fn code_change(db: &Db, s: &Stack, at: usize, k: &Known, raw: &str, mine: &str) -> (Option<String>, Option<String>) {
    for theirs in [k.diff.as_str(), raw] {
        if let Some(code) = code_diff(db, s, at, theirs, mine) {
            return (code, None);
        }
    }
    let name = format!("patch-{}.diff", at + 1);
    let d = similar::TextDiff::from_lines(raw, mine);
    let text = d.unified_diff().context_radius(3).header(&format!("a/{name}"), &format!("b/{name}")).to_string();
    let said = "The files could not be read as what is below the patch leaves them, so this is Phabricator's latest diff against the diff that is sent, as text.";
    (Some(format!("diff --git a/{name} b/{name}\n{text}")), Some(said.into()))
}

/// Why the patch at `at` (from 0) is not to be submitted while HEAD is
/// where `head` says, if it is not: HEAD on the stack, the patch's commit
/// in your workspace names a revision the patch does not, none or another,
/// as after `moz-phab submit` there. Submitted, the patch would make a
/// second revision, or send its diff to another revision than the
/// commit's. Any other patch, and any other difference, is not held back.
pub fn held_by_head(head: &crate::head::Head, at: usize) -> Option<String> {
    let crate::head::At::Stack(on) = &head.at else { return None };
    let (k, _, d) = on.messages.iter().find(|(k, _, _)| *k == at)?;
    let crate::head::Differs::Revision(ours, Some(theirs)) = d else { return None };
    if ours.as_ref() == Some(theirs) {
        return None;
    }
    let said = d.revisions(*k)?;
    let so = match ours {
        None => "so that no second revision is made",
        Some(_) => "so that its diff goes to the revision you keep",
    };
    Some(format!("{}{}: sync the commit messages first, {so}.", said[..1].to_uppercase(), &said[1..]))
}

/// Whether patch `p`, number `n` in its stack, is one Phabricator can be
/// sent: a diff of some file, each file text.
fn sendable(p: &Patch, n: usize) -> Result<(), String> {
    let files = crate::diff::parse_diff(&p.diff, "");
    if files.is_empty() {
        return Err(format!("Patch {n} changes no file, so there is nothing to send to Phabricator."));
    }
    if let Some(f) = files.iter().find(|f| crate::whole::binary(f)) {
        return Err(format!(
            "Patch {n} changes {}, a file that is not text, and stacked cannot send such a file to Phabricator yet, so nothing was sent.",
            f.path
        ));
    }
    Ok(())
}

/// What submitting the patch at `at` of stack `s` sends, read from
/// Phabricator at `url` with `call`: or why nothing can be, said. `arc`,
/// the `.arcconfig` of the stack's checkout.
fn prepare_with(db: &Db, s: &Stack, at: usize, url: &str, arc: &Arc, call: &mut Call) -> Result<Prepared, String> {
    let p = db.patch(&s.patches[at]);
    let n = at + 1;
    let titled = wip_title(p.title());
    let summary = summary_of(&p.message, &trailer());
    let (revision, known, repository) = match crate::phab::revision_of(&p.message) {
        Some((d, _)) => {
            let r = read_remote(call, &d, true).map_err(|e| format!("{d} could not be read from Phabricator, so nothing was sent: {e}"))?;
            if !r.in_work() {
                return Err(format!(
                    "{d} is {}, not Changes Planned: only a revision in work is updated, and nothing was sent.",
                    status_said(&r)
                ));
            }
            let Some(k) = known(db, s, at, &r.raw) else {
                return Err(format!("{d}'s latest diff is not a version of patch {n} that stacked knows: it came from elsewhere, and nothing was sent."));
            };
            let repository = match (&r.repository, &arc.callsign) {
                (Some(phid), _) => repository_by(call, "phids", phid),
                (None, Some(cs)) => repository_by(call, "callsigns", cs),
                (None, None) => Ok(Repository { phid: None, vcs: "git".into() }),
            }
            .map_err(|e| format!("The repository of {d} could not be read from Phabricator, so nothing was sent: {e}"))?;
            (Some(r), Some(k), repository)
        }
        None => {
            let Some(cs) = &arc.callsign else {
                return Err(format!(
                    "Patch {n} has no revision yet, and the checkout {} has no .arcconfig that names its repository on Phabricator (repository.callsign), so nothing was sent.",
                    s.repo.as_deref().unwrap_or_default()
                ));
            };
            let repository =
                repository_by(call, "callsigns", cs).map_err(|e| format!("The repository {cs} could not be read from Phabricator, so nothing was sent: {e}"))?;
            (None, None, repository)
        }
    };
    let new_diff = known.as_ref().is_none_or(|k| !k.current);
    let parent = parent(db, s, at, call, revision.as_ref());
    let reviewers = (!titled.reviewers.is_empty()).then(|| look_up(call, &titled.reviewers));
    let (reviewers_now, parents_now) = match &revision {
        Some(r) => (reviewer_names(call, &r.reviewers), revision_names(call, &r.parents)),
        None => (Vec::new(), Vec::new()),
    };
    let (code, code_said) = match (&revision, &known) {
        (Some(r), Some(k)) if new_diff => code_change(db, s, at, k, &r.raw, &p.diff),
        _ => (None, None),
    };
    Ok(Prepared {
        stack: s.name.clone(),
        change: p.change.clone(),
        patch: p.id.clone(),
        at,
        url: url.to_string(),
        from: None,
        revision,
        known,
        title: titled.title,
        summary,
        bug: titled.bug,
        new_diff,
        repository,
        parent,
        reviewers,
        reviewers_now,
        parents_now,
        code,
        code_said,
    })
}

/// Phabricator's address: that of revision link `link`, else the one the
/// checkout's `.arcconfig` names, else the settings', else that of the API
/// reached.
fn base_url(link: Option<&str>, arc: &Arc, via: &Via) -> String {
    link.and_then(|l| l.rsplit_once('/').map(|(b, _)| b.to_string()))
        .or_else(|| arc.uri.clone())
        .or_else(|| crate::config::get().reviews.url.clone())
        .or_else(|| via.api().map(|a| a.trim_end_matches('/').trim_end_matches("/api").to_string()))
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string()
}

/// What submitting the patch at `at` of stack `name`, in the database at
/// `path`, to Phabricator as work in progress sends, read and checked: or
/// why nothing can be, said. Nothing is sent.
pub fn prepare(path: &Path, name: &str, at: usize) -> Result<Prepared, String> {
    let db = Db::load(path).map_err(|e| e.0)?;
    let s = db.stack(name).map_err(|e| e.0)?.clone();
    let p = s.patches.get(at).map(|id| db.patch(id).clone()).ok_or(format!("{name} has no patch {}", at + 1))?;
    crate::check::can_check(&s).map_err(|e| format!("{}, so nothing was sent.", e.0))?;
    sendable(&p, at + 1)?;
    let named = crate::phab::revision_of(&p.message);
    if named.is_none()
        && let Some(why) = crate::ask::blocked(&db, name)
    {
        return Err(format!("Patch {} has no revision yet, and a new revision's link goes into the stack, but {why}; nothing was sent.", at + 1));
    }
    let arc = arcconfig(Path::new(s.repo.as_deref().unwrap_or_default()));
    let from = named.as_ref().map(|(_, l)| l.clone()).or_else(|| arc.uri.clone());
    let via = Via::to(from.as_deref()).map_err(|e| format!("{e}, so nothing was sent."))?;
    let url = base_url(named.as_ref().map(|(_, l)| l.as_str()), &arc, &via);
    let mut call = |m: &str, v: Value| via.call(m, v);
    let mut out = prepare_with(&db, &s, at, &url, &arc, &mut call)?;
    out.from = from;
    Ok(out)
}

// -- sending -------------------------------------------------------------- //

/// The kinds of change differential.creatediff takes (`Diff.Kind`).
const ADD: u8 = 1;
const CHANGE: u8 = 2;
const DELETE: u8 = 3;
const MOVE_AWAY: u8 = 4;
const COPY_AWAY: u8 = 5;
const MOVE_HERE: u8 = 6;
const COPY_HERE: u8 = 7;
const MULTICOPY: u8 = 8;

/// A file's change as moz-phab makes one for differential.creatediff
/// (`Diff.Change`).
#[derive(Default)]
struct Change {
    old_path: Option<String>,
    cur_path: String,
    away: Vec<String>,
    old_mode: Option<String>,
    cur_mode: Option<String>,
    kind: u8,
    hunks: Vec<Value>,
}

/// A hunk as differential.creatediff takes it (`Diff.Hunk`), from its
/// header, `@@ -a,b +c,d @@`, and its lines, each with its newline: those
/// past the lengths its header gives left out, but git's `\ No newline at
/// end of file` after the last.
fn hunk(header: &str, lines: &[String]) -> Option<Value> {
    let ranges = header.strip_prefix("@@ -")?.split(" @@").next()?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |r: &str| -> Option<(u64, u64)> {
        let mut parts = r.split(',');
        let start = parts.next()?.parse().ok()?;
        let len = parts.next().map_or(Some(1), |l| l.parse().ok())?;
        Some((start, len))
    };
    let ((old_off, old_len), (new_off, new_len)) = (range(old)?, range(new)?);
    let (mut olds, mut news) = (0, 0);
    let mut kept = Vec::new();
    for l in lines {
        let full = olds >= old_len && news >= new_len;
        match l.chars().next() {
            Some('\\') => {}
            _ if full => break,
            Some('-') => olds += 1,
            Some('+') => news += 1,
            _ => {
                olds += 1;
                news += 1;
            }
        }
        // An empty line of a hunk is git's empty context line.
        kept.push(if l == "\n" { " \n".to_string() } else { l.clone() });
    }
    let (mut added, mut deleted) = (0, 0);
    let (mut old_eof, mut new_eof) = (true, true);
    let mut prev = ' ';
    for l in &kept {
        match l.chars().next() {
            Some('+') => added += 1,
            Some('-') => deleted += 1,
            _ => {}
        }
        if l.ends_with("No newline at end of file\n") {
            old_eof &= prev == '+';
            new_eof &= prev == '-';
        }
        prev = l.chars().next().unwrap_or(' ');
    }
    Some(json!({
        "oldOffset": old_off,
        "oldLength": old_len,
        "newOffset": new_off,
        "newLength": new_len,
        "addLines": added,
        "delLines": deleted,
        "isMissingOldNewline": !old_eof,
        "isMissingNewNewline": !new_eof,
        "corpus": kept.concat(),
    }))
}

/// The changes of `diff`, a patch's, as differential.creatediff takes them
/// and moz-phab makes them of a commit (`Diff.Change.to_conduit`): a file's
/// kind, paths and modes from its header, its hunks as they are; a file
/// moved or copied, a change for where it came from too; sorted by path.
/// `node`, the commit each names: the patch's id.
fn changes(diff: &str, node: &str) -> Result<Vec<Value>, String> {
    // Each file: its header's lines, then its hunks, each its header and
    // its lines; a carriage return kept, as it is the file's.
    type Hunks = Vec<(String, Vec<String>)>;
    let mut files: Vec<(Vec<String>, Hunks)> = Vec::new();
    for line in diff.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if bare.starts_with("diff --git ") {
            files.push((vec![bare.to_string()], Vec::new()));
            continue;
        }
        let Some((header, hunks)) = files.last_mut() else { continue };
        if bare.starts_with("@@") {
            hunks.push((bare.to_string(), Vec::new()));
            continue;
        }
        match hunks.last_mut() {
            Some((_, lines)) => lines.push(format!("{bare}\n")),
            None => header.push(bare.trim_end().to_string()),
        }
    }
    let mut out: Vec<Change> = Vec::new();
    // The change of `path`, made if it is not there yet (`change_for`).
    fn change_for(out: &mut Vec<Change>, path: &str) -> usize {
        match out.iter().position(|c| c.cur_path == path) {
            Some(k) => k,
            None => {
                out.push(Change {
                    cur_path: path.to_string(),
                    kind: CHANGE,
                    ..Default::default()
                });
                out.len() - 1
            }
        }
    }
    for (header, hunks) in files {
        let said = |prefix: &str| header.iter().find_map(|l| l.strip_prefix(prefix)).map(str::to_string);
        let moved = |prefix: &str| said(prefix).map(|p| crate::diff::unquote(&p, false));
        let index_mode = said("index ").and_then(|l| l.split(' ').nth(1).map(String::from));
        let side = |prefix: &str| header.iter().find(|l| l.starts_with(prefix)).and_then(|l| crate::diff::side_path(l));
        let named = crate::diff::header_path(header[0].trim_start_matches("diff --git "));
        let (a_path, b_path) = match (moved("rename from ").or(moved("copy from ")), moved("rename to ").or(moved("copy to "))) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                let a = side("--- ").or_else(|| side("+++ ")).unwrap_or_else(|| named.clone());
                let b = side("+++ ").unwrap_or_else(|| a.clone());
                (a, b)
            }
        };
        let a_mode = said("old mode ").or(said("deleted file mode ")).or(index_mode.clone()).unwrap_or_default();
        let b_mode = said("new mode ").or(said("new file mode ")).or(index_mode).unwrap_or_default();
        let k = change_for(&mut out, &b_path);
        for (h, lines) in &hunks {
            out[k].hunks.push(hunk(h, lines).ok_or(format!("{b_path}: a hunk stacked cannot read, {h}"))?);
        }
        let modes = |c: &mut Change| {
            if a_mode != b_mode {
                c.old_mode = Some(a_mode.clone());
                c.cur_mode = Some(b_mode.clone());
            }
        };
        if said("new file mode ").is_some() {
            out[k].kind = ADD;
            out[k].cur_mode = Some(b_mode.clone());
        } else if said("deleted file mode ").is_some() {
            out[k].kind = DELETE;
            out[k].old_mode = Some(a_mode.clone());
            out[k].old_path = Some(a_path.clone());
        } else if let Some(here) = match (said("rename from ").is_some(), said("copy from ").is_some()) {
            (true, _) => Some(MOVE_HERE),
            (false, true) => Some(COPY_HERE),
            _ => None,
        } {
            out[k].kind = here;
            modes(&mut out[k]);
            out[k].old_path = Some(a_path.clone());
            let o = change_for(&mut out, &a_path);
            out[o].kind = match (here, out[o].kind) {
                (MOVE_HERE, MOVE_AWAY | COPY_AWAY) => MULTICOPY,
                (MOVE_HERE, MULTICOPY) => MULTICOPY,
                (MOVE_HERE, _) => MOVE_AWAY,
                (_, MULTICOPY) => MULTICOPY,
                _ => COPY_AWAY,
            };
            out[o].away.push(b_path.clone());
        } else {
            out[k].kind = CHANGE;
            modes(&mut out[k]);
            out[k].old_path = Some(a_path.clone());
        }
    }
    out.sort_by(|a, b| a.cur_path.cmp(&b.cur_path));
    let props = |m: &Option<String>| m.as_ref().map_or(json!({}), |m| json!({ "unix:filemode": m }));
    Ok(out
        .into_iter()
        .map(|c| {
            json!({
                "metadata": {},
                "oldPath": c.old_path,
                "currentPath": c.cur_path,
                "awayPaths": c.away,
                "oldProperties": props(&c.old_mode),
                "newProperties": props(&c.cur_mode),
                "commitHash": node,
                "type": c.kind,
                "fileType": 1,
                "hunks": c.hunks,
            })
        })
        .collect())
}

/// Seconds since the epoch of `date`, as git's mail writes it: `Tue, 14
/// Apr 2020 12:02:20 +0000`.
fn epoch(date: &str) -> Option<i64> {
    let d = date.trim();
    let d = d.split_once(", ").map_or(d, |(_, rest)| rest);
    let parts: Vec<&str> = d.split_whitespace().collect();
    let [day, month, year, time, zone, ..] = parts[..] else { return None };
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let m = MONTHS.iter().position(|n| month.to_ascii_lowercase().starts_with(n))? as i64 + 1;
    let (y, day): (i64, i64) = (year.parse().ok()?, day.parse().ok()?);
    let hms: Vec<i64> = time.split(':').map(|t| t.parse().ok()).collect::<Option<_>>()?;
    let secs = hms.first()? * 3600 + hms.get(1)? * 60 + hms.get(2).copied().unwrap_or(0);
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let z: i64 = zone.trim_start_matches(['+', '-']).parse().ok()?;
    let offset = sign * ((z / 100) * 3600 + (z % 100) * 60);
    // Days from 1970-01-01 to the date, as Howard Hinnant counts them.
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + secs - offset)
}

/// Commit `node` of the repository at `repo` as Phabricator's repository
/// names it, held in `vcs`: in a Mercurial one, as git-cinnabar has it,
/// if it has it, as moz-phab does (`get_public_node`); else as it is.
fn public_node(repo: &Path, node: &str, vcs: &str) -> String {
    if vcs == "git" || node.is_empty() {
        return node.to_string();
    }
    match crate::git::git(repo, &["cinnabar", "git2hg", node]) {
        Ok(hg) if !hg.trim().is_empty() && hg.trim().bytes().any(|b| b != b'0') => hg.trim().to_string(),
        _ => node.to_string(),
    }
}

/// The commit of patch `p`, as differential.setdiffproperty sets
/// `local:commits`, as moz-phab sets it: its author, when, its summary
/// (`title`), its message, the commit it is (the patch's id: it is no
/// commit of the workspace's) and the one it is on, `base`.
fn commits(p: &Patch, title: &str, base: &str) -> String {
    let author = p.author.clone().or_else(crate::ops::default_author).unwrap_or_default();
    let (name, email) = match author.rsplit_once(" <") {
        Some((n, e)) => (n.trim().to_string(), e.trim_end_matches('>').to_string()),
        None => (author.trim().to_string(), String::new()),
    };
    let time = p.date.as_deref().and_then(epoch).unwrap_or_else(crate::db::now);
    let mut data = serde_json::Map::new();
    data.insert(
        p.id.clone(),
        json!({
            "author": name,
            "authorEmail": email,
            "time": time,
            "summary": title,
            "message": p.message,
            "commit": p.id,
            "parents": [base],
        }),
    );
    Value::Object(data).to_string()
}

/// Revision `phid` edited with transaction `txn` alone, in a call of its
/// own.
fn edit(call: &mut Call, phid: &str, txn: Value) -> Result<Value, String> {
    call("differential.revision.edit", json!({"objectIdentifier": phid, "transactions": [txn]}))
}

/// The patch of change `change`, in stack `name` of the database at
/// `path`, given the trailer naming revision `d` at `link`, as a new
/// version, at once: the history says the submit made it.
fn take_link(path: &Path, name: &str, change: &str, d: &str, link: &str) -> crate::db::Result<()> {
    Db::edit(path, |db| {
        crate::ops::by_hand(db, name)?;
        let s = db.stack(name)?.clone();
        let Some(at) = s.patches.iter().position(|id| db.patch(id).change == change) else {
            return crate::db::err(format!("the patch is not in {name} any more"));
        };
        let old = db.patch(&s.patches[at]).clone();
        if let Some((other, _)) = crate::phab::revision_of(&old.message) {
            return crate::db::err(format!("its message names {other} already"));
        }
        let message = format!("{}\n\n{}: {link}", old.message.trim_end(), trailer());
        let new = crate::ops::revise(&old, crate::mbox::Draft { message, ..crate::mbox::Draft::from(&old) });
        let id = db.add_patch(new)?;
        let mut patches = s.patches.clone();
        patches[at] = id;
        db.set_stack(name, patches, &format!("submitted patch {} as {d}, in work", at + 1))?;
        Ok(())
    })
}

/// `s` ended as a sentence is: with a full stop, unless it has one, or
/// another end.
fn sentence(s: String) -> String {
    let t = s.trim_end();
    if t.ends_with(['.', '!', '?']) { t.to_string() } else { format!("{t}.") }
}

/// Sends what `p` says, with `call`, once the stack and the revision are
/// seen to be as they were when it was read: the stack in the database at
/// `path`. What came of it, said: what was done and what was not, when a
/// call fails partway.
fn send_with(path: &Path, p: &Prepared, call: &mut Call) -> String {
    sentence(sent(path, p, call))
}

/// As `send_with`, what came of it said a sentence after another.
fn sent(path: &Path, p: &Prepared, call: &mut Call) -> String {
    let n = p.number();
    let db = match Db::load(path) {
        Ok(db) => db,
        Err(e) => return format!("Nothing was sent: {}.", e.0),
    };
    let s = match db.stack(&p.stack) {
        Ok(s) => s.clone(),
        Err(e) => return format!("Nothing was sent: {}.", e.0),
    };
    if s.patches.get(p.at) != Some(&p.patch) {
        return format!("Patch {n} changed since what is sent was read, so nothing was sent. Submit it again to see what is sent now.");
    }
    let patch = db.patch(&p.patch).clone();
    if p.revision.is_none()
        && let Some(why) = crate::ask::blocked(&db, &p.stack)
    {
        return format!("Nothing was sent: a new revision's link goes into the stack, but {why}.");
    }
    if let Some(r) = &p.revision {
        match read_remote(call, &r.id, false) {
            Ok(now) if now.same_as(r) => {}
            Ok(_) => return format!("{} changed on Phabricator since you reviewed it, so nothing was sent. Submit again to review it as it is now.", r.id),
            Err(e) => return format!("{} could not be read from Phabricator again, so nothing was sent: {e}", r.id),
        }
    }
    // What a failure before the revision is touched leaves of it.
    let left = match &p.revision {
        Some(r) => format!("so {} was left as it was", r.id),
        None => "so no revision was made of it".to_string(),
    };
    let repo = s.repo.clone().unwrap_or_default();
    let base = public_node(Path::new(&repo), s.base.as_deref().unwrap_or_default(), &p.repository.vcs);
    // The diff, with its commit.
    let diff = match p.new_diff {
        false => None,
        true => {
            let changes = match changes(&patch.diff, &patch.id) {
                Ok(c) => c,
                Err(e) => return format!("Nothing was sent: {e}."),
            };
            let mut params = json!({
                "changes": changes,
                "sourceMachine": p.url,
                "sourceControlSystem": p.repository.vcs,
                "sourceControlPath": "/",
                "sourceControlBaseRevision": base,
                "creationMethod": "stacked",
                "lintStatus": "none",
                "unitStatus": "none",
                "sourcePath": repo,
                "branch": if p.repository.vcs == "git" { "HEAD" } else { "default" },
            });
            if let Some(phid) = &p.repository.phid {
                params["repositoryPHID"] = json!(phid);
            }
            let made = match call("differential.creatediff", params) {
                Ok(v) => v,
                Err(e) => return format!("Phabricator refused the diff of patch {n}, so nothing changed there: {e}"),
            };
            let (Some(id), Some(phid)) = (made.get("diffid").cloned(), made.get("phid").and_then(Value::as_str).map(String::from)) else {
                return format!("Phabricator made the diff of patch {n}, but did not say which, {left}.");
            };
            let property = json!({"diff_id": id, "name": "local:commits", "data": commits(&patch, &p.title, &base)});
            if let Err(e) = call("differential.setdiffproperty", property) {
                return format!("The diff of patch {n} was made on Phabricator, but its commit's author and message were not set, {left}: {e}");
            }
            Some(phid)
        }
    };
    // The revision, made or edited.
    let mut txns = Vec::new();
    let r = p.revision.as_ref();
    if r.is_none_or(|r| r.title != p.title) {
        txns.push(json!({"type": "title", "value": p.title}));
    }
    if r.is_none_or(|r| summary_of(&format!("\n{}", r.summary), &trailer()) != p.summary) {
        txns.push(json!({"type": "summary", "value": p.summary}));
    }
    if let Some(b) = p.bug.as_ref().filter(|b| r.is_none_or(|r| r.bug != **b)) {
        txns.push(json!({"type": "bugzilla.bug-id", "value": b}));
    }
    if let Some(phid) = &diff {
        txns.push(json!({"type": "update", "value": phid}));
    }
    let (plan_now, plan_after) = p.plan_changes();
    if plan_now {
        txns.push(json!({"type": "plan-changes", "value": true}));
    }
    let (d, phid) = match r {
        None => match call("differential.revision.edit", json!({ "transactions": txns })) {
            Ok(v) => match (v.pointer("/object/id").and_then(Value::as_u64), v.pointer("/object/phid").and_then(Value::as_str)) {
                (Some(id), Some(phid)) => (format!("D{id}"), phid.to_string()),
                _ => return format!("The diff of patch {n} was made on Phabricator, and a revision of it, but Phabricator did not say which."),
            },
            Err(e) => return format!("The diff of patch {n} was made on Phabricator, but no revision was made of it: {e}"),
        },
        Some(r) => {
            if !txns.is_empty()
                && let Err(e) = call("differential.revision.edit", json!({"objectIdentifier": r.phid, "transactions": txns}))
            {
                return match diff {
                    Some(_) => format!("The new diff of patch {n} was made on Phabricator, but {} was not updated with it: {e}", r.id),
                    None => format!("{} was not updated with the title and summary of patch {n}: {e}", r.id),
                };
            }
            (r.id.clone(), r.phid.clone())
        }
    };
    let planned = if plan_now { ", in Changes Planned" } else { "" };
    let mut said = vec![match r {
        None => format!("Patch {n} is {d} on Phabricator now, in Changes Planned."),
        Some(_) if diff.is_some() => format!("{d} has the new diff of patch {n} now{planned}."),
        Some(_) if p.text_changes() => format!("{d} has the title and summary of patch {n} now{planned}."),
        Some(_) if p.bug_changes() => format!("{d} has the bug of patch {n} now{planned}."),
        Some(_) if plan_now => format!("{d} is Changes Planned now."),
        Some(_) => format!("{d} keeps its diff, title and summary."),
    }];
    if plan_after {
        said.push(match edit(call, &phid, json!({"type": "plan-changes", "value": true})) {
            Ok(_) => format!("{d} stays Changes Planned."),
            Err(e) => format!("But {d} was not put back in Changes Planned, and may be Needs Review now: {e}"),
        });
    }
    // A new revision's link, into the patch.
    if r.is_none() {
        let link = format!("{}/{d}", p.url);
        said.push(match take_link(path, &p.stack, &p.change, &d, &link) {
            Ok(()) => format!("The message of patch {n} names {d} now."),
            Err(e) => format!("But the message of patch {n} could not take the link to {d}: {}.", e.0),
        });
    }
    // Best effort, each on its own: the parent, then the reviewers.
    match &p.parent {
        Parent::Set { id, phid: parent, .. } => {
            let kind = if r.is_none() { "parents.add" } else { "parents.set" };
            said.push(match edit(call, &phid, json!({"type": kind, "value": [parent]})) {
                Ok(_) => format!("The parent of {d} is {id} now."),
                Err(e) => format!("But the parent of {d} was not set to {id}: {e}"),
            });
        }
        Parent::Already { .. } => {}
        Parent::Nothing(why) => said.push(why.replace("the new revision", &d)),
    }
    if let Some(rv) = p.reviewers_to_set() {
        said.push(match edit(call, &phid, json!({"type": "reviewers.set", "value": rv.value()})) {
            Ok(_) => p.reviewers_said(true).replace("the new revision", &d),
            Err(e) => format!("But the reviewers of {d} were not set: {e}"),
        });
    } else if p.reviewers.as_ref().is_some_and(|rv| !rv.missing.is_empty() || rv.failed.is_some()) {
        said.push(p.reviewers_said(true).replace("the new revision", &d));
    }
    said.into_iter().map(sentence).collect::<Vec<_>>().join(" ")
}

/// Sends what `p` says to Phabricator, once the stack and the revision are
/// seen to be as they were when it was read, in the database at `path`:
/// what came of it, said.
pub fn send(path: &Path, p: &Prepared) -> String {
    let via = match Via::to(p.from.as_deref()) {
        Ok(v) => v,
        Err(e) => return format!("Nothing was sent: {e}."),
    };
    let mut call = |m: &str, v: Value| via.call(m, v);
    send_with(path, p, &mut call)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn a_title_is_read_as_moz_phab_reads_it() {
        let t = wip_title("Bug 1 - Fix foo r=alice,bob");
        assert_eq!((t.title.as_str(), t.bug.as_deref()), ("WIP: Bug 1 - Fix foo r=alice,bob", Some("1")));
        assert_eq!(t.reviewers, ["alice", "bob"]);
        // Asked of a group and of one who blocks, given by another: those
        // asked first, each list written again as moz-phab writes it.
        let t = wip_title("Bug 2 - x r?#build, carol! r=dave");
        assert_eq!(t.reviewers, ["#build", "carol!", "dave"]);
        assert_eq!(t.title, "WIP: Bug 2 - x r?#build,carol! r=dave");
        // The typo `r!`: each blocking, given.
        let t = wip_title("Bug 3 - y r!erin,frank.");
        assert_eq!((t.title.as_str(), t.reviewers.clone()), ("WIP: Bug 3 - y r=erin!,frank!.", vec!["erin!".to_string(), "frank!".into()]));
        // Another's specifier, `sr=`, is none of its names; nor is one in a word.
        assert_eq!(wip_title("Bug 4 - z r=ann, sr=bo").reviewers, ["ann"]);
        assert_eq!(wip_title("Bug 5 - zr=ann (r=bo)").reviewers, ["bo"]);
        // The bug in any case, said `Bug N`; `WIP:` once; none named.
        let t = wip_title("bug 123 - lower");
        assert_eq!((t.title.as_str(), t.bug.as_deref()), ("WIP: Bug 123 - lower", Some("123")));
        assert_eq!(wip_title("WIP: Bug 6 - again").title, "WIP: Bug 6 - again");
        assert_eq!(wip_title("wip Bug 7").title, "WIP: Bug 7");
        let t = wip_title("Fix it");
        assert_eq!((t.title.as_str(), t.bug, t.reviewers.is_empty()), ("WIP: Fix it", None, true));
        // A specifier with no name goes.
        assert_eq!(wip_title("Bug 9 - x r=").title, "WIP: Bug 9 - x");
        assert_eq!(wip_title("Bug 12x - no bug").bug, None);
    }

    #[test]
    fn a_summary_leaves_out_the_lines_naming_a_revision() {
        let t = "Differential Revision";
        assert_eq!(summary_of("Bug 1 - x\n\nBecause.\n\nDifferential Revision: https://p.example/D1", t), "Because.");
        assert_eq!(summary_of("T\n\nA\n\nDifferential Revision: https://p.example/D2\n\nB", t), "A\n\nB");
        assert_eq!(summary_of("T\n\nA\nDifferential Revision: https://p.example/D2\nB", t), "A\n\nB");
        assert_eq!(summary_of("Title alone", t), "");
        assert_eq!(summary_of("T\n\nDifferential Revision: D7", t), "", "a bare one, as the settings' trailer");
    }

    #[test]
    fn a_date_of_git_s_mail_is_read_as_seconds() {
        assert_eq!(epoch("Tue, 14 Apr 2020 12:02:20 +0000"), Some(1586865740));
        assert_eq!(epoch("Tue, 14 Apr 2020 14:02:20 +0200"), Some(1586865740));
        assert_eq!(epoch("1 Jan 1970 00:00:00 -0100"), Some(3600));
        assert_eq!(epoch("yesterday"), None);
    }

    #[test]
    fn a_patch_s_diff_is_sent_as_moz_phab_makes_its_changes() {
        let diff = concat!(
            "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n",
            "diff --git a/b.txt b/b.txt\nnew file mode 100644\nindex 0000000..3\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+bee\n\\ No newline at end of file\n",
            "diff --git a/c.txt b/c.txt\ndeleted file mode 100755\nindex 4..0000000\n--- a/c.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-sea\n",
            "diff --git a/o.txt b/n.txt\nsimilarity index 80%\nrename from o.txt\nrename to n.txt\nindex 5..6 100644\n--- a/o.txt\n+++ b/n.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "diff --git a/x.sh b/x.sh\nold mode 100644\nnew mode 100755\n",
            "-- \n2.40.0\n",
        );
        let c = changes(diff, "p1").unwrap();
        let of = |path: &str| c.iter().find(|v| v["currentPath"] == path).unwrap().clone();
        let paths: Vec<&str> = c.iter().map(|v| v["currentPath"].as_str().unwrap()).collect();
        assert_eq!(paths, ["a.txt", "b.txt", "c.txt", "n.txt", "o.txt", "x.sh"], "sorted by path");
        let a = of("a.txt");
        assert_eq!((a["type"].clone(), a["oldPath"].clone(), a["commitHash"].clone()), (json!(CHANGE), json!("a.txt"), json!("p1")));
        assert_eq!(a["oldProperties"], json!({}));
        assert_eq!(
            a["hunks"],
            json!([{"oldOffset": 1, "oldLength": 2, "newOffset": 1, "newLength": 2, "addLines": 1, "delLines": 1,
                "isMissingOldNewline": false, "isMissingNewNewline": false, "corpus": " one\n-two\n+TWO\n"}])
        );
        let b = of("b.txt");
        assert_eq!((b["type"].clone(), b["oldPath"].clone(), b["newProperties"].clone()), (json!(ADD), Value::Null, json!({"unix:filemode": "100644"})));
        assert_eq!(b["hunks"][0]["corpus"], "+bee\n\\ No newline at end of file\n");
        assert_eq!((b["hunks"][0]["isMissingOldNewline"].clone(), b["hunks"][0]["isMissingNewNewline"].clone()), (json!(false), json!(true)));
        let d = of("c.txt");
        assert_eq!((d["type"].clone(), d["oldProperties"].clone()), (json!(DELETE), json!({"unix:filemode": "100755"})));
        let (n, o) = (of("n.txt"), of("o.txt"));
        assert_eq!((n["type"].clone(), n["oldPath"].clone()), (json!(MOVE_HERE), json!("o.txt")));
        assert_eq!((o["type"].clone(), o["awayPaths"].clone(), o["hunks"].clone()), (json!(MOVE_AWAY), json!(["n.txt"]), json!([])));
        let x = of("x.sh");
        assert_eq!((x["oldProperties"].clone(), x["newProperties"].clone()), (json!({"unix:filemode": "100644"}), json!({"unix:filemode": "100755"})));
        // The mail's signature is no line of the last hunk.
        assert_eq!(n["hunks"][0]["corpus"], "-old\n+new\n");
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "A U Thor")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_COMMITTER_NAME", "C O Mitter")
            .env("GIT_COMMITTER_EMAIL", "committer@example.com")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// A checkout whose `.arcconfig` names its Phabricator and its
    /// repository there, and a stack `s` of three patches of it: a.txt's
    /// line 2, then b.txt added, then a.txt's line 20, with messages
    /// `messages`. The database, and the checkout.
    fn stack(messages: [&str; 3]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path().join("src");
        std::fs::create_dir(&r).unwrap();
        git_ok(&r, &["init", "-q", "-b", "main"]);
        git_ok(&r, &["config", "commit.gpgsign", "false"]);
        std::fs::write(r.join(".arcconfig"), r#"{"phabricator.uri": "https://phabricator.example.com/", "repository.callsign": "TEST"}"#).unwrap();
        let lines: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        std::fs::write(r.join("a.txt"), &lines).unwrap();
        git_ok(&r, &["add", "."]);
        git_ok(&r, &["commit", "-qm", "base"]);
        std::fs::write(r.join("a.txt"), lines.replace("line 2\n", "LINE 2\n")).unwrap();
        git_ok(&r, &["commit", "-qam", messages[0]]);
        std::fs::write(r.join("b.txt"), "bee\n").unwrap();
        git_ok(&r, &["add", "."]);
        git_ok(&r, &["commit", "-qm", messages[1]]);
        std::fs::write(r.join("a.txt"), lines.replace("line 2\n", "LINE 2\n").replace("line 20\n", "LINE 20\n")).unwrap();
        git_ok(&r, &["commit", "-qam", messages[2]]);
        let db = tmp.path().join("db.jsonl");
        let read = crate::import::read_range(&r, "HEAD~3..HEAD", false, false).unwrap();
        Db::edit(&db, |d| crate::ops::import(d, "s", read.drafts, false, Some((read.base, r.display().to_string())))).unwrap();
        (tmp, db, r)
    }

    /// A revision as the fake has it.
    #[derive(Clone, Default)]
    struct Rev {
        status: String,
        draft: bool,
        title: String,
        summary: String,
        bug: String,
        diffs: Vec<u64>,
        reviewers: Vec<(String, bool)>,
        parents: Vec<String>,
    }

    /// Phabricator as the tests have it: revisions by number, diffs by id,
    /// its users and groups; each call made, and those to refuse (a
    /// method, or a method and a transaction's type: `edit:parents.set`).
    #[derive(Default)]
    struct Fake {
        revisions: BTreeMap<u64, Rev>,
        diffs: BTreeMap<u64, String>,
        users: Vec<(&'static str, &'static str)>,
        groups: Vec<(&'static str, &'static str)>,
        calls: Vec<(String, Value)>,
        refuse: Vec<&'static str>,
    }

    const WRITES: [&str; 3] = ["differential.creatediff", "differential.setdiffproperty", "differential.revision.edit"];

    impl Fake {
        /// Revision `n` in work, its latest diff `raw`.
        fn with(mut self, n: u64, status: &str, raw: &str, title: &str) -> Fake {
            let id = 1000 + self.diffs.len() as u64;
            self.diffs.insert(id, raw.to_string());
            let bug = wip_title(title).bug.unwrap_or_default();
            self.revisions.insert(n, Rev { status: status.into(), title: title.into(), bug, diffs: vec![id], ..Default::default() });
            self
        }

        fn writes(&self) -> Vec<String> {
            self.calls.iter().filter(|(m, _)| WRITES.contains(&m.as_str())).map(|(m, p)| {
                let kinds: Vec<&str> = p["transactions"].as_array().into_iter().flatten().filter_map(|t| t["type"].as_str()).collect();
                if kinds.is_empty() { m.clone() } else { format!("{m} {}", kinds.join(",")) }
            }).collect()
        }

        fn revision(&self, n: u64) -> Value {
            let r = &self.revisions[&n];
            let reviewers: Vec<Value> = r.reviewers.iter().map(|(p, b)| json!({"reviewerPHID": p, "isBlocking": b})).collect();
            let name = match r.status.as_str() { "changes-planned" => "Changes Planned", "needs-review" => "Needs Review", "draft" => "Draft", "published" => "Closed", _ => "?" };
            json!({"id": n, "phid": format!("PHID-DREV-{n}"), "fields": {
                "title": r.title, "summary": r.summary, "status": {"value": r.status, "name": name}, "isDraft": r.draft,
                "bugzilla.bug-id": r.bug, "repositoryPHID": "PHID-REPO-1"},
                "attachments": {"reviewers": {"reviewers": reviewers}}})
        }

        fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
            self.calls.push((method.to_string(), p.clone()));
            let kinds: Vec<String> = p["transactions"].as_array().into_iter().flatten().filter_map(|t| t["type"].as_str()).map(|k| format!("edit:{k}")).collect();
            if self.refuse.iter().any(|r| *r == method || kinds.iter().any(|k| k == r)) {
                return Err(format!("{method}: refused"));
            }
            let num = |v: &Value| v.as_str().map_or(v.as_u64(), |s| s.trim_start_matches("PHID-DREV-").trim_start_matches('D').parse().ok());
            Ok(match method {
                "differential.revision.search" => {
                    let c = &p["constraints"];
                    let ns: Vec<u64> = c["ids"].as_array().or(c["phids"].as_array()).into_iter().flatten().filter_map(num).collect();
                    json!({"data": ns.iter().filter(|n| self.revisions.contains_key(n)).map(|n| self.revision(*n)).collect::<Vec<_>>()})
                }
                "edge.search" => {
                    let n = num(&p["sourcePHIDs"][0]).unwrap();
                    json!({"data": self.revisions[&n].parents.iter().map(|d| json!({"destinationPHID": d})).collect::<Vec<_>>()})
                }
                "differential.querydiffs" => {
                    let n = p["revisionIDs"][0].as_u64().unwrap();
                    Value::Object(self.revisions[&n].diffs.iter().map(|d| (d.to_string(), json!({}))).collect())
                }
                "differential.getrawdiff" => json!(self.diffs[&p["diffID"].as_u64().unwrap()]),
                "diffusion.repository.search" => json!({"data": [{"phid": "PHID-REPO-1", "fields": {"vcs": "git"}}]}),
                "user.search" | "project.search" => {
                    let (list, field, mark) = if method == "user.search" { (&self.users, "username", "") } else { (&self.groups, "slug", "#") };
                    let c = &p["constraints"];
                    let wanted: Vec<String> = c["usernames"].as_array().or(c["slugs"].as_array()).or(c["phids"].as_array()).into_iter().flatten().filter_map(|v| v.as_str()).map(|s| s.trim_start_matches('#').to_lowercase()).collect();
                    let found: Vec<Value> = list.iter().filter(|(name, phid)| wanted.contains(&name.to_lowercase()) || wanted.contains(&phid.to_lowercase())).map(|(name, phid)| json!({"phid": phid, "fields": {field: name}})).collect();
                    let _ = mark;
                    json!({"data": found, "maps": {"slugMap": {}}})
                }
                "differential.creatediff" => {
                    let id = 2000 + self.diffs.len() as u64;
                    self.diffs.insert(id, render(&p["changes"]));
                    json!({"diffid": id, "phid": format!("PHID-DIFF-{id}")})
                }
                "differential.setdiffproperty" => Value::Null,
                "differential.revision.edit" => {
                    let n = match p.get("objectIdentifier") {
                        Some(o) => num(o).unwrap(),
                        None => {
                            let n = 200 + self.revisions.len() as u64;
                            self.revisions.insert(n, Rev { status: "needs-review".into(), ..Default::default() });
                            n
                        }
                    };
                    for t in p["transactions"].as_array().unwrap() {
                        let r = self.revisions.get_mut(&n).unwrap();
                        let v = &t["value"];
                        match t["type"].as_str().unwrap() {
                            "title" => r.title = v.as_str().unwrap().into(),
                            "summary" => r.summary = v.as_str().unwrap().into(),
                            "bugzilla.bug-id" => r.bug = v.as_str().unwrap().into(),
                            "update" => {
                                r.diffs.push(v.as_str().unwrap().trim_start_matches("PHID-DIFF-").parse().unwrap());
                                // A new diff asks for review again.
                                if !r.draft { r.status = "needs-review".into() }
                            }
                            "plan-changes" if r.status == "changes-planned" => return Err("plan-changes: it is Changes Planned already".into()),
                            "plan-changes" => r.status = "changes-planned".into(),
                            "parents.add" | "parents.set" => r.parents = v.as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect(),
                            "reviewers.set" => {
                                r.reviewers = v.as_array().unwrap().iter().map(|p| {
                                    let p = p.as_str().unwrap();
                                    match p.strip_prefix("blocking(") { Some(b) => (b.trim_end_matches(')').to_string(), true), None => (p.to_string(), false) }
                                }).collect();
                                r.reviewers.sort();
                            }
                            k => return Err(format!("no transaction {k}")),
                        }
                    }
                    json!({"object": {"id": n, "phid": format!("PHID-DREV-{n}")}})
                }
                m => return Err(format!("not as Phabricator is asked: {m}")),
            })
        }
    }

    /// A diff as getrawdiff writes the changes creatediff was given.
    fn render(changes: &Value) -> String {
        let mut out = String::new();
        for c in changes.as_array().unwrap() {
            let cur = c["currentPath"].as_str().unwrap();
            let old = c["oldPath"].as_str().unwrap_or(cur);
            match c["type"].as_u64().unwrap() {
                1 => out += &format!("diff --git a/{cur} b/{cur}\nnew file mode {}\n--- /dev/null\n+++ b/{cur}\n", c["newProperties"]["unix:filemode"].as_str().unwrap()),
                3 => out += &format!("diff --git a/{cur} b/{cur}\ndeleted file mode {}\n--- a/{cur}\n+++ /dev/null\n", c["oldProperties"]["unix:filemode"].as_str().unwrap()),
                4 | 5 | 8 => continue,
                _ => out += &format!("diff --git a/{old} b/{cur}\n--- a/{old}\n+++ b/{cur}\n"),
            }
            for h in c["hunks"].as_array().unwrap() {
                out += &format!("@@ -{},{} +{},{} @@\n{}", h["oldOffset"], h["oldLength"], h["newOffset"], h["newLength"], h["corpus"].as_str().unwrap());
            }
        }
        out
    }

    /// Prepares the submit of patch `at` of `s` with `fake`, then sends it:
    /// what was prepared, and what came of it.
    fn submit(db: &Path, at: usize, fake: &mut Fake) -> (Result<Prepared, String>, Option<String>) {
        let d = Db::load(db).unwrap();
        let s = d.stack("s").unwrap().clone();
        let arc = arcconfig(Path::new(s.repo.as_deref().unwrap()));
        let ready = prepare_with(&d, &s, at, "https://phabricator.example.com", &arc, &mut |m, v| fake.call(m, v));
        let sent = ready.as_ref().ok().map(|p| send_with(db, p, &mut |m, v| fake.call(m, v)));
        (ready, sent)
    }

    fn diff_of(db: &Path, at: usize) -> String {
        let d = Db::load(db).unwrap();
        d.patch(&d.stack("s").unwrap().patches[at]).diff.clone()
    }

    fn message_of(db: &Path, at: usize) -> String {
        let d = Db::load(db).unwrap();
        d.patch(&d.stack("s").unwrap().patches[at]).message.clone()
    }

    const LINK: &str = "Differential Revision: https://phabricator.example.com";

    #[test]
    fn a_new_revision_is_made_in_changes_planned_its_parent_and_reviewers_set_and_its_link_goes_into_the_patch() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", &format!("Bug 2 - two\n\n{LINK}/D12"), "Bug 3 - three r=alice,#build!\n\nBecause."]);
        let mut fake = Fake { users: vec![("alice", "PHID-USER-a")], groups: vec![("build", "PHID-PROJ-b")], ..Default::default() }.with(12, "needs-review", "", "Bug 2");
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!(p.said()[0], "Patch 3 is sent to Phabricator, at https://phabricator.example.com, as a new revision, with the status Changes Planned. Its title is \"WIP: Bug 3 - three r=alice,#build!\", and its bug is 3.");
        assert_eq!(p.said()[1], "Its parent is set to D12, the revision of patch 2.");
        assert_eq!(p.said()[2], "The reviewers of the new revision are set to alice and #build (blocking).");
        assert_eq!(
            fake.writes(),
            [
                "differential.creatediff",
                "differential.setdiffproperty",
                "differential.revision.edit title,summary,bugzilla.bug-id,update,plan-changes",
                "differential.revision.edit parents.add",
                "differential.revision.edit reviewers.set",
            ]
        );
        assert_eq!(sent.unwrap(), "Patch 3 is D201 on Phabricator now, in Changes Planned. The message of patch 3 names D201 now. The parent of D201 is D12 now. The reviewers of D201 are alice and #build (blocking).");
        let made = &fake.revisions[&201];
        assert_eq!((made.status.as_str(), made.title.as_str(), made.summary.as_str(), made.bug.as_str()), ("changes-planned", "WIP: Bug 3 - three r=alice,#build!", "Because.", "3"));
        assert_eq!((made.parents.clone(), made.reviewers.clone()), (vec!["PHID-DREV-12".to_string()], vec![("PHID-PROJ-b".to_string(), true), ("PHID-USER-a".to_string(), false)]));
        // The diff as moz-phab makes one, on the stack's base, and its commit.
        let diff = fake.calls.iter().find(|(m, _)| m == "differential.creatediff").unwrap().1.clone();
        let d = Db::load(&db).unwrap();
        let base = d.stack("s").unwrap().base.clone().unwrap();
        assert_eq!((diff["sourceControlBaseRevision"].as_str(), diff["repositoryPHID"].as_str(), diff["sourceControlSystem"].as_str()), (Some(base.as_str()), Some("PHID-REPO-1"), Some("git")));
        let property = fake.calls.iter().find(|(m, _)| m == "differential.setdiffproperty").unwrap().1.clone();
        let commits: Value = serde_json::from_str(property["data"].as_str().unwrap()).unwrap();
        let commit = commits.as_object().unwrap().values().next().unwrap();
        assert_eq!((commit["author"].as_str(), commit["authorEmail"].as_str()), (Some("A U Thor"), Some("author@example.com")));
        // The patch names it now, a new version, and the history says why.
        assert_eq!(message_of(&db, 2), format!("Bug 3 - three r=alice,#build!\n\nBecause.\n\n{LINK}/D201"));
        let s = d.stack("s").unwrap();
        assert_eq!(s.op, "submitted patch 3 as D201, in work");
        assert_eq!(d.versions(&d.patch(&s.patches[2]).change).len(), 2);
    }

    #[test]
    fn a_revision_in_work_takes_a_new_diff_and_stays_changes_planned() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        // Its latest diff is the version of the patch before the one in the
        // stack.
        let earlier = diff_of(&db, 2);
        let mine = earlier.replace("+LINE 20", "+Line twenty");
        Db::edit(&db, |d| {
            let old = d.patch(&d.stack("s")?.patches[2]).clone();
            crate::ops::replace(d, "s", 2, crate::mbox::Draft { diff: mine.clone(), ..crate::mbox::Draft::from(&old) }, "edit").map(|_| ())
        })
        .unwrap();
        let mut fake = Fake::default().with(13, "changes-planned", &earlier, "WIP: Bug 3 - three");
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert!(p.new_diff && !p.known.as_ref().unwrap().current, "{:?}", p.known);
        assert_eq!(p.said()[0], "D13, at https://phabricator.example.com/D13, gets a new diff of patch 3, and stays Changes Planned.");
        assert_eq!(p.said()[1], "Patch 2 has no revision, so D13 gets no parent.");
        // The code: a.txt as Phabricator's version leaves it, then this one.
        let code = p.code.clone().unwrap();
        assert!(code.contains("-LINE 20\n+Line twenty\n") && code.starts_with("diff --git a/a.txt b/a.txt\n"), "{code}");
        assert_eq!(p.known.as_ref().map(|k| k.version), Some(1));
        assert_eq!(
            fake.writes(),
            ["differential.creatediff", "differential.setdiffproperty", "differential.revision.edit update", "differential.revision.edit plan-changes"]
        );
        assert_eq!(sent.unwrap(), "D13 has the new diff of patch 3 now. D13 stays Changes Planned. Patch 2 has no revision, so D13 gets no parent.");
        assert_eq!(fake.revisions[&13].status, "changes-planned");
        // Sent again: nothing changed, nothing is sent.
        fake.calls.clear();
        let (ready, _) = {
            let d = Db::load(&db).unwrap();
            let s = d.stack("s").unwrap().clone();
            let arc = arcconfig(Path::new(s.repo.as_deref().unwrap()));
            (prepare_with(&d, &s, 2, "https://phabricator.example.com", &arc, &mut |m, v| fake.call(m, v)), ())
        };
        let p = ready.unwrap();
        assert!(p.nothing(), "{p:?}");
        assert_eq!(p.nothing_said(), "D13 is patch 3 as it is already, with its title and summary, so nothing was sent. Patch 2 has no revision, so D13 gets no parent.");
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn a_new_message_alone_sends_no_new_diff() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\nWhy, said again.\n\n{LINK}/D13")]);
        let mut fake = Fake::default().with(13, "changes-planned", &diff_of(&db, 2), "WIP: Bug 3 - three");
        fake.revisions.get_mut(&13).unwrap().summary = "Why.".into();
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert!(!p.new_diff && p.known.as_ref().unwrap().current);
        assert_eq!(p.code, None);
        assert_eq!(p.message_before().unwrap(), "WIP: Bug 3 - three\n\nWhy.");
        assert_eq!(fake.writes(), ["differential.revision.edit summary"]);
        assert_eq!(sent.unwrap(), "D13 has the title and summary of patch 3 now. Patch 2 has no revision, so D13 gets no parent.");
    }

    #[test]
    fn a_latest_diff_of_the_same_change_with_more_context_is_the_patch_as_it_is() {
        let (_tmp, db, r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        // As moz-phab sends it: more context than the stack's diff has.
        let wide = git_ok(&r, &["diff", "-U10", "HEAD~1", "HEAD"]);
        assert!(!crate::ops::same_effect(&wide, &diff_of(&db, 2)));
        let mut fake = Fake::default().with(13, "changes-planned", &wide, "WIP: Bug 3 - three");
        let (ready, _) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!((p.new_diff, p.known.as_ref().map(|k| k.current)), (false, Some(true)));
        assert!(p.nothing(), "{p:?}");
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn a_latest_diff_that_is_no_version_of_the_patch_is_refused_and_nothing_is_written() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let theirs = diff_of(&db, 2).replace("+LINE 20", "+Someone else's 20");
        let mut fake = Fake::default().with(13, "changes-planned", &theirs, "WIP: Bug 3 - three");
        let (ready, _) = submit(&db, 2, &mut fake);
        assert_eq!(ready.unwrap_err(), "D13's latest diff is not a version of patch 3 that stacked knows: it came from elsewhere, and nothing was sent.");
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn a_revision_not_in_work_is_refused_and_a_draft_is_put_in_changes_planned() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let mut fake = Fake::default().with(13, "needs-review", &diff_of(&db, 2), "WIP: Bug 3 - three");
        let (ready, _) = submit(&db, 2, &mut fake);
        assert_eq!(ready.unwrap_err(), "D13 is Needs Review, not Changes Planned: only a revision in work is updated, and nothing was sent.");
        assert!(fake.writes().is_empty());
        // A draft is in work, as moz-phab takes it: it goes to Changes Planned.
        let mut fake = Fake::default().with(13, "draft", &diff_of(&db, 2), "WIP: Bug 3 - three");
        fake.revisions.get_mut(&13).unwrap().draft = true;
        let (ready, sent) = submit(&db, 2, &mut fake);
        assert_eq!(ready.unwrap().said()[0], "D13, at https://phabricator.example.com/D13, keeps its diff, title and summary, as they are those of patch 3 already, and goes from Draft to Changes Planned.");
        assert_eq!(fake.writes(), ["differential.revision.edit plan-changes"]);
        assert_eq!(sent.unwrap(), "D13 is Changes Planned now. Patch 2 has no revision, so D13 gets no parent.");
    }

    #[test]
    fn a_failure_partway_says_what_was_done_and_what_was_not() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", "Bug 3 - three"]);
        let mut fake = Fake { refuse: vec!["edit:title"], ..Default::default() };
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(sent.unwrap(), "The diff of patch 3 was made on Phabricator, but no revision was made of it: differential.revision.edit: refused.");
        assert_eq!(message_of(&db, 2), "Bug 3 - three", "no link: there is no revision");
        let mut fake = Fake { refuse: vec!["differential.setdiffproperty"], ..Default::default() };
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(sent.unwrap(), "The diff of patch 3 was made on Phabricator, but its commit's author and message were not set, so no revision was made of it: differential.setdiffproperty: refused.");
        assert_eq!(fake.writes(), ["differential.creatediff", "differential.setdiffproperty"]);
        // Phabricator refuses the diff: nothing changed there.
        let mut fake = Fake { refuse: vec!["differential.creatediff"], ..Default::default() };
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(sent.unwrap(), "Phabricator refused the diff of patch 3, so nothing changed there: differential.creatediff: refused.");
    }

    #[test]
    fn the_parent_is_the_revision_of_the_patch_below_when_it_is_open() {
        let (_tmp, db, _r) = stack([&format!("Bug 1 - one\n\n{LINK}/D11"), &format!("Bug 2 - two\n\n{LINK}/D12"), "Bug 3 - three"]);
        // The bottom patch gets none, and nothing is asked of it.
        let mut fake = Fake::default().with(11, "changes-planned", &diff_of(&db, 0), "WIP: Bug 1 - one");
        let (ready, _) = submit(&db, 0, &mut fake);
        assert_eq!(ready.unwrap().said()[1], "Patch 1 is at the bottom of the stack, so D11 gets no parent.");
        assert!(!fake.writes().iter().any(|w| w.contains("parents")));
        // Its parent is D11 already: none is sent.
        let mut fake = Fake::default().with(11, "needs-review", "", "Bug 1").with(12, "changes-planned", &diff_of(&db, 1), "WIP: Bug 2 - two");
        fake.revisions.get_mut(&12).unwrap().parents = vec!["PHID-DREV-11".into()];
        let (ready, _) = submit(&db, 1, &mut fake);
        let p = ready.unwrap();
        assert_eq!((p.said()[1].as_str(), p.nothing()), ("Its parent is D11, the revision of patch 1, already.", true));
        assert!(fake.writes().is_empty());
        // Another parent: set to D11, in a call of its own; refused, the
        // rest is kept, and said.
        fake.revisions.get_mut(&12).unwrap().parents = vec!["PHID-DREV-99".into()];
        fake.refuse = vec!["edit:parents.set"];
        let (ready, sent) = submit(&db, 1, &mut fake);
        assert_eq!(ready.unwrap().parents_now, ["PHID-DREV-99"], "not found: its PHID");
        assert_eq!(fake.writes(), ["differential.revision.edit parents.set"]);
        assert_eq!(sent.unwrap(), "D12 keeps its diff, title and summary. But the parent of D12 was not set to D11: differential.revision.edit: refused.");
        // A closed revision below: none, its parents left as they are.
        fake.revisions.get_mut(&11).unwrap().status = "published".into();
        fake.refuse.clear();
        let (ready, _) = submit(&db, 1, &mut fake);
        assert_eq!(ready.unwrap().said()[1], "D11, the revision of patch 1, is Closed, so the parents of D12 are left as they are.");
    }

    #[test]
    fn the_reviewers_are_those_the_title_names_that_phabricator_has() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three r=alice,bobb r?#build\n\n{LINK}/D13")]);
        let title = "WIP: Bug 3 - three r?#build r=alice,bobb";
        let mut fake = Fake { users: vec![("alice", "PHID-USER-a")], groups: vec![("build", "PHID-PROJ-b")], ..Default::default() }.with(13, "changes-planned", &diff_of(&db, 2), title);
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!(p.said()[2], "No reviewer named bobb was found, so the reviewers of D13 are set to #build and alice.");
        assert_eq!(fake.writes(), ["differential.revision.edit reviewers.set"]);
        assert!(sent.unwrap().ends_with("No reviewer named bobb was found; the reviewers of D13 are #build and alice."));
        let set = fake.calls.iter().rev().find(|(m, _)| m == "differential.revision.edit").unwrap().1["transactions"][0]["value"].clone();
        assert_eq!(set, json!(["PHID-USER-a", "PHID-PROJ-b"]), "users first, then groups, as moz-phab sets them");
        // Already those: nothing is sent; the review says who they are.
        fake.calls.clear();
        let (ready, _) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!((p.reviewers_change(), p.reviewers_now.clone()), (false, vec!["#build".to_string(), "alice".to_string()]));
        assert!(fake.writes().is_empty());
        // Refused: the rest is kept, and said.
        fake.revisions.get_mut(&13).unwrap().reviewers.clear();
        fake.refuse = vec!["edit:reviewers.set"];
        let (_, sent) = submit(&db, 2, &mut fake);
        assert!(sent.unwrap().ends_with("But the reviewers of D13 were not set: differential.revision.edit: refused."));
    }

    #[test]
    fn a_patch_whose_commit_names_a_revision_it_does_not_is_held_back_alone() {
        use crate::head::{At, Differs, Head, OnStack};
        let head = |d: Differs, k: usize| Head {
            dir: PathBuf::from("/checkout"),
            branch: Some("main".into()),
            commit: "c".repeat(40),
            at: At::Stack(OnStack {
                commits: vec!["a".into(), "b".into(), "c".into()],
                messages: vec![(k, "its commit's message".into(), d)],
                above: vec![],
            }),
        };
        let names = |ours: Option<&str>, theirs: Option<&str>| Differs::Revision(ours.map(String::from), theirs.map(String::from));
        assert_eq!(
            held_by_head(&head(names(None, Some("D34")), 2), 2).as_deref(),
            Some("Patch 3 names no revision in the stack and D34 in your workspace's commit: sync the commit messages first, so that no second revision is made.")
        );
        assert_eq!(
            held_by_head(&head(names(Some("D13"), Some("D34")), 2), 2).as_deref(),
            Some("Patch 3 names D13 in the stack and D34 in your workspace's commit: sync the commit messages first, so that its diff goes to the revision you keep.")
        );
        // The stack's patch alone names one; what they say differs; another
        // patch of the stack; HEAD elsewhere: not held back.
        assert_eq!(held_by_head(&head(names(Some("D13"), None), 2), 2), None);
        assert_eq!(held_by_head(&head(Differs::Text, 2), 2), None);
        assert_eq!(held_by_head(&head(names(None, Some("D34")), 2), 0), None);
        let elsewhere = Head { at: At::Elsewhere, ..head(names(None, Some("D34")), 2) };
        assert_eq!(held_by_head(&elsewhere, 2), None);
    }

    #[test]
    fn a_refused_parent_or_reviewers_keeps_the_new_diff_and_says_so() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", &format!("Bug 2 - two\n\n{LINK}/D12"), &format!("Bug 3 - three r=alice\n\n{LINK}/D13")]);
        let earlier = diff_of(&db, 2);
        Db::edit(&db, |d| {
            let old = d.patch(&d.stack("s")?.patches[2]).clone();
            let draft = crate::mbox::Draft { diff: earlier.replace("+LINE 20", "+Line twenty"), ..crate::mbox::Draft::from(&old) };
            crate::ops::replace(d, "s", 2, draft, "edit").map(|_| ())
        })
        .unwrap();
        let mut fake = Fake { users: vec![("alice", "PHID-USER-a")], refuse: vec!["edit:parents.set", "edit:reviewers.set"], ..Default::default() }
            .with(12, "needs-review", "", "Bug 2 - two")
            .with(13, "changes-planned", &earlier, "WIP: Bug 3 - three r=alice");
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(
            sent.unwrap(),
            "D13 has the new diff of patch 3 now. D13 stays Changes Planned. But the parent of D13 was not set to D12: differential.revision.edit: refused. But the reviewers of D13 were not set: differential.revision.edit: refused."
        );
        assert_eq!(
            fake.writes(),
            [
                "differential.creatediff",
                "differential.setdiffproperty",
                "differential.revision.edit update",
                "differential.revision.edit plan-changes",
                "differential.revision.edit parents.set",
                "differential.revision.edit reviewers.set",
            ]
        );
        let r = &fake.revisions[&13];
        assert_eq!((r.diffs.len(), r.status.as_str(), r.parents.is_empty(), r.reviewers.is_empty()), (2, "changes-planned", true, true));
    }

    #[test]
    fn a_title_naming_no_reviewers_leaves_them_as_they_are() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let mut fake = Fake::default().with(13, "changes-planned", &diff_of(&db, 2), "WIP: Bug 3 - three");
        fake.revisions.get_mut(&13).unwrap().reviewers = vec![("PHID-USER-z".into(), false)];
        let (ready, _) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!(p.said()[2], "The title names no reviewers, so the reviewers of D13 are left as they are.");
        assert!(p.nothing(), "{p:?}");
        assert_eq!(p.reviewers_now, ["PHID-USER-z"], "not found: its PHID");
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn a_revision_changed_since_it_was_read_is_sent_nothing() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\nNew.\n\n{LINK}/D13")]);
        let mut fake = Fake::default().with(13, "changes-planned", &diff_of(&db, 2), "WIP: Bug 3 - three");
        let d = Db::load(&db).unwrap();
        let s = d.stack("s").unwrap().clone();
        let arc = arcconfig(Path::new(s.repo.as_deref().unwrap()));
        let p = prepare_with(&d, &s, 2, "https://phabricator.example.com", &arc, &mut |m, v| fake.call(m, v)).unwrap();
        fake.revisions.get_mut(&13).unwrap().title = "Someone's title".into();
        let sent = send_with(&db, &p, &mut |m, v| fake.call(m, v));
        assert_eq!(sent, "D13 changed on Phabricator since you reviewed it, so nothing was sent. Submit again to review it as it is now.");
        assert!(fake.writes().is_empty());
    }
}
