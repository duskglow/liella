# handoff: libm-f128-trig-int-5 (LIBM-F128-TRIG-INT-5)

**State.** Branch `task/libm-f128-trig-int-5` (on -4's branch), role mei:~/w/s2. Run 1 stopped at the context
checkpoint (~150k). Code commit (WIP subject) adds asinf128/acosf128 to the three-limb tier:
`crates/math/src/wide_int/quad/arc.rs` (`asin`, `acos`, `root_one_minus_sq`, `root3`, `atan_le3`, proof steps
7-9 in the module doc), dispatch in `crates/math/src/wide_int/arc.rs` (`F::PREC == 113`), `big_u64` made
`pub(super)`. Plants file `handoff/LIBM-F128-TRIG-INT-5-plants.txt` (5 plants).

**Route (chosen by estimate, check with the percall).** atan2 route: exact `1 − a²` (A² in 256 bits), a root
from the `double` seed, one 128-bit and one 192-bit Newton step (the reciprocal refined once: `2^-98.8`),
root within `2^-186`, then -4's kernel on 192-bit operands (`atan_le3`: `y − cx`, `x + cy` on significands /4).
Estimate ~1 450 instructions against glibc ~9 300 (-4's percall). A direct asin kernel was rejected: centres
with their own Taylor coefficients need ~20 terms of 192 bits per centre (tens of KB) and a root above 1/2
anyway. Proved bound `2^-160.2`; rounding test assumes `2^-152`.

**Jobs run (mei).** `libm-f128-trig-int-5-t1` (code commit): `cargo test -p liella-math --lib wide_int::quad`
all pass; asin/acos 3 200 calls 0 undecided worst `2^-162.15`, hard cases 21 120 calls 0 undecided worst
`2^-161.77`. Started detached on head cc573bd93 (code unchanged since): `-lint` (xtask clippy, static, then the
10^7 release sweep `arc_sine_agrees_with_the_wide_route_long`), `-plants`
(/dev/shm/claude/LIBM-F128-TRIG-INT-5/plants-1), `-percall` (libm_f128_trig.c). Read their logs first.

**Open / next step (in order).**
1. Read `-lint`, `-plants`, `-percall` results (`~/logs/<label>.log` on mei). Fix what they find.
2. The f128 boundary still passes `soft_sqrt` (integer isqrt) as `hw`: change `crates/boundary/src/posix/s10_math.tsv`
   rows `asin fq` / `acos fq` into `f` (unchanged) and `q` (`math::wide::asin_sqrt(v, m, crate::hot::sqrtsd)`,
   as the `l` rows), regenerate with `cargo xtask math-gen` on the role (generated file: never hand-edit), recount.
3. Decision record: `cargo xtask records new decision D-10.97` (reserved), body = the route and proof summary
   above (cite Tang ARITH 10 1991; Brent & Zimmermann ch. 3-4; Muller ch. 2), `records check`.
4. Commit with a non-WIP subject (new commits only, never --amend), sync, `cargo xtask affected --run --base
   origin/next` under ~/bin/gate, `cargo xtask so` + `hot_text_products`, re-run plants and percall on the final
   head, stage and book timing on Ivy Bridge (nico/kotori, less busy) and N100 (nozomi).

**Do not redo.** The route estimate; the first test run (t1) passed as written.
