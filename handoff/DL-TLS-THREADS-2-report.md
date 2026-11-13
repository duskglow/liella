# DL-TLS-THREADS-2 report: why dl-tls-threads-gate2 was red, and the start-up jitter measurements

Branch `task/dl-tls-threads-2`, from `task/dl-tls-threads` 9769a5c36 (code tree 1f09927be). No code change in this
run (the coordinator's note: measure and report, do not chase the layout until the #4347 ruling). Role mei:~/w/s2
(Skylake-SP, the CI host class, kernel 6.8.0-149); second host keke (Zen 1, kernel 6.8.0-147), GNU ld 2.42 on both.

## Trees measured
| tag | tree |
|---|---|
| n | origin/next 8a2711482 |
| h | this branch's head (code 1f09927be) |
| m | h merged with origin/next (local merge, not pushed) |
| mb | m plus bbe7c0530 re-applied (the cost fix of ruling #5285; local, not pushed) |

Each tree was synced to the role and built in both profiles (`cargo xtask so`; release with `LIELLA_FULL_PROFILE=1`),
then `hot_text_products init_data_clusters_hold_at_every_text_phase` ran against each profile (job
`dl-tls-threads-2-tree-<tag>`). The products were measured with a copy of LAYOUT-STABILITY-RETHINK's `measure.py`
(handoff/layout-stability-rethink.md §1: `liella-crt1.o` merged with a pad object, 19 pads 0..0x10000, every mode
through the relay harness, one warm-up, five runs, lowest kept, perf attribution of true/locale/tool), restricted to the
`pre` and `cold` regions, both profiles, without `layout-check` (jobs `dl-tls-threads-2-meas-<tag>` on mei;
`dl-tls-threads-2-meas-keke` ran the same sweep on keke over the same mei-built products). Scripts and full
matrices: `~/work/DL-TLS-THREADS-2/` on both hosts (`measure.py` patched by environment: `LSR_REGIONS`, `LSR_SRC`,
`LSR_LAYOUT`; `diag.py` names the function of every text fault).

## (2) The data-phase test on mei
| tree | fast | release |
|---|---|---|
| n | pass (tool 54 at all ten phases) | pass (56) |
| h | **fail: tool 55 at cold 0x800, 54 at the other nine** (same as keke's gate) | pass (56) |
| m (h + next) | **pass** (54 at all ten) | pass (56) |
| mb (m + bbe7c0530) | pass (55 at all ten: one more everywhere) | pass (56) |

## (1) Tool-mode faults at the 19 pads (0 30 800 1000 2000 ... f000 10000); mei and keke identical
| tree | profile | pre | cold |
|---|---|---|---|
| n | fast | 54 54 55 55 55 56 56 56 56 56 55 55 55 55 55 54 54 54 54 | 54 54 54 54 54 54 55 56 54 54 54 54 54 54 54 54 54 54 54 |
| h | fast | 54 54 54 55 55 56 56 56 56 56 55 55 55 55 55 54 54 54 54 | 54 54 **55** 54 54 54 55 55 54 54 54 54 54 54 54 54 54 54 54 |
| m | fast | 54 54 55 55 55 56 56 56 56 56 55 55 55 55 55 54 54 54 54 | 54 54 54 54 54 54 55 55 54 54 54 54 54 54 54 54 54 54 54 |
| mb | fast | 55 55 55 55 56 57 57 57 57 57 56 56 56 56 56 56 55 55 55 | 55 55 55 55 55 55 56 57 55 55 55 55 55 56 55 55 55 55 55 |
| n, h, m, mb | release | 56 56 56 56 56 56 56 56 54 54 54 55 55 55 56 56 56 56 56 | 56 x14, 57 at 0xc000, 56 x4 |

Unpadded counts (fast / release): n, h, m: true 11/10, locale 51/53, tool 54/56, fork 5/5, localtime 19/19, float
4/3. mb fast: locale 52, tool 55, float 3 (release unchanged). Ranges over the pads (fast): n pre true 11-12, locale
51-53, fork 5-6, localtime 17-19; cold true 11-12, locale 51-53, tool 54-56. h and m: the same classes with the same
widths (cold tool 54-55). The keke run differs from mei only in "NOISY" flags (a run of the five not equal to the
lowest); every lowest count is the same.

## Cause of the red (perf attribution of the tool mode's text faults, `diag.py`)
Out of the hot group the tool mode runs `_start`, the probe's `main`, `text::builtin_charset`, `CtypeFile::wide`, a
`global_locale` `OnceCell::try_init` closure and `global_locale::setlocale_locked<CSysImpl,Glue>`, on origin/next
and on this branch alike. This branch's code growth reorders cold `.text`: `setlocale_locked` moves from 0x6f1300
(next) to 0x64f580 (h). The `cold 0x800` pad shifts it across the 0x650000 64 KiB boundary: faults at 0x64fd80 and
0x650000, one window more. At `cold 0x4000` (both trees) the probe's `main` crosses 0x4a0000 the same way. So the red
is #4347's jitter (functions outside the hot group meet a window boundary), not this change's TLS words or data
clusters (`.data1` takes ceil(size / 4 KiB) pages and `.rodata1` one window at every phase on all four trees).
Merged with origin/next, `setlocale_locked` sits at 0x6223c0 and the test's pads miss every crossing.

## The cost fix (bbe7c0530) and the jitter
Re-applied on m it builds (dead hot-text keys only warn now): `locale::publish_global` and a
`locale::name::candidates` closure match no section in the fast profile. `publish_global`'s body now sits in
`thread::each_block::<locale::publish_global::{closure}>`, outside the group (0x608300), so fast tool and locale
take one more fault at every phase (tool 55..57, base 55). The pattern of the jitter is the same, shifted by one;
release is unchanged. DL-TLS-THREADS-COST must repair the keys (tools/hottext/regen.sh or `cargo xtask
hottext-repair`, both profiles) and show fast tool back at 54.

## Jobs (this run)
mei: dl-tls-threads-2-lint0 (clippy + static on the inherited head, rc 0), dl-tls-threads-2-tree-n/-m/-mb (rc 0),
dl-tls-threads-2-tree-h (rc 1: the fast-profile test failure reproduced on purpose, evidence only),
dl-tls-threads-2-meas-h/-n/-m/-mb (rc 0), dl-tls-threads-2-diag1/2/3 (diag.py, rc 0), dl-tls-threads-2-pack (rc 0),
dl-tls-threads-2-lint1 (final head, clippy + static). keke: dl-tls-threads-2-meas-keke (rc 0).
Earlier run: dl-tls-threads-gate2 (keke, rc 1) superseded by this analysis; dl-tls-threads-plants3 (keke) was still
running at the start of this run (3 of 3 CAUGHT so far, on the same code tree).

## RULING NEEDED
- How #4347 applies (the coordinator's open question): the red is jitter of cold text outside the hot group, the
  merge with next passes on the CI host class. Recommend: land this branch (it does not move any fault count at the
  unpadded phase: n and h are equal) and leave the fix to LAYOUT-RETHINK (trace-at-build).

## INFORMATIONAL
- Rule slip: one `python3 - <<'X'` heredoc used to patch my scratch copy of measure.py (outside the tree).
- The merge author in the local scratch merge was set with `-c user.name=x` (never pushed).
