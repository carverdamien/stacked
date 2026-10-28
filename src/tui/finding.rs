//! The search of the middle, as self-review's `/`: what is typed goes in
//! the bottom bar, edited as the comment editor edits a line, and as it is
//! typed the cursor goes to the first match from where it was, the middle
//! scrolled to it, the matches in sight lit. Tab goes on to the next match
//! and shift-Tab back to the one before (`find_next` and `find_previous`,
//! the keys of the search's context), past either end round to the other;
//! Enter ends the search on the match, Esc where the cursor was. A
//! line matches when it holds what is typed, whatever its case, as
//! self-review's search and the search's own screen match.
//!
//! It reads the rows the middle is drawn from, as the view of the code
//! built them: the hunks or the whole files, whichever the middle shows. A
//! line of code is read without its `+`, `-` or space, and a line folded
//! as the one line it is. The searches are kept between runs (`state`), as
//! self-review keeps them: `/` opens with the last one, selected, so that
//! what is typed takes its place, and ↑↓ go through those before it.

use super::*;
use crate::keys::{Context, Key};

/// The search of the middle, open in the bottom bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Find {
    /// What is typed: a line of the comment editor's.
    pub ed: Editor,
    /// What is typed is a search before, selected, as `/` opens with the
    /// last one: what is typed next takes its place.
    pub selected: bool,
    /// The search before that ↑↓ took, by its place among those kept; and
    /// what was typed before ↑ took the first, for ↓ to give back.
    at: Option<usize>,
    draft: String,
    /// How many lines hold what is typed, and what the bar says of them;
    /// the rows they were counted in, as `App::built` says them.
    pub matches: usize,
    pub said: String,
    seen: Option<String>,
    /// Where the middle was when `/` was pressed: Esc goes back there.
    back: Back,
}

/// Where the middle was, and the cursor, when `/` was pressed; and what
/// the rows were built from then, with where the cursor's row is among
/// them, to find it again in the rows built anew meanwhile (each file
/// whole, read since; another width; the agent's stream grown).
#[derive(Debug, Clone, PartialEq)]
struct Back {
    cursor: usize,
    top: usize,
    compact: Compact,
    focus: Focus,
    on_banner: bool,
    built: Option<String>,
    place: crate::rows::Place,
}

impl Find {
    /// What is searched for: what is typed, without the blanks at its ends.
    pub fn query(&self) -> String {
        self.ed.line().0.trim().to_string()
    }

    /// What is searched for, as the search compares it.
    fn folded(&self) -> Vec<char> {
        self.query().chars().map(folded).collect()
    }
}

