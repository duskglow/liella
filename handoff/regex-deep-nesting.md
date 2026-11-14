# REGCOMP-DEEP-NESTING(-2): handoff

State: branch `task/regcomp-deep-nesting-2` (continues `task/regcomp-deep-nesting`), cut from origin/next d53c0d551.
HEAD: see the report json (handoff/reports/REGCOMP-DEEP-NESTING-2.json). Build role used: keke:~/w/n1.

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

## Exact next step (continuation)
Profile regcomp_literal_nosub and regcomp_ipv4 (validation/wl_trace.sh, base d53c0d551 primed on keke) and remove
the surplus. Suspects, not measured: the inline `Stack` arrays initialised on every regcomp (levels 16, work 32,
hide 32, entries 16, frames 8 x the large `Frame` holding a whole `Rep`), and `Rep` copied through `Frame` on each
suspension of a bounded repetition (ipv4's `{3}`, `{1,3}`). Then rerun percall (validation/percall/regcomp.c) and
the direct gate.
