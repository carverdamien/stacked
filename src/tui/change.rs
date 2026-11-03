//! The change from one version of a patch to another as a patch of its own
//! (`crate::change`), as the screens of interdiffs draw it, in the views of
//! the code that `1` picks there: unified, side by side, or side by side and
//! compact; each file whole with `3`, as in the middle; the lines the change
//! of base made dim. What a screen knows of each version's base, the change
//! read from them away from the screen (`App::changes`), and the line of
//! the interdiff nearest each line of the change, for a screen to keep its
//! place from one view to the other (`Near`).

use super::interdiffs::{Stop, Two, cut, half_of, paired, said_stop};
use super::*;
use crate::change::{Change, Side};
use crate::ops::{CodePlace, InterLine};

/// The change of a patch as a screen has it (`App::changed`).
#[derive(Debug, Clone)]
pub(super) enum Changed {
    /// The bases of its versions cannot be read: why.
    Cannot(String),
    /// Its files are being read.
    Reading,
    Read(Arc<Change>),
    /// Reading its files failed: why.
    Failed(String),
}

/// A version of a patch on its base, as a screen knows it: the repository
/// and the commit its base is, the ids of the patches below it, bottom
/// first, and its diff, as patch `id` has it, or `raw`, a diff of its own,
/// known by `id`, as a revision's latest on Phabricator.
#[derive(Debug, Clone)]
pub(super) struct On {
    pub repo: String,
    pub base: String,
    pub below: Vec<String>,
    pub id: String,
    pub raw: Option<String>,
}

impl On {
    /// What it is known by, among the changes read.
    fn key(&self) -> String {
        format!("{}\0{}\0{}\0{}", self.repo, self.base, self.below.join(","), self.id)
    }
}

/// The view of the code a view of the change is: unified, side by side, or
/// side by side and compact.
pub(super) fn code_view(v: InterLayout) -> Layout {
    match v {
        InterLayout::ChangeSides => Layout::Split,
        InterLayout::ChangeCompact => Layout::Compact,
        _ => Layout::Unified,
    }
}

/// What the views of the change say once, at the top of the code, of how
/// to read it, of versions `two`, drawn in view `how` of the code: what
/// each sign or side is, and that the messages are compared over the code;
/// and, the two on other bases (`rebased`), what is dim.
pub(super) fn change_said(two: &Two, how: Layout, rebased: bool) -> String {
    let Two { old, new, .. } = two;
    let mut said = match how {
        Layout::Unified => format!(
            "The code is the change from {old} to {new} as a patch of its own, each file as {old} leaves it on its base, then as {new} leaves it on its own: - for a line only {old} has, + for one only {new} has."
        ),
        Layout::Compact => format!(
            "The code is the change from {old} to {new} as a patch of its own, each file as {old} leaves it on its base on the left, and as {new} leaves it on its own on the right, each side's lines one after another."
        ),
        _ => format!(
            "The code is the change from {old} to {new} as a patch of its own, each file as {old} leaves it on its base on the left, and as {new} leaves it on its own on the right."
        ),
    };
    said += &format!(" Where the messages differ, their lines are over the code, - for those of {old}, + for those of {new}.");
    if rebased {
        said += &format!(
            " The two are on other bases: a line drawn dim is one the change of base made, not either patch, one taken out that the base of {old} has and the base of {new} has not, or one put in that only the base of {new} has. A file only the change of base touches is left out."
        );
    }
    said
}

/// The rows of change `c`, as the middle draws a patch's files, folded `w`
/// wide: each file's name, its hunks, or each file whole (`whole`), what
/// changed in a line marked. Drawn only.
fn change_rows(c: &Change, w: usize, whole: bool) -> Vec<Row> {
    let p = Patch::new("change".into(), None, None, String::new(), c.diff.clone());
    let deco = Deco { whole: whole.then(|| Ok(c.files.clone())), ..Default::default() };
    let all = patch_rows(&p, &[], w, &deco);
    // Its message's rows, empty, left out.
    let from = all.iter().position(|r| r.kind == Kind::Blank).map_or(0, |k| k + 1);
    all.into_iter().skip(from).collect()
}

