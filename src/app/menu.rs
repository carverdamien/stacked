//! StackED's File and Edit menus on macOS's menu bar, and the panels and
//! alerts the File menu shows: AppKit's, called through objc2. The File
//! menu's items have an object of StackED's own class (`Target`) as their
//! target, whose methods do what `File` says; Close Window goes to the
//! window in front, as its close button would. The Edit menu's items go to
//! what is in front too, a panel's text field, and have their shortcuts
//! only while a panel runs (`Shortcuts`).

use std::cell::{OnceCell, RefCell};
use std::ffi::CStr;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertStyle, NSApplication, NSMenu, NSMenuDelegate, NSMenuItem, NSMenuItemValidation, NSModalResponse, NSModalResponseOK,
    NSOpenPanel, NSSavePanel,
};
use objc2_foundation::{NSArray, NSObject, NSString, ns_string};

use super::windows::{Ask, File, titles};

/// What the menu's target holds: what the File menu's items do, the
/// databases Open Recent listed when last built, and the Edit menu's items,
/// whose shortcuts the panels set.
struct Ivars {
    file: File,
    recent: RefCell<Vec<PathBuf>>,
    edit: Vec<EditItem>,
}

define_class!(
    // SAFETY: NSObject has nothing a class of its must do, and `Target`
    // does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        /// File → New…
        #[unsafe(method(newDocument:))]
        fn new_document(&self, _sender: Option<&AnyObject>) {
            self.ivars().file.create(&Panels(self));
        }

        /// File → Open…
        #[unsafe(method(openDocument:))]
        fn open_document(&self, _sender: Option<&AnyObject>) {
            self.ivars().file.open(&Panels(self));
        }

        /// An item of File → Open Recent, its database's path its object.
        #[unsafe(method(openRecent:))]
        fn open_recent(&self, sender: Option<&NSMenuItem>) {
            let Some(path) = sender.and_then(|item| item.representedObject()) else { return };
            let Some(path) = path.downcast_ref::<NSString>() else { return };
            self.ivars().file.open_recent(Path::new(&path.to_string()), &Panels(self));
        }

        /// File → Open Recent → Clear Menu.
        #[unsafe(method(clearRecentDocuments:))]
        fn clear_recent(&self, _sender: Option<&AnyObject>) {
            self.ivars().file.clear();
        }

        /// `alert_later`'s alert, said.
        #[unsafe(method(sayLater:))]
        fn say_later(&self, text: &NSString) {
            alert(&text.to_string());
        }
    }

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSMenuDelegate for Target {
        /// Open Recent, as it opens: built again from the state kept.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.fill_recent(menu);
        }
    }

    unsafe impl NSMenuItemValidation for Target {
        /// Clear Menu, only with databases to clear; the rest always.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            item.action() != Some(sel!(clearRecentDocuments:)) || !self.ivars().recent.borrow().is_empty()
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, file: File, edit: Vec<EditItem>) -> Retained<Target> {
        let this = Target::alloc(mtm).set_ivars(Ivars {
            file,
            recent: RefCell::new(Vec::new()),
            edit,
        });
        // SAFETY: NSObject's init, as it is.
        unsafe { msg_send![super(this), init] }
    }

    /// `menu`, Open Recent, made of the recent databases, the most recent
    /// first, each titled by its file's name and holding its path; then
    /// Clear Menu.
    fn fill_recent(&self, menu: &NSMenu) {
        let mtm = self.mtm();
        let recent = self.ivars().file.recent();
        menu.removeAllItems();
        for (db, title) in recent.iter().zip(titles(&recent)) {
            let it = item(mtm, &title, Some(sel!(openRecent:)), "", Some(self));
            let path = NSString::from_str(&db.to_string_lossy());
            let object: &AnyObject = &path;
            // SAFETY: an NSString, which the item keeps.
            unsafe { it.setRepresentedObject(Some(object)) };
            it.setToolTip(Some(&path));
            menu.addItem(&it);
        }
        if !recent.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        menu.addItem(&item(mtm, "Clear Menu", Some(sel!(clearRecentDocuments:)), "", Some(self)));
        *self.ivars().recent.borrow_mut() = recent;
    }
}

thread_local! {
    /// The menu's target, kept for as long as StackED runs: an item does not
    /// keep its target.
    static TARGET: OnceCell<Retained<Target>> = const { OnceCell::new() };
}

/// An item of a menu titled `title`, sending `action` to `target` (else to
/// what is in front: the window, the text field being edited), with ⌘ and
/// `key` as its shortcut if `key` is not empty.
fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str, target: Option<&Target>) -> Retained<NSMenuItem> {
    // SAFETY: each action is a method of `Target`'s, `performClose:`,
    // NSWindow's, or one of `EDIT`'s, which AppKit's text fields and their
    // windows have; each takes the item that sends it.
    let it = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), action, &NSString::from_str(key)) };
    if let Some(target) = target {
        let target: &AnyObject = target;
        // SAFETY: the target is kept in `TARGET` for as long as StackED runs.
        unsafe { it.setTarget(Some(target)) };
    }
    it
}

