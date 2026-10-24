//! The resolver's environment in secure-execution mode (owner ruling 2026-10-03;
//! `spec/security/threat-model.md`): `tests/oracle/secure_env.c`, built against the host C library
//! and statically against Liella's products, runs with `HOSTALIASES`, `RES_OPTIONS` and
//! `LOCALDOMAIN` set, as a set-group-ID copy (the kernel sets `AT_SECURE`) and plainly. The
//! secure runs must ignore all three, the plain runs honour them, and both libraries must agree.
//! The set-group-ID copy needs no privilege: it uses a supplementary group of the user running
//! the test; without one (or without a C compiler) the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/secure_env.c");
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

/// A supplementary group of this user other than its effective group, if any.
fn other_group() -> Option<String> {
    let id = |a: &str| String::from_utf8(Command::new("id").arg(a).output().ok()?.stdout).ok();
    let egid = id("-g")?.trim().to_owned();
    id("-G")?.split_whitespace().find(|g| *g != egid).map(str::to_owned)
}

fn run(exe: &Path, aliases: &Path) -> String {
    let o = Command::new(exe)
        .env_clear()
        .env("HOSTALIASES", aliases)
        .env("RES_OPTIONS", "ndots:7")
        .env("LOCALDOMAIN", "evil.test")
        .env("LIELLA_NSCACHE", "0")
        .output()
        .unwrap();
    assert!(o.status.success(), "{} failed", exe.display());
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn secure_processes_ignore_the_resolver_environment() {
    // Clauses: s1_1_g_4, s1_1_c_7, s1_3_c_2, s16_4_c_1_3 (owner ruling 2026-10-03).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let Some(group) = other_group() else {
        skip_note::skipped("skipped: this user has no supplementary group for a set-group-ID copy");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("secure-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let aliases = dir.join("aliases");
    std::fs::write(&aliases, "short evil.example\n").unwrap();
    let mut outs = Vec::new();
    for liella in [false, true] {
        let exe = dir.join(if liella { "liella" } else { "host" });
        assert!(build(&exe, liella), "secure_env.c does not build (liella: {liella})");
        let plain = run(&exe, &aliases);
        assert!(Command::new("chgrp").arg(&group).arg(&exe).status().unwrap().success());
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o2755)).unwrap();
        let secure = run(&exe, &aliases);
        assert!(secure.starts_with("AT_SECURE=1\n"), "the copy did not run in secure mode:\n{secure}");
        assert_eq!(secure, "AT_SECURE=1\nndots=1\nsearch[0] is LOCALDOMAIN's: no\nhostalias(short)=(null)\n", "liella: {liella}");
        assert_eq!(plain, "AT_SECURE=0\nndots=7\nsearch[0] is LOCALDOMAIN's: yes\nhostalias(short)=evil.example\n", "liella: {liella}");
        outs.push((plain, secure));
    }
    assert_eq!(outs[0], outs[1], "Liella and the reference agree");
    let _ = std::fs::remove_dir_all(&dir);
}
