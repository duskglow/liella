# Validation profiles (input for the optimization thread)

TEST thread, milestone 2, 2026-10-04. Realistic workloads on the validation builds
(`validation/profile.sh`; summarised by `validation/profile_report.py`): coreutils 9.12, sed 4.10
and bash 5.3 built against the host glibc 2.39 (**dynamically linked**) and statically against
Liella 5dd6c9d with the workarounds of `validation/README.md` (`liella-w`; coreutils uses gnulib's
regex, sed uses Liella's).

**Machine: AMD EPYC 7551 (Zen 1)** (not the timing machine; other agents were building, so
each row carries the load average before and after). Times are CPU seconds (`perf stat -r 5`
task-clock, children included, after one discarded warm-up run) and one wall-clock run. The
comparison is static Liella against dynamic glibc: glibc's rows include `ld.so` start-up, which
weighs only in the short `printf-loop` processes. Profiles: `perf record -F 2000` of one run of
each build (run as the calling user; perf through `sudo`). For Liella a sample is the C
library's when its symbol is defined in `libc.a` or is Liella's Rust or C-layer code; Rust
names are shortened to their path. Per-workload top-25 lists: `validation/results/profiles/`.

## Summary

| Workload | What it does | Liella / glibc CPU | Where Liella's time goes |
|---|---|---|---|
| sed-s | `sed -E 's/([a-z]+) ([0-9]+)/\2:\1/; s/\.so$/.a/'`, 4 M lines | **10.5** | regex execution (`Matcher::run`, `insert`: 33%), allocation per match (`dealloc`, `alloc`, `alloc_hint`, `TryVec` growth: ~30%), `copy_u8` 15%; glibc runs this in sed's own code (85% program) |
| sed-utf8 | `sed 's/[[:alpha:]]*a[[:alpha:]]*/<&>/g'`, UTF-8 | **6.9** | the same regex and allocation profile |
| cut-tr-uniq | `cut \| tr \| uniq -c` | 1.64 | `fwrite` path: `stdio::fwrite`, `move_units`, `copy_u8`, `write_raw`, `parts` (~35%) |
| seq | `seq 1 3000000` | 1.63 | `copy_u8` 24%, `move_units` 23%, `mempcpy` 12%, `__memcmpeq` 10% (glibc: `memcmp`, `memmove`) |
| sort-C | `sort` in the C locale | 1.34 | `memcmp` (C layer) 57%, `copy_u8` 12%, `__memcmpeq` 5% |
| ls-lR | `ls -lR /usr/include /usr/share/man` | 1.32 | `strlen`, `copy_u8`, `move_units`, `localeconv`, `Locale::eq`, `readdir` |
| bash-loop | 300,000 iterations of string and arithmetic work | 1.29 | `liella_heap_free` 10%, `strlen` 9%, `copy_u8` 5%, `malloc` 5%, `alloc_hint` 4%, `liella_machine_tp` 3% |
| sort-utf8 | `sort -k1,1` in C.UTF-8 | 1.21 | `strcoll`: `strlen` 12%, `Locale::compare_bytes` 5%, `strcoll` 4%, `liella_machine_tp` 3%, `__errno_location` 2% (glibc: `strcoll` is `strcmp` in C.UTF-8) |
| du | `du -s /usr/share /usr/lib` | 1.09 | kernel 82%; `fill_u8`, `fstatat64`, `DirStream::next` |
| sort-n | `sort -n -k2,2` | 1.08 | `copy_u8`, `memcmp`, `liella_heap_free` |
| wc | `wc -lwm` in C.UTF-8 | 1.03 | the program (93%) |
| printf-loop | 20,000 starts of `printf` | 1.02 | start-up: `locale::name::lookup_alias` **11.7%** of all samples, `Archive::lookup`, `CtypeFile::parse` (glibc: `ld.so` relocation) |
| md5sum | `md5sum` of 196 MB | 0.98 | the program |

**Hottest Liella functions across the workloads**, the optimization thread's candidates (P10
rule 11: "what validation profiles show"):

