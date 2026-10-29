//! stacked: a stack editor. Patches, stacks and every edit made to them,
//! comments, and what claude and Phabricator said, all in one file that is
//! only ever appended to (`db`). The library holds all of it; `tui` draws
//! it (in a browser with `web`, in a window of its own with `app`), and the
//! binary's commands script it.

// Keys join their parts with NUL, so "\00" in a key is NUL then "0".
#![allow(clippy::octal_escapes)]

#[cfg(feature = "app")]
pub mod app;
pub mod ask;
pub mod blocks;
#[cfg(all(feature = "app", target_os = "macos"))]
pub mod bundle;
pub mod check;
pub mod claude;
pub mod complete;
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
pub mod home;
#[cfg(feature = "app")]
pub mod icon;
pub mod import;
pub mod keys;
pub mod links;
pub mod lint;
pub mod markdown;
pub mod mbox;
#[cfg(feature = "demo")]
pub mod memory;
pub mod messages;
pub mod ops;
pub mod paint;
pub mod panes;
#[cfg(feature = "moz-phab")]
pub mod phab;
pub mod progress;
pub mod reading;
pub mod reconcile;
pub mod refs;
#[cfg(feature = "moz-phab")]
pub mod review;
pub mod rows;
pub mod search;
pub mod setup;
pub mod state;
#[cfg(feature = "moz-phab")]
pub mod submit;
pub mod thread;
pub mod todo;
pub mod tree;
pub mod trim;
#[cfg(feature = "try-push")]
pub mod try_push;
pub mod tui;
pub mod view;
#[cfg(feature = "web")]
pub mod web;
pub mod whole;
pub mod wrap;
