//! Asking claude: the prompts, its allowed tools, and reading what it
//! answers and the stream it writes on the way.

use serde_json::{Map, Value};

pub const CLAUDE_PROMPT: &str = "You are answering code-review comments. Stdin is a JSON object mapping an \
opaque id to {file, preview, conversation}, where 'preview' is the source \
line and 'conversation' is the review thread for that line: one or more \
turns, each labelled 'Reviewer:' or 'You (claude, earlier):', or \
'<name> (on Phabricator):' for a comment a reviewer left on the revision. \
Reply to the LATEST reviewer turn, using the earlier turns as context. Answer concisely \
and concretely. Do NOT modify any files. You cannot build the project or \
run its tests, so do not try; reason from the code. Write each reply over \
several short lines (\\n line breaks in the string, a line per point) so \
it is easy to read in a terminal. Output ONLY a JSON object mapping the \
SAME ids to your reply string -- no prose, no markdown, no code fences.";

pub const CLAUDE_REVIEW_PROMPT: &str = "You are reviewing code changes. Stdin is a JSON object {guidance, commit, \
diff, comments}: 'guidance' says what the reviewer wants you to look for \
(review generally when it is empty), 'commit' is the commit's subject and \
message, 'diff' the changes to review, and 'comments' what has already \
been said about them, not to be repeated. You may read files in the \
repository for context, but do NOT modify anything, and you cannot build \
the project or run its tests, so do not try. Report what a careful \
reviewer would point out, one comment per point, anchored to a line of the \
diff. Output ONLY a JSON object {\"comments\": [{\"file\": the path as \
the diff names it, \"side\": \"+\" for an added line, \"-\" for a \
removed one, \" \" for context, \"line\": that line's exact text from the \
diff without its leading +/-/space, \"text\": your comment, over several \
short lines (\\n line breaks, a line per point) so it is easy to read in \
a terminal}]}, with an \
empty list when there is nothing to say -- no prose, no markdown, no code \
fences.";

pub const CLAUDE_PROMPT_EDIT: &str = "You are addressing code-review comments. Stdin is a JSON object mapping an \
opaque id to {file, preview, conversation, commit?, target?}, where \
'preview' is the source line and 'conversation' is the review thread for \
that line: one or more turns, each labelled 'Reviewer:' or 'You (claude, \
earlier):', or '<name> (on Phabricator):' for a comment a reviewer left on \
the revision. Address the LATEST reviewer turn, using the earlier turns as \
context. You cannot build the project or run its tests, so do not try; \
reason from the code. Handle each item by its shape:\n\
- target=='message': the comment is on commit 'commit's MESSAGE. Reword \
that commit: `git commit --amend -m <msg>` (or `-F <file>`) when it is \
HEAD, otherwise rewrite it via the rebase recipe below. Do not edit files.\n\
- a 'commit' field but no target: the diff line shown currently lives in \
commit 'commit'. If the latest reviewer turn asks to MOVE / relocate this \
change into a DIFFERENT commit, do it with git history operations: resolve \
the destination commit from the reviewer's words against the commit list \
below (fall back to `git log`), then use the rebase recipe below to split \
it out of 'commit' and apply it onto the destination (or `git cherry-pick` \
/ `git apply`). Otherwise, make the requested change directly in the \
working tree with your file-editing tools.\n\
- neither field: an ordinary working-tree change; MAKE THE EDIT directly \
with your file-editing tools.\n\
REBASE RECIPE (rewrite a non-HEAD commit, non-interactively): rebases here \
are ALREADY non-interactive -- do NOT set or pass any editor (no `git -c \
core.editor=`/`-c sequence.editor=`, no `GIT_SEQUENCE_EDITOR=...`/`GIT_EDITOR\
=...` prefix); those forms are unnecessary and are blocked by the permission \
gate. Instead: (1) build the rebase plan from `git log --reverse \
--format='%h %s' <base>..HEAD` and write it -- ONE `pick`/`edit`/\
`drop <sha> <subject>` line for EVERY commit in the range (a missing line \
DROPS that commit) -- to the todo file named below with your file-editing \
tools; mark every commit you need to change `edit` (NOT `reword`: the \
message editor is a silent no-op, so `reword` would keep the old message \
and not pause); (2) run a plain `git rebase -i <base>` (the plan you wrote \
is applied automatically); (3) at each `edit` stop, reword with `git commit \
--amend -m <msg>`/`-F <file>` and/or make file edits, then `git rebase \
--continue`. Never expect an editor to open.\n\
Rewriting or reordering a commit changes the SHA of it and every later \
commit, so work one commit at a time and re-resolve commits by \
subject/content rather than a stale SHA. If a rebase or cherry-pick \
conflicts and you cannot resolve it cleanly, run `git rebase --abort` (or \
`git cherry-pick --abort`) to leave history unchanged and say so in your \
reply. Then output ONLY a JSON object mapping the SAME ids to a short \
reply describing what you changed (or why you did not), over several short \
lines (\\n line breaks, a line per point) so it is easy to read in a \
terminal -- no prose, no \
markdown, no code fences.";

