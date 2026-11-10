# LIBM-F32-ONEPASS-REST: handoff (binary32 functions onto the one-pass `float` path)

State: branch `task/libm-f32-onepass-rest-2` (LIBM-F32-ONEPASS-REST's branch with origin/next 4070276c5 merged,
ruling #4253). Role shiki:~/w/sh2. Decisions in force: D-10.72 (extends D-10.63 under D-10.67, D-10.68). Plan rows
to read: D-10.63, D-10.67, D-10.68, D-10.72. Report of this run: handoff/reports/LIBM-F32-ONEPASS-REST-2.json.

## Done (both runs)
* exp10f, logf, log2f, log10f on the one-pass path (`fast32::exp10_onepass_any`, `fast32::log_onepass::<BASE>`;
  proofs in their doc comments); tsv rows -> `mret::elem32_onepass`; their seven partial-write ALLOW entries removed.
  Run 1 evidence (code unchanged since except exp10f's test below): exhaustive 2^32 x 4 directions, 0 mismatches each;
  8/8 plants (handoff/LIBM-F32-ONEPASS-REST-plants.txt); Ivy Bridge time Liella/glibc: logf 1.16, log2f 1.18,
  log10f 0.70, exp10f 1.44 (11-07: 2.60, 2.35, -, 2.60). N100 (read this run, nozomi:f32rest-n100, rc 0, two equal
  rounds): exp10f 1.59, logf 1.60, log2f 1.43, log10f 0.82, expf 1.66.
* Run 2: CI's red (static partial-write, 7 findings in these four functions) was a STALE PRODUCT: the CI job ran 32 s
  (static only, no `so`) and walked the `libm.so.6` an earlier tree had left in its reused directory (its exp10f was the
  pre-branch code: `stmxcsr`, `cvtss2sd` at +0x32), against this branch's ALLOW list. A fresh `cargo xtask so` on the
  role: 5 findings, all allowed. Fix (17587fa67): `cargo xtask so` writes `target/liella/libm.so.6.source-digest`
  (FNV-1a of crates/, csys/, manifests, lock, toolchain, cargo config) after its check passes; `cargo xtask static`
  checks the product only when the digest matches the tree, else prints why it skipped (`so` still checks every product
  it builds). Seen on the role: static skipped the pre-change binary ("no digest"), then checked the rebuilt one.
* Run 2: exp10f's exact-power refusal as one compare (804875533, #4277): `(bits(x) - bits(1)).rotate_right(20) <=
  0x1a`, the same operand set as the two tests LLVM had compiled branchless (+5 instructions over expf's path).

## Results of run 2 (head 804875533)
* Lint shiki:f32r2-lint2 rc 0 (xtask unit test, `cargo xtask clippy`, static, `so`, static) on the code of 804875533.
* Plants shiki:f32r2-plants rc 0: 4 CAUGHT, 0 SURVIVED, 0 WARN (handoff/LIBM-F32-ONEPASS-REST-2-plants.txt).
* Gate shiki:f32r2-gate (`affected --run --base origin/next`) rc 1. It escalated itself to FULL because xtask
  changed. 15 suites, 6313 passed, 1 failed: `liella::hot_text_products
  init_coreutils_shaped_program_stays_within_its_page_fault_bound`, 59 minor faults > bound 58 (5 equal runs).
  shiki:f32r2-basefault (rc 0) ran the same test on an exact origin/next 4070276c5 copy (RAM, `cargo xtask so`, same
  fast profile): 56 faults, pass; on this branch's role: 59, fail. So THE BRANCH CAUSES IT (deterministic, not a flake).
  The program uses only libc, and the branch changes no libc code. Its `so` reports 19 hot-text keys matching no
  function section against 18 on origin/next. `target/product/fast/hot-text.matched` has 7990 keys against 8013.
  Missing on the branch: generic instances in liella_boundary such as `ret::at::<i8>`, `ret::at::<i32>`,
  `ret::found::<i8>`/`<i32>`, two `ret::arc4` instances, a `stdio::with<..mntent::next_entry..>` instance and
  several `liella_abi::pack::Packer::items` instances. A `<u8>` instance appears instead. Working hypothesis (NOT
  VERIFIED): run 1's changes to liella_boundary (s10_math_gen.rs routing) and to math alter which generic instances
  survive inlining, so hot-text keys tied to those mangled names go dead, and the functions drop into cold text.
  Pending: shiki:f32r2-sweep (exp10f exhaustive 2^32 x 4, started after the gate) and shiki:f32r2-counts (branch counts
  FMA and SSE2, the bench stage `~/work/LIBM-F32-ONEPASS-REST-2/libm.tar`, then origin/next counts from a RAM copy:
  `=== origin/next` section of ~/logs/f32r2-counts.log, #4274).

## Not done / open
* Ivy Bridge re-time of exp10f: not booked. The change is an integer test (instruction counts are reliable for it);
  this run could not cancel a booking (`ops timing-cancel 151` refused by the harness), so it booked none. The periodic
  timing sweep judges the row; target <= 1.3x.
* Booking #151 (nozomi, run 1) may still be open: the coordinator should cancel it.

## Next (in order)
0. Fix the coreutils-shaped page-fault regression (gate red above). First confirm which instance names are lost
   (`diff` of the two `hot-text.matched` files: base copy at shiki:/mnt/zram/claude/LIBM-F32-ONEPASS-REST-2/base-2,
   which ram-sweep may have removed by then; rebuild from `git archive origin/next`). Then make the branch fit the
   bound 58 without loosening it (ALWAYS FORWARD). Options: the test's own advice (tools/hottext/regen.sh with a
   coreutils program), or keep the lost instances' names stable. Re-run the FULL gate (xtask changed, so it
   escalates). Then read f32r2-sweep and f32r2-counts.
1. sinf/cosf/tanf (ALLOW `cvtss2sd %xmm0,%xmm3` at +0x25): the merge is `let xd = f64::from(x)` in
   `fast32/trig.rs::trig_core`. TRIG_MIN = 2^-26 and TRIG_MAX = 2^19 are `float`s, so test |x| on the bits first
   (`(b & 0x7fff_ffff) - 0x3280_0000 >= 0x4900_0000 - 0x3280_0000` refuses) and build xd exactly from the bits
   (`((b & 0x7fff_ffff) << 29) + (896 << 52)`, sign to bit 63): the same value and refusal set for every caller, no
   `cvtss2sd`, and subnormals no longer raise denormal-operand before refusal. Remove the three ALLOW entries; prove
   with the trig exhaustive sweeps and a plant on the bit construction (sign dropped).
2. acoshf/asinhf ALLOW entries (`cvtss2sd` at +0x3f / +0x54 in `crate::hot::{acoshf,asinhf}_nearest` paths): same
   pattern (range test on bits, operand from bits).
3. powf: many exact cases; refuse on the bits or prove the kernel exact there. Start from `onepass_scaled` and
   `log_onepass`'s bit split.

## Do not redo
* The exhaustive sweeps of logf/log2f/log10f (code unchanged since run 1). The partial-write CI red (cause fixed).
