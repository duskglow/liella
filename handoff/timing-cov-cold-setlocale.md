# TIMING-COV-COLD-SETLOCALE: first `setlocale(LC_ALL, "en_US.UTF-8")`, per-phase evidence

State: branch `task/timing-cov-cold-setlocale` (from origin/next e0c6b146), role kanon:~/w/kn1. Verdict DO NOT LAND:
the code left on the branch (setlocale's composite-name buffer moved out of line, skipped when every category has
one name) measures about -120 user instructions per first call, and its fault changes are code-layout effects.
It is not worth a landing on its own. This run's value is the breakdown below.

## Decisions in force
Ruling #3348 (CtypeFile checks at load), ruling #3347 (static first-view storage only without a second ownership
path; not done, needs the unsafe-category ruling). No new D-row.

## Where the first setlocale goes (static probe, kanon, `perf stat -r 200`, delta P -> PS)
Probe: `cs.c` (P = snprintf+strcmp as in the cold child, S = setlocale, G = bindtextdomain+dgettext),
`meas.sh`, `sym.sh` in `kanon:~/work/TIMING-COV-COLD-SETLOCALE/` (base/, after/, after2/ hold counts, straces, fault
traces).

| side | user instr | kernel instr | faults |
|---|---:|---:|---:|
| Liella base e0c6b146 | 63,036 | 295k | 32 |
| glibc dynamic | 23,001 | 97k | 9 |
| glibc static | 21,229 | 88k | 8 |

Liella's 32-33 faults in the first setlocale (perf record -d, symbolized):
- **heap first use: 14** (first_cache 2, refill 6 metadata pages spread over 4 reserved regions, plus 6 slab pages:
  one per size class the first view uses: archive path name, locale name, `Item` table (2 pages), `Tables`,
  `LocaleView`), plus **7 syscalls** (prlimit, 4 NORESERVE mmaps of 8/8/16/32 MB, 2 munmaps). glibc: brk only.
- archive: 9 (header, name table, LC_CTYPE parse + check 3, record headers 2, LC_COLLATE, view fill 1).
- stack: 3 (frames: setlocale 5.5 KB, replace 3 KB, intern_locked 6.3 KB with the LocaleView value), .bss 4, text 2,
  vdso 1 (vdso lookup from the heap's shard choice: about 6k user instructions with `Vdso::parse`).
- User instructions: LocaleView::new about 25%, name walk + archive scan + compare about 25%, vdso 9%, CtypeFile parse 6%.
So the heap's first use holds about 60% of the kernel gap; the locale code itself cannot close it.

## Tried this run
1. Static NUL-terminated copies of the fixed paths (archive, alias file) in `intern_name_locked`: setlocale -2 mmap
   faults, but the first 32-byte-class allocation then happens in bindtextdomain+gettext (G +1 fault). It moves the
   cost and does not remove it, so it was REVERTED.
2. Composite buffer out of line and skipped for a single name (kept on the branch): -120 user instructions; the
   stack-page saving seen with change 1 did not hold on the final head (same 3 stack faults); text faults moved by
   layout (P went 20 -> 18 for unrelated code).
Suggested NEGATIVE row for docs/saturation/05 (first setlocale): "static path names: cost moves to the next
allocation of that class; out-of-line composite buffer: -120 user instructions, faults by layout only".

## Next step (exact)
A heap story: make the first allocation of a process touch one region. Put the first cache, the shard data and the
first slab records together, reserve with one mmap, and avoid the vdso getcpu lookup on the first allocation.
Measure it with this probe (`_`, `P`, `PS`, `PSG`) and the cold rows. Then ruling #3347 (boundary one-shot arena for
the first view: one allocation instead of five size classes). Timing was not run (no change worth timing).

## Jobs (kanon)
tcs-counts-base, tcs-counts-after (superseded: change 1), tcs-counts-after2 (final head counts + clippy + static),
tcs-gate (direct tier, --base origin/next), tcs-plants (`handoff/timing-cov-cold-setlocale.plants`).
