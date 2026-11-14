# REGCOMP-FALLIBLE-ALLOC: handoff (run 1, checkpoint at ~150k context)

State: branch `task/regcomp-fallible-alloc`, cut from origin/next 8467f601d (REGCOMP-SCOPE-COST-2/-3 landed: D-14.23).
HEAD: see handoff/reports/REGCOMP-FALLIBLE-ALLOC.json. Build role: mei:~/w/s4. Plan rows: D-14.8, D-14.22, D-14.23,
D-14.4; rulings #5594 (remove the cap, fully fallible allocation) and #5595 (bounded repeats).

## Done (commit 325019699, WIP)
* `program.rs`: `MAX_INSTS` (2^20 instructions) and `MAX_NODES` (2^21 states) removed. Kept only a representation
  guard, `NODES_REPRESENTABLE` = u32::MAX (node ids are u32 below `NONE`): `Builder::emit` stops with REG_ESPACE once
  the running node count passes `Builder::limit` (= the guard, or a caller's lower limit), and `derive` sums node
  bases with checked adds (`d.checked_add(1)`, was `d + 1`). `ProgRef::from_bytes` no longer filters `nodes <= 2^21`
  (the section sizes must fill the block exactly, which bounds it).
* `regex.rs`: `compile` = no limit (what regcomp uses); new `compile_limited(.., max_nodes)` and
  `HARNESS_MAX_NODES = 1 << 21` for in-process callers that bound their own work.
* Harness callers switched to `compile_limited(.., HARNESS_MAX_NODES)`: fuzz/fuzz_targets/regex.rs and regex_dfa.rs
  (libFuzzer's malloc limit reports one huge request as a failure), regex_oracle_diff.rs (the oracle keeps its own
  cap; both must refuse alike), core tests regex_budget_time.rs (replays fuzz seeds).
* Allocation audit (item 2): every allocation in the compiler and matcher is already fallible. Sites, all
  `TryVec`/`InlineVec`/scratch pools with `nomem(..)?`/`map_err(ESpace)`: parse.rs `Stack` spill (174, 176), arena
  `with_capacity` (876), hide_subpatterns `wrap`/`map` resize (924, 926); program.rs `Builder` insts/inst_chain/
  chains/scopes push, `entries` Stack, set parts reserve (1043), class table (1098), derive base/owner/indeg/queue/
  rank (1150-1200), seen (1258, 1326), preds (1422-1441), serialise reserve and pushes (1506-1563); exec.rs caps
  `extend_with` (556), pending_head/pending_caps (763, 767), buckets (857), best_caps (966), info (1440), prefix/pows
  reserve (1465), scratch extends (1522, 1537), per-search pools `take`/`take_filled` (scratch.rs `Pool`, `heap`);
  regex.rs `Groups::push`; required.rs (resize/reserve `.ok()?`: skips the analysis); boundary regex.rs block
  `TryVec` + `caller_alloc::dup_bytes`, regs `calloc`/`realloc` checked. No conversion was needed; no infallible
  growth found. required.rs keeps its own 2^18-instruction bound: it skips the optional required-bytes prefilter,
  never refuses a pattern.
* Tests: regex_deep_nesting `nested_repetitions_compile_without_a_size_cap` (rep/rep2 at 1446, 1447; search at 1000
  and 3000); the 100,000-level case still REG_ESPACE (u32 guard, found ~65,000 levels in). s14_search_clauses: the
  24 stacked `{2,}` line now uses `compile_limited` (uncapped it needs GiBs). regex_enomem: new
  `regcomp_and_regexec_answer_reg_espace_at_the_process_limit` (child, RLIMIT_AS = size + 8 MiB after compiling a
  depth-1446 program: depth 3000 rep/rep2, `a{2,}`x24 and 20 nested `{1,2}` must give REG_ESPACE; regexec nmatch 0
  and 2 on the big program REG_ESPACE; a small pattern still works). D-14.23 `tests` reference renamed.
* Check job regcomp-fallible-alloc-check1 (uncommitted tree = the commit minus the last test fix; evidence only):
  xtask clippy clean; core regex_deep_nesting (4 passed, 4.3 s incl. depth 3000), s14_search_clauses, regex_budget_time,
  regex_stacked green; the limit child passed every compile case and failed code 23: regexec nmatch 0 returned 0 on the
  nullable depth-1446 program (answered without the tables). Fixed in 325019699 (trailing `b`), NOT re-run.

## Later in run 1 (heads 56d7e2ac0, fb1474b9f)
* D-14.23's `tests` list had lost its ", " separator (static records check red in lint); fixed 56d7e2ac0.
* Job regcomp-fallible-alloc-lint3 on fb1474b9f: workspace clippy -D warnings, xtask clippy and xtask static all
  PASSED; then regex_enomem: the old test passes, the new limit child fails code 23 only: every compile case
  (depth 3000 rep/rep2, `a{2,}`x24, 20 nested `{1,2}`) gives REG_ESPACE under the limit, but regexec nmatch 0 of the
  depth-1446 program on "xaby" returns 0 (a correct match) within the limit. (lint2: on "xay" it was REG_NOMATCH from
  the required-bytes prefilter, hence the `b` in the subject now.)
* Likely cause (not yet verified): the limit is set as current size + 8 MiB right after the depth-1446 compile, whose
  ~165 MB of freed temporaries the test binary's allocator keeps mapped, so the search tables fit in retained memory.

## Next step (continuation)
1. Fix the limit child's regexec case at its cause: measure what the search allocates (e.g. print `size_bytes()`
   around the call without the limit), then make the limit bite: trim or re-measure after the compile's memory is
   returned, or compile the big program in an earlier child state so its temporaries are gone, or set the limit from
   the program's own size plus less than its search tables. Do not drop the case: regexec's REG_ESPACE is part of the
   brief's proof. Then rerun `cargo test -p liella-boundary --test regex_enomem`.