const GIT_SUBCOMMANDS: [&str; 14] = [
    "status",
    "log",
    "show",
    "diff",
    "add",
    "restore",
    "reset",
    "commit",
    "rebase",
    "cherry-pick",
    "apply",
    "checkout",
    "switch",
    "config",
];

const READ_COMMANDS: [&str; 22] = [
    "grep", "rg", "find", "ls", "cat", "head", "tail", "wc", "sort", "uniq", "cut", "tr", "awk",
    "sed", "fd", "tree", "which", "file", "stat", "dirname", "basename", "realpath",
];

/// The git history operations, and read-only tools, an editing ask may run
/// without asking: no network, so an ask can never touch a remote. No
/// stash either: a worktree shares it with the user's.
pub fn allowed_tools() -> String {
    let mut tools: Vec<String> = GIT_SUBCOMMANDS
        .iter()
        .map(|s| format!("Bash(git {s} *)"))
        .collect();
    tools.push("Bash(git -c *)".into());
    tools.extend(["Read", "Grep", "Glob", "LS"].map(String::from));
    tools.extend(READ_COMMANDS.iter().map(|c| format!("Bash({c} *)")));
    tools.join(",")
}

/// Text with a leading ```lang and a trailing ``` taken off, as
/// `^```[a-zA-Z]*\n?` and `\n?```$` would.
fn unfence(text: &str) -> String {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t.to_string();
    };
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let rest = match rest.strip_suffix("```") {
        Some(r) => r.strip_suffix('\n').unwrap_or(r),
        None => rest,
    };
    rest.trim().to_string()
}

/// Best-effort extraction of an {id: reply} map from claude's output.
pub fn parse_claude_replies(text: &str) -> Option<Map<String, Value>> {
    let text = unfence(text);
    let obj: Value = serde_json::from_str(&text).ok().or_else(|| {
        let (i, j) = (text.find('{')?, text.rfind('}')?);
        if j <= i {
            return None;
        }
        serde_json::from_str(&text[i..=j]).ok()
    })?;
    let obj = obj.as_object()?;
    Some(
        obj.iter()
            .map(|(k, v)| {
                let reply = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (k.clone(), Value::String(reply))
            })
            .collect(),
    )
}

/// The notes a review answered with, or None if unreadable.
pub fn parse_review_notes(text: &str) -> Option<Vec<Map<String, Value>>> {
    let text = unfence(text);
    let candidates = [
        text.clone(),
        match (text.find('{'), text.rfind('}')) {
            (Some(i), Some(j)) if j >= i => text[i..=j].to_string(),
            _ => String::new(),
        },
    ];
    let obj: Value = candidates
        .iter()
        .find_map(|c| serde_json::from_str(c).ok())?;
    let comments = obj.as_object()?.get("comments")?.as_array()?;
    Some(
        comments
            .iter()
            .filter_map(Value::as_object)
            .filter(|n| {
                let t = match n.get("text") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(v) => v.to_string(),
                };
                !t.trim().is_empty()
            })
            .cloned()
            .collect(),
    )
}

