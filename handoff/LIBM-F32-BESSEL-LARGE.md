# LIBM-F32-BESSEL-LARGE handoff (run 3, LIBM-F32-BESSEL-LARGE-3: PENDING JOBS)

State: branch `task/libm-f32-bessel-large-3` (stacks on -2; base next fe4c4893d, not merged), role kanon:~/w/kn1.
Decision in force: D-10.109 (records/decisions/D-10/D-10.109.toml, measured sentence updated in run 3). Spec: the
original brief ~/state/briefs/LIBM-F32-BESSEL-LARGE.md "The change".

## Done in run 3
- step 0: clippy and static green on the inherited head 482a14d0f (job libm-f32-bessel-large-3-lint0, kanon).
- gate4 red (`init_data_clusters_hold_at_every_text_phase`): reproduced on kanon (diag1: `tool` 55, 54 at `data
  0x3000`). The branch's writable object was `const LG_TAB: [&[u64]; 4]`: indexed at run time it is an array of
  four slice pointers, which a PIC build places in `.data.rel.ro` (relocated at start-up). Now one `static LG_ALL:
  [u64; 202]` of plain words built at compile time from the four tables, with `LG_OFF` offsets (`lg_tab(f)`): .rodata
  only. Test after the fix: passes, `tool` 54 at all 10 phases (uniform; diag2, diag3). Test and bound untouched.
- mid/tiny rows: run 2's band test sat ahead of the fast path. Tried (diag2) the band from bessel_core's refusal
  inside `bessel_eval`: mid j0f +10, y1f +8, tiny +1..+7 (a second call on the inlined refusal path keeps x/mode live
  across it). Kept (diag3): `bessel_eval` is next's code again; `special::{j0,j1,y0,y1}` for `float` make ONE
  out-of-line call `bessel32_rest(n, name, x, mode)` (band via `fast32::bessel_large`, else `*_accurate`) where they
  called `*_accurate`: same cold-path shape as next. Percall kanon (base next fe4c4893d / head): mid j0f 217.6/217.6,
  y1f 254.9/254.9; tiny j0f 94.0/94.0, j1f 119.7/119.7, y0f 340.9/340.9, y1f 313.6/313.6; large j0f 176646/405.5,
  j1f 179299/407, y0f 176650/420, y1f 179312/423.5 (glibc 292-318: 1.33-1.39x).
- Tests: `bessel_large_band_edges` (2^19 edges, FLT_MAX, inf, NaN, Y at x < 0), `bessel32_rest_is_each_functions_path`;
  fast32/tests.rs `fast()` = eval then the band for Bessel, used by `agree_oracle`, so the exhaustive sweeps still
  cover the band. Math Bessel unit tests 11/11 (release, diag3).

## Jobs on the final head (kanon)
gate libm-f32-bessel-large-3-gate, plants libm-f32-bessel-large-3-plants (handoff/LIBM-F32-BESSEL-LARGE-3-plants.txt:
yneg, offset, restmap, table, margin), percall libm-f32-bessel-large-3-percall, sweeps libm-f32-bessel-large-3-sweeps
(the enumeration + four exhaustive sweeps, release). Diagnostic jobs diag1-3 are evidence, not final.

## Open / next
- jnf(0|1, x) for |x| >= 2^19 still takes the accurate path (as on next): fast_bessel_n::jnf calls fast32::eval
  only (ynf not checked). Follow-up: give its refusal the same one-call rest path (measure the jnf rows first).
- If a job is red: read its log on kanon (~/logs/<label>.log); fix with new commits; rerun only the failing job.
Not to redo: the table enumeration (current for LG_B = 8), the design, the dispatch experiment above.
