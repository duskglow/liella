//! The scanf engine (`liella::scanf::scan`) driven directly, without C varargs (spec §12.7;
//! D-12.6, D-12.13): the format grammar, scansets, widths, assignment suppression, `%n`, `%%`,
//! positional arguments, `%m`, the wide and narrow text conversions, encoding and read errors,
//! and the floating conversions' formats and rounding. `scanf_products.rs` compares the whole
//! engine with the reference library; these cases pin the engine's own contract in seconds.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::panic)]

use liella::error::{Errno, Result};
use liella::locale::Locale;
use liella::num::float::Rounding;
use liella::scanf::{scan, Ctx, FmtUnit, Input, Outcome, Scanned, Set, Sink};

/// Input units, with an optional read error at a position.
struct Units {
    u: Vec<u32>,
    pos: usize,
    fail_at: Option<usize>,
}

impl Input for Units {
    fn next(&mut self) -> Result<Option<u32>> {
        if self.fail_at == Some(self.pos) {
            return Err(Errno::EIO);
        }
        let c = self.u.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        Ok(c)
    }
    fn back(&mut self, _: u32) {
        self.pos -= 1;
    }
}

/// What the engine stored, by argument number (pointer `n` is `n * 16`).
#[derive(Debug, PartialEq, Clone)]
enum St {
    Int(usize, usize, u64),
    Bytes(usize, Vec<u8>),
    Wide(usize, Vec<u32>),
    AllocB(usize, Vec<u8>),
    AllocW(usize, Vec<u32>),
    Null(usize),
}

#[derive(Default)]
struct Rec(Vec<St>);

impl Sink for Rec {
    fn pointer(&mut self, n: usize) -> usize {
        n * 16
    }
    fn int(&mut self, addr: usize, size: usize, value: u64) {
        // The low `size` bytes are what reaches the object.
        let mask = if size >= 8 { u64::MAX } else { (1u64 << (size * 8)) - 1 };
        self.0.push(St::Int(addr / 16, size, value & mask));
    }
    fn bytes(&mut self, addr: usize, data: &[u8]) {
        self.0.push(St::Bytes(addr / 16, data.to_vec()));
    }
    fn wide(&mut self, addr: usize, data: &[u32]) {
        self.0.push(St::Wide(addr / 16, data.to_vec()));
    }
    fn alloc_bytes(&mut self, addr: usize, data: &[u8]) -> Result<()> {
        self.0.push(St::AllocB(addr / 16, data.to_vec()));
        Ok(())
    }
    fn alloc_wide(&mut self, addr: usize, data: &[u32]) -> Result<()> {
        self.0.push(St::AllocW(addr / 16, data.to_vec()));
        Ok(())
    }
    fn null(&mut self, addr: usize) {
        self.0.push(St::Null(addr / 16));
    }
}

const UTF8: Locale = Locale::c_utf8(b"C.UTF-8");

struct Run {
    out: Scanned,
    pos: usize,
    st: Vec<St>,
}

fn go<F: FmtUnit>(fmt: &[F], input: Vec<u32>, fail_at: Option<usize>, loc: &Locale, rounding: Rounding, set: Set) -> Run {
    let ctx = Ctx { loc, enc: loc.encoding(), rounding, set };
    let mut inp = Units { u: input, pos: 0, fail_at };
    let mut rec = Rec::default();
    let out = scan(fmt, &mut inp, &mut rec, ctx).unwrap();
    Run { out, pos: inp.pos, st: rec.0 }
}

/// Byte format over byte input in the C locale, C99 set.
fn n(input: &[u8], fmt: &str) -> Run {
    go(fmt.as_bytes(), input.iter().map(|&b| u32::from(b)).collect(), None, &Locale::C, Rounding::Nearest, Set::C99)
}

/// Byte format over byte input in C.UTF-8.
fn u(input: &str, fmt: &[u8]) -> Run {
    let utf8 = UTF8;
    go(fmt, input.bytes().map(u32::from).collect(), None, &utf8, Rounding::Nearest, Set::C99)
}