/// An item of the Edit menu: its title, the action it sends to what is in
/// front (a panel's text field, or its window, which undoes), and the key
/// of its shortcut, with ⌘, and with ⇧ too if it is a capital.
struct Edit {
    title: &'static str,
    action: &'static CStr,
    key: &'static str,
}

/// The Edit menu: Undo and Redo, a separator (`None`), then Cut, Copy,
/// Paste and Select All.
const EDIT: [Option<Edit>; 7] = [
    Some(Edit { title: "Undo", action: c"undo:", key: "z" }),
    Some(Edit { title: "Redo", action: c"redo:", key: "Z" }),
    None,
    Some(Edit { title: "Cut", action: c"cut:", key: "x" }),
    Some(Edit { title: "Copy", action: c"copy:", key: "c" }),
    Some(Edit { title: "Paste", action: c"paste:", key: "v" }),
    Some(Edit { title: "Select All", action: c"selectAll:", key: "a" }),
];

/// An item of the Edit menu, with the key of its shortcut.
type EditItem = (Retained<NSMenuItem>, &'static str);

/// The Edit menu as `EDIT` has it, its items sending their actions to what
/// is in front, with no shortcut until a panel runs; and those items.
fn edit_menu(mtm: MainThreadMarker) -> (Retained<NSMenu>, Vec<EditItem>) {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Edit"));
    let mut items = Vec::new();
    for e in &EDIT {
        match e {
            Some(e) => {
                let it = item(mtm, e.title, Some(Sel::register(e.action)), "", None);
                menu.addItem(&it);
                items.push((it, e.key));
            }
            None => menu.addItem(&NSMenuItem::separatorItem(mtm)),
        }
    }
    (menu, items)
}

/// Puts StackED's File and Edit menus on the menu bar, after the
/// application menu winit makes. File: New… (⌘N), Open… (⌘O), Open Recent,
/// and Close Window (⌘W), which closes the window in front as its close
/// button does; what its items open goes to the window set as `file` says.
/// Edit: Undo, Redo, Cut, Copy, Paste and Select All, with their shortcuts
/// only while a panel runs. Once, as eframe starts, on the main thread.
pub fn install(file: File) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else { return };
    let (edit, edit_items) = edit_menu(mtm);
    let target = Target::new(mtm, file, edit_items);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("File"));
    menu.addItem(&item(mtm, "New…", Some(sel!(newDocument:)), "n", Some(&target)));
    menu.addItem(&item(mtm, "Open…", Some(sel!(openDocument:)), "o", Some(&target)));
    let recent = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Open Recent"));
    recent.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    target.fill_recent(&recent);
    let open_recent = item(mtm, "Open Recent", None, "", None);
    open_recent.setSubmenu(Some(&recent));
    menu.addItem(&open_recent);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&item(mtm, "Close Window", Some(sel!(performClose:)), "w", None));
    let file_item = item(mtm, "File", None, "", None);
    file_item.setSubmenu(Some(&menu));
    bar.insertItem_atIndex(&file_item, 1);
    let edit_item = item(mtm, "Edit", None, "", None);
    edit_item.setSubmenu(Some(&edit));
    bar.insertItem_atIndex(&edit_item, 2);
    TARGET.with(|t| {
        let _ = t.set(target);
    });
}

/// AppKit's panels and alerts, as the File menu asks for them, from the
/// menu's target, which holds the Edit menu's items: a panel has their
/// shortcuts while it runs.
struct Panels<'a>(&'a Target);

impl Panels<'_> {
    /// `panel` run until it is closed, its text fields given the Edit
    /// menu's shortcuts meanwhile.
    fn run(&self, panel: &NSSavePanel) -> NSModalResponse {
        let _shortcuts = Shortcuts::set(&self.0.ivars().edit);
        panel.runModal()
    }
}

impl Ask for Panels<'_> {
    fn databases(&self) -> Vec<PathBuf> {
        let panel = NSOpenPanel::openPanel(self.0.mtm());
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(true);
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&jsonl()));
        panel.setMessage(Some(ns_string!("Pick stacked databases (.jsonl files): each opens in a window of its own.")));
        if self.run(&panel) != NSModalResponseOK {
            return Vec::new();
        }
        panel.URLs().iter().filter_map(|url| url.to_file_path()).collect()
    }

    fn new_database(&self) -> Option<PathBuf> {
        let panel = NSSavePanel::savePanel(self.0.mtm());
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&jsonl()));
        panel.setCanCreateDirectories(true);
        panel.setNameFieldStringValue(ns_string!("stacked.jsonl"));
        panel.setPrompt(Some(ns_string!("Create")));
        panel.setMessage(Some(ns_string!("Name the new database: an empty one is made there. StackED never replaces a file.")));
        if self.run(&panel) != NSModalResponseOK {
            return None;
        }
        panel.URL().and_then(|url| url.to_file_path())
    }

    fn alert(&self, text: &str) {
        alert(text);
    }
}

