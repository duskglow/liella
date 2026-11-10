# TIMING-SWEEP-1110 handoff

State: branch task/timing-sweep-1110 (results and handoff only, no code), base next b14f639cc. Results:
results/timing-sweep-1110-table.md (summary, per-row tables with the 1107b/1107/1106e before column, per-family
medians before/after, ten largest per host, rows slower than before, the nine reopen rows).

## What ran (all rc 0)

- kanon:timing-sweep-1110-stage (`cargo xtask bench --stage ~/work/TIMING-SWEEP-1110/sweep1110.tar --only
  cbench_libm,cbench_hot,cbench_locale,cbench_memref`, role kn1; tarball sha256 b514340f03e0493e...).
- Ivy Bridge (kotori, booking #141): kotori:timing-sweep-1110-ivb-libm, -ivb-hot, -ivb-locale, -ivb-memref.
- N100 (nozomi, booking #142): nozomi:timing-sweep-1110-n100-libm, -n100-hot, -n100-locale, -n100-memref.
- Every job: `~/bin/timing <label> sh ~/work/TIMING-SWEEP-1110/run.sh cbench_<g> out-<g>` (run.sh = `cd tree &&
  ./xtask-bin sweep run --out ~/work/TIMING-SWEEP-1110/out-<g> --only cbench_<g>`). All `sweep: COMPLETE`. Ivy Bridge
  memref bench rc 1: limit flags on `__memcmpeq_eq_128`, `__memcmpeq_tail_1k` (as in 1107b).
- Both bookings cancelled after the last job. Nothing left running.
- Scratch: ~/w/.scratch/TIMING-SWEEP-1110 (`analyze.py <new-dir> <repo>` builds the tables; `fetch.sh`; `new/`).

## Decision input for the coordinator

exp on the N100: 1.728x glibc (14.00 / 8.12 ns, spread 0.095), down from 2.169 in 1107b. It is no longer above 2x.

## Ten largest ratios per host (all libm long double / complex / yn)

- Ivy Bridge: asinl 817, ynf 760, yn 557, cpowf 552, cpow 498, atanl 457, lgammal 450, atan2l 448, asinhl 407,
  acosl 395.
- N100: ynf 907, yn 623, cpow 561, cpowf 494, asinl 478, csin 404, csinf 303, atan2l 293, jnf 292, atanl 288.
- Outside libm, Ivy Bridge: iswctype_en_us 1.98, strcasecmp_c_256_pageend 1.82, nl_langinfo_l_en_us 1.50,
  memcmp_diff8_127 / _96 1.42, memrchr_64_hit 1.39, iswalpha_l_en_us 1.29, memcmp_diff8_64 1.25,
  __memcmpeq_head_128 / _1k 1.23.
- Outside libm, N100: strcasecmp_c_256_pageend 1.94, iswctype_en_us 1.82, strcasecmp_c_256_xpage 1.66,
  strcasecmp_c_256 / _en_256 / _utf8_256 1.55, iswalpha_l_en_us 1.46, strcasecmp_c_256_odd 1.37,
  strncasecmp_c_64 1.15, strncmp_eq16 1.10.

## Rows slower than their earlier figure (not chased here)

- Against the 1107 tables: Ivy Bridge exp_qnan 0.854 -> 1.481 (spread 0.455; N100 unchanged 3.13, so likely noise
  or an Ivy Bridge-only effect; exp was touched by LIBM-ENVELOPE-ASM and LIBM-F64-EXP-FAMILY-QUICK-2, not isolated).
  N100 sqrt 1.225 -> 1.341 (spread 0.105; none of the nine landings names sqrt; not identified).
- Against 1106e (no 1107 figure): N100 strcasecmp_{c,en,utf8}_256 0.80 -> 1.55 (spread <= 0.003, instr 1.12): the
  largest real regression in this sweep; the landing was not identified. Also nl_langinfo_l_en_us 1.22 -> 1.50 (Ivy
  Bridge), getenv_miss 0.37 -> 0.46, memchr_64 0.54 -> 0.69 and memcpy_16 0.40 -> 0.51 (N100; all still faster than
  glibc), memchr_64 1.12 -> 1.18 and memcpy_16 0.78 -> 0.89 (Ivy Bridge), strncmp_names64 0.97 -> 1.08 (Ivy Bridge)
  and strncmp_eq16 1.00 -> 1.05 / 1.10.

## Next step

A story for the N100 strcasecmp 256-byte rows (1.55-1.94x, instr 1.12-1.37), bisected with `bench --stage --base
<ref> --rows cbench_locale/strcasecmp_c_256`. The reopen rows other than coshf stay open (log10f 1.64 and y0/y1 1.63
on the N100 are the largest). towctrans still has no bench row.