/// Wide format over wide input in C.UTF-8.
fn w(input: &str, fmt: &str) -> Run {
    let utf8 = UTF8;
    let f: Vec<i32> = fmt.chars().map(|c| c as i32).collect();
    go(&f, input.chars().map(u32::from).collect(), None, &utf8, Rounding::Nearest, Set::C99)
}

fn a(k: usize) -> Outcome {
    Outcome::Assigned(k)
}

fn int(arg: usize, size: usize, v: i64) -> St {
    let mask = if size == 8 { u64::MAX } else { (1u64 << (size * 8)) - 1 };
    St::Int(arg, size, (v as u64) & mask)
}

fn b(arg: usize, s: &[u8]) -> St {
    St::Bytes(arg, s.to_vec())
}

fn wide(arg: usize, s: &str) -> St {
    St::Wide(arg, s.chars().map(u32::from).collect())
}

fn f64_of(st: &St) -> f64 {
    match st {
        St::Bytes(_, d) if d.len() == 8 => f64::from_le_bytes(d[..8].try_into().unwrap()),
        other => panic!("not a double: {other:?}"),
    }
}

#[test]
fn directives_white_space_and_ordinary_characters() {
    // s12_7_c_1: white space matches any amount, none included.
    let r = n(b"  \t5", " %d");
    assert_eq!((r.out.outcome, r.pos, r.st), (a(1), 4, vec![int(1, 4, 5)]));
    let r = n(b"1\n\n 2", "%d%d");
    assert_eq!(r.st, vec![int(1, 4, 1), int(2, 4, 2)]);
    // s12_7_c_2: an ordinary character must match; the mismatch stops the scan, left unread.
    let r = n(b"1;2", "%d,%d");
    assert_eq!((r.out.outcome, r.pos), (a(1), 1));
    let r = n(b"a5b", "a%db");
    assert_eq!((r.out.outcome, r.pos), (a(1), 3));
    // s12_7_g_0: a field that cannot start is a matching failure with nothing consumed.
    let r = n(b"x", "%d");
    assert_eq!((r.out.outcome, r.pos), (a(0), 0));
    // `%%` skips white space, then matches one `%`.
    let r = n(b"5  %6", "%d%%%d");
    assert_eq!((r.out.outcome, r.st), (a(2), vec![int(1, 4, 5), int(2, 4, 6)]));
    let r = n(b"5 x", "%d %%");
    assert_eq!((r.out.outcome, r.pos), (a(1), 2));
}

#[test]
fn end_of_input_and_input_failures() {
    // s12_7_c_0: EOF only when input fails before the first conversion.
    assert_eq!(n(b"", "%d").out.outcome, Outcome::InputFailure);
    assert_eq!(n(b"   ", "%d").out.outcome, Outcome::InputFailure);
    assert_eq!(n(b"", "%s").out.outcome, Outcome::InputFailure);
    assert_eq!(n(b"", "%c").out.outcome, Outcome::InputFailure);
    assert_eq!(n(b"", "%[a]").out.outcome, Outcome::InputFailure);
    assert_eq!(n(b"", "x%d").out.outcome, Outcome::InputFailure);
    assert_eq!(n(b"7", "%d %d").out.outcome, a(1));
    // D-12.13: a suppressed conversion counts as a conversion; `%n` does not.
    assert_eq!(n(b"7", "%*d %d").out.outcome, a(0));
    assert_eq!(n(b"", "%n%d").out.outcome, Outcome::InputFailure);
    // A read error is an input failure, reported through `errno`.
    let fmt = "%d %d";
    let r = go(fmt.as_bytes(), b"5 6".iter().map(|&c| u32::from(c)).collect(), Some(0), &Locale::C, Rounding::Nearest, Set::C99);
    assert_eq!((r.out.outcome, r.out.error), (Outcome::InputFailure, Some(Errno::EIO)));
    let r = go(fmt.as_bytes(), b"5 6".iter().map(|&c| u32::from(c)).collect(), Some(2), &Locale::C, Rounding::Nearest, Set::C99);
    assert_eq!((r.out.outcome, r.out.error, r.st), (a(1), Some(Errno::EIO), vec![int(1, 4, 5)]));
    assert_eq!(n(b"5", "%d").out.error, None);
}

