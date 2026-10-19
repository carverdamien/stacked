//! A comment is a thread of turns. The agent's replies are fenced ```agent
//! blocks naming what it was asked to do (```claude, as self-review and
//! stacked's first files have them, is read the same), a reviewer's
//! comments pulled from Phabricator are fenced ```reviewer blocks naming
//! who wrote them, and every other line is yours. A turn you deleted is a
//! fenced ```deleted block naming whose it was, what it said kept in it,
//! quoted, to be brought back: the thread shows that it was deleted, and
//! the agent is not told it.

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
/// it names none), the name for a reviewer's; and whether you deleted it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub body: String,
    pub tag: Option<String>,
    /// Deleted: shown as such, and not told to the agent; what it said is
    /// kept, to bring it back.
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

/// `text` with turn `i` of all its turns deleted, what it said kept to
/// bring it back; none if there is no such turn, it is deleted already, or
/// it is the only one that stands.
pub fn delete_turn(text: &str, i: usize) -> Option<String> {
    let mut turns = all_turns(text);
    if turns.iter().filter(|t| !t.deleted).count() < 2 {
        return None;
    }
    let t = turns.get_mut(i).filter(|t| !t.deleted)?;
    t.deleted = true;
    Some(render_thread(&turns))
}

/// `text` with deleted turn `i` of all its turns back, as it was; none if
/// there is no such turn, or it is not deleted.
pub fn bring_back_turn(text: &str, i: usize) -> Option<String> {
    let mut turns = all_turns(text);
    let t = turns.get_mut(i).filter(|t| t.deleted)?;
    t.deleted = false;
    Some(render_thread(&turns))
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
    fn any_turn_is_written_again_or_deleted_and_brought_back() {
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
        // Deleted, it stands no more: your two turns stay two, the agent is
        // not told it, and what it said is kept, code and all.
        let gone = delete_turn(&text, 1).unwrap();
        let said: Vec<String> = thread_turns(&gone).into_iter().map(|t| t.body).collect();
        assert_eq!(said, ["hi", "how are you", "Fine."]);
        let all = all_turns(&gone);
        assert_eq!(all.len(), 4);
        assert!(all[1].deleted && all[1].role == Role::Agent && all[1].body == "hi\n\n```rust\nlong code\n```", "{all:?}");
        assert_eq!((delete_turn(&gone, 1), edit_turn(&gone, 1, "x")), (None, None), "deleted already");
        // Your last word deleted, the last word is ann's; ann's deleted, the
        // last word is yours again, to edit.
        let gone = delete_turn(&gone, 3).unwrap();
        assert_eq!(last_role(&gone), Some(Role::You));
        assert_eq!((last_mine(&gone).as_deref(), last_mine_at(&gone)), (Some("how are you"), Some(2)));
        assert_eq!(all_turns(&gone)[3].name(3), "ann's reply");
        // A reply after a deleted turn is a turn of its own.
        let more = append_turn(&gone, Role::You, "well?", None);
        assert_eq!(all_turns(&more).len(), 5);
        assert_eq!(last_mine(&more).as_deref(), Some("well?"));
        // Brought back, each is as it was.
        let back = bring_back_turn(&bring_back_turn(&gone, 1).unwrap(), 3).unwrap();
        assert_eq!(all_turns(&back), all_turns(&text));
        assert_eq!(bring_back_turn(&back, 1), None, "not deleted");
        // The only turn that stands is not deleted: the comment is.
        assert_eq!(delete_turn("hi", 0), None);
        let answered = delete_turn(&append_turn("q", Role::Agent, "a", Some("answer")), 1).unwrap();
        assert_eq!(delete_turn(&answered, 0), None);
    }
}
