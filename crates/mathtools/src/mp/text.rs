//! Text: exact parsing of decimal and C hexadecimal floating constants, exact hex display and
//! rounded decimal display.

use super::nat::Nat;
use super::{Big, Kind, Round};

/// Decimal exponents beyond this magnitude are refused (the exact value would need millions of
/// bits; no format comes near).
const DEC_EXP_LIMIT: i64 = 400_000;

enum Parsed {
    Special(Big),
    /// `±man * 2^exp`.
    Bin(bool, Nat, i64),
    /// `±man * 10^exp`.
    Dec(bool, Nat, i64),
}

fn digit(c: u8, radix: u32) -> Option<u64> {
    (c as char).to_digit(radix).map(u64::from)
}

fn parse_exp(s: &[u8]) -> Option<i64> {
    let (neg, s) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if s.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for &c in s {
        v = v.saturating_mul(10).saturating_add(digit(c, 10)? as i64);
    }
    Some(if neg { -v } else { v })
}

fn lex(s: &str) -> Option<Parsed> {
    let s = s.trim();
    let (neg, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let lower = body.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        return Some(Parsed::Special(Big::inf(neg)));
    }
    if lower == "nan" {
        return Some(Parsed::Special(Big::nan()));
    }
    let b = lower.as_bytes();
    let (hex, b) = if b.starts_with(b"0x") { (true, &b[2..]) } else { (false, b) };
    let radix: u32 = if hex { 16 } else { 10 };
    let exp_char = if hex { b'p' } else { b'e' };
    let (mant, exp) = match b.iter().position(|&c| c == exp_char) {
        Some(i) => (&b[..i], parse_exp(&b[i + 1..])?),
        None => (b, 0),
    };
    let mut man = Nat::zero();
    let mut frac_digits: i64 = 0;
    let mut seen_point = false;
    let mut any = false;
    for &c in mant {
        if c == b'.' {
            if seen_point {
                return None;
            }
            seen_point = true;
            continue;
        }
        let d = digit(c, radix)?;
        man = man.mul_small(u64::from(radix)).add_small(d);
        any = true;
        if seen_point {
            frac_digits += 1;
        }
    }
    if !any {
        return None;
    }
    Some(if hex {
        Parsed::Bin(neg, man, exp.saturating_sub(4 * frac_digits))
    } else {
        Parsed::Dec(neg, man, exp.saturating_sub(frac_digits))
    })
}

impl Big {
    /// Exact parse of a decimal (`-1.25e-3`), C hex float (`-0x1.8p-3`), `inf`/`infinity` or
    /// `nan`. Decimals that are not dyadic rationals (such as `0.1`) give `None`; use
    /// [`Big::parse_rounded`] for those. Decimal exponents beyond ±400000 are refused.
    pub fn parse(s: &str) -> Option<Big> {
        match lex(s)? {
            Parsed::Special(b) => Some(b),
            Parsed::Bin(neg, man, exp) => Some(Big::fin(neg, man, exp)),
            Parsed::Dec(neg, man, exp) => {
                if man.is_zero() {
                    return Some(Big::zero(neg));
                }
                if exp.abs() > DEC_EXP_LIMIT {
                    return None;
                }
                let p5 = Nat::from_u64(5).pow(exp.unsigned_abs());
                if exp >= 0 {
                    Some(Big::fin(neg, man.mul(&p5), exp))
                } else {
                    let (q, r) = man.divrem(&p5);
                    if r.is_zero() { Some(Big::fin(neg, q, exp)) } else { None }
                }
            }
        }
    }