#[test]
fn invalid_specifications_stop_the_scan_without_eof() {
    // s12_7_c_3 (D-12.13): `%y`, `%5%`, an unclosed `%[` and a trailing `%` stop the scan.
    let r = n(b"1 2", "%d %y%d");
    assert_eq!((r.out.outcome, r.st), (a(1), vec![int(1, 4, 1)]));
    assert_eq!(n(b"1 % 2", "%d %5% %d").out.outcome, a(1));
    let r = n(b"1 abc", "%d %[abc");
    assert_eq!((r.out.outcome, r.st.len()), (a(1), 1));
    assert_eq!(n(b"1", "%d%").out.outcome, a(1));
}

#[test]
fn grammar_edges_of_the_specification() {
    // s12_7_c_3 (D-12.13, D-12.24 (1)): `%0$` and `m` on a
    // non-text conversion are invalid (the scan stops, nothing stored); a width of 0 means none.
    let r = n(b"5 6", "%0$d %d");
    assert_eq!((r.out.outcome, r.st), (a(0), vec![]));
    let r = n(b"5 6", "%md %d");
    assert_eq!((r.out.outcome, r.st), (a(0), vec![]));
    let r = n(b"1.5", "%mf");
    assert_eq!((r.out.outcome, r.st), (a(0), vec![]));
    let r = n(b"12345 x", "%0d %c");
    assert_eq!((r.out.outcome, r.st), (a(2), vec![int(1, 4, 12345), b(2, b"x")]));
    let r = n(b"abc", "%0s");
    assert_eq!((r.out.outcome, r.st), (a(1), vec![b(1, b"abc\0")]));
}

#[test]
fn wide_sets_with_members_above_255() {
    // s12_7_c_3 (decision record §2): a range above 255 is tested against the set's text.
    let r = w("\u{3b2}\u{3b3}x", "%l[\u{3b1}-\u{3c9}]");
    assert_eq!((r.out.outcome, r.st), (a(1), vec![St::Wide(1, vec![0x3b2, 0x3b3, 0])]));
    let r = w("\u{3b2}x", "%l[^\u{3b1}-\u{3c9}]");
    assert_eq!(r.out.outcome, a(0));
}

#[test]
fn long_fields_are_stored_in_chunks_after_each_other() {
    // s12_7_c_3 (decision record §3): text goes to the caller's object 1024 units at a time, each chunk after the
    // last (the test sink records address / 16: 16 + 1024 bytes is 65, 16 + 4096 is 257).
    let r = n(&[b'a'; 1500], "%s");
    let mut tail = vec![b'a'; 476];
    tail.push(0);
    assert_eq!((r.out.outcome, r.st), (a(1), vec![b(1, &[b'a'; 1024]), b(65, &tail)]));
    let r = w(&"a".repeat(1500), "%ls");
    let mut tail = vec![0x61u32; 476];
    tail.push(0);
    assert_eq!((r.out.outcome, r.st), (a(1), vec![St::Wide(1, vec![0x61; 1024]), St::Wide(257, tail)]));
}

#[test]
fn wide_input_the_encoding_cannot_represent_fails() {
    // s12_7_c_3 (D-12.13): wide input stored as bytes; a character the locale cannot encode is EILSEQ.
    let f: Vec<i32> = "%s".chars().map(|c| c as i32).collect();
    let r = go(&f, vec![0x20ac, 0x61], None, &Locale::C, Rounding::Nearest, Set::C99);
    assert_eq!((r.out.outcome, r.out.error), (a(0), Some(Errno::EILSEQ)));
}

