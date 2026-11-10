# RELEASE-NOTES-1107: release notes and README status for the 11-07 pre-release

**State:** branch `task/release-notes-1107`, cut from origin/next 70cb44d76; one docs commit (docs/RELEASE-NOTES.md,
README.md) plus this handoff and handoff/reports/RELEASE-NOTES-1107.json. Docs only: no code, no tests, no plants.
Left for the owner's review; the coordinator lands it before the promotion.

## Measured tree
origin/next moved during the run (70cb44d76 -> 9643c81b0: LIBM-F64-LOG-QUICK-2, SIGNAL-TEST-THREAD-AUDIT-2,
XTASK-BENCH-TUNABLES-AND-MATHTOOLS). Both were measured; the notes describe **9643c81b0** (it holds the log quick
phase the brief names). It was measured from a detached scratch worktree synced to keke:~/w/n3 (my branch has no
merge of next; the brief allows none). The branch's own tree is 70cb44d76 + docs; the notes merge cleanly onto
9643c81b0 (neither file changed between the two).

| job (keke) | tree | result |
|---|---|---|
| release-notes-1107-status | 70cb44d76 | 5465 specified, 577 pending (SUPERSEDED by -2: next moved) |
| release-notes-1107-symcov | 70cb44d76 | 557 undefined (SUPERSEDED by -2) |
| release-notes-1107-status-2 | 9643c81b0 | rc 0: 5467 specified, 577 pending (M 173, P14 85, P15 316, deferral 3) |
| release-notes-1107-symcov-2 | 9643c81b0 | rc 0: 557 of 4305 undefined; phase 14 110, phase 15 161, out of scope 286 |
| release-notes-1107-static | branch tree ee6aea0a | rc 0 (`cargo xtask static`) |

Host CPU for status/symcov: AMD EPYC 7551 (Zen 1), Linux 6.8.0-147.

## Sources of each figure in the notes
- exp: handoff/LIBM-F64-EXP-QUICK-WIRE.md (IVB 1.889 spread 0.675, N100 2.174). log: LIBM-F64-LOG-QUICK.md on
  9643c81b0 (IVB 1.935, N100 2.113). Before-figures: results/timing-sweep-libm-1107-table.md (66b8d3ba).
- sinl/cosl/sincosl cycles: handoff/libm-ld-trig-int.md. expf/exp2f: libm-f32-onepass.md (unbooked timing only:
  no figure quoted).
- libm distribution: buckets computed from the 166 rows of results/timing-sweep-libm-1107-table.md (awk over
  the IVB/N100 time columns); per-precision medians copied from its per-family tables.
- non-libm: results/timing-sweep-1106e-table.md (group verdict counts and medians computed from its table) and
  results/timing-sweep-zen3-1107-table.md (summary row).
- ld.so: DL-LDSO-RUN.md, DL-LDSO-PRODUCT.md, DL-BOOT-RELOC.md; static PIE: STATIC-PIE-LOAD-OFFSET.md,
  STATIC-PIE-BACKTRACE-BIAS-TEST.md; libnsl/RPC: D-29.5, D-29.6.

## Release scans (on the branch tree)
- (a) `cargo xtask static`: rc 0 (job above). Lab names: the notes name CPU classes only; the commit message
  was amended before push to drop a host name.
- (b) Secret scan, pass 1 (tools/secret_scan.py, the RELEASE-SECRET-SCAN method; run locally, read-only, on the
  worktree): 18,651 blobs, 0 hits (12 allowlisted dropped). Pass 2 (gitleaks via tools/secret_scan_cut.sh) NOT
  RUN: gitleaks is not installed locally or on keke and the install needs a network download.
- (c) SECURITY-PRIVATE check of current files: a grep for glibc near crash/overflow/weakness terms (21 lines).
  Process-rule text (SECURITY.md, docs/PRINCIPLES.md, plan phases 14/15) is excluded. The remaining hits are
  given to the coordinator by location in the hand-back only.

## Open / next
- If the promoted tree is later than 9643c81b0, re-run status/symcov on it and refresh the figures (about 10 min).
- RULING NEEDED (owner): whether the notes' claim list is right (written from handoffs, not re-measured).
- INFORMATIONAL: `cargo xtask so` warns `c_malloc` never used (boundary/src/heap.rs:501) in the product build.
- Not to redo: the libm and non-libm timing tables (no new timing was run for this story).
