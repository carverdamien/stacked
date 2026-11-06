//! The Configure screen, which `Configure…` opens: from the menu of the
//! button `< stacked >` at the top left of the screen, which a click opens,
//! and from the end of every menu Space opens. Every setting there is, a
//! row each, in the order `stacked config` prints them, the colours' and
//! the keys' last: its name; what the global file gives it, the settings'
//! file that every checkout reads, found as `stacked config` finds it
//! (`App::config_file`); what the local file gives it, the checkout's own
//! `.stacked/local.toml`; and what is in effect here, the local file's over
//! the global file's, else the default. A setting read from one file alone
//! says so in the other's column: stacked web's are the global file's alone,
//! and the linter's the local file's alone. A secret, as `reviews.token`, is
//! never shown: only whether it is set.
//!
//! The cursor is on a row and a column, the global file's or the local
//! file's: Enter, Space and a click on the cell picked open its menu, which
//! sets it with what fits its kind, a choice of its values, or a line to
//! type it in, and takes it out of the file; Delete takes it out too. Each
//! is written by `config::set` and `config::unset` (`set_local`,
//! `unset_local`), refused in their words, as `stacked config` is; the
//! screen reads both files again after each write. stacked web's settings
//! count at once (`App::configured`).
//!
//! On the rows of `web.notifications` and `web.bell`, a third column,
//! `< Test >`: a test notice sent to stacked web's pages, which show it as
//! the system's banner even while they are looked at; or the bell rung
//! once, at the level in effect, or at the one being typed. Grayed, with
//! why, where nothing can show it or ring it: the terminal, StackED, and a
//! stacked web with no page.

use super::*;
use crate::config::Given;

/// A column of the screen the cursor may be in: the global file's, the
/// local file's, or the test buttons'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Global,
    Local,
    Test,
}

impl Column {
    /// The file it is, as the screen names it.
    pub fn file(self) -> &'static str {
        match self {
            Column::Global => "the global file",
            _ => "the local file",
        }
    }
}

/// What a file's column says of a setting: that the file is never read for
/// it, and why, in a few words and whole; that it does not set it; or what
/// it sets it to, as the file writes it, and as it is typed again (nothing
/// of a secret, which is only said to be set).
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Never { short: &'static str, why: String },
    Unset,
    Set { shown: String, typed: String },
}

impl Cell {
    /// As its column shows it.
    fn shown(&self) -> &str {
        match self {
            Cell::Never { short, .. } => short,
            Cell::Unset => "not set",
            Cell::Set { shown, .. } => shown,
        }
    }
}

/// A row of the screen: a setting, what each file gives it, what is in
/// effect, and how its value is given.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    pub key: String,
    pub global: Cell,
    pub local: Cell,
    pub effect: String,
    pub given: Given,
}

impl SettingRow {
    /// Its cell in `column`, a file's.
    pub fn cell(&self, column: Column) -> &Cell {
        match column {
            Column::Global => &self.global,
            _ => &self.local,
        }
    }

    /// Whether it has a test button: stacked web's notifications and bell.
    pub fn tested(&self) -> bool {
        self.key == NOTIFICATIONS || self.key == BELL
    }
}

/// The screen, open: its rows, as the files said them when last read; the
/// row and the column the cursor is on, and the first row shown; the screen
/// it was opened from, which Esc goes back to; the global file, and the
/// local file, or why there is none; what is wrong with either; the bell's
/// level in effect; and the setting being typed, in which file's column.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuring {
    pub rows: Vec<SettingRow>,
    pub sel: usize,
    pub column: Column,
    top: usize,
    back: Screen,
    pub global: PathBuf,
    pub local: std::result::Result<PathBuf, String>,
    pub wrong: Vec<String>,
    pub bell: u8,
    pub typing: Option<(String, Column)>,
}

/// A press of a test button, for `stacked web` to send its pages
/// (`App::take_tests`): the test notice, or the bell at a level.
#[derive(Debug, Clone, PartialEq)]
pub enum Tested {
    Notice(Notice),
    Bell(u8),
}

/// Where the editor runs, as the test buttons need it: in a terminal; in
/// `stacked web`, with how many pages it has; or in StackED's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Host {
    #[default]
    Terminal,
    Web {
        pages: usize,
    },
    Window,
}

/// The settings with a test button.
pub const NOTIFICATIONS: &str = "web.notifications";
pub const BELL: &str = "web.bell";

