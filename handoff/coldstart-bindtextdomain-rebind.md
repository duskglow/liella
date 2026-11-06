# COLDSTART-BINDTEXTDOMAIN-REBIND(-2): second bindtextdomain + dgettext, same directory

State: branch `task/coldstart-bindtextdomain-rebind-2` (stacked on `task/coldstart-bindtextdomain-rebind`, next e8189dd8),
code commit f84c4ee9 "Group the repeat gettext lookup's text with the hot functions". Role mei:~/w/s3.

## Cause (found in run 2)
Per-row probe of the bench's own cold child (scratch `rowprobe.c` = `#include "cbench_opt.c"` with per-row
perf_event_open user/kernel instructions + page faults and write(-1) markers for strace; mei ~/work/COLDSTART-BINDTEXTDOMAIN-REBIND-2/,
release products, bench env PATH/HOME/TZ). Row `bindtextdomain_gettext`, second pass, before: Liella user 1060 (glibc 812),
**0 syscalls, 2 page faults, +25.7k kernel instructions**; every other second-pass row had 0 faults.
`perf record -e page-faults -d -g` (Liella only): the two faults are instruction fetches in `intl::Resolution::same_texts`
and `catalog::mo::Mo::hinted` (the repeat lookup's code: the resolution cache's text compare and the hinted catalog search).
The first lookup never runs them, and they were not in the hot-text group: `tools/hottext/probes/p_cold.c` bound
`/nonexistent` and looked up once. So the cost was text layout, not the rebind (D-7.12 keeps the catalog) and not an
allocation (no heap fault in that row; allocation count therefore not pursued).

## Change
- `p_cold.c` writes a small .mo, looks it up, looks up an absent domain, rebinds the same directory, looks up again.
- `crates/libc-so/hot-text.list` regenerated with `tools/hottext/regen.sh` (fast, then release; 608 -> 641 keys).
- New test `hot_text_products::init_second_bindtextdomain_gettext_takes_no_text_fault` (page_faults.c mode `rebind`).
- Plants: `handoff/coldstart-bindtextdomain-rebind-2.plants` (drop same_texts / Mo::hinted from the list).

## Counts (mei, Zen 1, release, rowprobe, 3 runs identical)
second bindtextdomain_gettext: faults 2 -> 0, kernel 30.4k -> 4.7k (= the counter-read baseline), user 1060 -> 1058.
first bindtextdomain_gettext: faults 9 -> 5, kernel 152k -> 95k. Cold sequence total 101 -> 93 faults.
Moved the other way: first regcomp_regexec_regfree 25 -> 26 faults (kernel 128.5k -> 141.7k), a layout shift (RULING NEEDED).
User instructions per row unchanged on every row.

## Not done (budget)
- Coordinator step 1 (fresh N100 glibc time for second_/first_bindtextdomain_gettext): not run. Next: stage and run
  `~/bin/timing` on nozomi for `CBENCH_ONLY=cold` (cbench_opt + cbench_opt.host-dynamic, built on a build host).
- Saturation-register row and a D-row for "the hot-text probe covers repeat lookups": not written.
- regcomp +1 fault: not investigated (which page; whether a reorder of the group's tail avoids it).

## Jobs
mei: coldstart-bindtextdomain-rebind-2-gate (clippy, static, affected --base origin/next), coldstart-bindtextdomain-rebind-2-plants.
Diagnostics (evidence, not verification): -probe (rc 1, compile error, fixed), -probe2, -pf, -regen, -rows.

Plan rows: D-7.12. Things not to redo: the same-directory catalog keep (exists), the setlocale-bump theory (refuted).
