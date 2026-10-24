# Spec versus man pages: the 31 findings, settled

**Spec:** the clauses each row names. An independent review compared the spec with the installed
man pages (man-pages 6.7, POSIX 3p) and reported 31 contradictions (2026-10-04). Each one is
settled here under the three-way rule (implementation-plan.md §1.4): (a) the spec clause,
(b) a black-box probe of glibc 2.39, (c) the official documentation (the man page; POSIX 3p
pages count as (c) too). The kernel wins where the clause is kernel-mediated, and the owner's
rulings stand.

**Clauses:** `s9_3_f_0`, `s10_0_f_0`, `s10_6_c_7`, `s10_6_g_0`, `s1_6a_c_0`, `s3_1_i_0`,
`s6_2_f_2`, `s7_4_c_1`, `s7_6_c_0`, `s7_7_c_4`, `s7_8_c_p1`, `s8_4_c_2`, `s8_4_c_3`, `s8_4_g_0`,
`s11_11_c_0`, `s11_11_f_0`, `s15_1_c_2`, `s16_4_c_1_2`, `s17_5_c_3`, `s14_4_c_3`, `s18_4_g_1`,
`s18_4_f_0`, `s18_6_c_3`, `s25_2_c_2`, `s23_2_c_1`, `s23_1_c_3`, `s25_3_f_0`, `s25_4_c_0`,
`s29_3_c_0`, `s20_8_c_1`, `s22_8_c_1`, `s20_1_c_2`.

**Probes:** `crates/core/tests/oracle/specman_probe.c` (every finding but p2-4; with `-DNO_LIBM`
it also links against Liella's static products, which have no libm, `rpmatch` or compatibility
symbols yet) and `crates/core/tests/oracle/specman_dns.sh` (p2-4: a stub DNS server and eight
search domains in a private user, network and mount namespace). **Record:** `specman.glibc.txt`
(glibc 2.39-0ubuntu8.9, kernel 6.8.0-146, Zen 1). Findings p1-11, p1-12 and p3-10 rest on earlier
records (`catalogs.glibc.txt`, `noproc.glibc.txt`). The rpmatch case uses a locale compiled for
the probe whose `yesexpr` is `^[jJ]` and `noexpr` `^[eE]`.

## Outcomes

"Spec corrected" means the spec disagreed with both glibc and the documents: the text is
changed (evidence in the commit), `spec/clauses.tsv` regenerated, and Liella and its tests follow.
"Ruled" means an owner ruling wrote the clause: it stands, and the row says which source is the
outlier. "RULING NEEDED" carries a recommendation.