/// The test notice: of no run, so that a click on it does nothing more
/// (`App::noticed`).
pub const TEST_RUN: &str = "stacked-test";
pub const TEST_TITLE: &str = "stacked";
pub const TEST_BODY: &str = "This is a test notice from stacked.";

/// A test button, as drawn.
pub const TEST: &str = "< Test >";

/// The button at the top left of the screen, as the top bar draws it.
pub const STACKED: &str = "< stacked >";

/// What a choice of a setting's values is a choice of, in its menu: what
/// is picked is kept nowhere, but written once Enter does the item.
pub const SETTING: &str = "setting";

/// What a file gives `key`, as its column shows it: `text`, the file, if
/// there is one.
fn cell_of(text: Option<&str>, key: &str) -> Cell {
    let Some(v) = text.and_then(|t| crate::config::written(t, key)) else { return Cell::Unset };
    if crate::config::secret(key) {
        return Cell::Set { shown: "set, not shown".into(), typed: String::new() };
    }
    let typed = match &v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Cell::Set { shown: v.to_string(), typed }
}

/// What is in effect for `key`, its `values`, as the screen shows it: in the
/// file's form; not set; or, a secret, only whether it is set.
fn effect_of(key: &str, values: &[String]) -> String {
    match (values.is_empty(), crate::config::secret(key)) {
        (true, _) => "not set".into(),
        (false, true) => "set, not shown".into(),
        (false, false) => crate::config::check(key, values).unwrap_or_else(|_| values.join(" ")),
    }
}

/// What the text of the file at `path` is, if there is one: none if it is
/// not there; what is wrong, if it cannot be read.
fn text_of(path: &Path, wrong: &mut Vec<String>) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            wrong.push(format!("{}: {e}", path.display()));
            None
        }
    }
}

impl Configuring {
    /// The screen, read now: the global file at `global`, the local file of
    /// the checkout `checkout` is in, and what is in effect there; opened
    /// from `back`.
    fn read(global: &Path, checkout: &Path, back: Screen) -> Configuring {
        let (cfg, mut wrong) = crate::config::in_effect(global, checkout);
        let global_text = text_of(global, &mut wrong);
        let local = crate::config::local_to_write(checkout).map_err(|e| e.trim_end_matches("; nothing written").to_string());
        let local_text = local.as_ref().ok().and_then(|p| text_of(p, &mut wrong));
        wrong.dedup();
        let rows = crate::config::every_key()
            .into_iter()
            .map(|key| {
                let global = match crate::config::local_only(&key) {
                    true => Cell::Never {
                        short: "not read: local only",
                        why: format!("{key} is a checkout's own setting, read only from its .stacked/local.toml, never from the global file."),
                    },
                    false => cell_of(global_text.as_deref(), &key),
                };
                let local = match (&local, crate::config::global_only(&key)) {
                    (_, true) => Cell::Never {
                        short: "not read: global only",
                        why: format!("{key} is read only from the global file, the settings' file, never from a checkout's own .stacked/local.toml."),
                    },
                    (Err(why), false) => Cell::Never { short: "no local file", why: format!("There is no local file: {why}.") },
                    (Ok(_), false) => cell_of(local_text.as_deref(), &key),
                };
                let effect = effect_of(&key, &crate::config::value_of(&cfg, &key).unwrap_or_default());
                let given = crate::config::given(&key).unwrap_or(Given::Typed("any text"));
                SettingRow { key, global, local, effect, given }
            })
            .collect();
        Configuring {
            rows,
            sel: 0,
            column: Column::Global,
            top: 0,
            back,
            global: global.to_path_buf(),
            local,
            wrong,
            bell: cfg.web.bell,
            typing: None,
        }
    }

    /// The row the cursor is on.
    pub fn row(&self) -> Option<&SettingRow> {
        self.rows.get(self.sel)
    }
}

impl App {
    /// `Configure…`: the screen, read now, opened from the screen this is
    /// on, the cursor on its first row, in the global file's column.
    pub(super) fn open_configure(&mut self) {
        if self.screen == Screen::Configure {
            return;
        }
        self.configuring = Some(Box::new(Configuring::read(&self.config_file, &self.checkout, self.screen)));
        self.screen = Screen::Configure;
    }

