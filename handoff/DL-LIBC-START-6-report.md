# DL-LIBC-START-6 report

Branch `task/dl-libc-start-6`, code head ff0309508 (handoff commit on top). Role mei:~/w/s1.

## Commits
- 9b5719179 Let a versioned relocation reference bind a base definition (crates/ld/src/symbols.rs `Want::Reference`,
  crates/ld/tests/symbols.rs rows, spec/behaviour/dynlink.md probe row, records/pending/DL-LIBC-START-6-1.toml).
- e9ad1229a Check that a program's needed version gives a relocation reference (second probe; test; plants file).
- 7f628a3d9, ff0309508 pending record format fixes (list separator, key order; static records/records-gen).

## The `__fpu_control` bug (sym___fpu_control dynamic red in -5)
- `readelf -r libc.so.6`: `R_X86_64_GLOB_DAT __fpu_control@@GLIBC_2.2.5`, symbol GLOBAL DEFAULT. So libc does not bind
  locally; the ld.so lookup was the cause.
- `liella_ld::symbols::Object::lookup` with `Want::Version(name)` (what `reference_want` gave every versioned
  reference) never accepted a base (index 1) definition. The program's `__fpu_control` is unversioned (index 1 in a
  program that has `versym`), so libc's reference skipped it and bound libc's own 0x037f.
- Probe-settled (black box, glibc 2.39 on mei; sources were libv.c/libv.map (`f@@VER_1`, `caller()` calls `f`),
  libpre.c (unversioned `f`, has versym), prog.c (+`-DOWN`: program defines `f`), progneed.c (calls `f@VER_1`)):
  plain 1; `LD_PRELOAD` unversioned 2; program-defined 3; program's needed `f@VER_1` + preload 2. Recorded as a row in
  spec/behaviour/dynlink.md "Symbol binding".
- Fix: `Want::Reference(version)` for relocation references (both the verneed and the own-definition branch of
  `reference_want`): exact version (hidden or not), else a base definition; an exact one later in the same chain wins.
  `Want::Version` (dlvsym, start.rs handshake) keeps "a base definition never matches a name" (spec dlvsym row).
- Direct check before the gate (job dl-libc-start-6-t1, rc 0): `cargo xtask so`, liella-ld `--test symbols` 9 passed,
  `sym___fpu_control` passed (dynamic, under Liella's ld.so; no skip).

## Jobs (mei)
- dl-libc-start-6-step0 rc 0: clippy + static on the inherited head d45e62e70.
- dl-libc-start-6-probe1, -probe2 rc 0: the binding probes (evidence).
- dl-libc-start-6-recnew rc 0: `records new decision D-NEW --series D-27`.
- dl-libc-start-6-t1 rc 0: so + the two direct tests (head 9b5719179).
- dl-libc-start-6-static rc 0 (head ff0309508).
- FINAL HEAD ff0309508: dl-libc-start-6-gate3 (clippy, static, affected --run), dl-libc-start-6-plants3 (7 plants),
  dl-libc-start-6-counts-head3 (cbench_startup after gate3). Results: see the hand-back / json.
- SUPERSEDED: dl-libc-start-6-gate, -gate2 (rc 1: static records/records-gen on the pending record's format);
  dl-libc-start-6-plants, -plants2, -counts-head, -counts-head2 (stopped by me with kill-label: tree changed).

## Counts
Base 07a4db6ac (dl-libc-start-5-counts-base): startup_instr_c 0.017, _printf 0.478, _setloc 0.458, _wctype 0.464,
startup_kinstr_c 0.204 (spread 1%/1%). -5 head (dl-libc-start-5-counts-head): same instr rows, kinstr_c 0.208 (2%/1%).
This change touches only the ld.so lookup (not static start-up); -6 head: dl-libc-start-6-counts-head3.

## RULING NEEDED
- Unchanged from -5: #4807, Liella's own `libc_nonshared.a` + installed `libc.so` script (recommend a product story).

## INFORMATIONAL
- Two decision ids needed: records/pending/DL-LIBC-START-6-1.toml (amends D-27.11), and a D-10.5 amends record
  (the dynamic `__fpu_control` now runs and passes; D-10.5's body still says it waits for Liella's ld).
- s23_timefmt fails check 75 on the reference (out of scope, static-only).

## Refusals
None.