/// The Edit menu's shortcuts (⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V, ⌘A), set for as long as
/// this is kept. AppKit gives these keys to a text field only through items
/// of the menu bar that have them. The menu bar takes the keys of its items
/// even when they are greyed out, though, which would leave StackED's
/// windows without ⌘C and ⌘V: the shortcuts are cleared once this is
/// dropped, whatever happened meanwhile.
struct Shortcuts<'a>(&'a [EditItem]);

impl<'a> Shortcuts<'a> {
    /// Each of `items` given its shortcut.
    fn set(items: &'a [EditItem]) -> Shortcuts<'a> {
        for (it, key) in items {
            it.setKeyEquivalent(&NSString::from_str(key));
        }
        Shortcuts(items)
    }
}

impl Drop for Shortcuts<'_> {
    fn drop(&mut self) {
        for (it, _) in self.0 {
            it.setKeyEquivalent(ns_string!(""));
        }
    }
}

/// What the panels let be picked and named: .jsonl files.
fn jsonl() -> Retained<NSArray<NSString>> {
    NSArray::from_retained_slice(&[NSString::from_str("jsonl")])
}

/// `text` said in an alert of StackED's, StackED brought forward for it: on
/// stderr if this is not the main thread, which alone may show one.
pub fn alert(text: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "stacked: {text}");
        return;
    };
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    let alert = NSAlert::new(mtm);
    alert.setMessageText(ns_string!("StackED"));
    alert.setInformativeText(&NSString::from_str(text));
    alert.setAlertStyle(NSAlertStyle::Critical);
    alert.runModal();
}

/// `text` said in an alert once AppKit next waits for events: from within
/// eframe's passes, where none may be shown. On stderr if the File menu is
/// not there to say it.
pub fn alert_later(text: &str) {
    TARGET.with(|t| match t.get() {
        Some(target) => {
            let text = NSString::from_str(text);
            // SAFETY: `sayLater:` is `Target`'s, and takes an NSString.
            let _: () = unsafe { msg_send![&**target, performSelector: sel!(sayLater:), withObject: &*text, afterDelay: 0.0f64] };
        }
        None => {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "stacked: {text}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;
    use objc2::runtime::{AnyClass, AnyProtocol};

    #[test]
    fn the_menu_s_target_is_a_class_appkit_can_send_its_items_to() {
        // Made as the Objective-C runtime has it, which checks its methods
        // against the protocols': each item's action is a method of its,
        // and so is what its menu and items ask it.
        let class = Target::class();
        for action in [
            sel!(newDocument:),
            sel!(openDocument:),
            sel!(openRecent:),
            sel!(clearRecentDocuments:),
            sel!(sayLater:),
            sel!(menuNeedsUpdate:),
            sel!(validateMenuItem:),
        ] {
            assert!(class.instance_method(action).is_some(), "{action:?}");
        }
        let delegate = AnyProtocol::get(c"NSMenuDelegate").expect("AppKit's");
        assert!(class.conforms_to(delegate));
    }

    #[test]
    fn the_edit_menu_has_a_mac_app_s_items_shortcuts_and_actions() {
        // As the menu shows them while a panel runs: ⌘ and the key, and ⇧
        // too for a capital, which AppKit reads as ⇧ and the letter.
        let shown: Vec<String> = EDIT
            .iter()
            .map(|e| match e {
                Some(e) if e.key.chars().all(|c| c.is_ascii_uppercase()) => format!("{} ⇧⌘{}", e.title, e.key),
                Some(e) => format!("{} ⌘{}", e.title, e.key.to_uppercase()),
                None => "—".into(),
            })
            .collect();
        assert_eq!(shown, ["Undo ⌘Z", "Redo ⇧⌘Z", "—", "Cut ⌘X", "Copy ⌘C", "Paste ⌘V", "Select All ⌘A"]);
        // Each action is AppKit's: the text view that edits a field has it,
        // or the field's window, which undoes.
        let text = AnyClass::get(c"NSTextView").expect("AppKit's");
        let window = AnyClass::get(c"NSWindow").expect("AppKit's");
        for e in EDIT.iter().flatten() {
            let action = Sel::register(e.action);
            assert!(text.instance_method(action).is_some() || window.instance_method(action).is_some(), "{action:?}");
        }
    }
}
