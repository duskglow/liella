// Specified from spec §8.1, §8.2 and §8.3 (the compat `__strto*_internal` / `__wcsto*_internal`
// entry points and their `group` argument, D-8.1 compat; STRTO-INTERNAL-GROUP-3).

//! The `group` argument of all fourteen Rust-side compat entry points, against the reference.
//!
//! PROBE RECORD: `tests/data/strto_internal_group.txt` is the unedited output of
//! `crates/core/tests/oracle/strto_group_table.c` built against the reference (glibc 2.39,
//! Ubuntu 2.39-0ubuntu8.9, kanon, 2026-10-24) with `LOCPATH` naming the locales below. Each line:
//! locale, flag, input, then value/end(/errno) per form. Settled there, besides run 1's table:
//! - C locale: the flag is a no-op (the `g=1` lines equal the `g=0` lines).
//! - `grpsame` (thousands_sep == decimal_point == `.`): integers still group (`1.234.567` ->
//!   1234567, end 9), so a separator equal to the radix does NOT turn grouping off (#2058).
//! - `grpstop` (`3;CHAR_MAX`): `1,234,567` -> 1234567/9 and `1234,567` -> 1234567/8.
//!
//! The locales are the test's own (no host locale is read): LC_NUMERIC-only sources written here
//! and compiled with `localedef -c` into `target/test-locales/strto-group` (no localedef: the
//! test says so and passes). The checks run in a re-executed child, which sets `LC_NUMERIC`.
#![no_builtins]
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use core::ffi::c_char;
use core::ptr::null_mut;
use std::fmt::Write as _;

use liella_boundary::errno;
use liella_boundary::posix::s07_locale as s;
use liella_boundary::posix::s08_numeric as n;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;

const LC_NUMERIC: i32 = 1;
const PROBE: &str = "LIELLA_STRTO_GROUP";
const RECORD: &str = include_str!("data/strto_internal_group.txt");
/// name, thousands_sep, decimal_point, grouping (localedef syntax).
const LOCALES: [(&str, char, char, &str); 5] =
    [("grp3", ',', '.', "3;3"), ("grp32", ',', '.', "3;2"), ("grpstop", ',', '.', "3;-1"), ("grpde", '.', ',', "3;3"), ("grpsame", '.', '.', "3;3")];

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn c(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).unwrap()
}

fn wide(s: &str) -> Vec<i32> {
    s.bytes().map(i32::from).chain([0]).collect()
}

/// Compile the test's own locales once per process (tests run in parallel and share the build
/// directory); `None` when localedef is missing.
fn locales() -> Option<std::path::PathBuf> {
    static ONCE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    ONCE.get_or_init(build_locales).clone()
}

