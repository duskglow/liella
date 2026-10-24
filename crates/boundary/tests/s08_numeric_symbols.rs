// Specified from spec §8 and its Appendix B list. Generated stubs were replaced by real tests of
// the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Translation-layer tests for the exported symbols of spec §8 (212 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The `long double` and `_Float128` exports are single-statement C forwarders (OD-C4) around
//! the `liella_*_bits` functions; their tests exercise those functions with the values as bit
//! structs. The forwarders' own calling-convention half is covered by the phase-4 conformance
//! suite, which calls them from C.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::approx_constant, non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::float_cmp, clippy::bool_assert_comparison, missing_docs)]

use core::ptr::null_mut;

use liella_abi::ctypes::{c_char, c_int, wchar_t, F128Bits, LdBits};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as mem;
use liella_boundary::posix::s08_numeric as s;
use liella_boundary::selftest::{self, ChildEnd};

const EINVAL: i32 = 22;
const ERANGE: i32 = 34;
const SIGABRT: i32 = 6;

fn c(t: &str) -> Vec<c_char> {
    t.bytes().map(|b| i8::from_le_bytes([b])).chain([0]).collect()
}

fn w(t: &str) -> Vec<wchar_t> {
    t.chars().map(|ch| ch as wchar_t).chain([0]).collect()
}

fn txt(v: &[c_char]) -> String {
    v.iter().take_while(|&&b| b != 0).map(|&b| char::from(b.to_le_bytes()[0])).collect()
}

/// Text in library memory, copied out with `strcpy`.
fn read(p: *const c_char) -> String {
    let mut b = vec![0 as c_char; mem::strlen(p) + 1];
    mem::strcpy(b.as_mut_ptr(), p);
    txt(&b)
}

/// `f` reports `ERANGE` and returns `want`.
fn erange<T: PartialEq + core::fmt::Debug>(f: impl FnOnce() -> T, want: T) {
    errno::set_raw(0);
    assert_eq!(f(), want);
    assert_eq!(errno::get(), ERANGE);
}

/// A `double` as x87 bits (exact: every double is a long double).
fn ld(x: f64) -> LdBits {
    let b = x.to_bits();
    let (sign, exp, frac) = (b >> 63, ((b >> 52) & 0x7ff) as i64, b & ((1 << 52) - 1));
    assert!(exp != 0 && exp != 0x7ff, "normal doubles only");
    let se = ((sign << 15) | (exp - 1023 + 16383) as u64) as u16;
    LdBits { mant: (1 << 63) | (frac << 11), se, pad: [0; 3] }
}

fn ld_max() -> LdBits {
    LdBits { mant: u64::MAX, se: 0x7FFE, pad: [0; 3] }
}

/// A `double` as binary128 bits.
fn q(x: f64) -> F128Bits {
    let b = x.to_bits();
    let (sign, exp, frac) = (b >> 63, (b >> 52) & 0x7ff, b & ((1 << 52) - 1));
    let e = exp - 1023 + 16383;
    F128Bits { hi: (sign << 63) | (e << 48) | (frac >> 4), lo: (frac & 0xf) << 60 }
}

/// Edge values and a spread of patterns for the bit queries.
fn samples<T: TryFrom<u64> + Copy>() -> Vec<T> {
    let mut v = vec![0u64, 1, 2, 3, 0x80, 0xff, 0x100, 0x7fff, 0x8000, 0xffff, 0x1_0000, u64::from(u32::MAX), 1 << 63, u64::MAX, u64::MAX - 1];
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..64 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.push(x);
        v.push(x >> (x % 64));
    }
    v.into_iter().map(|u| T::try_from(u).ok().unwrap_or_else(|| T::try_from(u & 0xff).ok().unwrap())).collect()
}

#[test]
fn sym_a64l() {
    assert_eq!(s::a64l(c("./").as_ptr()), 64);
    assert_eq!(s::a64l(c("zzzzzz").as_ptr()), -1, "sign-extended from 32 bits");
    assert_eq!(s::a64l(c("1!").as_ptr()), 3, "stops outside the alphabet");
}

#[test]
fn sym_abs() {
    assert_eq!((s::abs(-5), s::abs(5), s::abs(0)), (5, 5, 0));
    assert_eq!(s::abs(i32::MIN), i32::MIN, "undefined in C; wraps here");
}

#[test]
fn sym_atof() {
    assert_eq!(s::atof(c(" 1.5e2junk").as_ptr()), 150.0);
    assert_eq!(s::atof(c("x").as_ptr()), 0.0);
}

#[test]
fn sym_atoi() {
    assert_eq!(s::atoi(c(" -42x").as_ptr()), -42);
    assert_eq!(s::atoi(c("x").as_ptr()), 0);
    assert_eq!(s::atoi(c("4294967297").as_ptr()), 1, "truncated to int, no error reporting");
}

