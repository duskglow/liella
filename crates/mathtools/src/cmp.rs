//! The measurement of D-10.3: one function of one format, evaluated by glibc and by Liella on the
//! same operands (random samples, a hard-case corpus, or every `float`), compared bit for bit,
//! and each result classified against the [`mp`] oracle: correctly rounded or not, and its error
//! in ulps.
//!
//! The two libraries cannot share a process (they define the same names), so each side runs the
//! probe `probe/mathcmp.c` built against it; [`Probe`] streams operand records to it and reads
//! the results back. `cargo xtask mathcmp` builds the probes and runs the `mathcmp` binary.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::mp::{self, Approx, Big, Fmt, Round};

/// The formats, by the probe's letters.
pub fn format(letter: char) -> Option<Fmt> {
    Some(match letter {
        'f' => Fmt::F32,
        'd' => Fmt::F64,
        'l' => Fmt::F80,
        'q' => Fmt::F128,
        _ => return None,
    })
}

/// A rounding mode by the probe's letters.
pub fn round(letter: char) -> Option<Round> {
    Some(match letter {
        'N' => Round::Nearest,
        'U' => Round::Up,
        'D' => Round::Down,
        'Z' => Round::Zero,
        _ => return None,
    })
}

/// The number of operands of a function the probe knows, 0 for an unknown one.
pub fn arity(func: &str) -> usize {
    match func {
        "fma" => 3,
        "fmod" | "remainder" | "fdim" | "fmax" | "fmin" | "nextafter" | "copysign" | "pow" | "atan2" | "hypot" => 2,
        "sqrt" | "rint" | "nearbyint" | "ceil" | "floor" | "trunc" | "round" | "roundeven" | "logb" | "nextup" | "nextdown" | "exp" | "exp2"
        | "exp10" | "expm1" | "log" | "log2" | "log10" | "log1p" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh"
        | "tanh" | "asinh" | "acosh" | "atanh" | "cbrt" | "erf" | "erfc" | "tgamma" | "lgamma" | "j0" | "j1" | "y0" | "y1" => 1,
        _ => 0,
    }
}

/// The oracle of a function: its value at a working precision, or `None` where the oracle has
/// none (the exact functions, whose results are compared without one, and the special functions
/// of a later milestone).
pub fn oracle(func: &str, ops: &[Big], prec: u32) -> Option<Approx> {
    let x = ops.first()?;
    // tanh(x) for |x| > 200 and expm1(x) for x < −200 lie within 2^-288 of their limits ±1 and −1,
    // closer than half a last place of every format, on the same side as at ±200: the oracle
    // evaluates them there (beyond, its exponentials of −|x| run out of exponent range).
    let far = Big::from_f64(200.0);
    let limit = x.is_finite() && x.cmp_abs(&far).is_gt() && (func == "tanh" || (func == "expm1" && x.is_neg()));
    let near_limit = if x.is_neg() { far.neg() } else { far };
    // Likewise erf(x) for |x| > 12 and erfc(x) for x < −12 lie within 2^-212 of ±1 and 2: evaluated
    // at ±12 (their series would otherwise run 2x² terms).
    let twelve = Big::from_f64(12.0);
    let erf_limit = x.is_finite() && x.cmp_abs(&twelve).is_gt() && (func == "erf" || (func == "erfc" && x.is_neg()));
    let (limit, near_limit) = if erf_limit { (true, if x.is_neg() { twelve.neg() } else { twelve }) } else { (limit, near_limit) };
    let x = if limit { &near_limit } else { x };
    let y = ops.get(1);
    Some(match func {
        "sqrt" => {
            let v = x.sqrt(prec, Round::Nearest);
            let exact = !x.is_finite() || v.mul(&v).identical(x);
            Approx { value: if x.is_neg() && !x.is_zero() { Big::nan() } else { v }, exact }
        }
        "fma" => Approx { value: x.mul(y?).add(ops.get(2)?), exact: true },
        "exp" => mp::exp(x, prec),
        "exp2" => mp::exp2(x, prec),
        "exp10" => mp::exp10(x, prec),
        "expm1" => mp::expm1(x, prec),
        "log" => mp::log(x, prec),
        "log2" => mp::log2(x, prec),
        "log10" => mp::log10(x, prec),
        "log1p" => mp::log1p(x, prec),
        "sin" => mp::sin(x, prec),
        "cos" => mp::cos(x, prec),
        "tan" => mp::tan(x, prec),
        "asin" => mp::asin(x, prec),
        "acos" => mp::acos(x, prec),
        "atan" => mp::atan(x, prec),
        "sinh" => mp::sinh(x, prec),
        "cosh" => mp::cosh(x, prec),
        "tanh" => mp::tanh(x, prec),
        "asinh" => mp::asinh(x, prec),
        "acosh" => mp::acosh(x, prec),
        "atanh" => mp::atanh(x, prec),
        "cbrt" => mp::cbrt(x, prec),
        "erf" => mp::erf(x, prec),
        "erfc" => mp::erfc(x, prec),
        "lgamma" => mp::lgamma(x, prec),
        "tgamma" => mp::tgamma(x, prec),
        "j0" => mp::bessel_j(0, x, prec),
        "j1" => mp::bessel_j(1, x, prec),
        "y0" => mp::bessel_y(0, x, prec),
        "y1" => mp::bessel_y(1, x, prec),
        "pow" => mp::pow(x, y?, prec),
        "atan2" => mp::atan2(x, y?, prec),
        "hypot" => {
            // A smaller operand more than 400 binades below the larger moves the result above |big|
            // by under 2^-800 relative, on the same side as at 400 binades: evaluated there.
            let y = y?;
            let (big, small) = if x.cmp_abs(y).is_ge() { (x, y) } else { (y, x) };
            match (big.ilogb(), small.ilogb()) {
                (Some(b), Some(s)) if b - s > 400 => mp::hypot(big, &Big::from_f64(1.0).mul_pow2(b - 400), prec),
                _ => mp::hypot(x, y, prec),
            }
        }
        _ => return None,
    })
}

