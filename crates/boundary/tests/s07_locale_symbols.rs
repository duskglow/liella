// Specified from spec §7 and its Appendix B list (strfmon, §7.5: phase 8; the catalogs, §7.7–7.8: phase 13). Generated stubs were
// replaced by real tests of the SAME NAME (see crates/spectest); `cargo xtask stubs` now only
// checks coverage. rpmatch: milestone 5 (2026-10-04, D-7.9).

//! Translation-layer tests for the exported symbols of spec §7 (35 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The built-in locales (`C`, `C.UTF-8`) need no files; the named locales are compared with the
//! reference library in `crates/core/tests/locale_products.rs`. Tests that set the global
//! locale hold [`GLOBAL`] and restore `"C"`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

use std::sync::Mutex;

#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use core::ffi::c_char;
use core::ptr::{null, null_mut};

use liella_abi::ctypes::locale_t;
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s07_locale as s;

const LC_CTYPE: i32 = 0;
const LC_NUMERIC: i32 = 1;
const LC_ALL: i32 = 6;
const LC_ALL_MASK: i32 = 0x1FBF;
const LC_NUMERIC_MASK: i32 = 1 << LC_NUMERIC;
const CODESET: i32 = 14;
const RADIXCHAR: i32 = 0x10000;
const GLOBAL_LOCALE: locale_t = usize::MAX as locale_t;

static GLOBAL: Mutex<()> = Mutex::new(());