#[test]
fn sym_atol() {
    errno::set_raw(0);
    assert_eq!(s::atol(c("99999999999999999999").as_ptr()), i64::MAX);
    assert_eq!(errno::get(), 0, "no error reporting");
}

#[test]
fn sym_atoll() {
    assert_eq!(s::atoll(c("\t+17").as_ptr()), 17);
}

#[test]
fn sym_div() {
    let r = s::div(-7, 2);
    assert_eq!((r.quot, r.rem), (-3, -1), "truncated toward zero");
    assert_eq!(selftest::in_child_quiet(|_| { s::div(1, 0); 0 }).unwrap(), ChildEnd::Signaled(SIGABRT), "division by zero: diagnostic and abort (S6)");
    assert_eq!(selftest::in_child_quiet(|_| { s::div(i32::MIN, -1); 0 }).unwrap(), ChildEnd::Signaled(SIGABRT));
}

#[test]
fn sym_ecvt() {
    let (mut d, mut g) = (0, 0);
    let p = s::ecvt(-3.14159, 3, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d, g), ("314".into(), 1, 1));
    let p = s::ecvt(0.000123, 2, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d, g), ("12".into(), -3, 0));
    let p = s::ecvt(9.99, 2, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d), ("10".into(), 2), "rounding carries into the point");
}

#[test]
fn sym_ecvt_r() {
    let (mut d, mut g) = (0, 0);
    let mut b = [0 as c_char; 8];
    assert_eq!(s::ecvt_r(2.5, 4, Some(&mut d), Some(&mut g), b.as_mut_ptr(), 8), 0);
    assert_eq!((txt(&b), d, g), ("2500".into(), 1, 0));
    let mut small = [0 as c_char; 3];
    assert_eq!(s::ecvt_r(123.0, 5, Some(&mut d), Some(&mut g), small.as_mut_ptr(), 3), 0);
    assert_eq!(txt(&small), "12", "digits beyond the buffer are dropped (D-8.10)");
}

#[test]
fn sym_fcvt() {
    let (mut d, mut g) = (0, 0);
    let p = s::fcvt(3.14159, 2, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d, g), ("314".into(), 1, 0));
    let p = s::fcvt(1234.5, 0, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d), ("1234".into(), 4), "round half to even");
}

#[test]
fn sym_fcvt_r() {
    let (mut d, mut g) = (0, 0);
    let mut b = [0 as c_char; 8];
    assert_eq!(s::fcvt_r(-0.5, 1, Some(&mut d), Some(&mut g), b.as_mut_ptr(), 8), 0);
    assert_eq!((txt(&b), d, g), ("5".into(), 0, 1));
}

#[test]
fn sym___ffs() {
    assert_eq!((s::__ffs(0), s::__ffs(1), s::__ffs(0x50), s::__ffs(i32::MIN)), (0, 1, 5, 32));
}

#[test]
fn sym_ffs() {
    assert_eq!((s::ffs(0), s::ffs(1), s::ffs(0x50), s::ffs(i32::MIN)), (0, 1, 5, 32));
}

#[test]
fn sym_ffsl() {
    assert_eq!((s::ffsl(0), s::ffsl(1 << 40), s::ffsl(i64::MIN), s::ffsl(-1)), (0, 41, 64, 1));
}

#[test]
fn sym_ffsll() {
    assert_eq!((s::ffsll(0), s::ffsll(1 << 40), s::ffsll(i64::MIN), s::ffsll(-1)), (0, 41, 64, 1));
}

#[test]
fn sym_gcvt() {
    let mut b = [0 as c_char; 32];
    assert_eq!(s::gcvt(0.000123456, 3, b.as_mut_ptr()), b.as_mut_ptr());
    assert_eq!(txt(&b), "0.000123");
    s::gcvt(1e20, 5, b.as_mut_ptr());
    assert_eq!(txt(&b), "1e+20", "%.*g");
}

#[test]
fn sym_imaxabs() {
    assert_eq!((s::imaxabs(-5), s::imaxabs(i64::MAX)), (5, i64::MAX));
}

#[test]
fn sym_imaxdiv() {
    let r = s::imaxdiv(7, -2);
    assert_eq!((r.quot, r.rem), (-3, 1));
    assert_eq!(selftest::in_child_quiet(|_| { s::imaxdiv(i64::MIN, -1); 0 }).unwrap(), ChildEnd::Signaled(SIGABRT));
}