/// A running probe.
pub struct Probe {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
    writer: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl Probe {
    /// Start `exe` on `func` in `fmt` and `mode`, feeding it `input` (operand records) from a
    /// thread.
    pub fn start(exe: &Path, func: &str, fmt: char, mode: char, input: Vec<u8>) -> Result<Probe, String> {
        let mut child = Command::new(exe)
            .args([func, &fmt.to_string(), &mode.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        let mut stdin = BufWriter::new(child.stdin.take().ok_or("no stdin")?);
        let out = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let writer = std::thread::spawn(move || stdin.write_all(&input).and_then(|()| stdin.flush()));
        Ok(Probe { child, out, writer: Some(writer) })
    }

    /// The next result: its encoding and the raised flags.
    pub fn result(&mut self) -> Option<(u128, u8)> {
        let mut rec = [0u8; 17];
        self.out.read_exact(&mut rec).ok()?;
        let mut v = [0u8; 16];
        v.copy_from_slice(&rec[..16]);
        Some((u128::from_le_bytes(v), rec[16]))
    }

    /// Wait for the probe; an error if it failed.
    pub fn finish(mut self) -> Result<(), String> {
        if let Some(w) = self.writer.take() {
            w.join().map_err(|_| "writer panicked".to_string())?.map_err(|e| e.to_string())?;
        }
        let st = self.child.wait().map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err(format!("probe exited with {st}")) }
    }
}

/// A deterministic generator (xorshift64*).
pub struct Rng(u64);

impl Rng {
    /// Seeded (a zero seed is replaced).
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed })
    }
    /// The next 64 bits.
    pub fn bits(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// A random operand of `fmt`: a quarter uniform over all encodings, the rest of moderate
/// magnitude (exponents within ±`span`) with random sign and significand.
pub fn operand(fmt: Fmt, rng: &mut Rng, span: i32) -> u128 {
    let bits = (u128::from(rng.bits()) << 64) | u128::from(rng.bits());
    let total = fmt.total_bits();
    let mask = if total >= 128 { u128::MAX } else { (1u128 << total) - 1 };
    if rng.bits() % 4 == 0 {
        // The x87 format's explicit bit is set except for zero exponents, so most encodings are
        // values rather than invalid operands.
        let b = bits & mask;
        return if fmt.explicit_lead { b | (1u128 << 63) } else { b };
    }
    let frac_bits = if fmt.explicit_lead { fmt.prec } else { fmt.prec - 1 };
    let e = (rng.bits() % (2 * span as u64 + 1)) as i32 - span;
    let biased = (e + fmt.emax) as u128;
    let frac = bits & ((1u128 << (fmt.prec - 1)) - 1);
    let lead = if fmt.explicit_lead { 1u128 << 63 } else { 0 };
    let sign = u128::from(rng.bits() & 1) << (frac_bits + fmt.exp_bits);
    sign | (biased << frac_bits) | lead | frac
}

/// The outcome counts of a run.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    /// Records compared.
    pub n: u64,
    /// Bit-identical results (NaNs compared by kind only).
    pub identical: u64,
    /// Identical results whose flags differ.
    pub flags_differ: u64,
    /// Records the oracle classified (a correctly rounded result was determined).
    pub classified: u64,
    /// glibc's result correctly rounded.
    pub glibc_cr: u64,
    /// Liella's result correctly rounded.
    pub liella_cr: u64,
    /// Mismatches where only Liella is correctly rounded (glibc not correctly rounded).
    pub glibc_not_cr: u64,
    /// Mismatches where only glibc is correctly rounded (a Liella error).
    pub liella_not_cr: u64,
    /// Mismatches where neither is.
    pub neither_cr: u64,
    /// Largest error of glibc's results in ulps.
    pub glibc_max_ulp: f64,
    /// Largest error of Liella's results in ulps.
    pub liella_max_ulp: f64,
    /// Records the oracle could not decide.
    pub undetermined: u64,
    /// Records where Liella is not correctly rounded (operands, Liella, the correct result).
    pub wrong: Vec<(Vec<u128>, u128, u128)>,
    /// Some mismatching records (operands, glibc, Liella), for the report.
    pub samples: Vec<(Vec<u128>, u128, u128, u8, u8)>,
}

