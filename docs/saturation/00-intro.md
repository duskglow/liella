# Saturation register: introduction, sources and method of reading

**Numbering:** new sections are appended with the next free number at merge time; branches should not pick a number by hand (`tools/appendmerge.py` keeps both blocks). Sections 9 to 21 were renumbered (DOCS-REGISTER-RENUMBER) after parallel branches had all taken 9, 12 or 13. A merge-tool bug then left up to eleven copies of the PERF-NEXT-BATCH-2 section and repeated 13.x entries; DOCS-REGISTER-DEDUP (#2019) merged each set into one copy (section 14, with every story's row update and result paragraph) and renumbered sections 14 and up in file order (the old section 17 is now 15).

SATURATION-REGISTER, 2026-10-23. Rebuilt from the repository for the owner's pre-phase-14 review (standing gate of 2026-10-09: every call that a function or bench row is at or near its largest achievable gain, with the reason, then a brainstorm of improvements). The coordinator's running register was lost with the scratchpad on 2026-10-19.

Nothing was measured for this page. Every figure is copied from a file in the tree, named in the row.

## Sources and how far they can be trusted

| Source | What it gives | Date of the data | Caveat |
|---|---|---|---|
| `bench/instr_limits.tsv` | instructions per call, Liella vs glibc, AMD EPYC 7551 (Zen 1), memory and string rows | LIMITS-TIGHTEN, LIMITS-TIGHTEN-2: 2026-10-11 to 2026-10-20 | Newest per-call counts in the tree. Counts only, not cycles. |
| `bench/parity-measure.tsv` | static-pair time ratio `liella_static_ratio` (Liella / glibc, so below 1.00 is faster), and instruction ratio `instr_ratio_ss`, 150 rows | 2026-10-07 | Oldest data used here. Every row touched since (printf, heap, gettext, strstr, fts, qsort, locale) is **recheck** for its ratio. |
| `docs/benchmarks.md` | Haswell-EP / Broadwell-EP ratios and causes, commit `5cc49dc` | 2026-10-08 | Says itself that `main` has moved on. |
| `bench/perf-ledger.tsv`, `docs/perf-ledger.md` | 312 improvement rows and 40-odd `rejected` rows with the measured reason | 2026-10-04 to 2026-10-20 | The `liella-historic:` notes it cites are not in this tree. Reasons are quoted from the ledger's one-line gain column. |
| `handoff/*.md` (97 notes, the 2026-10-19 to 2026-10-22 stories) | recent profiles with the limit stated by the story | 2026-10-19 to 2026-10-22 | These are the best-supported claims. |
| `benchmarks/progress.md` | the optimization stage's stop rule and before/after tables | 2026-10-04/05 | The stop rule (P10 (10)): "diminishing returns are not pursued". |

**What the tree does not contain.** There is no row anywhere marked "saturated". Few notes say "at the limit" outright. What the tree has is three kinds of evidence, and the register separates them in the **Basis** column:

* **M** = a measured gap or parity in a recent note or counter table, plus a stated reason the remainder cannot be removed (kernel work shared with glibc, a floor of one pass, and so on). These are the strongest claims.
* **R** = the idea was tried and measured negative, then reverted (ledger `rejected` rows). The row is saturated only for that technique.
* **C** = a claim in a note that no measurement in the tree backs up, or a limit that is a regression bound, not a gain bound.

Ratio convention in the **Now** column: Liella / glibc, so below 1.00 is faster. "i" is instructions per call (Zen 1, `bench/instr_limits.tsv`, 2026-10-20 unless said), "t" is time (Haswell-EP, `bench/parity-measure.tsv`, 2026-10-07).

**recheck** means one of: the claim is older than a later change in the same code; no measurement in the tree supports it; or the only ratio in the tree is from the 2026-10-07 table.

---
