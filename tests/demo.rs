//! `stacked demo`, done for the reader step by step, as Tab does it: each
//! step done, the tour over, nothing written on disk; any section started
//! with the game as the sections before it leave it; and the steps done by
//! hand, as the tour says them.
#![cfg(feature = "demo")]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use stacked::demo::{SECTIONS, STEPS, Step, Tour, place};
use stacked::thread::{Role, last_role};
use stacked::tui::{App, Focus, Mode};

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The tour drawn on a terminal of 120 columns and 40 lines.
fn screen(app: &mut App, tour: &Tour) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    tour.draw(app, &mut buf);
    text(&buf)
}

/// A patch's one hunk, a side of it: before (`-`) or after (`+`), its
/// context with it.
fn side(diff: &str, sign: char) -> String {
    diff.lines()
        .skip_while(|l| !l.starts_with("@@"))
        .skip(1)
        .filter(|l| l.starts_with(' ') || l.starts_with(sign))
        .map(|l| format!("{}\n", &l[1..]))
        .collect()
}

/// What there is of stacked's in the temporary directory.
fn temporary() -> BTreeSet<String> {
    std::fs::read_dir(std::env::temp_dir())
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains("stacked"))
                .collect()
        })
        .unwrap_or_default()
}

fn titles(app: &App) -> Vec<String> {
    let s = app.db.stack("game").unwrap();
    s.patches.iter().map(|p| app.db.patch(p).title().to_string()).collect()
}

/// The step done as Tab does it, until the tour goes on: a step that
/// opens a screen, done again, leaves it.
fn tab(app: &mut App, tour: &mut Tour) {
    let was = tour.index();
    tour.show(app);
    let started = Instant::now();
    while tour.index() == was && !tour.over() {
        assert!(started.elapsed() < Duration::from_secs(20), "{:?} never done", tour.step());
        std::thread::sleep(Duration::from_millis(5));
        app.refresh();
        tour.tick(app);
        if tour.index() == was && !tour.over() && !tour.waiting(app) {
            tour.show(app);
        }
    }
}

/// Until the tour is at `step`, or `what` is so: refreshed, ticked.
fn until(app: &mut App, tour: &mut Tour, what: &str, mut cond: impl FnMut(&App, &Tour) -> bool) {
    let started = Instant::now();
    while !cond(app, tour) {
        assert!(started.elapsed() < Duration::from_secs(20), "never {what}, at {:?}", tour.step());
        std::thread::sleep(Duration::from_millis(5));
        app.refresh();
        tour.tick(app);
    }
}

fn press(app: &mut App, code: KeyCode) {
    let _ = app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Space, unless a menu is open, then the menu's item whose label starts
/// with `label`: ↓ to it, and Enter.
fn choose(app: &mut App, label: &str) {
    if !matches!(app.mode, Mode::Menu(_)) {
        press(app, KeyCode::Char(' '));
    }
    let all: Vec<String> = match &app.mode {
        Mode::Menu(m) => m.items.iter().map(|i| i.label.clone()).collect(),
        _ => vec![],
    };
    let want = all.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("no {label:?} in {all:?}"));
    for _ in 0..all.len() * 2 {
        match &app.mode {
            Mode::Menu(m) if m.sel == want => break,
            _ => press(app, KeyCode::Down),
        }
    }
    press(app, KeyCode::Enter);
}