1. **Regex execution** (`liella::pattern::regex::exec::Matcher` with the POSIX policy): 7–10×
   glibc+sed on ordinary `s///` scripts, dominated by per-match allocation and `TryVec` growth
   (`Bucket::push`, `resize`) and `Matcher::insert`. Correctness first: the regex also has the
   submatch bug and the runaway cases of the triage table (T5, T6), which milestone 3 fixes; the
   timing should be retaken after that.
2. **Byte copies**: `liella_mem::copy_u8` and `liella_boundary::conv::move_units` appear in
   nearly every workload (12–24% in `sort-C`, `seq`, `cut-tr-uniq`), plus `mempcpy`; glibc uses
   `__memmove_avx_unaligned(_erms)` for the same work.
3. **stdio output** (`fwrite` → `stdio::parts` → `write_raw` → `move_units`): about a third of
   `cut-tr-uniq`.
4. **`memcmp`/`__memcmpeq`**: `sort-C`'s comparison loop (57% in `liella_machine_memcmp`, about
   glibc's share in `__memcmp_avx2_movbe`, plus 5% `__memcmpeq`).
5. **Allocator**: `liella_heap_free`, `alloc_hint`, `malloc` (bash-loop 20%, sort-n 3%).
6. **`strcoll` in C.UTF-8**: `strlen` + `compare_bytes` where glibc reduces to `strcmp`.
7. **Start-up**: `locale::name::lookup_alias` at every process start (`printf-loop`: 11.7% of
   all samples) and the locale archive lookup (P10 rule 8, cold start).
8. **`liella_machine_tp` and `__errno_location`**: 2–3% in `sort-utf8` and `bash-loop`.

## Per workload

### sort-C

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 5.294 | 1.17% | 2.44 | 7.69, 6.62, 6.35 | 7.87, 6.70, 6.38 |
| liella | 7.085 | 2.34% | 3.30 | 7.72, 6.69, 6.38 | 7.07, 6.59, 6.35 |

Liella / glibc CPU time: **1.34**

glibc: C library 74.8%, program 13.9%, kernel 11.8% of samples. Hottest C-library functions: `__memcmp_avx2_movbe` 59.0%, `__memmove_avx_unaligned` 14.2%, `__memchr_avx2` 0.7%, `_IO_file_xsputn@@GLIBC_2.2.5` 0.4%, `fwrite_unlocked` 0.2%, `__memmove_avx_unaligned_erms` 0.0%, `__GI___pthread_disable_asynccancel` 0.0%, `__GI___libc_write` 0.0%, `_IO_default_xsputn` 0.0%, `__mempcpy@plt` 0.0%.

liella: C library 77.3%, program 12.7%, kernel 11.1% of samples. Hottest C-library functions: `liella_machine_memcmp` 56.8%, `liella_mem::copy_u8` 12.3%, `__memcmpeq` 5.3%, `liella_boundary::conv::move_units` 0.8%, `liella_legacy::stdio_compat::write_raw::liella_boundary::stdio::Glue` 0.5%, `liella_boundary::stdio::fwrite` 0.4%, `fwrite_unlocked` 0.3%, `liella_boundary::stdio::parts` 0.2%, `memmove` 0.1%, `memcmp` 0.1%.

### sort-utf8

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 10.351 | 1.16% | 3.68 | 6.55, 6.50, 6.32 | 5.81, 6.34, 6.27 |
| liella | 12.573 | 1.46% | 4.19 | 5.82, 6.33, 6.27 | 5.43, 6.19, 6.23 |

Liella / glibc CPU time: **1.21**

glibc: C library 21.8%, program 70.1%, kernel 7.7% of samples. Hottest C-library functions: `__strcmp_avx2` 13.3%, `__memcmp_avx2_movbe` 2.9%, `__strcoll_l` 2.3%, `__memmove_avx_unaligned` 0.9%, `__memchr_avx2` 0.5%, `__errno_location` 0.5%, `cfree@GLIBC_2.2.5` 0.5%, `strcoll` 0.3%, `_IO_file_xsputn@@GLIBC_2.2.5` 0.2%, `fwrite_unlocked` 0.1%.