    /// The screen read again from both files, the cursor on the row and in
    /// the column it was in; and stacked web's settings as they are now in
    /// effect, which count at once (`configured`).
    fn configure_again(&mut self) {
        let Some(c) = self.configuring.as_ref() else { return };
        let mut new = Configuring::read(&self.config_file, &self.checkout, c.back);
        (new.sel, new.column, new.top, new.typing) = (c.sel.min(new.rows.len().saturating_sub(1)), c.column, c.top, c.typing.clone());
        self.configuring = Some(Box::new(new));
        self.configured();
    }

    /// The settings stacked web reads each time, made as they are now in
    /// effect, once the Configure screen wrote one: its notifications and
    /// its bell count at once. The others count from the next time stacked
    /// opens, but the agent's, which each ask reads as it starts.
    fn configured(&mut self) {
        let (cfg, _) = crate::config::in_effect(&self.config_file, &self.checkout);
        self.cfg.web = cfg.web;
    }

    /// The screen left, for the one it was opened from.
    fn leave_configure(&mut self) {
        self.screen = self.configuring.take().map_or(Screen::Main, |c| c.back);
    }

    /// The menu of the button `< stacked >`: `Configure…`, grayed on the
    /// Configure screen.
    pub(super) fn stacked_menu(&self) -> Menu {
        let here = self.screen == Screen::Configure;
        let note = here.then(|| "Configure… is grayed: this is the Configure screen.".to_string());
        Menu::new("stacked", vec![Item::new("Configure…", Action::Configure).when(!here)], note)
    }

    /// The row the cursor is on and its file's column, if it is in one.
    fn setting_here(&self) -> Option<(&SettingRow, Column)> {
        let c = self.configuring.as_ref()?;
        let row = c.row()?;
        (c.column != Column::Test).then_some((row, c.column))
    }

    /// The items of the menu of the cell the cursor is on, which Enter and
    /// Space open: what sets it, as fits its kind, a choice of its values or
    /// a line to type it in; and what takes it out of its file. On a test
    /// button, the test. Its title, and why what is grayed is.
    pub(super) fn setting_items(&self, items: &mut Vec<Item>) -> (String, Option<String>) {
        let Some(c) = self.configuring.as_ref() else { return (String::new(), None) };
        let Some(row) = c.row() else { return (String::new(), None) };
        let key = row.key.clone();
        if c.column == Column::Test {
            let bell = key == BELL;
            let why = self.test_why(bell);
            let (label, action) = match bell {
                true => ("Ring the bell once", Action::TestBell),
                false => ("Send a test notice", Action::TestNotice),
            };
            items.push(Item::new(label, action).when(why.is_none()));
            return (format!("{key}: its test"), why);
        }
        let column = c.column;
        let title = format!("{key} in {}", column.file());
        let cell = row.cell(column);
        if let Cell::Never { why, .. } = cell {
            items.push(Item::new("Set it…", Action::Said).when(false));
            return (title, Some(why.clone()));
        }
        let set = matches!(cell, Cell::Set { .. });
        match row.given {
            Given::OneOf(values) => {
                // On the value the file gives it, else on the one in effect.
                let now = match cell {
                    Cell::Set { typed, .. } => typed.clone(),
                    _ => row.effect.trim_matches('"').to_string(),
                };
                let list: Vec<Choice> = values
                    .iter()
                    .copied()
                    .map(|v| {
                        let lit = crate::config::check(&key, &[v.to_string()]).unwrap_or_else(|_| v.to_string());
                        Choice::new(v, v, format!("written as {key} = {lit}"), Some(Action::SetSetting(key.clone(), column, v.to_string())))
                    })
                    .collect();
                let mut choices = Choices::new(SETTING, "its value", list, Some(&now));
                // Many, a dropdown drawn as one, `Value [interdiff]`.
                if values.len() > 3 {
                    choices.named = Some("Value");
                }
                items.push(Item::new("Set it to", Action::Said).with(choices));
            }
            Given::Typed(_) | Given::List => items.push(Item::new("Type its value…", Action::EditSetting(key.clone(), column))),
        }
        items.push(Item::new("Take it out of this file", Action::UnsetSetting(key.clone(), column)).when(set));
        let why = (!set).then(|| format!("{} does not set {key}: there is nothing to take out of it.", capital(column.file())));
        (title, why)
    }

