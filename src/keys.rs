//! The editor's keys, as the settings map them: each of the few things a
//! key does, and the keys that do it, each in the context it is read in.
//! On the screens and the menus over them, wherever nothing is typed, a key
//! pressed is read as the one key the editor knows for that (↑ for `up`,
//! space for `menu`), so the rest of it knows only those; a key no action
//! has does nothing. In the search of the middle, what is typed is typed,
//! but for the search's own keys: those going from match to match, which
//! the settings map, and Esc, Enter, ↑ and ↓, which they do not. Two things
//! of one context on one key clash; of two contexts, one key can do both:
//! each is read where it is, and where both are, the search's own first.
//! The inline comment editor reads its keys as they are: they are what is
//! typed.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Where a key is read, as the settings map it: what the editor's
/// dispatch of keys tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    /// The screens and the menus over them, wherever nothing is typed.
    Screens,
    /// The search of the middle, while its bar is open.
    Search,
}

impl Context {
    pub const ALL: [Context; 2] = [Context::Screens, Context::Search];

    /// Its name, as `stacked config` says it.
    pub fn name(self) -> &'static str {
        match self {
            Context::Screens => "screens",
            Context::Search => "search",
        }
    }

    /// Where it is, said.
    pub fn said(self) -> &'static str {
        match self {
            Context::Screens => "on the screens and the menus over them, wherever nothing is typed",
            Context::Search => {
                "in the search of the middle, while its bar is open, which keeps for itself Esc, Enter, ↑, ↓, ctrl-c and the keys that type or edit what is typed"
            }
        }
    }
}

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
    /// What the menu closes, drops, stops or rejects where the cursor is:
    /// the item's own action, asking first where the item does.
    Delete,
    /// The search of what the middle shows.
    Find,
    /// In the search, the next match, and the one before.
    FindNext,
    FindPrevious,
    /// The menu of the views of the code, to pick one: unified, side by
    /// side, compact, unified split in two; on a screen of interdiffs, the
    /// menu of their views, and of the change's.
    View,
    /// Reading rather than commenting, or back.
    Read,
    /// Each file whole rather than its hunks, or back: of the patch the
    /// middle shows, and, on a screen of interdiffs, of the change drawn.
    Whole,
    /// The link of the line of a commit message the cursor is on, opened in
    /// the browser; of several, the menu of them.
    OpenLink,
    Quit,
}

/// The keys that once were and are no longer, by their names in the
/// settings, each with why: a settings' file that still names one is told
/// so, and the key is passed over.
pub const GONE: [(&str, &str); 2] = [
    ("keep_stack", "the sync of the commit messages it picked in is gone"),
    ("keep_workspace", "the sync of the commit messages it picked in is gone"),
];

