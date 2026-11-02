//! A comment is a thread of turns. The agent's replies are fenced ```agent
//! blocks naming what it was asked to do (```claude, as self-review and
//! stacked's first files have them, is read the same), a reviewer's
//! comments pulled from Phabricator are fenced ```reviewer blocks naming
//! who wrote them, and every other line is yours. A turn that holds fences
//! of its own (a code block) is fenced with more backticks than they have,
//! ````agent, and only a line of as many backticks ends it. A conversation is
//! rewound from one of your turns: your new text takes the place of that
//! turn and of every turn after it. A turn of yours holding a line that
//! would be read as one of those fences (```agent) is fenced too, as
//! yours, ```you, with more backticks than its own: the rest are as
//! written.
//!
//! A reviewer's comment edited on Phabricator is a turn of its own, after
//! the rest: their edit of that comment, its fence naming the comment's
//! first line as it was filed, after the reviewer's name and `edit`
//! (```reviewer ann edit Hi Da), and its heading quoting it (`ann's edit of
//! the comment that said "Hi Da"`). The comment's own turn stays as it was,
//! for what answered it to read as it was written, redacted: `redacted`
//! after the reviewer's name on its fence (```reviewer ann redacted), and
//! on its heading (`ann's comment (redacted)`).
//!
//! A turn of the agent's whose run was asked at an effort (claude's
//! `--effort`) says so on its fence, after the way it was asked: `effort`
//! and the effort's name (```agent answer effort max). It is headed
//! `agent(max)`, and named `the agent's reply (max)`. A turn written before
//! efforts were picked, or by an ask that went at the agent's own, has
//! none, and reads as it always did. What the agent is told of a thread
//! says none of it.
//!
//! A turn of an answer-only ask's (a reply, or a comment it opened) names
//! the stack it was asked on, last on its fence: `stack` and that stack's
//! rev (```agent answer effort max stack 12). While the stack has changed
//! since, its heading says how many edits old that stack is, after the
//! agent: `agent(max), on the stack 2 edits ago`, or `agent(max), on
//! another version of the stack` once the stack is on a state that was not
//! edited from it (`asked_on_said`). The turns of other asks, and those of
//! before, name none. What the agent is told of a thread says none of it
//! either.
//!
//! A turn deleted by stacked's first way of deleting one is a fenced
//! ```deleted block, what it said quoted in it: it is read only to be left
//! out, shown nowhere and told to no one, and nothing writes one any more.
//! Once no database holds such turns, `DELETED_OPEN`, `Turn::deleted` and
//! the split between `all_turns` and `thread_turns` can go.

pub const AGENT_OPEN: &str = "```agent";
/// The agent's fence as it was first written.
const AGENT_WAS: &str = "```claude";
pub const REVIEWER_OPEN: &str = "```reviewer";
pub const YOU_OPEN: &str = "```you";
pub const DELETED_OPEN: &str = "```deleted";
pub const FENCE_CLOSE: &str = "```";
/// The word after a reviewer's name on the fence of their edit of a
/// comment of theirs above it, the first line of that comment after it.
pub const EDIT: &str = "edit";
/// The word after a reviewer's name on the fence of a turn of theirs that
/// an edit of it below replaces, before `edit` if it is one too.
pub const REDACTED: &str = "redacted";
/// The word before the name of the effort the agent's run was asked at, on
/// the fence of a turn of the agent's, after the way it was asked
/// (```agent answer effort max).
pub const EFFORT: &str = "effort";
/// The word before the rev of the stack an answer-only ask was asked on,
/// last on the fence of a turn of the agent's that it brought back, after
/// the effort if there is one (```agent answer effort max stack 12).
pub const STACK: &str = "stack";

/// How much of a comment's first line the heading of an edit of it quotes,
/// before `…`.
const QUOTED: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    You,
    Agent,
    Reviewer,
}

/// A turn: who, what, and the fence's tag -- the mode for claude's ("" when
/// it names none), the name for a reviewer's; and whether it was deleted,
/// the first way: then shown nowhere, and told to no one.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub body: String,
    pub tag: Option<String>,
    pub deleted: bool,
    /// A reviewer's edit, on Phabricator, of a comment of theirs above it:
    /// that comment's first line, as it was filed.
    pub edit: Option<String>,
    /// A reviewer's turn that an edit of it below replaces: shown, and told,
    /// as it was, said redacted.
    pub redacted: bool,
    /// The agent's, its run asked at an effort: that effort's name, as
    /// claude's `--effort` takes it (`max`). None for a turn of before
    /// efforts were picked, or of an ask that went at the agent's own.
    pub effort: Option<String>,
    /// The agent's, brought back by an answer-only ask: the stack that ask
    /// was asked on, by its rev, whose age its heading says while the stack
    /// has changed since. None for another ask's, and for one of before.
    pub stack: Option<usize>,
}

impl Turn {
    /// Turn `i` of its thread, said: `your comment`, `the agent's reply`,
    /// `ann's reply`, `ann's edit of the comment that said "Hi Da"`; one an
    /// edit replaces, `ann's comment (redacted)`; the agent's, its run
    /// asked at an effort, `the agent's reply (max)`.
    pub fn name(&self, i: usize) -> String {
        match self.redacted {
            true => format!("{} ({REDACTED})", self.named(i)),
            false => self.named(i),
        }
    }