#[test]
fn widths_lengths_and_suppression() {
    // s12_7_c_3: `%[*][width][m][len]conv`.
    let r = n(b"12345", "%3d%d");
    assert_eq!(r.st, vec![int(1, 4, 123), int(2, 4, 45)]);
    let r = n(b"-12", "%2d");
    assert_eq!((r.pos, r.st), (2, vec![int(1, 4, -1)]));
    let r = n(b"x 7", "%*s %d");
    assert_eq!((r.out.outcome, r.st), (a(1), vec![int(1, 4, 7)]));
    let r = n(b"1 2 3", "%hhd %hd %lld");
    assert_eq!(r.st, vec![int(1, 1, 1), int(2, 2, 2), int(3, 8, 3)]);
    let r = n(b"4 5 6 7 8", "%ld %jd %zd %td %qd");
    assert_eq!(r.st, vec![int(1, 8, 4), int(2, 8, 5), int(3, 8, 6), int(4, 8, 7), int(5, 8, 8)]);
    // `%n`: units consumed so far, at the length's size; suppressed, it stores nothing.
    let r = n(b"12 ab", "%d%n %*s%lln%hhn");
    assert_eq!((r.out.outcome, r.st), (a(1), vec![int(1, 4, 12), int(2, 4, 2), int(3, 8, 5), int(4, 1, 5)]));
    let r = n(b"12", "%d%*n");
    assert_eq!(r.st, vec![int(1, 4, 12)]);
}

#[test]
fn integer_conversions() {
    // s12_7_c_3 and D-12.13: strtol/strtoul fields, prefixes, one unit of pushback.
    let r = n(b"0x1f 017 9 -0X1A 8", "%i %i %u %x %o");
    assert_eq!((r.out.outcome, r.pos), (a(4), 17));
    assert_eq!(r.st, vec![int(1, 4, 31), int(2, 4, 15), int(3, 4, 9), int(4, 4, -26)]);
    let r = n(b"+5 + 6", "%d %d");
    assert_eq!((r.out.outcome, r.pos), (a(1), 4));
    let r = n(b"0xg", "%i");
    assert_eq!((r.out.outcome, r.pos, r.st), (a(1), 2, vec![int(1, 4, 0)]));
    let r = n(b"0x10", "%p");
    assert_eq!(r.st, vec![int(1, 8, 16)]);
    let r = n(b"101 0b11", "%b %b");
    assert_eq!(r.st, vec![int(1, 4, 5), int(2, 4, 3)]);
    let r = n(b"ff", "%X");
    assert_eq!(r.st, vec![int(1, 4, 255)]);
    let r = n(b"1234", "%'d");
    assert_eq!(r.st, vec![int(1, 4, 1234)]);
}

#[test]
fn integers_out_of_range_store_the_nearest_bound() {
    // s12_7_f_0 (D-12.6): clamped, without an error; unsigned `-` negates modulo 2^N.
    assert_eq!(n(b"70000", "%hd").st, vec![int(1, 2, 32767)]);
    assert_eq!(n(b"-70000", "%hd").st, vec![int(1, 2, -32768)]);
    assert_eq!(n(b"70000", "%hu").st, vec![int(1, 2, 65535)]);
    assert_eq!(n(b"-1", "%hu").st, vec![int(1, 2, 65535)]);
    assert_eq!(n(b"-2", "%hhu").st, vec![int(1, 1, 254)]);
    assert_eq!(n(b"9223372036854775808", "%ld").st, vec![int(1, 8, i64::MAX)]);
    assert_eq!(n(b"-9223372036854775808", "%lld").st, vec![int(1, 8, i64::MIN)]);
    assert_eq!(n(b"99999999999999999999", "%lu").st, vec![int(1, 8, -1)]);
    assert_eq!(n(b"4294967296", "%u").st, vec![int(1, 4, -1)]);
    let r = n(b"99999999999", "%d");
    assert_eq!((r.out.outcome, r.out.error, r.st), (a(1), None, vec![int(1, 4, i64::from(i32::MAX))]));
}

