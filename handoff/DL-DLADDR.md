# DL-DLADDR (DL-18): handoff (run 1, stopped at the context checkpoint)

**State.** Branch `task/dl-dladdr`, cut from origin/next 06d7940b1. Role mei:~/w/s2. Commits: `36a9b8fdf` (probe
tools/probe/dl/dladdr/), `e0b9747ff` (WIP code, **never compiled**). No jobs ran. DL-DLOPEN-CONCURRENT (DL-16c) was
not on next: the code adds a minimal `read_section`/`View` in `crates/boundary/src/ld/dlopen.rs` over the DL-16
chain (no epochs); DL-16c's merge must fold its reader into this one (or this into its).

**Done (WIP, unbuilt).** `crates/boundary/src/ld/dlopen/dladdr.rs` (dladdr/dladdr1/dlinfo entries, symbol rule,
SERINFO, PHDR, TLS from `Obj::tls`, `keep_start_paths`), `read_section`/`View` and `Env::read_paths` in
`dlopen.rs`, `search::dirs` + test `search_dirs_follow_the_search` (crates/ld/tests/search_cache.rs),
`StartInfo::{dladdr,dlinfo}` (rt/start.rs, ld.rs, runtime.rs), exports in `posix/s27_dynlink.rs` (GLIBC_2.34),
`dladdr`/`dladdr1`/`dlinfo` removed from `xtask/src/compat.rs` DEFERRED.

**Probe-settled (reference, Ubuntu 24.04 glibc 2.39; `sh tools/probe/dl/dladdr/run.sh SCRATCH`).**
- dladdr symbol: only a symbol that HOLDS the address (`value <= a < value+size`, or `a == value` for size 0); a
  gap with no holder gives sname/saddr NULL (ret still 1). Overlap: the highest start wins (s_impl over the
  lower-index, earlier s_outer). Equal starts: the lowest .dynsym index (s_b_alias idx 11 over s_b 20 and weak
  s_b_weak 18; weak s_w1 idx 9 over global s_w2 14). IFUNC: reported by its resolver address under the IFUNC name.
  STT_TLS (value 0) never matches. .plt, .dynsym, .eh_frame, .bss, base+0/1/64: object found, no symbol.
- Miss (NULL, 8, -1, stack, heap): ret 0, `*info` untouched; dlerror untouched. Program without -rdynamic:
  fname `./probe` (argv[0]), sname NULL; with -rdynamic `main` found. fbase = mapping base (l_addr for a DSO).
  ld.so: `/lib64/ld-linux-x86-64.so.2`; libc: full path; vDSO: fname `linux-vdso.so.1` with symbols.
- dladdr1: SYMENT -> the Elf64_Sym (NULL when no holder); LINKMAP -> link map (== handle; dlopen(NULL) for the
  program); flags 0 or 3: extra untouched, ret 1; miss: extra untouched.
- dlinfo: requests 0, 3 (CONFIGADDR), 7, 8, 12+ -> -1 `unsupported dlinfo request`. LMID 0. LINKMAP == handle.
  ORIGIN: directory of the path (libc `/lib/x86_64-linux-gnu`). TLS_MODID 2 for the first dlopen'd TLS module, 0
  without TLS. TLS_DATA NULL until the calling thread touched the block, then == &var. PHDR returns phnum,
  `*arg` = run-time phdr (base+64). SERINFO: `dls_size = 16 + 16*cnt + sum(len+1)`, flags 0, strings after the
  array; order RPATH (object, if no RUNPATH), LD_LIBRARY_PATH, RUNPATH (`$ORIGIN` expanded), then
  /lib/x86_64-linux-gnu, /usr/lib/x86_64-linux-gnu, /lib, /usr/lib; a buffer with dls_cnt one short still ret 0.

**Decisions to write (pending records, series D-27, `records new decision D-NEW --series D-27 --story DL-DLADDR`).**
(1) the symbol rule above; (2) readers lock-free via read_section, first call builds under the lock, a thread
inside its own first build gets "not found"; (3) dlinfo refuses a handle that is not a published link map
(`dlinfo: invalid handle`), NULL arg (`dlinfo: invalid argument`) and an unknown origin (program, ld.so:
`dlinfo: the origin of the object is unknown`); (4) SERINFO honours the caller's dls_cnt/dls_size and stores the
entries written in dls_cnt; (5) TLS_DATA is the block's address whenever it exists (dlopen'd TLS lives in the
static surplus, DL-16d) -> a docs/differences-from-glibc.md row (RULING NEEDED on a compat flag: recommend none).

**Gaps (INFORMATIONAL).** The vDSO is not in Liella's chain (P-12), so dladdr misses vDSO addresses; start-time
`l_name` stays the soname (dli_fname uses the recorded path instead).

**Exact next step.** Sync (`sync_role.sh --story DL-DLADDR --base origin/next`), `cargo xtask compat-gen`
(regenerates csys/src/machine/compat_aliases.S), clippy, fix. Then: conformance/c/s27_dladdr.c (+ helper with the
probe's asm symbol block; backtrace test: dladdr over `__builtin_return_address` through a dlopen'd helper
callback) and s27_dlinfo.c; append the record to spec/behaviour/dynlink.glibc.txt and a "dladdr and dlinfo"
section to dynlink.md; pending records; plants file (nearest-symbol `off >= s.size` -> `>`, LINKMAP
`v.node_addr(i)` -> `v.node_addr(0)`, TLS_MODID `p.module` -> constant) with
`cmd:cargo xtask affected --only conformance:s27_dladdr --with-exports --run`; gate
`cargo xtask affected --run --base origin/next` (escalates to FULL: compat_aliases.S).

**Plan rows.** plan/ph14/design.md §8, §10; plan/ph14/dl-16-dlopen.md §4; D-27.1, D-27.30, D-27.32; dynlink.md P-6, P-12.
