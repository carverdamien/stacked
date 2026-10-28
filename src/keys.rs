//! The editor's keys, as the settings map them: each of the few things a
//! key does, and the keys that do it. A key pressed is read as the one key
//! the editor knows for that (↑ for `up`, space for `menu`), so the rest of
//! it knows only those; a key no action has does nothing. The inline
//! comment editor reads its keys as they are: they are what is typed.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    /// What is to be done where the cursor is.
    Enter,
    /// All that can be done there: the menu.
    Menu,
    /// What can be done there, said.
    Help,
    /// Back: out of a menu, a list, a stream.
    Back,
    /// The next view of the code: unified, side by side, compact.
    View,
    /// Reading rather than commenting, or back.
    Read,
    /// Each file whole rather than its hunks, or back.
    Whole,
    Quit,
}

impl Key {
    pub const ALL: [Key; 16] = [
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::PageUp,
        Key::PageDown,
        Key::Home,
        Key::End,
        Key::Enter,
        Key::Menu,
        Key::Help,
        Key::Back,
        Key::View,
        Key::Read,
        Key::Whole,
        Key::Quit,
    ];

    /// Its name in the settings.
    pub fn name(self) -> &'static str {
        match self {
            Key::Up => "up",
            Key::Down => "down",
            Key::Left => "left",
            Key::Right => "right",
            Key::PageUp => "page_up",
            Key::PageDown => "page_down",
            Key::Home => "home",
            Key::End => "end",
            Key::Enter => "enter",
            Key::Menu => "menu",
            Key::Help => "help",
            Key::Back => "back",
            Key::View => "view",
            Key::Read => "read",
            Key::Whole => "whole",
            Key::Quit => "quit",
        }
    }

    /// The keys that do it unless the settings say otherwise.
    pub fn default_keys(self) -> &'static [&'static str] {
        match self {
            Key::Up => &["up"],
            Key::Down => &["down"],
            Key::Left => &["left"],
            Key::Right => &["right"],
            Key::PageUp => &["pageup"],
            Key::PageDown => &["pagedown"],
            Key::Home => &["home"],
            Key::End => &["end"],
            Key::Enter => &["enter"],
            Key::Menu => &["space"],
            Key::Help => &["?"],
            Key::Back => &["esc"],
            Key::View => &["1"],
            Key::Read => &["2"],
            Key::Whole => &["3"],
            Key::Quit => &["ctrl-c"],
        }
    }

    /// The key the editor knows for it.
    pub fn event(self) -> KeyEvent {
        let (code, mods) = match self {
            Key::Up => (KeyCode::Up, KeyModifiers::NONE),
            Key::Down => (KeyCode::Down, KeyModifiers::NONE),
            Key::Left => (KeyCode::Left, KeyModifiers::NONE),
            Key::Right => (KeyCode::Right, KeyModifiers::NONE),
            Key::PageUp => (KeyCode::PageUp, KeyModifiers::NONE),
            Key::PageDown => (KeyCode::PageDown, KeyModifiers::NONE),
            Key::Home => (KeyCode::Home, KeyModifiers::NONE),
            Key::End => (KeyCode::End, KeyModifiers::NONE),
            Key::Enter => (KeyCode::Enter, KeyModifiers::NONE),
            Key::Menu => (KeyCode::Char(' '), KeyModifiers::NONE),
            Key::Help => (KeyCode::Char('?'), KeyModifiers::NONE),
            Key::Back => (KeyCode::Esc, KeyModifiers::NONE),
            Key::View => (KeyCode::Char('1'), KeyModifiers::NONE),
            Key::Read => (KeyCode::Char('2'), KeyModifiers::NONE),
            Key::Whole => (KeyCode::Char('3'), KeyModifiers::NONE),
            Key::Quit => (KeyCode::Char('c'), KeyModifiers::CONTROL),
        };
        KeyEvent::new(code, mods)
    }
}

/// A key as the settings name it: `up`, `pageup`, `enter`, `space`, `esc`,
/// `tab`, `f1`, a character (`?`, `k`), any of them after `ctrl-` or
/// `alt-`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
}

