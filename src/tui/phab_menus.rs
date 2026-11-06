//! The Phabricator submenu, made in one place so that it is the same
//! wherever it is: the stack's menu and each patch's end with its entry, a
//! line then `Phabricator` (`App::phab_entry`), which opens it
//! (`App::phab_menu`); the menu of the Phabricator line over a patch's
//! message holds its items directly (`App::phab_items`). Its title says
//! what it is of: `D123 on Phabricator` for a patch's revision,
//! `Phabricator` for a patch that names none yet, `the stack NAME on
//! Phabricator` for the stack (`App::phab_title`). Its items come in one
//! order, each where it applies: for a patch naming its revision, Open D123
//! in your browser, then the submit, what differs, the pull, the
//! drop of the pull waiting its turn, and Fetch the latest from
//! Phabricator. Its note says why the stack is read-only first, then why
//! each grayed item is. None of it built without the `moz-phab` feature,
//! nor while reviews come from nowhere: those gates are here alone.

use super::*;

/// The label of the entry that opens a Phabricator submenu, in the stack's
/// menu and in each patch's.
#[cfg(feature = "moz-phab")]
pub(super) const PHABRICATOR: &str = "Phabricator";

/// What a Phabricator submenu is of: the stack, or the patch the middle
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhabOf {
    Stack,
    Patch,
}

/// A Phabricator submenu's pieces, before they are a menu: its title, its
/// items in their order, and why those grayed are.
#[cfg_attr(not(feature = "moz-phab"), allow(dead_code))]
pub(super) struct PhabItems {
    pub title: String,
    pub items: Vec<Item>,
    pub why: Vec<String>,
}

impl App {
    /// The entry of a menu that opens the Phabricator submenu `of`: a line,
    /// then `Phabricator`. Nothing built without the `moz-phab` feature, nor
    /// while reviews come from nowhere.
    pub(super) fn phab_entry(&self, items: &mut Vec<Item>, of: PhabOf) {
        #[cfg(feature = "moz-phab")]
        if self.cfg.reviews.source != crate::config::Source::None {
            items.push(Item::line());
            items.push(Item::new(PHABRICATOR, Action::PhabMenu(of)));
        }
        #[cfg(not(feature = "moz-phab"))]
        let _ = (self, items, of);
    }

    /// The items of the Phabricator submenu `of`, in their order, under its
    /// title, with why those grayed are: for the stack, its pull, the drop of
    /// its pull waiting its turn, and the fetch; for the patch the middle
    /// shows, the opening of its revision in your browser when it names one
    /// (`Action::OpenLink`, as a link of its message opens), its submit (held back by HEAD, or not), what differs from its
    /// revision, the pull of that revision, the drop of the pull waiting its
    /// turn when it is that revision's, and the fetch. None built without
    /// the `moz-phab` feature, nor while reviews come from nowhere.
    pub(super) fn phab_items(&self, of: PhabOf) -> Option<PhabItems> {
        #[cfg(feature = "moz-phab")]
        if self.cfg.reviews.source != crate::config::Source::None {
            let mut items = Vec::new();
            let mut why = Vec::new();
            match of {
                PhabOf::Stack => {
                    items.push(Item::new(revisions::PULL, Action::Pull));
                    items.extend(self.pull_drop_item());
                }
                PhabOf::Patch => {
                    if let Some((d, link)) = self.shown_patch().and_then(|p| crate::phab::revision_of(&p.message)) {
                        items.push(Item::new(format!("Open {d} in your browser"), Action::OpenLink(link)));
                    }
                    let held = self.submit_held(self.sel);
                    let (item, submit_why) = self.submit_item(self.read_only().is_none() && held.is_none());
                    items.push(item);
                    why.extend(held.into_iter().chain(submit_why));
                    why.extend(self.revision_items(&mut items));
                    if let Some(p) = self.shown_patch() {
                        items.extend(self.pull_drop_item_of(&p.change));
                    }
                }
            }
            items.push(Item::new(revisions::FETCH, Action::Fetch));
            return Some(PhabItems { title: self.phab_title(of), items, why });
        }
        let _ = of;
        None
    }

    /// The title of the Phabricator submenu `of`, and of the Phabricator
    /// line's menu: `the stack NAME on Phabricator`; for a patch, `D123 on
    /// Phabricator`, its revision's, or `Phabricator` while it names none.
    #[cfg(feature = "moz-phab")]
    pub(super) fn phab_title(&self, of: PhabOf) -> String {
        let on = match of {
            PhabOf::Stack => Some(format!("the stack {}", self.stack)),
            PhabOf::Patch => self.shown_patch().and_then(|p| crate::phab::revision_of(&p.message)).map(|(d, _)| d),
        };
        on.map_or_else(|| PHABRICATOR.into(), |on| format!("{on} on {PHABRICATOR}"))
    }

    /// The Phabricator submenu `of`, which its entry opens: its items
    /// (`phab_items`) under its title, and why each grayed item is under
    /// them, the stack being read-only first. Nothing to say, no note.
    #[cfg(feature = "moz-phab")]
    pub(super) fn phab_menu(&self, of: PhabOf) -> Menu {
        let PhabItems { title, items, why: grayed } = self.phab_items(of).unwrap_or_else(|| PhabItems {
            title: self.phab_title(of),
            items: Vec::new(),
            why: Vec::new(),
        });
        let mut why: Vec<String> = self.read_only().map(String::from).into_iter().collect();
        why.extend(grayed);
        Menu::new(title, items, (!why.is_empty()).then(|| why.join("\n")))
    }
}