    /// Writes `key` in the file of `column`: `values`, or taken out of it
    /// (None), as `stacked config` writes it, refused in its words; the
    /// bottom line saying what came of it, and, written in the global file,
    /// that the local file sets it too, and is the one used here. The
    /// screen read again after.
    pub(super) fn write_setting(&mut self, key: &str, column: Column, values: Option<Vec<String>>) {
        let global = self.config_file.clone();
        let written = match (column, &values) {
            (Column::Global, Some(v)) => crate::config::set(&global, key, v).map(|_| global.clone()),
            (Column::Global, None) => crate::config::unset(&global, key).map(|_| global.clone()),
            (_, Some(v)) => crate::config::set_local(&self.checkout, key, v),
            (_, None) => crate::config::unset_local(&self.checkout, key),
        };
        self.configure_again();
        self.status = match written {
            Ok(at) => {
                let did = if values.is_some() { "written in" } else { "taken out of" };
                let mut said = format!("{key} {did} {}", at.display());
                let c = self.configuring.as_ref();
                let local = c.and_then(|c| c.local.as_ref().ok().cloned());
                let over = c.and_then(|c| c.rows.iter().find(|r| r.key == key)).is_some_and(|r| matches!(r.local, Cell::Set { .. }));
                if let (Column::Global, true, Some(l)) = (column, over, local) {
                    said += &format!(": {} sets it too, and is the one used in this checkout", l.display());
                }
                said
            }
            Err(e) => e,
        };
    }

    /// A line to type the value of the setting the cursor is on in its
    /// file's column, from what that file gives it, else from what is in
    /// effect; nothing of a secret.
    pub(super) fn type_setting(&mut self, key: &str, column: Column) {
        let Some(c) = self.configuring.as_mut() else { return };
        let Some(row) = c.rows.iter().find(|r| r.key == key) else { return };
        let from = match (row.cell(column), crate::config::secret(key)) {
            (_, true) => String::new(),
            (Cell::Set { typed, .. }, _) => typed.clone(),
            _ => match row.given {
                Given::List => row.effect.replace("not set", "[]"),
                _ if row.effect == "not set" => String::new(),
                _ => row.effect.trim_matches('"').to_string(),
            },
        };
        c.typing = Some((key.to_string(), column));
        self.prompt(Asking::Setting, &from);
    }

    /// What `text`, typed for the setting being typed, gives, said: what is
    /// written, in the file's form, a secret not shown; or why it is
    /// refused, in `stacked config`'s words; and whether Enter takes it.
    pub(super) fn setting_said(&self, text: &str) -> (String, bool) {
        let Some((key, column)) = self.configuring.as_ref().and_then(|c| c.typing.clone()) else { return (String::new(), false) };
        match crate::config::typed(&key, text).and_then(|v| crate::config::check(&key, &v)) {
            Ok(_) if crate::config::secret(&key) => (format!("{key}, not shown, in {}", column.file()), true),
            Ok(lit) => (format!("{key} = {lit}, in {}", column.file()), true),
            Err(e) => (e, false),
        }
    }

    /// `text`, typed for the setting being typed, written in its file.
    pub(super) fn setting_typed(&mut self, text: &str) {
        let Some((key, column)) = self.configuring.as_mut().and_then(|c| c.typing.take()) else { return };
        match crate::config::typed(&key, text) {
            Ok(values) => self.write_setting(&key, column, Some(values)),
            Err(e) => self.status = e,
        }
    }

    /// The line being typed left: nothing written.
    pub(super) fn setting_untyped(&mut self) {
        if let Some(c) = self.configuring.as_mut() {
            c.typing = None;
        }
    }

    /// What the box of the line being typed says on its top: the setting,
    /// its file, and what to type.
    pub(super) fn setting_title(&self) -> String {
        let Some((key, column)) = self.configuring.as_ref().and_then(|c| c.typing.clone()) else { return String::new() };
        let what = match crate::config::given(&key) {
            Ok(Given::Typed(what)) => what.to_string(),
            Ok(Given::List) => "a list, as [\"one\", \"two\"], or [] for none".into(),
            _ => "its value".into(),
        };
        format!("{key} in {}: {what}", column.file())
    }

    /// Whether the line being typed is a secret's: drawn as dots.
    pub(super) fn typing_secret(&self) -> bool {
        self.configuring.as_ref().and_then(|c| c.typing.as_ref()).is_some_and(|(k, _)| crate::config::secret(k))
    }

