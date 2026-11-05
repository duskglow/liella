# PARITY-ROWS-1105: next non-libm parity rows by instructions:u, with a profile per row

Counter runs on keke (jobs.tsv labels): parity-rows-1105-counts (cbench_opt; its second command named the groups without
the `cbench_` prefix and failed, so it is superseded for the other groups) and parity-rows-1105-counts2 (cbench_hot,
cbench_memref, cbench_locale, cbench_patterns, cbench_stdio, cbench_time, cbench_malloc, cbench_netdb, cbench_fstree);
`cargo xtask bench --compare --counters --runs 3`, origin/next d10a6adc. Raw: bench-cbench_opt.out, bench-others.out.
Per-call profiles: parity-rows-1105-percall (validation/percall/parity1105.c plus fwrite4k.c) and -percall2 (the
parity1105.c rerun with memset behind a pointer and fnmatch/glob/nftw in the C locale, as the bench runs them);
validation/wl_percall.sh, instructions:u per call. perf annotate was not run (sampling on the VM loses samples).
Time = Liella/glibc dynamic ns ratio of the same run. None of the 1104 stories has landed on next yet (their rows are
all still above 1.05x), so they stay with those stories.

| Story | Rows | Instr ratio | Time ratio | Profile (percall new/glibc) |
|---|---|---|---|---|
| PERF-REALLOC-USABLE | realloc_within_usable | 1.11 | 1.33 | 203 / 188; malloc_free control 112 / 142 |
| PERF-FWRITE-4K-2 | fwrite_4k | 1.14 | 1.15 | 815 / 644; memcpy control 440 / 425 |
| PERF-NFTW-ENTRY | nftw_phys_1000, fts_physical_1000 | 1.10, 1.08 | 1.05, 1.00 | nftw 35333 / 9565 (about 640 more per entry) |
| PERF-FNMATCH-BRACKET | fnmatch_bracket | 1.07 | 1.09 | 999 / 891 (C locale) |
| PERF-MEMSET-4K | memset_4k | 1.13 | 1.03 | 285 / 250 |
| PERF-GLOB-SORTED | glob_1000_sorted | 1.07 | 0.87 | 85735 / 68437 |

## Not briefed
- regexec_backref: 1.04 now (was 1.11), below the 1.05 line.
- sys_close_ebadf 1.06 (time 1.04): 25 instructions of a syscall wrapper that mostly measures the kernel; left, low value.
- Open stories (excluded by the brief): strncmp_* (1.06 to 1.09), getc/putc (PERF-STDIO-CHAR-FAST), strstr_hit_4k / strstr_hit_64
  (PERF-STRSTR-HIT-4K-3), sprintf_chk_ls_nlink, gettext_* / ngettext_found, strncasecmp/strcasecmp_*, memcmp/bcmp/__memcmpeq rows,
  wctype rows (iswctype, iswalpha, towupper), nl_langinfo/ctype_b_loc/isalpha_c/wcwidth (1104 stories), strcmp_*mix (PERF-STRCMP-MIX), fnmatch_pathname*.
- strnlen_16 1.08, strnlen_16_mix 1.06: the open strnlen story. memset_128 1.08 (time 0.76) and memcpy_144/256 1.06: rep-movsb/size-band rows,
  excluded; Liella is faster in time.
- Libm and wordexp_*: excluded. re_mt4_shared/distinct (1.37, 1.36): Liella far faster in time (0.04, 0.23), threaded wait cost.

## Notes
- fnmatch_bracket under en_US.UTF-8 is 0.81x (Liella faster); the gap is only in the single-byte locale path the bench uses.
- nftw/fts: the same per-entry excess (about 600 instructions) appears in two shapes (percall 38 files, bench 1000 files), so the
  shared per-entry path is the suspect. PERF-GLOB-SORTED may overlap it.
- validation/percall/parity1105.c is a new probe spec added with these briefs (no product code changed).
