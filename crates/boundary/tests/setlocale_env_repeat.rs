//! `setlocale(cat, "")` takes each category's name from the environment on every call (s7_1_c_0:
//! `LC_ALL`, then `LC_<cat>`, then `LANG`), and a category whose current name is the name selected
//! is not looked up again (s7_2_c_4). SETLOCALE-CTYPE-ENV answers that repeated call without the
//! lock when nothing changes, and reads only the variables the category needs: these checks
//! prove that the environment is still read every time (a changed `LANG` or `LC_ALL` between two
//! calls is seen) and that the repeated call stays on the no-change path (its cost against the
//! query). A static program linked against the products counts its own user-mode instructions
//! over fixed loops (`perf_event_open`, self-monitoring, as in `setlocale_posix_repeat.rs`). Only
//! built-in locales are used (no locale files). Instruction counts of a fixed loop are
//! deterministic; no wall clock is involved.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Calls in each counted loop.
const CALLS: u64 = 2000;

/// The most instructions per call `setlocale(cat, "")` asked again may cost over
/// `setlocale(cat, NULL)` asked again, with the eight-entry environment below: the walk over it
/// and the selection. The locked path it must not take cost about 1,000 more.
const ENV_EXTRA: u64 = 400;

// `perf_event_attr` as in setlocale_posix_repeat.rs. The program starts with an empty environment
// and sets its own. Output: "<failed check, 0 for none> <ctype ""> <all ""> <ctype NULL> <all NULL>"
// (instructions per loop), or "<failed check> noperf ...".
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
static void want(int check, int cat, const char *name, size_t mb) {
    const char *r = setlocale(cat, "");
    if (!bad && (!r || strcmp(r, name) != 0 || MB_CUR_MAX != mb)) bad = check;
}
static uint64_t count(int cat, const char *name) {
    ioctl(fd, 0x2403, 0);
    ioctl(fd, 0x2400, 0);
    for (int i = 0; i < CALLS; i++) sink += (uintptr_t)setlocale(cat, name);
    ioctl(fd, 0x2401, 0);
    uint64_t n = 0;
    if (read(fd, &n, sizeof n) != sizeof n) { printf("%d noperf read %d\n", bad, errno); exit(0); }
    return n;
}
int main(void) {
    const char *comp;
    setenv("PATH", "/usr/bin:/bin", 1);
    setenv("HOME", "/nonexistent", 1);
    setenv("LOGNAME", "u", 1);
    setenv("LS_COLORS", "di=01;34", 1);
    setenv("LESS", "-R", 1);
    setenv("LC_ALLX", "C.UTF-8", 1);
    setenv("LANGUAGE", "C.UTF-8", 1);
    setenv("LANG", "C", 1);
    want(1, LC_CTYPE, "C", 1);
    want(2, LC_CTYPE, "C", 1);
    /* LANG changed between two calls: the new value is seen. */
    setenv("LANG", "C.UTF-8", 1);
    want(3, LC_CTYPE, "C.UTF-8", 6);
    want(4, LC_CTYPE, "C.UTF-8", 6);
    setenv("LANG", "POSIX", 1);
    want(5, LC_CTYPE, "C", 1);
    setenv("LANG", "C.UTF-8", 1);
    want(6, LC_ALL, "C.UTF-8", 6);
    want(7, LC_ALL, "C.UTF-8", 6);
    /* LC_ALL decides every category, and is read again on each call too. */
    setenv("LC_ALL", "C", 1);
    want(8, LC_ALL, "C", 1);
    want(9, LC_CTYPE, "C", 1);
    unsetenv("LC_ALL");
    want(10, LC_ALL, "C.UTF-8", 6);
    setenv("LC_ALL", "", 1);
    want(11, LC_CTYPE, "C.UTF-8", 6);
    /* LC_<cat> over LANG: a composite name for LC_ALL, then each category its own name. */
    setenv("LC_CTYPE", "C", 1);
    comp = setlocale(LC_ALL, "");
    if (!bad && (!comp || strcmp(comp, "C.UTF-8") == 0 || MB_CUR_MAX != 1 || strcmp(setlocale(LC_CTYPE, NULL), "C") != 0
                 || strcmp(setlocale(LC_NUMERIC, NULL), "C.UTF-8") != 0)) bad = 12;
    unsetenv("LC_CTYPE");
    want(13, LC_ALL, "C.UTF-8", 6);
    uint64_t attr[16] = {0};
    attr[0] = (uint64_t)64 << 32;
    attr[1] = 1;
    attr[5] = 1 | 1 << 5 | 1 << 6;
    fd = syscall(298, attr, 0, -1, -1, 0);
    if (fd < 0) { printf("%d noperf %d\n", bad, errno); return 0; }
    uint64_t ctype = count(LC_CTYPE, ""), all = count(LC_ALL, "");
    uint64_t q = count(LC_CTYPE, NULL), all_q = count(LC_ALL, NULL);
    want(14, LC_ALL, "C.UTF-8", 6);
    printf("%d %llu %llu %llu %llu\n", bad, (unsigned long long)ctype, (unsigned long long)all, (unsigned long long)q,
           (unsigned long long)all_q);
    return 0;
}
"#;

#[test]
fn setlocale_env_is_read_on_every_call_and_again_takes_the_no_change_path() {
    // Trace: s7_1_c_0 (the environment's selection), s7_2_c_4 (a category already named so is not
    // looked up again), s7_1_c_p1 ("POSIX" is the built-in "C").
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = root().join("target/test-scratch/setlocale_env_repeat").join(format!("{}", std::process::id()));
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
    let bad: u64 = text.split(' ').next().and_then(|w| w.parse().ok()).unwrap_or_else(|| panic!("unexpected output {text:?}"));
    assert_eq!(bad, 0, "setlocale_env_repeat: check {bad} failed: setlocale(cat, \"\") did not select what the environment names now");
    if text.contains("noperf") {
        println!("SKIP (counts only): no user-mode instruction counter for this process ({text}): perf_event_open refused");
        return;
    }
    let v: Vec<u64> = text.split(' ').map(|w| w.parse().unwrap_or_else(|_| panic!("unexpected output {text:?}"))).collect();
    let [_, ctype, all, q, all_q] = v[..] else { panic!("unexpected output {text:?}") };
    println!(
        "user instructions per call over {CALLS} calls: LC_CTYPE \"\" {}, NULL {}; LC_ALL \"\" {}, NULL {}",
        ctype / CALLS,
        q / CALLS,
        all / CALLS,
        all_q / CALLS
    );
    for (what, query, asked) in [("LC_CTYPE", q, ctype), ("LC_ALL", all_q, all)] {
        assert!(
            asked <= query + ENV_EXTRA * CALLS,
            "setlocale_env_repeat: {what} \"\" again {} instructions per call, the query again {}: left the no-change path",
            asked / CALLS,
            query / CALLS
        );
    }
}
