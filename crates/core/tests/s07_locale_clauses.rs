// Specified from glibc-functional-spec.md §7. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §7 (37 clauses).
//!
//! The tests use the built-in locales (`C`, `POSIX`, `C.UTF-8`), which need no files; the named
//! locales are compared with the reference library line by line in `locale_products.rs`, and
//! their data formats in `locale_data.rs`. The global locale is process-wide, so the tests
//! that set it hold [`GLOBAL`] and restore `"C"`.
//!
//! `strfmon` (§7.5) takes C varargs: its clauses are checked through the C interface by the
//! self-checking program `conformance/c/s07_strfmon.c` (built against the products and run once
//! per test binary; each clause test requires its clause's checks to have run and passed), and
//! through `liella::monetary` with conventions that have a currency symbol and grouping, which
//! the built-in locales lack.
//!
//! The message catalogs (§7.7, §7.8) translate only in a named locale: those tests run in
//! re-executed children with a locale compiled by `localedef` (see the section's comment); they
//! are compared with the reference library in `catalogs_products.rs`.

#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::sync::{Mutex, OnceLock};

use core::ffi::c_char;
use core::ptr::{null, null_mut};

use liella::error::Errno;
use liella::locale::file::Cat;
use liella::monetary::{self, Amounts, Conventions, Placement};
use liella::num::{Float, Format, Rounding};
use liella_abi::ctypes::locale_t;
use liella_abi::layout::struct_lconv;
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s06_multibyte as mb;
use liella_boundary::posix::s07_locale as x;
use liella_legacy::global_locale as gl;

const LC_CTYPE: i32 = 0;
const LC_NUMERIC: i32 = 1;
const LC_TIME: i32 = 2;
const LC_COLLATE: i32 = 3;
const LC_MONETARY: i32 = 4;
const LC_MESSAGES: i32 = 5;
const LC_ALL: i32 = 6;
const LC_ALL_MASK: i32 = 0x1FBF;
const LC_NUMERIC_MASK: i32 = 1 << LC_NUMERIC;
const LC_CTYPE_MASK: i32 = 1 << LC_CTYPE;
const GLOBAL_LOCALE: locale_t = usize::MAX as locale_t;

const CODESET: i32 = 14;
const RADIXCHAR: i32 = 0x10000;
const DAY_1: i32 = 0x20007;
const D_T_FMT: i32 = 0x20028;
const YESEXPR: i32 = 0x50000;
const PAPER_HEIGHT: i32 = 0x70000;
const ADDRESS_POSTAL_FMT: i32 = 0x90000;

static GLOBAL: Mutex<()> = Mutex::new(());

/// Holds the global locale, and leaves it `"C"` when dropped.
struct Global(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for Global {
    fn drop(&mut self) {
        x::setlocale(LC_ALL, c("C").as_ptr());
    }
}

fn global() -> Global {
    Global(GLOBAL.lock().unwrap_or_else(|e| e.into_inner()))
}

fn c(s: &str) -> Vec<c_char> {
    s.bytes().map(|b| b as c_char).chain([0]).collect()
}

/// The C text at `p`, through the exported strlen/memcpy (no unsafe in tests).
fn text(p: *const c_char) -> String {
    assert!(!p.is_null());
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

fn set(cat: i32, name: &str) -> Option<String> {
    let p = x::setlocale(cat, c(name).as_ptr());
    (!p.is_null()).then(|| text(p))
}

fn query(cat: i32) -> String {
    text(x::setlocale(cat, null()))
}

fn langinfo(item: i32) -> String {
    text(x::nl_langinfo(item))
}

/// A copy of the `struct lconv` `localeconv` returned, through the exported memcpy.
fn lconv() -> struct_lconv {
    let mut v = struct_lconv {
        decimal_point: 0,
        thousands_sep: 0,
        grouping: 0,
        int_curr_symbol: 0,
        currency_symbol: 0,
        mon_decimal_point: 0,
        mon_thousands_sep: 0,
        mon_grouping: 0,
        positive_sign: 0,
        negative_sign: 0,
        int_frac_digits: 0,
        frac_digits: 0,
        p_cs_precedes: 0,
        p_sep_by_space: 0,
        n_cs_precedes: 0,
        n_sep_by_space: 0,
        p_sign_posn: 0,
        n_sign_posn: 0,
        int_p_cs_precedes: 0,
        int_p_sep_by_space: 0,
        int_n_cs_precedes: 0,
        int_n_sep_by_space: 0,
        int_p_sign_posn: 0,
        int_n_sign_posn: 0,
    };
    st::memcpy((&raw mut v).cast(), x::localeconv().cast(), size_of::<struct_lconv>());
    v
}

fn field(p: usize) -> String {
    text(p as *const c_char)
}

#[test]
fn s7_1_c_0() {
    let _g = global();
    // Each category is selected separately.
    set(LC_ALL, "C.UTF-8").unwrap();
    set(LC_CTYPE, "C").unwrap();
    assert_eq!(langinfo(CODESET), "ANSI_X3.4-1968", "LC_CTYPE alone changed");
    assert_eq!(query(LC_NUMERIC), "C.UTF-8", "the other categories kept theirs");
}

#[test]
fn s7_1_c_r1() {
    let _g = global();
    assert_eq!((langinfo(CODESET), mb::__ctype_get_mb_cur_max()), ("ANSI_X3.4-1968".into(), 1));
    set(LC_CTYPE, "C.UTF-8").unwrap();
    assert_eq!((langinfo(CODESET), mb::__ctype_get_mb_cur_max()), ("UTF-8".into(), 6), "the multibyte encoding");
    let mut w: i32 = 0;
    assert_eq!(mb::mbrtowc(Some(&mut w), c("é").as_ptr(), 2, None), 2);
    assert_eq!(w, 0xE9);
}

#[test]
fn s7_1_c_r2() {
    let _g = global();
    assert_eq!(set(LC_COLLATE, "C.UTF-8").as_deref(), Some("C.UTF-8"));
    // The built-in locales order by code point, which UTF-8 byte order preserves.
    assert!(st::strcoll(c("a").as_ptr(), c("é").as_ptr()) < 0);
    assert!(st::strcoll(c("é").as_ptr(), c("😀").as_ptr()) < 0);
}

#[test]
fn s7_1_c_r3() {
    let _g = global();
    assert_eq!(langinfo(RADIXCHAR), ".");
    let lc = lconv();
    assert_eq!(field(lc.decimal_point), ".");
    assert_eq!(field(lc.thousands_sep), "", "no grouping in C");
}

#[test]
fn s7_1_c_r4() {
    let _g = global();
    let lc = lconv();
    assert_eq!(field(lc.currency_symbol), "", "C has no currency");
    assert_eq!((lc.frac_digits, lc.int_frac_digits), (127, 127), "CHAR_MAX: unspecified");
}

#[test]
fn s7_1_c_r5() {
    let _g = global();
    assert_eq!(langinfo(DAY_1), "Sunday");
    assert_eq!(langinfo(D_T_FMT), "%a %b %e %H:%M:%S %Y");
    assert_eq!(set(LC_TIME, "POSIX").as_deref(), Some("C"), "POSIX is C");
}

#[test]
fn s7_1_c_r6() {
    let _g = global();
    assert_eq!(langinfo(YESEXPR), "^[yY]");
    assert_eq!(set(LC_MESSAGES, "C.UTF-8").as_deref(), Some("C.UTF-8"));
}

#[test]
fn s7_1_c_r7() {
    let _g = global();
    for cat in 7..=12 {
        assert_eq!(set(cat, "C.UTF-8").as_deref(), Some("C.UTF-8"), "GNU category {cat}");
        assert_eq!(query(cat), "C.UTF-8");
    }
    assert_eq!(langinfo(ADDRESS_POSTAL_FMT), "%a%N%f%N%d%N%b%N%s %h %e %r%N%C-%z %T%N%c%N");
    assert_eq!(x::nl_langinfo(PAPER_HEIGHT) as usize, 297, "a word item is its value (D-7.10)");
}

#[test]
fn s7_1_c_p1() {
    let _g = global();
    let addr = x::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut()) as usize;
    assert_ne!(addr, 0);
    let seen = std::thread::spawn(move || {
        x::uselocale(addr as locale_t);
        let r = (langinfo(CODESET), mb::__ctype_get_mb_cur_max());
        x::uselocale(GLOBAL_LOCALE);
        r
    })
    .join()
    .unwrap();
    assert_eq!(seen, ("UTF-8".into(), 6), "the thread's own locale");
    assert_eq!(langinfo(CODESET), "ANSI_X3.4-1968", "the global locale elsewhere");
    for name in ["C", "POSIX", "C.UTF-8", "C.utf8"] {
        assert!(set(LC_ALL, name).is_some(), "{name} is built in");
    }
    x::freelocale(addr as locale_t);
}

#[test]
fn s7_2_c_0() {
    let _g = global();
    assert_eq!(set(LC_ALL, "C.UTF-8").as_deref(), Some("C.UTF-8"), "every category");
    for cat in [LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE, LC_MONETARY, LC_MESSAGES] {
        assert_eq!(query(cat), "C.UTF-8");
    }
    assert_eq!(set(LC_NUMERIC, "C").as_deref(), Some("C"), "one category");
}

#[test]
fn s7_2_c_1() {
    let _g = global();
    assert_eq!(query(LC_ALL), "C");
    set(LC_CTYPE, "C.UTF-8").unwrap();
    assert_eq!(query(LC_CTYPE), "C.UTF-8");
    assert_eq!(
        query(LC_ALL),
        "LC_CTYPE=C.UTF-8;LC_NUMERIC=C;LC_TIME=C;LC_COLLATE=C;LC_MONETARY=C;LC_MESSAGES=C;LC_PAPER=C;LC_NAME=C;LC_ADDRESS=C;LC_TELEPHONE=C;LC_MEASUREMENT=C;LC_IDENTIFICATION=C",
        "a composite name"
    );
}

#[test]
fn s7_2_c_2() {
    // The order of the variables (the setlocale(cat, "") lookup, pure).
    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&[u8]) -> Option<&'static [u8]> {
        move |k| pairs.iter().find(|(n, _)| n.as_bytes() == k).map(|(_, v)| v.as_bytes())
    }
    let all = env(&[("LC_ALL", "C.UTF-8"), ("LC_NUMERIC", "POSIX"), ("LANG", "C")]);
    assert_eq!(gl::env_name(Cat::Numeric, &all), b"C.UTF-8", "LC_ALL first");
    let cat = env(&[("LC_NUMERIC", "POSIX"), ("LANG", "C.UTF-8")]);
    assert_eq!(gl::env_name(Cat::Numeric, &cat), b"POSIX", "then LC_<cat>");
    assert_eq!(gl::env_name(Cat::Time, &cat), b"C.UTF-8", "then LANG");
    let empty = env(&[("LC_ALL", ""), ("LANG", "")]);
    assert_eq!(gl::env_name(Cat::Time, &empty), b"C", "empty values are ignored; else C");
}

#[test]
fn s7_2_c_3() {
    let _g = global();
    set(LC_CTYPE, "C.UTF-8").unwrap();
    set(LC_TIME, "POSIX").unwrap();
    let saved = query(LC_ALL);
    set(LC_ALL, "C").unwrap();
    assert_eq!(set(LC_ALL, &saved).as_deref(), Some(saved.as_str()), "the composite name restores");
    assert_eq!((query(LC_CTYPE), query(LC_NUMERIC)), ("C.UTF-8".into(), "C".into()));
}

