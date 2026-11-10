# LIBM-SINF-IVB-RATCHET: handoff (sinf's Ivy Bridge loss from the bit-built trig operand, owner #4424)

State: branch `task/libm-sinf-ivb-ratchet`, net code change NONE (b04a91485 tried an Estrin split of `trig_core`'s
cos polynomial, a8acfbc5e reverted it: measured no gain). Role mei:~/w/s2. Decisions in force: D-10.67, D-10.68,
D-10.72 (unchanged; D-10.79 reserved and NOT used). Report: handoff/reports/LIBM-SINF-IVB-RATCHET.json.
Verdict: INCOMPLETE, target not reached (sinf IVB still ~1.83x; target <= 1.78x).

## Measured (kotori, Ivy Bridge E3-1265L v2, booking #155; cbench_libm rows run directly, Liella static bench and
## glibc dynamic bench interleaved, 5 rounds; ns per call)
| row | Liella before (head 463e43fcd) | Liella Estrin (b04a91485) | glibc |
|---|---|---|---|
| sinf | 27.83-27.91 | 28.00-28.16 | 15.16-15.38 |
| cosf | 27.84-27.90 | 28.01-28.18 | 15.30-15.92 |
| tanf | 34.00-34.13 | 33.66-33.85 | 41.7-42.2 |
| asinhf / acoshf | 51.7 / 42.4 | 51.7 / 42.4 | 36.5 / 29.7 |
Rounds 1-2 of the before run read 22-25 ns: CPU frequency, not code. A stack-offset sweep (setarch -R, env padding 0..1024
in 16-byte steps, job kotori:libm-sinf-ivb-ratchet-ivb-stack) showed no offset effect, and the fast runs scaled
expf, sinf and glibc's sinf by the same factor (x0.89, x0.80). Use interleaved ratios, never single-run ns, on this host.

perf stat, sinf, Liella vs glibc (same jobs): cycles 27.0M / 16.0M, instructions 53.8M / 30.1M (IPC 2.0 / 1.9),
idq.dsb_uops 53.4M / 32.0M against idq.mite_uops 0.64M / 1.2M, resource_stalls.rs 12.4M / 6.7M (46% / 42% of cycles).
cosf: the same to within 2%. Liella's sinf and cosf disassemble to the same instructions in a different schedule, both
entries 64-byte aligned, 0x200 bytes.

## Cause: what is ruled out, what fits
* NOT alignment / decoded-icache / LSD: ~99% of uops come from the DSB on both functions.
* NOT the cos polynomial's latency: Estrin (`z*K2 + z^2*(K4 + z*K6)`, one multiply shorter on the chain) measured
  +0.6% slower; reverted (NEGATIVE; add it to the saturation register's libm rows, not done here: budget).
* NOT stack/data placement (stack sweep above).
* FITS: the loop is throughput-bound (IPC ~2, RS full 46% of cycles), so instruction count is time. The bit-built
  operand added 6 instructions per call (85.2 -> 91.2, +7%, run 4's counts), sinf's time rose 3.5%. cosf, tanf and
  the hyperbolic rows gained because their old `cvtss2sd` destination was a register the bench's previous call last
  wrote (D-10.67's false dependency); sinf's old destination apparently was not, so sinf only paid the 6 instructions.
  NOT VERIFIED: the base build's register choice for sinf (needs the base tree's disassembly: kanon
  ~/work/LIBM-F32-ONEPASS-REST-4/base.tar).

## Next step (exact)
1. Confirm the register story: disassemble sinf and cosf in run 4's base tree; sinf's `cvtss2sd` destination should
   be a register the bench loop does not write, cosf's one it does.
2. Fix: a 1-instruction operand without a false dependency, `cvtss2sd %xmm0,%xmm0` (destination = source, which the
   partial-write check accepts as owned; xtask/src/partial_write.rs). LLVM ties the destination only when x is dead
   after the conversion, and x stays live for the refusal paths, so plain Rust cannot force it. Options:
   (a) the export's boundary wrapper converts with inline asm (`UNSAFE(...)` in liella-boundary) and passes the
   `double` into a `trig_core_d(b, xd)` (interface change for sinf/cosf/tanf/sincosf);
   (b) `xorps` zero idiom + `cvtss2sd` from asm, same placement. Expect sinf back to ~85 instructions and <= 1.78x;
   cosf/tanf keep their gain (no false dependency either way). Time IVB and N100 (sinf, cosf, tanf, asinhf, acoshf)
   interleaved; counts.

## Do not redo
* The DSB/MITE and RS counters above, the stack-offset sweep, the Estrin trial.