/// The lines of an interdiff, by the file and the line of each version's
/// code each is (`ops::code_places`), each by its place among what a screen
/// draws, and whether its version takes it out: the one nearest a line of
/// the change is found among them (`find`). A line of a file the interdiff
/// has no line of, `none`.
#[derive(Debug, Default)]
pub(super) struct Near {
    sides: [Placed; 2],
    none: usize,
}

/// The lines of an interdiff of one version's code, by file: each its line
/// of the file, whether the version takes it out, and its place among what
/// is drawn (`Near`).
type Placed = HashMap<String, Vec<(usize, bool, usize)>>;

impl Near {
    /// Of an interdiff's lines from its first hunk's header, at `places`
    /// in each version's code, the first of them at `first` among what is
    /// drawn.
    pub(super) fn new(places: &[[Option<CodePlace>; 2]], first: usize, none: usize) -> Near {
        let mut out = Near { none, ..Default::default() };
        for (k, at) in places.iter().enumerate() {
            for (side, place) in at.iter().enumerate() {
                if let Some(p) = place {
                    out.sides[side].entry(p.path.clone()).or_default().push((p.line, p.gone, first + k));
                }
            }
        }
        out
    }

    /// The line of the interdiff nearest line `lines[0]` of file `path` as
    /// the old version leaves it, or `lines[1]` as the new one does: the
    /// one at it, else the nearest, the first of those as near; one the
    /// version keeps before one it takes out there.
    pub(super) fn find(&self, path: &str, lines: [Option<usize>; 2]) -> usize {
        let mut best: Option<(usize, bool, usize)> = None;
        for (side, line) in lines.iter().enumerate() {
            let (Some(n), Some(all)) = (line, self.sides[side].get(path)) else { continue };
            for &(l, gone, at) in all {
                let d = (l.abs_diff(*n), gone, at);
                if best.is_none_or(|b| d < b) {
                    best = Some(d);
                }
            }
        }
        best.map_or(self.none, |(.., at)| at)
    }
}

/// Where row `r` of a change is in each version's files: its line as the
/// old version leaves it, and as the new one does; none on a side it is no
/// line of. A line of a hunk by the line of the change's diff it is,
/// `lines` (its sign and numbers); a hunk's header at its first line; a
/// line in no hunk, of a file shown whole, at its line in the new one; a
/// file's name, and its header, at the top.
fn row_place(r: &Row, lines: &HashMap<usize, (char, usize, usize)>) -> [Option<usize>; 2] {
    match (r.kind, r.diff_at) {
        (Kind::Ctx, _) if r.outside => [None, r.new_no],
        (Kind::Hunk, Some(at)) => lines.get(&(at + 1)).map_or([Some(0), Some(0)], |&(_, o, n)| [Some(o), Some(n)]),
        (Kind::Ctx | Kind::Add | Kind::Del, Some(at)) => match lines.get(&at) {
            Some(&('-', o, _)) => [Some(o), None],
            Some(&('+', _, n)) => [None, Some(n)],
            Some(&(_, o, n)) => [Some(o), Some(n)],
            None => [Some(0), Some(0)],
        },
        _ => [Some(0), Some(0)],
    }
}