#[test]
fn s7_2_c_4() {
    let _g = global();
    // Two compiled locales under one name: UTF-8 in a/, ISO-8859-1 in b/.
    let base = root().join("target/test-locales-same");
    let (a, b) = (base.join("a"), base.join("b"));
    if !oracle::locale(&a.join("xx"), "de_DE", "UTF-8", "LC_CTYPE")
        || !oracle::locale(&b.join("xx"), "de_DE", "ISO-8859-1", "LC_CTYPE")
    {
        skip_note::skipped("skipped: no localedef");
        return;
    }
    let (a, b) = (a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned());
    let base = base.to_string_lossy().into_owned();
    in_child("s7_2_c_4", "same", &base, &[("LOCPATH", &a), ("LC_ALL", "")], || {
        need(LC_ALL, "xx");
        assert_eq!(langinfo(CODESET), "UTF-8");
        setenv("LOCPATH", &b);
        assert_eq!(set(LC_ALL, "xx").as_deref(), Some("xx"));
        assert_eq!(langinfo(CODESET), "UTF-8", "LC_ALL: the same name keeps its data (LOCPATH not read again)");
        assert_eq!(set(LC_CTYPE, "xx").as_deref(), Some("xx"));
        assert_eq!(langinfo(CODESET), "UTF-8", "one category: the same");
        need(LC_CTYPE, "C");
        assert_eq!(set(LC_NUMERIC, "xx").as_deref(), Some("xx"), "the category's own name, not the composite");
        need(LC_CTYPE, "xx");
        assert_eq!(langinfo(CODESET), "ISO-8859-1", "after another name: looked up again");
        setenv("LOCPATH", &a);
        setenv("LC_ALL", "xx");
        assert_eq!(set(LC_ALL, "").as_deref(), Some("xx"));
        assert_eq!(langinfo(CODESET), "ISO-8859-1", "from the environment: the same");
        assert_eq!(query(LC_ALL), "xx");
    });
}

#[test]
fn s7_2_f_0() {
    let _g = global();
    set(LC_CTYPE, "C.UTF-8").unwrap();
    assert!(set(LC_ALL, "xx_YY.NOPE").is_none());
    assert!(set(LC_CTYPE, "C.UTF-8/../x").is_none(), "a name with '/' is not a locale");
    assert_eq!(query(LC_CTYPE), "C.UTF-8", "unchanged");
    assert!(set(LC_ALL, "LC_CTYPE=C;LC_NUMERIC=nope").is_none(), "a composite name with an unknown part");
    assert_eq!(query(LC_CTYPE), "C.UTF-8");
    assert!(x::setlocale(99, c("C").as_ptr()).is_null(), "an unknown category");
    // D-7.7 (owner ruling 2026-10-04): a locale name is never resolved as a path. `.`, `..`, and
    // names whose candidates would be empty, `.` or `..` (`..@euro` tries `..`; `@x` tries ``)
    // are not found, from LOCPATH directories whose parent or self is a compiled locale; the
    // same names from the environment leave the C locale.
    for n in [&b"."[..], b"..", b"", b"a/b", b"/x"] {
        assert!(!liella::locale::name::acceptable(n), "{n:?}");
    }
    let mut tried = Vec::new();
    let _ = liella::locale::name::candidates::<()>(&liella::locale::name::parse(b"..@euro"), |c| {
        tried.push(String::from_utf8_lossy(c).into_owned());
        None
    });
    assert_eq!(tried, ["..@euro", ".@euro", "@euro"], "`..`, `.` and `` are never tried");
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef (the path cases)");
        return;
    };
    let de = format!("{locales}/de_DE.UTF-8");
    let sub = format!("{de}/LC_MESSAGES");
    in_child("s7_2_f_0", "paths", &locales, &[("LOCPATH", &locales)], || {
        need(LC_ALL, "de_DE.UTF-8");
        set(LC_ALL, "C").unwrap();
        for (dir, names) in [(&de, &[".", "@x", "_x", "@"][..]), (&sub, &["..", "..@euro", "..@x", ".."][..])] {
            setenv("LOCPATH", dir);
            for &n in names {
                assert!(set(LC_ALL, n).is_none(), "{n} under {dir}");
                assert!(set(LC_CTYPE, n).is_none(), "{n} under {dir}");
                assert_eq!(query(LC_ALL), "C", "{n} under {dir}");
                setenv("LC_ALL", n);
                assert!(set(LC_ALL, "").is_none(), "LC_ALL={n} under {dir}");
                assert!(set(LC_MESSAGES, "").is_none(), "LC_ALL={n} under {dir}");
                assert_eq!(query(LC_ALL), "C", "LC_ALL={n} under {dir}: the C locale stays");
            }
            setenv("LC_ALL", "");
        }
        setenv("LOCPATH", &locales);
        setenv("LANG", "../de_DE.UTF-8/LC_MESSAGES/..");
        assert!(set(LC_ALL, "").is_none(), "LANG with a slash");
        setenv("LANG", "de_DE.UTF-8");
        assert_eq!(set(LC_ALL, "").as_deref(), Some("de_DE.UTF-8"), "the environment still works");
    });
}

#[test]
fn s7_2_t_0() {
    let _g = global();
    let own = x::newlocale(LC_ALL_MASK, c("C").as_ptr(), null_mut()) as usize;
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let t = std::thread::spawn(move || {
        x::uselocale(own as locale_t);
        wait.recv().unwrap();
        let r = langinfo(CODESET);
        x::uselocale(GLOBAL_LOCALE);
        r
    });
    set(LC_ALL, "C.UTF-8").unwrap();
    go.send(()).unwrap();
    assert_eq!(t.join().unwrap(), "ANSI_X3.4-1968", "a thread that called uselocale keeps its locale");
    assert_eq!(std::thread::spawn(|| langinfo(CODESET)).join().unwrap(), "UTF-8", "the others follow the global one");
    x::freelocale(own as locale_t);
}

#[test]
fn s7_2_g_0() {
    let _g = global();
    let p = x::setlocale(LC_ALL, c("C.UTF-8").as_ptr());
    assert_eq!(text(p), "C.UTF-8");
    assert_eq!(text(x::setlocale(LC_ALL, null())), "C.UTF-8", "the same name until the next change");
    // Names are interned for the process (D-7.8): an earlier result stays readable.
    let q = x::setlocale(LC_ALL, c("C").as_ptr());
    assert_eq!((text(p), text(q)), ("C.UTF-8".into(), "C".into()));
}

#[test]
fn s7_3_c_1() {
    let base = x::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
    let mixed = x::newlocale(LC_CTYPE_MASK, c("C").as_ptr(), base);
    assert!(!mixed.is_null(), "base is consumed into the result");
    assert_eq!(text(x::nl_langinfo_l(CODESET, mixed)), "ANSI_X3.4-1968", "LC_CTYPE from the name");
    // _NL_LOCALE_NAME(LC_NUMERIC): the object's own name for the category (setlocale would
    // report the global locale's).
    assert_eq!(text(x::nl_langinfo_l(LC_NUMERIC << 16 | 0xFFFF, mixed)), "C.UTF-8", "the rest from base");
    x::freelocale(mixed);
    let fresh = x::newlocale(LC_NUMERIC_MASK, c("C.UTF-8").as_ptr(), null_mut());
    assert_eq!(text(x::nl_langinfo_l(CODESET, fresh)), "ANSI_X3.4-1968", "a NULL base is C");
    x::freelocale(fresh);
}

#[test]
fn s7_3_c_2() {
    let l = x::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
    let d = x::duplocale(l);
    assert!(!d.is_null() && d != l);
    x::freelocale(l);
    assert_eq!(text(x::nl_langinfo_l(CODESET, d)), "UTF-8", "the copy outlives the original");
    x::freelocale(d);
    let g = x::duplocale(GLOBAL_LOCALE);
    assert!(!g.is_null() && g != GLOBAL_LOCALE, "the global locale is copied into an object");
    x::freelocale(g);
}

#[test]
fn s7_3_c_3() {
    for _ in 0..1000 {
        let l = x::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
        assert!(!l.is_null());
        x::freelocale(l);
    }
    x::freelocale(GLOBAL_LOCALE);
    x::freelocale(null_mut());
}

#[test]
fn s7_3_c_4() {
    std::thread::spawn(|| {
        assert_eq!(x::uselocale(null_mut()), GLOBAL_LOCALE, "initially the global locale");
        let l = x::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
        assert_eq!(x::uselocale(l), GLOBAL_LOCALE);
        assert_eq!(x::uselocale(null_mut()), l, "0 only queries");
        assert_eq!(langinfo(CODESET), "UTF-8");
        assert_eq!(x::uselocale(GLOBAL_LOCALE), l, "back to the global locale");
        assert_eq!(x::uselocale(null_mut()), GLOBAL_LOCALE);
        x::freelocale(l);
    })
    .join()
    .unwrap();
}

#[test]
fn s7_3_f_0() {
    errno::set_raw(0);
    assert!(x::newlocale(1 << 30, c("C").as_ptr(), null_mut()).is_null());
    assert_eq!(errno::get(), 22, "EINVAL: a bad mask");
    errno::set_raw(0);
    assert!(x::newlocale(LC_ALL_MASK, c("xx_YY.NOPE").as_ptr(), null_mut()).is_null());
    assert_eq!(errno::get(), 2, "ENOENT: an unknown name");
}

#[test]
fn s7_4_c_1() {
    let _g = global();
    set(LC_ALL, "C.UTF-8").unwrap();
    let lc = lconv();
    assert_eq!(field(lc.decimal_point), ".");
    assert_eq!(field(lc.int_curr_symbol), "");
    assert_eq!((lc.p_cs_precedes, lc.n_sign_posn), (127, 127));
}

#[test]
fn s7_4_c_2() {
    let _g = global();
    assert_eq!(langinfo(CODESET), "ANSI_X3.4-1968");
    let l = x::newlocale(LC_ALL_MASK, c("C.UTF-8").as_ptr(), null_mut());
    assert_eq!(text(x::nl_langinfo_l(CODESET, l)), "UTF-8");
    assert_eq!(text(x::__nl_langinfo_l(DAY_1, l)), "Sunday");
    assert_eq!(langinfo(0x7FFF_0000), "", "an unknown item: the empty string");
    x::freelocale(l);
}

#[test]
fn s7_4_g_0() {
    let _g = global();
    let p = x::nl_langinfo(CODESET);
    set(LC_NUMERIC, "C.UTF-8").unwrap();
    assert_eq!(text(p), "ANSI_X3.4-1968", "valid while LC_CTYPE is unchanged");
    // Locale data stay mapped for the process (D-7.7): earlier results stay readable.
    set(LC_CTYPE, "C.UTF-8").unwrap();
    assert_eq!(text(p), "ANSI_X3.4-1968");
}

// ------------------------------------------------------------------------------ §7.5

/// A C conformance program's run: its exit status and standard error.
struct CRun {
    status: Option<i32>,
    stderr: String,
}

static STRFMON: OnceLock<CRun> = OnceLock::new();

