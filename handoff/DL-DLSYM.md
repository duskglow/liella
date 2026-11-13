# DL-DLSYM (DL-16b): handoff (run 1, checkpoint)

**State.** Branch `task/dl-dlsym`, WIP head `14cca2489` (cut from DL-DLOPEN-LOAD-3 `216bfc645`, not yet on
next). Role shiki:~/w/sh1. Stopped at the context checkpoint (~190k of the ~150k checkpoint): the code is
written and compiles; tests have not run yet.

**Done (WIP commit).**
- `crates/boundary/src/ld/dlopen/dlsym.rs` (new): the loader's `dlsym`/`dlvsym` entry. Scopes: `RTLD_DEFAULT` =
  global scope, then a `dlopen`ed caller's root local scope (#5127, P-5); `RTLD_NEXT` = objects after the caller in
  the scope it was loaded with (global for start-time callers, the root's local scope otherwise); a handle = its local
  scope, the program's handle = global scope. Handle check = one range test over the node array (D-27.32), text
  `dlsym: invalid handle` / `dlvsym: invalid handle`. IFUNC: resolver called after the borrow ends. `STT_TLS` of a
  start-time object: `tp + tp_offset + st_value`. Texts name the handle's object or the caller (program = `argv[0]`;
  start-time object = its path, searched again only for a text; else `DT_SONAME`). Success clears dlerror. Takes the
  writer lock (lock-free readers are DL-16c's).
- `crates/boundary/src/ld/dlopen.rs`: `Node.root`; `Loader.{n_start, n_start_global, unique, unique_mark}`; the
  namespace's unique table (`Uniq`: table entry, else the start's binding = the first unique definition in the start's
  global scope; rolled back in `unwind`; `pin_unique` sets `nodelete`); DEEPBIND scope order (local first, each object
  once); `adopt_tls` (start TLS layout recomputed, same inputs as `link`); `adopt_deps` (start-time `DT_NEEDED` lists by
  `DT_SONAME` or by path identity).
- `crates/ld/src/reloc.rs`: `UniqueTable::{len, is_empty, truncate, owners}`.
- `StartInfo::dlsym` (rt), `runtime.rs` stores it, `s27_dynlink.rs`: `liella_dl_sym` + Rust twins `dlsym`/`dlvsym`
  (`export! asm GLIBC_2.34`); `csys/src/machine/dl.S`: frame-free `dlsym`/`dlvsym` entries passing the return address;
  `xtask/src/compat.rs`: dlsym/dlvsym leave `DEFERRED`.
- `crates/ld/tests/dlsym.rs`: `s27_1_c_3_dlsym_scopes_handles_and_texts` (44 numbered checks: handles.c's checks,
  P-3, P-5, DEEPBIND, unique pair, IFUNC, TLS, the dynlink.md version table, texts, foreign handles, #5127) and
  `s27_1_f_0_static_program_dlsym_fails_with_text`. NOT RUN YET.

**Exact next step.**
1. Read `~/logs/dl-dlsym-t1.log` on shiki (compat-gen, clippy, `xtask so`, `cargo test -p liella-ld --test dlsym
   --test dlopen`). compat-gen rewrites `csys/src/machine/compat_aliases.S` in the role: copy that generated file back
   into the worktree (`scp`) and commit it (it is `@generated`; never hand-edit).
2. Fix what the test finds (risks: the `VER_2 { } VER_1;` empty version nodes in the test's map; `EI_OSABI` GNU of the
   unique objects; `adopt_deps` for bare names without `DT_SONAME`).
3. `cargo xtask static`; plants file `handoff/DL-DLSYM-plants.txt` (RTLD_NEXT `.skip(1)` -> `.skip(0)` and `p + 1` ->
   `p` in dlsym.rs; NULL-handle scope without the program; `Uniq` bypassed; DEEPBIND order); the gate (direct tier,
   `--base origin/next`); records: a pending D-NEW series D-27 for DL-16b's design (lock in dlsym until DL-16c, unique
   start fallback, texts, invalid-handle text).
4. Coordinator note: probe the process capacity (513th object) on the reference; dynlink.md row (+ differences row if
   it differs).

**Open / RULING NEEDED.** `handles.c` (`conformance/c/s27_dlsym_handles.c`, PENDING-LIELLA) is on next (CONF-DYN-HELPERS
landed) but not on this branch's base (DL-DLOPEN-LOAD-3 is not on next): flipping it needs a merge of origin/next, which
the brief does not allow. Recommend: allow `git merge origin/next` on this branch, or flip it in the landing.

**Do not redo:** the design above; the scope rules come from P-5 (dynlink.md).

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §2, §8 DL-16b; D-27.32, D-27.33; dynlink.md P-3..P-6, "Symbol binding".