/// Change `c` of the patch at `slot` among what is drawn, as stops of the
/// cursor, in view `how` of the code, `w` wide after `indent` columns:
/// unified, a row each; side by side, its rows paired as the code view
/// pairs a patch's (`interdiffs::paired`), `~` dim where a side has
/// nothing; compact, as the code view's compact view draws a patch, the
/// two sides scrolled together: the old side's rows in the left column and
/// the new side's in the right, each side's one after another, with no gap
/// where the other has more (`panes::pane_streams`), a row of both sides
/// (a file's name, a hunk's header) across both while it is on the same
/// line of each. Each file whole with `whole`. A line the change of base
/// made dim (`Change::dim`), over its own colours. Each stop shows the line
/// of the interdiff nearest it (`near`), and the lines of the change it
/// shows (`Stop::rows`).
pub(super) fn change_stops(c: &Change, how: Layout, whole: bool, w: usize, indent: usize, slot: usize, near: &Near) -> Vec<Stop> {
    let block = w.saturating_sub(indent);
    let half = half_of(block);
    let fold = match how {
        Layout::Unified => block,
        _ => half,
    }
    .saturating_sub(1);
    let rows = change_rows(c, fold, whole);
    let mut lines: HashMap<usize, (char, usize, usize)> = HashMap::new();
    for fd in crate::diff::parse_diff(&c.diff, "") {
        for l in fd.hunks.iter().flat_map(|h| &h.lines) {
            lines.insert(l.at, (l.origin, l.old_no, l.new_no));
        }
    }
    // Each row: as the middle draws it, dim where the change of base made
    // it; the line of the interdiff nearest it; the line of the change it
    // is, when it is one of its diff.
    let (mut cells, mut nears, mut keys) = (Vec::new(), Vec::new(), Vec::new());
    let mut path = String::new();
    for r in &rows {
        if let Some(p) = &r.path {
            path.clone_from(p);
        }
        let dim = r.diff_at.is_some_and(|at| c.dim.contains(&at)) && matches!(r.kind, Kind::Add | Kind::Del);
        let parts = row_parts(r, false);
        cells.push(match dim {
            true => parts.into_iter().map(|(t, st)| (t, st.add_modifier(Modifier::DIM))).collect::<Parts>(),
            false => parts,
        });
        nears.push(near.find(&path, row_place(r, &lines)));
        keys.push(r.diff_at.filter(|_| matches!(r.kind, Kind::Add | Kind::Del | Kind::Ctx) && !r.outside).map(|at| (slot, at)));
    }
    let lead = || vec![(" ".repeat(indent), Style::new())];
    let changed = |i: usize| matches!(rows[i].kind, Kind::Add | Kind::Del);
    let stop_of = |at: &[usize], lines: Vec<Parts>| -> Stop {
        let mut shows: Vec<usize> = at.iter().map(|&i| nears[i]).collect();
        shows.sort_unstable();
        shows.dedup();
        Stop {
            lines,
            shows,
            rows: at.iter().filter_map(|&i| keys[i]).collect(),
            slot: Some(slot),
            changed: at.iter().any(|&i| changed(i)),
        }
    };
    let mut out: Vec<Stop> = Vec::new();
    match how {
        Layout::Split => {
            for (parts, at, first) in paired(&rows, &cells, block, indent) {
                match out.last_mut() {
                    Some(s) if !first => {
                        let more = stop_of(&at, vec![parts]);
                        s.lines.extend(more.lines);
                        s.shows.extend(more.shows);
                        s.shows.sort_unstable();
                        s.shows.dedup();
                        s.rows.extend(more.rows.into_iter().filter(|r| !s.rows.contains(r)).collect::<Vec<_>>());
                        s.changed |= more.changed;
                    }
                    _ => out.push(stop_of(&at, vec![parts])),
                }
            }
        }
        Layout::Compact => {
            let p = crate::panes::pane_streams(&rows);
            for y in 0..p.left.len().max(p.right.len()) {
                let (l, r) = (p.left.get(y).copied(), p.right.get(y).copied());
                let mut parts = lead();
                let at: Vec<usize> = l.into_iter().chain(r).collect();
                match (l, r) {
                    (Some(i), Some(j)) if i == j && crate::panes::spans(rows[i].kind) => parts.extend(cut(&cells[i], block).0),
                    _ => {
                        for (k, side) in [l, r].into_iter().enumerate() {
                            if k == 1 {
                                parts.push((" │".into(), color::meta()));
                            }
                            let (cell, n) = side.map_or((vec![], 0), |i| cut(&cells[i], half));
                            parts.extend(cell);
                            parts.push((" ".repeat(half - n), Style::new()));
                        }
                    }
                }
                let mut at = at;
                at.dedup();
                out.push(stop_of(&at, vec![parts]));
            }
        }
        _ => {
            for (i, cell) in cells.iter().enumerate() {
                let mut parts = lead();
                parts.extend(cut(cell, block).0);
                match out.last_mut() {
                    Some(s) if rows[i].cont => s.lines.push(parts),
                    _ => out.push(stop_of(&[i], vec![parts])),
                }
            }
        }
    }
    out
}

