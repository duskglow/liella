//! The integrated name-service cache end to end in the products (D-16.10):
//! `tests/oracle/nscache_check.c`, a self-checking program (the reference has no such cache, so
//! this is not a differential test) linked statically against Liella's products and run by
//! `tests/oracle/nscache_run.sh` as root in private mount and network namespaces with their own
//! `/dev/shm`, `/mnt` and `/etc` (no region of the system's `/dev/shm` is used or left). It
//! covers each requirement of D-16.10: a process's first lookup goes to the sources; `shadow` and
//! `gshadow` never enter the table; fresh processes see edits at once; global and per-category
//! files invalidate what they should; the rate limit, `liella_nscache_invalidate` and generations
//! seen without `stat`; keys and per-process steps (environment, locale and IDNA,
//! `AI_ADDRCONFIG`, overrides, a modified `_res`) not leaking; squatting, corruption and killed
//! writers; `AT_SECURE`, differing IDs, credential changes, `unshare(CLONE_NEWUSER)` and `setns` (a user
//! namespace never shares a region); the standard descriptors closed (output still `EBADF`);
//! UID reuse; the size cap and eviction; DNS TTLs (a hit within the TTL, expiry, TTL 0, RFC 2308
//! negative answers, the one-hour and five-minute caps read from the region); disabling. Needs the products
//! (`cargo xtask so`), a C compiler and passwordless `sudo`; without the latter two the test says
//! so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

#[path = "support/timeout.rs"]
mod timeout;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The checks the program makes (each prints `ok NAME`); a check missing from the output fails.
const CHECKS: &[&str] = &[
    "first-lookup-uses-the-sources",
    "shared-region",
    "region-file",
    "hit",
    "shadow-never-cached",
    "new-process-sees-edit",
    "global-file-invalidates-all /etc/nsswitch.conf",
    "global-file-invalidates-all /etc/host.conf",
    "global-file-invalidates-all /etc/resolv.conf",
    "global-file-invalidates-all /etc/gai.conf",
    "category-file-invalidates-its-own /etc/hosts",
    "category-file-invalidates-its-own /etc/passwd",
    "category-file-invalidates-its-own /etc/group",
    "category-file-invalidates-its-own /etc/services",
    "category-file-invalidates-its-own /etc/protocols",
    "category-file-invalidates-its-own /etc/networks",
    "file-disappears",
    "file-appears",
    "no-stat-within-the-window",
    "shared-generation-seen-without-stat",
    "invalidate-api",
    "invalidate-bypasses-the-rate-limit",
    "key-localdomain",
    "key-res-options",
    "key-hostaliases",
    "idna-after-the-cache",
    "addrconfig-after-the-cache",
    "overrides-bypass",
    "modified-res-bypasses",
    "dns-ttl-expiry",
    "dns-ttl-zero-never-cached",
    "dns-negative-per-rfc2308",
    "dns-negative-without-soa-not-cached",
    "dns-shared",
    "dns-ttl-caps",
    "squat-other-owner",
    "squat-wrong-mode",
    "squat-symlink",
    "squat-fifo",
    "squat-oversize",
    "squat-directory",
    "squat-symlink-target-untouched",
    "corrupt-random-bytes",
    "corrupt-truncated-small",
    "corrupt-truncated-to-smaller-table",
    "corrupt-bad-offsets",
    "corrupt-bad-checksums",
    "corrupt-writer-killed-mid-write",
    "corrupt-bad-header",
    "corrupt-writer-sigkilled",
    "at-secure-private",
    "ruid-differs-private",
    "credential-change-rekeys",
    "uid-reuse-validity-stamp",
    "unshare-newuser-rekeys",
    "setns-user-rekeys",
    "standard-descriptors-closed",
    "size-cap-and-eviction",
    "disabled-no-file",
    "native-enable-and-categories",
];

#[test]
fn the_cache_meets_d_16_10() {
    // Trace: no spec clause (the integrated name-service cache is Liella's own, self-checked); D-16.10.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the checks need private mount and network namespaces)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("nscache_check");
    let mut cc = Command::new("cc");
    cc.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
        .arg(r.join("target/liella/liella-crt1.o"))
        .arg("-I")
        .arg(r.join("include"))
        .arg(r.join("crates/core/tests/oracle/nscache_check.c"))
        .arg(r.join("target/liella/libc.a"))
        .arg("-lgcc");
    let built = oracle::build(cc, &exe);
    assert!(built, "nscache_check.c does not link against the products");
    let o = Command::new("timeout")
        .arg(timeout::secs(400))
        .arg("sh")
        .arg(r.join("crates/core/tests/oracle/nscache_run.sh"))
        .arg(&exe)
        .arg(dir.join("nscache"))
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    eprintln!("{out}{}", String::from_utf8_lossy(&o.stderr));
    let failed: Vec<&str> = out.lines().filter(|l| l.starts_with("FAIL")).collect();
    assert!(failed.is_empty(), "failed checks:\n{}", failed.join("\n"));
    for c in CHECKS {
        let ok = format!("ok {c}");
        assert!(out.lines().any(|l| l == ok || l.starts_with(&format!("{ok} ("))), "check not made: {c}");
    }
    let basics = out.lines().filter(|l| l.starts_with("ok cached-equals-uncached")).count();
    assert_eq!(basics, 15, "every category answered the same with and without the cache");
    assert!(o.status.success(), "status {:?}", o.status);
}