struct Global(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for Global {
    fn drop(&mut self) {
        s::setlocale(LC_ALL, c("C").as_ptr());
    }
}

fn global() -> Global {
    Global(GLOBAL.lock().unwrap_or_else(|e| e.into_inner()))
}

fn c(t: &str) -> Vec<c_char> {
    t.bytes().map(|b| b as c_char).chain([0]).collect()
}

/// The C text at `p`, through the exported strlen/memcpy (no unsafe in tests).
fn text(p: *const c_char) -> String {
    assert!(!p.is_null());
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// `newlocale(mask, name, base)` and `duplocale` through either name.
fn objects(new: extern "C" fn(i32, *const c_char, locale_t) -> locale_t, dup: extern "C" fn(locale_t) -> locale_t, free: extern "C" fn(locale_t)) {
    let l = new(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
    assert!(!l.is_null());
    assert_eq!(text(s::nl_langinfo_l(CODESET, l)), "UTF-8");
    let d = dup(l);
    free(l);
    assert_eq!(text(s::nl_langinfo_l(CODESET, d)), "UTF-8", "the duplicate is independent");
    let m = new(LC_NUMERIC_MASK, c("C").as_ptr(), d);
    assert_eq!(text(s::nl_langinfo_l(CODESET, m)), "UTF-8", "base supplies the other categories");
    free(m);
    errno::set_raw(0);
    assert!(new(LC_ALL_MASK, c("xx_YY.NOPE").as_ptr(), null_mut()).is_null());
    assert_eq!(errno::get(), 2, "ENOENT");
    errno::set_raw(0);
    assert!(new(1 << 30, c("C").as_ptr(), null_mut()).is_null());
    assert_eq!(errno::get(), 22, "EINVAL");
}

#[test]
fn sym_setlocale() {
    let _g = global();
    assert_eq!(text(s::setlocale(LC_ALL, null())), "C");
    assert_eq!(text(s::setlocale(LC_CTYPE, c("C.UTF-8").as_ptr())), "C.UTF-8");
    let one = s::setlocale(LC_NUMERIC, c("POSIX").as_ptr());
    assert_eq!(text(one), "C", "a single category's name is a string (NUL-terminated)");
    assert!(text(s::setlocale(LC_ALL, null())).starts_with("LC_CTYPE=C.UTF-8;LC_NUMERIC=C;"));
    assert!(s::setlocale(LC_ALL, c("xx_YY.NOPE").as_ptr()).is_null());
    assert!(s::setlocale(-1, c("C").as_ptr()).is_null(), "an unknown category");
}

/// Repeated queries (answered from the query cache, WL-HOT-LS) follow every change of the
/// global locale, including a return to an earlier locale.
#[test]
fn sym_setlocale_query_follows_changes() {
    let _g = global();
    for round in 0..3 {
        for (name, want) in [("C.UTF-8", "C.UTF-8"), ("POSIX", "C"), ("C.UTF-8", "C.UTF-8")] {
            assert!(!s::setlocale(LC_TIME, c(name).as_ptr()).is_null());
            for _ in 0..2 {
                assert_eq!(text(s::setlocale(LC_TIME, null())), want, "round {round}");
                assert_eq!(text(s::setlocale(LC_CTYPE, null())), "C", "round {round}");
                let all = text(s::setlocale(LC_ALL, null()));
                let time = format!("LC_TIME={want};");
                assert_eq!(all.contains(&time), want != "C", "round {round}: {all}");
            }
        }
        assert!(!s::setlocale(LC_ALL, c("C").as_ptr()).is_null());
        assert_eq!(text(s::setlocale(LC_TIME, null())), "C");
        assert_eq!(text(s::setlocale(LC_ALL, null())), "C");
    }
}

#[test]
fn sym_newlocale() {
    objects(s::newlocale, s::duplocale, s::freelocale);
}

#[test]
fn sym___newlocale() {
    objects(s::__newlocale, s::__duplocale, s::__freelocale);
}

#[test]
fn sym_duplocale() {
    let g = s::duplocale(GLOBAL_LOCALE);
    assert!(!g.is_null() && g != GLOBAL_LOCALE);
    assert_eq!(text(s::nl_langinfo_l(RADIXCHAR, g)), ".");
    s::freelocale(g);
}

#[test]
fn sym___duplocale() {
    let g = s::__duplocale(GLOBAL_LOCALE);
    assert!(!g.is_null() && g != GLOBAL_LOCALE);
    s::__freelocale(g);
}

#[test]
fn sym_freelocale() {
    for _ in 0..100 {
        s::freelocale(s::newlocale(LC_ALL_MASK, c("C").as_ptr(), null_mut()));
    }
    s::freelocale(GLOBAL_LOCALE);
}

#[test]
fn sym___freelocale() {
    s::__freelocale(s::__newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut()));
}

fn use_in_thread(uselocale: extern "C" fn(locale_t) -> locale_t) {
    let l = s::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut()) as usize;
    std::thread::spawn(move || {
        assert_eq!(uselocale(null_mut()), GLOBAL_LOCALE, "0 queries: a fresh thread uses the global locale");
        assert_eq!(uselocale(l as locale_t), GLOBAL_LOCALE);
        assert_eq!(text(s::nl_langinfo(CODESET)), "UTF-8");
        assert_eq!(uselocale(null_mut()) as usize, l, "0 queries");
        assert_eq!(uselocale(GLOBAL_LOCALE) as usize, l);
    })
    .join()
    .unwrap();
    s::freelocale(l as locale_t);
}

#[test]
fn sym_uselocale() {
    use_in_thread(s::uselocale);
}

/// `tb::read::<Locale>()` (the one `%fs`-relative load, plan S2 *tb-field*) reads the same
/// word as the block reached through the thread pointer, on several threads with different
/// `uselocale` choices at once.
#[test]
fn tb_read_locale_matches_thread_block() {
    // Trace: no spec clause (thread-block access plumbing); S2 tb-field, D-X.16.
    use core::sync::atomic::Ordering::Relaxed;
    use liella_boundary::tb;
    let both = || (tb::read::<tb::Locale>(), tb::current().locale.load(Relaxed));
    let start = std::sync::Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|i| {
            let start = start.clone();
            std::thread::spawn(move || {
                let (r, b) = both();
                assert_eq!(r, b, "fresh thread");
                let l = s::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
                assert!(!l.is_null());
                s::uselocale(l);
                start.wait();
                assert_eq!(both(), (l as usize, l as usize), "thread {i} after uselocale");
                start.wait();
                s::uselocale(GLOBAL_LOCALE);
                let (r, b) = both();
                assert_eq!(r, b, "thread {i} back on the global locale");
                s::freelocale(l);
                l as usize
            })
        })
        .collect();
    let mut seen: Vec<usize> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 4, "each thread had its own locale object");
    assert_eq!(tb::read::<tb::Locale>(), tb::current().locale.load(Relaxed), "main thread");
}

