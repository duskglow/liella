# TIMING-SWEEP-1107B handoff

State: branch task/timing-sweep-1107b (results and handoff only, no code), base next 9643c81b0. Results:
results/timing-sweep-1107b-table.md (summary, per-row tables, per-family medians, morning before column for libm).

## What ran (all rc 0 unless noted)

- keke:tsw1107b-stage (`cargo xtask bench --stage ... --only cbench_libm,cbench_locale,cbench_memref`, role n3),
  keke:tsw1107b-stage-hot (`--only cbench_hot`).
- Ivy Bridge (kotori, booking #134): kotori:tsw1107b-ivb-libm, kotori:tsw1107b-ivb-libm-2 (repeat, same products),
  kotori:tsw1107b-ivb-locale, kotori:tsw1107b-ivb-memref (sweep COMPLETE, bench rc 1: limit check flagged
  `__memcmpeq_eq_128`, `__memcmpeq_tail_1k`; the jobs.tsv rc is 0).
- N100 (nozomi, booking #135): nozomi:tsw1107b-n100-libm, nozomi:tsw1107b-n100-locale, nozomi:tsw1107b-n100-memref.
- PENDING at hand-back (17:53 UTC, running about 20 min): kotori:tsw1107b-ivb-hot and nozomi:tsw1107b-n100-hot
  (`sweep run --only cbench_hot`, out `~/work/TIMING-SWEEP-1107B/out-hot`). They hold the strncmp rows.
  Re-attach: `ssh <host> '~/bin/jobwait tsw1107b-ivb-hot'` (and `tsw1107b-n100-hot`). If either still runs at 19:50
  UTC: `~/bin/kill-label <label> --kill`. Then `ops timing-cancel 134` and `ops timing-cancel 135`.
- Scratch (tables, script): /home/claude/w/.scratch/TIMING-SWEEP-1107B (`analyze.py <dir>` makes the per-row tables
  from rounds.tsv; put `out-hot` into `new/<ivb|n100>/hot` and rerun it to add the strncmp rows).

## Findings (details in the results file)

- sinl/cosl now faster than glibc (112.7 -> 0.58 Ivy Bridge, 88.2 -> 0.70 N100); log 5.2-5.9 -> 1.93; exp improved
  (N100 3.35 -> 2.17).
- expf and exp2f about 19-21% slower than the morning on N100 (Liella 13.30 -> 15.91 ns, glibc unchanged, spread
  <= 0.013); same direction on Ivy Bridge. Not isolated to one landing.
- Ivy Bridge exp-family rows (and `add_baseline`) have round spreads of 0.37-0.68 in two runs, and glibc's own ns
  on those rows moved; low-confidence verdicts there (see also handoff/LIBM-F64-EXP-IVB-REGRESS.md).
- Wide classification median 1.02 / 1.08; iswctype_en_us 1.99 / 1.82 slowest. strstr_hit_64 1.25 / 1.18.
  Ivy Bridge compare: 128-byte and head_1k bcmp/__memcmpeq rows 1.10-1.23, memcmp_diff8_* 1.25-1.42.
- Skipped: sincos and towctrans (no bench row on next).

## Next step

Collect the two hot jobs, add the strncmp rows to the results file, cancel bookings 134 and 135. Then a story to
bisect the expf/exp2f N100 change (`bench --stage --base <ref> --rows cbench_libm/expf,cbench_libm/exp2f`).
