# PERF-GETTEXT-LANGUAGE-STEADY(-3) (branch task/perf-gettext-language-steady-3, role keke:~/w/n2)

State: INCOMPLETE (target 1.05x not reached on any row; no timing yet). Branch = -2's head febbb7f8 (next 2c1e25b9
merged + the per-call spec validation/percall/gettext.c + docs) plus two library commits in crates/legacy/src/intl.rs:
- b84685e0 Inline the address test of the gettext resolution key (`Resolution::matches` #[inline(always)] down to the
  generation/category/length/address words; list compare `same_list` and text compares `same_texts` out of line).
- 97ae80d0 Inline the gettext hit plan into translate (`Resolution::hit` #[inline(always)], one caller).
No new unsafe, no ABI or behaviour change (the inlined fast path returns true only in cases the old expression did).

## Ruling #2896 / #2937 (ngettext_found +3)
keke:perf-gettext-language-steady-2-percall2 (rc 0): base 3d7e667a (fused compare) vs next's back-out: found 623 -> 630,
ngettext 728 -> 735 (+7 each), steady 909 -> 736. So the cost belongs to next's back-out (shared path), as #2937 says.
This run's change removes it and more (below).

## Profile (one gettext_found call, --list, Zen 1; scratch listing, not in repo)
621 i: `matches` 70 = 16 register saves/restores/ret + ~12 locale/list word offsets computed before the address test +
the real compares; `hit` 147 = 6 saves + 8 spilled arguments + `Mo` construction (~25) + selector compare (~20) + index
probe; `translate` 86 between `matches` and `hit` = memo gate loads, plan bounds and hit's argument setup (no cut found).
Tool note: `wl_trace.sh --list` lines reach the job log only with `WLP_TAIL=0` (the default keeps the last 60 lines);
the log line is cut at 150 characters, so long mangled names leave no instruction text: objdump the pc binary at the
listed addresses (`~/wl/percall/<sha>/<key>/pc`) to annotate.

## Counts (wl_percall, keke, Zen 1, instructions per call; glibc / -2 head febbb7f8 / b84685e0 / 97ae80d0)
gettext_found 455 / 630 / 580 / 558 (1.23); ngettext_found 597 / 735 / 685 / 662 (1.11);
gettext_language_steady 466 / 736 / 706 / 684 (1.47); gettext_language_alternating 2227 / 1358 / 1305 / 1283.
No row slower at either step. Saturation register 14 row 6 run 6 records it.

## Jobs (keke)
- perf-gettext-language-steady-3-list-found (trace, on febbb7f8), -percall1 (base febbb7f8), -percall2 (base b84685e0): rc 0.
- perf-gettext-language-steady-3-gate (head 97ae80d0 tree; clippy --all-features -D warnings, xtask static, affected
  --base origin/next --run; 19 of 233 targets): rc 0.
- NO PLANTS VERIFIED. Plants file: keke ~/work/PERF-GETTEXT-LANGUAGE-STEADY-3/plants.txt (4 plants on the new fast
  path, `cargo test -p liella --test catalogs_products`, the end-to-end catalogs test with the LANGUAGE-change case).
  -plants: baseline rc 101 (the file said `-p liella-core`; the crates/core package is `liella`; fixed).
  -plants2: baseline rc 101: in the plants copy (/dev/shm/claude/PERF-GETTEXT-LANGUAGE-STEADY-3/plants-2/src)
  `crates/core/tests/oracle/catalogs_run.sh` is MISSING, although it is tracked in git and present in the role (its
  siblings such as aio_run.sh were copied). So `cargo xtask plants`' source copy drops that file: find out why
  (its exclude rules) before plants can use catalogs_products. Also, the direct tier did not select catalogs_products
  for a crates/legacy/src/intl.rs change: check the map, or name the direct test of intl.rs that covers the
  resolution cache.

## Open / next step
1. `cargo xtask bench --compare --counters --runs 5 cbench_opt` (gettext rows) on this head (not run: budget), then
   timing on a booked timing host for the rows before any parity claim.
2. Shared path: `hit`'s `Mo::from_fields` built on the stack and passed by pointer to `find_in_hinted` (~25) and the
   selector compare (~20); `translate`'s 60 of key setup before the slot test.
3. LANGUAGE's ~95 (runs 2-4 say no safe cut without new unsafe in liella-boundary: RULING if tried).
Plan rows: saturation 14 row 6 (runs 1-6), D-7.5, D-7.12.
