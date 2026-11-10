# LIBM-PARTIAL-WRITE-CHECK: handoff (partial xmm write check on libm hot paths)

State: branch `task/libm-partial-write-check` (cut from origin/next 2bc03ede6). Code at a8de8c163 (759cffc07 check +
a8de8c163 clippy fix); this note and the report json are on top. Role keke:~/w/n2. Decision in force: D-10.68 (follows
D-10.67). Plan rows to read: D-10.67, D-10.68.

## What landed
* `xtask/src/partial_write.rs`, `cargo xtask partial-write [<libm.so.6>...]`. It walks every libm.so.6 export from its
  entry, takes both outcomes of each branch (so it covers the FMA and SSE2 builds), and stops at the first call,
  return or indirect jump; at a join it keeps only the registers owned on every incoming path. It reports each
  merge-semantic scalar SSE instruction (list and reasoning from the SDM and the optimization manual are in the
  module doc) whose destination (for VEX forms, the first source) is not owned on that path. Owned means one of
  three things: written earlier on the path, an argument register (taken from the Rust signature; unknown types count
  as all 8), or also a source of the same instruction. Output format: `func+0xoff: insn (why) [hot path of export]`.
* Wiring: `cargo xtask so` runs the check after it publishes libm.so.6 and fails on any finding not in `ALLOW`.
  `cargo xtask static` (check name `partial-write`) runs it when target/liella/libm.so.6 exists and otherwise prints
  why it skipped (a static run on a fresh tree has no product). `jcc::split_insn` is now `pub(crate)` so the two
  checks share it.
* `ALLOW` holds the 12 findings present on next, each with a reason. It is a ratchet: a new finding fails `so`, and
  a fix that changes the register or the instruction leaves its entry dead, so delete the entry with the fix.

## Findings on origin/next (keke:libm-partial-write-check-findings-next, binary = `cargo xtask so` of 2bc03ede6)
791 exports walked, 12 findings, all `cvtss2sd`/`cvtsi2sd` merges into a register the call has not written:
acoshf+0x3f `cvtss2sd %xmm0,%xmm2`; asinhf+0x54 `cvtss2sd %xmm1,%xmm2`; cosf/sinf/tanf+0x25 `cvtss2sd %xmm0,%xmm3`;
exp10f+0x32 `cvtss2sd %xmm0,%xmm1`; logf+0x39, log2f+0x39, log10f+0x39 `cvtss2sd %xmm0,%xmm1`; logf+0x95, log2f+0x9d,
log10f+0x9d `cvtsi2sd %ecx,%xmm2` (exponent conversion). expf/exp2f are clean after D-10.67.

## Not done (budget: 60 calls) -> LIBM-F32-ONEPASS-REST
* No finding was fixed or timed in this run. Each fix is cheap. For the cvtss2sd ones, build the operand from bits
  (D-10.67's `onepass_scaled`) or put a zero idiom first. For the log family's `cvtsi2sd %ecx`, write the double
  exponent from bits, or zero the register first (`xorps`/`pxor`). The brief requires booked timing on Ivy Bridge and
  the N100 for each fix. Remove the matching `ALLOW` entry with each fix.
* The check's limits: it walks to the first call only, and a signature with an unknown type (F128Bits) treats xmm0-7
  as arguments, which can hide a finding.

## Plants (handoff/LIBM-PARTIAL-WRITE-CHECK-plants.txt, keke:libm-partial-write-check-plants)
`expf-cvtss2sd-restored` puts back the pre-D-10.67 operand, and `cargo xtask so` should fail on the expf finding.
Five checker mutations should each fail one unit test: forgets writes (the zero-idiom break case), walks past a
call, drops cvtss2sd, follows only one branch outcome, checks the VEX destination instead of the merge source. The
brief's "pxor break passes" and "after a call is not reported" cases are unit tests on synthetic objdump text
(`zero_idiom_break_passes`, `merge_after_a_call_is_not_reported`). Rust source cannot plant a pxor into the product.

## Jobs
keke:libm-partial-write-check-lint2: clippy, unit tests, check, static; rc 0 on a8de8c163. Superseded:
libm-partial-write-check-lint (clippy: unused field), libm-partial-write-check-findings-next (rc 1 on purpose: it is
the findings run on next, before the allow-list). Started detached at hand-back: libm-partial-write-check-gate
(affected --base origin/next) and libm-partial-write-check-plants.

Next step: LIBM-F32-ONEPASS-REST fixes the 12 findings and times each one (Ivy Bridge + N100). It deletes the ALLOW
entries as it goes.
