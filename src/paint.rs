//! Painting on a frame's buffer: the colours, named for what they show and
//! set in the settings' `[colors]`, text clipped to an area, and the boxes
//! menus and editors are drawn in.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Each colour there is, by what it paints: its name in the settings'
/// `[colors]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Added,
    Removed,
    AddedWord,
    RemovedWord,
    Hunk,
    File,
    Meta,
    Header,
    You,
    Agent,
    Reviewer,
    Note,
    Bar,
    Warn,
    Status,
    Cursor,
    Picked,
    Code,
}

impl Paint {
    pub const ALL: [Paint; 18] = [
        Paint::Added,
        Paint::Removed,
        Paint::AddedWord,
        Paint::RemovedWord,
        Paint::Hunk,
        Paint::File,
        Paint::Meta,
        Paint::Header,
        Paint::You,
        Paint::Agent,
        Paint::Reviewer,
        Paint::Note,
        Paint::Bar,
        Paint::Warn,
        Paint::Status,
        Paint::Cursor,
        Paint::Picked,
        Paint::Code,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Paint::Added => "added",
            Paint::Removed => "removed",
            Paint::AddedWord => "added_word",
            Paint::RemovedWord => "removed_word",
            Paint::Hunk => "hunk",
            Paint::File => "file",
            Paint::Meta => "meta",
            Paint::Header => "header",
            Paint::You => "you",
            Paint::Agent => "agent",
            Paint::Reviewer => "reviewer",
            Paint::Note => "note",
            Paint::Bar => "bar",
            Paint::Warn => "warn",
            Paint::Status => "status",
            Paint::Cursor => "cursor",
            Paint::Picked => "picked",
            Paint::Code => "code",
        }
    }

    pub fn named(name: &str) -> Option<Paint> {
        Paint::ALL.into_iter().find(|p| p.name() == name)
    }

    /// What it paints, said.
    pub fn what(self) -> &'static str {
        match self {
            Paint::Added => "The lines a patch adds.",
            Paint::Removed => "The lines a patch removes.",
            Paint::AddedWord => "In a line added in place of one removed, the two almost the same: what it changes.",
            Paint::RemovedWord => "In a line removed, another added in its place, the two almost the same: what changes.",
            Paint::Hunk => "A hunk's @@ line.",
            Paint::File => "A file's name in a patch, the titles of boxes, and the records of the history.",
            Paint::Meta => "A patch's index lines, the borders of the panes and the menus, and the editor's keys.",
            Paint::Header => "The first row of each pane.",
            Paint::You => "Your comments and turns: the bar left of what you said, and your name.",
            Paint::Agent => "The agent's comments and turns, the same way.",
            Paint::Reviewer => "A reviewer's comments and turns, the same way.",
            Paint::Note => "What a menu says under its items, the tour's box, the comment editor's frame, and a run going in Runs….",
            Paint::Bar => "The top bar.",
            Paint::Warn => "The top bar when the stack cannot change.",
            Paint::Status => "The bottom line: what was just done, or the keys to press.",
            Paint::Cursor => "The cursor, the same on every line whatever the line's own colours: on a line of the middle, a pane's pick, a menu's item. What changed in a line almost the same as its pair keeps its own colours under it.",
            Paint::Picked => "What a pane has picked while the cursor is in another: its patch, its comment, the review's part; in the cursor's own pane, the pick is drawn as the cursor.",
            Paint::Code => "Code in what a comment says, as its markdown has it: `code` in a line, and code blocks.",
        }
    }

    /// What it is when the settings do not say, as git writes a colour.
    pub fn default_spec(self) -> &'static str {
        match self {
            Paint::Added => "green",
            Paint::Removed => "red",
            Paint::AddedWord => "green 22",
            Paint::RemovedWord => "red 52",
            Paint::Hunk => "magenta",
            Paint::File => "cyan bold",
            Paint::Meta => "blue",
            Paint::Header => "cyan bold",
            Paint::You => "172",
            Paint::Agent => "134",
            Paint::Reviewer => "32",
            Paint::Note => "yellow",
            Paint::Bar => "brightwhite blue",
            Paint::Warn => "black yellow",
            Paint::Status => "black brightwhite",
            Paint::Cursor => "reverse",
            Paint::Picked => "normal 238",
            Paint::Code => "252 236",
        }
    }
}

