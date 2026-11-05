# STRSTR-SHORT-DENSE-PARITY handoff (after -3: measurement pinned; ss_hit_64 fixed at the pinned address; dense_4k open)

State: branch task/strstr-short-dense-parity-3 (from origin/next e4370040, which holds -2). Role shiki:~/w/sh2 (Haswell-EP,
AVX2; Liella's AVX2 strstr counts are host-independent). Commits: 93602d7c (pin haystack alignment: percall specs +
cbench find_rows), f1851a6f (second unaligned head block in strpair), then docs/report. Report json:
handoff/reports/STRSTR-SHORT-DENSE-PARITY-3.json. Decisions in force: owner ruling #2686 (adaptive filter), ruling #2813
(pin the measurement). No new D-row (measurement fix + local kernel change, behaviour unchanged).

## 1. Measurement pinned (ruling #2813)
Every strstr haystack in validation/percall/strstr_cycles.c (one `_Alignas(4096)` pool), strstr_getline.c (ss_text:
aligned_alloc) and bench/c/cbench_memref.c find_rows starts 16 bytes past a 4096-aligned buffer (16 = malloc's guaranteed
alignment, neither 32- nor 64-aligned). Evidence that counts no longer follow code size:
- job -pin-a (base a65a823f = pre-PARITY-2 library, new = e4370040 library, both pinned): every hit row -47, every miss
  row -36, exactly; the one exception, strstr_hit_4k 1223 -> 1227 (+4), therefore is NOT layout: it is a code cost of
  -2's out-of-line find_next pass (hit_4k is the only row whose first candidate fails). Ruling #2813's "layout" reading
  of that +4 does not hold once pinned.
- job -counts (base e4370040, new f1851a6f, whose strpair code size differs): all 4 KiB and 1 KiB rows identical (0).
- base column of -counts = new column of -pin-a on every row (deterministic; sd 0.00 everywhere).
Pinning moved Liella's ss_hit_64 from 141 (-2, malloc address on the Zen 1 host) to 176: at offset 16 the kernel walked
to a 64-byte boundary (block, loop64, then the block loop again for the hit).

## 2. Per-phase split at the pinned offset (wl_trace.sh --list, job -trace, head 93602d7c; kernel offsets)
| phase | ss_hit_64 (176) | ss_hit_1k (333) | strstr_dense_4k (947) |
|---|---|---|---|
| Rust before kernel / after (verify, return) | 46 / 31 | 46 / 31 | 50 / 31 |
| kernel entry (page check, level, broadcasts) | 16 | 16 | 16 |
| head block + mode setup (+2 padding nops) | 21 | 21 | 21 |
| block loop (to 64 B; resolve at the end) | 31 | 31 | 31 |
| loop64 + mode | 17 | 20 | 20 |
| loop128s (one stream) + stop logic | 0 | 129 | 24 |
| loop128 pair (23 per 128 B) | 0 | 25 (re-test of the stop group) | 738 (32 groups) |
| exit | 6 | 6 | 6 |
harness ~9 per call. Traces: shiki ~/work/strstr-short-dense-parity-3/tr_*.txt (listing in the ~/logs/wlt-* logs named there).

## 3. Change and result (instructions:u per call, wl_percall, pinned, job -counts)
f1851a6f: on AVX2, when p's page holds 64 bytes from p, a second unaligned block at p + 32 (partner at p + 32 - off) is
tested directly; the aligned run resumes at the 64-byte boundary in (p, p + 64]. Strings up to 64 bytes from p are then
settled in two block tests whatever p's alignment.
| row | Zen 1 glibc (old address) | Haswell-EP glibc (pinned) | before | after |
|---|---|---|---|---|
| ss_hit_64 | 163.2 | 102 | 176 | 138 (0.85x Zen 1) |
| ss_miss_64 | 339.8 | 176 | 181 | 143 |
| ss_hit_1k | 355.2 | 661 | 333 | 333 (0.94x) |
| strstr_dense_4k | 818 | 2253 | 947 | 947 (1.16x, open) |
| strstr_hit_short | 153 | 87 | 126 | 126 |
| ss_hit_4k / ss_pairmiss_4k / ss_miss_4k | | 2245 / 2254 / 6446 | 666 / 659 / 925 | unchanged |
| strstr_scan / verify / miss / hit _4k | | 2253 / 29145 / 4833 / 4861 | 672 / 926 / 919 / 1227 | unchanged |
No row slower. The Zen 1 glibc column was measured at the old (unpinned) addresses: NOT re-measured with the pinned spec
(this run's host is Haswell-EP). glibc takes another path on Haswell-EP (short rows 87-102, long rows 2-3x Zen 1's).

## Jobs (shiki)
Final head f1851a6f code (+ docs): -check (clippy + static + strstr_find, rc 0), -counts (rc 0), -plants2 (5 CAUGHT,
plants file ~/work/strstr-short-dense-parity-3/plants.txt), -gate (direct tier), -final (clippy + static on the docs head).
Superseded: -plants (resume-below WARN-NO-MSG: caught by SIGSEGV, regex lacked it; fixed in -plants2). Diagnostic:
-pin-a (pinned counts on 93602d7c vs a65a823f), -trace.

## Next step (in order)
1. Re-measure the Zen 1 glibc reference with the pinned specs (needs a Zen 1 role) so the ratios compare like with like.
2. strstr_dense_4k (947 vs 818; 1.05x = 859, -88 needed): AVX2 pair-test floor is 4 ops per 32 B plus (G + 3) per group
   of G blocks: G = 4 gives 23 per 128 B (738 here), G = 8 gives 21.5 (about -48 on 4 KiB; 256-aligned groups never
   cross a page). Pair with a direct exit from non-destructive group min vectors (vpcmpeqb + vpmovmskb per block instead
   of re-running the 13-instruction block loop; also drops the 23-instruction pair re-test after a one-stream stop), and
   the kernel entry (16). Fixed cost must reach <= ~171 with G = 8 (now ~209 incl. harness). No bound proved yet.
3. strstr_hit_4k: the +4 since -2 is find_next's out-of-line cost; trace it if it matters.
Not to redo: -1's DENSE hand-back and SSE2 prefix step; -2's Rust restructure.
Plan rows to read: docs/saturation/03 strstr rows (row "pinned haystack alignment, second head block").
