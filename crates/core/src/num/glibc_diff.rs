//! Differential tests of number -> text against the host C library, black box (COLD-REWRITE-W-far-05).
//!
//! The test hosts run glibc 2.39, the reference. Each value goes through [`format_float`] here and
//! through the host's `snprintf` (`double`), `strfromf` (`float`) or `strfroml` (x87 `long double`)
//! in `tools/probe/printf_oracle.py` (its own process, so the calls reach the host library, never
//! Liella's exports), in the same rounding mode, and the bytes must be identical. The values aim at
//! the hard cases: exact ties at a digit position, neighbours of decimal rounding boundaries, powers
//! of ten, subnormals, the extremes, the `%g` style switch and hex rounding carries. `binary128` has
//! no host path here (no `_Float128` argument type in the oracle); it shares every routine with the
//! other formats and `tests/float_engine.rs` checks it.
//!
//! `differential_quick` runs in the suite; `differential_millions` (ignored) is the full run:
//! `cargo test --release -p liella --lib glibc_diff -- --ignored --nocapture`, size from
//! `LIELLA_GLIBC_DIFF_N` (default 2,000,000 `double` values plus a tenth as many `float` and x87).
//! A host without `python3` fails these tests (it never skips them).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_possible_truncation)]

extern crate std;

use std::format;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::string::String;
use std::vec::Vec;

use super::float::{Float, Format, Rounding, Value};
use super::format::{exact, format_float, round_digits, Bounded, Case, Sink, Spec, Style};

const MODES: [(Rounding, i32); 4] = [(Rounding::Nearest, 0), (Rounding::Down, 0x400), (Rounding::Up, 0x800), (Rounding::Zero, 0xc00)];