/// A colour as git writes one (`color.diff.new = "green bold"`): a
/// foreground, then a background, each a name (`black`, `red`, `green`,
/// `yellow`, `blue`, `magenta`, `cyan`, `white`, any of these after
/// `bright`, `default` for the terminal's own, `normal` for none), a number
/// from 0 to 255, or `#rrggbb` (`#rgb`); and anywhere, attributes: `bold`,
/// `dim`, `italic`, `ul`, `blink`, `reverse`, `strike`.
pub fn parse_color(spec: &str) -> Result<Style, String> {
    let mut st = Style::new();
    let mut colours = 0;
    for word in spec.split_whitespace() {
        let w = word.to_ascii_lowercase();
        if let Some(m) = attribute(&w) {
            st = st.add_modifier(m);
            continue;
        }
        let Some(c) = colour(&w) else {
            return Err(format!(
                "{word:?} is not a colour (a name such as green or brightblue, a number from 0 to 255, #rrggbb) nor an attribute (bold, dim, italic, ul, blink, reverse, strike)"
            ));
        };
        match (colours, c) {
            (0, Some(c)) => st = st.fg(c),
            (1, Some(c)) => st = st.bg(c),
            (0 | 1, None) => {}
            _ => return Err(format!("{spec:?} has more than two colours: a foreground, then a background")),
        }
        colours += 1;
    }
    Ok(st)
}

fn attribute(w: &str) -> Option<Modifier> {
    Some(match w {
        "bold" => Modifier::BOLD,
        "dim" => Modifier::DIM,
        "italic" => Modifier::ITALIC,
        "ul" | "underline" => Modifier::UNDERLINED,
        "blink" => Modifier::SLOW_BLINK,
        "reverse" => Modifier::REVERSED,
        "strike" => Modifier::CROSSED_OUT,
        _ => return None,
    })
}

/// The colour `w` names: Some(None) for `normal`, which changes none.
fn colour(w: &str) -> Option<Option<Color>> {
    const BASIC: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    const ANSI: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    let basic = |n: &str| BASIC.iter().position(|b| *b == n);
    if w == "normal" {
        return Some(None);
    }
    if w == "default" {
        return Some(Some(Color::Reset));
    }
    if let Some(i) = basic(w) {
        return Some(Some(ANSI[i]));
    }
    if let Some(i) = w.strip_prefix("bright").and_then(basic) {
        return Some(Some(ANSI[i + 8]));
    }
    if let Ok(n) = w.parse::<u8>() {
        return Some(Some(Color::Indexed(n)));
    }
    let hex = w.strip_prefix('#')?;
    let digit = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n)?, 16).ok();
    let (r, g, b) = match hex.len() {
        6 => (digit(0, 2)?, digit(2, 2)?, digit(4, 2)?),
        3 => (digit(0, 1)? * 17, digit(1, 1)? * 17, digit(2, 1)? * 17),
        _ => return None,
    };
    Some(Some(Color::Rgb(r, g, b)))
}

pub mod color {
    use super::*;

    /// What `p` is as the settings say.
    fn of(p: Paint) -> Style {
        crate::config::get().colors.style(p)
    }

    pub fn add() -> Style {
        of(Paint::Added)
    }
    pub fn del() -> Style {
        of(Paint::Removed)
    }
    /// What a line added changes, against the line removed in its place.
    pub fn add_word() -> Style {
        of(Paint::AddedWord)
    }
    /// What changes of a line removed, against the line added in its place.
    pub fn del_word() -> Style {
        of(Paint::RemovedWord)
    }
    pub fn hunk() -> Style {
        of(Paint::Hunk)
    }
    pub fn file() -> Style {
        of(Paint::File)
    }
    /// Your comments and turns: their bar, your name.
    pub fn you() -> Style {
        of(Paint::You)
    }
    pub fn agent() -> Style {
        of(Paint::Agent)
    }
    pub fn reviewer() -> Style {
        of(Paint::Reviewer)
    }
    /// What a menu says under its items, the tour, the comment editor's
    /// frame, a run going.
    pub fn note() -> Style {
        of(Paint::Note)
    }
    pub fn meta() -> Style {
        of(Paint::Meta)
    }
    pub fn bar() -> Style {
        of(Paint::Bar)
    }
    /// The bar, when the stack cannot be changed or does not apply.
    pub fn warn() -> Style {
        of(Paint::Warn)
    }
    pub fn status() -> Style {
        of(Paint::Status)
    }
    /// The cursor, whatever the colours of the line it is on: see
    /// `on_cursor`.
    pub fn cursor() -> Style {
        of(Paint::Cursor)
    }
    /// What a pane has picked, while the cursor is in another.
    pub fn picked() -> Style {
        of(Paint::Picked)
    }
    /// Code in a comment's markdown.
    pub fn code() -> Style {
        of(Paint::Code)
    }
    pub fn dim() -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }
    /// A pane's header row.
    pub fn header() -> Style {
        of(Paint::Header)
    }
}

