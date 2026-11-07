# Timing sweep Zen 3 (1107): every non-libm and libm row on one Zen 3 class

Build: origin/next 66b8d3ba. Staged once on a build host with `cargo xtask bench --stage <tar> --only <all 18
cbench_* groups>` (52 products matched bench-stage.manifest; tarball sha256 08381188503cc987...), unpacked whole on
the timing host and run unchanged with the sweep tool (docs/benchmarks.md section 6, "The periodic timing sweep"):
`./xtask-bin sweep run --out <dir>` (the 17 non-libm groups, its default) and then
`./xtask-bin sweep run --out <dir> --only cbench_libm`. Each runs `bench --no-build --staged --counters --only
<groups>`: Liella and glibc on the same host, rounds interleaved. Both runs exited 0 (`sweep: COMPLETE`).

CPU class: Zen 3 (AMD Ryzen 5 5500 class: 6 cores / 12 threads, AVX2, no AVX-512), host glibc 2.39 (the same
distribution build as the other timing hosts). This is the first sweep on this class: there is no earlier Zen 3
table, so `sweep compare` (which compares one class across two sweeps) does not apply, and this file gives absolute
ratios only.

Measurement conditions (details in handoff/timing-sweep-natsumi.md):
- governor performance on all 12 CPUs, scaling max frequency 3.6 GHz (the base clock; the CPU's boost has no
  switch under its frequency driver, and the cap keeps it from boosting); swap device present but 0 bytes used;
  the periodic system timers were not masked. The run was booked and held every slot of the host's timing queue.
- The host's timing-intruder log has one entry for these runs: a firmware-update service at 2 s, about 2 minutes
  into the non-libm run.
- No `ratio_host` limits exist for this CPU model in bench/host_limits.tsv: rows are measured and reported, not
  gated (0 rows flagged).
- Run-to-run spread median 0.008 on the non-libm rows; Ivy Bridge 0.009 and N100 0.012 in 1106E.

Columns: `instr` / `cycles` = Liella/glibc instructions / cycles per call (counters of this run; `-` = not counted);
`time` = median Liella/glibc time ratio over the interleaved rounds; `spread` = (max - min) / median of the round
ratios; verdict faster (< 0.95), parity, slower (> 1.05), as the sweep tool judges them. The `wordexp` rows are timed,
not judged (owner deferral, as in the earlier sweeps). The sweep tool requires the selected `ratio_host` rows (309
non-libm, 166 libm) to be measured; the tables hold every row the bench printed. Analyzer:
handoff/timing-sweep-natsumi/analyze.py.

The orientation block at the end of the summaries copies the 1106E counts for Ivy Bridge and N100 from
results/timing-sweep-1106e-table.md: different CPU classes, a different commit (next 34c98f61) and a different row
set (407 rows). They are context, not a comparison. There is no earlier libm table for any class on next to set
beside the libm counts.

## Zen 3, non-libm rows

| rows judged | faster | parity | slower | time median | time p10 | time p90 | time geomean | spread median | spread p90 | spread max | instr median (counted rows) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 376 | 274 | 64 | 38 | 0.752 | 0.270 | 1.050 | 0.627 | 0.008 | 0.059 | 0.568 | 0.866 (369) |

Rows with time printed 0.000 (< 0.0005; faster, left out of the geomean): cbench_patterns/regexec_pathological_4k
Rows flagged by the bench's per-host limit check: 0 (no limits exist for this CPU model; see the handoff).
Deferred rows (timed, not judged): cbench_opt/wordexp_arith 1.437, cbench_opt/wordexp_param_forms 1.320, cbench_opt/wordexp_plain_8 0.640, cbench_opt/wordexp_quoted 1.891, cbench_opt/wordexp_split_var 0.942, cbench_opt/wordexp_tilde_var 1.553, cbench_patterns/wordexp_arith 1.588, cbench_patterns/wordexp_tilde_var 1.623
Judged rows whose verdict lies inside their own spread (spread > 0.10 and |time - 1| < spread): 1: cbench_memref/memcpy_4k_off

### Zen 3, non-libm rows: slowest 20 rows

| bench/row | time | instr | cycles | spread | rounds |
|---|---|---|---|---|---|
| cbench_hot/getline_80 | 1.750 | 1.019 | 1.742 | 0.021 | 5 |
| cbench_opt/strcmp_late_256 | 1.650 | 0.885 | 1.648 | 0.000 | 3 |
| cbench_memref/strcmp_64_mix | 1.507 | 1.164 | 1.489 | 0.008 | 5 |
| cbench_opt/gettext_language_steady | 1.496 | 1.400 | 1.494 | 0.001 | 3 |
| cbench_memref/memset_144 | 1.453 | 1.085 | 1.415 | 0.000 | 5 |
| cbench_memref/memset_256 | 1.453 | 1.085 | 1.416 | 0.000 | 5 |
| cbench_locale/iswctype_en_us | 1.427 | 1.767 | 1.447 | 0.010 | 5 |
| cbench_unmeasured/qsort_c_cmp_1024_e4 | 1.401 | 1.309 | 1.396 | 0.008 | 3 |
| cbench_unmeasured/qsort_c_cmp_1024 | 1.381 | 1.313 | 1.365 | 0.004 | 3 |
| cbench_unmeasured/qsort_c_cmp_1024_e8 | 1.372 | 1.283 | 1.364 | 0.015 | 3 |
| cbench_hot/memrchr_64 | 1.324 | 0.846 | 1.320 | 0.003 | 5 |
| cbench_locale/strcasecmp_c_16 | 1.298 | 1.060 | 1.300 | 0.004 | 5 |
| cbench_memref/strcmp_16_mix | 1.279 | 1.129 | 1.280 | 0.008 | 5 |
| cbench_memref/strlen_64_mix | 1.275 | 0.951 | 1.265 | 0.005 | 5 |
| cbench_memref/strlen_64 | 1.272 | 0.941 | 1.263 | 0.000 | 5 |
| cbench_memref/strnlen_4k | 1.268 | 0.946 | 1.285 | 0.025 | 5 |
| cbench_memref/rawmemchr_64_mix | 1.266 | 0.951 | 1.256 | 0.004 | 5 |
| cbench_locale/iswalpha_l_en_us | 1.260 | 1.489 | 1.282 | 0.020 | 5 |
| cbench_locale/iswalpha_utf8 | 1.256 | 1.347 | 1.247 | 0.012 | 5 |
| cbench_unmeasured/qsort_c_cmp_1024_e64 | 1.252 | 1.182 | 1.252 | 0.007 | 3 |

## Zen 3, libm rows

| rows judged | faster | parity | slower | time median | time p10 | time p90 | time geomean | spread median | spread p90 | spread max | instr median (counted rows) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 169 | 5 | 8 | 156 | 6.189 | 1.569 | 201.248 | 9.756 | 0.006 | 0.073 | 0.355 | 5.207 (169) |

Rows flagged by the bench's per-host limit check: 0 (no limits exist for this CPU model; see the handoff).
Judged rows whose verdict lies inside their own spread (spread > 0.10 and |time - 1| < spread): 0

### Zen 3, libm rows: slowest 20 rows

| bench/row | time | instr | cycles | spread | rounds |
|---|---|---|---|---|---|
| cbench_libm/ynf | 725.192 | 681.296 | 722.602 | 0.005 | 3 |
| cbench_libm/cexp | 714.404 | 475.100 | 715.003 | 0.331 | 3 |
| cbench_libm/cpow | 578.476 | 518.036 | 578.291 | 0.202 | 3 |
| cbench_libm/yn | 527.059 | 484.491 | 526.987 | 0.223 | 3 |
| cbench_libm/cpowf | 437.637 | 776.752 | 436.346 | 0.003 | 3 |
| cbench_libm/asinl | 437.520 | 1680.626 | 437.055 | 0.001 | 3 |
| cbench_libm/cexpf | 376.413 | 706.187 | 376.649 | 0.007 | 3 |
| cbench_libm/csin | 344.763 | 325.701 | 345.005 | 0.222 | 3 |
| cbench_libm/atanl | 293.399 | 7405.102 | 292.526 | 0.055 | 3 |
| cbench_libm/atan2l | 290.139 | 3697.964 | 289.656 | 0.008 | 3 |
| cbench_libm/acosl | 274.910 | 3933.857 | 274.951 | 0.005 | 3 |
| cbench_libm/csinf | 241.355 | 399.862 | 241.515 | 0.002 | 3 |
| cbench_libm/clog | 215.938 | 178.032 | 216.407 | 0.022 | 3 |
| cbench_libm/erfl | 212.440 | 922.314 | 211.955 | 0.002 | 3 |
| cbench_libm/erfcl | 209.996 | 911.172 | 209.839 | 0.009 | 3 |
| cbench_libm/asinhl | 204.722 | 1175.925 | 205.227 | 0.005 | 3 |
| cbench_libm/jnf | 201.248 | 174.995 | 201.365 | 0.012 | 3 |
| cbench_libm/csqrt | 195.019 | 281.634 | 195.035 | 0.008 | 3 |
| cbench_libm/clogf | 191.509 | 269.818 | 191.651 | 0.001 | 3 |
| cbench_libm/lgammal | 171.672 | 973.384 | 170.976 | 0.022 | 3 |

## Orientation only: other CPU classes, other commit (results/timing-sweep-1106e-table.md)

| class (non-libm rows) | faster | parity | slower | spread median |
|---|---|---|---|---|
| Ivy Bridge 1106E (407 judged rows) | 252 | 99 | 56 | 0.009 |
| N100 1106E (407 judged rows) | 313 | 74 | 20 | 0.012 |

