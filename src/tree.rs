//! A name's tree of edits drawn as `git log --graph` would: newest first, a
//! lane per branch, each stack a dot on its lane (the one the name is on
//! filled), a lane closing where its branch joins the stack it was edited
//! from.

use crate::db::Db;

/// A line of the graph: the stack's rev and the lanes drawn before it.
pub struct Line {
    pub rev: usize,
    pub lanes: String,
}

pub fn graph(db: &Db, name: &str) -> Vec<Line> {
    let head = db.head(name).map(|(r, _)| r);
    let mut nodes = db.tree(name);
    nodes.reverse();
    // The rev each lane waits for: the parent of the last stack drawn on it.
    let mut lanes: Vec<Option<usize>> = Vec::new();
    let mut out = Vec::new();
    for (rev, s) in nodes {
        let mine: Vec<usize> = (0..lanes.len()).filter(|&i| lanes[i] == Some(rev)).collect();
        let col = match mine.first() {
            Some(&c) => c,
            None => match lanes.iter().position(Option::is_none) {
                Some(free) => free,
                None => {
                    lanes.push(None);
                    lanes.len() - 1
                }
            },
        };
        let mut text = String::new();
        for (i, lane) in lanes.iter().enumerate() {
            text.push(if i == col {
                if Some(rev) == head { '●' } else { '○' }
            } else if mine.contains(&i) {
                '┘'
            } else if lane.is_some() {
                '│'
            } else {
                ' '
            });
            text.push(' ');
        }
        out.push(Line {
            rev,
            lanes: text,
        });
        for &i in &mine {
            lanes[i] = None;
        }
        lanes[col] = s.parent;
        while lanes.last() == Some(&None) {
            lanes.pop();
        }
    }
    out
}
