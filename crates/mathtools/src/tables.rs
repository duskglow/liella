//! The constant tables of `liella-math`'s elementary functions (D-10.3), computed with [`mp`]
//! and written as Rust source, one file per family in `crates/math/src/coeffs/` (re-exported as
//! `coeffs::tables`).
//!
//! Every value is a mathematical constant evaluated far beyond the precision stored (`WORK`
//! bits, correctly rounded at the end): powers `2^(j/64)`, logarithms of the reduction
//! constants, `sin(kπ/64)`, `atan(j/64)`, `π` and `ln 2` and their splits for Cody–Waite
//! reduction, the bits of `2/π` for Payne–Hanek reduction, and the rational series
//! coefficients `1/n!` and `1/(2k+1)`. Double-double values (`dd`) are stored as two `f64` bit
//! patterns: the nearest `f64` to the value, then the nearest `f64` to the remainder (together
//! about 107 bits). The output is deterministic, so `--check` regenerates it and compares.

use std::fmt::Write as _;

use crate::mp::{self, Big, Fmt, Round};

/// Working precision of every constant (bits).
const WORK: u32 = 400;

pub(crate) fn f64_of(x: &Big) -> f64 {
    let (b, _) = x.round_to(Fmt::F64, Round::Nearest);
    f64::from_bits(b as u64)
}

/// The double-double nearest `x`: (hi, lo) bit patterns.
pub(crate) fn dd(x: &Big) -> (u64, u64) {
    let hi = f64_of(x);
    let lo = f64_of(&x.sub(&Big::from_f64(hi)));
    (hi.to_bits(), lo.to_bits())
}

/// `x` rounded to `bits` significant bits (to nearest), as an `f64`.
fn short(x: &Big, bits: u32) -> f64 {
    f64_of(&x.round(bits, Round::Nearest))
}

/// A Cody–Waite split of `c` into parts of `bits` significant bits each (the last part full),
/// so that `n * part` is exact for `|n| < 2^(53 - bits)`.
fn split(c: &Big, bits: u32, parts: usize) -> Vec<f64> {
    let mut rest = c.clone();
    let mut out = Vec::new();
    for i in 0..parts {
        let p = if i + 1 == parts { f64_of(&rest) } else { short(&rest, bits) };
        rest = rest.sub(&Big::from_f64(p));
        out.push(p);
    }
    out
}

fn approx(a: mp::Approx) -> Big {
    a.value
}

/// `v` rounded to the nearest multiple of `2^-42` (an `f64` of at most 42 significant bits when
/// `|v| < 1`): `e * part + hi` is then exact for `|e| < 2^11` and `|hi| < 1`.
fn on_grid42(v: &Big) -> f64 {
    let s = 4_398_046_511_104.0; // 2^42
    (f64_of(v) * s).round() / s
}

