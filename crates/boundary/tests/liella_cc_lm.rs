//! `scripts/liella-cc` links Liella's `libm.a` for `-lm` (GAWK-LM-CHECK-2): a program using the
//! maths library links and runs with `-lm`, and without it the maths names stay undefined, as
//! with the system compiler and glibc (whose `libc.a` does not define them).
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const PROGRAM: &str = r#"
#include <math.h>
#include <stdio.h>
int main(int argc, char **argv) {
    volatile double x = argc + 1.5;
    (void)argv;
    printf("%.6f %.6f %.6f %.6f\n", fmod(x * 7.0, x), pow(x, 3.0), sin(x), exp(x));
    return 0;
}
"#;

fn scratch() -> PathBuf {
    let dir = root().join("target/test-scratch/liella_cc_lm").join(format!("{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.c"), PROGRAM).unwrap();
    dir
}

fn link(dir: &PathBuf, out: &str, lm: bool) -> std::process::Output {
    let products = std::env::var_os("LIELLA_PRODUCTS").map_or_else(|| root().join("target/liella"), PathBuf::from);
    assert!(products.join("libc.a").exists() && products.join("libm.a").exists(), "the products are missing: run `cargo xtask so`");
    let mut cmd = Command::new(root().join("scripts/liella-cc"));
    cmd.env("LIELLA_PRODUCTS", &products).env_remove("LIELLA_CC_NO_LIBM").env_remove("LIELLA_CC_EXTRA");
    cmd.current_dir(dir).args(["-O1", "-o", out, "m.c"]);
    if lm {
        cmd.arg("-lm");
    }
    cmd.output().unwrap()
}

#[test]
fn liella_cc_links_libm_for_lm() {
    // Trace: no spec clause (validation driver scripts/liella-cc; validation/README.md T34).
    let dir = scratch();
    let out = link(&dir, "m", true);
    assert!(out.status.success(), "liella-cc ... -lm failed: {}", String::from_utf8_lossy(&out.stderr));
    let run = Command::new(dir.join("m")).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "0.000000 15.625000 0.598472 12.182494\n");
}

#[test]
fn liella_cc_without_lm_leaves_maths_undefined() {
    // Trace: no spec clause (validation driver scripts/liella-cc; T34: libc.a holds no fmod, pow, sin, exp).
    let dir = scratch();
    let out = link(&dir, "nolm", false);
    assert!(!out.status.success(), "linked without -lm: libc.a defines a maths name glibc's libc does not");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("undefined reference to `fmod'") || err.contains("undefined reference to `pow'"), "{err}");
}