    /// Why the test button of the notice (or of the bell, `bell`) is
    /// grayed, if it is: nothing shows a notice, nor rings the bell, but a
    /// page of stacked web; and a bell at 0 never rings.
    pub(super) fn test_why(&self, bell: bool) -> Option<String> {
        let why = match (self.host, bell) {
            (Host::Terminal, false) => "No page shows the test notice here: this is stacked in a terminal, and notices are stacked web's, shown in its page. Press it in stacked web.",
            (Host::Terminal, true) => "No page rings the bell here: this is stacked in a terminal, and the bell is stacked web's, rung in its page. Press it in stacked web.",
            (Host::Window, false) => "StackED shows no notices: they are stacked web's, shown in its page. Press it in stacked web.",
            (Host::Window, true) => "StackED rings no bell: it is stacked web's, rung in its page. Press it in stacked web.",
            (Host::Web { pages: 0 }, false) => "No stacked web page is open, so there is nothing to show the notice in.",
            (Host::Web { pages: 0 }, true) => "No stacked web page is open, so there is no page to ring the bell.",
            (Host::Web { .. }, true) if self.bell_level() == 0 => "The bell's level is 0, so it never rings: set web.bell above 0 first.",
            _ => return None,
        };
        Some(why.into())
    }

    /// The level the bell's test rings at: the one being typed for
    /// `web.bell`, if it is one; else the one in effect.
    fn bell_level(&self) -> u8 {
        let Some(c) = self.configuring.as_ref() else { return self.cfg.web.bell };
        if let Mode::Prompt(p) = &self.mode
            && p.asking == Asking::Setting
            && c.typing.as_ref().is_some_and(|(k, _)| k == BELL)
            && let Ok(lit) = crate::config::check(BELL, &[p.ed.text()])
        {
            return lit.parse().unwrap_or(c.bell);
        }
        c.bell
    }

    /// A test button pressed: the test notice, or the bell (`bell`), kept
    /// for stacked web to send its pages, and said; grayed, why.
    pub(super) fn press_test(&mut self, bell: bool) {
        if let Some(why) = self.test_why(bell) {
            self.status = why;
            return;
        }
        if bell {
            let level = self.bell_level();
            self.tested.push(Tested::Bell(level));
            self.status = format!("stacked web's page rings the bell once, at {level}: a browser plays a sound only once the page had a click or a key");
        } else {
            self.tested.push(Tested::Notice(Notice { run: TEST_RUN.into(), title: TEST_TITLE.into(), body: TEST_BODY.into() }));
            self.status = "the test notice is sent to stacked web's page, which shows it as the system's banner: the browser asks first whether the page may show banners, if you never said".into();
        }
    }

    /// The test buttons pressed since they were last taken, taken: for
    /// stacked web to send its pages.
    pub fn take_tests(&mut self) -> Vec<Tested> {
        std::mem::take(&mut self.tested)
    }

    /// The test button of row `i` pressed by a click, the cursor going
    /// there: on the screen, or while the value of `web.bell` is typed,
    /// which the bell's then rings at.
    pub(super) fn click_test(&mut self, i: usize) {
        let Some(c) = self.configuring.as_mut() else { return };
        let Some(row) = c.rows.get(i).filter(|r| r.tested()) else { return };
        let bell = row.key == BELL;
        if !matches!(self.mode, Mode::Prompt(_)) {
            (c.sel, c.column) = (i, Column::Test);
        }
        self.press_test(bell);
    }

    /// Row `i`, in `column` if it is one, picked by the mouse, as the
    /// arrows would; the test button only on a row that has one.
    pub(super) fn pick_setting(&mut self, i: usize, column: Option<Column>) {
        let Some(c) = self.configuring.as_mut() else { return };
        let Some(row) = c.rows.get(i) else { return };
        let tested = row.tested();
        c.sel = i;
        match column {
            Some(Column::Test) if !tested => {}
            Some(col) => c.column = col,
            None if c.column == Column::Test && !tested => c.column = Column::Local,
            None => {}
        }
    }

    /// Whether the cursor is on row `i`, in `column` if it is one.
    pub(super) fn on_setting(&self, i: usize, column: Option<Column>) -> bool {
        self.configuring.as_ref().is_some_and(|c| c.sel == i && column.is_none_or(|col| col == c.column))
    }