#[test]
fn sym___uselocale() {
    use_in_thread(s::__uselocale);
}

#[test]
fn sym_localeconv() {
    let _g = global();
    let lc = s::localeconv();
    assert!(!lc.is_null());
    let mut first = [0usize; 1];
    st::memcpy(first.as_mut_ptr().cast(), lc.cast(), 8);
    assert_eq!(text(first[0] as *const c_char), ".", "decimal_point");
    assert_eq!(s::localeconv(), lc, "the same per-thread buffer");
}

#[test]
fn sym_nl_langinfo() {
    let _g = global();
    assert_eq!(text(s::nl_langinfo(CODESET)), "ANSI_X3.4-1968");
    s::setlocale(LC_CTYPE, c("C.UTF-8").as_ptr());
    assert_eq!(text(s::nl_langinfo(CODESET)), "UTF-8");
    assert_eq!(text(s::nl_langinfo(-5)), "", "unknown items give the empty string");
}

#[test]
fn sym_nl_langinfo_l() {
    let l = s::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
    assert_eq!(text(s::nl_langinfo_l(CODESET, l)), "UTF-8");
    assert_eq!(text(s::nl_langinfo_l(0xFFFF, l)), "C.UTF-8", "_NL_LOCALE_NAME of LC_CTYPE");
    s::freelocale(l);
}

#[test]
fn sym___nl_langinfo_l() {
    let l = s::newlocale(LC_ALL_MASK, c("C").as_ptr(), null_mut());
    assert_eq!(text(s::__nl_langinfo_l(CODESET, l)), "ANSI_X3.4-1968");
    s::freelocale(l);
}

// ------------------------------------------------------------------------------ §7.7, §7.8
//
// The gettext settings are process-wide and a translating locale needs `LOCPATH`: the checks
// that translate run in a re-executed child ([`translating`]) with `de_DE.UTF-8` compiled by the
// host's `localedef` into `target/test-locales` (without it they say so and pass) and the domain
// `sym` bound to fixture catalogs (`where` -> `wo` in `LC_MESSAGES` and `wann` in `LC_TIME`; `file`
// / `files` -> `Datei` / `Dateien`). The checks in this process use the C locale, which translates
// nothing, and absolute catalog paths.

use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;

const LC_TIME: i32 = 2;
const LC_MESSAGES: i32 = 5;
const PROBE: &str = "LIELLA_S7_SYMBOL";
const FIXTURES: &str = "LIELLA_S7_SYMBOL_DIR";

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A `.mo` image of `pairs` (sorted by key, no hash table), as the GNU gettext manual lays it out.
fn mo_image(pairs: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut p = pairs.to_vec();
    p.sort_by(|a, b| a.0.cmp(b.0));
    let n = p.len();
    let base = 28 + 16 * n;
    let (mut descs, mut strings) = (Vec::new(), Vec::new());
    for (k, _) in &p {
        descs.push((k.len(), base + strings.len()));
        strings.extend_from_slice(k);
        strings.push(0);
    }
    for (_, v) in &p {
        descs.push((v.len(), base + strings.len()));
        strings.extend_from_slice(v);
        strings.push(0);
    }
    let mut out = Vec::new();
    for w in [0x9504_12de, 0, n, 28, 28 + 8 * n, 0, base] {
        out.extend_from_slice(&(w as u32).to_le_bytes());
    }
    for (l, o) in descs {
        out.extend_from_slice(&(l as u32).to_le_bytes());
        out.extend_from_slice(&(o as u32).to_le_bytes());
    }
    out.extend_from_slice(&strings);
    out
}

/// A one-plane `.cat` image (Appendix D.2.2) with message 1 of set 1, `text`.
fn cat_image(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for w in [0x9604_08de_u32, 1, 1, 2, 1, 0, 0, 0, 0] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    out
}

