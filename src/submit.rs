//! A patch of a stack sent to Phabricator, one patch at a time, as
//! moz-phab's `submit --wip` sends a single commit: as a new revision, or
//! as a new diff of the revision its trailer (`Differential Revision:`)
//! names, in Changes Planned, the status of work in progress. To a revision
//! open but not in work (in review, accepted), only the patch's title,
//! summary and parent: no new diff, the status kept. Built with the
//! `moz-phab` feature.
//!
//! Nothing is sent before what will be is read and checked (`prepare`): a
//! revision that landed or was abandoned is refused, and so is one in work
//! whose latest diff is no version of the patch the database holds, as it
//! came from elsewhere. Then only what changed is sent (`send`): a new diff
//! and its commit's author and message, the title, the summary and the
//! bug, the status kept or put in Changes Planned; after that, in a call
//! of its own, so that a refusal undoes none of the rest, the parent set to
//! the revision of the patch below. No reviewers are set, as `moz-phab
//! submit --wip` sets none: those the title names stay in it, written as
//! moz-phab writes them. A new revision's link goes into the patch, as a
//! new version of it, at once. The workspace's commits are not touched.

use std::path::Path;
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::check::file_diff;
use crate::db::{Db, Patch, Stack};
// A word character as Python's `re` reads `\w`, and the first bug of a
// title as moz-phab reads it: a line of a message is read so too, for the
// bugs it names to open in the browser (`links`).
use crate::links::{bug_in, word};
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
            Via::Conduit(c) => c.call(method, params).map_err(String::from),
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

/// What moz-phab makes of a commit's title (its message's first line) to
/// submit it as work in progress: the revision's title, and the bug.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Titled {
    /// `WIP: ` and the title, its reviewers and its bug written as
    /// moz-phab writes them (`revision_title`).
    pub title: String,
    /// From `Bug 123` (or `b=123`): the first.
    pub bug: Option<String>,
}

/// The reviewers title `t` names, as moz-phab reads them
/// (`parse_reviewers`): those asked (`r?`), then those who gave it (`r=`),
/// each as written: `alice`, `bob!` (blocking), `#build` (a group).
fn reviewers_in(t: &str) -> (Vec<String>, Vec<String>) {
    let c: Vec<char> = t.chars().collect();
    let names = |kind: char| -> Vec<String> {
        specs(&c, &[kind])
            .iter()
            .filter_map(|m| m.names)
            .flat_map(|r| split_names(&c, r))
            .collect()
    };
    (names('?'), names('='))
}

/// Title `first` as moz-phab's `submit --wip` reads it, and what it sends
/// of it: the typo `r!` fixed, the reviewers asked (`r?`) and given (`r=`)
/// read, then written again as it writes them, as it does with `--wip` or
/// without, though with it it sets none; the first bug said `Bug N`; and
/// `WIP: ` before it all, in place of one written already.
pub fn wip_title(first: &str) -> Titled {
    let t = morph_blocking(first);
    let c: Vec<char> = t.chars().collect();
    let (request, granted) = reviewers_in(&t);
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
    }
}

/// Whether title `t` says the revision is in work, as moz-phab reads it,
/// and as `wip_title` takes it off: `WIP` first, in any case, then `:`, a
/// space or nothing.
pub fn title_in_work(t: &str) -> bool {
    let c: Vec<char> = t.chars().collect();
    c.len() >= 3 && c[..3].iter().collect::<String>().eq_ignore_ascii_case("wip") && matches!(c.get(3), None | Some(':' | ' '))
}

/// The title of commit title `first` as moz-phab writes it (`wip_title`),
/// `WIP: ` first only when `theirs`, the revision's title, has it: what a
/// submit sends a revision that is not in work, and how the Phabricator
/// line compares the title of any revision.
pub fn title_against(first: &str, theirs: &str) -> String {
    let wip = wip_title(first).title;
    match title_in_work(theirs) {
        true => wip,
        false => wip.strip_prefix("WIP: ").unwrap_or(&wip).to_string(),
    }
}

/// The title and summary of a revision, `title` and `summary` as its search
/// found them, and as patch `p` would send them, as a submit reads them
/// (`wip_title`, `summary_of`, the lines naming a revision left out of
/// both): its title `WIP: ` first, or not, as the revision's is
/// (`title_against`, which a submit to a revision not in work sends). None
/// when they are the same to the letter, the white space at their end
/// aside, as a submit compares them before it sends them: where their lines
/// wrap too, as that is what lands, so that a message wrapped again since it
/// was submitted differs until it is submitted again. Phabricator gives a
/// title and summary back as a submit sent them, over Conduit, to the
/// letter: nothing else is to be allowed for. What the Phabricator line
/// compares, and what a pull takes as no version of the patch's message.
pub(crate) fn message_against(title: &str, summary: &str, p: &Patch, trailer: &str) -> Option<(String, String)> {
    let before = format!("{title}\n\n{}", summary_of(&format!("\n{summary}"), trailer)).trim_end().to_string();
    let mine = title_against(p.title(), title);
    let after = format!("{mine}\n\n{}", summary_of(&p.message, trailer)).trim_end().to_string();
    (before != after).then_some((before, after))
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
/// Its reviewers are not read: a submit sets none, in work or not.
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
    /// Its parents' PHIDs, sorted.
    pub parents: Vec<String>,
    /// Its latest diff: its id, the commit it is on, as Phabricator names it
    /// (empty where it says none), and the diff, raw.
    pub diff: u64,
    pub base: String,
    pub raw: String,
}

impl Remote {
    /// Whether it is in work, as moz-phab takes it: Changes Planned, or a
    /// draft.
    pub fn in_work(&self) -> bool {
        self.status == "changes-planned" || self.draft
    }

    /// Whether it is open, as `phab::open` reads one: neither landed nor
    /// abandoned.
    pub fn open(&self) -> bool {
        !matches!(self.status.as_str(), "published" | "abandoned")
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
    let found = call("differential.revision.search", json!({"constraints": {"ids": [n]}}))?;
    let r = found.pointer("/data/0").cloned().ok_or(format!("Phabricator has no revision {id}"))?;
    let phid = r.get("phid").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut parents = crate::phab::edges(call, &phid, "revision.parent")?;
    parents.sort();
    let diffs = call("differential.querydiffs", json!({"revisionIDs": [n]}))?;
    let (diff, latest) = diffs
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.parse::<u64>().ok()?, v)))
        .max_by_key(|(k, _)| *k)
        .ok_or(format!("{id} has no diff"))?;
    let base = latest.get("sourceControlBaseRevision").and_then(Value::as_str).unwrap_or_default().to_string();
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
        parents,
        diff,
        base,
        raw,
    })
}

/// Stores in `db` what a submit read of revision `r` at `link`, as every
/// read of Phabricator is stored (`Db::store_revision`): its state, its
/// parents named by `names`, PHID by PHID, where the submit named them,
/// else as what is stored names them (`phab::parents_known`); and its
/// latest diff, raw, when it was read with it (`raw`). Its comments are
/// not read, and stay as they were stored.
fn store_read(db: &mut Db, link: &str, r: &Remote, names: Option<&[String]>, raw: bool) -> crate::db::Result<()> {
    let parents: Vec<(String, String)> = r.parents.iter().enumerate().map(|(k, p)| (p.clone(), names.and_then(|n| n.get(k)).cloned().unwrap_or_else(|| p.clone()))).collect();
    let state = crate::db::State {
        missing: false,
        status: r.status.clone(),
        title: r.title.clone(),
        summary: r.summary.clone(),
        bug: r.bug.clone(),
        phid: r.phid.clone(),
        parents: crate::phab::parents_known(db, &parents),
        diff: r.diff,
        base: r.base.clone(),
    };
    if raw {
        db.store_raw_diff(link, r.diff, &r.raw)?;
    }
    db.store_revision(link, &r.id, Some(state), None).map(|_| ())
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
/// from 1, oldest first; whether it is the version in the stack, as it
/// would be sent; its diff, as the database holds it; and whether it was
/// known by its changes alone (`ops::same_changes`), made on what was below
/// the patch before, so that it is never the version in the stack, and a
/// new diff of the patch is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub version: usize,
    pub current: bool,
    pub diff: String,
    pub by_changes: bool,
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

pub use crate::head::and;

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
    /// How its latest diff is a version of the patch: none for a revision
    /// not in work whose latest diff is no version of it.
    pub known: Option<Known>,
    /// The title, the summary and the bug sent.
    pub title: String,
    pub summary: String,
    pub bug: Option<String>,
    /// A new diff is made: the code changed, or the revision is new; never
    /// for a revision that is not in work.
    pub new_diff: bool,
    pub repository: Repository,
    pub parent: Parent,
    /// The revision's parents as they are, by revision: what the review
    /// says they were.
    pub parents_now: Vec<String>,
    /// The code from the revision's latest diff to the one sent, each file
    /// as both leave it on what is below the patch, as a diff; none when it
    /// is the same. And what that diff is, when it is not that; and whether
    /// it is the files so, rather than the two diffs diffed as text.
    pub code: Option<String>,
    pub code_said: Option<String>,
    pub code_files: bool,
    /// The diff the new diff is made of, when one is (`send`): the patch's
    /// own, as the stack has it.
    pub diff: String,
}

impl Prepared {
    /// The patch's number in the stack, from 1.
    pub fn number(&self) -> usize {
        self.at + 1
    }

