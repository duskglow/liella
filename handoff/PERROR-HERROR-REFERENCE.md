# PERROR-HERROR-REFERENCE

Branch task/perror-herror-reference (stacked on task/psignal-stderr-stream).

## Done
- #2041: perror byte-orients an undecided stderr (`report::perror` passes orient=true); a wide stream stays wide.
- #2042: herror writes one text straight to descriptor 2 (no stream, no ordering with pending stderr text, no
  error flag, no orientation); errno is the caller's afterwards, also when the write fails.
- #2043: perror keeps the caller's errno (deliberate; differences row updated, COMPAT-11 registered).
- Pinned in conformance/c/s20_psignal_stream.c (run by psignal_stderr_stream_cases): perror and herror, cases a-d, f
  no longer KNOWN-DIFF. s2_2_c_6 now checks a wide stream; diag_products' orientation line is now a known line only
  for a redirected-after-start-up descriptor 2 (see below).

## Open
- diag_diff oracle: after the first perror on a descriptor 2 redirected by dup2 after start-up, the reference leaves
  stderr undecided (a later perror then orients); a fresh stderr (started with 2 as a file, pipe or /dev/null) is
  byte-oriented after the first perror, which Liella matches. Liella orients in both. Recorded as a KNOWN line in
  diag_products; the dup2 state dependence was not traced further. Recommend: leave (needs a ruling if the owner wants it).
- spec/clauses.tsv s2_2_c_6 still says perror leaves an unoriented stderr unoriented (spec text unchanged here).
