# HOTTEXT-REGEN-UNBLOCK

Branch `task/hottext-regen-unblock` (from `origin/next` 01ff5a63). Ruling in force: #4079 / D-X.47 (a
start-up or fork key of `crates/libc-so/hot-text.list` that matches no function section of the profile
being built fails `cargo xtask so`). This story keeps that rule as it is and removes the two ways it
blocked work on 2026-11-07. Decision row: `D-NEW` in `plan/07-decisions/x-implementation-decisions.md`
(the coordinator assigns the id).

## What changed

1. **A stale key no longer blocks its own repair.** `cargo xtask so --hottext-regen` builds the products
   with the dead-key check reported, not enforced (`xtask/src/hottext.rs`: `regen_mode`,
   `dead_key_verdict`). Only `tools/hottext/regen.sh --build` passes it: regen builds in the profile
   `LIELLA_FULL_PROFILE` selects, traces, rewrites the list (merge.py drops a key from a profile whose
   trace no longer hits it), then runs plain `cargo xtask so`, which must pass the check. Both profiles:
   `bash tools/hottext/regen.sh . --build` and `LIELLA_FULL_PROFILE=1 bash tools/hottext/regen.sh . --build`.
   This replaces the drop-and-rebuild workaround (`hotregen_remote.sh`). No gate can run with the check off:
   - `so` takes no other argument (a typo fails instead of building normally) and refuses the flag when
     `CI` is set;
   - `cargo xtask static` has a new check `hottext` (alone: `cargo xtask hottext-check`): it fails when any
     tracked file other than `xtask/src/hottext.rs`, `tools/hottext/regen.sh` or a `.md` names the flag.
   - INFORMATIONAL: the build hosts' `~/bin/static-checks` mirrors the static list and needs a `hottext`
     entry from whoever maintains it.
2. **No inlining lottery for the fork tier.** The plain (non-generic) fork-tier functions the list names are
   `#[inline(never)]`: `spawn::wait4`, `fork::{fork, child_reset}`, `timer::after_fork_child`,
   `process::exit_now`. The `hottext` static check fails when a fork-tier key of a plain workspace function
   lacks the attribute (`INLINE_NEVER_TIERS`), so a regen that lists a new one asks for it at once.
3. The list itself is unchanged on this branch: the change only adds symbols, so no key goes dead. The
   newly outlined `spawn::wait4` (fast) and `timer::after_fork_child` (release) are not yet in that profile's
   group; the next `regen.sh --build` run (both profiles) adds them. The A run below used `regen.sh --build`
   in both profiles end to end (fast 482 keys traced, release 421; no dead key, the final plain `so` passed).

## Measurements (Zen 1 build host, deterministic counters)

Start-up rows: `bench/c/cbench_startup.c` with `CBENCH_COUNTERS=1` (user-mode instructions of one child,
min of 5). p_fork: `perf stat -e instructions:u` of `tools/hottext/probes/p_fork` (parent + child, min of 7).
Faults: `crates/core/tests/hot_text_products.rs` (all 12 tests passed in every column).

| row | base fast | A fast | B fast | base release | A release | B release |
|---|---|---|---|---|---|---|
| startup_instr_c | 2243 | 2246 (+3) | pending | 2147 | 2172 (+25) | pending |
| startup_instr_setloc | 67687 | 67697 (+10) | pending | 68051 | 68096 (+45) | pending |
| startup_instr_printf | 71519 | 71534 (+15) | pending | 71861 | 71879 (+18) | pending |
| startup_instr_wctype | 68711 | 68729 (+18) | pending | 69081 | 69107 (+26) | pending |
| p_fork instructions:u | 3548 | 3552 (+4) | pending | 3469 | 3502 (+33) | pending |
| faults: startup only (bound 23) | 18 | 18 | pending | 17 | 17 | pending |
| faults: setlocale+printf (bound 68) | 56 | 56 | pending | 57 | 57 | pending |
| faults: tool (bound 71) | 59 | 59 | pending | 60 | 60 | pending |
| faults: fork child (bound 9) | 6 | 6 | pending | 6 | 6 | pending |

(`startup_kinstr_c`, user+kernel, moved by thousands between runs in both directions: kernel noise, not used.)
- base = origin/next 01ff5a63.
- A = `#[inline(never)]` on all 12 plain start-up and fork functions (+ list regenerated). The start-up rows
  rise: in fast `process::run_thread_dtors` (exit path) and `locale::refresh_words` (setlocale path) stop
  being inlined; in release also `rt::start::program` and `runtime::apply_irelative`. Faults unchanged.
- B = fork tier only (this branch, list unchanged): job `hrg-measure-fork` (results in
  `~/work/hrg/fork/`), queued at hand-back because the build host's slots were full. Expected: start-up rows
  equal to base (no start-up function changed), p_fork within a few instructions of base.

## RULING NEEDED

Start-up tier: marking its 7 plain functions (`rt::start::program`, `cpu::init_with`,
`runtime::{apply_irelative, run_fini}`, `locale::refresh_words`, `process::{exit, run_thread_dtors}`) costs
+3 to +18 user instructions per start-up row in fast and +18 to +45 in release (column A), with no fault
change. Options: (a) accept that cost for stable start-up keys; (b) leave the start-up tier to the build check
plus `regen.sh --build` (this branch: a flip now costs one regen run, no hand edits); (c) mark only the
start-up functions that already have a symbol in both profiles (`cpu::init_with`, `runtime::run_fini`,
`process::exit`; probably free, not measured alone) and measure again. Recommendation: (b) now, (c) as a follow-up if a
start-up key flips again.

## Jobs (build host, `~/logs/<label>.log`)

- `hrg-measure-base`: base, both profiles: rc 0 (numbers above).
- `hrg-regen-after`: variant A, `regen.sh --build` in both profiles, then the measurements: rc 0.
- Final head, started in order by one detached chain (`~/logs/hrg-chain2.log`), each through the slot queue:
  `hrg-measure-fork` (variant B numbers; builds the products in both profiles, so it is also the
  `cargo xtask so` fast + `LIELLA_FULL_PROFILE=1` check), `hrg-gate` (`cargo xtask clippy`, `cargo xtask
  static`, `so` release and fast, `cargo xtask affected --run --files <changed files>`), `hrg-plants`
  (`handoff/HOTTEXT-REGEN-UNBLOCK-plants.txt`, 8 plants).
- Superseded: `hrg-regen-fork` (stopped while queued; it would have regenerated the list for variant B).

## Not to redo

- The DWARF route (keep a key alive when its function was inlined into a listed caller) is not available
  without changing the products: the product profiles carry no debug info.
- Generic, impl and closure keys (`runtime::start::<false>`, `Registry::register<..>`, `run_all<..>`,
  `with_list`/`each_stream` closures, `Heap::park_others<..>`, `Features::detect<..>`, core's
  `Option::map_or` with a `start` closure) are not marked: the attribute would also stop inlining at their
  other (stdio, heap) call sites. The build check still guards them; `regen.sh --build` repairs them.
