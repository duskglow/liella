//! Operand placement as a contract (LAYOUT-PLACEMENT-SWEEP, layout inventory L16, D-X.52): the
//! string kernels whose path depends on where their operands sit in a page (head page tests,
//! the back-up at a page end) must cost about the same wherever the operands are. A kernel edit
//! that sends some placements to a slow path (BENCH-CASECMP-ALIGN-2: a page-aligned start took
//! the generic loop; a pair near a page end took byte steps, 554-1949 instructions where 254
//! was usual) is invisible to the correctness tests and to a bench at one fixed offset.
//!
//! A static program linked against the products counts its own user-mode instructions
//! (`perf_event_open`, self-monitoring, as in `libm_fma_select.rs`) over `REPS` calls of each
//! kernel at one fixed length, with each operand at every page offset in `OFFSETS` (and every
//! pair of them for the two-operand kernels). Instruction counts of a fixed loop are
//! deterministic; no wall clock is involved. Two bounds per kernel:
//! - all placements: the worst costs at most K_all percent of the median (`BOUNDS`);
//! - head placements (every operand at a page offset of 32 or less, so the measured bytes lie in
//!   one page and no page test should change the path): the worst costs at most K_head percent
//!   of their median (`BOUNDS`; this one sees a page-aligned start sent to a slower loop).
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Page offsets of each operand (the brief's set: starts at and just after a page start, at
/// 16- and 32-byte steps, and in the last 64 bytes of a page down to the last byte).
const OFFSETS: [u32; 11] = [0, 1, 15, 16, 31, 32, 4032, 4064, 4065, 4080, 4095];

/// The largest head offset (operands at or below it keep their measured bytes in one page).
const HEAD_MAX: u32 = 32;

/// Per kernel: (name, worst over median for all placements in percent, the same for head
/// placements in percent). K is the kernel's measured spread on origin/next 03c9a4972 (mei,
/// Xeon 8151, AVX2 kernels; D-X.52) plus about 10 percent, rounded up to 5. The spreads differ
/// by kernel: strcmp, strncmp and strncasecmp already take longer paths near a page end
/// (426, 470 and 527 percent measured), so one K for all would not see a casecmp regression
/// (280 percent measured; 645 before BENCH-CASECMP-ALIGN-2). Bounds only tighten (owner 11-07).
const BOUNDS: [(&str, u64, u64); 9] = [
    ("memchr", 115, 110),      // measured 102 / 100
    ("memcmp", 110, 110),      // measured 100 / 100
    ("rawmemchr", 115, 110),   // measured 104 / 100
    ("strcasecmp", 310, 115),  // measured 280 / 104
    ("strcmp", 470, 140),      // measured 426 / 130
    ("strlen", 140, 110),      // measured 125 / 100
    ("strncasecmp", 580, 125), // measured 527 / 113
    ("strncmp", 520, 140),     // measured 470 / 128
    ("strnlen", 140, 110),     // measured 126 / 100
];

// `perf_event_attr` written as words (PERF_ATTR_SIZE_VER0 = 64 bytes): type PERF_TYPE_HARDWARE
// (0) and size in word 0, config PERF_COUNT_HW_INSTRUCTIONS (1), flags disabled |
// exclude_kernel | exclude_hv in word 5. 298 = SYS_perf_event_open on x86-64; the ioctls are
// PERF_EVENT_IOC_ENABLE/DISABLE/RESET. Each operand has two pages of its own (a string at page
// offset 4095 runs into the second). Output: `kernel a_off b_off instructions_per_call`, b_off
// -1 for one-operand kernels.
const PROGRAM: &str = r#"
#define _GNU_SOURCE
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <strings.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>
long syscall(long, ...);
#define REPS 64
#define LEN 256
#define NLEN 64
static const unsigned offs[] = {0, 1, 15, 16, 31, 32, 4032, 4064, 4065, 4080, 4095};
#define NOFF (sizeof offs / sizeof offs[0])
static int (*volatile f_casecmp)(const char *, const char *) = strcasecmp;
static int (*volatile f_ncasecmp)(const char *, const char *, size_t) = strncasecmp;
static int (*volatile f_strcmp)(const char *, const char *) = strcmp;
static int (*volatile f_strncmp)(const char *, const char *, size_t) = strncmp;
static int (*volatile f_memcmp)(const void *, const void *, size_t) = memcmp;
static size_t (*volatile f_strlen)(const char *) = strlen;
static size_t (*volatile f_strnlen)(const char *, size_t) = strnlen;
static void *(*volatile f_memchr)(const void *, int, size_t) = memchr;
static void *(*volatile f_rawmemchr)(const void *, int) = rawmemchr;
static long fd;
static volatile long sink;
static char *A, *B;
static long call(int k, const char *a, const char *b) {
    switch (k) {
    case 0: return f_casecmp(a, b);
    case 1: return f_ncasecmp(a, b, NLEN);
    case 2: return f_strcmp(a, b);
    case 3: return f_strncmp(a, b, NLEN);
    case 4: return f_memcmp(a, b, LEN);
    case 5: return (long)f_strlen(a);
    case 6: return (long)f_strnlen(a, LEN + 64);
    case 7: return (long)(f_memchr(a, '#', LEN) != 0);
    default: return (long)(f_rawmemchr(a, '#') != 0);
    }
}
static const char *names[] = {"strcasecmp", "strncasecmp", "strcmp", "strncmp", "memcmp",
                              "strlen", "strnlen", "memchr", "rawmemchr"};
static void place(int k, char *a, char *b) {
    for (int i = 0; i < LEN; i++) {
        a[i] = (char)('a' + i % 26);
        b[i] = k <= 1 ? (char)('A' + i % 26) : a[i];
    }
    a[LEN] = k == 8 ? '#' : 0;
    b[LEN] = 0;
}
static uint64_t measure(int k, const char *a, const char *b) {
    sink = call(k, a, b);
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int r = 0; r < REPS; r++) sink = call(k, a, b);
    ioctl(fd, 0x2401, 0);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) { printf("noperf read %d\n", errno); _exit(0); }
    return (n + REPS / 2) / REPS;
}
int main(void) {
    uint64_t attr[16] = {0};
    attr[0] = (uint64_t)64 << 32;
    attr[1] = 1;
    attr[5] = 1 | 1 << 5 | 1 << 6;
    fd = syscall(298, attr, 0, -1, -1, 0);
    if (fd < 0) { printf("noperf %d\n", errno); return 0; }
    char *m = mmap(0, 4 * 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED) { printf("mmap %d\n", errno); return 1; }
    A = m;
    B = m + 2 * 4096;
    for (int k = 0; k < 9; k++) {
        int pair = k <= 4;
        for (unsigned i = 0; i < NOFF; i++) {
            for (unsigned j = 0; j < (pair ? NOFF : 1); j++) {
                char *a = A + offs[i], *b = B + offs[j];
                place(k, a, b);
                printf("%s %u %d %llu\n", names[k], offs[i], pair ? (int)offs[j] : -1,
                       (unsigned long long)measure(k, a, b));
            }
        }
    }
    return 0;
}
"#;

