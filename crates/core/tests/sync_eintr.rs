//! Which thread and IPC waits end with `EINTR` when a handled signal interrupts them, with and without
//! `SA_RESTART` (§22.7 Failure `s22_7_f_0`, §20.5 `s20_5_c_1`; no new clause id). `tests/oracle/sync_eintr_diff.c`
//! starts a waiter thread per wait, shows it blocked through `/proc/self/task/<tid>/stat` (state `S`, bounded), sends
//! `SIGUSR1` with `pthread_kill` and prints whether the wait returned (and with what) or stayed blocked. The record
//! [`GLIBC`] is what the reference printed: `sem_timedwait`/`sem_clockwait` and `sigtimedwait` return `EINTR` even
//! with `SA_RESTART`; `sem_wait` returns it only without; `sigsuspend` and `pause` always; the condition, mutex,
//! rwlock, barrier, join and `sigwait` waits never do (checked by result: the wait is still pending after the
//! signal). Liella's output must equal the record. Needs the products (`cargo xtask so`) and a C compiler; without
//! them the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;
#[path = "support/oracle.rs"]
mod oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

const GLIBC: &str = concat!(
    "sem_wait restart=0: returned rc=-1 errno=EINTR handler=1\n",
    "sem_wait restart=1: stayed blocked handler=1\n",
    "sem_timedwait restart=0: returned rc=-1 errno=EINTR handler=1\n",
    "sem_timedwait restart=1: returned rc=-1 errno=EINTR handler=1\n",
    "sem_clockwait restart=0: returned rc=-1 errno=EINTR handler=1\n",
    "sem_clockwait restart=1: returned rc=-1 errno=EINTR handler=1\n",
    "pthread_cond_wait restart=0: stayed blocked handler=1\n",
    "pthread_cond_wait restart=1: stayed blocked handler=1\n",
    "pthread_cond_timedwait restart=0: stayed blocked handler=1\n",
    "pthread_cond_timedwait restart=1: stayed blocked handler=1\n",
    "pthread_mutex_timedlock restart=0: stayed blocked handler=1\n",
    "pthread_mutex_timedlock restart=1: stayed blocked handler=1\n",
    "pthread_mutex_clocklock restart=0: stayed blocked handler=1\n",
    "pthread_mutex_clocklock restart=1: stayed blocked handler=1\n",
    "pthread_rwlock_timedrdlock restart=0: stayed blocked handler=1\n",
    "pthread_rwlock_timedrdlock restart=1: stayed blocked handler=1\n",
    "pthread_rwlock_timedwrlock restart=0: stayed blocked handler=1\n",
    "pthread_rwlock_timedwrlock restart=1: stayed blocked handler=1\n",
    "pthread_rwlock_clockrdlock restart=0: stayed blocked handler=1\n",
    "pthread_rwlock_clockrdlock restart=1: stayed blocked handler=1\n",
    "pthread_rwlock_clockwrlock restart=0: stayed blocked handler=1\n",
    "pthread_rwlock_clockwrlock restart=1: stayed blocked handler=1\n",
    "pthread_barrier_wait restart=0: stayed blocked handler=1\n",
    "pthread_barrier_wait restart=1: stayed blocked handler=1\n",
    "pthread_join restart=0: stayed blocked handler=1\n",
    "pthread_join restart=1: stayed blocked handler=1\n",
    "sigwait restart=0: stayed blocked handler=1\n",
    "sigwait restart=1: stayed blocked handler=1\n",
    "sigtimedwait restart=0: returned rc=-1 errno=EINTR handler=1\n",
    "sigtimedwait restart=1: returned rc=-1 errno=EINTR handler=1\n",
    "sigsuspend restart=0: returned rc=-1 errno=EINTR handler=1\n",
    "sigsuspend restart=1: returned rc=-1 errno=EINTR handler=1\n",
    "pause restart=0: returned rc=-1 errno=EINTR handler=1\n",
    "pause restart=1: returned rc=-1 errno=EINTR handler=1\n",
);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/sync_eintr_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-pthread"]).arg(&src);
    }
    oracle::build_quiet(cmd, out)
}

fn run(exe: &Path) -> String {
    let o = Command::new(exe).output().unwrap();
    assert!(o.status.success(), "{}: {:?} {}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Compare line by line so a failure names the wait.
fn same(what: &str, got: &str, want: &str) {
    let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
    for i in 0..g.len().max(w.len()) {
        assert_eq!(g.get(i), w.get(i), "{what}: line {i}");
    }
}

/// Clauses `s22_7_f_0` (the semaphore waits' `EINTR`) and `s20_5_c_1` (`pause`); the other waits' contracts have no clause id.
#[test]
fn sync_eintr_contracts() {
    let r = root();
    if skip_note::missing("cc") || !r.join("target/liella/libc.a").exists() || !r.join("target/liella/liella-crt1.o").exists() {
        skip_note::skipped("sync_eintr: needs cc and the products (cargo xtask so)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let lie = dir.join("sync_eintr_liella");
    assert!(build(&lie, true), "the Liella build of sync_eintr_diff.c failed");
    same("Liella", &run(&lie), GLIBC);
    let refx = dir.join("sync_eintr_ref");
    if build(&refx, false) {
        let got = run(&refx);
        if got != GLIBC {
            eprintln!("note: the host reference no longer equals the record:\n{got}");
        }
    }
}
