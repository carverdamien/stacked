//! The system's dialogs, as StackED's File menu asks for them: the Open and
//! Save dialogs, and alerts. They are rfd's (`System`): on macOS, AppKit's
//! Open and Save panels and the system's alerts; elsewhere, the desktop
//! portal's file chooser, over D-Bus (its library loaded as StackED runs,
//! nothing of GTK's built in), else zenity's, and zenity's alerts. Each is
//! asked on a thread of its own (`Dialogs`), one at a time, so that the
//! windows go on meanwhile; the tests answer them with stand-ins of their
//! own, through `Ask`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};

use super::windows::{Ask, File};

/// What the Open dialog says, over the databases to pick.
const OPEN_SAYS: &str = "Pick stacked databases (.jsonl files): each opens in a window of its own.";

/// What the Save dialog of File → New… says, over the name to give.
const NEW_SAYS: &str = "Name the new database: an empty one is made there. StackED never replaces a file.";

/// The system's own dialogs, rfd's.
pub struct System;

/// The dialog's filter: stacked's databases, .jsonl files.
fn databases() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("stacked databases", &["jsonl"])
}

impl Ask for System {
    fn databases(&self) -> Vec<PathBuf> {
        let dialog = databases().set_title(OPEN_SAYS);
        crate::launcher::with_edit_keys(move || dialog.pick_files()).unwrap_or_default()
    }

    fn new_database(&self) -> Option<PathBuf> {
        let dialog = databases().set_file_name("stacked.jsonl").set_can_create_directories(true).set_title(NEW_SAYS);
        crate::launcher::with_edit_keys(move || dialog.save_file())
    }

    /// `text` said in an alert of the system's, and on stderr too, for
    /// where no alert can be shown (no zenity on a Linux desktop).
    fn alert(&self, text: &str) {
        alert(text);
    }
}

/// `text` said in an alert of the system's, titled StackED, and on stderr.
pub fn alert(text: &str) {
    // Not eprintln!, which panics with nowhere to say it.
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "stacked: {text}");
    let shown = rfd::AsyncMessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("StackED")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    pollster::block_on(shown);
}

/// What the File menu asks of the dialogs, on their thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// File → New…
    New,
    /// File → Open…
    Open,
    /// An item of File → Open Recent.
    Recent(PathBuf),
    /// Something said in an alert.
    Alert(String),
}

/// The thread the dialogs are asked on, and what the File menu does with
/// what they answer (`File`): one of the menu's items at a time, while the
/// windows go on; the alerts in turn, after those asked before.
pub struct Dialogs {
    to: Sender<Job>,
    /// One of the menu's items runs: its dialog is open, or what it opens
    /// is opening.
    busy: Arc<AtomicBool>,
}

impl Dialogs {
    /// The dialogs' thread, asking `ask` and doing what `file` does with its
    /// answers.
    pub fn start(file: Arc<File>, ask: Arc<dyn Ask + Send + Sync>) -> Dialogs {
        let (to, jobs) = mpsc::channel::<Job>();
        let busy = Arc::new(AtomicBool::new(false));
        let done = busy.clone();
        let _ = std::thread::Builder::new().name("StackED's dialogs".into()).spawn(move || {
            for job in jobs {
                match &job {
                    Job::New => file.create(&*ask),
                    Job::Open => file.open(&*ask),
                    Job::Recent(db) => file.open_recent(db, &*ask),
                    Job::Alert(text) => ask.alert(text),
                }
                if !matches!(job, Job::Alert(_)) {
                    done.store(false, Ordering::Release);
                }
            }
        });
        Dialogs { to, busy }
    }

    /// `job` asked on the dialogs' thread: an item of the menu's, unless one
    /// runs already, which leaves it undone; an alert, always. Whether it is
    /// asked.
    pub fn ask(&self, job: Job) -> bool {
        if !matches!(job, Job::Alert(_)) && self.busy.swap(true, Ordering::AcqRel) {
            return false;
        }
        let item = !matches!(job, Job::Alert(_));
        let sent = self.to.send(job).is_ok();
        if !sent && item {
            self.busy.store(false, Ordering::Release);
        }
        sent
    }

    /// Whether one of the menu's items runs: its dialog open, or what it
    /// opens opening.
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}