### Zen 3, non-libm rows: all rows

| bench/row | instr | cycles | time | spread | rounds | verdict |
|---|---|---|---|---|---|---|
| cbench_fstree/fdopendir_readdir_closedir_99 | 1.001 | 0.999 | 0.993 | 0.058 | 5 | parity |
| cbench_fstree/fts_nochdir_flat_1000 | 0.827 | 0.841 | 0.848 | 0.018 | 5 | faster |
| cbench_fstree/fts_physical_1000 | 1.064 | 1.027 | 1.009 | 0.021 | 5 | parity |
| cbench_fstree/getcwd | 1.023 | 1.000 | 1.064 | 0.073 | 5 | slower |
| cbench_fstree/getmntent_50_lines | 1.046 | 0.950 | 0.954 | 0.061 | 5 | parity |
| cbench_fstree/mkstemp_unlink_close | 1.029 | 1.027 | 1.021 | 0.005 | 5 | parity |
| cbench_fstree/nftw_phys_1000 | 1.031 | 1.006 | 1.002 | 0.013 | 5 | parity |
| cbench_fstree/opendir_readdir_closedir_1000 | 1.005 | 1.006 | 0.989 | 0.018 | 5 | parity |
| cbench_fstree/readdir_entry | 1.004 | 0.999 | 0.990 | 0.042 | 5 | parity |
| cbench_fstree/realpath_5 | 1.044 | 1.003 | 1.009 | 0.007 | 5 | parity |
| cbench_fstree/scandir_alphasort_1000 | 0.978 | 0.927 | 0.921 | 0.028 | 5 | faster |
| cbench_hot/ctype_b_loc | 1.199 | 0.556 | 0.557 | 0.000 | 5 | faster |
| cbench_hot/ctype_get_mb_cur_max | 0.909 | 0.625 | 0.627 | 0.000 | 5 | faster |
| cbench_hot/errno_location | 0.900 | 0.750 | 0.751 | 0.000 | 5 | faster |
| cbench_hot/fwrite_unlocked_16 | 0.729 | 0.615 | 0.615 | 0.012 | 5 | faster |
| cbench_hot/fwrite_unlocked_64 | 0.758 | 0.774 | 0.773 | 0.020 | 5 | faster |
| cbench_hot/getc | 0.815 | 0.613 | 0.614 | 0.000 | 5 | faster |
| cbench_hot/getc_unlocked | 0.999 | 0.572 | 0.573 | 0.000 | 5 | faster |
| cbench_hot/getenv_hit_last | 0.805 | 0.595 | 0.593 | 0.001 | 5 | faster |
| cbench_hot/getenv_hit_last_2000 | 0.921 | 0.955 | 0.948 | 0.022 | 5 | faster |
| cbench_hot/getenv_hit_last_300 | 0.944 | 0.556 | 0.539 | 0.149 | 5 | faster |
| cbench_hot/getenv_hit_mid | 0.844 | 0.631 | 0.625 | 0.004 | 5 | faster |
| cbench_hot/getenv_hit_mid_2000 | 0.923 | 0.942 | 0.942 | 0.005 | 5 | faster |
| cbench_hot/getenv_hit_mid_300 | 0.958 | 0.507 | 0.507 | 0.005 | 5 | faster |
| cbench_hot/getenv_miss | 0.460 | 0.339 | 0.334 | 0.003 | 5 | faster |
| cbench_hot/getenv_miss_2000 | 0.915 | 0.964 | 0.951 | 0.021 | 5 | parity |
| cbench_hot/getenv_miss_300 | 0.904 | 0.557 | 0.555 | 0.013 | 5 | faster |
| cbench_hot/getline_80 | 1.019 | 1.742 | 1.750 | 0.021 | 5 | slower |
| cbench_hot/getline_long | 1.140 | 0.954 | 0.983 | 0.048 | 5 | parity |
| cbench_hot/memcpy_chk_64 | 0.931 | 1.000 | 1.000 | 0.000 | 5 | parity |
| cbench_hot/mempcpy_16 | 0.954 | 0.500 | 0.500 | 0.000 | 5 | faster |
| cbench_hot/mempcpy_64 | 1.000 | 0.890 | 0.889 | 0.000 | 5 | faster |
| cbench_hot/memrchr_4k | 0.897 | 1.091 | 1.069 | 0.035 | 5 | slower |
| cbench_hot/memrchr_64 | 0.846 | 1.320 | 1.324 | 0.003 | 5 | slower |
| cbench_hot/memrchr_64_hit | 1.000 | 1.125 | 1.124 | 0.000 | 5 | slower |
| cbench_hot/mutex_lock_unlock_normal | 0.847 | 0.650 | 0.650 | 0.003 | 5 | faster |
| cbench_hot/mutex_lock_unlock_normal_mt | 0.824 | 0.730 | 0.721 | 0.009 | 5 | faster |
| cbench_hot/mutex_lock_unlock_recursive | 0.878 | 0.600 | 0.600 | 0.000 | 5 | faster |
| cbench_hot/mutex_lock_unlock_recursive_mt | 0.849 | 0.655 | 0.655 | 0.001 | 5 | faster |
| cbench_hot/putc_unlocked | 0.897 | 0.596 | 0.596 | 0.000 | 5 | faster |
| cbench_hot/putenv_2000 | 0.131 | 0.174 | 0.183 | 0.130 | 5 | faster |
| cbench_hot/putenv_30 | 0.824 | 0.535 | 0.567 | 0.076 | 5 | faster |
| cbench_hot/putenv_remove_cycle_2000 | 0.143 | 0.196 | 0.199 | 0.076 | 5 | faster |
| cbench_hot/putenv_remove_cycle_30 | 1.000 | 0.706 | 0.729 | 0.075 | 5 | faster |
| cbench_hot/setenv_2000 | 0.146 | 0.189 | 0.188 | 0.015 | 5 | faster |
| cbench_hot/setenv_30 | 0.851 | 0.494 | 0.490 | 0.008 | 5 | faster |
| cbench_hot/setenv_300 | 0.190 | 0.128 | 0.135 | 0.165 | 5 | faster |
| cbench_hot/setenv_getenv_2000 | 0.201 | 0.255 | 0.255 | 0.021 | 5 | faster |
| cbench_hot/setenv_getenv_300 | 0.246 | 0.172 | 0.172 | 0.071 | 5 | faster |
| cbench_hot/strchr_16_hit | 0.965 | 0.889 | 0.889 | 0.000 | 5 | faster |
| cbench_hot/strchr_16_miss | 0.965 | 0.727 | 0.726 | 0.000 | 5 | faster |
| cbench_hot/strchr_64_hit | 0.921 | 0.916 | 0.917 | 0.000 | 5 | faster |
| cbench_hot/strchr_64_miss | 0.921 | 0.917 | 0.917 | 0.000 | 5 | faster |
| cbench_hot/strchrnul_16_hit | 0.926 | 0.778 | 0.779 | 0.000 | 5 | faster |
| cbench_hot/strchrnul_16_miss | 0.926 | 0.778 | 0.779 | 0.000 | 5 | faster |
| cbench_hot/strchrnul_64_hit | 0.889 | 0.872 | 0.873 | 0.000 | 5 | faster |
| cbench_hot/strchrnul_64_miss | 0.889 | 0.806 | 0.806 | 0.000 | 5 | faster |
| cbench_hot/strcpy_chk_16 | 0.590 | 0.611 | 0.611 | 0.000 | 5 | faster |
| cbench_hot/strcpy_chk_64 | 0.586 | 0.778 | 0.777 | 0.000 | 5 | faster |
| cbench_hot/strncmp_diff8 | 1.000 | 1.001 | 1.000 | 0.004 | 5 | parity |
| cbench_hot/strncmp_eq16 | 1.029 | 0.900 | 0.900 | 0.004 | 5 | faster |
| cbench_hot/strncmp_names64 | 0.977 | 1.182 | 1.181 | 0.000 | 5 | slower |
| cbench_hot/sys_close_ebadf | 1.059 | 0.996 | 1.009 | 0.028 | 5 | parity |
| cbench_hot/sys_fstat | 1.007 | 1.013 | 1.021 | 0.024 | 5 | parity |
| cbench_hot/sys_read_1 | 1.011 | 1.009 | 1.016 | 0.062 | 5 | parity |
| cbench_hot/unsetenv_cycle_2000 | 0.141 | 0.188 | 0.193 | 0.078 | 5 | faster |
| cbench_hot/unsetenv_cycle_30 | 0.965 | 0.696 | 0.728 | 0.073 | 5 | faster |
| cbench_hot/unsetenv_miss_2000 | 0.150 | 0.210 | 0.204 | 0.023 | 5 | faster |
| cbench_hot/unsetenv_miss_30 | 0.657 | 0.469 | 0.464 | 0.218 | 5 | faster |
| cbench_locale/fnmatch_casefold_en_32 | 0.943 | 0.928 | 0.910 | 0.029 | 5 | faster |
| cbench_locale/fnmatch_en_us | 0.299 | 0.317 | 0.316 | 0.110 | 5 | faster |
| cbench_locale/iconv_utf8_8859_15_4k | 0.321 | 0.326 | 0.326 | 0.020 | 5 | faster |
| cbench_locale/iconv_utf8_ascii_translit_4k | 0.410 | 0.478 | 0.478 | 0.029 | 5 | faster |
| cbench_locale/iconv_utf8_utf16_4k | 0.485 | 0.489 | 0.483 | 0.002 | 5 | faster |
| cbench_locale/isalpha_c | 1.436 | 0.667 | 0.668 | 0.000 | 5 | faster |
| cbench_locale/iswalpha_en_us | 1.245 | 0.913 | 0.931 | 0.012 | 5 | faster |
| cbench_locale/iswalpha_l_en_us | 1.489 | 1.282 | 1.260 | 0.020 | 5 | slower |
| cbench_locale/iswalpha_utf8 | 1.347 | 1.247 | 1.256 | 0.012 | 5 | slower |
| cbench_locale/iswctype_en_us | 1.767 | 1.447 | 1.427 | 0.010 | 5 | slower |
| cbench_locale/localeconv_en_us | 0.491 | 0.457 | 0.458 | 0.043 | 5 | faster |
| cbench_locale/mbrtowc_en_us_3 | 0.889 | 0.882 | 0.880 | 0.001 | 5 | faster |
| cbench_locale/mbrtowc_utf8_1 | 0.202 | 0.278 | 0.285 | 0.032 | 5 | faster |
| cbench_locale/mbrtowc_utf8_2 | 0.241 | 0.283 | 0.283 | 0.003 | 5 | faster |
| cbench_locale/mbrtowc_utf8_3 | 0.889 | 0.897 | 0.898 | 0.002 | 5 | faster |
| cbench_locale/newlocale_free | 0.292 | 0.314 | 0.313 | 0.006 | 5 | faster |
| cbench_locale/nl_langinfo_en_us | 0.839 | 0.701 | 0.701 | 0.000 | 5 | faster |
| cbench_locale/nl_langinfo_l_en_us | 1.249 | 0.700 | 0.701 | 0.000 | 5 | faster |
| cbench_locale/snprintf_f_en_us | 0.433 | 0.409 | 0.429 | 0.072 | 5 | faster |
| cbench_locale/strcasecmp_c_16 | 1.060 | 1.300 | 1.298 | 0.004 | 5 | slower |
| cbench_locale/strcasecmp_c_256 | 1.068 | 1.001 | 1.000 | 0.002 | 5 | parity |
| cbench_locale/strcasecmp_en_256 | 1.068 | 1.001 | 0.996 | 0.034 | 5 | parity |
| cbench_locale/strcasecmp_utf8_256 | 1.068 | 1.002 | 1.001 | 0.010 | 5 | parity |
| cbench_locale/strcoll_utf8_16 | 0.897 | 1.072 | 1.071 | 0.000 | 5 | slower |
| cbench_locale/strftime_names_en_us | 1.011 | 0.861 | 0.881 | 0.025 | 5 | faster |
| cbench_locale/strncasecmp_c_64 | 1.029 | 1.077 | 1.077 | 0.000 | 5 | slower |
| cbench_locale/strtod_c | 0.316 | 0.302 | 0.303 | 0.057 | 5 | faster |
| cbench_locale/strtod_en_us | 0.316 | 0.307 | 0.306 | 0.059 | 5 | faster |
| cbench_locale/strtod_utf8 | 0.316 | 0.302 | 0.302 | 0.056 | 5 | faster |
| cbench_locale/toupper_c | 1.049 | 0.503 | 0.502 | 0.000 | 5 | faster |
| cbench_locale/towupper_l_en_us | 1.000 | 0.909 | 0.909 | 0.003 | 5 | faster |
| cbench_locale/towupper_utf8 | 1.325 | 1.000 | 1.000 | 0.000 | 5 | parity |
| cbench_locale/wcwidth_utf8 | 1.163 | 0.958 | 0.962 | 0.197 | 5 | parity |
| cbench_malloc/calloc_free_4096 | 0.441 | 0.837 | 0.837 | 0.025 | 3 | faster |
| cbench_malloc/calloc_free_64 | 0.608 | 0.649 | 0.649 | 0.001 | 3 | faster |
| cbench_malloc/heap_mix_real | 0.736 | 0.701 | 0.697 | 0.021 | 3 | faster |
| cbench_malloc/heap_mix_real_2t | - | - | 0.438 | 0.018 | 3 | faster |
| cbench_malloc/lifo_batch64 | 0.735 | 0.954 | 0.861 | 0.055 | 3 | faster |
| cbench_malloc/malloc_free_1024 | 0.830 | 0.781 | 0.772 | 0.095 | 3 | faster |
| cbench_malloc/malloc_free_128 | 0.830 | 0.780 | 0.770 | 0.006 | 3 | faster |
| cbench_malloc/malloc_free_16 | 0.830 | 0.977 | 0.973 | 0.008 | 3 | parity |
| cbench_malloc/malloc_free_256 | 0.830 | 0.972 | 0.965 | 0.016 | 3 | parity |
| cbench_malloc/malloc_free_32 | 0.830 | 0.781 | 0.774 | 0.003 | 3 | faster |
| cbench_malloc/malloc_free_64 | 0.830 | 0.944 | 0.946 | 0.012 | 3 | faster |
| cbench_malloc/realloc_grow_16step | 0.524 | 0.488 | 0.491 | 0.020 | 3 | faster |
| cbench_malloc/realloc_grow_x2 | 0.828 | 0.930 | 0.933 | 0.042 | 3 | faster |
| cbench_malloc/realloc_moving | 0.564 | 0.585 | 0.581 | 0.004 | 3 | faster |
| cbench_malloc/realloc_null_24 | 0.856 | 0.820 | 0.791 | 0.033 | 3 | faster |
| cbench_malloc/realloc_within_usable | 0.995 | 0.824 | 0.816 | 0.004 | 3 | faster |
| cbench_malloc/xthread_free_64 | - | - | 0.469 | 0.110 | 3 | faster |
| cbench_memref/__memcmpeq_eq_128 | 1.028 | 0.636 | 0.616 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_eq_16 | 0.770 | 0.448 | 0.501 | 0.160 | 5 | faster |
| cbench_memref/__memcmpeq_eq_1k | 0.919 | 0.830 | 0.840 | 0.022 | 5 | faster |
| cbench_memref/__memcmpeq_eq_32 | 1.153 | 0.629 | 0.601 | 0.166 | 5 | faster |
| cbench_memref/__memcmpeq_eq_64 | 0.967 | 0.675 | 0.638 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_eq_8 | 0.854 | 0.533 | 0.502 | 0.199 | 5 | faster |
| cbench_memref/__memcmpeq_head_128 | 1.198 | 0.910 | 0.900 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_head_16 | 0.668 | 0.615 | 0.601 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_head_1k | 1.198 | 0.910 | 0.900 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_head_32 | 0.903 | 0.626 | 0.601 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_head_64 | 0.804 | 0.815 | 0.801 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_head_8 | 0.668 | 0.612 | 0.601 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_tail_128 | 1.025 | 0.628 | 0.616 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_tail_16 | 0.859 | 0.552 | 0.601 | 0.166 | 5 | faster |
| cbench_memref/__memcmpeq_tail_1k | 0.247 | 0.283 | 0.270 | 0.014 | 5 | faster |
| cbench_memref/__memcmpeq_tail_32 | 1.485 | 0.616 | 0.605 | 0.165 | 5 | faster |
| cbench_memref/__memcmpeq_tail_64 | 0.959 | 0.650 | 0.638 | 0.000 | 5 | faster |
| cbench_memref/__memcmpeq_tail_8 | 0.671 | 0.626 | 0.601 | 0.000 | 5 | faster |
| cbench_memref/bcmp_eq_128 | 0.952 | 0.602 | 0.573 | 0.000 | 5 | faster |
| cbench_memref/bcmp_eq_16 | 0.661 | 0.416 | 0.463 | 0.166 | 5 | faster |
| cbench_memref/bcmp_eq_1k | 0.913 | 0.833 | 0.838 | 0.029 | 5 | faster |
| cbench_memref/bcmp_eq_32 | 1.111 | 0.528 | 0.504 | 0.165 | 5 | faster |
| cbench_memref/bcmp_eq_64 | 0.928 | 0.607 | 0.585 | 0.000 | 5 | faster |
| cbench_memref/bcmp_eq_8 | 0.796 | 0.502 | 0.547 | 0.160 | 5 | faster |
| cbench_memref/bcmp_head_128 | 1.001 | 0.832 | 0.819 | 0.000 | 5 | faster |
| cbench_memref/bcmp_head_16 | 0.567 | 0.572 | 0.547 | 0.000 | 5 | faster |
| cbench_memref/bcmp_head_1k | 0.997 | 0.821 | 0.819 | 0.003 | 5 | faster |
| cbench_memref/bcmp_head_32 | 0.754 | 0.572 | 0.547 | 0.000 | 5 | faster |
| cbench_memref/bcmp_head_64 | 0.672 | 0.745 | 0.728 | 0.003 | 5 | faster |
| cbench_memref/bcmp_head_8 | 0.565 | 0.563 | 0.547 | 0.000 | 5 | faster |
| cbench_memref/bcmp_tail_128 | 0.840 | 0.585 | 0.573 | 0.000 | 5 | faster |
| cbench_memref/bcmp_tail_16 | 0.722 | 0.487 | 0.547 | 0.160 | 5 | faster |
| cbench_memref/bcmp_tail_1k | 0.239 | 0.273 | 0.263 | 0.013 | 5 | faster |
| cbench_memref/bcmp_tail_32 | 1.241 | 0.564 | 0.550 | 0.165 | 5 | faster |
| cbench_memref/bcmp_tail_64 | 0.784 | 0.616 | 0.585 | 0.000 | 5 | faster |
| cbench_memref/bcmp_tail_8 | 0.567 | 0.572 | 0.547 | 0.000 | 5 | faster |
| cbench_memref/memchr_16 | 0.892 | 0.736 | 0.728 | 0.000 | 5 | faster |
| cbench_memref/memchr_16_mix | 0.916 | 0.766 | 0.754 | 0.004 | 5 | faster |
| cbench_memref/memchr_4k | 0.781 | 1.033 | 1.007 | 0.060 | 5 | parity |
| cbench_memref/memchr_64 | 0.728 | 1.003 | 1.000 | 0.000 | 5 | parity |
| cbench_memref/memchr_64_mix | 0.695 | 0.896 | 0.821 | 0.013 | 5 | faster |
| cbench_memref/memchr_64k | 0.770 | 1.001 | 1.003 | 0.002 | 5 | parity |
| cbench_memref/memcmp_128 | 0.924 | 0.709 | 0.693 | 0.000 | 5 | faster |
| cbench_memref/memcmp_144 | 0.720 | 0.847 | 0.849 | 0.003 | 5 | faster |
| cbench_memref/memcmp_16 | 0.933 | 0.658 | 0.638 | 0.000 | 5 | faster |
| cbench_memref/memcmp_16_mix | 0.938 | 0.620 | 0.612 | 0.002 | 5 | faster |
| cbench_memref/memcmp_256 | 0.912 | 0.995 | 1.000 | 0.003 | 5 | parity |
| cbench_memref/memcmp_4k | 0.871 | 0.873 | 0.878 | 0.006 | 5 | faster |
| cbench_memref/memcmp_64 | 0.997 | 0.678 | 0.668 | 0.000 | 5 | faster |
| cbench_memref/memcmp_64_mix | 1.000 | 0.768 | 0.751 | 0.004 | 5 | faster |
| cbench_memref/memcmp_64k | 0.943 | 0.999 | 0.999 | 0.001 | 5 | parity |
| cbench_memref/memcmp_diff8_127 | 1.201 | 0.821 | 0.819 | 0.000 | 5 | faster |
| cbench_memref/memcmp_diff8_64 | 1.253 | 0.846 | 0.819 | 0.003 | 5 | faster |
| cbench_memref/memcmp_diff8_96 | 1.207 | 0.832 | 0.819 | 0.000 | 5 | faster |
| cbench_memref/memcpy_128 | 0.935 | 0.660 | 0.638 | 0.000 | 5 | faster |
| cbench_memref/memcpy_144 | 0.922 | 0.845 | 0.837 | 0.000 | 5 | faster |
| cbench_memref/memcpy_16 | 0.949 | 0.486 | 0.456 | 0.000 | 5 | faster |
| cbench_memref/memcpy_16_mix | 1.005 | 1.009 | 1.003 | 0.001 | 5 | parity |
| cbench_memref/memcpy_256 | 0.922 | 0.729 | 0.718 | 0.004 | 5 | faster |
| cbench_memref/memcpy_4k | 10.381 | 0.815 | 0.816 | 0.001 | 5 | faster |
| cbench_memref/memcpy_4k_off | 0.991 | 0.795 | 0.795 | 0.568 | 5 | faster |
| cbench_memref/memcpy_64 | 1.001 | 0.797 | 0.782 | 0.000 | 5 | faster |
| cbench_memref/memcpy_64_mix | 1.001 | 1.010 | 1.018 | 0.011 | 5 | parity |
| cbench_memref/memcpy_64k | 112.028 | 0.998 | 0.967 | 0.037 | 5 | parity |
| cbench_memref/memmem_miss_4k | 0.187 | 0.186 | 0.190 | 0.016 | 5 | faster |
| cbench_memref/memmem_miss_64 | 0.548 | 0.307 | 0.303 | 0.010 | 5 | faster |
| cbench_memref/memmove_128 | 0.931 | 0.757 | 0.710 | 0.072 | 5 | faster |
| cbench_memref/memmove_144 | 0.919 | 0.804 | 0.799 | 0.121 | 5 | faster |
| cbench_memref/memmove_16 | 0.950 | 0.989 | 0.993 | 0.012 | 5 | parity |
| cbench_memref/memmove_256 | 0.922 | 0.726 | 0.742 | 0.021 | 5 | faster |
| cbench_memref/memmove_4k | 1.036 | 0.832 | 0.852 | 0.084 | 5 | faster |
| cbench_memref/memmove_64 | 1.007 | 1.050 | 1.044 | 0.007 | 5 | parity |
| cbench_memref/memmove_64k | 0.925 | 1.009 | 1.007 | 0.003 | 5 | parity |
| cbench_memref/memmove_overlap64_4k | 10.273 | 0.999 | 0.994 | 0.006 | 5 | parity |
| cbench_memref/memmove_overlap8_4k | 10.222 | 0.927 | 0.917 | 0.010 | 5 | faster |
| cbench_memref/memset_128 | 0.960 | 0.736 | 0.728 | 0.000 | 5 | faster |
| cbench_memref/memset_144 | 1.085 | 1.415 | 1.453 | 0.000 | 5 | slower |
| cbench_memref/memset_16 | 1.053 | 0.487 | 0.456 | 0.000 | 5 | faster |
| cbench_memref/memset_16_mix | 1.008 | 0.922 | 0.897 | 0.014 | 5 | faster |
| cbench_memref/memset_256 | 1.085 | 1.416 | 1.453 | 0.000 | 5 | slower |
| cbench_memref/memset_4k | 1.041 | 1.010 | 1.010 | 0.000 | 5 | parity |
| cbench_memref/memset_4k_off | 1.007 | 1.006 | 1.005 | 0.000 | 5 | parity |
| cbench_memref/memset_64 | 1.001 | 0.797 | 0.782 | 0.000 | 5 | faster |
| cbench_memref/memset_64_mix | 1.001 | 0.977 | 0.977 | 0.006 | 5 | parity |
| cbench_memref/memset_64k | 1.025 | 1.014 | 1.001 | 0.039 | 5 | parity |
| cbench_memref/rawmemchr_16 | 0.915 | 0.693 | 0.671 | 0.004 | 5 | faster |
| cbench_memref/rawmemchr_16_mix | 0.935 | 0.816 | 0.801 | 0.004 | 5 | faster |
| cbench_memref/rawmemchr_4k | 0.863 | 1.152 | 1.151 | 0.027 | 5 | slower |
| cbench_memref/rawmemchr_64 | 0.941 | 1.177 | 1.181 | 0.000 | 5 | slower |
| cbench_memref/rawmemchr_64_mix | 0.951 | 1.256 | 1.266 | 0.004 | 5 | slower |
| cbench_memref/rawmemchr_64k | 0.822 | 1.008 | 1.007 | 0.001 | 5 | parity |
| cbench_memref/strcasestr_hit_4k | 0.065 | 0.068 | 0.068 | 0.005 | 5 | faster |
| cbench_memref/strcasestr_hit_64 | 0.452 | 0.599 | 0.605 | 0.018 | 5 | faster |
| cbench_memref/strcasestr_miss_4k | 0.038 | 0.039 | 0.039 | 0.011 | 5 | faster |
| cbench_memref/strcasestr_miss_64 | 0.237 | 0.329 | 0.328 | 0.010 | 5 | faster |
| cbench_memref/strcmp_16_mix | 1.129 | 1.280 | 1.279 | 0.008 | 5 | slower |
| cbench_memref/strcmp_64_mix | 1.164 | 1.489 | 1.507 | 0.008 | 5 | slower |
| cbench_memref/strlen_16 | 0.907 | 0.796 | 0.782 | 0.000 | 5 | faster |
| cbench_memref/strlen_16_mix | 0.934 | 0.743 | 0.708 | 0.010 | 5 | faster |
| cbench_memref/strlen_4k | 0.909 | 1.129 | 1.133 | 0.021 | 5 | slower |
| cbench_memref/strlen_64 | 0.941 | 1.263 | 1.272 | 0.000 | 5 | slower |
| cbench_memref/strlen_64_mix | 0.951 | 1.265 | 1.275 | 0.005 | 5 | slower |
| cbench_memref/strlen_64k | 0.901 | 1.007 | 1.007 | 0.001 | 5 | parity |
| cbench_memref/strnlen_16 | 1.077 | 0.766 | 0.751 | 0.000 | 5 | faster |
| cbench_memref/strnlen_16_mix | 1.058 | 0.835 | 0.837 | 0.004 | 5 | faster |
| cbench_memref/strnlen_4k | 0.946 | 1.285 | 1.268 | 0.025 | 5 | slower |
| cbench_memref/strnlen_64 | 0.854 | 1.128 | 1.142 | 0.000 | 5 | slower |
| cbench_memref/strnlen_64_mix | 0.873 | 1.178 | 1.192 | 0.006 | 5 | slower |
| cbench_memref/strnlen_64k | 0.919 | 1.036 | 1.031 | 0.001 | 5 | parity |
| cbench_memref/strstr_hit_4k | 0.788 | 1.124 | 1.114 | 0.028 | 5 | slower |
| cbench_memref/strstr_hit_64 | 0.763 | 0.775 | 0.770 | 0.001 | 5 | faster |
| cbench_memref/strstr_miss_4k | 0.121 | 0.146 | 0.145 | 0.016 | 5 | faster |
| cbench_memref/strstr_miss_64 | 0.380 | 0.255 | 0.251 | 0.013 | 5 | faster |
| cbench_memref/wcsstr_hit_4k | 0.160 | 0.221 | 0.218 | 0.010 | 5 | faster |
| cbench_memref/wcsstr_hit_64 | 0.644 | 0.588 | 0.695 | 0.001 | 5 | faster |
| cbench_memref/wcsstr_miss_4k | 0.171 | 0.192 | 0.186 | 0.009 | 5 | faster |
| cbench_memref/wcsstr_miss_64 | 0.636 | 0.694 | 0.694 | 0.000 | 5 | faster |
| cbench_opt/cold_exec_empty | - | - | 0.474 | 0.059 | 3 | faster |
| cbench_opt/execvp_missing | 1.012 | 1.004 | 1.011 | 0.020 | 3 | parity |
| cbench_opt/fnmatch_names_casefold | 0.715 | 0.775 | 0.775 | 0.012 | 3 | faster |
| cbench_opt/fnmatch_names_ch | 0.482 | 0.494 | 0.500 | 0.004 | 3 | faster |
| cbench_opt/fnmatch_names_class | 0.541 | 0.596 | 0.597 | 0.004 | 3 | faster |
| cbench_opt/fnmatch_names_lib_so | 0.394 | 0.476 | 0.475 | 0.053 | 3 | faster |
| cbench_opt/fnmatch_names_period | 0.695 | 0.804 | 0.806 | 0.006 | 3 | faster |
| cbench_opt/fnmatch_names_question | 0.396 | 0.468 | 0.480 | 0.037 | 3 | faster |
| cbench_opt/fnmatch_names_star_c | 0.396 | 0.426 | 0.422 | 0.009 | 3 | faster |
| cbench_opt/fnmatch_paths_leading_dir | 0.409 | 0.494 | 0.491 | 0.007 | 3 | faster |
| cbench_opt/fnmatch_paths_pathname | 0.425 | 0.483 | 0.479 | 0.011 | 3 | faster |
| cbench_opt/fprintf_du_line | 0.924 | 0.896 | 0.888 | 0.016 | 3 | faster |
| cbench_opt/fprintf_ls_line | 0.739 | 0.681 | 0.669 | 0.027 | 3 | faster |
| cbench_opt/gettext_c_locale | 0.139 | 0.146 | 0.147 | 0.027 | 3 | faster |
| cbench_opt/gettext_found | 1.137 | 1.176 | 1.176 | 0.003 | 3 | slower |
| cbench_opt/gettext_language_alternating | 1.092 | 0.949 | 1.050 | 0.046 | 3 | parity |
| cbench_opt/gettext_language_putenv_alone | 0.693 | 0.471 | 0.569 | 0.040 | 3 | faster |
| cbench_opt/gettext_language_steady | 1.400 | 1.494 | 1.496 | 0.001 | 3 | slower |
| cbench_opt/gettext_missing | 0.282 | 0.257 | 0.247 | 0.016 | 3 | faster |
| cbench_opt/gettext_no_catalog | 0.365 | 0.331 | 0.332 | 0.010 | 3 | faster |
| cbench_opt/gettext_vary_msgid | 0.402 | 0.354 | 0.345 | 0.065 | 3 | faster |
| cbench_opt/iconv_8859_15_utf8_latin | 0.354 | 0.346 | 0.344 | 0.004 | 3 | faster |
| cbench_opt/iconv_open_conv_close_name | 0.283 | 0.241 | 0.229 | 0.055 | 3 | faster |
| cbench_opt/iconv_utf16_utf8_mixed | 0.753 | 0.660 | 0.660 | 0.102 | 3 | faster |
| cbench_opt/iconv_utf8_8859_15_latin_4k | 0.333 | 0.353 | 0.355 | 0.005 | 3 | faster |
| cbench_opt/iconv_utf8_utf16_ascii_4k | 0.215 | 0.232 | 0.232 | 0.002 | 3 | faster |
| cbench_opt/iconv_utf8_utf16_mixed_4k | 0.620 | 0.638 | 0.638 | 0.010 | 3 | faster |
| cbench_opt/iconv_utf8_utf16_name | 0.720 | 0.762 | 0.749 | 0.058 | 3 | faster |
| cbench_opt/iconv_utf8_utf32_mixed_4k | 0.614 | 0.661 | 0.660 | 0.010 | 3 | faster |
| cbench_opt/localtime_r_new_york | 0.071 | 0.109 | 0.111 | 0.005 | 3 | faster |
| cbench_opt/localtime_r_posix_rule | 0.615 | 1.014 | 1.013 | 0.005 | 3 | parity |
| cbench_opt/mktime_new_york | 0.593 | 0.782 | 0.758 | 0.012 | 3 | faster |
| cbench_opt/mktime_posix_rule | 0.826 | 1.046 | 1.052 | 0.019 | 3 | slower |
| cbench_opt/mktime_utc | 0.558 | 0.750 | 0.750 | 0.022 | 3 | faster |
| cbench_opt/ngettext_found | 0.943 | 0.945 | 0.947 | 0.006 | 3 | faster |
| cbench_opt/re_grep_alt | 0.069 | 0.081 | 0.080 | 0.004 | 3 | faster |
| cbench_opt/re_grep_comment | 0.193 | 0.190 | 0.189 | 0.007 | 3 | faster |
| cbench_opt/re_grep_dotstar | 0.246 | 0.263 | 0.261 | 0.004 | 3 | faster |
| cbench_opt/re_grep_icase_absent | 0.310 | 0.356 | 0.354 | 0.011 | 3 | faster |
| cbench_opt/re_grep_ipv4 | 0.070 | 0.094 | 0.093 | 0.010 | 3 | faster |
| cbench_opt/re_grep_literal | 0.492 | 0.368 | 0.366 | 0.009 | 3 | faster |
| cbench_opt/re_mt4_distinct | 1.546 | 1.115 | 0.171 | 0.010 | 3 | faster |
| cbench_opt/re_mt4_shared | 1.696 | 1.162 | 0.023 | 0.015 | 3 | faster |
| cbench_opt/re_search_1byte | 0.223 | 0.201 | 0.203 | 0.003 | 3 | faster |
| cbench_opt/re_search_sed_so | 0.553 | 0.448 | 0.452 | 0.008 | 3 | faster |
| cbench_opt/re_search_sed_swap | 0.285 | 0.284 | 0.283 | 0.005 | 3 | faster |
| cbench_opt/re_sed_kv | 0.210 | 0.208 | 0.206 | 0.006 | 3 | faster |
| cbench_opt/re_sed_swap | 0.080 | 0.087 | 0.086 | 0.016 | 3 | faster |
| cbench_opt/regcomp_regfree_ipv4 | 0.797 | 0.801 | 0.785 | 0.015 | 3 | faster |
| cbench_opt/regcomp_regfree_sed_kv | 0.387 | 0.426 | 0.428 | 0.001 | 3 | faster |
| cbench_opt/regexec_sed_so | 0.369 | 0.327 | 0.324 | 0.006 | 3 | faster |
| cbench_opt/regexec_sed_swap | 0.283 | 0.282 | 0.282 | 0.004 | 3 | faster |
| cbench_opt/snprintf_d | 0.839 | 0.727 | 0.741 | 0.012 | 3 | faster |
| cbench_opt/snprintf_e | 0.963 | 0.851 | 0.848 | 0.025 | 3 | faster |
| cbench_opt/snprintf_f2 | 0.445 | 0.424 | 0.430 | 0.006 | 3 | faster |
| cbench_opt/snprintf_f_large | 0.169 | 0.349 | 0.350 | 0.002 | 3 | faster |
| cbench_opt/snprintf_g | 0.579 | 0.521 | 0.519 | 0.002 | 3 | faster |
| cbench_opt/snprintf_ld_large | 0.838 | 0.728 | 0.733 | 0.016 | 3 | faster |
| cbench_opt/snprintf_ls_line | 1.009 | 0.845 | 0.835 | 0.004 | 3 | faster |
| cbench_opt/snprintf_mixed | 0.605 | 0.517 | 0.512 | 0.009 | 3 | faster |
| cbench_opt/snprintf_s_d_colon | 1.019 | 0.833 | 0.823 | 0.008 | 3 | faster |
| cbench_opt/snprintf_s_path | 0.580 | 0.631 | 0.612 | 0.031 | 3 | faster |
| cbench_opt/snprintf_x08 | 0.907 | 0.809 | 0.823 | 0.018 | 3 | faster |
| cbench_opt/sprintf_chk_ls_nlink | 1.034 | 0.925 | 0.899 | 0.003 | 3 | faster |
| cbench_opt/strcmp_equal_16 | 1.036 | 1.000 | 1.000 | 0.000 | 3 | parity |
| cbench_opt/strcmp_late_256 | 0.885 | 1.648 | 1.650 | 0.000 | 3 | slower |
| cbench_opt/strcmp_names | 1.022 | 1.206 | 1.197 | 0.012 | 3 | slower |
| cbench_opt/strcoll_c_names | 0.864 | 1.069 | 1.052 | 0.022 | 3 | slower |
| cbench_opt/strcoll_c_utf8_names | 0.864 | 1.082 | 1.050 | 0.020 | 3 | parity |
| cbench_opt/strcoll_en_us_equal_16 | 0.018 | 0.019 | 0.019 | 0.034 | 3 | faster |
| cbench_opt/strcoll_en_us_names | 0.430 | 0.463 | 0.461 | 0.011 | 3 | faster |
| cbench_opt/strcoll_en_us_utf8_16 | 0.879 | 0.716 | 0.707 | 0.001 | 3 | faster |
| cbench_opt/strftime_c_en_us | 0.438 | 0.389 | 0.391 | 0.028 | 3 | faster |
| cbench_opt/strftime_date_en_us | 0.629 | 0.557 | 0.574 | 0.032 | 3 | faster |
| cbench_opt/strftime_iso8601_tz | 0.538 | 0.470 | 0.481 | 0.023 | 3 | faster |
| cbench_opt/strftime_log | 0.584 | 0.517 | 0.530 | 0.038 | 3 | faster |
| cbench_opt/strftime_ls_iso | 0.546 | 0.482 | 0.492 | 0.023 | 3 | faster |
| cbench_opt/strftime_ls_recent_c | 0.605 | 0.529 | 0.544 | 0.035 | 3 | faster |
| cbench_opt/strftime_ls_recent_en_us | 0.605 | 0.518 | 0.529 | 0.035 | 3 | faster |
| cbench_opt/wordexp_arith | 1.169 | 1.446 | 1.437 | 0.008 | 3 | deferred |
| cbench_opt/wordexp_param_forms | 1.187 | 1.319 | 1.320 | 0.004 | 3 | deferred |
| cbench_opt/wordexp_plain_8 | 0.593 | 0.642 | 0.640 | 0.026 | 3 | deferred |
| cbench_opt/wordexp_quoted | 1.479 | 1.873 | 1.891 | 0.007 | 3 | deferred |
| cbench_opt/wordexp_split_var | 1.082 | 0.944 | 0.942 | 0.003 | 3 | deferred |
| cbench_opt/wordexp_tilde_var | 1.346 | 1.575 | 1.553 | 0.006 | 3 | deferred |
| cbench_patterns/fnmatch_bracket | 0.866 | 0.838 | 0.831 | 0.028 | 5 | faster |
| cbench_patterns/fnmatch_pathname | 0.812 | 0.806 | 0.785 | 0.060 | 5 | faster |
| cbench_patterns/fnmatch_pathname_nomatch | 1.000 | 0.894 | 0.893 | 0.002 | 5 | faster |
| cbench_patterns/fnmatch_star | 0.868 | 0.822 | 0.785 | 0.039 | 5 | faster |
| cbench_patterns/glob_1000_nosort | 0.970 | 0.989 | 0.988 | 0.015 | 5 | parity |
| cbench_patterns/glob_1000_sorted | 0.853 | 0.806 | 0.801 | 0.025 | 5 | faster |
| cbench_patterns/regcomp_regexec_regfree_email | 0.279 | 0.316 | 0.320 | 0.026 | 5 | faster |
| cbench_patterns/regexec_backref | 1.001 | 0.954 | 0.983 | 0.003 | 5 | parity |
| cbench_patterns/regexec_class_nomatch_1k | 0.196 | 0.171 | 0.168 | 0.035 | 5 | faster |
| cbench_patterns/regexec_literal_1k | 0.039 | 0.048 | 0.047 | 0.004 | 5 | faster |
| cbench_patterns/regexec_pathological_4k | 0.000 | 0.000 | 0.000 | 0.054 | 5 | faster |
| cbench_patterns/regexec_submatch_1k | 0.073 | 0.102 | 0.102 | 0.012 | 5 | faster |
| cbench_patterns/wordexp_arith | 1.189 | 1.595 | 1.588 | 0.016 | 5 | deferred |
| cbench_patterns/wordexp_tilde_var | 1.375 | 1.586 | 1.623 | 0.048 | 5 | deferred |
| cbench_process/cfsetspeed_cfgetospeed | 0.475 | 0.707 | 0.707 | 0.000 | 3 | faster |
| cbench_process/fork_exit_wait | 1.081 | 0.988 | 0.902 | 0.023 | 3 | faster |
| cbench_process/isatty_devnull | 0.961 | 0.994 | 0.981 | 0.013 | 3 | parity |
| cbench_process/isatty_pty | 0.961 | 0.984 | 0.993 | 0.018 | 3 | parity |
| cbench_process/openpty_close | 0.991 | 0.983 | 0.982 | 0.007 | 3 | parity |
| cbench_process/popen_pclose_true | 1.029 | 1.015 | 1.001 | 0.008 | 3 | parity |
| cbench_process/posix_spawn_true | 1.049 | 1.045 | 1.007 | 0.007 | 3 | parity |
| cbench_process/posix_spawn_true_actions | 1.063 | 1.053 | 1.009 | 0.017 | 3 | parity |
| cbench_process/posix_spawn_true_heap256m | 1.042 | 1.077 | 1.001 | 0.017 | 3 | parity |
| cbench_process/posix_spawnp_true | 1.043 | 1.067 | 1.007 | 0.001 | 3 | parity |
| cbench_process/system_exit0 | 1.031 | 1.001 | 1.010 | 0.006 | 3 | parity |
| cbench_process/tcgetattr | 1.006 | 1.013 | 1.022 | 0.017 | 3 | parity |
| cbench_process/tcsetattr_now | 0.793 | 0.737 | 0.745 | 0.010 | 3 | faster |
| cbench_process/ttyname_r | 1.011 | 1.007 | 1.008 | 0.006 | 3 | parity |
| cbench_process/vfork_exit_wait | 0.999 | 0.986 | 1.005 | 0.024 | 3 | parity |
| cbench_process/waitpid_echild | 1.082 | 1.048 | 1.050 | 0.022 | 3 | slower |
| cbench_startup/exec_hello_locale | - | - | 0.757 | 0.010 | 5 | faster |
| cbench_startup/exec_true_echo | - | - | 0.623 | 0.016 | 5 | faster |
| cbench_startup/exec_true_static | - | - | 0.449 | 0.009 | 5 | faster |
| cbench_startup/spawn_bin_true | - | - | 1.007 | 0.014 | 5 | parity |
| cbench_startup/startup_instr_c | 0.018 | - | 0.570 | 0.055 | 5 | faster |
| cbench_startup/startup_instr_printf | 0.498 | - | 0.787 | 0.031 | 5 | faster |
| cbench_startup/startup_instr_setloc | 0.478 | - | 0.788 | 0.054 | 5 | faster |
| cbench_startup/startup_instr_wctype | 0.484 | - | 0.816 | 0.032 | 5 | faster |
| cbench_startup/startup_kinstr_c | 0.243 | - | 0.561 | 0.053 | 5 | faster |
| cbench_stdio/fprintf_d | 0.923 | 0.822 | 0.822 | 0.060 | 3 | faster |
| cbench_stdio/fputs_32 | 0.845 | 0.804 | 0.815 | 0.012 | 3 | faster |
| cbench_stdio/fwrite_4k | 1.593 | 0.983 | 0.983 | 0.006 | 3 | parity |
| cbench_stdio/putc | 0.752 | 0.706 | 0.708 | 0.000 | 3 | faster |
| cbench_stdio/snprintf_d | 0.839 | 0.746 | 0.752 | 0.015 | 3 | faster |
| cbench_stdio/snprintf_mixed | 0.593 | 0.519 | 0.518 | 0.008 | 3 | faster |
| cbench_time/gmtime_r | 0.545 | 0.757 | 0.757 | 0.002 | 3 | faster |
| cbench_time/localtime_r_new_york | 0.060 | 0.094 | 0.094 | 0.008 | 3 | faster |
| cbench_time/localtime_r_new_york_2100 | 0.057 | 0.090 | 0.089 | 0.005 | 3 | faster |
| cbench_time/localtime_r_posix_rule | 0.610 | 1.010 | 1.009 | 0.008 | 3 | parity |
| cbench_time/localtime_r_utc | 0.586 | 1.202 | 1.196 | 0.012 | 3 | slower |
| cbench_time/mktime_new_york | 0.508 | 0.626 | 0.636 | 0.024 | 3 | faster |
| cbench_time/strftime_c | 0.473 | 0.428 | 0.434 | 0.001 | 3 | faster |
| cbench_time/strftime_iso | 0.562 | 0.501 | 0.505 | 0.008 | 3 | faster |
| cbench_time/timegm | 0.563 | 0.862 | 0.863 | 0.001 | 3 | faster |
| cbench_unmeasured/fts_entry_deep2k | 0.279 | 0.522 | 0.517 | 0.014 | 3 | faster |
| cbench_unmeasured/fts_entry_flat | 0.822 | 0.846 | 0.836 | 0.008 | 3 | faster |
| cbench_unmeasured/qsort_c_cmp_1024 | 1.313 | 1.365 | 1.381 | 0.004 | 3 | slower |
| cbench_unmeasured/qsort_c_cmp_1024_e16 | 0.772 | 0.766 | 0.767 | 0.039 | 3 | faster |
| cbench_unmeasured/qsort_c_cmp_1024_e24 | 0.775 | 0.788 | 0.789 | 0.037 | 3 | faster |
| cbench_unmeasured/qsort_c_cmp_1024_e4 | 1.309 | 1.396 | 1.401 | 0.008 | 3 | slower |
| cbench_unmeasured/qsort_c_cmp_1024_e64 | 1.182 | 1.252 | 1.252 | 0.007 | 3 | slower |
| cbench_unmeasured/qsort_c_cmp_1024_e8 | 1.283 | 1.364 | 1.372 | 0.015 | 3 | slower |
| cbench_unmeasured/readdir_entry_flat | 1.000 | 0.989 | 0.994 | 0.008 | 3 | parity |

