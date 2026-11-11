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
| program needing VER_1/VER_2 run against the unversioned stub | `no version information available (required by ./ver)` twice, then an internal consistency message from the loader, exit 127 |
| weak `w` in `libw1.so`, strong `w` in `libw2.so` | link order w1,w2 → the weak one; w2,w1 → the strong one; `LD_DYNAMIC_WEAK=1` → unchanged |
| undefined weak data and function references | both 0 |
| `STB_GNU_UNIQUE` `u_obj` in two objects, each `dlopen`ed `RTLD_LOCAL` | one object for both (the first's); a plain global `u_obj` in a third object stays its own |

Liella's lookup (D-27.11) follows every row; `crates/ld/tests/symbols.rs` checks the same corpus. The stub row is the
loader's version check, not the lookup: Liella's lookup matches any definition of an object without `versym`, and the
load-time version check (DL-10) refuses the load (exit 127, a message naming the object and the version; D-27.11).

## Notes for the implementation stories

- DL-16: the texts above are the targets; the `e_machine` case reports a file-missing text for
  an existing file (follow it, or record a difference with its gain).
- DL-17: `dlclose(NULL)` and an unknown handle: the reference crashes; returning non-zero with a
  dlerror text costs nothing and gains robustness (a difference row is needed).
- DL-30/R14-16: the version-not-found status 1 differs from every other load failure (127).
