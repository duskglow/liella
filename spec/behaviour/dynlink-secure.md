# dynlink-secure

**Spec:** §27 (the dynamic loader), `spec/security/threat-model.md` "The dynamic loader" and the
environment table. **Plan:** DL-0a `DL-PROBE-SECURE` (plan/06-phases/17-phase-14-dynamic-linking.md),
input to DL-0d (`plan/ph14/threat-model-draft.md`) and DL-29.

**Question:** what the reference loader does in a secure (`AT_SECURE`) process for the
threat-model open questions: the `LD_PRELOAD`/`LD_AUDIT` exception, `$ORIGIN`, empty and relative
search elements, `/etc/ld.so.preload`, a damaged cache, which variables stay in the environment
(`LD_BIND_*` and the rest), `TZ`, and the diagnostic variables. A fact record, not a decision.

**Probe:** `tools/probe/dlsecure/` (`probe.c`, `lib.c`, `audit.c`, `run.sh SRCDIR SCRATCH`).
**Record:** `dynlink-secure.glibc.txt`, made on shiki (Ubuntu GLIBC 2.39-0ubuntu8.9, kernel
6.8.0) as job `dl-probe-secure-record2`. Built with `cc -std=gnu17 -O1`, every case run with
`env -i PATH=/usr/bin:/bin` plus the case's variables; the scratch directory prints as `<dir>`,
loader `<pid>` prefixes and addresses as `<hex>`. The probe prints `AT_SECURE`, whether real and
effective IDs match, the loaded objects other than the vDSO, the loader and libc (from
`dl_iterate_phdr`), and its whole environment; test libraries write `ctor <name>` to stderr when
loaded, the audit module writes `audit la_version`.

## How secure mode is made without root (R14-18: confirmed)

