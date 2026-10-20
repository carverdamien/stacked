//! A comment is a thread of turns. The agent's replies are fenced ```agent
//! blocks naming what it was asked to do (```claude, as self-review and
//! stacked's first files have them, is read the same), a reviewer's
//! comments pulled from Phabricator are fenced ```reviewer blocks naming
//! who wrote them, and every other line is yours. A conversation is
//! rewound from one of your turns: your new text takes the place of that
//! turn and of every turn after it.
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
pub const DELETED_OPEN: &str = "```deleted";
pub const FENCE_CLOSE: &str = "```";

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
}

impl Turn {
    /// Turn `i` of its thread, said: `your comment`, `the agent's reply`,
    /// `ann's reply`.
    pub fn name(&self, i: usize) -> String {
        let whose = match self.role {
            Role::You => "your".to_string(),
            Role::Agent => "the agent's".to_string(),
            Role::Reviewer => match self.tag.as_deref() {
                Some(who) if !who.is_empty() => format!("{who}'s"),
                _ => "the reviewer's".to_string(),
            },
        };
        format!("{whose} {}", if i == 0 { "comment" } else { "reply" })
    }
}

/// Who an opening fence line starts a turn for, its tag, and whether the
/// turn was deleted.
pub fn fence(line: &str) -> Option<(Role, String, bool)> {
    let s = line.trim();
    if let Some(rest) = s.strip_prefix(DELETED_OPEN).and_then(|r| r.strip_prefix(' ')) {
        let (who, tag) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
        let role = match who {
            "you" => Role::You,
            "agent" => Role::Agent,
            "reviewer" => Role::Reviewer,
            _ => return None,
        };
        return Some((role, tag.trim().to_string(), true));
    }
    for (open, role) in [(AGENT_OPEN, Role::Agent), (AGENT_WAS, Role::Agent), (REVIEWER_OPEN, Role::Reviewer)] {
        if s == open {
            return Some((role, String::new(), false));
        }
        if let Some(rest) = s.strip_prefix(open).and_then(|r| r.strip_prefix(' ')) {
            return Some((role, rest.trim().to_string(), false));
        }
    }
    None
}

/// A fenced turn, closed: what its fence held. A deleted turn's lines are
/// quoted, so that none of them ends it.
fn closed(role: Role, tag: &str, deleted: bool, body: &[&str]) -> Turn {
    let lines: Vec<&str> = if deleted {
        body.iter().map(|l| l.strip_prefix("> ").or_else(|| l.strip_prefix('>')).unwrap_or(l)).collect()
    } else {
        body.to_vec()
    };
    Turn {
        role,
        body: lines.join("\n").trim().to_string(),
        tag: (role != Role::You).then(|| tag.to_string()),
        deleted,
    }
}

/// Splits a stored comment into ordered turns, those deleted too, empty
/// ones dropped. A fence inside a reply (a code block of its own) does not
/// end it.
pub fn all_turns(text: &str) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut mine: Vec<&str> = Vec::new();
    let mut open: Option<(Role, String, bool, Vec<&str>)> = None;
    let mut inner = false;

    let flush = |turns: &mut Vec<Turn>, mine: &mut Vec<&str>| {
        let body = mine.join("\n").trim().to_string();
        if !body.is_empty() {
            turns.push(Turn {
                role: Role::You,
                body,
                tag: None,
                deleted: false,
            });
        }
        mine.clear();
    };

    for line in text.lines() {
        let st = line.trim();
        match &mut open {
            None => {
                if let Some((role, tag, deleted)) = fence(line) {
                    flush(&mut turns, &mut mine);
                    open = Some((role, tag, deleted, Vec::new()));
                    inner = false;
                } else {
                    mine.push(line);
                }
            }
            Some((role, tag, deleted, body)) => {
                if st == FENCE_CLOSE && !inner {
                    turns.push(closed(*role, tag, *deleted, body));
                    open = None;
                } else {
                    if st.starts_with(FENCE_CLOSE) {
                        inner = !inner;
                    }
                    body.push(line);
                }
            }
        }
    }
    match open {
        Some((role, tag, deleted, body)) => turns.push(closed(role, &tag, deleted, &body)),
        None => flush(&mut turns, &mut mine),
    }
    turns.retain(|t| !t.body.is_empty());
    turns
}

/// A thread as lines to read: who said each turn, then what, its lines
/// after the first indented; a turn deleted the first way left out.
pub fn said_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in all_turns(text).into_iter().filter(|t| !t.deleted) {
        let who = match t.role {
            Role::You => "you",
            Role::Agent => "agent",
            Role::Reviewer => t.tag.as_deref().unwrap_or("reviewer"),
        };
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
                Role::You => return t.body.clone(),
                Role::Agent => AGENT_OPEN,
                Role::Reviewer => REVIEWER_OPEN,
            };
            let tag = t.tag.as_deref().unwrap_or("");
            let open = if tag.is_empty() {
                open.to_string()
            } else {
                format!("{open} {tag}")
            };
            format!("{open}\n{}\n{FENCE_CLOSE}", t.body)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn append_turn(text: &str, role: Role, body: &str, tag: Option<&str>) -> String {
    let mut turns = all_turns(text);
    turns.push(Turn {
        role,
        body: body.trim().to_string(),
        tag: tag.map(String::from),
        deleted: false,
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
        let who = match t.role {
            Role::You => "you".to_string(),
            Role::Agent => "agent".to_string(),
            Role::Reviewer => t.tag.clone().filter(|w| !w.is_empty()).unwrap_or_else(|| "reviewer".into()),
        };
        out.push(format!("# {who}:"));
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
    let mut open: Option<Role> = None;
    let mut inner = false;
    for line in lines {
        let st = line.trim();
        match open {
            None => {
                if let Some((role, _, _)) = fence(line) {
                    roles.push(LineRole::Fence);
                    open = Some(role);
                    inner = false;
                } else {
                    roles.push(LineRole::You);
                }
            }
            Some(role) => {
                if st == FENCE_CLOSE && !inner {
                    roles.push(LineRole::Fence);
                    open = None;
                } else {
                    if st.starts_with(FENCE_CLOSE) {
                        inner = !inner;
                    }
                    roles.push(if role == Role::Agent {
                        LineRole::Agent
                    } else {
                        LineRole::Reviewer
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