liella: C library 34.6%, program 58.5%, kernel 5.4% of samples. Hottest C-library functions: `liella_machine_strlen` 12.4%, `liella::locale::Locale::compare_bytes` 4.9%, `strcoll` 3.8%, `liella_machine_memcmp` 3.4%, `liella_machine_tp` 2.6%, `__errno_location` 2.1%, `liella_heap_free` 1.6%, `liella_mem::copy_u8` 1.3%, `__memcmpeq` 0.6%, `liella_boundary::conv::move_units` 0.5%.

### sort-n

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 8.331 | 3.48% | 2.44 | 12.36, 7.69, 6.71 | 12.28, 7.90, 6.80 |
| liella | 8.985 | 1.01% | 3.41 | 11.78, 7.87, 6.80 | 10.65, 7.81, 6.79 |

Liella / glibc CPU time: **1.08**

glibc: C library 8.4%, program 84.0%, kernel 8.7% of samples. Hottest C-library functions: `__memcmp_avx2_movbe` 4.1%, `__memmove_avx_unaligned` 2.9%, `__memchr_avx2` 0.4%, `_IO_file_xsputn@@GLIBC_2.2.5` 0.3%, `cfree@GLIBC_2.2.5` 0.3%, `fwrite_unlocked` 0.3%, `__mempcpy_avx_unaligned` 0.0%, `__memmove_avx_unaligned_erms` 0.0%, `_IO_do_write@@GLIBC_2.2.5` 0.0%, `__GI___pthread_enable_asynccancel` 0.0%.

liella: C library 12.2%, program 80.6%, kernel 7.7% of samples. Hottest C-library functions: `liella_mem::copy_u8` 3.3%, `liella_machine_memcmp` 3.2%, `liella_heap_free` 2.6%, `liella_boundary::stdio::fwrite` 1.0%, `liella_boundary::conv::move_units` 0.6%, `liella_legacy::stdio_compat::write_raw::liella_boundary::stdio::Glue` 0.4%, `__memcmpeq` 0.3%, `liella_boundary::stdio::parts` 0.2%, `fwrite_unlocked` 0.2%, `memmove` 0.2%.

### wc

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | .593 | 1.47% | 0.57 | 10.02, 7.77, 6.79 | 9.70, 7.74, 6.79 |
| liella | .612 | 1.02% | 0.59 | 9.70, 7.74, 6.79 | 9.32, 7.69, 6.78 |

Liella / glibc CPU time: **1.03**

glibc: C library 0.2%, program 93.7%, kernel 6.1% of samples. Hottest C-library functions: `strcmp` 0.1%, `__strcmp_avx2` 0.1%, `__vfork` 0.0%.

liella: C library 0.1%, program 93.2%, kernel 6.7% of samples. Hottest C-library functions: `__read_chk` 0.1%, `strcmp` 0.1%.

### md5sum

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | .355 | 0.72% | 0.34 | 9.14, 7.68, 6.78 | 9.14, 7.68, 6.78 |
| liella | .347 | 0.75% | 0.35 | 9.14, 7.68, 6.78 | 8.81, 7.63, 6.77 |

Liella / glibc CPU time: **0.98**

glibc: C library 0.4%, program 84.2%, kernel 15.4% of samples. Hottest C-library functions: `read` 0.1%, `_nl_make_l10nflist.localalias` 0.1%, `_dl_lookup_symbol_x` 0.1%, `dl_main` 0.1%, `__vfork` 0.0%.

liella: C library 0.1%, program 83.1%, kernel 16.8% of samples. Hottest C-library functions: `liella_boundary::stdio::parts` 0.1%, `__vfork` 0.0%.

### cut-tr-uniq

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | .770 | 2.46% | 0.37 | 8.58, 7.61, 6.76 | 8.58, 7.61, 6.76 |
| liella | 1.261 | 2.50% | 0.69 | 8.58, 7.61, 6.76 | 8.37, 7.58, 6.76 |

Liella / glibc CPU time: **1.64**

