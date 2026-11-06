# TIMING-N100-MEMCMP handoff (after -2: COMPLETE, the branch should land)

State: branch `task/timing-n100-memcmp-2` (from task/timing-n100-memcmp, merged origin/next 19d032c5 at 8d417a77;
TIMING-COV-MEMCMP-NONAVX2 had not landed, fold.S on next was unchanged). Commits: ddc378df (129-256 interleave
reverted), 720ab326 (code: 256-byte step only to 8 KiB, old 128-byte two-branch step above; SSE2 phase pin; test
sizes 8192/8193/8449), 431761fa (saturation row, plants msgs), then this handoff. Role kanon:~/w/kn1. Plants file:
handoff/TIMING-N100-MEMCMP-2-plants.txt. Decisions: no new D-row (local to fold.S; reasons in its comments).

## Why memcmp_64k slowed with the 256-byte step (N100, perf stat -r 5, Liella only, per call)
| per call | next (128 x 2 branches) | B (256 x 1) | C (128 x 1) | E = final |
|---|---|---|---|---|
| cycles 64k | 4773 | 5058 | 4894 | 4784 |
| L2-hit demand loads 64k | 214 | 308 | 232 | 211 |
| ld_head l1_miss_at_ret 64k | 21.0 | 40.7 | 12.0 | 23.0 |
| branch-misses 4k | 1.00 | 0.00 | 1.00 | 0.00 |
| instructions 4k / 64k | 553 / 8233 | 499 / 7219 | 557 / 8237 | 501 / 8236 |
Same loads (8201), no split loads: the 256-byte stride per load site leaves more of the L2-resident stream to demand
misses (prefetch coverage). C (128-byte one-branch step) keeps the 4k mispredict and is slower (NEGATIVE).
D (256 to 8 KiB, C above) left 64k +0.7%: hence E uses the old loop above 8 KiB.

## Ivy Bridge 4k +2.3%
The AVX2 growth moved the SSE2 classes 257 bytes (.Lcmp_sse_mid phase 20 -> 21 mod 64). Pinned with `.p2align 6, 0xcc;
.fill 20, 1, 0xcc` before .Lcmp_sse_mid (never run, after a jmp): memcmp_4k 141.41 = next 141.41.

## Timing (staged trees from kanon, interleaved, 7 rounds, Liella static vs glibc host-dynamic, median ns)
N100 (nozomi:timing-n100-memcmp-2-n100-final): memcmp_4k 116.37 -> 109.00 (L/G 1.084 -> 1.015); 64k 1978.6 -> 1977.5;
256 7.53 = 7.53. Ivy Bridge (kotori:timing-n100-memcmp-2-ivb-final): 144 10.84 =; 256 15.65 =; 4k 141.41 =; 64k
4000.7 -> 3990.5. All four trees (next/B/Cp/Dp) are in nozomi:timing-n100-memcmp-2-n100 / kotori:...-ivb. Bookings 88/89.

## Jobs (final code 720ab326)
kanon: timing-n100-memcmp-2-gate (clippy -D warnings, static, direct-tier affected), timing-n100-memcmp-2-plants2
(9 plants). Superseded: timing-n100-memcmp-2-plants (rc 0; 2 WARN-NO-MSG, SIGSEGV deaths not in the msg regex: fixed).
Not run: the `bench --compare --counters` count job (budget); instruction counts above are N100 perf stat.

## Open / next
* memcmp_256 still 1.06 (N100) / 1.26 (Ivy Bridge): needs its own idea (the interleave did nothing).
* +3 instructions on memcmp_64k (size test + jmp); timing is at parity.
Do not redo: 128-byte one-branch step, 129-256 interleave (both NEGATIVE, docs/saturation/03).
