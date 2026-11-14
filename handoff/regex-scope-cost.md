# REGCOMP-SCOPE-COST: handoff (run 2, REGCOMP-SCOPE-COST-2: fusion done, back-reference rows slower)

State: branch `task/regcomp-scope-cost-2` (on top of run 1's `task/regcomp-scope-cost`, cut from origin/next
239a714a5). HEAD: see handoff/reports/REGCOMP-SCOPE-COST-2.json. Build role: mei:~/w/s2. **Do not land yet**: three
back-reference per-call rows are slower than the base (below).

## Done (ruling #5593)
* A level of a repetition scope is one `ENTER` (in the enclosing scope) and one `EXIT` (in its own). `ENTER.c` is an
  optional iteration's skip (`NONE` for a mandatory one): no `SPLIT` for `X*` or a bounded repeat's optional copies.
  `SCOPE_GROUP` (bit 30 of `a`): the operand is a plain group whose number is the scope's first group; `ENTER` opens
  it after the reset, `EXIT` closes it. `SCOPE_SHARED` (bit 31 of `a`, both instructions) replaces `ENTER.c == SHARED`.
  Not fused: `X+`'s loop split, hidden groups (wrap). Files: program.rs (both compiler forms, `eps`, `head`), exec.rs
  (thread-set insert, back-reference walk, fast search, one-pass walk + 2 x nodes log), dfa.rs, required.rs.
* Depth sweep (spec/behaviour/regex-nesting-depth.md, job regcomp-scope-cost-2-sweep): rep/rep2 compile through 1446,
  REG_ESPACE from 1447 (was 915); plus 1181 (was 914); bnd 14 (was 12; reference 16, ruling #5595 recorded there).
  Nodes per depth: rep n^2 + 3n + 2, rep2 n^2 + 4n + 3. Bytes: about 80 per node (rep 1446: 164944 KiB peak RSS;
  1000: 79580 KiB, 0.13 s, reference 150144 KiB, 1.49 s; 914: 66608 KiB, was 146056). For REGCOMP-FALLIBLE-ALLOC
  (ruling #5594): the reference's 2000 / 3000 need about 4.0 M / 9.0 M nodes, about 320 / 720 MB at this rate.
* Tests: regex_deep_nesting `nested_repetitions_compile_to_the_cap` (914, 1000, 1446 compile; 1447 REG_ESPACE);
  regex_paths `(a)*b` stays one-pass; dfa.rs test's over-cap stack 150 -> 200 stars (150 now fits the DFA's 1024).
* Plants: handoff/REGCOMP-SCOPE-COST-2-plants.txt, 11 of 11 CAUGHT (job regcomp-scope-cost-2-plants2, on 6175febcb).
* Gate: affected --run --base origin/next green on 6175febcb (regcomp-scope-cost-2-gate2).
* Pending decision record records/pending/REGCOMP-SCOPE-COST-2-1.toml (series D-14).

## Open: per-call rows (wl_percall, mei, base 32c24df04 = next 239a714a5's code; instructions per call)
| row | glibc | base | 6175febcb | 3c5954c81 (head) |
|---|---|---|---|---|
| rx_backref_long_c | 43010.9 | 73617.1 | 73987.1 | 73917.1 |
| rx_backref_c | 21731.9 | 21971.2 | 22046.2 | 22033.2 |
| rx_backref | 27977.9 | 21999.2 | 22074.2 | 22061.2 |
| regexec_sed_groups | 30901.0 | 8187.2 | 8183.2 | 8183.2 |
| regcomp_ipv4 | 292830.0 | 131161.4 | 121052.4 | 121052.4 |
Every regcomp row and the other regexec rows are equal or better (regcomp_ipv4 -10109, regcomp_email_alt -157).
Head's change (copy the captures only for an inlined iteration, as the replaced split did) recovered only 13 / 70.

## Next step
Profile the three rx_backref rows before changing anything (validation/wl_trace.sh, base 32c24df04 primed on mei):
which phase holds the +62 / +300. Candidates, unverified: the `EXIT` group check in the walk's `OP_JMP | OP_EXIT` arm
(one compare per hop), `enter_caps` not inlined, a different inlining (`mark_inline` hop counts change when a SPLIT
and an OPEN/CLOSE node disappear), or the walk's skip-first order bucketing more. Fix, rerun percall (both specs),
then gate (affected --run --base origin/next) and plants on the final head.

## Not to redo
The fusion design and its plants; the depth sweep; base priming on mei (32c24df04 is primed).
Plan rows: D-14.4, D-14.8; handoff/regex-deep-nesting.md.
