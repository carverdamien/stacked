//! `stacked demo`, done for the reader step by step, as Tab does it: each
//! step done, the tour over, nothing written on disk.
#![cfg(feature = "demo")]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use stacked::demo::{STEPS, Tour};
use stacked::thread::{Role, last_role};

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
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
    // Undone, the agent's change gives O's mistake back. And 120 columns
    // leave the middle too narrow for two sides: step 11 says so.
    assert!(texts[9].contains("patches 4 and 5 are O's mistake and X's win again"), "{}", texts[9]);
    assert!(texts[10].contains("needs a wider middle"), "{}", texts[10]);
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
    // At its end, the board as its patches leave it.
    let end = said.last().unwrap();
    for row in ["  O  |  .  |  X", "  X  |  X  |  .", "  O  |  .  |  ."] {
        assert!(end.contains(row), "{end}");
    }
    // Nothing on disk: not the database, nor anything of stacked's made
    // in the temporary directory.
    assert!(!std::path::Path::new("/nonexistent").exists());
    assert_eq!(temporary(), before);
}

/// The tour's steps, done for the reader, until it is at step `k`.
fn up_to(app: &mut stacked::tui::App, tour: &mut Tour, k: usize) {
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
    let at = |app: &stacked::tui::App, title: &str| {
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
    assert!(moved.text.contains("This needs more than the patch: O blocking the bottom left corner"), "{}", moved.text);
    assert!(app.db.notes(&change).is_empty(), "gone from patch 4");
    assert_eq!(app.db.stack("game").unwrap().patches[3], mistake, "nothing changed");
    assert_eq!(tour.step(), 5, "still to ask");
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    tour.draw(&mut app, &mut buf);
    assert!(text(&buf).contains("Asked about patch 4 alone"), "{}", text(&buf));
    // Asked again there, on the stack: patches 4 and 5 made again.
    up_to(&mut app, &mut tour, 6);
    at(&app, "O blocks the diagonal at the bottom left");
    at(&app, "X blocks O's left column");
}

#[test]
fn a_patch_moved_where_it_applies_is_what_undo_takes_back() {
    let (mut app, mut tour) = Tour::new().unwrap();
    up_to(&mut app, &mut tour, 8);
    // Patch 4, O's bottom row, after patch 5: rows apart, it applies, out
    // of turn; the step is done.
    stacked::db::Db::edit(&app.path, |db| stacked::ops::move_patch(db, "game", 3, 4)).unwrap();
    app.refresh();
    tour.tick(&mut app);
    assert_eq!(tour.step(), 9);
    let s = app.db.stack("game").unwrap();
    assert_eq!(app.db.patch(&s.patches[4]).title(), "O blocks the diagonal at the bottom left");
    // Undo takes back the move, not the agent's change: the tour says so.
    let (said, _) = tour.say(&app);
    assert!(said.contains("your move is taken back"), "{said}");
}