glibc: C library 31.0%, program 46.3%, kernel 22.6% of samples. Hottest C-library functions: `_IO_file_xsputn@@GLIBC_2.2.5` 9.6%, `__memchr_avx2` 8.2%, `__memmove_avx_unaligned_erms` 6.3%, `fwrite_unlocked` 5.1%, `__mempcpy@plt` 0.5%, `__memcmp_avx2_movbe` 0.5%, `__mempcpy_avx_unaligned` 0.3%, `_IO_file_write@@GLIBC_2.2.5` 0.2%, `__GI___libc_write` 0.1%, `_IO_default_xsputn` 0.1%.

liella: C library 41.8%, program 41.5%, kernel 17.1% of samples. Hottest C-library functions: `liella_boundary::stdio::fwrite` 7.8%, `liella_boundary::conv::move_units` 7.8%, `liella_mem::copy_u8` 7.1%, `liella_legacy::stdio_compat::write_raw::liella_boundary::stdio::Glue` 6.0%, `liella_boundary::stdio::parts` 4.6%, `fwrite_unlocked` 3.7%, `memmove` 2.6%, `liella_machine_tp` 1.1%, `liella_machine_memcmp` 0.3%, `__memcmpeq` 0.2%.

### seq

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | .043 | 4.51% | 0.04 | 7.94, 7.50, 6.74 | 7.94, 7.50, 6.74 |
| liella | .070 | 0.76% | 0.07 | 7.94, 7.50, 6.74 | 7.94, 7.50, 6.74 |

Liella / glibc CPU time: **1.63**

glibc: C library 74.5%, program 17.1%, kernel 8.3% of samples. Hottest C-library functions: `__memcmp_avx2_movbe` 53.6%, `__memmove_avx_unaligned_erms` 17.1%, `_dl_relocate_object` 1.0%, `_getopt_internal` 0.8%, `_dl_lookup_symbol_x` 0.8%, `__mempcpy_avx_unaligned` 0.6%, `update_active.constprop.0` 0.6%.

liella: C library 75.4%, program 17.9%, kernel 6.6% of samples. Hottest C-library functions: `liella_mem::copy_u8` 24.0%, `liella_boundary::conv::move_units` 22.9%, `mempcpy` 12.1%, `__memcmpeq` 10.5%, `liella_machine_memcmp` 5.6%, `liella_link_init` 0.3%.

### printf-loop

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 12.204 | 3.51% | 15.96 | 6.41, 7.18, 6.65 | 2.76, 5.70, 6.16 |
| liella | 12.453 | 2.53% | 13.84 | 2.36, 5.47, 6.08 | 2.27, 4.60, 5.71 |

Liella / glibc CPU time: **1.02**

glibc: C library 14.4%, program 1.2%, kernel 69.4% of samples. Hottest C-library functions: `do_lookup_x` 1.4%, `_dl_relocate_object` 1.1%, `strcmp` 0.9%, `handle_amd` 0.8%, `_dl_lookup_symbol_x` 0.8%, `dl_main` 0.7%, `__GI___tunables_init` 0.5%, `_nl_intern_locale_data` 0.4%, `check_match` 0.3%, `_int_malloc` 0.3%.

liella: C library 26.6%, program 1.7%, kernel 57.9% of samples. Hottest C-library functions: `liella::locale::name::lookup_alias` 11.7%, `liella::locale::archive::Archive::lookup` 2.3%, `liella_machine_strlen` 2.0%, `liella_boundary::process::getenv` 1.2%, `liella_mem::copy_u8` 0.9%, `liella::locale::ctype::CtypeFile::parse` 0.9%, `liella_legacy::global_locale::replace::liella_boundary::locale::Glue` 0.9%, `liella_machine_memcmp` 0.8%, `liella_rt::heap::Heap::refill::liella_boundary::sys::CSysImpl::KernelPages` 0.5%, `liella_boundary::conv::move_units` 0.3%.

### ls-lR

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | .066 | 0.38% | 0.06 | 2.59, 4.30, 5.54 | 2.59, 4.30, 5.54 |
| liella | .087 | 1.36% | 0.08 | 2.59, 4.30, 5.54 | 2.59, 4.30, 5.54 |

