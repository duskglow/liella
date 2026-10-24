# The ways into Liella

Liella is a C library: glibc 2.39's ABI for x86_64, written in Rust. A program can reach it in
four ways. This document says what each is, when to use it, what it guarantees, and what it
costs. The design is decision D-X.10 of [implementation-plan.md](../implementation-plan.md);
every build parameter and command is in [toolchain.md](toolchain.md).

| | Way in | For | Interface | Stability | Bound to a compiler? |
|---|---|---|---|---|---|
| 1 | **C and POSIX** | Every existing program | glibc's symbols and versions (`malloc@@GLIBC_2.2.5`, ...), the system headers | glibc's: versioned symbols, never changed | No |
| 2 | **Native C ABI** | Programs in any language that want Liella's services without POSIX's conventions | `liella_<area>_<name>` in nodes `LIELLA_NATIVE_<n>`; [`include/liella/native.h`](../include/liella/native.h) | A published node never changes; additions go in a new node | No |
| 3 | **Static Rust** | Rust programs linked statically | the crate `liella-native`, feature `static` | That of the source the program is built from | No: one compiler builds the whole program |
| 4 | **Rust dylib** | Rust programs sharing one copy of the clean API | `libliella_rust.so.<abi>`, Rust ABI version `lr<N>` ([`spec/abi/rust-abi.toml`](../spec/abi/rust-abi.toml)) | Per Rust ABI version: fixed toolchain, fixed options, coexisting versions | **Yes**: the exact rustc of its version |

## 1. C and POSIX

The default, unchanged by the other three. A C program compiles against the system's glibc
2.39 headers and links against Liella's `libc.a` or `libc.so.6` exactly as against glibc.
Every function behaves as `glibc-functional-spec.md` describes (deliberate differences are in
[differences-from-glibc.md](differences-from-glibc.md)).

**Use it** for every existing program, and for new C programs that are written to POSIX.

## 2. The native C ABI

A second set of C functions over the same state, with conventions that suit languages other
than C, Rust first: byte slices as a pointer and a length (never NUL-terminated), results as a
value or a negated error number (no `errno`), fixed-width integer types, explicit
out-parameters, structures without implicit padding. Version 1 (`LIELLA_NATIVE_1`) covers the
stateful services a program needs: the heap, the environment, file descriptors and the file
system calls, the clocks, the current time zone and conversions, the current locale, and the
process's arguments, ID and exit; and memory for secrets (decision D-X.11):

* `liella_secure_zero(ptr, len)` zeroes `len` bytes so that the stores are made even if the
  bytes are never read again, whether the call is compiled separately, inlined or link-time
  optimised (the zeroing is followed by a call to an assembly barrier no optimiser can see
  into; `cargo xtask native` checks an LTO build for it). `explicit_bzero` and
  `__explicit_bzero_chk` call it.
* `liella_heap_alloc_secure(size, align)` gives a heap block, aligned as `liella_heap_alloc`'s
  and reading as zero, in a mapping of its own that is kept out of core dumps. Whatever releases
  it (`liella_heap_free`, `free`, `realloc` or `liella_heap_realloc` moving it) zeroes it before
  its memory can be reused by anything, the heap's caches included; a reallocated block stays
  secure. Each block costs at least a page, and ordinary blocks pay nothing for the feature.

It also switches the integrated name-service cache (decision D-16.10), which keeps the answers
of the built-in `files` and `dns` sources for hosts, passwd, group (group lists included),
services, protocols and networks, in a table of the process or in the shared region of the
effective user (`/dev/shm/liella-nscache-<euid>`, used only when it can be trusted). The cache
never changes an answer: a lookup sees a change of a watched file at most about a second late
(at once in a process started after it), and DNS data never outlives its TTL. A process's first
lookup goes to the sources (the table is attached by the second), so a process that makes one
lookup pays nothing for the cache.

