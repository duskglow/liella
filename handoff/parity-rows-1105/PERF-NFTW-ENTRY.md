# PERF-NFTW-ENTRY: nftw and fts per-entry cost

Id: PERF-NFTW-ENTRY
Kind: code
Priority: 54
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 80k

## Rows (instr/op Liella vs glibc: run label parity-rows-1105-counts / -counts2 on keke, 3 rounds, origin/next d10a6adc; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1105-percall / -percall2, spec validation/percall/parity1105.c, instructions:u per call, deterministic)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| nftw_phys_1000 (bench) | 6281096.1 | 5687022.1 | 1.10 | 1.05 |
| fts_physical_1000 (bench) | 6880727.8 | 6352609.1 | 1.08 | 1.00 |
| nftw_phys_38 (percall, 38 files + a + text, FTW_PHYS, C locale) | 35333.3 | 9565.1 | 3.69 | - |

## Hypothesis (from the per-call profile; no perf annotate, sampling on the VM loses samples)

Per entry Liella nftw costs about 640 more instructions than glibc in the percall (25800 over about 40 entries) and about 594 more in the bench (594000 over 1000 files): the same per-entry excess in two shapes, so it is a fixed per-entry cost, not tree size. fts is 530 per entry over, nearly the same, so the shared piece underneath (opendir/readdir/lstat wrappers, per-entry path building, or the sorting/compare of entries) is the suspect rather than nftw alone. The percall ratio is large because glibc is cheap per entry on a flat directory; the bench ratio is small because both pay a large common cost per file. First work: wl_trace.sh one nftw callback and list which functions take the 640.
First step of the story: re-run validation/wl_percall.sh --spec validation/percall/parity1105.c (and, for the call tree, validation/wl_trace.sh) and confirm before editing.

## The change

Remove the excess per-entry cost shared by nftw and fts_read so both rows are at most 1.05x glibc in instructions, with traversal order and callback arguments unchanged. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
