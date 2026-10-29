//! A try push of Firefox's CI, built with the `try-push` feature. A
//! comment asked as a try push (`Way::Try`) has the agent select the CI
//! tasks it asks for, as few or as many as it says, each by its exact label
//! in the task graph and with why it is relevant: it pushes nothing and
//! changes nothing. What it selects lands for your review, as any of its
//! work does: each task accepted or rejected, its reason shown as the
//! agent's comment, which you may answer, the work then going back to the
//! agent for another round. Closing the review with tasks accepted pushes
//! them to try.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A task the agent selected for a try push: its exact label in the task
/// graph; why it is relevant, as the agent said it, its references in a
/// `refs` block at its end; and your answer to that, written in the
/// review, with which the work goes back to the agent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Task {
    pub label: String,
    #[serde(default)]
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

/// How many tasks mach pushes without asking: past them, it asks on its
/// terminal whether to go on, and, told yes, runs them at the lowest
/// priority (push.py's `LARGE_PUSH_THRESHOLD`).
pub const THRESHOLD: usize = 1000;

/// How many times each task may be run, as mach's `--rebuild` takes it:
/// once is no rebuild.
pub const REBUILDS: std::ops::RangeInclusive<u32> = 2..=20;

/// How many tasks a push of `labels` labels schedules, as push.py counts
/// them against its threshold: each label once, times `rebuild`.
pub fn scheduled(labels: usize, rebuild: Option<u32>) -> usize {
    labels * rebuild.unwrap_or(1) as usize
}

/// `n` with a comma between each three of its digits: `1,240`.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Whether `label` can be a task's label as `./mach try fuzzy` is given
/// it, a term that selects that task alone: something, and no white space,
/// which would split it into terms.
fn label_of(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty() && !t.chars().any(|c| c.is_whitespace() || c.is_control())).then(|| t.to_string())
}

