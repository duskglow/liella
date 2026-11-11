# LIBM-INVALID-CONVERT-SCAN (-2, -3): handoff

State: branch task/libm-invalid-convert-scan-3 (stacked on -2; origin/next 672acd9aa merged in -3, LIBM-LD-ARC-DIRECT
included), head = last commit on the branch. Decision in force: D-10.95 (records/decisions/D-10/D-10.95.toml).
Verdict of -3: INCOMPLETE. One count row is still above base (lgammaf, below); everything else is done and measured.

## Sites (scan by type: the `float-casts` static check, 14 casts in scope, each with a range)
| site | function | operand range | fix |
|---|---|---|---|
| wide_int/arc.rs root_seed, sqrt | asinl, acosl | [2^63, 2^64] every call | `wide_int::big_u64` (ARC-DIRECT landed the same helper; one helper kept) |
| wide_int/arc.rs div | atan2l, asinl, acosl | (2^62, 2^63] | big_u64 |
| wide_int/arc.rs soft_sqrt | internal | [2^52, 2^54) | big_u64 |
| wide_int/quad.rs div3 (new on next, found by float-casts in -3) | tanf128 | (2^62, 2^63], 2^63 when the divisor's top word < 2^63+2^11 | big_u64 (moved to wide_int.rs) |
| fast32/gamma.rs eval_lgamma sign | lgammaf, lgammaf_r | any float | parity of trunc(x) from the bits (`trunc_is_odd`, -3) |
| core/num/parse.rs finish (#4627) | strto* | abs < 5000 | RANGE: (no invalid possible) |
| dd.rs, fast32/bessel.rs, fast_bessel_n.rs, mp.rs, special.rs | various | guarded | RANGE: |

## Results on keke (-3)
- lint2 (head 56b7003): clippy, static, records, flags test (2 tests) green. lint3 (head 1e36c93): workspace clippy green,
  float-casts green; static red on records only (a bare `|` in D-10.95's body), fixed in the handoff commit, NOT re-run.
- Accuracy (keke:libm-invalid-convert-scan-3-accuracy, release, 10^7 operands x 4 modes): arc atan/asin/acos/atan2/tan
  0 refused, undecided 0/4/16/0/0, worst 2^-103.38/-103.13/-103.20/-103.38/-96.85; quad (tanf128 etc.) 2*10^8 calls,
  0 undecided, worst 2^-158.26. lgammaf: the value path is unchanged (only the sign is computed differently) and the
  sign equals the old conversion's parity for every float (proof in D-10.95), so no 4*2^32 exhaustive run.
- Counts (wl_percall vs base 672acd9aa = next, instructions/call): atan2l 863.3->860.3, acosl 770.6->767.6, asinl
  742.5->739.5, atanl 622.7->619.7, tanl 768.7->765.7 (all -3), tanf128 1703.5->1702.5 (-1), sin/cos/sincosf128 equal.
  lgammaf (new spec validation/percall/lgammaf.c): -2's clamp measured pos 315.8->328.8 (+13), neg +1; -3's bit read
  pos 315.8->318.8 (+3), neg 8411.0->8413.1, lgammaf_r_neg 8419.0->8421.2 (+2). STILL ABOVE BASE: next step 1.
- Plants (handoff/LIBM-INVALID-CONVERT-SCAN-plants.txt, \d replaced by [0-9]+): plants-1 (stale head, stopped) showed
  seed-as-u64-flags, seed-as-u64-static, div-as-u64-flags CAUGHT with their messages. plants2 and gate2 on head
  1e36c93 were still running at hand-back (keke labels libm-invalid-convert-scan-3-plants2, -gate2): read them.

## Next steps (in order)
1. lgammaf +3/+2: the parity bits are if-converted and run before `x < 0` on every call. Try computing the sign only in
   the negative branch the fast path already takes (look in `eval` for where x < 0 is known, e.g. the reflection
   step) or a form LLVM keeps under the branch; re-count with `wl_percall.sh --spec validation/percall/lgammaf.c`
   until every row is at or below base. Then re-run gate, plants (gamma-parity-shift is the gamma plant), lint.
2. Disassembly check: ~/work/libm-invalid-convert-scan-3/disasm.sh on keke (lists per function the truncating
   conversions and the unsigned-64 sequence, cvtt*2si beside `sar 0x3f`) after `cargo xtask so`; expect none in the
   five fixed functions. Script also in the -3 scratch; not run.
3. liella::hot_text_products on the merged head; booked Ivy Bridge cycles for asinl/acosl/atan2l (the root-seed stall
   was ARC-DIRECT's to time, #4614; this branch changes 1-3 instructions per row, so the periodic sweep may suffice:
   coordinator's call).

## INFORMATIONAL
- wl_percall's check column reads MISMATCH for lgammaf(-2.5) (`%.9g` of the value) against the reference; the value
  path is not touched by this story (base had the same code). lgammaf on negative operands is 21x the reference by
  instructions (8411 vs 395): a performance story of its own.
- strto* flags gap: STRTO-FENV-FLAGS (#4639), not this story.
