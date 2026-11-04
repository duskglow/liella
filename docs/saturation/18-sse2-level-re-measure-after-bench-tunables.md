## 18. SSE2-level re-measure after BENCH-TUNABLES (PERF-SSE2-REMEASURE, 2026-10-23, Zen 1 host)

Why: before BENCH-TUNABLES, `xtask bench` dropped GLIBC_TUNABLES (the METHOD FINDING in 13.4), so "SSE2 forced" bench numbers taken through it on AVX2 hosts were AVX2 numbers on both sides (Liella reads the tunable too). All numbers below are instructions:u, SSE2 forced with `glibc.cpu.hwcaps=-AVX2` on BOTH sides, checked: the glibc column changes against the default run in every tool used. Jobs on Zen 1 host: `perf-sse2-remeasure-bench4` (`cargo xtask bench --compare --counters --runs 3 [--tunables glibc.cpu.hwcaps=-AVX2] cbench_opt cbench_hot`, paired, alternating builds, forced pass then default pass, rc 0), `perf-sse2-remeasure-counts2` (wl_percall, GLIBC_TUNABLES exported to every column, COUNT_FRESH=1; wl_count with WLC_ENV; rc 0). Tree measured: ca804fb8 (next of 10-23 plus BENCH-TUNABLES; it holds neither PERF-STRCMP-NONAVX2-3 nor PERF-MEMRCHR-NONAVX2, so their heads d4ed9370 / 912bfbba were the `base` column of wl_percall and wl_count). The strcmp / memrchr percall cases (spec `sse2_rows.c` in the story's scratch on Zen 1 host) follow the shapes of the bench rows, not their exact strings.

Tools: wl_count was already fixed by SSE2-GAP-4 (WLC_ENV reaches all three columns and checks itself: make-n glibc 936.6M forced vs 895.3M default, bash-loop 21046M vs 20201M: "changed (ok)"). wl_percall keeps the caller's environment (wl_percall_run.sh has no `env -i`), so an exported GLIBC_TUNABLES reaches every column (glibc gl_80 193.1 forced vs 178.1 default).

| Decision | Measure (SSE2 forced, instr/op or per call) | glibc | Liella before | Liella with the story | Verdict |
|---|---|---|---|---|---|
| PERF-STRCMP-NONAVX2-2/-3 | bench strcmp_late_256 | 120.1 | 198.1 (1.65) | - (not landed) | gap is REAL at SSE2 |
| | percall late_256 shape | 122.0 | 200.0 | 183.0 (1.50) | holds: the story cuts 17 instr (8.5%) at SSE2; default 111 = 111 unchanged |
| | bench strcmp_equal_16 | 31.0 | 34.0 (1.10) | - | gap small and real |
| | percall equal_16 / strncmp eq16 / diff8 shapes | 58.0 / 68.0 / 41.0 | 44.0 / 69.0 / 58.0 | 44.0 / 69.0 / 58.0 | the story leaves these unchanged; strncmp_diff8 shape still 1.41 |
| | wl_count make-n / bash-loop (new = before, base = story) | 936.6M / 21046M | 793.0M / 19507M | 793.0M / 19519M | holds: Liella 0.85 / 0.93 of glibc at SSE2; the story moves make-n 1.0000, bash-loop +0.06% (sd 0.11%), outputs SAME |
| PERF-MEMRCHR-NONAVX2 | percall memrchr_64_hit / memrchr_4k shapes | 31.0 / 1147.0 | 39.0 / 1203.0 | 34.0 / 986.0 | holds: both rows improve at SSE2 (4k now below glibc); default 30/462 -> 30/461 unchanged |
| PERF-GETLINE-SHORT-2/-3 (landed) | percall gl_80 / gl_long / gl_long_win | 193.1 / 4251.8 / 3645.2 | (landed) | 205.8 / 5680.1 / 3990.2 | gl_80 HOLDS (Skylake-SP host 204.8 vs 195.0 then; its percall forced glibc correctly). gl_long_win 3990.2 here vs 2777.7 on Skylake-SP host: host-dependent (GUESS: the copy policy differs on Zen 1), not a forcing error |
| PERF-STRCOLL-UTF8 (NEGATIVE) | bench strcoll_en_us_utf8_16 | 35311.7 | 45874.9 (1.30) | - | NEGATIVE STANDS: the Haswell-EP host numbers in 13.2 row 9 (45621.6 / 35251.4) match the true SSE2 numbers; the row is not in the string kernels, so the dropped tunable changed nothing |

### 18.1 The cbench_hot rows of bench4 (PERF-SSE2-REMEASURE-3, read 2026-11-04)

Same job and tree as above (Liella = ca804fb8, so "before" for the strcmp-3 and memrchr stories, which were measured as percall shapes above). instr/op Liella / glibc (ratio), SSE2 forced and default:

| Decision | Bench row (cbench_hot) | SSE2 forced | Default | Agrees with the percall shape |
|---|---|---|---|---|
| PERF-STRCMP-NONAVX2-2/-3 | strncmp_diff8 | 56.0 / 39.0 (1.44) | 36.0 / 34.0 (1.06) | yes: shape 58.0 / 41.0 (1.41), the bench loop is 2 instr lighter on both sides |
| | strncmp_eq16 | 67.0 / 66.0 (1.02) | 36.1 / 34.0 (1.06) | yes: shape 69.0 / 68.0 |
| PERF-MEMRCHR-NONAVX2 | memrchr_64_hit | 36.0 / 28.0 (1.29) | 27.0 / 27.0 (1.00) | yes: shape before 39.0 / 31.0, 3 instr of harness on both sides |
| | memrchr_4k | 1200.8 / 1144.6 (1.05) | 459.5 / 475.5 (0.97) | yes: shape before 1203.0 / 1147.0 |
| | memrchr_64 (miss) | 53.1 / 53.0 (1.00) | 33.1 / 39.0 (0.85) | no shape measured; at parity forced |
| PERF-GETLINE-SHORT-2/-3 (landed) | getline_80 | 235.2 / 229.0 (1.03) | 213.7 / 214.1 (1.00) | direction yes (shape gl_80 1.07); the bench row's buffer and line differ from the shape |
| | getline_long | 9758.4 / 8469.4 (1.15) | 7654.8 / 6916.2 (1.11) | direction yes (shape gl_long 1.34); a different line length |

Forcing check: the glibc column moves between the passes in every row (strncmp_diff8 39.0 vs 34.0, memrchr_4k 1144.6 vs 475.5, getline_80 229.0 vs 214.1), and the bench4 log prints `tunables: GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 (both sides)` for the forced pass. Every verdict in the table above stands: the bench rows show the same gaps as the percall shapes. Open at SSE2 (Liella above glibc): strncmp_diff8 (1.44), memrchr_64_hit (1.29, 36 -> about 31 with PERF-MEMRCHR-NONAVX2 by the shape's -5), getline_long (1.15).

### 18.2 Other "SSE2 forced" / "AVX2 off" results in the register

* Section 13.2 rows 1-3, 5 and 10 were measured on Ivy Bridge host A (no AVX2: a native SSE2 level, not affected); 13.3 PERF-STRLEN-NONAVX2 likewise. Row 7 PERF-WCTYPE-L ran at the default level (not affected).
* 13.4 PERF-STRCHR-NONAVX2 found the problem itself and was re-measured by PERF-STRCHR-NONAVX2-2 with `--tunables` (not affected).
* 13.5 PERF-STRNLEN-RAWMEMCHR, 13.8 PERF-STRNLEN-BODY-3/-4 and 13.10 PERF-STRNLEN-MID-3 forced SSE2 through wl_percall with GLIBC_TUNABLES exported to both libcs; 13.7 PERF-STRNLEN-BODY used an in-process probe; 13.12 PERF-MEMRCHR-NONAVX2 ran the built `cbench_hot` binaries directly with the tunable. None went through `xtask bench` without `--tunables` (not affected).
* The batch measurements of sections 14 and 15 name `xtask bench --tunables` (BENCH-TUNABLES; not affected), and so does the proof line of 15.1. `xtask bench` still ignores a caller's own GLIBC_TUNABLES (only `--tunables` reaches both sides), so a 15.1 row result that gives `glibc.cpu.hwcaps=-AVX2` without naming the option is affected only if it was exported instead. The text shows the level was really forced where it compares both levels: row 9 `memset_4k` (i 1.15 forced vs 1.12 default), row 2 `__memcmpeq_head_*` (37.4-39.4 forced vs 28.4 default). Row 6's `-noavx2` job (`perf-regexec-backref-noavx2`, Skylake-SP host) logs `tunables: GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 (both sides)` (checked 2026-11-04: forced, not affected). NOT CHECKED: the realloc row (job `perf-realloc-usable-base3`); its log is not on the Zen 1 or Skylake-SP host. Its verdict (the guess is refuted: the in-place case is already the fused path) is a code-path finding, not a level-dependent ratio.

No other register result is known to have used a forced level through `xtask bench` without `--tunables`.