/// The lines of interdiff `lines` that compare its versions' mails, as the
/// interdiff draws them (`inter_parts`), after `indent` columns: those from
/// its first hunk's header to its code, the two lines naming the versions
/// left out, when one of them differs; none else. Each a stop showing its
/// line, by its place among what is drawn from `first`.
pub(super) fn message_stops(lines: &[&InterLine], first: usize, indent: usize) -> Vec<Stop> {
    let names = lines.iter().take_while(|l| l.text.starts_with("--- ") || l.text.starts_with("+++ ")).count();
    let mail: Vec<&InterLine> = lines[names..].iter().take_while(|l| !l.code()).copied().collect();
    if !mail.iter().any(|l| l.text.starts_with(['-', '+'])) {
        return vec![];
    }
    inter_parts(&mail)
        .into_iter()
        .enumerate()
        .map(|(k, parts)| {
            let mut drawn = vec![(" ".repeat(indent), Style::new())];
            drawn.extend(parts);
            Stop::new(vec![drawn], vec![first + names + k])
        })
        .collect()
}

/// What stands in the place of a change not drawn, `why`, after `indent`
/// columns, `w` wide: a stop that shows no line.
fn instead(why: &str, w: usize, indent: usize) -> Stop {
    said_stop(why, w.saturating_sub(indent), indent)
}

/// The code of one patch in a view of the change, as `changed` has it,
/// `lines` its interdiff from its first hunk's header, the first of them at
/// `first` among what is drawn, `slot` its patch's place there: the change
/// (`change_stops`), or, two versions that leave each file the same, that
/// said; while it is read, that said; where it cannot be read, why, and the
/// interdiff after, as an interdiff (`as_interdiff`).
pub(super) fn code_stops(
    changed: &Changed,
    how: Layout,
    (near, slot): (&Near, usize),
    whole: bool,
    (w, indent): (usize, usize),
    as_interdiff: impl FnOnce() -> Vec<Stop>,
) -> Vec<Stop> {
    match changed {
        Changed::Read(c) if c.diff.is_empty() => vec![instead("The two versions leave each file they touch the same.", w, indent)],
        Changed::Read(c) => change_stops(c, how, whole, w, indent, slot, near),
        Changed::Reading => vec![instead("Reading the files of both versions…", w, indent)],
        Changed::Cannot(why) | Changed::Failed(why) => {
            let mut out = vec![instead(&format!("The change cannot be drawn: {why}. Here is the interdiff instead."), w, indent)];
            out.extend(as_interdiff());
            out
        }
    }
}