| # | Finding | (b) glibc 2.39 | (c) documents | Outcome | Clauses |
|---|---|---|---|---|---|
| p1-1 | `initstate` with fewer than 8 bytes | NULL, `EINVAL` | random(3) BUGS: `EINVAL` set, NULL not returned | RULING NEEDED: the spec agrees with glibc and the page's BUGS note is out of date. Recommend keeping the spec. | `s9_3_f_0` |
| p1-2 | `math_errhandling` | 3 (`MATH_ERRNO \| MATH_ERREXCEPT`) | math_error(7): "not supported by glibc" (a note from 2.8) | RULING NEEDED: the spec agrees with glibc; the page is out of date. Recommend keeping. | `s10_0_f_0` |
| p1-3 | Domain errors set `EDOM` | `lrint`, `lround`, `remquo`, `fma`: no `errno`, `FE_INVALID` (`lround(1e300)` raised no flag) | lrint(3), lround(3), remquo(3), fma(3): no `errno` | **Spec corrected** (`errno`: both agree). `lround`'s missing `FE_INVALID` is glibc's alone: when Liella has a libm it follows the page and raises it (a difference to record then). | `s10_0_f_0` |
| p1-4 | Pole and range errors set `ERANGE` | `logb(0)`, `erf` underflow, `fma` overflow: no `errno`, flags raised; `erfc(30)`: `ERANGE`; the `hypot` underflow the page names could not be produced (exact results) | logb(3), erf(3), fma(3): no `errno`; erfc(3): no `errno` | **Spec corrected** for `logb`, `erf` and `fma`. `erfc` is RULING NEEDED: the spec agrees with glibc, and the page is the outlier. Recommend keeping. | `s10_0_f_0` |
| p1-5 | `feenableexcept`, `fedisableexcept`, `fegetexcept` return 0 or non-zero | They return the previous set and the current set | fenv(3): the previous set, or -1; `fegetexcept`: the set | **Spec corrected.** No Liella libm yet. | `s10_6_c_7`, `s10_6_g_0` |
| p1-6 | `getsubopt`: unknown suboption's value pointer | `*valuep` is the whole `name[=value]` | getsubopt(3): the same | **Spec corrected.** Liella already answers so (probed). | `s1_6a_c_0` |
| p1-7 | `PTRDIFF_MAX` rejected since 2.29 | NULL, `ENOMEM` (the version cannot be probed) | malloc(3) HISTORY: 2.30 | RULING NEEDED (version text only). Recommend writing 2.30, as the page says. | `s3_1_i_0` |
| p1-8 | `mbrtowc(NULL)` resets the state | With a partial character pending: `(size_t)-1`, `EILSEQ` | mbrtowc(3): `EILSEQ` | **Spec corrected.** Liella answers the same, and afterwards its state is initial where glibc's is not; the page leaves the state undefined after an error. | `s6_2_f_2` |
| p1-9 | `localeconv` and `nl_langinfo` pointers valid until the locale changes | The same `struct lconv` and string pointers on every call | The pages: "may be overwritten" (a portability caution) | Not a contradiction: the pages permit less than glibc gives, and the spec states what glibc gives. | `s7_4_c_1` |
| p1-10 | `rpmatch` against the locale's expressions | In a locale whose expressions lack y/n, "y" and "n" are -1 | rpmatch(3): `^[Yy]`/`^[Nn]` always accepted | RULING NEEDED: the spec agrees with glibc, and the page is the outlier. Recommend keeping. (Liella's `rpmatch`, milestone 5, follows the locale's expressions only: D-7.9.) | `s7_6_c_0` |
| p1-11 | `LANGUAGE` ignored for `C.UTF-8` | Ignored (`catalogs.glibc.txt`: `LANG=C.UTF-8 LANGUAGE=it` gives the untranslated text) | gettext(3): only "C" | Ruled (owner, 2026-10-04): the page is the outlier. | `s7_7_c_4` |
| p1-12 | `catopen` stops at a directory or non-catalog | Stops (`EINVAL`, or -1 with `errno` 0) | catopen(3): an implementation-defined path is used | Ruled (owner, 2026-10-04): glibc agrees with the ruling; the page is silent on the stop and so is the outlier. | `s7_8_c_p1` |
| p1-13 | `ecvt` returns `ndigit` digits | Capped at 17 (`ecvt`, `fcvt`, `_r` forms) and 21 (`q` forms) | ecvt(3): "reduced to a system-specific limit" | **Spec corrected; Liella fixed** (it gave any number). glibc's 17th digit is not correctly rounded (`33333333333333330` for 1/3): Liella keeps exact digits (`s8_4_g_0`, ecvt(3) "the low order digit is rounded"), a recorded difference. By-product: glibc's `gcvt` is capped at 17 too, where the spec (`%.*g`) and gcvt(3) are not, also recorded. | `s8_4_c_2`, `s8_4_c_3`, `s8_4_g_0` |
| p1-14 | `y0`/`y1`/`yn` underflow raises no `FE_UNDERFLOW` | `y0(1e300)` and the others: an accurate tiny value, no error at all | y0(3): `ERANGE`, no `FE_UNDERFLOW` | Not a contradiction with the spec: glibc reports no range error here, so the general rule is never invoked. The page describes older behaviour. | `s10_0_f_0` |
| p2-1 | `aio_return` twice returns the same status | The same status (5, `errno` 0) | aio_return(3): `EINVAL`; aio_return(3p): "may" | Ruled (owner, 2026-10-04): glibc agrees with the ruling, and the Linux page is the outlier. | `s11_11_c_0` |
| p2-2 | Bad offset, count or descriptor reported by `aio_error`, never at submission | Submission succeeds; `aio_error` gives `EBADF` (also for offsets: `aio.glibc.txt`) | aio_read(3): at submission; POSIX: either | RULING NEEDED: the spec agrees with glibc; the Linux page differs; POSIX allows both. Recommend keeping. | `s11_11_f_0` |
| p2-3 | `socketpair` is `AF_UNIX` only | `AF_TIPC`: `EAFNOSUPPORT` here (the kernel has no TIPC module loaded) | socketpair(2): also `AF_TIPC` since 4.12 | RULING NEEDED: kernel-mediated; this kernel cannot show the TIPC case. Recommend correcting the spec to "AF_UNIX, and AF_TIPC when the kernel provides it" (the page). | `s15_1_c_2` |
| p2-4 | The search list holds at most 6 domains | Lookups try all 8 domains (res_search and getaddrinfo); `_res.dnsrch` shows 6 | resolv.conf(5): unlimited since 2.26 | **Spec corrected** (the lookups use the whole list; the ABI array shows 6). Liella behaves the same (probed). | `s16_4_c_1_2` |
| p2-5 | `getpass`: "the length is limited" | No limit (a 3 MiB line returned whole) | getpass(3) NOTES: no limit | Already corrected in milestone 1 (owner ruling: Liella bounds at 1 MiB with an override, a recorded difference). | `s17_5_c_3` |
| p2-6 | `tdelete` result unspecified only when the tree becomes empty | Deleting the root of {5,3,8} returned the node that took its place (in the tree, valid); the only node: non-NULL | tsearch(3): a dangling pointer when the root is deleted | RULING NEEDED: glibc is consistent with the spec here; the page warns that the result is unusable. Recommend relaxing the spec to "unspecified whenever the root is deleted" (no program can rely on more). | `s14_4_c_3` |
| p3-1 | `posix_spawn` reports child-side failures as its result | `ENOENT` for a missing file (and `posix_spawnp`) | posix_spawn(3): exit status 127 | RULING NEEDED: the spec agrees with glibc (2.24+); the page describes the older behaviour. Recommend keeping. | `s18_4_g_1`, `s18_4_f_0` |
| p3-2 | `waitid` gives 8 bits | `si_status` 44 for `_exit(300)` | _exit(3p): the full value | Ruled (owner, 2026-10-03; the kernel wins): POSIX is the outlier. | `s18_6_c_3` |
| p3-3 | `msgrcv` with a negative type: "first ≤ \|type\|" | The lowest type ≤ \|type\| (types 3, 1, 2, `-3` gives 1) | msgrcv(2): the lowest type | **Spec corrected** (kernel-mediated; Liella passes the call through). | `s25_2_c_2` |
| p3-4 | `settimeofday` with a time zone fails with `EINVAL` | `EINVAL` (unprivileged; `settimeofday(NULL, &tz)` and without a zone `EPERM`) | gettimeofday(2): the zone is ignored | RULING NEEDED: the spec agrees with glibc; the page is the outlier. Recommend keeping. | `s23_2_c_1` |
| p3-5 | `gettimeofday`'s zone filled with zeros | `{0, 0}` written (0x55 bytes before) | "ignored, not passed to the kernel" | Not a contradiction: the page says what the kernel gets, and glibc fills the zone with zeros as the spec says. | `s23_1_c_3` |
| p3-6 | `mq_notify` `EBUSY` for the caller too | `EBUSY` on the second registration by the same process | mq_notify(3): "another process" | Kernel-mediated; the kernel wins. The spec stays and the page is imprecise. | `s25_3_f_0` |
| p3-7 | `shm_open` name at most `NAME_MAX` | 255 bytes ok, 256 `ENAMETOOLONG` | shm_open(3): `PATH_MAX` | Ruled (owner, 2026-10-04): glibc agrees with the ruling; the page is the outlier. | `s25_4_c_0` |
| p3-8 | `isastream` returns 0 for valid descriptors | 0 for fd 0, `EBADF` for a closed one | unimplemented(2): the system calls fail with `ENOSYS` | Not a contradiction: the page describes the kernel's system calls, not glibc's function. | `s29_3_c_0` |
| p3-9 | `strsignal`'s buffer for unknown signals is per thread | Each thread gets its own buffer | strsignal(3): MT-Unsafe race | RULING NEEDED: the spec agrees with glibc; the attribute is a conservative label. Recommend keeping. | `s20_8_c_1` |
| p3-10 | Thread names: `prctl` for the calling thread | The calling thread's name is set and read without `/proc` (`noproc.glibc.txt`) | pthread_setname_np(3): always the `/proc` comm file | RULING NEEDED: the spec agrees with glibc; the page describes older behaviour. Recommend keeping. | `s22_8_c_1` |
| p3-11 | `SIGRTMIN` is a function call | 34, from `__libc_current_sigrtmin()` | nptl(7): "defined with the value 34" | Not a contradiction: the page gives the run-time value; the macro expands to the call. Liella: 33 (D-20.1). | `s20_1_c_2` |

**Totals:** 8 spec corrections (p1-3, p1-4 in part, p1-5, p1-6, p1-8, p1-13, p2-4, p3-3; one Liella
fix, `ecvt`). 6 settled by owner rulings or the kernel (p1-11, p1-12, p2-1, p3-2, p3-6, p3-7), plus
p2-5 already corrected. 12 RULING NEEDED (p1-1, p1-2, p1-4 for `erfc`, p1-7, p1-10, p2-2, p2-3,
p2-6, p3-1, p3-4, p3-9, p3-10; most with "keep the spec"; p1-4 counts under two outcomes). 5 not
contradictions (p1-9, p1-14, p3-5,
p3-8, p3-11).
