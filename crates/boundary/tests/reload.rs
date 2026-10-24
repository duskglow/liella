//! Reloading time-zone and locale data on request (D-X.13): `liella_tz_invalidate` and
//! `liella_locale_invalidate`. Zones and locale files stay pinned until then; after an
//! invalidation the next use compares each file with what was read, reuses what did not change
//! (no growth, the same pointers) and reads again what did. Each scenario changes process-wide
//! state (`TZ`, `LOCPATH`, the global locale), so it runs in a re-executed child of this test
//! binary; the fixtures (copies of installed zone files, locales compiled with `localedef`) are
//! in a scratch directory under `CARGO_TARGET_TMPDIR`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use liella_abi::layout::struct_tm;
use liella_boundary::native::locale::liella_locale_invalidate;
use liella_boundary::native::tz::liella_tz_invalidate;
use liella_boundary::posix::s01_startup as env;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s07_locale as loc;
use liella_boundary::posix::s23_time as tm;

#[path = "../../core/tests/support/scratch.rs"]
mod scratch;

const PROBE: &str = "LIELLA_RELOAD_PROBE";
const LC_ALL: i32 = 6;
const DAY_1: i32 = 0x20007;
/// 2026-07-01 12:00:00 UTC: summer in Berlin (+2 h), Tokyo +9 h all year.
const T: i64 = 1_782_907_200;

/// Run the test `name` again in a child with the probe set and `dir` passed; the child's
/// output must end with `ok`.
fn in_child(name: &str, dir: &Path) {
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    for l in out.lines().filter(|l| l.contains("loaded ")) {
        println!("{l}");
    }
    assert!(o.status.success() && out.contains("child: ok"), "{name} in its child: {out}\n{}", String::from_utf8_lossy(&o.stderr));
}

fn probe() -> Option<PathBuf> {
    std::env::var_os(PROBE).map(PathBuf::from)
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn set(name: &str, value: &str) {
    assert_eq!(env::setenv(c(name).as_ptr(), c(value).as_ptr(), 1), 0);
}

/// Replace the file at `to` with a copy of `from` (a new inode, as a package update does).
fn replace(from: &Path, to: &Path) {
    let tmp = to.with_extension("new");
    std::fs::copy(from, &tmp).unwrap();
    std::fs::rename(&tmp, to).unwrap();
}

fn zero_tm() -> struct_tm {
    struct_tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0, tm_gmtoff: 0, tm_zone: 0 }
}

/// The NUL-terminated text at `addr` (read through the library's own `strlen` and `memcpy`).
fn text(addr: usize) -> Vec<u8> {
    let p = core::ptr::with_exposed_provenance::<core::ffi::c_char>(addr);
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    v
}

/// `localtime_r(T)`: the offset and the address of the abbreviation.
fn local() -> (i64, usize) {
    let mut out = zero_tm();
    assert!(!tm::localtime_r(Some(&T), Some(&mut out)).is_null());
    (out.tm_gmtoff, out.tm_zone)
}

/// `localtime(T)` (an implicit tzset): the offset.
fn implicit() -> i64 {
    let p = tm::localtime(Some(&T));
    assert!(!p.is_null());
    let mut out = zero_tm();
    st::memcpy(core::ptr::from_mut(&mut out).cast(), p.cast_const().cast(), size_of::<struct_tm>());
    out.tm_gmtoff
}

fn zone_fixture(d: &Path) -> PathBuf {
    let zone = d.join("zone");
    std::fs::copy("/usr/share/zoneinfo/Europe/Berlin", &zone).unwrap();
    zone
}

#[test]
fn tz_invalidate_reloads_a_changed_zone_file() {
    // Clauses: s23_6_c_0, s23_6_c_2, s23_6_g_0 (D-X.13, D-23.6).
    if let Some(d) = probe() {
        let zone = d.join("zone");
        set("TZ", &format!(":{}", zone.display()));
        tm::tzset();
        let (berlin, abbr) = local();
        assert_eq!(berlin, 7200);
        // Nothing changed: the same zone, the same abbreviation pointer (no new copy).
        let checked = std::time::Instant::now();
        assert_eq!(liella_tz_invalidate(), 0);
        assert_eq!(local(), (7200, abbr), "an unchanged file is reused");
        // Replaced: an implicit selection re-checks the file of the current zone at most once a second (D-23.6), so it
        // is pinned until an invalidation only while less than that has passed since the last check. How long the
        // steps take is the machine's business (a loaded host took 1.4 s here): the pin is asserted only when the
        // elapsed time proves no check was due (half the interval, for the coarse clock the check reads), and a
        // later implicit selection may already see the new file.
        replace(Path::new("/usr/share/zoneinfo/Asia/Tokyo"), &zone);
        let seen = implicit();
        if checked.elapsed() < std::time::Duration::from_millis(500) {
            assert_eq!(seen, 7200, "still the pinned zone (under the recheck interval since the last check)");
        } else {
            assert!(seen == 7200 || seen == 32400, "the pinned or the replaced zone, not a third: {seen}");
        }
        assert_eq!(liella_tz_invalidate(), 0);
        assert_eq!(local().0, 32400, "read again after the invalidation");
        assert_eq!(implicit(), 32400);
        // The old abbreviation is still readable (never freed).
        assert_eq!(text(abbr), b"CEST");
        // A removed file: the zone is read again from the rule fallback (UTC named by the value).
        std::fs::remove_file(&zone).unwrap();
        assert_eq!(liella_tz_invalidate(), 0);
        assert_eq!(local().0, 0, "a missing file is missing");
        println!("child: ok");
        return;
    }
    let d = scratch::Scratch::dir("reload-tz");
    zone_fixture(&d);
    in_child("tz_invalidate_reloads_a_changed_zone_file", &d);
}

