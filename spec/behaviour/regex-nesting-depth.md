# Regex nesting depth: where regcomp stops (REGCOMP-SCOPE-COST, 2026-11-14)

Black-box probe of glibc 2.39 (Ubuntu GLIBC 2.39-0ubuntu8.9, x86_64, Skylake-SP host), ERE, C locale.
Probe: [regex-nesting-depth.probe.c](regex-nesting-depth.probe.c) (`depth_probe <shape> <n>...`: one forked child per
depth runs `regcomp(REG_EXTENDED)` with a 4 GiB address-space limit and a 40 s alarm; the parent prints the
return code, the seconds and the child's peak RSS in KiB). Liella: the same source linked statically against
the products of origin/next 239a714a5 (conformance/run.sh's link line).

**Clauses:** `s14_9_c_1_1`, `s14_9_c_1_2` (regcomp's REG_ESPACE); D-14.4 / D-14.8 (the size caps).

Shapes (`n` levels; `(`x n, middle, close x n):

| shape | pattern at n = 2 |
|---|---|
| rep | `((a)*)*` |
| rep2 | `((a*)*)*` |
| opt | `((a)?)?` |
| plus | `((a)+)+` |
| bnd | `((a){1,2}){1,2}` |

## Reference (rc 0 = compiled, 12 = REG_ESPACE; seconds; peak RSS KiB)

| n | rep | rep2 | opt |
|---|---|---|---|
| 914 | 0, 1.15 s, 124416 | 0, 1.15 s, 124416 | 0, 0.09 s, 52224 |
| 1000 | 0, 1.49 s, 150144 | 0, 1.45 s, 150144 | 0, 0.11 s, 62592 |
| 1500 | 0, 4.4 s, 347520 | 0, 4.4 s, 349440 | 0, 0.27 s, 135936 |
| 2000 | 0, 9.9 s, 566400 | 0, 10.0 s, 566784 | 0, 0.48 s, 241152 |
| 2500 | 0, 18.7 s, 834816 | 0, 18.6 s, 835200 | 0, 0.77 s, 364416 |
| 3000 | 0, 31.6 s, 1320192 | 0, 31.3 s, 1320576 | 0, 1.16 s, 528000 |

rep and rep2 grow with the square of the depth in time and memory (about 140 bytes x n^2). Depths above 3000 are
not settled by this probe.

| n | plus | bnd |
|---|---|---|
| 12 | 0, 0.03 s, 24960 | 0, 0.02 s, 17664 |
| 14 | 0, 0.15 s, 117504 | 0, 0.11 s, 78336 |
| 16 | 0, 0.74 s, 554496 | 0, 0.49 s, 357120 |
| 20, 24, 32 | 12 (at the 4 GiB limit) | 12 (at the 4 GiB limit) |

plus and bnd grow by about 4.6x per level from 10 on; within 4 GiB the reference compiles 16 and answers
REG_ESPACE from 20.

## Liella after the fusion (REGCOMP-SCOPE-COST-2, same probe and limits, Skylake-SP host)

A level is one `ENTER` (with the loop's split and the group's open) in the enclosing scope and one `EXIT` (with the
group's close) in its own: 2k + 1 nodes at depth k. The cap is still 2^21 nodes (D-14.8).

| shape | compiles through | REG_ESPACE from | at the last accepted depth | at 1000 |
|---|---|---|---|---|
| rep | 1446 | 1447 | 0.29 s, 164944 KiB | 0.13 s, 79580 KiB |
| rep2 | 1446 | 1447 | 0.29 s, 165076 KiB | 0.13 s, 79620 KiB |
| plus | 1181 | 1182 | 0.25 s, 154264 KiB | 0.17 s, 111024 KiB |
| opt | every depth probed (3000) | - | 0.003 s, 3072 KiB at 3000 | - |
| bnd | 14 | 15 | 0.15 s, 77568 KiB | - |

Nodes: rep n^2 + 3n + 2 (1446: 2,095,256; 1447: 2,098,152 > 2^21), rep2 n^2 + 4n + 3. Peak RSS is about 80 bytes
per node (164944 KiB / 2,095,256 nodes), so the reference's 2000 and 3000 would need about 4.0 M and 9.0 M nodes, 320
and 720 MB at that rate (the reference: 553 MiB and 1.26 GiB). Earlier depths: rep 250 5892 KiB, 500 20596 KiB, 914
66608 KiB (was 146056 KiB). `X+` keeps its loop split (its `ENTER` is also the mandatory copy's entry): 3k + 1 nodes.

Remaining gaps: rep and rep2 from 1447 to at least 3000, the cap's (ruling #5594: the cap is removed with fully
fallible allocation in REGCOMP-FALLIBLE-ALLOC). bnd at 15 and 16: bounded repeats of bounded repeats are exponential
in both libraries (about 2.1x per level here, 4.6x in the reference); refusing them past the cap is D-14.8 working as
intended (ruling #5595).

## Liella before the fusion (origin/next 239a714a5)

| shape | compiles through | REG_ESPACE from | at the last accepted depth |
|---|---|---|---|
| rep | 914 | 915 | 0.38 s, 146056 KiB |
| rep2 | 914 | 915 | 0.37 s, 146084 KiB |
| plus | 914 | 915 | 0.32 s, 146052 KiB |
| opt | every depth probed (3000) | - | 0.005 s, 3072 KiB at 3000 |
| bnd | 12 | 14 | 0.09 s, 31488 KiB |

REG_ESPACE comes in 1 ms (the cap is found before the program is built), at every depth up to 100,000.

Gaps (Liella refuses, the reference compiles): rep and rep2 from 915 to at least 3000; bnd at 14 and 16.
Liella compiles plus from 20 to 914, which the reference refuses within 4 GiB.

Why the cap falls at 915 (Liella's compiler, D-14.8's node count): each `( ... )*` level emits SPLIT and ENTER
in the enclosing scope and OPEN, CLOSE and EXIT inside its own, and an instruction costs one node per enclosing
scope plus one, so level k costs 5k + 3 nodes and the program 2.5 n^2 + 6.5 n + 2 (n = 914: 2,094,433;
n = 915: 2,099,012; the cap is 2^21 = 2,097,152).