#[test]
fn the_tour_is_done_step_by_step_in_memory() {
    let before = temporary();
    let (mut app, mut tour) = Tour::new().unwrap();
    tour.quick(true);
    let (mut said, mut texts) = (Vec::new(), Vec::new());
    while !tour.over() {
        let k = tour.index();
        let shown = screen(&mut app, &tour);
        assert!(shown.contains(&format!("stacked demo · step {} of {STEPS}", k + 1)), "{shown}");
        texts.push(tour.say(&app).0);
        said.push(shown);
        // Nothing goes on by itself: the step waits for the reader.
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(10));
            app.refresh();
            tour.tick(&mut app);
        }
        assert_eq!(tour.index(), k, "{:?} went on by itself", tour.step());
        tab(&mut app, &mut tour);
    }
    assert_eq!(said.len(), STEPS);
    // A step names what it is about: the step before it, which did, is
    // gone from the screen.
    let at = |s: Step| &texts[place(s)];
    assert!(at(Step::Read).contains("to read patch 3, X takes the top right corner"), "{}", at(Step::Read));
    assert!(at(Step::Ask).starts_with("Ask the agent to fix O's mistake in patch 4"), "{}", at(Step::Ask));
    assert!(at(Step::Question).contains("write \"Who is winning?\""), "{}", at(Step::Question));
    // The moves played; O's mistake, and X's win off it, made again by the
    // stand-in, which said so in the thread; a question answered; patch 5
    // dropped, then back; and O's next move, asked on the stack.
    let d = &app.db;
    assert_eq!(
        titles(&app),
        [
            "X takes the center",
            "O takes the top left corner",
            "X takes the top right corner",
            "O blocks the diagonal at the bottom left",
            "X blocks O's left column",
            "O blocks X's middle row",
        ]
    );
    assert!(d.log(Some("game")).iter().any(|(_, r)| matches!(r, stacked::db::Record::Head(h) if h.op.starts_with("undo drop"))));
    let block = d.stack("game").unwrap().patches[3].clone();
    let note = d.notes(&d.patch(&block).change).into_iter().next().cloned().unwrap();
    assert_eq!(last_role(&note.text), Some(Role::Agent), "{}", note.text);
    assert!(note.text.contains("Patch 4: O takes the bottom left corner") && note.text.contains("Patch 5 changes with it"), "{}", note.text);
    assert!(note.text.contains("Who is winning?") && note.text.contains("X is ahead"), "{}", note.text);
    // Patch 1 read, and back to commenting.
    assert!(!app.reading);
    // Each patch is the whole board, before its move and after it: the
    // moves under it are seen with it, from the board the one under it
    // leaves. At the tour's end, the board as its patches leave it.
    let mut board = stacked::demo::board(&['.'; 9]);
    for p in &d.stack("game").unwrap().patches {
        let diff = &d.patch(p).diff;
        assert_eq!(side(diff, '-'), board, "{diff}");
        board = side(diff, '+');
    }
    let end = said.last().unwrap();
    for row in ["  O  |  .  |  X", "  X  |  X  |  O", "  O  |  .  |  ."] {
        assert!(board.contains(&format!("\n{row}\n")), "{board}");
        assert!(end.contains(row), "{end}");
    }
    // Nothing on disk: not the database, nor anything of stacked's made
    // in the temporary directory.
    assert!(!std::path::Path::new("/nonexistent").exists());
    assert_eq!(temporary(), before);
}

#[test]
fn each_section_starts_with_the_game_the_sections_before_it_leave() {
    for s in &SECTIONS {
        let (mut app, mut tour) = Tour::new().unwrap();
        tour.forward(&mut app, place(s.first)).unwrap();
        assert_eq!(tour.step(), s.first, "{}", s.title);
        // There is something to do there: the step is not done already.
        assert!(!tour.done(&app), "{}: done already", s.title);
        let n = titles(&app).len();
        let fixed = titles(&app).iter().any(|t| t == "O blocks the diagonal at the bottom left");
        match s.first {
            Step::Welcome => assert_eq!(n, 0),
            Step::Comment | Step::Ask => assert!(n == 5 && !fixed, "{}", s.title),
            Step::Runs | Step::Help | Step::End => assert_eq!(n, 6, "{}", s.title),
            _ => assert!(n == 5 && fixed, "{}: {:?}", s.title, titles(&app)),
        }
        // Its title, and the step's words, on the screen.
        let shown = screen(&mut app, &tour);
        assert!(shown.contains(s.title), "{shown}");
    }
}

