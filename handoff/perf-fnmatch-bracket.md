# PERF-FNMATCH-BRACKET handoff
Branch task/perf-fnmatch-bracket. Code head: c8804248 (report commit follows it).
Change: bracket_has in crates/core/src/pattern/fnmatch.rs tests Char and Range members by plain comparison when no FNM_CASEFOLD and code-point collation (no per-member collation lookup or fold). Rules locales and casefold keep member_has.
Percall spec added: validation/percall/fnmatch_bracket.c (the brief's parity1105.c is not on origin/next).
Results so far (en_US.UTF-8, mei, head a1afbbb, job percall3): fnmatch_bracket 1815 base to 1797 new, glibc 2177.8. This is the UTF-8 row only. The brief's C-locale row was NOT measured yet: job percall4 (--locale C) is running.
Gate on the first code head (gate, rc 0) is superseded by gate2 on the final head.
Plants: the first plants2 run showed the differential test is vacuous (the general matcher tries direct first): range and casefold plants SURVIVED. Fixed by the test direct_bracket_ranges_hold_both_ends_and_follow_case_folding; plants3 re-runs on the final head and must be read.
The profile step (bracket parse versus star backtrack) was not done; star re-scan was not touched.
Next: read gate2, plants3, percall4 in the jobs.tsv on mei; if the C-locale ratio is still above 1.05 profile further (second change needed).
