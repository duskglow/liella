# LIBM-CEXP-COLD-MIDDLE handoff (run 1: blocked, no code change)

State: branch task/libm-cexp-cold-middle, cut from origin/next 9042cc44f; only this note and the report json.

## Why nothing was changed

The brief fixes forward LIBM-CEXP-ROUTE's middle tier (ruling #4885: `fused::cexp_nearest` spills x for the
inline middle-tier call; cexp typical 396 -> 397). At dispatch (2026-11-11 23:30Z) that code was NOT in next:

* `git merge-base --is-ancestor origin/task/libm-cexp-route-2 origin/next` -> not an ancestor (route-1 neither);
* `ops log LIBM-CEXP-ROUTE-2`: queued_ci, unheld 23:29:35Z (ruling #4885), not landed;
* next's `crates/math/src/complex_fast.rs` has no middle tier: `cexp_nearest` is `parts` + two `round`s, falling
  back to `axis` and then to `mathret::cplx_fast_cold` -> `cplx` -> the 384-bit route.

The brief is `Stacks-on: next` and names no file on the route branch, so the rules allow neither reading that
code (`git show` exception covers only named files) nor merging it ("no merge/rebase unless your brief allows").
Waiting for the train in this run would mean polling, which the rules forbid.

## Exact next step

Re-dispatch after LIBM-CEXP-ROUTE-2 lands (`ops hold LIBM-CEXP-COLD-MIDDLE --until-landed LIBM-CEXP-ROUTE-2`), or
re-point it with `ops stack LIBM-CEXP-COLD-MIDDLE LIBM-CEXP-ROUTE-2` if the fix should ride with the route story.
Then: in `crates/math/src/complex_fast.rs` (both builds through `cexp_kernels!`) leave the hot `cexp_nearest`
returning `None` on an undecided part, and call the middle tier from the cold side (`mathret::cplx_fast_cold`
or a `#[cold] #[inline(never)]` helper reached only from the declined branch) so x needs no spill in the hot
kernel. Measure cexp typical / cexp_bench / cexpf percall on the same host as primed next.

## Plan rows to read

Ruling #4885; D-10.53 (cexp quick tier); D-10.74 (axis path); LIBM-CEXP-ROUTE-2's handoff once landed.
