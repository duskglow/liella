# REGCOMP-SCOPE-COST: handoff (run 3, REGCOMP-SCOPE-COST-3: back-reference rows fixed, ready to land)

State: branch `task/regcomp-scope-cost-3` (stacked on run 2's `task/regcomp-scope-cost-2`, on run 1's
`task/regcomp-scope-cost`, cut from origin/next 239a714a5). HEAD: see handoff/reports/REGCOMP-SCOPE-COST-3.json.
Build role: kanon:~/w/kn1 (Broadwell-EP). Plan rows: D-14.4, D-14.8; handoff/regex-deep-nesting.md.

## Done
* Run 1: probe rows (spec/behaviour/regex-nesting-depth.md). Run 2 (ruling #5593): a repetition scope's level is one
  `ENTER` (optional iteration's skip in `ENTER.c`, fused group open, `SCOPE_GROUP` bit 30) and one `EXIT` (fused
  close); rep/rep2 compile through 1446 (was 914), plus 1181, bnd 14. Pending record
  records/pending/REGCOMP-SCOPE-COST-2-1.toml (series D-14).
* Run 3: the +62/+62/+300 instructions per call on the rx_backref rows, profiled and removed.
  Profile (wl_trace.sh, one rx_backref_c regexec, call 11): the pattern `\(b[a-z]*\)@\1` has NO repetition scope
  (`[a-z]*` is not nullable), so its program was unchanged; all +62 sat in `exec::walk` (1771 vs base 1709, every other
  function equal to the instruction). Cause: code generation. The fused optional `ENTER` arm (a second 32-element
  capture copy plus a recursive `reach`) grew walk's frame (0x158 -> 0x178) and its entry set-up, for every walk.
  Fix 1 (98707d04d): that arm moved to `walk_optional` (inline(never)); the fused `EXIT` close got its own guarded
  arm so a plain `JMP` pays no test: +62 -> +27. The rest was register pressure, and the trace showed the real cost
  centre: about 50 of walk's ~125 instructions per entry were the split arm's capture-copy set-up (bounds tests,
  copy_caps' unrolled addresses) hoisted to walk's entry, paid even though this pattern never copies.
  Fix 2 (e4ffabb81): `reach_copy` (inline(never)) holds the copy, carry save and reach for the split and the fused
  optional `ENTER`; walk keeps only the test. walk 1709 -> 1445 in the traced call.

## Per-call rows (wl_percall, Broadwell-EP, base 32c24df04 = next 239a714a5's code; instructions per call)
| row | glibc | base | run 2 head | final |
|---|---|---|---|---|
| rx_backref_long_c | 43119.9 | 73617.2 | 73917.2 | 72247.2 |
| rx_backref_c | 21776.9 | 21971.2 | 22033.2 | 21707.2 |
| rx_backref | 28022.9 | 21999.2 | 22061.2 | 21735.2 |
| regexec_sed_groups | 30906.0 | 8187.2 | 8183.2 | 8183.2 |
| re_search_sed_groups | 31084.9 | 8313.2 | 8309.2 | 8309.2 |
| regexec_sed_tail | 1067.5 | 361.0 | 361.0 | 361.0 |
| regexec_short (locale.c) | 8410.0 | 1272.6 | - | 1272.6 |
| regcomp_ipv4 | 292909.1 | 131161.5 | 121052.4 | 121052.4 |
| regcomp_email_alt | 177860.5 | 101981.4 | - | 101824.4 |
| regcomp_kv_bre | 208815.4 | 70854.3 | - | 70796.3 |
| regcomp_sed_groups | 137516.6 | 75673.3 | - | 75628.3 |
| regcomp_literal_nosub | 9996.3 | 17475.2 | - | 17457.2 |
Every row equal or better than base (job regcomp-scope-cost-3-percall). Parity by instructions; no timing taken
(no rep-string, x87 or vector change). rx_backref_c is now below the reference by instructions.

## Final head verification (kanon)
* Gate: `cargo xtask affected --run` green, 8 suites, 5301 passed, 0 failed, 2 conformance passed, peak 4.4G
  (regcomp-scope-cost-3-gate). Lint: workspace clippy -D warnings, xtask clippy, xtask static (regcomp-scope-cost-3-lint2).
* Plants: handoff/REGCOMP-SCOPE-COST-3-plants.txt = run 2's eleven plus walk-optional-guard and walk-exit-guard
  (regcomp-scope-cost-3-plants): still running at hand-back; the first four read (walk-optional-guard,
  walk-exit-guard, group-fusion-off, eps-skip) all CAUGHT with their message. The dispatcher reads its rc.
* Depth sweep on the final head (regcomp-scope-cost-3-sweep, run 2's probe and limits): unchanged from run 2. rep and
  rep2 compile through 1446, REG_ESPACE from 1447 (and at 3000); plus through 1181, REG_ESPACE from 1182; opt 3000;
  bnd through 14, REG_ESPACE from 15. rep 1446: 0.48 s, 165368 KiB; 1000: 0.20 s, 79968 KiB (Broadwell-EP).

## Open
Nothing for this story. Next: REGCOMP-FALLIBLE-ALLOC removes the 2^21 cap (rulings #5594/#5595; not touched here).
rx_backref_long_c stays 1.68x the reference: its cost is the thread-set search itself (run::{closure#2}, insert),
a separate story if wanted.

## Not to redo
The fusion design and its plants; the profile above (walk's entry set-up was the surplus); base priming on kanon
(32c24df04 is primed; a role-style copy of it for wl_trace is ~/work/REGCOMP-SCOPE-COST-3/base-role on kanon).
