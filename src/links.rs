//! The links a line of a commit message or of a comment's text has, each to
//! open in your browser (`refs::open_link`): every web link written in it,
//! `https://…` or `http://…`, as markdown writes one too (`[its
//! text](https://…)`, named by its text, and `<https://…>`); and, with the
//! moz-phab feature, each bug it names (`Bug 1234567`, `b=123`), `bug` a
//! word of its own and the rest as moz-phab reads a bug in a title, on the
//! settings' Bugzilla (`reviews.bugzilla`), and the revision its trailer
//! names (`Differential Revision:`), at the trailer's link or, a bare
//! `D123`, on the settings' Phabricator, as `phab::revision_of` reads it. A
//! bug written in a web link is the link's, not one of its own, as is one in
//! a markdown link's text; a link is there once, the revision's link not
//! again as a web link.

use crate::config::Reviews;

/// What a link of a line is. At the same place in the line, the first comes
/// first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum What {
    /// The revision the line's trailer names.
    Revision,
    /// A bug the line names.
    Bug,
    /// A web link, as it is written.
    Web,
    /// The web link of a markdown link, `[its text](https://…)`, named by
    /// its text.
    Named,
}

/// A link of a line: what it is, what the menu names it, and the web link it
/// opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub what: What,
    /// `bug 1234567`, `D123`, the web link as it is written, or a markdown
    /// link's text, its markup left out.
    pub said: String,
    pub url: String,
}

impl Link {
    /// What a sentence says of it: `bug 1234567`, `D123`, the web link as it
    /// is written, or a markdown link's text in quotes, `"the docs"`.
    pub fn name(&self) -> String {
        match self.what {
            What::Named => format!("\"{}\"", self.said),
            _ => self.said.clone(),
        }
    }
}

/// A word character, as Python's `re` reads `\w` in a pattern of text.
pub(crate) fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The links of `line`, a line of a commit message or of a comment's text,
/// in the order they come in it, each once: its web links; and, with the
/// moz-phab feature, the revision its trailer names and the bugs it names,
/// `reviews` saying where those are. Only those `refs::link` takes, which
/// open.
pub fn of_line(line: &str, reviews: &Reviews) -> Vec<Link> {
    let chars: Vec<char> = line.chars().collect();
    let web = web_links(&chars);
    let mut found: Vec<(usize, Link)> = web.iter().map(|(at, _, link)| (*at, link.clone())).collect();
    #[cfg(feature = "moz-phab")]
    {
        // The trailer's revision, where its value starts.
        if let Some((said, url)) = crate::phab::revision_in(line, &reviews.trailer, reviews.url.as_deref()) {
            found.push((reviews.trailer.chars().count() + 1, Link { what: What::Revision, said, url }));
        }
        let bugzilla = reviews.bugzilla.trim_end_matches('/');
        for (at, _, n) in bugs_named(&chars) {
            if web.iter().any(|(from, to, _)| (*from..*to).contains(&at)) {
                continue;
            }
            let url = format!("{bugzilla}/show_bug.cgi?id={n}");
            found.push((at, Link { what: What::Bug, said: format!("bug {n}"), url }));
        }
    }
    #[cfg(not(feature = "moz-phab"))]
    let _ = reviews;
    found.sort_by_key(|(at, l)| (*at, l.what));
    let mut out: Vec<Link> = Vec::new();
    for (_, l) in found {
        if crate::refs::link(&l.url).is_ok() && !out.iter().any(|o| o.url == l.url) {
            out.push(l);
        }
    }
    out
}

/// Whether `s` starts with `prefix`, an ASCII one, in any case.
fn starts_lower(s: &[char], prefix: &str) -> bool {
    s.len() >= prefix.len() && s.iter().zip(prefix.chars()).all(|(c, p)| c.to_ascii_lowercase() == p)
}

