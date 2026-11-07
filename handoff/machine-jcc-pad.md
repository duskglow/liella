# MACHINE-JCC-PAD handoff (2026-11-07)

**State:** branch `task/machine-jcc-pad` (from origin/next 83b79b15e), commits 10d7e37ab (CBENCH_COUNTERS=mite),
8e0caad2c (padding + `cargo xtask jcc-check`), 8f2551b11 (D-4.67, REQUIRED guard, wl_percall `--event mite`, plants
file), then this handoff. **Verdict: INCOMPLETE.** The cycle counts on the loaded Skylake-SP host and the timing on
Ivy Bridge and N100 must be checked before this lands.

## What was done
* `crates/boundary/jcc_pad.rs` (shared by `build.rs` with `include!` and by xtask): fold.S and scan.S are assembled with
  `-Wa,-malign-branch-boundary=32 -Wa,-malign-branch=jcc+fused+jmp+call+ret+indirect -Wa,-malign-branch-prefix-size=5`
  (every jump kind in Intel's JCC-erratum white paper, not only the jcc+fused+jmp of `-mbranches-within-32B-boundaries`).
  Code size: fold.S 12505 -> 12583 bytes, scan.S 22063 -> 22703; 8 scan.S sections go from 16- to 32-byte
  alignment (the assembler raises them). The classed pages' `.org` slots still fit, and classed-xref passes on the
  products. Hot-text renaming keeps each section's alignment.
* `xtask/src/jcc.rs`, `cargo xtask jcc-check`: objdump -d/-h. It fails on a jump, or a macro-fusible pair (Intel's
  fusion table: test/and all Jcc; cmp/add/sub with carry/zero/signed forms; inc/dec without carry; no imm+mem, no RIP),
  that crosses or ends on a 32-byte boundary, and on a section holding a jump that is aligned below 32. It is a
  `static` check (assembles the files) and also runs on `libc.a` in `so` (the sections the files declare). Unpadded,
  it reports 33 jumps in fold.S and 161 in scan.S. It REQUIRES fold.S and scan.S in the list, because a file dropped
  from jcc_pad.rs would otherwise drop out of the check as well.
* Measuring aids: `CBENCH_COUNTERS=mite` (run a bench program directly: the uops column = IDQ.MITE_UOPS, raw 0x0479)
  and `validation/wl_percall.sh --event mite` (= r0479:u; Sandy Bridge to Skylake family only).

## Evidence (Skylake-SP host, load 28-35: cycles ADVISORY; table: handoff/machine-jcc-pad/skx_counts.txt)
Staged trees: after = this branch; before = the same tree with the padded-file list emptied (and so.rs's archive
check removed for that build only). `count.sh` runs every cbench_memref/cbench_hot program, Liella and glibc, with
CBENCH_COUNTERS=1 and =mite, pinned to one CPU.
* MITE uops per call, summed over 175 rows: **88596 -> 14510 (-84%)**; 104 rows fall by more than 0.5. Examples:
  `__memcmpeq_eq_16` 21.2 -> 3.7, `strlen_16` 14.1 -> 0.2, `memchr_16` 18.1 -> 0.1, `getenv_hit_last_2000`
  7270 -> 83.
* **Instructions go up where NOPs land in loops** (prefix padding adds none): getenv/putenv `_2000` rows about
  +1000 per call (+9%: about 0.5 NOP per environment entry in scan.S's environment scanner loop), `_300` rows about
  +150, memrchr_4k +2; 91 rows up by more than 0.05.
* Cycles (advisory): getenv rows fall (`getenv_hit_last_2000` 7129 -> 6359), but `memchr_16` 10.0 -> 14.8,
  `strlen_16` 8.5 -> 11.7 and `__memcmpeq_eq_16` 9.7 -> 13.4 RISE on this loaded host while MITE drops to ~0. Not
  understood. Load noise is possible, so do not trust these numbers either way. Rerun them on an idle Skylake-family core.

## Pending jobs (read before deciding)
* keke `machine-jcc-pad-gate` (clippy, static, affected --base origin/next) and `machine-jcc-pad-plants`
  (handoff/machine-jcc-pad.plants, 10 plants: flags, file list, build.rs, checker logic), queued at hand-back.
* kotori `machine-jcc-pad-ivb`, nozomi `machine-jcc-pad-n100`: `~/work/machine-jcc-pad/time.sh`: before/after
  `--compare` of cbench_memref and cbench_hot, rounds b a a b. Read the Liella/glibc ratio per row per round.

## Next step
1. Read the gate/plants (fix any red: only the failing suite) and the two timing logs. If any row is slower after
   padding beyond spread (likely candidates are the getenv `_2000` loops and the short memchr/strlen rows), find the
   NOP sites in that loop (`objdump -d` of scan.S's object: `nop`/`xchg %ax,%ax` inside the loop) and remove the need
   for them: align the loop head (`.p2align 5`) or reorder so the jump sits inside a window. jcc-check keeps the
   result honest. Then measure again.
2. Re-measure the 16-byte rows' cycles on an idle Skylake-family core before crediting or blaming the padding.

## Not done / open
* No booked Skylake-family timing (only counters on a loaded host). The other machine .S files are not padded (not
  measured as hot). Zen not measured: Zen 1 and Zen 2 do not have the erratum, so there the change only adds bytes.
* RULING NEEDED: none. INFORMATIONAL: `c_malloc` dead-code warning in a bench build (not this change).
