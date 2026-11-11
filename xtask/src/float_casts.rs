//! `float-casts` (static check, D-10.95): every float-to-integer `as` cast in liella-math (and in
//! the string-to-number parser, [`SCOPE`]) states
//! why its operand is in the target's range. On x86-64 before AVX-512 such a cast is a signed
//! conversion (`cvttsd2si` / `cvttss2si`; `as u64` adds a second one on `x − 2^63`): an operand
//! outside the signed range (or a NaN) raises invalid in hardware, which the math envelope must
//! then clear with an MXCSR write, a serialising instruction on every call (LIBM-INVALID-CONVERT-SCAN:
//! the long double arc kernels paid it on every `asinl` / `acosl`). Values that may reach `2^63`
//! go through the bits instead (`wide_int::arc::big_u64`).
//!
//! The casts are found by type, not by text: clippy's `cast_possible_truncation` and
//! `cast_sign_loss`, forced on over every `allow` (`--force-warn`), in a target directory of
//! their own (the gate's clippy cache stays as it is). A site passes when a `RANGE:` comment
//! stands on its line, within the five lines above or on the next line, or its line clamps the
//! operand (`.clamp(`). The check fails when clippy reports no float cast at all (a changed
//! message format would otherwise pass everything).

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// The checked sources: the math crate, and the decimal parser behind `strtod` & co., which runs
/// outside the math envelope (a flag it raises is one the caller sees).
const SCOPE: [&str; 2] = ["crates/math/", "crates/core/src/num/parse.rs"];

/// Lines above a cast that may hold its `RANGE:` comment.
const ABOVE: usize = 5;

/// The check: `Err` lists every float-to-integer cast without a range proof.
pub fn static_check(root: &Path) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target/float-casts"))
        .args(["clippy", "-q", "-p", "liella-math", "-p", "liella", "--lib", "--message-format=short", "--"])
        .args(["-A", "clippy::all", "--force-warn", "clippy::cast_possible_truncation", "--force-warn", "clippy::cast_sign_loss"])
        .output()
        .map_err(|e| format!("cannot run cargo clippy: {e}"))?;
    let text = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!("cargo clippy failed:\n{text}"));
    }
    let mut seen = BTreeSet::new();
    let mut bad = Vec::new();
    for line in text.lines() {
        let Some((loc, msg)) = line.split_once(": warning: casting `") else { continue };
        let Some((from, rest)) = msg.split_once("` to `") else { continue };
        let Some((to, _)) = rest.split_once('`') else { continue };
        if !matches!(from, "f32" | "f64") || to.starts_with('f') {
            continue;
        }
        let mut parts = loc.rsplitn(3, ':');
        let (_col, lno, file) = (parts.next(), parts.next(), parts.next());
        let (Some(lno), Some(file)) = (lno.and_then(|l| l.parse::<usize>().ok()), file) else { continue };
        if !SCOPE.iter().any(|p| file.starts_with(p)) {
            continue;
        }
        if !seen.insert((file.to_string(), lno)) {
            continue; // both lints name the same site
        }
        let src = std::fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let lines: Vec<&str> = src.lines().collect();
        let at = lno.saturating_sub(1);
        let window = lines.get(at.saturating_sub(ABOVE)..(at + 2).min(lines.len())).unwrap_or(&[]);
        let clamped = lines.get(at).is_some_and(|l| l.contains(".clamp("));
        if !clamped && !window.iter().any(|l| l.contains("RANGE:")) {
            bad.push(format!("{file}:{lno}: `{from}` to `{to}` with no `RANGE:` comment or clamp"));
        }
    }
    if seen.is_empty() {
        return Err("clippy reported no float-to-integer cast in the checked sources: the check cannot see them (message format changed?)".into());
    }
    if bad.is_empty() {
        println!("float-casts: {} float-to-integer casts in the checked sources, each with its range", seen.len());
        Ok(())
    } else {
        Err(format!("float-to-integer casts whose operand range is not stated (D-10.95; an operand at or past the signed range raises invalid in hardware):\n{}", bad.join("\n")))
    }
}