/// The quick `log` phase's table and constant (D-10.60; Tang, ACM TOMS 16(4), 1990, with the
/// reciprocals of few bits that make the reduction exact, Gal & Bachelis, ACM TOMS 17(1), 1991).
/// `m` in `[45/64, 90/64)`, 128 intervals `[F_j, F_j + w_j)`: `F_j = (180 + j)/256`, `w_j = 2^-8`
/// for `j < 76`, `F_j = 1 + (j − 76)/128`, `w_j = 2^-7` for `j >= 76`. Row `j`: `c_j` (8
/// significant bits, the one of the two nearest `2/(2F_j + w_j)` with the smaller `max |c·m − 1|`
/// over the interval; `c_j = 1` for the two intervals next to 1), `b_j = c_j·F_j − 1` (exact),
/// `-ln c_j = hi + lo` with `hi` a multiple of `2^-42` and `lo` the nearest `f64` to the rest.
fn log_quick(o: &mut Out, ln2: &Big) {
    let p = WORK;
    let mut rows = Vec::new();
    for j in 0..128i32 {
        let (f, w) = if j < 76 { (f64::from(180 + j) / 256.0, 1.0 / 256.0) } else { (1.0 + f64::from(j - 76) / 128.0, 1.0 / 128.0) };
        let c = if j == 75 || j == 76 {
            1.0
        } else {
            let ideal = 2.0 / (2.0 * f + w);
            let g = if ideal < 1.0 { 1.0 / 256.0 } else { 1.0 / 128.0 };
            let lo = (ideal / g).floor() * g;
            let worst = |c: f64| f64::max((c * f - 1.0).abs(), (c * (f + w) - 1.0).abs());
            if worst(lo) <= worst(lo + g) { lo } else { lo + g }
        };
        assert!((c * f - 1.0).abs() < 1.0 / 128.0 && (c * (f + w) - 1.0).abs() <= 1.0 / 128.0, "LOG_Q128[{j}]: |r| reaches 2^-7");
        let l = approx(mp::log(&Big::from_f64(c), p)).neg();
        let hi = on_grid42(&l);
        let lo = f64_of(&l.sub(&Big::from_f64(hi)));
        rows.push([c.to_bits(), (c * f - 1.0).to_bits(), hi.to_bits(), lo.to_bits()]);
    }
    let _ = writeln!(
        o.src,
        "\n/// The quick `log` phase's rows `[c_j, b_j = c_j·F_j − 1, hi, lo]` with `-ln c_j = hi + lo`, `hi` a multiple of 2^-42 (D-10.60;\n/// intervals `[F_j, F_j + w_j)` of `m` in `[45/64, 90/64)`, `c_j` of 8 significant bits; derivation at `tables.rs::log_quick`).\npub(crate) static LOG_Q128: [[u64; 4]; 128] = ["
    );
    for r in &rows {
        let _ = writeln!(o.src, "    [{:#018x}, {:#018x}, {:#018x}, {:#018x}], // c = {}", r[0], r[1], r[2], r[3], f64::from_bits(r[0]));
    }
    o.src.push_str("];\n");
    let hi = on_grid42(ln2);
    let lo = f64_of(&ln2.sub(&Big::from_f64(hi)));
    o.f64_table("LN2_Q", "`ln 2` for the quick `log` phase: a multiple of 2^-42 (`e * part` exact for |e| < 2^11) and the nearest `f64` to the rest.", &[hi, lo]);
}

struct Out {
    src: String,
    files: Vec<(&'static str, String)>,
}

impl Out {
    /// Ends one family's file: `file` (a name in `crates/math/src/coeffs/`) is the header and
    /// the tables written since the previous file.
    fn close(&mut self, file: &'static str, doc: &str) {
        let body = std::mem::take(&mut self.src);
        let mut src = String::new();
        src.push_str("// @generated by `cargo xtask tables` (crates/mathtools/src/tables.rs). Do not edit.\n");
        let _ = writeln!(src, "//! {doc} (D-10.3): one family's file of the constant tables, all re-exported as");
        src.push_str("//! `coeffs::tables`. Each value was computed by the project's arbitrary-precision arithmetic\n");
        src.push_str("//! (`liella-mathtools::mp`) at 400 bits or more and rounded to nearest once: a double-double is\n");
        src.push_str("//! the nearest `f64` and the nearest `f64` to the remainder. The derivation of each table is in\n");
        src.push_str("//! its comment.\n");
        src.push_str("#![allow(clippy::unreadable_literal, clippy::excessive_precision)]\n");
        src.push_str(&body);
        self.files.push((file, src));
    }

    fn dd_table(&mut self, name: &str, doc: &str, values: &[Big]) {
        let _ = writeln!(self.src, "\n/// {doc}\npub(crate) static {name}: [[u64; 2]; {}] = [", values.len());
        for v in values {
            let (h, l) = dd(v);
            let _ = writeln!(self.src, "    [{h:#018x}, {l:#018x}], // {}", f64::from_bits(h));
        }
        self.src.push_str("];\n");
    }

    fn dd_const(&mut self, name: &str, doc: &str, v: &Big) {
        let (h, l) = dd(v);
        let _ = writeln!(self.src, "\n/// {doc}\npub(crate) const {name}: [u64; 2] = [{h:#018x}, {l:#018x}]; // {}", f64::from_bits(h));
    }