/// C0 control characters but tab, which a terminal cannot show.
pub fn clean(line: &str) -> String {
    line.chars()
        .filter(|&c| !((c as u32) < 0x20 && c != '\t' && c != '\n' || c == '\x7f'))
        .filter(|&c| c != '\n')
        .collect()
}

fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// JSON with ", " and ": ", for a one-line summary.
fn dumps(v: &Value) -> String {
    match v {
        Value::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}: {}", Value::String(k.clone()), dumps(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(a) => format!("[{}]", a.iter().map(dumps).collect::<Vec<_>>().join(", ")),
        v => v.to_string(),
    }
}

/// A one-line gist of a tool call: its command, path or query.
fn tool_summary(inp: Option<&Value>) -> String {
    match inp {
        Some(Value::Object(m)) => {
            for f in ["command", "file_path", "path", "pattern", "query", "url"] {
                if let Some(v) = m.get(f) {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        Value::Null => continue,
                        other => other.to_string(),
                    };
                    if !s.is_empty() && s != "false" && s != "0" {
                        return cut(&s, 160);
                    }
                }
            }
            if m.is_empty() {
                String::new()
            } else {
                cut(&dumps(&Value::Object(m.clone())), 160)
            }
        }
        Some(Value::Null) | None => String::new(),
        Some(v) => cut(
            &match v {
                Value::String(s) => s.clone(),
                o => o.to_string(),
            },
            160,
        ),
    }
}

/// A one-line gist of a tool result.
fn result_summary(content: Option<&Value>) -> String {
    let text = match content {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_object)
            .map(|b| text_of(b.get("text")))
            .collect::<Vec<_>>()
            .join(" "),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = cut(&text, 160);
    if t.is_empty() {
        "(no output)".into()
    } else {
        t
    }
}

/// The text blocks of an assistant message, joined.
pub fn assistant_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_object)
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .map(|b| text_of(b.get("text")))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn message_lines(message: &Value) -> Vec<String> {
    let blocks: Vec<Value> = match message.get("content") {
        Some(Value::String(s)) => vec![serde_json::json!({"type": "text", "text": s})],
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for b in blocks.iter().filter_map(Value::as_object) {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => out.extend(text_of(b.get("text")).lines().map(String::from)),
            Some("tool_use") => {
                let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
                out.push(format!("⚙ {name}: {}", tool_summary(b.get("input"))));
            }
            Some("tool_result") => out.push(format!("  ↳ {}", result_summary(b.get("content")))),
            // What it thinks, when it is not kept from the stream.
            Some("thinking") => out.extend(text_of(b.get("thinking")).lines().filter(|l| !l.trim().is_empty()).map(|l| format!("✻ {l}"))),
            _ => {}
        }
    }
    out
}

/// One stream-json event as transcript lines.
pub fn format_event(event: &Value) -> Vec<String> {
    let t = event.get("type").and_then(Value::as_str);
    match t {
        Some("system") if event.get("subtype").and_then(Value::as_str) == Some("init") => {
            let model = event.get("model").and_then(Value::as_str).unwrap_or("");
            vec![if model.is_empty() {
                "● session started".into()
            } else {
                format!("● session started · {model}")
            }]
        }
        // How far its thinking has gone, its words kept from the stream.
        Some("system") if event.get("subtype").and_then(Value::as_str) == Some("thinking_tokens") => {
            let n = event.get("estimated_tokens").and_then(Value::as_u64).unwrap_or(0);
            vec![format!("✻ thinking · ~{n} tokens")]
        }
        Some("assistant") | Some("user") => {
            message_lines(event.get("message").unwrap_or(&Value::Null))
        }
        Some("result") => {
            let mut out = vec![if event
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "✗ the agent reported an error".to_string()
            } else {
                "✓ the agent finished".to_string()
            }];
            out.extend(denials(event).into_iter().map(|d| format!("⛔ not allowed: {d}")));
            out
        }
        _ => Vec::new(),
    }
}

