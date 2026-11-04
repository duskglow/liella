## 9. Method notes and gaps

* Every `rejected` row of `bench/perf-ledger.tsv` is quoted by story; the ledger's one-line `gain_as_recorded` is the only evidence. The `liella-historic:` notes it cites are not in this tree and were not read.
* I read passages (by grep and line range, not whole files) of the 25 handoff notes that mention a floor, kernel share, ceiling or a recommendation to stop (`strstr-*`, `fts-*`, `realpath-*`, `scandir-*`, `qsort-*`, `memcmp-*`, `mem*-policy`, `alloc-*`, `posix-spawnp-row`, `execvp-path-2`, `gettext-memo-hit-cost`). Not read: `environ-*`, `math-m5-*`, `cold-rewrite2-*`, `hottext-*`, `static-relro*`, the `timer-*` and `notify-*` notes. They are correctness or startup stories; the grep for saturation wording found "ceiling" there only in the sense of a back-off limit.
* Math (`libm-ulps`, `docs/libm-accuracy.tsv`) has accuracy ceilings, not performance ones; they are not in this register.
* No benchmark was run. The newest instruction counts are from 2026-10-20 (Zen 1); the newest time ratios are from 2026-10-07 and 10-08. A fresh full bench of the current head would turn every **recheck** in sections 3, 5, 6 and 7 into a measured row.
