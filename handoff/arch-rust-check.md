# ARCH-RUST-CHECK (A-2, OD-10): aarch64 cross check of the crates that build without csys

State: branch `task/arch-rust-check` (from origin/next 089f1eeb), code commit 48564418; this note and the report
JSON are the next commit. Plan item: plan/06-phases/20-post-release-ledger-and-experiments.md A-2 (marked Done).
Decisions in force: RL-1 provisional (plain armv8.0-a, no target-feature flags). No D-row was needed (the plan item
records it).

## What was done

* `rust-toolchain.toml`: `aarch64-unknown-linux-gnu` added to `targets`.
* `cargo xtask arch-check` (xtask/src/arch.rs `CROSS_TARGET`, `CHECK_ALL_TARGETS`, `CHECK_LIB_ONLY`,
  `cross_check_args`; main.rs `arch_check`): `cargo check --target aarch64-unknown-linux-gnu` over
  - all targets and features: kernel, mem, abi, legacy, ld, native, thread-db, mathtools, spectest;
  - the library only, all features: rt, math, core (their tests dev-depend on liella-boundary, i.e. csys).
* `crates/boundary/build.rs`: stops on any `CARGO_CFG_TARGET_ARCH` other than x86_64 with a "not ported"
  message naming A-3, A-6, A-8. Before this, the build script compiled csys with the host `cc` whatever the
  target, so a foreign target silently got x86_64 objects. x86_64 takes the same path as before.
* docs/arch-seams.md: the check set, the result, and the "compiles is not ported" limit.

## Findings

* The check set needed no `cfg` changes at all: its seams (abi layouts, the `Machine` trait's MXCSR/x87 words,
  `arch_prctl` constants and trait methods, software float models) are data, not instructions. They compile on
  aarch64 but are still x86_64's meaning until A-3, A-5, A-9 and A-10 replace them (labelled in arch-seams.md).
* The brief asks for `compile_error!` stubs outside the check set. The only crate outside the set that the build
  reaches first is boundary, and its build script runs before any item compiles, so the stub is the build-script
  guard (a `compile_error!` in boundary's lib.rs would never be reached). libc-so and libm-so stop through it.

## Jobs (keke, role n2, head 48564418)

* `arch-rust-check-cross`: `cargo xtask arch-check`, rc 0.
* `arch-rust-check-plants`: ~/work/arch-rust-check/plants.sh in a RAM copy (/dev/shm/claude/ARCH-RUST-CHECK/plants-1):
  baseline rc 0; P1 (x86_64 intrinsic import in liella-kernel) CAUGHT by E0432; P2 (liella-mem dev-depends on
  liella-boundary) CAUGHT by the guard message; restored rc 0. Job rc 0.
* `arch-rust-check-gate`: x86_64 direct tier, `cargo xtask affected --base origin/next --run`: see the report JSON.
* Foreground on the role: `cargo clippy --workspace --all-targets --all-features -- -D warnings` rc 0,
  `cargo xtask static` rc 0.

## Next step

A-3 (syscalls arch column) and A-5 (probed aarch64 layouts) can start; they extend `CHECK_*` in xtask/src/arch.rs
when more crates become portable. Not to redo: the per-crate exploration (all twelve passed as they were).
