//! `strfmon`'s rules (§7.5; D-7.11) through `liella::monetary` with conventions built here, so
//! that each placement, grouping and error rule is checked on its own (the named locales are
//! compared with glibc in `monetary_products.rs`). Written before the COLD-REWRITE2-cold-02
//! rewrite and passing on the old code: they pin the behaviour the rewrite keeps.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::error::Errno;
use liella::monetary::{self, Amounts, Conventions, Placement};
use liella::num::float::{Float, Format, Rounding};

/// Amounts as `f64` values; records which conversions asked for a `long double`.
struct Doubles {
    values: Vec<f64>,
    asked: Vec<bool>,
}

impl Amounts for Doubles {
    fn amount(&mut self, long_double: bool) -> Float {
        self.asked.push(long_double);
        Float::unpack(u128::from(self.values.remove(0).to_bits()), Format::F64)
    }
}

fn place(cs_precedes: u8, sep_by_space: u8, sign_posn: u8) -> Placement {
    Placement { cs_precedes, sep_by_space, sign_posn }
}

/// `$`, `USD `, grouping by threes, signs `""`/`-`, two fraction digits, the given placements
/// (positive, negative) for both `%n` and `%i`.
fn conv(p: Placement, n: Placement) -> Conventions<'static> {
    Conventions {
        int_curr_symbol: b"USD ",
        currency_symbol: b"$",
        decimal_point: b".",
        thousands_sep: b",",
        grouping: &[3],
        positive_sign: b"",
        negative_sign: b"-",
        int_frac_digits: 2,
        frac_digits: 2,
        national: [p, n],
        international: [p, n],
    }
}

fn run_mode(c: &Conventions<'_>, max: usize, fmt: &str, values: &[f64], mode: Rounding) -> Result<String, Errno> {
    let mut buf = vec![0u8; max];
    let mut a = Doubles { values: values.to_vec(), asked: Vec::new() };
    let n = monetary::format(&mut buf, fmt.as_bytes(), c, &mut a, mode)?;
    assert_eq!(buf.get(n), Some(&0), "{fmt}: terminated");
    Ok(String::from_utf8(buf[..n].to_vec()).unwrap())
}

fn run(c: &Conventions<'_>, fmt: &str, v: f64) -> String {
    run_mode(c, 128, fmt, &[v], Rounding::Nearest).unwrap()
}

#[test]
fn placement_of_symbol_and_sign() {
    // Clauses: s7_5_c_0 (D-7.11: cs_precedes, sep_by_space, sign_posn as observed).
    let base = place(1, 0, 1);
    let cases: [((u8, u8, u8), &str); 22] = [
        ((1, 0, 1), "-$1,234.50"),
        ((1, 1, 1), "-$ 1,234.50"),
        ((1, 2, 1), "- $1,234.50"),
        ((0, 0, 1), "-1,234.50$"),
        ((0, 1, 1), "-1,234.50 $"),
        ((0, 2, 1), "-1,234.50$"),
        ((1, 0, 2), "$1,234.50-"),
        ((1, 1, 2), "$ 1,234.50-"),
        ((1, 2, 2), "$1,234.50-"),
        ((0, 0, 2), "1,234.50$-"),
        ((0, 1, 2), "1,234.50 $-"),
        ((0, 2, 2), "1,234.50$ -"),
        ((1, 0, 3), "-$1,234.50"),
        ((1, 2, 3), "- $1,234.50"),
        ((0, 1, 3), "1,234.50 -$"),
        ((1, 0, 4), "$-1,234.50"),
        ((1, 1, 4), "$- 1,234.50"),
        ((1, 2, 4), "$ -1,234.50"),
        ((0, 0, 4), "1,234.50$-"),
        ((1, 0, 0), "($1,234.50)"),
        ((0, 1, 0), "(1,234.50 $)"),
        ((1, 0, 7), "-$1,234.50"),
    ];
    for ((cs, sep, posn), want) in cases {
        let c = conv(base, place(cs, sep, posn));
        assert_eq!(run(&c, "%n", -1234.5), want, "negative, cs_precedes {cs} sep_by_space {sep} sign_posn {posn}");
    }
    // The positive form with an empty sign: a space only between two non-empty parts.
    for (p, want) in [((1, 2, 1), "$1,234.50"), ((0, 2, 2), "1,234.50$"), ((1, 1, 1), "$ 1,234.50"), ((1, 0, 0), "$1,234.50"), ((1, 127, 1), "$1,234.50"), ((127, 0, 1), "$1,234.50")] {
        let c = conv(place(p.0, p.1, p.2), base);
        assert_eq!(run(&c, "%n", 1234.5), want, "positive {p:?}");
    }
    // sep_by_space 2: the empty sign still holds its place next to the symbol, so the space
    // between them is printed when text follows it (ja_JP's `JPY 1,235`; probe-settled), but
    // never as a leading or trailing space (D-7.11: spaces only between non-empty parts).
    for (p, want) in [((1, 2, 4), "$ 1,234.50"), ((0, 2, 3), "1,234.50 $"), ((1, 2, 3), "$1,234.50"), ((0, 2, 4), "1,234.50$"), ((1, 2, 2), "$1,234.50")] {
        let c = conv(place(p.0, p.1, p.2), base);
        assert_eq!(run(&c, "%n", 1234.5), want, "positive {p:?}");
        assert_eq!(run(&c, "%i", 1234.5), want.replace('$', "USD"), "positive %i {p:?}");
    }
    // With `!` the symbol's space goes with it.
    for (p, want) in [((1, 2, 4), "-1,234.50"), ((1, 2, 1), "-1,234.50"), ((0, 2, 2), "1,234.50-"), ((0, 2, 3), "1,234.50-")] {
        let c = conv(base, place(p.0, p.1, p.2));
        assert_eq!(run(&c, "%!n", -1234.5), want, "negative %!n {p:?}");
    }
    // `!` drops the symbol and the space that separated it.
    let c = conv(place(1, 1, 1), place(0, 1, 2));
    assert_eq!((run(&c, "%!n", 1234.5), run(&c, "%!n", -1234.5)), ("1,234.50".into(), "1,234.50-".into()));
    // `(` overrides sign_posn; `+` keeps the locale's.
    assert_eq!((run(&c, "%(n", -1234.5), run(&c, "%+n", -1234.5)), ("(1,234.50 $)".into(), "1,234.50 $-".into()));
    // An empty negative_sign prints `-`; positive_sign is printed.
    let mut c = conv(base, base);
    c.negative_sign = b"";
    c.positive_sign = b"+";
    assert_eq!((run(&c, "%n", -2.0), run(&c, "%n", 2.0)), ("-$2.00".into(), "+$2.00".into()));
}