    /// What Delete does on the screen: the cell the cursor is on taken out
    /// of its file; or why not, it does not set it, or it is never read.
    pub(super) fn setting_deleting(&self) -> Option<std::result::Result<Action, String>> {
        let (row, column) = self.setting_here()?;
        Some(match row.cell(column) {
            Cell::Never { why, .. } => Err(why.clone()),
            Cell::Unset => Err(format!("{} is not set in {}", row.key, column.file())),
            Cell::Set { .. } => Ok(Action::UnsetSetting(row.key.clone(), column)),
        })
    }

    /// A key on the screen: ↑↓ (pgup pgdn home end) from row to row; ← →
    /// from the global file to the local file and to the test button of a
    /// row that has one; Enter, the menu of the cell, or the test pressed;
    /// Esc back to where it was opened from.
    pub(super) fn configure_key(&mut self, k: KeyEvent) -> Effect {
        let page = self.page();
        let Some(c) = self.configuring.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let last = c.rows.len().saturating_sub(1);
        match k.code {
            KeyCode::Esc => {
                self.leave_configure();
                return Effect::None;
            }
            KeyCode::Up => c.sel = c.sel.saturating_sub(1),
            KeyCode::Down => c.sel = (c.sel + 1).min(last),
            KeyCode::PageUp => c.sel = c.sel.saturating_sub(page),
            KeyCode::PageDown => c.sel = (c.sel + page).min(last),
            KeyCode::Home => c.sel = 0,
            KeyCode::End => c.sel = last,
            KeyCode::Left => {
                c.column = match c.column {
                    Column::Test => Column::Local,
                    _ => Column::Global,
                }
            }
            KeyCode::Right => {
                let tested = c.row().is_some_and(SettingRow::tested);
                c.column = match c.column {
                    Column::Global => Column::Local,
                    _ if tested => Column::Test,
                    col => col,
                }
            }
            KeyCode::Enter if c.column == Column::Test => {
                let bell = c.row().is_some_and(|r| r.key == BELL);
                self.press_test(bell);
                return Effect::None;
            }
            KeyCode::Enter => {
                self.menu_here();
                return Effect::None;
            }
            _ => {}
        }
        // A row with no test button: the cursor in the local file's column.
        if c.column == Column::Test && !c.row().is_some_and(SettingRow::tested) {
            c.column = Column::Local;
        }
        Effect::None
    }

