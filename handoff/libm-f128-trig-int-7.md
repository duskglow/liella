# handoff: libm-f128-trig-int-7 (LIBM-F128-TRIG-INT-7, continues -6)

**State.** Branch `task/libm-f128-trig-int-7` (on -6's branch, merged with origin/next 07a4db6ac+), role mei:~/w/s4.
The asinf128/acosf128 three-limb tier (`crates/math/src/wide_int/quad/arc.rs`, D-10.97) is unchanged from -6;
this run only merged next and fixed the merge fallout. The route, proof and counts are in
`handoff/libm-f128-trig-int-5.md` and `handoff/libm-f128-trig-int-6.md`; not repeated here.

**Why -6 went red.** CI's STARTUP-DATA-WINDOWS read 21 written start-up pages > 20 (layout jitter, not the kernels).
LAYOUT-DATA-CLUSTERS-4C has landed on next since; on the merged head the same check reads **14 written pages
(budget 20), 1 read-only window (budget 1)** (job `-check2`). No section moved, no bound changed.

**Done in -7.**
* Inherited head 1f74d0d68: `libm-f128-trig-int-7-lint0` (xtask clippy + static) rc 0.
* Merge of origin/next (bc391541a): one conflict, `crates/math/src/wide_int/arc.rs`: next moved `big_u64` to
  `wide_int.rs` (now `pub(crate)`, with a debug range assert). The arc copy is dropped and `quad/arc.rs` imports
  `big_u64` from `wide_int` (its operands, `[2^62, 2^64]`, are inside the assert's range). `s10_math.tsv` and the
  generated `s10_math_gen.rs` merged cleanly; `cargo xtask math-gen` on the role left it unchanged.
* 8b607be23: next's new `float-casts` static check (D-10.95) flagged `quad/arc.rs` step 9's `f64 as usize` index;
  a `RANGE:` comment states it (y', x' in `[2^52, 2^53)`, ix ≈ 256/x': positive, below 2^10).
* `-check2` on 8b607be23 rc 0: math-gen unchanged, xtask clippy, static, `cargo xtask so`,
  `cargo test -p liella --test hot_text_products` 21/21.
* `handoff/LIBM-F128-TRIG-INT-7-plants.txt`: -6's six plants (every pattern still matches exactly one site).

**Jobs (mei), on 481058bd3** (code identical to the final head; later commits are handoff/report only):
`libm-f128-trig-int-7-gate` (direct tier, `--base origin/next`), `libm-f128-trig-int-7-plants`
(/mnt/zram/claude/LIBM-F128-TRIG-INT-7/plants-1), `libm-f128-trig-int-7-percall` (wl_percall,
libm_f128_trig.c, `--base none`).
* `-plants` rc 0: 6 CAUGHT, 0 SURVIVED, 0 WARN (each by its named message).
* `-percall` rc 0, instructions per call, glibc / -6 (a60ccf957, keke) / -7: asinf128 9271.5 / 1354.8 / **1341.0**
  (0.14), acosf128 9604.8 / 1390.8 / **1375.2** (0.14).
* `-gate` rc 0 (direct tier, `affected: ok`, peak 3.5G, 36 min; `test -p liella-math` 1286 s; 2 SKIPPED lines).

**Superseded.** `-check1` (rc 1: float-casts static, fixed in 8b607be23).

**Open / next step.** 1. If any job above is red, fix it (gate: run only the failing suite). 2. Booked timing on
Ivy Bridge and N100 for asinf128/acosf128 (recipe in `handoff/libm-f128-trig-int-6.md`), still not run.

**Do not redo.** The route, the proof, the plants, the -6 counts, the merge.
