# REGCOMP-FALLIBLE-ALLOC(-2): handoff

State: branch `task/regcomp-fallible-alloc-2` (continues `task/regcomp-fallible-alloc`), cut from origin/next 8467f601d.
HEAD: see handoff/reports/REGCOMP-FALLIBLE-ALLOC-2.json. Build role: shiki:~/w/sh2. Plan rows: D-14.8 (amended by
the pending record records/pending/REGCOMP-FALLIBLE-ALLOC-2-1.toml, series D-14, id to be assigned), D-14.4, D-14.23;
rulings #5594 (no fixed cap, fully fallible allocation) and #5595 (bounded repeats).

## Done
* Run 1 (325019699 and follow-ups): `MAX_INSTS`/`MAX_NODES` removed from program.rs; only the u32 node-id guard
  (`NODES_REPRESENTABLE`) remains, with checked node-base sums. `compile` has no limit; harnesses (fuzz targets,
  regex_oracle_diff, regex_budget_time, the 24-stacked-`{2,}` line of s14_search_clauses) use
  `compile_limited(.., HARNESS_MAX_NODES)`. Allocation audit: every compiler and matcher allocation was already
  fallible (site list in the pending record's source run, previous version of this file); nothing converted.
* Run 2: the red limit-child case fixed at its cause. regexec with nmatch 0 on the depth-1446 program needs only the
  fast pass's per-node tables (tens of MiB). Under the host's malloc (plain run) that memory was already mapped
  (VmSize +68 KiB; VmSize 110 MiB after the compile against a 236 MiB peak), so it matched at every headroom. Under
  Liella's malloc (the gate's exports-interposed run) it answers REG_ESPACE, which is also correct. Whether it fits
  depends on the allocator, not on Liella. nmatch 2 does not fit and answers REG_ESPACE in both. The child now expects
  REG_ESPACE from nmatch 2, lifts the soft limit, expects a match from nmatch 0 on the same program (a failed search
  leaves it usable), then puts the limit back on for the small-pattern check. Gate regcomp-fallible-alloc-2-gate
  (head 2c4d9665b) had this one failure only (interposed run; 5320 passed); fixed in 27a577370.
* Percall on 2c4d9665b (product code unchanged since), base 8467f601d: regexec_sed_tail 361.0 -> 361.0,
  regexec_sed_groups 8181.2 -> 8181.2; regcomp_literal_nosub 17457.2 -> 17224.2, sed_groups 75628.3 -> 75281.9,
  email_alt 101824.4 -> 101144.6, kv_bre 70796.3 -> 70323.9, ipv4 121052.4 -> 119481.1 (instructions per call).
* Docs: pending decision record amending D-14.8; differences row (stacked `+`) and COMPAT-4's `liella` text say there
  is no fixed cap, the process's limits bound it, and the OOM killer may end an unlimited process as with the
  reference; spec/behaviour/regex-nesting-depth.md has the new sweep section (rep/rep2 to 3000 and bnd to 16 compile;
  REG_ESPACE under 512 MiB, no signal; reference numbers alongside).
* Plants file handoff/REGCOMP-FALLIBLE-ALLOC-2-plants.txt: node cap restored (regex_deep_nesting), derive's node
  table infallible and the search's heap pools infallible (regex_enomem's limit child).

## Jobs (shiki)
On 2c4d9665b: lint3 green, sweep green, plants 3/3 CAUGHT (plants-1), percall as above, gate 1 failure (fixed).
On the final head (this commit), started detached at the context checkpoint: regcomp-fallible-alloc-2-gate2,
-plants2, -percall-regcomp2, -percall-regexec2 (labels with the story prefix).

## Open / next
* If gate2 is red: read its failing suite; rerun only that suite after the fix.
* The pending record needs its D-14 id from the coordinator (CI holds the branch until then).

## Not to redo
The audit; the design (no cap in the product, `compile_limited` for harnesses); the regexec diagnosis above.
Process slips: one heredoc used for a one-line file edit (no effect); one diagnostic job stopped by its label.
