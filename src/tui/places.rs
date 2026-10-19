//! Where the cursor was in each pane, kept in memory while the editor is
//! open, by what the pane showed: the patches of each stack, the middle and
//! the comments of the stack and of each patch (whichever its version), the
//! history of each stack, the runs, each transcript and each review. Coming
//! back to one, the cursor is where it was: on what it was on (a line, a
//! comment, a patch, a record, a run), found again, else at the same place.

use std::collections::HashMap;

use super::*;

/// Where the middle and the comments pane were on what they showed.
#[derive(Debug, Clone, Default)]
struct PanesAt {
    /// The middle's cursor, and what its row is (a line's key, a comment's
    /// id and turn), to find it again; how far down the middle it was.
    cursor: usize,
    row: (Option<String>, Option<String>, Option<usize>),
    down: usize,
    /// The comments pane's: on its header, or on a comment.
    notes_head: bool,
    note_sel: usize,
    note: Option<String>,
}

/// Where a list's cursor was: its line, and what is on it; how far down
/// the list it was; in the history, the stack diff's cursor beside it.
#[derive(Debug, Clone)]
struct ListAt {
    pick: usize,
    id: Option<String>,
    down: usize,
    hist_focus: HistFocus,
    hist_cursor: usize,
    hist_top: usize,
}

/// Where the cursor was in each pane.
#[derive(Debug, Default)]
pub struct Places {
    /// The patches pane, by stack: on its header (None), or on a patch, by
    /// its change.
    list: HashMap<String, Option<String>>,
    /// The middle and the comments pane, by what they show: the stack
    /// (`stack:NAME`), or a patch, by its change.
    panes: HashMap<String, PanesAt>,
    /// The history, by stack; the runs.
    lists: HashMap<String, ListAt>,
    /// How far up each run's transcript was scrolled.
    transcripts: HashMap<String, usize>,
    /// The line of each run's review.
    reviews: HashMap<String, usize>,
}

impl App {
    /// What the panes' place on `subject` is kept under: the stack's own,
    /// or the patch's change, the same for each of its versions.
    fn place_of(&self, subject: &str) -> Option<String> {
        match subject {
            "" => None,
            s if s.starts_with("stack:") => Some(s.to_string()),
            id => Some(self.db.patch(id).change.clone()),
        }
    }

    /// Where the middle and the comments pane are on `subject`, what their
    /// rows show, kept: they are to show something else.
    pub(super) fn keep_panes(&mut self, subject: &str) {
        let Some(place) = self.place_of(subject) else { return };
        let Some(r) = self.rows.get(self.cursor) else { return };
        let notes = self.notes();
        let at = PanesAt {
            cursor: self.cursor,
            row: (r.key.clone(), r.note.clone(), r.turn),
            down: self.cursor.saturating_sub(self.top),
            notes_head: self.notes_head,
            note_sel: self.note_sel,
            note: notes.get(self.note_sel).map(|(_, id)| id.clone()),
        };
        self.places.panes.insert(place, at);
    }

    /// The middle and the comments pane on `subject` again, its rows built:
    /// their cursors where they were, if they were on it before.
    pub(super) fn back_to_panes(&mut self, subject: &str) {
        let Some(at) = self.place_of(subject).and_then(|p| self.places.panes.get(&p).cloned()) else { return };
        let known = at.row.0.is_some() || at.row.1.is_some();
        let found = known.then(|| self.rows.iter().position(|r| (r.key.clone(), r.note.clone(), r.turn) == at.row)).flatten();
        self.cursor = found.unwrap_or(at.cursor).min(self.rows.len().saturating_sub(1));
        self.top = self.cursor.saturating_sub(at.down);
        let notes = self.notes();
        self.notes_head = at.notes_head;
        let note = at.note.as_ref().and_then(|id| notes.iter().position(|(_, n)| n == id));
        self.note_sel = note.unwrap_or(at.note_sel).min(notes.len().saturating_sub(1));
    }

    /// Where the patches pane is on this stack, kept: another is to be the
    /// current one.
    pub(super) fn keep_list_of_patches(&mut self) {
        let on = if self.on_stack { None } else { self.patch_id().map(|id| self.db.patch(&id).change.clone()) };
        self.places.list.insert(self.stack.clone(), on);
    }

    /// The patches pane on this stack again: where it was, if it was on it
    /// before; else on the first patch.
    pub(super) fn back_to_list_of_patches(&mut self) {
        let at = match self.places.list.get(&self.stack) {
            Some(None) => None,
            Some(Some(change)) => self.current().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == *change)),
            None => Some(0),
        };
        match at {
            Some(i) if i < self.patch_count() => {
                self.sel = i;
                self.on_stack = false;
            }
            _ => {
                self.sel = 0;
                self.on_stack = true;
            }
        }
    }

    /// What the list of this screen keeps its place under, if it does: the
    /// history, by stack; the runs. (The tree, the stacks and the versions
    /// open on where the stack is.)
    fn list_place(&self) -> Option<String> {
        match self.screen {
            Screen::History => Some(format!("history:{}", self.stack)),
            Screen::Runs => Some("runs".into()),
            _ => None,
        }
    }

    /// What each line of this screen's list is, to find it again.
    fn list_ids(&self) -> Vec<String> {
        match self.screen {
            Screen::History => self.history().iter().map(|e| e.rev.to_string()).collect(),
            Screen::Runs => self.runs_newest().iter().map(|r| r.id.clone()).collect(),
            _ => vec![],
        }
    }

    /// Where the cursor is in the list of this screen, kept, as it is drawn.
    pub(super) fn keep_list(&mut self) {
        let Some(place) = self.list_place() else { return };
        let at = ListAt {
            pick: self.pick,
            id: self.list_ids().get(self.pick).cloned(),
            down: self.pick.saturating_sub(self.pick_top),
            hist_focus: self.hist_focus,
            hist_cursor: self.hist_cursor,
            hist_top: self.hist_top,
        };
        self.places.lists.insert(place, at);
    }

    /// The list of this screen, opened again: the cursor where it was; in
    /// the history, the stack diff too, if it is of the same record.
    pub(super) fn back_to_list(&mut self) {
        let Some(at) = self.list_place().and_then(|p| self.places.lists.get(&p).cloned()) else { return };
        let ids = self.list_ids();
        let found = at.id.as_ref().and_then(|id| ids.iter().position(|i| i == id));
        self.pick = found.unwrap_or(at.pick).min(ids.len().saturating_sub(1));
        self.pick_top = self.pick.saturating_sub(at.down);
        if self.screen == Screen::History {
            self.hist_rev = self.history().get(self.pick).map(|e| e.rev);
            if found.is_some() {
                self.hist_focus = at.hist_focus;
                self.hist_cursor = at.hist_cursor;
                self.hist_top = at.hist_top;
            }
        }
    }

    /// How far up run `id`'s transcript was, kept; and as it was.
    pub(super) fn keep_transcript(&mut self) {
        if let Some(id) = &self.transcript {
            self.places.transcripts.insert(id.clone(), self.scroll);
        }
    }

    pub(super) fn transcript_was(&self, id: &str) -> usize {
        self.places.transcripts.get(id).copied().unwrap_or(0)
    }

    /// The line of the review being left, kept; and as it was, for run
    /// `run`'s.
    pub(super) fn keep_review(&mut self) {
        if let Some(rv) = &self.reviewing {
            self.places.reviews.insert(rv.run.clone(), rv.sel);
        }
    }

    pub(super) fn review_was(&self, run: &str) -> Option<usize> {
        self.places.reviews.get(run).copied()
    }
}
