# dynlink

**Spec:** §27 (dynamic linking: `dlopen`, `dlsym`, `dlvsym`, `dlerror`, `dlclose`, `dlinfo`,
`dladdr`, `dl_iterate_phdr`, the program interpreter).
**Clauses:** `s27_1_c_1`, `s27_1_c_1_1`, `s27_1_c_1_2`, `s27_1_c_1_3`, `s27_1_c_3`, `s27_1_c_4`,
`s27_1_c_5`, `s27_1_c_6`, `s27_1_c_7`, `s27_1_c_8`, `s27_1_g_0`, `s27_1_f_0`. **Plan:** phase 14 story DL-0b
(`DL-PROBE-BEHAVIOUR`, plan/06-phases/17-phase-14-dynamic-linking.md). Inputs to DL-11 (the order
corpus is its oracle), DL-12 (static-TLS surplus), DL-16 (dlerror texts), DL-17 (counters), DL-30.

**Probe:** `tools/probe/dl/run.sh SCRATCH` builds a corpus from `tools/probe/dl/dllib.c` (one
source, many shared objects chosen by `-D` options) and `tools/probe/dl/dlprobe.c` (the probe
program; built with `-DORDER_MAIN` it is the order program `dlorder`), runs every probe and prints
the record. **Record:** `dynlink.glibc.txt` (SCRATCH shown as `<dir>`). Recorded on Haswell-EP host (Ubuntu
24.04, glibc 2.39-0ubuntu8.9, gcc 13.3, x86_64) with `LC_ALL=C` and no `LD_*`/`GLIBC_TUNABLES` in
the environment. Each failure case runs in its own forked child. Every object prints with
`write(2)`, so the record shows execution order regardless of buffering. Black box only: the
reference loader and libraries are executed, and the placeholder libraries' exported names are
listed with `nm -D` (the public ABI). No source, disassembly or debugger was used.

## Results

**dlerror texts** (record `== errors`). The text is `<name as given or as found>: <reason>`:

