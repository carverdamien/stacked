//! A comment is a thread of turns. The agent's replies are fenced ```agent
//! blocks naming what it was asked to do (```claude, as self-review and
//! stacked's first files have them, is read the same), a reviewer's
//! comments pulled from Phabricator are fenced ```reviewer blocks naming
//! who wrote them, and every other line is yours.

pub const AGENT_OPEN: &str = "```agent";
/// The agent's fence as it was first written.
const AGENT_WAS: &str = "```claude";
pub const REVIEWER_OPEN: &str = "```reviewer";
pub const FENCE_CLOSE: &str = "```";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    You,
    Agent,
    Reviewer,
}

/// A turn: who, what, and the fence's tag -- the mode for claude's ("" when
/// it names none), the name for a reviewer's.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub body: String,
    pub tag: Option<String>,
}

/// Who an opening fence line starts a turn for, and its tag.
pub fn fence(line: &str) -> Option<(Role, String)> {
    let s = line.trim();
    for (open, role) in [(AGENT_OPEN, Role::Agent), (AGENT_WAS, Role::Agent), (REVIEWER_OPEN, Role::Reviewer)] {
        if s == open {
            return Some((role, String::new()));
        }
        if let Some(rest) = s.strip_prefix(open).and_then(|r| r.strip_prefix(' ')) {
            return Some((role, rest.trim().to_string()));
        }
    }
    None
}

/// Splits a stored comment into ordered turns, empty ones dropped. A fence
/// inside a reply (a code block of its own) does not end it.
pub fn thread_turns(text: &str) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut mine: Vec<&str> = Vec::new();
    let mut open: Option<(Role, String, Vec<&str>)> = None;
    let mut inner = false;

    let flush = |turns: &mut Vec<Turn>, mine: &mut Vec<&str>| {
        let body = mine.join("\n").trim().to_string();
        if !body.is_empty() {
            turns.push(Turn {
                role: Role::You,
                body,
                tag: None,
            });
        }
        mine.clear();
    };

    for line in text.lines() {
        let st = line.trim();
        match &mut open {
            None => {
                if let Some((role, tag)) = fence(line) {
                    flush(&mut turns, &mut mine);
                    open = Some((role, tag, Vec::new()));
                    inner = false;
                } else {
                    mine.push(line);
                }
            }
            Some((role, tag, body)) => {
                if st == FENCE_CLOSE && !inner {
                    turns.push(Turn {
                        role: *role,
                        body: body.join("\n").trim().to_string(),
                        tag: Some(tag.clone()),
                    });
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
        Some((role, tag, body)) => turns.push(Turn {
            role,
            body: body.join("\n").trim().to_string(),
            tag: Some(tag),
        }),
        None => flush(&mut turns, &mut mine),
    }
    turns.retain(|t| !t.body.is_empty());
    turns
}

/// Turns back to stored text.
pub fn render_thread(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(|t| {
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
    let mut turns = thread_turns(text);
    turns.push(Turn {
        role,
        body: body.trim().to_string(),
        tag: tag.map(String::from),
    });
    render_thread(&turns)
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
                if let Some((role, _)) = fence(line) {
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