Liella / glibc CPU time: **1.32**

glibc: C library 16.1%, program 9.6%, kernel 74.3% of samples. Hottest C-library functions: `localeconv@@GLIBC_2.2.5` 2.7%, `__ctype_get_mb_cur_max` 2.1%, `_int_malloc` 2.0%, `__tz_convert` 1.4%, `__printf_buffer` 0.7%, `__vsprintf_internal` 0.7%, `tzset_internal` 0.7%, `__tzfile_compute` 0.7%, `__printf_buffer_write` 0.7%, `__strlen_avx2` 0.7%.

liella: C library 24.6%, program 15.6%, kernel 59.8% of samples. Hottest C-library functions: `liella_machine_strlen` 4.5%, `liella_mem::copy_u8` 2.9%, `liella_boundary::conv::move_units` 2.6%, `localeconv` 2.1%, `liella::locale::Locale::core::cmp::PartialEq::eq` 2.0%, `liella_machine_memcmp` 1.5%, `liella_boundary::dirtree::readdir` 1.0%, `liella::locale::Locale::item` 1.0%, `memmove` 0.9%, `liella_machine_tp` 0.9%.

### du

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | .188 | 1.53% | 0.18 | 2.55, 4.26, 5.52 | 2.58, 4.24, 5.51 |
| liella | .205 | 2.10% | 0.23 | 2.58, 4.24, 5.51 | 2.58, 4.24, 5.51 |

Liella / glibc CPU time: **1.09**

glibc: C library 5.2%, program 6.6%, kernel 88.4% of samples. Hottest C-library functions: `__GI___readdir64` 1.0%, `malloc` 0.5%, `_int_free_merge_chunk` 0.5%, `__memmove_avx_unaligned_erms` 0.5%, `_int_free` 0.5%, `cfree@GLIBC_2.2.5` 0.5%, `do_lookup_x` 0.3%, `__close` 0.2%, `__fcntl64_nocancel_adjusted` 0.2%, `_int_malloc` 0.2%.

liella: C library 12.8%, program 5.7%, kernel 81.7% of samples. Hottest C-library functions: `liella_mem::fill_u8` 1.6%, `fstatat64` 1.6%, `liella::fs::dir::DirStream::next::liella_boundary::sys::CSysImpl` 1.5%, `liella_mem::copy_u8` 1.4%, `liella_boundary::dirtree::readdir` 0.9%, `liella_boundary::conv::move_units` 0.9%, `liella_boundary::dirtree::install` 0.5%, `liella_sys_getdents64` 0.5%, `liella_rt::heap::Heap::alloc_hint::liella_boundary::KernelPages` 0.5%, `liella_heap_free` 0.5%.

### sed-s

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 11.166 | 0.63% | 11.49 | 2.61, 4.16, 5.46 | 2.92, 3.92, 5.29 |
| liella | 117.416 | 0.65% | 113.30 | 2.97, 3.61, 5.00 | 2.11, 2.75, 3.88 |

Liella / glibc CPU time: **10.52**

glibc: C library 13.8%, program 84.9%, kernel 0.8% of samples. Hottest C-library functions: `_int_free` 2.2%, `malloc` 1.8%, `__memmove_avx_unaligned_erms` 1.8%, `__memmove_avx_unaligned` 1.6%, `cfree@GLIBC_2.2.5` 1.3%, `realloc` 0.9%, `_int_malloc` 0.8%, `__getdelim` 0.7%, `__memchr_avx2` 0.6%, `_IO_file_xsputn@@GLIBC_2.2.5` 0.6%.

