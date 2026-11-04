# REGEX-BACKREF-FIXED (X-3b)
Branch task/regex-backref-fixed. Change: back-reference patterns start with a ring of 8 frontiers (BR_RING) instead of 16;
longer threads use the pending list. regexec_backref 24,493 -> 22,913 instr/call (glibc 21,970; 1.11 -> 1.04); no other regex row moved.
Not parity; see docs/saturation/06 row. Test: crates/core/tests/regex_backref_ring.rs. Plants (in ~/work/REGEX-BACKREF-FIXED/plants.sh on shiki):
ring boundary off by one, pending threads dropped. Plant (d) of the plan (scratch clearing) was not used: the fixed cost was the ring setup, not clearing.
Jobs on shiki: regex-backref-fixed-plants, -static (clippy+static), -gate (affected --base origin/next), -percall.
NOTE: label regex-backref-fixed-percall also exists once earlier in jobs.tsv (baseline code, 24,493): the later row is the final head.
Earlier: regex-backref-fixed-base (baseline), regex-backref-fixed-percall2 (after, same code, before the test commit).
NOT VERIFIED at hand-back: gate, plants, static, final percall were still running.