/// Text in `st` under the cursor, as the settings' `cursor` paints it.
pub fn on_cursor(st: Style) -> Style {
    under(color::cursor(), st)
}

/// Text in `st` under a cursor painted `cursor`: in the cursor's colours,
/// the terminal's own where it names none, whatever `st`'s were, and not
/// dim, so that the cursor looks the same on every line; bold, italic,
/// underlined or struck through as `st` is.
fn under(cursor: Style, st: Style) -> Style {
    let kept = st.add_modifier - Modifier::DIM - Modifier::REVERSED;
    Style::new()
        .fg(cursor.fg.unwrap_or(Color::Reset))
        .bg(cursor.bg.unwrap_or(Color::Reset))
        .remove_modifier(Modifier::DIM | Modifier::REVERSED)
        .add_modifier(kept | cursor.add_modifier)
}

/// Writes `text` at `(x, y)` of `area`, cut at `w` and the area's edge.
pub fn put(buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, text: &str, st: Style) {
    if y >= area.height as usize || x >= area.width as usize {
        return;
    }
    let w = w.min(area.width as usize - x);
    buf.set_stringn(area.x + x as u16, area.y + y as u16, text, w, st);
}

/// `parts` written one after another from `(x, y)` of `area`, each in its
/// style, cut at `w`; padded to it in the last one's.
pub fn put_parts(buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, parts: &[(String, Style)]) {
    let mut at = 0;
    for (k, (text, st)) in parts.iter().enumerate() {
        let room = w.saturating_sub(at);
        let text = if k + 1 == parts.len() { fit(text, room) } else { text.chars().take(room).collect() };
        put(buf, area, x + at, y, room, &text, *st);
        at += text.chars().count();
    }
}

/// `text` cut to `w` characters and padded to them.
pub fn fit(text: &str, w: usize) -> String {
    let mut s: String = text.chars().take(w).collect();
    let n = s.chars().count();
    s.extend(std::iter::repeat_n(' ', w - n));
    s
}