/// One measured placement: page offsets of the operands (b `None` for one-operand kernels).
struct Row {
    a: u32,
    b: Option<u32>,
    n: u64,
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

#[test]
fn string_kernels_cost_about_the_same_at_every_operand_page_offset() {
    // Trace: no spec clause (a cost property of the kernels; D-X.52, layout inventory L16).
    if !std::arch::is_x86_feature_detected!("avx2") {
        println!("SKIP: no AVX2: the bounds were measured for the AVX2 kernels");
        return;
    }
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = root().join("target/test-scratch/placement_sweep").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.c"), PROGRAM).unwrap();
    let out = Command::new(root().join("scripts/liella-cc"))
        .env("LIELLA_PRODUCTS", &products)
        .env_remove("LIELLA_CC_EXTRA")
        .current_dir(&dir)
        .args(["-O1", "-fno-builtin", "-o", "p", "p.c"])
        .output()
        .unwrap();
    assert!(out.status.success(), "liella-cc failed: {}", String::from_utf8_lossy(&out.stderr));
    let out = Command::new(dir.join("p")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.starts_with("noperf") {
        println!("SKIP: no user-mode instruction counter for this process ({}): perf_event_open refused", text.trim());
        return;
    }
    let mut rows: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 4, "unexpected output line {line:?}");
        let b: i64 = f[2].parse().unwrap();
        rows.entry(f[0].to_string()).or_default().push(Row {
            a: f[1].parse().unwrap(),
            b: u32::try_from(b).ok(),
            n: f[3].parse().unwrap(),
        });
    }
    assert_eq!(rows.len(), 9, "expected nine kernels, got {:?}", rows.keys().collect::<Vec<_>>());
    let place = |r: &Row| r.b.map_or_else(|| format!("{}", r.a), |b| format!("{}/{}", r.a, b));
    let mut failures = Vec::new();
    for (k, v) in &rows {
        let &(_, k_all, k_head) = BOUNDS.iter().find(|b| b.0 == k).unwrap_or_else(|| panic!("{k}: no bound"));
        let expect = if v[0].b.is_some() { OFFSETS.len() * OFFSETS.len() } else { OFFSETS.len() };
        assert_eq!(v.len(), expect, "{k}: {} placements, expected {expect}", v.len());
        let mut all: Vec<u64> = v.iter().map(|r| r.n).collect();
        let med = median(&mut all);
        let worst = v.iter().max_by_key(|r| r.n).unwrap();
        let head: Vec<&Row> = v.iter().filter(|r| r.a <= HEAD_MAX && r.b.is_none_or(|b| b <= HEAD_MAX)).collect();
        let mut hn: Vec<u64> = head.iter().map(|r| r.n).collect();
        let hmed = median(&mut hn);
        let hworst = head.iter().max_by_key(|r| r.n).unwrap();
        println!(
            "{k}: median {med}, worst {} at {} ({}%); head median {hmed}, head worst {} at {} ({}%)",
            worst.n,
            place(worst),
            worst.n * 100 / med,
            hworst.n,
            place(hworst),
            hworst.n * 100 / hmed
        );
        if worst.n * 100 > k_all * med {
            failures.push(format!("{k}: {} instructions at {} against a median of {med} (bound {k_all}%)", worst.n, place(worst)));
        }
        if hworst.n * 100 > k_head * hmed {
            failures.push(format!("{k}: {} instructions at head placement {} against a head median of {hmed} (bound {k_head}%)", hworst.n, place(hworst)));
        }
    }
    assert!(failures.is_empty(), "placement-dependent cost (a slow path at some page offsets):\n{}", failures.join("\n"));
}