/// One interdiff's stops in view `v` of the change, `lines` as `ops` gives
/// them, the two lines naming the versions first, of versions `two`, `w`
/// wide with `indent` columns before each line: what says how to read it
/// (`change_said`), the lines comparing the messages (`message_stops`),
/// then its code (`code_stops`), as an interdiff where it cannot be drawn.
/// Each stop with the lines of `lines` it shows.
pub(super) fn one_change_stops(lines: &[&InterLine], v: InterLayout, two: &Two, (changed, near): (&Changed, &Near), whole: bool, w: usize, indent: usize) -> Vec<Stop> {
    let how = code_view(v);
    let rebased = matches!(changed, Changed::Read(c) if c.rebased);
    let mut out = vec![said_stop(&change_said(two, how, rebased), w.saturating_sub(indent), indent)];
    // Where it is drawn, the messages over it; else the interdiff has them.
    if matches!(changed, Changed::Read(_) | Changed::Reading) {
        out.extend(message_stops(lines, 0, indent));
    }
    out.extend(code_stops(changed, how, (near, 0), whole, (w, indent), || super::interdiffs::interdiff_stops(lines, InterLayout::Interdiff, two, w, indent)));
    out
}

impl App {
    /// Commit `named` of the repository at `repo`, its files to be read as
    /// the check reads them (`check::reachable`): as git names it, else, a
    /// Mercurial changeset as Phabricator names one, as git-cinnabar has it
    /// (`git cinnabar hg2git`, as moz-phab maps them); asked once. Why not,
    /// said of `name`, the version on it.
    pub(super) fn commit_named(&self, repo: &str, named: &str, name: &str) -> std::result::Result<String, String> {
        let key = (repo.to_string(), named.to_string());
        let known = self.bases_read.borrow().get(&key).cloned();
        let read = known.unwrap_or_else(|| {
            let at = Path::new(repo);
            let r = match crate::check::reachable(at, named) {
                Ok(()) => Ok(named.to_string()),
                Err(e) => crate::git::git(at, &["cinnabar", "hg2git", named])
                    .ok()
                    .map(|sha| sha.trim().to_string())
                    .filter(|sha| !sha.is_empty() && sha.bytes().any(|b| b != b'0') && crate::check::reachable(at, sha).is_ok())
                    .ok_or(e.0),
            };
            self.bases_read.borrow_mut().insert(key, r.clone());
            r
        });
        read.map_err(|e| format!("{name}'s {}", e.strip_prefix("its ").unwrap_or(&e)))
    }

    /// Patch `at` (from 0) of stack `s`, named `name`, on its base: the
    /// stack's repository and base commit, the patches below it. Why not,
    /// when the stack has no base, or its base cannot be read.
    pub(super) fn on_stack(&self, s: &Stack, at: usize, name: &str) -> std::result::Result<On, String> {
        let (Some(repo), Some(base)) = (&s.repo, &s.base) else {
            return Err(format!("{name} has no base commit to read its files on"));
        };
        let base = self.commit_named(repo, base, name)?;
        let id = s.patches.get(at).ok_or_else(|| format!("{name} has no patch {}", at + 1))?;
        Ok(On { repo: repo.clone(), base, below: s.patches[..at].to_vec(), id: id.clone(), raw: None })
    }

    /// Version `p` of a patch, named `name`, on the base of the first stack
    /// that held it (`Db::first_use`), as the versions of a patch say each
    /// came in. Why not, when no stack held it.
    pub(super) fn on_first(&self, p: &Patch, name: &str) -> std::result::Result<On, String> {
        let (_, s) = self.db.first_use(&p.id).ok_or_else(|| format!("{name} was never in a stack, so it has no base to read its files on"))?;
        let at = s.patches.iter().position(|id| *id == p.id).unwrap_or_default();
        self.on_stack(s, at, name)
    }

    /// The change from version `old` to `new`, named `names`: read, being
    /// read, away from the screen (`change::Reader`), or why it cannot be.
    pub(super) fn changed(&self, old: std::result::Result<On, String>, new: std::result::Result<On, String>, names: [&str; 2]) -> Changed {
        let (old, new) = match (old, new) {
            (Err(why), _) | (_, Err(why)) => return Changed::Cannot(why),
            (Ok(o), Ok(n)) => (o, n),
        };
        let key = format!("{}\0\0{}", old.key(), new.key());
        let side = |on: &On| Side {
            repo: PathBuf::from(&on.repo),
            base: on.base.clone(),
            below: on.below.iter().map(|id| self.db.patch(id).diff.clone()).collect(),
            diff: on.raw.clone().unwrap_or_else(|| self.db.patch(&on.id).diff.clone()),
        };
        match self.changes.get(&key, || (side(&old), side(&new), names.map(String::from))) {
            None => Changed::Reading,
            Some(Ok(c)) => Changed::Read(c),
            Some(Err(why)) => Changed::Failed(why),
        }
    }

