# handoff: libm-ld-trig-int (LIBM-LD-TRIG-INT, -2)

**State.** Branch `task/libm-ld-trig-int-2` (from `task/libm-ld-trig-int`, cut from origin/next bb7f6ae1c). Role
mei:~/w/s4 (Skylake-SP build host). Code: 07202bd37 (kernel + wiring), 628a2ce84 (test fix); -2 added the percall spec
and plants file (a3abee27b) and the D-row, saturation section 25 and this handoff (no code change in -2).

**Results (-2).**
* Inherited gate `kanon:libm-ld-trig-int-gate` (628a2ce84 code): rc 0, 7654 passed, 0 failed, peak 15G.
* Step-0 lint on the inherited head `mei:libm-ld-trig-int-2-lint`: rc 0.
* Percall `validation/percall/libm_ld_trig.c` (Skylake-SP, instructions:u, sd 0): sinl 582.2 (glibc 1127.2), cosl
  583.3 (1126.7), near 1e10 588 (1105), near 1e300 587 (1094), sincosl 1080.6 (1227.5); all self-checks ok.
  Base bb7f6ae1c (primed on mei with wl_prime_base.sh): sinl 82 228.5, cosl 82 227.4, sincosl 164 371.
* 10^7 sweep (release, `tier_agrees_with_the_wide_route_long`): 8x10^7 calls, 0 undecided, 0 refused, every
  result equal to the 384-bit route, worst error 2^-95.68 (bound 2^-94); near multiples of pi/2 at every exponent:
  131 048 calls, 65 524 undecided by design (one of sin/cos per operand), worst 2^-117.32, closest |f| 2^-75.
* Plants `handoff/LIBM-LD-TRIG-INT-plants.txt`: 4 CAUGHT (table bit, quadrant, bound halved, carry), no WARN.
* Profile (wl_trace, call 11 of sinl, 560 instructions): kernel 371 (66%), `wide::sin` 86, wrapper+symbols 103.
* Cycles per call (perf stat -r 5, (C(N) - C(0))/N, N = 200 000, 3 interleaved rounds, medians, booked idle hosts,
  binaries of a3abee27b = the final head's code): Ivy Bridge (nico, E3-1220L v2) sinl 264.9 (glibc 547.6, 0.48x),
  cosl 263.6 (551.3), 1e10 ~277 (~551, 0.50x), 1e300 ~276 (~551, 0.50x), sincosl 466.8 (630.2, 0.74x). N100
  (nozomi) sinl 312.2 (glibc 565.0, 0.55x), cosl 313.4 (563.2, 0.56x), sinl_1e10 322.0 (566.0), cosl_1e10 323.6
  (565.5), sinl_1e300 324.0 (571.9), cosl_1e300 324.3 (573.0) (0.57x), sincosl 580.1 (715.8, 0.81x). The N100
  table is not in saturation section 25 (it finished after that commit): move it there in the next story.
* Final head 87dc43940 (code = 628a2ce84's; -2 added docs, spec, plants): mei:libm-ld-trig-int-2-lint-final,
  -sweep-final, -plants-final, -percall-final3 rc 0; -gate (direct tier, --base origin/next) see the hand-back.
  Superseded: -percall-final (rc 2: role synced dirty by the untracked report json), -percall-final2 (its --rev
  named a mistyped sha; same figures). Bookings 117/118 cancelled.

**Decisions in force.** D-10.52 (the tier), D-10.56 (method and measurements; id reserved by the coordinator),
D-10.19 (correct rounding), D-10.25 (dispatch pattern; plain `mul` only).

**Open / next step (target 300 instructions not reached: 582).** In expected order of gain (saturation section 25):
(1) `sincosl` reduces twice (1081 = 2 x 540): one reduction and table step for both; (2) the shared x87 entry
(wrapper, decode, special cases: 189 of 560, every x87 function pays it); (3) the kernel's variable-count 128-bit
shifts (normalise d, then shift u and S back) where j >= 1 needs only absolute precision; (4) two checks per table
word in `reduce`. Then the other long double families on the same kernel (D-10.52).

**Do not redo.** The design, constants and proof (module doc of wide_int.rs); the continued-fraction search is in the
tests. Timing binaries: `~/work/LIBM-LD-TRIG-INT-2/timing/` on mei, nico, nozomi (`run.sh <dir>`).
