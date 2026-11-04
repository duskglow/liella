# STDIO-EOF-PARTIAL-REFERENCE-3

Ported the code of STDIO-EOF-PARTIAL-REFERENCE-2 (commits 80dd4294, 232131ed; the hand-off commits were not ported) onto the
public root as one commit; it applied without conflicts. Intent kept: fgets after a partial line + read error is NULL with the
bytes left and no NUL; getdelim returns the partial record and stops at the error indicator; EOF is sticky until clearerr.
Extra fix: the stream_engine unit test `read_line_into_stops_at_delimiter_full_output_end_and_errors` still expected the old
Ok((3,true)); now expects Err(EIO) with the bytes kept (the gate was red on it).
Verified: gate (eofp3-gate2: 4475 passed, 0 failed), plants (eofp3-plants: sticky, corefill, errflag, fgetsnul all CAUGHT),
clippy and static clean.
Still open (not done): the instructions:u check of the getc/fgets/getline bench rows (origin/next vs head vs glibc) was never run;
getdelim also returns a partial record on an ENOMEM from the sink (not reference-checked).

## STDIO-EOF-PARTIAL-REFERENCE-4
CI red cause: the port made getdelim return the bytes read so far on ANY error, including a failed buffer growth (ENOMEM from
the sink), so `enomem_stream_grow getdelim_out_of_memory_keeps_a_valid_buffer` got a partial record instead of -1/ENOMEM and
kept reading /dev/zero. Fix: the partial record applies to read errors only; a failed growth (ENOMEM/EOVERFLOW) is the error
as before (`grow_failed` in `stdio_compat::getdelim`). Merged origin/next. Still open: the instructions:u check of the
getc/fgets/getline bench rows (next vs head vs glibc) was never run.

## STDIO-EOF-PARTIAL-REFERENCE-5
CI red cause (`stdio_short_io pipe_getline_interrupted_then_resumes`, line 500 `errno == EINTR`, left 0): getdelim now returns the
partial record ("par") as a success after an interrupted read, and the export only sets `errno` on an Err, so `errno` stayed 0.
(Reproduced on keke before the fix; the "10 min, 4.4 GB" in the CI line is the harness, not this test's loop.)
Fix: `Env::set_errno` (one implementor, boundary), called in `stdio_compat::getdelim`'s partial-record path with the read error;
the error indicator is set as before. Not changed: the test (its expectation, errno EINTR + ferror + no feof, matches the reference).
Merged origin/next. Plants: `noerrno` (drop the set_errno), `growpartial` (partial record on a failed growth).
Bench check (instructions:u, cbench_hot, next vs head, glibc): job `stdio-eof-partial-reference-5-bench`; results in keke
~/work/stdio-eof-partial-reference-5/bench-{next,head}.log. Next step: read those logs against the getc/fgets/getline rows.
Results (final head): gate stdio-eof-partial-reference-5-gate (FULL affected: 5117 passed, 0 failed, conformance 14/0), plants CAUGHT x2,
bench (cbench_hot, liella_instr_user, next vs head): getc 50.0/50.0, getc_unlocked 43.0/43.0, getline_80 175.8/175.8, getline_long 3390.2/3390.2
(glibc 27.4/15.3/214.0/6914); no fgets row exists in cbench_hot, so fgets was not counted. No row slower.
