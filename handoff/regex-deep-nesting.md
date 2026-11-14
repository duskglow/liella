# REGCOMP-DEEP-NESTING (-2, -3, -4): handoff

State: branch `task/regcomp-deep-nesting-4` (continues `-3`, `-2`, `task/regcomp-deep-nesting`), cut from origin/next
d53c0d551 (not rebased). HEAD: see handoff/reports/REGCOMP-DEEP-NESTING-4.json. Build role run 4: mei:~/w/s1.
**Run 4 result: every regcomp/regexec percall row equal or better than base; ready to land once its jobs are green.**

## What the story does
regcomp no longer refuses deep nesting (parse.rs `MAX_DEPTH = 256` / REG_ESPACE removed; ruling #5468, no differences
row). The tree is an arena (`Parsed::nodes`, children before parents; drop cannot recurse; `nullable` and group ranges
computed on insertion). Parser (`Parser::tree`) and `hide_subpatterns` use explicit stacks. **Compiler (run 4, ruling
#5529): hybrid.** `Builder::node`/`body`/`rep_rec` recurse on the native stack for the first `RECURSE_DEPTH = 64`
nested calls (next's recursive formulation on the arena); the subtree at that depth goes to `Builder::compile` (the
run 2/3 explicit frame stack + `Rep` state machine), which emits the same instructions in the same order.
Decision: records/pending/REGCOMP-DEEP-NESTING-2-1.toml (series D-14; id to be assigned), updated for the hybrid.

## Proofs (run 4)
* Stack bound: release products (`objdump` of libc.a): `node` frame 160 B (6 pushes + 0x68 + ret), `rep_rec` 192 B,
  `compile` 0x328 + 56. A level holds at most one `node` + one `rep_rec`: 64 x 352 B = 22.5 KiB + ~0.9 KiB, inside
  the 64 KiB-stack test thread (which compiles and matches depth 100,000: regex_deep_nesting green).
* Unit tests (program.rs `recursion_tests`, `#[cfg(test)]` hand-off counter, zero product cost):
  `compiler_recurses_below_its_bound_and_hands_off_at_it` (63 nested groups: 0 hand-offs; 64: exactly 1; 4: 0) and
  `recursion_and_explicit_stack_build_the_same_program` (22 patterns: cat, alt, plain/bounded/scoped/shared
  repetitions, hidden groups, back-references, BRE; identical insts, chains, scopes, entry).
* Percall (instructions/call, mei, base d53c0d551 / run 3 / run 4 head, glibc):
  | row | glibc | base | run 3 | run 4 |
  |---|---|---|---|---|
  | regcomp_literal_nosub | 9976 | 17559 | 17787 | 17475 (-84) |
  | regcomp_ipv4 | 292830 | 132910 | 134802 | 131161 (-1749) |
  | regcomp_sed_groups | 137476 | 77906 | 76782 | 75673 |
  | regcomp_email_alt | 177816 | 106428 | 104818 | 101981 |
  | regcomp_kv_bre | 208749 | 74139 | 72113 | 70854 |
  regexec_sed_groups 8187, regexec_sed_tail 361, re_search_sed_groups 8313, rx_backref 21999, rx_backref_c 21971,
  rx_backref_long_c 73617: identical to base.
* Profile (wl_trace, literal_nosub call 11): the hybrid alone left +104 (node +89 per-call overhead, parser ~+21);
  fixed by compiling leaf kids in place in the recursive cat/alt loops (`quick`, `leaf` inline(always)), reading only
  two facts per child in `facts`, and keeping `facts` out of line so `Parser::add` inlines into the leaf path (+74
  when it did not).

## Plants
handoff/REGCOMP-DEEP-NESTING-4-plants.txt: switch depth infinite / 0, depth uncounted, recursive repetition off by
one, run 3's plants moved onto the hybrid (fast-path plants now caught by the equivalence test), facts plants, one
equivalent (quick-off: same program, cost only). Run plants2 (head 59b9cb85f): 10 CAUGHT, 1 EQUIVALENT, 2 SURVIVED:
leaf-facts-nullable (run 3's plant, never run before) and alt-facts-nullable. Cause: nothing checked the nullable
facts the parser computes on insertion for patterns without hidden groups (hide_subpatterns does not recompute them
there), and those facts are `can_be_null`. Fixed with regex_deep_nesting `nullable_facts_follow_the_leaves`
(commit a787339d1). Final (code a787339d1, mei): gate3 3999 passed / 0 failed, conformance 2/2, clippy + static
clean, peak 1.7G; plants3 12 CAUGHT, 1 EQUIVALENT, 0 SURVIVED, no WARN; percall3 = the table above.
Superseded: check1-5 (development), gate/plants (stopped to fix a doc), gate2/plants2 (before the facts test),
percall/percall2 (older head; percall2 refused a dirty sync). Process slip: one local edit used a heredoc. Switch-0 is caught by the hand-off unit test: a percall/instruction
test cannot catch it deterministically in-tree (both paths build the same program; a cost bound would need a reference
the plant does not move, and absolute instruction bounds drift with unrelated code).

## Not to redo
Iterative parser / hide_subpatterns / explicit-stack compiler are done; the hybrid is in. Do not reintroduce unbounded
recursion. Probe-settled behaviour and the D-14.8 nested-repetition cap (915) are unchanged from run 2 (ruling #5505).

## Next step
Land. Follow-ups (coordinator): REGCOMP-SCOPE-COST (per-scope node cost, ruling #5505); matcher capture cost with
many groups (REG_ESPACE / seconds at 10,000 groups through the C entry).