fn write(dir: &std::path::Path, rel: &str, bytes: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Run `check` in a re-executed copy of `test`, alone, in `de_DE.UTF-8` with the domain `sym`
/// bound to the fixtures and the default domain `sym`.
fn translating(test: &str, check: impl FnOnce()) {
    if std::env::var(PROBE).is_ok_and(|v| v == test) {
        let dir = std::env::var(FIXTURES).unwrap();
        assert!(!s::setlocale(LC_ALL, c("de_DE.UTF-8").as_ptr()).is_null());
        assert!(!s::bindtextdomain(c("sym").as_ptr(), c(&dir).as_ptr()).is_null());
        assert!(!s::textdomain(c("sym").as_ptr()).is_null());
        check();
        CSys.exit_group(0);
    }
    let locales = root().join("target/test-locales");
    if !compile_locale_atomic(&locales.join("de_DE.UTF-8"), "de_DE", "UTF-8", "LC_MESSAGES") {
        return skip_note::skipped(&format!("skipped: no localedef; the translating checks of {test} not run"));
    }
    let dir = root().join(format!("target/test-scratch/s07-sym-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let header: &[u8] = b"Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=n != 1;\n";
    write(&dir, "de_DE.UTF-8/LC_MESSAGES/sym.mo", &mo_image(&[(b"", header), (b"where", b"wo"), (b"file\0files", b"Datei\0Dateien")]));
    write(&dir, "de_DE.UTF-8/LC_TIME/sym.mo", &mo_image(&[(b"", header), (b"where", b"wann")]));
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, test)
        .env(FIXTURES, &dir)
        .env("LOCPATH", &locales)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(o.status.success(), "{test} failed in its child: {}", child_report(&o));
}

/// In this process (the C locale): every lookup returns its argument itself and keeps `errno`.
fn untranslated(lookup: impl Fn(*const c_char) -> *mut c_char) {
    let _g = global();
    let id = c("where");
    errno::set_raw(1234);
    assert_eq!(lookup(id.as_ptr()), id.as_ptr().cast_mut());
    assert_eq!(errno::get(), 1234);
}

#[test]
fn sym_gettext() {
    untranslated(|p| s::gettext(p));
    assert!(s::gettext(null()).is_null(), "NULL is returned as given");
    translating("sym_gettext", || {
        assert_eq!(text(s::gettext(c("where").as_ptr())), "wo", "the default domain, LC_MESSAGES");
        let other = c("other");
        assert_eq!(s::gettext(other.as_ptr()), other.as_ptr().cast_mut());
    });
}

#[test]
fn sym_dgettext() {
    untranslated(|p| s::dgettext(c("sym").as_ptr(), p));
    translating("sym_dgettext", || {
        assert_eq!(text(s::dgettext(c("sym").as_ptr(), c("where").as_ptr())), "wo");
        assert_eq!(text(s::dgettext(null(), c("where").as_ptr())), "wo", "NULL: the default domain");
        assert_eq!(text(s::dgettext(c("none").as_ptr(), c("where").as_ptr())), "where");
    });
}

#[test]
fn sym___dgettext() {
    untranslated(|p| s::__dgettext(c("sym").as_ptr(), p));
    translating("sym___dgettext", || {
        assert_eq!(text(s::__dgettext(c("sym").as_ptr(), c("where").as_ptr())), "wo");
    });
}

#[test]
fn sym_dcgettext() {
    untranslated(|p| s::dcgettext(c("sym").as_ptr(), p, LC_TIME));
    translating("sym_dcgettext", || {
        assert_eq!(text(s::dcgettext(c("sym").as_ptr(), c("where").as_ptr(), LC_TIME)), "wann", "the category's catalog");
        assert_eq!(text(s::dcgettext(null(), c("where").as_ptr(), LC_MESSAGES)), "wo");
        assert_eq!(text(s::dcgettext(c("sym").as_ptr(), c("where").as_ptr(), LC_ALL)), "where", "LC_ALL: untranslated");
        assert_eq!(text(s::dcgettext(c("sym").as_ptr(), c("where").as_ptr(), 99)), "where");
    });
}

#[test]
fn sym___dcgettext() {
    untranslated(|p| s::__dcgettext(c("sym").as_ptr(), p, LC_MESSAGES));
    translating("sym___dcgettext", || {
        assert_eq!(text(s::__dcgettext(c("sym").as_ptr(), c("where").as_ptr(), LC_TIME)), "wann");
    });
}

/// The untranslated choice of the plural functions in this process: `msgid` for 1, else
/// `msgid_plural`.
fn untranslated_plural(f: impl Fn(*const c_char, *const c_char, u64) -> *mut c_char) {
    let _g = global();
    let (one, many) = (c("file"), c("files"));
    assert_eq!(f(one.as_ptr(), many.as_ptr(), 1), one.as_ptr().cast_mut());
    assert_eq!(f(one.as_ptr(), many.as_ptr(), 0), many.as_ptr().cast_mut());
    assert_eq!(f(one.as_ptr(), many.as_ptr(), u64::MAX), many.as_ptr().cast_mut(), "n is an unsigned long");
}

#[test]
fn sym_ngettext() {
    untranslated_plural(|a, b, n| s::ngettext(a, b, n));
    translating("sym_ngettext", || {
        let (one, many) = (c("file"), c("files"));
        assert_eq!(text(s::ngettext(one.as_ptr(), many.as_ptr(), 1)), "Datei");
        assert_eq!(text(s::ngettext(one.as_ptr(), many.as_ptr(), 2)), "Dateien");
    });
}

#[test]
fn sym_dngettext() {
    untranslated_plural(|a, b, n| s::dngettext(c("sym").as_ptr(), a, b, n));
    translating("sym_dngettext", || {
        let (one, many) = (c("file"), c("files"));
        assert_eq!(text(s::dngettext(c("sym").as_ptr(), one.as_ptr(), many.as_ptr(), 5)), "Dateien");
        assert_eq!(s::dngettext(c("none").as_ptr(), one.as_ptr(), many.as_ptr(), 5), many.as_ptr().cast_mut());
    });
}

#[test]
fn sym_dcngettext() {
    untranslated_plural(|a, b, n| s::dcngettext(c("sym").as_ptr(), a, b, n, LC_MESSAGES));
    translating("sym_dcngettext", || {
        let (one, many) = (c("file"), c("files"));
        assert_eq!(text(s::dcngettext(null(), one.as_ptr(), many.as_ptr(), 1, LC_MESSAGES)), "Datei");
        assert_eq!(s::dcngettext(c("sym").as_ptr(), one.as_ptr(), many.as_ptr(), 2, LC_TIME), many.as_ptr().cast_mut(), "LC_TIME's catalog lacks it");
    });
}

#[test]
fn sym_textdomain() {
    translating("sym_textdomain", || {
        assert_eq!(text(s::textdomain(null())), "sym");
        let mut name = c("other");
        let p = s::textdomain(name.as_ptr());
        assert_ne!(p, name.as_mut_ptr(), "a copy");
        assert_eq!(s::textdomain(null()), p, "the query returns the same copy");
        name[0] = b'X' as c_char;
        assert_eq!(text(p), "other");
        assert_eq!(text(s::textdomain(c("").as_ptr())), "messages", "\"\" restores the default");
    });
}

#[test]
fn sym_bindtextdomain() {
    translating("sym_bindtextdomain", || {
        assert_eq!(text(s::bindtextdomain(c("unbound").as_ptr(), null())), "/usr/share/locale");
        assert!(s::bindtextdomain(null(), c("/x").as_ptr()).is_null(), "a NULL domain");
        assert!(s::bindtextdomain(c("").as_ptr(), c("/x").as_ptr()).is_null(), "an empty domain");
        let dir = c("/nowhere");
        let p = s::bindtextdomain(c("sym").as_ptr(), dir.as_ptr());
        assert_eq!(text(p), "/nowhere");
        assert_ne!(p, dir.as_ptr().cast_mut(), "a copy");
        assert_eq!(text(s::gettext(c("where").as_ptr())), "where", "no catalog there");
    });
}

#[test]
fn sym_bind_textdomain_codeset() {
    translating("sym_bind_textdomain_codeset", || {
        assert!(s::bind_textdomain_codeset(c("sym").as_ptr(), null()).is_null(), "never set");
        assert!(s::bind_textdomain_codeset(null(), c("UTF-8").as_ptr()).is_null());
        assert_eq!(text(s::bind_textdomain_codeset(c("sym").as_ptr(), c("ASCII").as_ptr())), "ASCII");
        assert_eq!(text(s::bind_textdomain_codeset(c("sym").as_ptr(), null())), "ASCII");
        assert!(text(s::bindtextdomain(c("sym").as_ptr(), null())).len() > 1, "the directory is kept");
        assert_eq!(text(s::gettext(c("where").as_ptr())), "wo");
        assert!(!s::bind_textdomain_codeset(c("sym").as_ptr(), c("NOPE").as_ptr()).is_null());
        assert_eq!(text(s::gettext(c("where").as_ptr())), "where", "an unknown codeset: untranslated");
    });
}

#[test]
fn sym__nl_msg_cat_cntr() {
    use std::sync::atomic::Ordering::SeqCst;
    translating("sym__nl_msg_cat_cntr", || {
        let cntr = &liella_boundary::data::_nl_msg_cat_cntr;
        let before = cntr.load(SeqCst);
        s::textdomain(null());
        s::gettext(c("where").as_ptr());
        assert_eq!(cntr.load(SeqCst), before, "queries and lookups leave it");
        s::textdomain(c("sym").as_ptr());
        assert_ne!(cntr.load(SeqCst), before, "a domain set moves it");
        let mid = cntr.load(SeqCst);
        s::setlocale(LC_MESSAGES, c("C").as_ptr());
        assert_ne!(cntr.load(SeqCst), mid, "a locale change moves it");
    });
}

#[test]
fn sym__nl_domain_bindings() {
    use std::sync::atomic::Ordering::SeqCst;
    translating("sym__nl_domain_bindings", || {
        assert_ne!(liella_boundary::data::_nl_domain_bindings.load(SeqCst), 0, "non-NULL after a binding");
    });
}

#[test]
fn sym__nl_default_dirname() {
    assert_eq!(&liella_boundary::data::_nl_default_dirname, b"/usr/share/locale\0");
    assert_eq!(size_of_val(&liella_boundary::data::_nl_default_dirname), 18, "the symbol's size");
}

#[test]
fn sym__libc_intl_domainname() {
    assert_eq!(&liella_boundary::data::_libc_intl_domainname, b"libc\0");
    assert_eq!(size_of_val(&liella_boundary::data::_libc_intl_domainname), 5, "the symbol's size");
}

/// A catalog file below `target/test-scratch` for the X/Open tests in this process.
fn cat_file(name: &str, bytes: &[u8]) -> String {
    let dir = root().join("target/test-scratch");
    let file = format!("s07-sym-{}-{name}", std::process::id());
    write(&dir, &file, bytes);
    dir.join(file).to_string_lossy().into_owned()
}

#[test]
fn sym_catopen() {
    let path = cat_file("open.cat", &cat_image("opened"));
    errno::set_raw(0);
    let cd = s::catopen(c(&path).as_ptr(), 0);
    assert_ne!(cd as usize, usize::MAX);
    assert_eq!(errno::get(), 0, "errno untouched on success");
    assert_eq!(text(s::catgets(cd, 1, 1, null())), "opened");
    assert_eq!(s::catclose(cd), 0);
    errno::set_raw(0);
    assert_eq!(s::catopen(c(&format!("{path}.missing")).as_ptr(), 0) as usize, usize::MAX, "(nl_catd)-1");
    assert_eq!(errno::get(), 2, "ENOENT");
    errno::set_raw(0);
    assert_eq!(s::catopen(null(), 0) as usize, usize::MAX, "a NULL name");
    assert_eq!(errno::get(), 22, "EINVAL");
    let _ = std::fs::remove_file(path);
}

#[test]
fn sym_catgets() {
    let path = cat_file("gets.cat", &cat_image("got"));
    let cd = s::catopen(c(&path).as_ptr(), 0);
    let dflt = c("dflt");
    errno::set_raw(4);
    let p = s::catgets(cd, 1, 1, dflt.as_ptr());
    assert_eq!(text(p), "got");
    assert_eq!(errno::get(), 4, "found: errno untouched");
    errno::set_raw(0);
    assert_eq!(s::catgets(cd, 1, 2, dflt.as_ptr()), dflt.as_ptr().cast_mut(), "the default itself");
    assert_eq!(errno::get(), 42, "ENOMSG");
    errno::set_raw(0);
    assert!(s::catgets(cd, 9, 9, null()).is_null(), "a NULL default is returned as NULL");
    assert_eq!(s::catclose(cd), 0);
    errno::set_raw(0);
    assert_eq!(s::catgets(usize::MAX as *mut core::ffi::c_void, 1, 1, dflt.as_ptr()), dflt.as_ptr().cast_mut());
    assert_eq!(errno::get(), 9, "EBADF");
    let _ = std::fs::remove_file(path);
}

#[test]
fn sym_catclose() {
    let path = cat_file("close.cat", &cat_image("x"));
    let cd = s::catopen(c(&path).as_ptr(), 0);
    errno::set_raw(4);
    assert_eq!(s::catclose(cd), 0);
    assert_eq!(errno::get(), 4);
    errno::set_raw(0);
    assert_eq!(s::catclose(usize::MAX as *mut core::ffi::c_void), -1);
    assert_eq!(errno::get(), 9, "EBADF");
    errno::set_raw(0);
    assert_eq!(s::catclose(null_mut()), -1);
    assert_eq!(errno::get(), 9, "EBADF");
    let _ = std::fs::remove_file(path);
}

/// The C locale's answers (`YESEXPR` `^[yY]`, `NOEXPR` `^[nN]`), as the host's glibc gives them
/// (probed 2026-10-04), globally and as a thread's own locale (D-7.9); NULL is -1 (the
/// reference crashes).
#[test]
fn sym_rpmatch() {
    let _g = global();
    assert!(!s::setlocale(6, c"C".as_ptr()).is_null());
    for (text, want) in [(c"y", 1), (c"Y", 1), (c"yes", 1), (c"n", 0), (c"N", 0), (c"no", 0), (c"nein", 0), (c"x", -1), (c"", -1), (c" y", -1), (c"ja", -1), (c"1", -1)] {
        assert_eq!(s::rpmatch(text.as_ptr()), want, "{text:?}");
    }
    assert_eq!(s::rpmatch(null()), -1, "NULL");
    let own = s::newlocale(0x1FBF, c"C".as_ptr(), null_mut());
    let before = s::uselocale(own);
    assert_eq!([s::rpmatch(c"yes".as_ptr()), s::rpmatch(c"No".as_ptr()), s::rpmatch(c"?".as_ptr())], [1, 0, -1]);
    s::uselocale(before);
    s::freelocale(own);
}

// §7.5: strfmon, strfmon_l and __strfmon_l are variadic C adapters over `liella_vstrfmon5`, which
// takes a `va_list`: nothing that reads a `va_list` can be called from this binary in the plain
// pass (it would link the C variadic adapters, whose targets only the products export). Each is
// checked through real C varargs (conformance/c/s07_symbols_strfmon.c, one block per symbol;
// s07_strfmon.c covers the clauses).

/// Run C conformance programs (conformance/c/<name>.c) against the products. `cargo xtask test`
/// builds the products first.
fn conformance(names: &[&str]) {
    // The runner rebuilds its programs in one directory: one run at a time.
    static RUNNER: Mutex<()> = Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

#[test]
fn langinfo_one_byte_items_are_nul_terminated() {
    // Clauses: s7_4_c_2
    // LANGINFO-DIFF: the "C"/"POSIX" one-byte items (monetary CHAR_MAX items, week-calendar items,
    // measurement) are strings of length 1, as on glibc 2.39 (the program is also run there as a probe).
    conformance(&["s07_langinfo_items"]);
}

#[test]
fn sym_strfmon() {
    // The calling thread's locale; a `double` and a `long double` amount; E2BIG.
    conformance(&["s07_symbols_strfmon", "s07_strfmon"]);
}

#[test]
fn sym___strfmon_l() {
    // The given locale object's conventions; E2BIG.
    conformance(&["s07_symbols_strfmon"]);
}

#[test]
fn sym_strfmon_l() {
    // The given locale object's conventions; E2BIG.
    conformance(&["s07_symbols_strfmon", "s07_strfmon"]);
}

#[test]
fn ctype_words_follow_uselocale_and_setlocale() {
    // D-5.5 (probes in spec/behaviour/ctype-words.md): a fetched `__ctype_*_loc` pointer and the
    // byte functions follow `setlocale`, `uselocale`, thread creation and `fork`; another
    // thread's `setlocale` reaches every thread on the global locale (D-7.8). Needs real threads
    // the runtime lists, hence a C program.
    // Clauses: s5_1_c_0, s7_1_c_p1, s7_2_t_0 (D-5.5, D-7.8).
    conformance(&["s05_ctype_words"]);
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

/// The child's exit status, stdout and stderr (the last 60 lines of each) for a failure message.
fn child_report(o: &std::process::Output) -> String {
    let tail = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(60)..].join("\n")
    };
    format!("exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}", o.status, tail(&o.stdout), tail(&o.stderr))
}

/// One round of the wide answers for U+0080..U+00FF: (`iswalpha`, `towupper`, `towlower`, `wcwidth`).
fn latin_round() -> Vec<(i32, u32, u32, i32)> {
    use liella_boundary::posix::{s05_ctype as ct, s06_multibyte as mb};
    (0x80u32..=0xff).map(|c| (ct::iswalpha(c), ct::towupper(c), ct::towlower(c), mb::wcwidth(c as i32))).collect()
}

#[test]
fn latin1_wide_answers_after_setlocale() {
    // WL-REGRESS-2: a compiled UTF-8 LC_CTYPE answers U+0080..U+00FF from a per-view cache filled
    // on first use (D-7.13). The first call (fill), a second call (cached) and a call after
    // switching to C.UTF-8 and back give the Unicode answers (s5_2_g_0, s5_3_c_0). Runs in a
    // re-executed child with en_US.UTF-8 compiled by the host's `localedef` (without it it says so).
    // Clauses: s5_2_c_0, s5_2_g_0, s5_3_c_0 (D-7.13).
    const TEST: &str = "latin1_wide_answers_after_setlocale";
    if std::env::var(PROBE).is_ok_and(|v| v == TEST) {
        assert!(!s::setlocale(LC_ALL, c("en_US.UTF-8").as_ptr()).is_null());
        let first = latin_round();
        let second = latin_round();
        assert_eq!(first, second, "cached answers equal the first-use fill");
        let at = |c: u32| first[(c - 0x80) as usize];
        // U+00E9 e-acute: alphabetic, upper U+00C9, width 1; U+00C9 lowers to U+00E9.
        assert_eq!(at(0xe9), (at(0xe9).0, 0xc9, 0xe9, 1));
        assert_ne!(at(0xe9).0, 0, "U+00E9 is alphabetic");
        assert_eq!((at(0xc9).1, at(0xc9).2), (0xc9, 0xe9));
        // U+00D7 and U+00F7 (multiplication, division signs): not alphabetic, map to themselves.
        assert_eq!(at(0xd7), (0, 0xd7, 0xd7, 1));
        assert_eq!(at(0xf7), (0, 0xf7, 0xf7, 1));
        // U+00FF y-diaeresis uppercases outside Latin-1 (U+0178); U+0080 is a control: width -1.
        assert_eq!(at(0xff).1, 0x178);
        assert_eq!(at(0x80), (0, 0x80, 0x80, -1));
        assert!(first.iter().filter(|e| e.0 != 0).count() > 50, "Latin-1 letters are alphabetic");
        assert!(!s::setlocale(LC_ALL, c("C.UTF-8").as_ptr()).is_null());
        assert!(!s::setlocale(LC_ALL, c("en_US.UTF-8").as_ptr()).is_null());
        assert_eq!(latin_round(), first, "after a second setlocale");
        CSys.exit_group(0);
    }
    let locales = root().join("target/test-locales");
    if !compile_locale_atomic(&locales.join("en_US.UTF-8"), "en_US", "UTF-8", "LC_CTYPE") {
        return skip_note::skipped(&format!("skipped: no localedef; {TEST} not run"));
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([TEST, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, TEST)
        .env("LOCPATH", &locales)
        .output()
        .unwrap();
    assert!(o.status.success(), "{TEST} failed in its child: {}", child_report(&o));
}
