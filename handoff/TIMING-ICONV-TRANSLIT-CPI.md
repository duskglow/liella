# TIMING-ICONV-TRANSLIT-CPI: iconv UTF-8 -> ASCII//TRANSLIT, more cycles than instructions

State: branch `task/timing-iconv-translit-cpi`, code head `f221e5a2` (memo + test + D-NEW row), then the plants file and
this note. Role shiki:~/w/sh2 (Haswell-EP build host).

## Where the time went (profile from the brief, Ivy Bridge, booked)

Liella cycles: `Conv::convert` 53% (with `encode_char`, `derived_translit`, `combining_class`, `props` inlined),
`unicode::decomposition` 30%, `internal_move` 10%, `Codec::encode` 6%. The bench text is `"abcdefg é h €."` x 256, so
one call transliterates 512 characters, only two distinct ones. Each derivation is a binary search over DECOMP (about
13 dependent loads over a table larger than L1, branchless, hence the low branch-miss count), a partition_point over
CCC and a property lookup per part, then a linear search of ASCII_EQUIVALENTS: latency-bound chains, which is where
the low IPC comes from. The front-end/flags-merge/store-forward events in the brief are symptoms inside that inlined
derivation; removing the repeated derivation removes them with it, so they were not chased separately
(perf annotate on the timing host was not needed).

## Change (D-NEW, number to be reserved)

`Conv::convert` keeps each transliteration it works out in a per-call table (`iconv::Memo`: 8 slots, direct-mapped on
`c % 8`, results up to 11 bytes, only lossy results), made lazily on the first character the target cannot encode
(`Option<Memo>`: pure-ASCII and non-translit calls only store the discriminant). A repeat takes its bytes from the
slot. The lookup is out of line (`Conv::encode_memo`, `#[inline(never)]`), so the conversion loop stays small; the two
small copies on that path use `mem::copy_inline` instead of `copy_from_slice` (memcpy calls). Nothing is kept between
calls or in `iconv_t` (no ABI or layout change); within one call the descriptor and locale are fixed, so a kept result
equals a new derivation; E2BIG, //IGNORE and the irreversible count behave as before.

## Results (final code head)

- Instructions per call (shiki, `--counters --runs 3`, Liella; glibc in brackets):
  translit 395,931 -> 190,502 (glibc 458,239; ratio 0.86 -> 0.42);
  utf16 74,623 -> 74,580 (124,633); 8859-15 51,599 -> 51,551 (160,775). No row rose.
- Time on shiki (Haswell-EP, NOT booked, advisory): translit Liella/glibc 1.21 -> 0.40. utf16 time 0.63 -> 0.70 there
  with fewer instructions and a byte-identical fused path: unbooked-host noise, judge it on the timing hosts.
- Timing on the timing hosts (bookings 38, 39 were still draining; hosts idle, load ~0.0-1.0; binaries of f221e5a2):
  N100 (nozomi, `~/logs/timing-iconv-translit-cpi-time4-nozomi.log`, `--runs 5`): translit Liella/glibc median
  0.522 (sweep: 1.453), utf16 0.765, 8859-15 0.275. Ivy Bridge (nico): NOT VERIFIED; the run went past my 500 s
  timeout before the iconv rows (it ran the whole cbench_locale group), log `...-time4-nico.log`.
  Bookings cancelled. Staging lesson: the stage needs the root Cargo.toml, crates/ and csys/ besides target/liella,
  bench/, xtask/Cargo.toml and xtask-bin (else "not a benchmark" / "bench/budgets.tsv: No such file"); use
  `--only <group>` not positional with --compare; ~/bin/timing writes no log, so redirect it to ~/logs yourself.
  Next timing step: rerun on Ivy Bridge with a longer timeout (one cbench_locale group with counters: over 8 min).
- Gate `timing-iconv-translit-cpi-gate` (static + affected --base origin/next): 5 suites ok, 1584 passed, 0 failed,
  peak 2.6G. Plants `timing-iconv-translit-cpi-plants2`: 5 CAUGHT, 1 EQUIVALENT (unused-slot check: only key 0 could
  match, U+0000 is never transliterated), 0 WARN.
- Superseded: `timing-iconv-translit-cpi-plants` (rc 3: plants file not yet synced), `-check` (wrong package name).

## Test

`iconv_translit_memo_repeats_and_collisions` (s06_multibyte_clauses; trace s6_4_c_1_1): repeats, slot collisions
(U+00E9/U+00F1, U+20AC/U+00C4), E2BIG on a kept result, irreversible count, C-locale '?' with //IGNORE.

## Open / next

- RULING: none. D-NEW needs its number (`ops reserve`).
- Next: a saturation-register row for this row (positive: memo; not tried: a direct-mapped two-stage decomposition
  table in the generator, which would also help the first occurrence and other decomposition users).
- Do not redo: flags-merge / store-forward hunting in the old inlined body; the body it pointed at no longer runs per
  repeated character.
