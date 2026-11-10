# FUZZ-LOCK-FRESH: stale lock files escalated every gate to FULL

Branch `task/fuzz-lock-fresh`, cut from `origin/next` 01ff5a63. Code commit 67f818c9; this note is a separate commit
touching only handoff/.

## Cause

Two tracked lock files had fallen behind their manifests, so the first build in any tree rewrote them:

| Lock | Rewritten by | Lines | Missing entries |
|---|---|---|---|
| `fuzz/Cargo.lock` | `cargo xtask fuzz-check` (`cargo check --bins` in `fuzz/`; a step of `static`, `test` and `affected`) | +31 | liella-boundary, liella-ld, liella-math; liella-abi, liella-boundary and liella-ld as dependencies of liella-fuzz |
| `crates/Cargo.lock` (product workspace) | `cargo xtask so` (`cargo rustc --manifest-path crates/*-so/Cargo.toml`) | +16 | liella-ld, liella-ld-so |
| `Cargo.lock` (main) | — | 0 | already fresh |

`cargo xtask affected --base origin/next` then saw two changed build files and escalated to FULL. The keke job
`fzl-repro` (log `~/logs/fzl-repro.log`, origin/next unchanged) shows this step by step: clean at the start; `so`
rewrote `crates/Cargo.lock`; `fuzz-check` rewrote `fuzz/Cargo.lock`; `affected --list` gave
`FULL (escalated): crates/Cargo.lock, fuzz/Cargo.lock: build configuration`.

## Change

- Fresh `crates/Cargo.lock` and `fuzz/Cargo.lock`, written by cargo through `cargo xtask locks --update` in the role
  (`fzl-update-2`). They are line for line the rewrites `so` and `fuzz-check` produced in `fzl-repro` (+16 / +31).
- `xtask/src/locks.rs`, new command `cargo xtask locks --check | --update`. `--check` runs
  `cargo metadata --format-version 1 --manifest-path <m> --locked` on `Cargo.toml`, `crates/Cargo.toml` and
  `fuzz/Cargo.toml` (fuzz on its pinned nightly, as fuzz-check does). Cargo fails without writing anything when a lock
  would change, and the check names the stale lock together with cargo's `error:` line. It works offline.
- It runs as static check `lock-fresh` (it comes before `fuzz-check`).
- `fuzz-check` now passes `--locked`, so it can never rewrite the fuzz lock again. If the lock is stale, its error
  message points to `locks --check`.
- `affected` needs no special case: the locks no longer change.
- Unit test `locks::tests::stale_lock_is_reported_and_left_alone` uses a temporary three-crate workspace. It checks
  four things: a fresh lock passes; adding a path dependency fails the check; the check leaves the lock untouched; and
  after an update the check passes again.
- Plants: `handoff/FUZZ-LOCK-FRESH-plants.txt`. There are two: `no-locked` drops `--locked`, and `ok-on-fail` reports a
  failed resolution as a pass. Both are CAUGHT.
- No decision row was added.

## Proof

- `fzl-update-2`: before the update, `locks --check` failed on `crates/Cargo.lock` and `fuzz/Cargo.lock` and left
  them untouched; after `--update` it printed `3 lock files match their manifests`.
- `fzl-lint`: clippy, static and the locks unit test passed. After clippy and static (fuzz-check included),
  `git status` showed the lock files unchanged.
- `fzl-proof`: after clippy, static, `so` and `fuzz-check`, `git status --porcelain` in the role is empty.
  `affected --list --base HEAD`: `changed files: 0`, no FULL. Against origin/next, this branch escalates only for its
  own committed lock refresh and xtask files (once, as expected).
- `fzl-gate` (the FULL gate): 32 suites ok, 11297 tests passed, 0 failed, 126 conformance programs passed,
  `affected: ok`. After it, `git status --porcelain` in the role is still empty.

## Jobs (keke, role `~/w/fzl`, logs `~/logs/<label>.log`)

| Label | What | rc |
|---|---|---|
| fzl-gate | `affected --run --base origin/next` under `~/bin/gate 16G` (FULL, 3721 s) | 0 |
| fzl-plants | `cargo xtask plants handoff/FUZZ-LOCK-FRESH-plants.txt --dir /dev/shm/claude/fzl/plants-1` | 0 |
| fzl-lint | clippy (workspace, all targets, all features), `xtask static`, the locks test, the lock-files-unchanged check | 0 |
| fzl-proof | `so`, fuzz-check, `git status`, `affected --list` against HEAD and origin/next | 0 |
| fzl-update-2 | `locks --check` (stale before) and `--update`, the diff, the locks test | 0 |
| fzl-repro | diagnosis on unchanged origin/next | 0 |
| fzl-update | superseded: the role's xtask binary was not rebuilt (see below) | 1 |

`fzl-update` failed because the rsync kept mtimes from chisato that were older than the xtask binary built earlier in
the role, so cargo reused the old binary, which did not know the `locks` command. Touching the changed sources fixed
it. Separately, `cargo xtask` with an unknown command prints the usage and exits 0.

Follow-up for the coordinator: `~/bin/static-checks` on the VMs is described as the same list as `cargo xtask static`,
so it needs `lock-fresh` added.
