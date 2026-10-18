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
    let mut said = Vec::new();
    while !tour.over() {
        let k = tour.step();
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
        tour.draw(&mut app, &mut buf);
        let screen = text(&buf);
        assert!(screen.contains(&format!("stacked demo · {} of {STEPS}", k + 1)), "{screen}");
        said.push(screen);
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
    // The moves played; O's mistake made good by the stand-in, which said
    // so in the thread; the move made, then undone.
    let d = &app.db;
    let titles: Vec<&str> = d.stack("game").unwrap().patches.iter().map(|p| d.patch(p).title()).collect();
    assert_eq!(
        titles,
        [
            "X takes the center",
            "O takes the top left corner",
            "X takes the top right corner",
            "O blocks the diagonal at the bottom left",
            "X takes the middle of the left side",
        ]
    );
    let block = d.stack("game").unwrap().patches[3].clone();
    let note = d.notes(&d.patch(&block).change).into_iter().next().cloned().unwrap();
    assert_eq!(last_role(&note.text), Some(Role::Agent), "{}", note.text);
    assert!(note.text.contains("Moved O to the bottom left corner"), "{}", note.text);
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
