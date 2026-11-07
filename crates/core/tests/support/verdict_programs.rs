//! The program inputs of a verdict key (TVA-MATH-VERDICT-CACHE, D-10.57): the shared objects a
//! dynamic reference program loads. Kept apart from `verdict.rs` because it starts the program
//! (through the dynamic loader), and `verdict.rs` itself runs nothing, so a pure-Rust sweep can
//! include it and stay pure Rust. Included with `#[path]`, next to `verdict.rs`, by the C-oracle
//! sweeps only; a target that includes this file builds or runs programs and is never
//! `nextest::PASS0_ONLY`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::verdict::Key;

/// The shared objects program `prog` loads, as the dynamic loader resolves them
/// (`LD_TRACE_LOADED_OBJECTS=1`, ld.so(8): it lists the dependencies instead of running the
/// program); `None` if the program cannot be started, the loader reports a missing object, or
/// lists none.
pub(crate) fn loaded_objects(prog: &Path) -> Option<Vec<PathBuf>> {
    let out = Command::new(prog).env_clear().env("LD_TRACE_LOADED_OBJECTS", "1").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut objs = Vec::new();
    for l in text.lines().map(str::trim) {
        if l.contains("not found") {
            return None;
        }
        // `libm.so.6 => /usr/lib/.../libm.so.6 (0x...)`, or the loader: `/lib64/ld-linux-x86-64.so.2 (0x...)`.
        let path = match l.split_once("=> ") {
            Some((_, rest)) => rest,
            None => l,
        };
        let path = path.split(" (").next().unwrap_or("");
        if path.starts_with('/') {
            objs.push(PathBuf::from(path));
        }
    }
    objs.sort();
    objs.dedup();
    (!objs.is_empty()).then_some(objs)
}

impl Key {
    /// Adds the bytes of every shared object the dynamic program `prog` loads
    /// ([`loaded_objects`]); the key is incomplete if they cannot be resolved.
    #[must_use]
    pub(crate) fn loaded_by(self, prog: &Path) -> Self {
        self.resolved("loaded by", prog, loaded_objects)
    }
}
