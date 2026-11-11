# handoff: libm-ld-arc-ratchet (LIBM-LD-ARC-RATCHET; follows libm-ld-arc-cycles.md)

**State.** Branch `task/libm-ld-arc-ratchet`, cut from origin/next 7d31b0621. Role keke:~/w/n1. No code change: the
brief's premise did not hold (below), so the run stopped at step 1 for a ruling. Decisions in force: D-10.52, D-10.56,
D-10.19, D-10.73, D-10.75 (unchanged; the reserved D-10.83 is not used).

**Finding: the named base has no integer tier, and the 607 / 480 levels were never landed.**
- `git cat-file -e 7a4221395:crates/math/src/wide_int/arc.rs` fails: arc.rs does not exist at 7a42213951644f6c8e3ed417cd133e777b39fb7a.
  Neither task/libm-ld-trig-inverse-int nor -2 is an ancestor of it (`git merge-base --is-ancestor`).
- percall at that base (keke:libm-ld-arc-ratchet-measure1, column `base`) shows atanl on the 384-bit route:
  atanl 161285.7, atanl_big 10690.6, asinl 175971.0, acosl 178208.6, atan2l 167574.2 instructions per call.
  Before INVERSE-INT-2 merged, next had the 384-bit route for these functions and nothing near 607.
- The 607 / 480 figures come from libm-ld-trig-inverse-int.md, `-count1` at head e476091b4. That is a commit on the
  INVERSE-INT branch. It is reachable from next but next never stood at it, and that head had the integer-root seed
  from before #4377. Rule #4512 allows measuring only a landed revision of next, so I did not measure e476091b4. The
  inverse-int handoff says atanl's path did not change between e476091b4 and 76b7b092a, and that the +50 came from
  codegen (inlining). If so, the difference is a code-placement effect and not a correctness fix. NOT VERIFIED.
- The 623 / 487 levels in the brief belong to task/libm-ld-arc-chain. When this run measured, that branch was queued
  for CI and not yet in next. next 7d31b0621 measures 632.0 / 498.4 (same as the libm-ld-arc-cycles result).
- The base trace (`wl_trace.sh <sha> ...`, run from the role directory) exits 2 with "not a git repository". The
  role is not a git tree. Tracing that base would show only the 384-bit route anyway.

**Profile of the current code (next 7d31b0621; wl_trace --list, call 11; `-measure1` log and the wlt-*.log it names).**

| phase | atanl (621) | atanl_big (483) |
|---|---|---|
| boundary: atanl, liella_atanl_bits, mathret elem/exit, mxcsr/x87 cw | ~125 | ~125 |
| `wide::atan<F80>` (decode, NaN/zero/inf, cubic_tiny 35; `round` 46 after the tier) | 81 | 81 |
| `arc::atan_approx` (inlined `atan_le_in`, `div`, `sub_from`, `norm`) | 313 | 192 |
| `atan_small` (outlined local symbol, 112 insns, 17 `mul`, called from 4 sites in atan_approx) | 105 | 88 |

The other inverse functions on next 7d31b0621: asinl 1029.0, asinl_near1 1036.5, acosl 1066.0, acosl_near1 1037.6,
atan2l 869.3, tanl 768.7, tanl_1e300 773.5 (glibc 109.3 / 125.7 / 49 / 49 / 53 / 1079.9 / 1078.7).

**RULING NEEDED.** Which target applies? (a) Re-cut as an optimisation story, not a ratchet: it starts after
LIBM-LD-ARC-CHAIN lands (it changes the same code: 623 / 487), aims at <= 607 / 480 or lower, and starts from the
profile above. The candidates are `atan_small` outlined with 4 call sites (call, frame and argument moves on each
use) and `round` in wide::atan. Recommended. (b) Allow the e476091b4 measurement under #4512 (it is not a landed
revision) to name the codegen difference.

**Next step.** After ruling (a) and the ARC-CHAIN landing: `cargo xtask so` on the role, disassemble atan_approx and
`atan_small` (local symbols present only before `bench --stage`, #4460), try `atan_small` inlined into the
`atan_le_in` copy that atanl uses (measure code size and every inverse row), then run the 10^7 x 4-mode accuracy job,
plants and percall.

**Do not redo.** The base measurement above. Rule #4512's base 7a4221395 cannot answer this question.
