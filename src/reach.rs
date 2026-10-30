//! Whether the agent can be reached, for the ask dialog's Ask Now: its
//! program found where stacked looks for it, and the network reaching the
//! host claude talks to first. The program is looked for as the dialog
//! opens: a path, as it is; a name, in each folder of the PATH. The network
//! is checked away from the screen, never while it is drawn: a TCP
//! connection to that host, made within `TIMEOUT` or not at all, its name
//! looked up within that time too. The host is the proxy's that
//! `HTTPS_PROXY` (or `https_proxy`) names, when one is set; else
//! `ANTHROPIC_BASE_URL`'s, when it is set; else `api.anthropic.com`, port
//! 443. What a check found holds for `KEPT`, so that dialogs opened one
//! after the other do not check again. `agent.reach_check = false` in the
//! settings turns the network's check off, for a setup where it is wrong
//! though claude works.
//!
//! The tests never reach the network: what checks it for the editors of a
//! database is the stand-in a test sets for that database (`stand_in`),
//! which says it is reached, or not, at once or slowly; and a process that
//! keeps nothing, as the tests' own, checks nothing without one, as
//! Phabricator is never read from there.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the network is given: the host's name looked up, then a
/// connection made to it, within that time, or the host is not reached.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// How long what a check found holds: a dialog opened within it of the
/// last check does not check again.
pub const KEPT: Duration = Duration::from_secs(30);

/// The host claude talks to, unless the environment says another.
pub const API: &str = "api.anthropic.com";

/// Where claude connects first: a host and its port, and how the dialog
/// says it (`api.anthropic.com`, `the proxy proxy.example.com:3128`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub said: String,
}

/// Where claude connects first, as the environment `var` reads it: the
/// proxy `HTTPS_PROXY` names, else `https_proxy`'s, when one is set; else
/// the host of `ANTHROPIC_BASE_URL`, when it is set; else `API`, port 443.
/// One set to nothing, or to what names no host, is not set.
pub fn target(var: impl Fn(&str) -> Option<String>) -> Target {
    let set = |k: &str| var(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    if let Some((host, port)) = set("HTTPS_PROXY").or_else(|| set("https_proxy")).and_then(|p| authority(&p, 80)) {
        let said = format!("the proxy {}:{port}", bracketed(&host));
        return Target { host, port, said };
    }
    if let Some((host, port)) = set("ANTHROPIC_BASE_URL").and_then(|u| authority(&u, 443)) {
        return Target { said: bracketed(&host), host, port };
    }
    Target {
        host: API.into(),
        port: 443,
        said: API.into(),
    }
}

/// Where the environment of this process says claude connects first.
pub fn target_here() -> Target {
    target(|k| std::env::var(k).ok())
}

/// `host` as a link writes it: an IPv6 address in brackets.
fn bracketed(host: &str) -> String {
    if host.contains(':') { format!("[{host}]") } else { host.to_string() }
}

/// The host and port of `url`, a link (`https://user@host:8443/path`) or a
/// host alone (`host:3128`): the port as given, else as its scheme has it
/// (443 for https, 80 for http), else `port`. None if it names no host.
fn authority(url: &str, port: u16) -> Option<(String, u16)> {
    let (port, rest) = match url.split_once("://") {
        Some((scheme, rest)) => match scheme.to_ascii_lowercase().as_str() {
            "https" => (443, rest),
            "http" => (80, rest),
            _ => (port, rest),
        },
        None => (port, url),
    };
    let at = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let at = at.rsplit_once('@').map_or(at, |(_, a)| a);
    let (host, port) = match at.strip_prefix('[') {
        // An IPv6 address, in brackets.
        Some(v6) => {
            let (host, after) = v6.split_once(']')?;
            match after.strip_prefix(':') {
                Some(p) => (host, p.parse().ok()?),
                None if after.is_empty() => (host, port),
                None => return None,
            }
        }
        None => match at.rsplit_once(':') {
            Some((host, p)) => (host, p.parse().ok()?),
            None => (at, port),
        },
    };
    (!host.is_empty()).then(|| (host.to_string(), port))
}

/// Whether a TCP connection to `t` is made within `within`: its name looked
/// up, then each of its addresses tried in turn, in what is left of that
/// time. The connection is closed at once: nothing is sent.
pub fn connects(t: &Target, within: Duration) -> bool {
    let started = Instant::now();
    // The lookup may hang on a network that answers nothing: given up on
    // once the time is over, its thread left to end on its own.
    let (tx, rx) = std::sync::mpsc::channel();
    let name = (t.host.clone(), t.port);
    std::thread::spawn(move || {
        let _ = tx.send(name.to_socket_addrs().map(|a| a.collect::<Vec<_>>()));
    });
    let Ok(Ok(addresses)) = rx.recv_timeout(within) else { return false };
    addresses.iter().any(|a| {
        let left = within.saturating_sub(started.elapsed());
        !left.is_zero() && TcpStream::connect_timeout(a, left).is_ok()
    })
}

/// What checks whether the network reaches a target.
pub type Check = Arc<dyn Fn(&Target) -> bool + Send + Sync>;

/// What checks the network for the editors of a database, by its path, in
/// the place of the network: the tests'. For the editors of that database
/// alone, as each test's editor is on a database of its own, in a process
/// the tests share.
static STAND_INS: Mutex<Vec<(PathBuf, Check)>> = Mutex::new(Vec::new());

/// Has `check` answer whether the network reaches the agent, for the
/// editors of the database at `db`, rather than the network; with none,
/// the network again.
pub fn stand_in(db: &Path, check: Option<Check>) {
    let mut all = STAND_INS.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|(at, _)| at != db);
    if let Some(c) = check {
        all.push((db.to_path_buf(), c));
    }
}