#[test]
fn floating_conversions() {
    // s12_7_c_3, s12_7_g_0 (D-12.13): decimal, hexadecimal, inf/infinity, nan; partial fields fail.
    let r = n(b"1.5e3x", "%lf");
    assert_eq!((r.out.outcome, r.pos, f64_of(&r.st[0])), (a(1), 5, 1500.0));
    assert_eq!(f64_of(&n(b"-.5", "%lf").st[0]), -0.5);
    assert_eq!(f64_of(&n(b"0x1.8p1", "%lf").st[0]), 3.0);
    assert_eq!(f64_of(&n(b"5.", "%lf").st[0]), 5.0);
    assert_eq!(f64_of(&n(b"1e400", "%lf").st[0]), f64::INFINITY);
    assert_eq!(f64_of(&n(b"-INFINITY", "%lf").st[0]), f64::NEG_INFINITY);
    let r = n(b"INFx", "%lf");
    assert_eq!((r.pos, f64_of(&r.st[0])), (3, f64::INFINITY));
    let r = n(b"nan(1)", "%lf");
    assert!(f64_of(&r.st[0]).is_nan());
    assert_eq!(r.pos, 3);
    let r = n(b"1.25", "%3lf");
    assert_eq!((r.pos, f64_of(&r.st[0])), (3, 1.2));
    let r = n(b"1,5", "%e");
    assert_eq!((r.pos, r.st), (1, vec![b(1, &1.0f32.to_le_bytes())]));
    for (input, used) in [(&b"."[..], 1), (b"1e", 2), (b"1e+", 3), (b"0x", 2), (b"-", 1), (b"in", 2), (b"infin", 5), (b"0x1p", 4)] {
        let r = n(input, "%lf");
        assert_eq!((r.out.outcome, r.pos, r.st.len()), (a(0), used, 0), "{input:?}");
    }
    // `%*f` reads and stores nothing.
    let r = n(b"2.5 3", "%*f %d");
    assert_eq!((r.out.outcome, r.st), (a(1), vec![int(1, 4, 3)]));
}

#[test]
fn floating_formats_and_rounding() {
    // s12_7_c_3: float, double and x87 long double, in the caller's rounding mode.
    let r = n(b"1.5", "%Lf");
    match &r.st[0] {
        St::Bytes(1, d) => assert_eq!(d[..10], [0, 0, 0, 0, 0, 0, 0, 0xC0, 0xFF, 0x3F]),
        other => panic!("{other:?}"),
    }
    let tenth = |mode| {
        let r = go(b"%f".as_slice(), b"0.1".iter().map(|&c| u32::from(c)).collect(), None, &Locale::C, mode, Set::C99);
        r.st
    };
    assert_eq!(tenth(Rounding::Nearest), vec![b(1, &0x3DCC_CCCDu32.to_le_bytes())]);
    assert_eq!(tenth(Rounding::Up), vec![b(1, &0x3DCC_CCCDu32.to_le_bytes())]);
    assert_eq!(tenth(Rounding::Down), vec![b(1, &0x3DCC_CCCCu32.to_le_bytes())]);
    assert_eq!(tenth(Rounding::Zero), vec![b(1, &0x3DCC_CCCCu32.to_le_bytes())]);
}

#[test]
fn symbol_sets_and_a() {
    // s12_7_f_1, s12_7_f_2, s12_7_f_3: `%a` allocates before s, S, [ in the plain set only.
    let g = |input: &[u8], fmt: &str, set| go(fmt.as_bytes(), input.iter().map(|&c| u32::from(c)).collect(), None, &Locale::C, Rounding::Nearest, set);
    assert_eq!(g(b"xy", "%as", Set::Gnu).st, vec![St::AllocB(1, b"xy\0".to_vec())]);
    assert_eq!(g(b"xy", "%a[xy]", Set::Gnu).st, vec![St::AllocB(1, b"xy\0".to_vec())]);
    assert_eq!(g(b"0x1p3", "%a", Set::C99).st, vec![b(1, &8.0f32.to_le_bytes())]);
    let r = g(b"1.5s", "%as", Set::C99);
    assert_eq!((r.out.outcome, r.pos, r.st), (a(1), 4, vec![b(1, &1.5f32.to_le_bytes())]));
    assert_eq!(g(b"0b101", "%i", Set::C99).st, vec![int(1, 4, 0)]);
    assert_eq!(g(b"0b101", "%i", Set::C23).st, vec![int(1, 4, 5)]);
}