#[test]
fn sym___isoc23_strtoimax() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoimax(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoimax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtol_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtol_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtol_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtol() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtol(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtol(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoll_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoll_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoll_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoll() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoll(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoll(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoul_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoul_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoul_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoul() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoul(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoul(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoull_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoull_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoull_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoull() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoull(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoull(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_strtoumax() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__isoc23_strtoumax(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_strtoumax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoimax() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoimax(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoimax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstol_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstol_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstol_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstol() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstol(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstol(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoll_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoll_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoll_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoll() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoll(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoll(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoul_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoul_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoul_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoul() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoul(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoul(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoull_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoull_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoull_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoull() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoull(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoull(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___isoc23_wcstoumax() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__isoc23_wcstoumax(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 5, "C23: 0b is binary");
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__isoc23_wcstoumax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_l64a() {
    assert_eq!(read(s::l64a(0)), "", "0 is the empty text");
    assert_eq!(read(s::l64a(64)), "./");
    assert_eq!(read(s::l64a(0x1_0000_0001)), "/", "only the low 32 bits");
}

#[test]
fn sym_labs() {
    assert_eq!((s::labs(-5), s::labs(i64::MAX)), (5, i64::MAX));
}

#[test]
fn sym_ldiv() {
    let r = s::ldiv(7, -2);
    assert_eq!((r.quot, r.rem), (-3, 1));
    assert_eq!(selftest::in_child_quiet(|_| { s::ldiv(i64::MIN, -1); 0 }).unwrap(), ChildEnd::Signaled(SIGABRT));
}

#[test]
fn sym_llabs() {
    assert_eq!((s::llabs(-5), s::llabs(i64::MAX)), (5, i64::MAX));
}

#[test]
fn sym_lldiv() {
    let r = s::lldiv(7, -2);
    assert_eq!((r.quot, r.rem), (-3, 1));
    assert_eq!(selftest::in_child_quiet(|_| { s::lldiv(i64::MIN, -1); 0 }).unwrap(), ChildEnd::Signaled(SIGABRT));
}

#[test]
fn sym_qecvt() {
    // The C forwarder passes the long double as these bits.
    let (mut d, mut g) = (0, 0);
    let p = s::liella_qecvt_bits(ld(-3.14159), 3, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d, g), ("314".into(), 1, 1));
    let p = s::liella_qecvt_bits(ld_max(), 4, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d), ("1190".into(), 4933), "LDBL_MAX");
}

#[test]
fn sym_qecvt_r() {
    let (mut d, mut g) = (0, 0);
    let mut b = [0 as c_char; 8];
    assert_eq!(s::liella_qecvt_r_bits(ld(2.5), 4, Some(&mut d), Some(&mut g), b.as_mut_ptr(), 8), 0);
    assert_eq!((txt(&b), d), ("2500".into(), 1));
}

#[test]
fn sym_qfcvt() {
    let (mut d, mut g) = (0, 0);
    let p = s::liella_qfcvt_bits(ld(3.14159), 2, Some(&mut d), Some(&mut g));
    assert_eq!((read(p), d, g), ("314".into(), 1, 0));
}

#[test]
fn sym_qfcvt_r() {
    let (mut d, mut g) = (0, 0);
    let mut b = [0 as c_char; 8];
    assert_eq!(s::liella_qfcvt_r_bits(ld(0.25), 2, Some(&mut d), Some(&mut g), b.as_mut_ptr(), 8), 0);
    assert_eq!((txt(&b), d), ("25".into(), 0));
}

#[test]
fn sym_qgcvt() {
    let mut b = [0 as c_char; 32];
    s::liella_qgcvt_bits(ld(1.0 / 3.0), 4, b.as_mut_ptr());
    assert_eq!(txt(&b), "0.3333");
}

#[test]
fn sym_stdc_bit_ceil_uc() {
    let reference = |x: u8| x.checked_next_power_of_two().unwrap_or(0);
    for x in samples::<u8>() {
        assert_eq!(s::stdc_bit_ceil_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_ceil_ui() {
    let reference = |x: u32| x.checked_next_power_of_two().unwrap_or(0);
    for x in samples::<u32>() {
        assert_eq!(s::stdc_bit_ceil_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_ceil_ul() {
    let reference = |x: u64| x.checked_next_power_of_two().unwrap_or(0);
    for x in samples::<u64>() {
        assert_eq!(s::stdc_bit_ceil_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_ceil_ull() {
    let reference = |x: u64| x.checked_next_power_of_two().unwrap_or(0);
    for x in samples::<u64>() {
        assert_eq!(s::stdc_bit_ceil_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_ceil_us() {
    let reference = |x: u16| x.checked_next_power_of_two().unwrap_or(0);
    for x in samples::<u16>() {
        assert_eq!(s::stdc_bit_ceil_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_floor_uc() {
    let reference = |x: u8| if x == 0 { 0 } else { 1 << (8 - 1 - x.leading_zeros()) };
    for x in samples::<u8>() {
        assert_eq!(s::stdc_bit_floor_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_floor_ui() {
    let reference = |x: u32| if x == 0 { 0 } else { 1 << (32 - 1 - x.leading_zeros()) };
    for x in samples::<u32>() {
        assert_eq!(s::stdc_bit_floor_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_floor_ul() {
    let reference = |x: u64| if x == 0 { 0 } else { 1 << (64 - 1 - x.leading_zeros()) };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_bit_floor_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_floor_ull() {
    let reference = |x: u64| if x == 0 { 0 } else { 1 << (64 - 1 - x.leading_zeros()) };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_bit_floor_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_floor_us() {
    let reference = |x: u16| if x == 0 { 0 } else { 1 << (16 - 1 - x.leading_zeros()) };
    for x in samples::<u16>() {
        assert_eq!(s::stdc_bit_floor_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_width_uc() {
    let reference = |x: u8| 8 - x.leading_zeros();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_bit_width_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_width_ui() {
    let reference = |x: u32| 32 - x.leading_zeros();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_bit_width_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_width_ul() {
    let reference = |x: u64| 64 - x.leading_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_bit_width_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_width_ull() {
    let reference = |x: u64| 64 - x.leading_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_bit_width_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_bit_width_us() {
    let reference = |x: u16| 16 - x.leading_zeros();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_bit_width_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_ones_uc() {
    let reference = |x: u8| x.count_ones();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_count_ones_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_ones_ui() {
    let reference = |x: u32| x.count_ones();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_count_ones_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_ones_ul() {
    let reference = |x: u64| x.count_ones();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_count_ones_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_ones_ull() {
    let reference = |x: u64| x.count_ones();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_count_ones_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_ones_us() {
    let reference = |x: u16| x.count_ones();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_count_ones_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_zeros_uc() {
    let reference = |x: u8| x.count_zeros();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_count_zeros_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_zeros_ui() {
    let reference = |x: u32| x.count_zeros();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_count_zeros_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_zeros_ul() {
    let reference = |x: u64| x.count_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_count_zeros_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_zeros_ull() {
    let reference = |x: u64| x.count_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_count_zeros_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_count_zeros_us() {
    let reference = |x: u16| x.count_zeros();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_count_zeros_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_one_uc() {
    let reference = |x: u8| if x == 0 { 0 } else { x.leading_zeros() + 1 };
    for x in samples::<u8>() {
        assert_eq!(s::stdc_first_leading_one_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_one_ui() {
    let reference = |x: u32| if x == 0 { 0 } else { x.leading_zeros() + 1 };
    for x in samples::<u32>() {
        assert_eq!(s::stdc_first_leading_one_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_one_ul() {
    let reference = |x: u64| if x == 0 { 0 } else { x.leading_zeros() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_leading_one_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_one_ull() {
    let reference = |x: u64| if x == 0 { 0 } else { x.leading_zeros() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_leading_one_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_one_us() {
    let reference = |x: u16| if x == 0 { 0 } else { x.leading_zeros() + 1 };
    for x in samples::<u16>() {
        assert_eq!(s::stdc_first_leading_one_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_zero_uc() {
    let reference = |x: u8| if x == u8::MAX { 0 } else { x.leading_ones() + 1 };
    for x in samples::<u8>() {
        assert_eq!(s::stdc_first_leading_zero_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_zero_ui() {
    let reference = |x: u32| if x == u32::MAX { 0 } else { x.leading_ones() + 1 };
    for x in samples::<u32>() {
        assert_eq!(s::stdc_first_leading_zero_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_zero_ul() {
    let reference = |x: u64| if x == u64::MAX { 0 } else { x.leading_ones() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_leading_zero_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_zero_ull() {
    let reference = |x: u64| if x == u64::MAX { 0 } else { x.leading_ones() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_leading_zero_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_leading_zero_us() {
    let reference = |x: u16| if x == u16::MAX { 0 } else { x.leading_ones() + 1 };
    for x in samples::<u16>() {
        assert_eq!(s::stdc_first_leading_zero_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_one_uc() {
    let reference = |x: u8| if x == 0 { 0 } else { x.trailing_zeros() + 1 };
    for x in samples::<u8>() {
        assert_eq!(s::stdc_first_trailing_one_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_one_ui() {
    let reference = |x: u32| if x == 0 { 0 } else { x.trailing_zeros() + 1 };
    for x in samples::<u32>() {
        assert_eq!(s::stdc_first_trailing_one_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_one_ul() {
    let reference = |x: u64| if x == 0 { 0 } else { x.trailing_zeros() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_trailing_one_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_one_ull() {
    let reference = |x: u64| if x == 0 { 0 } else { x.trailing_zeros() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_trailing_one_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_one_us() {
    let reference = |x: u16| if x == 0 { 0 } else { x.trailing_zeros() + 1 };
    for x in samples::<u16>() {
        assert_eq!(s::stdc_first_trailing_one_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_zero_uc() {
    let reference = |x: u8| if x == u8::MAX { 0 } else { x.trailing_ones() + 1 };
    for x in samples::<u8>() {
        assert_eq!(s::stdc_first_trailing_zero_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_zero_ui() {
    let reference = |x: u32| if x == u32::MAX { 0 } else { x.trailing_ones() + 1 };
    for x in samples::<u32>() {
        assert_eq!(s::stdc_first_trailing_zero_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_zero_ul() {
    let reference = |x: u64| if x == u64::MAX { 0 } else { x.trailing_ones() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_trailing_zero_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_zero_ull() {
    let reference = |x: u64| if x == u64::MAX { 0 } else { x.trailing_ones() + 1 };
    for x in samples::<u64>() {
        assert_eq!(s::stdc_first_trailing_zero_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_first_trailing_zero_us() {
    let reference = |x: u16| if x == u16::MAX { 0 } else { x.trailing_ones() + 1 };
    for x in samples::<u16>() {
        assert_eq!(s::stdc_first_trailing_zero_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_has_single_bit_uc() {
    let reference = |x: u8| x.is_power_of_two();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_has_single_bit_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_has_single_bit_ui() {
    let reference = |x: u32| x.is_power_of_two();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_has_single_bit_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_has_single_bit_ul() {
    let reference = |x: u64| x.is_power_of_two();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_has_single_bit_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_has_single_bit_ull() {
    let reference = |x: u64| x.is_power_of_two();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_has_single_bit_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_has_single_bit_us() {
    let reference = |x: u16| x.is_power_of_two();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_has_single_bit_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_ones_uc() {
    let reference = |x: u8| x.leading_ones();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_leading_ones_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_ones_ui() {
    let reference = |x: u32| x.leading_ones();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_leading_ones_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_ones_ul() {
    let reference = |x: u64| x.leading_ones();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_leading_ones_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_ones_ull() {
    let reference = |x: u64| x.leading_ones();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_leading_ones_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_ones_us() {
    let reference = |x: u16| x.leading_ones();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_leading_ones_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_zeros_uc() {
    let reference = |x: u8| x.leading_zeros();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_leading_zeros_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_zeros_ui() {
    let reference = |x: u32| x.leading_zeros();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_leading_zeros_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_zeros_ul() {
    let reference = |x: u64| x.leading_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_leading_zeros_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_zeros_ull() {
    let reference = |x: u64| x.leading_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_leading_zeros_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_leading_zeros_us() {
    let reference = |x: u16| x.leading_zeros();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_leading_zeros_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_ones_uc() {
    let reference = |x: u8| x.trailing_ones();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_trailing_ones_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_ones_ui() {
    let reference = |x: u32| x.trailing_ones();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_trailing_ones_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_ones_ul() {
    let reference = |x: u64| x.trailing_ones();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_trailing_ones_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_ones_ull() {
    let reference = |x: u64| x.trailing_ones();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_trailing_ones_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_ones_us() {
    let reference = |x: u16| x.trailing_ones();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_trailing_ones_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_zeros_uc() {
    let reference = |x: u8| x.trailing_zeros();
    for x in samples::<u8>() {
        assert_eq!(s::stdc_trailing_zeros_uc(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_zeros_ui() {
    let reference = |x: u32| x.trailing_zeros();
    for x in samples::<u32>() {
        assert_eq!(s::stdc_trailing_zeros_ui(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_zeros_ul() {
    let reference = |x: u64| x.trailing_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_trailing_zeros_ul(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_zeros_ull() {
    let reference = |x: u64| x.trailing_zeros();
    for x in samples::<u64>() {
        assert_eq!(s::stdc_trailing_zeros_ull(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_stdc_trailing_zeros_us() {
    let reference = |x: u16| x.trailing_zeros();
    for x in samples::<u16>() {
        assert_eq!(s::stdc_trailing_zeros_us(x), reference(x), "{x:#x}");
    }
}

#[test]
fn sym_strfromd() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::strfromd(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), 3.14159), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::strfromd(b.as_mut_ptr(), 3, c("%e").as_ptr(), 3.14159), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::strfromd(null_mut(), 0, c("%g").as_ptr(), 3.14159), 7);
    assert_eq!(s::strfromd(b.as_mut_ptr(), 16, c("%A").as_ptr(), 1.0), 6);
    assert_eq!(txt(&b), "0X1P+0");
    errno::set_raw(0);
    assert_eq!(s::strfromd(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), 3.14159), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfromf128() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::liella_strfromf128_bits(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), q(3.14159)), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::liella_strfromf128_bits(b.as_mut_ptr(), 3, c("%e").as_ptr(), q(3.14159)), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::liella_strfromf128_bits(null_mut(), 0, c("%g").as_ptr(), q(3.14159)), 7);
    assert_eq!(s::liella_strfromf128_bits(b.as_mut_ptr(), 16, c("%A").as_ptr(), q(1.0)), 6);
    assert_eq!(txt(&b), "0X1P+0");
    errno::set_raw(0);
    assert_eq!(s::liella_strfromf128_bits(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), q(3.14159)), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfromf32() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::strfromf32(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), 3.14159f32), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::strfromf32(b.as_mut_ptr(), 3, c("%e").as_ptr(), 3.14159f32), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::strfromf32(null_mut(), 0, c("%g").as_ptr(), 3.14159f32), 7);
    assert_eq!(s::strfromf32(b.as_mut_ptr(), 16, c("%A").as_ptr(), 1.0f32), 6);
    assert_eq!(txt(&b), "0X1P+0");
    errno::set_raw(0);
    assert_eq!(s::strfromf32(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), 3.14159f32), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfromf32x() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::strfromf32x(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), 3.14159), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::strfromf32x(b.as_mut_ptr(), 3, c("%e").as_ptr(), 3.14159), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::strfromf32x(null_mut(), 0, c("%g").as_ptr(), 3.14159), 7);
    assert_eq!(s::strfromf32x(b.as_mut_ptr(), 16, c("%A").as_ptr(), 1.0), 6);
    assert_eq!(txt(&b), "0X1P+0");
    errno::set_raw(0);
    assert_eq!(s::strfromf32x(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), 3.14159), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfromf64() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::strfromf64(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), 3.14159), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::strfromf64(b.as_mut_ptr(), 3, c("%e").as_ptr(), 3.14159), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::strfromf64(null_mut(), 0, c("%g").as_ptr(), 3.14159), 7);
    assert_eq!(s::strfromf64(b.as_mut_ptr(), 16, c("%A").as_ptr(), 1.0), 6);
    assert_eq!(txt(&b), "0X1P+0");
    errno::set_raw(0);
    assert_eq!(s::strfromf64(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), 3.14159), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfromf64x() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), ld(3.14159)), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 3, c("%e").as_ptr(), ld(3.14159)), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::liella_strfroml_bits(null_mut(), 0, c("%g").as_ptr(), ld(3.14159)), 7);
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 16, c("%A").as_ptr(), ld(1.0)), 6);
    assert_eq!(txt(&b), "0X8P-3", "the x87 significand's top nibble leads, as the reference library prints (D-8.9)");
    errno::set_raw(0);
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), ld(3.14159)), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfromf() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::strfromf(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), 3.14159f32), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::strfromf(b.as_mut_ptr(), 3, c("%e").as_ptr(), 3.14159f32), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::strfromf(null_mut(), 0, c("%g").as_ptr(), 3.14159f32), 7);
    assert_eq!(s::strfromf(b.as_mut_ptr(), 16, c("%A").as_ptr(), 1.0f32), 6);
    assert_eq!(txt(&b), "0X1P+0");
    errno::set_raw(0);
    assert_eq!(s::strfromf(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), 3.14159f32), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strfroml() {
    let mut b = [9 as c_char; 16];
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 16, c("%.3f").as_ptr(), ld(3.14159)), 5);
    assert_eq!(txt(&b), "3.142");
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 3, c("%e").as_ptr(), ld(3.14159)), 12, "the full length is returned");
    assert_eq!(txt(&b), "3.", "truncated and terminated within n");
    assert_eq!(s::liella_strfroml_bits(null_mut(), 0, c("%g").as_ptr(), ld(3.14159)), 7);
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 16, c("%A").as_ptr(), ld(1.0)), 6);
    assert_eq!(txt(&b), "0X8P-3", "the x87 significand's top nibble leads, as the reference library prints (D-8.9)");
    errno::set_raw(0);
    assert_eq!(s::liella_strfroml_bits(b.as_mut_ptr(), 16, c("%5.1f").as_ptr(), ld(3.14159)), -1, "no width or flags");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_strtod() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtod(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___strtod_internal() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::__strtod_internal(t.as_ptr(), Some(e), 0);
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___strtod_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::__strtod_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtod_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtod_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&c("1e-50"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___strtof128_internal() {
    // The C forwarder converts these bits to _Float128.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtof128_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.hi, r.lo), (0x3FFF_8000_0000_0000, 0));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let third = s::liella_strtof128_bits(c("0.1").as_ptr(), None);
    assert_eq!((third.hi, third.lo), (0x3FFB_9999_9999_9999, 0x9999_9999_9999_999A), "correctly rounded in 113 bits");
}

#[test]
fn sym_strtof128_l() {
    // The C forwarder converts these bits to _Float128.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtof128_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.hi, r.lo), (0x3FFF_8000_0000_0000, 0));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let third = s::liella_strtof128_bits(c("0.1").as_ptr(), None);
    assert_eq!((third.hi, third.lo), (0x3FFB_9999_9999_9999, 0x9999_9999_9999_999A), "correctly rounded in 113 bits");
}

#[test]
fn sym_strtof128() {
    // The C forwarder converts these bits to _Float128.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtof128_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.hi, r.lo), (0x3FFF_8000_0000_0000, 0));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let third = s::liella_strtof128_bits(c("0.1").as_ptr(), None);
    assert_eq!((third.hi, third.lo), (0x3FFB_9999_9999_9999, 0x9999_9999_9999_999A), "correctly rounded in 113 bits");
}

#[test]
fn sym_strtof32_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof32_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&c("1e-50"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof32() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof32(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&c("1e-50"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof32x_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof32x_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof32x() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof32x(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof64_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof64_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof64() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof64(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&c("1e-400"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof64x_l() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_strtold_bits(c("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_strtold_bits(c("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym_strtof64x() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_strtold_bits(c("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_strtold_bits(c("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym___strtof_internal() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::__strtof_internal(t.as_ptr(), Some(e), 0);
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&c("1e-50"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___strtof_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::__strtof_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&c("1e-50"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtof_l() {
    let f = |t: &[c_char], e: &mut *mut c_char| s::strtof_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = c(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = c("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&c("nan(12)"), &mut e).is_nan());
    erange(|| f(&c("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&c("1e-50"), &mut e), 0.0);
    let t = c("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&c("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_strtoimax() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoimax(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoimax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_strtol() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtol(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtol(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___strtol_internal() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtol_internal(s.as_ptr(), Some(e), b, 0);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtol_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___strtol_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtol_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtol_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtol_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtol_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtol_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtold() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_strtold_bits(c("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_strtold_bits(c("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym___strtold_internal() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_strtold_bits(c("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_strtold_bits(c("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym___strtold_l() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_strtold_bits(c("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_strtold_bits(c("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym_strtold_l() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = c("1.5x");
    let mut e = null_mut();
    let r = s::liella_strtold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_strtold_bits(c("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_strtold_bits(c("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym_strtoll() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoll(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoll(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___strtoll_internal() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtoll_internal(s.as_ptr(), Some(e), b, 0);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtoll_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___strtoll_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtoll_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtoll_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoll_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoll_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoll_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoq() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoq(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = c("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoq(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoul() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoul(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoul(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___strtoul_internal() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtoul_internal(s.as_ptr(), Some(e), b, 0);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtoul_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___strtoul_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtoul_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtoul_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoul_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoul_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoul_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoull() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoull(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoull(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___strtoull_internal() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtoull_internal(s.as_ptr(), Some(e), b, 0);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtoull_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___strtoull_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::__strtoull_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__strtoull_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoull_l() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoull_l(s.as_ptr(), Some(e), b, null_mut());
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoull_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_strtoumax() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtoumax(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtoumax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_strtouq() {
    let f = |s: &[c_char], e: &mut *mut c_char, b: c_int| s::strtouq(s.as_ptr(), Some(e), b);
    let t = c("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = c("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = c("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = c("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = c("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = c("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = c("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::strtouq(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstod() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstod(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___wcstod_internal() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::__wcstod_internal(t.as_ptr(), Some(e), 0);
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___wcstod_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::__wcstod_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstod_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstod_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&w("1e-50"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___wcstof128_internal() {
    // The C forwarder converts these bits to _Float128.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstof128_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.hi, r.lo), (0x3FFF_8000_0000_0000, 0));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let third = s::liella_wcstof128_bits(w("0.1").as_ptr(), None);
    assert_eq!((third.hi, third.lo), (0x3FFB_9999_9999_9999, 0x9999_9999_9999_999A), "correctly rounded in 113 bits");
}

#[test]
fn sym_wcstof128_l() {
    // The C forwarder converts these bits to _Float128.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstof128_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.hi, r.lo), (0x3FFF_8000_0000_0000, 0));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let third = s::liella_wcstof128_bits(w("0.1").as_ptr(), None);
    assert_eq!((third.hi, third.lo), (0x3FFB_9999_9999_9999, 0x9999_9999_9999_999A), "correctly rounded in 113 bits");
}

#[test]
fn sym_wcstof128() {
    // The C forwarder converts these bits to _Float128.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstof128_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.hi, r.lo), (0x3FFF_8000_0000_0000, 0));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let third = s::liella_wcstof128_bits(w("0.1").as_ptr(), None);
    assert_eq!((third.hi, third.lo), (0x3FFB_9999_9999_9999, 0x9999_9999_9999_999A), "correctly rounded in 113 bits");
}

#[test]
fn sym_wcstof32_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof32_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&w("1e-50"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof32() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof32(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&w("1e-50"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof32x_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof32x_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof32x() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof32x(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof64_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof64_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof64() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof64(t.as_ptr(), Some(e));
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f64::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e400"), &mut e), f64::INFINITY);
    erange(|| f(&w("1e-400"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof64x_l() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_wcstold_bits(w("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_wcstold_bits(w("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym_wcstof64x() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_wcstold_bits(w("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_wcstold_bits(w("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym___wcstof_internal() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::__wcstof_internal(t.as_ptr(), Some(e), 0);
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&w("1e-50"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym___wcstof_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::__wcstof_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&w("1e-50"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstof_l() {
    let f = |t: &[wchar_t], e: &mut *mut wchar_t| s::wcstof_l(t.as_ptr(), Some(e), null_mut());
    let mut e = null_mut();
    let t = w(" 0x1.8p1rest");
    assert_eq!(f(&t, &mut e), 3.0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(8));
    let t = w("-INFINITYx");
    assert_eq!(f(&t, &mut e), f32::NEG_INFINITY);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(9));
    assert!(f(&w("nan(12)"), &mut e).is_nan());
    erange(|| f(&w("1e40"), &mut e), f32::INFINITY);
    erange(|| f(&w("1e-50"), &mut e), 0.0);
    let t = w("e5");
    assert_eq!(f(&t, &mut e), 0.0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no conversion");
    assert_eq!(f(&w("0.1"), &mut e), 0.1, "correctly rounded");
}

#[test]
fn sym_wcstoimax() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoimax(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoimax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstol() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstol(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstol(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstol_internal() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstol_internal(s.as_ptr(), Some(e), b, 0);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstol_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstol_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstol_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstol_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstol_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstol_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstol_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstold() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_wcstold_bits(w("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_wcstold_bits(w("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym___wcstold_internal() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_wcstold_bits(w("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_wcstold_bits(w("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym___wcstold_l() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_wcstold_bits(w("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_wcstold_bits(w("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym_wcstold_l() {
    // The C forwarder (csys/src/adapter/float.c) converts these bits to long double.
    let t = w("1.5x");
    let mut e = null_mut();
    let r = s::liella_wcstold_bits(t.as_ptr(), Some(&mut e));
    assert_eq!((r.mant, r.se), (0xC000_0000_0000_0000, 0x3FFF));
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(3));
    let m = s::liella_wcstold_bits(w("-0x1p-16445").as_ptr(), None);
    assert_eq!((m.mant, m.se), (1, 0x8000), "the smallest subnormal");
    erange(|| s::liella_wcstold_bits(w("1e5000").as_ptr(), None).se, 0x7FFF);
}

#[test]
fn sym_wcstoll() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoll(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoll(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstoll_internal() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstoll_internal(s.as_ptr(), Some(e), b, 0);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstoll_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstoll_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstoll_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstoll_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoll_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoll_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoll_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoq() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoq(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), -31);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MAX);
    let t = w("-99999999999999999999999");
    erange(|| f(&t, &mut e, 10), i64::MIN);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoq(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoul() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoul(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoul(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstoul_internal() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstoul_internal(s.as_ptr(), Some(e), b, 0);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstoul_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstoul_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstoul_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstoul_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoul_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoul_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoul_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoull() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoull(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoull(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstoull_internal() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstoull_internal(s.as_ptr(), Some(e), b, 0);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstoull_internal(t.as_ptr(), None, 10, 0), 77, "a NULL end pointer");
}

#[test]
fn sym___wcstoull_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::__wcstoull_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::__wcstoull_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoull_l() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoull_l(s.as_ptr(), Some(e), b, null_mut());
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoull_l(t.as_ptr(), None, 10, null_mut()), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstoumax() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstoumax(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstoumax(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}

#[test]
fn sym_wcstouq() {
    let f = |s: &[wchar_t], e: &mut *mut wchar_t, b: c_int| s::wcstouq(s.as_ptr(), Some(e), b);
    let t = w("  -0x1fz");
    let mut e = null_mut();
    assert_eq!(f(&t, &mut e, 16), 31u64.wrapping_neg(), "a negated magnitude wraps");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(7), "the end pointer");
    let t = w("0x");
    assert_eq!(f(&t, &mut e, 0), 0);
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1), "\"0x\" without digits stops after the 0");
    let t = w("+");
    assert_eq!(f(&t, &mut e, 10), 0);
    assert_eq!(e.cast_const(), t.as_ptr(), "no digits: the end is the input");
    let t = w("99999999999999999999999");
    erange(|| f(&t, &mut e, 10), u64::MAX);
    let t = w("12");
    errno::set_raw(0);
    assert_eq!(f(&t, &mut e, 37), 0);
    assert_eq!(errno::get(), EINVAL, "an invalid base");
    let t = w("0b101");
    assert_eq!(f(&t, &mut e, 0), 0, "C99: 0b is not a prefix");
    assert_eq!(e.cast_const(), t.as_ptr().wrapping_add(1));
    let t = w("077");
    assert_eq!(f(&t, &mut e, 0), 0o77, "a leading 0 is octal");
    assert_eq!(s::wcstouq(t.as_ptr(), None, 10), 77, "a NULL end pointer");
}