#[test]
fn international_symbol() {
    // Clauses: s7_5_c_0 (the first three bytes of int_curr_symbol; the int_ placements).
    let mut c = conv(place(1, 0, 1), place(1, 0, 1));
    c.international = [place(1, 1, 1), place(0, 1, 2)];
    assert_eq!(run(&c, "%i", 5.0), "USD 5.00");
    assert_eq!(run(&c, "%i", -5.0), "5.00 USD-");
    assert_eq!(run(&c, "%n", -5.0), "-$5.00", "%n keeps the national placements");
    c.int_curr_symbol = b"";
    assert_eq!(run(&c, "%i", 5.0), "5.00", "no symbol: no space");
    c.int_curr_symbol = b"EUR";
    assert_eq!(run(&c, "%i", 5.0), "EUR 5.00");
    c.int_frac_digits = 3;
    assert_eq!(run(&c, "%i", 5.0), "EUR 5.000", "int_frac_digits");
}

#[test]
fn number_rounding_and_fraction() {
    // Clauses: s7_5_c_0 (the right precision, frac_digits, the rounding mode, the sign bit).
    let mut c = conv(place(1, 0, 1), place(1, 0, 1));
    assert_eq!(run(&c, "%.0n", 1234.5), "$1,234", "ties to even");
    assert_eq!(run(&c, "%.0n", 1235.5), "$1,236");
    assert_eq!(run(&c, "%.3n", 2.0), "$2.000");
    assert_eq!(run(&c, "%n", 0.005), "$0.01", "0.005 is a little above in binary");
    assert_eq!(run(&c, "%n", -0.0), "-$0.00", "D-7.11: the sign bit");
    assert_eq!(run(&c, "%n", -0.001), "-$0.00");
    assert_eq!(run(&c, "%n", 1e15), "$1,000,000,000,000,000.00");
    assert_eq!(run_mode(&c, 64, "%.0n", &[1.1], Rounding::Up).unwrap(), "$2");
    assert_eq!(run_mode(&c, 64, "%.0n", &[-1.1], Rounding::Down).unwrap(), "-$2");
    assert_eq!(run_mode(&c, 64, "%.0n", &[1.9], Rounding::Zero).unwrap(), "$1");
    c.frac_digits = 127;
    assert_eq!(run(&c, "%n", 3.0), "$3.00", "unspecified frac_digits: 2");
    c.frac_digits = 0xff;
    assert_eq!(run(&c, "%n", 3.0), "$3.00");
    c.frac_digits = 0;
    assert_eq!(run(&c, "%n", 3.5), "$4", "no fraction, no decimal point");
    c.frac_digits = 1;
    c.decimal_point = b"\xd9\xab";
    assert_eq!(run(&c, "%n", 3.25), "$3\u{66b}2", "a multibyte decimal point");
    c.decimal_point = b".";
    assert_eq!(run(&c, "%n", f64::INFINITY), "$inf");
    assert_eq!(run(&c, "%n", f64::NEG_INFINITY), "-$inf");
    assert_eq!(run(&c, "%n", f64::NAN), "$nan");
    assert_eq!(run(&c, "%n", -f64::NAN), "-$nan", "D-7.11: -NaN is negative");
}

