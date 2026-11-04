# PERF-GETLINE-LONG (run 3: plants all CAUGHT; gl_80 base and bench --compare counts still not taken)

Branch task/perf-getline-long. Role keke:~/w/n2. Per-call instructions:u (keke, wl_percall, strstr_getline.c):
gl_long 3592.0 (before, label PERF-GETLINE-LONG-percall-before) -> 3392.7 (known-miss skip only, percall-after, superseded)
-> 3108.3 (final, percall-after2); glibc 2639.2 (now 1.18). gl_long_win 1926.8 -> 1932.8 (glibc 2071). gl_80 190.4 vs glibc 178.1 (no base number taken: CHECK it did not rise).

Cause (counter-confirmed): a line longer than the unread window made the getdelim front path run its fused scan+copy over the
whole window, miss, and the general path scanned and copied the same window again.
Change: front path, on a miss where the whole window fits the caller's buffer, finishes the copy (the AVX2 kernel already stored
every whole 32-byte block: Env::find_copy_done), consumes the window and passes `prefix` bytes to the general getdelim, which
continues after them (EOF after a prefix returns the prefix as the record). Not done under the stream lock path, nor with the error indicator set.
Remaining gap to glibc (~470/call): the wasted fused scan of the first window on a miss; the second piece's find+copy.
Ideas: predict a miss from the previous call (record longer than window) and go straight to the general path; fused scan for the general pieces.

NOT DONE: plants (partial-consumption logic: prefix, EOF after prefix, err-flag guard, done offset), a regression test (long line, EOF without
newline after a full window, cap == window+1 boundary, multithread), bench --compare counts, gate2 result, saturation-register row.
Jobs: static2 rc0 and percall-after2 rc0 on this head's code; gate2 started (read jobs.tsv).

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