impl Chord {
    pub fn parse(name: &str) -> Result<Chord, String> {
        let mut rest = name;
        let (mut ctrl, mut alt) = (false, false);
        loop {
            let lower = rest.to_ascii_lowercase();
            if let Some(r) = ["ctrl-", "c-", "control-"].iter().find_map(|p| lower.strip_prefix(p).map(|_| p.len())) {
                ctrl = true;
                rest = &rest[r..];
            } else if let Some(r) = ["alt-", "a-", "m-", "meta-"].iter().find_map(|p| lower.strip_prefix(p).map(|_| p.len())) {
                alt = true;
                rest = &rest[r..];
            } else {
                break;
            }
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "up" | "↑" => KeyCode::Up,
            "down" | "↓" => KeyCode::Down,
            "left" | "←" => KeyCode::Left,
            "right" | "→" => KeyCode::Right,
            "pageup" | "page-up" | "page_up" | "pgup" => KeyCode::PageUp,
            "pagedown" | "page-down" | "page_down" | "pgdn" | "pgdown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "enter" | "return" | "ret" => KeyCode::Enter,
            "space" | "spc" => KeyCode::Char(' '),
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" | "shift-tab" => KeyCode::BackTab,
            "backspace" | "bs" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" | "ins" => KeyCode::Insert,
            f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
                KeyCode::F(f[1..].parse().unwrap())
            }
            _ => {
                let mut chars = rest.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(if ctrl { c.to_ascii_lowercase() } else { c }),
                    _ => return Err(format!("{name:?} is not a key")),
                }
            }
        };
        Ok(Chord { code, ctrl, alt })
    }

    /// Whether `k` is this key. Shift is in the character (`K`, `?`).
    pub fn matches(&self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let code = match k.code {
            KeyCode::Char(c) if ctrl => KeyCode::Char(c.to_ascii_lowercase()),
            c => c,
        };
        code == self.code && ctrl == self.ctrl && alt == self.alt
    }

    /// As the help says it.
    pub fn label(&self) -> String {
        let key = match self.code {
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::PageUp => "pgup".into(),
            KeyCode::PageDown => "pgdn".into(),
            KeyCode::Home => "home".into(),
            KeyCode::End => "end".into(),
            KeyCode::Enter => "Enter".into(),
            KeyCode::Char(' ') => "Space".into(),
            KeyCode::Esc => "Esc".into(),
            KeyCode::Tab => "Tab".into(),
            KeyCode::BackTab => "shift-Tab".into(),
            KeyCode::Backspace => "Backspace".into(),
            KeyCode::Delete => "Delete".into(),
            KeyCode::Insert => "Insert".into(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Char(c) => c.to_string(),
            _ => "?".into(),
        };
        let mut out = String::new();
        if self.ctrl {
            out.push_str("ctrl-");
        }
        if self.alt {
            out.push_str("alt-");
        }
        out + &key
    }
}

/// Each action's keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    keys: Vec<(Key, Vec<Chord>)>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap {
            keys: Key::ALL
                .iter()
                .map(|&k| (k, k.default_keys().iter().map(|n| Chord::parse(n).unwrap()).collect()))
                .collect(),
        }
    }
}

impl Keymap {
    /// The keymap of `named` (an action's name to its keys), the defaults
    /// for the actions it does not name; and what it got wrong.
    pub fn from_names(named: &[(String, Vec<String>)]) -> (Keymap, Vec<String>) {
        Keymap::default().with_names(named)
    }

    /// This keymap with `named` over it, the actions it does not name as
    /// they are; and what it got wrong: a key for two things, if `named`
    /// gives it to one of them.
    pub fn with_names(&self, named: &[(String, Vec<String>)]) -> (Keymap, Vec<String>) {
        let mut map = self.clone();
        let mut wrong = Vec::new();
        for (action, names) in named {
            let Some(key) = Key::ALL.iter().find(|k| k.name() == action) else {
                wrong.push(format!("keys: there is no {action:?}"));
                continue;
            };
            let mut chords = Vec::new();
            for n in names {
                match Chord::parse(n) {
                    Ok(c) => chords.push(c),
                    Err(e) => wrong.push(format!("keys.{action}: {e}")),
                }
            }
            if chords.is_empty() {
                wrong.push(format!("keys.{action}: no key left for it; it keeps its own"));
                continue;
            }
            if let Some(slot) = map.keys.iter_mut().find(|(k, _)| k == key) {
                slot.1 = chords;
            }
        }
        // A key for two things does the first.
        let named_here = |k: &Key| named.iter().any(|(action, _)| action == k.name());
        let mut seen: Vec<(Chord, Key)> = Vec::new();
        for (key, chords) in &map.keys {
            for c in chords {
                if let Some((_, other)) = seen.iter().find(|(s, _)| s == c) {
                    if named_here(key) || named_here(other) {
                        wrong.push(format!("keys: {} is {} and {}; it does the first", c.label(), other.name(), key.name()));
                    }
                } else {
                    seen.push((*c, *key));
                }
            }
        }
        (map, wrong)
    }

