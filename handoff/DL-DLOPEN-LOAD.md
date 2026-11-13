# DL-DLOPEN-LOAD (DL-16a): handoff (run 1, context checkpoint)

**State.** Branch `task/dl-dlopen-load`, stacked on DL-LIBC-START-10 (`cdafa3d1c`, not on next yet). WIP code commit
`717f85a80`; this note and `handoff/reports/DL-DLOPEN-LOAD.json` on top. Role keke:~/w/n2. INCOMPLETE: the run hit the
~150k context checkpoint after the survey and the groundwork; `dlopen` does not work yet.

**Done (717f85a80).**
- `crates/ld/src/dlopen.rs` (pure, no unsafe): `mode()` (unknown bits refused first with `invalid mode parameter`,
  no binding with the prefixed `invalid mode for dlopen(): Invalid argument`), `Refusal` + `of_elf_error()` +
  `loadable()` (every probed header text; `ET_EXEC`, `DF_1_PIE`, `PT_TLS` -> static-TLS text until DL-16d),
  `Refusal::skipped(searched)` (e_machine always skipped -> file-missing text; class/data/OS ABI skipped only in a
  search), `reopen()` (count saturates at `u32::MAX` and pins), `compose()` (text into a buffer, NUL, truncation),
  `bare()`.
- `crates/ld/src/reloc.rs`: `relocate` = `relocate_first` (pass 1: every reference resolved, no foreign code; now also
  checks the field of each deferred IFUNC/IRELATIVE entry) + `relocate_deferred` (pass 2, resolvers). Start-time
  behaviour unchanged except that a bad IFUNC field is refused in pass 1.
- `ThreadBlock::dlerror` at 0x280 (from `tail`, after glibc's reserved area); `StartInfo::dlopen` (ld.so passes 0 for
  now). DL-15 (DL-TLS-THREADS) may also touch `rt/src/tls.rs`: expect a small merge there.

**Design for the rest (decided in this run; write it as the pending D-27 record).**
1. *No start-up cost:* `link()` must not allocate. It records per start-time object 8 words in a static next to
   `FINI_HOOKS` (phdr (0 = ld.so itself), phnum, bias, dev, ino, run-time address of its NUL-terminated `DT_NEEDED`
   name = bias + `DT_STRTAB` + offset in the strtab slice, TLS module, TLS offset) plus page size, secure flag,
   `LD_LIBRARY_PATH`/`LD_ORIGIN_PATH` spans, `AT_EXECFN`, platform/hwcaps (`&'static`). The first `dlopen` rebuilds the
   start-time `Obj`/`Object` set from memory with `image_view` (as `own_object` does) and keeps those scratch maps.
2. *State:* one `Loader` in a fresh `MAP_NORESERVE` mapping at the first `dlopen` (address in a static): `objs`,
   `syms` (MaybeUninit arrays, first `n` initialised, contiguous for `reloc::Objects`), `nodes` (repr(C) link map:
   `l_addr, l_name, l_ld, l_next (AtomicUsize), l_prev`, then private: soname, dev/ino, range, opens, flags
   nodelete/global/state, deps (u32 list in the bump arena), fini words, init-log link, file mapping, reservation),
   `global` (usize indices), a scope scratch array, a bump arena (names, dep lists) with mark/rewind. Cap ~2048
   objects (refuse beyond, with a text). Never hold `&mut Loader` across constructor calls (re-entrant `dlopen`).