#[test]
fn grouping_rules() {
    // Clauses: s7_5_c_0 (mon_grouping: sizes from the right, the last repeating, 127 stops; `^`).
    let mut c = conv(place(1, 0, 1), place(1, 0, 1));
    let g = |c: &Conventions<'_>| run(c, "%.0n", 1234567890.0);
    assert_eq!(g(&c), "$1,234,567,890");
    c.grouping = &[3, 2];
    assert_eq!(g(&c), "$1,23,45,67,890");
    c.grouping = &[3, 127];
    assert_eq!(g(&c), "$1234567,890");
    // 127 stops even when more than 127 digits remain (a size-127 group would add a second separator).
    let wide = format!("{:.0}", 1e140);
    let (head, tail) = wide.split_at(wide.len() - 3);
    assert_eq!(run_mode(&c, 400, "%.0n", &[1e140], Rounding::Nearest).unwrap(), format!("${head},{tail}"), "127 stops");
    c.grouping = &[2, 0xff];
    assert_eq!(g(&c), "$12345678,90");
    c.grouping = &[];
    assert_eq!(g(&c), "$1234567890");
    c.grouping = &[127];
    assert_eq!(g(&c), "$1234567890");
    c.grouping = &[1];
    assert_eq!(run(&c, "%.0n", 123.0), "$1,2,3");
    c.grouping = &[3];
    assert_eq!(run(&c, "%^.0n", 1234567890.0), "$1234567890", "`^`");
    assert_eq!(run(&c, "%.0n", 999.0), "$999", "no separator before the first group");
    c.thousands_sep = b"\xe2\x80\xaf";
    assert_eq!(run(&c, "%.0n", 1234567.0), "$1\u{202f}234\u{202f}567", "a multibyte separator");
    c.thousands_sep = b"";
    assert_eq!(run(&c, "%.0n", 1234567.0), "$1234567", "an empty separator");
}

#[test]
fn left_precision_and_alignment() {
    // Clauses: s7_5_c_0 (`#`: fill to lp digits' positions, a separator one position; the shorter
    // lead-in padded in front).
    let c = conv(place(1, 0, 1), place(1, 0, 1));
    assert_eq!(run(&c, "%#5n", 1234.5), " $ 1,234.50");
    assert_eq!(run(&c, "%#5n", -1234.5), "-$ 1,234.50");
    assert_eq!(run(&c, "%=*#6n", 12.0), " $*****12.00");
    assert_eq!(run(&c, "%=*^#6n", 12.0), " $****12.00", "no grouping: no separator positions");
    assert_eq!(run(&c, "%#2n", 12345.0), " $12,345.00", "longer than lp: not cut");
    assert_eq!(run(&c, "%(#5n", 1234.5), " $ 1,234.50", "the text after is not padded");
    assert_eq!(run(&c, "%(#5n", -1234.5), "($ 1,234.50)");
    assert_eq!(run(&c, "%!#5n", 1.0), "      1.00", "aligned on the printed text");
    assert_eq!(run(&c, "%#3n", f64::INFINITY), " $   inf");
    let s = conv(place(0, 1, 2), place(0, 1, 2));
    assert_eq!(run(&s, "%#4n", 1.0), "    1.00 $", "lead-ins equal: none padded");
    assert_eq!(run(&s, "%#4n", -1.0), "    1.00 $-");
    let mut g = conv(place(1, 0, 1), place(1, 0, 1));
    g.thousands_sep = b"\xe2\x80\xaf";
    assert_eq!(run(&g, "%#5n", 1.0), " $     1.00", "a multibyte separator: one fill position");
    g.thousands_sep = b"";
    // An empty separator still takes its positions in `#` padding (ruling #336, probe: the reference
    // prints `%#5n` of 0 with five fill positions under an empty separator and grouping 3).
    // The printed separators are empty and take no position: 9 + 2 reserved, 7 digits, 4 fill.
    assert_eq!(run(&g, "%#9n", 1234567.0), " $    1234567.00", "an empty separator: positions counted, text empty");
    assert_eq!(run(&g, "%#5n", 1.0), " $     1.00", "an empty separator: one fill position");
}

