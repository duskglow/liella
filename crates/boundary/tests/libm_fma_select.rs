//! The libm product runs the FMA builds of the `double` exp/log fast paths on a CPU with FMA, and
//! `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA` keeps the baseline (D-10.25, D-10.50, #2920): `libm.a`
//! carries its own copy of the dispatch byte, which its load-time constructor decides. Both builds
//! return the same bits, so the only observable is the work done: a static program linked with
//! `-lm` counts its own user-mode instructions over a fixed loop of `exp` and `log` calls
//! (`perf_event_open`, self-monitoring), once with the tunable and once without. Instruction
//! counts of a fixed loop are deterministic; no wall clock is involved.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Calls in the counted loop (each iteration one `exp` and one `log`).
const CALLS: u64 = 4000;

// `perf_event_attr` written as words (PERF_ATTR_SIZE_VER0 = 64 bytes, no kernel header needed):
// type PERF_TYPE_HARDWARE (0) and size in word 0, config PERF_COUNT_HW_INSTRUCTIONS (1), flags
// disabled | exclude_kernel | exclude_hv in word 5. 298 = SYS_perf_event_open on x86-64; the
// ioctls are PERF_EVENT_IOC_ENABLE/DISABLE/RESET.
const PROGRAM: &str = r#"
#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <unistd.h>
long syscall(long, ...);
int main(void) {
    uint64_t attr[16] = {0};
    attr[0] = (uint64_t)64 << 32;
    attr[1] = 1;
    attr[5] = 1 | 1 << 5 | 1 << 6;
    volatile double x = 0.75, sink = 0;
    sink += exp(x) + log(x + 1.0);
    long fd = syscall(298, attr, 0, -1, -1, 0);
    if (fd < 0) { printf("noperf %d\n", errno); return 0; }
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int i = 0; i < 4000; i++) sink += exp(x + i * 1e-4) + log(x + 1.0 + i * 1e-4);
    ioctl(fd, 0x2401, 0);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) { printf("noperf read %d\n", errno); return 0; }
    printf("%llu\n", (unsigned long long)n);
    return 0;
}
"#;

/// The program's count, or `None` (with the reason printed) when the kernel refuses the counter.
fn count(exe: &PathBuf, tunables: Option<&str>) -> Option<u64> {
    let mut cmd = Command::new(exe);
    cmd.env_remove("GLIBC_TUNABLES");
    if let Some(t) = tunables {
        cmd.env("GLIBC_TUNABLES", t);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.starts_with("noperf") {
        println!("SKIP: no user-mode instruction counter for this process ({text}): perf_event_open refused");
        return None;
    }
    Some(text.parse().unwrap_or_else(|_| panic!("unexpected output {text:?}")))
}

#[test]
fn libm_product_selects_the_fma_builds() {
    // Trace: no spec clause (D-10.25 run-time FMA builds; D-10.50 the libm product's own decision).
    if !std::arch::is_x86_feature_detected!("fma") {
        println!("SKIP: this CPU has no FMA (or the OS does not save the AVX state): one build only");
        return;
    }
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists() && products.join("libm.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = root().join("target/test-scratch/libm_fma_select").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.c"), PROGRAM).unwrap();
    let out = Command::new(root().join("scripts/liella-cc"))
        .env("LIELLA_PRODUCTS", &products)
        .env_remove("LIELLA_CC_NO_LIBM")
        .env_remove("LIELLA_CC_EXTRA")
        .current_dir(&dir)
        .args(["-O1", "-o", "m", "m.c", "-lm"])
        .output()
        .unwrap();
    assert!(out.status.success(), "liella-cc failed: {}", String::from_utf8_lossy(&out.stderr));
    let exe = dir.join("m");
    let Some(fused) = count(&exe, None) else { return };
    let Some(plain) = count(&exe, Some("glibc.cpu.hwcaps=-FMA")) else { return };
    println!("user instructions for {CALLS} exp + {CALLS} log calls: FMA builds {fused}, -FMA {plain}");
    // At least one instruction per call fewer: the libm product reached its FMA builds, and the
    // tunable reached its decision.
    assert!(fused + CALLS <= plain, "libm FMA selection: default {fused} instructions, -FMA {plain}: the FMA builds did not run (or the tunable was ignored)");
}