    /// What `k` does, if anything.
    pub fn action(&self, k: &KeyEvent) -> Option<Key> {
        if k.kind == KeyEventKind::Release {
            return None;
        }
        self.keys
            .iter()
            .find(|(_, chords)| chords.iter().any(|c| c.matches(k)))
            .map(|(key, _)| *key)
    }

    /// `k` as the key the editor knows for what it does.
    pub fn translate(&self, k: &KeyEvent) -> Option<KeyEvent> {
        self.action(k).map(Key::event)
    }

    /// The first key that does `key`, as the help says it.
    pub fn label(&self, key: Key) -> String {
        self.keys
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, c)| c.first())
            .map_or_else(|| "?".into(), Chord::label)
    }

    /// The names of `key`'s keys, for the settings.
    pub fn names(&self, key: Key) -> Vec<String> {
        self.keys
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, cs)| cs.iter().map(chord_name).collect())
            .unwrap_or_default()
    }
}

/// A key's name, as the settings write it.
fn chord_name(c: &Chord) -> String {
    let key = match c.code {
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "backtab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Insert => "insert".into(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Char(ch) => ch.to_string(),
        _ => "?".into(),
    };
    format!("{}{}{key}", if c.ctrl { "ctrl-" } else { "" }, if c.alt { "alt-" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn names_are_read_and_said() {
        assert_eq!(Chord::parse("ctrl-c").unwrap(), Chord { code: KeyCode::Char('c'), ctrl: true, alt: false });
        assert_eq!(Chord::parse("C-x").unwrap().code, KeyCode::Char('x'));
        assert_eq!(Chord::parse("PgDn").unwrap().code, KeyCode::PageDown);
        assert_eq!(Chord::parse("f5").unwrap().code, KeyCode::F(5));
        assert_eq!(Chord::parse("K").unwrap().code, KeyCode::Char('K'));
        assert!(Chord::parse("nope").is_err());
        assert_eq!(Chord::parse("space").unwrap().label(), "Space");
        assert_eq!(Chord::parse("alt-j").unwrap().label(), "alt-j");
    }

    #[test]
    fn a_keymap_reads_its_keys_as_the_editors() {
        let named = vec![
            ("up".to_string(), vec!["up".to_string(), "k".to_string()]),
            ("menu".to_string(), vec!["m".to_string()]),
            ("quit".to_string(), vec!["q".to_string(), "ctrl-c".to_string()]),
            ("whole".to_string(), vec!["w".to_string()]),
            ("fly".to_string(), vec!["x".to_string()]),
        ];
        let (map, wrong) = Keymap::from_names(&named);
        assert_eq!(wrong, ["keys: there is no \"fly\""]);
        let k = |c| map.translate(&ev(KeyCode::Char(c), KeyModifiers::NONE));
        assert_eq!(k('k').map(|e| e.code), Some(KeyCode::Up));
        assert_eq!(k('m').map(|e| e.code), Some(KeyCode::Char(' ')));
        assert_eq!(k(' '), None, "space no longer is the menu");
        // Each file whole is 3 unless the settings say otherwise.
        assert_eq!((k('w'), k('3')), (Some(Key::Whole.event()), None));
        assert_eq!(Keymap::default().label(Key::Whole), "3");
        assert_eq!(k('q'), Some(Key::Quit.event()));
        assert_eq!(map.translate(&ev(KeyCode::Up, KeyModifiers::NONE)).map(|e| e.code), Some(KeyCode::Up));
        // Shift is in the character.
        assert_eq!(map.translate(&ev(KeyCode::Char('?'), KeyModifiers::SHIFT)).map(|e| e.code), Some(KeyCode::Char('?')));
        assert_eq!(map.label(Key::Menu), "m");
        // The same key twice: the first does it, and it is said.
        let (_, wrong) = Keymap::from_names(&[("down".into(), vec!["up".into()])]);
        assert_eq!(wrong, ["keys: ↑ is up and down; it does the first"]);
    }
}
