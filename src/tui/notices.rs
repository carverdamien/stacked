//! The notices of `stacked web`: what the editor learns as it reads the
//! database again (`App::reload`), of every stack, not only the one shown,
//! sent to its pages, which show each as a banner of the system's while the
//! page is not what you look at (web/index.html). The work that lands and
//! waits for your review: an ask's change or answer, a follow-up's, come
//! into the review of the work it follows up, a lint job's, a reconcile
//! with HEAD, a pull from Phabricator made; a run that fails, but one you
//! stopped or dropped yourself; and a try push followed that is over, done
//! on Treeherder, with how many of its tasks failed, or not pushed to try.
//! Each is of a run that was going when the database was read before, and
//! landed since: what was there before is never told. Its title names its
//! stack, and its body says what it is in a sentence, in the words of the
//! banner and of the bottom line. They are kept only once `stacked web`
//! asks for them (`keep_notices`), as `web.notifications` says; a click on
//! one in the page goes to what it tells (`noticed`).

use super::*;
use crate::db::Run;

/// What the editor learned, told: the run it is of, its title, and its
/// body.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub run: String,
    pub title: String,
    pub body: String,
}

/// How long a notice's body is at most, in characters: cut there, as a
/// banner shows only a few lines.
const BODY: usize = 300;

/// `said` as a sentence: its first letter a capital, a full stop after it
/// unless it ends with one, `?`, `!` or `…`; in the place of a colon, a
/// semicolon or a comma it ends with, as a summary does with nothing said
/// after its colon.
fn sentence(said: &str) -> String {
    let said = said.trim_end_matches([' ', ':', ';', ',']);
    let mut chars = said.chars();
    let mut out: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    if !out.ends_with(['.', '?', '!', '…']) {
        out.push('.');
    }
    out
}

/// What landed work `r`, waiting for your review, is, as the banner counts
/// it; and, an ask's, how it was asked, as the banner says it after what it
/// holds: `a landed ask` and `, asked at max: "Why add b?"`; `a lint of the
/// stack`, and nothing more, which its kind says.
fn landed_said(r: &Run) -> (&'static str, String) {
    if crate::held::pulled(r) {
        return ("a pull from Phabricator", String::new());
    }
    if crate::held::by_hand(r) {
        return ("an edit by hand", String::new());
    }
    if crate::held::linted(r) {
        return ("a lint of the stack", String::new());
    }
    if crate::reconcile::is(r) {
        return ("a reconcile with HEAD", String::new());
    }
    ("a landed ask", banner::landed_how(r, 60))
}

/// Why run `r` failed, as the bottom line says it: the agent's ask, the
/// linter's; and a reconcile with HEAD's and a pull from Phabricator's,
/// which the bottom line says by their summary alone, named.
fn failed_said(r: &Run) -> String {
    if crate::held::linted(r) {
        return format!("the linter could not lint the stack {}: {}", r.stack, r.summary);
    }
    if crate::reconcile::is(r) {
        return format!("the reconcile with HEAD failed: {}", r.summary);
    }
    if crate::held::pulled(r) {
        return format!("the pull from Phabricator failed: {}", r.summary);
    }
    let at = crate::thread::asked_at(ask::effort_said(r));
    format!("the agent's {}{at} failed: {}", r.what, r.summary)
}

/// Whether run `r` ended as you ended it, in this editor or in any other:
/// an agent stopped, or dropped (asked again, as the work it went on from
/// was rejected, sent back or edited by hand; stopped as you accepted an
/// edit, or as you closed or sent back the review of the work it follows
/// up); the linter stopped; a lint job, a reconcile with HEAD or a pull
/// from Phabricator dropped before its turn; a try push stopped before mach
/// said it pushed.
fn ended_by_you(r: &Run) -> bool {
    r.summary.starts_with(ask::STOPPED_SAID)
        || ask::info(r).is_some_and(|i| i.dropped)
        || [crate::lint::STOPPED, crate::lint::DROPPED, crate::reconcile::DROPPED].contains(&r.summary.as_str())
        || pull_dropped(r)
        || push_stopped(r)
}

/// Whether pull `r` is one you dropped before its turn: never, built
/// without the `moz-phab` feature.
fn pull_dropped(r: &Run) -> bool {
    #[cfg(feature = "moz-phab")]
    return r.summary == crate::review::DROPPED;
    #[cfg(not(feature = "moz-phab"))]
    return {
        let _ = r;
        false
    };
}

/// Whether try push `r` is one you stopped before mach said it pushed:
/// never, built without the `try-push` feature.
fn push_stopped(r: &Run) -> bool {
    #[cfg(feature = "try-push")]
    return r.summary == crate::try_push::STOPPED;
    #[cfg(not(feature = "try-push"))]
    return {
        let _ = r;
        false
    };
}

/// The work that follow-up `r`, landed, came into the review of, by its
/// run, if `r` is a follow-up and that work still waits for your review.
fn followed_up(db: &Db, r: &Run) -> Option<String> {
    let on = ask::info(r).filter(|i| i.follow)?.on?;
    db.run(&on).filter(|w| crate::held::waiting(db, w)).map(|w| w.id.clone())
}

