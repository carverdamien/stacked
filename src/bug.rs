//! A bug filed for a patch, as a comment of yours on it asks, saying the
//! word bug (`mentioned`), as Firefox's own bug-filing skill files one,
//! with no Bugzilla key: the agent drafts it
//! (`Way::Bug`, built with the `moz-phab` feature), its product and
//! component as `./mach file-info bugzilla-component` names them, its type,
//! its summary and its description written by the skill's rules; the draft
//! lands for your review, to read. Closing the review opens Bugzilla's form
//! to file a bug (`enter_bug.cgi`), filled in with the draft, in your
//! browser: nothing is filed until you submit it there, under your own
//! account. Then, the patch's title naming no bug, the bug's number, typed
//! or pasted as a link, goes in the patch's title as moz-phab writes it
//! (`Bug N - title`), a new version of the patch, its message alone
//! changed, which the next submit sends; the title naming one already, it
//! keeps it, and nothing changes.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The one command of mach an ask drafting a bug may run, as claude takes
/// it: the component of a file, which only reads.
pub const MACH_RULE: &str = "Bash(./mach file-info bugzilla-component *)";

/// The types of a bug, as Bugzilla's `bug_type` takes them.
pub const TYPES: [&str; 3] = ["defect", "enhancement", "task"];

/// The fields of the form besides the draft's own that a draft may fill in,
/// when the bug needs them: the bugs it blocks, those it depends on, and
/// the links it sees also, each as Bugzilla's form names it.
pub const MORE: [&str; 3] = ["blocked", "dependson", "see_also"];

/// A bug the agent drafted: its product and component, its type (one of
/// `TYPES`, or none for Bugzilla's own), its summary and its description;
/// and the other fields of the form it fills in (`MORE`), in that order.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Draft {
    pub product: String,
    pub component: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bug_type: String,
    pub short_desc: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<(String, String)>,
}

impl Draft {
    /// What it is, in a line: `Core :: Networking, “Restored tabs come
    /// back with blank titles”`.
    pub fn said(&self) -> String {
        format!("{} :: {}, “{}”", self.product, self.component, self.short_desc)
    }

    /// The fields of the form it fills in, in order, those it leaves empty
    /// left out.
    pub fn fields(&self) -> Vec<(&str, &str)> {
        let mut out = vec![("product", self.product.as_str()), ("component", self.component.as_str())];
        if !self.bug_type.is_empty() {
            out.push(("bug_type", self.bug_type.as_str()));
        }
        out.push(("short_desc", self.short_desc.as_str()));
        if !self.comment.is_empty() {
            out.push(("comment", self.comment.as_str()));
        }
        out.extend(self.more.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        out
    }
}

/// What is written as text in the answer `v`: a string, or the text of
/// what is written with references (`{"text", "refs"}`), its references
/// left out, as the form has no place for them.
fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => o.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(all)) => all.iter().map(|v| text_of(Some(v))).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

/// The bug the agent's answer `answer` (its last JSON object) drafts, its
/// `bug`: none unless it names a product, a component and a summary. A
/// component given as `Product :: Component`, as mach prints it, with no
/// product, is split; a summary is one line; a type that is none of
/// `TYPES` is left for Bugzilla to set.
pub fn draft_of(answer: Option<&Map<String, Value>>) -> Option<Draft> {
    let o = answer?.get("bug")?.as_object()?;
    let field = |k: &str| text_of(o.get(k)).trim().to_string();
    let (mut product, mut component) = (field("product"), field("component"));
    if let Some((p, c)) = component.split_once("::").map(|(p, c)| (p.trim().to_string(), c.trim().to_string()))
        && (product.is_empty() || product == p)
    {
        (product, component) = (p, c);
    }
    let bug_type = field("bug_type").to_lowercase();
    let short_desc = field("short_desc").split_whitespace().collect::<Vec<_>>().join(" ");
    if product.is_empty() || component.is_empty() || short_desc.is_empty() {
        return None;
    }
    Some(Draft {
        product,
        component,
        bug_type: if TYPES.contains(&bug_type.as_str()) { bug_type } else { String::new() },
        short_desc,
        comment: text_of(o.get("comment")).trim_end().to_string(),
        more: MORE.iter().map(|k| (k.to_string(), field(k))).filter(|(_, v)| !v.is_empty()).collect(),
    })
}