impl Key {
    pub const ALL: [Key; 21] = [
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
        Key::Delete,
        Key::Find,
        Key::FindNext,
        Key::FindPrevious,
        Key::View,
        Key::Read,
        Key::Whole,
        Key::OpenLink,
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
            Key::Delete => "delete",
            Key::Find => "find",
            Key::FindNext => "find_next",
            Key::FindPrevious => "find_previous",
            Key::View => "view",
            Key::Read => "read",
            Key::Whole => "whole",
            Key::OpenLink => "open_link",
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
            // A Mac's delete key is Backspace; its forward Delete, fn-delete.
            Key::Delete => &["delete", "backspace"],
            Key::Find => &["/"],
            Key::FindNext => &["tab"],
            Key::FindPrevious => &["shift-tab"],
            Key::View => &["1"],
            Key::Read => &["2"],
            Key::Whole => &["3"],
            Key::OpenLink => &["o"],
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
            Key::Delete => (KeyCode::Delete, KeyModifiers::NONE),
            Key::Find => (KeyCode::Char('/'), KeyModifiers::NONE),
            Key::FindNext => (KeyCode::Tab, KeyModifiers::NONE),
            Key::FindPrevious => (KeyCode::BackTab, KeyModifiers::NONE),
            Key::View => (KeyCode::Char('1'), KeyModifiers::NONE),
            Key::Read => (KeyCode::Char('2'), KeyModifiers::NONE),
            Key::Whole => (KeyCode::Char('3'), KeyModifiers::NONE),
            Key::OpenLink => (KeyCode::Char('o'), KeyModifiers::NONE),
            Key::Quit => (KeyCode::Char('c'), KeyModifiers::CONTROL),
        };
        KeyEvent::new(code, mods)
    }

    /// Where it is read.
    pub fn context(self) -> Context {
        match self {
            Key::FindNext | Key::FindPrevious => Context::Search,
            _ => Context::Screens,
        }
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

    /// Whether the search of the middle keeps this key for itself, whatever
    /// the settings: Esc, Enter, ↑ and ↓, which mean there what they mean in
    /// every line typed; ctrl-c, which quits; and the keys that type, or
    /// edit what is typed, as the comment editor's line does.
    pub fn kept_by_search(&self) -> bool {
        match self.code {
            KeyCode::Char(c) => match (self.ctrl, self.alt) {
                (false, false) => true,
                (true, _) => "acehky".contains(c.to_ascii_lowercase()),
                (false, true) => "bf".contains(c.to_ascii_lowercase()),
            },
            KeyCode::Esc | KeyCode::Enter | KeyCode::Up | KeyCode::Down => true,
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End | KeyCode::Backspace | KeyCode::Delete => true,
            _ => false,
        }
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

/// Why the settings no longer take `keys.NAME`, a key that is gone, as
/// `why` says.
pub fn gone(name: &str, why: &str) -> String {
    format!("keys.{name} is no longer supported: {why}")
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
                match GONE.iter().find(|(name, _)| name == action) {
                    Some((_, why)) => wrong.push(format!("{}; it is passed over", gone(action, why))),
                    None => wrong.push(format!("keys: there is no {action:?}")),
                }
                continue;
            };
            let mut chords = Vec::new();
            for n in names {
                match Chord::parse(n) {
                    Ok(c) if key.context() == Context::Search && c.kept_by_search() => {
                        wrong.push(format!("keys.{action}: the search of the middle keeps {} for itself", c.label()))
                    }
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
        // A key for two things of one context does the first. Of two
        // contexts, each is read where it is: they never meet.
        let named_here = |k: &Key| named.iter().any(|(action, _)| action == k.name());
        let mut seen: Vec<(Chord, Key)> = Vec::new();
        for (key, chords) in &map.keys {
            for c in chords {
                if let Some((_, other)) = seen.iter().find(|(s, other)| s == c && other.context() == key.context()) {
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

    /// What `k` does on the screens, if anything.
    pub fn action(&self, k: &KeyEvent) -> Option<Key> {
        self.action_in(Context::Screens, k)
    }

    /// What `k` does in `context`, if anything.
    pub fn action_in(&self, context: Context, k: &KeyEvent) -> Option<Key> {
        if k.kind == KeyEventKind::Release {
            return None;
        }
        self.keys
            .iter()
            .filter(|(key, _)| key.context() == context)
            .find(|(_, chords)| chords.iter().any(|c| c.matches(k)))
            .map(|(key, _)| *key)
    }

    /// `k` as the key the editor knows for what it does on the screens.
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
        KeyCode::BackTab => "shift-tab".into(),
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

    #[test]
    fn delete_is_delete_and_backspace_unless_the_settings_say_otherwise() {
        // A Mac's delete key sends Backspace, its fn-delete Delete: either
        // is the editor's Delete.
        let map = Keymap::default();
        for code in [KeyCode::Delete, KeyCode::Backspace] {
            assert_eq!(map.translate(&ev(code, KeyModifiers::NONE)), Some(Key::Delete.event()), "{code:?}");
        }
        assert_eq!((map.label(Key::Delete), map.names(Key::Delete)), ("Delete".to_string(), vec!["delete".to_string(), "backspace".to_string()]));
        assert_eq!(Key::Delete.context(), Context::Screens);
        // Another key, and neither of those any more.
        let (map, wrong) = Keymap::from_names(&[("delete".into(), vec!["x".into()])]);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(map.translate(&ev(KeyCode::Char('x'), KeyModifiers::NONE)), Some(Key::Delete.event()));
        assert_eq!(map.translate(&ev(KeyCode::Backspace, KeyModifiers::NONE)), None);
        assert_eq!(map.label(Key::Delete), "x");
    }

    #[test]
    fn a_link_opens_with_o_unless_the_settings_say_otherwise_and_clashes_on_the_screens_alone() {
        // o, read on the screens.
        let map = Keymap::default();
        let o = ev(KeyCode::Char('o'), KeyModifiers::NONE);
        assert_eq!((map.translate(&o), Key::OpenLink.context()), (Some(Key::OpenLink.event()), Context::Screens));
        assert_eq!((map.label(Key::OpenLink), map.names(Key::OpenLink)), ("o".to_string(), vec!["o".to_string()]));
        // Another key, and o no longer.
        let (map, wrong) = Keymap::from_names(&[("open_link".into(), vec!["b".into()])]);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(map.translate(&ev(KeyCode::Char('b'), KeyModifiers::NONE)), Some(Key::OpenLink.event()));
        assert_eq!(map.translate(&o), None);
        // A key of the screens already: said, and the first does it.
        let (map, wrong) = Keymap::from_names(&[("open_link".into(), vec!["space".into()])]);
        assert_eq!(wrong, ["keys: Space is menu and open_link; it does the first"]);
        assert_eq!(map.translate(&ev(KeyCode::Char(' '), KeyModifiers::NONE)), Some(Key::Menu.event()));
    }

    #[test]
    fn each_key_is_read_in_its_context_and_clashes_only_there() {
        // The defaults: no two things of one context on one key.
        let (map, wrong) = Keymap::from_names(&[]);
        assert!(wrong.is_empty(), "{wrong:?}");
        for context in Context::ALL {
            let mut seen: Vec<Chord> = Vec::new();
            for (_, chords) in map.keys.iter().filter(|(k, _)| k.context() == context) {
                for c in chords {
                    assert!(!seen.contains(c), "{} twice in {}", c.label(), context.name());
                    seen.push(*c);
                }
            }
        }
        // The search's own: Tab and shift-Tab, read there, and nothing on
        // the screens.
        assert_eq!((map.label(Key::FindNext), map.label(Key::FindPrevious)), ("Tab".to_string(), "shift-Tab".to_string()));
        assert_eq!(map.names(Key::FindPrevious), ["shift-tab"]);
        assert_eq!(Chord::parse("backtab").unwrap(), Chord::parse("Shift-Tab").unwrap());
        let tab = ev(KeyCode::Tab, KeyModifiers::NONE);
        let back = ev(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!((map.action_in(Context::Search, &tab), map.action_in(Context::Search, &back)), (Some(Key::FindNext), Some(Key::FindPrevious)));
        assert_eq!(map.translate(&tab), None, "Tab is nothing on the screens");
        // One key in two contexts: each does its own there, and no clash is
        // said.
        let (map, wrong) = Keymap::from_names(&[("menu".into(), vec!["tab".into()])]);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(map.translate(&tab).map(|e| e.code), Some(KeyCode::Char(' ')));
        assert_eq!(map.action_in(Context::Search, &tab), Some(Key::FindNext));
        let (map, wrong) = Keymap::from_names(&[("find_next".into(), vec!["pagedown".into()])]);
        assert!(wrong.is_empty(), "{wrong:?}");
        let page = ev(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!((map.action(&page), map.action_in(Context::Search, &page)), (Some(Key::PageDown), Some(Key::FindNext)));
        // Two things of one context on one key: said, and the first does it.
        let (map, wrong) = Keymap::from_names(&[("find_next".into(), vec!["ctrl-n".into()]), ("find_previous".into(), vec!["ctrl-n".into()])]);
        assert_eq!(wrong, ["keys: ctrl-n is find_next and find_previous; it does the first"]);
        assert_eq!(map.action_in(Context::Search, &ev(KeyCode::Char('n'), KeyModifiers::CONTROL)), Some(Key::FindNext));
        // What the search keeps for itself is neither of them: a key that
        // types or edits what is typed, Esc, ↑, ctrl-c.
        let all = ["n", "space", "esc", "up", "ctrl-c", "ctrl-k", "left"].map(String::from).to_vec();
        let (map, wrong) = Keymap::from_names(&[("find_next".into(), all)]);
        assert_eq!(wrong.len(), 8, "{wrong:?}");
        assert_eq!(wrong[0], "keys.find_next: the search of the middle keeps n for itself");
        assert_eq!(wrong[1], "keys.find_next: the search of the middle keeps Space for itself");
        assert_eq!(wrong[7], "keys.find_next: no key left for it; it keeps its own");
        assert_eq!(map.label(Key::FindNext), "Tab");
    }

    #[test]
    fn a_key_that_is_gone_is_said_so_and_passed_over() {
        // The sync's keys, gone with it: said, with why, and passed over;
        // the other keys named as they say.
        let named = vec![("keep_stack".to_string(), vec!["s".to_string()]), ("keep_workspace".to_string(), vec!["w".to_string()]), ("menu".to_string(), vec!["m".to_string()])];
        let (map, wrong) = Keymap::from_names(&named);
        assert_eq!(
            wrong,
            [
                "keys.keep_stack is no longer supported: the sync of the commit messages it picked in is gone; it is passed over",
                "keys.keep_workspace is no longer supported: the sync of the commit messages it picked in is gone; it is passed over",
            ]
        );
        assert_eq!(map.label(Key::Menu), "m");
        for c in ['s', 'w'] {
            assert_eq!(map.translate(&ev(KeyCode::Char(c), KeyModifiers::NONE)), None, "{c}");
        }
        assert!(Key::ALL.iter().all(|k| GONE.iter().all(|(name, _)| k.name() != *name)));
    }
}
