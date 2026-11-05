//! The libm product runs the FMA builds of the `double` exp/log fast paths on a CPU with FMA, and
//! `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA` keeps the baseline (D-10.25, D-NEW, #2920): `libm.a`
//! carries its own copy of the dispatch byte, which its first math call decides. Both builds
//! return the same bits, so the only observable is the work done: a static program linked with
//! `-lm` counts its own user-mode instructions over a fixed loop of `exp` and `log` calls
//! (`perf_event_open`, self-monitoring), once with the tunable and once without. Instruction
//! counts of a fixed loop are deterministic; no wall clock is involved.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

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
    // Trace: no spec clause (D-10.25 run-time FMA builds; D-NEW the libm product's own decision).
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