impl Tally {
    /// Add another shard's counts.
    pub fn merge(&mut self, o: Tally) {
        self.n += o.n;
        self.identical += o.identical;
        self.flags_differ += o.flags_differ;
        self.classified += o.classified;
        self.glibc_cr += o.glibc_cr;
        self.liella_cr += o.liella_cr;
        self.glibc_not_cr += o.glibc_not_cr;
        self.liella_not_cr += o.liella_not_cr;
        self.neither_cr += o.neither_cr;
        self.glibc_max_ulp = self.glibc_max_ulp.max(o.glibc_max_ulp);
        self.liella_max_ulp = self.liella_max_ulp.max(o.liella_max_ulp);
        self.undetermined += o.undetermined;
        for w in o.wrong {
            if self.wrong.len() < 10 {
                self.wrong.push(w);
            }
        }
        for s in o.samples {
            if self.samples.len() < 20 {
                self.samples.push(s);
            }
        }
    }
}

/// Is `b` a NaN encoding of `fmt`?
pub fn is_nan(fmt: Fmt, b: u128) -> bool {
    Big::from_bits(fmt, b).is_nan()
}

/// One run's parameters.
pub struct Run<'a> {
    /// Function name.
    pub func: &'a str,
    /// Format letter.
    pub fmt: char,
    /// Rounding mode letter.
    pub mode: char,
    /// The reference probe.
    pub host: PathBuf,
    /// Liella's probe, if compared.
    pub liella: Option<PathBuf>,
    /// Classify with the oracle (all records, or only mismatches).
    pub oracle: Oracle,
}

/// When the oracle runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Oracle {
    /// Never.
    Off,
    /// For mismatching records only.
    Mismatches,
    /// For every record.
    All,
}

/// Evaluate one shard of operand records (`records` of `arity` encodings each).
pub fn shard(run: &Run<'_>, records: &[Vec<u128>]) -> Result<Tally, String> {
    let fmt = format(run.fmt).ok_or("unknown format")?;
    let mode = round(run.mode).ok_or("unknown mode")?;
    let mut input = Vec::with_capacity(records.len() * 48);
    for r in records {
        for v in r {
            input.extend_from_slice(&v.to_le_bytes());
        }
    }
    let mut host = Probe::start(&run.host, run.func, run.fmt, run.mode, input.clone())?;
    let mut ours = match &run.liella {
        Some(p) => Some(Probe::start(p, run.func, run.fmt, run.mode, input)?),
        None => None,
    };
    let mut t = Tally::default();
    for ops in records {
        let (g, gf) = host.result().ok_or("host probe ended early")?;
        let (l, lf) = match ours.as_mut() {
            Some(p) => p.result().ok_or("Liella probe ended early")?,
            None => (g, gf),
        };
        t.n += 1;
        let same = g == l || (is_nan(fmt, g) && is_nan(fmt, l));
        if same {
            t.identical += 1;
            if gf != lf {
                t.flags_differ += 1;
            }
        }
        let want = match run.oracle {
            Oracle::All => true,
            Oracle::Mismatches => !same,
            Oracle::Off => false,
        };
        if (!same || gf != lf) && t.samples.len() < 20 {
            t.samples.push((ops.clone(), g, l, gf, lf));
        }
        if !want {
            continue;
        }
        let args: Vec<Big> = ops.iter().map(|b| Big::from_bits(fmt, *b)).collect();
        if oracle(run.func, &args, 64).is_none() {
            continue;
        }
        let cr = mp::correctly_rounded(fmt, mode, 4096, |p| oracle(run.func, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true }));
        let Ok((cr, _)) = cr else {
            t.undetermined += 1;
            continue;
        };
        t.classified += 1;
        let ok = |b: u128| b == cr || (is_nan(fmt, b) && is_nan(fmt, cr));
        let (gok, lok) = (ok(g), ok(l));
        t.glibc_cr += u64::from(gok);
        t.liella_cr += u64::from(lok);
        if !lok && t.wrong.len() < 10 {
            t.wrong.push((ops.clone(), l, cr));
        }
        if !same {
            match (gok, lok) {
                (false, true) => t.glibc_not_cr += 1,
                (true, false) => t.liella_not_cr += 1,
                _ => t.neither_cr += 1,
            }
        }
        if !gok || !lok {
            let exact = oracle(run.func, &args, 2 * fmt.prec + 40).map(|a| a.value).unwrap_or_else(Big::nan);
            t.glibc_max_ulp = t.glibc_max_ulp.max(mp::ulp_error(fmt, g, &exact));
            t.liella_max_ulp = t.liella_max_ulp.max(mp::ulp_error(fmt, l, &exact));
        }
    }
    host.finish()?;
    if let Some(p) = ours {
        p.finish()?;
    }
    Ok(t)
}
