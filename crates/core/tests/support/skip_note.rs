//! Run-time skips that show (MSGFMT-SILENT-PASS). A test whose tool is missing (`cc`, `msgfmt`,
//! `gencat`, ...) returns early and passes; libtest hides an `eprintln!` of a passing test, so
//! that pass said nothing. [`skipped`] writes the line straight to the process's stderr, which
//! libtest does not capture (`cargo test` shows it in the log), and appends it to the file named
//! by `LIELLA_SKIP_LOG` when that is set (`cargo xtask nextest` sets it, because nextest hides the
//! output of a passing test, and prints the collected lines after its report).
//!
//! The line is `skipped: <test binary>: <what is missing>`: the convention of the namespace tests.
//! [`missing`] answers "is this tool absent", and `LIELLA_FORCE_NO_TOOLS=msgfmt,gencat` makes it
//! answer yes for the named tools (no root, no uninstalling), to see the skip path on any host.
//! A tool that exists is never reported missing unless it is named there.
#![allow(dead_code)]

use std::io::Write;
use std::process::{Command, Stdio};

/// Whether `tool` cannot be run: named in `LIELLA_FORCE_NO_TOOLS` (a comma list), or `tool --version`
/// does not start.
pub(crate) fn missing(tool: &str) -> bool {
    forced(tool, std::env::var("LIELLA_FORCE_NO_TOOLS").ok().as_deref())
        || Command::new(tool).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_err()
}

/// Whether `list` (the value of `LIELLA_FORCE_NO_TOOLS`) names `tool`.
fn forced(tool: &str, list: Option<&str>) -> bool {
    list.is_some_and(|l| l.split(',').any(|t| t.trim() == tool))
}

/// Report a run-time skip: `msg` is what was missing, with or without a leading `skipped: `.
pub(crate) fn skipped(msg: &str) {
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    let exe = exe.rsplit_once('-').map_or(exe.as_str(), |(stem, hash)| if hash.len() == 16 { stem } else { exe.as_str() });
    let body = msg.strip_prefix("skipped: ").unwrap_or(msg);
    let line = format!("skipped: {exe}: {body}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
    if let Some(path) = std::env::var_os("LIELLA_SKIP_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
