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