/// `text` folded into lines of at most `w` characters, at spaces.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > w {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// A cleared, bordered box of `w` by `h` at `(x, y)` of `area`, `title` on
/// its top edge.
#[allow(clippy::too_many_arguments)]
pub fn window(buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, h: usize, title: &str, st: Style) {
    let w = w.min((area.width as usize).saturating_sub(x));
    let h = h.min((area.height as usize).saturating_sub(y));
    if w < 6 || h < 2 {
        return;
    }
    // Cleared for real, so nothing under it shows through.
    for row in y..y + h {
        for col in x..x + w {
            buf[(area.x + col as u16, area.y + row as u16)].reset();
        }
    }
    let horiz = "─".repeat(w - 2);
    put(buf, area, x, y, w, &format!("┌{horiz}┐"), st);
    put(buf, area, x, y + h - 1, w, &format!("└{horiz}┘"), st);
    for row in y + 1..y + h - 1 {
        put(buf, area, x, row, 1, "│", st);
        put(buf, area, x + w - 1, row, 1, "│", st);
    }
    if !title.is_empty() {
        let title: String = format!(" {title} ").chars().take(w - 4).collect();
        put(buf, area, x + 2, y, w - 4, &title, color::file());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A colour as shown: the terminal's own foreground or background, or
    /// one of its own.
    #[derive(Debug, PartialEq)]
    enum Shown {
        Fg,
        Bg,
        Is(Color),
    }

    /// How text in `s` looks: its colours as shown, the terminal's own
    /// where it names none, swapped when reversed; and its other
    /// attributes.
    fn looks(s: Style) -> (Shown, Shown, Modifier) {
        let shown = |c: Option<Color>, own: Shown| match c {
            None | Some(Color::Reset) => own,
            Some(c) => Shown::Is(c),
        };
        let (fg, bg) = (shown(s.fg, Shown::Fg), shown(s.bg, Shown::Bg));
        match s.add_modifier.contains(Modifier::REVERSED) {
            true => (bg, fg, s.add_modifier - Modifier::REVERSED),
            false => (fg, bg, s.add_modifier),
        }
    }

    #[test]
    fn a_word_changed_stands_out_of_its_line_under_the_cursor_too() {
        // Under the cursor, a line is in the cursor's colours whatever its
        // own (`under`), but what changed in it keeps its own (`row_parts`):
        // a word in the cursor's colours would vanish into it. Whatever the
        // colours for words become, they must look otherwise than their
        // line's, and than the cursor's.
        let cursor = parse_color(Paint::Cursor.default_spec()).unwrap();
        for (word, line) in [(Paint::AddedWord, Paint::Added), (Paint::RemovedWord, Paint::Removed)] {
            let (w, l) = (parse_color(word.default_spec()).unwrap(), parse_color(line.default_spec()).unwrap());
            assert_ne!(looks(w), looks(l), "{}: as {} looks", word.name(), line.name());
            assert_ne!(looks(w), looks(under(cursor, l)), "{}: as the cursor looks on {}", word.name(), line.name());
        }
    }

    #[test]
    fn the_cursor_looks_the_same_on_every_line() {
        // As it is when the settings do not say: the terminal's own colours
        // reversed, whatever the line's own, dim or reversed as they may be.
        let cursor = parse_color(Paint::Cursor.default_spec()).unwrap();
        for p in Paint::ALL {
            let st = parse_color(p.default_spec()).unwrap();
            for st in [st, st.add_modifier(Modifier::DIM), st.add_modifier(Modifier::REVERSED)] {
                let (fg, bg, m) = looks(under(cursor, st));
                assert_eq!((fg, bg), (Shown::Bg, Shown::Fg), "{}: {st:?}", p.name());
                assert!(!m.contains(Modifier::DIM), "{}: {st:?}", p.name());
            }
        }
        // Bold, italic, underlined as the line is.
        let line = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED | Modifier::DIM);
        assert_eq!(looks(under(cursor, line)), (Shown::Bg, Shown::Fg, Modifier::BOLD | Modifier::UNDERLINED));
        // A cursor of colours of its own: in them, on any line.
        let yellow = parse_color("black yellow").unwrap();
        let line = Style::new().fg(Color::Green).bg(Color::Indexed(22)).add_modifier(Modifier::REVERSED);
        assert_eq!(looks(under(yellow, line)), (Shown::Is(Color::Black), Shown::Is(Color::Yellow), Modifier::empty()));
    }

    #[test]
    fn a_colour_is_read_as_git_writes_one() {
        let fg = |c: Color| Style::new().fg(c);
        assert_eq!(parse_color("green"), Ok(fg(Color::Green)));
        assert_eq!(parse_color("white"), Ok(fg(Color::Gray)), "git's white is ANSI's 7");
        assert_eq!(parse_color("brightwhite"), Ok(fg(Color::White)));
        assert_eq!(parse_color("brightblue"), Ok(fg(Color::LightBlue)));
        assert_eq!(parse_color("172"), Ok(fg(Color::Indexed(172))));
        assert_eq!(parse_color("#d78700"), Ok(fg(Color::Rgb(0xd7, 0x87, 0))));
        assert_eq!(parse_color("#f1b"), Ok(fg(Color::Rgb(0xff, 0x11, 0xbb))));
        // A foreground, then a background; attributes anywhere.
        assert_eq!(parse_color("bold cyan"), Ok(fg(Color::Cyan).add_modifier(Modifier::BOLD)));
        assert_eq!(
            parse_color("black yellow ul"),
            Ok(fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::UNDERLINED))
        );
        assert_eq!(parse_color("normal blue"), Ok(Style::new().bg(Color::Blue)), "normal changes nothing");
        assert_eq!(parse_color("default"), Ok(fg(Color::Reset)));
        assert_eq!(parse_color(""), Ok(Style::new()));
        assert!(parse_color("purpleish").unwrap_err().contains("\"purpleish\" is not a colour"));
        assert!(parse_color("red green blue").unwrap_err().contains("more than two colours"));
        assert!(parse_color("256").is_err() && parse_color("#12345").is_err());
        // Every default is one, and each colour is found by its name.
        for p in Paint::ALL {
            assert!(parse_color(p.default_spec()).is_ok(), "{}", p.name());
            assert_eq!(Paint::named(p.name()), Some(p));
        }
    }
}