/// `s` percent-encoded, as Python's `urllib.parse.quote` with nothing safe
/// encodes it for a form's query: every byte of its UTF-8 but the
/// unreserved (letters, digits, `-`, `_`, `.`, `~`), `%` and two upper-case
/// hex digits.
pub fn encoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The form to file `d` on the Bugzilla at `bugzilla` (the settings'
/// `reviews.bugzilla`, a slash at its end or not): its `enter_bug.cgi`, each
/// field of the draft percent-encoded in its query, as the skill's
/// file-bug.py makes it.
pub fn form_url(bugzilla: &str, d: &Draft) -> String {
    let query: Vec<String> = d.fields().into_iter().map(|(k, v)| format!("{}={}", encoded(k), encoded(v))).collect();
    format!("{}/enter_bug.cgi?{}", bugzilla.trim_end_matches('/'), query.join("&"))
}

/// The number of the bug `typed` names, as the dialog takes it: the number
/// alone (`2086657`, `#2086657`, `bug 2086657`), or a link to the bug pasted
/// (`https://bugzilla.mozilla.org/show_bug.cgi?id=2086657`, its `id` in its
/// query; `https://bugzil.la/2086657`, the number its path ends with). None
/// for anything else, or 0.
pub fn number_in(typed: &str) -> Option<u64> {
    let t = typed.trim();
    let number = |s: &str| -> Option<u64> {
        (!s.is_empty() && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten().filter(|n| *n > 0)
    };
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        crate::refs::link(t).ok()?;
        let (path, query) = t.split_once('?').unwrap_or((t, ""));
        let query = query.split('#').next().unwrap_or_default();
        if let Some(id) = query.split('&').find_map(|kv| kv.strip_prefix("id=")) {
            return number(id);
        }
        let path = path.split('#').next().unwrap_or_default().trim_end_matches('/');
        return path.rsplit('/').next().and_then(number).filter(|_| path.matches('/').count() >= 3);
    }
    let t = t.strip_prefix('#').unwrap_or(t);
    let t = match t.get(..3) {
        Some(b) if b.eq_ignore_ascii_case("bug") => t[3..].trim_start(),
        _ => t,
    };
    number(t)
}

/// Title `title` naming bug `n`, as moz-phab writes it: `Bug N - title`.
pub fn titled(title: &str, n: u64) -> String {
    format!("Bug {n} - {title}")
}

/// Whether `title` names no bug, as moz-phab reads one in a title
/// (`links::bug_in`): then the bug drafted for its patch goes in it.
#[cfg(feature = "moz-phab")]
pub fn names_none(title: &str) -> bool {
    named_in(title).is_none()
}

/// The bug the title `title` names, as moz-phab reads one in a title
/// (`links::bug_in`): its number. None when it names none.
#[cfg(feature = "moz-phab")]
pub fn named_in(title: &str) -> Option<String> {
    crate::links::bug_in(&title.chars().collect::<Vec<_>>()).map(|(_, _, n)| n)
}

/// Whether `text`, what you ask the agent in a comment, says the word bug,
/// in any case, as a word of its own (`bug`, `Bug:`, `BUG`, `follow-up
/// bug`, `bug 123`, `bug123`), or its plural (`bugs`): then a bug may be
/// drafted from it, as it may be asking for one, whatever its patch's
/// title names. Not in another word
/// (`debug`, `bugzilla`, `humbug`): a word is made of letters, digits and
/// `_`, but for the digits of a number right after `bug`.
pub fn mentioned(text: &str) -> bool {
    let s: Vec<char> = text.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    (0..s.len()).any(|at| {
        let is = |k: usize, c: char| s.get(at + k).is_some_and(|x| x.eq_ignore_ascii_case(&c));
        if !(is(0, 'b') && is(1, 'u') && is(2, 'g')) || (at > 0 && word(s[at - 1])) {
            return false;
        }
        let end = if is(3, 's') { at + 4 } else { at + 3 };
        s.get(end).is_none_or(|c| !word(*c) || (end == at + 3 && c.is_ascii_digit()))
    })
}

/// Bug `n` put in the title of the patch of `change` in stack `name`, as
/// moz-phab writes it (`titled`): a new version of the patch, its message
/// alone changed, written at once as any change of the stack is, refused
/// as any is (a patch that should not change, `Db::guard`). Not while the
/// agent changes the stack, nor once the title names a bug already, which
/// would name two. Where the patch is, from 0.
#[cfg(feature = "moz-phab")]
pub fn put_in_title(db: &mut crate::db::Db, name: &str, change: &str, n: u64) -> crate::db::Result<usize> {
    db.writable(name)?;
    crate::ops::by_hand(db, name)?;
    let s = db.stack(name)?;
    let Some(at) = s.patches.iter().position(|p| db.patch(p).change == change) else {
        return crate::db::err(format!("the patch the bug was drafted for is no longer in {name}: nothing changed"));
    };
    let old = db.patch(&s.patches[at]).clone();
    if !names_none(old.title()) {
        return crate::db::err(format!("the title of patch {} names a bug already: nothing changed", at + 1));
    }
    let message = titled(&old.message, n);
    crate::ops::replace(db, name, at, crate::mbox::Draft { message, ..crate::mbox::Draft::from(&old) }, "reword")?;
    Ok(at)
}

/// What an ask drafting a bug is told it does, `what` being the patch it
/// is for, `named` the bug its title names, if it names one, `here` where
/// it works and how it reads the stack, `web` what it may read of the web:
/// the skill's rules for each field, in its own words, close to them; and
/// nothing filed, nothing opened, nothing changed. The title naming a bug,
/// the bug drafted is another one, as the comments ask, which may point at
/// the title's.
pub fn prompt(what: &str, named: Option<&str>, here: &str, web: &str) -> String {
    let (whose, then) = match named {
        None => (
            "the patch's title names no bug, and the bug you draft is the one it is for".to_string(),
            ", and the patch's title then names it".to_string(),
        ),
        Some(n) => (
            format!(
                "the patch's title names bug {n} already, and the bug you draft is another one, a \
                 follow-up, as the comments ask. Where it fits, set `see_also`, `blocked` or \
                 `dependson` to bug {n}"
            ),
            format!(": the patch's title keeps bug {n}"),
        ),
    };
    format!(
        "You are drafting a Bugzilla bug for {what}, as the comments below ask: {whose}. {here} Do \
         NOT modify anything, and do NOT file the bug: run no `file-bug.py`, open nothing, and send \
         nothing to Bugzilla. Once you answer, stacked opens Bugzilla's form to file a bug, filled in \
         with your draft, in the user's browser: the user reviews it there, and files it under their \
         own name{then}. Draft it so. (1) The product and the component: run `./mach \
         file-info bugzilla-component FILE` on a file the patch changes that stands for it best; it \
         prints `Product :: Component`. If it prints `UNKNOWN`, pick the closest sensible component \
         rather than leave it blank. (2) The type: `defect` when the software shipped does not behave \
         as expected (a regression, a crash, an error); `enhancement` for a new feature or function, \
         or a change of how an existing feature behaves; `task` for a change of a configuration or a \
         parameter, or a refactoring: engineering changes that users do not see (tooling, tests, \
         documentation, the build). (3) The summary, shaped to the type: for a defect, the symptom \
         in the words of someone who hits it (\"Restored tabs come back with blank titles\"), not the \
         suspected cause nor the fix, either of which can turn out wrong, as the symptom is what \
         people search for; for an enhancement, the capability wanted (\"Allow pinned tabs to be \
         restored per window\"); for a task, the action to take (\"Use `declareLazy` in the remaining \
         sessionstore modules\"). The mechanism, the list of files and any counts belong in the \
         description, not in the summary. Leave out what goes stale or reads as code in prose: a \
         count (\"Six modules…\"), a helper's namespace prefix (`declareLazy`, not \
         `XPCOMUtils.declareLazy`), glob shorthand (`lazy.*`). Use ordinary words, never a term \
         coined for the occasion: where the project has no established name for a thing, describe \
         it. When a sibling bug exists, match its shape, so that the two are found together, a \
         trailing clause saying only what sets this one apart. (4) The description: plain text, \
         each code identifier (a function, variable, class, file, pref or flag name) in backticks, \
         so that it renders as code in the filed bug. For a test failure, include a link to the \
         test's dashboard: `https://tests.firefox.dev/test.html?test=PATH`. {web}"
    )
}

/// What an ask drafting a bug ends with, `end` saying how it gives its
/// answer: its reply to each comment, and the bug's fields.
pub fn answer_said(end: &str) -> String {
    format!(
        "\n{end}: {{\"replies\": {{label: your reply to that comment: the bug you drafted, its \
         product and component and its summary, in short}}, \"bug\": {{\"product\": the product, \
         \"component\": the component, \"bug_type\": \"defect\", \"enhancement\" or \"task\", \
         \"short_desc\": the summary, \"comment\": the description; and only when the bug needs \
         them, \"blocked\": the bugs it blocks, \"dependson\": the bugs it depends on, \"see_also\": \
         links to see also, each a string, several comma-separated}}}}, labels as above ([c1] is \
         \"c1\")."
    )
}

/// The JSON Schema of what an ask drafting a bug gives besides its
/// replies: `bug`, its fields, each a string, the type one of `TYPES`.
pub fn schema() -> (&'static str, Value) {
    let mut props = Map::new();
    for k in ["product", "component", "short_desc", "comment"].into_iter().chain(MORE) {
        props.insert(k.into(), serde_json::json!({"type": "string"}));
    }
    props.insert("bug_type".into(), serde_json::json!({"enum": TYPES}));
    (
        "bug",
        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": ["product", "component", "bug_type", "short_desc", "comment"],
            "additionalProperties": false
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Draft {
        Draft {
            product: "Core".into(),
            component: "Networking: HTTP".into(),
            bug_type: "defect".into(),
            short_desc: "Tabs & windows lose their `title`".into(),
            comment: "Steps:\n1. Open `about:blank` — then é\n2. 100% done?".into(),
            more: vec![("blocked".into(), "123, 456".into())],
        }
    }

    #[test]
    fn the_form_s_link_holds_each_field_percent_encoded_on_the_settings_bugzilla() {
        let url = form_url("https://bugzilla.mozilla.org", &draft());
        assert_eq!(
            url,
            "https://bugzilla.mozilla.org/enter_bug.cgi?product=Core&component=Networking%3A%20HTTP&bug_type=defect\
             &short_desc=Tabs%20%26%20windows%20lose%20their%20%60title%60\
             &comment=Steps%3A%0A1.%20Open%20%60about%3Ablank%60%20%E2%80%94%20then%20%C3%A9%0A2.%20100%25%20done%3F\
             &blocked=123%2C%20456"
        );
        // A slash at the end of the settings' Bugzilla or not, the same.
        assert_eq!(form_url("https://bugzilla.mozilla.org/", &draft()), url);
        let other = form_url("https://bugzilla-dev.example.com/", &draft());
        assert!(other.starts_with("https://bugzilla-dev.example.com/enter_bug.cgi?product=Core&"), "{other}");
        // What it leaves empty is not in it; the link opens as one.
        let bare = Draft { bug_type: String::new(), comment: String::new(), more: vec![], ..draft() };
        assert_eq!(form_url("https://b.example", &bare), "https://b.example/enter_bug.cgi?product=Core&component=Networking%3A%20HTTP&short_desc=Tabs%20%26%20windows%20lose%20their%20%60title%60");
        assert!(crate::refs::link(&url).is_ok());
        assert_eq!(encoded("a-b_c.d~e/f+g=h"), "a-b_c.d~e%2Ff%2Bg%3Dh");
    }

    #[test]
    fn the_draft_is_read_from_the_answer_s_bug_whatever_its_shape() {
        let answer = |v: Value| v.as_object().cloned().unwrap();
        let a = answer(serde_json::json!({"bug": {
            "product": "", "component": "Core :: DOM: Events", "bug_type": "Task",
            "short_desc": "  Use `declareLazy`\nin the rest ", "comment": {"text": "Why.", "refs": []},
            "blocked": 7, "dependson": [8, "9"], "see_also": "", "whatever": "x"
        }}));
        let d = draft_of(Some(&a)).unwrap();
        assert_eq!((d.product.as_str(), d.component.as_str(), d.bug_type.as_str()), ("Core", "DOM: Events", "task"));
        assert_eq!((d.short_desc.as_str(), d.comment.as_str()), ("Use `declareLazy` in the rest", "Why."));
        assert_eq!(d.more, [("blocked".to_string(), "7".to_string()), ("dependson".to_string(), "8, 9".to_string())]);
        assert_eq!(d.said(), "Core :: DOM: Events, “Use `declareLazy` in the rest”");
        // A type Bugzilla has not: left for it to set.
        let a = answer(serde_json::json!({"bug": {"product": "Firefox", "component": "Tabbed Browser", "bug_type": "bug", "short_desc": "S"}}));
        assert_eq!(draft_of(Some(&a)).unwrap().bug_type, "");
        // No summary, no component, no bug: no draft.
        for v in [serde_json::json!({"bug": {"product": "Core", "component": "DOM", "short_desc": " "}}), serde_json::json!({"bug": {"product": "Core", "short_desc": "S"}}), serde_json::json!({"replies": {}})] {
            assert_eq!(draft_of(Some(&answer(v.clone()))), None, "{v}");
        }
        assert_eq!(draft_of(None), None);
    }

    #[test]
    fn a_bug_s_number_is_read_typed_or_pasted_as_a_link() {
        for (typed, n) in [
            ("2086657", Some(2086657)),
            (" #2086657 ", Some(2086657)),
            ("Bug 2086657", Some(2086657)),
            ("https://bugzilla.mozilla.org/show_bug.cgi?id=2086657", Some(2086657)),
            ("https://bugzilla.mozilla.org/show_bug.cgi?format=default&id=2086657#c3", Some(2086657)),
            ("https://bugzil.la/2086657", Some(2086657)),
            ("https://bugzilla.mozilla.org/", None),
            ("https://bugzilla.mozilla.org/show_bug.cgi?id=abc", None),
            ("0", None),
            ("", None),
            ("12a", None),
            ("D123", None),
        ] {
            assert_eq!(number_in(typed), n, "{typed:?}");
        }
        assert_eq!(titled("shout two", 7), "Bug 7 - shout two");
    }

    #[test]
    fn the_word_bug_is_found_in_any_case_as_a_word_of_its_own_or_its_plural() {
        for text in [
            "bug",
            "Bug: the title is lost",
            "THIS IS A BUG",
            "File a follow-up bug for it.",
            "as bug 123 says",
            "see bug123",
            "(bug)",
            "these are bugs.",
            "Bugs, both of them",
            "line one\nand a bug on line two",
        ] {
            assert!(mentioned(text), "{text:?}");
        }
        for text in ["", "debug it", "bugzilla is down", "humbug", "a bug_fix", "bugsy", "bugs2", "b ug", "the build"] {
            assert!(!mentioned(text), "{text:?}");
        }
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn the_bug_a_title_names_is_read_as_moz_phab_reads_it() {
        assert_eq!(named_in("Bug 1234567 - shout two").as_deref(), Some("1234567"));
        assert_eq!(named_in("shout two"), None);
        assert!(names_none("shout two") && !names_none("Bug 7 - shout two"));
    }
}
