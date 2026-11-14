# REGCOMP-SCOPE-COST: handoff (run 1, stopped for a ruling)

State: branch `task/regcomp-scope-cost`, cut from origin/next 239a714a5. HEAD: see
handoff/reports/REGCOMP-SCOPE-COST.json. Build role: mei:~/w/s1. No product code changed: the run measured, found that
the brief's target cannot be met under its own constraints, and stops for a ruling (RULING NEEDED below).

## What was measured (spec/behaviour/regex-nesting-depth.md, probe regex-nesting-depth.probe.c)
* Reference (glibc 2.39, ERE, 4 GiB / 40 s per depth): `((a)*)*` and `((a*)*)*` compile through 3000 (the deepest
  probed depth that answered), at quadratic cost: 1.5 s / 147 MiB at 1000, 9.9 s / 553 MiB at 2000, 31.6 s / 1.26 GiB
  at 3000. `((a)?)?` is cheap (1.2 s / 516 MiB at 3000). `((a)+)+` and `((a){1,2}){1,2}` grow about 4.6x per level:
  16 compiles (542 / 349 MiB), 20 is REG_ESPACE at the 4 GiB limit.
* Liella (origin/next): rep, rep2, plus compile through 914 (0.38 s, 143 MiB) and answer REG_ESPACE from 915 in 1 ms;
  opt compiles at every depth (5 ms at 3000); bnd compiles through 12, REG_ESPACE from 14.
* Gaps: rep/rep2 915..3000+ and bnd 14..16 (Liella refuses, the reference compiles). Liella compiles plus 20..914,
  which the reference refuses within 4 GiB.

## Why: the per-level cost has a floor that the 2^21 cap cannot hold at depth 2000
A matcher state (node) is (instruction, j), j = scopes entered since the last consumed byte, 0..=depth; so an
instruction inside k repetition scopes costs k + 1 nodes (program.rs `emit`, `derive`). Each `( ... )*` level emits
SPLIT + ENTER in the enclosing scope and OPEN + CLOSE + EXIT in its own (`rep_rec` / `rep_init`): level k costs 5k + 3,
the program 2.5 n^2 + 6.5 n + 2 nodes. n = 914 is 2,094,433 and 915 is 2,099,012 > 2^21, the measured boundary
exactly. Every j is reachable at every one of these instructions (a closure that climbs out to level m and descends
again reaches OPEN_k with j = k - m for every m < k), so a tighter j range per instruction saves nothing on this shape.
The floor: each level needs at least one instruction in the enclosing scope (enter) and one in its own (exit), so
even with ENTER+OPEN+loop SPLIT and CLOSE+EXIT fused into two instructions a level costs about 2k + 1 nodes and the
cap admits n <= 1447 (1447: 2,096,703 nodes). Depth 2000 needs about 4.0 M nodes fused (10 M today), 3000 needs
9.0 M (22.5 M). Liella's regcomp memory at the cap (143 MiB for 2.09 M nodes, about 70 B per node) is already the
reference's at the same depth (121 MiB at 914), so the reference's depths are a question of the cap, not of a
leak in Liella's bookkeeping: no duplicated subtrees or per-scope copies were found for these shapes (bounded
repeats copy by design, D-14.8).

## RULING NEEDED (the brief says "the 2^21 cap stays" and "never raise a bound"; meeting it needs one of these)
1. Fuse the per-level instructions (ENTER carries the group open, EXIT the group close, ENTER takes the loop split):
   5k + 3 -> about 2k + 1 nodes per level, cap reached at 1447 instead of 915, and 2.5x less regcomp memory and
   closure work for every grouped or nullable repetition. Touches the instruction set in program.rs (emit, derive,
   one_pass_check, first_bytes, to_bytes), exec.rs (19 opcode sites incl. the capture and back-reference matchers),
   dfa.rs and required.rs. Meets the reference's 1000, not 1500/2000/3000. **Recommended** whatever is chosen in 2.
2. To reach the reference's 2000-3000: replace the node cap by a memory bound sized for the reference's own cost
   (with 1: 9 M nodes at 3000, about 600 MiB at today's 70 B per node, half the reference's 1.26 GiB). That is a
   bound change, the owner's call. Alternative: keep 2^21 and accept the gap as a resource limit (a differences row:
   REG_ESPACE from 1448 after 1, where the reference spends seconds and hundreds of MiB).
3. bnd (bounded repeats of bounded repeats, exponential in both libraries): a constant factor buys at most one more
   level; recommend accepting 14 vs 16 as the cap working as intended (D-14.8: real blow-up is refused).
Recommendation: 1 now (a continuation, M/L), and 2 as "memory bound" only if the owner wants the reference's
2000-3000; otherwise a differences row for the remaining gap.

## Not to redo
The depth sweep on both sides (job logs on mei: regcomp-scope-cost-probe2, regcomp-scope-cost-lprobe). The per-level
model above (verified against the boundary). Reachability pruning of j for this shape (no gain, see above).

## Next step
After the ruling: implement 1 (and 2 if granted), then the brief's proof: depth sweep test (Liella accepts every
depth the reference accepts in the table), conformance + fuzz corpora, regcomp/regexec percall vs next, the 64 KiB
stack test at 100,000, plants (one restoring the 5-instruction level, caught by the sweep).
Plan rows: D-14.4, D-14.8 (plan/07-decisions/s14-searching-sorting-and-patterns.md); handoff/regex-deep-nesting.md.