| Failure | Text after `<path>: ` |
|---|---|
| file missing (by path, or by name after search) | `cannot open shared object file: No such file or directory` (the name as passed) |
| text file, empty file, 7-byte ELF prefix | `file too short` |
| directory | `cannot read file data: Is a directory` |
| truncated object (200 bytes, headers past end) | `cannot read file data` |
| `EI_CLASS` 32-bit | `wrong ELF class: ELFCLASS32` |
| `EI_DATA` big-endian | `ELF file data encoding not little-endian` |
| `EI_OSABI` 97 | `ELF file OS ABI invalid` |
| `e_machine` ARM | `cannot open shared object file: No such file or directory` (the *file-missing* text, although the file exists) |
| `ET_REL` object | `only ET_DYN and ET_EXEC can be loaded` |
| PIE executable | `cannot dynamically load position-independent executable` |
| non-PIE executable | `cannot dynamically load executable` |
| missing dependency | `libGone.so: cannot open shared object file: ...` (the dependency's DT_NEEDED name, not the opened file) |
| undefined symbol, `RTLD_NOW` | `undefined symbol: undefined_symbol_xyz` (prefix: the object that needs it) |
| version missing | `<dir>/libVer.so: version `VER_2' not found (required by <dir>/libW.so)` |
| mode 0 | `invalid mode for dlopen(): Invalid argument` |
| `dlsym` missing | `<object>: undefined symbol: <name>`; for `RTLD_DEFAULT`/`RTLD_NEXT` the object is the calling program |
| `dlvsym` missing version | `<object>: undefined symbol: <name>, version <version>` (also for an object without versions) |
| `dlinfo` request 9999 | returns -1, `unsupported dlinfo request` (no object prefix) |
| non-ELF file of 64 bytes or more (64, 4096, 65536 bytes of text; 4096 zero bytes) | `invalid ELF header` |
| `EI_CLASS` 0, 3 or 255 | `wrong ELF class: ELFCLASS32` (the 32-bit text for every class but 64-bit) |
| `EI_VERSION` 0 | `ELF file version ident does not match current one` |
| `e_version` 0 | `ELF file version does not match current one` |
| `e_phentsize` 32 | `ELF file's phentsize not the expected size` |
| `e_phnum` 0, or every `PT_LOAD` retyped `PT_NULL` | `object file has no loadable segments` |
| `PT_DYNAMIC` retyped `PT_NULL` | `object file has no dynamic section` |
| a `PT_LOAD` whose `p_vaddr` and `p_offset` differ modulo the page | `ELF load command address/offset not page-aligned` |
| `e_ehsize` 0; a `PT_LOAD` with `p_align` 3 | loads (Liella refuses both as malformed headers: `malformed ELF object`, its reader's rule) |

The rows after `dlinfo` are the gaps probe (`tools/probe/dl/gaps/run.sh SCRATCH`, record `== gaps` and
`== noload-pending`, ruling #5266, DL-DLOPEN-LOAD-2; same host and settings as above). It also settles
that an `RTLD_NOLOAD` miss clears a pending dlerror text (`dlerror()` is NULL after a failed open then
a NOLOAD miss), as a successful open and a NOLOAD hit do. Liella's dlopen uses these texts
(`liella_ld::dlopen::Refusal`) and checks the header, then the loadable segments, then the dynamic
section, in that order. Liella uses the misaligned-segment text for every malformed `PT_LOAD` layout
its planner refuses (`map::MapError::BadSegments`), a choice the probe does not decide.

Other calls: `dlopen("")` and `dlopen(NULL)` return a handle (the program); `RTLD_NOW|RTLD_LAZY`
is accepted; `RTLD_NOLOAD` of an unloaded object returns NULL with **no** dlerror text; an object
with an undefined function opens with `RTLD_LAZY`, `dlsym` finds the function that would fail,
and a later `RTLD_NOW` (also `RTLD_NOW|RTLD_NOLOAD`) open of the same object succeeds. After
every error, a second `dlerror()` returns NULL. Opening an object twice returns the same handle;
its destructor runs at the second `dlclose`. **`dlclose(NULL)` kills the process with SIGSEGV.**

**dl_iterate_phdr** (record `== phdr`). At start: 4 objects in the order program, vdso, libc,
loader (program name `""`, libc holds TLS module 1); `dlpi_adds` 4, `dlpi_subs` 0. `adds` counts
every object mapped (a second open of the same object does not count), `subs` every object
unmapped (`dlclose` of A unmaps A and B, +2, while C is still held). A `dlopen` refused at the
header (`text.so`) changes neither; one that maps the object and then fails on a dependency
(`libMissingDep.so`) bumps both by one. `RTLD_NODELETE` objects stay in the list after `dlclose`.

**Static-TLS surplus** (record `== tls`): with default tunables, one `dlopen` of an object with an
initial-exec TLS block of 1664 bytes succeeds and 1728 fails (64-byte steps); thirteen distinct
128-byte blocks load one after another and the fourteenth fails. The text is
`<path>: cannot allocate memory in static TLS block`.

**`$LIB`, `$PLATFORM`, `$ORIGIN`** (record `== subst`): in a DT_RUNPATH, `$LIB` and `${LIB}`
expand to `lib/x86_64-linux-gnu`, `$PLATFORM` to `haswell` on Haswell-EP host (a CPU-dependent value),
`$ORIGIN` to the object's directory. In a `dlopen` argument the tokens are not expanded (the
dlerror text shows them literally); `dlopen("$ORIGIN/libC.so")` from the program does expand
`$ORIGIN` (to the program's directory) and succeeds.

**Placeholder libraries** (record `== compat`): `libdl.so.2`, `libpthread.so.0`, `librt.so.1`,
`libutil.so.1`, `libanl.so.1` load; each exports only `__lib<name>_version_placeholder` under
each of its version nodes, and every probed function resolves through it to `libc.so.6`.
`libthread_db.so.1` does not load in a program without the `ps_*` callbacks (`undefined symbol:
ps_pdwrite`).

**Order** (record `== order`, DL-11's oracle). `dlorder` is linked `-lA -lD`, A needs B and C, D
needs C; D has DT_INIT/DT_FINI and a constructor; A has constructors and destructors with
priorities 200 and 300. Start: `preinit_array main`, C, B, `DT_INIT D`, `ctor D`, A (`ctor200`,
`ctor300`, then the plain constructor), the program's constructor and `init_array`, then `main`.
`dlopen` of E (needs C, F): F then E. `dlclose` E: `dtor E`, then the atexit handler E's
constructor registered, then `dtor F`. `exit` with E still open: atexit handlers in reverse
(including E's), the program's `fini_array` and destructor, then A, D (destructor, then
DT_FINI), B, E, C, F. `_exit`: nothing. A dependency cycle H <-> I opened by `dlopen("libH")`:
`ctor I`, `ctor H`; `dlclose`: `dtor H`, `dtor I`.

**Order without a C library** (tools/probe/dl/order.sh, DL-11, D-27.19). `-nostdlib` objects print what
runs; each program's `_start` calls the termination function it gets in `%rdx`. The loader runs the
program's preinit array but not its constructors or `init_array` (its C library's start does), and
does not run a shared object's `DT_PREINIT_ARRAY` (made by retagging `DT_INIT_ARRAY`). Start order:
`main -> R, Q, P; P -> R`: R, P, Q. `main -> L, K; L -> N; N -> K; K -> M`: M, K, N, L. A cycle at
start, `main -> H; H <-> I`: H, I. `main -> X; X -> Y; Y -> Z, W; Z -> X`: W, Y, X, Z. The
reference's termination function cannot be called without its C library (an internal assertion,
status 127), so the exit order comes from the same programs linked with it: the program's
`fini_array` and destructor, then the objects in the reverse of the start order (H <-> I: I, H;
cycle of three: Z, X, Y, W). Rule that fits every case here and DL-0b's: a depth-first walk over the
objects in reverse load order, each after the objects it needs, a cycle member already being walked
passed over, the root (program, or the object opened) last; at exit the same walk over every loaded
object, reversed.

**Loader exit status and messages** (record `== loader`). Missing DT_NEEDED: 127, `<prog>: error
while loading shared libraries: <name>: cannot open shared object file: No such file or
directory`. Undefined function: 127 `<prog>: symbol lookup error: <object>: undefined symbol:
<name>`, at the call with lazy binding, before `main` with `-z now` or `LD_BIND_NOW=1`. **A missing
symbol version exits 1, not 127** (`<prog>: <lib>: version `VER_2' not found (required by
<prog>)`). `ld.so` with no program: 1, `missing program name` and a `--help` hint; unknown option
1; nonexistent, text file, directory, wrong class: 127 with the dlerror texts above; a shared
library with no entry point run through `ld.so`: SIGSEGV (139). `--verify`: 0 dynamic program, 1
static program or non-ELF, 2 shared library. `--list` of a program with a missing dependency:
127. `--version`, `--help`: 0.

**BIND_NOW share** (record `== bindnow`): of 681 regular ELF64 files in Haswell-EP host's `/usr/bin`, 1 is
static, 666 dynamic with BIND_NOW (DT_FLAGS `BIND_NOW` or DT_FLAGS_1 `NOW`), 14 lazy; 667 are
`ET_DYN`. Lazy binding is the rare case for system programs (input to R14-13 / DL-10).

**Symbol binding** (DL-SYMBOLS probe, Haswell-EP host, same system; scratch probe of `lib.c`/`prog.c` built
with gcc 13.3 / binutils, black box: only the values the bound functions return were observed). `libv.so` defines
`f@VER_1` (hidden) and `f@@VER_2`; `h@VER_1`, `h@VER_2` (both hidden); `b` (base, index 1); `g@@VER_1`, `g@VER_2`
(hidden, and before `g@@VER_1` in the symbol table); `m@VER_1`, `m@VER_2` (hidden), `m@@VER_3`. The same answers with
`--hash-style=gnu` and `--hash-style=sysv`:

| Request | Binds |
|---|---|
| program linked against an unversioned stub (references without a version) | `f`→VER_1, `h`→VER_1, `b`→base, `g`→VER_1, `m`→VER_1 (lowest version index, hidden or not) |
| program linked against `libv.so` | `f`→VER_2 (the default at link time), `f@VER_1`→VER_1, `h@VER_2`→VER_2 |
| `dlsym` | `f`→VER_2, `h`→NULL (`undefined symbol: h`), `b`→base, `g`→VER_1, `m`→VER_3 (base or default only) |
| `dlvsym` | `f`/VER_1, `f`/VER_2, `h`/VER_1, `h`/VER_2 bind that version; `f`/VER_9, `b`/VER_1, `b`/`libv.so` (the base node's name) → NULL (`undefined symbol: f, version VER_9`) |
| an unversioned (base, index 1) `f` earlier in the scope, against `libv.so`'s own `f@@VER_1` reference (`JUMP_SLOT`) (DL-LIBC-START-6 probe, Zen 1 host, glibc 2.39) | in the program (exported; the object has `versym`) → the program's; in an `LD_PRELOAD` object with `versym` → the preloaded one; a program's needed `f@VER_1` (`vna_other`) against the same preload → the preloaded one (a versioned reference accepts a base definition; the program's `__fpu_control` reaches libc this way) |
| program needing VER_1/VER_2 run against the unversioned stub | `no version information available (required by ./ver)` twice, then an internal consistency message from the loader, exit 127 |
| weak `w` in `libw1.so`, strong `w` in `libw2.so` | link order w1,w2 → the weak one; w2,w1 → the strong one; `LD_DYNAMIC_WEAK=1` → unchanged |
| undefined weak data and function references | both 0 |
| `STB_GNU_UNIQUE` `u_obj` in two objects, each `dlopen`ed `RTLD_LOCAL` | one object for both (the first's); a plain global `u_obj` in a third object stays its own |

Liella's lookup (D-27.11) follows every row; `crates/ld/tests/symbols.rs` checks the same corpus. The stub row is the
loader's version check, not the lookup: Liella's lookup matches any definition of an object without `versym`, and the
load-time version check (DL-10) refuses the load (exit 127, a message naming the object and the version; D-27.11).

## dlopen and dlsym probes P-1..P-13 (DL-16P)

**Probe:** `tools/probe/dl/dlopen/run.sh SCRATCH` (DL-16P `DL-DLOPEN-PROBES`, 2026-11-12) builds a corpus from
`tools/probe/dl/dlopen/lib.c` (one source, many shared objects chosen by `-D` options) and
`tools/probe/dl/dlopen/probe.c` (`probe pN DIR`, one function per probe), and prints the record. **Record:** the
`== P-n` sections at the end of `dynlink.glibc.txt` (DL-16P record header; SCRATCH shown as `<dir>`). Same reference
and conditions as above (Haswell-EP host, Ubuntu 24.04, glibc 2.39-0ubuntu8.9, gcc 13.3, `LC_ALL=C`, no `LD_*`).
Each case runs in its own forked child (`alarm(30)` as a hang guard; none fired). P-8 and P-9 record "blocked" when a
call has not answered within 5 s, then release the waiting constructor, so nothing is left waiting; "blocked" is a
probe observation, never a test verdict. Black box only. The probes cover clauses `s27_1_c_1`, `s27_1_c_3`,
`s27_1_c_5`, `s27_1_c_8`, `s27_1_g_0` and `s27_1_f_0`; the questions come from plan/ph14/dl-16-dlopen.md §6, and the
last column says how each answer decides the plan's choice.

| Probe | Program case | Observed in the reference | Decides |
|---|---|---|---|
| P-1 binding (`s27_1_c_1`) | `probe p1`: `RTLD_NOW\|RTLD_LAZY` on `libU1` (undefined function) | the open succeeds (constructor runs); `RTLD_NOW` alone on the same object fails `undefined symbol: undefined_symbol_xyz` | both bits set = lazy binding. Before DL-20 Liella binds eagerly (plan §1), so this is DL-20's interim gap, like `RTLD_LAZY` |
| P-1 unknown bits (`s27_1_c_1`, `s27_1_f_0`) | `probe p1` sweep: every bit outside `0x110f` and outside the eight glibc-internal high bits `0x01000000`-`0x80000000` (17 bits; the raw lines for `0x01000000`-`0x20000000` and `0x80000000` in `dynlink.glibc.txt` predate the exclusion and are kept as they are) with `RTLD_NOW` on `libC1` and `RTLD_LAZY` on `libU1` | every swept bit: NULL, dlerror `invalid mode parameter` (no object prefix), checked before the file is looked for (also for a missing file); an unknown bit with no binding mode gives the same text, not mode 0's | Liella refuses every unknown mode bit (every bit outside `RTLD_LAZY\|RTLD_NOW\|RTLD_NOLOAD\|RTLD_DEEPBIND\|RTLD_GLOBAL\|RTLD_NODELETE`) with `invalid mode parameter`, before the search. The eight high bits `0x01000000`, `0x02000000`, `0x04000000`, `0x08000000`, `0x10000000`, `0x20000000`, `0x40000000` and `0x80000000` are internal, undocumented flags of glibc (some used for its profiling; confirmed by glibc's maintainers, 2026-11-13): reserved for Liella (Liella owes glibc's internal meaning of them nothing; refused like any unknown bit with `invalid mode parameter`; kept available for a possible future Liella-defined use, none planned, any use a new public-interface decision), and excluded from the reference comparison (the probe sweep skips all eight). DL-16a's test sweeps every undefined bit, these included. D-27 pending record `DLOPEN-INTERNAL-BITS-1` |
| P-2 identity (`s27_1_c_1`) | `probe p2`: `id/libId.so.1` (`DT_SONAME libId.so.1`), symlink `id/sym.so`, hard link `id/hard.so`, `id/../id/…`, `./libId.so.1`, a copy `id2/libId.so.1` | symlink, hard link, `..` path, `./` path and the bare `DT_SONAME`: the same handle (constructor once), and `l_name` stays the first name used (the hard link's path when it came first). The bare `DT_SONAME` finds the loaded object although no search path holds the file; before any load it fails with the file-missing text, and `NOLOAD` gives NULL. A copy (another inode, same `DT_SONAME`) opened by path is a second object. When the copy is loaded first, the bare `DT_SONAME` returns the copy. A `DT_NEEDED libId.so.1` whose `DT_RUNPATH` holds only the copy binds to the already-loaded original. `NOLOAD` by the bare file name `hard.so` gives NULL | identity = `(st_dev, st_ino)` of the found file, plus a `DT_SONAME` match of a bare name (no `/`) against loaded objects before any search, for `dlopen` and `DT_NEEDED` alike (plan §1 as written). A bare name is never matched against file names. `l_name` keeps the name of the first open |
| P-3 `RTLD_GLOBAL` (`s27_1_c_1`, `s27_1_g_0`) | `probe p3`: `libA3` needs `libB3`; `libZ3` references `fn_A3` and `fn_B3` with no `DT_NEEDED` | `RTLD_GLOBAL` puts the root **and its dependency** in the global scope (`RTLD_DEFAULT` finds both; `libZ3` then opens `RTLD_NOW`). After an `RTLD_LOCAL` open neither is found. A `NOLOAD\|GLOBAL` re-open, and a plain `NOW\|GLOBAL` re-open of a lazily opened root, return the same handle and promote both. `NOLOAD\|GLOBAL` of the dependency alone promotes only it (`fn_A3` still NULL) | `RTLD_GLOBAL` (first open or promotion) appends the opened object's whole local scope (itself and its dependencies, in breadth-first order, the ones not yet global) to the global scope. Promoting a dependency promotes only its own local scope |
| P-4 sibling scope (`s27_1_g_0`) | `probe p4`: `libA4` needs `libB4` then `libC4`; `libB4` calls `fn_C4` (sibling) and `fn_A4` (root), linked with neither | both bind (`C4`, `A4`) under `RTLD_NOW` and `RTLD_LAZY`; constructors C4, B4, A4 | a new object is relocated against [global scope, the root's local scope] (plan §2 as written) |
| P-5 `RTLD_NEXT`/`RTLD_DEFAULT` (`s27_1_c_3`) | `probe p5`: `who()` in the program, `libL5`, `libM5`, `libG5`; `libL5` needs `libM5`; each object calls `dlsym` itself | from `libL5` (`RTLD_LOCAL`): `RTLD_NEXT` `who` gives `M5`'s and `puts` gives libc's; its own `fn_L5` and the program's `who` are not found. `RTLD_DEFAULT` `who` gives the program's, and `fn_M5`/`fn_L5` (local only) are found. From `libG5` opened `RTLD_GLOBAL`, the last object of the global scope: `RTLD_NEXT` finds `puts` (libc is its dependency) and nothing defined only in earlier objects. `libL5` opened `RTLD_GLOBAL` answers as when local. From the program, `RTLD_NEXT` `who` reaches a `GLOBAL` object (`G5`, `L5`) and nothing local. Every failure text names the **calling object** (`<dir>/libL5.so: undefined symbol: …`) | `RTLD_NEXT` = the objects after the caller in the scope the caller was loaded with (the global scope for start-time objects; the root's local scope for a `dlopen`'d object), never NULL just because the caller is last in the global scope. `RTLD_DEFAULT` from a `dlopen`'d object = the global scope, then the caller's local scope (the plan's "the global scope" is widened by this). The failure text names the caller's object |
| P-6 dlerror (`s27_1_c_5`) | `probe p6`: a failed `dlopen`, then a successful `dlopen`/`dlsym`/`dlinfo`, then `dlerror()`; threads | any later **successful** `dl*` call clears the pending text (`dlerror()` gives NULL). A failure followed by a failed `dlsym` reports the second; the next call gives NULL. A failure in thread A is invisible to the main thread and to a thread created later, and A still reads its own text afterwards. A thread that failed and exited leaves nothing behind | per-thread text (plan §4); every `dl*` entry point clears the calling thread's pending text when it succeeds, so the text describes the thread's most recent `dl*` call. This replaces the plan's "until the next failure" |
| P-7 recursive `dlopen` (`s27_1_c_1`) | `probe p7`: `libA7`'s constructor `dlopen`s `libB7` (`DT_NEEDED libA7.so`), or itself (`NOLOAD`, then plain) | order: `ctor A7 begin`, `ctor B7`, B7's constructor calls `fn_A7` (works), A7's constructor continues and ends; A7's constructor runs once (depth 1). The inner `dlopen`s of A7 return the outer handle; the inner handle of B7 is B7's | an inner `dlopen` that reaches an `Initialising` object returns its handle without running its constructors again; the inner set's own new objects initialise before the outer constructor resumes (plan §3 as written) |
| P-8 readers during a constructor (`s27_1_c_3`, `s27_1_c_8`) | `probe p8`: thread A's `dlopen(libA8)` runs a constructor that waits on a pipe for thread B | `dlsym(RTLD_DEFAULT, puts)`, `dlsym(RTLD_DEFAULT)` of the object being initialised (opened `GLOBAL`), `dlopen(libc.so.6, NOLOAD)` and `dlopen` of a new object: **blocked** until the constructor returns. `dl_iterate_phdr`: returns during the constructor and already lists `libA8` | Liella's readers (`dlsym`, `dl_iterate_phdr`) take no lock and never block (plan §4, D-27.1). `dlopen` in another thread waits for the writer lock, as in the reference. The object being initialised is published before its constructors run (the reference's `dl_iterate_phdr` agrees) |
| P-9 `fork` during `dlopen` (`s27_1_c_1`) | `probe p9`: `fork` while thread T is inside `libA9`'s constructor; the child calls `dl_iterate_phdr`, `dlsym`, `dlopen(libA9, NOLOAD)`, `dlopen(libC1)` (three orders) | `fork` returns at once. In the child every call returns: 5 objects listed including `libA9`, `dlsym` finds, `NOLOAD` of `libA9` (whose constructor never completes in the child) returns a handle, and a new `dlopen` succeeds and runs its constructor | the child can use the loader after a fork made during another thread's `dlopen` (plan §4's prepare/child step). The child's view of the in-progress set (listed, `NOLOAD` succeeds) is recorded; DL-16c chooses whether the child keeps the half-initialised objects |
| P-10 IFUNC on a failing `dlopen` (`s27_1_f_0`) | `probe p10`: `libI10` defines IFUNC `ifn` (resolver prints); `libU10` references an undefined function; roots `R10a` (I10 then U10), `R10b` (U10 then I10), `R10c` (own IFUNC, needs U10) | `RTLD_NOW`: all three fail with `<dir>/libU10.so: undefined symbol: undefined_symbol_xyz`, but for `R10b` **the resolver of `libI10` ran first**. For `R10a`/`R10c` it did not, and no constructor ran in any. `RTLD_LAZY`: all open, constructors run, no resolver at load | the reference runs foreign code (an IFUNC resolver of an object processed before the failing one) in a `dlopen` that fails. Liella's two passes (plan §2, pending decision 2) never do. Whether that becomes a differences row is open (maintainer ruling #5125) |
| P-11 surplus TLS in older threads (`s27_1_c_1`) | `probe p11`: `libT11` with `tv_ie` (initial-exec, 42), `tv_gd` (7), `tv_bss`; a thread created before the `dlopen`, one after | every thread sees 42 / 7 / 0, including the thread created before the `dlopen`. A write in one thread is invisible to the others, and `dlsym(tv_ie)` is the calling thread's own address | DL-16d initialises the surplus block of every live thread from the module's image (plan §5 point 1) |
| P-12 link maps (`s27_1_c_1`) | `probe p12`: `dlinfo(RTLD_DI_LINKMAP)` | the link map **equals** the handle for `dlopen(NULL)`, `dlopen("")`, a path, a relative `./libC1b.so` and `libc.so.6`. The program's `l_name` is `""` and its `l_prev` is NULL. `l_name` is the name as opened (`./libC1b.so` stays relative). The chain by `l_next`: program, `linux-vdso.so.1`, libc, `ld.so`, then the `dlopen`'d objects in load order | handle = link map (plan §1 as written). `l_name` keeps the caller's spelling of a path |
| P-13 program, `ld.so`, libc (`s27_1_c_1`, `s27_1_f_0`) | `probe p13` (PIE build and `-no-pie` build) | the program by its own path or as `/proc/self/exe`: NULL, `cannot dynamically load position-independent executable` (PIE) or `cannot dynamically load executable` (non-PIE). `NOLOAD`: NULL, no text. `ld.so` by name and by `/lib64/ld-linux-x86-64.so.2`: the start-time handle; by its real path `/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2`: **a different handle** (a second object with that `l_name`). libc by name, `NOLOAD`, real path and a `..` path: the start-time handle | the program is refused by path like any executable (identity does not match it). `libc.so.6` by any name is the start-time object. For `ld.so`, Liella's `(st_dev, st_ino)` identity returns the start-time object for every path. That differs from the reference (one loader per process); whether that becomes a differences row is open (maintainer ruling #5126) |

## The link-time archive libc_nonshared.a (DL-LIBC-NONSHARED, D-27.28)

Clauses `s1_2_c_1`, `s1_2_c_3`, `s1_2_c_5`. Black box only, on the Zen 1 build host (glibc 2.39, gcc 13): `ar t` and
`readelf -sW` of the installed archive, the installed `libc.so` script, `nm` of linked programs, and program behaviour
(DL-NONSHARED-OPTOUT's probes P1-P2 and this story's `__cxa_finalize` probe). No source or disassembly.

| member | defines (all hidden) | reaches in libc.so.6 | how settled |
|---|---|---|---|
| `atexit` | `atexit` | `__cxa_atexit@@GLIBC_2.2.5` | symbol listing; libc.so.6 exports no `atexit`; a linked program has `atexit` local (`t`) |
| `at_quick_exit` | `at_quick_exit` | `__cxa_at_quick_exit@@GLIBC_2.10` | symbol listing; libc.so.6 exports no `at_quick_exit` |
| `pthread_atfork` | `__pthread_atfork`, `pthread_atfork` (weak, same address) | `__register_atfork@@GLIBC_2.3.2` | symbol listing; libc.so.6 has `pthread_atfork` only as the non-default `@GLIBC_2.2.5`, so a new link takes the member |
| `stack_chk_fail_local` | `__stack_chk_fail_local` | `__stack_chk_fail@@GLIBC_2.4` | symbol listing |

All members reference `__dso_handle` (undefined, hidden: the compiler's start files define it).

* **The tag is the VALUE of `__dso_handle`.** In a `-no-pie` program the value is 0, and its address is not. There,
  `__cxa_finalize(&__dso_handle)` leaves the `atexit` handler, the `at_quick_exit` handler and the atfork handler
  registered (the quick handler still runs at `quick_exit`). `__cxa_finalize(0)` runs the handler. In a PIE the value is
  the address, and `__cxa_finalize(__dso_handle)` runs the module's `atexit` handler, then drops its `at_quick_exit`
  handler AND its atfork handler (a later `fork` does not call it).
* **At `dlclose`** (P2): a shared object's own `atexit` handler runs when it is unloaded. Its `at_quick_exit` and
  `pthread_atfork` handlers are dropped, and a later `quick_exit` or `fork` calls neither.
* **The installed `libc.so`** is `GROUP ( <libc.so.6> <libc_nonshared.a> AS_NEEDED ( <ld-linux-x86-64.so.2> ) )`.
  The reference names absolute paths. Liella's names bare files, which ld finds on the library search path.

## Notes for the implementation stories

- DL-16: the texts above are the targets; the `e_machine` case reports a file-missing text for
  an existing file (follow it, or record a difference with its gain).
- DL-17: `dlclose(NULL)` and an unknown handle: the reference crashes; returning non-zero with a
  dlerror text costs nothing and gains robustness (a difference row is needed).
- DL-30/R14-16: the version-not-found status 1 differs from every other load failure (127).