* `liella_nscache_enable(on)`: enable or disable the cache for the process (1 if it was
  enabled, else 0). Disabled, nothing is mapped, `stat`ed or kept, and a lookup costs one atomic
  load. `LIELLA_NSCACHE=0` in the environment disables it at start, except under `AT_SECURE`.
* `liella_nscache_category(category, on)`: the same for one category
  (`LIELLA_NSCACHE_HOSTS`, `_PASSWD`, `_GROUP`, `_SERVICES`, `_PROTOCOLS`, `_NETWORKS`); 1 if it
  was enabled, else 0, `-EINVAL` for another number.
* `liella_nscache_invalidate(category)`: invalidate one category, or every one
  (`LIELLA_NSCACHE_ALL`), in this process's table and in the user's shared region, so that every
  process of the user drops those answers; the next lookup validates the watched files at once,
  whatever the time since the last check. 0, or `-EINVAL`.
* `liella_nscache_status(&info)`: `struct liella_nscache_info`: enabled, the enabled categories,
  the table in use (`LIELLA_NSCACHE_MODE_OFF`, `_IDLE`, `_PRIVATE`, `_SHARED`, `_UNAVAILABLE`), why
  it is not the shared region (`LIELLA_NSCACHE_REASON_*`: no region location, secure mode or
  differing IDs, a refused file, an unavailable one), the region's owner, a generation that moves
  with every invalidation, and this process's hits, misses, validation `stat` calls and stores.

It also chooses the engine of POSIX asynchronous I/O (decision D-11.11). A process's `aio_*`
requests run on io_uring when the kernel offers and permits it, and otherwise on worker threads;
the engine is chosen at the first request. The workers are chosen without trying io_uring in
secure-execution mode (`AT_SECURE`), under a seccomp filter (one that kills on a system call it
does not list would otherwise kill the process at `io_uring_setup`), with `LIELLA_AIO_URING=0` in
the environment at the first request, or after:

* `liella_aio_use_threads()`: run this process's requests on worker threads, never on io_uring,
  for the life of the process and in its `fork` children. 0, or `-EBUSY` once a request has
  started io_uring (call it before the first request).

It reloads time-zone and locale data on request (decision D-X.13). Both are kept for the life of
the process once read (a zone file is checked again only where `localtime` and `tzset` check
it); after a tzdata or locale update, a long-running program calls:

* `liella_tz_invalidate()`: every zone kept from a file is compared with its file before its
  next use and read again if the file changed; the zone `TZ` names is then selected, as by
  `tzset`. 0.
* `liella_locale_invalidate()`: every mapped locale file and charmap is compared with its file
  before its next use and read again if it changed; the global locale is loaded again by its
  names. 0, or `-ENOENT` when one of its names no longer resolves (it is then unchanged).
  `locale_t` objects keep the data they were made with.

It offers the environment contract, a Liella extension that is off by default (decision D-1.12).
Off, `getenv` reads the live `environ` on every call, exactly as the GNU C library does. A
program with a large environment that calls `getenv` often (per request, per message) can turn
it on:

* `liella_environ_contract(on)`: non-zero `on` turns the contract on, 0 off; the previous state
  (1 on, 0 off). While it is on, `getenv` answers from a hashed table of the names, hits and
  misses alike, in about the same time for 30 variables or 2,000. `setenv`, `putenv`,
  `unsetenv`, `clearenv` and `liella_env_*` keep the table current. **The contract:** a program
  that changes the environment any other way (stores into a slot of `environ`, edits the name
  of a string it gave to `putenv`) calls `liella_environ_reload()` afterwards; until then
  `getenv` may answer as before the change (never with another variable's value). A program
  that assigns a new array to `environ` is seen at once. `secure_getenv` and the library's own
  lookups (`TZ`, locale variables, ...) are unaffected.
* `liella_environ_reload()`: rebuild the table from `environ` (nothing while the contract is off).

Threads: lookups take no lock and a lookup racing a writer falls back to a scan, so `getenv`
stays async-signal-safe; serialise `liella_environ_reload` with other environment writers, as for
`setenv`. A `fork` child keeps the table and the contract; a new program image (`exec`) starts
with it off, and nothing in the environment turns it on. Memory: the table is allocated when the
contract is first turned on, 16 bytes per variable rounded up to a power of two (512 bytes at
least; 32 KiB for 2,000 variables), and is kept when it is turned off.

Old data is never unmapped or freed, so pointers into it (`tm_zone`, `tzname`, a `locale_t`)
stay valid. In Rust: `liella_native::tz::invalidate()` and `liella_native::locale::invalidate()`.

It bounds `getpass` (decision D-17.8): a line of more than the bound (1 MiB by default,
`LIELLA_GETPASS_DEFAULT_LIMIT`) is read to its end and discarded, every buffer of the library's
that held it is zeroed, and `getpass` fails with `EOVERFLOW`; a truncated password is never
returned.

* `liella_getpass_set_limit(bytes)`: set the process's bound (0: the default again); the bound
  that was in force, or `-EINVAL` above `PTRDIFF_MAX`. It holds for the life of the process and
  in its `fork` children (`liella_native::getpass::set_limit` in Rust).

It has an opt-in diagnostic channel (decision D-2.9). By default the library writes nothing to a
program's standard error of its own accord. `LIELLA_DIAG=<topic>[,<topic>...]` in the
environment (`all` for every topic; unknown names are ignored; ignored under `AT_SECURE`) turns
topics on. It is read once, lazily: when the library first meets an event it could report, not
at process start, so a program may also set it with `setenv` before then; changes after that
first read have no effect. With topics on, the library reports rare events of those topics
as one line each on descriptor 2, `<program>: liella: <topic>: <text>`, written with a single
`write` (async-signal-safe, no allocation, no stdio). Lines come once per episode (when a failure
begins and when it ends), never once per event. Off, it costs nothing on any common path. Topics:

* `timers`: a `SIGEV_THREAD` timer's notification thread cannot be created (the thread limit, or
  memory), so the library retries it with a back-off (decision D-23.3); one line when such an
  episode begins for a timer, one when its notification thread starts again. A sign that the
  process is starved of threads or memory.

The interface is described once, in [`spec/abi/native.tsv`](../spec/abi/native.tsv), from which
the header is generated and against which the exports are checked.

**Until the first release, Butterfly Wing** (owner, 2026-10-03), `LIELLA_NATIVE_1` and `lr1`
are open: functions are still added (for example `liella_secure_zero` and the secure allocator,
the `liella_nscache_*` functions, 2026-10-03, and `liella_getpass_set_limit`, 2026-10-04) and `lr1` is rebuilt. The guarantee below and the immutability of lr versions begin
at that release and are not yet binding.

**Guarantee.** A published version node never changes: no function is removed from it, and
no signature, structure layout, constant or behaviour in it changes. New functions go in a
new node (`LIELLA_NATIVE_2`, ...), which names the previous one as its parent. A program
records the node of every function it uses (`liella_fd_read@LIELLA_NATIVE_1`), so it keeps
working with every later `libc.so.6`, and fails to load, rather than misbehave, with an older
one that lacks a node it needs.

**Use it** from any language with a C foreign-function interface, and from Rust when the
program must not depend on a particular Rust compiler. It is also the layer the two Rust ways
are built on.

## 3. Static Rust