impl App {
    /// From now on, what the editor learns as it reads the database again
    /// is kept as notices (`notice_landed`), for `stacked web` to send its
    /// pages: only once it asks, so that the terminal's editor and StackED's
    /// keep none.
    pub fn keep_notices(&mut self) {
        self.notices.get_or_insert_with(Vec::new);
    }

    /// The notices kept since they were last taken, taken.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        self.notices.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Each of `was`, the runs going when the database was read before, that
    /// landed since and tells something, kept as a notice, if notices are
    /// kept: not one this editor stops, which you stopped here.
    pub(super) fn notice_landed(&mut self, was: &[String]) {
        let Some(mut kept) = self.notices.take() else { return };
        for id in was.iter().filter(|id| !self.bg.stopping.contains(id)) {
            let Some(r) = self.db.run(id).filter(|r| r.state != RunState::Started) else { continue };
            if let Some(said) = self.notice_said(r) {
                kept.push(Notice {
                    run: r.id.clone(),
                    title: format!("stacked: {}", r.stack),
                    body: sentence(&short(&said, BODY)),
                });
            }
        }
        self.notices = Some(kept);
    }

    /// What landed run `r` tells, as its notice says it: the work it brought
    /// back, waiting for your review, with what it holds; why it failed; for
    /// a follow-up, what came into the review of the work it follows up; for
    /// a try push, what it came to. None for a run that tells nothing so:
    /// one you stopped or dropped, one that brought nothing to review.
    fn notice_said(&self, r: &Run) -> Option<String> {
        if r.what == crate::db::TRY_PUSH {
            return self.try_push_said(r);
        }
        let waiting = crate::held::waiting(&self.db, r);
        if r.state == RunState::Failed {
            if ended_by_you(r) {
                return None;
            }
            // Failed, it waits for your review all the same when it holds
            // a tool the agent could not use.
            let failed = failed_said(r);
            return Some(if waiting { format!("{failed}. Its work waits for your review") } else { failed });
        }
        if waiting {
            let (what, how) = landed_said(r);
            // Gone on from an edit under review, guessing: reviewed after it.
            let after = if ask::guessing(&self.db, r).is_some() { " after an edit under review" } else { "" };
            return Some(format!("{what} waits for your review{after}: {}{how}", crate::held::left(&self.db, r)));
        }
        followed_up(&self.db, r).map(|_| format!("a follow-up landed: {}", r.summary))
    }

    /// What try push `r`, landed, tells: over and waiting for your review,
    /// done on Treeherder or not pushed to try; failed, why it was not made,
    /// but you stopped it. None built without the `try-push` feature, which
    /// says nothing of a try push.
    fn try_push_said(&self, r: &Run) -> Option<String> {
        #[cfg(feature = "try-push")]
        return match r.state {
            RunState::Failed => (!ended_by_you(r)).then(|| format!("the try push was not made: {}", r.summary)),
            _ => crate::try_push::over_said(r)
                .filter(|_| crate::held::waiting(&self.db, r))
                .map(|said| format!("{said}. Its summary waits for your review")),
        };
        #[cfg(not(feature = "try-push"))]
        return {
            let _ = r;
            None
        };
    }

    /// The notice of run `run` clicked in a page of `stacked web`, which the
    /// click brought forward: on the main screen with nothing open on it,
    /// the run's stack made the current one, then the review of what it
    /// brought back opened, as Enter on its line of the banner opens it (for
    /// a follow-up, the review of the work it follows up), if that still
    /// waits for your review; else what came of it said in the bottom line,
    /// as its notice said it. On any other screen, or with a menu, a dialog
    /// or a comment being written open, nothing is done: what is open is
    /// never left for a notice.
    pub fn noticed(&mut self, run: &str) {
        if self.screen != Screen::Main || !matches!(self.mode, Mode::Normal) {
            return;
        }
        let Some(r) = self.db.run(run).cloned() else { return };
        if self.db.stack(&r.stack).is_err() {
            return;
        }
        if r.stack != self.stack {
            self.switch_stack(&r.stack);
        }
        let review = Some(r.id.clone()).filter(|_| crate::held::waiting(&self.db, &r)).or_else(|| followed_up(&self.db, &r));
        match review {
            Some(id) => self.open_review(&id, None),
            None => {
                if let Some(said) = self.notice_said(&r) {
                    self.status = said;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_s_body_is_a_sentence() {
        assert_eq!(sentence("the agent's answer failed: it ran out"), "The agent's answer failed: it ran out.");
        assert_eq!(sentence("done."), "Done.");
        assert_eq!(sentence("éclat?"), "Éclat?");
        assert_eq!(sentence("cut short…"), "Cut short…");
        // Nothing said after a summary's colon: a full stop in its place.
        assert_eq!(sentence("the agent's change failed: the agent failed (1): "), "The agent's change failed: the agent failed (1).");
    }
}