#[test]
fn opened_before_the_tour_starts_on_its_sections_and_goes_to_any() {
    let (mut app, mut tour) = Tour::new().unwrap();
    tour.offer_sections();
    assert_eq!(tour.menu(), Some(0));
    let shown = screen(&mut app, &tour);
    for s in &SECTIONS {
        assert!(shown.contains(s.title), "{}:\n{shown}", s.title);
    }
    assert!(shown.contains("Esc: start at the beginning"), "{shown}");
    // ↓ to the move's section, and Enter: a new game, played up to there.
    let moving = SECTIONS.iter().position(|s| s.title == "Learn how to move a patch in the stack").unwrap();
    for _ in 0..moving {
        tour.menu_key(&mut app, KeyCode::Down).unwrap();
    }
    tour.menu_key(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(tour.menu(), None);
    assert_eq!(tour.step(), Step::Move);
    assert!(titles(&app).iter().any(|t| t == "X blocks O's left column"), "{:?}", titles(&app));
    let shown = screen(&mut app, &tour);
    assert!(shown.contains("the sections before it done for you") && shown.contains("shift-Tab: sections"), "{shown}");
    // Over the tour, the list goes back to where it was with Esc.
    tour.menu_key(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(tour.step(), Step::Move);
}

#[test]
fn the_tour_says_the_agent_works_while_it_does() {
    let (mut app, mut tour) = Tour::new().unwrap();
    tour.forward(&mut app, place(Step::Ask)).unwrap();
    // Asked at the stand-in's own pace: the step says what to watch.
    tour.show(&mut app);
    until(&mut app, &mut tour, "with the agent", |app, tour| tour.say(app).0.starts_with("The agent is working on your comment"));
    assert_eq!(tour.step(), Step::Ask);
    until(&mut app, &mut tour, "fixed", |_, tour| tour.step() == Step::Question);
}

#[test]
fn asked_about_patch_4_alone_the_agent_sends_the_comment_to_the_stack() {
    let (mut app, mut tour) = Tour::new().unwrap();
    // Up to the ask, the comment written on O's mistake.
    tour.forward(&mut app, place(Step::Ask)).unwrap();
    tour.quick(true);
    let at = |app: &App, title: &str| titles(app).iter().position(|t| t == title).unwrap();
    let mistake = app.db.stack("game").unwrap().patches[at(&app, "O takes the bottom right corner")].clone();
    let change = app.db.patch(&mistake).change.clone();
    let note = app.db.notes(&change).into_iter().next().cloned().unwrap();
    // Asked to change patch 4 alone: it needs more than the patch, patch 5
    // too; the comment goes to the stack, nothing changed.
    stacked::ask::queue(&app.path, "game", &[(note.change.clone(), note.id.clone())], stacked::db::Way::Change, false).unwrap();
    app.launch();
    until(&mut app, &mut tour, "moved to the stack", |app, _| {
        app.db.stack_notes("game").into_iter().any(|n| n.moved.is_some() && last_role(&n.text) == Some(Role::Agent))
    });
    let moved = app.db.stack_notes("game").into_iter().find(|n| n.moved.is_some()).cloned().unwrap();
    assert!(moved.text.contains("This needs more than the patch: if O takes the bottom left corner in patch 4"), "{}", moved.text);
    assert!(app.db.notes(&change).is_empty(), "gone from patch 4");
    assert_eq!(app.db.stack("game").unwrap().patches[3], mistake, "nothing changed");
    assert_eq!(tour.step(), Step::Ask, "still to ask");
    assert!(screen(&mut app, &tour).contains("You asked the agent about patch 4 alone"));
    // Asked again there, on the stack: patches 4 and 5 made again; and the
    // question is asked in that thread.
    tab(&mut app, &mut tour);
    at(&app, "O blocks the diagonal at the bottom left");
    at(&app, "X blocks O's left column");
    assert!(tour.say(&app).0.contains("among the stack's own comments"), "{}", tour.say(&app).0);
    tab(&mut app, &mut tour);
    assert_eq!(tour.step(), Step::Reading);
}

#[test]
fn a_patch_goes_nowhere_but_where_it_was_played() {
    let (mut app, mut tour) = Tour::new().unwrap();
    // The five moves played.
    tour.forward(&mut app, place(Step::Pick)).unwrap();
    let played = app.db.stack("game").unwrap().patches.clone();
    // Each the board the patches under it leave: anywhere else, it does
    // not apply, and the moves stay in turn.
    for from in 0..5 {
        for to in (0..5).filter(|&to| to != from) {
            let moved = stacked::db::Db::edit(&app.path, |db| stacked::ops::move_patch(db, "game", from, to));
            assert!(moved.is_err(), "patch {} moved to {}", from + 1, to + 1);
        }
    }
    app.refresh();
    assert_eq!(app.db.stack("game").unwrap().patches, played);
}

#[test]
fn the_views_say_how_to_widen_a_middle_too_narrow_for_two_sides() {
    let (mut app, mut tour) = Tour::new().unwrap();
    tour.forward(&mut app, place(Step::Views)).unwrap();
    // On patch 4, its comments beside it: 120 columns leave too narrow a
    // middle for two sides, and the step says how to widen it.
    app.on_stack = false;
    app.sel = 3;
    app.focus = Focus::Diff;
    for _ in 0..2 {
        screen(&mut app, &tour);
    }
    assert!(tour.say(&app).0.contains("needs a wider middle than this one"), "{}", tour.say(&app).0);
    // ← to the patches and ← again: hidden, the middle has the room.
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    for _ in 0..2 {
        screen(&mut app, &tour);
    }
    assert!(!tour.say(&app).0.contains("wider middle"), "{}", tour.say(&app).0);
    // Through the three views, and back to unified: done.
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('1'));
        screen(&mut app, &tour);
        app.refresh();
        tour.tick(&mut app);
    }
    assert_eq!(tour.step(), Step::History);
}

