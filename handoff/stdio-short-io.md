# STDIO-SHORT-IO / -2 / -3
Branch task/stdio-short-io-3 (contains task/stdio-short-io-2). Tests: crates/core/tests/stdio_short_io.rs (22 live + 1 #[ignore]d `reference_values`).

## State
- Run 3 added 8 tests: fscanf (hand-built SysV va_list into `vfscanf`) over a pipe: input in pieces, EINTR mid-conversion / before input / after the first conversion (each with a clearerr retry), EOF mid-conversion (6 shapes), mismatch leaves the byte and EOF repeats; and `fflush` on a full nonblocking pipe.
- Probe-backed: C probe (kept in the scratch dir, run on keke against the reference): every value pinned equals the reference's, and Liella matched on the first run (no difference found, no code change).
- Reference results pinned: EINTR after "12" in `%d %d` gives return 1 (12 stored), ferror, no feof, errno EINTR; EINTR with no input gives EOF with ferror; EOF mid-number stores it ("0x" with %i stores 0, feof); mismatch is 0 and leaves the byte.
- fflush on a full pipe: -1, ferror, EAGAIN; a second fflush returns 0 with ferror still set; the 12 buffered bytes are DISCARDED (nothing arrives after the reader drains). Liella does the same. (Not "a second fflush completes with the data": the reference drops it. Follow-reference, pinned.)
- Run 2's differences 3-5 belong to STDIO-EOF-PARTIAL-REFERENCE; difference 2 (short write retry) kept, COMPAT-13 (renumbered at the port: next already used 11).

## Plants
~/work/STDIO-SHORT-IO-3/plants.sh <n> on keke (RAM copy, exit status = 1 iff a plant survived): the 4 run-2 plants plus flush-error-swallowed, scanf-error-as-eof, scanf-pushback-lost. Run 2's script was fine in exit logic; the garbled log was a doubled invocation, not a script defect (unverified).

## Next step
Nothing for this story. Plan rows: plan/next-batch-1024-2.md (STDIO-SHORT-IO).