/// `conformance/c/<name>.c` built statically against the products (as `conformance/run.sh`
/// builds it, with its `LINK-LIBM` directive: `libm.a` before `libc.a`; `s07_strfmon.c` calls
/// `fesetround`) and run, once per test binary.
fn c_run(cell: &'static OnceLock<CRun>, name: &str) -> &'static CRun {
    cell.get_or_init(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (crt, libc) = (root.join("target/liella/liella-crt1.o"), root.join("target/liella/libc.a"));
        assert!(crt.exists() && libc.exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
        let src = root.join(format!("conformance/c/{name}.c"));
        let libm = std::fs::read_to_string(&src).unwrap().lines().any(|l| l.trim() == "/* LINK-LIBM: yes */").then(|| root.join("target/liella/libm.a"));
        assert!(libm.as_ref().is_none_or(|m| m.exists()), "{name}.c needs libm.a (LINK-LIBM): run `cargo xtask so`");
        let dir = root.join("target/test-oracles");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(format!("{name}-{}", std::process::id()));
        let b = std::process::Command::new("cc")
            .args(["-std=c17", "-O1", "-static", "-nostdlib", "-fno-builtin", "-fstack-protector-strong"])
            .arg(&crt)
            .arg(&src)
            .args(libm)
            .arg(&libc)
            .args(["-lgcc", "-o"])
            .arg(&exe)
            .output()
            .unwrap();
        assert!(b.status.success(), "{name}.c does not build: {}", String::from_utf8_lossy(&b.stderr));
        let r = std::process::Command::new(&exe).current_dir(&dir).stdin(std::process::Stdio::null()).output().unwrap();
        let _ = std::fs::remove_file(&exe);
        CRun { status: r.status.code(), stderr: String::from_utf8_lossy(&r.stderr).into_owned() }
    })
}

/// The checks of `clause` in the C program `name` ran to their end and passed.
fn c_clause(cell: &'static OnceLock<CRun>, name: &str, clause: &str) {
    let r = c_run(cell, name);
    let failed = r.stderr.lines().any(|l| l.starts_with(&format!("FAIL {clause}:")));
    let ended = r.stderr.lines().any(|l| l == format!("end {clause}"));
    assert!(ended && !failed, "{name}.c, {clause} (exit status {:?}):\n{}", r.status, r.stderr);
}

/// Amounts given as `f64` values, in order.
struct Doubles(Vec<f64>);

impl Amounts for Doubles {
    fn amount(&mut self, _long_double: bool) -> Float {
        Float::unpack(u128::from(self.0.remove(0).to_bits()), Format::F64)
    }
}

/// Conventions with a symbol before the amount, grouping by threes and the sign first (as the
/// usual `en_US` locale has them).
fn dollars() -> Conventions<'static> {
    let national = Placement { cs_precedes: 1, sep_by_space: 0, sign_posn: 1 };
    let international = Placement { cs_precedes: 1, sep_by_space: 1, sign_posn: 1 };
    Conventions {
        int_curr_symbol: b"USD ",
        currency_symbol: b"$",
        decimal_point: b".",
        thousands_sep: b",",
        grouping: &[3, 3],
        positive_sign: b"",
        negative_sign: b"-",
        int_frac_digits: 2,
        frac_digits: 2,
        national: [national; 2],
        international: [international; 2],
    }
}

/// `strfmon` through `liella::monetary` into a buffer of `max` bytes.
fn fmon(max: usize, fmt: &str, amounts: &[f64]) -> Result<String, Errno> {
    let mut buf = vec![0u8; max];
    let n = monetary::format(&mut buf, fmt.as_bytes(), &dollars(), &mut Doubles(amounts.to_vec()), Rounding::Nearest)?;
    Ok(String::from_utf8(buf[..n].to_vec()).unwrap())
}

/// §7.5: format `double` or `long double` amounts (`%n` national, `%i` international) by
/// `LC_MONETARY`; flags control the fill character, grouping, sign style (`+`, `(`) and the
/// currency symbol; at most `max` bytes are written. Through the C interface with real varargs
/// in the "C" locale (`conformance/c/s07_strfmon.c`), and through `liella::monetary` with
/// conventions that have a symbol and grouping.
#[test]
fn s7_5_c_0() {
    c_clause(&STRFMON, "s07_strfmon", "s7_5_c_0");
    let f = |fmt: &str, v: f64| fmon(64, fmt, &[v]).unwrap();
    assert_eq!(f("%n", 1234.567), "$1,234.57");
    assert_eq!(f("%n", -1234.567), "-$1,234.57");
    assert_eq!(f("%i", 1234.567), "USD 1,234.57", "the international symbol's first three bytes");
    assert_eq!(f("%^n", 1234.567), "$1234.57", "no grouping");
    assert_eq!(f("%!n", 1234.567), "1,234.57", "no symbol");
    assert_eq!(f("%(n", -1234.567), "($1,234.57)", "parentheses for negatives");
    assert_eq!(f("%(n", 1234.567), "$1,234.57");
    assert_eq!(f("%+n", -1234.567), "-$1,234.57", "the locale's signs");
    assert_eq!(f("%=0#8n", 1234.567), " $000001,234.57", "fill to 8 digits, separators counted");
    assert_eq!(f("%14n", 1234.567), "     $1,234.57");
    assert_eq!(f("%-14n|", 1234.567), "$1,234.57     |");
    assert_eq!(f("%.3i", 1234.567), "USD 1,234.567");
    assert_eq!(fmon(64, "%n and %i", &[1.0, 2.0]).unwrap(), "$1.00 and USD 2.00", "several amounts");
    assert_eq!(fmon(64, "%q", &[1.0]), Err(Errno::EINVAL));
}

/// §7.5: -1 with `E2BIG` when the result does not fit (with its terminator).
#[test]
fn s7_5_f_0() {
    c_clause(&STRFMON, "s07_strfmon", "s7_5_f_0");
    assert_eq!(fmon(10, "%n", &[1234.567]).unwrap(), "$1,234.57", "9 bytes and the terminator");
    for max in 0..10 {
        assert_eq!(fmon(max, "%n", &[1234.567]), Err(Errno::E2BIG), "{max}");
    }
}
/// §7.6: `rpmatch` classifies a response with the calling thread's locale's `YESEXPR` and
/// `NOEXPR` (POSIX extended expressions, searched for; the locales anchor them): 1, 0 or -1,
/// with no y/n fallback beyond the locale's own expressions (D-7.9). The global C and C.UTF-8
/// locales (both `^[yY]` and `^[nN]`), then a thread's own locale; the named locales are
/// compared with the reference in `locale_products.rs`.
#[test]
fn s7_6_c_0() {
    let _g = global();
    for name in [c"C", c"C.UTF-8"] {
        assert!(!x::setlocale(6, name.as_ptr()).is_null());
        assert_eq!((x::rpmatch(c"yes".as_ptr()), x::rpmatch(c"No".as_ptr()), x::rpmatch(c"maybe".as_ptr())), (1, 0, -1), "{name:?}");
    }
    assert!(!x::setlocale(6, c"C".as_ptr()).is_null());
    // On this thread only: the C locale's expressions, ^[yY] and ^[nN].
    let own = x::newlocale(LC_ALL_MASK, c("C").as_ptr(), null_mut()) as usize;
    assert_ne!(own, 0);
    let before = x::uselocale(own as locale_t);
    let m = |r: &str| x::rpmatch(c(r).as_ptr());
    let got: Vec<i32> = ["y", "Yes", "yes!", "n", "No", "nope", "x", "", " y", "1", "j"].iter().map(|r| m(r)).collect();
    assert_eq!(got, [1, 1, 1, 0, 0, 0, -1, -1, -1, -1, -1]);
    assert_eq!(text(x::nl_langinfo(YESEXPR)), "^[yY]", "the expression consulted");
    assert_eq!(x::rpmatch(null()), -1, "no response: unrecognized");
    x::uselocale(before);
    x::freelocale(own as locale_t);
}
// ------------------------------------------------------------------------------ §7.7, §7.8
//
// The gettext settings (default domain, bindings) and `LANGUAGE` are process-wide, and a named
// `LC_MESSAGES` locale needs `LOCPATH`: every test that translates runs in a re-executed child of
// its own ([`in_child`]), with a named locale compiled by the host's `localedef` into
// `target/test-locales` ([`test_locales`]; without `localedef` the test says so and passes) and
// catalogs written below `target/test-scratch` ([`fixtures`]). The catalog formats and the plural
// rules are also checked through `liella::catalog` directly.

use liella::catalog::{mo, plural, xopen};
use liella_boundary::posix::s01_startup as envx;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;

const PROBE: &str = "LIELLA_S7_PROBE";
const FIXTURES: &str = "LIELLA_S7_FIXTURES";
const ENOENT: i32 = 2;
const EBADF: i32 = 9;
const EINVAL: i32 = 22;
const ENOMSG: i32 = 42;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}


/// `target/test-locales` with `de_DE.UTF-8` compiled in it, or `None` without `localedef`.
fn test_locales() -> Option<String> {
    let dir = root().join("target/test-locales");
    if !oracle::locale(&dir.join("de_DE.UTF-8"), "de_DE", "UTF-8", "LC_MESSAGES") {
        return None;
    }
    Some(dir.to_string_lossy().into_owned())
}

/// A test's fixture directory; the parent removes it when the test ends, passed or failed.
struct Fixtures {
    path: String,
    owned: bool,
}

impl std::ops::Deref for Fixtures {
    type Target = str;
    fn deref(&self) -> &str {
        &self.path
    }
}

