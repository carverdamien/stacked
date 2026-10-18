//! `stacked demo`, done for the reader step by step, as Tab does it: each
//! step done, the tour over, nothing written on disk.
#![cfg(feature = "demo")]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use stacked::demo::{STEPS, Tour};
use stacked::tui::{App, Focus, Mode};
use stacked::thread::{Role, last_role};

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
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

#[test]
fn the_tour_is_done_step_by_step_in_memory() {
    let before = temporary();
    let (mut app, mut tour) = Tour::new().unwrap();
    let (mut said, mut texts) = (Vec::new(), Vec::new());
    while !tour.over() {
        let k = tour.step();
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
        tour.draw(&mut app, &mut buf);
        let screen = text(&buf);
        assert!(screen.contains(&format!("stacked demo · {} of {STEPS}", k + 1)), "{screen}");
        said.push(screen);
        texts.push(tour.say(&app).0);
        // Nothing goes on by itself: the step waits for the reader.
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(50));
            app.refresh();
            tour.tick(&mut app);
        }
        assert_eq!(tour.step(), k, "step {} went on by itself", k + 1);
        tour.show(&mut app);
        let started = Instant::now();
        while tour.step() == k && !tour.over() {
            assert!(started.elapsed() < Duration::from_secs(20), "step {} never done:\n{}", k + 1, said.last().unwrap());
            std::thread::sleep(Duration::from_millis(50));
            app.refresh();
            tour.tick(&mut app);
        }
    }
    assert_eq!(said.len(), STEPS);
    // A step names what it is about: the step before it, which did, is
    // gone from the screen.
    assert!(texts[3].contains("to read patch 3, X takes the top right corner"), "{}", texts[3]);
    assert!(texts[5].starts_with("Ask the agent to fix O's mistake in patch 4"), "{}", texts[5]);
    // Undone, the agent's change gives O's mistake back. Patch 5, picked
    // since the move, has no comments: 120 columns leave room for two sides.
    assert!(texts[9].contains("patches 4 and 5 are O's mistake and X's win again"), "{}", texts[9]);
    assert!(texts[10].starts_with("Press 1 to switch") && !texts[10].contains("needs a wider middle"), "{}", texts[10]);
    // The moves played; O's mistake, and X's win off it, made again by the
    // stand-in, which said so in the thread; undone, then redone.
    let d = &app.db;
    let titles: Vec<&str> = d.stack("game").unwrap().patches.iter().map(|p| d.patch(p).title()).collect();
    assert_eq!(
        titles,
        [
            "X takes the center",
            "O takes the top left corner",
            "X takes the top right corner",
            "O blocks the diagonal at the bottom left",
            "X blocks O's left column",
        ]
    );
    assert!(d.log(Some("game")).iter().any(|(_, r)| matches!(r, stacked::db::Record::Head(h) if h.op.starts_with("redo"))));
    let block = d.stack("game").unwrap().patches[3].clone();
    let note = d.notes(&d.patch(&block).change).into_iter().next().cloned().unwrap();
    assert_eq!(last_role(&note.text), Some(Role::Agent), "{}", note.text);
    assert!(note.text.contains("Patch 4: O takes the bottom left corner") && note.text.contains("Patch 5 changes with it"), "{}", note.text);
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
    for row in ["  O  |  .  |  X", "  X  |  X  |  .", "  O  |  .  |  ."] {
        assert!(board.contains(&format!("\n{row}\n")), "{board}");
        assert!(end.contains(row), "{end}");
    }
    // Nothing on disk: not the database, nor anything of stacked's made
    // in the temporary directory.
    assert!(!std::path::Path::new("/nonexistent").exists());
    assert_eq!(temporary(), before);
}

/// The tour's steps, done for the reader, until it is at step `k`.
fn up_to(app: &mut App, tour: &mut Tour, k: usize) {
    while tour.step() < k {
        let was = tour.step();
        tour.show(app);
        let started = Instant::now();
        while tour.step() == was {
            assert!(started.elapsed() < Duration::from_secs(20), "step {} never done", was + 1);
            std::thread::sleep(Duration::from_millis(50));
            app.refresh();
            tour.tick(app);
        }
    }
}

