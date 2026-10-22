//! stacked: a stack editor. Patches, stacks and every edit made to them,
//! comments, and what claude and Phabricator said, all in one file that is
//! only ever appended to (`db`). The library holds all of it; `tui` draws
//! it, and the binary's commands script it.

// Keys join their parts with NUL, so "\00" in a key is NUL then "0".
#![allow(clippy::octal_escapes)]

pub mod ask;
pub mod blocks;
pub mod check;
pub mod claude;
pub mod config;
pub mod db;
#[cfg(feature = "demo")]
pub mod demo;
pub mod diff;
pub mod editor;
pub mod git;
pub mod head;
pub mod held;
pub mod history;
pub mod import;
pub mod keys;
pub mod markdown;
pub mod mbox;
#[cfg(feature = "demo")]
pub mod memory;
pub mod ops;
pub mod paint;
pub mod panes;
#[cfg(feature = "moz-phab")]
pub mod phab;
pub mod progress;
pub mod reading;
pub mod refs;
#[cfg(feature = "moz-phab")]
pub mod review;
pub mod rows;
pub mod search;
pub mod setup;
pub mod thread;
pub mod todo;
pub mod tree;
pub mod trim;
pub mod tui;
pub mod view;
#[cfg(feature = "web")]
pub mod web;