    /// Parse rounded to nearest at `prec` bits (exact constants are rounded too).
    pub fn parse_rounded(s: &str, prec: u32) -> Option<Big> {
        match lex(s)? {
            Parsed::Special(b) => Some(b),
            Parsed::Bin(neg, man, exp) => Some(Big::fin(neg, man, exp).round(prec, Round::Nearest)),
            Parsed::Dec(neg, man, exp) => {
                if man.is_zero() {
                    return Some(Big::zero(neg));
                }
                if exp.abs() > DEC_EXP_LIMIT {
                    return None;
                }
                let p5 = Big::fin(false, Nat::from_u64(5).pow(exp.unsigned_abs()), 0);
                let m = Big::fin(neg, man, 0);
                // m * 10^e = m * 5^e * 2^e; one correctly rounded operation either way.
                Some(if exp >= 0 {
                    m.mul(&p5).round(prec, Round::Nearest).mul_pow2(exp)
                } else {
                    m.div(&p5, prec, Round::Nearest).mul_pow2(exp)
                })
            }
        }
    }

    /// Exact C hex-float text: `-0x1.8p-3`, `0x0p+0`, `inf`, `-inf`, `nan`.
    pub fn to_hex(&self) -> String {
        let sign = if self.is_neg() { "-" } else { "" };
        match self.kind {
            Kind::Nan => "nan".to_string(),
            Kind::Inf => format!("{sign}inf"),
            Kind::Zero => format!("{sign}0x0p+0"),
            Kind::Fin => {
                let bl = self.man.bit_len();
                let e = self.top();
                let fbits = bl - 1;
                let pad = (4 - fbits % 4) % 4;
                let frac = self.man.sub(&Nat::from_u64(1).shl(fbits)).shl(pad);
                let digits = ((fbits + pad) / 4) as usize;
                let es = if e >= 0 { format!("+{e}") } else { e.to_string() };
                if digits == 0 {
                    format!("{sign}0x1p{es}")
                } else {
                    format!("{sign}0x1.{}p{es}", frac.to_hex(digits))
                }
            }
        }
    }

    /// Decimal scientific text with `digits` significant digits, rounded to nearest (ties to
    /// even): `-1.2500e-3`.
    pub fn to_decimal(&self, digits: usize) -> String {
        let sign = if self.is_neg() { "-" } else { "" };
        match self.kind {
            Kind::Nan => return "nan".to_string(),
            Kind::Inf => return format!("{sign}inf"),
            Kind::Zero => return format!("{sign}0"),
            Kind::Fin => {}
        }
        if self.top().abs() > 1 << 20 {
            return self.to_hex(); // a decimal would need millions of digits of work
        }
        let digits = digits.max(1) as i64;
        let a = self.abs();
        // Estimate k = floor(log10 |x|) from the binary exponent, then fix it up (a carry out
        // of the last digit moves k by one, so this settles within a few steps).
        let mut k = ((self.top() as f64) * std::f64::consts::LOG10_2).floor() as i64;
        let mut s = String::new();
        for _ in 0..8 {
            s = scaled_int(&a, digits - 1 - k).to_dec();
            let len = s.len() as i64;
            if len > digits {
                k += 1;
            } else if len < digits {
                k -= 1;
            } else {
                break;
            }
        }
        let (h, t) = s.split_at(1.min(s.len()));
        if t.is_empty() { format!("{sign}{h}e{k}") } else { format!("{sign}{h}.{t}e{k}") }
    }
}

/// `round(|x| * 10^s)` to the nearest integer (ties to even), exactly.
fn scaled_int(x: &Big, s: i64) -> Nat {
    let p = Nat::from_u64(10).pow(s.unsigned_abs());
    // x * 10^s = num / den with integers.
    let (mut num, mut den) = (x.man.clone(), Nat::from_u64(1));
    if s >= 0 {
        num = num.mul(&p);
    } else {
        den = p;
    }
    if x.exp >= 0 {
        num = num.shl(x.exp as u64);
    } else {
        den = den.shl(x.exp.unsigned_abs());
    }
    let (q, r) = num.divrem(&den);
    match r.shl(1).cmp(&den) {
        std::cmp::Ordering::Greater => q.add_small(1),
        std::cmp::Ordering::Equal if q.is_odd() => q.add_small(1),
        _ => q,
    }
}