#[test]
fn text_conversions_and_scansets() {
    // s12_7_c_3: %s skips white space and stops at it; %c and %[ do not skip.
    let r = n(b"  ab cd", "%s");
    assert_eq!((r.pos, r.st), (4, vec![b(1, b"ab\0")]));
    let r = n(b"abcd", "%2s%s");
    assert_eq!(r.st, vec![b(1, b"ab\0"), b(2, b"cd\0")]);
    let r = n(b" x", "%c");
    assert_eq!(r.st, vec![b(1, b" ")]);
    let r = n(b"abc", "%2c");
    assert_eq!((r.pos, r.st), (2, vec![b(1, b"ab")]));
    // D-12.13: `]` first (after `^`) and `-` first or last are literals; a reversed range is three
    // characters.
    assert_eq!(n(b"]a]b", "%[]a]").st, vec![b(1, b"]a]\0")]);
    assert_eq!(n(b"ab]", "%[^]]").st, vec![b(1, b"ab\0")]);
    assert_eq!(n(b"a-b", "%[a-]").st, vec![b(1, b"a-\0")]);
    assert_eq!(n(b"-ab", "%[-a]").st, vec![b(1, b"-a\0")]);
    assert_eq!(n(b"abcd", "%[a-c]").st, vec![b(1, b"abc\0")]);
    assert_eq!(n(b"c-ab", "%[c-a]").st, vec![b(1, b"c-a\0")]);
    assert_eq!(n(b"a b,c", "%[^,]").st, vec![b(1, b"a b\0")]);
    assert_eq!(n(b"1234", "%2[0-9]").st, vec![b(1, b"12\0")]);
    // A set that matches nothing is a matching failure that writes nothing.
    let r = n(b"b", "%[a]");
    assert_eq!((r.out.outcome, r.pos), (a(0), 0));
    assert!(r.st.iter().all(|s| matches!(s, St::Bytes(_, d) if d.is_empty())), "{:?}", r.st);
}

#[test]
fn allocation() {
    // s12_7_c_3: `m` allocates; a failed conversion stores NULL (D-12.13).
    assert_eq!(n(b"abc", "%ms").st, vec![St::AllocB(1, b"abc\0".to_vec())]);
    assert_eq!(n(b"ab", "%3mc").st, vec![St::AllocB(1, b"ab".to_vec())]);
    let r = n(b"", "%ms");
    assert_eq!((r.out.outcome, r.st), (Outcome::InputFailure, vec![St::Null(1)]));
    let r = n(b"b", "%m[a]");
    assert_eq!((r.out.outcome, r.st), (a(0), vec![St::Null(1)]));
    let r = n(b"ab", "%*ms");
    assert_eq!((r.out.outcome, r.st), (a(0), vec![]));
    let r = u("h\u{e9}", b"%mls");
    assert_eq!(r.st, vec![St::AllocW(1, vec![0x68, 0xE9, 0])]);
}

#[test]
fn positional_arguments() {
    // s12_7_c_3: `%n$` names the argument.
    assert_eq!(n(b"4 9", "%2$d %1$d").st, vec![int(2, 4, 4), int(1, 4, 9)]);
    assert_eq!(n(b"4 9", "%1$d %1$d").st, vec![int(1, 4, 4), int(1, 4, 9)]);
    assert_eq!(n(b"ab 3", "%2$s %1$d").st, vec![b(2, b"ab\0"), int(1, 4, 3)]);
}

