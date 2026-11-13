# LIBM-DIRECTED-INPLACE(-2 ... -5): the double exp quick phase in the caller's directed rounding

State: branch task/libm-directed-inplace-5 (head: see handoff/reports/LIBM-DIRECTED-INPLACE-5.json). Role mei:~/w/s1.
Decision D-10.94 (records/decisions/D-10/D-10.94.toml). Run -3's report: handoff/LIBM-DIRECTED-INPLACE-3-report.md.

## Run 5 (after LAYOUT-STARTUP-STATE-TIME landed)
- Merged origin/next d01086da3 (b9658901c). One conflict, crates/libc-so/hot-text.list (train-owned): next's copy
  taken. `cargo xtask math-gen` and `stubs` on the merged tree changed nothing. `hottext-repair --at d01086da3` would
  drop `locale::publish_global` (and then static fails: hot-text.unpinned still names it); per the brief no list is
  edited here, so its output was NOT kept: the lists are next's, byte for byte.
- next's new long-sweeps static check wanted this branch's ignored `exp_directed_accuracy_job` in the runner: row
  added in xtask/src/longsweeps.rs (EXP_DIR_N=10^7, its own default; measured 2.26 s test time, job
  libm-directed-inplace-5-lint, `long-sweeps --only exp_directed_accuracy_job` ok).
- Plants (handoff/LIBM-DIRECTED-INPLACE-5-plants.txt, run 2's plants that still apply plus run 3's): 6/6 CAUGHT,
  job libm-directed-inplace-5-plants.
- FULL gate (`affected --run --base origin/next --tier full`, it escalates anyway): job libm-directed-inplace-5-gate;
  result in handoff/reports/LIBM-DIRECTED-INPLACE-5.json. The check that held runs 4/5 is hot_text_products' first
  localtime_r (bound 20).

## Done (runs 1-3, unchanged)
- `fast64::quick::exp_directed::<UP>` (both builds) with its proof (quick.rs module doc "Directed rounding",
  `directed_bounds`), `hot::exp_directed`, `mathret::elem_fast_dir` / cold `elem_directed`, tests in
  crates/boundary/tests/exp_quick.rs, D-10.94, plants (run 2: 4/4, run 3: 3/3).
- Run 3: the exp_upward stall (direction picked from the MXCSR value as data) fixed with a const generic picked by a
  branch: Zen 1 unbooked 68.3 -> 22.4 ns (glibc 18.2).

## Run 4: why CI was red (first localtime_r 21 > 20, first localtime_r + mktime 21 > 20)
Fault map, keke (Zen 1), job labels libm-directed-inplace-4-so / -fmnext / -fm2; script
~/work/LIBM-DIRECTED-INPLACE-4/fmap.sh on keke (links page_faults.c against a products dir, relay count as the
test takes it, then `perf record -e page-faults -d` + tools/hottext/faultmap.py). next 9042cc44f products built
with validation/wl_prep.sh (~/wl/products/9042cc44f9d3...).

| mode | next 9042cc44f | this branch | bound |
| localtime (relay, 6 runs identical) | 19 | 21 | 20 |
| localtime_mktime | 19 | 21 | 20 |
| whole process (direct run, all faults) | 181 | 181 | - |

- The text faults in the window are the same pages in both (localtime_r 0x436000, zone::local_in 0x480000; hot
  text unchanged). No libm code on the path (no math/exp/mathret symbol in the map).
- The whole +2 is the `.bss` page 0x87b000. On next, setenv (before the window) touches it first
  (`liella_boundary::process::TABLE_ON` read, `ENV_OWNED` write). On the branch the `.bss` order changed:
  TABLE_ON/ENV_OWNED moved to 0x87add8/0x87ade0 (the page `.data1`'s tail shares) and `sys::TSC_STATE` moved from
  0x87ada4 to 0x87b170. So the first touch of 0x87b000 now falls inside localtime_r:
  `liella_legacy::time_compat::GENERATION` (0x87b690, read fault) then `liella_boundary::sys::TSC_STATE` (write
  fault). The total stays 181: the faults moved into the window rather than being added.
- Why the order moved: this story edits the liella_boundary crate (hot/exp_log.rs, mathret.rs, selftest.rs,
  s10_math_gen.rs), which changes its codegen units and so the emission order of its `.bss` objects. None of the
  four objects is declared in crates/libc-so/startup-state.list, so their page is left to emission order.

## Open questions / for the coordinator
- LAYOUT-JITTER (data, not text): the objects the first localtime_r touches, `liella_legacy::time_compat::GENERATION`
  and `liella_boundary::sys::TSC_STATE` (and setenv's `process::TABLE_ON` / `ENV_OWNED`), sit in plain `.bss`.
  Recommendation for the data-clusters lane (not edited here, per the brief): declare them together in
  startup-state.list (LAYOUT-DATA-CLUSTERS-9's lesson: declared one at a time, the fault moves to the next object).
- Per-call MXCSR read cost (~20 TSC ticks on Zen 1) on every elem_* entry: a separate design story (run 3).

## Not to redo
The merges with next 9042cc44f (68e2c202c) and d01086da3 (b9658901c). The fault maps above. The run-3 stall fix
and its proof. The 6 plants of run 5.

## Next step
1. If the run-5 FULL gate is green the branch lands. If its first localtime_r is still over the bound: the fault map
   (run 4's recipe, ~/work/LIBM-DIRECTED-INPLACE-4/fmap.sh on keke) of the merged head against next, then stop.
2. Booked timing of exp/exp_upward/expf_upward on Ivy Bridge (nico or kotori) and N100 (target <= 2x glibc).
3. Then log, sin, pow with the same recipe (a direction-dependent constant is a const generic picked by a
   branch, never data from MXCSR; for log decline x = 1, two-sided monotone rounding test, Fast2Sum error terms per
   Boldo-Graillat-Muller).