#[test]
fn by_hand_a_refused_move_a_drop_and_its_undo() {
    let (mut app, mut tour) = Tour::new().unwrap();
    tour.forward(&mut app, place(Step::Move)).unwrap();
    // As the step says, from patch 4, where its versions left the cursor:
    // ↓ to patch 5, Move this patch…, ↑ to the top, Enter. Refused, and
    // patch 5 is still being moved.
    assert!(app.focus == Focus::List && !app.on_stack && app.sel == 3, "not on patch 4");
    press(&mut app, KeyCode::Down);
    assert_eq!(app.sel, 4);
    choose(&mut app, "Move this patch");
    for _ in 0..4 {
        press(&mut app, KeyCode::Up);
    }
    press(&mut app, KeyCode::Enter);
    app.refresh();
    tour.tick(&mut app);
    assert!(matches!(app.mode, Mode::Moving { .. }));
    assert_eq!(tour.step(), Step::Move, "the step waits for Esc");
    // The tour says how to stop moving it; the status line says why it was
    // refused, and the keys that go on from there.
    assert!(tour.say(&app).0.contains("press Esc to stop moving it"), "{}", tour.say(&app).0);
    let shown = screen(&mut app, &tour);
    assert!(shown.contains("does not apply") && shown.contains("esc: leave it where it was"), "{shown}");
    // Esc: patch 5 where it was, and the tour goes on, to the drop.
    press(&mut app, KeyCode::Esc);
    app.refresh();
    tour.tick(&mut app);
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(tour.step(), Step::Drop);
    assert_eq!(titles(&app)[4], "X blocks O's left column");
    // Patch 5 dropped, after stacked asks; then Undo brings it back.
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    app.refresh();
    tour.tick(&mut app);
    assert_eq!(titles(&app).len(), 4);
    assert_eq!(tour.step(), Step::Undo);
    choose(&mut app, "Undo: drop");
    app.refresh();
    tour.tick(&mut app);
    assert_eq!(titles(&app)[4], "X blocks O's left column");
    assert_eq!(tour.step(), Step::Tree);
}
