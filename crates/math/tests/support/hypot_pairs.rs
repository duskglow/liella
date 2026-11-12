//! The `double` hypot kernel's hard pairs (D-10.104), shared by the math crate's kernel test
//! (the software root, `SoftOps`) and the boundary's (the hardware root, `Hw`; LIBM-HYPOT-DIRECTED):
//! bit patterns, near-binade pairs at every magnitude, the gap boundary (26/27/28), ties (triples
//! with an odd 54-bit hypotenuse) and exact roots at several scales, overflow at `DBL_MAX`, two
//! subnormals (the `2^-1022` tininess edge), a subnormal beside a normal, and roots just off a midpoint.

fn gcd64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Pythagorean triples `s^2 − t^2, 2st, s^2 + t^2` with legs below `2^53`, from `s` in `s0..s0 + n`.
fn triples(s0: u64, n: u64, ties: bool) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for s in s0..s0 + n {
        for t in [s * 21 / 50, s * 21 / 50 + 1, s * 2 / 5, s * 2 / 5 + 1] {
            let (a, b, c) = (s * s - t * t, 2 * s * t, s * s + t * t);
            if (s + t) % 2 == 1 && gcd64(s, t) == 1 && a < 1 << 53 && b < 1 << 53 && (c >= 1 << 53) == ties && c < 1 << 54 {
                out.push((a as f64, b as f64));
            }
        }
    }
    out
}

/// Every pair of the kernel test (module comment), deterministic.
pub fn hard_pairs() -> Vec<(f64, f64)> {
    let mut st = 0x243f_6a88_85a3_08d3u64;
    let mut next = move || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    };
    let mant = (1u64 << 52) - 1;
    let mut v: Vec<(f64, f64)> = Vec::new();
    for _ in 0..3000 {
        let (p, q) = (next() % (0x7ff << 52), next() % (0x7ff << 52));
        v.push((f64::from_bits(p), f64::from_bits(q)));
        let e = 1 + next() % 2046;
        let eb = (e + next() % 5).saturating_sub(2).min(2046);
        v.push((f64::from_bits(e << 52 | (next() & mant)), f64::from_bits(eb << 52 | (next() & mant))));
    }
    for d in 25..=29u64 {
        for _ in 0..60 {
            let e = 30 + next() % 2016;
            v.push((f64::from_bits(e << 52 | (next() & mant)), f64::from_bits((e - d) << 52 | (next() & mant))));
        }
    }
    let ties = triples(95_000_000, 3000, true);
    let exact = triples(66_000_000, 600, false);
    assert!(ties.len() > 100 && exact.len() > 100, "{} ties, {} exact roots", ties.len(), exact.len());
    for &(a, b) in ties.iter().chain(&exact) {
        for k in [-1000, -1, 0, 900, 960] {
            v.push((a * 2f64.powi(k), b * 2f64.powi(k)));
        }
    }
    let max = f64::MAX;
    for b in [max, 1.0, max * 2f64.powi(-26), max * 2f64.powi(-27), max * 0.7, f64::from_bits(1)] {
        v.push((max, b));
        v.push((max * 0.75, b.min(max * 0.7)));
    }
    let c = 3_184_525_836_262_886u64; // about 2^51.5: two such subnormals have a root next to 2^-1022
    for j in 0..300u64 {
        v.push((f64::from_bits(c + j % 3), f64::from_bits(c - j)));
        v.push((f64::from_bits(next() & mant), f64::from_bits(next() & mant)));
        v.push((f64::from_bits((1 + next() % 30) << 52 | (next() & mant)), f64::from_bits(next() & mant)));
    }
    // The fast entry's gap exit adds `a·2^-60` built from `a`'s bits, a normal number only for a
    // field above 60: every field around that edge, beside a subnormal and 27 binades down.
    for ea in 27..=64u64 {
        v.push((f64::from_bits(ea << 52 | (next() & mant)), f64::from_bits(next() & mant)));
        v.push((f64::from_bits(ea << 52 | (next() & mant)), f64::from_bits((ea - 27) << 52 | (next() & mant))));
    }
    // The largest subnormal beside B = 2^25 + k·2^22 (k < 48): the integer root crosses 2^52, so each
    // direction's tininess edge (nearest: A^2 + B^2 <= 2^104 − 2^51) has pairs on both sides of it.
    for k in 0..48u64 {
        v.push((f64::from_bits(mant), f64::from_bits((1 << 25) + (k << 22))));
    }
    // Roots just off a midpoint, no tie: for odd s in [2^26.29, 2^26.5), hypot(s^2 − 1, s) lies about
    // 3/(8 s^2) above the midpoint s^2 − 1/2 and hypot(s^2, s) about 1/(8 s^2) below s^2 + 1/2, both
    // inside the root's 2^-104 error: only Ziv's margin sends them to the exact compare.
    for i in 0..64u32 {
        let s = f64::from(82_200_001 + 198_000 * i);
        v.push((s * s - 1.0, s));
        v.push((s * s, s));
    }
    v
}
