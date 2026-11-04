//! `RESOLV_HOST_CONF` end to end in the products against the reference (D-16.13). No spec clause:
//! §16 is silent on `host.conf`'s environment overrides. `tests/oracle/hostconf_env.c` is built
//! against the host C library and statically against Liella's products and run in a private user
//! and mount namespace (`unshare -r -m`, no root) with its own `/etc/hosts` and
//! `/etc/nsswitch.conf` bound over the system's; the variable names a file in the scratch
//! directory (`multi on`, `multi off`) or a path that cannot be opened (observed: the defaults).
//! Both libraries must print the same addresses. Needs the products (`cargo xtask so`), a C
//! compiler and unprivileged user namespaces; without them the test says so and passes.
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
    let src = r.join("crates/core/tests/oracle/hostconf_env.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// The program's output with `RESOLV_HOST_CONF` set to `conf` (`None`: not set).
fn run(exe: &Path, dir: &Path, conf: Option<&str>) -> String {
    let script = r#"mount --bind "$1/hosts" /etc/hosts && mount --bind "$1/nsswitch.conf" /etc/nsswitch.conf && shift && exec "$@""#;
    let mut cmd = Command::new("timeout");
    cmd.arg(timeout::secs(60)).args(["unshare", "-r", "-m", "sh", "-c", script, "sh"]).arg(dir).arg(exe).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    if let Some(c) = conf {
        cmd.env("RESOLV_HOST_CONF", c);
    }
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "run failed: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Trace: no spec clause (D-16.13). Record: the named file replaces `/etc/host.conf`; one that
/// cannot be opened gives the defaults (`multi off`).
#[test]
fn resolv_host_conf_names_the_file() {
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("unshare").args(["-r", "-m", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no unprivileged user and mount namespaces");
        return;
    }
    let out = r.join("target/test-oracles");
    std::fs::create_dir_all(&out).unwrap();
    let (host, ours) = (out.join("hostconf_env_host"), out.join("hostconf_env_liella"));
    assert!(build(&host, false), "hostconf_env.c does not build against the host C library");
    assert!(build(&ours, true), "hostconf_env.c does not link against the products");
    let dir = std::env::temp_dir().join(format!("hostconf-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hosts"), "127.0.0.1 localhost\n10.9.8.1 multi.test\n10.9.8.2 multi.test\n").unwrap();
    std::fs::write(dir.join("nsswitch.conf"), "hosts: files\n").unwrap();
    std::fs::write(dir.join("on.conf"), "multi on\n").unwrap();
    std::fs::write(dir.join("off.conf"), "multi off\n").unwrap();
    let on = dir.join("on.conf");
    let off = dir.join("off.conf");
    let missing = dir.join("no-such-dir/host.conf");
    let cases: [(&str, String, &str); 3] = [
        ("on.conf", on.display().to_string(), "10.9.8.1\n10.9.8.2\n"),
        ("off.conf", off.display().to_string(), "10.9.8.1\n"),
        ("unopenable", missing.display().to_string(), "10.9.8.1\n"),
    ];
    for (what, path, want) in &cases {
        let reference = run(&host, &dir, Some(path));
        assert_eq!(&reference, want, "{what}: the reference's output");
        assert_eq!(run(&ours, &dir, Some(path)), reference, "{what}: Liella differs from the reference");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