2. Sweep (scripts ready on mei: ~/work/REGCOMP-FALLIBLE-ALLOC/sweep.sh + depth.c; copies in the scratch dir): jobrun
   label regcomp-fallible-alloc-sweep; record rep 3000 time/maxrss (no limit) and the REG_ESPACE/exit0 rows under
   512 MiB into spec/behaviour/regex-nesting-depth.md.
3. Docs: pending record `cargo xtask records new decision D-NEW --series D-14 --story REGCOMP-FALLIBLE-ALLOC` with
   `amends = "D-14.8"` (no fixed cap; the bound is the process's limits; with no limit and default overcommit the
   kernel's OOM killer may end the process, as with the reference; harness limit; u32 guard). Update
   docs/differences-from-glibc.md row (line ~190: "still end at the 2^21-node cap" -> no fixed cap) and the COMPAT-4
   record's `liella` text (records/compat/COMPAT-4.toml; not the generated docs/compat-flags.md).
4. Plants (`cargo xtask plants`, LIELLA_LAYOUT_TRACE=ungrouped): (a) node cap restored in `Builder::new`
   (`limit.min(NODES_REPRESENTABLE)` -> `limit.min(1 << 21)`), msg `nested_repetitions_compile_without_a_size_cap.*(panicked|FAILED)`;
   (b) derive's `nomem(owner.resize(nodes, (0, 0)))?` -> `owner.resize(nodes, (0, 0)).expect("plant")`, caught by the
   limit child (abort), msg `the limit child failed`.
5. Gate (affected --run --base origin/next), percall regcomp_*/regexec_* vs base 8467f601d (emit lost the MAX_INSTS
   compare: expect equal or better).

## Not to redo
The audit above; the design (no cap in the product, `compile_limited` for harnesses). Process slip this run: one empty
`python3 - <<EOF` heredoc (no effect).