impl std::fmt::Display for Fixtures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.path)
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// The fixture directory of this test: in the parent a fresh directory that `make` fills, in a
/// child the parent's (from the environment).
fn fixtures(name: &str, make: impl FnOnce(&std::path::Path)) -> Fixtures {
    if let Ok(path) = std::env::var(FIXTURES) {
        return Fixtures { path, owned: false };
    }
    let dir = root().join(format!("target/test-scratch/s07-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = Fixtures { path: dir.to_string_lossy().into_owned(), owned: true };
    make(&dir);
    f
}

/// Run `body` alone in a child (the test `test` re-executed with the probe `name`), with `env` and
/// the fixture directory `dir` set. A failed assertion in the child fails the test with its output.
/// In a child for another probe, nothing happens.
fn in_child(test: &str, name: &str, dir: &str, env: &[(&str, &str)], body: impl FnOnce()) {
    match std::env::var(PROBE) {
        Ok(p) if p == name => {
            body();
            CSys.exit_group(0)
        }
        Ok(_) => return,
        Err(_) => {}
    }
    // The child starts without the tester's locale environment (a shell that exports LC_CTYPE or
    // LANG changes what `setlocale(LC_ALL, "")` composes); `env` names what the case needs.
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.starts_with("LC_") || k == "LANG" || k == "LANGUAGE" || k == "LOCPATH") {
            cmd.env_remove(k);
        }
    }
    let o = cmd
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .env(FIXTURES, dir)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{test}/{name} failed in its child: {}", child_report(&o));
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

/// A `.cat` image (Appendix D.2.2) of `(set, msg, text)` messages: plane size `size`, depth `depth`.
fn cat_image(msgs: &[(u32, u32, &str)], size: u32, depth: u32) -> Vec<u8> {
    let slots = (size * depth) as usize;
    let mut le = vec![0u8; slots * 12];
    let mut pool = Vec::new();
    for &(set, msg, s) in msgs {
        let key = set + 1;
        let idx = (key * msg) % size;
        let plane = (0..depth).find(|p| le[((p * size + idx) * 12) as usize..][..4] == [0; 4]).unwrap();
        let at = ((plane * size + idx) * 12) as usize;
        le[at..at + 4].copy_from_slice(&key.to_le_bytes());
        le[at + 4..at + 8].copy_from_slice(&msg.to_le_bytes());
        le[at + 8..at + 12].copy_from_slice(&(pool.len() as u32).to_le_bytes());
        pool.extend_from_slice(s.as_bytes());
        pool.push(0);
    }
    let mut out = Vec::new();
    for w in [0x9604_08de_u32, size, depth] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(&le);
    out.extend(vec![0u8; slots * 12]); // the big-endian table, not read on x86_64
    out.extend_from_slice(&pool);
    out
}

fn write(dir: &std::path::Path, rel: &str, bytes: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// A catalog with a UTF-8 header and `pairs`.
fn utf8_mo(extra_header: &str, pairs: &[(&[u8], &[u8])]) -> Vec<u8> {
    let header = format!("Content-Type: text/plain; charset=UTF-8\n{extra_header}");
    let mut all: Vec<(&[u8], &[u8])> = vec![(b"", header.as_bytes())];
    all.extend_from_slice(pairs);
    mo_image(&all)
}

/// `dcgettext(domain, id, category)` as text.
fn dc(domain: &str, id: &str, category: i32) -> String {
    text(x::dcgettext(c(domain).as_ptr(), c(id).as_ptr(), category))
}

fn bind_dir(domain: &str, dir: &str) {
    assert!(!x::bindtextdomain(c(domain).as_ptr(), c(dir).as_ptr()).is_null());
}

fn need(cat: i32, name: &str) {
    assert!(set(cat, name).is_some(), "setlocale({cat}, {name})");
}

fn setenv(name: &str, value: &str) {
    assert_eq!(envx::setenv(c(name).as_ptr(), c(value).as_ptr(), 1), 0);
}

/// The first byte of the C text at `p` (through the exported memcpy).
fn first_byte(p: *const c_char) -> u8 {
    let mut b = [0u8; 1];
    st::memcpy(b.as_mut_ptr().cast(), p.cast(), 1);
    b[0]
}

/// §7.7: the language is the calling thread's `LC_MESSAGES` locale (`C`, `POSIX` and `C.UTF-8`
/// translate nothing), or the elements of `LANGUAGE` in order, each msgid falling through to the
/// next element that has it; `LANGUAGE` is read at every lookup (D-7.5) and ignored for a `C`
/// `LC_MESSAGES`.
#[test]
fn s7_7_c_0() {
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c0", |d| {
        write(d, "de_DE.UTF-8/LC_MESSAGES/t.mo", &utf8_mo("", &[(b"where", b"de"), (b"only-de", b"nur-de")]));
        write(d, "it/LC_MESSAGES/t.mo", &utf8_mo("", &[(b"where", b"it")]));
        write(d, "nl/LC_MESSAGES/t.mo", &utf8_mo("", &[(b"where", b"nl")]));
        write(d, "de_DE.UTF-8/LC_TIME/t.mo", &utf8_mo("", &[(b"where", b"de-time")]));
    });
    // The hit plan (GETTEXT-4; D-7.12): after a few lookups have made a slot's plan, every
    // change that selects other catalogs or another answer is seen at the next lookup, as without
    // a plan. One child per kind of change, so a stale plan fails each one on its own.
    let put = |rel: &str, pairs: &[(&[u8], &[u8])]| {
        let at = std::path::Path::new(&*dir).join(rel);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        let new = at.with_extension("new");
        std::fs::write(&new, utf8_mo("", pairs)).unwrap();
        std::fs::rename(&new, &at).unwrap();
    };
    let warm = |domain: &str, id: &str, want: &str| {
        for k in 0..3 {
            assert_eq!(dc(domain, id, LC_MESSAGES), want, "lookup {k} before the change");
        }
    };
    let env = [("LOCPATH", &*locales)];
    // Every kind runs; the ones a stale plan answered are listed together at the end.
    let mut stale = Vec::new();
    put("de_DE.UTF-8/LC_MESSAGES/pa.mo", &[(b"where", b"pa")]);
    put("de_DE.UTF-8/LC_MESSAGES/pb.mo", &[(b"where", b"pb")]);
    kind(&mut stale, "plan_textdomain", &dir, &env, || {
        bind_dir("pa", &dir);
        bind_dir("pb", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        assert!(!x::textdomain(c("pa").as_ptr()).is_null());
        for _ in 0..3 {
            assert_eq!(text(x::gettext(c("where").as_ptr())), "pa");
        }
        assert!(!x::textdomain(c("pb").as_ptr()).is_null());
        assert_eq!(text(x::gettext(c("where").as_ptr())), "pb", "textdomain: the new domain's catalog");
    });
    put("pa/de_DE.UTF-8/LC_MESSAGES/pc.mo", &[(b"where", b"dir-a")]);
    put("pb/de_DE.UTF-8/LC_MESSAGES/pc.mo", &[(b"where", b"dir-b")]);
    kind(&mut stale, "plan_bindtextdomain", &dir, &env, || {
        need(LC_ALL, "de_DE.UTF-8");
        bind_dir("pc", &format!("{dir}/pa"));
        warm("pc", "where", "dir-a");
        bind_dir("pc", &format!("{dir}/pb"));
        assert_eq!(dc("pc", "where", LC_MESSAGES), "dir-b", "bindtextdomain: the new directory's catalog");
    });
    put("de_DE.UTF-8/LC_MESSAGES/pe.mo", &[(b"e", "\u{e9}".as_bytes())]);
    kind(&mut stale, "plan_codeset", &dir, &env, || {
        need(LC_ALL, "de_DE.UTF-8");
        bind_dir("pe", &dir);
        warm("pe", "e", "\u{e9}");
        assert!(!x::bind_textdomain_codeset(c("pe").as_ptr(), c("ISO-8859-1").as_ptr()).is_null());
        let e = x::dgettext(c("pe").as_ptr(), c("e").as_ptr());
        assert_eq!((st::strlen(e), first_byte(e)), (1, 0xe9), "bind_textdomain_codeset: converted");
    });
    kind(&mut stale, "plan_ctype", &dir, &env, || {
        need(LC_ALL, "de_DE.UTF-8");
        bind_dir("pe", &dir);
        warm("pe", "e", "\u{e9}");
        // The thread's own locale: the same LC_MESSAGES (the same slot), a C LC_CTYPE.
        let de = x::newlocale(LC_ALL_MASK, c("de_DE.UTF-8").as_ptr(), null_mut());
        assert!(!de.is_null());
        let ascii = x::newlocale(LC_CTYPE_MASK, c("C").as_ptr(), de);
        assert!(!ascii.is_null());
        let old = x::uselocale(ascii);
        let got = dc("pe", "e", LC_MESSAGES);
        x::uselocale(old);
        x::freelocale(ascii);
        assert_ne!(got, "\u{e9}", "another LC_CTYPE: not the catalog's UTF-8 bytes");
        assert_eq!(dc("pe", "e", LC_MESSAGES), "\u{e9}", "the global locale again");
    });
    kind(&mut stale, "plan_setlocale", &dir, &env, || {
        put("de_DE.UTF-8/LC_MESSAGES/ps.mo", &[(b"where", b"one")]);
        bind_dir("ps", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        warm("ps", "where", "one");
        put("de_DE.UTF-8/LC_MESSAGES/ps.mo", &[(b"where", b"two")]);
        need(LC_MESSAGES, "C");
        need(LC_MESSAGES, "de_DE.UTF-8");
        assert_eq!(dc("ps", "where", LC_MESSAGES), "two", "setlocale: the file checked at once");
    });
    put("it/LC_MESSAGES/pl.mo", &[(b"where", b"it")]);
    put("nl/LC_MESSAGES/pl.mo", &[(b"where", b"nl")]);
    kind(&mut stale, "plan_language", &dir, &env, || {
        bind_dir("pl", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        setenv("LANGUAGE", "it"); // env-ok: in the child (kind runs in_child)
        warm("pl", "where", "it");
        setenv("LANGUAGE", "nl"); // env-ok: in the child (kind runs in_child)
        assert_eq!(dc("pl", "where", LC_MESSAGES), "nl", "LANGUAGE: its catalog at the next lookup");
        assert_eq!(envx::unsetenv(c("LANGUAGE").as_ptr()), 0); // env-ok: in the child (kind runs in_child)
    });
    kind(&mut stale, "plan_recheck", &dir, &env, || {
        let _ = std::fs::remove_file(std::path::Path::new(&*dir).join("de_DE.UTF-8/LC_MESSAGES/pm.mo"));
        put("de/LC_MESSAGES/pm.mo", &[(b"where", b"late")]);
        bind_dir("pm", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        warm("pm", "where", "late");
        put("de_DE.UTF-8/LC_MESSAGES/pm.mo", &[(b"where", b"early")]);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(dc("pm", "where", LC_MESSAGES), "early", "a missing earlier path is tried again after a second");
    });
    kind(&mut stale, "plan_file", &dir, &env, || {
        put("de_DE.UTF-8/LC_MESSAGES/pf.mo", &[(b"where", b"old")]);
        bind_dir("pf", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        warm("pf", "where", "old");
        put("de_DE.UTF-8/LC_MESSAGES/pf.mo", &[(b"where", b"new")]);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(dc("pf", "where", LC_MESSAGES), "new", "a replaced file is seen after a second");
    });
    assert!(stale.is_empty(), "a stale answer after: {stale:?}");
    // The TSC gate (GETTEXT-6; D-7.14): a process that turned the counter off (PR_TSC_SIGSEGV, as replay tools
    // and sandboxes do) before its first lookup must not execute `rdtsc` (SIGSEGV kills the child) and must
    // still see a replaced file after a second.
    in_child("s7_7_c_0", "tsc_off", &dir, &[("LOCPATH", &locales)], || {
        const PR_TSC_SIGSEGV: u64 = 2;
        assert_eq!(CSys.prctl_int(liella_kernel::ops::PrctlInt::SetTsc, PR_TSC_SIGSEGV, 0), Ok(0), "PR_SET_TSC");
        let at = std::path::Path::new(&*dir).join("de_DE.UTF-8/LC_MESSAGES/tsc.mo");
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, utf8_mo("", &[(b"where", b"old")])).unwrap();
        bind_dir("tsc", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        for k in 0..4 {
            assert_eq!(dc("tsc", "where", LC_MESSAGES), "old", "lookup {k} with the counter off");
        }
        let new = at.with_extension("new");
        std::fs::write(&new, utf8_mo("", &[(b"where", b"new")])).unwrap();
        std::fs::rename(&new, &at).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(dc("tsc", "where", LC_MESSAGES), "new", "a replaced file is seen after a second");
    });
    in_child("s7_7_c_0", "lc_messages", &dir, &[("LOCPATH", &locales)], || {
        bind_dir("t", &dir);
        assert_eq!(dc("t", "where", LC_MESSAGES), "where", "the C locale translates nothing");
        need(LC_ALL, "de_DE.UTF-8");
        assert_eq!(dc("t", "where", LC_MESSAGES), "de");
        assert_eq!(dc("t", "where", LC_TIME), "de-time", "dcgettext: <dir>/<locale>/<CATEGORY>/<domain>.mo");
        assert_eq!(dc("t", "where", LC_ALL), "where", "LC_ALL is no category");
        for name in ["C", "POSIX", "C.UTF-8"] {
            need(LC_MESSAGES, name);
            assert_eq!(dc("t", "where", LC_MESSAGES), "where", "{name}");
        }
        // The thread's own locale counts.
        need(LC_MESSAGES, "C");
        let de = x::newlocale(1 << LC_MESSAGES, c("de_DE.UTF-8").as_ptr(), null_mut());
        assert!(!de.is_null());
        let old = x::uselocale(de);
        assert_eq!(dc("t", "where", LC_MESSAGES), "de");
        x::uselocale(old);
        x::freelocale(de);
    });
    in_child("s7_7_c_0", "language", &dir, &[("LOCPATH", &locales), ("LANGUAGE", "zz::it:de_DE.UTF-8")], || {
        bind_dir("t", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        assert_eq!(dc("t", "where", LC_MESSAGES), "it", "zz has no catalog; empty elements are skipped");
        assert_eq!(dc("t", "only-de", LC_MESSAGES), "nur-de", "per msgid, the next element");
        setenv("LANGUAGE", "de_DE.UTF-8");
        assert_eq!(dc("t", "where", LC_MESSAGES), "de", "read at every lookup");
        setenv("LANGUAGE", "C:it");
        assert_eq!(dc("t", "where", LC_MESSAGES), "where", "an element C ends the search");
        setenv("LANGUAGE", "it");
        need(LC_MESSAGES, "C");
        assert_eq!(dc("t", "where", LC_MESSAGES), "where", "ignored for a C LC_MESSAGES");
    });
    // `LANGUAGE` is compared by its text at every lookup (owner ruling 2026-10-05), never by the
    // identity of `environ` or of the entry: the value edited in place, and a new entry with the
    // same text, take effect (or not) at the next call, also when the search is cached.
    in_child("s7_7_c_0", "language_text", &dir, &[("LOCPATH", &locales)], || {
        bind_dir("t", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        setenv("LANGUAGE", "it");
        assert_eq!(dc("t", "where", LC_MESSAGES), "it");
        let value = envx::getenv(c("LANGUAGE").as_ptr());
        assert!(!value.is_null());
        for round in 0..50 {
            let (now, want) = if round % 2 == 0 { (b"nl", "nl") } else { (b"it", "it") };
            st::memcpy(value.cast(), now.as_ptr().cast(), 2);
            assert_eq!(envx::getenv(c("LANGUAGE").as_ptr()), value, "the same entry");
            assert_eq!(dc("t", "where", LC_MESSAGES), want, "edited in place, round {round}");
        }
        // putenv keeps the string: each one is leaked.
        let entry = |s: &str| std::ffi::CString::new(s).unwrap().into_raw();
        assert_eq!(envx::putenv(entry("LANGUAGE=nl")), 0);
        assert_eq!(dc("t", "where", LC_MESSAGES), "nl", "a new entry");
        assert_eq!(envx::putenv(entry("LANGUAGE=nl")), 0);
        assert_eq!(dc("t", "where", LC_MESSAGES), "nl", "a new entry with the same text");
        assert_eq!(envx::putenv(entry("LANGUAGE=it")), 0);
        assert_eq!(dc("t", "where", LC_MESSAGES), "it");
        assert_eq!(envx::unsetenv(c("LANGUAGE").as_ptr()), 0);
        assert_eq!(dc("t", "where", LC_MESSAGES), "de", "unset: the locale");
    });
    // A replaced catalog file is checked at most once a second, but at once after a `textdomain`,
    // `bindtextdomain` or `bind_textdomain_codeset` that changes its value, or a `setlocale` that
    // changes the locale (owner ruling 2026-10-05, revised 2026-10-23 #1056: the same value again
    // is no change; D-7.12): each of them sees the new file at the next lookup. The file is
    // replaced by a rename (a new inode, a new size).
    in_child("s7_7_c_0", "rechecks", &dir, &[("LOCPATH", &locales)], || {
        let path = std::path::Path::new(&*dir).join("de_DE.UTF-8/LC_MESSAGES/r.mo");
        let replace = |text: &str| {
            let new = path.with_extension("new");
            std::fs::write(&new, utf8_mo("", &[(b"where", text.as_bytes())])).unwrap();
            std::fs::rename(&new, &path).unwrap();
        };
        replace("v");
        bind_dir("r", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        assert_eq!(dc("r", "where", LC_MESSAGES), "v");
        let resets: [(&str, &dyn Fn()); 4] = [
            ("bindtextdomain", &|| bind_dir("r", &format!("{dir}/"))),
            ("textdomain", &|| assert!(!x::textdomain(c("r").as_ptr()).is_null())),
            ("bind_textdomain_codeset", &|| assert!(!x::bind_textdomain_codeset(c("r").as_ptr(), c("UTF-8").as_ptr()).is_null())),
            ("setlocale", &|| {
                need(LC_MESSAGES, "C");
                need(LC_MESSAGES, "de_DE.UTF-8");
            }),
        ];
        for (k, (name, reset)) in resets.iter().enumerate() {
            let text = "v".repeat(k + 2);
            replace(&text);
            reset();
            assert_eq!(dc("r", "where", LC_MESSAGES), text, "{name} checks the file at once");
        }
    });
    // A changed `LANGUAGE` is not a setting change (owner ruling 2026-10-07, P4; D-7.12): it
    // selects the cached search for its value, and catalogs keep their one-second clock, missing
    // paths included. Alternating values within a second neither see a replaced file nor a new
    // one (before the ruling every change checked each file and retried each missing path); a
    // second later both are seen.
    in_child("s7_7_c_0", "language_flips", &dir, &[("LOCPATH", &locales)], || {
        let base = std::path::Path::new(&*dir);
        let path = base.join("de_DE.UTF-8/LC_MESSAGES/f.mo");
        let added = base.join("nl/LC_MESSAGES/f.mo");
        let put = |at: &std::path::Path, text: &str| {
            let new = at.with_extension("new");
            std::fs::write(&new, utf8_mo("", &[(b"where", text.as_bytes())])).unwrap();
            std::fs::rename(&new, at).unwrap();
        };
        need(LC_ALL, "de_DE.UTF-8");
        let values = ["nl:de_DE.UTF-8", "de_DE.UTF-8"];
        let mut within = false;
        // A slow machine may take a second over one round: it is tried again (never passed).
        for _ in 0..5 {
            let _ = std::fs::remove_file(&added);
            put(&path, "old");
            bind_dir("f", &dir);
            let start = std::time::Instant::now();
            setenv("LANGUAGE", values[0]);
            assert_eq!(dc("f", "where", LC_MESSAGES), "old", "checked at once after bindtextdomain");
            put(&path, "new");
            put(&added, "nl");
            let mut got = Vec::new();
            for round in 0..20 {
                let v = values[round % 2];
                setenv("LANGUAGE", v);
                got.push((v, dc("f", "where", LC_MESSAGES)));
            }
            if start.elapsed() < std::time::Duration::from_millis(800) {
                for (v, text) in &got {
                    assert_eq!(text, "old", "LANGUAGE={v} within the second: no check, no retry");
                }
                within = true;
                break;
            }
        }
        assert!(within, "no round finished within 0.8 s");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        setenv("LANGUAGE", values[0]);
        assert_eq!(dc("f", "where", LC_MESSAGES), "nl", "a missing path is tried again after a second");
        setenv("LANGUAGE", values[1]);
        assert_eq!(dc("f", "where", LC_MESSAGES), "new", "a replaced file is seen after a second");
        assert_eq!(envx::unsetenv(c("LANGUAGE").as_ptr()), 0);
    });
    // A search whose first candidates are missing paths and whose catalog comes later (here
    // `de_DE.UTF-8` misses, `de` has it) skips the missing ones within the second (GETTEXT-3: the
    // candidate memo, D-7.12), but a catalog that appears at an earlier candidate is still seen
    // at once after each setting change, and a second later without one.
    in_child("s7_7_c_0", "memo", &dir, &[("LOCPATH", &locales)], || {
        let base = std::path::Path::new(&*dir);
        let early = base.join("de_DE.UTF-8/LC_MESSAGES/m.mo");
        let put = |at: &std::path::Path, text: &str| {
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            let new = at.with_extension("new");
            std::fs::write(&new, utf8_mo("", &[(b"where", text.as_bytes())])).unwrap();
            std::fs::rename(&new, at).unwrap();
        };
        put(&base.join("de/LC_MESSAGES/m.mo"), "late");
        bind_dir("m", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        let resets: [(&str, &dyn Fn()); 4] = [
            ("bindtextdomain", &|| bind_dir("m", &format!("{dir}/"))),
            ("textdomain", &|| assert!(!x::textdomain(c("m").as_ptr()).is_null())),
            ("bind_textdomain_codeset", &|| assert!(!x::bind_textdomain_codeset(c("m").as_ptr(), c("UTF-8").as_ptr()).is_null())),
            ("setlocale", &|| {
                need(LC_MESSAGES, "C");
                need(LC_MESSAGES, "de_DE.UTF-8");
            }),
        ];
        for (k, (name, reset)) in resets.into_iter().enumerate() {
            let _ = std::fs::remove_file(&early);
            // A new spelling each time: the same string again is no change (ruling 2026-10-23).
            bind_dir("m", &format!("{dir}{}", "/.".repeat(k + 1)));
            for _ in 0..3 {
                assert_eq!(dc("m", "where", LC_MESSAGES), "late", "{name}: the later candidate");
            }
            put(&early, name);
            reset();
            assert_eq!(dc("m", "where", LC_MESSAGES), name, "{name}: the new earlier catalog at once");
        }
        let _ = std::fs::remove_file(&early);
        bind_dir("m", &dir);
        assert_eq!(dc("m", "where", LC_MESSAGES), "late");
        put(&early, "second");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(dc("m", "where", LC_MESSAGES), "second", "a missing earlier path is tried again after a second");
        std::fs::remove_file(&early).unwrap();
        // A relative directory: its missing paths are tried at every lookup, so the memo never
        // covers them.
        std::env::set_current_dir(base).unwrap();
        bind_dir("m", ".");
        for _ in 0..3 {
            assert_eq!(dc("m", "where", LC_MESSAGES), "late", "relative: the later candidate");
        }
        put(&early, "relative");
        assert_eq!(dc("m", "where", LC_MESSAGES), "relative", "a relative missing path is tried at every lookup");
        std::fs::remove_file(&early).unwrap();
    });
}

/// [`in_child`] for one kind of change of the hit plan test: a failure is recorded in `stale`
/// (the other kinds still run).
fn kind(stale: &mut Vec<&'static str>, name: &'static str, dir: &str, env: &[(&str, &str)], body: impl FnOnce()) {
    if std::env::var(PROBE).is_ok() {
        return in_child("s7_7_c_0", name, dir, env, body);
    }
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| in_child("s7_7_c_0", name, dir, env, body))).is_err() {
        stale.push(name);
    }
}

/// §7.7: `gettext` returns the translation, or its argument itself; keys are whole msgids
/// (contexts `ctx\x04id` included), compared byte for byte.
#[test]
fn s7_7_c_1() {
    {
        let _g = global();
        let id = c("hello");
        assert_eq!(x::gettext(id.as_ptr()), id.as_ptr().cast_mut(), "no catalog: the argument itself");
    }
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c1", |d| {
        let pairs: [(&[u8], &[u8]); 4] = [(b"hello", b"hallo"), (b"menu\x04open", b"Oeffnen"), (b"empty", b""), (b"open", b"auf")];
        write(d, "de_DE.UTF-8/LC_MESSAGES/messages.mo", &utf8_mo("", &pairs));
    });
    in_child("s7_7_c_1", "gettext", &dir, &[("LOCPATH", &locales)], || {
        bind_dir("messages", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        let (hello, missing) = (c("hello"), c("missing"));
        let p = x::gettext(hello.as_ptr());
        assert_eq!(text(p), "hallo");
        assert_eq!(x::gettext(hello.as_ptr()), p, "the same library-owned string every time");
        assert_eq!(x::gettext(missing.as_ptr()), missing.as_ptr().cast_mut(), "untranslated: the argument itself");
        assert_eq!(text(x::gettext(c("menu\x04open").as_ptr())), "Oeffnen");
        assert_eq!(text(x::gettext(c("open").as_ptr())), "auf");
        let other = c("menu\x04close");
        assert_eq!(x::gettext(other.as_ptr()), other.as_ptr().cast_mut(), "no fall-back from a context to the bare msgid");
        assert_eq!(text(x::gettext(c("Hello").as_ptr())), "Hello");
        assert_eq!(text(x::gettext(c("empty").as_ptr())), "", "an empty translation");
        assert!(text(x::gettext(c("").as_ptr())).starts_with("Content-Type:"), "\"\" is the header entry");
    });
}

/// §7.7: `ngettext` selects a form with the catalog's `Plural-Forms` (the default rule `n != 1`
/// without one, or when it cannot be used; never trapping, D-7.5); without a translation it
/// returns `msgid` for 1 and `msgid_plural` otherwise.
#[test]
fn s7_7_c_2() {
    {
        let _g = global();
        let (one, many) = (c("file"), c("files"));
        assert_eq!(x::ngettext(one.as_ptr(), many.as_ptr(), 1), one.as_ptr().cast_mut());
        for n in [0, 2, u64::MAX] {
            assert_eq!(x::ngettext(one.as_ptr(), many.as_ptr(), n), many.as_ptr().cast_mut(), "{n}");
        }
    }
    let rule = |r: &str, n: u64| -> u64 {
        let p = plural::compile(format!("Plural-Forms: {r}\n").as_bytes()).unwrap();
        plural::evaluate(p.as_ref().map(|p| p.as_slice()), n)
    };
    let ru = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
    assert_eq!([1, 2, 5, 11, 21, 22, 111].map(|n| rule(ru, n)), [0, 1, 2, 2, 0, 1, 2]);
    assert_eq!(rule("nplurals=2; plural=n/0;", 5), 1, "a division by zero: the default rule");
    assert_eq!(rule("nplurals=2; plural=n;", 7), 0, "beyond nplurals: form 0");
    assert_eq!(rule("nplurals=2; plural=-n;", 1), 0, "unusable (unary minus): n != 1");
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c2", |d| {
        write(d, "de_DE.UTF-8/LC_MESSAGES/ru.mo", &utf8_mo(&format!("Plural-Forms: {ru}\n"), &[(b"file\0files", b"r0\0r1\0r2")]));
        write(d, "de_DE.UTF-8/LC_MESSAGES/none.mo", &utf8_mo("", &[(b"file\0files", b"Datei\0Dateien")]));
    });
    in_child("s7_7_c_2", "ngettext", &dir, &[("LOCPATH", &locales)], || {
        need(LC_ALL, "de_DE.UTF-8");
        bind_dir("ru", &dir);
        bind_dir("none", &dir);
        let (one, many) = (c("file"), c("files"));
        let ng = |d: &str, n: u64| text(x::dngettext(c(d).as_ptr(), one.as_ptr(), many.as_ptr(), n));
        assert_eq!([1, 2, 5, 21, 22].map(|n| ng("ru", n)), ["r0", "r1", "r2", "r0", "r1"]);
        assert_eq!([0, 1, 2].map(|n| ng("none", n)), ["Dateien", "Datei", "Dateien"], "no Plural-Forms: n != 1");
        assert_eq!(text(x::dgettext(c("none").as_ptr(), one.as_ptr())), "Datei", "gettext gives form 0");
        assert_eq!(text(x::dngettext(c("none").as_ptr(), one.as_ptr(), c("other").as_ptr(), 2)), "Dateien", "only the singular is the key");
    });
}

/// §7.7: domains. `textdomain` sets (a copy of) the default domain (`messages` initially, `""`
/// restores it); `bindtextdomain` sets where a domain's catalogs are (`dir/<lang>/LC_MESSAGES/
/// <domain>.mo`; `/usr/share/locale` unbound); `bind_textdomain_codeset` sets the output encoding
/// (`""`: as stored).
#[test]
fn s7_7_c_3() {
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c3", |d| {
        write(d, "a/de_DE.UTF-8/LC_MESSAGES/one.mo", &utf8_mo("", &[(b"where", b"a-one"), (b"e", "\u{e9}".as_bytes())]));
        write(d, "b/de_DE.UTF-8/LC_MESSAGES/one.mo", &utf8_mo("", &[(b"where", b"b-one")]));
        write(d, "a/de_DE.UTF-8/LC_MESSAGES/two.mo", &utf8_mo("", &[(b"where", b"a-two")]));
    });
    in_child("s7_7_c_3", "domains", &dir, &[("LOCPATH", &locales)], || {
        need(LC_ALL, "de_DE.UTF-8");
        assert_eq!(text(x::textdomain(null())), "messages");
        let mut name = c("one");
        let p = x::textdomain(name.as_ptr());
        assert_ne!(p, name.as_mut_ptr(), "a copy");
        name[0] = b'X' as c_char;
        assert_eq!(text(x::textdomain(null())), "one");
        assert_eq!(text(x::bindtextdomain(c("one").as_ptr(), null())), "/usr/share/locale", "unbound");
        let a = format!("{dir}/a");
        let q = x::bindtextdomain(c("one").as_ptr(), c(&a).as_ptr());
        assert_eq!(text(q), a);
        assert_eq!(x::bindtextdomain(c("one").as_ptr(), null()), q, "the query returns the copy");
        bind_dir("two", &a);
        assert_eq!(text(x::gettext(c("where").as_ptr())), "a-one", "the default domain");
        assert_eq!(dc("two", "where", LC_MESSAGES), "a-two", "a domain's own catalog");
        bind_dir("one", &format!("{dir}/b"));
        assert_eq!(dc("one", "where", LC_MESSAGES), "b-one", "rebinding takes effect at the next lookup");
        bind_dir("one", &a);
        assert_eq!(text(x::textdomain(c("").as_ptr())), "messages");
        assert_eq!(text(x::dgettext(null(), c("where").as_ptr())), "where", "messages has no catalog");
        // Output codeset: the catalog's UTF-8 `é` in ISO-8859-1, then as stored again. First
        // without one: the locale's codeset is the catalog's (no conversion, kept with the
        // catalog, GETTEXT-3); a codeset bound later still converts.
        assert_eq!(dc("one", "e", LC_MESSAGES), "\u{e9}", "no codeset: the locale's, the same");
        assert!(x::bind_textdomain_codeset(c("one").as_ptr(), null()).is_null(), "never set");
        assert_eq!(text(x::bind_textdomain_codeset(c("one").as_ptr(), c("ISO-8859-1").as_ptr())), "ISO-8859-1");
        for _ in 0..2 {
            let e = x::dgettext(c("one").as_ptr(), c("e").as_ptr());
            assert_eq!((st::strlen(e), first_byte(e)), (1, 0xe9), "converted, also with the codecs kept");
        }
        assert_eq!(text(x::bind_textdomain_codeset(c("one").as_ptr(), c("").as_ptr())), "");
        assert_eq!(dc("one", "e", LC_MESSAGES), "\u{e9}", "\"\": the bytes as stored");
    });
}

/// §7.7, D-7.12: a catalog missing at the first lookup is found at once when its domain is bound
/// to another directory that has it, or to its own directory spelled differently; a
/// `bindtextdomain` with the unchanged directory string before each lookup keeps the answer
/// right, and the missing path is still tried again after a second.
#[test]
fn s7_7_c_3_bound_later() {
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c3b", |d| {
        write(d, "b/de_DE.UTF-8/LC_MESSAGES/late.mo", &utf8_mo("", &[(b"where", b"b-late")]));
        write(d, "a/de_DE.UTF-8/LC_MESSAGES/other.mo", &utf8_mo("", &[(b"where", b"a-other")]));
    });
    in_child("s7_7_c_3_bound_later", "bound later", &dir, &[("LOCPATH", &locales)], || {
        tsc_off();
        need(LC_ALL, "de_DE.UTF-8");
        let a = format!("{dir}/a");
        let at_a = |domain: &str, value: &str| {
            write(std::path::Path::new(&a), &format!("de_DE.UTF-8/LC_MESSAGES/{domain}.mo"), &utf8_mo("", &[(b"where", value.as_bytes())]));
        };
        bind_dir("late", &a);
        for _ in 0..3 {
            bind_dir("late", &a);
            assert_eq!(dc("late", "where", LC_MESSAGES), "where", "no catalog yet, the same directory again");
        }
        at_a("late", "a-late");
        bind_dir("late", &format!("{dir}/b"));
        assert_eq!(dc("late", "where", LC_MESSAGES), "b-late", "bound to a directory that has it: found at once");
        bind_dir("late", &format!("{a}/"));
        assert_eq!(dc("late", "where", LC_MESSAGES), "a-late", "the directory spelled differently: found at once");
        bind_dir("late", &format!("{a}/"));
        assert_eq!(dc("late", "where", LC_MESSAGES), "a-late", "the same string again keeps it");
        assert_eq!(dc("other", "where", LC_MESSAGES), "where", "other is unbound");
        bind_dir("other", &a);
        assert_eq!(dc("other", "where", LC_MESSAGES), "a-other", "a domain bound later: found at once");
        bind_dir("later", &a);
        assert_eq!(dc("later", "where", LC_MESSAGES), "where", "no catalog yet");
        at_a("later", "a-later");
        bind_dir("later", &a);
        skip_interval();
        assert_eq!(dc("later", "where", LC_MESSAGES), "a-later", "a missing path is tried again after a second");
    });
}