struct S(Vec<u8>);
impl Sink for S {
    fn put(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    F32,
    F64,
    F80,
}

/// One comparison: a value, a conversion and a mode; `ecvt` compares `round_digits(exact(x), n)`
/// against the host's `%.<n-1>e` instead of `format_float`.
struct Case1 {
    kind: Kind,
    bits: u128,
    spec: Spec,
    mode: usize,
    ecvt: bool,
}

fn conv_text(spec: Spec) -> String {
    let c = match spec.style {
        Style::Exp => 'e',
        Style::Fixed => 'f',
        Style::General => 'g',
        Style::Hex => 'a',
    };
    let c = if spec.case == Case::Upper { c.to_ascii_uppercase() } else { c };
    let alt = if spec.alternate { "#" } else { "" };
    match spec.precision {
        Some(p) => format!("%{alt}.{p}{c}"),
        None => format!("%{alt}{c}"),
    }
}

/// The host's texts for all cases, in order.
fn host_all(cases: &[Case1]) -> Vec<String> {
    let script = format!("{}/../../tools/probe/printf_oracle.py", env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new("python3")
        .arg("-I")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 runs the host-library oracle (tools/probe/printf_oracle.py)");
    let mut input = Vec::new();
    for c in cases {
        let k = match c.kind {
            Kind::F32 => 'f',
            Kind::F64 => 'd',
            Kind::F80 => 'l',
        };
        input.extend_from_slice(format!("{k} {} {} {:x}\n", MODES[c.mode].1, conv_text(c.spec), c.bits).as_bytes());
    }
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let lines: Vec<String> = BufReader::new(child.stdout.take().unwrap()).lines().map(Result::unwrap).collect();
    writer.join().unwrap();
    assert!(child.wait().unwrap().success(), "the oracle failed");
    assert_eq!(lines.len(), cases.len(), "the oracle answered every record");
    lines
}

fn ours(kind: Kind, bits: u128, spec: Spec, mode: Rounding) -> String {
    let fmt = match kind {
        Kind::F32 => Format::F32,
        Kind::F64 => Format::F64,
        Kind::F80 => Format::F80,
    };
    let mut s = S(Vec::new());
    format_float(&mut s, Float::unpack(bits, fmt), fmt, spec, b".", mode);
    String::from_utf8(s.0).unwrap()
}

/// `round_digits(exact(x), n)` as `%.<n-1>e` would print its digits: `d.ddd` and the exponent.
fn ecvt_text(bits: u128, n: usize, mode: Rounding) -> String {
    let x = Float::unpack(bits, Format::F64);
    let Value::Finite { mant, exp } = x.value else { panic!("finite values only") };
    let d = round_digits(&exact(mant, exp), n as i64, x.neg, mode);
    let mut s = String::new();
    if x.neg {
        s.push('-');
    }
    s.push(char::from(b'0' + d.digit(0)));
    if n > 1 {
        s.push('.');
    }
    for k in 1..n {
        s.push(char::from(b'0' + d.digit(k)));
    }
    let e = d.exp10 - 1;
    s.push_str(&format!("e{}{:02}", if e < 0 { '-' } else { '+' }, e.unsigned_abs()));
    s
}

fn compare(cases: &[Case1]) {
    let want = host_all(cases);
    let mut bad = Vec::new();
    for (c, w) in cases.iter().zip(&want) {
        let mode = MODES[c.mode].0;
        let got = if c.ecvt { ecvt_text(c.bits, c.spec.precision.unwrap() + 1, mode) } else { ours(c.kind, c.bits, c.spec, mode) };
        // Liella keeps C's `%#g` after a rounding carry (`%#.2g` of 99.5 is `1.0e+02`, P-1 digits
        // kept); the host prints `1.e+02`. Only that shape is excused (owner ruling 2026-10-22: keep C; COMPAT-8, D-8.11 (3)).
        let hash_g_carry = c.spec.style == Style::General && c.spec.alternate && (w.contains(".e") || w.contains(".E")) && {
            let (m, e) = got.split_once(['e', 'E']).unwrap_or((&got, ""));
            format!("{}.{}{e}", m.split('.').next().unwrap_or(""), if c.spec.case == Case::Upper { "E" } else { "e" }) == *w && m.split('.').nth(1).is_some_and(|f| f.bytes().all(|b| b == b'0'))
        };
        if &got != w && !hash_g_carry {
            let k = match c.kind {
                Kind::F32 => "float",
                Kind::F64 => "double",
                Kind::F80 => "x87",
            };
            bad.push(format!("{}{} of {:#x} ({k}) in {mode:?}: ours {got:?}, host {w:?}", if c.ecvt { "ecvt " } else { "" }, conv_text(c.spec), c.bits));
        }
    }
    assert!(bad.is_empty(), "{} of {} differ, first: {:#?}", bad.len(), cases.len(), &bad[..bad.len().min(12)]);
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Powers of ten as `double` bits, nearest (Rust's parser is correctly rounded).
fn pow10_bits(k: i32) -> u64 {
    format!("1e{k}").parse::<f64>().unwrap().to_bits()
}

/// A `double` aimed at a hard case (`kind` picks which).
fn value_f64(r: &mut Rng, kind: u64) -> u64 {
    match kind % 9 {
        0 => r.next(),
        // Subnormals and the smallest normals.
        1 => r.next() & ((1 << 53) - 1),
        // Exact ties: few significant bits, so the decimal expansion is short and ends in 5.
        2 => {
            let m = (r.below(1 << 20) | 1) as f64;
            let e = r.below(60) as i32 - 30;
            (m * 2f64.powi(e)).to_bits()
        }
        // Powers of ten and their neighbours.
        3 => pow10_bits(r.below(631) as i32 - 323).wrapping_add(r.below(5)).wrapping_sub(2),
        // Neighbours of decimal rounding boundaries: d.ddd5 x 10^k and d.ddd9 x 10^k.
        4 | 5 => {
            let digits = 1 + r.below(17) as usize;
            let mut s = String::new();
            for i in 0..digits {
                s.push(char::from(b'0' + if i == 0 { 1 + r.below(9) as u8 } else { r.below(10) as u8 }));
            }
            s.push(if kind % 9 == 4 { '5' } else { '9' });
            let k = r.below(600) as i32 - 300;
            let x: f64 = format!("{s}e{k}").parse().unwrap();
            x.to_bits().wrapping_add(r.below(3)).wrapping_sub(1)
        }
        // The %g fixed/exponent switch around 1e-5..1e-3 and 1e14..1e21.
        6 => pow10_bits(if r.below(2) == 0 { r.below(4) as i32 - 6 } else { 14 + r.below(8) as i32 }).wrapping_sub(r.below(4)),
        // Integers (exact, any length) and halves of them.
        7 => ((r.next() >> r.below(64)) as f64 / if r.below(2) == 0 { 1.0 } else { 2.0 }).to_bits(),
        // The extremes and the specials.
        _ => [0, 1 << 63, 1, (1 << 52) - 1, 1 << 52, 0x7fef_ffff_ffff_ffff, 0x7ff0_0000_0000_0000, 0xfff0_0000_0000_0000, 0x7ff8_0000_0000_0000, 0xfff8_0000_0000_0001, 0x3ff0_0000_0000_0000][r.below(11) as usize],
    }
}

/// An x87 value of a canonical encoding (the non-canonical ones are a documented difference).
fn value_f80(r: &mut Rng) -> u128 {
    let sign = u128::from(r.below(2)) << 79;
    let (exp, mant): (u64, u64) = match r.below(6) {
        0 => (0, r.next() >> 1 >> r.below(63)),
        1 => (0x7fff, 0x8000_0000_0000_0000 | ((r.next() >> 2) * r.below(2))),
        2 => (0x3fff - 70 + r.below(140), (1 << 63) | ((r.next() >> 40) << 20)),
        3 => (0x7ffe - r.below(3), (1 << 63) | r.next()),
        _ => (1 + r.below(0x7ffe), (1 << 63) | r.next()),
    };
    sign | (u128::from(exp) << 64) | u128::from(mant)
}

fn value_f32(r: &mut Rng) -> u128 {
    u128::from(match r.below(3) {
        0 => r.next() as u32 & 0x807f_ffff,
        1 => (r.next() as u32 & 0x803f_ffff) | 0x3f00_0000,
        _ => r.next() as u32,
    })
}

/// One random conversion: any style and case, `#` when `flags` (the `strfrom` forms take none),
/// precisions weighted to the small ones with some long ones.
fn spec(r: &mut Rng, flags: bool) -> Spec {
    let style = [Style::Exp, Style::Fixed, Style::General, Style::Hex][r.below(4) as usize];
    let precision = match r.below(12) {
        0 => None,
        1 => Some([50, 100, 330, 760, 1100, 1200][r.below(6) as usize]),
        2 => Some(r.below(41) as usize),
        _ => Some(r.below(21) as usize),
    };
    Spec { style, case: if r.below(4) == 0 { Case::Upper } else { Case::Lower }, precision, alternate: flags && r.below(3) == 0 }
}

fn cases(n_f64: u64, n_wide: u64, seed: u64) -> Vec<Case1> {
    let mut r = Rng(seed);
    let mut v = Vec::new();
    for i in 0..n_f64 {
        let bits = u128::from(value_f64(&mut r, i));
        let sp = spec(&mut r, true);
        let mode = r.below(4) as usize;
        v.push(Case1 { kind: Kind::F64, bits, spec: sp, mode, ecvt: false });
        // Every eighth value also through `round_digits(exact(x), n)` (the `ecvt` digits).
        let x = f64::from_bits(bits as u64);
        if i % 8 == 0 && x.is_finite() && x != 0.0 {
            let n = 1 + if r.below(4) == 0 { r.below(800) } else { r.below(20) } as usize;
            let spec = Spec { style: Style::Exp, case: Case::Lower, precision: Some(n - 1), alternate: false };
            v.push(Case1 { kind: Kind::F64, bits, spec, mode: r.below(4) as usize, ecvt: true });
        }
    }
    for _ in 0..n_wide {
        let (a, sa, ma) = (value_f80(&mut r), spec(&mut r, false), r.below(4) as usize);
        v.push(Case1 { kind: Kind::F80, bits: a, spec: sa, mode: ma, ecvt: false });
        let (b, sb, mb) = (value_f32(&mut r), spec(&mut r, false), r.below(4) as usize);
        v.push(Case1 { kind: Kind::F32, bits: b, spec: sb, mode: mb, ecvt: false });
    }
    v
}

#[test]
fn differential_quick() {
    // The host pins the conventions these rows rely on (and that the oracle reaches it).
    let pins = [
        Case1 { kind: Kind::F80, bits: 0x3fff_8000_0000_0000_0000, spec: Spec { style: Style::Hex, case: Case::Lower, precision: None, alternate: false }, mode: 0, ecvt: false },
        Case1 { kind: Kind::F32, bits: 1, spec: Spec { style: Style::Hex, case: Case::Lower, precision: None, alternate: false }, mode: 0, ecvt: false },
    ];
    assert_eq!(host_all(&pins), ["0x8p-3", "0x1p-149"]);
    compare(&cases(20_000, 4_000, 0x9E37_79B9_7F4A_7C15));
}

#[test]
fn every_style_and_mode_on_fixed_hard_values() {
    // Ties at every position, carries into a new decade or hex digit, the %g switch, subnormals.
    let xs: [f64; 20] = [0.5, 1.5, 2.5, 0.125, 0.375, 9.5, 99.5, 999.5, 0.0001, 0.00009999995, 9.9999995e-5, 123456.5, 1e21, 1e22, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, 1.96875, 1.03125, 1.09375];
    let mut v = Vec::new();
    for x in xs {
        for neg in [0u64, 1 << 63] {
            for style in [Style::Exp, Style::Fixed, Style::General, Style::Hex] {
                for p in (0..=20).map(Some).chain([None, Some(30), Some(1074)]) {
                    for alternate in [false, true] {
                        for case in [Case::Lower, Case::Upper] {
                            for mode in 0..4 {
                                v.push(Case1 { kind: Kind::F64, bits: u128::from(x.to_bits() | neg), spec: Spec { style, case, precision: p, alternate }, mode, ecvt: false });
                            }
                        }
                    }
                }
            }
        }
    }
    compare(&v);
}

#[test]
fn bounded_counts_the_full_length() {
    let mut buf = [0xffu8; 4];
    let mut b = Bounded::new(&mut buf);
    let x = Float::unpack(u128::from(1234.5f64.to_bits()), Format::F64);
    format_float(&mut b, x, Format::F64, Spec { style: Style::Fixed, case: Case::Lower, precision: Some(2), alternate: false }, b".", Rounding::Nearest);
    b.put(b"xyz");
    b.repeat(b'0', 5);
    assert_eq!((b.stored(), b.len), (4, 15));
    assert_eq!(&buf, b"1234");
}

#[test]
#[ignore = "minutes in release; the full differential run (LIELLA_GLIBC_DIFF_N)"]
fn differential_millions() {
    let n = std::env::var("LIELLA_GLIBC_DIFF_N").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000_000u64);
    // In slices, so the oracle's output for one slice stays in memory.
    for (k, seed) in (0..n.div_ceil(200_000)).zip([0x2545_F491_4F6C_DD1Du64, 0x9E37_79B9_7F4A_7C15].iter().cycle()) {
        compare(&cases(200_000, 20_000, seed ^ (k * 0x1000_0001)));
    }
    std::println!("glibc_diff: {n} double values (an eighth also as ecvt digits) and {} each of float and x87 identical to the host", n / 10);
}
