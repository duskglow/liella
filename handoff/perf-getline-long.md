# getline / getdelim on long records (PERF-GETLINE-LONG, -2, TIMING-COV-GETLINE-LONG)

State: branch task/timing-cov-getline-long (stacked on task/perf-getline-long-2 a521f95c, which origin/next already
contains). Role keke:~/w/r1. Code head: d46915ba (later commits on the branch are handoff only). NOT READY: plants3 rc 1 (see Plants).

## What is on the branch
- PERF-GETLINE-LONG (D-12.19 front path, "GETLINE-LONG" comment in `getdelim_front`): on a miss where the whole unread
  window fits the caller's buffer, the window is copied and consumed (`Partial(n)`), and the general getdelim goes on
  after those `prefix` bytes.
- TIMING-COV-GETLINE-LONG (d46915ba): the general `getdelim` no longer loads the stream state before `run()` (which loads
  it again) on a fully buffered stream; it reads the error indicator from `_flags`. Line-buffered / unbuffered streams
  still load first, for the flush-before-input test.
- Tests (crates/core/tests/s12_stdio_clauses.rs): `s12_5_c_5_records_around_the_buffer_size` (64-byte buffer through
  setvbuf on a fopencookie stream: records of 1, 63, 64, 65, 192 bytes after shifts 0/1/7/31/40, with and without the
  delimiter at EOF; read error in mid-record at 64/128/150/330/359; sticky EOF with a read count) and a getline step in
  `s12_1_g_1` (line-buffered output flushed before getline on a line-buffered input stream). The cookie test backing
  gained `fail_at` (reads at or past it return -1 with EIO) and `reads`.

## Probe-settled (keke, ~/work/TIMING-COV-GETLINE-LONG/sticky.c, glibc black box)
- After getline reached EOF, more data later: getline returns -1 without calling read; after clearerr it reads it.
- Read error mid-record (cookie read -1/EIO at byte 150 of a 300-byte line, 64-byte buffer): returns 150, record
  NUL-terminated, errno EIO, ferror 1, feof 0. The next getline returns -1 and leaves errno unchanged (0 in the probe).

## Measurements (keke, AVX2 path, wl_percall strstr_getline.c, job timing-cov-getline-long-percall)
gl_long glibc 2639.2 / base a521f95c 3108.3 / new 3081.7 (1.17). gl_80 178.1 / 190.4 / 189.8 (1.07). No row slower.

## Findings
- The brief's hypothesis (load/store paid per refill) does not hold in the code: `State::read_until_with` refills
  inside one `run()`, so load / store_read happen once per call. The extra per-call load was the one before `run()`,
  removed above for fully buffered streams. Reasoned from the code; the step-1 trace (wl_trace) was NOT run.
- The brief's evidence (5170.6 vs 4251.8) is the non-AVX2 path on next 730f416b; the numbers here are the AVX2 path
  (keke). The non-AVX2 path was not counted in this run.
- PERF-GETLINE-LONG-2's plants "all survived" under a hand-written script; this run's plants go through
  `cargo xtask plants` with a plant that panics in the partial branch (`partial-reached`) to prove it is reached.

## Plants (cargo xtask plants, keke, plants file ~/work/TIMING-COV-GETLINE-LONG/plants.txt, job plants3)
12 plants: CAUGHT err-store-skipped, last-piece-dropped, refill-pos-lost, flush-guard-full. SURVIVED partial-reached
(a panic in the `Partial(n)` branch), partial-pos-lost, partial-count-short, partial-copy-skipped, eof-after-prefix,
done-overclaims, err-flag-not-checked. prefix-not-counted is WARN-NO-MSG: the mutant `let mut len = 0;` does not compile
(ambiguous integer), so write `0usize`. err-flag-not-checked survives because the test's failing cookie keeps failing.
So the partial branch is NEVER reached under cargo test, by either s12_5_c_5 test: that is why run 2's plants survived.
Diagnostic (job timing-cov-getline-long-diag, ~/work/TIMING-COV-GETLINE-LONG/diag.txt, evidence only): the new test
reaches past read_room, past Glue.record, the front-path miss with a non-empty window, and `delim_locked`. The test
harness has other threads (`thread::alone()` false) and the new test does not hold the stream, so it takes the locked
path with `partial = false`. The old test's flockfile cases also never reach the branch, so one of the remaining
guards fails there: `sc::by_caller` (USER_LOCK in `_flags` after flockfile?), `room == n + 1`, or ERR_SEEN. Not
split further (budget).

## Open / next
- FIRST: add flockfile-held variants to `s12_5_c_5_records_around_the_buffer_size`, then one diag plants run that panics
  separately on `partial`, `by_caller`, `room == n + 1` to find the failing guard. Fix the test (or the guard, if
  flockfile does not set what `by_caller` reads, which would also mean the front path never helps a flockfile caller),
  until partial-reached is CAUGHT. Then rerun plants.txt (fix prefix-not-counted to `0usize`; give err-flag-not-checked
  a backing that recovers after one failure).
- Next step: count the non-AVX2 path (the row's evidence), then wl_trace one gl_long call to split the remaining
  ~440 instructions (front path's wasted first fused scan on a miss; find then copy per piece in the general path).
  Ideas from run 1: predict a miss from the previous call's record length; fused find+copy for general pieces.
- Timing (nico) not run: a 0.9% instruction change is below what a timing run resolves; left for the periodic sweep.
- RULING NEEDED: with the error indicator set, Liella's getdelim returns `Err(EIO)` (by the code, not tested); the reference leaves
  errno unchanged (probe). Not changed or asserted here; needs a ruling on whether to match.
- Report: handoff/reports/TIMING-COV-GETLINE-LONG.json. Plants file: keke ~/work/TIMING-COV-GETLINE-LONG/plants.txt.