/// `hypot` upward over a fixed pair set (hypot_near's shape: every binade, ratio in [1/4, 4]):
/// the counted loop and an XOR of the result bits, so the two builds' work and bits compare.
const HYPOT_PROGRAM: &str = r#"
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>
long syscall(long, ...);
int main(void) {
    uint64_t attr[16] = {0};
    attr[0] = (uint64_t)64 << 32;
    attr[1] = 1;
    attr[5] = 1 | 1 << 5 | 1 << 6;
    volatile double x = 0.75, y = 0.5;
    uint64_t bits = 0;
    fesetround(FE_UPWARD);
    long fd = syscall(298, attr, 0, -1, -1, 0);
    if (fd < 0) { printf("noperf %d\n", errno); return 0; }
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int i = 0; i < 4000; i++) {
        double r = hypot(ldexp(x + i * 1e-4, i % 2000 - 1000), ldexp(y + i * 3e-4, i % 2000 - 1000));
        uint64_t u; memcpy(&u, &r, sizeof u); bits ^= u + (uint64_t)i;
    }
    ioctl(fd, 0x2401, 0);
    fesetround(FE_TONEAREST);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) { printf("noperf read %d\n", errno); return 0; }
    printf("%llu %016llx\n", (unsigned long long)n, (unsigned long long)bits);
    return 0;
}
"#;

