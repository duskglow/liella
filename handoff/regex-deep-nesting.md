# REGCOMP-DEEP-NESTING: handoff (checkpoint, WIP)

State: branch `task/regcomp-deep-nesting`, cut from origin/next d53c0d551. HEAD = the WIP commit after this note.
**The code is NOT compiled or tested yet.** The run stopped at the ~150k context checkpoint (thread_common).

## What the story needs (brief)
Remove regcomp's nesting-depth refusal (parse.rs `MAX_DEPTH = 256`, REG_ESPACE) by making every pass that recursed
on nesting depth iterative, so depth is bounded by memory only. No differences row (coordinator ruling #5468).

## Passes that recursed on depth (all in crates/core/src/pattern/regex/; found by a self-recursion scan + reading)
1. parse.rs parser: `alternation` -> `branch` -> `group` -> `alternation` (mutual recursion).
2. parse.rs `hide_subpatterns`: `survey`, `plan`, `apply` (recursive tree walks).
3. The tree's own drop (`Node` held `TryBox<Node>` / `TryVec<Node>`: dropping a deep tree recursed).
4. program.rs `nullable`, `group_range::walk` (called per repetition, so also quadratic in rep nesting).
5. program.rs `Builder::node` <-> `Builder::rep` (the compiler).
The matcher (exec.rs, dfa.rs, required.rs) works on the flat instruction array: no tree recursion found there.
`regex_oracle/` is a frozen test-only copy: left alone (it still refuses depth > 256; check the oracle diff tests).

## What the WIP commit does (written, unverified)
* parse.rs: the tree is an arena `Parsed::nodes: TryVec<Item>` (every node after its children; `Node` children are u32
  ids; Cat/Alt children in `Parsed::kids`). Dropping it cannot recurse. `Item` carries `nullable` and `groups`
  (first/last group) computed when the node is added, plus `wrap` (hidden group; was a wrapper `Group` node).
* parse.rs: `Parser::tree()` replaces alternation/branch/group: open groups on a `Stack<Level, 16>`, branches and items
  of all open levels on one `work: Stack<u32, 32>`; `collect()` turns the top segment into Cat/Alt/one item/Empty.
  `too_deep`, `bracket_end`, `last_depth`, `MAX_DEPTH` removed. Error order kept (same token checks in same order).
* parse.rs `hide_subpatterns`: pre-order with an explicit stack; "before the last group" is `opened < ngroups`
  (groups met so far in pre-order); renumbering + facts recompute is one forward pass (children first).
* `Stack<T, N>`: N inline entries, then a TryVec (fallible: REG_ESPACE); keeps ordinary patterns allocation-free.
* program.rs: `Builder::compile/start/resume` + a `Rep` state machine (`Phase`) replace node/rep; the instruction
  emission order is meant to be identical to the recursive version (each recursive call is a suspension point;
  `share` travels in each `Call`). `can_be_null` = root item's `nullable`.

## Probe-settled (reference, keke, main thread, ERE): job keke:regcomp-deep-nesting-probe
Groups `((..(a)..))` on "xay": depths 255, 256, 1000, 10000 -> regcomp 0, regexec 0, pmatch[0..2] = (1,2).
Alternations `(a|(a|..(b)..))` on "xby": same, (1,2). `(x(x(..a..)))` on "zzxxxxxxxxa": compiles, no match.
Nested `((a)*)*..`: 255, 256, 1000 compile, match (0,0). Larger depths: not recorded here (see hand-back).
Probe source: scratch only (nest_probe.c), not in the tree.

## Next steps (exact)
1. Sync, build: fix compile errors (`cargo xtask clippy` on keke:~/w/n1). Check `program.rs` tests and any test
   that built `Node` directly.
2. Update crates/core/tests/regex_pre14_triage.rs `deep_nesting_stays_bounded_and_names_an_unbalanced_pattern`:
   balanced deep patterns now compile and match; the unbalanced cases (EPAREN, EBRACK) stay. Check
   spec/behaviour/regex-pre14-triage.md T30 text.
3. New test (e.g. crates/core/tests/regex_deep_nesting.rs): depths 255, 256, 1000, 10000, 100000 compile + match
   (groups, alternations); a 64 KiB-stack thread compiling+matching depth 100000 (and nested reps: must return
   Ok or REG_ESPACE from the 2^21-node cap without overflowing). Check exec with/without subexpressions at depth
   (NoScratch may refuse big searches: use the scratch API if needed).
4. Plants file: restore a recursive pass (e.g. make `compile` recurse) -> small-stack test; re-add a depth check in
   `Parser::tree` -> reproducer test.
5. Pending decision record (series D-14): `cargo xtask records new decision D-NEW --series D-14 --story REGCOMP-DEEP-NESTING`.
6. Gate (`affected --run --base origin/next`), regex corpora/fuzz, percall regcomp/regexec rows vs next.

## INFORMATIONAL
* Nested repetitions `((a)*)*...` grow the program by scope depth per instruction (MAX_NODES 2^21 cap, D-14.8):
  expect REG_ESPACE around depth 1000 where the reference compiles; that is the existing size cap, not the native
  stack. Not raised (rules: never raise a bound); a RULING may be wanted.