/// A character as the search compares it: whatever its case.
fn folded(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Where `q` is in `text`, both as the search compares them: where each
/// match starts, from the left, each after the one before it.
fn starts(text: &[char], q: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while !q.is_empty() && i + q.len() <= text.len() {
        if text[i..i + q.len()] == *q {
            out.push(i);
            i += q.len();
        } else {
            i += 1;
        }
    }
    out
}

/// A line of the middle as the search reads it: its characters, as the
/// search compares them, and where each is drawn, its row and its column
/// there; none for a blank that folding the line left out.
#[derive(Default)]
struct Read {
    text: Vec<char>,
    at: Vec<Option<(usize, usize)>>,
}

/// A line of a patch's message as it is written, `written`, read on its
/// pieces' characters (`drawn`, each where it is drawn): the characters of
/// each in turn, a blank of one as any blank of the other; the indent a
/// piece starts with passed over, and a blank the folding left out drawn
/// nowhere.
fn aligned(written: &str, drawn: &[((usize, usize), char)]) -> Read {
    let mut out = Read::default();
    let mut j = 0;
    for c in written.chars() {
        if !c.is_whitespace() {
            while drawn.get(j).is_some_and(|d| d.1.is_whitespace()) {
                j += 1;
            }
        }
        let here = drawn.get(j).filter(|d| d.1 == c || (c.is_whitespace() && d.1.is_whitespace()));
        out.at.push(here.map(|d| d.0));
        j += usize::from(here.is_some());
        out.text.push(folded(c));
    }
    out
}

impl App {
    /// `/`: the search of the middle, opened in the bottom bar; the cursor
    /// in the middle, whatever pane it was in. The last search is in it,
    /// selected, and searched at once.
    pub(super) fn open_find(&mut self) {
        let back = Back {
            cursor: self.cursor,
            top: self.top,
            compact: self.compact.clone(),
            focus: self.focus,
            on_banner: self.on_banner,
            built: self.built.clone(),
            place: crate::rows::place(&self.rows, self.cursor),
        };
        self.focus = Focus::Diff;
        self.on_banner = false;
        let last = self.finds.first().cloned().unwrap_or_default();
        let mut f = Find {
            ed: Editor::new(&last).single(),
            selected: !last.is_empty(),
            at: (!last.is_empty()).then_some(0),
            draft: String::new(),
            matches: 0,
            said: String::new(),
            seen: None,
            back,
        };
        self.find_typed(&mut f);
        self.mode = Mode::Find(Box::new(f));
    }

    /// A key while the search of the middle is open: Enter ends the search
    /// on the match, Esc where the cursor was, ↑↓ go through the searches
    /// before, whatever the settings map; the search's keys of the settings
    /// (Tab and shift-Tab) go from match to match; any other edits what is
    /// typed, searched again.
    pub(super) fn find_key(&mut self, mut f: Find, k: KeyEvent) -> Effect {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let step = self.cfg.keys.action_in(Context::Search, &k);
        match k.code {
            KeyCode::Esc => {
                self.find_back(&f.back);
                self.status = "the search is cancelled: the cursor is back where it was".into();
                return Effect::None;
            }
            // On the match: kept for the next `/`. With none, back where
            // the cursor was, as Esc.
            KeyCode::Enter => {
                let q = f.query();
                if !q.is_empty() && !self.find_lines(&f.folded()).is_empty() {
                    self.remember_find(&q);
                } else {
                    self.find_back(&f.back);
                    if !q.is_empty() {
                        self.status = format!("nothing matches \"{q}\": the cursor is back where it was");
                    }
                }
                return Effect::None;
            }
            KeyCode::Up => self.find_recall(&mut f, true),
            KeyCode::Down => self.find_recall(&mut f, false),
            _ if step == Some(Key::FindNext) => self.find_step(&mut f, true),
            _ if step == Some(Key::FindPrevious) => self.find_step(&mut f, false),
            _ => {
                let was = f.ed.line();
                // A search before, selected: what is typed, or put back
                // from what ctrl-k cut, takes its place, and Backspace or
                // Delete takes it away. A key that moves in it leaves it
                // there, no longer selected.
                let replaces = match k.code {
                    KeyCode::Char('h' | 'y') if ctrl => true,
                    KeyCode::Char(_) => !ctrl && !alt,
                    KeyCode::Backspace | KeyCode::Delete => true,
                    _ => false,
                };
                if f.selected && replaces {
                    f.ed = Editor::new("").single();
                }
                f.ed.key(k);
                let now = f.ed.line();
                if now != was {
                    f.selected = false;
                }
                if now.0 != was.0 {
                    f.at = None;
                    self.find_typed(&mut f);
                }
            }
        }
        self.mode = Mode::Find(Box::new(f));
        Effect::None
    }

    /// Back where the cursor was when `/` was pressed, in the pane it was
    /// in, the middle scrolled as it was.
    fn find_back(&mut self, b: &Back) {
        self.middle_back(b);
        self.focus = b.focus;
        self.on_banner = b.on_banner;
    }

    /// The middle's cursor where it was when `/` was pressed, the middle
    /// scrolled as it was; the rows built anew since, on the row found
    /// again in them, on its line of the screen.
    fn middle_back(&mut self, b: &Back) {
        if self.built == b.built || self.rows.is_empty() {
            self.cursor = b.cursor.min(self.rows.len().saturating_sub(1));
            self.top = b.top;
            self.compact = b.compact.clone();
            return;
        }
        let i = crate::rows::find(&self.rows, &b.place).0;
        self.cursor = i;
        self.top = i.saturating_sub(b.cursor.saturating_sub(b.top));
        // The compact panes go to it once drawn.
        self.compact = b.compact.clone();
        self.compact.cursor = None;
    }

    /// The rows built anew while the search is open: its matches counted
    /// again, the cursor on its match still; with none on it, searched
    /// again from where `/` was pressed, as if it were typed now.
    pub(super) fn find_again(&mut self) {
        if !matches!(&self.mode, Mode::Find(f) if f.seen != self.built) {
            return;
        }
        let Mode::Find(mut f) = std::mem::replace(&mut self.mode, Mode::Normal) else { return };
        let q = f.folded();
        let found = if q.is_empty() { Vec::new() } else { self.find_lines(&q) };
        let here = self.line_of(self.cursor).0;
        match found.iter().position(|l| l.0 == here) {
            Some(n) => {
                f.matches = found.len();
                f.said = format!("match {} of {}", n + 1, found.len());
                f.seen = self.built.clone();
            }
            None => self.find_typed(&mut f),
        }
        self.mode = Mode::Find(f);
    }

    /// What is typed, searched from where `/` was pressed: the cursor on
    /// the first match there or after it, past the last on the first; with
    /// none, or nothing typed, where it was.
    fn find_typed(&mut self, f: &mut Find) {
        self.middle_back(&f.back);
        let q = f.folded();
        let found = if q.is_empty() { Vec::new() } else { self.find_lines(&q) };
        f.matches = found.len();
        f.seen = self.built.clone();
        if found.is_empty() {
            f.said = if q.is_empty() { String::new() } else { "nothing matches".into() };
            return;
        }
        let from = self.line_of(self.cursor).0;
        match found.iter().position(|l| l.0 >= from) {
            Some(n) => self.find_on(f, &found, n, None),
            None => self.find_on(f, &found, 0, Some("top")),
        }
    }

    /// Tab, or shift-Tab: the next match, or the one before; past the
    /// last, the first, and before the first, the last.
    fn find_step(&mut self, f: &mut Find, forward: bool) {
        let q = f.folded();
        if q.is_empty() {
            return;
        }
        let found = self.find_lines(&q);
        f.matches = found.len();
        f.seen = self.built.clone();
        if found.is_empty() {
            f.said = "nothing matches".into();
            return;
        }
        let here = self.line_of(self.cursor).0;
        let next = if forward { found.iter().position(|l| l.0 > here) } else { found.iter().rposition(|l| l.0 < here) };
        match (next, forward) {
            (Some(n), _) => self.find_on(f, &found, n, None),
            (None, true) => self.find_on(f, &found, 0, Some("top")),
            (None, false) => self.find_on(f, &found, found.len() - 1, Some("bottom")),
        }
    }

    /// ↑, or ↓: the search kept before what is typed, or the one after it;
    /// past the latest, what was typed before ↑. Selected, and searched.
    fn find_recall(&mut self, f: &mut Find, older: bool) {
        let to = match (f.at, older) {
            (None, true) if !self.finds.is_empty() => {
                f.draft = f.ed.line().0;
                Some(0)
            }
            (Some(i), true) if i + 1 < self.finds.len() => Some(i + 1),
            (Some(0), false) => None,
            (Some(i), false) => Some(i - 1),
            _ => return,
        };
        let text = match to {
            Some(i) => self.finds[i].clone(),
            None => std::mem::take(&mut f.draft),
        };
        f.at = to;
        f.selected = to.is_some();
        f.ed = Editor::new(&text).single();
        self.find_typed(f);
    }

    /// The cursor on match `n` of `found`, the bar saying which; `round`,
    /// the end the search started again from, past the other.
    fn find_on(&mut self, f: &mut Find, found: &[(usize, usize)], n: usize, round: Option<&str>) {
        self.find_go(found[n].1);
        f.said = format!("match {} of {}", n + 1, found.len());
        if let Some(end) = round {
            f.said += &format!(": the search started again at the {end}");
        }
    }

    /// The cursor on row `i`; the middle scrolled to it if it is out of
    /// sight, a third of the way down, as a comment gone to is. The compact
    /// panes go there themselves, once drawn.
    fn find_go(&mut self, i: usize) {
        self.cursor = i;
        // A line less, for the title pinned on the first.
        let (h, third) = (self.mid_h.saturating_sub(1).max(1), self.mid_h / 3);
        match self.drawn_layout(self.mid_w) {
            // A half of the middle split in two is unified too.
            Layout::Unified | Layout::Halves => {
                if i < self.top || i >= self.top + h {
                    self.top = i.saturating_sub(third);
                }
            }
            Layout::Split => {
                let (lines, index) = split_lines(&self.rows);
                let line = |row: usize| index.get(&row).copied().unwrap_or(0);
                let (at, top) = (line(i), line(self.top));
                if at < top || at >= top + h {
                    self.top = lines.get(at.saturating_sub(third)).map_or(0, split_row);
                }
            }
            Layout::Compact => {}
        }
    }

    /// The first and the last row of the line row `i` is a piece of.
    fn line_of(&self, i: usize) -> (usize, usize) {
        let mut first = i;
        while first > 0 && self.rows.get(first).is_some_and(|r| r.cont) {
            first -= 1;
        }
        let mut last = first;
        while self.rows.get(last + 1).is_some_and(|r| r.cont) {
            last += 1;
        }
        (first, last)
    }

    /// The line of rows `first` to `last` as the search reads it: a line
    /// of code without its `+`, `-` or space, nor its pieces' `↪`; a line
    /// of the message folded between words, as it is written; any other as
    /// it is drawn.
    fn line_read(&self, first: usize, last: usize) -> Read {
        // Each character drawn, where it is drawn; a blank between the
        // pieces of a line of the message, drawn nowhere.
        let mut cells: Vec<(Option<(usize, usize)>, char)> = Vec::new();
        for (i, r) in (first..).zip(&self.rows[first..=last]) {
            let skip = match r.kind {
                Kind::Add | Kind::Del | Kind::Ctx => 1,
                Kind::Msg if i > first => {
                    cells.push((None, ' '));
                    r.text.chars().take_while(|c| *c == ' ').count()
                }
                _ => 0,
            };
            cells.extend(r.text.chars().enumerate().skip(skip).map(|(col, c)| (Some((i, col)), c)));
        }
        let written = match self.rows[first].msg_at {
            Some(k) if last > first && self.rows[first].kind == Kind::Msg => self.message_line(k),
            _ => None,
        };
        match written {
            Some(w) => {
                let drawn: Vec<((usize, usize), char)> = cells.into_iter().filter_map(|(at, c)| Some((at?, c))).collect();
                aligned(&w, &drawn)
            }
            None => Read {
                text: cells.iter().map(|c| folded(c.1)).collect(),
                at: cells.into_iter().map(|c| c.0).collect(),
            },
        }
    }

    /// Line `k` of the message of the patch the middle shows, as written.
    fn message_line(&self, k: usize) -> Option<String> {
        let id = self.patch_id().filter(|_| !self.on_stack)?;
        self.db.patch(&id).message.split('\n').nth(k).map(str::to_string)
    }

    /// The lines of the middle that hold `q`, as the search compares it:
    /// for each, its first row, and the row its first match starts on.
    fn find_lines(&self, q: &[char]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut first = 0;
        while first < self.rows.len() {
            let (_, last) = self.line_of(first);
            let read = self.line_read(first, last);
            if let Some(&s) = starts(&read.text, q).first() {
                let on = read.at[s..].iter().flatten().next().map_or(first, |at| at.0);
                out.push((first, on));
            }
            first = last + 1;
        }
        out
    }

    /// The matches of the search on row `i`, drawn from `(x, y)` of
    /// `area`, `w` wide: reversed from how the row is drawn; on the line
    /// the cursor is on, the match gone to, bold and underlined too.
    pub(super) fn paint_found(&self, buf: &mut Buffer, area: Rect, (x, y, w): (usize, usize, usize), i: usize) {
        let Mode::Find(f) = &self.mode else { return };
        let q = f.folded();
        if q.is_empty() || i >= self.rows.len() || y >= area.height as usize {
            return;
        }
        let (first, last) = self.line_of(i);
        let read = self.line_read(first, last);
        let on = (first..=last).contains(&self.cursor);
        for s in starts(&read.text, &q) {
            for &(row, col) in read.at[s..s + q.len()].iter().flatten() {
                if row != i || col >= w || x + col >= area.width as usize {
                    continue;
                }
                let cell = &mut buf[(area.x + (x + col) as u16, area.y + y as u16)];
                cell.modifier.toggle(Modifier::REVERSED);
                if on {
                    cell.modifier.insert(Modifier::BOLD | Modifier::UNDERLINED);
                }
            }
        }
    }

    /// The search's line, over the bottom bar: `/` and what is typed (a
    /// search before, selected, reversed), what it found, and its keys.
    /// Where the cursor is: in what is typed.
    pub(super) fn draw_find(&self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let Mode::Find(f) = &self.mode else { return None };
        let (w, y) = (area.width as usize, (area.height as usize).checked_sub(1)?);
        let (text, col) = f.ed.line();
        let said = if f.said.is_empty() { String::new() } else { format!("   {}", f.said) };
        put(buf, area, 0, y, w, &fit(&format!(" /{text}{said}   {}", self.hint()), w), color::status());
        if f.selected {
            put(buf, area, 2, y, w.saturating_sub(2), &text, color::status().add_modifier(Modifier::REVERSED));
        }
        // A click on it is on nothing; out of it, Esc.
        spot_in(&self.spots, area, (0, y, w, 1), Spot::Box);
        let x = (2 + col).min(w.saturating_sub(1));
        Some((area.x + x as u16, area.y + y as u16))
    }

    /// The search's keys: those going from match to match as the settings
    /// have them, the others as they are, whatever the settings map.
    pub(super) fn find_hint(&self, f: &Find) -> String {
        let before = if self.finds.is_empty() { "" } else { " · ↑↓: the searches before" };
        let key = |k: Key| self.cfg.keys.label(k).to_lowercase();
        match (f.query().is_empty(), f.matches) {
            (true, _) => format!("type what to look for{before} · esc: back"),
            (false, 0) => format!("enter or esc: back where you were{before}"),
            _ => format!(
                "{}: the next match · {}: the one before · enter: stay there · esc: back where you were{before}",
                key(Key::FindNext),
                key(Key::FindPrevious)
            ),
        }
    }

    /// What `/` does, said with its key, and the keys going from match to
    /// match, as the settings have them; the search's others as they are,
    /// whatever the settings map.
    pub(super) fn find_help(&self) -> String {
        let key = |k: Key| self.cfg.keys.label(k);
        format!(
            "Press {} to search what the middle shows: as you type, the cursor goes to the first match from where it is, and the matches in sight are lit. In the search, {} goes to the next match and {} to the one before, Enter stays on the match, Esc goes back where you were, and ↑↓ go through the searches before.",
            key(Key::Find),
            key(Key::FindNext),
            key(Key::FindPrevious)
        )
    }

    /// `q` kept for the next `/`, first among the searches and once: in
    /// this run, and in the next (`state`), but for the tour's database,
    /// which keeps nothing on disk.
    fn remember_find(&mut self, q: &str) {
        self.finds.retain(|s| s != q);
        self.finds.insert(0, q.to_string());
        self.finds.truncate(crate::state::SEARCHES);
        if !self.in_memory() {
            crate::state::searched(&crate::home::get().state, q);
        }
    }

    /// The searches kept from the runs before, for `/`: none for the tour.
    pub(super) fn kept_finds(&self) -> Vec<String> {
        if self.in_memory() { Vec::new() } else { crate::state::searches(&crate::home::get().state) }
    }

    /// Whether the database is the tour's, kept in memory.
    pub(super) fn in_memory(&self) -> bool {
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path) {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{aligned, folded, starts};

    fn chars(s: &str) -> Vec<char> {
        s.chars().map(folded).collect()
    }

    #[test]
    fn a_match_is_found_whatever_its_case_each_after_the_one_before() {
        assert_eq!(starts(&chars("Two, TWO and two"), &chars("two")), [0, 5, 13]);
        assert_eq!(starts(&chars("aaaa"), &chars("aa")), [0, 2]);
        assert!(starts(&chars("two"), &chars("three")).is_empty());
        assert!(starts(&chars("two"), &[]).is_empty());
    }

    #[test]
    fn a_line_of_the_message_folded_is_read_as_written() {
        // "   A summary typed long" folded as "   A summary" and "   typed
        // long": the blank between them drawn nowhere, the indents passed
        // over.
        let pieces = ["   A summary", "   typed long"];
        let mut drawn = Vec::new();
        for (row, p) in pieces.iter().enumerate() {
            let skip = if row == 0 { 0 } else { 3 };
            drawn.extend(p.chars().enumerate().skip(skip).map(|(col, c)| ((row, col), c)));
        }
        let read = aligned("A summary typed long", &drawn);
        assert_eq!(read.text.iter().collect::<String>(), "a summary typed long");
        let at = |k: usize| read.at[k];
        assert_eq!((at(0), at(8), at(9), at(10)), (Some((0, 3)), Some((0, 11)), None, Some((1, 3))));
        // A word broken where the line ends: no blank between its halves.
        let drawn: Vec<((usize, usize), char)> = "abc".chars().enumerate().map(|(c, ch)| ((0, c), ch)).chain("def".chars().enumerate().map(|(c, ch)| ((1, c), ch))).collect();
        let read = aligned("abcdef", &drawn);
        assert!(read.at.iter().all(Option::is_some));
        assert_eq!(read.at[3], Some((1, 0)));
    }
}