/// What checks the network for the editors of the database at `db`: the
/// stand-in set for that database, if one is; else a connection made for
/// real (`connects`), but never from a process that keeps nothing, as the
/// tests' own: there, none, and nothing is checked.
pub fn checker(db: &Path) -> Option<Check> {
    let all = STAND_INS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, c)) = all.iter().find(|(at, _)| at == db) {
        return Some(c.clone());
    }
    if crate::home::get().dir == Path::new(crate::home::NOWHERE) {
        return None;
    }
    Some(Arc::new(|t: &Target| connects(t, TIMEOUT)))
}

/// Why `program`, what an ask runs as claude, cannot be run, if it cannot,
/// as the dialog says it: a path that is not there, or not a program; a
/// name that no folder of `path` (the PATH) holds as a program.
pub fn missing(program: &str, path: Option<&std::ffi::OsStr>) -> Option<String> {
    if program.contains('/') {
        let p = Path::new(program);
        return match p.exists() {
            false => Some(format!("{program} is not found")),
            true if !runnable(p) => Some(format!("{program} is not a program that can be run")),
            true => None,
        };
    }
    let found = path.is_some_and(|path| {
        std::env::split_paths(path).any(|dir| {
            // An empty folder of the PATH is the current one.
            let dir = if dir.as_os_str().is_empty() { PathBuf::from(".") } else { dir };
            runnable(&dir.join(program))
        })
    });
    (!found).then(|| format!("{program} is not found on the PATH"))
}

