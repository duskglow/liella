# PERF-GETLINE-LONG (WIP, NOT READY: no plants, gate2 not read, gl_80 base not measured)

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
