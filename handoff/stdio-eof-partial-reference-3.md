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