    /// The screen: on top, what it is, the two files and what is wrong with
    /// them; then a line naming the columns, and the rows under it, from
    /// where it is scrolled to, the row the cursor is on in sight, and the
    /// cell the cursor is in under the cursor; a test button where a row
    /// has one, dim while it is grayed. Longer than the screen, its scroll
    /// bar beside its rows; the wheel scrolls them.
    pub(super) fn draw_configure(&mut self, buf: &mut Buffer, body: Rect) {
        let (w, h) = (body.width as usize, body.height as usize);
        let turned = self.turned(Wheeled::Settings);
        let free = self.free(Wheeled::Settings);
        let why = [self.test_why(false), self.test_why(true)];
        let Some(c) = self.configuring.as_mut() else { return };
        let mut head: Vec<(String, Style)> = vec![(
            " Configure: each setting, what the global file and the local file give it, and what is in effect here".into(),
            color::header(),
        )];
        head.push((format!(" Global: {}, the settings' file, which every checkout reads", c.global.display()), Style::new()));
        head.push((
            match &c.local {
                Ok(p) => format!(" Local: {}, this checkout's own, read over the global file", p.display()),
                Err(why) => format!(" Local: none, as {why}"),
            },
            Style::new(),
        ));
        for wrong in c.wrong.iter().take(3) {
            head.push((format!(" Wrong: {wrong}"), color::warn()));
        }
        if c.wrong.len() > 3 {
            head.push((format!(" Wrong: {} more, which `stacked config` says", c.wrong.len() - 3), color::warn()));
        }
        for (y, (text, st)) in head.iter().enumerate() {
            put(buf, body, 0, y, w, &fit(text, w), *st);
        }
        // The columns: the setting's name as wide as the longest, a third
        // of the screen at most; the room left, less the test button's, in
        // three, the files' and what is in effect.
        let name_w = c.rows.iter().map(|r| r.key.chars().count()).max().unwrap_or(0).clamp(12, (w / 3).max(12)) + 2;
        let test_w = TEST.chars().count() + 3;
        let cell_w = w.saturating_sub(name_w + test_w + 3) / 3;
        let xs = [name_w + 1, name_w + 2 + cell_w, name_w + 3 + 2 * cell_w];
        let test_x = name_w + 3 + 3 * cell_w;
        let at = head.len();
        let title_row = format!(
            "{}│{}│{}│{}│ test",
            fit(" setting", name_w),
            fit(" global", cell_w),
            fit(" local", cell_w),
            fit(" in effect", cell_w)
        );
        put(buf, body, 0, at, w, &fit(&title_row, w), color::meta());
        let top = at + 1;
        let room = h.saturating_sub(top);
        let list = Rect::new(body.x, body.y + top as u16, body.width, room as u16);
        self.scrolls.borrow_mut().push((list, Wheeled::Settings));
        if let Some(n) = turned {
            c.top = moved(c.top, n, c.rows.len().saturating_sub(room));
        }
        if !free {
            if c.sel < c.top {
                c.top = c.sel;
            } else if c.sel >= c.top + room {
                c.top = c.sel + 1 - room;
            }
        }
        c.top = c.top.min(c.rows.len().saturating_sub(room.max(1)));
        let c = (**c).clone();
        let shown = Shown { top: c.top, rows: room, all: c.rows.len() };
        let tw = self.pane_bar(Wheeled::Settings, list, buf.area, (0, room), shown);
        for (y, (i, row)) in c.rows.iter().enumerate().skip(c.top).take(room).enumerate() {
            let on = i == c.sel;
            let name_st = if on { Style::new().patch(color::picked()) } else { Style::new() };
            put(buf, list, 0, y, name_w.min(tw), &fit(&format!(" {}", row.key), name_w), name_st);
            spot_in(&self.spots, list, (0, y, name_w, 1), Spot::Setting(i, None));
            // The files' cells, then what is in effect, which has no cell of
            // its own to change (its column given only to have one).
            for (k, (cell, col)) in [(Some(&row.global), Column::Global), (Some(&row.local), Column::Local), (None, Column::Test)].into_iter().enumerate() {
                let x = xs[k];
                put(buf, list, x - 1, y, 1, "│", color::meta());
                let (text, mut st) = match cell {
                    Some(cell @ (Cell::Never { .. } | Cell::Unset)) => (cell.shown().to_string(), color::dim()),
                    Some(cell) => (cell.shown().to_string(), Style::new()),
                    None => (row.effect.clone(), Style::new()),
                };
                if on && cell.is_some() && c.column == col {
                    st = on_cursor(st);
                }
                let cw = cell_w.min(tw.saturating_sub(x));
                put(buf, list, x, y, cw, &fit(&format!(" {text}"), cw), st);
                if cell.is_some() {
                    spot_in(&self.spots, list, (x, y, cell_w, 1), Spot::Setting(i, Some(col)));
                } else {
                    spot_in(&self.spots, list, (x, y, cell_w, 1), Spot::Setting(i, None));
                }
            }
            put(buf, list, test_x - 1, y, 1, "│", color::meta());
            if row.tested() {
                let ok = why[(row.key == BELL) as usize].is_none();
                let buttons = [(TEST.to_string(), ok, on && c.column == Column::Test)];
                self.draw_buttons(buf, list, (test_x.saturating_sub(1), y, test_w + 1), &buttons, |_| Spot::Setting(i, Some(Column::Test)));
            }
        }
        self.areas.borrow_mut().push(list);
    }

    /// What the bottom line says on the screen: the keys there are, in a
    /// file's column; on a test button, what Enter does, or why it is
    /// grayed.
    pub(super) fn configure_hint(&self) -> String {
        let Some(c) = self.configuring.as_ref() else { return String::new() };
        if c.column == Column::Test {
            let bell = c.row().is_some_and(|r| r.key == BELL);
            return match self.test_why(bell) {
                Some(why) => format!("{} · ←: the local file · esc: back", why.trim_end_matches('.')),
                None if bell => "enter: ring the bell once · ←: the local file · esc: back · ?: what can I do here".into(),
                None => "enter: send a test notice · ←: the local file · esc: back · ?: what can I do here".into(),
            };
        }
        let other = match c.column {
            Column::Global => "→: the local file",
            _ => "←→: the other file",
        };
        let delete = match self.setting_deleting() {
            Some(Ok(_)) => "delete: take it out · ",
            _ => "",
        };
        format!("↑↓ choose · {other} · enter: change it · {delete}esc: back · ?: what can I do here")
    }