The facade crate `liella-native` (`crates/native`) with feature `static` gives a Rust program
the clean library as plain Rust (`liella`, `liella-math`, and the public types of `liella-rt`
and `liella-kernel`) and the stateful services as safe Rust functions over the native C ABI;
for secrets, `liella_native::secure::secure_zero(&mut [u8])` and `SecureVec`, a growable byte
buffer in secure heap blocks that the heap zeroes whenever it lets go of one (when the buffer
moves and when it is dropped); for the name-service cache, `liella_native::nscache` (`enable`,
`set_category`, `invalidate`, `status` with a `Category` enum and a `Status` whose `mode()`
decodes the table in use). The Rust dylib (section 4) contains the same wrappers.
The program is linked like Liella's own products: statically, with Liella's start object and
`libc.a`, and no host C library. The crate has no logic of its own and no unsafe code outside
its declarations of the C ABI (`src/ffi.rs`).

**Guarantee.** None across compilers is needed: the program and the library parts it uses are
compiled by one compiler into one executable. The program sees the API of the Liella source it
is built from.

**Use it** for Rust programs that can be linked statically: the simplest way, and the only
one that runs today (section 6). With feature `dynamic` instead, the same program links
against `libc.so.6` and still compiles the clean API in; it then depends only on the native C
ABI's nodes, not on a compiler.

## 4. The Rust dylib

`libliella_rust.so.<abi>` is `liella-native` (feature `dylib`) and the whole clean API,
compiled once as a Rust `dylib`. Rust programs built by the same toolchain link it instead of
compiling those crates in, so several programs share one copy of the code on disk and in
memory. Its stateful services call `libc.so.6`'s native C ABI; the dylib itself holds no
state.

