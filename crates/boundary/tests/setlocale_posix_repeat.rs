//! `setlocale(cat, "POSIX")` asked again for the locale a category already has costs what
//! `setlocale(cat, "C")` costs (SETLOCALE-POSIX-ATTR). `"POSIX"` selects the built-in locale that
//! reports itself as `"C"` (s7_1_c_p1), and a category whose current name is the name asked for is
//! not looked up again (s7_2_c_4). Comparing the name as given with the reported `"C"` missed that
//! check on every call: the whole locale was rebuilt, compared and copied under the lock (about
//! 2,300 instructions more per call, part of it in memmove, whose count followed stack alignment).
//! The result is the same either way, so the only observable is the work done: a static program
//! linked against the products counts its own user-mode instructions over fixed loops
//! (`perf_event_open`, self-monitoring, as in `libm_fma_select.rs`). Only built-in locales are
//! used (no locale files, no environment). Instruction counts of a fixed loop are deterministic;
//! no wall clock is involved.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Calls in each counted loop.
const CALLS: u64 = 2000;

/// The most instructions per call `setlocale(cat, "C")` asked again may cost over
/// `setlocale(cat, NULL)` asked again (the name's handling in the boundary and the inline
/// compare): 75 measured on a Zen 1 host for both categories (fast profile), plus 10 for string
/// kernels that differ by CPU class. A library call for the compare cost 20-35 more.
const NAME_EXTRA: u64 = 85;

// `perf_event_attr` as words (PERF_ATTR_SIZE_VER0 = 64 bytes): PERF_TYPE_HARDWARE (0) and the size
// in word 0, PERF_COUNT_HW_INSTRUCTIONS (1), disabled | exclude_kernel | exclude_hv in word 5.
// 298 = SYS_perf_event_open on x86-64; the ioctls are PERF_EVENT_IOC_ENABLE/DISABLE/RESET.
// Each loop starts from C.UTF-8, makes one call that changes the category (not counted), then
// counts CALLS calls that ask for the name it now has. Output: "<checks> <c> <posix> <all_c>
// <all_posix>" (instructions per loop), or "noperf ...".
const PROGRAM: &str = r#"
#include <errno.h>
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>
long syscall(long, ...);
static long fd;
static int bad;
static uintptr_t sink;
static uint64_t loop(int cat, const char *name) {
    if (!setlocale(LC_ALL, "C.UTF-8")) { printf("no C.UTF-8\n"); exit(1); }
    const char *r = setlocale(cat, name);
    if (!r || strcmp(r, "C") != 0 || MB_CUR_MAX != 1) bad++;
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int i = 0; i < CALLS; i++) sink += (uintptr_t)setlocale(cat, name);
    ioctl(fd, 0x2401, 0);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) { printf("noperf read %d\n", errno); exit(0); }
    r = setlocale(cat, name);
    if (!r || strcmp(r, "C") != 0 || MB_CUR_MAX != 1) bad++;
    return n;
}
static uint64_t query(int cat) {
    if (!setlocale(LC_ALL, "C")) { printf("no C\n"); exit(1); }
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int i = 0; i < CALLS; i++) sink += (uintptr_t)setlocale(cat, NULL);
    ioctl(fd, 0x2401, 0);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) { printf("noperf read %d\n", errno); exit(0); }
    return n;
}
int main(void) {
    uint64_t attr[16] = {0};
    attr[0] = (uint64_t)64 << 32;
    attr[1] = 1;
    attr[5] = 1 | 1 << 5 | 1 << 6;
    fd = syscall(298, attr, 0, -1, -1, 0);
    if (fd < 0) { printf("noperf %d\n", errno); return 0; }
    uint64_t c = loop(LC_CTYPE, "C"), posix = loop(LC_CTYPE, "POSIX");
    uint64_t all_c = loop(LC_ALL, "C"), all_posix = loop(LC_ALL, "POSIX");
    uint64_t q = query(LC_CTYPE), all_q = query(LC_ALL);
    printf("%d %llu %llu %llu %llu %llu %llu\n", bad, (unsigned long long)c, (unsigned long long)posix,
           (unsigned long long)all_c, (unsigned long long)all_posix, (unsigned long long)q,
           (unsigned long long)all_q);
    return 0;
}
"#;

#[test]
fn setlocale_posix_again_costs_what_c_costs() {
    // Trace: s7_2_c_4 (a category already named so is not looked up again) and s7_1_c_p1 ("POSIX"
    // is the built-in "C"); the bound is a cost property (SETLOCALE-POSIX-ATTR).
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = root().join("target/test-scratch/setlocale_posix_repeat").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.c"), PROGRAM.replace("CALLS", &CALLS.to_string())).unwrap();
    let out = Command::new(root().join("scripts/liella-cc"))
        .env("LIELLA_PRODUCTS", &products)
        .env_remove("LIELLA_CC_EXTRA")
        .current_dir(&dir)
        .args(["-O1", "-o", "p", "p.c"])
        .output()
        .unwrap();
    assert!(out.status.success(), "liella-cc failed: {}", String::from_utf8_lossy(&out.stderr));
    let out = Command::new(dir.join("p")).env_clear().output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.starts_with("noperf") {
        println!("SKIP: no user-mode instruction counter for this process ({text}): perf_event_open refused");
        return;
    }
    let v: Vec<u64> = text.split(' ').map(|w| w.parse().unwrap_or_else(|_| panic!("unexpected output {text:?}"))).collect();
    let [bad, c, posix, all_c, all_posix, q, all_q] = v[..] else { panic!("unexpected output {text:?}") };
    println!(
        "user instructions per call over {CALLS} calls: LC_CTYPE \"C\" {}, \"POSIX\" {}, NULL {}; LC_ALL \"C\" {}, \"POSIX\" {}, NULL {}",
        c / CALLS,
        posix / CALLS,
        q / CALLS,
        all_c / CALLS,
        all_posix / CALLS,
        all_q / CALLS
    );
    assert_eq!(bad, 0, "setlocale(cat, \"C\"/\"POSIX\") from C.UTF-8 did not report \"C\" with MB_CUR_MAX 1");
    // "POSIX" may cost its longer name (a few instructions per call), never another path: at most
    // a quarter more than "C" plus 8 per call (the miss was more than ten times "C").
    for (what, plain, alias) in [("LC_CTYPE", c, posix), ("LC_ALL", all_c, all_posix)] {
        let limit = plain + plain / 4 + 8 * CALLS;
        assert!(
            alias <= limit,
            "setlocale_posix_repeat: {what} \"POSIX\" again {} instructions per call, \"C\" again {}: \"POSIX\" left the no-change path",
            alias / CALLS,
            plain / CALLS
        );
    }
    // "C" again is answered from the query cache like the query itself: it may add only the
    // name's own handling (its length, the no-change compare made inline), never a library call
    // for the compare or the locked path's frame (SETLOCALE-C-ROWS: those were 20-35 per call).
    for (what, query, asked) in [("LC_CTYPE", q, c), ("LC_ALL", all_q, all_c)] {
        assert!(
            asked <= query + NAME_EXTRA * CALLS,
            "setlocale_posix_repeat: {what} \"C\" again {} instructions per call, the query again {}: more than {NAME_EXTRA} for the name",
            asked / CALLS,
            query / CALLS
        );
    }
}