/// The web links written in `s`, a line's characters, as they are written:
/// each markdown link to one (`markdown_link`) and each `<https://…>`
/// (`angle_link`); and each other `https://` or `http://`, in any case, that
/// no word runs on into from before it, up to the white space after it or
/// what a link as written cannot hold (`<`, `>`, `"`, `` ` ``), less what
/// then ends it that ends a sentence or what holds the link (`.`, `,`, `;`,
/// `:`, `!`, `?`, `'`, `*`, `_`, `~`; a `)`, `]` or `}` that no `(`, `[` or
/// `{` of the link opens); each one naming a site (`refs::link`). Where each
/// starts and ends, a markdown link's text and brackets with it, and the
/// link.
fn web_links(s: &[char]) -> Vec<(usize, usize, Link)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if let Some((end, link)) = markdown_link(s, i).or_else(|| angle_link(s, i)) {
            out.push((i, end, link));
            i = end;
            continue;
        }
        let scheme = ["https://", "http://"].into_iter().find(|p| starts_lower(&s[i..], p));
        let Some(scheme) = scheme.filter(|_| i == 0 || !word(s[i - 1])) else {
            i += 1;
            continue;
        };
        let mut end = i + scheme.len();
        while end < s.len() && !s[end].is_whitespace() && !matches!(s[end], '<' | '>' | '"' | '`') {
            end += 1;
        }
        // The scheme ends with `/`, which is never taken off.
        while let Some(&last) = s[..end].last() {
            let held = &s[i..end];
            let unopened = |open: char, close: char| last == close && held.iter().filter(|&&c| c == open).count() < held.iter().filter(|&&c| c == close).count();
            let ends = matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_' | '~');
            if !(ends || unopened('(', ')') || unopened('[', ']') || unopened('{', '}')) {
                break;
            }
            end -= 1;
        }
        let url: String = s[i..end].iter().collect();
        if crate::refs::link(&url).is_ok() {
            out.push((i, end, web(url)));
        }
        i = end;
    }
    out
}

/// Web link `url`, said as it is written.
fn web(url: String) -> Link {
    Link { what: What::Web, said: url.clone(), url }
}

/// The markdown link that starts at `i` of `s`, if one does and goes to a
/// web link: `[its text](https://…)`, its text up to the first `]`, as
/// markdown draws it (`markdown::inline`), its link from the `(` right after
/// that up to the `)` that closes it, a `(` and `)` of the link's own kept
/// in it, or up to a space, a title in quotes after it (`"a title"`). Where
/// it ends, after its `)`; and the link, named by its text, its markup left
/// out: said as it is written when the text is blank or holds a web link,
/// so that a link is never named by another.
fn markdown_link(s: &[char], i: usize) -> Option<(usize, Link)> {
    if s[i] != '[' {
        return None;
    }
    let close = i + 1 + s[i + 1..].iter().position(|&c| c == ']')?;
    if s.get(close + 1) != Some(&'(') {
        return None;
    }
    let from = close + 2;
    let (mut to, mut depth) = (from, 0usize);
    while let Some(&c) = s.get(to).filter(|c| !c.is_whitespace()) {
        match c {
            ')' if depth == 0 => break,
            ')' => depth -= 1,
            '(' => depth += 1,
            _ => {}
        }
        to += 1;
    }
    // Its `)`: right after the link, or after its title.
    let mut end = to;
    while s.get(end).is_some_and(|c| c.is_whitespace()) {
        end += 1;
    }
    if end > to && let Some(&quote) = s.get(end).filter(|&&c| c == '"' || c == '\'') {
        end += 1 + s.get(end + 1..)?.iter().position(|&c| c == quote)? + 1;
        while s.get(end).is_some_and(|c| c.is_whitespace()) {
            end += 1;
        }
    }
    if s.get(end) != Some(&')') {
        return None;
    }
    let url: String = s[from..to].iter().collect();
    crate::refs::link(&url).ok()?;
    let text: String = s[i + 1..close].iter().collect();
    let named: String = crate::markdown::inline(&text).into_iter().filter(|(_, m)| *m != crate::markdown::Mark::Url).map(|(t, _)| t).collect();
    let named = named.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = text.chars().collect();
    let link = match named.is_empty() || !web_links(&chars).is_empty() {
        true => web(url),
        false => Link { what: What::Named, said: named, url },
    };
    Some((end + 1, link))
}

