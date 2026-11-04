# Saturation register: calls at or near their largest achievable gain

The register is one file per section under `docs/saturation/`, so that performance stories do not edit the same
file. Put a new row in the section file it belongs to; a new section is a new file `docs/saturation/<nn>-<slug>.md`
starting with `## <nn>. <title>` and a line here. Do not renumber by hand: `cargo xtask register --check` (also part
of `cargo xtask static`) requires every section file to be listed here, section numbers to be unique and contiguous,
and every row id (first cell of a `Row` table) to be unique across files.

- [Introduction and sources](saturation/00-intro.md): sources, caveats and how to read the tables
- [1. Kernel-shared work (the remainder is the kernel's)](saturation/01-kernel-shared-work.md): section 1
- [2. Safety bound, spec or ABI](saturation/02-safety-bound-spec-or-abi.md): section 2
- [3. Memory and string kernels (Zen 1 instruction counts, `bench/instr_limits.tsv`)](saturation/03-memory-and-string-kernels.md): section 3
- [4. Allocator](saturation/04-allocator.md): section 4
- [5. Locale, ctype, time, gettext](saturation/05-locale-ctype-time-gettext.md): section 5
- [6. Printf, regex, wordexp, iconv](saturation/06-printf-regex-wordexp-iconv.md): section 6
- [7. Rows that are not near the limit (for the brainstorm)](saturation/07-rows-that-are-not-near-the.md): section 7
- [8. Claims with no measurement behind them (all rows marked C above)](saturation/08-claims-with-no-measurement-behind-them.md): section 8
- [9. Method notes and gaps](saturation/09-method-notes-and-gaps.md): section 9
- [10. Fresh measurement (BENCH-REFRESH-1023-2, 2026-10-23)](saturation/10-fresh-measurement.md): section 10
- [11. Where the section 10 numbers live (BENCH-REFRESH-1023-3, 2026-10-23)](saturation/11-where-the-section-10-numbers-live.md): section 11
- [12. The five run-1 benches, re-measured with cycles (BENCH-REFRESH-1023-4, 2026-10-23)](saturation/12-the-five-run-1-benches-re.md): section 12
- [13. Next: perf stories from the 10-23 numbers (PERF-NEXT-BATCH, 2026-10-23)](saturation/13-next-perf-stories-from-the-10.md): section 13
- [14. Next ranked perf stories after PERF-NEXT-BATCH (PERF-NEXT-BATCH-2, 2026-10-23)](saturation/14-next-ranked-perf-stories-after-perf.md): section 14
- [15. The seven benches section 14 skipped, re-measured and ranked (PERF-NEXT-BATCH-3, 2026-10-24)](saturation/15-the-seven-benches-section-14-skipped.md): section 15
- [16. f64 exp/log/pow: where the 3.4-5.4x over glibc goes (MATH-M6-F64-GAP-BREAKDOWN, 2026-10-24, counted)](saturation/16-f64-exp-log-pow-gap-breakdown.md): section 16
