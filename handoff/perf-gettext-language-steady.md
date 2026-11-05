# PERF-GETTEXT-LANGUAGE-STEADY (branch task/perf-gettext-language-steady, role mei:~/w/s1)

State: DO NOT LAND pending a ruling. Code head 123dfe88 (proven by the jobs below); a docs commit on top (this note, register
row 14 "Row 6 run 4", report json).
Commits: 9d0076cd validation/percall/gettext.c (new per-call spec: found, ngettext found, LANGUAGE steady and alternating,
the cbench_opt fixture). 123dfe88 the boundary host (crates/boundary/src/catalog.rs) drops `language_probe`, `text_at` and
`text_equals`, so `intl::translate` reads LANGUAGE through `Host::language` again (the fused aligned compare of
GETTEXT-LANGUAGE-ALIGNED-COMPARE-2, measured +131 by its author, was still on next: the back-out branch
task/perf-regress-1023-1105 is not in next).

## Profile (wl_trace, one call, Liella; reference = glibc per-call total)
- found (614 i; glibc 446): translate 254, Resolution::hit 147, matches 74, find_in_hinted 65, same_bytes 21, gettext 28,
  strlen 11. Phases: entry and msgid strlen ~50, locale/lock/LANGUAGE/clock ~60, slot match 74 + key setup ~80, catalog
  hit (hash hint, find, compare) ~230, return ~20. The gap to glibc (~170) is the shared found path, not LANGUAGE.
- alternating (937 i on next): translate 341, hit 147, matches 118 + closure 38, text_equals_words 93, find_in_hinted 65,
  env_find 41, strlen 22.
- steady: the trace of call 11 stepped into the vDSO clock read (the one-second recheck fell on the traced call under gdb;
  200000-step limit). Re-run with a larger --skip. Per-call count says it all: 909 on next, 742 after.

## Counts (mei = Skylake-SP; jobs perf-gettext-language-steady-before / -after, rc 0)
percall glibc / next / change: steady 457 / 909 / 742; alternating 2041.6 / 1511.4 / 1361.5; found 446 / 652.8 / 626;
ngettext 586 / 728 / 731.
cbench_opt before / after / glibc: steady 1002.6 / 803.5 / 443.6 (2.26 -> 1.81); alternating 1273.7 / 1120.1 / 873.0
(1.46 -> 1.28); found 616.1 / 609.7 / 443.9 (1.39 -> 1.37); ngettext 750.6 / 787.7 / 670.8 (1.12 -> 1.17, SLOWER).
Note: the "before" bench ran while the role was at 9d0076cd (next code); a sync attempted mid-run did not apply.

## Gate and plants
- gate2 (clippy --workspace --all-targets --all-features -D warnings, xtask static, xtask affected --run): rc 0, 10 test
  targets + 3 conformance programs, 236 passed. plants2 (cargo xtask plants, 2 plants): 2 CAUGHT (LANGUAGE ignored by
  the host; list compare in matches dropped), no WARN.
- Superseded: -gate (role still at 9d0076cd: selected 0 suites), -plants (wrong package name liella-core; baseline rc 101).

## RULING NEEDED
- ngettext_found +37 in cbench (+3 per call) against steady -199 / alternating -154. The LANGUAGE-unset path does the
  same work before and after; probably codegen of the translate instantiation changing. Recommend: profile ngettext_found
  (wl_trace ngettext_found ngettext, base vs head, --list) before landing; land if the delta is codegen noise.
- Overlap with task/perf-regress-1023-1105 (also backs out the fused compare): land only one. This branch leaves the
  unused legacy defaults (`Host::language_probe` etc., `Resolution::list_text`, `Key::list_checked`) and
  `strz::text_equals_words` + its test in place; the full removal belongs to whichever back-out lands.

## Next step
Profile ngettext_found (base vs head). Then the shared found path: `matches` 74 instructions for a key whose domain and
locale should match by address (check why the address fast path is not taken), and `Resolution::hit` 147.
Target 1.05x is not reached: steady 1.81 / 1.62 per call, found 1.37 / 1.40.