Rust has no stable ABI: a dylib works only with programs built by exactly the compiler and
options that built it. So each build is named by a **Rust ABI version**, `lr<N>`, defined in
[`spec/abi/rust-abi.toml`](../spec/abi/rust-abi.toml): the exact rustc (release and commit),
LLVM, the target and its features, the codegen options, the panic strategy, the absence of
`std`, the allocator arrangement, the linker, and the native C ABI node it calls. The
identifier is part of the public interface, in three places: the file name and soname
(`libliella_rust.so.lr1`), an ELF note (`.note.liella.rustabi`), and an exported string
(`LIELLA_RUST_ABI`); [toolchain.md](toolchain.md#63-how-the-version-is-embedded-and-how-to-read-it)
shows how to read each.

**Guarantee.** Per version: a defined version is never changed and never rebuilt by another
toolchain. A new toolchain or any changed option is a new version, built beside the old ones
under its own name. A program records the version it was built against (`DT_NEEDED
libliella_rust.so.lr1`) and gets exactly that one; versions coexist on a system and in one
process. There is no compatibility between versions: moving a program to a new version means
rebuilding it with that version's toolchain.

**Use it** only when sharing the compiled clean API between Rust programs matters more than
the coupling to one compiler, and the programs are built together with the dylib (a
distribution, a product built by one toolchain). Otherwise use way 3, or way 2 directly.

## 5. One instance of state

A process has exactly one C library, and every piece of process state belongs to it: the
heap, thread blocks, `errno`, the environment, the current locale, the time zone, the
streams, the exit handlers. If a Rust program kept its own copy of any of these, a C library
it calls, or C code in the same process, would see a different heap or environment, and
`exit` would skip the other copy's handlers.

So the dividing line in ways 3 and 4 is:

* **Stateful services** are reached only through the native C ABI of the library that owns
  the state (`libc.a` or `libc.so.6`). The Rust facade wraps those calls; it stores nothing.
  The global allocator of a Rust program is the C library's heap.
* **Stateless computation** (parsers, formatters, Unicode, calendar arithmetic, mathematics) is
  compiled into each Rust program or into the dylib. Several copies in one process are
  harmless: they hold nothing.

Rust data structures never cross between separately compiled objects; only the C ABI's
integers, byte slices and `#[repr(C)]` structures do. This is also why several Rust ABI
versions can share a process: each dylib's Rust types stay inside the programs built against
it, and all of them share the one C library's state.

## 6. What works today

| Way | Build | Run |
|---|---|---|
| 1, static (`libc.a`) | Yes | Yes |
| 1, dynamic (`libc.so.6`) | Yes | No: needs Liella's dynamic loader (phase 14) |
| 2, static | Yes | Yes (`conformance/c/native_v1.c`) |
| 2, dynamic | Yes | No: phase 14 |
| 3, static (`examples/native_static.rs`) | Yes | Yes, in the gate (`cargo xtask native`) |
| 3 with `dynamic` (`examples/native_dynamic.rs`) | Yes | No: phase 14 |
| 4 (`examples/native_rust_dylib.rs`, lr1) | Yes, in the gate (`cargo xtask rust-dylib`) | No: phase 14 |

Every Rust program is `#![no_std]` until the C library provides the symbols `std` needs
(`getcwd`, `realpath`, `dl_iterate_phdr`, `_dl_find_object`); [toolchain.md,
section 8](toolchain.md#8-what-cannot-work-yet) lists every limitation.

## 7. The complexity, stated

The owner accepted this complexity (D-X.10); it is listed here so that it is never hidden.

1. **Two ABIs to keep consistent.** The POSIX exports (way 1) and the native C ABI (way 2)
   reach the same state through different entry points, so both must keep the same semantics:
   a heap block from `liella_heap_alloc` is freed by `free`, an environment variable set by
   `liella_env_set` is seen by `getenv`, `liella_process_exit` runs the `atexit` handlers.
   The native functions are thin conversions over the same internal implementation as the
   POSIX ones, and the conformance program `conformance/c/native_v1.c` checks that the heap
   and the environment are shared across both. Every new native node adds functions that
   must stay consistent for good.
2. **The dylib is coupled to one compiler.** A Rust ABI version names one exact rustc, its
   pinned toolchain, installed with rustup beside the workspace's own (pinned to
   an exact release in `rust-toolchain.toml`). Every machine that builds or tests the dylib needs that toolchain installed
   too (the gate fails without it), and every supported version adds one. Offering the dylib
   to programs of a newer compiler means defining, building, testing and shipping a new
   version.
3. **Versions accumulate.** Every supported version is a file to build (each with its own
   toolchain), test and ship. Retiring one breaks the programs built against it, so it is a
   recorded decision.
4. **What a program must do when Liella adds lr2.** Nothing, to keep running: its
   `DT_NEEDED` names lr1, which stays installed while lr1 is supported. To move to lr2, it is
   rebuilt with lr2's toolchain against lr2's build files; its source does not change. A
   program cannot link two versions at once (rustc refuses: both define the global allocator
   and the panic handler).
5. **The version does not cover Liella's source.** A Rust ABI version fixes the compiler and
   options, not the Liella source compiled. A build from changed source keeps the symbol
   names but may change layouts and inlined code that programs already contain, and the
   loader cannot tell. Therefore lr versions are immutable (owner ruling, 2026-10-02): a
   released lr version is never rebuilt from changed source, and any change, however small,
   requires a new lr version ([toolchain.md](toolchain.md#7-introducing-a-new-rust-abi-version)).
6. **Interposition.** Two versions in one process each define `LIELLA_RUST_ABI`, and the
   toolchain's `core` and `alloc` symbols when they share a compiler. A reference without a
   handle binds to the first definition in lookup order, as for any symbol: to identify a
   loaded file, use `dlsym` on its handle or its note. Liella's own crates never collide:
   their symbol names carry the version (`-C metadata`).
7. **Build-time files.** A dylib program needs, at compile time, the dylib under the name
   rustc expects (`libliella_native.so`) and the rlibs of the same build; at run time only
   `libliella_rust.so.<abi>` and `libc.so.6`.
8. **Current limitations.** Dynamically linked programs cannot run until phase 14; `std`
   cannot be used yet; nothing unwinds (`panic = "abort"` everywhere); the dylib is built for
   the baseline CPU only; no second real toolchain has been exercised (the coexistence check
   uses a synthetic version built by the same compiler).