    fn f64_table(&mut self, name: &str, doc: &str, values: &[f64]) {
        let _ = writeln!(self.src, "\n/// {doc}\npub(crate) const {name}: [u64; {}] = [", values.len());
        for v in values {
            let _ = writeln!(self.src, "    {:#018x}, // {v:e}", v.to_bits());
        }
        self.src.push_str("];\n");
    }
}

/// The bits of `x` (in [0, 1)) after the binary point, `words` 64-bit words, truncated.
fn fraction_words(x: &Big, words: usize) -> Vec<u64> {
    let mut frac = x.clone();
    let mut out = Vec::with_capacity(words);
    for _ in 0..words {
        let mut w: u64 = 0;
        for _ in 0..2 {
            frac = frac.mul_pow2(32);
            let int = frac.floor();
            let v = int.to_i64().unwrap_or(0) as u64;
            frac = frac.sub(&int);
            w = (w << 32) | v;
        }
        out.push(w);
    }
    out
}

/// Words of `2/π` stored: enough for Payne–Hanek reduction of every finite binary128 value
/// (exponents up to 16383, plus the fraction bits the reduction keeps): the 384-bit reduction
/// (`mp::reduce_wide`, 9 words from bit 16270) reads up to word 263, the 768-bit one
/// (`mpw::reduce_wide`, 15 words) up to word 269.
pub const TWO_OVER_PI_WORDS: usize = 312;

/// The generated files, one per family: each file's name in `crates/math/src/coeffs/` and its
/// source. `crates/math/src/coeffs.rs` (hand-written) declares them and re-exports them all as
/// `coeffs::tables`. A family's tables change only its own file.
pub fn generate() -> Vec<(&'static str, String)> {
    let p = WORK;
    let pi = mp::pi(p + 17_000);
    let ln2 = mp::ln2(p);
    let ln10 = mp::ln10(p);
    let mut o = Out { src: String::new(), files: Vec::new() };

    // exp: 2^(j/64).
    let t: Vec<Big> = (0..64).map(|j| approx(mp::exp2(&Big::from_i64(j).mul_pow2(-6), p))).collect();
    o.dd_table("EXP2_J64", "`2^(j/64)` for j = 0..63 (mp::exp2).", &t);
    let t: Vec<Big> = (-32..=32).map(|j| approx(mp::exp2(&Big::from_i64(j).mul_pow2(-6), p)).sub(&Big::one())).collect();
    o.dd_table("EXP2M1_J64", "`2^(j/64) - 1` for j = -32..32 (index j + 32; mp::exp2 minus 1, exact).", &t);
    o.dd_const("LN2", "`ln 2` (mp::ln2).", &ln2);
    o.dd_const("INV_LN2", "`1/ln 2`.", &Big::one().div(&ln2, p, Round::Nearest));
    o.dd_const("LN10", "`ln 10` (mp::ln10).", &ln10);
    o.dd_const("INV_LN10", "`1/ln 10`.", &Big::one().div(&ln10, p, Round::Nearest));
    let log10_2 = ln2.div(&ln10, p, Round::Nearest);
    o.dd_const("LOG10_2", "`log10(2)` = ln 2 / ln 10.", &log10_2);
    o.dd_const("LOG2_10", "`log2(10)` = ln 10 / ln 2.", &ln10.div(&ln2, p, Round::Nearest));
    o.f64_table("LN2_64_CW", "`ln 2 / 64` split for Cody–Waite reduction: three parts of 36 bits and a full fourth part, so `k * part` is exact for |k| < 2^17.", &split(&ln2.mul_pow2(-6), 36, 4));
    o.f64_table("LOG10_2_64_CW", "`log10(2) / 64` split as `LN2_64_CW`.", &split(&log10_2.mul_pow2(-6), 36, 4));
    o.f64_table("SCALES", "`64 / ln 2`, `64 * log2(10)` and `64 / π` rounded to `f64` (only to choose a reduction index).", &[
        f64_of(&Big::from_i64(64).div(&ln2, p, Round::Nearest)),
        f64_of(&ln10.div(&ln2, p, Round::Nearest).mul_pow2(6)),
        f64_of(&Big::from_i64(64).div(&pi, p, Round::Nearest)),
    ]);
    // The quick exp phase (D-10.51; Tang's table with N = 128): 2^(j/128), ln2/128 in two parts, 128/ln2.
    let t: Vec<Big> = (0..128).map(|j| approx(mp::exp2(&Big::from_i64(j).mul_pow2(-7), p))).collect();
    o.dd_table("EXP2_J128", "`2^(j/128)` for j = 0..127 (mp::exp2): the quick `exp` phase's table (D-10.51).", &t);
    o.f64_table("LN2_128_CW", "`ln 2 / 128` split for the quick `exp` phase: a part of 36 bits (`k * part` exact for |k| < 2^17) and the nearest `f64` to the rest.", &split(&ln2.mul_pow2(-7), 36, 2));
    o.f64_table("SCALE_128", "`128 / ln 2` rounded to `f64` (only to choose the quick `exp` phase's reduction index).", &[f64_of(&Big::from_i64(128).div(&ln2, p, Round::Nearest))]);

    // log: c_i = 1/(1 + i/128) rounded to 12 bits (c_0 = 1), L_i = -log(c_i).
    let mut c = Vec::new();
    let mut l = Vec::new();
    for i in -37..=53i64 {
        let ci = if i == 0 { 1.0 } else { short(&Big::from_i64(128).div(&Big::from_i64(128 + i), p, Round::Nearest), 12) };
        c.push(ci);
        l.push(approx(mp::log(&Big::from_f64(ci), p)).neg());
    }
    o.f64_table("LOG_C", "`c_i` = 1/(1 + i/128) rounded to 12 significant bits, for i = -37..53 (index i + 37); `c_0` = 1.", &c);
    o.dd_table("LOG_L", "`-ln(c_i)` for the `c_i` of `LOG_C` (mp::log; `-ln(c_0)` = 0).", &l);
    // log2/log10: the same table pre-scaled by 1/ln b (register 16.5 #7), and log10(2) split for e·log10(2).
    let l2: Vec<Big> = l.iter().map(|v| v.div(&ln2, p, Round::Nearest)).collect();
    o.dd_table("LOG2_L", "`-log2(c_i)` = `-ln(c_i)/ln 2` for the `c_i` of `LOG_C` (`-log2(c_0)` = 0).", &l2);
    let l10: Vec<Big> = l.iter().map(|v| v.div(&ln10, p, Round::Nearest)).collect();
    o.dd_table("LOG10_L", "`-log10(c_i)` = `-ln(c_i)/ln 10` for the `c_i` of `LOG_C` (`-log10(c_0)` = 0).", &l10);
    o.f64_table("LOG10_2_E", "`log10(2)` split for a logarithm's exponent: 42 significant bits (`e * part` exact for |e| < 2^11) and the nearest `f64` to the rest.", &split(&log10_2, 42, 2));
    log_quick(&mut o, &ln2);
    o.close("exp_log.rs", "Tables and constants of the exponentials and logarithms");

    // Trigonometric: sin(kπ/64), k = 0..127, and π/64 splits.
    // The multiples of π/2 are exact (0, ±1); the rest from mp::sin of kπ/64 at the working precision.
    let t: Vec<Big> = (0..128i64)
        .map(|k| match k {
            0 | 64 => Big::zero(false),
            32 => Big::one(),
            96 => Big::from_i64(-1),
            _ => approx(mp::sin(&pi.mul(&Big::from_i64(k)).mul_pow2(-6).round(p, Round::Nearest), p)),
        })
        .collect();
    o.dd_table("SIN_K_PI64", "`sin(kπ/64)` for k = 0..127 (`cos(kπ/64)` is entry k + 32 mod 128 with the sign of the half).", &t);
    o.f64_table("PI64_CW", "`π/64` split for Cody–Waite reduction: three parts of 38 bits and a full fourth part, so `n * part` is exact for |n| < 2^15.", &split(&pi.mul_pow2(-6), 38, 4));
    o.dd_const("PI64", "`π/64`.", &pi.mul_pow2(-6));
    o.dd_const("PI", "`π` (mp::pi).", &pi);
    o.dd_const("PI_2", "`π/2`.", &pi.mul_pow2(-1));
    o.dd_const("PI_4", "`π/4`.", &pi.mul_pow2(-2));
    let pi_bits = 64 * TWO_OVER_PI_WORDS as u32 + 512;
    let two_over_pi = Big::from_i64(2).div(&mp::pi(pi_bits), 64 * TWO_OVER_PI_WORDS as u32 + 128, Round::Zero);
    let words = fraction_words(&two_over_pi, TWO_OVER_PI_WORDS);
    let _ = writeln!(o.src, "\n/// The bits of `2/π` after the binary point (2/π < 1), most significant first: {TWO_OVER_PI_WORDS} words, truncated (mp::pi at {pi_bits} bits).\npub(crate) static TWO_OVER_PI: [u64; {TWO_OVER_PI_WORDS}] = [");
    for chunk in words.chunks(4) {
        let s: Vec<String> = chunk.iter().map(|w| format!("{w:#018x}")).collect();
        let _ = writeln!(o.src, "    {},", s.join(", "));
    }
    o.src.push_str("];\n");

    // atan(j/64).
    let t: Vec<Big> = (0..=64).map(|j| approx(mp::atan(&Big::from_i64(j).mul_pow2(-6), p))).collect();
    o.dd_table("ATAN_J64", "`atan(j/64)` for j = 0..64 (mp::atan).", &t);
    o.close("trig.rs", "Tables and constants of the trigonometric functions and their inverses");

    // Series coefficients.
    let mut fact = Big::one();
    let mut inv_fact = Vec::new();
    for n in 0..32i64 {
        if n > 0 {
            fact = fact.mul(&Big::from_i64(n));
        }
        inv_fact.push(Big::one().div(&fact, p, Round::Nearest));
    }
    o.dd_table("INV_FACT", "`1/n!` for n = 0..31.", &inv_fact);
    let inv_odd: Vec<Big> = (0..32i64).map(|k| Big::one().div(&Big::from_i64(2 * k + 1), p, Round::Nearest)).collect();
    o.dd_table("INV_ODD", "`1/(2k+1)` for k = 0..31.", &inv_odd);
    let fact = |n: i64| (1..=n).fold(Big::one(), |a, k| a.mul(&Big::from_i64(k)));
    let odd_fact: Vec<Big> = (0..16i64).map(|k| Big::one().div(&fact(2 * k + 1), p, Round::Nearest)).collect();
    o.dd_table("INV_ODD_FACT", "`1/(2k+1)!` for k = 0..15 (sine series).", &odd_fact);
    let even_fact: Vec<Big> = (0..16i64).map(|k| Big::one().div(&fact(2 * k), p, Round::Nearest)).collect();
    o.dd_table("INV_EVEN_FACT", "`1/(2k)!` for k = 0..15 (cosine series).", &even_fact);
    o.close("series.rs", "Rational series coefficients");
    mp_constants(&mut o);
    o.close("mp_const.rs", "Constants of the special functions in the fixed-precision formats");
    crate::special_tables::emit(&mut o.src);
    o.close("erf.rs", "Taylor tables of the `erf`/`erfc` fast tier");
    crate::gamma_tables::emit(&mut o.src);
    o.close("gamma.rs", "Taylor tables of the `lgamma`/`tgamma` fast tier");
    crate::bessel_tables::emit(&mut o.src);
    o.close("bessel.rs", "Taylor, series and Hankel tables of the `j0`/`j1`/`y0`/`y1` fast tier");
    crate::ld_explog_tables::emit(&mut o.src);
    o.close("wide_int_el.rs", "Fixed-point constants and tables of the x87 `long double` exponentials' and logarithms' integer tier");
    o.files
}

/// Significand limbs of `liella-math`'s fixed-precision type (`mp::LIMBS` there).
const MP_LIMBS: usize = 6;

/// `|x|` (nonzero) as `liella-math`'s fixed-precision constant: the exponent `E` with
/// `|x|` in `[2^(E-1), 2^E)` and the 384-bit significand `floor(|x| · 2^(384 - E))`, least
/// significant limb first.
fn mp_words(x: &Big) -> (i64, [u64; MP_LIMBS]) {
    mp_words_n::<MP_LIMBS>(x)
}

/// Significand limbs of `liella-math`'s 768-bit type (`mpw::LIMBS` there).
const MPW_LIMBS: usize = 12;

/// [`mp_words`] for a significand of `L` limbs.
fn mp_words_n<const L: usize>(x: &Big) -> (i64, [u64; L]) {
    let a = x.abs();
    let e = a.ilogb().unwrap_or(0) + 1;
    let m = a.mul_pow2(64 * L as i64 - e).floor();
    let mut limbs = [0u64; L];
    for (i, limb) in limbs.iter_mut().enumerate() {
        let mut v = 0u64;
        for half in (0..2).rev() {
            let q = m.mul_pow2(-(64 * i as i64 + 32 * half)).floor();
            let r = q.sub(&q.mul_pow2(-32).floor().mul_pow2(32));
            v = (v << 32) | r.to_i64().unwrap_or(0) as u64;
        }
        *limb = v;
    }
    (e, limbs)
}

impl Out {
    fn mp_const(&mut self, name: &str, doc: &str, v: &Big) {
        let (e, l) = mp_words(v);
        let s: Vec<String> = l.iter().map(|w| format!("{w:#018x}")).collect();
        let _ = writeln!(self.src, "\n/// {doc} (about {:e})\npub(crate) const {name}: crate::mp::MpConst = ({e}, [{}]);", v.to_f64(), s.join(", "));
    }