#[test]
fn byte_input_to_wide_destinations() {
    // s12_7_c_3 (D-12.13): byte input decoded for %ls %lc %l[, the width counting characters.
    let r = u("caf\u{e9} ok", b"%ls");
    assert_eq!((r.pos, r.st), (5, vec![wide(1, "caf\u{e9}\0")]));
    let r = u("\u{e9}\u{e0}\u{fc}!", b"%3ls");
    assert_eq!((r.pos, r.st), (6, vec![wide(1, "\u{e9}\u{e0}\u{fc}\0")]));
    let r = u("\u{e9}x", b"%lc");
    assert_eq!(r.st, vec![wide(1, "\u{e9}")]);
    let r = u("\u{e9}x", b"%C");
    assert_eq!(r.st, vec![wide(1, "\u{e9}")]);
    let r = u("a\u{e9}b", b"%S");
    assert_eq!(r.st, vec![wide(1, "a\u{e9}b\0")]);
    // The set is tested byte by byte.
    let r = u("a\u{e9}z", b"%l[a\xc3\xa9]");
    assert_eq!(r.st, vec![wide(1, "a\u{e9}\0")]);
    // An invalid or incomplete sequence fails the conversion with EILSEQ.
    let bad = |input: &[u8], fmt: &str| go(fmt.as_bytes(), input.iter().map(|&c| u32::from(c)).collect(), None, &UTF8, Rounding::Nearest, Set::C99);
    let r = bad(b"ab\xff", "%ls");
    assert_eq!((r.out.outcome, r.out.error), (a(0), Some(Errno::EILSEQ)));
    let r = bad(b"a\xc3", "%ls");
    assert_eq!((r.out.outcome, r.out.error), (a(0), Some(Errno::EILSEQ)));
    let r = bad(b"\xff", "%lc");
    assert_eq!((r.out.outcome, r.out.error), (a(0), Some(Errno::EILSEQ)));
    // Narrow destinations take the bytes as they are.
    let r = bad(b"a\xffb", "%s");
    assert_eq!((r.out.outcome, r.out.error, r.st), (a(1), None, vec![b(1, b"a\xffb\0")]));
}

#[test]
fn wide_input() {
    // s12_7_c_3 (D-12.13): wide formats over wide input; narrow destinations are encoded with one
    // terminator.
    let r = w("12 h\u{e9}llo x", "%d %ls");
    assert_eq!((r.out.outcome, r.st), (a(2), vec![int(1, 4, 12), wide(2, "h\u{e9}llo\0")]));
    let r = w("h\u{e9}llo", "%s");
    assert_eq!(r.st, vec![b(1, b"h\xc3\xa9llo\0")]);
    let r = w("\u{e9}\u{e9}\u{e9}", "%2s");
    assert_eq!((r.pos, r.st), (2, vec![b(1, b"\xc3\xa9\xc3\xa9\0")]));
    let r = w("\u{e9}", "%c");
    assert_eq!(r.st, vec![b(1, b"\xc3\xa9")]);
    let r = w("\u{e9}\u{e8}a", "%l[\u{e8}\u{e9}]");
    assert_eq!(r.st, vec![wide(1, "\u{e9}\u{e8}\0")]);
    let r = w("\u{e9}\u{e8}a", "%[\u{e0}-\u{ff}]");
    assert_eq!(r.st, vec![b(1, b"\xc3\xa9\xc3\xa8\0")]);
    let r = w("1.5 x", "%lf %lc");
    assert_eq!((r.out.outcome, f64_of(&r.st[0]), r.st[1].clone()), (a(2), 1.5, wide(2, "x")));
    let r = w("ab", "%mls");
    assert_eq!(r.st, vec![St::AllocW(1, vec![0x61, 0x62, 0])]);
    let r = w("5 % 6", "%d %% %d");
    assert_eq!(r.out.outcome, a(2));
}