    /// As `name` says turn `i`, one an edit replaces not said so.
    pub fn named(&self, i: usize) -> String {
        let whose = match self.role {
            Role::You => "your".to_string(),
            Role::Agent => "the agent's".to_string(),
            Role::Reviewer => match self.tag.as_deref() {
                Some(who) if !who.is_empty() => format!("{who}'s"),
                _ => "the reviewer's".to_string(),
            },
        };
        // A reviewer's edit of a comment of theirs above: that comment,
        // quoted.
        if let Some(was) = &self.edit {
            return format!("{whose} edit of the comment that said \"{}\"", quoted(was));
        }
        named_at(&format!("{whose} {}", if i == 0 { "comment" } else { "reply" }), self.agent_effort())
    }

    /// The effort the agent's run was asked at, on a turn of the agent's
    /// that says one.
    pub fn agent_effort(&self) -> Option<&str> {
        self.effort.as_deref().filter(|_| self.role == Role::Agent)
    }
}

/// The agent, as a turn of its is headed and as it is listed: `agent`; its
/// run asked at an effort, `agent(max)`.
pub fn agent_at(effort: Option<&str>) -> String {
    match effort {
        Some(e) => format!("agent({e})"),
        None => "agent".into(),
    }
}

/// `said`, what names a turn of the agent's or its work, with the effort
/// its run was asked at after it: `the agent's reply (max)`. As it is
/// without one.
pub fn named_at(said: &str, effort: Option<&str>) -> String {
    match effort {
        Some(e) => format!("{said} ({e})"),
        None => said.to_string(),
    }
}

/// What follows the word that says an ask was asked, its run asked at an
/// effort: ` at max`, as in `asked at max: "why?"`, `opened by the agent
/// at max`. Nothing without one.
pub fn asked_at(effort: Option<&str>) -> String {
    effort.map_or(String::new(), |e| format!(" at {e}"))
}

/// How old the stack an answer-only ask was asked on is, `since`, as the
/// heading of a turn it brought back says it after the agent: `on the
/// stack 1 edit ago`, `on the stack 2 edits ago`, `on another version of
/// the stack`. Nothing while the stack is on that state still.
pub fn asked_on_said(since: crate::db::Since) -> Option<String> {
    match since {
        crate::db::Since::Edits(0) => None,
        crate::db::Since::Edits(1) => Some("on the stack 1 edit ago".into()),
        crate::db::Since::Edits(n) => Some(format!("on the stack {n} edits ago")),
        crate::db::Since::Elsewhere => Some("on another version of the stack".into()),
    }
}

/// The first line of `text`, as the heading of an edit quotes it: cut with
/// `…` when long.
fn quoted(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    match first.chars().count() > QUOTED {
        true => format!("{}…", first.chars().take(QUOTED - 1).collect::<String>().trim_end()),
        false => first.to_string(),
    }
}

/// How many backticks start `line`, past its indentation.
fn ticks(line: &str) -> usize {
    line.trim_start().chars().take_while(|&c| c == '`').count()
}

/// Who an opening fence line starts a turn for, its tag, whether the turn
/// was deleted, and how many backticks the fence has: three, or more when
/// the turn holds fences of its own.
pub fn fence(line: &str) -> Option<(Role, String, bool, usize)> {
    let s = line.trim();
    let n = ticks(s);
    if n < 3 {
        return None;
    }
    // What follows the backticks, as the fences of three name it.
    let s = &s[n..];
    if let Some(rest) = s.strip_prefix(word(DELETED_OPEN)).and_then(|r| r.strip_prefix(' ')) {
        let (who, tag) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
        let role = match who {
            "you" => Role::You,
            "agent" => Role::Agent,
            "reviewer" => Role::Reviewer,
            _ => return None,
        };
        return Some((role, tag.trim().to_string(), true, n));
    }
    for (open, role) in [(AGENT_OPEN, Role::Agent), (AGENT_WAS, Role::Agent), (REVIEWER_OPEN, Role::Reviewer), (YOU_OPEN, Role::You)] {
        let open = word(open);
        if s == open {
            return Some((role, String::new(), false, n));
        }
        if let Some(rest) = s.strip_prefix(open).and_then(|r| r.strip_prefix(' ')) {
            return Some((role, rest.trim().to_string(), false, n));
        }
    }
    None
}

/// What a fence of three backticks says after them: `agent`, `reviewer`.
fn word(open: &str) -> &str {
    &open[FENCE_CLOSE.len()..]
}

/// Whether `st`, a line trimmed, ends a turn fenced with `n` backticks: of
/// three, as every turn was fenced at first, a fence alone, unless one of
/// the turn's own is open (`inner`); of more, a line of as many backticks
/// at least, and nothing else.
fn ends(st: &str, n: usize, inner: bool) -> bool {
    match n {
        3 => st == FENCE_CLOSE && !inner,
        n => st.len() >= n && st.chars().all(|c| c == '`'),
    }
}

/// A turn fenced: `open`'s words (`agent answer`, `reviewer ann`) after the
/// fence on its first line, then `body`, between fences of three backticks,
/// or of one more than the most that start a line of `body`, so that none
/// of its own fences ends it.
pub fn fenced(open: &str, body: &str) -> String {
    let most = body.lines().map(ticks).max().unwrap_or(0);
    let fence = "`".repeat(if most >= 3 { most + 1 } else { 3 });
    format!("{fence}{open}\n{body}\n{fence}")
}

