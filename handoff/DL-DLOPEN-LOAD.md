# DL-DLOPEN-LOAD (DL-16a): handoff (run 2, DL-DLOPEN-LOAD-2)

**State.** Branch `task/dl-dlopen-load-2` (stacked on DL-LIBC-START-10 `cdafa3d1c`, not on next yet; run 1's
`717f85a80` included). Role kanon:~/w/kn1. Code `a9a210921`, plants file `92c7deee1`, probe record `98c729e91`,
this note and `handoff/reports/DL-DLOPEN-LOAD-2.json` on top. **dlopen and dlerror work end to end**
(kernel-started and `ld.so PROGRAM`); the branch must not land until the gate is green and the deviation rows and
records below exist.

**Jobs on `98c729e91` (kanon).** `dl-dlopen-load-2-plants`: 5 CAUGHT / 0 SURVIVED / 0 WARN. `dl-dlopen-load-2-gate`
(direct tier, `--base origin/next`): 6491 passed, **1 failed**: `xtask affected::tests::a_loader_change_does_not_select_the_math_suites`
("crates/ld/src/lib.rs (Direct) is loader code: no liella-math suite": liella-math's suites are now selected for a
loader change). Cause NOT found yet: no Cargo.toml or affected.rs change on the branch; the math tests do not call
dlopen. Suspects: the new `crates/ld/tests/dlopen.rs` (a liella-ld suite that needs the products) or the new
boundary modules changing the selector's graph. Check first whether it also fails on origin/next and on
DL-LIBC-START-10's head (`cargo test -p xtask --bin xtask a_loader_change`), then fix the cause. Peak 14G.
Clippy + static clean on the code head (job `dl-dlopen-load-2-t4`, which also ran the new test green).

**Done in run 2.**
- `crates/boundary/src/ld/dlopen.rs` (child of ld.rs): the loader (one `MAP_NORESERVE` mapping made at the first call;
  start-time objects re-read from their memory images with `image_view`/`own_object`; nodes with the public
  `struct link_map` prefix; global scope; path and dependency arenas), the re-entrant futex writer lock, search with
  the caller as requester (`search::search`, cache read per call, env from the start), `(st_dev, st_ino)` identity,
  `liella_ld::dlopen::check` per candidate, new-set BFS, `check_versions`, pass 1 for the whole set, then pass 2
  (#5125), RELRO, one Release publication, constructors after the `&mut Loader` borrow ends, destructors of
  dlopen'd objects first in `fini_all`. Failure: text set first, then every new reservation and file map unmapped
  and the arenas rewound. `ld.so` by any path (its DT_SONAME) = the start-time ld.so (#5126).
- `ld.rs`: `link` records per object (phdr, phnum, bias, dev, ino) in `START_OBJS` (no allocation at start);
  `run_init` records argc/argv/envp/auxv in `START_ARGS` (the stack is rearranged after `ld.so PROGRAM`, and the
  C library stores its own `__libc_stack_end`: never re-read the stack later). `StartInfo::dlopen` is wired.
- libc: `csys/src/machine/dl.S` (`dlopen` passes its return address), `liella_dl_open`, `dlerror`, the
  `dlopen` asm twin (`posix/s27_dynlink.rs`); `crate::dlerr` (per-thread 8 KiB record at the thread block's 0x280,
  `set`/`clear`/`take`); static programs: NULL + `<file>: shared objects cannot be loaded into a statically linked
  program` (#5005). `DEFERRED` lost `dlopen`/`dlerror`; `compat_aliases.S` regenerated (both @GLIBC_2.2.5).
- `liella_ld::dlopen`: probed texts and `check()` order (header, segments, dynamic, PIE, TLS); `of_map_error`.
- Probe `tools/probe/dl/gaps` (#5266) recorded in `spec/behaviour/dynlink.md` + `.glibc.txt`.
- `xtask/src/policy.rs`: `slice` permitted in `src/ld/` and `src/dlerr.rs` (same trust as ld.rs).
- Test `crates/ld/tests/dlopen.rs` (s27_1_c_1/c_5): 24 checks incl. every unused mode bit, texts, unwind via
  `/proc/self/maps`, NOLOAD/success clearing. Plants: `handoff/DL-DLOPEN-LOAD-2-plants.txt` (5).

**Still owed (next run, in order).**
1. Records (on the role, nothing else running there): `cargo xtask records new compat COMPAT-NEW` twice (#5125: a
   failing dlopen runs no IFUNC resolver, restore = run pass 2 per object as at start; #5126: ld.so by any path is the
   start-time object, restore = map a second ld.so) and `records new decision D-NEW --series D-27 --story
   DL-DLOPEN-LOAD` for the design (loader state, lock, publication, failure unwind, texts order); then two rows in
   `docs/differences-from-glibc.md` (table after line 304) naming the COMPAT ids. Also a row (or D-row) for
   `e_ehsize`/`p_align` refusals that the reference loads (reader rule, now visible through dlopen).
2. A static-program test (s27_1_f_0, libc.a link: NULL + text) and a unit test file for `liella_ld::dlopen`
   (mode sweep, `check` order, `reopen` saturation, `compose` truncation).
3. CONF-DYN-HELPERS: flip its pending dlopen programs once it is on next.

**Known gaps (not bugs of this branch, later stories):** start-time objects get `l_name` = DT_SONAME (not path);
dlopen'd objects referencing TLS symbols of start-time objects (no `tls` placement adopted; DL-16d); one
`UniqueTable` per call (DL-16b namespace table); the `$ORIGIN` of a start-time object is unknown; requester chain is
[caller, program] for the root and [dependent, program] for dependencies (no root/caller RPATH inheritance);
fork/lock (DL-16c); dlerror record not freed at thread exit (DL-15 hook).

**Do not redo:** the probes (gaps answered), the driver, the stack-recording fix (START_ARGS).

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §1-§4, §8; spec/behaviour/dynlink.md "dlerror texts".
