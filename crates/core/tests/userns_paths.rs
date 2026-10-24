//! The privileged paths of the network, namespace, host-id and login-record interfaces, tested
//! without root (plan next-batch-1024, USERNS-PRIV-PATHS). Phases 10 and 11 left these "untested
//! for lack of privilege": a successful `rresvport` and `bindresvport` bind to a reserved port,
//! `unshare`/`setns` into real namespaces, `login`/`logout`/`logwtmp` writing the system `utmp`
//! and `wtmp`, and `sethostid`'s success inside and `EPERM`/`EACCES` outside a namespace.
//!
//! One C program (embedded below) is built against the host C library and statically against
//! Liella's products. It runs as the unprivileged test user (`plain`) and as the root of a private
//! user, mount and network namespace (`unshare -r -m -n`, `ns`), where it mounts private tmpfs
//! instances over `/var/log`, `/var/run` and `/etc`, so the login records and the host id go to
//! scratch files and the host's own `utmp`, `wtmp` and `hostid` are never opened for writing
//! (the program stops before any write when a mount fails). The two builds must print the same
//! lines, and the Liella lines must show the expected results (so two equally broken runs
//! do not pass). Needs the products (`cargo xtask so`), a C compiler, and unprivileged user
//! namespaces (`unshare -r -m -n`); without them the test says what it did not run and passes.
//! Never runs as root. No file-system dependence: only private tmpfs files are read back.
//!
//! What stays unrun (it would need real root, listed not run): `reboot`, `settimeofday`,
//! binding a reserved port in the host's network namespace, and writing the host's real
//! `/var/run/utmp`, `/var/log/wtmp` and `/etc/hostid`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/oracle.rs"]
mod oracle;
#[path = "support/skip_note.rs"]
mod skip_note;
#[path = "support/timeout.rs"]
mod timeout;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const PROGRAM: &str = r#"
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>
#include <utmp.h>

static void port_of(const char *what, int fd, int lo, int hi) {
    struct sockaddr_in a; socklen_t n = sizeof a;
    int ok = fd >= 0 && getsockname(fd, (struct sockaddr *)&a, &n) == 0;
    int p = ok ? ntohs(a.sin_port) : -1;
    printf("%s: bound=%d in_range=%d\n", what, ok, p >= lo && p <= hi);
}

static void dump(const char *what, const char *path) {
    FILE *f = fopen(path, "rb");
    struct utmp u; int k = 0;
    if (!f) { printf("%s: cannot open errno=%d\n", what, errno); return; }
    while (fread(&u, sizeof u, 1, f) == 1) {
        char line[UT_LINESIZE + 1], user[UT_NAMESIZE + 1], host[UT_HOSTSIZE + 1];
        snprintf(line, sizeof line, "%.*s", UT_LINESIZE, u.ut_line);
        snprintf(user, sizeof user, "%.*s", UT_NAMESIZE, u.ut_user);
        snprintf(host, sizeof host, "%.*s", UT_HOSTSIZE, u.ut_host);
        printf("%s[%d]: type=%d line_is_pts=%d line=%s user=%s host=%s pid_is_ours=%d time_set=%d\n", what, k++, (int)u.ut_type,
               strncmp(line, "pts/", 4) == 0, strncmp(line, "pts/", 4) == 0 ? "pts" : line, user, host, u.ut_pid == getpid(), u.ut_tv.tv_sec != 0);
    }
    printf("%s: records=%d\n", what, k);
    fclose(f);
}

static void plain(void) {
    int port = 1023, fd;
    { int rc; errno = 0; rc = sethostid(0x1234); printf("sethostid(outside): rc=%d errno=%d\n", rc, errno); }
    errno = 0; fd = rresvport(&port); printf("rresvport(outside): ok=%d errno=%d\n", fd >= 0, fd >= 0 ? 0 : errno);
    if (fd >= 0) close(fd);
    fd = socket(AF_INET, SOCK_STREAM, 0);
    { int rc; errno = 0; rc = bindresvport(fd, NULL); printf("bindresvport(outside): rc=%d errno=%d\n", rc, errno); }
    close(fd);
}

static int mount_scratch(const char *where) {
    if (mount("tmpfs", where, "tmpfs", 0, "mode=755,size=1m") != 0) { printf("mount %s failed errno=%d\n", where, errno); return 0; }
    return 1;
}

