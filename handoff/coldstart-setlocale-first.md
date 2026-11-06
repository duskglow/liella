# COLDSTART-SETLOCALE-FIRST: first `setlocale(LC_ALL, "en_US.UTF-8")`

State: branch `task/coldstart-setlocale-first`, cut from origin/next e8189dd8 (contains 730f416b). **No code change**:
measurement only, this file and `handoff/reports/COLDSTART-SETLOCALE-FIRST.json`. Result: INCOMPLETE (the budget ran out
before a change could be proven; see "Next step").

## Step 0 (re-time on the N100)

The planning job nozomi:CSP1106-n100 is DONE (`~/work/COLDSTART-PLAN-1106/` on nozomi, staged 730f416b). Its bench has only
Liella's `first_setlocale_en_US` (122437 ns/op; first_* rows get no glibc column in a plain staged bench), so the ratio comes
from its per-step probe (`counts-n100.txt`, medians of 50, deltas of "S" over "_" the empty run):

| N100 | user instr | kernel instr | faults | time |
|---|---:|---:|---:|---:|
| Liella S - _ | +63.5k | +353k | +38 | +0.15 ms |
| glibc S - _ | +22.7k | +124k | +12 | +0.07 ms |

That's still about 2.1x, well above 1.05x, so the row is still open. I ran no extra timing job.

## Measured on keke (role n4, build of e8189dd8, my own probe; medians of 11)

| keke | iu | ik | pf |
|---|---:|---:|---:|
| Liella `_` / `S` | 3058 / 65591 | 206451 / 600229 | 15 / 57 |
| glibc `_` / `S` | 118101 / 140761 | 629507 / 758243 | 52 / 65 |

Deltas: Liella +62.5k user, **+394k kernel**, +42 faults; glibc +22.7k, +129k, +13. Kernel work (faults and syscalls) is
most of the gap.

**Syscalls in Liella's S** (strace): the archive (openat, fstat, mmap 3 MB, close), the same work glibc does. The
**first heap use** adds more: mmap 8 MiB, 64 KiB, 8 MiB, 16 MiB, 32 MiB, munmap twice (alignment trim), plus
`openat/read/close /proc/sys/vm/mmap_min_addr` and `prlimit64(RLIMIT_AS)`. I did not check against the `_` strace that those
last two belong to heap init.

**Faults by address region** (`perf record -d -e page-faults -c 1`; IPs resolved with addr2line on the static probe):
- Heap, about 14: `Heap::refill` x6 and `Heap::first_cache` x2 (heap metadata, including one page per size-class slab at a
  256 KiB stride), `TryVec<Item>::resize` x2 (the item tables of all 12 categories, `LocaleView::new`), `internal_move` x2 and
  copy x1 (moving the boxed `LocaleView`), `intern_locked` x1. The heap is used by `intern_name_locked` (the archive path,
  the name, the composite name), `LocaleView::new` (one `TryVec<Item>` with every category's items, plus the `Tables` box)
  and the `TryBox<LocaleView>`.
- Archive pages, 7 (glibc's whole S delta is 13 faults): `find_named` x2 and `name::candidates` x1 (hash and name tables),
  `Category::word` x1 and a `try_fold` x2 inside `CtypeFile::parse` (the class/map/width `Table3` header checks spread over
  `LC_CTYPE`), `Collate::parse` x1, `LocaleView::new` x1 (fill_table).
- The rest are text/.bss/stack faults shared with `_`.

The user-instruction profile couldn't be split finely: perf throttles to about 30 samples on this short run, and keke has no
valgrind. The 2000-period profile matches the brief's (LocaleView::new 19%, copy_avx2 13%, find_named 10%,
CtypeFile::parse 6%, Archive::lookup / Locale::with / TimeNames::of about 3% each).

## Findings for the hypothesis (sized by faults; each fault costs roughly 5-9k kernel instructions here)

1. Heap: **the largest part of the row** (about 14 of 42 faults plus 7 to 9 syscalls). Caveat for real workloads: a program
   that later calls malloc pays the heap init anyway, so static storage only moves that cost. The real saving is the size-class
   slab pages the locale's allocations alone touch (about 5 faults), plus the zero-fill-then-fill of the item tables. The row
   itself would lose the whole heap init. Whether the row or the workload is the target needs the coordinator's view (see the
   recommendation below).
2. Eager ctype derivation: about 3 archive faults (the `Table3` header checks in `CtypeFile::parse`) plus its user
   instructions. `CtypeFile::parse` also runs **twice** per selection: once in `codeset_matches` (mod.rs:561, which needs only
   validity and the codeset) and again when `Locale::with` builds the ctype. `ascii_case_runs` runs in both.
   Semantics-preserving first cut: give `codeset_matches` a validate-only entry with no `ascii_case` (`ascii_case` doesn't
   affect validity). Deferring the `Table3` checks to first use would change when a corrupt `LC_CTYPE` is rejected (at
   setlocale vs at first class call), and that needs a ruling (spec Appendix D.1).
3. Lookup and names: find_named and candidates touch 3 archive pages; glibc's whole delta is 13 faults. Not measured
   separately yet.

## Next step (exact)

1. Implement 2a (validate-only `codeset_matches`), then 1: the first global view's item tables, `Tables`, `LocaleView` and
   interned names in fixed `.bss` storage (one slot; later views keep using the heap). Count with the probe below (`S` and
   `ST` = S then the case functions) before and after, and keep only what lowers faults and kernel instructions with no
   measured row slower.
2. Tests: toupper/tolower/isalpha/towupper give the archive's answers when first used after setlocale (§7, D.1), and a second
   setlocale updates the name. Plants as the brief lists them, run through `cargo xtask plants`.
3. Gate (direct tier, `--base origin/next`), then an N100 staged bench of cbench_opt (`first_setlocale_en_US`).

Probe (keke `~/work/COLDSTART-SETLOCALE-FIRST/cs.c`, `meas.sh`; link: `gcc -O2 -static -nostdlib -I target/liella/include
target/liella/liella-crt1.o cs.c target/liella/libc.a -lgcc`): argv[1] contains `S`: `setlocale(LC_ALL, "en_US.UTF-8")`;
contains `T`: `toupper('a') + tolower('Q') + isalpha(0xe9) + towupper(0xe9) + towlower('I')`. Note: variant `T` alone ran in
"C" (it lacks the S); use `ST`.

## Things not to redo
- Step 0 is settled at 730f416b (the row is still about 2.1x on the N100 by the probe deltas).
- In jobrun scripts on keke, `cargo` is not on PATH: export `$HOME/.cargo/bin` (my first job, csf-counts-before, failed on that).
- perf sampling can't profile this probe finely (throttled). Use faults with `-d` and addr2line, or a loop harness.

## Jobs
keke:csf-counts-before (rc 1, cargo not on PATH), keke:csf-counts-before2 (rc 0, counts and profiles above). No gate, no plants
and no timing (no code change). Nothing left running.

## RULING NEEDED
- Row vs workload for part 1. Recommendation: do it. The row loses the whole heap init, and real programs still save the
  locale-only slab pages.
- Deferring the `CtypeFile` table checks to first use (part 2b). Recommendation: no. Keep load-time validation and do only
  the validate-only `codeset_matches` split (2a).
