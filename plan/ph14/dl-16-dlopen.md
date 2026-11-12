# DL-16 plan: `dlopen`, `dlsym`, `dlvsym`, `dlerror` in Liella's ld.so (DL-16-PLAN, 2026-11-12)

<!-- Sources: dlopen(3), dlsym(3), dlerror(3), dlinfo(3), ld.so(8), <dlfcn.h>, <link.h>, the ELF gABI, the
     x86-64 psABI, the GNU symbol-versioning document, and black-box probes of the reference
     (spec/behaviour/dynlink.md; the probe notes of DLOPEN-FAILURE-PATHS and DLSYM-SPECIAL-HANDLES). No loader source. -->

Status: PROPOSED (planning story). It refines item 20 of `plan/06-phases/17-phase-14-dynamic-linking.md` and
`plan/ph14/design.md` §5, §6, §8, §9, §10; where it is more precise it says so. Pending decisions:
`records/pending/DL-16-PLAN-1.toml` (object list, lock, visibility), `-2` (a failed `dlopen` leaves no trace),
`-3` (handles and counts; ruling #5006), `-4` (static programs deferred; ruling #5005) and the registered compat
placeholder `-5`. DLOPEN-FAILURE-PATHS and DLSYM-SPECIAL-HANDLES are folded in (#2629, #2634, #2637, #2643):
their probes (`tools/probe/dl/run.sh`, `tools/probe/dl/handles/`) become DL-16's conformance programs.

**What DL-16 builds on.** DL-7 `liella_ld::map` (D-27.15: reserve, map, unmap the reservation on failure), DL-8
`liella_ld::search` + `cache` (D-27.16: search order, secure mode, the cache), DL-9 `liella_ld::symbols` (D-27.11:
GNU/SysV hash, versions, `Scope`, the probe table of `dynlink.md`), DL-10 `liella_ld::reloc` (D-27.17, D-27.18: eager
relocation, IFUNC after the non-IFUNC relocations of an object, RELRO last), DL-11 `liella_ld::init` (D-27.19:
`init_order`/`fini_order` over a set), DL-12 `liella_ld::tls` (D-27.20: variant II layout with the 64 KiB surplus,
R14-4; `fill_dtv`, `lookup`), DL-13 (D-27.22: `LIELLA_PRIVATE` with the build-tag check), and the boundary driver
`crates/boundary/src/ld.rs` (`load`, `link`, `run_init`). All of these run once, at start, over the initial set.
DL-16 runs them again, later, under a lock, over a *new set*, with a way back on failure.

## 1. The object list, namespaces, counts and modes

**What the ABI requires.** `<link.h>`'s `struct link_map` prefix (`l_addr`, `l_name`, `l_ld`, `l_next`, `l_prev`) is
public: programs cast a `dlopen` handle to it and walk `l_next`, and `dlinfo(RTLD_DI_LINKMAP)` (DL-18) returns it.
So a handle is the object's link map (design §10), the main program's `l_name` is `""` (probe `== phdr`), and the
private fields follow the prefix. Namespaces are ABI too (`dlmopen@GLIBC_2.3.4`, `Lmid_t`, `LM_ID_BASE` 0,
`LM_ID_NEWLM` -1, `RTLD_DI_LMID`), but nothing in DL-16 needs a second one: **DL-16 has `LM_ID_BASE` only and does
not export `dlmopen`** (DL-22 does, R14-11). Room for DL-22: the object chain, the global scope, the unique-symbol
table and the TLS module table are fields of one `Namespace` value (one instance now), and every object records its
namespace id (always 0 in DL-16).

**The list.** One chain of link maps in load order (program, its dependencies as loaded at start, then each
`dlopen`'s new objects), linked through `l_next`/`l_prev`. In DL-16 nothing is ever removed, so the chain is
**append-only**: a new node is fully built, then made reachable with one Release store to the tail's `l_next`.
Readers walk it without a lock (Acquire loads). No reclamation is needed until DL-17, and no array is copied per
`dlopen` (a per-`dlopen` snapshot array would leak O(n^2) memory without DL-17's grace periods). The global scope
and the address index (§4) are chunked append-only arrays for the same reason: chunks never move, a published
length says how much is valid.

**Per-object state** (private fields): `state` (`Mapped`, `Relocated`, `Initialising`, `Ready`; DL-17 adds
`Finalising`), `opens` (direct `dlopen` calls holding it), `deps` (its resolved `DT_NEEDED` objects) and `local`
(its breadth-first local scope, itself first), `global` (in the global scope), `nodelete` (`RTLD_NODELETE`,
`DF_1_NODELETE`, a bound `STB_GNU_UNIQUE` definition, an initial object, or `opens` saturated), `dev`/`ino` of the
file it came from, its TLS module id (0 = none).

**Counts.** `dlopen` of a new object sets `opens = 1`; a re-open (including `RTLD_NOLOAD`) adds one. DL-16 never
decrements; it records what DL-17 needs (`opens`, `deps`, `nodelete`). Liveness for DL-17 is reachability: an
object stays while it is `nodelete`, initial, or reachable through `deps` from an object with `opens > 0` (probe:
`dlclose` of A unmaps A and B while C is still held; the second `dlclose` of a twice-opened object runs its
destructor). `opens` saturates at `u32::MAX` and the object becomes `nodelete` (a wrapped count could unmap a live
object in DL-17; pending decision 3).

**Modes** (`<dlfcn.h>` values). Exactly one binding mode is required: mode 0 fails with `invalid mode for dlopen():
Invalid argument` (probed); `RTLD_NOW|RTLD_LAZY` is accepted (probed; which binding it gets is probe P-1).
`RTLD_LOCAL` (default): the new objects join no global scope. `RTLD_GLOBAL`: the opened object (and, by probe P-3,
possibly its dependencies) is appended to the global scope; a re-open with `RTLD_GLOBAL` promotes an `RTLD_LOCAL`
object. `RTLD_NOLOAD`: never maps; returns the existing handle (count +1, `GLOBAL`/`NODELETE` promotions applied) or
NULL with **no** dlerror text (probed). `RTLD_NODELETE`: sets `nodelete`. `RTLD_DEEPBIND`: the new set's lookup scope
is its local scope first, then the global scope (design §5). `RTLD_LAZY` before DL-20: functions are bound eagerly
like `RTLD_NOW` (same answers for every symbol that is defined at load time, because the global scope only grows by
appending); an undefined *function* reference therefore fails the open until DL-20 brings the PLT trampoline and late
definitions (probed reference: the open succeeds). This is an interim gap of the phase, listed in DL-20's tests, not a
shipped difference. Unknown mode bits: probe P-1 decides (follow the reference if it refuses them with a text).
A re-open never re-relocates (probed: an `RTLD_NOW` re-open of an object first opened lazily succeeds).

**Finding the object and its dependencies** (DL-8, D-27.16). `dlopen("")`/`dlopen(NULL)` return the program's
handle (probed). A name with a `/` is opened as given; otherwise the D-27.16 search runs with the **calling object**
as the requester (its `DT_RPATH` when it has no `DT_RUNPATH`, the program's `DT_RPATH` likewise, `LD_LIBRARY_PATH`
outside secure mode, the caller's `DT_RUNPATH`, the cache, the default directories; dlopen(3), ld.so(8)). Each new
object's own `DT_NEEDED` entries are searched with that object as the requester. The secure-mode rules of the threat
model (B/C, R14-15) apply unchanged. `$ORIGIN`/`$LIB`/`$PLATFORM` in names follow the `== subst` record of
`dynlink.md`. The caller is the object containing `dlopen`'s return address (§4 address index); the address is
captured by the exported entry in `libc.so.6` (a C-layer shim or a `liella_machine_*` primitive: **seam**, per arch)
and passed to `ld.so` through `LIELLA_PRIVATE`. Identity: before mapping, a found file's `(st_dev, st_ino)` is
compared with every loaded object (and `DT_SONAME`/name matches skip the search: probe P-2), so one file is never
mapped twice. An incompatible candidate (class, data, OS ABI, machine) is skipped and the search goes on; when no
candidate is left the text is the file-missing one (probed for `e_machine`, also with an explicit path): Liella follows
this, one rule for both cases.

## 2. Relocation scope, `dlsym`, `dlvsym`

**Scopes.** The *global scope* is the program, its start-time dependencies in load order, then `RTLD_GLOBAL`
objects in the order they became global. A `dlopen`'d root's *local scope* is its breadth-first dependency list,
itself first. Every object newly loaded by one `dlopen` is relocated against [global scope, then the root's local
scope] (`RTLD_DEEPBIND`: local first), so a dependency B of root A sees symbols of A's other dependencies (probe P-4).
Objects that were already loaded keep the scope they were relocated with. `DT_SYMBOLIC`/`DF_SYMBOLIC` puts the object
itself first (as at start, D-27.17). Versions, weak/unique/IFUNC/protected rules are D-27.11's; `STB_GNU_UNIQUE`
lookups go through the namespace's unique table (name + version -> first definition; probed: two `RTLD_LOCAL`
objects share the first one's `u_obj`), and binding one pins its definer.

**Relocation in two passes** (pending decision 2). Pass 1 resolves every symbol reference of the whole new set
(L14-7's memo by symbol index), checks versions (`VERNEED` against `VERDEF`) and places TLS; it may fail and runs no
foreign code. Pass 2 applies the relocations in D-27.17's order (dependencies first, IFUNC after the non-IFUNC
relocations of an object, RELRO last); it cannot fail. So no IFUNC resolver and no constructor ever runs in a `dlopen`
that fails (probe P-10 records whether the reference differs; if it does, a neutral differences row with that gain).

**`dlsym(handle, name)`**: the handle's local scope in order (handle = program: the global scope, probed). The
default-version rules are the `dynlink.md` table (`f`->VER_2, hidden `h`->NULL, `b`->base, `m`->VER_3).
**`RTLD_DEFAULT`**: the global scope. **`RTLD_NEXT`**: the objects after the caller's object in the global scope
(probed with helpers 1 and 2: helper 1 -> helper 2's `f`); from an `RTLD_LOCAL` object and from the last object is
probe P-5. **`dlvsym`**: exact version, hidden versions included, the base node's name is not a version (probed).
IFUNC definitions return the resolver's result; `STT_TLS` definitions return the calling thread's address (initial
modules through DL-12's static offsets in DL-16b; `dlopen`'d modules in DL-16d). Failures set the probed texts
(`<object>: undefined symbol: <name>[, version <v>]`; the program is the object for `RTLD_DEFAULT` and the NULL
handle, the caller for `RTLD_NEXT`). A handle that Liella did not return is refused with a dlerror text (pending decision 3, ruling #5006): the special
handles first, then one lookup in the address index or handle set, never a scan on the `dlsym` path.

## 3. Initialisers and failure

**Order.** After pass 2 the new set is published (§4), then `liella_ld::init::init_order` runs over the new objects
only (dependencies first, already-initialised objects skipped, cycles in load order; probed: `dlopen` of E needing C
and F, with C already loaded, runs F then E). Each object goes `Initialising` -> `Ready`. A constructor may call
`dlopen`, `dlsym`, `dl_iterate_phdr` (the writer lock is re-entrant for its owner, design §9); an inner `dlopen` of an
object that is `Initialising` returns its handle without running its constructors again (probe P-7 checks the
reference's order for this cycle).

**A failed `dlopen` leaves no trace** (pending decision 2; DLOPEN-FAILURE-PATHS). Every failure point is before the
first foreign code (§2). The way back, in reverse: TLS module ids and surplus ranges released; every new object's
reservation unmapped (D-27.15's unmap path) and its file descriptor closed; counts bumped on already-loaded objects
during the attempt restored; no chain, scope, unique-table or address-index entry published; loader-arena memory of
the attempt freed; one dlerror text set. Counters follow the probe: a refusal at the header changes neither
`dlpi_adds` nor `dlpi_subs`; an attempt that mapped k objects bumps both by k (the counters stay "objects ever mapped"
and "objects ever unmapped", so `adds - subs` is the live count). Plants (the three of the folded story, kept): skip
the unmap of the root on a missing dependency (the test counts mappings in `/proc/self/maps`); skip restoring a
dependency's count; publish the chain before pass 1 ends.

## 4. dlerror, locking, and room for DL-17

**dlerror** (D-27.2, design §10). Per thread: a pointer slot in Liella's private part of the thread block (the
`tcbhead_t` prefix is untouched, D-22.10) to a record in the loader's arena (R14-5, never the interposable `malloc`),
allocated at the thread's first failure and released by DL-15's thread-exit hook. The text is built at failure in the
probed shapes; `dlerror()` returns it once and the next call returns NULL (probed); the returned string stays valid
until the thread's next `dl*` failure or its exit (longer than dlopen(3) promises). Probe P-6 settles whether a later
*successful* call clears a pending text and that a failure in one thread is invisible in another.

**One writer lock** per namespace: a futex lock owned by `ld.so` (no libc calls), re-entrant for the owning thread.
`dlopen` (and later `dlclose`, `dlmopen`) hold it for the whole call. **Readers take no lock** (D-27.1):
`dl_iterate_phdr`, `_dl_find_object`, `dladdr` (DL-18), `dlsym`, the lazy resolver (DL-20) and `__tls_get_addr`'s slow
path read published state only, and every such read goes through one function, `read_section(|view| ...)`, which is
empty in DL-16 and gets DL-17's per-thread reader epochs. **Publication** (pending decision 1): one point per
`dlopen`, after pass 2 and before the constructors: the chain append, the global-scope append, the address index and
`dlpi_adds` together, so code that a constructor starts in another thread can unwind (`_dl_find_object`) and look up
(`dlsym`) the new objects. The `dlopen` call itself returns after the constructors. Probe P-8 records what the
reference does with a `dlsym` from another thread during a slow constructor (blocks or not); Liella's readers never
block, so a constructor that waits for a thread calling `dlsym` cannot deadlock. The s27_1_c_8 ruling (#2607) holds:
callbacks run without the lock, a callback may call `dlopen` (R14-6 differences row, DL-17).

**dl_iterate_phdr and _dl_find_object** in a dynamic program walk the chain from DL-16 on (the program, its
dependencies, `dlopen`'d objects, in load order); DL-17 swaps in the sorted address index with grace periods. The
address index in DL-16 is a chunked array of (start, end, object), searched linearly by `dlopen`/`dlsym` caller
lookup (rare calls); DL-17 sorts it.

**Lock order** (one rule, all stories): loader writer lock -> DL-15's thread-list lock -> the loader arena's lock ->
libc's internal locks (a constructor may call `malloc`, stdio, `pthread_create`). `fork` takes the loader lock first in
its prepare step and releases it in parent and child, so a child never inherits a lock held by a thread that does not
exist there (probe P-9: the reference's child can `dlopen` after a fork made during another thread's `dlopen`).

**Room for DL-17.** `opens`, `deps`, `nodelete`, `l_prev`, the `Finalising` state, `read_section`, the counters,
the chunked arrays (DL-17 replaces them with RCU snapshots) and DL-11's `fini_order` over a set. DL-16 adds no
`dlclose` export.

## 5. TLS of `dlopen`'d objects: what DL-16 needs from DL-15

DL-16d places a `dlopen`'d module's block in the static surplus (R14-4, first fit with the block's alignment); a
module that does not fit fails with `<path>: cannot allocate memory in static TLS block` (the probed exhaustion
text) until DL-19 adds dynamic blocks (R14-5). Before DL-16d, a `dlopen` of an object with `PT_TLS` fails with that
text (interim, like `RTLD_LAZY`). DL-16d needs from DL-15:

1. A **live-thread list** with its own lock, taken after the loader lock (§4). `pthread_create` initialises the new
   thread's static TLS from the module table *and* inserts the thread into the list under that lock; `dlopen` fills
   the new module's surplus block in every listed thread under the same lock before publication. Either order of a
   racing create and `dlopen` then initialises the block exactly once.
2. `__liella_tls_init(block)` that copies the images of a **growable, published module table** (not the start-time
   array), so threads created after a `dlopen` get its module.
3. The **dtv** contract: a generation word, capacity growth from the loader arena, and the rule that a surplus
   module's dtv entry is `TP - offset`, filled eagerly for live threads or lazily by `__tls_get_addr`'s slow path when
   the thread's generation is behind.
4. The TLSDESC static-return primitive (`liella_machine_ld_tlsdesc_static`) for surplus modules; the dynamic one
   stays with DL-19.
5. A thread-exit hook (`LIELLA_PRIVATE`) that releases the thread's loader state (the dlerror record now, dynamic
   blocks in DL-19).

If DL-15's design differs, DL-16d adapts to it; points 1 and 2 are the race that must be closed either way.

## 6. Stories, order, tests and probes

Clause tests name their clause (`s27_1_c_1*`, `c_3`, `c_5`, `f_0`, `g_0`, `c_4`; `s27_1_c_8`; `s27_3`). Every story
adds malformed-input cases, plants with `msg:` fields through `cargo xtask plants`, ticks the §0a seam checklist, and
compares with the reference by a probe in `tools/probe/dl/` recorded in `spec/behaviour/dynlink.md` + `.glibc.txt`.

| Story | Size | After | Builds | Proof |
|---|---|---|---|---|
| **DL-16P** `DL-DLOPEN-PROBES` | S | now | probes P-1..P-13 below, no product code | `dynlink.md` rows, each from a probe program |
| **DL-16H** `CONF-DYN-HELPERS` | S | DL-14 | conformance harness: a directive that builds helper objects and links the test dynamically (Liella's `libc.so.6` + `ld.so`, and the reference) | the two folded probes run as conformance programs on the reference; a plant that drops a helper is caught |
| **DL-16a** `DL-DLOPEN-LOAD` | M | DL-14, DL-16H | `dlopen`/`dlerror` exports (`libc.so.6` + `libdl.so.2` versions per `spec/`), caller capture, writer lock, append-only chain, new-set load (search, identity, map, version check, two-pass relocation, publication, init), all modes but `GLOBAL`/`DEEPBIND`, failure unwind, per-thread dlerror | `s27_1_c_1*`, `s27_1_c_5`, `s27_1_f_0`: every dlerror text of `dynlink.md`; mode matrix; failure leaves `/proc/self/maps` and counters as probed; 3 failure plants |
| **DL-16b** `DL-DLSYM` | M | DL-16a | `dlsym`/`dlvsym`, `RTLD_DEFAULT`/`RTLD_NEXT`, handle scopes, `RTLD_GLOBAL` (+ promotion), `RTLD_DEEPBIND`, unique table, handle validation, `STT_TLS` of initial modules | `s27_1_c_3`, `c_1_3`, `g_0`; `handles.c` adopted; version table rows; plants: `RTLD_NEXT` starting at the caller, `dlopen(NULL)` scope without the program, unique table bypassed |
| **DL-16c** `DL-DLOPEN-CONCURRENT` | M | DL-16a (DL-15 for the thread cases) | `dl_iterate_phdr`/`_dl_find_object` over the chain, `adds`/`subs`, `read_section`, recursive `dlopen` from constructors, `fork` prepare/child, lock-order rule | `s27_1_c_8`; constructor calls `dlopen`/`dlsym`/`malloc`/`printf`; unwinding a C++ exception through a `dlopen`'d object; concurrent `dlopen` + `dl_iterate_phdr` loop (iteration budget, no wall clock); plant: publish before pass 2 |
| **DL-16d** `DL-DLOPEN-TLS-SURPLUS` | M | DL-15, DL-16a | §5: surplus placement, module ids, live-thread init, dtv, `dlsym` of `STT_TLS` in `dlopen`'d modules | `s27_3` (surplus part; `s27_1_c_4`, the TLS pin, is DL-17's): GD/LD/IE/TLSDESC in a `dlopen`'d object, threads before and after; surplus exhaustion text; plant: skip one live thread |

Order: DL-16P now (parallel to DL-14); DL-16H and DL-16a after DL-14; then DL-16b ‖ DL-16c; DL-16d after DL-15
and DL-16a. DL-17 needs DL-16b and DL-16c; DL-18, DL-20 and DL-28 need DL-16b; DL-19 builds on DL-16d.

**Probes for DL-16P** (black box, reference only; each one line in `dynlink.glibc.txt`):
P-1 mode bits: `RTLD_NOW|RTLD_LAZY` on an object with an undefined function; unknown bits (0x10000, 0x80000000).
P-2 identity: the same file through a symlink, a hard link, `./name`, and by its `DT_SONAME`; same handle?
P-3 `RTLD_GLOBAL`: are the root's dependencies found by `RTLD_DEFAULT` afterwards; promotion by a `NOLOAD|GLOBAL`
re-open, and of the dependencies. P-4 a dependency B of a `dlopen`'d A binds a symbol only A's other dependency
defines. P-5 `RTLD_NEXT` and `RTLD_DEFAULT` called from an `RTLD_LOCAL` object; `RTLD_NEXT` from the last object.
P-6 dlerror: failure, success, then `dlerror()`; failure in thread A, `dlerror()` in thread B. P-7 a constructor of A
`dlopen`s B that needs A; constructor order and handles. P-8 thread B's `dlsym(RTLD_DEFAULT)` during A's constructor
that waits on a pipe (a hang guard decides "blocked", probe only). P-9 `fork` while another thread is inside a
constructor of a `dlopen`; the child `dlopen`s. P-10 an IFUNC resolver of object 1 when object 2 of the same `dlopen`
has an undefined symbol under `RTLD_NOW`: does it run. P-11 a surplus module's initial value in a thread created
before the `dlopen`. P-12 `dlinfo(RTLD_DI_LINKMAP)` of a handle equals the handle; `l_name` of `dlopen(NULL)`.
P-13 `dlopen` of the program by path, of `ld.so`, of `libc.so.6` by name (same handle as the start-time one).

## 7. Risks

* **DL-16a overrun** (the largest). Split point: "`RTLD_NOW|RTLD_LOCAL` load + dlerror texts" / "`NOLOAD`,
  `NODELETE`, failure-unwind plants". Checkpoint early with a WIP.
* **No dynamic conformance harness** blocks every clause test (DLSYM-SPECIAL-HANDLES found it): DL-16H first.
* **TLS**: most real plugins (anything with `__thread`, C++ runtimes) carry `PT_TLS`, so `dlopen` is of limited use
  until DL-16d, which waits for DL-15. DL-21's unmodified binaries need DL-16d and DL-20.
* **`RTLD_LAZY` interim** (undefined function refused until DL-20); listed in DL-20's tests.
* **Deadlocks**: constructors calling into libc under the loader lock; mitigated by the lock-order rule, the
  re-entrant lock, lock-free readers, and DL-16c's constructor corpus.
* **`fork` during `dlopen`**: closed by the prepare/child step (probe P-9).
* **Security** (every DL story): `dlopen` names come from programs, often from configuration; the secure-mode search
  rules, the bounds checks of DL-7..DL-10 and the identity check apply; count saturation pins instead of wrapping.
  Malformed-object cases in every story's tests.
* **Static programs** (ruling #5005, pending decision 4): `dlopen` in `libc.a` is out of DL-16 and deferred, not
  dropped. DL-16a gives `libc.a` failing entry points (`NULL` and a dlerror text, never a crash, no link error); a
  differences row records it, with a registered compat placeholder (pending `-5`). A later phase-14 item, **DL-16S
  `DL-DLOPEN-STATIC`** (its own story after DL-17 and DL-20), loads objects into static programs.

## 8. Story briefs: "The change" sections (for the coordinator)

**DL-16P `DL-DLOPEN-PROBES`.** Settle probes P-1 to P-13 of `plan/ph14/dl-16-dlopen.md` §6 against the reference, black
box only, as programs under `tools/probe/dl/<topic>/` with a `run.sh SCRATCH`, and record each answer as a row of
`spec/behaviour/dynlink.md` (+ the raw lines in `dynlink.glibc.txt`). No product code. Where an answer is undefined or
unsafe in the reference (a crash, a deadlock), record it as such: DL-16 specifies it (#2607 principle).

**DL-16H `CONF-DYN-HELPERS`.** Give the conformance harness a dynamic mode: a directive in a `conformance/c` program
names helper sources that are built as shared objects, and the program is linked dynamically and run under Liella's
`ld.so` (from `target/liella`) and under the reference. Adopt `tools/probe/dl/handles/` and the failure corpus of
`tools/probe/dl/run.sh` as the first two programs (expected to fail on Liella until DL-16a/b: wire them as pending in
the clause tests, not as skips). Plant: a helper left unbuilt must be caught.

**DL-16a `DL-DLOPEN-LOAD`.** Implement `dlopen` and `dlerror` (plan §1, §3, §4): exports in `libc.so.6` and the
`libdl.so.2` placeholder per the symbol versions in `spec/`, the caller-address capture (seam), the re-entrant writer
lock, the append-only chain with `LM_ID_BASE` only, the new-set load (D-27.16 search with the caller as requester,
`(dev, ino)` identity, D-27.15 map, version check, two-pass relocation, one publication, D-27.19 init), modes
`NOW`/`LAZY` (eager)/`LOCAL`/`NOLOAD`/`NODELETE`, counts with saturation, the failure unwind, per-thread dlerror in the
loader arena, and failing `dlopen`/`dlsym`/`dlvsym`/`dlerror` entry points in `libc.a` (ruling #5005: `NULL` and a
dlerror text, never a crash). Proof: `s27_1_c_1*`, `s27_1_c_5` and `s27_1_f_0` with every dlerror text of
`dynlink.md`; a static program's `dlopen` fails with a text; the three failure plants.

**DL-16b `DL-DLSYM`.** Implement `dlsym` and `dlvsym` (plan §2): handle scopes, `RTLD_DEFAULT`, `RTLD_NEXT` through
the caller's object, `RTLD_GLOBAL` and promotion, `RTLD_DEEPBIND`, the unique-symbol table with pinning, IFUNC and
initial-module `STT_TLS` answers, handle validation (pending decision 3). Proof: `s27_1_c_3`, `s27_1_c_1_3`,
`s27_1_g_0`; `handles.c` as a conformance program; the version-table rows of `dynlink.md`; plants on `RTLD_NEXT` and
the NULL-handle scope.

**DL-16c `DL-DLOPEN-CONCURRENT`.** Make the loader's readers lock-free and safe against `dlopen` (plan §4):
`dl_iterate_phdr` and `_dl_find_object` over the chain in dynamic programs with the probed `adds`/`subs`,
`read_section`, recursive loader calls from constructors, the `fork` step, the lock-order rule. Proof: `s27_1_c_8`;
the constructor corpus; a C++ exception through a `dlopen`'d object; a concurrent loop with an iteration budget.

**DL-16d `DL-DLOPEN-TLS-SURPLUS`.** Let `dlopen` load objects with `PT_TLS` into the static surplus (plan §5) on top of
DL-15's thread list and module table: module ids, first-fit placement, initialisation in every live thread, dtv
update, `dlsym` of `STT_TLS`. Proof: the surplus part of `s27_3` (`s27_1_c_4` is DL-17's); threads created before
and after; the exhaustion text; plant: one live thread skipped.

**DL-16S `DL-DLOPEN-STATIC`** (later phase-14 item, after DL-17 and DL-20; ruling #5005). Let a program linked with
`libc.a` load shared objects: probe the reference first (what is loaded, which libc the object binds to, TLS), then
decide the design and whether it is the default or the registered compat option (pending `-5`). Until it lands the
DL-16a failing entry points and the differences row stand.
