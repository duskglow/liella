# STARTUP-DATA-PAGE-ALIGN: the written start-up data takes the same pages whatever precedes it

State: branch `task/startup-data-page-align-2` (run 2, STARTUP-DATA-PAGE-ALIGN-2), stacked on run 1's
`task/startup-data-page-align` 1bf84786a, cut from origin/next 3e7723955. Code head 71762fa7f (+ this handoff).
Role shiki:~/w/sh1. Decision: `records/pending/STARTUP-DATA-PAGE-ALIGN-1.toml` (D-NEW, series D-X, amends D-X.62,
D-X.67; point (3) added in run 2). Plants: `handoff/STARTUP-DATA-PAGE-ALIGN-plants.txt` (4).

## What the branch does
* `xtask/src/startup_ro.rs` `relro_plan` (run 1): the `.data.rel.ro*` sections that start-up functions reach, and the
  relro sections of the `rw` keys, are renamed `.data.rel.ro.local.liella_startup`; the first is `..._head`, aligned to
  4096. GNU ld takes `*(.data.rel.ro.local*)` first, so the cluster sits at the head of relro on a page boundary.
* `crates/boundary/src/heap.rs` (run 1): `RESERVE_MEM` is `#[repr(C, align(4096))]`.
* `csys/src/machine/entry.S` (run 2): the start file's empty 64 KiB relro alignment section (D-X.67) is named
  `.data.rel.ro.local`, so it is the first `.local` input (the start file comes first in the link), ahead of the cluster.
* `layout/contracts.tsv` (run 2): STARTUP-DATA-WINDOWS rw-budget-fast 12 -> 7, rw-budget-release 14 -> 9.
* Tests: `init_startup_data_pages_hold_at_every_data_phase` (hot_text_products; run 2 adds: `.data.rel.ro` holds less
  than a page of `*fill*` in every padded link) and `startup_relro_sections_form_one_group_headed_by_the_first` (xtask).

## Why run 1's gate went red (sdpa-gate, kanon, rc 1) and the fix
`init_first_float_conversion_stays_within_its_page_fault_bound` 4 > 3 and
`init_coreutils_shaped_program_stays_within_its_page_fault_bound` 51 > 50, every run. Reproduced on shiki (fast: base
3/50, run-1 head 4/51; release tool 49 -> 50). perf page-fault addresses: the extra fault is a read of a relro object
by snprintf's float path, on the cluster's first page. Cause: the start file's 64 KiB-aligned `.data.rel.ro` came in
the second ld pattern, BEHIND the renamed cluster, and padded the rest of relro to the next 64 KiB boundary (0xe3f8
bytes of fill; relro 0xae88 -> 0x19288 bytes). Relro then spanned two fault-around windows, and `.got`'s start-up read
no longer mapped the cluster's window. Renaming the start file's section into `.data.rel.ro.local` puts it first:
relro is back to one window, cluster right after the boundary, no gap.

## Proof (shiki; values from STARTUP-DATA-WINDOWS and the relayed fault probe)
| tree | profile | written start-up pages | first %.2f | tool | startup only |
|---|---|---|---|---|---|
| base 3e7723955 | fast | 11 | 3 | 50 | 11 |
| run-1 head 042e09b37 | fast | 7 | 4 | 51 | 11 |
| head 6f9bb9a32 (fix) | fast | 7 | 3 | 50 | 11 |
| head + LIBM-F32-BESSEL-LARGE-6 merged | fast | 7 | 3 | 50 | 11 |
| base 3e7723955 | release | 14 | 3 | 49 | 10 |
| run-1 head | release | 9 | 3 | 50 | 10 |
| head (fix) | release | 9 | 3 | 49 | 10 |
| head + bessel-6 merged | release | 9 | 3 | 49 | 10 |
Run 1 (kanon): fast 7 (head+bessel-6), release 9 / 9. The read-only window stays at 1 everywhere.
Sizes (shiki, base -> fix): fast libc.a 14077412 -> 14071836, libc.so.6 5824984 -> 5829112 (+4128); release libc.a
11696378 -> 11692274, libc.so.6 5293152 -> 5297248 (+4096); static probe (page_faults.c) fast 5853072 -> 5853096,
release 4510680 -> 4510680.

## Not done
1. mei and keke proof: REFUSED in run 2 (the command that shipped the scripts and trees to mei and keke was denied
   by the permission layer). The budgets are at the highest measured over kanon and shiki; CI on mei is the check.
2. `global_locale::SLOTS` (release, ~112 KiB `.lbss`) straddles two pages; page-aligning it should take release 9 -> 8.
3. Sizes on keke/mei not taken (layout should be host-independent but the traced keys come from the build host).

## Jobs (shiki, final head 71762fa7f)
sdpa2-gate (cargo xtask clippy, cargo xtask static, affected --run --base origin/next): running at hand-back.
sdpa2-plants (4 plants, `cargo xtask plants`): running at hand-back; baseline ok, sdpa-relro-align CAUGHT so far.
Diagnosis (not on the final head): sdpa2-lint0 (inherited head clean), sdpa2-base, sdpa2-head, sdpa2-faults1,
sdpa2-fix, sdpa2-besl. Scratch: shiki ~/work/STARTUP-DATA-PAGE-ALIGN-2/ (build.sh, faults.sh, addrs.sh, mapsum.py),
/mnt/zram/claude/STARTUP-DATA-PAGE-ALIGN-2/ (base/, besl/, out/).
