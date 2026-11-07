//! `private-paths` (static): no lab network address and no personal home path anywhere in the tree (D-X.45).
//!
//! Beside `lab-names`, but over EVERY tracked file, `handoff/` and the tool scripts included: `handoff/` may name
//! lab hosts, yet what reaches main is public. Two kinds of text fail:
//! - a lab address, `10.0.100.<d>` or `10.0.0.<d>` not preceded by a digit or `.` (so `110.0.0.1` and `010.0.0.1`
//!   pass). Other private 10/8 addresses are ordinary test data (resolver fixtures) and pass. Scripts that reach a
//!   lab host take its address from the operator's machine through `validation/lab_host.sh`.
//! - a personal home path: `/home/<name>` with `<name>` not a placeholder, or a Windows profile path
//!   `<drive>:/Users` / `<drive>:\Users`. Write `~` or a repo-relative path instead.
//!
//! The one documented placeholder form: `/home/<name>` with `<name>` (trailing digits ignored) in [`PLACEHOLDERS`],
//! the fixture accounts of the passwd/wordexp tests and benches. A test needing a new fixture home adds its name
//! there. Files that are not UTF-8 are read lossily (fuzz seeds hold NUL bytes and are still scanned).

use std::path::Path;

/// Fixture home directories the tests and benches use (`/home/alice`, `/home/stress00042`, ...).
const PLACEHOLDERS: [&str; 12] = ["alice", "bob", "carol", "dave", "trail", "h", "me", "x", "pc", "cbench", "stress", "user"];

fn name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

/// The first private address or personal path on a line, if any.
fn hit(line: &str) -> Option<String> {
    let b = line.as_bytes();
    let lab = [["10", "0", "100", ""].join("."), ["10", "0", "0", ""].join(".")];
    for prefix in &lab {
        let mut from = 0;
        while let Some(i) = line[from..].find(prefix.as_str()) {
            let s = from + i;
            let digit = b.get(s + prefix.len()).is_some_and(u8::is_ascii_digit);
            if digit && (s == 0 || !(b[s - 1].is_ascii_digit() || b[s - 1] == b'.')) {
                return Some(format!("lab address `{prefix}x`"));
            }
            from = s + 1;
        }
    }
    let mut from = 0;
    while let Some(i) = line[from..].find("/home/") {
        let s = from + i + "/home/".len();
        let e = s + b[s..].iter().take_while(|&&c| name_char(c)).count();
        let name = line[s..e].trim_end_matches(|c: char| c.is_ascii_digit());
        if e > s && !PLACEHOLDERS.contains(&name) {
            return Some(format!("home path `/home/{}`", &line[s..e]));
        }
        from = s;
    }
    let lower = line.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut from = 0;
    while let Some(i) = lower[from..].find("users") {
        let s = from + i;
        if s >= 3 && lb[s - 3].is_ascii_alphabetic() && lb[s - 2] == b':' && matches!(lb[s - 1], b'/' | b'\\') {
            return Some("Windows profile path `<drive>:/Users`".to_string());
        }
        from = s + 1;
    }
    None
}

/// Findings (`path:line: what`) in one file's text.
fn scan(path: &str, text: &str) -> Vec<String> {
    text.lines().enumerate().filter_map(|(n, l)| hit(l).map(|w| format!("{path}:{}: {w}", n + 1))).collect()
}

pub fn check(root: &Path) -> Result<(), String> {
    let mut found = Vec::new();
    for path in crate::labnames::tracked_with(root, true)? {
        if let Ok(bytes) = std::fs::read(root.join(&path)) {
            found.extend(scan(&path, &String::from_utf8_lossy(&bytes)));
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    let first: Vec<&str> = found.iter().take(40).map(String::as_str).collect();
    Err(format!("{} lab address / personal path hit(s) (name the host, write `~` or a repo-relative path, or for a test fixture home add the name to PLACEHOLDERS in xtask/src/privpaths.rs):\n  {}", found.len(), first.join("\n  ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(a: u8, b: u8) -> String {
        format!("10.0.{a}.{b}")
    }

    #[test]
    fn planted_lab_address_is_caught_everywhere() {
        for path in ["handoff/x.md", "validation/wl_gate.sh", "xtask/src/labnames.rs"] {
            assert_eq!(scan(path, &format!("ssh claude@{}", addr(100, 53))).len(), 1, "{path}");
            assert_eq!(scan(path, &format!("on {} today", addr(0, 36))).len(), 1, "{path}");
        }
    }

    #[test]
    fn other_addresses_pass() {
        assert!(scan("t.rs", &format!("1{} 0{} {} 10.1.1.1 10.20 {}.", addr(100, 53), addr(0, 1), addr(1, 0), addr(100, 0).trim_end_matches(".0"))).is_empty());
    }

    #[test]
    fn planted_home_path_is_caught() {
        // Built at run time so this file passes its own check.
        let (h, u) = (["", "home", ""].join("/"), ["Users", ""].join("/"));
        for l in [format!("see {h}claude/w/x"), format!("{h}rmill"), format!("cd C:/{u}someone"), format!(r"C:\{}\x", &u[..5]), format!("d:/{}x", u.to_lowercase()), format!("HOME={h}jdoe2")] {
            assert_eq!(scan("handoff/x.md", &l).len(), 1, "{l}");
        }
    }

    #[test]
    fn placeholders_and_non_paths_pass() {
        for l in ["/home/alice:/bin/sh", "HOME=/home/cbench", "/home/stress49999", "/home/stress{k:05}", "/home/<user>", "/home/", "~/w/x", "EUSERS", "elf_users"] {
            assert!(scan("t.rs", l).is_empty(), "{l}");
        }
    }
}
