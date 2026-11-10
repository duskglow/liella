# STRCASECMP-N100-REGRESS: N100 strcasecmp 256-byte rows 0.80x -> 1.55x glibc (escalation #4182)

State: branch task/strcasecmp-n100-regress, role keke:~/w/n3. Verdict INCOMPLETE (one row slower, plants not run).
Decision in force: D-4.70 (amends D-4.67). Plan rows to read: D-4.67, D-4.70, D-5.5, D-5.6.

## Landing and mechanism
* Landing: 8e0caad2c "Keep the hot machine assembly's jumps off 32-byte boundaries" (MACHINE-JCC-PAD, D-4.67),
  flag `-Wa,-malign-branch-prefix-size=5`. Its own timing covered cbench_memref/cbench_hot only, not cbench_locale.
* Mechanism (objdump of scan.S assembled with each flag set, keke ~/work/STRCASECMP-N100-REGRESS/asm/): with prefix
  size 5 the assembler puts `cs cs cs cs` on the four head instructions of `.Lcord_top64` in
  liella_machine_casecmp_ord_avx2 (lea, lea, and, plus one `cs` on the fourth), run once per 64 bytes, and on two
  instructions of `.Lcord_head_exact` (taken by the pinned page-aligned case_a). The Alder Lake-N E-cores have no
  decoded-uop cache and decode these every pass; Ivy Bridge (uop cache) is unaffected. Instructions do not show it
  (prefixes are not instructions): only the time ratio moved.
* Confounder: c90ece4dc/c73c5d06f (BENCH-CASECMP-ALIGN) pinned cbench_locale's operands in the same interval
  (case_a page offset 0, case_b 0x800); that moved the instruction ratio 1.068 -> 1.122 (head_exact path, +9),
  not the time.
* No commit bisect of counts was run: the counts cannot see prefixes, and the timing below isolates the flag on one
  tree (only jcc_pad.rs differs between the variants).

## Timing (booked; same staged tree, only the prefix size differs; rounds p5 p0 p3 p0 p5 p3)
Liella/glibc time per row (cbench_locale, `--compare`-equivalent `bench --no-build --staged --counters`):
| row | N100 p5 | N100 p3 | N100 p0 | IVB p5 | IVB p0 |
|---|---|---|---|---|---|
| strcasecmp_c_256 | 1.549 | 0.830 | 0.835 | 0.937 | 0.923 |
| strcasecmp_c_256_odd | 1.368 | 0.839 | 0.836 | 0.835 | 0.830 |
| strcasecmp_c_256_xpage | 1.644 | 0.920 | 0.921 | 1.084 | 1.081 |
| strcasecmp_c_256_pageend | 1.933 | 1.232 | 1.230 | 1.818 | 1.819 |
| strcasecmp_c_16 | 1.072 | 1.072 | 1.072 | 1.034 | 1.019 |
| strncasecmp_c_64 | 1.149 | **1.200** | 1.200 | 1.002 | 0.981 |
Instructions/glibc on N100: strcasecmp_c_256 1.122 (p5) -> 1.165 (p3), strncasecmp_c_64 1.147 -> 1.162 (NOPs).
Jobs: nozomi:strcasecmp-n100-regress-time-nozomi, kotori:strcasecmp-n100-regress-time-kotori (logs
~/logs/<label>.log; the second p5/p3 rounds and the IVB p3 rounds were still running at hand-back).
Stage trees: keke ~/work/STRCASECMP-N100-REGRESS/stage_{p5,p3,p0}.tar (job strcasecmp-n100-regress-stage2; its
`none` variant was refused by the layout contract JCC-HOT-ASM, as it should be, so rc 1: evidence only).

## Change
crates/boundary/jcc_pad.rs: prefix size 5 -> 3 (fold.S, scan.S). jcc-check unchanged and still applies.

## Open / exact next step
1. strncasecmp_c_64 on N100 is slower (1.149 -> 1.200): NOPs now land on its head path (head + second block). Find
   the NOP sites in liella_machine_casecmp_ord_avx2's `.Lcord_head`/`.Lcord_head_next` with
   `objdump -d` of scan.S at prefix 3, and lay the block out so no padding is needed there (move the fused
   `cmpq $64,%rdx; ja` or the jnz so it sits inside a 32-byte window). jcc-check keeps it honest. Re-time N100.
2. Count cbench_memref and cbench_hot (instructions) p5 vs p3 before landing: NOPs replace 4-5-byte prefix padding
   in other kernels' loops (getenv scanner was +1000/call with NOPs in D-4.67's evidence). CI's instruction limits
   may trip.
3. Plants: none run. No test observes the prefix size; see prevention below.
4. Read the late rounds of both timing logs (second p5/p3 on N100; p3 on IVB).

## Prevention (proposal, separate story)
A static check in `jcc-check` (xtask/src/jcc.rs): reject any instruction in the padded sections carrying more than 3
redundant segment prefixes (count 0x2e/0x3e bytes before the opcode in objdump output), so a flag change or a new
kernel cannot reintroduce long prefix runs. Plus: a padding or layout story must time cbench_locale and every bench
whose kernels it touches on the E-core class, not only memref/hot.

RULING NEEDED: none. INFORMATIONAL: the strcasecmp_c_256_pageend row stays 1.23x (N100) / 1.82x (IVB): the
start-near-page-end byte fallback (BENCH-CASECMP-ALIGN open item 1), not this regression.
