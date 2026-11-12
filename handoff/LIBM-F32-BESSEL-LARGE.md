# LIBM-F32-BESSEL-LARGE handoff (run 4, LIBM-F32-BESSEL-LARGE-4: PENDING JOBS)

State: branch `task/libm-f32-bessel-large-4` = run 3's branch (`task/libm-f32-bessel-large-3`, 7e4cbc7ce) merged with
next d1b3a596c (merge commit 3cf2ccf6c); role kanon:~/w/kn1. Decision in force: D-10.109
(records/decisions/D-10/D-10.109.toml, unchanged in run 4). Spec: ~/state/briefs/LIBM-F32-BESSEL-LARGE.md "The change".

## Done in run 4 (the merge; no product code changed)
- Step 0: clippy and static green on the inherited head 7e4cbc7ce (job lint0).
- Merged origin/next (57 commits). One conflict, crates/math/src/fast32/tests.rs `agree_oracle`: next gave it a getter
  `get` and added `exhaustive_oracle_by` (erff/erfcf tiny paths, D-10.107); run 3 had made it call `fast` (eval, then
  the Bessel large band). Resolved: next's `agree_oracle` and `exhaustive_oracle_by` as they are; run 3's eval -> fast
  change now sits in the getter `exhaustive_oracle` passes (its only callers are the four Bessel sweeps; `fast` is
  `eval` for every other function). Both sides' tests kept; fast32.rs auto-merged; nothing else changed by hand.
- Merged head: `cargo xtask clippy`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo xtask static`: rc 0 (lint1).
- Layout (job layout: `cargo xtask so`, then hot_text_products --nocapture, fast profile): 23/23 pass;
  `init_data_clusters_hold_at_every_text_phase` reads `tool` 52 at all 10 phases (uniform, so run 3's .rodata table
  fix holds). 52, not run 3's 54: next's LAYOUT-STARTUP-STATE-LOCALE records tool 54 -> 52 (fast profile) in its
  handoff. Next alone was not measured on kanon in this run.
- Plants (handoff/LIBM-F32-BESSEL-LARGE-4-plants.txt, with the tool's results): run 3's five plus sweepband (the band
  core's answer negated; caught by a LIELLA_SWEEP_STRIDE=65537 j0f sweep, so the sweeps reach the band through the
  merged getter; with next's `eval` getter it would survive): 6 CAUGHT, 0 SURVIVED, 0 WARN.
- Gate (direct tier, --base origin/next): 32 suites ok; 11656 passed, 0 failed, 1270 ignored; conformance 125/125;
  `gate: peak memory 16G (page cache included; reclaimed under the limit) oom_kill=0 rc=0`.
- Percall (percall2, kanon, instructions per call, glibc / base next d1b3a596c / head c3b6a032d): large j0f
  296.2/176646.3/405.5, j1f 291.7/179299.4/407.0, y0f 302.1/176650.3/420.0, y1f 317.8/179312.4/423.5 (1.33-1.40x
  glibc); mid j0f 286.5/217.3/217.3, y1f 299.3/254.7/254.7; tiny j0f 54.3/94.0/94.0, j1f 67.8/119.7/119.7, y0f
  98.2/340.9/340.9, y1f 64.5/313.6/313.6 (mid and tiny equal to next). Value checks: base and head give the same
  value on every row; 6 rows print MISMATCH against glibc on next and the head alike (not from this branch).

## Jobs on the final head (kanon)
lint1, layout, plants, gate, percall2, sweeps (bessel_large_table_is_the_refused_set and the four exhaustive sweeps,
release; about 2 h in run 3; still running at hand-back). Superseded: percall (rc 2 at once: its base, next
d1b3a596c, was not primed on kanon; primed with validation/wl_prime_base.sh, then percall2). Scripts on kanon:
~/work/LIBM-F32-BESSEL-LARGE-4/final.sh (stopped by PID after it started the gate) and final2.sh (gate, percall2, sweeps).

## Run 3's design and levels (kept)
The band runs from bessel_core's refusal through one out-of-line call `bessel32_rest(n, name, x, mode)` in
special::{j0,j1,y0,y1} for `float`; table `static LG_ALL: [u64; 202]` of plain words (.rodata) with LG_OFF offsets.

## Open / next
- jnf(0|1, x) for |x| >= 2^19 still takes the accurate path (as on next): fast_bessel_n::jnf calls fast32::eval only
  (ynf not checked). Follow-up: give its refusal the same one-call rest path (measure the jnf rows first).
- If the sweeps job is red: read ~/logs/libm-f32-bessel-large-4-sweeps.log on kanon; fix with new commits; rerun only it.
Not to redo: the table enumeration (current for LG_B = 8), the design, run 3's dispatch experiments, the merge.