    fn mpw_const(&mut self, name: &str, doc: &str, v: &Big) {
        let (e, l) = mp_words_n::<MPW_LIMBS>(v);
        let s: Vec<String> = l.iter().map(|w| format!("{w:#018x}")).collect();
        let _ = writeln!(self.src, "\n/// {doc} (about {:e})\npub(crate) const {name}: crate::mpw::MpConst = ({e}, [{}]);", v.to_f64(), s.join(", "));
    }

    /// A constant of a wider Ziv step (`mpx`, 1536 bits; `mpy`, 3072 bits): `L` limbs.
    fn mpn_const<const L: usize>(&mut self, module: &str, name: &str, doc: &str, v: &Big) {
        let (e, l) = mp_words_n::<L>(v);
        let s: Vec<String> = l.iter().map(|w| format!("{w:#018x}")).collect();
        let _ = writeln!(self.src, "\n/// {doc} (about {:e})\npub(crate) const {name}: crate::{module}::MpConst = ({e}, [{}]);", v.to_f64(), s.join(", "));
    }

    fn mp_table(&mut self, name: &str, doc: &str, values: &[Big]) {
        let _ = writeln!(self.src, "\n/// {doc}\npub(crate) static {name}: [crate::mp::MpConst; {}] = [", values.len());
        for v in values {
            let (e, l) = mp_words(v);
            let s: Vec<String> = l.iter().map(|w| format!("{w:#018x}")).collect();
            let _ = writeln!(self.src, "    ({e}, [{}]), // {:e}", s.join(", "), v.to_f64());
        }
        self.src.push_str("];\n");
    }
}

/// Bernoulli numbers `B_0 .. B_m`: `B_n = Σ_k (-1)^k k! S(n, k) / (k + 1)` with the Stirling
/// numbers of the second kind from `S(n, k) = k S(n-1, k) + S(n-1, k-1)`, summed exactly as
/// integers over the common denominator `L = lcm(1, ..., m + 1)` and divided once at `prec`
/// bits.
pub fn bernoulli(m: usize, prec: u32) -> Vec<Big> {
    let top = m as i64 + 1;
    let mut l = Big::one();
    for p in 2..=top {
        if (2..p).take_while(|d| d * d <= p).all(|d| p % d != 0) {
            let mut q = p;
            while q * p <= top {
                q *= p;
            }
            l = l.mul(&Big::from_i64(q));
        }
    }
    let lbits = l.ilogb().unwrap_or(0) as u32 + 8;
    let lk: Vec<Big> = (0..=m).map(|k| l.div(&Big::from_i64(k as i64 + 1), lbits, Round::Nearest)).collect();
    let mut fact = vec![Big::one()];
    for k in 1..=m {
        fact.push(fact[k - 1].mul(&Big::from_i64(k as i64)));
    }
    let mut s: Vec<Big> = vec![Big::one()]; // S(0, 0) = 1
    let mut out = Vec::with_capacity(m + 1);
    for n in 0..=m {
        if n > 0 {
            let mut next = vec![Big::zero(false); n + 1];
            for k in 1..=n {
                let a = if k < s.len() { s[k].mul(&Big::from_i64(k as i64)) } else { Big::zero(false) };
                next[k] = a.add(&s[k - 1]);
            }
            s = next;
        }
        let mut num = Big::zero(false);
        for (k, skn) in s.iter().enumerate() {
            let term = fact[k].mul(skn).mul(&lk[k]);
            num = if k % 2 == 0 { num.add(&term) } else { num.sub(&term) };
        }
        out.push(if num.is_zero() { num } else { num.div(&l, prec, Round::Nearest) });
    }
    out
}

/// Euler's constant by Brent and McMillan's formula: with `A = Σ (n^k/k!)² H_k` and
/// `B = Σ (n^k/k!)²`, `γ = A/B − ln n` within `π e^(−4n)`; `n = 96` gives about 550 bits, and
/// wider requests take `n ≥ (prec + 66)·ln 2 / 4` (a fixed 96 left the 2048-bit oracle value
/// wrong beyond bit 550, which the 768-bit Bessel step of D-10.20 would have inherited).
pub fn euler_gamma(prec: u32) -> Big {
    let w = prec + 64;
    let n = Big::from_i64(96.max((f64::from(w + 2) * core::f64::consts::LN_2 / 4.0) as i64 + 1));
    let n2 = n.mul(&n);
    let (mut a, mut b) = (Big::zero(false), Big::one());
    let mut t = Big::one();
    let mut h = Big::zero(false);
    let mut k = 1i64;
    loop {
        t = t.mul(&n2).div(&Big::from_i64(k * k), w, Round::Nearest);
        h = h.add(&Big::one().div(&Big::from_i64(k), w, Round::Nearest)).round(w, Round::Nearest);
        a = a.add(&t.mul(&h)).round(w, Round::Nearest);
        b = b.add(&t).round(w, Round::Nearest);
        if k > 400 && t.ilogb().unwrap_or(0) < b.ilogb().unwrap_or(0) - i64::from(w) - 8 {
            break;
        }
        k += 1;
    }
    a.div(&b, w, Round::Nearest).sub(&approx(mp::log(&n, w))).round(prec, Round::Nearest)
}

/// The constants of the special functions (milestone 3) in `liella-math`'s fixed-precision
/// format, appended to the generated source.
fn mp_constants(o: &mut Out) {
    let p = 520;
    let pi = mp::pi(p);
    let gamma = euler_gamma(p);
    o.mp_const("MP_PI", "`π` (mp::pi)", &pi);
    o.mp_const("MP_LN2", "`ln 2` (mp::ln2)", &mp::ln2(p));
    o.mp_const("MP_EULER", "Euler's constant `γ` (Brent–McMillan, `euler_gamma`)", &gamma);
    o.mp_const("MP_LN10", "`ln 10` (mp::ln10)", &approx(mp::log(&Big::from_i64(10), p)));
    o.mp_const("MP_LN_PI", "`ln π` (the reflection of `lgamma`)", &approx(mp::log(&pi, p)));
    let ln_2pi = approx(mp::log(&pi.mul_pow2(1), p));
    o.mp_const("MP_HALF_LN_2PI", "`ln(2π)/2`", &ln_2pi.mul_pow2(-1));
    let sqrt_pi = pi.sqrt(p, Round::Nearest);
    o.mp_const("MP_TWO_OVER_SQRT_PI", "`2/√π`", &Big::from_i64(2).div(&sqrt_pi, p, Round::Nearest));
    o.mp_const("MP_INV_SQRT_PI", "`1/√π`", &Big::one().div(&sqrt_pi, p, Round::Nearest));
    o.mp_const("MP_TWO_OVER_PI", "`2/π`", &Big::from_i64(2).div(&pi, p, Round::Nearest));
    // Stirling: c_k = B_2k / (2k (2k - 1)), k = 1..STIRLING_TERMS; the sign of c_k is (-1)^(k+1).
    let b = bernoulli(2 * STIRLING_TERMS, 1200);
    // The 768-bit second step of Ziv's strategy (`mpw`, D-10.20), from 840 bits.
    let pw = 840;
    o.mpw_const("MPW_PI", "`π` (mp::pi, 768 bits)", &mp::pi(pw));
    o.mpw_const("MPW_LN2", "`ln 2` (mp::ln2, 768 bits)", &mp::ln2(pw));
    o.mpw_const("MPW_EULER", "Euler's constant `γ` (`euler_gamma`, 768 bits)", &euler_gamma(pw));
    // The 1536- and 3072-bit steps of the complex `_Float128` functions (`mpx`, `mpy`, D-10.17),
    // from 1600 and 3150 bits.
    let px = 1600;
    o.mpn_const::<24>("mpx", "MPX_PI", "`π` (mp::pi, 1536 bits)", &mp::pi(px));
    o.mpn_const::<24>("mpx", "MPX_LN2", "`ln 2` (mp::ln2, 1536 bits)", &mp::ln2(px));
    o.mpn_const::<24>("mpx", "MPX_EULER", "Euler's constant `γ` (`euler_gamma`, 1536 bits)", &euler_gamma(px));
    let py = 3150;
    o.mpn_const::<48>("mpy", "MPY_PI", "`π` (mp::pi, 3072 bits)", &mp::pi(py));
    o.mpn_const::<48>("mpy", "MPY_LN2", "`ln 2` (mp::ln2, 3072 bits)", &mp::ln2(py));
    o.mpn_const::<48>("mpy", "MPY_EULER", "Euler's constant `γ` (`euler_gamma`, 3072 bits)", &euler_gamma(py));
    let ck: Vec<Big> = (1..=STIRLING_TERMS as i64).map(|k| b[2 * k as usize].div(&Big::from_i64(2 * k * (2 * k - 1)), p, Round::Nearest)).collect();
    o.mp_table("MP_STIRLING", &format!("`|B_2k| / (2k(2k − 1))` for k = 1..{STIRLING_TERMS} (Bernoulli numbers from `bernoulli`); the term's sign is `(−1)^(k+1)`."), &ck);
}

/// Terms of Stirling's series stored (enough below 2^-380 once the argument is 40 or more).
pub const STIRLING_TERMS: usize = 130;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod mp_constant_tests {
    use super::*;

    #[test]
    fn bernoulli_numbers_and_euler_constant() {
        let b = bernoulli(12, 200);
        let f = |x: &Big| x.to_f64();
        assert_eq!(f(&b[0]), 1.0);
        assert_eq!(f(&b[1]), -0.5);
        assert_eq!(f(&b[2]), 1.0 / 6.0);
        assert_eq!(f(&b[3]), 0.0);
        assert_eq!(f(&b[4]), -1.0 / 30.0);
        assert_eq!(f(&b[12]), -691.0 / 2730.0);
        // γ = 0.57721566490153286060651209008240243104215933593992...
        let g = euler_gamma(200);
        let want = Big::from_f64(0.577_215_664_901_532_9);
        assert!(g.sub(&want).abs().to_f64() < 1e-16);
        // Against 50 decimal digits: γ·10^49 truncated.
        let digits = "57721566490153286060651209008240243104215933593992";
        let mut v = Big::zero(false);
        for ch in digits.chars() {
            v = v.mul(&Big::from_i64(10)).add(&Big::from_i64(i64::from(ch.to_digit(10).unwrap())));
        }
        let mut ten49 = Big::one();
        for _ in 0..50 {
            ten49 = ten49.mul(&Big::from_i64(10));
        }
        let err = g.mul(&ten49).sub(&v).abs();
        assert!(err.to_f64() < 1.0, "γ disagrees in the first 50 digits");
    }
}
