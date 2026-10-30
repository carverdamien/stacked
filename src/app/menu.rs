//! StackED's menu bar: egui's own, at the top of each of its windows, the
//! same on every system, with one menu, File: New…, Open…, Open Recent,
//! Close Window and Quit StackED. It is used with the mouse: none of its
//! items has a shortcut, on any system, and it takes no key, so that every
//! key goes to the editor, as in a terminal. What its items open or make,
//! they ask of the system's dialogs, away from the windows (`dialogs`).
//!
//! There is no Edit menu: copy and paste are egui's, from the system's
//! keys, as the window reads them (`app::inputs`): the text selected with
//! the mouse copied, the clipboard typed where text is written.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::containers::menu::{MenuButton, SubMenuButton};
use eframe::egui::{self, Sense};

use super::dialogs::{Dialogs, Job};
use super::windows::{Ask, File, titles};

/// An item of the File menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    New,
    Open,
    /// An item of Open Recent: its database.
    Recent(PathBuf),
    /// Open Recent's Clear Menu.
    Clear,
    Close,
    Quit,
}

/// A row of the File menu: an item and its title; Open Recent, a menu of
/// its own; or a separator.
enum Row {
    Item(Item, &'static str),
    Recent,
    Separator,
}

/// The File menu.
const FILE: [Row; 6] = [
    Row::Item(Item::New, "New…"),
    Row::Item(Item::Open, "Open…"),
    Row::Recent,
    Row::Separator,
    Row::Item(Item::Close, "Close Window"),
    Row::Item(Item::Quit, "Quit StackED"),
];

/// The menus as the bar shows them, each row its title and nothing after
/// it, as no item has a shortcut; a separator `—`.
pub fn rows() -> Vec<(&'static str, Vec<String>)> {
    let shown = FILE
        .iter()
        .map(|r| match r {
            Row::Item(_, title) => title.to_string(),
            Row::Recent => format!("Open Recent {}", SubMenuButton::RIGHT_ARROW),
            Row::Separator => "—".into(),
        })
        .collect();
    vec![("File", shown)]
}

/// What a window's bar keeps from pass to pass: Open Recent as it was when
/// the File menu last opened, and whether that menu is open.
#[derive(Debug, Default)]
pub struct Bar {
    recent: Vec<PathBuf>,
    open: bool,
}

/// The bar of every window of the set: what the File menu does (`File`),
/// the dialogs it asks, and whether StackED is quitting.
pub struct Menu {
    file: Arc<File>,
    dialogs: Dialogs,
    quitting: AtomicBool,
}

/// A button of the bar's, sensing clicks but never taking the keyboard's
/// focus: Tab, Enter and Space stay the editor's.
fn button(text: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text.into()).sense(Sense::CLICK)
}

impl Menu {
    /// The bar of the window set whose File menu `file` is, its dialogs
    /// asked of `ask`, on a thread of their own.
    pub fn new(file: Arc<File>, ask: Arc<dyn Ask + Send + Sync>) -> Arc<Menu> {
        let dialogs = Dialogs::start(file.clone(), ask);
        Arc::new(Menu {
            file,
            dialogs,
            quitting: AtomicBool::new(false),
        })
    }

    /// `text` said in an alert, after those said before, on the dialogs'
    /// thread.
    pub fn alert(&self, text: String) {
        self.dialogs.ask(Job::Alert(text));
    }

    /// Whether StackED is quitting, by File → Quit StackED.
    pub fn quitting(&self) -> bool {
        self.quitting.load(Ordering::Acquire)
    }

    /// Whether an item of the File menu runs, its dialog open: those that
    /// open one are greyed out meanwhile.
    pub fn busy(&self) -> bool {
        self.dialogs.busy()
    }

