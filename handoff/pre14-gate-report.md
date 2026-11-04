# Gate report before phase 14 (V-6, PRE14-GATE-REPORT)

Written 2026-11-04 from the tree at `432f1ef7` (origin/next) on branch `task/pre14-gate-report`. Facts only,
each with its source path. Nothing was measured or re-run for this page. Numbers older than the release cut
(10-24) are marked **(pre-cut)**. Stage plan: `plan/06-phases/16-validation-and-optimization-before-phase.md`.
Phase-14 implementation (DL-1 onwards) waits for this report and for the owner's review of
`docs/saturation-register.md` (index) and `docs/saturation/`.

## 1. Stage status: V-1 to V-6

| Story | State | Source |
|---|---|---|
| V-1 PRE14-OWNER-ITEMS | **done** 2026-10-23: seccomp check, AIO fallback, caller-stack serialisation on timers, mq and AIO, each with a test and a plant | plan file above, state table |
| V-2 PRE14-REGEX-TRIAGE | **done** 2026-10-23: T28, T29 fixed; T30 error code fixed, depth bound kept | `validation/README.md` triage rows T28-T30; `spec/behaviour/regex-pre14-triage.md` |
| V-3 PRE14-FUZZ-CAMPAIGN | **results not in the tree**: `docs/fuzzing.md` ("Results") says "Pending", the table has no rows, "None recorded yet" | `docs/fuzzing.md` |
| V-4 PRE14-BENCH-RERUN | **no result in the tree**: `docs/benchmarks.md` is still the `5cc49dc` page (2026-10-08) with no before/after table; no branch or report for it found | `docs/benchmarks.md` lines 1-25 |
| V-5 PRE14-VALIDATION-RERUN | **done** 2026-10-23 ("Seventh runs") | `validation/README.md` line 119 on |
| SATURATION-REGISTER | **landed**, split per section (16 sections plus intro), 2026-10-23/24 | `docs/saturation-register.md` |
| V-6 (this page) | written; two of its inputs (V-3 results, V-4 table) are missing, see section 6 | |

## 2. Validation with triage (V-5, pre-cut 2026-10-23, merged head `ed6d4449`)

Ten packages, statically linked against `libc.a`, as is, no stand-in; `liella-cc` links Liella's `libm.a`
(`validation/README.md`, "Seventh runs"). Dynamic linking is not exercised (D-22.10).

| Package | Liella versus glibc |
|---|---|
| coreutils (761), gnulib tests (601), sed, findutils, grep gnulib, diffutils, tar, gzip, make `-noload` | equal to glibc, except coreutils `misc/stdbuf` and `misc/responsive` (2 fail, T17) |
| grep tests (128) | 91 pass, 2 xpass (T31), 1 fail `stack-overflow` (T30, kept depth bound); `ere` (T28) and `case-fold-titlecase` (T29) now pass |
| gawk | **does not link as is** (T34, new): `libc.a` exports `fmod`, so configure drops `-lm` |
| make | does not link: `dlopen`/`dlsym`/`dlerror`/`dlclose` (T27, phase 14) |
| bash `run-*` (86) | 81 identical; `run-builtins`, `run-glob-bracket` differ (T4); three fail identically under both (T21) |

No package chose a gnulib replacement under Liella that it did not choose under glibc.

Open triage rows (class, status), `validation/README.md` "Triage":

* T4, T17, T27, T33: environment, wait for phase 14 (`dlopen`, `LD_PRELOAD`).
* T30: bug-fixed in error code; bound deliberately kept, differences row kept (V-2).
* T34: bug; fix story `PRE14-LIBC-FMOD` is "proposed" only in the V-5 text; no branch for it found.
* T20 (`stime` in `libc.a`): recorded as phase-15 compat. T22 (stdio needs the heap to print): marked RULING NEEDED (assignment) in the table. T24 (`__nss_hostname_digits_dots` signature): spec gap, pending. T23: 20 libm names glibc keeps in `libc.a`, listed as gaps in the table.
* T16 (`errno` after failed `setlocale`): glibc-specific, no change (cosmetic).
* T14 rows (`ulimit -v`): analysed in the table; coreutils `split/suffix-length` now passes.