3. *Entry:* `csys/src/machine/dl.S` (picked up by crates/boundary/build.rs's glob) under `LIELLA_EXPORT`:
   `dlopen: movq (%rsp),%rdx; jmp liella_dl_open` (frame-free; the compat stub's `jmp dlopen@PLT` keeps the
   caller's return address). `export! { asm "GLIBC_2.34"; fn dlopen }` twin; `dlerror` a plain `export!` in
   `posix/s27_dynlink.rs`. `liella_dl_open` calls the loader through `StartInfo::dlopen` (stored by
   `runtime::early_init`) with `liella_machine_call3`; 0 (static program, other loader) -> NULL and
   `<name>: shared objects cannot be loaded into a statically linked program` (ruling #5005). Remove `dlopen`,
   `dlerror` from `xtask/src/compat.rs` `DEFERRED` and regenerate `compat_aliases.S` with `cargo xtask compat-gen`
   on the role. dlsym/dlvsym stay with DL-16b (exporting a failing dlsym in libc.so.6 would turn load-time failures
   into silent run-time ones; RULING NEEDED if #5005 wants them in libc.a now).
4. *dlerror:* a shared boundary module (built into ld.so and libc): record = 16 KiB anonymous mapping per thread at its
   first failure, address in `ThreadBlock::dlerror`; word 0 pending flag, then the text. `set(parts)`, `clear()` on
   every successful `dl*` call (#5127, P-6), `take()` returns the text once. Thread-exit release is DL-15's hook.
5. *Driver (`crates/boundary/src/ld/dlopen.rs`, child of ld.rs):* re-entrant futex lock (owner = thread pointer,
   depth; Drepper's three-state mutex); mode check; NULL/"" -> program node; bare name -> `DT_SONAME` match; else
   `search::search` with chain [caller's object, program] (caller = node whose range holds the return address);
   open/fstat (directory text), `of_elf_error`/`loadable`/`skipped`; `(dev, ino)` identity -> re-open (`reopen`,
   promotions, no re-relocation); NOLOAD -> NULL, no text. New set BFS (deps searched with [dependent, root, caller,
   program]; `INTERP` name -> ld.so's node; missing -> `<needed>: cannot open shared object file: ...`), tentative
   nodes after `n`; `check_versions` (texts as dynlink.md); scope = global ++ root's local (DEEPBIND: local first);
   `relocate_first` for all new objects in reverse load order, then `relocate_deferred`; `protect_relro`; publish
   (links, `n`, `opens = 1`, nodelete from mode or `DF_1_NODELETE`, GLOBAL appends the local scope, adds counter);
   `init_order` over the new set (<= 64 objects: `init::OBJECTS_MAX`), constructors with argc/argv/envp from
   `__libc_stack_end`; each initialised node joins an init log; `fini_all` runs the log backwards before the
   start-time objects. Failure before publication: unmap reservations and file maps, adds += k and subs += k, rewind
   the arena, set the text.
6. *Not in DL-16a:* vdso node in the chain and `dl_iterate_phdr` over it (DL-16c), dlsym (DL-16b), unload (DL-17),
   `PT_TLS` objects (DL-16d), lazy binding (DL-20). Start-time nodes get no deps lists (their closure is global).

**Open (spec gaps, no probe yet; one small probe settles them):** text for a long non-ELF file (chosen `invalid ELF
header`), for other malformed headers (`malformed ELF object`), for an object without `PT_DYNAMIC`, for `EI_CLASS`
values other than 1/2; whether a NOLOAD miss clears a pending text (chosen: clears).

**Still owed by the story:** the driver and libc parts above; differences rows + registered compat options for
rulings #5125 (no IFUNC resolver in a failing `dlopen`) and #5126 (`ld.so` by any path is the start-time object):
records via `cargo xtask records new compat COMPAT-NEW` and pending decisions `records new decision D-NEW --series
D-27 --story DL-DLOPEN-LOAD`; tests `s27_1_c_1*`, `s27_1_c_5`, `s27_1_f_0` (every dynlink.md text, mode sweep over all
32 bits, static program), a unit test file for `liella_ld::dlopen`; the three failure plants (root unmap skipped,
dependency count restore — EQUIVALENT in this design: nothing is bumped on loaded objects — and publication before
pass 1 ends); CONF-DYN-HELPERS is handed back (queued for CI), so its pending dlopen programs can be flipped once it
lands. `state/PH14-OWNER-RULINGS.md` named by the brief does not exist; the brief's own summary of #5125/#5126/#5127
was used.

**Do not redo:** the survey (this note has its results), the pure module, the relocation split.

**Next step:** wait for this run's lint job, then write `dl.S`, the dlerror module, the libc exports, and the driver
per point 5; build with `cargo xtask so`; one C program + helper objects test under Liella's ld.so (see
`crates/ld/tests/reloc.rs` around line 853 for how a program is linked against Liella's libc and ld.so).

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §1-§4 and §8; spec/behaviour/dynlink.md; D-27.34.
