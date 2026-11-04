# REGEX-LAZY-DFA (ledger X-3a): handoff

State: branch `task/regex-lazy-dfa` (from origin/next `ba27dcfe`); note commit `48dad27f`, then this
report commit. Planning story: no matcher code.

Delivered: `plan/07-decisions/x3-regex-lazy-dfa.md` (PROPOSED), with the profile, the negative
result, the capture-DFA design (16 KiB cap, fallback, time bound), the per-pattern regcomp
choice, the proof plan and the X-3b..X-3e split. Also a negative row in
`docs/saturation/06-printf-regex-wordexp-iconv.md`.

Decisions in force: D-14.4, D-14.12, D-14.13, D-14.16 (read these rows). No new D-row. No
placeholder.

Profile (job `regex-lazy-dfa-profile` on keke role n4, rc 0, 13:08-13:24Z 2026-11-04; outputs in
keke:~/work/regex-lazy-dfa/: compare.log and <row>.rep). Instructions per call, Liella vs glibc:
`regexec_backref` 24,460 vs 21,950 (1.11, the only row above parity); `class_nomatch_1k` 1.00;
`submatch_1k` 0.07; email regcomp+exec+free 0.27; `re_sed_kv` 0.21; `re_sed_swap` 0.08. On the
backref row the matcher is 74.7% of the instructions (run 32.7, attempt 14.3, insert 13.8) and
scratch clearing and copies are 7.7%.

Not to redo: the existence and span DFAs exist (D-14.13/D-14.16). The ledger's "t 3.1 to 5.8"
is stale. A DFA on back-references does not pay: about 10% at most (estimate, NOT MEASURED).

Open: the ledger row X-3 should be marked "X-3a done: negative; X-3b = REGEX-BACKREF-FIXED (S)".
I left that edit to the coordinator because the ledger is a planning index.

Next step: X-3b `REGEX-BACKREF-FIXED` (S): cut `attempt` setup and scratch clearing on the
back-reference path. Proof: gate, plants, percall, static.

INFORMATIONAL: the brief says `Kind: planning` ("run no jobs, JOBS: none") and also asks for a
profile job and static. I ran both and name them.
