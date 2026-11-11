# DL-INIT-ORDER report (phase 14 DL-11)

Commits (all on task/dl-init-order, base origin/next 8253a107c): e6fca8868 code, tests, probe; 5117d9444 preinit rule in
one place + plants file; a29f73eec D-27.19 + spec/behaviour/dynlink.md; 4c0ef7989 handoff; then this report.

## Jobs (kanon, role kn1)
* dl-init-order-lint (e6fca8868): clippy, static, `cargo xtask so`, `cargo test -p liella-ld --test init_order`: rc 0,
  7/7 passed.
* dl-init-order-plants (5117d9444; later commits change no code): `cargo xtask plants handoff/DL-INIT-ORDER-plants.txt`:
  6 plants, 6 CAUGHT, 0 SURVIVED, 0 EQUIVALENT, 0 WARN-RC-ONLY, 0 WARN-NO-MSG. deps-after-dependents,
  fini-not-reversed, dt-init-after-array (pure tests), preinit-in-shared-object, loader-init-skipped,
  no-termination-function (entry.S; the corpus run).
* dl-init-order-gate: STOPPED by me (kill-label). `affected` escalated to FULL because csys/src/machine/entry.S defines
  `_start` ("may be part of the exported C ABI"); developers do not run FULL. The change in entry.S is in the
  `LIELLA_LD` branch only (`liella_machine_ld_enter` keeps `%rdx`), which is assembled into ld.so alone.
* dl-init-order-gate2 (final code): clippy, static, `affected --base origin/next --tier direct --files <every changed
  file but entry.S> --run`, then `cargo xtask so` and the ld.so suites that run entry.S's ld variant by name
  (init_order, reloc, ldso_link, ldso_run). Result: see the hand-back / report json.
* dl-init-order-counts / dl-init-order-counts-next: `cargo xtask bench --compare --counters cbench_startup` on this
  branch and on an origin/next copy (~/work/DL-INIT-ORDER/next), same host. Liella instr/op, branch vs next:
  instr_c 2150 vs 2150, setloc 67910 vs 67906, printf 71704 vs 71703, wctype 68934 vs 68934 (glibc's own rows move by
  up to 1 between the runs); kinstr_c 156871 vs 155639 (kernel; glibc's moved 707677 vs 699469 in the same pair).
  These are static programs and never enter ld.so; no start-up row moved beyond run-to-run noise.
* dl-init-order-recnew: `cargo xtask records new decision D-27.19` (in the next copy; skeleton filled by hand).

## Probe-settled behaviour
tools/probe/dl/order.sh against the reference loader (Ubuntu glibc 2.39, kanon): see spec/behaviour/dynlink.md
"Order without a C library" and the RECORD in crates/ld/tests/init_order.rs. The test also re-runs the reference live.

## INFORMATIONAL
* The order corpus of DL-0b (`dlorder`, linked with the C library: atexit/`__cxa_atexit` interplay, dlopen, dlclose)
  cannot run under Liella's ld.so yet (needs DL-12 TLS, DL-13 libc early init, and dlopen). This story proves the
  loader's part with a `-nostdlib` corpus of the same graph plus five more; the libc interplay stays for DL-13: libc
  must run the program's DT_INIT/INIT_ARRAY when crt1 passes `init` NULL, and leave the program's fini to `rtld_fini`
  (ld.so's `fini_all` already runs the program's DT_FINI_ARRAY/DT_FINI first).
* `affected` escalates any change to csys/src/machine/entry.S to FULL (it defines `_start`). A per-symbol mapping for
  `liella_machine_ld_enter` would not help: the file-level rule sees `_start`. Splitting the ld variant into its own
  file (e.g. machine/ld_entry.S) would let ld-only changes gate at the direct tier.

## RULING NEEDED
None. D-27.19 decides local details (0/all-ones array entries skipped; the program's fini run by the loader's
termination function).

## Refusals
None. sync_role.sh reported BUSY while my plants job ran in the role dir; I waited and synced again.

## Leftovers
Nothing running. Scratch: kanon ~/scratch/dl-init-order (probe build), ~/work/DL-INIT-ORDER/next (origin/next copy
for the counts), /mnt/zram/claude/DL-INIT-ORDER/plants-1.