/// Whether `p` is a file this user may run.
fn runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The target an environment of `vars` gives.
    fn of(vars: &[(&str, &str)]) -> Target {
        target(|k| vars.iter().find(|(name, _)| *name == k).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn claude_connects_to_its_proxy_else_its_base_url_s_host_else_anthropic_s_api() {
        let api = Target {
            host: API.into(),
            port: 443,
            said: API.into(),
        };
        assert_eq!(of(&[]), api);
        // Set to nothing, or to what names no host: not set.
        assert_eq!(of(&[("HTTPS_PROXY", " "), ("ANTHROPIC_BASE_URL", "")]), api);
        assert_eq!(of(&[("HTTPS_PROXY", "http://:3128")]), api);
        // The base URL's host, at its scheme's port unless one is given.
        let base = of(&[("ANTHROPIC_BASE_URL", "https://llm.example.com/v1")]);
        assert_eq!((base.host.as_str(), base.port, base.said.as_str()), ("llm.example.com", 443, "llm.example.com"));
        let base = of(&[("ANTHROPIC_BASE_URL", "http://localhost:8080")]);
        assert_eq!((base.host.as_str(), base.port), ("localhost", 8080));
        // The proxy, over the base URL: with a user, a path, an IPv6
        // address, or no scheme; HTTPS_PROXY over https_proxy.
        let proxy = of(&[("HTTPS_PROXY", "http://me:secret@proxy.example.com:3128/"), ("ANTHROPIC_BASE_URL", "https://llm.example.com")]);
        assert_eq!(
            (proxy.host.as_str(), proxy.port, proxy.said.as_str()),
            ("proxy.example.com", 3128, "the proxy proxy.example.com:3128")
        );
        let proxy = of(&[("https_proxy", "proxy.example.com:8080")]);
        assert_eq!((proxy.host.as_str(), proxy.port), ("proxy.example.com", 8080));
        let proxy = of(&[("HTTPS_PROXY", "http://upper"), ("https_proxy", "http://lower")]);
        assert_eq!((proxy.host.as_str(), proxy.port), ("upper", 80));
        let proxy = of(&[("HTTPS_PROXY", "https://[::1]:8443")]);
        assert_eq!((proxy.host.as_str(), proxy.port, proxy.said.as_str()), ("::1", 8443, "the proxy [::1]:8443"));
    }

    #[test]
    fn the_network_is_reached_when_a_connection_is_made_in_time() {
        // A listener of this machine's: reached. Port 0, where none can
        // listen: refused, so not.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let at = |port| Target {
            host: "127.0.0.1".into(),
            port,
            said: String::new(),
        };
        assert!(connects(&at(port), TIMEOUT));
        assert!(!connects(&at(0), TIMEOUT));
    }

    #[test]
    fn a_process_that_keeps_nothing_checks_the_network_only_through_a_stand_in() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("db.jsonl");
        assert_eq!(crate::home::get().dir, Path::new(crate::home::NOWHERE));
        assert!(checker(&db).is_none(), "nothing checked");
        stand_in(&db, Some(Arc::new(|t: &Target| t.port == 1)));
        let check = checker(&db).expect("the stand-in");
        let t = |port| Target {
            host: "nowhere".into(),
            port,
            said: String::new(),
        };
        assert!(check(&t(1)) && !check(&t(2)));
        // Another database's editors have none.
        assert!(checker(&tmp.path().join("other.jsonl")).is_none());
        stand_in(&db, None);
        assert!(checker(&db).is_none());
    }

    #[test]
    fn the_agent_s_program_is_found_on_the_path_or_where_its_path_says() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let claude = bin.join("claude-here");
        std::fs::write(&claude, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = bin.join("plain");
        std::fs::write(&plain, "not a program\n").unwrap();
        let path = std::env::join_paths([tmp.path().join("nowhere"), bin.clone()]).unwrap();
        // A name: in a folder of the PATH, as a program.
        assert_eq!(missing("claude-here", Some(&path)), None);
        assert_eq!(missing("claude", Some(&path)), Some("claude is not found on the PATH".into()));
        assert_eq!(missing("plain", Some(&path)), Some("plain is not found on the PATH".into()));
        assert_eq!(missing("claude-here", None), Some("claude-here is not found on the PATH".into()));
        // A path: there, and a program.
        let at = claude.display().to_string();
        assert_eq!(missing(&at, None), None);
        let gone = bin.join("gone").display().to_string();
        assert_eq!(missing(&gone, Some(&path)), Some(format!("{gone} is not found")));
        let file = plain.display().to_string();
        assert_eq!(missing(&file, Some(&path)), Some(format!("{file} is not a program that can be run")));
    }
}
