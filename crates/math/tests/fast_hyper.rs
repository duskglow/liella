//! PERF-LIBM-HYPERBOLIC: the fast in-place entries of `fast_hyper` agree bit for bit with the accurate
//! double-double path of `hyper` (round-to-nearest) wherever they answer, and they answer for nearly
//! every operand of their domains. Spec clause: §10.3 (the hyperbolic functions are correctly
//! rounded; no new clause). The sweep stride of the `float` runs is `LIELLA_HYPER_STRIDE` (default 8191).

use liella_math::fast_hyper as fh;
use liella_math::hyper;
use liella_math::status::Rounding;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }
    fn unit(&mut self) -> f64 {
        f64::from(u32::try_from(self.next() >> 21).unwrap_or(0)) / 4_294_967_296.0
    }
}

type F64Fast = fn(f64) -> Option<f64>;
type F64Slow = fn(f64, Rounding) -> (f64, liella_math::Status);
type F32Fast = fn(f32) -> Option<f32>;
type F32Slow = fn(f32, Rounding) -> (f32, liella_math::Status);

/// (name, fast, accurate, ranges sampled).
type Row = (&'static str, F64Fast, F64Slow, Vec<(f64, f64)>);

fn f64_rows() -> Vec<Row> {
    vec![
        ("sinh", fh::sinh_nearest, hyper::sinh::<f64>, vec![(-10.0, 10.0), (-0.5, 0.5), (-700.0, 700.0), (-60.0, 60.0)]),
        ("cosh", fh::cosh_nearest, hyper::cosh::<f64>, vec![(-10.0, 10.0), (-0.01, 0.01), (-700.0, 700.0), (-60.0, 60.0)]),
        ("tanh", fh::tanh_nearest, hyper::tanh::<f64>, vec![(-10.0, 10.0), (-0.5, 0.5), (-21.0, 21.0)]),
        ("asinh", fh::asinh_nearest, hyper::asinh::<f64>, vec![(-10.0, 10.0), (-0.01, 0.01), (-1e6, 1e6), (-1e12, 1e12)]),
        ("acosh", fh::acosh_nearest, hyper::acosh::<f64>, vec![(1.0, 100.0), (1.0, 1.001), (1.0, 1e9)]),
        ("atanh", fh::atanh_nearest, hyper::atanh::<f64>, vec![(-0.99, 0.99), (-0.001, 0.001), (0.999, 0.999_999_9), (-1.0, 1.0)]),
    ]
}

#[test]
fn double_entries_match_the_accurate_path() {
    let n: usize = std::env::var("LIELLA_HYPER_N").ok().and_then(|s| s.parse().ok()).unwrap_or(40_000);
    let mut rng = Lcg(0x5eed_1234_abcd_ef01);
    for (name, fast, slow, ranges) in f64_rows() {
        let (mut answered, mut total) = (0usize, 0usize);
        for &(lo, hi) in &ranges {
            for _ in 0..n {
                let x = lo + (hi - lo) * rng.unit();
                total += 1;
                if let Some(v) = fast(x) {
                    answered += 1;
                    let (w, _) = slow(x, Rounding::Nearest);
                    assert_eq!(v.to_bits(), w.to_bits(), "{name}({x:e}): fast {v:e}, accurate {w:e}");
                }
            }
        }
        // the narrow ranges (|x| < 2^-8 and the like) mostly fall outside the fast domain by design
        assert!(answered * 10 >= total * 6, "{name}: fast path answered only {answered} of {total}");
    }
}

#[test]
fn float_entries_match_the_accurate_path() {
    let stride: u32 = std::env::var("LIELLA_HYPER_STRIDE").ok().and_then(|s| s.parse().ok()).unwrap_or(8191);
    let rows: Vec<(&str, F32Fast, F32Slow)> = vec![
        ("sinhf", fh::sinhf_nearest, hyper::sinh::<f32>),
        ("coshf", fh::coshf_nearest, hyper::cosh::<f32>),
        ("tanhf", fh::tanhf_nearest, hyper::tanh::<f32>),
        ("asinhf", fh::asinhf_nearest, hyper::asinh::<f32>),
        ("acoshf", fh::acoshf_nearest, hyper::acosh::<f32>),
        ("atanhf", fh::atanhf_nearest, hyper::atanh::<f32>),
    ];
    for (name, fast, slow) in rows {
        let (mut answered, mut seen) = (0u64, 0u64);
        let mut bits = 0u32;
        loop {
            let x = f32::from_bits(bits);
            // only the operands inside the entry's domain are interesting: skip NaN, infinities and
            // magnitudes the accurate path answers trivially
            if x.is_finite() && x.abs() > 1e-4 && x.abs() < 100.0 {
                seen += 1;
                if let Some(v) = fast(x) {
                    answered += 1;
                    let (w, _) = slow(x, Rounding::Nearest);
                    assert_eq!(v.to_bits(), w.to_bits(), "{name}({x:e}): fast {v:e}, accurate {w:e}");
                }
            }
            match bits.checked_add(stride) {
                Some(b) => bits = b,
                None => break,
            }
        }
        assert!(seen > 0 && answered * 100 >= seen, "{name}: fast path answered {answered} of {seen}");
    }
}