/// The link `<https://…>` that starts at `i` of `s`, if one does: what is
/// between its brackets, as it is written, with no space in it, a web link
/// naming a site. Where it ends, after its `>`; and the link.
fn angle_link(s: &[char], i: usize) -> Option<(usize, Link)> {
    if s[i] != '<' {
        return None;
    }
    let close = i + 1 + s[i + 1..].iter().position(|&c| c == '>' || c == '<' || c.is_whitespace())?;
    if s[close] != '>' || !["https://", "http://"].into_iter().any(|p| starts_lower(&s[i + 1..close], p)) {
        return None;
    }
    let url: String = s[i + 1..close].iter().collect();
    crate::refs::link(&url).ok()?;
    Some((close + 1, web(url)))
}

/// moz-phab's BUG_ID_RE, `(?:(?:bug|b=)(?:\s*)(\d+)(?=\b))` in any case: its
/// matches in `s`, as Python's `findall` finds them, each where it starts and
/// ends, and the bug's number. moz-phab takes the first of a title as the bug
/// a revision is for (`bug_in`).
#[cfg(feature = "moz-phab")]
pub fn bugs_in(s: &[char]) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let low = |k: usize| s.get(i + k).map(char::to_ascii_lowercase);
        let after = if (low(0), low(1), low(2)) == (Some('b'), Some('u'), Some('g')) {
            i + 3
        } else if low(0) == Some('b') && s.get(i + 1) == Some(&'=') {
            i + 2
        } else {
            i += 1;
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
            out.push((i, j, s[digits..j].iter().collect()));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The first match of moz-phab's BUG_ID_RE in `s` (`bugs_in`): where it
/// starts and ends, and the bug's number. The bug moz-phab reads in a title.
#[cfg(feature = "moz-phab")]
pub fn bug_in(s: &[char]) -> Option<(usize, usize, String)> {
    bugs_in(s).into_iter().next()
}

/// The bugs `s`, a line's characters, names, to open: the matches of
/// moz-phab's BUG_ID_RE (`bugs_in`) that start a word, after no letter,
/// digit or `_`, so that `debug 7` names no bug; where each starts and
/// ends, and the bug's number. A title submitted is read as moz-phab reads
/// it still (`bug_in`).
#[cfg(feature = "moz-phab")]
fn bugs_named(s: &[char]) -> Vec<(usize, usize, String)> {
    bugs_in(s).into_iter().filter(|&(at, _, _)| at == 0 || !word(s[at - 1])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reviews() -> Reviews {
        crate::config::Config::default().reviews
    }

    /// The links of `line`, each what it is said, and where it goes.
    fn links(line: &str, reviews: &Reviews) -> Vec<(String, String)> {
        of_line(line, reviews).into_iter().map(|l| (l.said, l.url)).collect()
    }

    fn pair(said: &str, url: &str) -> (String, String) {
        (said.to_string(), url.to_string())
    }

    #[test]
    fn a_web_link_is_found_as_it_is_written_less_what_ends_it() {
        let r = reviews();
        let web = |line: &str| -> Vec<String> { of_line(line, &r).into_iter().filter(|l| matches!(l.what, What::Web | What::Named)).map(|l| l.url).collect() };
        assert_eq!(web("See https://example.com/a/b."), ["https://example.com/a/b"]);
        assert_eq!(web("HTTP://Example.com/A, then http://example.org:8080/x?y=1#z!"), ["HTTP://Example.com/A", "http://example.org:8080/x?y=1#z"]);
        // In what holds it: what it opens is its own; what it closes is not.
        assert_eq!(web("(see https://example.com/a)"), ["https://example.com/a"]);
        assert_eq!(web("the [page](https://example.com/b) and <https://example.com/c>"), ["https://example.com/b", "https://example.com/c"]);
        assert_eq!(web("https://en.wikipedia.org/wiki/Foo_(bar), and \"https://example.com/q\"."), ["https://en.wikipedia.org/wiki/Foo_(bar)", "https://example.com/q"]);
        assert_eq!(web("'https://example.com/it's' and `https://example.com/code`"), ["https://example.com/it's", "https://example.com/code"]);
        // None: no site, another scheme, a word running on into it, a link
        // with no scheme.
        for none in ["https://", "https:///a", "ftp://example.com/a", "xhttps://example.com/a", "example.com/a", "Bug 1 - a title"] {
            assert!(web(none).is_empty(), "{none:?}");
        }
        // The same link twice: once.
        assert_eq!(web("https://example.com/a or https://example.com/a"), ["https://example.com/a"]);
    }

    #[test]
    fn markdown_writes_a_link_named_by_its_text_or_whole_between_angle_brackets() {
        let r = reviews();
        let found = |line: &str| -> Vec<(What, String, String)> { of_line(line, &r).into_iter().map(|l| (l.what, l.said, l.url)).collect() };
        let named = |said: &str, url: &str| (What::Named, said.to_string(), url.to_string());
        let web = |url: &str| (What::Web, url.to_string(), url.to_string());
        // Named by its text, its markup left out; the sentence's end and
        // what holds it not of it.
        assert_eq!(found("Read [the docs](https://example.com/a)."), [named("the docs", "https://example.com/a")]);
        assert_eq!(found("(see [**the**  `docs`](https://example.com/a))"), [named("the docs", "https://example.com/a")]);
        // Its link up to the `)` that closes it, its own brackets kept; two
        // with nothing between them, two; a title after it, not of it.
        assert_eq!(found("[Foo](https://en.wikipedia.org/wiki/Foo_(bar))."), [named("Foo", "https://en.wikipedia.org/wiki/Foo_(bar)")]);
        assert_eq!(found("[a](https://example.com/a),[b](https://example.com/b)"), [named("a", "https://example.com/a"), named("b", "https://example.com/b")]);
        assert_eq!(found("[a](https://example.com/a \"The title\") then"), [named("a", "https://example.com/a")]);
        assert_eq!(found("![a picture](https://example.com/p.png)"), [named("a picture", "https://example.com/p.png")]);
        // Its text a link, or holding one: said as the link it goes to,
        // never named by another.
        assert_eq!(found("[https://example.com/a](https://example.com/a)"), [web("https://example.com/a")]);
        assert_eq!(found("[see https://example.com/x](https://example.com/y)"), [web("https://example.com/y")]);
        // To no web link, or no markdown link: what is written as a link is.
        assert!(found("[a](#here) and [b](b.html)").is_empty());
        assert_eq!(found("[a] (https://example.com/a)"), [web("https://example.com/a")]);
        assert_eq!(found("[a](https://example.com/a and more"), [web("https://example.com/a")]);
        // Between angle brackets, whole; with a space in it, no such link.
        assert_eq!(found("<https://example.com/a.>, <HTTP://example.com/b>"), [web("https://example.com/a."), web("HTTP://example.com/b")]);
        assert_eq!(found("<https://example.com/a b>"), [web("https://example.com/a")]);
        assert!(found("<https://> <ftp://example.com/a>").is_empty());
        // The same link named and bare: once, the first.
        assert_eq!(found("[a](https://example.com/a) or https://example.com/a"), [named("a", "https://example.com/a")]);
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_bug_is_read_as_moz_phab_reads_one_each_on_the_settings_bugzilla() {
        let r = reviews();
        let bug = |n: &str| pair(&format!("bug {n}"), &format!("https://bugzilla.mozilla.org/show_bug.cgi?id={n}"));
        assert_eq!(links("Bug 1234567 - Fix the crash. r=alice", &r), [bug("1234567")]);
        // In any case, spaced or not, `b=` too; each of them, once.
        assert_eq!(links("bug 12, BUG13, b=14, and Bug  12 again", &r), [bug("12"), bug("13"), bug("14")]);
        // As moz-phab reads it: no word after the number.
        assert!(links("bug 12a", &r).is_empty());
        assert!(links("a bug, and b=c", &r).is_empty());
        // The settings' Bugzilla, its last slash or not.
        let mut other = reviews();
        other.bugzilla = "https://bugzilla.example.com/b/".into();
        assert_eq!(links("Bug 5 - x", &other), [pair("bug 5", "https://bugzilla.example.com/b/show_bug.cgi?id=5")]);
        // A bug in a web link is the link's; the link and a bug after it,
        // each in its place.
        assert_eq!(
            links("Bug 9: see https://example.com/bug10?b=11, then bug 12.", &r),
            [bug("9"), pair("https://example.com/bug10?b=11", "https://example.com/bug10?b=11"), bug("12")]
        );
        // A bug in a markdown link's text, or between angle brackets, is the
        // link's too.
        assert_eq!(
            links("[Bug 6](https://example.com/b), <https://example.com/bug7>, bug 8", &r),
            [pair("Bug 6", "https://example.com/b"), pair("https://example.com/bug7", "https://example.com/bug7"), bug("8")]
        );
        // The first is the one moz-phab takes for a title.
        let title: Vec<char> = "WIP: bug 3 and Bug 4".chars().collect();
        assert_eq!(bug_in(&title), Some((5, 10, "3".to_string())));
        assert_eq!(bugs_in(&title).len(), 2);
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_bug_to_open_is_bug_as_a_word_of_its_own_a_title_submitted_read_as_moz_phab_reads_it() {
        let r = reviews();
        let bug = |n: &str| pair(&format!("bug {n}"), &format!("https://bugzilla.mozilla.org/show_bug.cgi?id={n}"));
        // After a letter, a digit or `_`, no bug: not in "debug 7", "ab=8",
        // "x_bug 9", "2bug 10".
        assert!(links("debug 7, ab=8, x_bug 9 and 2bug 10", &r).is_empty());
        // After anything else, or first, one; the number right after it too.
        assert_eq!(links("bug 1 (bug 7), bug8, -b=9: Bug 10.", &r), [bug("1"), bug("7"), bug("8"), bug("9"), bug("10")]);
        // A title submitted is read as moz-phab reads it, its bug 7 there.
        let title: Vec<char> = "WIP: debug 7".chars().collect();
        assert_eq!(bug_in(&title).map(|(_, _, n)| n).as_deref(), Some("7"));
        assert_eq!(crate::submit::wip_title("Fix the debug 7 view").bug.as_deref(), Some("7"));
        assert!(links("Fix the debug 7 view", &r).is_empty());
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn the_trailer_names_its_revision_at_its_link_or_a_bare_one_on_the_settings_phabricator() {
        let mut r = reviews();
        let link = "https://phabricator.services.mozilla.com/D123";
        // Its link once, said as the revision it is.
        assert_eq!(links(&format!("Differential Revision: {link}"), &r), [pair("D123", link)]);
        // Bare: on the settings' Phabricator, and none without one.
        assert!(links("Differential Revision: D45", &r).is_empty());
        r.url = Some("https://phab.example.com".into());
        assert_eq!(links("Differential Revision: D45", &r), [pair("D45", "https://phab.example.com/D45")]);
        // The settings' trailer; another line naming a revision is no trailer.
        r.trailer = "Revision".into();
        assert_eq!(links(&format!("Revision: {link}"), &r), [pair("D123", link)]);
        assert_eq!(links(&format!("Differential Revision: {link}"), &r), [pair(link, link)]);
        assert!(links("This backs out D45.", &r).is_empty());
    }

    #[cfg(not(feature = "moz-phab"))]
    #[test]
    fn without_moz_phab_only_web_links_are_links() {
        let r = reviews();
        let link = "https://phabricator.services.mozilla.com/D123";
        assert!(links("Bug 1234567 - Fix the crash, b=12", &r).is_empty());
        assert_eq!(links(&format!("Differential Revision: {link}"), &r), [pair(link, link)]);
        assert!(links("Differential Revision: D45", &r).is_empty());
    }
}