/// A fenced turn, closed: what its fence held. A deleted turn's lines are
/// quoted, so that none of them ends it.
fn closed(role: Role, tag: &str, deleted: bool, body: &[&str]) -> Turn {
    let lines: Vec<&str> = if deleted {
        body.iter().map(|l| l.strip_prefix("> ").or_else(|| l.strip_prefix('>')).unwrap_or(l)).collect()
    } else {
        body.to_vec()
    };
    // The agent's: the way it was asked, then the effort its run was asked
    // at, if it says one, then the stack an answer-only ask was asked on.
    let (tag, effort, stack) = match role {
        Role::Agent if !deleted => way_and_effort(tag),
        _ => (tag.to_string(), None, None),
    };
    let tag = tag.as_str();
    // A reviewer's: their name; `redacted`, if an edit of it replaces it;
    // `edit`, if it is one, then the first line of the comment it edits.
    let mut marks = (tag, false, None);
    if let Some((who, rest)) = tag.split_once(' ').filter(|_| role == Role::Reviewer && !deleted) {
        let (redacted, rest) = match after_word(rest, REDACTED) {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let edit = after_word(rest, EDIT).map(str::to_string);
        if redacted || edit.is_some() {
            marks = (who, redacted, edit);
        }
    }
    let (tag, redacted, edit) = marks;
    Turn {
        role,
        body: lines.join("\n").trim().to_string(),
        tag: (role != Role::You).then(|| tag.to_string()),
        deleted,
        edit,
        redacted,
        effort,
        stack,
    }
}

/// What an agent's fence says after `agent`, `tag`, read: the way it was
/// asked; the effort its run was asked at, after `effort`, if it says one;
/// and last, after `stack`, the rev of the stack an answer-only ask was
/// asked on, if it says one (`answer effort max stack 12`).
fn way_and_effort(tag: &str) -> (String, Option<String>, Option<usize>) {
    let words: Vec<&str> = tag.split_whitespace().collect();
    let (words, stack) = match words.as_slice() {
        [rest @ .., mark, rev] if *mark == STACK && rev.parse::<usize>().is_ok() => (rest, rev.parse::<usize>().ok()),
        all => (all, None),
    };
    match words {
        [way @ .., mark, effort] if *mark == EFFORT => (way.join(" "), Some(effort.to_string()), stack),
        _ if stack.is_some() => (words.join(" "), None, stack),
        _ => (tag.to_string(), None, None),
    }
}

/// What the fence of a turn of the agent's says after the backticks: `agent`,
/// the way it was asked (`tag`), then the effort its run was asked at, if it
/// had one, then the stack an answer-only ask was asked on, by its rev, if
/// it was one: `agent answer effort max stack 12`.
pub fn agent_open(tag: &str, effort: Option<&str>, stack: Option<usize>) -> String {
    let mut open = word(AGENT_OPEN).to_string();
    let rev = stack.map(|r| r.to_string());
    for w in [Some(tag).filter(|t| !t.is_empty()), effort.map(|_| EFFORT), effort, rev.as_ref().map(|_| STACK), rev.as_deref()]
        .into_iter()
        .flatten()
    {
        open.push(' ');
        open.push_str(w);
    }
    open
}

/// What follows word `w` at the start of `s`, if it starts with it as a
/// word of its own.
fn after_word<'a>(s: &'a str, w: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(w)?;
    (rest.is_empty() || rest.starts_with(' ')).then(|| rest.trim_start())
}

/// Splits a stored comment into ordered turns, those deleted too, empty
/// ones dropped. A fence inside a reply (a code block of its own) does not
/// end it.
pub fn all_turns(text: &str) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut mine: Vec<&str> = Vec::new();
    let mut open: Option<(Role, String, bool, usize, Vec<&str>)> = None;
    let mut inner = false;

    let flush = |turns: &mut Vec<Turn>, mine: &mut Vec<&str>| {
        let body = mine.join("\n").trim().to_string();
        if !body.is_empty() {
            turns.push(Turn {
                role: Role::You,
                body,
                tag: None,
                deleted: false,
                edit: None,
                redacted: false,
                effort: None,
                stack: None,
            });
        }
        mine.clear();
    };

    for line in text.lines() {
        let st = line.trim();
        match &mut open {
            None => {
                if let Some((role, tag, deleted, n)) = fence(line) {
                    flush(&mut turns, &mut mine);
                    open = Some((role, tag, deleted, n, Vec::new()));
                    inner = false;
                } else {
                    mine.push(line);
                }
            }
            Some((role, tag, deleted, n, body)) => {
                if ends(st, *n, inner) {
                    turns.push(closed(*role, tag, *deleted, body));
                    open = None;
                } else {
                    if *n == 3 && st.starts_with(FENCE_CLOSE) {
                        inner = !inner;
                    }
                    body.push(line);
                }
            }
        }
    }
    match open {
        Some((role, tag, deleted, _, body)) => turns.push(closed(role, &tag, deleted, &body)),
        None => flush(&mut turns, &mut mine),
    }
    turns.retain(|t| !t.body.is_empty());
    turns
}

/// Who said turn `t`, as the editor heads it and a thread read as lines
/// says it: `you`, `agent`, a reviewer's name (`ann`); the agent's, its run
/// asked at an effort, `agent(max)`; a reviewer's edit by what it edits
/// (`ann's edit of the comment that said "Hi Da"`); either `(redacted)`
/// once an edit replaces it.
pub fn said_by(t: &Turn) -> String {
    match t.role {
        Role::You => "you".into(),
        Role::Agent => agent_at(t.agent_effort()),
        Role::Reviewer if t.edit.is_some() => t.name(1),
        Role::Reviewer => {
            let who = t.tag.clone().filter(|w| !w.is_empty()).unwrap_or_else(|| "reviewer".into());
            if t.redacted { format!("{who} ({REDACTED})") } else { who }
        }
    }
}

