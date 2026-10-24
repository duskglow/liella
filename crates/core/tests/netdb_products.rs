//! The netdb databases through the NSS `files` source, and `getaddrinfo`/`getnameinfo` without
//! DNS, end to end (spec §16.1–16.3, §16.5, §16.6, §15.7; D-16.1, D-16.2):
//! `tests/oracle/netdb_diff.c` built against the host C library and statically against
//! Liella's products, each run by `tests/oracle/netdb_run.sh` (the fixture `/etc` written in a
//! scratch directory, then the lookups with it bind-mounted over `/etc` in private mount and
//! network namespaces where only `lo` is up, as root through `sudo -n`); the outputs must be
//! identical line by line, except the recorded differences: [`KNOWN`] (a line that differs),
//! [`ADDED`] (a line only Liella prints) and [`DROPPED`] (a line only the reference prints).
//! Needs the products (`cargo xtask so`), a C compiler and passwordless `sudo`; without the
//! latter two the test says so and passes.
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
    let src = r.join("crates/core/tests/oracle/netdb_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision (D-16.2): text the reference's line starts
/// with, and why. A recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str); 13] = [
    ("byname []:", "an empty name is no host name (RFC 1123 names are not empty); the reference matches a hosts line that has an address and no name"),
    (
        "ent: [localhost] type=2 len=4 aliases: [ip6-localhost]",
        "gethostent returns each hosts line in its own family (the ::1 line as AF_INET6 ::1, as gethostbyname2 and gethostbyaddr return it); the reference turns ::1 into 127.0.0.1 and skips the other IPv6 lines",
    ),
    ("getnetbyname [loopback]:", "n_net is the network number as inet_network reads the text (right-aligned: 127 is 0x7f), so getnetbyaddr finds what getnetbyname returns for the number inet_network gives; the reference left-aligns it (0x7f000000)"),
    ("getnetbyname [example]:", "right-aligned network number (192.0.2 is 0xc00002), as above"),
    ("getnetbyname [exnet]:", "right-aligned network number, as above"),
    ("getnetbyname [bad]:", "a networks line whose number does not parse is no record, as a hosts line whose address does not parse; the reference reads it as 0xffffffff"),
    ("getnetbyaddr 127:", "getnetbyaddr takes the right-aligned number inet_network gives and finds the loopback network"),
    ("getnetbyaddr c00002:", "as above, for 192.0.2"),
    ("netent: [loopback]", "right-aligned network number in the enumeration"),
    ("netent: [example]", "right-aligned network number in the enumeration"),
    ("getservbyname [badport/tcp]:", "a services line whose port is above 65535 is no record; the reference keeps the low 16 bits (70000 is 4464)"),
    ("gai [localhost] [65536]", "a port is 0-65535: 65536 is EAI_SERVICE; the reference reads it as port 0"),
    ("gai [] [80]", "an empty node is no host name: EAI_NONAME (the reference answers with the nameless hosts line)"),
];

/// Lines only Liella prints: text the line starts with, and why.
const ADDED: [(&str, &str); 1] = [("ent: [gamma] type=10 len=16 aliases: [gamma6] addrs: 2001:db8::10", "gethostent returns the IPv6 lines of the hosts file (the reference skips them)")];

/// Lines only the reference prints: text the line starts with, and why.
const DROPPED: [(&str, &str); 2] = [
    ("netent: [bad] ffffffff", "a networks line whose number does not parse is no record"),
    ("servent: [badport] 4464/tcp", "a services line whose port is above 65535 is no record"),
];
/// `text` without the lines recorded in `list` (each must be found once).
fn without(text: &str, list: &[(&str, &str)], who: &str) -> Vec<String> {
    let mut seen = vec![false; list.len()];
    let mut out = Vec::new();
    for l in text.lines() {
        match list.iter().position(|(needle, _)| l.starts_with(needle)) {
            Some(i) if !seen[i] => {
                seen[i] = true;
                eprintln!("recorded line only {who} prints ({}):\n  {l}", list[i].1);
            }
            _ => out.push(l.to_string()),
        }
    }
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded line no longer printed by {who}: {:?}", list[i]);
    }
    out
}

#[test]
fn netdb_equals_the_reference() {
    // Clauses: s16_1_c_0, s16_1_c_1, s16_1_c_2, s16_1_g_1, s16_2_c_1, s16_2_c_2, s16_2_c_2_1, s16_2_c_2_2,
    // s16_2_c_2_3, s16_2_c_2_4, s16_2_c_2_5, s16_2_c_3, s16_2_c_4, s16_2_f_0, s16_2_g_0, s16_3_c_0, s16_3_f_0,
    // s16_5_c_0, s16_5_f_0, s16_6_c_0, s15_7_c_0 (D-16.1, D-16.2).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the lookups need private mount and network namespaces)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("netdb_diff_host"), dir.join("netdb_diff_liella"));
    assert!(build(&host, false), "netdb_diff.c does not build against the host C library");
    assert!(build(&ours, true), "netdb_diff.c does not link against the products");
    let script = r.join("crates/core/tests/oracle/netdb_run.sh");
    let wrap = root().join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(120));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let reference = run(&host, "netdb-host", None);
    // Liella with the name-service cache on and off (D-16.10), each in a private /dev/shm (`nscache_mode.sh`).
    for mode in ["on", "off"] {
        eprintln!("name-service cache {mode}");
        let (want, got) = (reference.clone(), run(&ours, &format!("netdb-liella-{mode}"), Some(mode)));
        let want = without(&want, &DROPPED, "the reference");
        let got = without(&got, &ADDED, "Liella");
        let known = KNOWN;
        let mut diffs = 0;
        let mut seen = vec![false; known.len()];
        for (k, (w, g)) in want.iter().zip(got.iter()).enumerate() {
            if let Some(i) = known.iter().position(|(needle, _)| w.starts_with(needle)) {
                if w != g {
                    seen[i] = true;
                    eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", known[i].1);
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
        assert_eq!(want.len(), got.len(), "line counts differ:\n--- glibc\n{}\n--- liella\n{}", want.join("\n"), got.join("\n"));
        assert_eq!(diffs, 0, "lines that differ from the reference");
        for (i, s) in seen.iter().enumerate() {
            assert!(s, "recorded difference no longer seen: {:?}", known[i]);
        }
    }
}