    /// The change from version `old` to version `new` of a patch, as the
    /// versions of a patch list them, each on the stack that first held it
    /// (`on_first`), named `names`; with the lines of their interdiff,
    /// `lines`, by their places among them, nearest each line of it.
    pub(super) fn versions_change(&self, old: &Patch, new: &Patch, lines: &[&InterLine], names: [&str; 2]) -> (Changed, Near) {
        let near = Near::new(&crate::ops::code_places(lines, old, new), 0, 0);
        (self.changed(self.on_first(old, names[0]), self.on_first(new, names[1]), names), near)
    }

    /// Of stack diff `diff`, from stack `before` to `after`, its versions
    /// named `names`: the change of each patch told with what changed in it,
    /// by its line's place (`changed`), with the lines of its interdiff, by
    /// their places, nearest each line of it (`Near`). None while the view
    /// drawn is not one of the change.
    pub(super) fn stack_changes(&self, diff: &[Diffed], (before, after): (Option<&Stack>, Option<&Stack>), names: [&str; 2], v: InterLayout) -> HashMap<usize, (Changed, Near)> {
        let mut out = HashMap::new();
        let (Some(before), Some(after), true) = (before, after, v.change()) else { return out };
        for (k, d) in diff.iter().enumerate() {
            let Diffed::Patch { at, changed: true, .. } = d else { continue };
            let Some(new_id) = after.patches.get(at.wrapping_sub(1)) else { continue };
            let new = self.db.patch(new_id);
            let Some(i) = before.patches.iter().position(|id| self.db.patch(id).change == new.change) else { continue };
            let old = self.db.patch(&before.patches[i]);
            let code: Vec<&InterLine> = diff[k + 1..].iter().map_while(|d| if let Diffed::Code(l) = d { Some(l) } else { None }).collect();
            let near = Near::new(&crate::ops::code_places(&code, old, new), k + 1, k);
            let changed = self.changed(self.on_stack(before, i, names[0]), self.on_stack(after, at - 1, names[1]), names);
            out.insert(k, (changed, near));
        }
        out
    }

    /// Why the change of the patches a screen shows cannot be drawn there,
    /// as their versions' bases are, the stacks `stacks` holding them, each
    /// with its name: none when every base can be read.
    pub(super) fn stacks_why(&self, stacks: &[(Option<&Stack>, &str)]) -> Option<String> {
        stacks.iter().find_map(|(s, name)| {
            let s = (*s)?;
            match (&s.repo, &s.base) {
                (Some(repo), Some(base)) => self.commit_named(repo, base, name).err(),
                _ => Some(format!("{name} has no base commit to read its files on")),
            }
        })
    }

    /// Whether the screen draws its interdiffs in a view of the change, as
    /// last drawn.
    pub(super) fn change_drawn(&self) -> bool {
        self.inter_screen() && self.inter_in(self.inter_w.get()).change()
    }

    /// 3, while the screen draws its interdiffs in a view of the change:
    /// each file of the change whole, or as its hunks again, as 3 shows a
    /// patch's in the middle. The screen keeps its place.
    pub(super) fn toggle_change_whole(&mut self) {
        let kept = self.keep_inter_place();
        self.change_whole = !self.change_whole;
        self.find_inter_place(kept);
        self.status = match self.change_whole {
            true => "whole files: each file of the change is shown whole".into(),
            false => "hunks: each file of the change is shown as its hunks".into(),
        };
    }
}