    /// The bar, drawn at the top of the window `ctx` paints, in a panel of
    /// its own: the item picked with the mouse, if one was. It takes no
    /// key: every key goes to the editor. `bar` is the window's.
    pub fn bar(&self, ctx: &egui::Context, bar: &mut Bar) -> Option<Item> {
        let mut picked = None;
        let busy = self.busy();
        let frame = egui::Frame::side_top_panel(&ctx.style()).fill(super::BG);
        // Each window's bar its own, as egui keeps a panel by its id.
        egui::TopBottomPanel::top(egui::Id::new(("StackED's menu bar", ctx.viewport_id()))).frame(frame).show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let (_, file) = MenuButton::from_button(button("File")).ui(ui, |ui| {
                    // As it opens: Open Recent read again from the state kept.
                    if !bar.open {
                        bar.recent = self.file.recent();
                        bar.open = true;
                    }
                    for r in &FILE {
                        match r {
                            Row::Item(item, title) => {
                                let enabled = !busy || !matches!(item, Item::New | Item::Open);
                                if ui.add_enabled(enabled, button(*title)).clicked() {
                                    picked = Some(item.clone());
                                }
                            }
                            Row::Recent => {
                                let open_recent = button("Open Recent").right_text(SubMenuButton::RIGHT_ARROW);
                                SubMenuButton::from_button(open_recent).ui(ui, |ui| {
                                    for (db, title) in bar.recent.iter().zip(titles(&bar.recent)) {
                                        if ui.add_enabled(!busy, button(title)).on_hover_text(db.display().to_string()).clicked() {
                                            picked = Some(Item::Recent(db.clone()));
                                        }
                                    }
                                    if !bar.recent.is_empty() {
                                        ui.separator();
                                    }
                                    if ui.add_enabled(!bar.recent.is_empty(), button("Clear Menu")).clicked() {
                                        picked = Some(Item::Clear);
                                    }
                                });
                            }
                            Row::Separator => {
                                ui.separator();
                            }
                        }
                    }
                });
                bar.open = file.is_some();
            });
        });
        picked
    }

    /// `item` done, from the window `ctx` paints: on the dialogs' thread if
    /// it opens one, unless another runs; Close Window closes this window,
    /// Quit StackED every window.
    pub fn act(&self, ctx: &egui::Context, item: Item) {
        match item {
            Item::New => {
                self.dialogs.ask(Job::New);
            }
            Item::Open => {
                self.dialogs.ask(Job::Open);
            }
            Item::Recent(db) => {
                self.dialogs.ask(Job::Recent(db));
            }
            Item::Clear => self.file.clear(),
            Item::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Item::Quit => {
                self.quitting.store(true, Ordering::Release);
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};

    use super::super::windows::Opened;
    use egui::os::OperatingSystem;
    use egui::{Key, Modifiers};

    /// The dialogs' answers, as the test gives them: the databases picked,
    /// how many times they were asked, and what is said.
    #[derive(Default)]
    struct Answers {
        databases: Vec<PathBuf>,
        asked: AtomicUsize,
        said: Mutex<Vec<String>>,
    }

    impl Ask for Answers {
        fn databases(&self) -> Vec<PathBuf> {
            self.asked.fetch_add(1, Ordering::Relaxed);
            self.databases.clone()
        }

        fn new_database(&self) -> Option<PathBuf> {
            self.asked.fetch_add(1, Ordering::Relaxed);
            None
        }

        fn alert(&self, text: &str) {
            self.said.lock().unwrap().push(text.to_string());
        }
    }

    /// Where the text `said` was painted in `out`, the middle of it.
    fn at(out: &egui::FullOutput, said: &str) -> Option<egui::Pos2> {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == said => Some(t.pos + t.galley.rect.center().to_vec2()),
            _ => None,
        })
    }

    /// The texts painted in `out`.
    fn texts(out: &egui::FullOutput) -> Vec<String> {
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// A pass of a window of 800 by 600 holding only the bar, `events` done
    /// in it: what it painted, the item picked, and the events left to the
    /// rest of the window, which is the editor's.
    fn pass(ctx: &egui::Context, menu: &Menu, bar: &mut Bar, events: Vec<egui::Event>) -> (egui::FullOutput, Option<Item>, Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let (mut picked, mut left) = (None, Vec::new());
        let out = ctx.run(input, |ctx| {
            picked = menu.bar(ctx, bar);
            left = ctx.input(|i| i.events.clone());
            if let Some(item) = picked.clone() {
                menu.act(ctx, item);
            }
        });
        (out, picked, left)
    }

    /// A click at `pos`: the mouse there, pressed and let go.
    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    /// `key` pressed with `modifiers`.
    fn press(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// The bar of a window set on the state file `state`, its dialogs
    /// answered by `answers`: and what it opens.
    fn menu(state: &std::path::Path, answers: Arc<Answers>) -> (Arc<Menu>, mpsc::Receiver<Opened>, Arc<AtomicUsize>) {
        let (to, from) = mpsc::channel::<Opened>();
        let woken = Arc::new(AtomicUsize::new(0));
        let woke = woken.clone();
        let wake = move || {
            woke.fetch_add(1, Ordering::Relaxed);
        };
        (Menu::new(Arc::new(File::new(to, Box::new(wake), state.to_path_buf())), answers), from, woken)
    }

    #[test]
    fn the_file_menu_is_the_only_one_and_has_no_shortcut_on_either_system() {
        assert_eq!(rows(), [("File", ["New…", "Open…", "Open Recent ⏵", "—", "Close Window", "Quit StackED"].map(String::from).to_vec())]);
        let tmp = tempfile::tempdir().unwrap();
        let (menu, _, _) = menu(&tmp.path().join("state.json"), Arc::default());
        for os in [OperatingSystem::Mac, OperatingSystem::Nix] {
            let ctx = egui::Context::default();
            ctx.set_fonts(super::super::fonts().0);
            ctx.set_os(os);
            let mut bar = Bar::default();
            // Closed: File, and no other menu.
            let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
            assert_eq!(texts(&out), ["File"], "{os:?}");
            // Open: its items, each alone on its row, with no shortcut beside
            // it.
            pass(&ctx, &menu, &mut bar, click(at(&out, "File").unwrap()));
            let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
            assert_eq!(texts(&out), ["File", "New…", "Open…", "Open Recent", "⏵", "Close Window", "Quit StackED"], "{os:?}");
        }
    }

    #[test]
    fn the_bar_s_items_do_what_they_say_with_the_mouse_and_it_takes_no_key() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        let (a, b) = (tmp.path().join("a.jsonl"), tmp.path().join("b.jsonl"));
        for db in [&a, &b] {
            crate::db::Db::create(db).unwrap();
        }
        crate::state::opened(&state, &a);
        let answers = Arc::new(Answers {
            databases: vec![b.clone()],
            ..Default::default()
        });
        let (menu, from, woken) = menu(&state, answers.clone());
        let opened = |n: usize| {
            let started = Instant::now();
            let mut got = Vec::new();
            while got.len() < n && started.elapsed() < Duration::from_secs(20) {
                got.extend(from.recv_timeout(Duration::from_millis(100)).ok().map(|o| o.path));
            }
            got
        };
        let idle = || {
            let started = Instant::now();
            while menu.busy() && started.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!menu.busy());
        };
        let ctx = egui::Context::default();
        ctx.set_os(OperatingSystem::Nix);
        let mut bar = Bar::default();
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        assert_eq!(texts(&out), ["File"]);
        // No key is the bar's, on either system: ctrl-n, ctrl-o, ctrl-w and
        // ctrl-q, and ⌘ and those keys on macOS, are left as they came, for
        // the editor; nothing is picked, no dialog asked.
        for (os, modifiers) in [
            (OperatingSystem::Nix, Modifiers::CTRL | Modifiers::COMMAND),
            (OperatingSystem::Mac, Modifiers::CTRL),
            (OperatingSystem::Mac, Modifiers::MAC_CMD | Modifiers::COMMAND),
        ] {
            ctx.set_os(os);
            // Each pressed then let go, so that egui takes none for a repeat.
            let keys: Vec<egui::Event> = [Key::N, Key::O, Key::W, Key::Q]
                .into_iter()
                .flat_map(|k| {
                    let up = egui::Event::Key {
                        key: k,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers,
                    };
                    [press(k, modifiers), up]
                })
                .collect();
            let (out, picked, left) = pass(&ctx, &menu, &mut bar, keys.clone());
            assert_eq!((picked, left), (None, keys), "{os:?}");
            assert!(out.viewport_output[&egui::ViewportId::ROOT].commands.is_empty(), "{os:?}");
        }
        // Tab, then Enter and Space: none of its buttons takes the focus, nor
        // is clicked, as these keys are the editor's.
        ctx.set_os(OperatingSystem::Nix);
        for k in [Key::Tab, Key::Enter, Key::Space] {
            let (_, picked, _) = pass(&ctx, &menu, &mut bar, vec![press(k, Modifiers::NONE)]);
            assert_eq!((picked, ctx.memory(|m| m.focused())), (None, None), "{k:?}");
        }
        assert_eq!(answers.asked.load(Ordering::Relaxed), 0);
        // A click on File opens it; on Open…, the Open dialog is asked, on the
        // dialogs' thread: the database picked opened, and the window set
        // woken to take it.
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        pass(&ctx, &menu, &mut bar, click(at(&out, "File").unwrap()));
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        let (_, picked, _) = pass(&ctx, &menu, &mut bar, click(at(&out, "Open…").unwrap()));
        assert_eq!(picked, Some(Item::Open));
        assert_eq!(opened(1), std::slice::from_ref(&b));
        assert_eq!((woken.load(Ordering::Relaxed), answers.asked.load(Ordering::Relaxed)), (1, 1));
        idle();
        // Open Recent, read as the File menu opens: the database opened last,
        // then a, then Clear Menu; a, clicked, opened.
        crate::state::opened(&state, &b);
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        pass(&ctx, &menu, &mut bar, click(at(&out, "File").unwrap()));
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        let recent = at(&out, "Open Recent").unwrap();
        pass(&ctx, &menu, &mut bar, vec![egui::Event::PointerMoved(recent)]);
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![egui::Event::PointerMoved(recent + egui::vec2(1.0, 0.0))]);
        let shown = texts(&out);
        let (first, second, clear) = (
            shown.iter().position(|t| t == "b.jsonl"),
            shown.iter().position(|t| t == "a.jsonl"),
            shown.iter().position(|t| t == "Clear Menu"),
        );
        assert!(first < second && second < clear && first.is_some(), "{shown:?}");
        let (_, picked, _) = pass(&ctx, &menu, &mut bar, click(at(&out, "a.jsonl").unwrap()));
        assert_eq!(picked, Some(Item::Recent(a.clone())));
        assert_eq!(opened(1), std::slice::from_ref(&a));
        assert!(answers.said.lock().unwrap().is_empty());
        idle();
        // Close Window closes this window; Quit StackED, every window,
        // StackED quitting.
        let ctx = egui::Context::default();
        let mut bar = Bar::default();
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        pass(&ctx, &menu, &mut bar, click(at(&out, "File").unwrap()));
        let (out, _, _) = pass(&ctx, &menu, &mut bar, vec![]);
        let (out, picked, _) = pass(&ctx, &menu, &mut bar, click(at(&out, "Close Window").unwrap()));
        assert_eq!(picked, Some(Item::Close));
        assert!(out.viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::Close));
        assert!(!menu.quitting());
        menu.act(&ctx, Item::Quit);
        assert!(menu.quitting());
    }
}