    /// What `?` says on the screen.
    pub(super) fn configure_help(&self) -> Vec<String> {
        let Some(c) = self.configuring.as_ref() else { return vec![] };
        let mut out = vec![
            "This screen shows every setting of stacked, a row each: its name; what the global file gives it, the settings' file that every checkout reads; what the local file gives it, this checkout's own .stacked/local.toml; and what is in effect here, the local file's over the global file's, else the default.".to_string(),
        ];
        out.push(match &c.local {
            Ok(l) => format!("The global file is {}. The local file is {}.", c.global.display(), l.display()),
            Err(why) => format!("The global file is {}. There is no local file here, as {why}.", c.global.display()),
        });
        match (c.row(), c.column) {
            (Some(row), Column::Test) => {
                let bell = row.key == BELL;
                out.push(match bell {
                    true => format!(
                        "Press Enter to have stacked web's page ring its bell once, at the level in effect, {}, or at the one being typed, even while you look at the page. A browser plays a sound only once the page had a click or a key.",
                        c.bell
                    ),
                    false => format!(
                        "Press Enter to send stacked web's page a test notice, \"{TEST_BODY}\", shown as the system's banner even while you look at the page. The browser asks first whether the page may show banners, if you never said."
                    ),
                });
                out.extend(self.test_why(bell).map(|why| format!("The button is grayed: {why}")));
            }
            (Some(row), column) => match row.cell(column) {
                Cell::Never { why, .. } => out.push(why.clone()),
                cell => {
                    let how = match row.given {
                        Given::OneOf(values) => format!("one of {}", values.join(", ")),
                        Given::Typed(what) => format!("typed: {what}"),
                        Given::List => "typed as a list, as [\"one\", \"two\"], or [] for none".into(),
                    };
                    out.push(format!(
                        "Press Enter for the menu of {} in {}: there, set it, {how}, or take it out of the file. It is written as `stacked config` writes it, and refused as it is refused.",
                        row.key,
                        column.file()
                    ));
                    if matches!(cell, Cell::Set { .. }) {
                        out.push(format!("Press Delete to take {} out of {}.", row.key, column.file()));
                    }
                }
            },
            (None, _) => {}
        }
        out.push("A secret, as reviews.token, is never shown: only whether it is set. A value a file gives wrongly is said on top, and its default is in effect.".into());
        out.push("Each change is written at once, and the screen reads both files again. stacked web's notices and bell follow it at once; the agent's settings count from its next ask; the others count from the next time stacked opens.".into());
        out.push("Press ↑↓ (pgup pgdn home end) for another setting; ← → for the global file, the local file, and the test buttons on the rows of web.notifications and web.bell; Esc to go back.".into());
        out
    }
}

/// `s` with its first letter a capital.
fn capital(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_s_cell_says_what_it_writes_and_a_secret_only_that_it_is_set() {
        let text = "editor = \"hx\"\n[agent]\nargs = [\"-v\"]\n[reviews]\nurl = \"https://phab.example.com\"\ntoken = \"api-secret\"\n[web]\nbell = 40\n";
        assert_eq!(cell_of(Some(text), "editor"), Cell::Set { shown: "\"hx\"".into(), typed: "hx".into() });
        assert_eq!(cell_of(Some(text), "agent.args"), Cell::Set { shown: "[\"-v\"]".into(), typed: "[\"-v\"]".into() });
        assert_eq!(cell_of(Some(text), "web.bell"), Cell::Set { shown: "40".into(), typed: "40".into() });
        assert_eq!(cell_of(Some(text), "reviews.token"), Cell::Set { shown: "set, not shown".into(), typed: String::new() });
        assert_eq!(cell_of(Some(text), "view.layout"), Cell::Unset);
        assert_eq!(cell_of(None, "editor"), Cell::Unset);
        assert_eq!(effect_of("reviews.token", &["api-secret".into()]), "set, not shown");
        assert_eq!(effect_of("reviews.token", &[]), "not set");
        assert_eq!(effect_of("web.bell", &["0".into()]), "0");
        assert_eq!(effect_of("agent.args", &["-v".into()]), "[\"-v\"]");
        assert_eq!(capital("the global file"), "The global file");
    }
}