static void ns(void) {
    int port = 1023, port2 = 1023, fd, fd2, s, nsfd;
    char name[64], line[64];
    struct sockaddr_in sin;

    if (!mount_scratch("/var/log") || !mount_scratch("/var/run") || !mount_scratch("/etc")) { printf("scratch mounts unavailable\n"); return; }
    printf("scratch mounts: ok\n");

    /* §15.8: a successful reserved-port bind, in the private network namespace. */
    fd = rresvport(&port);
    port_of("rresvport", fd, 512, 1023);
    fd2 = rresvport(&port2);
    printf("rresvport again: ok=%d lower=%d\n", fd2 >= 0, fd2 >= 0 && port2 < port);
    s = socket(AF_INET, SOCK_STREAM, 0);
    printf("bindresvport(NULL): rc=%d\n", bindresvport(s, NULL));
    port_of("bindresvport(NULL)", s, 1, 1023);
    close(s);
    s = socket(AF_INET, SOCK_STREAM, 0);
    memset(&sin, 0, sizeof sin); sin.sin_family = AF_INET; sin.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    printf("bindresvport(sin): rc=%d\n", bindresvport(s, &sin));
    port_of("bindresvport(sin)", s, 1, 1023);
    close(s);
    if (fd >= 0) close(fd);
    if (fd2 >= 0) close(fd2);

    /* §18.9: unshare and setns into real namespaces. */
    nsfd = open("/proc/self/ns/uts", O_RDONLY);
    gethostname(name, sizeof name);
    printf("unshare(NEWUTS): rc=%d\n", unshare(CLONE_NEWUTS));
    printf("sethostname(inner): rc=%d\n", sethostname("inner", 5));
    gethostname(line, sizeof line);
    printf("hostname inside: %s\n", line);
    { int rc; errno = 0; rc = setns(nsfd, CLONE_NEWNET); printf("setns(wrong type): rc=%d errno=%d\n", rc, errno); }
    { int rc, inner = open("/proc/self/ns/uts", O_RDONLY);
      errno = 0; rc = setns(nsfd, CLONE_NEWUTS);
      printf("setns(initial uts, not ours): rc=%d errno=%d\n", rc, errno);
      printf("unshare(NEWUTS) again: rc=%d\n", unshare(CLONE_NEWUTS));
      gethostname(line, sizeof line);
      printf("hostname in the second (copied): inner=%d\n", strcmp(line, "inner") == 0);
      printf("setns(first uts): rc=%d\n", setns(inner, CLONE_NEWUTS));
      gethostname(line, sizeof line);
      printf("hostname back in the first: %s\n", line);
      close(inner); }
    { int rc; errno = 0; rc = setns(-1, 0); printf("setns(bad fd): rc=%d errno=%d\n", rc, errno); }
    printf("unshare(NEWIPC): rc=%d\n", unshare(CLONE_NEWIPC));
    printf("unshare(NEWNET): rc=%d\n", unshare(CLONE_NEWNET));
    close(nsfd);

    /* §24.5: sethostid succeeds with the privilege the namespace gives, and writes the file. */
    { struct stat st; long id; int r = sethostid(0x12345678);
      id = gethostid();
      printf("sethostid(inside): rc=%d gethostid=%lx\n", r, id & 0xffffffffL);
      printf("/etc/hostid: size=%d\n", stat("/etc/hostid", &st) == 0 ? (int)st.st_size : -1); }

    /* §17.6: login, logout and logwtmp write the (scratch) utmp and wtmp. */
    { int m, sl; char *t; struct utmp u; int fdw;
      fdw = open("/var/run/utmp", O_CREAT | O_WRONLY, 0644); close(fdw);
      fdw = open("/var/log/wtmp", O_CREAT | O_WRONLY, 0644); close(fdw);
      m = posix_openpt(O_RDWR | O_NOCTTY);
      if (m < 0 || grantpt(m) != 0 || unlockpt(m) != 0 || (t = ptsname(m)) == NULL || (sl = open(t, O_RDWR | O_NOCTTY)) < 0) {
          printf("no pseudo-terminal: login on a terminal not run\n");
      } else {
          char tty[64]; const char *l;
          dup2(sl, 0);
          if (ttyname_r(0, tty, sizeof tty) != 0) { printf("ttyname_r failed\n"); return; }
          l = strncmp(tty, "/dev/", 5) == 0 ? tty + 5 : tty;
          memset(&u, 0, sizeof u);
          strcpy(u.ut_user, "alice"); strcpy(u.ut_host, "h.example"); strcpy(u.ut_line, "ignored");
          login(&u);
          dump("utmp after login", "/var/run/utmp");
          dump("wtmp after login", "/var/log/wtmp");
          printf("logout(line): rc=%d\n", logout(l));
          printf("logout(unknown): rc=%d\n", logout("nosuchline"));
          dump("utmp after logout", "/var/run/utmp");
          logwtmp(l, "", "");
          logwtmp("ttyX", "bob", "host2");
          dump("wtmp after logwtmp", "/var/log/wtmp");
          close(0);
      }
    }
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc < 2) return 2;
    if (strcmp(argv[1], "plain") == 0) plain();
    else ns();
    return 0;
}
"#;

/// `bindresvport` is a compat-versioned symbol (only in the shared library under that name); the
/// static build calls its `liella_compat_` definition.
fn build(src: &Path, out: &Path, liella: bool) -> bool {
    let r = root();
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin", "-Dbindresvport=liella_compat_bindresvport"]).arg(r.join("target/liella/liella-crt1.o")).arg(src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(src);
    }
    oracle::build(cmd, out)
}