#[test]
fn libm_product_selects_the_fma_hypot_kernel_in_a_directed_mode() {
    // Trace: no spec clause (D-10.25 run-time FMA builds; the directed `hypot` kernel's FMA build,
    // LIBM-HYPOT-DIRECTED-FAST). Both builds return the same bits; the FMA one does less work.
    if !std::arch::is_x86_feature_detected!("fma") {
        println!("SKIP: this CPU has no FMA (or the OS does not save the AVX state): one build only");
        return;
    }
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists() && products.join("libm.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = root().join("target/test-scratch/libm_fma_select_hypot").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("h.c"), HYPOT_PROGRAM).unwrap();
    let out = Command::new(root().join("scripts/liella-cc"))
        .env("LIELLA_PRODUCTS", &products)
        .env_remove("LIELLA_CC_NO_LIBM")
        .env_remove("LIELLA_CC_EXTRA")
        .current_dir(&dir)
        .args(["-O1", "-o", "h", "h.c", "-lm"])
        .output()
        .unwrap();
    assert!(out.status.success(), "liella-cc failed: {}", String::from_utf8_lossy(&out.stderr));
    let run = |t: Option<&str>| -> Option<(u64, String)> {
        let mut cmd = Command::new(dir.join("h"));
        cmd.env_remove("GLIBC_TUNABLES");
        if let Some(t) = t {
            cmd.env("GLIBC_TUNABLES", t);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.starts_with("noperf") {
            println!("SKIP: no user-mode instruction counter for this process ({text}): perf_event_open refused");
            return None;
        }
        let (n, bits) = text.split_once(' ').unwrap_or_else(|| panic!("unexpected output {text:?}"));
        Some((n.parse().unwrap_or_else(|_| panic!("unexpected output {text:?}")), bits.to_string()))
    };
    let Some((fused, fused_bits)) = run(None) else { return };
    let Some((plain, plain_bits)) = run(Some("glibc.cpu.hwcaps=-FMA")) else { return };
    println!("user instructions for {CALLS} upward hypot calls: FMA build {fused}, -FMA {plain}");
    assert_eq!(fused_bits, plain_bits, "hypot upward: the FMA build's bits differ from the baseline's");
    assert!(fused + CALLS <= plain, "hypot FMA selection: default {fused} instructions, -FMA {plain}: the FMA kernel did not run in the directed mode");
}

/// The `double` trig exports' bits over a fixed argument set: every binade 2^-36..2^20 (both
/// signs) and multiples of pi/2, the range of the in-place fast entries (LIBM-F64-TRIG-TIER).
const TRIG_PROGRAM: &str = r#"
#define _GNU_SOURCE
#include <math.h>
#include <stdio.h>
#include <string.h>
static unsigned long long b(double v) { unsigned long long u; memcpy(&u, &v, sizeof u); return u; }
int main(void) {
    for (int i = 0; i < 1400; i++) {
        double x = i < 1200 ? ldexp(1.0 + i * 0.0123456789, i % 57 - 36) * (i & 1 ? -1.0 : 1.0)
                            : (i - 1199) * 1.5707963267948966;
        volatile double v = x;
        double s, c;
        sincos(v, &s, &c);
        printf("%d %016llx %016llx %016llx %016llx %016llx\n", i, b(sin(v)), b(cos(v)), b(tan(v)), b(s), b(c));
    }
    return 0;
}
"#;

#[test]
fn libm_product_trig_fma_builds_return_the_baseline_bits() {
    // Trace: no spec clause (D-10.25 the FMA builds return the baseline's bits; D-10.50 the libm product's decision).
    if !std::arch::is_x86_feature_detected!("fma") {
        println!("SKIP: this CPU has no FMA (or the OS does not save the AVX state): one build only");
        return;
    }
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists() && products.join("libm.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = root().join("target/test-scratch/libm_fma_select_trig").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.c"), TRIG_PROGRAM).unwrap();
    let out = Command::new(root().join("scripts/liella-cc"))
        .env("LIELLA_PRODUCTS", &products)
        .env_remove("LIELLA_CC_NO_LIBM")
        .env_remove("LIELLA_CC_EXTRA")
        .current_dir(&dir)
        .args(["-O1", "-o", "t", "t.c", "-lm"])
        .output()
        .unwrap();
    assert!(out.status.success(), "liella-cc failed: {}", String::from_utf8_lossy(&out.stderr));
    let run = |tunables: Option<&str>| {
        let mut cmd = Command::new(dir.join("t"));
        cmd.env_remove("GLIBC_TUNABLES");
        if let Some(t) = tunables {
            cmd.env("GLIBC_TUNABLES", t);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let fused = run(None);
    let plain = run(Some("glibc.cpu.hwcaps=-FMA"));
    assert_eq!(fused.lines().count(), 1400, "unexpected output: {fused:.200}");
    assert_eq!(plain.lines().count(), 1400, "unexpected output: {plain:.200}");
    // Columns: index, sin, cos, tan, sincos sine, sincos cosine.
    for (f, p) in fused.lines().zip(plain.lines()) {
        assert_eq!(f, p, "trig FMA build (left) differs from the baseline (right)");
    }
}

/// `exp`'s tiers through the product (D-10.51, LIBM-F64-EXP-QUICK-WIRE): `@MISS@` holds operands
/// whose SSE2 quick phase declines and whose tier 2 decides, `@HIT@` operands the quick phase
/// decides. The program counts its user-mode instructions over 32 passes of each set (2048
/// calls) and prints both counts, then the bits of `exp` on each miss operand.
const TIER_PROGRAM: &str = r#"
#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>
long syscall(long, ...);
static const uint64_t miss[64] = {@MISS@};
static const uint64_t hit[64] = {@HIT@};
static double d(uint64_t u) { double v; memcpy(&v, &u, sizeof v); return v; }
static uint64_t b(double v) { uint64_t u; memcpy(&u, &v, sizeof u); return u; }
static long fd;
static uint64_t run(const uint64_t *xs) {
    volatile double sink = 0;
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int p = 0; p < 32; p++)
        for (int i = 0; i < 64; i++) sink += exp(d(xs[i]));
    ioctl(fd, 0x2401, 0);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) return 0;
    return n;
}
int main(void) {
    uint64_t attr[16] = {0};
    attr[0] = (uint64_t)64 << 32;
    attr[1] = 1;
    attr[5] = 1 | 1 << 5 | 1 << 6;
    volatile double warm = exp(0.75);
    (void)warm;
    fd = syscall(298, attr, 0, -1, -1, 0);
    if (fd < 0) { printf("noperf %d\n", errno); return 0; }
    uint64_t m = run(miss), h = run(hit);
    printf("%llu %llu\n", (unsigned long long)m, (unsigned long long)h);
    for (int i = 0; i < 64; i++) printf("%016llx\n", (unsigned long long)b(exp(d(miss[i]))));
    return 0;
}
"#;

/// A quick-phase miss through the product stays in the double-double kernels: per call, the miss
/// set may cost at most `TIER2_MARGIN` instructions more than the hit set (tier 2's SSE2 build is
/// about 160 instructions, one call traced on the Zen 1 host). The accurate path also starts with
/// that kernel, so this bound cannot tell tier 2 from the cold envelope plus the accurate path;
/// `exp_quick.rs::exp_quick_misses_are_answered_by_tier_two` checks which tier answers.
const TIER2_MARGIN: u64 = 400;

#[test]
fn libm_product_exp_quick_misses_take_tier_two() {
    // Trace: s10_0_f_0 (§10.2 exp correctly rounded, D-10.3); D-10.51 (the two-phase exp: a quick
    // phase miss goes to tier 2, whose answer is the correctly rounded e^x).
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists() && products.join("libm.a").exists(), "the products are missing: run `cargo xtask so`");
    // Operands from a fixed xorshift sample over [-10, 10] (the bench's range), SSE2 builds.
    let (mut miss, mut hit) = (Vec::new(), Vec::new());
    let mut st: u64 = 0x2545_f491_4f6c_dd1d;
    while miss.len() < 64 || hit.len() < 64 {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        let x = -10.0 + 20.0 * (f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0);
        match (liella_math::fast64::quick::exp_quick(x), liella_math::fast64::exp_nearest(x)) {
            (None, Some(v)) if miss.len() < 64 => miss.push((x, v)),
            (Some(_), _) if hit.len() < 64 => hit.push(x),
            _ => {}
        }
    }
    let list = |v: &mut dyn Iterator<Item = f64>| v.map(|x| format!("0x{:016x}ull", x.to_bits())).collect::<Vec<_>>().join(",");
    let src = TIER_PROGRAM
        .replace("@MISS@", &list(&mut miss.iter().map(|m| m.0)))
        .replace("@HIT@", &list(&mut hit.iter().copied()));
    let dir = root().join("target/test-scratch/libm_fma_select").join(format!("tier-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.c"), src).unwrap();
    let out = Command::new(root().join("scripts/liella-cc"))
        .env("LIELLA_PRODUCTS", &products)
        .env_remove("LIELLA_CC_NO_LIBM")
        .env_remove("LIELLA_CC_EXTRA")
        .current_dir(&dir)
        .args(["-O1", "-fno-builtin", "-o", "t", "t.c", "-lm"])
        .output()
        .unwrap();
    assert!(out.status.success(), "liella-cc failed: {}", String::from_utf8_lossy(&out.stderr));
    let out = Command::new(dir.join("t")).env("GLIBC_TUNABLES", "glibc.cpu.hwcaps=-FMA").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = text.lines();
    let head = lines.next().unwrap_or_default();
    let bits: Vec<&str> = lines.collect();
    if !head.starts_with("noperf") {
        let n: Vec<u64> = head.split(' ').map(|s| s.parse().unwrap_or_else(|_| panic!("unexpected output {head:?}"))).collect();
        let (m, h) = (n[0] / 2048, n[1] / 2048);
        println!("exp instructions per call, SSE2 builds: quick-phase misses {m}, hits {h}");
        assert!(m <= h + TIER2_MARGIN, "exp quick-phase misses cost {m} instructions per call against {h} for hits: a miss did not stop at tier 2");
    } else {
        println!("SKIP (count only): no user-mode instruction counter for this process ({head}): perf_event_open refused");
    }
    assert_eq!(bits.len(), 64, "unexpected output {text:?}");
    for ((x, v), got) in miss.iter().zip(bits) {
        assert_eq!(got, format!("{:016x}", v.to_bits()), "exp({x:e}) after a quick-phase miss: not the correctly rounded result");
    }
}