### Zen 3, libm rows: all rows

| bench/row | instr | cycles | time | spread | rounds | verdict |
|---|---|---|---|---|---|---|
| cbench_libm/acos | 2.280 | 7.243 | 7.283 | 0.355 | 3 | slower |
| cbench_libm/acosf | 3.153 | 5.483 | 5.491 | 0.005 | 3 | slower |
| cbench_libm/acosf128 | 18.451 | 16.917 | 16.952 | 0.019 | 3 | slower |
| cbench_libm/acosh | 12.071 | 28.939 | 28.920 | 0.010 | 3 | slower |
| cbench_libm/acoshf | 1.291 | 2.090 | 2.093 | 0.002 | 3 | slower |
| cbench_libm/acoshf128 | 7.835 | 7.532 | 7.568 | 0.010 | 3 | slower |
| cbench_libm/acoshl | 960.060 | 171.599 | 171.363 | 0.007 | 3 | slower |
| cbench_libm/acosl | 3933.857 | 274.951 | 274.910 | 0.005 | 3 | slower |
| cbench_libm/add_baseline | 1.000 | 1.001 | 1.000 | 0.000 | 3 | parity |
| cbench_libm/asin | 2.493 | 8.354 | 8.380 | 0.266 | 3 | slower |
| cbench_libm/asinf | 3.654 | 6.154 | 6.133 | 0.005 | 3 | slower |
| cbench_libm/asinf128 | 18.835 | 17.194 | 17.228 | 0.004 | 3 | slower |
| cbench_libm/asinh | 10.730 | 25.726 | 25.754 | 0.003 | 3 | slower |
| cbench_libm/asinhf | 1.192 | 2.378 | 2.377 | 0.003 | 3 | slower |
| cbench_libm/asinhf128 | 7.476 | 7.068 | 7.097 | 0.010 | 3 | slower |
| cbench_libm/asinhl | 1175.925 | 205.227 | 204.722 | 0.005 | 3 | slower |
| cbench_libm/asinl | 1680.626 | 437.055 | 437.520 | 0.001 | 3 | slower |
| cbench_libm/atan | 1.853 | 2.372 | 2.366 | 0.138 | 3 | slower |
| cbench_libm/atan2 | 1.085 | 1.534 | 1.531 | 0.022 | 3 | slower |
| cbench_libm/atan2f | 1.371 | 1.653 | 1.656 | 0.002 | 3 | slower |
| cbench_libm/atan2f128 | 26.103 | 23.008 | 23.084 | 0.005 | 3 | slower |
| cbench_libm/atan2l | 3697.964 | 289.656 | 290.139 | 0.008 | 3 | slower |
| cbench_libm/atanf | 2.413 | 3.293 | 3.295 | 0.003 | 3 | slower |
| cbench_libm/atanf128 | 27.204 | 24.485 | 24.534 | 0.006 | 3 | slower |
| cbench_libm/atanh | 9.481 | 22.422 | 22.351 | 0.003 | 3 | slower |
| cbench_libm/atanhf | 0.868 | 1.023 | 1.025 | 0.005 | 3 | parity |
| cbench_libm/atanhf128 | 6.908 | 7.574 | 7.644 | 0.013 | 3 | slower |
| cbench_libm/atanhl | 747.332 | 123.923 | 123.901 | 0.011 | 3 | slower |
| cbench_libm/atanl | 7405.102 | 292.526 | 293.399 | 0.055 | 3 | slower |
| cbench_libm/cbrt | 0.953 | 2.028 | 2.030 | 0.000 | 3 | slower |
| cbench_libm/cbrtf | 1.178 | 1.723 | 1.719 | 0.002 | 3 | slower |
| cbench_libm/cbrtf128 | 6.659 | 5.236 | 5.216 | 0.004 | 3 | slower |
| cbench_libm/cbrtl | 230.794 | 44.761 | 44.642 | 0.002 | 3 | slower |
| cbench_libm/cexp | 475.100 | 715.003 | 714.404 | 0.331 | 3 | slower |
| cbench_libm/cexpf | 706.187 | 376.649 | 376.413 | 0.007 | 3 | slower |
| cbench_libm/cexpl | 100.377 | 35.184 | 35.169 | 0.036 | 3 | slower |
| cbench_libm/clog | 178.032 | 216.407 | 215.938 | 0.022 | 3 | slower |
| cbench_libm/clogf | 269.818 | 191.651 | 191.509 | 0.001 | 3 | slower |
| cbench_libm/clogl | 781.921 | 170.971 | 171.524 | 0.018 | 3 | slower |
| cbench_libm/cos | 1.493 | 2.061 | 2.061 | 0.223 | 3 | slower |
| cbench_libm/cosf | 2.142 | 3.085 | 3.089 | 0.005 | 3 | slower |
| cbench_libm/cosf128 | 11.357 | 12.011 | 11.985 | 0.014 | 3 | slower |
| cbench_libm/cosh | 3.084 | 4.856 | 4.789 | 0.032 | 3 | slower |
| cbench_libm/coshf | 0.966 | 1.812 | 1.811 | 0.001 | 3 | slower |
| cbench_libm/coshf128 | 5.858 | 5.569 | 5.565 | 0.003 | 3 | slower |
| cbench_libm/coshl | 378.099 | 75.965 | 75.914 | 0.007 | 3 | slower |
| cbench_libm/cosl | 73.092 | 56.844 | 56.844 | 0.005 | 3 | slower |
| cbench_libm/cpow | 518.036 | 578.291 | 578.476 | 0.202 | 3 | slower |
| cbench_libm/cpowf | 776.752 | 436.346 | 437.637 | 0.003 | 3 | slower |
| cbench_libm/cpowl | 225.561 | 68.521 | 68.522 | 0.026 | 3 | slower |
| cbench_libm/csin | 325.701 | 345.005 | 344.763 | 0.222 | 3 | slower |
| cbench_libm/csinf | 399.862 | 241.515 | 241.355 | 0.002 | 3 | slower |
| cbench_libm/csqrt | 281.634 | 195.035 | 195.019 | 0.008 | 3 | slower |
| cbench_libm/csqrtf | 329.083 | 90.033 | 90.208 | 0.004 | 3 | slower |
| cbench_libm/csqrtl | 215.832 | 54.077 | 53.872 | 0.084 | 3 | slower |
| cbench_libm/erf | 1.036 | 2.074 | 2.074 | 0.002 | 3 | slower |
| cbench_libm/erfc | 1.273 | 2.483 | 2.485 | 0.008 | 3 | slower |
| cbench_libm/erfcf | 1.382 | 1.812 | 1.813 | 0.003 | 3 | slower |
| cbench_libm/erfcf128 | 9.542 | 9.410 | 9.447 | 0.011 | 3 | slower |
| cbench_libm/erfcl | 911.172 | 209.839 | 209.996 | 0.009 | 3 | slower |
| cbench_libm/erff | 1.331 | 1.730 | 1.729 | 0.002 | 3 | slower |
| cbench_libm/erff128 | 9.458 | 9.347 | 9.363 | 0.008 | 3 | slower |
| cbench_libm/erfl | 922.314 | 211.955 | 212.440 | 0.002 | 3 | slower |
| cbench_libm/exp | 1.882 | 3.233 | 3.238 | 0.002 | 3 | slower |
| cbench_libm/exp10 | 2.520 | 5.182 | 5.181 | 0.013 | 3 | slower |
| cbench_libm/exp10f | 1.606 | 3.580 | 3.577 | 0.001 | 3 | slower |
| cbench_libm/exp10f128 | 2.594 | 2.553 | 2.541 | 0.013 | 3 | slower |
| cbench_libm/exp10l | 14.460 | 8.093 | 8.101 | 0.006 | 3 | slower |
| cbench_libm/exp2 | 2.452 | 4.180 | 4.177 | 0.003 | 3 | slower |
| cbench_libm/exp2f | 1.893 | 3.516 | 3.517 | 0.004 | 3 | slower |
| cbench_libm/exp2f128 | 4.681 | 4.711 | 4.653 | 0.010 | 3 | slower |
| cbench_libm/exp2l | 18.842 | 10.085 | 10.083 | 0.006 | 3 | slower |
| cbench_libm/exp_overflow_errno | 2.522 | 2.748 | 2.739 | 0.002 | 3 | slower |
| cbench_libm/exp_qnan | 2.339 | 1.854 | 1.855 | 0.001 | 3 | slower |
| cbench_libm/exp_small | 1.882 | 3.065 | 3.063 | 0.006 | 3 | slower |
| cbench_libm/exp_tiny | 4.856 | 2.837 | 2.841 | 0.001 | 3 | slower |
| cbench_libm/exp_upward | 5.475 | 9.059 | 9.017 | 0.002 | 3 | slower |
| cbench_libm/expf | 1.796 | 3.584 | 3.591 | 0.001 | 3 | slower |
| cbench_libm/expf128 | 5.019 | 5.105 | 5.063 | 0.013 | 3 | slower |
| cbench_libm/expf_qnan | 4.822 | 3.605 | 3.608 | 0.004 | 3 | slower |
| cbench_libm/expf_upward | 5.222 | 9.092 | 9.089 | 0.001 | 3 | slower |
| cbench_libm/expl | 12.523 | 7.654 | 7.544 | 0.155 | 3 | slower |
| cbench_libm/expm1 | 1.809 | 2.429 | 2.434 | 0.010 | 3 | slower |
| cbench_libm/expm1f | 1.764 | 1.625 | 1.626 | 0.007 | 3 | slower |
| cbench_libm/expm1f128 | 3.673 | 4.255 | 4.206 | 0.010 | 3 | slower |
| cbench_libm/expm1l | 17.037 | 8.527 | 8.505 | 0.200 | 3 | slower |
| cbench_libm/hypot | 1.790 | 2.225 | 2.226 | 0.007 | 3 | slower |
| cbench_libm/hypotf | 1.373 | 2.501 | 2.507 | 0.002 | 3 | slower |
| cbench_libm/hypotf128 | 1.425 | 0.828 | 0.831 | 0.001 | 3 | faster |
| cbench_libm/hypotl | 42.306 | 11.001 | 10.998 | 0.003 | 3 | slower |
| cbench_libm/j0 | 1.201 | 1.661 | 1.697 | 0.083 | 3 | slower |
| cbench_libm/j0f | 0.706 | 0.978 | 0.978 | 0.006 | 3 | parity |
| cbench_libm/j0f128 | 3.546 | 3.838 | 3.849 | 0.020 | 3 | slower |
| cbench_libm/j0l | 49.958 | 29.029 | 29.052 | 0.003 | 3 | slower |
| cbench_libm/j1 | 1.222 | 1.603 | 1.566 | 0.041 | 3 | slower |
| cbench_libm/j1f | 0.709 | 0.947 | 0.946 | 0.001 | 3 | faster |
| cbench_libm/j1f128 | 3.518 | 3.810 | 3.825 | 0.023 | 3 | slower |
| cbench_libm/j1l | 49.861 | 29.111 | 29.082 | 0.003 | 3 | slower |
| cbench_libm/jn | 124.582 | 147.585 | 146.488 | 0.189 | 3 | slower |
| cbench_libm/jnf | 174.995 | 201.365 | 201.248 | 0.012 | 3 | slower |
| cbench_libm/jnf128 | 1.630 | 1.762 | 1.760 | 0.006 | 3 | slower |
| cbench_libm/jnl | 24.260 | 14.217 | 14.109 | 0.015 | 3 | slower |
| cbench_libm/lgamma | 1.730 | 2.232 | 2.196 | 0.128 | 3 | slower |
| cbench_libm/lgammaf | 8.080 | 9.183 | 9.149 | 0.008 | 3 | slower |
| cbench_libm/lgammaf128 | 7.895 | 7.469 | 7.492 | 0.006 | 3 | slower |
| cbench_libm/lgammal | 973.384 | 170.976 | 171.672 | 0.022 | 3 | slower |
| cbench_libm/log | 2.890 | 5.375 | 5.366 | 0.000 | 3 | slower |
| cbench_libm/log10 | 1.920 | 3.819 | 3.719 | 0.040 | 3 | slower |
| cbench_libm/log10f | 0.908 | 2.104 | 2.110 | 0.002 | 3 | slower |
| cbench_libm/log10f128 | 9.409 | 10.774 | 10.786 | 0.010 | 3 | slower |
| cbench_libm/log10l | 26.367 | 8.184 | 8.162 | 0.004 | 3 | slower |
| cbench_libm/log1p | 2.521 | 4.006 | 4.007 | 0.016 | 3 | slower |
| cbench_libm/log1pf | 1.998 | 2.436 | 2.431 | 0.009 | 3 | slower |
| cbench_libm/log1pf128 | 7.860 | 9.192 | 9.184 | 0.011 | 3 | slower |
| cbench_libm/log1pl | 31.340 | 7.974 | 7.976 | 0.003 | 3 | slower |
| cbench_libm/log2 | 2.970 | 6.520 | 6.510 | 0.001 | 3 | slower |
| cbench_libm/log2f | 1.858 | 3.565 | 3.572 | 0.001 | 3 | slower |
| cbench_libm/log2f128 | 10.478 | 11.843 | 11.885 | 0.009 | 3 | slower |
| cbench_libm/log2l | 26.347 | 8.095 | 8.150 | 0.001 | 3 | slower |
| cbench_libm/log_upward | 5.528 | 11.114 | 11.094 | 0.007 | 3 | slower |
| cbench_libm/logf | 1.792 | 3.636 | 3.623 | 0.000 | 3 | slower |
| cbench_libm/logf128 | 9.065 | 10.616 | 10.633 | 0.009 | 3 | slower |
| cbench_libm/logl | 23.975 | 7.843 | 7.834 | 0.002 | 3 | slower |
| cbench_libm/mxcsr_pair | 1.002 | 1.000 | 1.000 | 0.001 | 3 | parity |
| cbench_libm/mxcsr_wrapper_seq | 1.004 | 1.000 | 1.000 | 0.001 | 3 | parity |
| cbench_libm/pow | 2.232 | 4.034 | 4.051 | 0.006 | 3 | slower |
| cbench_libm/pow_upward | 6.193 | 10.058 | 10.139 | 0.020 | 3 | slower |
| cbench_libm/powf | 3.278 | 5.710 | 5.753 | 0.002 | 3 | slower |
| cbench_libm/powf128 | 5.207 | 5.710 | 5.752 | 0.006 | 3 | slower |
| cbench_libm/powl | 151.119 | 29.949 | 29.969 | 0.014 | 3 | slower |
| cbench_libm/sin | 1.580 | 2.192 | 2.196 | 0.187 | 3 | slower |
| cbench_libm/sin_huge | 3.675 | 5.834 | 5.816 | 0.015 | 3 | slower |
| cbench_libm/sin_upward | 4.475 | 2.550 | 2.545 | 0.064 | 3 | slower |
| cbench_libm/sinf | 2.149 | 3.314 | 3.315 | 0.002 | 3 | slower |
| cbench_libm/sinf128 | 11.360 | 11.985 | 11.949 | 0.014 | 3 | slower |
| cbench_libm/sinf_upward | 5.270 | 8.657 | 8.663 | 0.006 | 3 | slower |
| cbench_libm/sinh | 1.897 | 2.228 | 2.244 | 0.014 | 3 | slower |
| cbench_libm/sinhf | 0.665 | 0.735 | 0.733 | 0.026 | 3 | faster |
| cbench_libm/sinhf128 | 4.214 | 4.525 | 4.512 | 0.006 | 3 | slower |
| cbench_libm/sinhl | 336.239 | 112.402 | 112.068 | 0.002 | 3 | slower |
| cbench_libm/sinl | 73.105 | 57.017 | 57.015 | 0.006 | 3 | slower |
| cbench_libm/sqrt | 1.942 | 2.419 | 2.421 | 0.000 | 3 | slower |
| cbench_libm/sqrtf | 1.825 | 2.337 | 2.338 | 0.000 | 3 | slower |
| cbench_libm/sqrtf128 | 3.897 | 1.874 | 1.867 | 0.003 | 3 | slower |
| cbench_libm/sqrtl | 2.769 | 3.688 | 3.690 | 0.009 | 3 | slower |
| cbench_libm/tan | 1.634 | 2.392 | 2.404 | 0.016 | 3 | slower |
| cbench_libm/tanf | 0.924 | 1.045 | 1.046 | 0.021 | 3 | parity |
| cbench_libm/tanf128 | 10.753 | 11.107 | 11.110 | 0.013 | 3 | slower |
| cbench_libm/tanh | 2.238 | 3.114 | 3.119 | 0.073 | 3 | slower |
| cbench_libm/tanhf | 0.761 | 0.816 | 0.814 | 0.003 | 3 | faster |
| cbench_libm/tanhf128 | 4.936 | 5.089 | 5.105 | 0.008 | 3 | slower |
| cbench_libm/tanhl | 426.315 | 138.585 | 138.009 | 0.003 | 3 | slower |
| cbench_libm/tanl | 82.190 | 62.292 | 62.171 | 0.003 | 3 | slower |
| cbench_libm/tgamma | 1.232 | 1.697 | 1.701 | 0.257 | 3 | slower |
| cbench_libm/tgammaf | 0.477 | 0.517 | 0.518 | 0.006 | 3 | faster |
| cbench_libm/tgammaf128 | 14.800 | 13.856 | 13.893 | 0.011 | 3 | slower |
| cbench_libm/tgammal | 604.662 | 96.047 | 96.442 | 0.009 | 3 | slower |
| cbench_libm/y0 | 1.145 | 1.547 | 1.530 | 0.045 | 3 | slower |
| cbench_libm/y0f | 0.732 | 1.045 | 1.042 | 0.001 | 3 | parity |
| cbench_libm/y0f128 | 13.056 | 12.347 | 12.405 | 0.019 | 3 | slower |
| cbench_libm/y0l | 190.840 | 101.486 | 101.616 | 0.002 | 3 | slower |
| cbench_libm/y1 | 1.146 | 1.583 | 1.569 | 0.019 | 3 | slower |
| cbench_libm/y1f | 0.731 | 1.042 | 1.043 | 0.001 | 3 | parity |
| cbench_libm/y1f128 | 12.969 | 12.249 | 12.341 | 0.024 | 3 | slower |
| cbench_libm/y1l | 190.751 | 101.452 | 101.552 | 0.002 | 3 | slower |
| cbench_libm/yn | 484.491 | 526.987 | 527.059 | 0.223 | 3 | slower |
| cbench_libm/ynf | 681.296 | 722.602 | 725.192 | 0.005 | 3 | slower |
| cbench_libm/ynf128 | 6.441 | 6.154 | 6.189 | 0.019 | 3 | slower |
| cbench_libm/ynl | 94.014 | 50.954 | 50.966 | 0.003 | 3 | slower |

