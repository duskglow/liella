# getline / getdelim on long records (PERF-GETLINE-LONG -1..-3, TIMING-COV-GETLINE-LONG -1..-3)

State: branch task/timing-cov-getline-long-2 (stacked on task/timing-cov-getline-long, code d46915ba). Role shiki:~/w/sh1.
Code head: d46915ba (unchanged in -2). Test head: 98769b38. Later commits are handoff only. COMPLETE: test-only gain on top of
the small code gain; target (parity) not reached.

## What is on the branch
- PERF-GETLINE-LONG (D-12.19 front path, "GETLINE-LONG" in `getdelim_front`): on a miss where the whole unread window fits
  the caller's buffer, the window is copied and consumed (`Partial(n)`), and the general getdelim goes on after `prefix`.
- TIMING-COV-GETLINE-LONG (d46915ba): on a fully buffered stream the general `getdelim` no longer loads the stream state
  before `run()`, which loads it again. It reads the error indicator from `_flags`.
- TIMING-COV-GETLINE-LONG-2 (98769b38, tests only, crates/core/tests/s12_stdio_clauses.rs
  `s12_5_c_5_records_around_the_buffer_size`):
  - Every case runs in three locking modes: the stream's own locking, flockfile held by the caller, and
    `__fsetlocking(BYCALLER)`.
  - A new section sets the caller's `*n` to w-1 .. w+2 around the unread window w. The records are exactly the window
    then end of file, and 200 bytes ending in the delimiter.
  - After a mid-record read error the backing recovers: the next getline returns -1 with no read, and `clearerr`
    resumes the record.

## Why the partial branch was never reached (found in -2)
`delim_front` passes `partial = true` only when `thread::alone() || sc::by_caller(file)`. `by_caller` reads USER_LOCK,
which is `__fsetlocking(FSETLOCKING_BYCALLER)`, not flockfile. The cargo test harness has other threads, and the old
tests used internal locking or flockfile, so they always took `delim_locked` with `partial = false`. A BYCALLER stream
reaches the branch: plant partial-reached is now CAUGHT.
Side note, not changed: in a multi-threaded process a flockfile-holding caller never gets the partial path, because
`delim_locked` releases the lock between the front and general paths. When the caller already owns the lock, nothing
can interleave there, so the partial path would be safe. That could be a small win for a threaded flockfile reader.
Not measured; no workload row uses it.

## Probe-settled (glibc black box)
- keke ~/work/TIMING-COV-GETLINE-LONG/sticky.c (run 1): EOF is sticky (no read until clearerr). A mid-record read error
  returns the bytes before it, NUL-terminated, with errno EIO, ferror 1, feof 0.
- shiki ~/work/TIMING-COV-GETLINE-LONG-2/errrecover.c (run 2): the cookie fails once at byte 150, then recovers.
  getline returns 150 (EIO). The next getline returns -1 with errno 0 and 0 reads. After clearerr, getline returns
  151 (the rest of the record), and the next returns 5.
- errno with the error indicator already set: ruling #3408 (follow glibc: -1, errno untouched) goes to the story
  STDIO-STICKY-ERROR-ERRNO. Not changed or asserted here.

## Jobs (shiki, head 98769b38)
- timing-cov-getline-long-2-gate (direct tier) rc 0. timing-cov-getline-long-2-lint (xtask clippy + static) rc 0.
- timing-cov-getline-long-2-plants1 rc 0: 13 plants, 13 CAUGHT, 0 WARN. Plants file: shiki
  ~/work/TIMING-COV-GETLINE-LONG-2/plants.txt. It is run 1's file plus prefix-not-counted fixed to `0usize` and the new
  room-guard-short (`room >= n.saturating_sub(1)`).
- timing-cov-getline-long-2-percall2 rc 0 (instructions per call; columns glibc / base a521f95c / new):

| row | path | glibc | base | new | new/glibc |
|---|---|---|---|---|---|
| gl_long | AVX2 | 2645.6 | 3108.3 | 3081.7 | 1.16 |
| gl_80 | AVX2 | 180.0 | 190.4 | 189.8 | 1.05 |
| gl_long_win | AVX2 | 1184.6 | 1932.8 | 1930.7 | 1.63 |
| gl_long | non-AVX2 | 4258.2 | 5183.0 | 5156.4 | 1.21 |
| gl_80 | non-AVX2 | 195.0 | 206.6 | 206.0 | 1.06 |
| gl_long_win | non-AVX2 | 2758.7 | 2788.8 | 2786.7 | 1.01 |