fn build_locales() -> Option<std::path::PathBuf> {
    let dir = root().join("target/test-locales/strto-group");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).ok()?;
    for (name, sep, dp, g) in LOCALES {
        let dest = dir.join(name);
        if dest.join("LC_NUMERIC").exists() {
            continue;
        }
        let def = src.join(format!("{name}.def"));
        let text = format!("comment_char %\nescape_char /\nLC_NUMERIC\ndecimal_point \"<U{:04X}>\"\nthousands_sep \"<U{:04X}>\"\ngrouping {g}\nEND LC_NUMERIC\n", dp as u32, sep as u32);
        std::fs::write(&def, text).ok()?;
        let tmp = dir.join(format!(".build-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let made = std::process::Command::new("localedef").args(["-c", "-f", "UTF-8", "-i"]).arg(&def).arg(&tmp).output().is_ok_and(|o| o.status.code().is_some_and(|c| c <= 1));
        if made && tmp.join("LC_NUMERIC").exists() {
            let _ = std::fs::rename(&tmp, &dest);
        }
        let _ = std::fs::remove_dir_all(&tmp);
        if !dest.join("LC_NUMERIC").exists() {
            return None;
        }
    }
    Some(dir)
}

/// Run `check` alone in a re-executed child with `LOCPATH` naming the test's locales.
fn in_child(test: &str, check: impl FnOnce()) {
    if std::env::var(PROBE).is_ok_and(|v| v == test) {
        check();
        CSys.exit_group(0);
    }
    let Some(dir) = locales() else {
        return skip_note::skipped(&format!("skipped: no localedef; {test} not run"));
    };
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(PROBE, test).env("LOCPATH", &dir).output().unwrap();
    assert!(o.status.success(), "{test} failed in its child: {:?}\n{}\n{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

fn set_numeric(name: &str) {
    assert!(!s::setlocale(LC_NUMERIC, c(name).as_ptr()).is_null(), "setlocale(LC_NUMERIC, {name})");
}

/// One probe line for `s` in the current `LC_NUMERIC`, in the probe's format.
fn line(loc: &str, g: i32, s: &str) -> String {
    let cs = c(s);
    let p = cs.as_ptr();
    let w = wide(s);
    let wp = w.as_ptr();
    let mut e: *mut c_char = null_mut();
    let mut we: *mut i32 = null_mut();
    let off = |e: *mut c_char| e as isize - p as isize;
    let woff = |e: *mut i32| (e as isize - wp as isize) / 4;
    let mut o = format!("{loc} g={g} [{s}]");
    errno::set_raw(0);
    let v = n::__strtol_internal(p, Some(&mut e), 10, g);
    write!(o, " l={v}/{}/{}", off(e), errno::get()).unwrap();
    let v = n::__strtol_internal(p, Some(&mut e), 0, g);
    write!(o, " l0={v}/{}", off(e)).unwrap();
    let v = n::__strtoll_internal(p, Some(&mut e), 10, g);
    write!(o, " ll={v}/{}", off(e)).unwrap();
    errno::set_raw(0);
    let v = n::__strtoul_internal(p, Some(&mut e), 10, g);
    write!(o, " ul={v}/{}/{}", off(e), errno::get()).unwrap();
    let v = n::__strtoull_internal(p, Some(&mut e), 10, g);
    write!(o, " ull={v}/{}", off(e)).unwrap();
    let v = n::__wcstol_internal(wp, Some(&mut we), 10, g);
    write!(o, " wl={v}/{}", woff(we)).unwrap();
    let v = n::__wcstoll_internal(wp, Some(&mut we), 10, g);
    write!(o, " wll={v}/{}", woff(we)).unwrap();
    let v = n::__wcstoul_internal(wp, Some(&mut we), 10, g);
    write!(o, " wul={v}/{}", woff(we)).unwrap();
    let v = n::__wcstoull_internal(wp, Some(&mut we), 10, g);
    write!(o, " wull={v}/{}", woff(we)).unwrap();
    errno::set_raw(0);
    let v = n::__strtod_internal(p, Some(&mut e), g);
    write!(o, " d={:016x}/{}/{}", v.to_bits(), off(e), errno::get()).unwrap();
    let v = n::__strtof_internal(p, Some(&mut e), g);
    write!(o, " f={:08x}/{}", v.to_bits(), off(e)).unwrap();
    let v = n::liella_strtold_internal_bits(p, Some(&mut e), g);
    write!(o, " ld={:016x}:{:04x}/{}", v.mant, v.se, off(e)).unwrap();
    let v = n::__wcstod_internal(wp, Some(&mut we), g);
    write!(o, " wd={:016x}/{}", v.to_bits(), woff(we)).unwrap();
    let v = n::__wcstof_internal(wp, Some(&mut we), g);
    write!(o, " wf={:08x}/{}", v.to_bits(), woff(we)).unwrap();
    let v = n::liella_wcstold_internal_bits(wp, Some(&mut we), g);
    write!(o, " wld={:016x}:{:04x}/{}", v.mant, v.se, woff(we)).unwrap();
    o
}

/// Every form, both flag values, the C locale and the five grouping locales: the reference's
/// record line for line (§8.1 integers, §8.2 floats, §8.3 wide forms; no clause ID: the compat
/// `group` flag has no clause of its own).
#[test]
fn strto_internal_group_matches_the_reference_record() {
    in_child("strto_internal_group_matches_the_reference_record", || {
        let mut bad = Vec::new();
        for want in RECORD.lines() {
            let (loc, rest) = want.split_once(" g=").unwrap();
            let g: i32 = rest[..1].parse().unwrap();
            let input = &rest[rest.find('[').unwrap() + 1..rest.rfind(']').unwrap()];
            set_numeric(loc);
            let got = line(loc, g, input);
            if got != want {
                bad.push(format!("want {want}\n got {got}"));
            }
        }
        set_numeric("C");
        assert!(bad.is_empty(), "{} of {} lines differ:\n{}", bad.len(), RECORD.lines().count(), bad.join("\n"));
    });
}

/// Stressful environments (#2035): 100k separators stay linear (a quadratic check of each
/// candidate end would take minutes); a valid run reads to its end, a run of bad groups stops
/// after the first group (§8.1-§8.3; no clause ID: the compat `group` flag has none).
#[test]
fn strto_internal_group_100k_separators_is_linear() {
    in_child("strto_internal_group_100k_separators_is_linear", || {
        set_numeric("grp3");
        let good = format!("1{}", ",000".repeat(100_000));
        let bad = format!("1{}", ",00".repeat(100_000));
        let t = std::time::Instant::now();
        for (s, end) in [(&good, good.len()), (&bad, 1)] {
            let cs = c(s);
            let mut e: *mut c_char = null_mut();
            errno::set_raw(0);
            let v = n::__strtol_internal(cs.as_ptr(), Some(&mut e), 10, 1);
            assert_eq!((e as usize - cs.as_ptr() as usize, v == i64::MAX), (end, end > 1), "strtol over {} bytes", s.len());
            let _ = n::__strtod_internal(cs.as_ptr(), Some(&mut e), 1);
            assert_eq!(e as usize - cs.as_ptr() as usize, end, "strtod over {} bytes", s.len());
            let w = wide(s);
            let mut we: *mut i32 = null_mut();
            let _ = n::__wcstoul_internal(w.as_ptr(), Some(&mut we), 10, 1);
            assert_eq!((we as usize - w.as_ptr() as usize) / 4, end, "wcstoul over {} units", s.len());
        }
        set_numeric("C");
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "100k separators took {:?}", t.elapsed());
    });
}
