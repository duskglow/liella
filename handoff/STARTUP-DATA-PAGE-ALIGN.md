# STARTUP-DATA-PAGE-ALIGN: the written start-up data takes the same pages whatever precedes it

State: branch `task/startup-data-page-align`, code head 042e09b37 (+ this handoff), cut from origin/next 3e7723955.
Role kanon:~/w/kn1. Decision: `records/pending/STARTUP-DATA-PAGE-ALIGN-1.toml` (D-NEW, series D-X, amends D-X.62,
D-X.67). Plants: `handoff/STARTUP-DATA-PAGE-ALIGN-plants.txt` (3).

## What changed
* `xtask/src/startup_ro.rs` `relro_plan` (called from `place`, in `finish_archive` after the hot-text grouping):
  every `.data.rel.ro*` section that start-up functions reach (the same `reached` set as `.rodata1`), plus the relro
  sections of `rw` keys, gets renamed to `.data.rel.ro.local.liella_startup`. The first one in the member's
  section table becomes `..._head`, aligned to 4096 (`objcopy --set-section-alignment`). GNU ld's default script
  takes `*(.data.rel.ro.local*)` first, so the group sits at the head of relro and starts on a page boundary, after
  only the program's own `.local` inputs. All of it sits in one member (the joined Rust object, 449 relro inputs).
* `crates/boundary/src/heap.rs`: `RESERVE_MEM` is `#[repr(C, align(4096))]`, so its head takes one page.
* Tests: `init_startup_data_pages_hold_at_every_data_phase` (hot_text_products) links the probe unpadded, with
  0x4d0/0x800/0xc18/0x14d0 bytes of `.data.rel.ro.local` in the start file, and with `.lbss` pads. It checks that
  the cluster starts on a page boundary, has the same page count every time, and that RESERVE_MEM is page-aligned.
  Unit test: `startup_relro_sections_form_one_group_headed_by_the_first` (xtask).

## Proof so far (kanon only, written start-up pages from STARTUP-DATA-WINDOWS)
Page lists come from the scratch `pages.sh` (layout-check from a copy with rw budgets forced to 1); values are x 0x1000.
| tree | profile | pages | list |
|---|---|---|---|
| next 3e7723955 (before) | fast | 11 | 853 855 856 857 858 85a (relro/.got, scattered) 874 875 876 (.data1) 886 887 (RESERVE_MEM at +0xc18) |
| next + bessel-6, from LIBM-F32-BESSEL-LARGE handoff, keke (before) | fast | 13 | 853-858 85a 85b, 878-87a, 8a2 8a3 |
| head + bessel-6 merge (after) | fast | 7 | 850 851 (relro cluster) 869 (.got) 884 885 886 (.data1) 8af (RESERVE_MEM) |
| head (after) | release | 9 | 7b0 7b1, 7c5, 7e0-7e2, 7e7 (RESERVE_MEM), 81b 81c (`global_locale::SLOTS`, at +0x418) |
| head + bessel-6 merge (after) | release | 9 | same count |
Head fast: from the gate job's build (`sdpa-gate` log, `STARTUP-DATA-WINDOWS ok` line), not yet read.
Relro cluster: 64 sections (at most 10807 bytes) release, 83 (at most 11852) fast. The read-only window stays at 1.
Sizes, kanon: release libc.a 11692274 head / 11692282 head+besl; fast libc.a 14077412 next -> 14086484 head+besl
(includes the libm change); libc.so.6 fast 5824984 next -> 5824760 head+besl. The start-up probe (static) went from
5853072 bytes (next, fast) to 5927672 (head+besl, fast); not separated from the libm change, so this needs a
same-tree comparison.

## Not done (next steps)
1. Proof on keke and mei, both profiles, next vs next+bessel-6 vs one other libm branch (brief step 2). Only kanon has
   been measured so far.
2. `global_locale::SLOTS` (release, about 112 KiB in `.lbss`) straddles two pages. Page-aligning it (as with RESERVE_MEM)
   should take release from 9 to 8.
3. Tighten `layout/contracts.tsv` rw budgets (fast 12 -> 7, release 14 -> 9 or 8) once keke and mei agree.
4. Same-tree size comparison (next vs head) for libc.a, libc.so.6 and a static hello; fault rows from the gate.
5. Read-only window: it stays at 1 in every build above. `.rodata1` is already 64 KiB-aligned (D-X.62), so the same
   shift cannot move it while the group stays at or under 64 KiB (place() refuses more).

## Jobs (kanon)
sdpa-base-fast, sdpa-pages-base-fast (next, rc 0); sdpa-proof-besl, sdpa-proof-head (rc 0); sdpa-check1/2 (red:
clippy arithmetic, then the record's newline; both fixed); sdpa-gate (clippy, static, affected --base origin/next)
and sdpa-plants on 042e09b37: still running at hand-back. Scratch: kanon ~/work/STARTUP-DATA-PAGE-ALIGN/,
/mnt/zram/claude/STARTUP-DATA-PAGE-ALIGN/ (trees head/, besl/, out/).