Staleness check: the T26 row was marked open in the earlier README and is now marked fixed (PHASE-14-PLAN found it fixed in code, `crates/boundary/src/regex.rs`).

## 3. Fuzzing (V-3)

* 20 harnesses exist, one per listed parser: `fuzz/fuzz_targets/` (plan file, state table).
* Campaign method recorded: 30 min per target, Zen 1, two jobs `pre14-fuzz-campaign-a` and `-b`, 10 targets each (`docs/fuzzing.md`). The two jobs name all 20 targets.
* **Result: corpus sizes, coverage, executions and crashes were not copied into the tree.** The page says the jobs were still running at hand-back. The results exist only in `<work dir>/run-<stamp>-<a|b>/results.tsv` on the Zen 1 host. Whether any crash occurred cannot be stated from the repository. Any crash that may be a weakness goes to SECURITY-PRIVATE only, not here.
* The earlier host campaigns (2026-10-09 to 10-11, 20 min per target) are mentioned in `docs/fuzzing.md`; their numbers are not in the tree.

## 4. Before / after table (benchmarks)

No fresh full bench of the merged head is in the tree (V-4 not landed). The newest full measurement is `5cc49dc`
(2026-10-08, Haswell-EP E5-2697 v3 and Broadwell-EP E5-2699 v4, 161 `ratio_host` rows each; `docs/benchmarks.md` §2) **(pre-cut)**:

| CPU | faster (< 0.95) | parity | slower (> 1.05) | median row ratio |
|---|---:|---:|---:|---:|
| Haswell-EP | 55 | 14 | 92 | 1.14 |
| Broadwell-EP | 57 | 18 | 86 | 1.10 |

Partial newer data, not a full bench, all pre-cut and on different hosts and commits, so not comparable with the table above
(`docs/saturation/10-fresh-measurement.md` and `15-...md`; the register states it measured nothing itself):

* Ivy Bridge, `next` of 2026-10-23 (section 10): moved from slower to faster than glibc: `mbrtowc` UTF-8 1.655 -> 0.28/0.30/0.91; `strftime` 1.78-2.26 -> 0.41-0.67; `snprintf %e` 3.28 -> 0.90; `strcmp` names 1.49 -> 0.78; regex captures/backrefs 3.1-5.8 -> 0.08-1.30; `wordexp` 2.9-4.2 -> 0.72-1.76; iconv mixed 2.1-2.5 -> 0.61-0.74.
* Same section, moved the wrong way: `iswalpha` 0.89 -> 1.26, `towupper` 0.89 -> 1.03, `wcwidth` 0.77 -> 0.84. Above glibc and new: `strncmp` diff-8 1.75, `memcmp_128` 1.61, `strnlen_64` 1.41, `fnmatch` class 1.26.
* Noisy (spread above 10%, reported as measured): `strstr` (46%), `gettext` found/steady (64%), regex rows (15%).
* Zen 1 (section 15, `89c9209c`): `memcmp_144/256/4k` SSE2-forced i 1.57/1.43/1.37; `malloc`+`free` i 0.83, cycles 1.21-1.24.
* Math: f64 `exp`/`log`/`pow` are 3.4-5.4x glibc, broken down in `docs/saturation/16-f64-exp-log-pow-gap-breakdown.md`.

## 5. "Could not improve" list

The register (`docs/saturation/`) holds this list. Its own statement (`00-intro.md`): the tree has **no row marked saturated**; claims rest on three kinds of evidence (M measured with a stated reason, R tried and measured negative, C claim with no measurement).