#[test]
fn asked_about_patch_4_alone_the_agent_sends_the_comment_to_the_stack() {
    let (mut app, mut tour) = Tour::new().unwrap();
    // Up to the ask, the comment written on O's mistake.
    up_to(&mut app, &mut tour, 5);
    let at = |app: &App, title: &str| {
        let s = app.db.stack("game").unwrap();
        s.patches.iter().position(|p| app.db.patch(p).title() == title).unwrap()
    };
    let mistake = app.db.stack("game").unwrap().patches[at(&app, "O takes the bottom right corner")].clone();
    let change = app.db.patch(&mistake).change.clone();
    let note = app.db.notes(&change).into_iter().next().cloned().unwrap();
    // Asked to change patch 4 alone: it needs more than the patch, patch 5
    // too; the comment goes to the stack, nothing changed.
    stacked::ask::queue(&app.path, "game", &[(note.change.clone(), note.id.clone())], stacked::db::Way::Change, false).unwrap();
    app.launch();
    let started = Instant::now();
    let moved = loop {
        assert!(started.elapsed() < Duration::from_secs(20), "never moved to the stack");
        std::thread::sleep(Duration::from_millis(50));
        app.refresh();
        tour.tick(&mut app);
        if let Some(n) = app.db.stack_notes("game").into_iter().find(|n| n.moved.is_some()) {
            break n.clone();
        }
    };
    assert!(moved.text.contains("This needs more than the patch: if O takes the bottom left corner in patch 4"), "{}", moved.text);
    assert!(app.db.notes(&change).is_empty(), "gone from patch 4");
    assert_eq!(app.db.stack("game").unwrap().patches[3], mistake, "nothing changed");
    assert_eq!(tour.step(), 5, "still to ask");
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    tour.draw(&mut app, &mut buf);
    assert!(text(&buf).contains("You asked the agent about patch 4 alone"), "{}", text(&buf));
    // Asked again there, on the stack: patches 4 and 5 made again.
    up_to(&mut app, &mut tour, 6);
    at(&app, "O blocks the diagonal at the bottom left");
    at(&app, "X blocks O's left column");
}

#[test]
fn a_patch_goes_nowhere_but_where_it_was_played() {
    let (mut app, mut tour) = Tour::new().unwrap();
    // The five moves played.
    up_to(&mut app, &mut tour, 2);
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

fn press(app: &mut App, code: KeyCode) {
    let _ = app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Space, then the menu's item whose label starts with `label`: ↓ to it,
/// and Enter.
fn choose(app: &mut App, label: &str) {
    press(app, KeyCode::Char(' '));
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
fn by_hand_a_refused_move_says_how_to_stop_it_then_undo_and_redo() {
    let (mut app, mut tour) = Tour::new().unwrap();
    up_to(&mut app, &mut tour, 8);
    // As the step says, from the stack's header, where the history left
    // the cursor: ↓ to patch 5, Move this patch…, ↑ to the top, Enter.
    // Refused, and patch 5 is still being moved.
    assert!(app.focus == Focus::List && app.on_stack, "not on the stack's header");
    for _ in 0..5 {
        press(&mut app, KeyCode::Down);
    }
    assert_eq!(app.sel, 4);
    choose(&mut app, "Move this patch");
    for _ in 0..4 {
        press(&mut app, KeyCode::Up);
    }
    press(&mut app, KeyCode::Enter);
    app.refresh();
    tour.tick(&mut app);
    assert!(matches!(app.mode, Mode::Moving { .. }));
    assert_eq!(tour.step(), 8, "the step waits for Esc");
    // The tour says how to stop moving it; the status line says why it was
    // refused, and the keys that go on from there.
    let (said, _) = tour.say(&app);
    assert!(said.contains("press Esc to stop moving it"), "{said}");
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    tour.draw(&mut app, &mut buf);
    let screen = text(&buf);
    assert!(screen.contains("does not apply") && screen.contains("esc: leave it where it was"), "{screen}");
    // Esc: patch 5 where it was, and the tour goes on, to undo.
    press(&mut app, KeyCode::Esc);
    app.refresh();
    tour.tick(&mut app);
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(tour.step(), 9);
    let titles = |app: &App| -> Vec<String> {
        let s = app.db.stack("game").unwrap();
        s.patches.iter().map(|p| app.db.patch(p).title().to_string()).collect()
    };
    assert_eq!(titles(&app)[4], "X blocks O's left column");
    // Then as step 10 says: Undo takes the agent's change back, and Redo
    // makes it again.
    choose(&mut app, "Undo: agent: the stack");
    app.refresh();
    tour.tick(&mut app);
    assert_eq!(titles(&app)[3..], ["O takes the bottom right corner", "X takes the bottom left corner, and wins"]);
    assert_eq!(tour.step(), 9);
    choose(&mut app, "Redo: agent: the stack");
    app.refresh();
    tour.tick(&mut app);
    assert_eq!(titles(&app)[3..], ["O blocks the diagonal at the bottom left", "X blocks O's left column"]);
    assert_eq!(tour.step(), 10);
    // On patch 4, its comments beside it: too narrow a middle for two sides
    // at 120 columns, and the step says how to widen it.
    press(&mut app, KeyCode::Up);
    for _ in 0..2 {
        tour.draw(&mut app, &mut Buffer::empty(Rect::new(0, 0, 120, 40)));
    }
    let (said, _) = tour.say(&app);
    assert!(said.contains("needs a wider middle than this one"), "{said}");
}