/// For the test clock: the TSC gate off (it trusts the counter, not the skewed clock). Before the
/// child's first lookup, which decides whether the counter is used.
fn tsc_off() {
    const PR_TSC_SIGSEGV: u64 = 2;
    assert_eq!(CSys.prctl_int(liella_kernel::ops::PrctlInt::SetTsc, PR_TSC_SIGSEGV, 0), Ok(0), "PR_SET_TSC");
}

/// The test clock (`recheck::SKEW_NS`, selftest builds): moved past the one-second interval.
/// The child called [`tsc_off`] first.
fn skip_interval() {
    liella_legacy::recheck::SKEW_NS.fetch_add(1_100_000_000, core::sync::atomic::Ordering::Relaxed);
}

/// §7.7, D-7.12 (owner ruling 2026-10-23, #1056/#1057): an unchanged `bindtextdomain`,
/// `bind_textdomain_codeset` or `textdomain` does no file-system work and starts no check, so a
/// catalog created after a miss is found within the clock interval, not at once (as the reference
/// library, which keeps the miss); a changed value checks the files at once.
#[test]
fn s7_7_c_3_rebind_unchanged() {
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c3u", |_| {});
    in_child("s7_7_c_3_rebind_unchanged", "rebind", &dir, &[("LOCPATH", &locales)], || {
        use core::sync::atomic::Ordering::Relaxed;
        use liella_boundary::catalog::FS_CALLS;
        tsc_off();
        need(LC_ALL, "de_DE.UTF-8");
        let path = std::path::Path::new(&*dir).join("de_DE.UTF-8/LC_MESSAGES/u.mo");
        let put = |text: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let new = path.with_extension("new");
            std::fs::write(&new, utf8_mo("", &[(b"where", text.as_bytes())])).unwrap();
            std::fs::rename(&new, &path).unwrap();
        };
        bind_dir("u", &dir);
        assert!(!x::bind_textdomain_codeset(c("u").as_ptr(), c("UTF-8").as_ptr()).is_null());
        assert!(!x::textdomain(c("u").as_ptr()).is_null());
        assert_eq!(dc("u", "where", LC_MESSAGES), "where", "no catalog yet");
        // Within the interval (a run delayed past it is retried: it would see the file legitimately).
        let mut counted = None;
        for _ in 0..5 {
            let _ = std::fs::remove_file(&path);
            skip_interval();
            assert_eq!(dc("u", "where", LC_MESSAGES), "where", "no catalog: the path is checked, still missing");
            // The coarse clock: with the counter off, `Instant` (the vDSO's precise clock) would fault.
            let t = liella_legacy::recheck::now(&CSys);
            let before = FS_CALLS.load(Relaxed);
            put("one");
            bind_dir("u", &dir);
            assert!(!x::bind_textdomain_codeset(c("u").as_ptr(), c("UTF-8").as_ptr()).is_null());
            assert!(!x::textdomain(c("u").as_ptr()).is_null());
            let got = dc("u", "where", LC_MESSAGES);
            let calls = FS_CALLS.load(Relaxed) - before;
            if liella_legacy::recheck::now(&CSys) - t < 500_000_000 {
                counted = Some((calls, got));
                break;
            }
        }
        assert!(counted.is_some(), "five runs each took over half a second");
        let (calls, got) = counted.unwrap();
        assert_eq!(calls, 0, "unchanged rebinding and lookup: no file-system work");
        assert_eq!(got, "where", "unchanged rebinding: the miss is kept until the clock says");
        skip_interval();
        assert_eq!(dc("u", "where", LC_MESSAGES), "one", "a catalog created later: found within the clock interval");
        put("two");
        assert!(!x::bind_textdomain_codeset(c("u").as_ptr(), c("ISO-8859-1").as_ptr()).is_null());
        assert_eq!(dc("u", "where", LC_MESSAGES), "two", "a changed codeset: the file is checked at once");
        put("three");
        bind_dir("u", &format!("{dir}/."));
        assert_eq!(dc("u", "where", LC_MESSAGES), "three", "a changed directory: the file is found at once");
    });
}