liella: C library 98.3%, program 1.3%, kernel 0.1% of samples. Hottest C-library functions: `liella::pattern::regex::exec::Matcher::policy::Posix::run` 23.9%, `liella_mem::copy_u8` 15.3%, `liella_boundary::heap::Liella::core::alloc::global::GlobalAlloc::dealloc` 9.9%, `liella::pattern::regex::exec::Matcher::policy::Posix::insert` 9.2%, `liella_boundary::heap::Liella::core::alloc::global::GlobalAlloc` 5.8%, `liella_rt::heap::Heap::alloc_hint::liella_boundary::KernelPages` 5.8%, `liella_boundary::conv::move_units` 4.6%, `liella::mem::TryVec::resize` 4.3%, `liella::mem::TryVec::pattern::regex::exec::Bucket::push` 3.8%, `alloc::raw_vec::RawVecInner::finish_grow::liella` 2.9%.

### sed-utf8

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 2.970 | 1.20% | 3.08 | 1.17, 2.11, 3.47 | 1.13, 2.05, 3.43 |
| liella | 20.571 | 1.89% | 20.25 | 1.16, 1.98, 3.37 | 1.02, 1.65, 3.08 |

Liella / glibc CPU time: **6.93**

glibc: C library 41.1%, program 56.6%, kernel 2.4% of samples. Hottest C-library functions: `__mbrtowc` 17.4%, `__gconv_transform_utf8_internal` 7.5%, `_IO_file_xsputn@@GLIBC_2.2.5` 2.4%, `_int_free` 2.1%, `malloc` 1.9%, `__getdelim` 1.8%, `__memmove_avx_unaligned` 1.6%, `malloc_consolidate` 1.0%, `__memmove_avx_unaligned_erms` 1.0%, `__memchr_avx2` 0.9%.

liella: C library 96.0%, program 2.9%, kernel 0.5% of samples. Hottest C-library functions: `liella::pattern::regex::exec::Matcher::policy::Posix::run` 20.8%, `liella_mem::copy_u8` 14.7%, `liella_boundary::heap::Liella::core::alloc::global::GlobalAlloc::dealloc` 9.9%, `liella::pattern::regex::exec::Matcher::policy::Posix::insert` 8.5%, `liella_boundary::heap::Liella::core::alloc::global::GlobalAlloc` 5.5%, `liella_rt::heap::Heap::alloc_hint::liella_boundary::KernelPages` 5.5%, `liella::mem::TryVec::resize` 4.4%, `liella_boundary::conv::move_units` 4.2%, `liella::mem::TryVec::pattern::regex::exec::Bucket::push` 3.6%, `liella_boundary::regex::load_block` 2.9%.

### bash-loop

| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |
|---|---|---|---|---|---|
| glibc | 3.623 | 0.46% | 3.65 | 1.01, 1.59, 3.01 | 1.01, 1.54, 2.96 |
| liella | 4.677 | 0.29% | 4.68 | 1.00, 1.53, 2.95 | 1.00, 1.49, 2.89 |

Liella / glibc CPU time: **1.29**

glibc: C library 33.1%, program 66.4%, kernel 0.5% of samples. Hottest C-library functions: `malloc` 7.1%, `_int_free` 5.9%, `__strlen_avx2` 5.2%, `cfree@GLIBC_2.2.5` 4.7%, `__strcmp_avx2` 2.6%, `__strcpy_avx2` 2.4%, `__strchr_avx2` 1.7%, `__memmove_avx_unaligned_erms` 1.1%, `_int_malloc` 0.9%, `__ctype_b_loc` 0.6%.

liella: C library 50.0%, program 49.9%, kernel 0.2% of samples. Hottest C-library functions: `liella_heap_free` 10.1%, `liella_machine_strlen` 9.0%, `liella_mem::copy_u8` 5.0%, `malloc` 4.7%, `liella_rt::heap::Heap::alloc_hint::liella_boundary::KernelPages` 4.0%, `liella_machine_tp` 3.0%, `strcmp` 2.9%, `strcpy` 2.1%, `strlen` 1.8%, `liella_mem::find_u8` 1.7%.


## Milestone 6: grep, gawk, diffutils, tar, gzip, make (2026-10-05)

