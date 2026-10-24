# Compat versions: the old contracts (LG-0b, phase 15)

Spec: C0.6 (clause `s0_6_c0_6_0`), H7, Appendix C.3; plan `plan/06-phases/18-phase-15-legacy-tier-and.md` item LG-0b (R15-1, R15-2).
Probes: `tools/probe/compat/compat_libc.c`, `compat_thread.c`, `compat_libm.c`; the probed rows are listed in
`tools/probe/compat/LIST.tsv` (library, symbol, version, probe), checked by `cargo xtask static` (`compat-probes`:
every row is a `compat` row of `spec/abi/versions.tsv`, its probe binds `symbol@version` with `.symver`, and the record
has a line for it; once `spec/abi/compat-plan.tsv` exists, every `behaviour` row of it must be listed).
Record: [compat.glibc.txt](compat.glibc.txt), made with `sh tools/probe/compat/run.sh SCRATCH` on shiki (glibc 2.39,
Ubuntu 24.04, 26 CPUs, kernel affinity mask 8 bytes), twice with identical output. Black-box only: each old version is
bound with `.symver` and called; nothing here derives from the reference library's source.

Every observation line starts with `symbol@version`; `| default:` gives the default version's result for the same
call, and a `[default ...]` tag marks a line that observes the default version. Lines without that prefix in the libm
part are what the reference library wrote to stderr itself (the SVID-mode messages, e.g. `lgamma: SING error`).
`glob@GLIBC_2.2.5` is recorded in [patterns.md](patterns.md) (the ledger item), not here.

## What the record settles (summary; the record is authoritative)

- `memcpy@GLIBC_2.2.5`: for every size 0..10000 and shift probed (±1, ±3, ±8, ±15, ±16, ±64, ±1000) the result of an
  overlapping copy equals `memmove`'s (overlap-safe), on this CPU.
- `fmemopen@GLIBC_2.2.5` differs from the default in: size 0 is `EINVAL` (default opens); mode `w` writes a NUL at
  offset 0 at open; `SEEK_END` is relative to the string length in `r+` (default: the size); `a+` starts after the NUL
  (one byte further than default) and reads `NUL` first; a NULL-buffer `w+` stream reads back the whole size (8), the
  default only what was written (3); in mode `a`, a seek to 0 then a write goes to offset 0 (default: appends).
- `realpath@GLIBC_2.2.5`: a NULL `resolved` buffer is `EINVAL`; with a buffer it equals the default.
- `regexec@GLIBC_2.2.5`: under `REG_STARTEND` the reported match offsets are relative to `rm_so` (default: to the
  string start), see the `[2,4)` lines; return values agree.
- `posix_spawn{,p}@GLIBC_2.2.5`: a file without `#!` that `execve` rejects with `ENOEXEC` is run by `/bin/sh` (spawn
  returns 0); the default returns `ENOEXEC`. Other failures (missing file, not executable, bad file action) agree.
- `quick_exit@GLIBC_2.10`: also runs the calling thread's `thread_local` destructors (registered with
  `__cxa_thread_atexit_impl`) before the `at_quick_exit` handlers; the default does not. Neither flushes stdio nor runs
  `atexit` handlers.
- `nftw{,64}@GLIBC_2.2.5`: no difference in any case probed, including unknown flag bits (directory order is not
  recorded: visits are sorted, `FTW_SKIP_SIBLINGS` is not probed).
- `clock_*@GLIBC_2.2.5`: results and error numbers equal the default for every case.
- Affinity: see the `size=` lines. The 2.3.3 `pthread_attr_{get,set}affinity_np` fault for every size when called with
  the (attr, size, mask) shape (child killed by signal 11), consistent with a two-argument (attr, mask) entry; LG-2 must
  settle the shape with a further probe before implementing them. The other 2.3.3/2.3.4 entries return what the
  default does or differ in the size rules shown per line.
- `pthread_getattr_np@GLIBC_2.2.5` and `pthread_sigmask@GLIBC_2.2.5`: see their lines (all fields compared).
- libm: `_LIB_VERSION@GLIBC_2.2.5` starts at 2 (`_POSIX_`). In that mode and in -1 (`_IEEE_`) the old entries give
  the default's results except the NaN sign of `log*` of a negative number (old `+nan`, default `-nan`; -1: no
  difference at all). Modes 1 (`_XOPEN_`) and 3 (`_ISOC_`) also change the `log*` and `pow*` error cases; mode 0
  (`_SVID_`) changes those and `exp*`, `exp10*`, `fmod*`, `hypot` error cases (e.g. `HUGE` = `FLT_MAX` on overflow, `pow(0,
  negative)` = 0 with `EDOM`, `fmod(x, 0)` = `x` with `EDOM` and no invalid flag) and writes the stderr messages. A
  program-defined `matherr` was never called in any mode. `totalorder*` at 2.25-2.27, called by value, agree with the
  default. Per-line detail: the record.

## Compat-only names: the removed system calls and STREAMS (LEGACY-STUBS, LG-5)

**Clauses:** `s24_6_c_6`, `s29_3_c_0`, `s29_4_c_3`.

**Probe:** `crates/core/tests/oracle/legacy_stubs_probe.c`, a black-box C program that binds each
name by its compat version (`.symver name,name@GLIBC_2.2.5`) and calls it on the host glibc
(2.39, keke, kernel 6.8, job `legacy-stubs-probe`). Mode `seccomp` first installs a filter that
makes the old x86_64 system-call numbers (134 `uselib`, 156 `_sysctl`, 174 `create_module`, 177
`get_kernel_syms`, 178 `query_module`, 180 `nfsservctl`, 181 `getpmsg`, 182 `putpmsg`, 183-185) fail
with `EXDEV`: a name that answers `EXDEV` there makes its system call; one that answers `ENOSYS`
fails in the library. Output: [legacy-stubs.glibc.txt](legacy-stubs.glibc.txt) (a separate record from the LG-0b one above). Build: `cc -O1 legacy_probe.c`.

| Names | glibc 2.39 | Liella |
|---|---|---|
| `fattach`, `fdetach`, `getmsg`, `putmsg`, `getpmsg`, `putpmsg` | -1, `ENOSYS`, for every argument (valid, closed or NULL); no system call, not even 181/182 | the same (`liella_legacy::stubs::removed`) |
| `isastream` | 0 for an open descriptor, `errno` untouched; -1 `EBADF` for a closed one or -1 | the same (an `F_GETFD` check) |
| `bdflush`, `sysctl`, `__sysctl` | -1, `ENOSYS`; no system call; `sysctl` leaves `*oldlenp` alone | the same |
| `sstk` | `(void *)-1`, `ENOSYS`; no system call | the same |
| `create_module` | makes system call 174; `(caddr_t)-1` with the kernel's `ENOSYS` | the same |
| `get_kernel_syms`, `query_module`, `uselib` | make their system calls (177, 178, 134); -1 with the kernel's `ENOSYS` | the same |
| `nfsservctl` | makes system call 180; a `long` -1 (all 64 bits) with `ENOSYS` | the same |

Every name is compat-only (`@GLIBC_2.2.5`, no default version): exported through the generated
`.symver` stubs (`cargo xtask compat-gen`, `xtask/src/compat.rs` `BEHAVIOUR`), absent from
`libc.a` (R15-1, R15-2). No difference from glibc.