/// §7.7: `LANGUAGE` is ignored when the `LC_MESSAGES` locale is `C`, `POSIX` or `C.UTF-8`, and a
/// `C` or `POSIX` element of the list ends the search untranslated (D-7.12).
#[test]
fn s7_7_c_4() {
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c4", |d| {
        write(d, "it/LC_MESSAGES/t.mo", &utf8_mo("", &[(b"where", b"it")]));
        write(d, "de_DE.UTF-8/LC_MESSAGES/t.mo", &utf8_mo("", &[(b"where", b"de"), (b"only-de", b"nur-de")]));
    });
    in_child("s7_7_c_4", "language", &dir, &[("LOCPATH", &locales), ("LANGUAGE", "it")], || {
        bind_dir("t", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        assert_eq!(dc("t", "where", LC_MESSAGES), "it", "LANGUAGE is used in a named locale");
        for name in ["C", "POSIX", "C.UTF-8"] {
            need(LC_MESSAGES, name);
            assert_eq!(dc("t", "where", LC_MESSAGES), "where", "LANGUAGE ignored for {name}");
        }
        need(LC_MESSAGES, "de_DE.UTF-8");
        for list in ["C:it", "POSIX:it", "zz:C:it", "zz:POSIX:de_DE.UTF-8"] {
            setenv("LANGUAGE", list);
            assert_eq!(dc("t", "where", LC_MESSAGES), "where", "{list}: the element ends the search");
            assert_eq!(dc("t", "only-de", LC_MESSAGES), "only-de", "{list}: no fall-through past it");
        }
        setenv("LANGUAGE", "it:C");
        assert_eq!(dc("t", "where", LC_MESSAGES), "it", "elements before it still count");
        assert_eq!(dc("t", "only-de", LC_MESSAGES), "only-de", "but none after it, nor the locale");
    });
}