/// A thread as lines to read: who said each turn, then what, its lines
/// after the first indented; a turn deleted the first way left out.
pub fn said_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in all_turns(text).into_iter().filter(|t| !t.deleted) {
        let who = said_by(&t);
        for (k, line) in t.body.lines().enumerate() {
            out.push(if k == 0 { format!("{who}: {line}") } else { format!("  {line}") });
        }
    }
    out
}

/// The turns of a stored comment that stand, those deleted left out: what
/// the agent is told, and whose the last word is.
pub fn thread_turns(text: &str) -> Vec<Turn> {
    all_turns(text).into_iter().filter(|t| !t.deleted).collect()
}

/// Turns back to stored text.
pub fn render_thread(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(|t| {
            if t.deleted {
                let who = match t.role {
                    Role::You => "you",
                    Role::Agent => "agent",
                    Role::Reviewer => "reviewer",
                };
                let tag = match t.tag.as_deref() {
                    Some(tag) if !tag.is_empty() => format!(" {tag}"),
                    _ => String::new(),
                };
                let quoted: Vec<String> = t.body.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect();
                return format!("{DELETED_OPEN} {who}{tag}\n{}\n{FENCE_CLOSE}", quoted.join("\n"));
            }
            let open = match t.role {
                Role::You => return yours(&t.body),
                Role::Agent => word(AGENT_OPEN),
                Role::Reviewer => word(REVIEWER_OPEN),
            };
            let tag = t.tag.as_deref().unwrap_or("");
            // A reviewer's, redacted or an edit: named, as its marks follow
            // the name.
            if t.role == Role::Reviewer && (t.redacted || t.edit.is_some()) {
                let mut open = format!("{open} {}", if tag.is_empty() { "someone" } else { tag });
                if t.redacted {
                    open += &format!(" {REDACTED}");
                }
                if let Some(was) = &t.edit {
                    open += &format!(" {EDIT} {}", was.lines().next().unwrap_or("").trim());
                }
                return fenced(&open, &t.body);
            }
            // The agent's: the effort its run was asked at after the way.
            if t.role == Role::Agent {
                return fenced(&agent_open(tag, t.effort.as_deref(), t.stack), &t.body);
            }
            match tag {
                "" => fenced(open, &t.body),
                tag => fenced(&format!("{open} {tag}"), &t.body),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `body`, a turn of yours, as a thread holds it: as written, unless a
/// line of it would be read as a fence of a turn (```agent, ```reviewer):
/// then fenced as yours, with more backticks than any of its own.
pub fn yours(body: &str) -> String {
    let body = body.trim();
    match body.lines().any(|l| fence(l).is_some()) {
        true => fenced(word(YOU_OPEN), body),
        false => body.to_string(),
    }
}

pub fn append_turn(text: &str, role: Role, body: &str, tag: Option<&str>) -> String {
    appended(text, role, body, tag, None, None)
}

/// `text` with a turn of the agent's after the rest, `body`, asked in the
/// way `tag` names (`answer`), at `effort` when its run had one: the effort
/// on its fence, every turn before it as it was.
pub fn append_agent(text: &str, body: &str, tag: &str, effort: Option<&str>) -> String {
    appended(text, Role::Agent, body, Some(tag), effort, None)
}

/// As `append_agent`, the turn brought back by an answer-only ask asked on
/// the stack at `stack`, when it was one: that stack's rev on its fence.
pub fn append_agent_on(text: &str, body: &str, tag: &str, effort: Option<&str>, stack: Option<usize>) -> String {
    appended(text, Role::Agent, body, Some(tag), effort, stack)
}

fn appended(text: &str, role: Role, body: &str, tag: Option<&str>, effort: Option<&str>, stack: Option<usize>) -> String {
    let mut turns = all_turns(text);
    turns.push(Turn {
        role,
        body: body.trim().to_string(),
        tag: tag.map(String::from),
        deleted: false,
        edit: None,
        redacted: false,
        effort: effort.map(String::from),
        stack,
    });
    render_thread(&turns)
}

/// Your last turn of `text`, if the last word is yours: what you wrote
/// since the agent's or a reviewer's last turn, the comment itself if no
/// one has answered it.
pub fn last_mine(text: &str) -> Option<String> {
    thread_turns(text).pop().filter(|t| t.role == Role::You).map(|t| t.body)
}

/// Where your last turn is among all the turns of `text`, those deleted
/// too, if the last word is yours.
pub fn last_mine_at(text: &str) -> Option<usize> {
    let turns = all_turns(text);
    let at = turns.iter().rposition(|t| !t.deleted)?;
    (turns[at].role == Role::You).then_some(at)
}

/// `text` with your last turn written again as `body`; none if the last
/// word is not yours.
pub fn replace_last_turn(text: &str, body: &str) -> Option<String> {
    edit_turn(text, last_mine_at(text)?, body)
}

/// `text` with turn `i` of all its turns written again as `body`, whoever
/// said it; none if there is no such turn, it is deleted, or `body` is
/// empty.
pub fn edit_turn(text: &str, i: usize, body: &str) -> Option<String> {
    if body.trim().is_empty() {
        return None;
    }
    let mut turns = all_turns(text);
    let t = turns.get_mut(i).filter(|t| !t.deleted)?;
    t.body = body.trim().to_string();
    Some(render_thread(&turns))
}

/// Where a rewind from turn `i` of all the turns of `text` starts: that
/// turn if it is yours, else your turn above it; none if there is no turn
/// of yours there, nor above.
pub fn rewind_from(text: &str, i: usize) -> Option<usize> {
    let turns = all_turns(text);
    (0..=i.min(turns.len().checked_sub(1)?)).rev().find(|&j| !turns[j].deleted && turns[j].role == Role::You)
}

/// `text` rewound from turn `k` of all its turns: the turns before it as
/// they were, then `body`, yours, in the place of that turn and of every
/// turn after it. None if `body` is empty: then nothing changes.
pub fn rewind(text: &str, k: usize, body: &str) -> Option<String> {
    let mut turns = all_turns(text);
    if body.trim().is_empty() || k >= turns.len() {
        return None;
    }
    turns.truncate(k);
    turns.push(Turn {
        role: Role::You,
        body: body.trim().to_string(),
        tag: None,
        deleted: false,
        edit: None,
        redacted: false,
        effort: None,
        stack: None,
    });
    Some(render_thread(&turns))
}

/// What the editor of a rewind says first, in $EDITOR: its rules.
pub const REWIND_SAYS: [&str; 7] = [
    "# You are rewinding this conversation from one of your turns. Write",
    "# above these lines what takes its place. Once you save, that turn and",
    "# every turn after it, quoted below, are gone, and your text is the",
    "# conversation's last turn. The lines stacked wrote here, starting with",
    "# #, are dropped; any other line is yours, # or not. If you write",
    "# nothing, or quit without saving, the conversation is as it was.",
    "#",
];

/// The lines a rewind from turn `k` of all the turns of `text` writes in
/// its editor, each starting with `#`: what goes once it is saved, that
/// turn included, each turn under who said it.
pub fn rewound_lines(text: &str, k: usize) -> Vec<String> {
    let mut out = Vec::new();
    for t in all_turns(text).into_iter().skip(k).filter(|t| !t.deleted) {
        out.push(format!("# {}:", said_by(&t)));
        out.extend(t.body.lines().map(|l| if l.trim().is_empty() { "#".to_string() } else { format!("#   {l}") }));
    }
    out
}

/// `text` without the lines stacked `wrote` in it, as they were written:
/// any other line is the user's, `#` or not.
pub fn without_written(text: &str, wrote: &[String]) -> String {
    let wrote: std::collections::HashSet<&str> = wrote.iter().map(|l| l.trim_end()).collect();
    let kept: Vec<&str> = text.lines().filter(|l| !wrote.contains(l.trim_end())).collect();
    kept.join("\n").trim().to_string()
}

/// The mode claude's latest reply was given in, "unknown" for one from
/// before modes were recorded, None when there is no reply.
pub fn last_reply_mode(text: &str) -> Option<String> {
    let last = thread_turns(text)
        .into_iter()
        .rfind(|t| t.role == Role::Agent)?;
    Some(match last.tag.unwrap_or_default() {
        m if m.is_empty() => "unknown".into(),
        m => m,
    })
}

pub fn last_role(text: &str) -> Option<Role> {
    thread_turns(text).last().map(|t| t.role)
}

/// The fence tag claude's latest reply was given in (`change`, `answer`,
/// `test`), None when it has not replied.
pub fn last_reply_way(text: &str) -> Option<String> {
    thread_turns(text)
        .into_iter()
        .rfind(|t| t.role == Role::Agent)
        .map(|t| t.tag.unwrap_or_default())
}

/// The effort the run of the agent's latest reply was asked at, if it
/// says one.
pub fn last_reply_effort(text: &str) -> Option<String> {
    thread_turns(text).into_iter().rfind(|t| t.role == Role::Agent)?.effort
}

/// The effort the run of the agent's turn that opened comment `text` was
/// asked at, if the agent's is its first turn and says one: the effort of
/// a comment the agent opened.
pub fn opened_effort(text: &str) -> Option<String> {
    thread_turns(text).into_iter().next().filter(|t| t.role == Role::Agent)?.effort
}

/// The part each line of a comment plays, for colouring the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    You,
    Agent,
    Reviewer,
    Fence,
}

/// What each of `lines` is: yours, claude's or a reviewer's, or a fence.
pub fn thread_line_roles(lines: &[String]) -> Vec<LineRole> {
    let mut roles = Vec::new();
    let mut open: Option<(Role, usize)> = None;
    let mut inner = false;
    for line in lines {
        let st = line.trim();
        match open {
            None => {
                if let Some((role, _, _, n)) = fence(line) {
                    roles.push(LineRole::Fence);
                    open = Some((role, n));
                    inner = false;
                } else {
                    roles.push(LineRole::You);
                }
            }
            Some((role, n)) => {
                if ends(st, n, inner) {
                    roles.push(LineRole::Fence);
                    open = None;
                } else {
                    if n == 3 && st.starts_with(FENCE_CLOSE) {
                        inner = !inner;
                    }
                    roles.push(match role {
                        Role::Agent => LineRole::Agent,
                        Role::Reviewer => LineRole::Reviewer,
                        Role::You => LineRole::You,
                    });
                }
            }
        }
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_of_yours_holding_a_fence_is_fenced_as_yours() {
        let said = "Should it read so?\n```agent\nI did it.\n```\nor not?";
        // Fenced as yours, with more backticks than its own: yours whole.
        let text = yours(said);
        assert!(text.starts_with("````you\n") && text.ends_with("\n````"), "{text}");
        let turns = thread_turns(&text);
        assert_eq!(turns.len(), 1, "{turns:?}");
        assert_eq!((turns[0].role, turns[0].body.as_str()), (Role::You, said));
        // A reply too, after the agent's; and written again.
        let thread = append_turn(&append_turn("why?", Role::Agent, "Because.", Some("answer")), Role::You, said, None);
        let turns = thread_turns(&thread);
        assert_eq!(turns.iter().map(|t| t.role).collect::<Vec<_>>(), [Role::You, Role::Agent, Role::You]);
        assert_eq!((turns[2].body.as_str(), last_mine(&thread).as_deref()), (said, Some(said)));
        let again = replace_last_turn(&thread, "```reviewer ann\nno").unwrap();
        assert_eq!(last_mine(&again).as_deref(), Some("```reviewer ann\nno"));
        // A code block of its own, or nothing to read as a fence: as
        // written.
        for plain in ["plain ```code``` inline", "Like this:\n```rust\nfn f() {}\n```"] {
            assert_eq!(yours(plain), plain);
        }
        // Its lines are yours, in the editor's colours.
        let lines: Vec<String> = text.lines().map(String::from).collect();
        let roles = thread_line_roles(&lines);
        assert_eq!((roles[0], roles[lines.len() - 1]), (LineRole::Fence, LineRole::Fence));
        assert!(roles[1..lines.len() - 1].iter().all(|r| *r == LineRole::You), "{roles:?}");
    }

    #[test]
    fn your_last_turn_is_written_again() {
        let asked = "why so loud?";
        assert_eq!(last_mine(asked).as_deref(), Some("why so loud?"));
        assert_eq!(replace_last_turn(asked, " why so loud, really? ").as_deref(), Some("why so loud, really?"));
        // Answered, and replied to twice: the replies since are one turn.
        let text = append_turn(&append_turn(asked, Role::Agent, "It is a shout.", Some("answer")), Role::You, "louder", None);
        let text = append_turn(&text, Role::You, "much louder", None);
        assert_eq!(last_mine(&text).as_deref(), Some("louder\nmuch louder"));
        let again = replace_last_turn(&text, "quieter").unwrap();
        let turns = thread_turns(&again);
        assert_eq!(turns.len(), 3);
        assert_eq!((turns[1].role, turns[1].body.as_str(), turns[1].tag.as_deref()), (Role::Agent, "It is a shout.", Some("answer")));
        assert_eq!((turns[2].role, turns[2].body.as_str()), (Role::You, "quieter"));
        // The agent's last word, or a reviewer's: none of yours to edit.
        let answered = append_turn(asked, Role::Agent, "It is a shout.", Some("answer"));
        assert_eq!((last_mine(&answered), replace_last_turn(&answered, "x")), (None, None));
        assert_eq!(last_mine(&append_turn(asked, Role::Reviewer, "Agreed.", Some("ann"))), None);
    }

    #[test]
    fn any_turn_is_written_again_and_a_conversation_rewound_from_yours() {
        let text = append_turn("hi", Role::Agent, "hi\n\n```rust\nlong code\n```", Some("answer"));
        let text = append_turn(&text, Role::You, "how are you", None);
        let text = append_turn(&text, Role::Reviewer, "Fine.", Some("ann"));
        let names: Vec<String> = all_turns(&text).iter().enumerate().map(|(i, t)| t.name(i)).collect();
        assert_eq!(names, ["your comment", "the agent's reply", "your reply", "ann's reply"]);
        // The agent's reply, trimmed to what is worth keeping.
        let trimmed = edit_turn(&text, 1, "hi").unwrap();
        let turns = thread_turns(&trimmed);
        assert_eq!((turns[1].role, turns[1].body.as_str(), turns[1].tag.as_deref()), (Role::Agent, "hi", Some("answer")));
        assert_eq!(edit_turn(&text, 1, " "), None, "nothing to write: no edit");
        assert_eq!(edit_turn(&text, 9, "x"), None);
        // A rewind goes from your turn there, or from yours above.
        assert_eq!((0..4).map(|i| rewind_from(&text, i)).collect::<Vec<_>>(), [Some(0), Some(0), Some(2), Some(2)]);
        assert_eq!(rewind_from(&append_turn("", Role::Agent, "Look.", Some("answer")), 0), None, "none of yours");
        // What goes is written as # lines, under who said it, that turn
        // included; the new text takes its place and that of all after it.
        let lines = rewound_lines(&text, 2);
        assert_eq!(lines, ["# you:", "#   how are you", "# ann:", "#   Fine."]);
        let edited = format!("how is it going?\n{}", lines.join("\n"));
        let again = rewind(&text, 2, &without_written(&edited, &lines)).unwrap();
        let said: Vec<(Role, String)> = all_turns(&again).into_iter().map(|t| (t.role, t.body)).collect();
        assert_eq!(said, [(Role::You, "hi".into()), (Role::Agent, "hi\n\n```rust\nlong code\n```".into()), (Role::You, "how is it going?".into())]);
        // Lines of yours starting with #, kept; with nothing of yours, nothing
        // changes.
        assert_eq!(without_written(&format!("# Title\n{}", lines.join("\n")), &lines), "# Title");
        assert_eq!(rewind(&text, 2, &without_written(&lines.join("\n"), &lines)), None);
        // From the comment itself, the whole thread is written again.
        assert_eq!(all_turns(&rewind(&text, 0, "bye").unwrap()).len(), 1);
    }

    #[test]
    fn a_reply_with_fences_of_its_own_is_fenced_longer_and_read_whole() {
        // A code block whose fence names no language, as an agent writes one.
        let reply = "Shorter:\n\n```\n  returns the list.\n```\n\nThat is all.";
        let text = append_turn("Rewrite it.", Role::Agent, reply, Some("answer"));
        assert!(text.contains("\n````agent answer\n") && text.ends_with("\n````"), "{text}");
        let text = append_turn(&text, Role::You, "Thanks.", None);
        let said: Vec<(Role, String)> = all_turns(&text).into_iter().map(|t| (t.role, t.body)).collect();
        assert_eq!(said, [(Role::You, "Rewrite it.".into()), (Role::Agent, reply.into()), (Role::You, "Thanks.".into())]);
        // Fences longer still in it: one more backtick than they have.
        let text = fenced("reviewer ann", "````\nx\n````");
        assert!(text.starts_with("`````reviewer ann\n") && text.ends_with("\n`````"), "{text}");
        let t = &thread_turns(&text)[0];
        assert_eq!((t.role, t.body.as_str(), t.tag.as_deref()), (Role::Reviewer, "````\nx\n````", Some("ann")));
        // None in it: three, as ever.
        assert_eq!(fenced("agent answer", "Seen."), "```agent answer\nSeen.\n```");
        // The editor colours its lines as the thread reads them.
        let lines: Vec<String> = append_turn("hi", Role::Agent, reply, Some("answer")).lines().map(String::from).collect();
        let roles = thread_line_roles(&lines);
        assert_eq!(roles[0], LineRole::You);
        assert_eq!(roles[1], LineRole::Fence);
        assert!(roles[2..roles.len() - 1].iter().all(|r| *r == LineRole::Agent), "{roles:?}");
        assert_eq!(roles.last(), Some(&LineRole::Fence));
    }

    #[test]
    fn a_reviewer_s_edit_is_headed_by_what_it_edits_and_the_turn_it_replaces_is_redacted() {
        // As the fences have them: the comment, redacted; your reply; her
        // edit, the comment's first line on its fence.
        let text = [fenced("reviewer ann redacted", "Hi Da"), "Hi ann! My name is Damien".to_string(), fenced("reviewer ann edit Hi Da", "Hi Damien!")].join("\n");
        let turns = all_turns(&text);
        let names: Vec<String> = turns.iter().enumerate().map(|(i, t)| t.name(i)).collect();
        assert_eq!(names, ["ann's comment (redacted)", "your reply", "ann's edit of the comment that said \"Hi Da\""]);
        assert_eq!((turns[0].tag.as_deref(), turns[0].redacted, &turns[0].edit), (Some("ann"), true, &None));
        assert_eq!((turns[2].tag.as_deref(), turns[2].redacted, turns[2].edit.as_deref()), (Some("ann"), false, Some("Hi Da")));
        // Written again as read; an edit redacted in its turn, both marks.
        assert_eq!(render_thread(&turns), text);
        let mut again = turns.clone();
        again[2].redacted = true;
        let both = render_thread(&again);
        assert!(both.ends_with("```reviewer ann redacted edit Hi Da\nHi Damien!\n```"), "{both}");
        assert_eq!(all_turns(&both)[2].name(2), "ann's edit of the comment that said \"Hi Da\" (redacted)");
        // As a thread is read as lines, and as a rewind writes it: headed so.
        assert_eq!(said_lines(&text), ["ann (redacted): Hi Da", "you: Hi ann! My name is Damien", "ann's edit of the comment that said \"Hi Da\": Hi Damien!"]);
        assert_eq!(rewound_lines(&text, 2), ["# ann's edit of the comment that said \"Hi Da\":", "#   Hi Damien!"]);
        // A long first line, quoted short; only the first line.
        let long = "This sentence is long enough that the heading of an edit of it must cut it short somewhere.";
        let t = &all_turns(&fenced(&format!("reviewer ann edit {long}"), "Shorter."))[0];
        assert_eq!(t.edit.as_deref(), Some(long));
        assert_eq!(t.name(1), "ann's edit of the comment that said \"This sentence is long enough that the heading of an edit of…\"");
        // A reviewer's name and nothing else after it, as ever: a name.
        let t = &all_turns(&fenced("reviewer ann smith", "Hi."))[0];
        assert_eq!((t.tag.as_deref(), t.redacted, &t.edit), (Some("ann smith"), false, &None));
    }

    #[test]
    fn the_agent_s_turn_says_the_effort_its_run_was_asked_at_and_one_of_before_reads_as_ever() {
        // Filed at max: the effort on its fence, after the way it was asked.
        let text = append_agent("why so loud?", "It is a shout.", "answer", Some("max"));
        assert_eq!(text, "why so loud?\n```agent answer effort max\nIt is a shout.\n```");
        let t = &thread_turns(&text)[1];
        assert_eq!((t.role, t.tag.as_deref(), t.effort.as_deref()), (Role::Agent, Some("answer"), Some("max")));
        // Headed and named with it; the way it was asked as it was.
        assert_eq!((said_by(t), t.name(1)), ("agent(max)".to_string(), "the agent's reply (max)".to_string()));
        assert_eq!((last_reply_way(&text).as_deref(), last_reply_effort(&text).as_deref()), (Some("answer"), Some("max")));
        assert_eq!(said_lines(&text), ["you: why so loud?", "agent(max): It is a shout."]);
        assert_eq!(rewound_lines(&text, 0), ["# you:", "#   why so loud?", "# agent(max):", "#   It is a shout."]);
        // Written again, a reply added, a turn edited, a code block in one:
        // each turn keeps its own, or has none.
        let more = append_turn(&text, Role::You, "louder?", None);
        let more = append_agent(&more, "Louder:\n\n```\nSHOUT\n```", "change", Some("low"));
        let more = append_agent(&more, "As ever.", "answer", None);
        let more = edit_turn(&more, 1, "It is a shout, really.").unwrap();
        assert!(more.contains("\n````agent change effort low\nLouder:\n"), "{more}");
        let marks: Vec<(String, Option<String>)> = all_turns(&more).into_iter().map(|t| (said_by(&t), t.effort)).collect();
        assert_eq!(
            marks,
            [("you".into(), None), ("agent(max)".into(), Some("max".into())), ("you".into(), None), ("agent(low)".into(), Some("low".into())), ("agent".into(), None)]
        );
        assert_eq!(render_thread(&all_turns(&more)), more);
        // A turn of before: no effort, read and written as it was.
        let old = "why?\n```agent answer\nBecause.\n```\nthanks\n```claude change\nDone.\n```";
        let turns = all_turns(old);
        assert!(turns.iter().all(|t| t.effort.is_none()), "{turns:?}");
        assert_eq!(turns.iter().enumerate().map(|(i, t)| t.name(i)).collect::<Vec<_>>(), ["your comment", "the agent's reply", "your reply", "the agent's reply"]);
        assert_eq!(said_lines(old), ["you: why?", "agent: Because.", "you: thanks", "agent: Done."]);
        assert_eq!((last_reply_way(old).as_deref(), last_reply_effort(old)), (Some("change"), None));
        assert_eq!(render_thread(&turns), "why?\n```agent answer\nBecause.\n```\nthanks\n```agent change\nDone.\n```");
        // A comment the agent opened: its first turn says it, named the
        // agent's comment at it.
        let opened = fenced(&agent_open("answer", Some("xhigh"), None), "Look here.");
        assert_eq!(opened, "```agent answer effort xhigh\nLook here.\n```");
        assert_eq!((opened_effort(&opened).as_deref(), all_turns(&opened)[0].name(0)), (Some("xhigh"), "the agent's comment (xhigh)".into()));
        assert_eq!((opened_effort("mine"), opened_effort(&append_agent("mine", "Seen.", "answer", Some("max")))), (None, None));
        // No way named, the effort alone; the opening words as ever.
        assert_eq!((agent_open("", Some("high"), None), agent_open("test", None, None), agent_open("", None, None)), ("agent effort high".into(), "agent test".into(), "agent".into()));
        let t = &all_turns("```agent effort high\nHm.\n```")[0];
        assert_eq!((t.tag.as_deref(), t.effort.as_deref()), (Some(""), Some("high")));
        // A reviewer's name and the word: a name, as ever.
        let t = &all_turns(&fenced("reviewer effort max", "Hi."))[0];
        assert_eq!((t.tag.as_deref(), t.effort.as_deref(), said_by(t)), (Some("effort max"), None, "effort max".to_string()));
        // Said with it elsewhere, as what was asked ends with it.
        assert_eq!((agent_at(None), named_at("the agent's work", None), asked_at(None)), ("agent".into(), "the agent's work".into(), String::new()));
        assert_eq!(asked_at(Some("low")), " at low");
    }

    #[test]
    fn an_answer_s_turn_names_the_stack_it_was_asked_on_last_on_its_fence() {
        // Filed: the stack's rev last, after the effort, or alone.
        let text = append_agent_on("why so loud?", "It is a shout.", "answer", Some("max"), Some(12));
        assert_eq!(text, "why so loud?\n```agent answer effort max stack 12\nIt is a shout.\n```");
        let t = &thread_turns(&text)[1];
        assert_eq!((t.tag.as_deref(), t.effort.as_deref(), t.stack), (Some("answer"), Some("max"), Some(12)));
        // Read as the way it was asked and the effort, as ever: the way, the
        // heading, what a thread read as lines says.
        assert_eq!((last_reply_way(&text).as_deref(), last_reply_effort(&text).as_deref()), (Some("answer"), Some("max")));
        assert_eq!((said_by(t), t.name(1)), ("agent(max)".to_string(), "the agent's reply (max)".to_string()));
        assert_eq!(said_lines(&text), ["you: why so loud?", "agent(max): It is a shout."]);
        let alone = all_turns("```agent answer stack 3\nSeen.\n```");
        assert_eq!((alone[0].tag.as_deref(), alone[0].effort.as_deref(), alone[0].stack), (Some("answer"), None, Some(3)));
        // Written again with the thread, each turn's kept; one of before, or
        // of another ask, names none.
        let more = append_agent(&append_turn(&text, Role::You, "louder?", None), "Louder.", "change", Some("low"));
        let stacks: Vec<Option<usize>> = all_turns(&more).into_iter().map(|t| t.stack).collect();
        assert_eq!(stacks, [None, Some(12), None, None]);
        assert_eq!(render_thread(&all_turns(&more)), more);
        // A word that is no rev is part of the way, as before.
        let t = &all_turns("```agent answer stack x\nHm.\n```")[0];
        assert_eq!((t.tag.as_deref(), t.stack), (Some("answer stack x"), None));
        // How old, said.
        use crate::db::Since;
        let said: Vec<Option<String>> = [Since::Edits(0), Since::Edits(1), Since::Edits(2), Since::Elsewhere].into_iter().map(asked_on_said).collect();
        assert_eq!(
            said,
            [None, Some("on the stack 1 edit ago".into()), Some("on the stack 2 edits ago".into()), Some("on another version of the stack".into())]
        );
    }

    #[test]
    fn a_turn_deleted_the_first_way_is_read_only_to_be_left_out() {
        // As stacked's first way of deleting a turn wrote it: fenced, quoted.
        let text = "hi\n```deleted agent answer\n> hello\n```\nhow are you";
        let all = all_turns(text);
        assert_eq!(all.len(), 3);
        assert!(all[1].deleted && all[1].role == Role::Agent);
        let said: Vec<String> = thread_turns(text).into_iter().map(|t| t.body).collect();
        assert_eq!(said, ["hi", "how are you"]);
        // Kept as it was when the thread is written again.
        let more = append_turn(text, Role::You, "well?", None);
        assert!(more.contains("```deleted agent answer\n> hello\n```"), "{more}");
    }
}