/// Compile `name` (e.g. `en_US`) with `localedef` into `dir/<name>.UTF-8`; false without
/// `localedef`.
fn compile(dir: &Path, name: &str) -> bool {
    compile_locale_atomic(&dir.join(format!("{name}.UTF-8")), name, "UTF-8", "LC_TIME")
}

fn day1() -> Vec<u8> {
    text(loc::nl_langinfo(DAY_1).expose_provenance())
}

#[test]
fn locale_invalidate_reloads_a_changed_locale_file() {
    // Clauses: s7_1_c_0, s7_2_c_0, s7_2_c_1 (D-X.13, D-7.7).
    if let Some(d) = probe() {
        set("LOCPATH", &d.join("locales").display().to_string());
        let xx = d.join("locales/xx_XX.UTF-8");
        assert!(!loc::setlocale(LC_ALL, c("xx_XX.UTF-8").as_ptr()).is_null(), "the fixture locale loads");
        assert_eq!(day1(), b"Sunday");
        let before = loc::nl_langinfo(DAY_1).expose_provenance();
        assert_eq!(liella_locale_invalidate(), 0);
        assert_eq!((day1(), loc::nl_langinfo(DAY_1).expose_provenance()), (b"Sunday".to_vec(), before), "unchanged files are reused");
        // A replaced category file: pinned until an invalidation.
        replace(&d.join("locales/de_DE.UTF-8/LC_TIME"), &xx.join("LC_TIME"));
        assert!(!loc::setlocale(LC_ALL, c("xx_XX.UTF-8").as_ptr()).is_null());
        assert_eq!(day1(), b"Sunday", "still the pinned file");
        assert_eq!(liella_locale_invalidate(), 0);
        assert_eq!(day1(), b"Sonntag", "the global locale is loaded again");
        assert_eq!(text(before), b"Sunday", "the old item stays mapped");
        // A locale that disappeared: ENOENT, the global locale unchanged.
        std::fs::remove_dir_all(&xx).unwrap();
        assert_eq!(liella_locale_invalidate(), -2);
        assert_eq!(day1(), b"Sonntag");
        println!("child: ok");
        return;
    }
    let d = scratch::Scratch::dir("reload-locale");
    let locales = d.join("locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !(compile(&locales, "en_US") && compile(&locales, "de_DE")) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    std::fs::rename(locales.join("en_US.UTF-8"), locales.join("xx_XX.UTF-8")).unwrap();
    in_child("locale_invalidate_reloads_a_changed_locale_file", &d);
}

#[test]
fn reloads_race_with_readers() {
    // Clauses: s23_6_g_0, s7_2_c_0, s4_4_c_0 (D-X.13): conversions, nl_langinfo and strcoll in
    // other threads while the files are replaced and invalidated repeatedly: every answer is
    // one of the two valid ones, and nothing faults.
    if let Some(d) = probe() {
        let zone = d.join("zone");
        set("TZ", &format!(":{}", zone.display()));
        set("LOCPATH", &d.join("locales").display().to_string());
        tm::tzset();
        assert!(!loc::setlocale(LC_ALL, c("xx_XX.UTF-8").as_ptr()).is_null());
        let stop = AtomicBool::new(false);
        let seen = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    let (a, b) = (c("apple"), c("Banana"));
                    while !stop.load(Ordering::Relaxed) {
                        let off = local().0;
                        assert!(off == 7200 || off == 32400, "offset {off}");
                        let day = day1();
                        assert!(day == b"Sunday" || day == b"Sonntag", "{day:?}");
                        assert!(st::strcoll(a.as_ptr(), b.as_ptr()) < 0);
                        seen.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
            let swaps = [("/usr/share/zoneinfo/Asia/Tokyo", "de_DE.UTF-8"), ("/usr/share/zoneinfo/Europe/Berlin", "en_US.UTF-8")];
            for k in 0..60 {
                let (z, l) = swaps[k % 2];
                replace(Path::new(z), &zone);
                replace(&d.join("locales").join(l).join("LC_TIME"), &d.join("locales/xx_XX.UTF-8/LC_TIME"));
                assert_eq!(liella_tz_invalidate(), 0);
                assert_eq!(liella_locale_invalidate(), 0);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            stop.store(true, Ordering::Relaxed);
        });
        assert!(seen.load(Ordering::Relaxed) > 0);
        println!("child: ok");
        return;
    }
    let d = scratch::Scratch::dir("reload-race");
    zone_fixture(&d);
    let locales = d.join("locales");
    std::fs::create_dir_all(&locales).unwrap();
    if !(compile(&locales, "en_US") && compile(&locales, "de_DE")) {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let copied = std::process::Command::new("cp").arg("-r").arg(locales.join("en_US.UTF-8")).arg(locales.join("xx_XX.UTF-8")).status().unwrap();
    assert!(copied.success());
    in_child("reloads_race_with_readers", &d);
}

#[test]
fn a_fork_child_reloads_on_its_own() {
    // Clauses: s23_6_c_2, s23_6_g_0 (D-X.13): the child inherits the pinned zone and the
    // generation; its invalidation does not reach the parent, which keeps its zone until its own.
    if let Some(d) = probe() {
        let zone = d.join("zone");
        set("TZ", &format!(":{}", zone.display()));
        tm::tzset();
        assert_eq!(local().0, 7200);
        replace(Path::new("/usr/share/zoneinfo/Asia/Tokyo"), &zone);
        let child = |_: &liella_boundary::sys::CSys| -> i32 {
            let pinned = local().0 == 7200;
            let reloaded = liella_tz_invalidate() == 0 && local().0 == 32400;
            i32::from(!(pinned && reloaded))
        };
        assert_eq!(liella_boundary::selftest::in_child(child).unwrap(), liella_boundary::selftest::ChildEnd::Exited(0));
        assert_eq!(local().0, 7200, "the parent keeps its zone");
        assert_eq!(liella_tz_invalidate(), 0);
        assert_eq!(local().0, 32400);
        println!("child: ok");
        return;
    }
    let d = scratch::Scratch::dir("reload-fork");
    zone_fixture(&d);
    in_child("a_fork_child_reloads_on_its_own", &d);
}

#[test]
fn the_locale_file_table_is_bounded() {
    // Clauses: s7_2_c_0, s7_1_c_0 (D-7.7, D-X.13): loading more distinct locale files than the
    // tables hold fails those loads cleanly; a load that fails maps nothing (repeating it does
    // not grow the process's mappings), and the locales loaded before still work.
    const N: usize = 1100;
    if let Some(d) = probe() {
        set("LOCPATH", &d.join("many").display().to_string());
        let maps = || std::fs::read_to_string("/proc/self/maps").unwrap().lines().filter(|l| l.contains("/many/") && l.ends_with("/LC_CTYPE")).count();
        let mut loaded = 0;
        let mut failed = Vec::new();
        for k in 0..N {
            if loc::setlocale(0, c(&format!("l{k}.UTF-8")).as_ptr()).is_null() {
                failed.push(k);
            } else {
                loaded += 1;
            }
        }
        let after_first = maps();
        assert!(after_first <= liella_legacy::global_locale::FILES, "{after_first} mappings");
        assert!(after_first <= loaded, "every mapping belongs to a loaded locale ({after_first} for {loaded})");
        for _ in 0..20 {
            for &k in failed.iter().take(50) {
                assert!(loc::setlocale(0, c(&format!("l{k}.UTF-8")).as_ptr()).is_null(), "l{k} still fails");
            }
        }
        assert_eq!(maps(), after_first, "failed loads map nothing");
        assert!(!loc::setlocale(0, c("l0.UTF-8").as_ptr()).is_null(), "an early locale still loads");
        println!("loaded {loaded}, failed {}, mappings {after_first}", failed.len());
        println!("child: ok");
        return;
    }
    let d = scratch::Scratch::dir("reload-many");
    let locales = d.join("compiled");
    std::fs::create_dir_all(&locales).unwrap();
    if !compile(&locales, "en_US") {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let ctype = locales.join("en_US.UTF-8/LC_CTYPE");
    for k in 0..N {
        let dir = d.join(format!("many/l{k}.UTF-8"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::hard_link(&ctype, dir.join("LC_CTYPE")).unwrap();
    }
    in_child("the_locale_file_table_is_bounded", &d);
}

/// Compile locale `lang`.`charset` into `dest` with the host's `localedef`, atomically: it is built
/// in a private sibling directory and renamed into place, so that concurrent tests and test
/// binaries never see (or write into) a half-built locale. True when `dest/<probe>` exists after.
fn compile_locale_atomic(dest: &std::path::Path, lang: &str, charset: &str, probe: &str) -> bool {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if dest.join(probe).exists() {
        return true;
    }
    let Some(parent) = dest.parent() else { return false };
    let _ = std::fs::create_dir_all(parent);
    let tmp = parent.join(format!(".build-{}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed), dest.file_name().unwrap().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&tmp);
    let made = std::process::Command::new("localedef").args(["-i", lang, "-f", charset]).arg(&tmp).output().is_ok_and(|o| o.status.code().is_some_and(|c| c <= 1));
    if made && tmp.join(probe).exists() {
        // Losing the race (the destination appeared meanwhile) is fine: theirs is complete too.
        let _ = std::fs::rename(&tmp, dest);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    dest.join(probe).exists()
}