* §1 kernel-shared work, §2 safety bound / spec / ABI (e.g. `fread`/`fwrite` discard on read streams, spec `s12_8_c_1`; `memcmp` 129-2048 class dispatch at parity; `qsort` with a C comparator; `fts` per-entry path), §3 memory and string kernels (Zen 1 instruction counts), §4 allocator, §5 locale/ctype/time/gettext, §6 printf/regex/wordexp/iconv: each row carries claim, reason class, basis, source story and "tried, measured negative".
* §7 rows **not** near the limit, for the brainstorm: `wordexp`, regex captures and back-references, cold first-call rows (`first_mktime`, `second_setlocale`, ...), `strcasecmp` (since moved by STRCASE-AVX2).
* §8 claims with no measurement behind them (all rows marked C).
* The register's own caveat: no benchmark was run for it; instruction counts are 2026-10-20 (Zen 1) and time ratios 2026-10-07/08, so every **recheck** row in §§3, 5, 6, 7 waits for a fresh full bench (`09-method-notes-and-gaps.md`).
* Stage rule: "the stage gate is the report, not parity" (plan file, "Order and overlap"); benchmarks are not near glibc parity yet (`docs/benchmarks.md` §2).

## 6. Gaps in this report (what the tree could not supply)

1. V-3 results (section 3) and the V-4 before/after table (section 4) are not in the tree. The plan's gate says "benchmarks re-run on the merged result" and "after the last fix the benchmarks are re-run so the final table matches the shipped code"; neither is done as far as the tree shows. This report does not substitute numbers.
2. The `#939`, `#940`, `#941`, `#955` rulings: no text for them was found in `plan/`, `docs/` or `handoff/`; they are named in this story's brief only. Their question and recommendation must be taken from the issues themselves.
3. `handoff/reports/` has no JSON for V-1 to V-5; their results above come from `plan/06-phases/16-...md` and `validation/README.md`.

## 7. Owner items

### 7.1 Held phase-14 rulings (`plan/ph14/open-questions.md`; the plan's recommendation, no new recommendation here)

| ID | Question | Plan's recommendation | Blocks |
|---|---|---|---|
| R14-1 | How does `ld.so` relocate itself? | (a) Rust relocator in `boundary/src/bootstrap.rs` (category *slice*), with L14-1 link checks and a randomized-base test; also serves static-PIE `libc.a` | M14-3 |
| R14-2 | Separate `ld.so`, or `libc.so.6` as its own interpreter? | (a) separate small `ld.so` (checkable bootstrap invariant); costs a ~12-function private interface and a few pages per process | M14-3 |
| R14-3 | Install and search model | (a) compile-time prefix, sonames pinned to `<prefix>/lib`, cache at `<prefix>/etc/ld.so.cache`; `LD_LIBRARY_PATH` may override pinned names outside secure mode, never in secure mode | M14-4 |
| R14-12 | Export placement and private names | (a) move `__libc_stack_end` and rseq objects to `ld.so` per `versions.tsv`; GLIBC_PRIVATE names not exported, interface is `LIELLA_PRIVATE`; revisit (b) only if the M14-13 binary-compat sweep needs one | M14-1, M14-5b |
| R14-13 | Executable-stack objects (`PT_GNU_STACK` with X) | (a) refuse with dlerror (and at startup), a differences row if the reference accepts | M14-5a |
| R14-15 | Threat-model loader/environment text (`threat-model-draft.md`, DRAFT FOR OWNER) | approve the draft's B/C rules as the test checklist; probe placeholders filled by M14-0 before M14-4 | M14-4 |

The file also lists R14-4, R14-5, R14-8, R14-10, R14-11, R14-14, R14-16 with recommendations; they are not in the held set named by the brief.

### 7.2 Held issues

`#939`, `#940`, `#941`, `#955`: see gap 2 in section 6; recommendation not available from the tree.

### 7.3 Left for the owner by V-1 to V-5 and the register

* Review of the saturation register (standing gate of 2026-10-09; `docs/saturation/00-intro.md`): every claim, then the brainstorm in §7.
* T22 (stdio needs the heap to print): "RULING NEEDED: assignment (stdio/printf owner)" in `validation/README.md`, with a recommendation recorded there.
* T34 `libc.a` exports `fmod` (and other libm names): fix story `PRE14-LIBC-FMOD` proposed, not started (V-5 text).
* T30: the depth bound is kept with its differences row (V-2): owner already decided per the plan text; listed for completeness.
* Fuzz findings (if any) and the missing V-3/V-4 data (section 6, items 1).
* Dynamic validation of T4, T17, T27, T33 happens only after phase 14.
