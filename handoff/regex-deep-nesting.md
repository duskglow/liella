# REGCOMP-DEEP-NESTING(-2, -3): handoff

State: branch `task/regcomp-deep-nesting-3` (continues `-2` and `task/regcomp-deep-nesting`), cut from origin/next
d53c0d551. HEAD: see handoff/reports/REGCOMP-DEEP-NESTING-3.json. Build role run 3: mei:~/w/s3 (run 2: keke:~/w/n1).
**Not ready to land: two regcomp percall rows are still slower than base (ruling #5504). See "RUN 3" at the end.**

## What the story does
regcomp no longer refuses deep nesting (parse.rs `MAX_DEPTH = 256` / REG_ESPACE removed; coordinator ruling
#5468: no differences row). Every pass that recursed on nesting depth keeps its work on a heap stack instead:
1. parser (`Parser::tree`, was alternation -> branch -> group recursion);
2. `hide_subpatterns` (survey/plan/apply walks -> pre-order with an explicit stack);
3. the tree is an arena (`Parsed::nodes`), so drop cannot recurse; `nullable` and group ranges computed per node
   on insertion (were recursive walks, quadratic in repetition nesting);
4. compiler (`Builder::compile/start/resume` + `Rep` state machine replace `node`/`rep` recursion; same emission order).
The matcher (exec.rs, dfa.rs, required.rs) works on the flat program: no tree recursion. `regex_oracle/` (test-only
copy) keeps its 256 limit; the oracle diff stays green.
Decision: pending record records/pending/REGCOMP-DEEP-NESTING-2-1.toml (series D-14; id to be assigned).

## Tests and proofs (run 2)
* crates/core/tests/regex_deep_nesting.rs: groups (ERE, BRE) and nested alternations at 255, 256, 1,000, 10,000,
  100,000 compile and match (offsets = reference's; subexpressions checked up to 1,000); a 64 KiB-stack thread
  compiles and matches depth 100,000 and gets REG_ESPACE (cap) for `((a)*)*` at 100,000.
* regex_pre14_triage.rs T30 test + spec/behaviour/regex-pre14-triage.md T30 updated (balanced deep compiles).
* validation/percall/regcomp.c: new regcomp+regfree percall rows of ordinary patterns.
* Plants: handoff/REGCOMP-DEEP-NESTING-2-plants.txt (compiler recursion restored; depth limit; no heap spill).
* Gate, plants and percall results: RESULTS section below.

## Probe-settled (keke, scratch probe nest_probe.c, reference vs Liella static, ERE, main thread)
* Nested repetitions `(`^n `a` `)*`^n: Liella compiles up to depth 914, REG_ESPACE from 915 (D-14.8 2^21-node
  cap, kept by ruling #5482). The reference compiles 256, 500, 1,000 and 2,000 (deeper not settled by the probe).
  Liella's cost: every instruction counts (scope depth + 1) nodes, so the nodes grow with the square of the depth:
  about 2 x 2^21 / 914^2 = 5 nodes per level per enclosing scope (derived from the cap depth, not counted).
* Groups and alternations at 10,000 (nmatch 10,001) and the reproducer (REG_NOSUB, 255..1000): same answers.
* Liella regexec with all subexpressions at 10,000 nested ALTERNATIONS through the C entry returns REG_ESPACE
  (reference: match); the core API (no thread scratch) matches but takes seconds. Groups x states capture cost
  of the matcher: a follow-up, not this story (see INFORMATIONAL).

## Not to redo
The iterative passes are done and green; do not reintroduce recursion. Timing probe: compile is linear (depth 100,000:
70-150 ms release); regexec with subexpressions is the slow part at depth >= 10,000.

## Next step
Follow-ups for the coordinator: (a) lower the per-scope node cost so nested repetitions reach the reference's depths
(ruling #5482's separate story); (b) matcher capture cost with many groups (REG_ESPACE / seconds at 10,000 groups).

## RESULTS (run 2, keke:~/w/n1)
* Clippy + static clean on the final head (job regcomp-deep-nesting-2-gate-2, which also ran s14_search_clauses,
  regex_deep_nesting, regex_pre14_triage: all pass).
* Direct-tier gate regcomp-deep-nesting-2-gate-1 (head c41767000): RED only on s14_search_clauses s14_9_f_0, which
  pinned the old limit (300 nested groups, `a` + 300 `*` -> REG_ESPACE). Probed: the reference compiles both (and 1,000
  stars); expectation changed to 0 (commit 23ca7ec0b). Peak memory 2.0G. Conformance 2/2.
* Plants regcomp-deep-nesting-2-plants-2 (code identical to the final head): 3 of 3 CAUGHT, no WARN. plants-1 died in
  the baseline `cargo xtask so` (600 s timeout, cold copy on a host with 20 of 24 cores busy); rerun with --timeout 1500.
* Percall regcomp-deep-nesting-2-percall-1 (instructions/call, base d53c0d551 -> new, glibc):
  regcomp_literal_nosub 17559 -> 18452 (+5.1%, glibc 9976); regcomp_ipv4 133271 -> 136715 (+2.6%, glibc 292803);
  regcomp_sed_groups 77906 -> 76903; regcomp_email_alt 106615 -> 105947; regcomp_kv_bre 74139 -> 72935;
  regexec_sed_groups/sed_tail/re_search_sed_groups/rx_backref*: identical.
  **Two regcomp rows slower: the brief's "no slower ordinary row" is NOT met yet.**

## RUN 3 (mei:~/w/s3; base d53c0d551 primed there with validation/wl_prime_base.sh)
Profile first (wl_trace.sh, call 11 of regcomp, instructions; base traced from a copy of ~/wl/src/<base> with a
`.synced` marker and `--rev <base>`, as wl_percall documents for role trees):
* literal_nosub run 2 head: 18342 vs base 17449. The surplus was NOT the inline stack setup (the 8 frames' fill is
  16 stores): it was the compile loop's per-part overhead. Each concatenation item went through `start` call, `Step`
  unpack, frame push/pop and `resume` (~120 instructions per item vs ~30 for the recursive `node`); `Built::build`
  (the inlined loop) +552, parser (`facts` per leaf, `collect` pushing kids one by one) ~+330, one memset call ~+70.
* ipv4 run 2 head: 142676 vs 139232; `Built::build` +3692, mostly `Rep` (72-byte `Frame`) copied per copy of `{1,3}`.
Changes (commits 0c075ac3e, daee3c64a, 95683aace): leaves without a hidden group (byte, set, assert, back-reference,
empty) are compiled in place: a concatenation's and an alternation's leaf items in a loop (`Builder::cat`, `alt`), and
the repetition state machine runs in a driver loop (`rep_drive`, `RStep`) that compiles leaf copies itself, so a
`Rep` frame is pushed only for a copy that has parts. Same emission order. Parser: leaves get their facts inline in
`add`; `collect` copies a branch's items as slices (`Stack::copy_from`); inline stacks shrunk (work 16, open groups
4, subpattern walk 16). Plants file: handoff/REGCOMP-DEEP-NESTING-3-plants.txt (run 2's three, moved to the current
code, plus four on the fast paths and one equivalent) — NOT RUN yet.

Percall (instructions/call, mei, base d53c0d551 / run 2 head / run 3 head, glibc):
| row | glibc | base | run 2 | run 3 |
|---|---|---|---|---|
| regcomp_literal_nosub | 9976 | 17559 | 18452 | 17787 (+228, +1.3%) |
| regcomp_ipv4 | 292830 | 132910 | 136353 | 134802 (+1893, +1.4%) |
| regcomp_sed_groups | 137476 | 77906 | 76903 | 76782 |
| regcomp_email_alt | 177816 | 106428 | 105761 | 104818 |
| regcomp_kv_bre | 208749 | 74139 | 72935 | 72113 |
regexec_sed_groups 8187, regexec_sed_tail 361, re_search_sed_groups 8313, rx_backref 21999, rx_backref_c 21971,
rx_backref_long_c 73617: identical to base. Clippy, static and regex_deep_nesting / regex_pre14_triage /
s14_search_clauses green on the run 3 code.

## Exact next step (continuation)
Two rows are still slower (+228, +1893). Recommended: the hybrid the coordinator named: compile recursively (the
pre-story `node`/`rep`, native stack) while the nesting depth is below a fixed threshold (e.g. 64 levels, a few KiB
of stack, safe on the 64 KiB-stack test), and hand the subtree to the explicit-stack compiler beyond it; shallow
patterns then cost what base costs. Alternative if a second copy of the compiler is unwanted: shrink `Frame::Rep`
(keep `Rep` states on their own stack, the frame a 4-byte tag; `n: Option<u32>` as a sentinel) and re-trace ipv4
(`validation/wl_trace.sh --list`, aggregate `Built::build` by offset). Then percall (regcomp.c and regex.c), the
direct gate, and the plants file through `cargo xtask plants`.