/// What the agent was not let use, as its result says (`permission_denials`):
/// each tool, with what it was given, short.
pub fn denials(result: &Value) -> Vec<String> {
    result
        .get("permission_denials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|d| {
            let name = d.get("tool_name").and_then(Value::as_str).unwrap_or("a tool");
            match tool_summary(d.get("tool_input")) {
                what if what.is_empty() => name.to_string(),
                what => format!("{name}: {what}"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replies_from_messy_output() {
        let r = parse_claude_replies("```json\n{\"a\": \"yes\", \"b\": [1]}\n```").unwrap();
        assert_eq!(r["a"], "yes");
        assert_eq!(r["b"], "[1]");
        let r = parse_claude_replies("Sure! {\"a\": \"x\"} hope that helps").unwrap();
        assert_eq!(r["a"], "x");
        assert!(parse_claude_replies("no json").is_none());
        assert!(parse_claude_replies("[1, 2]").is_none());
    }

    #[test]
    fn parses_review_notes_keeping_those_with_text() {
        let n = parse_review_notes("{\"comments\": [{\"text\": \"a\"}, {\"text\": \"  \"}, 3]}")
            .unwrap();
        assert_eq!(n.len(), 1);
        assert!(parse_review_notes("{\"x\": 1}").is_none());
    }

    #[test]
    fn formats_stream_events() {
        let init = serde_json::json!({"type": "system", "subtype": "init", "model": "m"});
        assert_eq!(format_event(&init), ["● session started · m"]);
        let counted = serde_json::json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": 688});
        assert_eq!(format_event(&counted), ["✻ thinking · ~688 tokens"]);
        let kept = serde_json::json!({"type": "assistant", "message": {"content": [{"type": "thinking", "thinking": "", "signature": "x"}]}});
        assert!(format_event(&kept).is_empty(), "nothing of it to say");
        let said = serde_json::json!({"type": "assistant", "message": {"content": [{"type": "thinking", "thinking": "first\n\nthen"}]}});
        assert_eq!(format_event(&said), ["✻ first", "✻ then"]);
        let other = serde_json::json!({"type": "system", "subtype": "dev_intent"});
        assert!(format_event(&other).is_empty());
        let tool = serde_json::json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "a\nb"},
            {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}]}});
        assert_eq!(format_event(&tool), ["a", "b", "⚙ Bash: ls"]);
        let res = serde_json::json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "content": "x\n  y"}]}});
        assert_eq!(format_event(&res), ["  ↳ x y"]);
        assert_eq!(
            format_event(&serde_json::json!({"type": "result", "is_error": true})),
            ["✗ the agent reported an error"]
        );
        // What it was not let use, each said.
        let refused = serde_json::json!({"type": "result", "permission_denials": [
            {"tool_name": "mcp__moz__get_bug", "tool_input": {"id": 123}},
            {"tool_name": "Bash", "tool_input": {"command": "curl -s https://bugzilla.mozilla.org/rest/bug/123 | jq ."}}]});
        assert_eq!(
            format_event(&refused),
            [
                "✓ the agent finished",
                "⛔ not allowed: mcp__moz__get_bug: {\"id\": 123}",
                "⛔ not allowed: Bash: curl -s https://bugzilla.mozilla.org/rest/bug/123 | jq ."
            ]
        );
    }

    #[test]
    fn allowed_tools_has_no_network() {
        let t = allowed_tools();
        assert!(t.contains("Bash(git rebase *)") && t.contains("Bash(rg *)") && t.contains("Read"));
        assert!(!t.contains("push") && !t.contains("fetch"));
    }

    #[test]
    fn cleans_control_characters() {
        assert_eq!(clean("a\0b\tc\x1b"), "ab\tc");
    }
}
