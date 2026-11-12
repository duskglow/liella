# LIBM-F32-BESSEL-LARGE handoff (run 2, LIBM-F32-BESSEL-LARGE-2: INCOMPLETE, typical rows slower)

State: branch `task/libm-f32-bessel-large-2` (on `task/libm-f32-bessel-large`, next fe4c4893d), code head 80f128b5a,
role shiki:~/w/sh1. Decision record D-10.109 written (records/decisions/D-10/D-10.109.toml). Brief:
~/state/briefs/LIBM-F32-BESSEL-LARGE.md "The change"; method and design as in run 1 (see D-10.109's body).
DO NOT LAND as it stands: the mid and tiny rows are slower than next (below).

## Done in run 2
- gate2 red fixed: `bessel_large_table_is_the_refused_set` is a `long` row of xtask/src/longsweeps.rs (36 s in
  release on Haswell-EP 26 vCPU, sweeps job 1). Oracle module comment says the range reaches FLT_MAX (#4976).
- `bessel_large` is `#[inline(never)]` (d5d5114b0): inlined, it widened bessel_eval's frame (mid +12..+27, tiny
  +12..+21 instructions a call, percall2). It also made LLVM inline `Vdso::lookup_many::<5>` in the fast libc.a,
  so `so` failed on that dead hot-text key (run 1's plants rc 2). With the band out of line the key matches again:
  my `hottext-repair` drop (44d8749f4) is reverted (80f128b5a); crates/libc-so is identical to next. No hot-lists lane.
- Plants (handoff/LIBM-F32-BESSEL-LARGE-2-plants.txt: table, margin, phase3, oracle): plants2 4/4 CAUGHT, no WARN
  (on 44d8749f4: band inlined, key dropped); plants3 rc 2 (BASELINE `so` on d5d5114b0: the dropped key, above);
  plants4 on 80f128b5a: see handoff/reports/LIBM-F32-BESSEL-LARGE-2.json.
- Exhaustive: j0f_fast_path_agrees_exhaustively ok (1976 s, release) on the inlined code; the enumeration ok (36 s).

## Counts (wl_percall, libm_bessel.c, shiki, base = next fe4c4893d; glibc / base / head 80f128b5a)
- large: j0f 296 / 176646 / 384, j1f 292 / 179299 / 385, y0f 302 / 176650 / 386, y1f 318 / 179312 / 392 (1.23-1.32x).
- mid: j0f 286.5 / 217.6 / 235.3 (+17.7), y1f 299.3 / 254.9 / 259.7 (+4.8).
- tiny: j0f 94.0 -> 102.0, j1f 119.7 -> 129.7, y0f 340.9 -> 347.9, y1f 313.6 -> 318.3 (+4.7..+10).

## Next step (continuation)
0. gate4 (80f128b5a, shiki) rc 1: only `liella::hot_text_products init_data_clusters_hold_at_every_text_phase`
   failed (clippy, static ok). The change touches no start-up code: run that test on origin/next fe4c4893d in the
   same way first; if next fails too, or it moves with code growth, it is LAYOUT-JITTER (ruling #4347: do not
   chase, hand back `INCOMPLETE: LAYOUT-JITTER` with the numbers). Not analysed in run 2 (context checkpoint).
1. Profile first: `validation/wl_trace.sh --spec validation/percall/libm_bessel.c . bes_j0f_mid j0f` on head and on
   base fe4c4893d (primed on shiki); find where the +17.7 goes. Likely: the large-band test now sits at the top of
   `bessel_eval` (every band pays its compare and the `f < 2 || x > 0` test) and the out-of-line call changes the
   shared frame or the `Option<(f32, Status)>` return. Candidate: test the band only where the mid path refuses
   |x| >= 2^19 (after the tiny and mid dispatch), so the mid and tiny paths run next's instructions.
2. Re-measure percall (typical rows <= base, large within 2x), then gate, plants and the four exhaustive sweeps
   (sweeps script: ~/work/LIBM-F32-BESSEL-LARGE-2/sweeps3.sh on shiki, one release run per row; ~33 min each).
3. Update D-10.109's measured sentence if the large rows move.
Not to redo: the table enumeration (current for LG_B = 8), the design (bound holds at 0.45 of B), the hot-text
analysis above (do not drop or regenerate keys: the dead key came from the inlined band).
