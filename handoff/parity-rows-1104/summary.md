# PARITY-ROWS-1104: next non-libm parity rows by instructions:u

Counter runs on keke (jobs.tsv labels): parity-rows-1104-counts2 (cbench_opt), parity-rows-1104-counts3 (cbench_hot, memref,
locale, patterns, stdio, time, malloc, netdb, fstree); `cargo xtask bench --compare --counters --runs 3`, origin/next
ba27dcfe. Raw output: bench-cbench_opt.out, bench-others.out. (parity-rows-1104-counts, the first submission, never ran:
its queue process died with the ssh session; no result.) Time = Liella/glibc dynamic ns ratio of the same run. Percall
mode (validation/wl_percall.sh) and perf annotate were not run: hypotheses are from per-call instruction counts only.

| Story | Rows | Instr ratio | Time ratio |
|---|---|---|---|
| PERF-STDIO-CHAR-FAST | getc_unlocked, putc_unlocked, getc, putc | 2.82, 2.10, 1.84, 1.47 | 1.13, 1.05, 1.17, 0.95 |
| PERF-STRSTR-HIT-4K | strstr_hit_4k | 1.55 | 1.32 |
| PERF-MEMCMPEQ-EARLY | __memcmpeq/bcmp head/tail rows, memcmp_diff8_64/96/127, memcmp_4k | 1.17 to 1.54 | 0.60 to 1.41 |
| PERF-CTYPE-LANGINFO-ACCESS | nl_langinfo_l, ctype_b_loc, isalpha_c, wcwidth_utf8, nl_langinfo | 1.13 to 1.57 | 0.83 to 1.22 |
| PERF-STRCMP-MIX | strcmp_16_mix, strcmp_64_mix, strcmp_equal_16, strcmp_names | 1.08 to 1.16 | 0.76 to 1.21 |
| PERF-SPRINTF-CHK-NLINK | sprintf_chk_ls_nlink | 1.11 | 1.19 |

## Not briefed (above 1.05x, with reason)
- Libm, wordexp_* (1.08 to 1.48), memcpy/memmove/memset rows (rep movsb; ratios 1.06 to 1.12): excluded by the brief.
- Open stories: gettext_language_steady (1.58), gettext_found (1.33), gettext_language_alternating (1.20), ngettext_found (1.08)
  (same gettext path, assumed covered by the gettext story); strncasecmp_c_64 (1.38), strcasecmp_* (1.08), memcmp_256 (1.23),
  iswalpha/iswctype/towupper rows (1.35 to 2.12), fnmatch_pathname_nomatch (1.20).
- re_mt4_shared / re_mt4_distinct (1.35, 1.36): time ratio 0.03 and 0.20, Liella far faster; extra instructions are
  threaded-wait cost. Not worth a story.
- Below the top 6 by ratio times time, left for the next round: realloc_within_usable 1.11 (t 1.20), nftw_phys_1000 1.12,
  fts_physical_1000 1.08, fwrite_4k 1.14, getline_long 1.11, regexec_backref 1.11, strncmp_* 1.06 to 1.09 (t 1.32 to 1.35),
  memset_4k 1.12, strnlen_16 1.08 (strnlen body/mid is an open story), sys_close_ebadf 1.06, glob_1000_sorted 1.06,
  fnmatch_bracket 1.06.