/// The tasks the agent's answer `answer` (its last JSON object) selects,
/// in the order it gave them, each label once, the first reason kept; and
/// how many times each is to run, when it asked for more than once. A task
/// is `{"label", "why"}`, its reason a string or what is written with
/// references (`{"text", "refs"}`); a label alone is a task with no reason.
/// A label that cannot select its task (empty, or holding white space) is
/// left out.
pub fn tasks_of(answer: Option<&Map<String, Value>>) -> (Vec<Task>, Option<u32>) {
    let mut out: Vec<Task> = Vec::new();
    let given = answer.and_then(|o| o.get("tasks")).and_then(Value::as_array).cloned().unwrap_or_default();
    for t in &given {
        let (label, why) = match t {
            Value::String(s) => (label_of(s), String::new()),
            Value::Object(o) => (
                o.get("label").and_then(Value::as_str).and_then(label_of),
                o.get("why").map(crate::refs::written).unwrap_or_default(),
            ),
            _ => (None, String::new()),
        };
        let Some(label) = label else { continue };
        if !out.iter().any(|k| k.label == label) {
            out.push(Task { label, why: why.trim().to_string(), answer: None });
        }
    }
    let rebuild = answer
        .and_then(|o| o.get("rebuild"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= *REBUILDS.start())
        .map(|n| n.min(*REBUILDS.end()));
    (out, rebuild)
}

/// What a try push ask is told it does, `what` being what it pushes (the
/// stack, or a patch with those below it), `here` where it works and how
/// it reads the stack, `web` what it may read of the web: the tasks the
/// comments ask for, as few or as many as they say, exact labels, why each;
/// and nothing pushed, nothing changed.
pub fn prompt(what: &str, here: &str, web: &str, curl: bool) -> String {
    let labels = if curl { LABELS } else { "" };
    format!(
        "You are selecting the CI tasks of a try push of {what}, as the comments below ask for \
         them: as few or as many as they say, and for what they say. Each task is one of \
         Firefox's CI tasks, named by its exact label in the task graph, as `./mach try fuzzy \
         --show-chunk-numbers` lists them: `test-linux2404-64/opt-mochitest-browser-chrome-3`, \
         `build-linux64/debug`, `source-test-mozlint-eslint`; a chunked test is a task for each \
         chunk, its label ending with the chunk's number, and no label is a pattern. Say why \
         each is relevant: to what the patches change, and to what is asked. {here} Do NOT \
         push anything, nor run `./mach`: once the user has reviewed the tasks you select, \
         stacked pushes those they accept, with `./mach try fuzzy`, and checks first, with its \
         `--no-push`, that each label selects its task; a label the task graph does not have is \
         said then, and nothing is pushed. Do NOT modify anything: no file, no commit, no \
         branch. You cannot build the project or run its tests; reason from the code. \
         {web}{labels}"
    )
}

/// What a try push ask is told of where to find the labels, when it may
/// read the web with curl.
const LABELS: &str = " The labels of the tasks of mozilla-central's latest push are a JSON \
list at https://firefox-ci-tc.services.mozilla.com/api/index/v1/task/\
gecko.v2.mozilla-central.latest.taskgraph.decision/artifacts/public/target-tasks.json, and \
autoland's with `autoland` in place of `mozilla-central`: a try push may run any of them.";

/// What a try push ask ends its answer with, `end` saying how it gives
/// it: its replies, the tasks it selects, how many times each runs.
pub fn answer_said(end: &str) -> String {
    format!(
        "\n{end}: {{\"replies\": {{label: your reply to that comment: which tasks you selected, and \
         why, in short}}, \"tasks\": [{{\"label\": the task's exact label, \"why\": why it is relevant, \
         to what the patches change and to what is asked}}], \"rebuild\": how many times each task \
         runs, from 2 to 20, only when what is asked wants repeated runs (an intermittent failure)}}, \
         labels as above ([c1] is \"c1\"), each task once, in the order you would read them."
    )
}

/// The JSON Schema of what a try push ask gives besides its replies:
/// `tasks`, each its label and why, written as anything the agent writes
/// is (`written`, with references or not); `rebuild`, how many times each
/// runs.
pub fn schema(written: &Value) -> Vec<(&'static str, Value)> {
    let task = serde_json::json!({
        "type": "object",
        "properties": {"label": {"type": "string"}, "why": written},
        "required": ["label", "why"],
        "additionalProperties": false
    });
    vec![
        ("tasks", serde_json::json!({"type": "array", "items": task})),
        ("rebuild", serde_json::json!({"type": "integer", "minimum": REBUILDS.start(), "maximum": REBUILDS.end()})),
    ]
}

/// What a round asked again of a try push is told it selected before:
/// each task, as the user marked it (`mark`, by its place), with why, and
/// what the user answered to that; and how many times each was to run.
pub fn chosen_said(tasks: &[Task], rebuild: Option<u32>, mark: &dyn Fn(usize) -> Option<bool>) -> String {
    if tasks.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nThe tasks you selected then, each as the user marked it, with why you selected it and what the user answered:");
    for (k, t) in tasks.iter().enumerate() {
        let marked = match mark(k) {
            Some(true) => "accepted",
            Some(false) => "rejected",
            None => "not marked",
        };
        let (why, _) = crate::refs::split(&t.why);
        let why = if why.trim().is_empty() { "no reason given".to_string() } else { why.trim().to_string() };
        out += &format!("\n- `{}` ({marked}): {why}", t.label);
        if let Some(a) = &t.answer {
            out += &format!("\n  The user answered: {}", a.trim());
        }
    }
    if let Some(n) = rebuild {
        out += &format!("\nEach was to run {n} times.");
    }
    out
}

/// What your answers to the reasons of `tasks` say in the thread of the
/// comment that asked, as a turn of yours, once the work goes back to the
/// agent: each task's label, then what you answered. None without any.
pub fn answers_turn(tasks: &[Task]) -> Option<String> {
    let answered: Vec<String> =
        tasks.iter().filter_map(|t| Some(format!("- `{}`: {}", t.label, t.answer.as_deref()?.trim()))).collect();
    (!answered.is_empty()).then(|| format!("On the tasks the agent selected:\n{}", answered.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn the_tasks_are_read_from_the_answer_each_label_once() {
        let a = answer(serde_json::json!({
            "replies": {"c1": "Three tasks."},
            "tasks": [
                {"label": " test-linux2404-64/opt-xpcshell-1 ", "why": "It runs the tests of netwerk."},
                {"label": "build-linux64/debug", "why": {"text": "The build the tests need.", "refs": [{"url": "https://example.com/build"}]}},
                "source-test-mozlint-eslint",
                {"label": "test-linux2404-64/opt-xpcshell-1", "why": "Again."},
                {"label": "two words", "why": "Not a label."},
                {"label": "", "why": "Nothing."},
                {"why": "No label at all."},
                7
            ],
            "rebuild": 3
        }));
        let (tasks, rebuild) = tasks_of(Some(&a));
        let labels: Vec<&str> = tasks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["test-linux2404-64/opt-xpcshell-1", "build-linux64/debug", "source-test-mozlint-eslint"]);
        assert_eq!(tasks[0].why, "It runs the tests of netwerk.", "the first reason kept");
        assert!(tasks[1].why.starts_with("The build the tests need.") && tasks[1].why.contains("```refs\nhttps://example.com/build\n```"), "{}", tasks[1].why);
        assert_eq!((tasks[2].why.as_str(), &tasks[2].answer), ("", &None));
        assert_eq!(rebuild, Some(3));
    }

    #[test]
    fn a_rebuild_is_from_two_to_twenty_and_none_is_none() {
        let of = |v: Value| tasks_of(Some(&answer(serde_json::json!({"tasks": [], "rebuild": v})))).1;
        assert_eq!(of(serde_json::json!(1)), None, "once is no rebuild");
        assert_eq!(of(serde_json::json!(0)), None);
        assert_eq!(of(serde_json::json!(20)), Some(20));
        assert_eq!(of(serde_json::json!(50)), Some(20), "as many as mach takes");
        assert_eq!(of(serde_json::json!("three")), None);
        assert_eq!(tasks_of(None), (vec![], None));
        assert_eq!(scheduled(400, Some(3)), 1200, "as push.py counts them");
        assert_eq!(scheduled(1001, None), 1001);
        assert_eq!([0, 999, 1000, 1240, 1234567].map(thousands), ["0", "999", "1,000", "1,240", "1,234,567"]);
    }

    #[test]
    fn the_prompt_asks_for_what_the_comment_asks_exact_labels_and_why_pushing_nothing() {
        let p = prompt("the stack's patches", "Read the stack.", "You may read the web.", true);
        for said in [
            "as few or as many as they say",
            "exact label",
            "a chunked test is a task for each chunk",
            "no label is a pattern",
            "Say why each is relevant",
            "Do NOT push anything, nor run `./mach`",
            "Do NOT modify anything",
            "target-tasks.json",
        ] {
            assert!(p.contains(said), "{said:?} in {p}");
        }
        assert!(!p.contains("minimal"), "{p}");
        assert!(!prompt("x", "", "", false).contains("target-tasks.json"), "without curl, no address to read");
        let end = answer_said("End your reply with a JSON object");
        assert!(end.contains("\"tasks\"") && end.contains("\"rebuild\"") && end.contains("\"replies\""), "{end}");
    }

    #[test]
    fn what_a_round_asked_again_is_told_and_your_answers_in_the_thread() {
        let tasks = vec![
            Task { label: "a".into(), why: "For a.".into(), answer: Some("Not needed.".into()) },
            Task { label: "b".into(), why: String::new(), answer: None },
        ];
        let said = chosen_said(&tasks, Some(4), &|k| (k == 0).then_some(false));
        assert!(said.contains("- `a` (rejected): For a.\n  The user answered: Not needed."), "{said}");
        assert!(said.contains("- `b` (not marked): no reason given"), "{said}");
        assert!(said.ends_with("Each was to run 4 times."), "{said}");
        assert_eq!(answers_turn(&tasks).as_deref(), Some("On the tasks the agent selected:\n- `a`: Not needed."));
        assert_eq!(answers_turn(&tasks[1..]), None);
    }
}