    /// Whether the patch goes as work in progress: as a new revision, or to
    /// one in work. Else the revision is open but not in work, and gets the
    /// patch's title, summary and parent alone.
    pub fn in_work(&self) -> bool {
        self.revision.as_ref().is_none_or(Remote::in_work)
    }

    /// Why the code of the patch is not sent, when it differs from the
    /// revision's latest diff and the revision is not in work, said at the
    /// top of the review. None otherwise.
    pub fn kept_said(&self) -> Option<String> {
        let r = self.revision.as_ref().filter(|r| !r.in_work())?;
        if self.known.as_ref().is_some_and(|k| k.current) {
            return None;
        }
        Some(format!(
            "The code of patch {} is not sent: {} is {}, not in work, and only a revision in work gets a new diff.",
            self.number(),
            r.id,
            status_said(r)
        ))
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
    /// puts it back in Needs Review, as moz-phab says. Never for a revision
    /// not in work: its status stays as it is.
    fn plan_changes(&self) -> (bool, bool) {
        match &self.revision {
            None => (true, false),
            Some(r) if !r.in_work() => (false, false),
            Some(r) if r.status == "changes-planned" => (false, self.new_diff),
            Some(_) => (true, false),
        }
    }

    /// Whether the edit that sends the rest is made.
    fn main_edit(&self) -> bool {
        self.new_diff || self.message_changes() || self.plan_changes().0
    }

    /// Whether nothing at all is to be sent.
    pub fn nothing(&self) -> bool {
        !self.main_edit() && !self.parent_changes()
    }

    /// Whether the parent changes: what the review lists under what else
    /// changes.
    pub fn parent_changes(&self) -> bool {
        matches!(self.parent, Parent::Set { .. })
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

    /// What becomes of the status, said: `stays Changes Planned`; for a
    /// revision not in work, `stays Needs Review`.
    pub fn status_change(&self) -> String {
        match &self.revision {
            Some(r) if !r.in_work() => format!("stays {}", status_said(r)),
            Some(r) if r.status != "changes-planned" => format!("goes from {} to Changes Planned", status_said(r)),
            Some(_) => "stays Changes Planned".into(),
            None => "is Changes Planned".into(),
        }
    }

    /// What is sent, said: what to where, and its status; then its parent.
    /// For the dialog, and over the review.
    pub fn said(&self) -> Vec<String> {
        let n = self.number();
        let mut out = vec![match &self.revision {
            None => format!(
                "Patch {n} is sent to Phabricator, at {}, as a new revision, with the status Changes Planned. Its title is \"{}\"{}.",
                self.url,
                self.title,
                self.bug.as_ref().map_or(", and it names no bug".into(), |b| format!(", and its bug is {b}"))
            ),
            // Not in work: no diff, whatever the code; the review's top
            // says whether it differs.
            Some(r) if !r.in_work() => {
                let what = match (self.text_changes(), self.bug_changes()) {
                    (true, _) => format!("gets the title and summary of patch {n}"),
                    (false, true) => format!("gets the bug of patch {n}"),
                    (false, false) => format!("keeps its title and summary, as they are those of patch {n} already"),
                };
                format!("{}, at {}/{}, {what}, and {}.", r.id, self.url, r.id, self.status_change())
            }
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
        out
    }

    /// Why a new diff is sent when the revision's latest diff was known by
    /// its changes alone (`Known::by_changes`), said at the top of the
    /// review: though it makes the changes of a version of the patch, it was
    /// made on what was below the patch before, and no longer applies there.
    /// None otherwise, and for a revision not in work, which gets no diff.
    pub fn changes_said(&self) -> Option<String> {
        let k = self.known.as_ref().filter(|k| k.by_changes && self.new_diff)?;
        let id = self.revision.as_ref().map_or("the revision".to_string(), |r| r.id.clone());
        let n = self.number();
        Some(format!(
            "The latest diff of {id} makes the same changes as version {} of patch {n}, but it was made on the patches below patch {n} as they were before, and it no longer applies on them as they are now, so a new diff of patch {n} is sent, made on them.",
            k.version
        ))
    }

    /// What is done of the parent, said.
    fn parent_said(&self) -> String {
        match &self.parent {
            Parent::Set { id, below, .. } => format!("Its parent is set to {id}, the revision of patch {below}."),
            Parent::Already { id, below } => format!("Its parent is {id}, the revision of patch {below}, already."),
            Parent::Nothing(why) => why.clone(),
        }
    }

    /// What the status line says when nothing is to be sent: and why no
    /// parent is set, if that is so. For a revision not in work whose code
    /// differs, why the code is not sent.
    pub fn nothing_said(&self) -> String {
        let r = self.revision.as_ref().map_or("The revision".into(), |r| r.id.clone());
        let n = self.number();
        let mut out = match self.revision.as_ref().filter(|_| self.kept_said().is_some()) {
            Some(remote) => vec![
                format!("{r} is {}, not in work, so the code of patch {n} is not sent.", status_said(remote)),
                format!("The title and summary of {r} are those of patch {n} already, so nothing was sent."),
            ],
            None => vec![format!("{r} is patch {n} as it is already, with its title and summary, so nothing was sent.")],
        };
        if let Parent::Nothing(why) = &self.parent {
            out.push(why.clone());
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
/// When neither rule finds one, the first, in the same order, that makes
/// the same changes (`ops::same_changes`): moz-phab sends a diff with more
/// context than the database keeps, and once a rebase has changed lines of
/// that context, the diff no longer applies on what is below the patch.
/// Known so, it is never the version in the stack, as that diff was made on
/// what was below the patch before: a new diff of the patch is sent.
pub(crate) fn known(db: &Db, s: &Stack, at: usize, raw: &str) -> Option<Known> {
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
    let strict = effect(current)
        .or_else(|| files(current))
        .map(|k| current[k])
        .or_else(|| effect(earlier).or_else(|| files(earlier)).map(|k| earlier[k]));
    if let Some((version, v)) = strict {
        return Some(Known {
            version,
            current: v.id == p.id,
            diff: v.diff.clone(),
            by_changes: false,
        });
    }
    let (version, v) = *list.iter().find(|(_, v)| crate::ops::same_changes(raw, &v.diff))?;
    Some(Known {
        version,
        current: false,
        diff: v.diff.clone(),
        by_changes: true,
    })
}

/// The revision of the patch below, as the parent's rule reads it
/// (`parent_with`): not read, and why; not on Phabricator; or its PHID,
/// whether it is open, and its status, by its name.
#[derive(Debug, Clone, PartialEq)]
pub enum Below {
    Unread(String),
    Missing,
    Found { phid: String, open: bool, status: String },
}

/// The parent the revision of the patch at `at` (from 0) of stack `s` is
/// given, as a submit gives it: the revision of the patch below, when it
/// is open, read with `below` by its `D123` and its link, unless the rule
/// says sooner that none is set. `now`, the revision as it is, its `D123`
/// and its parents' PHIDs, sorted; none for a new one. What `below` could
/// not say, given back: the Phabricator line reads the revision below
/// from what is known of it, which may not be yet.
pub fn parent_with<E>(db: &Db, s: &Stack, at: usize, now: Option<(&str, &[String])>, below: impl FnOnce(&str, &str) -> Result<Below, E>) -> Result<Parent, E> {
    let me = now.map_or("the new revision".to_string(), |(id, _)| id.to_string());
    let left = match now {
        Some((_, parents)) if !parents.is_empty() => format!("so the parents of {me} are left as they are"),
        _ => format!("so {me} gets no parent"),
    };
    if at == 0 {
        return Ok(Parent::Nothing(format!("Patch 1 is at the bottom of the stack, {left}.")));
    }
    let p = db.patch(&s.patches[at - 1]);
    let Some((d, link)) = crate::phab::revision_of(&p.message) else {
        return Ok(Parent::Nothing(format!("Patch {at} has no revision, {left}.")));
    };
    if now.is_some_and(|(id, _)| id == d) {
        return Ok(Parent::Nothing(format!("Patch {at} names {d} too, {left}.")));
    }
    Ok(match below(&d, &link)? {
        Below::Unread(e) => Parent::Nothing(format!("The revision of patch {at}, {d}, could not be read, {left}: {e}.")),
        Below::Missing => Parent::Nothing(format!("Phabricator has no {d}, the revision of patch {at}, {left}.")),
        Below::Found { open: false, status, .. } => Parent::Nothing(format!("{d}, the revision of patch {at}, is {status}, {left}.")),
        Below::Found { phid, .. } => match now {
            Some((_, parents)) if parents == std::slice::from_ref(&phid) => Parent::Already { id: d, below: at },
            _ => Parent::Set { id: d, phid, below: at },
        },
    })
}

/// The parent the revision of the patch at `at` of stack `s` is given: the
/// revision of the patch below, read with `call`, when it is open. `now`,
/// the revision as it is; none for a new one.
fn parent(db: &Db, s: &Stack, at: usize, call: &mut Call, now: Option<&Remote>) -> Parent {
    let read = |d: &str, _: &str| -> Result<Below, std::convert::Infallible> {
        let n: u64 = d.trim_start_matches('D').parse().unwrap_or_default();
        Ok(match crate::phab::revision_by(call, json!({ "ids": [n] })) {
            Err(e) => Below::Unread(e),
            Ok(None) => Below::Missing,
            Ok(Some(r)) => Below::Found {
                phid: r.get("phid").and_then(Value::as_str).unwrap_or_default().to_string(),
                open: crate::phab::open(&r),
                status: r.pointer("/fields/status/name").and_then(Value::as_str).unwrap_or("closed").to_string(),
            },
        })
    };
    let Ok(p) = parent_with(db, s, at, now.map(|r| (r.id.as_str(), r.parents.as_slice())), read);
    p
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

/// The code from `theirs` to `mine`, two diffs of the patch at `at` of
/// stack `s`: each file either touches, as `theirs` leaves it and as `mine`
/// does, both applied on what is below the patch, as a diff; none when
/// none differs. None of it when either does not apply there, or the files
/// cannot be read.
fn code_diff(db: &Db, s: &Stack, at: usize, theirs: &str, mine: &str) -> Option<Option<String>> {
    let (repo, base) = (Path::new(s.repo.as_deref()?), s.base.as_deref()?);
    let below: Vec<&str> = s.patches[..at].iter().map(|id| db.patch(id).diff.as_str()).collect();
    files_diff(repo, base, &below, theirs, mine)
}

/// As `code_diff`, the patches below the patch `below`, applied in turn on
/// `base` in the repository at `repo`: the code from `theirs` to `mine`,
/// each file either touches as each leaves it there, as a diff; none when
/// no file differs. None of it when either does not apply there, or the
/// files cannot be read.
pub(crate) fn files_diff(repo: &Path, base: &str, below: &[&str], theirs: &str, mine: &str) -> Option<Option<String>> {
    let paths: Vec<String> = crate::check::paths_of(repo, &[theirs, mine]).into_iter().collect();
    let a = crate::check::left_by(repo, base, below, theirs, &paths).ok()??;
    let b = crate::check::left_by(repo, base, below, mine, &paths).ok()??;
    let out: String = paths.iter().zip(a.iter().zip(&b)).filter_map(|(p, (a, b))| file_diff(p, a, b)).collect();
    Some((!out.is_empty()).then_some(out))
}

/// Two diffs of the patch at `at`, `theirs` then `mine`, diffed as text:
/// as a diff of a file of their own, `patch-N.diff`.
pub(crate) fn diffs_diffed(at: usize, theirs: &str, mine: &str) -> String {
    let name = format!("patch-{}.diff", at + 1);
    let d = similar::TextDiff::from_lines(theirs, mine);
    let text = d.unified_diff().context_radius(3).header(&format!("a/{name}"), &format!("b/{name}")).to_string();
    format!("diff --git a/{name} b/{name}\n{text}")
}

/// The code the review shows, from the revision's latest diff, `raw`,
/// known as `k`, to the patch at `at` of stack `s`, `mine`: from the
/// database's version it is, else from `raw`, each file as it leaves it;
/// else, when neither applies on what is below the patch, the two diffs
/// diffed, as text. And what that is, said, when it is not the files; and
/// whether it is the files. Known by its changes alone, `raw` applies there
/// no more: the code is from the version whose changes it makes, said so.
fn code_change(db: &Db, s: &Stack, at: usize, k: &Known, raw: &str, mine: &str) -> (Option<String>, Option<String>, bool) {
    if k.by_changes {
        if let Some(code) = code_diff(db, s, at, &k.diff, mine) {
            let n = at + 1;
            let said = code.as_ref().map(|_| format!("Each file as version {} of patch {n} leaves it, then as patch {n} leaves it, both on the patches below patch {n}.", k.version));
            return (code, said, true);
        }
    } else {
        for theirs in [k.diff.as_str(), raw] {
            if let Some(code) = code_diff(db, s, at, theirs, mine) {
                return (code, None, true);
            }
        }
    }
    let said = "The files could not be read as what is below the patch leaves them, so this is Phabricator's latest diff against the diff that is sent, as text.";
    (Some(diffs_diffed(at, raw, mine)), Some(said.into()), false)
}

/// Why the patch at `at` (from 0) is not to be submitted while HEAD is
/// where `head` says, if it is not: HEAD on the stack, the patch's commit
/// in your workspace names a revision the patch does not, none or another,
/// as after `moz-phab submit` there: the commit is marked `!`. Submitted,
/// the patch would make a second revision, or send its diff to another
/// revision than the commit's. Any other patch, and any other difference,
/// is not held back.
pub fn held_by_head(head: &crate::head::Head, at: usize) -> Option<String> {
    let crate::head::Mark::Revision { theirs, ours } = head.mark(at)? else { return None };
    let said = format!("patch {} names {} in the stack and {theirs} in your workspace's commit", at + 1, ours.as_deref().unwrap_or("no revision"));
    let first = match ours {
        None => format!("give the patch {theirs} first, with Reconcile with HEAD on its menu, so that no second revision is made"),
        Some(_) => "make the patch and its commit name the same revision first, the patch with Reconcile with HEAD on its menu or the commit in your checkout, so that its diff goes to the revision you keep".into(),
    };
    Some(format!("{}{}: {first}.", said[..1].to_uppercase(), &said[1..]))
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
/// the `.arcconfig` of the stack's checkout. The revision it names, as it
/// was read, put in `read`, whatever comes of the rest. For a revision in
/// work, the patch must be one Phabricator can be sent (`sendable`), as
/// for a new one (`prepare` checks that before anything is read). A
/// revision open but not in work gets the title, the summary, the bug and
/// the parent alone: its title `WIP: ` first only if it has it
/// (`title_against`), its latest diff left as it is, of whatever version,
/// so that neither its repository nor the patch's files are looked at.
fn prepare_with(db: &Db, s: &Stack, at: usize, url: &str, arc: &Arc, call: &mut Call, read: &mut Option<Remote>) -> Result<Prepared, String> {
    let p = db.patch(&s.patches[at]);
    let n = at + 1;
    let titled = wip_title(p.title());
    let summary = summary_of(&p.message, &trailer());
    let (revision, known, repository) = match crate::phab::revision_of(&p.message) {
        Some((d, _)) => {
            let r = read_remote(call, &d, true).map_err(|e| format!("{d} could not be read from Phabricator, so nothing was sent: {e}"))?;
            *read = Some(r.clone());
            if !r.open() {
                return Err(format!("{d} is {}, not open: only an open revision is updated, and nothing was sent.", status_said(&r)));
            }
            if !r.in_work() {
                let k = known(db, s, at, &r.raw);
                let repository = Repository { phid: r.repository.clone(), vcs: "git".into() };
                (Some(r), k, repository)
            } else {
                sendable(p, n)?;
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
    let in_work = revision.as_ref().is_none_or(Remote::in_work);
    let new_diff = in_work && known.as_ref().is_none_or(|k| !k.current);
    let title = match &revision {
        Some(r) if !in_work => title_against(p.title(), &r.title),
        _ => titled.title,
    };
    let parent = parent(db, s, at, call, revision.as_ref());
    let parents_now = revision.as_ref().map_or_else(Vec::new, |r| revision_names(call, &r.parents));
    let (code, code_said, code_files) = match (&revision, &known) {
        (Some(r), Some(k)) if new_diff => code_change(db, s, at, k, &r.raw, &p.diff),
        _ => (None, None, false),
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
        title,
        summary,
        bug: titled.bug,
        new_diff,
        repository,
        parent,
        parents_now,
        code,
        code_said,
        code_files,
        diff: p.diff.clone(),
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
/// `path`, to Phabricator sends, read and checked: as work in progress, as
/// a new revision or to one in work; to one open but not in work, its
/// title, summary and parent alone. Or why nothing can be, said. Nothing
/// is sent. What it read of the revision is stored, whatever came of the
/// rest (`store_read`).
pub fn prepare(path: &Path, name: &str, at: usize) -> Result<Prepared, String> {
    let db = Db::load(path).map_err(|e| e.0)?;
    let s = db.stack(name).map_err(|e| e.0)?.clone();
    let p = s.patches.get(at).map(|id| db.patch(id).clone()).ok_or(format!("{name} has no patch {}", at + 1))?;
    crate::check::can_check(&s).map_err(|e| format!("{}, so nothing was sent.", e.0))?;
    let named = crate::phab::revision_of(&p.message);
    // A new revision is made of a diff; one there already is read first,
    // as one not in work takes none.
    if named.is_none() {
        sendable(&p, at + 1)?;
    }
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
    let mut read = None;
    let out = prepare_with(&db, &s, at, &url, &arc, &mut call, &mut read);
    // Stored as any read of Phabricator, the parents named as the submit
    // named them: the submit goes on whether that is written or not.
    if let (Some(r), Some((_, link))) = (&read, &named) {
        let names = out.as_ref().ok().map(|p| p.parents_now.clone());
        let _ = Db::edit(path, |d| store_read(d, link, r, names.as_deref(), true));
    }
    let mut out = out?;
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

/// The commit a submit says the patch is, as moz-phab says a commit is
/// (`local:commits`): `node`, as the checkout names it, and its parent;
/// and its tree.
#[derive(Debug, Clone, PartialEq)]
struct Made {
    node: String,
    parent: String,
    tree: String,
}

/// The commit the patch at `at` of stack `s` makes: the workspace's, when
/// HEAD in the checkout at `here` is on the stack and has the patch as a
/// commit, as `head::find` matches them, its parent the commit below it, or
/// the stack's base for the bottom patch, its tree the commit's; else the
/// patch's own id, on the stack's base, its tree the one the patches below
/// and the patch leave on the base, written in a check's index of the
/// whole base (`check::tree_of`). Why not, when the tree cannot be had.
fn made(db: &Db, s: &Stack, at: usize, here: Option<&Path>) -> Result<Made, String> {
    let (repo, base) = crate::ops::base_of(s).map_err(|e| e.0)?;
    let head = here.and_then(|h| crate::head::find(db, &s.name, h).ok().flatten());
    if let Some(h) = &head
        && let crate::head::At::Stack(on) = &h.at
        && let Some(sha) = on.commits.get(at)
    {
        let tree = crate::git::git(&h.dir, &["rev-parse", "--verify", "--quiet", &format!("{sha}^{{tree}}")]).map_err(|e| e.0)?;
        let parent = if at == 0 { base } else { on.commits[at - 1].clone() };
        return Ok(Made { node: sha.clone(), parent, tree: tree.trim().to_string() });
    }
    let diffs: Vec<&str> = s.patches[..=at].iter().map(|id| db.patch(id).diff.as_str()).collect();
    let tree = crate::check::tree_of(&repo, &base, &diffs).map_err(|e| e.0)?;
    Ok(Made { node: s.patches[at].clone(), parent: base, tree })
}

/// Commit `made` of patch `p`, as differential.setdiffproperty sets
/// `local:commits`, as moz-phab sets it (`set_diff_property`): its author,
/// when, its summary (`title`), its message, the commit and its parent as
/// Phabricator's repository names them, held in `vcs` (`public_node`), and
/// its tree; in a Mercurial one, the commit as the checkout names it too
/// (`rev`). By the commit, as the checkout names it.
fn commits(p: &Patch, title: &str, made: &Made, repo: &Path, vcs: &str) -> String {
    let author = p.author.clone().or_else(crate::ops::default_author).unwrap_or_default();
    let (name, email) = match author.rsplit_once(" <") {
        Some((n, e)) => (n.trim().to_string(), e.trim_end_matches('>').to_string()),
        None => (author.trim().to_string(), String::new()),
    };
    let time = p.date.as_deref().and_then(epoch).unwrap_or_else(crate::db::now);
    let mut commit = json!({
        "author": name,
        "authorEmail": email,
        "time": time,
        "summary": title,
        "message": p.message,
        "commit": public_node(repo, &made.node, vcs),
        "parents": [public_node(repo, &made.parent, vcs)],
        "tree": made.tree,
    });
    if vcs == "hg" {
        commit["rev"] = json!(made.node);
    }
    let mut data = serde_json::Map::new();
    data.insert(made.node.clone(), commit);
    Value::Object(data).to_string()
}

/// Revision `phid` edited with transaction `txn` alone, in a call of its
/// own.
fn edit(call: &mut Call, phid: &str, txn: Value) -> Result<Value, String> {
    call("differential.revision.edit", json!({"objectIdentifier": phid, "transactions": [txn]}))
}

/// Message `m` with trailer `line` put where moz-phab puts the trailer
/// naming a revision: at the end of its last paragraph when that is a
/// paragraph of trailers (`Key: value`, `head::ends_in_trailers`), else
/// after an empty line.
fn with_link(m: &str, line: &str) -> String {
    let m = m.trim_end();
    let lines: Vec<&str> = m.split('\n').collect();
    match crate::head::ends_in_trailers(&lines) {
        true => format!("{m}\n{line}"),
        false => format!("{m}\n\n{line}"),
    }
}

/// The patch of change `change`, in stack `name` of the database at
/// `path`, given the trailer naming revision `d` at `link` (`with_link`),
/// as a new version, at once: the history says the submit made it.
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
        let message = with_link(&old.message, &format!("{}: {link}", trailer()));
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
/// `path`, the commit it makes the workspace's when HEAD of the checkout
/// at `here` has it (`made`). What came of it, said: what was done and what
/// was not, when a call fails partway.
fn send_with(path: &Path, p: &Prepared, here: Option<&Path>, call: &mut Call) -> String {
    sentence(sent(path, p, here, call))
}

/// As `send_with`, what came of it said a sentence after another.
fn sent(path: &Path, p: &Prepared, here: Option<&Path>, call: &mut Call) -> String {
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
        let read = read_remote(call, &r.id, false);
        // Stored as any read of Phabricator: its parents named as the review
        // named them, while they are the same.
        if let (Ok(now), Some((_, link))) = (&read, crate::phab::revision_of(&patch.message)) {
            let names = (now.parents == r.parents).then_some(p.parents_now.as_slice());
            let _ = Db::edit(path, |d| store_read(d, &link, now, names, false));
        }
        match read {
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
    // The diff, with its commit: on the commit below it, as moz-phab says
    // one is, each of its files of that commit.
    let diff = match p.new_diff {
        false => None,
        true => {
            let commit = match made(&db, &s, p.at, here) {
                Ok(m) => m,
                Err(e) => return format!("The tree of patch {n}'s commit could not be worked out, so nothing was sent: {e}"),
            };
            let (node, base) = (public_node(Path::new(&repo), &commit.node, &p.repository.vcs), public_node(Path::new(&repo), &commit.parent, &p.repository.vcs));
            let changes = match changes(&patch.diff, &node) {
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
            let property = json!({"diff_id": id, "name": "local:commits", "data": commits(&patch, &p.title, &commit, Path::new(&repo), &p.repository.vcs)});
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
    // Best effort, on its own: the parent. No reviewers are set, as
    // moz-phab sets none with `--wip`.
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
    said.into_iter().map(sentence).collect::<Vec<_>>().join(" ")
}

/// Sends what `p` says to Phabricator, once the stack and the revision are
/// seen to be as they were when it was read, in the database at `path`,
/// HEAD looked at in the checkout at `here`: what came of it, said.
pub fn send(path: &Path, p: &Prepared, here: &Path) -> String {
    let via = match Via::to(p.from.as_deref()) {
        Ok(v) => v,
        Err(e) => return format!("Nothing was sent: {e}."),
    };
    let mut call = |m: &str, v: Value| via.call(m, v);
    send_with(path, p, Some(here), &mut call)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn a_title_is_read_as_moz_phab_reads_it() {
        let names = |t: &str| reviewers_in(&morph_blocking(t));
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let t = wip_title("Bug 1 - Fix foo r=alice,bob");
        assert_eq!((t.title.as_str(), t.bug.as_deref()), ("WIP: Bug 1 - Fix foo r=alice,bob", Some("1")));
        assert_eq!(names("Bug 1 - Fix foo r=alice,bob"), (vec![], s(&["alice", "bob"])));
        // Asked of a group and of one who blocks, given by another: each list
        // written again as moz-phab writes it, those asked first.
        assert_eq!(names("Bug 2 - x r?#build, carol! r=dave"), (s(&["#build", "carol!"]), s(&["dave"])));
        assert_eq!(wip_title("Bug 2 - x r?#build, carol! r=dave").title, "WIP: Bug 2 - x r?#build,carol! r=dave");
        // The typo `r!`: each blocking, given.
        assert_eq!(wip_title("Bug 3 - y r!erin,frank.").title, "WIP: Bug 3 - y r=erin!,frank!.");
        assert_eq!(names("Bug 3 - y r!erin,frank."), (vec![], s(&["erin!", "frank!"])));
        // Another's specifier, `sr=`, is none of its names; nor is one in a word.
        assert_eq!(names("Bug 4 - z r=ann, sr=bo"), (vec![], s(&["ann"])));
        assert_eq!(names("Bug 5 - zr=ann (r=bo)"), (vec![], s(&["bo"])));
        // The bug in any case, said `Bug N`; `WIP:` once; none named.
        let t = wip_title("bug 123 - lower");
        assert_eq!((t.title.as_str(), t.bug.as_deref()), ("WIP: Bug 123 - lower", Some("123")));
        assert_eq!(wip_title("WIP: Bug 6 - again").title, "WIP: Bug 6 - again");
        assert_eq!(wip_title("wip Bug 7").title, "WIP: Bug 7");
        let t = wip_title("Fix it");
        assert_eq!((t.title.as_str(), t.bug), ("WIP: Fix it", None));
        assert_eq!(names("Fix it"), (vec![], vec![]));
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
        parents: Vec<String>,
    }

    /// Phabricator as the tests have it: revisions by number, diffs by id,
    /// and the commits each diff says it is of (`local:commits`); each call
    /// made, and those to refuse (a method, or a method and a transaction's
    /// type: `edit:parents.set`). As Mozilla's Phabricator, it makes no
    /// revision of a diff whose commits lack their tree. It knows no users
    /// nor groups, nor reviewers: a submit in work asks nothing of them.
    #[derive(Default)]
    struct Fake {
        revisions: BTreeMap<u64, Rev>,
        diffs: BTreeMap<u64, String>,
        commits: BTreeMap<u64, Value>,
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

        /// The calls of reviewers: their look-ups, and their edits.
        fn of_reviewers(&self) -> Vec<String> {
            let edits = |p: &Value| p["transactions"].as_array().into_iter().flatten().any(|t| t["type"].as_str().is_some_and(|k| k.starts_with("reviewers.")));
            self.calls
                .iter()
                .filter(|(m, p)| matches!(m.as_str(), "user.search" | "user.query" | "project.search") || edits(p))
                .map(|(m, _)| m.clone())
                .collect()
        }

        fn writes(&self) -> Vec<String> {
            self.calls.iter().filter(|(m, _)| WRITES.contains(&m.as_str())).map(|(m, p)| {
                let kinds: Vec<&str> = p["transactions"].as_array().into_iter().flatten().filter_map(|t| t["type"].as_str()).collect();
                if kinds.is_empty() { m.clone() } else { format!("{m} {}", kinds.join(",")) }
            }).collect()
        }

        fn revision(&self, n: u64) -> Value {
            let r = &self.revisions[&n];
            let name = match r.status.as_str() {
                "changes-planned" => "Changes Planned",
                "needs-review" => "Needs Review",
                "needs-revision" => "Needs Revision",
                "accepted" => "Accepted",
                "draft" => "Draft",
                "published" => "Closed",
                "abandoned" => "Abandoned",
                _ => "?",
            };
            json!({"id": n, "phid": format!("PHID-DREV-{n}"), "fields": {
                "title": r.title, "summary": r.summary, "status": {"value": r.status, "name": name}, "isDraft": r.draft,
                "bugzilla.bug-id": r.bug, "repositoryPHID": "PHID-REPO-1"}})
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
                "differential.creatediff" => {
                    let id = 2000 + self.diffs.len() as u64;
                    self.diffs.insert(id, render(&p["changes"]));
                    json!({"diffid": id, "phid": format!("PHID-DIFF-{id}")})
                }
                "differential.setdiffproperty" => {
                    let data: Value = serde_json::from_str(p["data"].as_str().unwrap()).unwrap();
                    self.commits.insert(p["diff_id"].as_u64().unwrap(), data);
                    Value::Null
                }
                "differential.revision.edit" => {
                    // As Mozilla's Phabricator: the diff's commits read,
                    // each its tree.
                    for t in p["transactions"].as_array().unwrap().iter().filter(|t| t["type"] == "update") {
                        let id: u64 = t["value"].as_str().unwrap().trim_start_matches("PHID-DIFF-").parse().unwrap();
                        let commits = self.commits.get(&id).and_then(Value::as_object).cloned().unwrap_or_default();
                        if commits.values().any(|c| c.get("tree").is_none()) {
                            return Err(format!("{method}: Undefined array key \"tree\""));
                        }
                    }
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
    /// what was prepared, and what came of it. HEAD is looked at in no
    /// checkout.
    fn submit(db: &Path, at: usize, fake: &mut Fake) -> (Result<Prepared, String>, Option<String>) {
        submit_in(db, at, None, fake)
    }

    /// As `submit`, HEAD looked at in the checkout at `here`.
    fn submit_in(db: &Path, at: usize, here: Option<&Path>, fake: &mut Fake) -> (Result<Prepared, String>, Option<String>) {
        let ready = prepared(db, at, fake);
        let sent = ready.as_ref().ok().map(|p| send_with(db, p, here, &mut |m, v| fake.call(m, v)));
        (ready, sent)
    }

    /// What submitting patch `at` of `s` sends, read with `fake`: nothing is
    /// sent.
    fn prepared(db: &Path, at: usize, fake: &mut Fake) -> Result<Prepared, String> {
        let d = Db::load(db).unwrap();
        let s = d.stack("s").unwrap().clone();
        let arc = arcconfig(Path::new(s.repo.as_deref().unwrap()));
        prepare_with(&d, &s, at, "https://phabricator.example.com", &arc, &mut |m, v| fake.call(m, v), &mut None)
    }

    /// Patch `at` of `s` given its next version at once, of diff `diff`.
    fn new_code(db: &Path, at: usize, diff: &str) {
        Db::edit(db, |d| {
            let old = d.patch(&d.stack("s")?.patches[at]).clone();
            crate::ops::replace(d, "s", at, crate::mbox::Draft { diff: diff.to_string(), ..crate::mbox::Draft::from(&old) }, "edit").map(|_| ())
        })
        .unwrap();
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
    fn a_new_revision_is_made_in_changes_planned_its_parent_set_and_its_link_goes_into_the_patch() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", &format!("Bug 2 - two\n\n{LINK}/D12"), "Bug 3 - three r=alice,#build! r?bob\n\nBecause."]);
        let mut fake = Fake::default().with(12, "needs-review", "", "Bug 2");
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        // The reviewers the title names stay in it, written as moz-phab
        // writes them, and none is set: neither said nor asked.
        assert_eq!(
            p.said(),
            [
                "Patch 3 is sent to Phabricator, at https://phabricator.example.com, as a new revision, with the status Changes Planned. Its title is \"WIP: Bug 3 - three r?bob r=alice,#build!\", and its bug is 3.",
                "Its parent is set to D12, the revision of patch 2.",
            ]
        );
        assert_eq!(
            fake.writes(),
            [
                "differential.creatediff",
                "differential.setdiffproperty",
                "differential.revision.edit title,summary,bugzilla.bug-id,update,plan-changes",
                "differential.revision.edit parents.add",
            ]
        );
        assert!(fake.of_reviewers().is_empty(), "{:?}", fake.of_reviewers());
        assert_eq!(sent.unwrap(), "Patch 3 is D201 on Phabricator now, in Changes Planned. The message of patch 3 names D201 now. The parent of D201 is D12 now.");
        let made = &fake.revisions[&201];
        assert_eq!((made.status.as_str(), made.title.as_str(), made.summary.as_str(), made.bug.as_str()), ("changes-planned", "WIP: Bug 3 - three r?bob r=alice,#build!", "Because.", "3"));
        assert_eq!(made.parents, ["PHID-DREV-12"]);
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
        assert_eq!(message_of(&db, 2), format!("Bug 3 - three r=alice,#build! r?bob\n\nBecause.\n\n{LINK}/D201"));
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
        // The diff the review compares with Phabricator's as the one sent,
        // the patch's, is the one sent: the new diff of D13 does what it does.
        // The code it shows is each file as both leave it, not the two diffs
        // diffed as text.
        assert_eq!(p.diff, mine);
        assert!(p.code_files);
        let made = *fake.revisions[&13].diffs.last().unwrap();
        assert!(made != 1000 && crate::ops::same_effect(&fake.diffs[&made], &p.diff), "{}", fake.diffs[&made]);
        // Sent again: nothing changed, nothing is sent.
        fake.calls.clear();
        let (ready, _) = {
            let d = Db::load(&db).unwrap();
            let s = d.stack("s").unwrap().clone();
            let arc = arcconfig(Path::new(s.repo.as_deref().unwrap()));
            (prepare_with(&d, &s, 2, "https://phabricator.example.com", &arc, &mut |m, v| fake.call(m, v), &mut None), ())
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

    /// Upstream's change of a.txt's line 12 in the checkout at `r`, within
    /// the ten lines of context moz-phab sends around line 20 but not within
    /// the three the stack's diffs keep, and the stack of the database `db`
    /// rebased onto it, imported again, each patch as it was. The new base.
    fn rebased_upstream(db: &Path, r: &Path) -> String {
        git_ok(r, &["checkout", "-q", "-b", "upstream", "HEAD~3"]);
        let lines: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        std::fs::write(r.join("a.txt"), lines.replace("line 12\n", "line twelve\n")).unwrap();
        git_ok(r, &["commit", "-qam", "upstream"]);
        git_ok(r, &["checkout", "-q", "main"]);
        git_ok(r, &["rebase", "-q", "upstream"]);
        let read = crate::import::read_range(r, "upstream..main", false, false).unwrap();
        Db::edit(db, |d| crate::ops::import(d, "s", read.drafts, true, Some((read.base, r.display().to_string())))).unwrap();
        git_ok(r, &["rev-parse", "upstream"]).trim().to_string()
    }

    #[test]
    fn a_latest_diff_whose_wider_context_a_rebase_changed_is_known_by_its_changes_and_sent_again() {
        let (_tmp, db, r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        // As moz-phab sends it: ten lines of context around line 20 of a.txt,
        // from line 10 to line 30.
        let wide = git_ok(&r, &["diff", "-U10", "HEAD~1", "HEAD"]);
        let mut fake = Fake::default().with(13, "changes-planned", &wide, "WIP: Bug 3 - three");
        // On the base it was made on, the rules of before take it: the patch
        // as it is, and nothing to send.
        let (ready, _) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!(p.known.as_ref().map(|k| (k.version, k.current, k.by_changes)), Some((1, true, false)));
        assert!(p.nothing() && p.changes_said().is_none(), "{p:?}");
        // Rebased onto upstream's change, each patch as it was.
        let id = Db::load(&db).unwrap().stack("s").unwrap().patches[2].clone();
        let base = rebased_upstream(&db, &r);
        let d = Db::load(&db).unwrap();
        let s = d.stack("s").unwrap();
        assert_eq!((s.base.clone(), s.patches[2].clone()), (Some(base), id));
        // Phabricator's diff no longer applies on the patches below, but it
        // makes the changes of patch 3: known by them, never as the patch as
        // it is, and sent again, though the code is the patch's already.
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!(p.known.as_ref().map(|k| (k.version, k.current, k.by_changes)), Some((1, false, true)));
        assert!(p.new_diff && p.code.is_none(), "{p:?}");
        assert_eq!(
            p.changes_said().as_deref(),
            Some(
                "The latest diff of D13 makes the same changes as version 1 of patch 3, but it was made on the patches below patch 3 as they were before, and it no longer applies on them as they are now, so a new diff of patch 3 is sent, made on them."
            )
        );
        assert_eq!(p.said()[0], "D13, at https://phabricator.example.com/D13, gets a new diff of patch 3, and stays Changes Planned.");
        assert_eq!(
            fake.writes(),
            ["differential.creatediff", "differential.setdiffproperty", "differential.revision.edit update", "differential.revision.edit plan-changes"]
        );
        assert_eq!(sent.unwrap(), "D13 has the new diff of patch 3 now. D13 stays Changes Planned. Patch 2 has no revision, so D13 gets no parent.");
        // That diff, made on the new base, is the patch as it is: nothing is
        // sent again.
        fake.calls.clear();
        let (ready, _) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert_eq!(p.known.as_ref().map(|k| (k.current, k.by_changes)), Some((true, false)));
        assert!(p.nothing(), "{p:?}");
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn a_latest_diff_made_before_a_rebase_and_an_edit_shows_the_code_from_the_version_whose_changes_it_makes() {
        let (_tmp, db, r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let wide = git_ok(&r, &["diff", "-U10", "HEAD~1", "HEAD"]);
        rebased_upstream(&db, &r);
        // Then patch 3 takes one more file, as a lint edit gives one.
        Db::edit(&db, |d| {
            let old = d.patch(&d.stack("s")?.patches[2]).clone();
            let diff = format!("{}diff --git a/d.txt b/d.txt\nnew file mode 100644\n--- /dev/null\n+++ b/d.txt\n@@ -0,0 +1 @@\n+dee\n", old.diff);
            crate::ops::replace(d, "s", 2, crate::mbox::Draft { diff, ..crate::mbox::Draft::from(&old) }, "edit").map(|_| ())
        })
        .unwrap();
        let mut fake = Fake::default().with(13, "changes-planned", &wide, "WIP: Bug 3 - three");
        let (ready, sent) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        // Known by its changes as version 1, the one before the patch as it
        // is; the code from that version to this one, d.txt alone, said so.
        assert_eq!(p.known.as_ref().map(|k| (k.version, k.current, k.by_changes)), Some((1, false, true)));
        assert!(p.changes_said().unwrap().starts_with("The latest diff of D13 makes the same changes as version 1 of patch 3, but "), "{p:?}");
        let code = p.code.clone().unwrap();
        assert!(code.starts_with("diff --git a/d.txt b/d.txt\nnew file mode 100644\n") && !code.contains("a.txt"), "{code}");
        assert_eq!(p.code_said.as_deref(), Some("Each file as version 1 of patch 3 leaves it, then as patch 3 leaves it, both on the patches below patch 3."));
        assert_eq!(fake.writes()[0], "differential.creatediff");
        assert_eq!(sent.unwrap(), "D13 has the new diff of patch 3 now. D13 stays Changes Planned. Patch 2 has no revision, so D13 gets no parent.");
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
    fn a_revision_landed_or_abandoned_is_refused_and_a_draft_is_put_in_changes_planned() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        // Its title otherwise, its code another version: nothing is sent all
        // the same, nor read beyond the revision.
        for (status, name) in [("published", "Closed"), ("abandoned", "Abandoned")] {
            let mut fake = Fake::default().with(13, status, &diff_of(&db, 2).replace("+LINE 20", "+Line twenty"), "Bug 3 - an older title");
            let (ready, _) = submit(&db, 2, &mut fake);
            assert_eq!(ready.unwrap_err(), format!("D13 is {name}, not open: only an open revision is updated, and nothing was sent."));
            assert!(fake.writes().is_empty());
            let read: Vec<&str> = fake.calls.iter().map(|(m, _)| m.as_str()).collect();
            assert_eq!(read, ["differential.revision.search", "edge.search", "differential.querydiffs", "differential.getrawdiff"], "{status}");
        }
        // A draft is in work, as moz-phab takes it: it goes to Changes Planned.
        let mut fake = Fake::default().with(13, "draft", &diff_of(&db, 2), "WIP: Bug 3 - three");
        fake.revisions.get_mut(&13).unwrap().draft = true;
        let (ready, sent) = submit(&db, 2, &mut fake);
        assert_eq!(ready.unwrap().said()[0], "D13, at https://phabricator.example.com/D13, keeps its diff, title and summary, as they are those of patch 3 already, and goes from Draft to Changes Planned.");
        assert_eq!(fake.writes(), ["differential.revision.edit plan-changes"]);
        assert_eq!(sent.unwrap(), "D13 is Changes Planned now. Patch 2 has no revision, so D13 gets no parent.");
    }

    #[test]
    fn a_revision_in_review_gets_the_title_and_summary_alone_and_keeps_its_status() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\nBecause.\n\n{LINK}/D13")]);
        let mut fake = Fake::default().with(13, "needs-review", &diff_of(&db, 2), "Bug 3 - an older title");
        fake.revisions.get_mut(&13).unwrap().summary = "Why.".into();
        let (ready, sent) = submit(&db, 2, &mut fake);
        // Not refused: the title as moz-phab writes it, but for `WIP: `, as
        // the revision's title has none; no new diff, and no code shown.
        let p = ready.unwrap();
        assert!(!p.in_work() && !p.new_diff && p.code.is_none(), "{p:?}");
        assert_eq!((p.title.as_str(), p.summary.as_str()), ("Bug 3 - three", "Because."));
        assert_eq!(p.kept_said(), None, "its latest diff is the patch as it is");
        assert_eq!(
            p.said(),
            ["D13, at https://phabricator.example.com/D13, gets the title and summary of patch 3, and stays Needs Review.", "Patch 2 has no revision, so D13 gets no parent."]
        );
        // The title and summary alone: no diff made, so no repository read,
        // and no Changes Planned.
        assert_eq!(fake.writes(), ["differential.revision.edit title,summary"]);
        assert!(!fake.calls.iter().any(|(m, _)| m == "diffusion.repository.search"), "{:?}", fake.calls);
        assert!(fake.of_reviewers().is_empty(), "{:?}", fake.of_reviewers());
        assert_eq!(sent.unwrap(), "D13 has the title and summary of patch 3 now. Patch 2 has no revision, so D13 gets no parent.");
        let r = &fake.revisions[&13];
        assert_eq!((r.status.as_str(), r.title.as_str(), r.summary.as_str(), r.diffs.len()), ("needs-review", "Bug 3 - three", "Because.", 1));
    }

    #[test]
    fn an_accepted_revision_and_one_needing_revision_are_sent_the_same_wip_kept_where_their_title_has_it() {
        for (status, name) in [("accepted", "Accepted"), ("needs-revision", "Needs Revision")] {
            let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three r=alice\n\n{LINK}/D13")]);
            let mut fake = Fake::default().with(13, status, &diff_of(&db, 2), "WIP: Bug 3 - an older title");
            let (ready, sent) = submit(&db, 2, &mut fake);
            let p = ready.unwrap();
            // The reviewers the title names stay in it, as moz-phab writes
            // them; none is read or set.
            assert_eq!(p.title, "WIP: Bug 3 - three r=alice", "{status}");
            assert_eq!(p.said()[0], format!("D13, at https://phabricator.example.com/D13, gets the title and summary of patch 3, and stays {name}."));
            assert_eq!(fake.writes(), ["differential.revision.edit title"], "{status}");
            assert!(fake.of_reviewers().is_empty(), "{:?}", fake.of_reviewers());
            assert_eq!(sent.unwrap(), "D13 has the title and summary of patch 3 now. Patch 2 has no revision, so D13 gets no parent.");
            let r = &fake.revisions[&13];
            assert_eq!((r.status.as_str(), r.title.as_str()), (status, "WIP: Bug 3 - three r=alice"));
        }
    }

    #[test]
    fn a_revision_in_review_whose_parent_alone_differs_gets_the_parent_edit_alone() {
        let (_tmp, db, _r) = stack([&format!("Bug 1 - one\n\n{LINK}/D11"), &format!("Bug 2 - two\n\n{LINK}/D12"), "Bug 3 - three"]);
        let mut fake = Fake::default().with(11, "needs-review", "", "Bug 1 - one").with(12, "needs-review", &diff_of(&db, 1), "Bug 2 - two");
        fake.revisions.get_mut(&12).unwrap().parents = vec!["PHID-DREV-99".into()];
        let (ready, sent) = submit(&db, 1, &mut fake);
        let p = ready.unwrap();
        assert!(!p.message_changes() && p.parent_changes() && !p.nothing(), "{p:?}");
        assert_eq!(
            p.said(),
            [
                "D12, at https://phabricator.example.com/D12, keeps its title and summary, as they are those of patch 2 already, and stays Needs Review.",
                "Its parent is set to D11, the revision of patch 1.",
            ]
        );
        assert_eq!(fake.writes(), ["differential.revision.edit parents.set"]);
        assert_eq!(sent.unwrap(), "D12 keeps its diff, title and summary. The parent of D12 is D11 now.");
        let r = &fake.revisions[&12];
        assert_eq!((r.status.as_str(), r.parents.clone()), ("needs-review", vec!["PHID-DREV-11".to_string()]));
    }

    #[test]
    fn a_revision_in_review_whose_diff_alone_differs_is_sent_nothing() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        // Its latest diff is the version of the patch before the one in the
        // stack.
        let earlier = diff_of(&db, 2);
        new_code(&db, 2, &earlier.replace("+LINE 20", "+Line twenty"));
        let mut fake = Fake::default().with(13, "needs-review", &earlier, "Bug 3 - three");
        let p = prepared(&db, 2, &mut fake).unwrap();
        assert_eq!(p.known.as_ref().map(|k| (k.version, k.current)), Some((1, false)));
        assert!(p.nothing() && !p.new_diff && p.code.is_none(), "{p:?}");
        assert_eq!(
            p.kept_said().as_deref(),
            Some("The code of patch 3 is not sent: D13 is Needs Review, not in work, and only a revision in work gets a new diff.")
        );
        assert_eq!(
            p.nothing_said(),
            "D13 is Needs Review, not in work, so the code of patch 3 is not sent. The title and summary of D13 are those of patch 3 already, so nothing was sent. Patch 2 has no revision, so D13 gets no parent."
        );
        // Its latest diff from elsewhere, no version of the patch: left as it
        // is, as no code is sent, and nothing is sent either.
        let mut fake = Fake::default().with(13, "needs-review", &earlier.replace("+LINE 20", "+Someone else's 20"), "Bug 3 - three");
        let p = prepared(&db, 2, &mut fake).unwrap();
        assert!(p.known.is_none() && p.nothing() && p.kept_said().is_some(), "{p:?}");
        assert!(fake.writes().is_empty());
        // Known by its changes alone, made before a rebase: no new diff
        // either, and none said to be sent.
        let (_tmp, db, r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let wide = git_ok(&r, &["diff", "-U10", "HEAD~1", "HEAD"]);
        rebased_upstream(&db, &r);
        let mut fake = Fake::default().with(13, "needs-review", &wide, "Bug 3 - three");
        let p = prepared(&db, 2, &mut fake).unwrap();
        assert_eq!(p.known.as_ref().map(|k| (k.current, k.by_changes)), Some((false, true)));
        assert!(p.nothing() && p.changes_said().is_none() && p.kept_said().is_some(), "{p:?}");
    }

    #[test]
    fn a_revision_in_review_changed_since_it_was_read_is_sent_nothing() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\nNew.\n\n{LINK}/D13")]);
        // Retitled meanwhile, or accepted.
        for change in ["title", "status"] {
            let mut fake = Fake::default().with(13, "needs-review", &diff_of(&db, 2), "Bug 3 - three");
            let p = prepared(&db, 2, &mut fake).unwrap();
            assert!(!p.in_work() && !p.nothing(), "{p:?}");
            let r = fake.revisions.get_mut(&13).unwrap();
            match change {
                "title" => r.title = "Someone's title".into(),
                _ => r.status = "accepted".into(),
            }
            let sent = send_with(&db, &p, None, &mut |m, v| fake.call(m, v));
            assert_eq!(sent, "D13 changed on Phabricator since you reviewed it, so nothing was sent. Submit again to review it as it is now.", "{change}");
            assert!(fake.writes().is_empty(), "{change}");
        }
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
    fn the_parent_s_rule_reads_the_revision_below_only_where_it_decides_and_gives_back_what_could_not_be_said() {
        let unread = |_: &str, _: &str| -> Result<Below, ()> { panic!("the revision below is not read") };
        // The bottom patch, and one on a patch with no revision: none, the
        // revision below never read.
        let (_tmp, db, _r) = stack([&format!("Bug 1 - one\n\n{LINK}/D11"), "Bug 2 - two", &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let d = Db::load(&db).unwrap();
        let s = d.stack("s").unwrap();
        let on_d9 = ["PHID-DREV-9".to_string()];
        let none: &[String] = &[];
        let bottom = "Patch 1 is at the bottom of the stack, so the parents of D11 are left as they are.";
        assert_eq!(parent_with(&d, s, 0, Some(("D11", &on_d9[..])), unread), Ok(Parent::Nothing(bottom.into())));
        assert_eq!(parent_with(&d, s, 2, Some(("D13", none)), unread), Ok(Parent::Nothing("Patch 2 has no revision, so D13 gets no parent.".into())));
        // On a patch naming a revision: that read by its D123 and its link;
        // not known, given back.
        let (_tmp, db, _r) = stack([&format!("Bug 1 - one\n\n{LINK}/D11"), &format!("Bug 2 - two\n\n{LINK}/D12"), &format!("Bug 3 - three\n\n{LINK}/D13")]);
        let d = Db::load(&db).unwrap();
        let s = d.stack("s").unwrap();
        let mut asked = Vec::new();
        let read = parent_with(&d, s, 2, Some(("D13", none)), |id, link| {
            asked.push((id.to_string(), link.to_string()));
            Err::<Below, ()>(())
        });
        assert_eq!((read, asked), (Err(()), vec![("D12".to_string(), "https://phabricator.example.com/D12".to_string())]));
        // Read: open, the parent already, or set in place of the parents it
        // has; closed, missing or not read, none, said as a submit says it.
        let below = |b: Below| move |_: &str, _: &str| Ok::<Below, ()>(b);
        let open = Below::Found { phid: "PHID-DREV-12".into(), open: true, status: "Needs Review".into() };
        let on_d12 = ["PHID-DREV-12".to_string()];
        assert_eq!(parent_with(&d, s, 2, Some(("D13", &on_d12[..])), below(open.clone())), Ok(Parent::Already { id: "D12".into(), below: 2 }));
        let set = Parent::Set { id: "D12".into(), phid: "PHID-DREV-12".into(), below: 2 };
        for now in [Some(("D13", &on_d9[..])), Some(("D13", none)), None] {
            assert_eq!(parent_with(&d, s, 2, now, below(open.clone())), Ok(set.clone()), "{now:?}");
        }
        let closed = Below::Found { phid: "PHID-DREV-12".into(), open: false, status: "Landed".into() };
        assert_eq!(parent_with(&d, s, 2, Some(("D13", none)), below(closed)), Ok(Parent::Nothing("D12, the revision of patch 2, is Landed, so D13 gets no parent.".into())));
        let missing = "Phabricator has no D12, the revision of patch 2, so the parents of D13 are left as they are.";
        assert_eq!(parent_with(&d, s, 2, Some(("D13", &on_d9[..])), below(Below::Missing)), Ok(Parent::Nothing(missing.into())));
        let refused = "The revision of patch 2, D12, could not be read, so the new revision gets no parent: refused.";
        assert_eq!(parent_with(&d, s, 2, None, below(Below::Unread("refused".into()))), Ok(Parent::Nothing(refused.into())));
        // Naming the same revision: none, nothing read.
        assert_eq!(parent_with(&d, s, 2, Some(("D12", none)), unread), Ok(Parent::Nothing("Patch 2 names D12 too, so D12 gets no parent.".into())));
    }

    #[test]
    fn a_submit_in_work_asks_nothing_of_reviewers_whatever_the_title_names() {
        // The title names some, asked and given, a group and one blocking;
        // the revision has the title as moz-phab writes it, and one reviewer.
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three r=alice,bob! r?#build

{LINK}/D13")]);
        let title = "WIP: Bug 3 - three r?#build r=alice,bob!";
        let mut fake = Fake::default().with(13, "changes-planned", &diff_of(&db, 2), title);
        let (ready, _) = submit(&db, 2, &mut fake);
        let p = ready.unwrap();
        assert!(p.nothing(), "{p:?}");
        assert_eq!(p.said().len(), 2, "no word of reviewers: {:?}", p.said());
        assert!(fake.writes().is_empty());
        // A new diff: sent, and still nothing of reviewers, neither looked up
        // nor set nor read.
        Db::edit(&db, |d| {
            let old = d.patch(&d.stack("s")?.patches[2]).clone();
            let draft = crate::mbox::Draft { diff: old.diff.replace("+LINE 20", "+Line twenty"), ..crate::mbox::Draft::from(&old) };
            crate::ops::replace(d, "s", 2, draft, "edit").map(|_| ())
        })
        .unwrap();
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(sent.unwrap(), "D13 has the new diff of patch 3 now. D13 stays Changes Planned. Patch 2 has no revision, so D13 gets no parent.");
        assert!(fake.of_reviewers().is_empty(), "{:?}", fake.of_reviewers());
        let search = fake.calls.iter().find(|(m, _)| m == "differential.revision.search").unwrap().1.clone();
        assert_eq!(search, json!({"constraints": {"ids": [13]}}), "its reviewers not asked for");
    }

    #[test]
    fn a_patch_whose_commit_names_a_revision_it_does_not_is_held_back_alone() {
        use crate::head::{At, Head, Mark, OnStack};
        let head = |d: Mark, k: usize| Head {
            dir: PathBuf::from("/checkout"),
            branch: Some("main".into()),
            commit: "c".repeat(40),
            at: At::Stack(OnStack {
                commits: vec!["a".into(), "b".into(), "c".into()],
                marks: vec![(k, "its commit's message".into(), d)],
                above: vec![],
                subjects: vec![],
            }),
        };
        let names = |ours: Option<&str>, theirs: &str| Mark::Revision { theirs: theirs.into(), ours: ours.map(String::from) };
        assert_eq!(
            held_by_head(&head(names(None, "D34"), 2), 2).as_deref(),
            Some("Patch 3 names no revision in the stack and D34 in your workspace's commit: give the patch D34 first, with Reconcile with HEAD on its menu, so that no second revision is made.")
        );
        assert_eq!(
            held_by_head(&head(names(Some("D13"), "D34"), 2), 2).as_deref(),
            Some("Patch 3 names D13 in the stack and D34 in your workspace's commit: make the patch and its commit name the same revision first, the patch with Reconcile with HEAD on its menu or the commit in your checkout, so that its diff goes to the revision you keep.")
        );
        // Its commit's message other than the patch's, the patch alone
        // naming a revision or not; another patch of the stack; HEAD
        // elsewhere: not held back.
        assert_eq!(held_by_head(&head(Mark::Message, 2), 2), None);
        assert_eq!(held_by_head(&head(names(None, "D34"), 2), 0), None);
        let elsewhere = Head { at: At::Elsewhere, ..head(names(None, "D34"), 2) };
        assert_eq!(held_by_head(&elsewhere, 2), None);
    }

    #[test]
    fn a_refused_parent_keeps_the_new_diff_and_says_so() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", &format!("Bug 2 - two

{LINK}/D12"), &format!("Bug 3 - three

{LINK}/D13")]);
        let earlier = diff_of(&db, 2);
        Db::edit(&db, |d| {
            let old = d.patch(&d.stack("s")?.patches[2]).clone();
            let draft = crate::mbox::Draft { diff: earlier.replace("+LINE 20", "+Line twenty"), ..crate::mbox::Draft::from(&old) };
            crate::ops::replace(d, "s", 2, draft, "edit").map(|_| ())
        })
        .unwrap();
        let mut fake = Fake { refuse: vec!["edit:parents.set"], ..Default::default() }
            .with(12, "needs-review", "", "Bug 2 - two")
            .with(13, "changes-planned", &earlier, "WIP: Bug 3 - three");
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(
            sent.unwrap(),
            "D13 has the new diff of patch 3 now. D13 stays Changes Planned. But the parent of D13 was not set to D12: differential.revision.edit: refused."
        );
        assert_eq!(
            fake.writes(),
            [
                "differential.creatediff",
                "differential.setdiffproperty",
                "differential.revision.edit update",
                "differential.revision.edit plan-changes",
                "differential.revision.edit parents.set",
            ]
        );
        let r = &fake.revisions[&13];
        assert_eq!((r.diffs.len(), r.status.as_str(), r.parents.is_empty()), (2, "changes-planned", true));
    }

    /// What the last setdiffproperty said of its commit, by the commit, and
    /// the last creatediff.
    fn last_commit(fake: &Fake) -> (String, Value, Value) {
        let property = fake.calls.iter().rev().find(|(m, _)| m == "differential.setdiffproperty").unwrap().1.clone();
        let data: Value = serde_json::from_str(property["data"].as_str().unwrap()).unwrap();
        let (key, commit) = data.as_object().unwrap().iter().next().map(|(k, v)| (k.clone(), v.clone())).unwrap();
        let diff = fake.calls.iter().rev().find(|(m, _)| m == "differential.creatediff").unwrap().1.clone();
        (key, commit, diff)
    }

    #[test]
    fn the_commit_sent_has_its_tree_and_is_the_workspace_s_when_head_has_it() {
        // HEAD looked at nowhere: the patch's own id, on the base, its tree
        // the one the patches leave, the commit's in the checkout.
        let (_tmp, db, r) = stack(["Bug 1 - one", "Bug 2 - two", "Bug 3 - three"]);
        let sha = |rev: &str| git_ok(&r, &["rev-parse", rev]).trim().to_string();
        let id = Db::load(&db).unwrap().stack("s").unwrap().patches[2].clone();
        let mut fake = Fake::default();
        let (_, sent) = submit(&db, 2, &mut fake);
        assert!(sent.unwrap().starts_with("Patch 3 is D200 on Phabricator now"));
        let (key, commit, diff) = last_commit(&fake);
        assert_eq!((key.as_str(), commit["commit"].as_str(), commit["tree"].as_str()), (id.as_str(), Some(id.as_str()), Some(sha("HEAD^{tree}").as_str())));
        assert_eq!(commit["parents"], json!([sha("HEAD~3")]));
        assert_eq!((diff["sourceControlBaseRevision"].as_str(), diff["changes"][0]["commitHash"].as_str()), (Some(sha("HEAD~3").as_str()), Some(id.as_str())));
        assert!(commit.get("rev").is_none(), "git's: no rev");
        // HEAD on the stack, the patches its commits: the commit is the
        // workspace's, on the commit below it; the bottom patch's on the base.
        let (_tmp, db, r) = stack(["Bug 1 - one", "Bug 2 - two", "Bug 3 - three"]);
        let sha = |rev: &str| git_ok(&r, &["rev-parse", rev]).trim().to_string();
        let mut fake = Fake::default();
        let (_, sent) = submit_in(&db, 2, Some(&r), &mut fake);
        assert!(sent.unwrap().starts_with("Patch 3 is D200 on Phabricator now"));
        let (key, commit, diff) = last_commit(&fake);
        let said = |v: &Value| v.as_str().unwrap_or_default().to_string();
        assert_eq!((key, said(&commit["commit"]), said(&commit["tree"])), (sha("HEAD"), sha("HEAD"), sha("HEAD^{tree}")));
        assert_eq!(commit["parents"], json!([sha("HEAD~1")]));
        assert_eq!((said(&diff["sourceControlBaseRevision"]), said(&diff["changes"][0]["commitHash"])), (sha("HEAD~1"), sha("HEAD")));
        let (_, sent) = submit_in(&db, 0, Some(&r), &mut fake);
        assert!(sent.unwrap().starts_with("Patch 1 is D201 on Phabricator now"));
        let (_, commit, _) = last_commit(&fake);
        assert_eq!((said(&commit["commit"]), said(&commit["tree"])), (sha("HEAD~2"), sha("HEAD~2^{tree}")));
        assert_eq!(commit["parents"], json!([sha("HEAD~3")]));
    }

    #[test]
    fn phabricator_makes_no_revision_of_a_diff_whose_commit_has_no_tree() {
        // As the user's Phabricator refused one: the stand-in does too.
        let mut fake = Fake::default();
        let data = json!({"c": {"commit": "c", "parents": ["b"]}}).to_string();
        fake.call("differential.setdiffproperty", json!({"diff_id": 7, "name": "local:commits", "data": data})).unwrap();
        let refused = fake.call("differential.revision.edit", json!({"transactions": [{"type": "update", "value": "PHID-DIFF-7"}]}));
        assert_eq!(refused, Err("differential.revision.edit: Undefined array key \"tree\"".to_string()));
        assert!(fake.revisions.is_empty());
    }

    #[test]
    fn a_new_revision_s_trailer_goes_where_moz_phab_puts_one() {
        let line = "Differential Revision: https://p.example/D9";
        assert_eq!(with_link("Bug 1 - x", line), format!("Bug 1 - x\n\n{line}"));
        assert_eq!(with_link("Bug 1 - x\n\nWhy.\n", line), format!("Bug 1 - x\n\nWhy.\n\n{line}"));
        let signed = "Bug 1 - x\n\nWhy.\n\nReviewed-by: ann\nAcked-by: bo";
        assert_eq!(with_link(signed, line), format!("{signed}\n{line}"));
        // A last paragraph that is not trailers alone.
        let prose = "Bug 1 - x\n\nWhy: because.\nAnd more.";
        assert_eq!(with_link(prose, line), format!("{prose}\n\n{line}"));
    }

    #[test]
    fn a_submit_after_one_that_failed_partway_makes_a_new_diff_and_the_revision() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", "Bug 3 - three\n\nWhy.\n\nReviewed-by: ann"]);
        // The revision refused, the diff made: no revision, nor link.
        let mut fake = Fake { refuse: vec!["edit:title"], ..Default::default() };
        let (_, sent) = submit(&db, 2, &mut fake);
        assert_eq!(sent.unwrap(), "The diff of patch 3 was made on Phabricator, but no revision was made of it: differential.revision.edit: refused.");
        assert!(crate::phab::revision_of(&message_of(&db, 2)).is_none());
        // Submitted again: a diff of its own, and the revision of it; the
        // trailer after the paragraph of trailers the message ends with.
        fake.refuse.clear();
        let (_, sent) = submit(&db, 2, &mut fake);
        assert!(sent.unwrap().starts_with("Patch 3 is D200 on Phabricator now, in Changes Planned."));
        let made: Vec<u64> = fake.diffs.keys().copied().filter(|d| *d >= 2000).collect();
        assert_eq!((made.len(), fake.revisions[&200].diffs.clone()), (2, vec![made[1]]), "a new diff");
        assert_eq!(message_of(&db, 2), format!("Bug 3 - three\n\nWhy.\n\nReviewed-by: ann\n{LINK}/D200"));
    }

    #[test]
    fn a_revision_changed_since_it_was_read_is_sent_nothing() {
        let (_tmp, db, _r) = stack(["Bug 1 - one", "Bug 2 - two", &format!("Bug 3 - three\n\nNew.\n\n{LINK}/D13")]);
        let mut fake = Fake::default().with(13, "changes-planned", &diff_of(&db, 2), "WIP: Bug 3 - three");
        let d = Db::load(&db).unwrap();
        let s = d.stack("s").unwrap().clone();
        let arc = arcconfig(Path::new(s.repo.as_deref().unwrap()));
        let p = prepare_with(&d, &s, 2, "https://phabricator.example.com", &arc, &mut |m, v| fake.call(m, v), &mut None).unwrap();
        fake.revisions.get_mut(&13).unwrap().title = "Someone's title".into();
        let sent = send_with(&db, &p, None, &mut |m, v| fake.call(m, v));
        assert_eq!(sent, "D13 changed on Phabricator since you reviewed it, so nothing was sent. Submit again to review it as it is now.");
        assert!(fake.writes().is_empty());
    }
}