`validation/profile6.sh` (the same method as above): grep 3.12, gawk 5.4.1, diffutils 3.12,
tar 1.35, gzip 1.15 and make 4.4.1 from the suite builds, glibc 2.39 (**dynamic**) against Liella
built from test/validation 3535ebf (contains main 75c0f5c: the correctness fixes, **not** the
optimization thread's work, which is not merged yet), as is (`liella-m`; make from the
`-noload` pair, T27; gawk with the maths stand-ins, provisional). **Machine: Zen 1, 2 × AMD EPYC
7551**, under `~/bin/timing` (exclusive), load 1.0 to 1.9 throughout. Inputs in
`~/validation/work`: `words.txt` (166 MB, 4 M lines of words, numbers and paths), a copy
with every 500th line changed, its gzip, a tar of `/usr/include` (23 MB) and a makefile of
20,000 targets. Each workload's output is the same under both libraries (`CHECK=1` run). Top-25
lists: `validation/results/m6/profiles/`; timings: `timings.tsv`, `timings-grep.tsv` (grep
re-run with its output piped: grep stops at the first match when its output is `/dev/null`).

| Workload | What it does | Liella / glibc CPU | Where Liella's time goes |
|---|---|---|---|
| gawk-printf | `printf "%d %s %x %.3f\n"` 1 M times | **4.88** | `liella_mem::fill_u8` **41%**, `copy_u8` 16%, `num::format::exact` 7%, `printf::parse`, allocation (glibc: `__printf_fp_buffer` 18%). Microprobe `validation/probes/snp.c`: `snprintf("%.3f")` 3.8 µs against glibc's 0.4 µs, `%d`/`%x` 0.43 µs against 0.07 µs, `%s` 0.31 µs against 0.05 µs, independent of the buffer size |
| make-n | `make -n` over 20,000 targets | 1.41 | `liella_machine_strlen` 10%, `copy_u8` 7%, `alloc_hint`, `strcmp` (glibc: `_int_malloc`, `__strcmp_avx2`, `__strlen_avx2`) |
| grep-i-utf8 | `grep -ci 'qu[a-e]'` in C.UTF-8 | 1.39 | grep's DFA as under glibc, then Liella's regex `Matcher::run` 8% and allocation (glibc: `re_search_internal` 5%) |
| gawk-regex | `/^[a-f]+ [0-9]+ /` over every line | 1.38 | allocator: `liella_heap_free` 12%, `malloc` 8%, `alloc_hint` 6%, `alloc_slow` 6% (glibc: `malloc` 9%, `_int_free` 8%); gawk uses its own regex |
| tar-tv | `tar -tvf` of the `/usr/include` archive | 1.35 | start-up and `strlen`, `copy_u8` (a 20 ms run) |
| gzip-d | `gzip -dc` of the 166 MB file | 1.30 | `copy_u8` 17% and `move_units` 14% (glibc: `__memmove_avx_unaligned_erms` 10%): gzip's window copies |
| gawk-sum | `{ s += $2 }` | 1.27 | `num::parse::parse_float` 12% and `num::float::round` 6% (`strtod`; glibc `____strtod_l_internal` 9%) |
| gawk-utf8 | `length($1)` and an array in C.UTF-8 | 1.19 | `liella_heap_free` 7%, `copy_u8` 7%, `move_units` 5% |
| grep-E | `grep -cE '^[a-f]+ [0-9]{5} '` | 1.15 | grep's DFA (55% in both); `memrchr`, `rawmemchr` |
| tar-c | `tar -cf - /usr/include \| cat` | 1.10 | kernel |
| grep-F | `grep -c usr/lib/q` | 1.06 | grep's Boyer–Moore and `memchr` (as glibc's) |
| gzip-c | `gzip -c` of the 166 MB file | 1.01 | the program |
| diff | `diff` of the two 166 MB files | **0.90** | the program (`read_files` 79% in both) |

New candidates for the optimization thread (the earlier list stands): **the printf family**
(the largest gap measured in validation so far; `%.3f` 10× and the integer and string
conversions 6× glibc per call, with `fill_u8` dominating the float path); `strtod` (`gawk-sum`);
the allocator's free path (`gawk-regex`, `make-n`); `copy_u8`/`move_units` for medium copies
(`gzip-d`). These numbers predate the optimization thread's changes and should be retaken on
its merged result.