No row slower than base. The non-AVX2 gl_long gap (+898) is larger than the AVX2 one (+436). The largest AVX2 gap is
gl_long_win (+746, 1.63).
- Superseded and diagnostic jobs: timing-cov-getline-long-2-t1 (the test before the commit, rc 0) and
  timing-cov-getline-long-2-percall (rc 2: base not primed, then primed with wl_prime_base.sh).
  timing-cov-getline-long-2-trace and -trace2 (rc 2): wl_trace on gl_long getline (non-AVX2) said "getline was called
  N times or fewer; nothing traced" with skip 10 and with skip 3. GL_LOOP calls getline 20000 times, so the breakpoint
  at the `getline` symbol (0x61a580) is never hit. That is a wl_trace problem, not diagnosed.

## Next (in order)
1. Make wl_trace hit getline in the gl_long case. Check that the traced binary is the one the case runs, and whether
   the static binary reaches getline through another symbol. Then split one non-AVX2 gl_long call per phase.
2. Attack the phase that holds the surplus, non-AVX2 first. Ideas from run 1: predict a miss from the previous call's
   record length (skip the wasted first fused scan); fused find+copy for the general path's pieces. The AVX2
   gl_long_win row (1.63) is the other large gap.
3. Timing (Ivy Bridge, N100) was not run: this run changed no code, and a 0.9% instruction change is below what timing
   resolves.
- Report: handoff/reports/TIMING-COV-GETLINE-LONG-2.json.

# History of the PERF-GETLINE-LONG branch (landed on next before TIMING-COV-GETLINE-LONG-3 merged it)
## Run 2 (PERF-GETLINE-LONG-2): NOT READY, plants all survive
Added test s12_5_c_5_long_records_cross_the_window (crates/core/tests/s12_stdio_clauses.rs): lengths 1..12289 around the 4096 window, tail with/without newline, record after, flockfile held/not. It passes, but plants (script ~/work/perf-getline-long-2/plants2.sh on keke; copy of $S/plants.sh) ALL SURVIVE: eof-after-prefix, prefix-not-counted, window-not-consumed, copy-skipped, partial-count-short, done-overclaims. So the test does not reach the Partial path under cargo test (suspect: Glue.record/read_room/by_caller false in the harness, so delim_front returns General or Locked; or the window is not <= caller cap). Next step: find which branch getline takes in the test (temporary eprintln in a RAM copy), make the test reach `getdelim_front(.., partial=true)`; if a unit test of sc::getdelim_front with a mock Env is easier (crates/core/tests/stream_engine.rs has Mem), do that. Not covered at all: error mid-line, ERR_SEEN guard.
Jobs: lint rc0 and gate rc0 ran on the first test commit (before the flockfile edit); counts2 and gate2 were started on the final head (read jobs.tsv); counts (first) killed because I synced under it. Percall tool wl_percall no longer exists; counts use `cargo xtask bench --compare --counters cbench_hot`.

## Run 3 (PERF-GETLINE-LONG-3): test reaches the partial path, all 8 plants CAUGHT
Branch task/perf-getline-long-3, role keke:~/w/n2. Why run 2's plants survived (counter-confirmed with a RAM probe copy that counted
getdelim_front exits): `sc::by_caller` is __fsetlocking(FSETLOCKING_BYCALLER), not flockfile, and `thread::alone()` is false under the
threaded test harness, so every miss went through delim_locked (partial=false): probe "miss=145 partialflag=0 taken=0".
Test s12_5_c_5_long_records_cross_the_window rewritten: BYCALLER and internal locking; file stream (buffer = st_blksize) and cookie
streams (BUFSIZ 8192) read whole, 7 and 4097 bytes per read (Cookie.chunk); getline and getdelim(':'); warm caller buffer (3B) or not;
shifts 0/5/31/33/100; lengths 1, r-1, r, r+1, r+33 (r = bytes left in the window), B-1, B, B+1, 2B+3; with/without final delimiter
(r without tail = EOF right after the prefix). Probe on the new test: miss=2944 partialflag=1472 roomfits=2048 taken=1024.
Plants (script ~/work/perf-getline-long-3/plants3.sh, RAM copies /dev/shm/claude/perf-getline-long-3/plants-3 and -4, each test run
under timeout 600): eof-after-prefix, prefix-not-counted, window-not-consumed, copy-skipped, partial-count-short, room-guard,
partial-flag-ignored, done-overclaims: all CAUGHT (plants3 on 5f031f65; plants4 re-runs them on the final head 8d72878f).
Jobs: plants3 rc0 (superseded: head before the lint fix), gate rc1 (clippy: needless mut, byte_char_slices; fixed in 8d72878f),
gate2 and plants4 on 8d72878f (read jobs.tsv on keke).
Still not covered: error mid-line / ERR_SEEN guard (needs a reader failing with a window still open).
Next step: if gate2 and plants4 are green, take `cargo xtask bench --compare --counters cbench_hot` before/after for gl_long, gl_long_win,
gl_80 (gl_80 must not rise) and add the saturation-register row.
