# handoff: libm-ld-trig-inverse-int (LIBM-LD-TRIG-INVERSE-INT, -2)

**State.** Branch `task/libm-ld-trig-inverse-int-2` (run 1's WIP plus run 2; origin/next 628d4406f merged in), role
shiki:~/w/sh2. The x87 integer tier for atanl/atan2l/asinl/acosl/tanl is in `crates/math/src/wide_int/arc.rs`
(child of `wide_int.rs`; method and proof in its module doc), wired in `crates/math/src/wide.rs` ahead of each
384-bit route. Decision D-10.75 is written (`records/decisions/D-10/D-10.75.toml`). Nothing in the merged
LIBM-LD-EXPLOG-INT-3 kernels (`wide_int_el.rs`) duplicates arc.rs's division or root; both share `wide_int`'s
`mulhi`/`mul_wide`/`round`.

**Run 2 commits.** (1) Test fix: unit4's `error 2^NaN` was atan2(−1, LDBL_MAX), a pair that `wide::atan2`'s
tiny-ratio path (x > 0, |y/x| < 2^-72) handles before the tier; the exact value (2^-16384) underflows `to_f64`.
`check()` now skips that case for the sweep and the fixed points alike. (2) D-10.75 record, plants file, percall
probe `validation/percall/libm_ld_arc.c`. (3) The asinl/acosl root seed comes from `sqrtsd` passed in from the
boundary (#4377): `wide::asin_sqrt`/`acos_sqrt`, x87 rows of `s10_math.tsv` pass `crate::hot::sqrtsd`, regenerated
`s10_math_gen.rs`; the crate's own callers use `soft_sqrt` (truncated integer root). Proof step 2 redone for any
`double` root within 2^-52: root 2^-101.4, asin/acos 2^-100.1 (was 2^-113.8 / 2^-100.5 with the exact integer
root); the root test checks floor, nearest and ceiling roots. `finish` now checks the format before computing.

**Results (shiki, head 76b7b092a for code; later commits touch handoff/ and the record only).**
- `libm-ld-trig-inverse-int-2-check3`: static ok, wide_int unit tests 10 passed (arc worst 2^-103.7..-105.3, tan 2^-97.9, root 2^-103.7).
- `-sweep`: 10^7 operands per function × 4 modes vs the 384-bit route: 0 mismatches; undecided 0/0/4/8/0 of 4·10^7
  (atan, atan2, asin, acos, tan); worst 2^-103.4 arcs, 2^-96.85 tan.
- `-plants2`: 7/7 CAUGHT, no WARN (`handoff/LIBM-LD-TRIG-INVERSE-INT-2-plants.txt`). `-plants` refused (a bare `|`
  in a `lit:` pattern; fixed).
- `-count2` (instructions per call, glibc / base / new): asinl 109 / 175971 / 1029; acosl 49 / 178209 / 1066;
  atan2l 53 / 167574 / 869; atanl 26 / 161286 / 657 (atanl_big 26 / 10691 / 530); tanl 1080 / 89263 / 769.
  `-count1` (head e476091b4, integer-root seed): asinl 1189, acosl 1237, atanl 607, atanl_big 480.
- `-gate` (direct tier, `cargo xtask affected --run`): see the report json.

**Open.** (a) atanl rose 607 → 657 (atanl_big 480 → 530) between e476091b4 and 76b7b092a although its path did not
change (atan2l and tanl identical): probably inlining moved when asin/acos got their own instantiations; not
profiled. (b) Timing in cycles not done (x87 rows are judged in cycles; the reference counts are 26-125 instructions, so
the instruction ratio does not decide).

**Next step.** Profile atanl's +50 (validation/wl_trace.sh on atanl, the two heads) and pin the inlining if that is
the cause; then book timing (Ivy Bridge: nico or kotori, whichever is less busy; and N100) with `cargo xtask bench
--stage` and the cbench_libm rows asinl/acosl/atanl/atan2l/tanl, target <= 1.5x the reference in cycles.

**Do not redo.** Constants pinned by `arc_constants_are_the_rounded_values`. Decisions in force: D-10.52, D-10.56,
D-10.19, D-10.73, D-10.75.
