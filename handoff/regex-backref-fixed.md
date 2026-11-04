# REGEX-BACKREF-FIXED (X-3b)
Branch task/regex-backref-fixed. Change: back-reference patterns start with a ring of 8 frontiers (BR_RING) instead of 16;
longer threads use the pending list. regexec_backref 24,493 -> 22,913 instr/call (glibc 21,970; 1.11 -> 1.04); no other regex row moved.
Not parity; see docs/saturation/06 row. Test: crates/core/tests/regex_backref_ring.rs. Plants (in ~/work/REGEX-BACKREF-FIXED/plants.sh on shiki):
ring boundary off by one, pending threads dropped. Plant (d) of the plan (scratch clearing) was not used: the fixed cost was the ring setup, not clearing.
Jobs on shiki: regex-backref-fixed-plants, -static (clippy+static), -gate (affected --base origin/next), -percall.
NOTE: label regex-backref-fixed-percall also exists once earlier in jobs.tsv (baseline code, 24,493): the later row is the final head.
Earlier: regex-backref-fixed-base (baseline), regex-backref-fixed-percall2 (after, same code, before the test commit).
NOT VERIFIED at hand-back: gate, plants, static, final percall were still running.

## REGEX-BACKREF-FIXED-2 (continuation)
Cause of the plants BASELINE-RED: the late-start test used `.` for the middle atom, so it matched the first `x` and the leftmost match was at 0 (test bug, not matcher). Now `[.]`; also a type alias (clippy) and clause IDs s14_9_c_1_2 (trace lint). Matcher code unchanged since the first run.
Final-head jobs on mei: regex-backref-fixed-2-gate2 (clippy+static+affected, rc 0, 1068 passed), regex-backref-fixed-2-plants3 (p1 ring boundary, p2 pending dropped: both CAUGHT, script checks each plant applied).
Percall not rerun (no matcher change): the 22,913 instr/call figure above stands. Superseded: -plants, -static, -gate, -plants2 (red on test/lint defects fixed since).
Next step: none for this story; further fixed-cost cuts (glibc 21,970) would need new work.
