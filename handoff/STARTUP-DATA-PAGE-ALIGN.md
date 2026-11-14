# STARTUP-DATA-PAGE-ALIGN: the written start-up data takes the same pages whatever precedes it

State: branch `task/startup-data-page-align-3` (run 3, STARTUP-DATA-PAGE-ALIGN-3), stacked on run 2's
`task/startup-data-page-align-2` c0379f7ee and run 1, cut from origin/next 3e7723955. Code head 86b947476 (+ this
handoff). Role shiki:~/w/sh2. Decision: `records/pending/STARTUP-DATA-PAGE-ALIGN-1.toml` (D-NEW, series D-X, amends
D-X.62, D-X.67; run 3 adds SLOTS to point (2)). Plants: `handoff/STARTUP-DATA-PAGE-ALIGN-plants.txt` (5).
Proof script: `handoff/STARTUP-DATA-PAGE-ALIGN-proof.sh` (scratch clone of origin on the build host; usage in its head).

## What the branch does
* `xtask/src/startup_ro.rs` `relro_plan` (run 1): the start-up `.data.rel.ro*` sections become one
  `.data.rel.ro.local.liella_startup*` group at the head of relro, its first section aligned to 4096.
* `crates/boundary/src/heap.rs` (run 1): `RESERVE_MEM` is `#[repr(C, align(4096))]`.
* `csys/src/machine/entry.S` (run 2): the start file's 64 KiB relro alignment section is `.data.rel.ro.local`, ahead
  of the cluster (behind it, it padded relro by 56 KiB: +1 fault in first `%.2f` and the coreutils-shaped probe).
* `crates/legacy/src/global_locale.rs` (run 3): the 112 KiB locale table `SLOTS` is a `#[repr(C, align(4096))]`
  wrapper; start-up writes its first records, which straddled two pages in release products.
* `layout/contracts.tsv`: STARTUP-DATA-WINDOWS rw-budget-fast 12 -> 7 (run 2), rw-budget-release 14 -> 9 (run 2) -> 8
  (run 3). ro budgets unchanged at 1 (the read-only window is 1 in every build below: the shift cannot move it).
* Tests: `init_startup_data_pages_hold_at_every_data_phase` (hot_text_products: relro and `.lbss` pads; cluster on a
  page boundary with a constant page count; relro under a page of fill; RESERVE_MEM and, run 3, SLOTS page-aligned)
  and `startup_relro_sections_form_one_group_headed_by_the_first` (xtask).

## Proof (run 3; STARTUP-DATA-WINDOWS written start-up pages, fast / release; read-only windows 1 everywhere)
Trees built by the proof script from origin: next 545ad4882, Bessel = task/libm-f32-bessel-large-6 5eff9a8e6,
head d4036f24b (code = 86b947476 less the budget line). Release budget 8 = the highest over kanon, mei and keke.
| host | next | next+Bessel | head | head+Bessel | head fault-bound tests |
|---|---|---|---|---|---|
| mei (Skylake-SP) | 11 / 14 | 11 / 14 | 7 / 8 | 7 / 8 | pass (all four builds) |
| keke (Zen 1) | 11 / 14 | 11 / 14 | 7 / 8 | 7 / 8 | pass (all four builds) |
| kanon | not built | not built | 7 / 8 | 7 / 8 | pass (all four builds) |
At today's next the Bessel merge no longer moves next's count on mei or keke (it did at d53c0d551: 11 -> 13); the
head's count is the same with and without it on every host. Pages, mei (next -> head): fast, relro 0x853000-0x857000,
0x85a000, 0x85b000 (7 pages) -> 0x850000, 0x851000, 0x85a000 (3); data 0x878000-0x87a000 (3) -> 3; RESERVE_MEM 1 -> 1.
Release: relro 0x7b0000-0x7b6000 (7) -> 0x7b0000, 0x7b1000, 0x7b6000 (3); data 3 -> 3; RESERVE_MEM 0x7d6000+0x7d7000
-> 0x7d7000; SLOTS 0x80a000+0x80b000 -> 0x80c000. Full lists: `grep "^proof:" ~/logs/sdpa3-proof2-*.log` on mei/keke.
Sizes: head release libc.a 11692274, libc.so.6 5297248, identical to run 2's head on shiki (SLOTS's alignment costs no
file bytes: `.lbss` is NOBITS). Run 2's like-for-like cost (shiki, base 3e7723955 -> fix): libc.so.6 +4128 fast,
+4096 release; static probe +24 fast, +0 release.

## Jobs (final head 86b947476)
shiki: sdpa3-gate3 (clippy, static, affected --run --base origin/next) rc 0: 32 suites, 11760 passed, 0 failed;
conformance 134 / 0. sdpa3-plants2 rc 0: relro-align, reserve-align, slots-align, crt1-relro-local CAUGHT;
head-last WARN-NO-MSG: the mutant broke STARTUP-DATA-WINDOWS at `so` time (8 pages > fast budget 7) before its unit
test ran; its msg now also matches that catch (widened after the run, plants file only).
Proof (rc 0; head d4036f24b, code-identical to the final head but the budget line): mei sdpa3-proof2-next, sdpa3-proof2-head; keke the same two; kanon sdpa3-proof2-head.
Also: shiki sdpa3-lint0 (inherited head clean), sdpa3-gate1 (667cf6ce2, the code commit: green, 11760 passed).
Superseded: sdpa3-proof-next/-head on mei and keke (killed: the page-list prefix used sed and broke on `/`);
sdpa3-gate2 and sdpa3-plants on a2bc2358e (the budget edit had joined two columns of the contract row: static and
`so` rejected it; fixed in 86b947476).
Run 2's plants: sdpa-reserve-align was WARN-NO-MSG because the STARTUP-DATA-WINDOWS contract caught it at `so` time
(RESERVE_MEM alone on a page), before the test ran; its msg now matches either catch.

## Open
* Next step: none required. Optional: the remaining relro pages (fast 0x85a000, release 0x7b6000) are the `.got`
  reads; placing `.got` next to the cluster would save one more page per profile.