fn run(exe: &Path, mode: &str, wrap: &[&str]) -> String {
    let limit = timeout::secs(60);
    let mut cmd = Command::new("timeout");
    cmd.arg(&limit).args(wrap).arg(exe).arg(mode).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    let o = cmd.output().unwrap();
    format!("{}ended: {:?}\n", String::from_utf8_lossy(&o.stdout), o.status.code())
}

fn same(what: &str, want: &str, got: &str) {
    let diffs: Vec<String> = want.lines().zip(got.lines()).filter(|(w, g)| w != g).map(|(w, g)| format!("  glibc  {w}\n  liella {g}")).collect();
    assert!(diffs.is_empty() && want.lines().count() == got.lines().count(), "{what}: differs from the reference:\n{}\n--- reference\n{want}--- liella\n{got}", diffs.join("\n"));
}

/// Lines the Liella run must contain (the reference has them too, by `same`).
const EXPECT: &[&str] = &[
    "scratch mounts: ok",
    "rresvport: bound=1 in_range=1",
    "rresvport again: ok=1 lower=1",
    "bindresvport(NULL): rc=0",
    "bindresvport(NULL): bound=1 in_range=1",
    "bindresvport(sin): rc=0",
    "bindresvport(sin): bound=1 in_range=1",
    "unshare(NEWUTS): rc=0",
    "sethostname(inner): rc=0",
    "hostname inside: inner",
    "setns(initial uts, not ours): rc=-1 errno=1",
    "unshare(NEWUTS) again: rc=0",
    "hostname in the second (copied): inner=1",
    "setns(first uts): rc=0",
    "hostname back in the first: inner",
    "unshare(NEWIPC): rc=0",
    "unshare(NEWNET): rc=0",
    "sethostid(inside): rc=0 gethostid=12345678",
    "/etc/hostid: size=4",
    "utmp after login[0]: type=7 line_is_pts=1 line=pts user=alice host=h.example pid_is_ours=1 time_set=0",
    "wtmp after login[0]: type=7 line_is_pts=1 line=pts user=alice host=h.example pid_is_ours=1 time_set=0",
    "logout(line): rc=1",
    "logout(unknown): rc=0",
    "utmp after logout[0]: type=8 line_is_pts=1 line=pts user= host= pid_is_ours=1 time_set=1",
    "wtmp after logwtmp: records=3",
];

#[test]
fn privileged_paths_in_user_namespaces() {
    // Clauses: s15_8_c_2, s17_6_c_5, s18_9_c_3, s24_5_c_4.
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if Command::new("id").arg("-u").output().is_ok_and(|o| o.stdout.starts_with(b"0\n")) {
        skip_note::skipped("skipped: the test runs only as an unprivileged user");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("userns_paths.c");
    let tmp = dir.join(format!("userns_paths.c.{}", std::process::id()));
    std::fs::write(&tmp, PROGRAM).unwrap();
    std::fs::rename(&tmp, &src).unwrap();
    let (host, ours) = (dir.join("userns_paths_host"), dir.join("userns_paths_liella"));
    assert!(build(&src, &host, false), "userns_paths.c does not build against the host C library");
    assert!(build(&src, &ours, true), "userns_paths.c does not link against the products");

    // Outside any namespace: the privileged calls fail the same way.
    let want = run(&host, "plain", &[]);
    assert!(want.contains("sethostid(outside): rc=-1"), "the reference's plain run:\n{want}");
    let got = run(&ours, "plain", &[]);
    assert!(got.contains("sethostid(outside): rc=-1"), "sethostid succeeded outside a namespace:\n{got}");
    same("outside a namespace", &want, &got);

    let wrap = ["unshare", "-r", "-m", "-n"];
    if !Command::new("unshare").args(wrap).arg("true").status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no unprivileged user, mount and network namespaces (unshare -r -m -n); NOT RUN: the successful rresvport/bindresvport binds, unshare/setns into real namespaces, login/logout/logwtmp into scratch utmp/wtmp, sethostid inside; run only: the failures outside a namespace");
        return;
    }
    let want = run(&host, "ns", &wrap);
    if want.contains("scratch mounts unavailable") || want.contains("failed errno=") {
        skip_note::skipped("skipped: the namespace refuses the tmpfs mounts; NOT RUN: all in-namespace checks (rresvport/bindresvport binds, unshare/setns, login/logout/logwtmp, sethostid inside)");
        return;
    }
    let got = run(&ours, "ns", &wrap);
    same("in the namespace", &want, &got);
    let missing: Vec<&&str> = EXPECT.iter().filter(|l| !got.lines().any(|g| g == **l)).collect();
    assert!(missing.is_empty(), "Liella's run lacks {missing:?}:\n{got}");
}
