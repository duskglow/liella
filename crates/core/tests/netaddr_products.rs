//! Address conversion, interfaces, IPv6 options and Ethernet addresses end to end (spec
//! §15.3–15.8; D-15.1–D-15.5): `tests/oracle/netaddr_diff.c` built against the host C library
//! (with `-lresolv`, where `inet_net_pton`, `inet_net_ntop` and `inet_neta` live) and statically
//! against Liella's products (`libc.a`, with the empty `libresolv.a` for `-lresolv`, D-16.6);
//! the outputs must be identical line by line, except the recorded differences in [`KNOWN`].
//! Needs the products (`cargo xtask so`) and a C compiler; without the compiler the test says
//! so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/netaddr_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
            .arg(r.join("target/liella/liella-crt1.o"))
            .arg(&src)
            .arg(r.join("target/liella/libc.a"))
            .arg("-L")
            .arg(r.join("target/liella"))
            .args(["-lresolv", "-lgcc"]);
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src).arg("-lresolv");
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: text the line starts with, and why. A
/// recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 1] = [(
    "[4294967296] aton=0",
    "inet_network: the reference reads a part beyond 32 bits modulo 2^32 (4294967296 is 0, an overflow); every part of a network number is one byte, so Liella returns -1, as the reference does for 0x7f000001 and 1.2.65535",
)];

#[test]
fn netaddr_equals_the_reference() {
    // Clauses: s15_3_c_1, s15_3_c_2, s15_3_c_3, s15_3_c_4, s15_3_c2_0, s15_4_c_1, s15_4_c_2, s15_4_c_3, s15_5_c_0,
    // s15_6_c_0, s15_7_c_0, s15_8_c_3 (D-15.1 to D-15.5).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    assert!(r.join("target/liella/libresolv.a").exists(), "the libresolv placeholder is missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("netaddr_diff_host"), dir.join("netaddr_diff_liella"));
    assert!(build(&host, false), "netaddr_diff.c does not build against the host C library");
    assert!(build(&ours, true), "netaddr_diff.c does not link against the products");
    // The name-service cache (D-16.10) on and off.
    let run = |exe: &Path, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(60)).arg(exe).env_clear();
        // The program makes no cached lookup, so no region is made: only the variable differs.
        if mode == Some("off") {
            cmd.env("LIELLA_NSCACHE", "0");
        }
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, None);
    // Liella with the name-service cache on and off (D-16.10).
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let (want, got) = (reference.clone(), run(&ours, Some(mode)));
        let mut diffs = 0;
        let mut seen = [false; KNOWN.len()];
        for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
            if let Some(i) = KNOWN.iter().position(|(needle, _)| w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", KNOWN[i].1);
                }
                continue;
            }
            if w != g {
                diffs += 1;
                if diffs <= 40 {
                    eprintln!("line {}:\n  glibc  {w}\n  liella {g}", k + 1);
                }
            }
        }
        assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
        }
    }
}
