# PERF-SPRINTF-CHK-NLINK (WIP, NOT READY)
Branch task/perf-sprintf-chk-nlink. Trace (job perf-sprintf-chk-nlink-trace0, keke): chk_ls 893 instr/call: render_simple<false,true> 324,
boundary vsnprintf (inlined simple_into) 224, simple_conv 100, printf_text 60, vsnprintf6 49, simple_star 37, strlen 33, va_long 29.
The _chk entry is not the gap (confirms the 10-23 result).
Change: PairS plan (core printf.rs pair_s/is_pair) + render_pair_s (boundary printf.rs) writes "%s %*s " as pieces; core test pair_s_pieces_are_what_render_simple_writes.
NOT VERIFIED: the last commit's clippy/test not re-run after the saturating_sub fix; no counts after, no gate, no plants, no static.
Next: sync, cargo xtask clippy + static, test pair_s_pieces, gate, plants (pair_s conditions, fill, star sign), percall chk_ls + bench cbench_opt before/after, saturation row 3 update.
Not done: kept-plan (stream) path does not use render_pair_s.

## Run 3 (budget hit early; the two brief files cost ~35k context)
Added 17 ineligible look-alikes to pair_s_pieces_are_what_render_simple_writes (not list). Synced to keke:~/w/n3. NOT RUN: clippy, static, test, gate, plants (rerun, all 4 must be CAUGHT, esp. gate-kinds), counts. Plants script from run 2: ~/logs/perf-sprintf-chk-nlink-2-plants.log on keke (ssh 10.0.100.53, hostnames do not resolve from the agent shell).
Next: clippy+static on n3, run core test, gate, plants in /dev/shm/claude/PERF-SPRINTF-CHK-NLINK-3/plants-1, percall chk_ls.

## Run 4 (keke, role n3)
Run 3's plants run (plants-1) on the 24-look-alike test: first-flags, star-sign, after-off CAUGHT; gate-kinds SURVIVED. Cause: `kinds & !F_STAR` in pair_s is implied
by n == 2 (SIMPLE_MAX 8, so no F_MORE) and two `s` letters (no F_FLOAT): the condition was dead. Removed it (commit "Drop a pair_s condition...");
replacement plant gate-n (`n != 2` -> `n < 2`) is caught by "%s %s %s" in the `not` list (expected, not yet seen).
Lint (clippy --all-targets + xtask static) was green on the previous head (job -lint, rc 0); rerun as -lint2 on the final head.
Jobs queued on keke at hand-back (host busy, 20+ min queue; all still QUEUED): perf-sprintf-chk-nlink-4-lint2, -plants2 (script ~/work/PERF-SPRINTF-CHK-NLINK-4/plants2.sh, 4 plants),
-gate2 (affected --base origin/next), -percall (wl_percall chk_ls vs origin/next). Earlier -plants (rc1: gate-kinds survived), -gate (killed, old head) are superseded.
NOT VERIFIED: everything on the final head until those jobs finish. Next: read their rows in ~/logs/jobs.tsv; fix only what fails; update saturation row 3 with the percall numbers.