/// §7.7 with §1.3: `LANGUAGE`'s lookup is remembered across gettext calls (GETTEXT-ENV-SCAN,
/// ruling #504) and every change made through `setenv`, `putenv`, `unsetenv`, `clearenv`, a new
/// `environ`, a new pointer stored into `LANGUAGE`'s own slot and a `putenv` string edited in
/// place is seen by the next call. The one kind not seen: `LANGUAGE` made to appear by a direct
/// store into another slot, until the next writer call (docs/differences-from-glibc.md).
#[test]
fn s7_7_c_4_env_memo() {
    use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering::SeqCst};
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("c4m", |d| {
        for (lang, t) in [("de_DE.UTF-8", "de"), ("it", "it"), ("fr", "fr"), ("es", "es")] {
            write(d, &format!("{lang}/LC_MESSAGES/t.mo"), &utf8_mo("", &[(b"where", t.as_bytes())]));
        }
    });
    in_child("s7_7_c_4_env_memo", "memo", &dir, &[("LOCPATH", &locales)], || {
        bind_dir("t", &dir);
        need(LC_ALL, "de_DE.UTF-8");
        // C text the test owns and may change in place: its address stays valid for the process.
        let text = |t: &str| -> &'static [AtomicU8] { Box::leak(t.bytes().chain([0]).map(AtomicU8::new).collect()) };
        let at = |t: &'static [AtomicU8]| t.as_ptr() as usize;
        let edit = |t: &[AtomicU8], from: usize, b: &str| b.bytes().enumerate().for_each(|(i, b)| t[from + i].store(b, SeqCst));
        let w = || dc("t", "where", LC_MESSAGES);
        let pre: &'static [AtomicUsize] = Box::leak([at(text("AAA=1")), at(text("QQ=1")), 0].map(AtomicUsize::new).into());
        liella_boundary::ffi::environ.store(pre.as_ptr() as usize, SeqCst);
        assert_eq!(w(), "de", "LANGUAGE unset: the locale");
        pre[1].store(at(text("LANGUAGE=it")), SeqCst);
        assert_eq!(w(), "de", "an unset name made to appear by a direct store is not seen (the recorded difference)");
        setenv("CCC", "1");
        assert_eq!(w(), "it", "seen after the next writer call");
        let buf = text("LANGUAGE=it");
        let words = [at(text("AAA=1")), at(buf), at(text("ZZ=filler")), at(text("LANGUAGE=es")), 0];
        let arr: &'static [AtomicUsize] = Box::leak(words.map(AtomicUsize::new).into());
        liella_boundary::ffi::environ.store(arr.as_ptr() as usize, SeqCst);
        assert_eq!(w(), "it", "a new environ: the first entry wins");
        assert_eq!(w(), "it", "remembered");
        edit(buf, 9, "fr");
        assert_eq!(w(), "fr", "the remembered entry edited in place");
        edit(buf, 9, "it");
        assert_eq!(w(), "it", "and back");
        arr[1].store(at(text("LANGUAGE=fr")), SeqCst);
        assert_eq!(w(), "fr", "a new pointer in the remembered slot");
        arr[1].store(at(text("ZZZZZZZZ=fr")), SeqCst);
        assert_eq!(w(), "es", "the remembered slot renamed: the next entry");
        arr[2].store(at(text("LANGUAGE=it")), SeqCst);
        assert_eq!(w(), "es", "a direct store before the remembered slot is not seen (the recorded difference)");
        setenv("LANGUAGE", "fr");
        assert_eq!(w(), "fr", "after setenv: scanned again, the first entry replaced");
        setenv("LANGUAGE", "it");
        assert_eq!(w(), "it", "setenv of the remembered slot");
        let put = text("LANGUAGE=es");
        assert_eq!(envx::putenv(put.as_ptr().cast_mut().cast()), 0);
        assert_eq!(w(), "es", "putenv of the remembered slot");
        setenv("BBB", "1");
        assert_eq!(w(), "es", "another name set");
        assert_eq!(envx::unsetenv(c("AAA").as_ptr()), 0);
        assert_eq!(w(), "es", "entries moved by unsetenv");
        edit(put, 9, "fr");
        assert_eq!(w(), "fr", "the putenv string edited in place, at its new index");
        let gone = text("ZZZZZZZZ");
        assert_eq!(envx::putenv(gone.as_ptr().cast_mut().cast()), 0);
        assert_eq!(w(), "fr", "entries moved by putenv without '='");
        assert_eq!(envx::unsetenv(c("LANGUAGE").as_ptr()), 0);
        assert_eq!(w(), "de", "unset: the locale");
        setenv("LANGUAGE", "it");
        assert_eq!(w(), "it", "set again into the same array");
        assert_eq!(envx::unsetenv(c("LANGUAGE").as_ptr()), 0);
        assert_eq!(w(), "de", "unset again");
        let again: &'static [AtomicUsize] = Box::leak([at(text("LANGUAGE=it")), 0].map(AtomicUsize::new).into());
        liella_boundary::ffi::environ.store(again.as_ptr() as usize, SeqCst);
        assert_eq!(w(), "it", "unset remembered, then a new environ that sets it");
        assert_eq!(envx::clearenv(), 0);
        assert_eq!(w(), "de", "clearenv");
        setenv("LANGUAGE", "es");
        assert_eq!(w(), "es", "set after clearenv");
        let own: &'static [AtomicUsize] = Box::leak([at(text("LANGUAGE=fr")), 0].map(AtomicUsize::new).into());
        liella_boundary::ffi::environ.store(own.as_ptr() as usize, SeqCst);
        assert_eq!(w(), "fr", "environ assigned again");
        own[0].store(0, SeqCst);
        assert_eq!(w(), "de", "the remembered slot made the array's end");
    });
}

/// §7.7: `bindtextdomain` and `bind_textdomain_codeset` with a NULL or empty domain return NULL
/// and leave `errno` unchanged (D-7.12).
#[test]
fn s7_7_c_5() {
    let _g = global();
    let (empty, dir, codeset) = (c(""), c("/nonexistent"), c("UTF-8"));
    for preset in [0, 4, 12345] {
        for domain in [null(), empty.as_ptr()] {
            for d in [null(), dir.as_ptr()] {
                errno::set_raw(preset);
                assert!(x::bindtextdomain(domain, d).is_null());
                assert_eq!(errno::get(), preset);
            }
            for s in [null(), codeset.as_ptr()] {
                errno::set_raw(preset);
                assert!(x::bind_textdomain_codeset(domain, s).is_null());
                assert_eq!(errno::get(), preset);
            }
        }
    }
}