#[test]
fn field_width_and_text() {
    // Clauses: s7_5_c_0 (width in bytes, `-`, literal text, `%%`, flags repeatable).
    let c = conv(place(1, 0, 1), place(1, 0, 1));
    assert_eq!(run(&c, "%12n|", -1.5), "      -$1.50|");
    assert_eq!(run(&c, "%-12n|", -1.5), "-$1.50      |");
    assert_eq!(run(&c, "%3n", 1234.5), "$1,234.50", "a narrower width does not cut");
    assert_eq!(run(&c, "%=x-12n|", 1.5), "$1.50       |", "width pads with spaces, not the fill");
    assert_eq!(run(&c, "a %% b %n c", 1.0), "a % b $1.00 c");
    assert_eq!(run(&c, "%^^!!n", 1234.0), "1234.00", "repeated flags");
    assert_eq!(run(&c, "%=a=b#3n", 1.0), " $bb1.00", "the last fill wins");
    assert_eq!(run_mode(&c, 64, "%099999999999999999999n", &[1.0], Rounding::Nearest), Err(Errno::E2BIG), "a huge width saturates");
}

#[test]
fn malformed_and_overflow() {
    // Clauses: s7_5_c_0, s7_5_f_0 (EINVAL for a malformed conversion, E2BIG when the result
    // and its terminator do not fit, errors in format order).
    let c = conv(place(1, 0, 1), place(1, 0, 1));
    let r = |max: usize, fmt: &str| run_mode(&c, max, fmt, &[1.0, 2.0], Rounding::Nearest);
    for bad in ["%", "%q", "%#n", "%.n", "%=", "%+(n", "%(+n", "%++n", "%((n", "%L", "%Lq", "%5", "%5.2", "%#3.", "%Ld", "%-", "%^"] {
        assert_eq!(r(64, bad), Err(Errno::EINVAL), "{bad:?}");
    }
    assert_eq!(r(64, "%n %q"), Err(Errno::EINVAL));
    assert_eq!(r(4, "abc"), Ok("abc".into()));
    assert_eq!(r(3, "abc"), Err(Errno::E2BIG));
    assert_eq!(r(0, ""), Err(Errno::E2BIG), "no room for the terminator");
    assert_eq!(r(1, ""), Ok(String::new()));
    assert_eq!(r(2, "abc%q"), Err(Errno::E2BIG), "the text overflows before the bad conversion");
    assert_eq!(r(4, "abc%q"), Err(Errno::EINVAL));
    assert_eq!(r(0, "%q"), Err(Errno::EINVAL));
    assert_eq!(r(6, "%n"), Ok("$1.00".into()));
    assert_eq!(r(5, "%n"), Err(Errno::E2BIG));
    assert_eq!(r(12, "%12n"), Err(Errno::E2BIG), "the width counts");
}

#[test]
fn long_double_amounts() {
    // Clauses: s7_5_c_0 (`L`: the amount is a long double).
    // ARCH(x86_64): amounts in the x87 80-bit long double; binary128 on aarch64 (A-10)
    struct X87(Vec<bool>);
    impl Amounts for X87 {
        fn amount(&mut self, long_double: bool) -> Float {
            self.0.push(long_double);
            if long_double {
                // 1.5 * 2^1100: beyond any double.
                Float::unpack((0x3fffu128 + 1100) << 64 | 0xc000_0000_0000_0000, Format::F80)
            } else {
                Float::unpack(u128::from(1.5f64.to_bits()), Format::F64)
            }
        }
    }
    let c = conv(place(1, 0, 1), place(1, 0, 1));
    let mut buf = vec![0u8; 2048];
    // ARCH(x86_64): amounts in the x87 80-bit long double; binary128 on aarch64 (A-10)
    let mut a = X87(Vec::new());
    let n = monetary::format(&mut buf, b"%^.0Ln|%n|%Li", &c, &mut a, Rounding::Nearest).unwrap();
    assert_eq!(a.0, [true, false, true], "each conversion reads its own type");
    let s = String::from_utf8(buf[..n].to_vec()).unwrap();
    let parts: Vec<&str> = s.split('|').collect();
    assert_eq!(parts[1], "$1.50");
    assert!(parts[0].starts_with("$2") && parts[0].len() == 1 + 332, "1.5 * 2^1100 has 332 digits: {}", parts[0]);
    assert!(parts[2].starts_with("USD2") && parts[2].ends_with(".00"), "{}", parts[2]);
}
