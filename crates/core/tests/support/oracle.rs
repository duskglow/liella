//! Test programs compiled into the shared `target/test-oracles` directory (the C oracles, the
//! programs linked against the products). Every test process of every test binary may want the
//! same program at the same moment (nextest runs each test in its own process), so a build must
//! never expose a half-written file under the final name: one process saw the compiler's output
//! before it was executable (EACCES) or while it was still open for writing (ETXTBSY).
//!
//! [`build`] therefore compiles to a name of its own in the same directory and renames it onto
//! the final name (rename(2) replaces the name atomically; a process already running the old file
//! keeps it). A per-name lock file (flock) makes concurrent processes wait for one build instead of
//! all compiling; correctness does not depend on it. The build is skipped when the program exists and
//! `<name>.stamp` records the same command line and the same content hash of every input on it (the
//! files it names, plus the headers beside a `.c` source and under each include directory). Content,
//! not modification time, so file-system timestamp resolution does not matter.
//! Included with `#[path]` by the test files that compile programs.
#![allow(dead_code)]

use std::ffi::OsString;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::LOCK_EX;

static SEQ: AtomicU32 = AtomicU32::new(0);

/// Compile `cmd` (a compiler command line without `-o`) into `out`; the compiler's messages go
/// to the test's stderr. True if `out` is there and current.
pub(crate) fn build(cmd: Command, out: &Path) -> bool {
    build_with(cmd, out, false)
}

/// [`build`] with the compiler's output captured (for builds that may fail by design, for
/// example on a host without a header).
pub(crate) fn build_quiet(cmd: Command, out: &Path) -> bool {
    build_with(cmd, out, true)
}

/// Compile locale `lang`.`charset` into the directory `dest` (in the shared `target/test-locales`)
/// with the host's `localedef`, the same way: built in a private sibling directory and renamed into
/// place, so no test sees (or writes into) a half-built locale. True when `dest/<probe>` exists
/// after. localedef reports warnings with status 1 and still writes the locale.
pub(crate) fn locale(dest: &Path, lang: &str, charset: &str, probe: &str) -> bool {
    if dest.join(probe).exists() {
        return true;
    }
    let Some(parent) = dest.parent() else { return false };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let lock = File::options().create(true).truncate(false).write(true).open(sibling(dest, ".lock"));
    if let Ok(f) = &lock {
        let _ = CSys.flock(Fd::new(f.as_raw_fd()), LOCK_EX);
    }
    if dest.join(probe).exists() {
        return true;
    }
    let tmp = sibling(dest, &format!(".tmp-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&tmp);
    let made = Command::new("localedef").args(["-i", lang, "-f", charset]).arg(&tmp).output().is_ok_and(|o| o.status.code().is_some_and(|c| c <= 1));
    if made && tmp.join(probe).exists() {
        // Losing a race with a builder that holds no lock is fine: its locale is complete too.
        let _ = std::fs::rename(&tmp, dest);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    dest.join(probe).exists()
}

fn sibling(out: &Path, suffix: &str) -> PathBuf {
    let mut name = out.file_name().map(OsString::from).unwrap_or_default();
    name.push(suffix);
    out.with_file_name(name)
}

/// The command line as recorded in the stamp.
fn key(cmd: &Command) -> Vec<u8> {
    let mut k = cmd.get_program().as_bytes().to_vec();
    for a in cmd.get_args() {
        k.push(0);
        k.extend_from_slice(a.as_bytes());
    }
    k
}

/// FNV-1a over `bytes`, continuing from `h`. Fixed and portable, so a stamp stays valid across
/// toolchains (a changed algorithm could only force one rebuild).
fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The files the command line depends on: the files it names, the `.h` files beside each `.c`
/// source, and every header under each include directory (`-I dir`).
fn input_files(cmd: &Command) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();
    for a in cmd.get_args() {
        let p = Path::new(a);
        if p.is_dir() {
            headers_under(p, &mut files)?;
        } else if p.is_file() {
            files.push(p.to_path_buf());
            if p.extension().is_some_and(|e| e == "c") {
                headers_under_flat(p.parent()?, &mut files);
            }
        }
    }
    // Directory order differs between file systems; the hash must not.
    files.sort();
    files.dedup();
    Some(files)
}

fn headers_under_flat(dir: &Path, files: &mut Vec<PathBuf>) {
    for p in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
        if p.extension().is_some_and(|x| x == "h") {
            files.push(p);
        }
    }
    files.sort();
}

fn headers_under(dir: &Path, files: &mut Vec<PathBuf>) -> Option<()> {
    for p in std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()) {
        if p.is_dir() {
            headers_under(&p, files)?;
        } else if p.extension().is_some_and(|x| x == "h") {
            files.push(p);
        }
    }
    files.sort();
    Some(())
}

/// A hash of the names and contents of the input files. Content, not modification time: a
/// file system with 1-2 s (or no) timestamps cannot make an edited input look old.
fn inputs_hash(cmd: &Command) -> Option<u64> {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for f in input_files(cmd)? {
        h = fnv(h, f.as_os_str().as_bytes());
        h = fnv(h, &[0]);
        h = fnv(h, &std::fs::read(&f).ok()?);
        h = fnv(h, &[0]);
    }
    Some(h)
}

/// The stamp: the command line, then the hash of the inputs it was built from.
fn stamp_text(key: &[u8], inputs: u64) -> Vec<u8> {
    let mut t = key.to_vec();
    t.extend_from_slice(format!("\n{inputs:016x}").as_bytes());
    t
}

fn build_with(mut cmd: Command, out: &Path, quiet: bool) -> bool {
    let Some(dir) = out.parent() else { return false };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    // A static link against the products drops the library sections the program never reaches
    // (the archive's functions and data are in sections of their own): ~5x smaller programs,
    // so far less written per test run (CONF-DIET). Part of the command line, so of the stamp.
    let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
    if args.iter().any(|a| *a == "-static") && args.iter().any(|a| *a == "-nostdlib") && !args.iter().any(|a| *a == "-c" || *a == "-r") {
        cmd.arg("-Wl,--gc-sections");
    }
    let stamp = sibling(out, ".stamp");
    let key = key(&cmd);
    // Held until the end of the function; without it (no flock on this file system) concurrent
    // processes each build and rename, which is still correct.
    let lock = File::options().create(true).truncate(false).write(true).open(sibling(out, ".lock"));
    if let Ok(f) = &lock {
        let _ = CSys.flock(Fd::new(f.as_raw_fd()), LOCK_EX);
    }
    // Taken before the build: an input edited during it makes the stamp stale, never wrong.
    let want = inputs_hash(&cmd).map(|h| stamp_text(&key, h));
    if let Some(want) = &want
        && out.is_file()
        && std::fs::read(&stamp).is_ok_and(|s| s == *want)
    {
        return true;
    }
    let tag = format!(".tmp-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let tmp = sibling(out, &tag);
    cmd.arg("-o").arg(&tmp);
    let ok = if quiet { cmd.output().is_ok_and(|o| o.status.success()) } else { cmd.status().is_ok_and(|s| s.success()) };
    if !ok || std::fs::rename(&tmp, out).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    // After the program: a crash in between leaves the old stamp, which can only force a rebuild.
    let stamp_tmp = sibling(out, &format!("{tag}.stamp"));
    let wrote = want.as_ref().is_some_and(|w| std::fs::write(&stamp_tmp, w).is_ok() && std::fs::rename(&stamp_tmp, &stamp).is_ok());
    if !wrote {
        let _ = std::fs::remove_file(&stamp_tmp);
        let _ = std::fs::remove_file(&stamp);
    }
    true
}
