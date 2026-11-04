# CUTOVER-REDS-TRIAGE (planning only, 2026-11-04)

Branch task/cutover-reds-triage. No code changed. JOBS: none.

Method: read-only `ops` state (story states, last red event, held reasons, family = same name without `-N`),
`~/state/cutover/carry.tsv` (applies / CONFLICT-with-next), origin heads, liella-historic
(`git rev-list --count origin/next..origin/<branch>` = commits not already in historic next), and the 11-04
scope rulings (`state/PH14-OWNER-RULINGS.md`). 327 red: 55 whose latest red reason is "CUTOVER 10-24"
(queued_ci, held, at the public cut) and 272 others. Of the 272, 149 have a landed or live continuation in
their family and are CLOSE by that evidence alone; the other 123 collapse to 56 chains, only the newest member
of a chain matters (earlier members are CLOSE: superseded iteration).

Counts: REQUEUE 7, CONTINUE 31, PORT 0, CLOSE 289 (incl. 55 chains' older members and the 149 above).
PORT is empty on purpose: every historic-only branch is either already contained in historic next (0 ahead)
or has a carried successor. NOT VERIFIED: containment between integration branches (see the notes), and that
"lands through INTEGRATE-*" held rows really are in the public root (the cut rebuilt the root; CI marked them
landed in the old history).

## REQUEUE (carry "applies", gated green, held only by release policy; coordinator: `ops finish` + `ops queue`)

| Story | Evidence |
|---|---|
| INTEGRATE-MATH-4 | applies; "RELEASE CUT 2" hold; 67 commits, the math integration branch; requeue FIRST |
| XTASK-PLANTS | applies; RELEASE CUT 2 hold only |
| REGEX-WALLCLOCK-TESTS | applies; RELEASE CUT 2 hold only |
| WALLCLOCK-ASSERT-SWEEP | applies; RELEASE CUT 2 hold only |
| COLLATE-STRXFRM-DA-WEIGHT-2 | applies; RELEASE CUT 2 hold only |
| TIME-PROBE-MIDNIGHT | applies; red only as a member of train 228 (hconf clippy error was in other stories) |
| FUZZ-REGEX-TIMEOUTS-5 | applies; train 234/236 red was sym_perror (PERROR-HERROR-REFERENCE-2), not this branch |

Check before queuing MATH-M6-FPENV-HONOR-4 and MATH-M6-F32-BENCH-2 (both applies, RELEASE CUT 2 hold): if
INTEGRATE-MATH-4 already contains them (`git merge-base --is-ancestor`), CLOSE them; else REQUEUE after it.

## CONTINUE (partial or conflicting work worth finishing; base is origin/next unless named)

| Story | Base / what is left |
|---|---|
| HOSTCONF-DIAGNOSTICS-2, HOSTCONF-OPTIONS-PROBE-3, TIME-SPEC-GAPS-SETTLE-3, NS-NETWORKS-DNS-2 | all apply/conflict; train 228 red: `hconf` field missing in `liella_legacy::nss::Ctx` (clippy) and test ns_gaps. One story: compose the four on next, add the field, regenerate, re-gate |
| STRTO-INTERNAL-GROUP-5 | applies; plants-3 lost in the queue: rerun plants on a fresh copy, then finish (then REQUEUE) |
| PERROR-HERROR-REFERENCE-2 | CONFLICT, 6 commits; its sym_perror test broke trains 234/236: rebase on next, fix s12_stdio_symbols sym_perror |
| MEMSTREAM-FERROR-ROW | CONFLICT; same sym_perror train failure: rebase after PERROR-HERROR-REFERENCE-2 |
| STDIO-SHORT-IO-3 | CONFLICT, gated before the cut: rebase on next |
| STDIO-EOF-PARTIAL-REFERENCE-3 | (landed or live continuation: see CLOSE for -2) nothing left |
| COMPAT-FLAG-IDS-UNIQUE-2 | CONFLICT: rebase; id numbering has moved on next (`cargo xtask static` ids check) |
| SYSLOG-RESEND-ROW, HARNESS-REEXEC-IGNORED | CONFLICT, RELEASE CUT 2 hold only: rebase on next, re-gate |
| MATH-M6-F64-GAP-ASSEMBLY-2, -EXP-REDUCE-2, -ROUND-TEST-2, MATH-M6-F64-GAP-BREAKDOWN | CONFLICT on docs/saturation-register.md (INTEGRATE-REGISTER landed): rebase, regenerate the register by its xtask, never hand-merge; chain A of 5 (13.5) |
| PERF-STRSTR-LONG, PERF-CTYPE-LOC, PERF-WCTYPE-UTF8-REST-3, PERF-MEMSET-4K-SSE2-2, PERF-GETC-PUTC-TRIMS-2, PERF-FNMATCH-PATHNAME-3, BENCH-ROWS-UNMEASURED | CONFLICT, REGISTER FREEZE hold released now: rebase on next, regenerate the register, re-gate (counts); WCTYPE-UTF8-REST and -2 are CLOSE |
| MATH-M5-CPLX-F128-ZIV-A-4, -ZIV-B1-3, MATH-M5-CPLX-F128-ZIV-B2-3, MATH-M5-CPLX-F128-2 | applies; owner #1719/#1758: lands only with the full Ziv step. Base: MATH-M5-CPLX-F128-ZIV-C (ready, held, "Ziv-chain base after the cutover"); finish the chain through ZIV-C |
| MATH-M6-FPENV-HONOR-REST-3 | CONFLICT, 40 commits: software-path FTZ/DAZ; check what INTEGRATE-MATH-4 already holds, then rebase the rest |
| PERF-SSE2-REMEASURE-2 | CONFLICT, 3 commits: bench-tool rebase on next (xtask bench.rs moved) |
| TEST-SILENT-SKIPS | CONFLICT, 8 commits; red at verify 15:23; rebase and continue per plan section 1 |
| XTASK-BENCH-NAMES | CONFLICT in xtask/src/bench.rs vs next, 2 commits: merge next, re-gate |
| ENOMEM-FS-WALK-2 | origin only (no historic branch); run the walker tests on the test-only allocation-failure hook (ALLOC-FAIL-INJECT-4 carried); finish |
| LEGACY-OBJECTS-2 | 0 commits ahead of historic next (verify it landed via INTEGRATE-COMPAT-A/B); if not, rescope to R7 (_IO_* objects COMPAT-ONLY, inert) |

## PORT

None. No red story has unique work only in liella-historic without a carried successor or a clean containment.

## CLOSE

| Stories | Evidence |
|---|---|
| RPC-SERVER-6, RPC-SERVER-TCP-2, -TCP-3, RPC-CLIENT-TCP, RPC-AUTHUNIX, RPC-XENCRYPT-2, RPC-REPLY-SOURCE-2, RPC-GETRPC-CORPUS-2 (and all older RPC-*) | "lands through INTEGRATE-COMPAT-A/B" (both landed 10-24); RPC/NIS is now stubs (11-04 ruling R13; LEGACY-RPC-NIS-STUBS-3 running) |
| MALLOC-COMPAT-6 (and -1..-5), COMPAT-NSS-RESOLV-PT2-9 (-1..-8), COMPAT-NSS-RESOLV-4 (-1..-3), REGEXP-H-6, COMPAT-LIBM-1..3, COMPAT-SYMVER-GEN-2 | through INTEGRATE-COMPAT-A/B; -3/-8/-5 etc. are 0 commits ahead of historic next; REGEXP-H-7 and COMPAT-SYMVER-GEN-4 landed |
| COMPAT-BEHAVIOUR-1..8, COMPAT-CONDVAR-OLD-1..3 | LG-2/LG-3 old-behaviour compat versions: only for a user the LG-19 corpus finds (R14); 0 ahead of historic next. COMPAT-BEHAVIOUR-7 "group 1 done" is in historic if a corpus user appears |
| MATH-M6-F32-POWF, MATH-M6-F32-REST-2/-3, MATH-M6-F32-TRIG, MATH-M6-F32-BENCH, INTEGRATE-MATH-1..3, MATH-M6-FPENV-HONOR-1..3, MATH-M6-SQRT-FTZ-1..6, MATH-M6-F64-GAP-LOG-TABLES | through INTEGRATE-MATH-4 (REQUEUE) or MATH-M6-FPENV-HONOR-4; SQRT-FTZ-6 "superseded by FPENV-HONOR"; GAP-LOG-TABLES-2 landed. NOT VERIFIED: F32-TRIG / F32-BENCH contained in INTEGRATE-MATH-4 |
| DOCS-REGISTER-DEDUP, PERF-MEMCMP-DIFF-AVX2, PERF-MEMCMPEQ-HEAD, RELEASE-PUBLIC-TREE-TIDY(-2) | INTEGRATE-REGISTER landed |
| MATH-M5-CPLX-F128-ZIV, MATH-M5-CPLX-F128, -ZIV-A..A-3, -ZIV-B1..B1-2, -ZIV-B2..B2-2 | superseded by the ZIV-A/B/C split (#1759); older iterations of the CONTINUE chain |
| PERF-WCTYPE-UTF8-REST, -2, PERF-FNMATCH-PATHNAME(-1,-2), PERF-GETC-PUTC-TRIMS, PERF-MEMSET-4K-SSE2, PERF-HEAP-PAIR-IVB(-1..-2) | older iterations (newer -N carried); PERF-HEAP-PAIR-IVB-3 reverted (#1710), negative goes to register 13.2 row 2 |
| STDIO-EOF-PARTIAL-REFERENCE-2, PLAN-NEXT-BATCH-1024-2, RC-RECOUNT-1024-2/-3/-4, BENCH-REFRESH-1023 | -3 landed / planning or report-only (superseded by RC-SORTC-BISECT, landed) / stale measurement |
| MEMCMP-EQ-24-32, MUTATION-CAMPAIGN-1/-2/-2B, MATH-M5-CPLX-LD-1/-2, LEGACY-PROBE-COMPAT-1/-2 | 0 commits ahead of historic next (already contained) |
| GATE-THREADS-REQUIRED(-2) | +4.5% gate wall on kanon (#1113); the host set changed 11-03; re-measure in a fresh story if wanted |
| Remaining 149 reds with a landed or live family member | landed continuation (by name; not individually verified) |

## RULING NEEDED

None. One recommendation for the coordinator: the 31 CONTINUE rows are mostly rebases on one moving base; run them
in small waves (the register-edit group and the hconf group each as ONE integration story), as the earlier
INTEGRATE-* stories did.

IMPROVEMENT: `ops status --json` omits per-story state; a `ops list --state red --json` with last red reason and
carry status would remove the need to open the database for triage.