/// §7.7: never failing visibly. Damaged or unusable catalogs (truncated, a wrong magic number,
/// a directory, a FIFO, an unknown charset) give the untranslated text, `errno` is preserved by
/// every call, and translations stay valid library-owned strings.
#[test]
fn s7_7_g_0() {
    let Some(locales) = test_locales() else {
        skip_note::skipped("skipped: no localedef");
        return;
    };
    let dir = fixtures("g0", |d| {
        let good = utf8_mo("", &[(b"hello", b"hallo")]);
        write(d, "de_DE.UTF-8/LC_MESSAGES/good.mo", &good);
        write(d, "de_DE.UTF-8/LC_MESSAGES/short.mo", &good[..good.len() / 2]);
        let mut magic = good.clone();
        magic[0] = 0x12;
        write(d, "de_DE.UTF-8/LC_MESSAGES/magic.mo", &magic);
        std::fs::create_dir_all(d.join("de_DE.UTF-8/LC_MESSAGES/dir.mo")).unwrap();
        let fifo = d.join("de_DE.UTF-8/LC_MESSAGES/fifo.mo");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let nope = mo_image(&[(b"", b"Content-Type: text/plain; charset=NOPE\n"), (b"hello", b"hallo")]);
        write(d, "de_DE.UTF-8/LC_MESSAGES/nope.mo", &nope);
    });
    in_child("s7_7_g_0", "robust", &dir, &[("LOCPATH", &locales)], || {
        need(LC_ALL, "de_DE.UTF-8");
        for d in ["good", "short", "magic", "dir", "fifo", "nope", "missing"] {
            bind_dir(d, &dir);
        }
        for preset in [0, 4, 12, 12345] {
            for d in ["short", "magic", "dir", "fifo", "nope", "missing"] {
                errno::set_raw(preset);
                assert_eq!(dc(d, "hello", LC_MESSAGES), "hello", "{d}");
                assert_eq!(errno::get(), preset, "{d}");
            }
            errno::set_raw(preset);
            let p = x::dgettext(c("good").as_ptr(), c("hello").as_ptr());
            assert_eq!(text(p), "hallo");
            errno::set_raw(preset);
            x::textdomain(c("x").as_ptr());
            x::bindtextdomain(c("good").as_ptr(), c(&dir).as_ptr());
            x::bind_textdomain_codeset(c("good").as_ptr(), c("UTF-8").as_ptr());
            x::ngettext(c("a").as_ptr(), c("b").as_ptr(), 3);
            assert_eq!(errno::get(), preset);
        }
    });
    // The format itself: a damaged image is not a catalog.
    let good = utf8_mo("", &[(b"hello", b"hallo")]);
    assert!(mo::Mo::parse(&good).is_some());
    let mut cut = good[..good.len() - 7].to_vec(); // the last string (6 bytes) and one more
    cut.push(0);
    assert!(mo::Mo::parse(&cut).is_none(), "strings beyond the end");
}

/// §7.8: `catopen` finds a catalog by the `NLSPATH` templates (`%N`, `%L`, `%l`, `%t`, `%c`, `%%`)
/// or by a path; `catgets` retrieves messages by set and number, and returns the default for a
/// missing one; `catclose` releases the descriptor.
#[test]
fn s7_8_c_0() {
    let dir = fixtures("8c0", |d| {
        write(d, "full/de_DE.UTF-8@euro/app", &cat_image(&[(1, 1, "full name")], 1, 1));
        write(d, "parts/de/DE/UTF-8@euro/app.cat", &cat_image(&[(1, 1, "parts"), (1, 4, "one-four")], 3, 2));
        write(d, "pct%/app", &cat_image(&[(1, 1, "percent")], 1, 1));
        write(d, "abs", &cat_image(&[(1, 1, "absolute"), (1, 2, "second")], 3, 1));
    });
    let nlspath = format!("{dir}/none/%N:{dir}/full/%L/%N:{dir}/parts/%l/%t/%c/%N.cat");
    in_child("s7_8_c_0", "nlspath", &dir, &[("NLSPATH", &nlspath), ("LANG", "de_DE.UTF-8@euro")], || {
        let dflt = c("DEFAULT");
        let cd = x::catopen(c("app").as_ptr(), 0);
        assert_ne!(cd as usize, usize::MAX);
        assert_eq!(text(x::catgets(cd, 1, 1, dflt.as_ptr())), "full name", "%L");
        assert_eq!(x::catclose(cd), 0);
        setenv("NLSPATH", &format!("{dir}/parts/%l/%t/%c/%N.cat"));
        let cd = x::catopen(c("app").as_ptr(), 0);
        assert_eq!(text(x::catgets(cd, 1, 1, dflt.as_ptr())), "parts", "%l, %t, %c (the modifier stays with the codeset)");
        assert_eq!(text(x::catgets(cd, 1, 4, dflt.as_ptr())), "one-four", "the second plane (its first-plane slot holds 1,1)");
        assert_eq!(x::catgets(cd, 2, 8, dflt.as_ptr()), dflt.as_ptr().cast_mut(), "missing: the default itself");
        assert_eq!(x::catclose(cd), 0);
        setenv("NLSPATH", &format!("{dir}/pct%%/%N"));
        let cd = x::catopen(c("app").as_ptr(), 0);
        assert_eq!(text(x::catgets(cd, 1, 1, dflt.as_ptr())), "percent", "%%");
        assert_eq!(x::catclose(cd), 0);
    });
    {
        let dflt = c("DEFAULT");
        let cd = x::catopen(c(&format!("{dir}/abs")).as_ptr(), 0);
        assert_ne!(cd as usize, usize::MAX, "a name with / is a path");
        let p = x::catgets(cd, 1, 2, dflt.as_ptr());
        assert_eq!(text(p), "second");
        assert_eq!(x::catgets(cd, 1, 2, dflt.as_ptr()), p, "the same pointer every time");
        assert_eq!(x::catclose(cd), 0);
    }
    // The format directly.
    let img = cat_image(&[(1, 1, "one-one"), (1, 2, "one-two"), (2, 1, "two-one")], 3, 1);
    let mut z = img.clone();
    z.push(0);
    let cat = xopen::Cat::parse(&z, img.len()).unwrap();
    assert_eq!(cat.get(2, 1).map(|o| &z[o..o + 7]), Some(&b"two-one"[..]));
}

/// §7.8: in an `NLSPATH` template `%t` and `%c` keep the locale name's `@modifier`; the search
/// stops with `EINVAL` at a candidate that is a directory or not a catalog (a missing candidate
/// moves on to the next template).
#[test]
fn s7_8_c_p1() {
    let dir = fixtures("8cp1", |d| {
        write(d, "t/DE@euro/app", &cat_image(&[(1, 1, "territory")], 1, 1));
        write(d, "c/UTF-8@euro/app", &cat_image(&[(1, 1, "codeset")], 1, 1));
        write(d, "ok/app", &cat_image(&[(1, 1, "ok")], 1, 1));
        write(d, "bad/app", b"this is not a catalog\n");
        std::fs::create_dir_all(d.join("dir/app")).unwrap();
    });
    in_child("s7_8_c_p1", "modifier", &dir, &[("NLSPATH", &format!("{dir}/t/%t/%N")), ("LANG", "de_DE@euro")], || {
        let dflt = c("DEFAULT");
        let cd = x::catopen(c("app").as_ptr(), 0);
        assert_ne!(cd as usize, usize::MAX, "%t with the modifier");
        assert_eq!(text(x::catgets(cd, 1, 1, dflt.as_ptr())), "territory");
        assert_eq!(x::catclose(cd), 0);
        setenv("LANG", "de_DE.UTF-8@euro");
        setenv("NLSPATH", &format!("{dir}/c/%c/%N"));
        let cd = x::catopen(c("app").as_ptr(), 0);
        assert_ne!(cd as usize, usize::MAX, "%c with the modifier");
        assert_eq!(text(x::catgets(cd, 1, 1, dflt.as_ptr())), "codeset");
        assert_eq!(x::catclose(cd), 0);
        setenv("NLSPATH", &format!("{dir}/none/%N:{dir}/ok/%N"));
        let cd = x::catopen(c("app").as_ptr(), 0);
        assert_ne!(cd as usize, usize::MAX, "a missing candidate moves on");
        assert_eq!(x::catclose(cd), 0);
        for stop in ["dir", "bad"] {
            setenv("NLSPATH", &format!("{dir}/{stop}/%N:{dir}/ok/%N"));
            errno::set_raw(0);
            assert_eq!(x::catopen(c("app").as_ptr(), 0) as usize, usize::MAX, "{stop}: the search stops");
            assert_eq!(errno::get(), EINVAL, "{stop}");
        }
    });
}

/// §7.8: `catopen` fails with `(nl_catd)-1` and an error (`EINVAL` for an empty name, `ENOENT` when no candidate exists,
/// `EINVAL` for a file that is not a catalog, the open's error for a path); `catgets` returns the
/// default and sets `errno` (`ENOMSG`, `EBADF` for an invalid descriptor); `catclose` of an invalid
/// descriptor is -1 with `EBADF`.
#[test]
fn s7_8_f_0() {
    let dir = fixtures("8f0", |d| {
        write(d, "bad", b"this is not a catalog\n");
        write(d, "zero-plane", &{
            let mut i = cat_image(&[(1, 1, "x")], 1, 1);
            i[4..8].copy_from_slice(&0u32.to_le_bytes());
            i
        });
        write(d, "good", &cat_image(&[(1, 1, "x")], 1, 1));
    });
    let fail = |name: &str| -> i32 {
        errno::set_raw(0);
        assert_eq!(x::catopen(c(name).as_ptr(), 0) as usize, usize::MAX, "{name}");
        errno::get()
    };
    assert_eq!(fail(&format!("{dir}/missing")), ENOENT);
    assert_eq!(fail(&format!("{dir}/bad")), EINVAL);
    assert_eq!(fail(&format!("{dir}/zero-plane")), EINVAL, "plane size 0: not a catalog (no division)");
    assert_eq!(fail(&dir), EINVAL, "a directory");
    // An empty name searches the default templates with `%N` empty: the first existing path
    // (a directory such as /usr/share/locale/en/LC_MESSAGES/ under LANG=en_US.UTF-8) is not a
    // catalog, EINVAL; with none (LANG unset or C) the search ends with ENOENT. glibc 2.39
    // answers the same under each (probe, milestone 3; the isolated gate resets LANG).
    if std::env::var_os("NLSPATH").is_none() {
        let lang = std::env::var("LANG").ok().filter(|l| !l.is_empty()).unwrap_or_else(|| "C".into());
        let short = lang.split(['_', '.', '@']).next().unwrap_or("C").to_string();
        let met = [format!("/usr/share/locale/{lang}/"), format!("/usr/share/locale/{lang}/LC_MESSAGES/"), format!("/usr/share/locale/{short}/"), format!("/usr/share/locale/{short}/LC_MESSAGES/")]
            .iter()
            .any(|p| std::path::Path::new(p).exists());
        assert_eq!(fail(""), if met { EINVAL } else { ENOENT }, "an empty name (LANG={lang})");
    }
    let dflt = c("DEFAULT");
    let cd = x::catopen(c(&format!("{dir}/good")).as_ptr(), 0);
    for (set, msg) in [(1, 2), (2, 1), (0, 1), (-1, 1), (1, -1), (i32::MAX, 1)] {
        errno::set_raw(0);
        assert_eq!(x::catgets(cd, set, msg, dflt.as_ptr()), dflt.as_ptr().cast_mut());
        assert_eq!(errno::get(), ENOMSG, "{set},{msg}");
    }
    assert_eq!(x::catclose(cd), 0);
    let invalid = usize::MAX as *mut core::ffi::c_void;
    errno::set_raw(0);
    assert_eq!(x::catgets(invalid, 1, 1, dflt.as_ptr()), dflt.as_ptr().cast_mut());
    assert_eq!(errno::get(), EBADF);
    errno::set_raw(0);
    assert_eq!(x::catclose(invalid), -1);
    assert_eq!(errno::get(), EBADF);
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
