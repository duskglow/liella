# PERROR-HERROR-REFERENCE (ported by PERROR-HERROR-REFERENCE-3)

Branch task/perror-herror-reference-3, cut from origin/next.

## State
Ported from the pre-release history (task/perror-herror-reference-2, which had no merge base with next): one commit with
the product change, tests and docs of runs 1 and 2. Conflicts resolved:
- docs/compat-flags.md: next already has two rows numbered COMPAT-11 (`__libc_freeres`, `fnmatch`); both kept as on next.
  The perror errno row is now COMPAT-15 (next free id; differences row in docs/differences-from-glibc.md points to it).
  RULING NEEDED (informational): the duplicate COMPAT-11 on next should be renumbered by whoever owns it.
- Everything else applied cleanly (next had no competing change to report.rs, netdb.rs, the tests or spec).

## Decisions in force
- perror byte-orients an undecided stderr (a wide one stays wide); herror writes one text straight to descriptor 2;
  perror/herror keep the caller's errno (D-2.2, D-2.3; COMPAT-15 registered).

## The sym_perror failure (trains 234/236)
`liella-boundary::s12_stdio_symbols sym_perror` asserted `fwide(stderr, 0) == 0` after perror, the old "perror leaves stderr
unoriented" rule that this change reverses. It now expects `< 0` (byte-oriented; the child's stderr is a pipe, a fresh stream).
For MEMSTREAM-FERROR-ROW (same failure): check whether that test asserts an unoriented stream or an error flag after a
perror/herror-like write; if so it is the same stale expectation, not a product bug.

## Open
- diag_diff: after the first perror on a descriptor 2 redirected by dup2 after start-up, the reference leaves stderr
  undecided; Liella orients in both cases (KNOWN line in diag_products). Leave unless the owner rules otherwise.
- spec/clauses.tsv s2_2_c_6 text is unchanged (still describes the old rule); regenerate/edit via the spec owner.

## Next step
None for this story beyond the verification jobs named in the hand-back.

## Superseded note (run 5)
Plants unproven: perror-herror-reference-3-plants/-plants2 used a wrong package name; -plants3 baseline was red because the RAM copy lacks `cargo xtask so` output. Rerun the four plants (report.rs orient true->false twice, netdb.rs errno restore, netdb.rs fd 2->1) after building the product in a fresh copy.

## Run 5 (PERROR-HERROR-REFERENCE-5), head see report json
- Renumbered the perror option COMPAT-13 -> COMPAT-15 (compat-flags, differences row, handoff, run-3 report). Fixed the stale module note in report.rs.
- First plants run (plants, head bc51b86): fd1 CAUGHT; orient1, orient2 (report.rs true->false, text form / bare form) and
  errno_restore (netdb.rs herror) SURVIVED: sym_perror's two calls masked each other, and herror's errno had no test.
- Added sym_perror_text_orients, sym_perror_bare_orients (s12_stdio_symbols) and an errno check in sym_herror (s16): head a5d8c126.
  clippy --workspace --all-targets --all-features and cargo xtask static pass on a5d8c126.
- NOT VERIFIED at hand-back: jobs perror-herror-reference-5-gate2 and -plants2 (keke) were still running on a5d8c126.
  Next step: read both from ~/logs/jobs.tsv on keke; plants2 must print CAUGHT for orient1, orient2, errno_restore, fd1
  (log ~/logs/perror-herror-reference-5-plants2.log). A SURVIVED line means the new test does not reach that path.
