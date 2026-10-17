//! A name's tree of edits drawn as `git log --graph` would: newest first, a
//! lane per branch, each stack a dot on its lane (the one the name is on
//! filled), a lane closing where its branch joins the stack it was edited
//! from. The history draws the same lanes, past the records that are no
//! stack.

use crate::db::Db;

/// A line of the graph: the stack's rev and the lanes drawn before it.
pub struct Line {
    pub rev: usize,
    pub lanes: String,
}

/// The lanes open as a tree is drawn, newest first: the rev each waits
/// for, the parent of the last stack drawn on it.
#[derive(Debug, Default)]
pub struct Lanes(Vec<Option<usize>>);

impl Lanes {
    /// Stack `rev`, edited from `parent`, drawn: the lanes before it, its
    /// own marked (filled if it is the one the name is on), the lanes of
    /// the branches made from it closing.
    pub fn node(&mut self, rev: usize, parent: Option<usize>, head: bool) -> String {
        let lanes = &mut self.0;
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
                if head { '●' } else { '○' }
            } else if mine.contains(&i) {
                '┘'
            } else if lane.is_some() {
                '│'
            } else {
                ' '
            });
            text.push(' ');
        }
        for &i in &mine {
            lanes[i] = None;
        }
        lanes[col] = parent;
        while lanes.last() == Some(&None) {
            lanes.pop();
        }
        text
    }

    /// The lanes going on past a line that is no stack.
    pub fn passing(&self) -> String {
        self.0.iter().map(|l| if l.is_some() { "│ " } else { "  " }).collect()
    }
}

pub fn graph(db: &Db, name: &str) -> Vec<Line> {
    let head = db.head(name).map(|(r, _)| r);
    let mut nodes = db.tree(name);
    nodes.reverse();
    let mut lanes = Lanes::default();
    nodes
        .into_iter()
        .map(|(rev, s)| Line {
            rev,
            lanes: lanes.node(rev, s.parent, Some(rev) == head),
        })
        .collect()
}