Each case runs twice: `[plain]` the program, `[secure]` a copy with mode 2755 whose group is a
supplementary group of the caller (`users`) and not its real group. The kernel sets `AT_SECURE`
for it: every `[secure]` line that runs prints `secure 1 rgid_is_egid 0`. No root and no sudo.
The cases that need a private `/etc` (`ns ...`) run in a user + mount namespace
(`unshare --map-root-user --map-users=auto --map-groups=auto -m`, the caller's subordinate IDs):
the namespace's root binds a scratch directory over `/etc`; for `[secure]` it copies the probe to
a tmpfs, gives it a mapped group other than its own and mode 2755. Nothing outside the scratch
directory and the namespace changes. Not reachable this way: a set-ID program or library in a
trusted system directory (that needs root to place).

## Observations

| Item | `[plain]` | `[secure]` |
|---|---|---|
| `LD_PRELOAD` absolute path, untrusted directory | loaded | skipped, no message |
| `LD_PRELOAD` absolute path in `/lib/x86_64-linux-gnu`, file without a set-ID bit | loaded | skipped, no message |
| `LD_PRELOAD` bare name of a library in the trusted directory, no set-ID bit | loaded | not loaded; message `object '...' from LD_PRELOAD cannot be preloaded (cannot open shared object file): ignored.`; program runs |
| `LD_PRELOAD` bare name that does not exist | same message, program runs | same message, program runs |
| `LD_AUDIT` absolute path to a real audit module | accepted (`la_version` runs) | skipped, no message |
| `LD_AUDIT` bare name of a trusted-directory library that is not a module | message `cannot be loaded as audit interface: undefined symbol: la_version; ignored.` | skipped, no message |
| `LD_LIBRARY_PATH` absolute, empty elements (`:`, cwd holds the library), relative element | each finds the library | all ignored: `error while loading shared libraries: libsgo.so: cannot open shared object file`, exit 127 |
| `DT_RUNPATH` and `DT_RPATH` `$ORIGIN/lib`, program in an untrusted directory | library found | `$ORIGIN` element not used: exit 127 as above |
| `dlopen("$ORIGIN/lib/libsgo.so")` | expands, loads | fails: `$ORIGIN/lib/libsgo.so: cannot open shared object file: No such file or directory` (no expansion) |
| `dlopen("lib/libsgo.so")` (relative with a slash) | loads from the current directory | the same |
| `LD_DEBUG=libs` | 25 lines on stderr | nothing |
| `LD_DEBUG=libs LD_DEBUG_OUTPUT=<file>` | written to `<file>.<pid>` | nothing written (one file after both runs) |
| `LD_SHOW_AUXV=1`, also empty `LD_SHOW_AUXV=` | auxv printed on stdout, program runs | nothing printed |
| `LD_TRACE_LOADED_OBJECTS=1`, also empty | the object list, the program does not run | ignored, the program runs; the variable stays in the environment |
| `/etc/ld.so.preload` naming an untrusted absolute path (`ns`) | loaded | loaded |
| private cache naming a library outside the default directories (`ns`, control) | `dlopen` by soname succeeds | succeeds |
| cache missing, empty, 4096 bytes of `A`, cut to 16 bytes, cut to half (`ns`) | program starts, exit 0; the cache-only library is not found (`cannot open shared object file`), no message | garbage case also secure: the same |

**Environment passed to a secure program** (case `environment residue`, values in the record).
Removed: `LD_BIND_NOW`, `LD_BIND_NOT`, `LD_DYNAMIC_WEAK`, `LD_WARN`, `LD_VERBOSE`, `LD_HWCAP_MASK`,
`LD_ORIGIN_PATH`, `LD_PROFILE`, `LD_LIBRARY_PATH`, `LD_DEBUG_OUTPUT`, `LD_PRELOAD`, `LD_AUDIT`,
`LD_DEBUG`, `LD_SHOW_AUXV`, `GLIBC_TUNABLES`, `TZDIR`, `LOCPATH`, `NLSPATH`, `GCONV_PATH`,
`MALLOC_PERTURB_`, `MALLOC_ARENA_MAX`, `MALLOC_TRACE`, `HOSTALIASES`, `LOCALDOMAIN`, `RES_OPTIONS`,
`RESOLV_HOST_CONF`, `TMPDIR`. Kept: `PATH`, `TZ`, `MALLOC_CHECK_`, `LD_USE_LOAD_BIAS`,
`LD_ASSUME_KERNEL`, `LD_PROFILE_OUTPUT`, `LD_TRACE_LOADED_OBJECTS`. A `[plain]` program keeps all.

**`TZ` in a secure program** (`localtime_r` of 1970-06-29; `Asia/Tokyo` is `+32400 JST`):

| `TZ` | `[plain]` | `[secure]` |
|---|---|---|
| `Asia/Tokyo`, `:Asia/Tokyo`, `JST-9` | JST | JST |
| `/usr/share/zoneinfo/Asia/Tokyo`, `:/usr/share/zoneinfo/Asia/Tokyo` | JST | JST |
| `<dir>/zone/myzone`, `:<dir>/zone/myzone` (a copy of Tokyo outside the zone directory) | JST | offset 0, empty zone name |
| `../../../..<dir>/zone/myzone` | JST | offset 0, empty zone name |
| `Asia/../Asia/Tokyo` | JST | offset 0, zone `Asia` |
| `TZDIR=<dir>/zone TZ=myzone` | JST | offset 0, zone `myzone` (`TZDIR` removed) |

So the reference honours a zone name, a POSIX rule and an absolute path inside the default zone
directory, and in a secure program does not read a file outside it or through `..`.

## Clauses covered

Each section of this record bears on these clauses of `spec/clauses.tsv`; the record is a fact
record, so it fixes no clause text, and DL-0d (`plan/ph14/threat-model-draft.md`) decides what
Liella does.

| Section | Clause IDs |
|---|---|
| `LD_AUDIT` skipped in a secure process | `s27_2_c_0` |
| `LD_LIBRARY_PATH`, `DT_RUNPATH`/`DT_RPATH` `$ORIGIN`, relative and empty elements, cache damage | `s27_1_c_1_1` |
| `dlopen` with `$ORIGIN` or a relative path with a slash | `s27_1_c_1` |
| Environment passed to a secure program (`LD_*`, `GLIBC_TUNABLES`, `TZDIR`, `MALLOC_*` ...) | `s1_1_c_7`, `s1_1_g_4`, `s1_8_c_0` |
| `TZ` in a secure program | `s23_6_c_0`, `s23_6_c_2` |

No clause ID names the following behaviour in §27 yet (they are gaps in the clause list, for
DL-0d and the plan to close, not a claim that the behaviour is unspecified): the `LD_PRELOAD`
exception for secure processes and its trusted-directory rule; `/etc/ld.so.preload`; `$ORIGIN`
in `DT_RUNPATH` for a secure program; the diagnostic variables `LD_DEBUG`, `LD_DEBUG_OUTPUT`,
`LD_SHOW_AUXV` and `LD_TRACE_LOADED_OBJECTS` in a secure process (no clause ID).

## Not settled here

- Whether a `LD_PRELOAD`/`LD_AUDIT` entry in a trusted directory *with* a set-user-ID or
  set-group-ID bit is honoured: needs such a file in a system directory (root). The record shows
  only that entries without the bit, and every entry outside the trusted directory, are skipped.
- `$ORIGIN` for a program that is itself in a trusted directory: same reason.
- Whether `LD_BIND_NOW` changes binding in a secure program (only its removal from the
  environment is recorded).
