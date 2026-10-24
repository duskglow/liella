# Liella vs glibc: performance progress

One entry per completed phase, newest last. Every entry measures both sides in the same
session on the same machine, with the same method. Ratios are Liella ÷ glibc, so lower is
better and 1.00 is parity.

**How to reproduce:**

* Liella: `cargo xtask bench` runs `crates/boundary/examples/bench_mem.rs` and `bench_heap.rs`
  in release mode, and checks `bench/budgets.tsv`.
* glibc: `sh benchmarks/run_glibc_ref.sh` builds `benchmarks/glibc_ref.c` (`cc -O2`, dynamically
  linked, so glibc's IFUNCs choose the same AVX2/ERMS paths any program gets).
* Single-call rows are the fastest of 41 batches of 2,000 calls, after 2,000 warm-up calls.
  Threaded rows are wall time per malloc/free pair with every thread running at once, best of
  5 runs of 200,000 pairs per thread.

**Machine:** Intel Core i9-14900KF (AVX2, ERMS), 32 logical CPUs, one NUMA node. WSL2,
kernel 5.15.167.4 (built without `CONFIG_NUMA`). Host C library: Ubuntu glibc 2.39.

---

## Entry 1: after phase 3 (2026-10-01; OD-C2 data)

Liella state:

* The `mem*`/`str*` primitives are safe Rust loops in `liella-mem`: 64-byte blocks for copy
  and fill, and 32-byte word scans for search and compare. Baseline x86-64 build (SSE2), no
  assembly.
* `strlen` reads one byte at a time: safe Rust may not read past the terminator (F1).
* The heap is the phase-3 sharded design (D-3.10), with one lock per shard and no per-thread
  caches yet.

### Memory and string primitives (ns per call)

| Function | Size | glibc | Liella | Ratio |
|---|---:|---:|---:|---:|
| `memcpy` | 64 B | 1.80 | 1.63 | 0.91 |
| `memcpy` | 4 KiB | 24.50 | 34.91 | 1.42 |
| `memcpy` | 64 KiB | 926.40 | 2033.39 | 2.19 |
| `memset` | 64 B | 1.80 | 1.09 | 0.61 |
| `memset` | 4 KiB | 13.13 | 34.73 | 2.65 |
| `memset` | 64 KiB | 723.14 | 1797.99 | 2.49 |
| `memcmp` | 64 B | 2.17 | 2.53 | 1.17 |
| `memcmp` | 4 KiB | 27.63 | 67.36 | 2.44 |
| `memcmp` | 64 KiB | 639.01 | 1523.50 | 2.38 |
| `memchr` | 64 B | 2.52 | 3.54 | 1.40 |
| `memchr` | 4 KiB | 20.81 | 147.23 | 7.08 |
| `memchr` | 64 KiB | 426.37 | 2377.85 | 5.58 |
| `strlen` | 64 B | 2.16 | 16.91 | 7.83 |
| `strlen` | 4 KiB | 16.22 | 740.63 | 45.66 |
| `strlen` | 64 KiB | 339.85 | 11945.03 | 35.15 |

The phase-2 budgets compare against the hardware copy bandwidth (`ref_memcpy_4k` = 22.46 ns in
this run) rather than against glibc's own function:

| Function | Ratio | Budget |
|---|---:|---:|
| `memcpy` | 1.55× | 1.10× (not met) |
| `memset` | 1.55× | 1.10× (not met) |
| `memcmp` | 3.0× | none |
| `memchr` | 6.6× | none |
| `strlen` | 33× | 1.10× (not met) |

### Allocator (ns per malloc/free pair)

| Case | glibc | Liella | Ratio |
|---|---:|---:|---:|
| 64 B | 4.69 | 37.79 | 8.06 |
| 4 KiB | 11.51 | 41.58 | 3.61 |
| 64 KiB | 12.05 | 47.21 | 3.92 |
| 1 MiB | 10.80 | 452.73 | 41.92 |
| 1,000 live 64 B blocks, then all freed (per block) | 6.21 | 40.31 | 6.49 |
| 1 thread, 64 B | 4.64 | 38.49 | 8.30 |
| 4 threads at once, 64 B (wall time per pair) | 1.18 | 148.24 | 125.6 |
| 16 threads at once, 64 B (wall time per pair) | 0.52 | 363.52 | 699.1 |

**Reading the allocator rows:**

* **Single thread.** Every call takes the shard lock (an uncontended futex CAS) and, on `free`,
  looks the segment up in the registry. glibc's tcache serves both from a per-thread list
  without any atomic operation.
* **Contention.** All threads share one shard (threads do not know their node until phase 6),
  so they serialise on one lock. glibc spreads threads over arenas and tcaches and scales. This
  is the multi-core gap phase 6's per-thread caches must close. The phase-3 layout was
  restructured so they need no layout change (D-3.10).
* **1 MiB.** Liella maps and unmaps every large block (two system calls). glibc raises its
  `mmap` threshold after the first free of a mapped chunk, then serves 1 MiB from its heap
  without a system call.

### Expected after phase 4

These are projections under the OD-C2 standing approval, to be replaced by measurements. The
targets are per glibc function, at 4 KiB.

| Function | Now | Change in phase 4 | Target |
|---|---:|---|---:|
| `strlen` | 45.7× | Assembly scanner in the C layer: aligned AVX2 32-byte loads (SSE2 fallback), page-safe because an aligned load never crosses a page. Rust validates the result. | ≤ 1.1× |
| `strnlen`, `rawmemchr` | not measured | The same scanner, bounded. | ≤ 1.1× |
| `memcpy`, `memmove` | 1.42× | `rep movsb` (ERMS/FSRM) above a size threshold set from measurement. The Rust block loops stay below it. Rust checks the regions before the C call. | ≤ 1.1× |
| `memset` | 2.65× | `rep stosb` above the threshold. | ≤ 1.1× |
| `memchr`, `memcmp` | 7.1×, 2.4× | Stay in safe Rust, gaining from the x86-64-v3 (AVX2) hwcaps build. | ≤ 2× (estimate) |
| Allocator | 8× (1 thread), 700× (16 threads) | No change planned in phase 4. Per-thread caches, NUMA shard selection and a large-block reuse policy arrive with threads (phase 6). | Phase 6 |

---

## Entry 2: after phase 4 (2026-10-01)

Liella changes since entry 1:

* `strlen`, `strnlen` and `rawmemchr` are C-layer assembly scanners: AVX2, with SSE2 as the
  fallback (D-4.14).
* `memcpy`, `memmove` (disjoint regions) and `memset` use `rep movsb`/`rep stosb` from 1,280
  bytes on ERMS CPUs.
* The allocator is unchanged.

### Memory and string primitives (ns per call)

| Function | Size | glibc | Liella | Ratio | Entry 1 |
|---|---:|---:|---:|---:|---:|
| `memcpy` | 64 B | 2.03 | 2.01 | 0.99 | 0.91 |
| `memcpy` | 4 KiB | 25.05 | 24.08 | 0.96 | 1.42 |
| `memcpy` | 64 KiB | 921.11 | 887.78 | 0.96 | 2.19 |
| `memset` | 64 B | 1.82 | 1.28 | 0.70 | 0.61 |
| `memset` | 4 KiB | 13.31 | 14.59 | 1.10 | 2.65 |
| `memset` | 64 KiB | 738.55 | 703.60 | 0.95 | 2.49 |
| `strlen` | 64 B | 2.19 | 1.92 | 0.88 | 7.83 |
| `strlen` | 4 KiB | 16.34 | 17.89 | 1.09 | 45.66 |
| `strlen` | 64 KiB | 344.21 | 323.47 | 0.94 | 35.15 |
| `strnlen` | 64 B | 2.78 | 2.47 | 0.89 | – |
| `strnlen` | 4 KiB | 16.93 | 17.46 | 1.03 | – |
| `strnlen` | 64 KiB | 338.34 | 333.30 | 0.99 | – |
| `rawmemchr` | 64 B | 2.22 | 2.05 | 0.92 | – |
| `rawmemchr` | 4 KiB | 21.49 | 21.77 | 1.01 | – |
| `rawmemchr` | 64 KiB | 431.99 | 408.83 | 0.95 | – |
| `memcmp` | 64 B | 2.18 | 2.56 | 1.17 | 1.17 |
| `memcmp` | 4 KiB | 27.87 | 69.91 | 2.51 | 2.44 |
| `memcmp` | 64 KiB | 655.94 | 1472.67 | 2.25 | 2.38 |
| `memchr` | 64 B | 2.90 | 3.59 | 1.24 | 1.40 |
| `memchr` | 4 KiB | 20.95 | 146.35 | 6.99 | 7.08 |
| `memchr` | 64 KiB | 430.35 | 2275.65 | 5.29 | 5.58 |

All phase-2 budgets now pass. Against the hardware copy reference at 4 KiB (22.98 ns), the
ratios are `memcpy` 1.05×, `memset` 0.63× and `strlen` 0.78×.

**Projection versus result:**

| Function | Projected | Result |
|---|---|---|
| `strlen`, `strnlen`, `rawmemchr` | ≤ 1.1× glibc | Met |
| `memcpy`, `memset` | ≤ 1.1× glibc | Met |
| `memchr`, `memcmp` | ≤ 2× glibc, from the x86-64-v3 build | Not met |

The x86-64-v3 build measured `memchr` at 116 ns (5.5×) and `memcmp` unchanged at 69.7 ns.
Two rewrites of the safe loops as vector reductions measured about 20× slower and were
reverted. Closing the gap needs C-layer primitives under P7, which is an owner decision because
OD-C2 (c) keeps these two functions in Rust.

### Allocator (ns per malloc/free pair)

| Case | glibc | Liella | Ratio |
|---|---:|---:|---:|
| 64 B | 4.91 | 35.90 | 7.31 |
| 4 KiB | 11.82 | 39.50 | 3.34 |
| 64 KiB | 12.15 | 45.35 | 3.73 |
| 1 MiB | 10.90 | 383.57 | 35.2 |
| 1,000 live 64 B blocks (per block) | 6.25 | 38.25 | 6.12 |
| 1 thread, 64 B | 4.70 | 37.99 | 8.08 |
| 4 threads, 64 B (wall time per pair) | 1.18 | 153.37 | 130 |
| 16 threads, 64 B (wall time per pair) | 0.53 | 335.09 | 632 |

The allocator is unchanged since entry 1, so these differences are run-to-run variation.
Per-thread caches are phase 6.


---

## Entry 3: after phase 5 (2026-10-01)

Liella changes since entry 2: the thin kernel surfaces (low-level I/O, the file-system calls,
sockets, signals, clocks, limits, `sysconf`, IPC). The memory primitives and the allocator are
unchanged, and `cargo xtask bench` still meets all 14 budgets.

New this entry: `crates/boundary/examples/bench_calls.rs` and the matching rows of
`benchmarks/glibc_ref.c` time the exports a program calls most against the same calls in
glibc. Same method: fastest of 41 batches of 2,000 calls.

### Thin calls (ns per call)

| Call | glibc | Liella | Ratio | Path |
|---|---:|---:|---:|---|
| `getpid` | 47.50 | 48.72 | 1.03 | system call |
| `getppid` | 47.34 | 47.37 | 1.00 | system call |
| `clock_gettime(CLOCK_MONOTONIC)` | 11.93 | 11.75 | 0.98 | vDSO |
| `time(NULL)` | 2.32 | 3.02 | 1.30 | vDSO `time` |
| `gettimeofday` | 12.33 | 11.75 | 0.95 | vDSO |
| `sysconf(_SC_PAGESIZE)` | 4.17 | 4.19 | 1.00 | auxiliary vector |
| `sysconf(_SC_NPROCESSORS_ONLN)` | 1221.72 | 1352.15 | 1.11 | reads sysfs each call (both) |
| `read` 1 byte from `/dev/zero` | 68.97 | 87.34 | 1.27 | system call |
| `sigprocmask` (query) | 52.26 | 64.21 | 1.23 | system call |
| `stat("/")` | 159.96 | 166.37 | 1.04 | system call |
| `open` + `close` (`/dev/null`) | 416.70 | 420.87 | 1.01 | two system calls |

`read` and `sigprocmask` varied across the four runs of this session: 68.4–69.7 and
52.0–53.1 ns in the other three, at parity with glibc. The code was the same in every run. The
system-call rows are dominated by the kernel entry, so the translation layer adds about a
nanosecond or less.

**Found by these measurements and fixed before this entry:**

* `time()` was a full `clock_gettime(CLOCK_REALTIME)` (11.9 ns, 5.2× glibc). It now reads the
  vDSO's `time` entry as glibc does (D-23.1), which also gives glibc's exact value: whole
  seconds as of the last tick.
* `sysconf(_SC_PAGESIZE)` took 792 ns in processes Liella did not start (tests, benchmarks):
  every auxiliary-vector lookup re-read `/proc/self/auxv`. The vector is now read once and
  searched in place (4.19 ns). Under Liella's own runtime the lookup always read memory.

### Memory primitives and allocator

Unchanged code; this session's figures are within entry 2's run-to-run variation (for example
`memcpy` 4 KiB 21.29 ns against glibc 24.17, `strlen` 4 KiB 17.26 against 16.45, `memchr`
4 KiB 145.63 against 20.57, 64 B `malloc`/`free` 37.40 against 4.63, 16 threads 349.97 against
0.56). The `memchr`/`memcmp` gap awaits the owner's decision (OD-C2 c); the allocator's
single-thread and contention gaps are phase 6's per-thread caches.

### Expected after phase 6

The plan sets no allocator budget yet; the targets below are proposals for the owner to
confirm or replace (they would become `bench/budgets.tsv` rows, rule P6).

| Area | Now | Phase 6 change | Proposed target |
|---|---:|---|---|
| Allocator, 1 thread, 64 B | 8.1× | Per-thread caches in front of the shards; no lock or atomic on the fast path. | ≤ 1.5× |
| Allocator, 16 threads, 64 B | 625× | Per-thread caches, and each thread's shard chosen by its NUMA node. | ≤ 2× |
| Allocator, 1 MiB | 41× | Large blocks reused instead of mapped and unmapped per call. | ≤ 2× |
| `gettid` | system call | Cached in the thread block (D-4.15 offset 0x84), as glibc does. | a load |

### `memchr` and `memcmp`: moving to the C layer (OD-C2 revised, 2026-10-01)

The owner approved C-layer assembly for both, with the treatment of `strlen` (D-4.14): aligned
vector loads that never cross a page, the path chosen once from CPUID, and Rust checking the
result independently of the assembly. Expected after phase 6a, against this entry's glibc
figures:

| Function | Size | glibc | Liella now | Ratio now | Expected | Why |
|---|---:|---:|---:|---:|---:|---|
| `memchr` | 64 B | 2.85 | 3.44 | 1.21 | ≤ 1.1 | one or two vector compares instead of a word loop |
| `memchr` | 4 KiB | 20.57 | 145.63 | 7.08 | ≤ 1.1 | 32-byte AVX2 compares, 128-byte unrolled groups, as `rawmemchr` (1.01× today) |
| `memchr` | 64 KiB | 422.33 | 2351.09 | 5.57 | ≤ 1.1 | as 4 KiB; memory-bound at the end |
| `memcmp` | 64 B | 2.15 | 2.46 | 1.14 | ≤ 1.1 | vector compare of both inputs, mismatch by `pmovmskb`/`tzcnt` |
| `memcmp` | 4 KiB | 27.36 | 67.80 | 2.48 | ≤ 1.1 | 32-byte AVX2 compares in unrolled groups |
| `memcmp` | 64 KiB | 601.31 | 1517.57 | 2.52 | ≤ 1.1 | as 4 KiB |

`memcmp` reads two inputs, so an aligned load on one is in general unaligned on the other. Its
loads stay inside both regions (the length bounds them), so no page-crossing argument is
needed; `memchr` reads up to a bound too. Both therefore load within `[s, s + n)` only, and need
no over-read at all.

**Result (phase 6a, D-4.17).** Both functions are now C-layer assembly. Same host and session,
`cargo xtask bench` against `sh benchmarks/run_glibc_ref.sh`, ns/op; the 64-byte figures are
the medians of four runs of each:

| Function | Size | glibc | Liella | Ratio | Expected | Met |
|---|---:|---:|---:|---:|---:|---|
| `memchr` | 64 B | 2.81 | 2.75 | 0.98 | ≤ 1.1 | yes |
| `memchr` | 4 KiB | 20.44 | 19.91 | 0.97 | ≤ 1.1 | yes |
| `memchr` | 64 KiB | 419.23 | 420.60 | 1.00 | ≤ 1.1 | yes |
| `memcmp` | 64 B | 2.11 | 2.28 | 1.08 | ≤ 1.1 | yes |
| `memcmp` | 4 KiB | 27.17 | 26.42 | 0.97 | ≤ 1.1 | yes |
| `memcmp` | 64 KiB | 634.36 | 624.17 | 0.98 | ≤ 1.1 | yes |

The 64-byte `memcmp` carries the Rust checks (a bounds check and one byte comparison) on a call
of about 2 ns; that is most of its 8%. `bench/budgets.tsv` now holds both as budgets instead of
regression guards: `memchr_4k` within 1.1× of a 4 KiB copy (as `strlen`), `memcmp_4k` within
1.5× (it reads two inputs; glibc's own ratio is 1.27×).

---

## Entry 4: after phase 6 (2026-10-01)

Per-thread heap caches (D-3.11 to D-3.13) and the thread runtime (D-22.9). Same machine and
method; Liella rows are the medians of three `cargo xtask bench` runs, glibc from one
`sh benchmarks/run_glibc_ref.sh` in the same session. `bench_heap`'s `ref_*` rows (the host
glibc reached through Rust's allocator, measured in the same process) are the budgets' base;
the C reference below is the comparison this file tracks.

**New:** `cargo xtask bench` also builds C benchmarks against the products
(`bench/c/cbench_*.c`): the thread block exists only in a process Liella started.
`glibc_ref.c` gains `gettid` and `pthread_self`.

### Allocator (ns per malloc/free pair)

| Case | glibc | Liella | Ratio | Entry 2 ratio | Budget (owner) |
|---|---:|---:|---:|---:|---|
| 64 B | 4.48 | 5.25 | 1.17 | 7.31 | ≤ 1.5× (1.31× the `ref` row) |
| 4 KiB | 11.05 | 6.61 | 0.60 | 3.34 | — |
| 64 KiB | 11.30 | 6.73 | 0.60 | 3.73 | — |
| 1 MiB | 10.32 | 18.91 | 1.83 | 35.2 | ≤ 2× (1.90× the `ref` row) |
| Batch: 1,000 live 64 B blocks, then all freed (per pair) | 5.94 | 4.26 | 0.72 | 6.12 | — |
| 1 thread, 64 B | 4.35 | 5.37 | 1.23 | 8.08 | — |
| 4 threads at once, 64 B (wall time per pair) | 1.09 | 1.40 | 1.28 | 130 | — |
| 16 threads at once, 64 B (wall time per pair) | 0.52 | 0.61 | 1.17 | 632 | ≤ 2× (1.00× the `ref` row) |

* **Single thread.** The fast path is the thread's own slab: a find-first-set in the local
  bitmap and two plain stores, with the cache found through the thread block. The remaining
  gap to glibc's tcache has not been profiled; the likely cost is the slab lookup on `free`
  (glibc reads a header in front of the block, Liella's blocks have none).
* **Contention** is gone: each thread allocates from slabs it owns, and remote frees go to a
  second bitmap. 16 threads scale like glibc's.
* **1 MiB** is the closest to its budget (5% headroom on the `ref` row). A freed large block is
  kept in its thread's cache and found again by size (D-3.13), so no system call remains; glibc
  instead raises its mmap threshold after the first free and serves 1 MiB from the main heap.
  Where Liella's 19 ns go (cache search, large-table update, the CACHED flag) has not been
  profiled yet; that comes first if this row ever needs to move.

### Thread block reads (ns per call)

| Function | glibc | Liella | Ratio | Notes |
|---|---:|---:|---:|---|
| `gettid` | 45.17 | 1.57 | 0.03 | glibc makes the system call; Liella reads the TID cached in the thread block (D-22.9) |
| `pthread_self` | 1.72 | 1.04 | 0.60 | a thread-pointer read in both |
| `getppid` (system call, for scale) | 45.70 | 45.41 | 0.99 | |

Liella's two loads are measured through a function pointer (the headers declare
`pthread_self` `const`, which lets the compiler hoist a direct call out of the loop), so they
include an indirect call. The budget is `gettid` ≤ 5 ns and ≤ 0.1× `getppid`.

### Memory primitives

Unchanged code since entry 3. Over the eight bench runs of this session, every scanner row
stayed at 1.00–1.04× the 4 KiB copy except two single-row outliers, `rawmemchr_4k` once
(1.13×, 0.5 ns over its 1.1× budget) and `strlen_4k` once (1.16×, 1.3 ns over);
both passed in every other run. The machine was shared with another build during this
session. Noted here so a later entry can tell noise from a regression.

### Expected after phase 7

Phase 7 (locale and text) adds no hot path to these rows. Its own measurements (the
classification and case-mapping tables, `iconv` throughput) start in entry 5.

## Entry 5: after phase 7 (2026-10-01)

Locale and text. **A different machine from entries 1–4:** the owner's VM, an AMD EPYC 7551
(16 cores, two NUMA nodes), Ubuntu 24.04 (Linux 6.8, glibc 2.39). The rows of earlier entries
are not comparable with this one. `bench/c/cbench_locale.c` builds both against the products
and against the host glibc (`cc -O2`), so the two columns run identical code; medians of three
runs, each row the best of seven rounds. `cargo xtask bench` on this machine: all 26 budgets
met (the five new ones are sanity bounds; the phase has no owner budgets).

### Classification, case mapping, decoding (ns per call)

| Function | Locale | glibc | Liella | Ratio |
|---|---|---:|---:|---:|
| `isalpha` | C | 2.37 | 9.36 | 3.95 |
| `toupper` | C | 2.03 | 4.05 | 2.00 |
| `towupper` | C.UTF-8 | 5.21 | 5.75 | 1.10 |
| `iswalpha` | C.UTF-8 | 5.41 | 6.76 | 1.25 |
| `wcwidth` | C.UTF-8 | 4.11 | 5.07 | 1.23 |
| `mbrtowc`, a 3-byte character | C.UTF-8 | 25.09 | 46.22 | 1.84 |
| `strcoll`, 16 bytes, equal up to the last | C.UTF-8 | 6.09 | 24.07 | 3.95 |
| `newlocale` + `freelocale` | C.UTF-8 | 5,869 | 5,415 | 0.92 |

All are measured through function pointers (no header macros). The byte functions find the
thread's current locale on every call (thread block, then the interned locale, D-7.8), where
glibc reads one thread-local pointer; that is the likely cause of `isalpha`'s gap, not yet
profiled. The wide functions read the same Unicode tables in both. `strcoll` in the built-ins
compares bytes, as glibc does; its gap (finding the locale, measuring both strings before
comparing) has not been profiled either.

### `iconv`, 4 KiB of mixed text (ns per conversion)

| Conversion | glibc | Liella | Ratio | Before tuning |
|---|---:|---:|---:|---:|
| UTF-8 → UTF-16LE | 8,807 | 38,783 | 4.40 | 12.5 |
| UTF-8 → ISO-8859-15 (a system charmap) | 14,620 | 59,985 | 4.10 | 27.5 |
| UTF-8 → ASCII//TRANSLIT (C.UTF-8) | 57,773 | 90,913 | 1.57 | 2.56 |

The text is 75% ASCII, with a two-byte and a three-byte character every 16 bytes. Profiling
(`perf`) found the charmap encoder searching all of ISO-8859-15's 256 one-character ranges
and stalling on a byte-wise store before a word-wise read. Fixed in this phase: ranges that
continue one another merge when parsed, a sequence is computed in one word, and runs of ASCII
convert in one loop when both sides keep ASCII as itself. The remainder is the generic
per-character step on the non-ASCII characters (decode to a code point, then encode), where
glibc's modules convert in specialised loops. No budget asks for more yet.

### `strtod` (ns per call)

| Input | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `"3.14159"`, C | 116.95 | 2,268 | 19.4 |
| `"3.14159"`, C.UTF-8 | 117.06 | 2,272 | 19.4 |

The radix lookup (D-8.3) costs nothing measurable; the cost is the parsing engine's, from phase
4: every call clears a 12 KB digit buffer and rounds with exact big arithmetic, even for inputs a
double computes exactly. This had not been measured before.

### Expected after phase 8

No change to these rows: the gaps below wait for the optimization pass (owner, 2026-10-01).

## Entry 6: after phase 8 (2026-10-02)

Streams and formatting. The same machine as entry 5 (the owner's VM, AMD EPYC 7551, Ubuntu
24.04, glibc 2.39). `bench/c/cbench_stdio.c` builds both against the products and against the
host glibc (`cc -O2`), so the two columns run identical code; medians of three runs, each row
the best of seven rounds. Streams are fully buffered on `/dev/null` (an 8 KiB caller buffer), so
the system call is a small part of each row. `cargo xtask bench`: all 32 budgets met; entry 5's
rows did not move.

### Formatting and stream output (ns per call)

| Call | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `snprintf(buf, 64, "%d", n)` | 83.2 | 509.6 | 6.1 |
| `snprintf(buf, 64, "%s=%d (%5.2f) %x", ...)` | 507.3 | 4,013 | 7.9 |
| `fprintf(f, "%d\n", n)` | 91.5 | 595.9 | 6.5 |
| `fputs`, 32 bytes | 14.2 | 63.7 | 4.5 |
| `putc` (the exported function) | 3.9 | 41.9 | 10.7 |
| `fwrite`, 4 KiB | 221.1 | 478.9 | 2.2 |

**No owner targets exist yet.** The phase-8 exit asks for `printf("%d")` and `fputs` throughput
targets "set in the phase-0 benchmark table", but that table never had them. This phase adds
sanity bounds only (`bench/budgets.tsv`, `cbench_stdio`); the targets are for the owner to set.

Profile (`perf`, the Liella build of the whole benchmark; no call graph, the products have no
frame pointers): zero-filling 17% (`liella_mem::fill_u8`), the heap 18% (`alloc` and `free`, plus
3% vector growth), copying 11%, the format parser 6%, the stream write path the rest. Causes as
far as the code shows:

* Every printf call parses the whole format into heap vectors (pieces, argument classes, the
  fetched arguments) before writing anything, so even `"%d"` allocates and frees several times.
  The first call also sets up the heap (8 MiB of address space), so `snprintf` into a caller
  buffer fails when no memory can be mapped (found by the §2.7 clause tests; the spec does not
  promise that it does not allocate).
* A floating-point conversion clears a 12 KB digit buffer (`num::format::Decimal`, the same
  engine cost as `strtod` in entry 5).
* Each stream operation takes the stream lock and copies the stream state between the C `FILE`
  window and Liella's record (`load`/`store`); that is the likely cost of `putc`, not profiled per row.

### Expected after phase 9

No change to these rows: they are listed below as optimization targets for the optimization pass,
unless the owner sets budgets that require work now.

## Entry 7: after phase 9 (2026-10-02)

Time conversion and zones. The same machine as entries 5 and 6 (the owner's VM, AMD EPYC 7551,
Ubuntu 24.04, glibc 2.39). `bench/c/cbench_time.c` builds both against the products and against
the host glibc (`cc -O2`), so the two columns run identical code; medians of three runs, each
row the best of seven rounds. `cargo xtask bench`: all 37 budgets met (the five new rows are
sanity bounds; the phase has no owner budgets). One phase-7 sanity bound was raised: `isalpha_c`
measured 10.03 ns against a bound of 10, and the phase-8 products, rebuilt and run on the same
day, measured 10.1 ns too, so the machine, not this phase, moved; the bound (set within 7% of
entry 5's 9.36 ns) is now 12 ns.

### Broken-down time, zones, formatting (ns per call)

| Call | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `gmtime_r` | 36.2 | 131.1 | 3.6 |
| `timegm` | 86.0 | 174.9 | 2.0 |
| `localtime_r`, `TZ=UTC0` | 64.2 | 600.8 | 9.4 |
| `localtime_r`, `America/New_York` (2024) | 889.3 | 1,164.2 | 1.3 |
| `localtime_r`, `America/New_York` (2100, the footer rule) | 937.5 | 1,188.1 | 1.3 |
| `localtime_r`, `EST5EDT,M3.2.0,M11.1.0` | 66.7 | 1,186.1 | 17.8 |
| `mktime`, `America/New_York`, `tm_isdst = -1` | 217.9 | 2,437.0 | 11.2 |
| `strftime`, `%Y-%m-%dT%H:%M:%S%z` | 203.5 | 1,168.8 | 5.7 |
| `strftime`, `%c` (C locale) | 386.3 | 1,387.4 | 3.6 |

Profile (`perf`, the Liella build of the whole benchmark): reading `TZ` and `TZDIR` from the
environment on every call to decide whether the zone changed (`getenv` and `strlen`, about 22%);
128-bit division in the calendar arithmetic (`__divti3` and `u128_div_rem`, about 19%); the
transition dates of a POSIX rule computed again on every call (`rule::day_of`, 8%); some heap
traffic; and in `strftime`, a codeset comparison of the locale per call. glibc's `localtime_r`
with a zone file is itself slow here (it checks the zone on every call), which is why that row is
close. Under `cargo xtask bench`, whose environment is much larger, `localtime_r` with `TZ=UTC0`
takes about 1,240 ns instead of 600, which confirms that the environment scan dominates.

### Expected after phase 10

No change to these rows: they are listed below as optimization targets for the optimization pass,
unless the owner sets budgets that require work now.

## Review before phase 10 (2026-10-02; plan rules P8, P9)

The owner's standing performance rule (P9): before each phase, every profiled gap of the
completed phases is either closed by a small, targeted fix or deferred to the optimization pass.
Same machine and method as entries 5–7; Liella before → after, with glibc for reference
(ns per call). `cargo xtask bench`: all 37 budgets met. The first run reported
`bench_heap/threads16_64` over budget because glibc's reference measured 0.80 ns that run; five
further runs gave 1.63–2.38 ns for the reference and a steady 2.1 ns for Liella (noise, not a
regression; no heap code changed), and the rerun met every budget.

| Gap | Before | After | glibc | Fix (or why deferred) |
|---|---:|---:|---:|---|
| `strtod`, `"3.14159"` | 2,268 | 88 | 117 | Fixed: inputs of at most 19 significant digits skip the 12 KB digit buffer, and the classic exact fast path (binary64, round to nearest, value ≤ 2^53, exponent within ±22: one correctly rounded IEEE operation) handles exact cases; 200,000 random inputs agree with Rust's correctly rounded parser. |
| `isalpha`, C | 9.36 | 5.09 | 2.37 | Fixed in part: the built-in locales read the "C" class table (one load) instead of computing the class. The rest is finding the thread's locale per call. |
| `putc` | 41.9 | 15.0 | 3.9 | Fixed in part: a fully buffered stream with room in its write window takes the bytes there directly, as header-inline `putc` does; the stream lock is skipped while Liella knows the process has one thread (P5). The rest is finding the stream's record per call. |
| `fputs`, 32 bytes | 63.7 | 45.4 | 14.2 | As `putc`. |
| `fwrite`, 4 KiB | 478.9 | 358.7 | 221.1 | Lock skip only (4 KiB is larger than the window path). |
| `snprintf("%d")` | 509.6 | 474.3 | 83.2 | Deferred. Holding the parsed format inline instead of in heap vectors was tried and measured slower (676 ns: a piece is about 100 bytes, so the copies cost more than the allocations); the remedy is architectural (parse into caller storage or render in one pass). |
| `fprintf("%d\n")` | 595.9 | 508.8 | 91.5 | Lock skip only; as `snprintf`. |
| mixed format with a double | 4,013 | 3,976 | 507 | Deferred: the float path clears a 12 KB `Decimal` per conversion; the remedy (an exact fast path for doubles, or digits generated on demand) touches the formatting engine. |
| `gmtime_r` | 131.1 | 47.8 | 36.2 | Fixed: 64-bit calendar arithmetic for every `time_t` (128-bit only to normalise arbitrary fields). |
| `timegm` | 174.9 | 77.1 | 86.0 | As `gmtime_r`. |
| `localtime_r`, UTC | 600.8 | 89.3 | 64.2 | Fixed: `TZ`/`TZDIR` read again only when the environment changes. |
| `localtime_r`, New York | 1,164 | 297.5 | 889.3 | As above (now faster than glibc). |
| `localtime_r`, POSIX rule | 1,186 | 302.0 | 66.7 | Fixed in part: 64-bit rule dates. The rest is computing six transitions per call (deferred: see P8 below). |
| `mktime`, New York | 2,437 | 447.3 | 217.9 | Fixed: no `stat` per call for a zone `TZ` names (§23.6 asks it only for `/etc/localtime`), plus the above. |
| `strftime`, ISO 8601 | 1,169 | 612.7 | 203.5 | Fixed in part: encodings found only for text fields, no allocation for numbers, 64-bit digits, directive tables. The rest is the general field engine (deferred). |
| `strftime`, `%c` | 1,387 | 1,028 | 386.3 | As above. |
| `strcoll`, built-in locales | 24.1 | unchanged (28.4 in the `cargo xtask bench` run) | 6.1 | Deferred: both strings are measured before comparing (and `strcmp` does the same); one pass needs a new C-layer compare scanner, which rule P7 holds to exhaustive boundary tests. |
| `iconv`, non-ASCII | 4.1–4.4× | unchanged | | Deferred: specialised conversion loops are significant work. |
| `malloc`/`free`, 1 MiB | 1.83× | unchanged | | Deferred: within its 2× budget; not profiled. |

## Entry 8: after phase 10 (2026-10-03)

Name services and databases. The same machine as entries 5–7 (the owner's VM, AMD EPYC 7551,
Ubuntu 24.04, glibc 2.39). `bench/c/cbench_netdb.c` builds both against the products and against
the host glibc (`cc -O2`), so the two columns run identical code; medians of three runs, each
row the best of seven rounds. Nothing reaches the network: the numeric rows set `AI_NUMERICHOST`
(or `NI_NUMERICHOST|NI_NUMERICSERV`), `localhost` is in the VM's `/etc/hosts`, the user and
service rows read the VM's `/etc` files (read-only), and the resolver rows only build and parse
messages. The program checks every call once before timing it, so no row times an error path.

**What each library consults on this VM** (`nsswitch.conf`: `hosts: files dns`,
`passwd`/`group: files systemd`, `services: db files`; the system calls observed with `strace`):
`localhost` and `root` are found by `files` in both, so neither reaches `dns` or `systemd`. For
`services`, Liella treats `db` as unavailable (a module source, phase 14) and reads
`/etc/services`; the reference makes no per-call system call for `db` either and reads the same
file. Both check `nsswitch.conf` and `resolv.conf` with `stat` on a host lookup; before the
review below, Liella also checked `host.conf` on every lookup of any database, and all three
files for numeric `getaddrinfo`/`getnameinfo`, where the reference makes no system call.

### Address conversion, lookups, resolver messages (ns per call)

| Call | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `inet_pton`, IPv4 | 29.96 | 39.16 | 1.31 |
| `inet_pton`, IPv6 | 76.54 | 127.67 | 1.67 |
| `inet_ntop`, IPv4 | 227.71 | 54.58 | 0.24 |
| `inet_ntop`, IPv6 | 440.41 | 73.21 | 0.17 |
| `inet_aton` | 48.31 | 32.86 | 0.68 |
| `getaddrinfo`, `AI_NUMERICHOST`, IPv4 (+ `freeaddrinfo`) | 133.04 | 5,969 | 44.9 |
| `getaddrinfo`, `AI_NUMERICHOST`, IPv6 (+ `freeaddrinfo`) | 155.62 | 5,966 | 38.3 |
| `getaddrinfo("localhost")`, `AF_INET`, files (+ `freeaddrinfo`) | 8,813 | 10,833 | 1.23 |
| `getnameinfo`, `NI_NUMERICHOST\|NI_NUMERICSERV` | 306.0 | 5,743 | 18.8 |
| `getpwnam_r("root")` | 5,214 | 7,257 | 1.39 |
| `getgrgid_r(0)` | 5,149 | 7,035 | 1.37 |
| `getservbyname_r("http", "tcp")` | 9,297 | 13,024 | 1.40 |
| `res_mkquery`, `www.example.com` A | 1,627 | 2,068 | 1.27 |
| `dn_comp`, against an earlier name | 55.48 | 389.5 | 7.02 |
| `dn_expand` | 45.16 | 80.42 | 1.78 |

No IDNA row: without DNS the conversion is reached only by a non-ASCII name with `AI_IDN |
AI_NUMERICHOST`, and there the libraries do different work (the reference converts the name
first, about 5.7 µs; Liella tries it as a numeric address first and fails without converting,
with the same `EAI_NONAME`). A row comparing the same work needs a non-ASCII name in
`/etc/hosts`, which the benchmark does not write.

The `inet_pton` IPv4 row moves with code layout alone: the same Liella source measured 37–55 ns
across rebuilds this session (the IPv6 row 105–143 ns, in the other direction), and the
reference's own binary 30 or 41 ns depending on the benchmark build.

Profile (`perf` and `strace`, the Liella build of the benchmark):

* **Numeric `getaddrinfo` and `getnameinfo`** built the full lookup context on every call:
  `stat` of `nsswitch.conf`, `host.conf` and `resolv.conf` (about 1–1.5 µs each on this VM),
  decoding the switch configuration, and preparing the `dns` resolver, none of which a numeric
  call uses.
* **`getpwnam_r`, `getgrgid_r`, `getservbyname_r`**: the switch decoded twice per call (the
  file's lines, then the empty `__nss_configure_lookup` overrides; `nss::load` 10–16%), a `stat`
  of `host.conf` that only host lookups need, and the whole file read before searching (12.8 KB
  of `/etc/services`, four `read`s, where the record is in the first 4 KiB).
* **`getaddrinfo("localhost")`**: system calls dominate (three `stat`s, then the hosts file);
  the reference makes about as many, but checks `host.conf` only once. The rest is decoding the
  switch and copying the lookup context (the switch configuration, 1.3 KB, by value).
* **`dn_comp`**: two heap vectors for the compression list per call and a 1 KB label table
  cleared per call (`name::pack`), then the generic copies.
* **`res_mkquery`**: a `getrandom` system call for each query ID (the reference makes none per
  call, observed), plus the `stat` of `resolv.conf` both make.

### Expected after phase 11

No change to these rows. Phase 11 (processes and terminals) adds no hot path to them; its own
measurements (`posix_spawn` and `fork` with `wait`, `isatty`, `tcgetattr`) start in entry 9.

## Review before phase 11 (2026-10-03; plan rules P8, P9)

Same machine and method as entry 8; Liella before → after (after: medians of three
`cargo xtask bench` runs), glibc for reference (ns per call, from a run in the same session as
the after column). `cargo xtask bench`: all 46 budgets met in each of the three runs; no
pre-existing row moved beyond its earlier range. Every test file covering the changed code
passes in both passes (plain and `export,selftest`), with `netdb_products`, `users_products`,
`dns_products` and `netaddr_products`.

| Gap | Before | After | glibc | Fix (or why deferred) |
|---|---:|---:|---:|---|
| `getaddrinfo`, numeric IPv4 | 5,969 | 463.7 | 133.1 | Fixed in part: a call that looks up no name and no service (`gai::uses_switch`: no node, an empty or numeric node, `AI_NUMERICHOST`; no service, a port, `AI_NUMERICSERV`) runs with a context that reads no configuration file (`Ctx::bare`); `_res_hconf` is still filled at the first call. The rest is deferred: the context carries the 1.3 KB switch configuration by value, and the result is built through four or five heap vectors before the caller's block. |
| `getaddrinfo`, numeric IPv6 | 5,966 | 405.0 | 160.1 | As above. |
| `getnameinfo`, numeric | 5,743 | 270.4 | 305.6 | Fixed: as above (`gai::ni_uses_switch`); now faster than glibc. |
| `getpwnam_r("root")` | 7,257 | 5,219 | 5,188 | Fixed: `host.conf` is checked by host lookups only (`Ctx::with_multi`), and the `__nss_configure_lookup` overrides are decoded only once one exists. Parity. |
| `getgrgid_r(0)` | 7,035 | 4,975 | 5,128 | As `getpwnam_r`. |
| `getservbyname_r("http", "tcp")` | 13,024 | 8,244 | 9,252 | Fixed: as `getpwnam_r`, and the `files` source stops reading at the record found (`liella::system::find_line`, the same lines in the same order as before). Now faster than glibc. |
| `dn_comp` | 389.5 | 185.1 | 55.45 | Fixed in part: the compression list lives on the stack up to 32 entries, and the label table holds 16-bit offsets (256 bytes to clear instead of 1 KB). The rest is deferred: the text goes through a 256-byte wire buffer, then is packed with generic copies; packing straight from the text is an engine change. |
| `getaddrinfo("localhost")` | 10,833 | 10,953 | 8,845 | Deferred (P8 below): three configuration files checked with `stat` per host lookup, and the hosts file read and parsed per lookup. Checking `host.conf` once, as the reference does, would change behaviour (an edited file is read again today). |
| `res_mkquery` | 2,068 | 2,024 | 1,638 | Deferred: one `getrandom` per query ID. A generator seeded once would remove it, but the source of DNS query IDs is a security decision for the owner, not a small fix. |
| `dn_expand` | 80.42 | 74.24 | 44.74 | Deferred: the name is unpacked into a 256-byte buffer, then written as text; one pass is the same engine change as `dn_comp`. |
| `inet_pton`, IPv6 | 127.7 | 141.1 | 77.53 | Deferred: within code-layout variance (105–143 ns over this session's builds, no change on its path); the text is measured (`strlen`) before parsing (P2). |

## Entry 9: after phase 11 (2026-10-03)

Processes and terminals. The same machine as entries 5–8 (the owner's VM, AMD EPYC 7551,
Ubuntu 24.04, glibc 2.39, kernel 6.8, 16 vCPUs). `bench/c/cbench_process.c` builds both against
the products and against the host glibc (`cc -O2`), so the two columns run identical code;
medians of three runs, each row the best of seven rounds, both builds run in the same shell (so
their children inherit the same environment). The program checks every call once before timing
it and reaps every child; the two rows that time a failure (`waitpid` with no child, `isatty` on
`/dev/null`) check that failure. The terminal rows use a pty pair from `openpty`.

**The spawn rows are dominated by the kernel and the child program**, which are the same for both
libraries: `/bin/true` (and `/bin/sh` for `system` and `popen`) is the system's dynamically linked
program, and on this VM creating, executing and reaping a process costs 0.2–0.6 ms. Compare their
ratios, not their absolute values, and expect ±15% between runs (process creation on this VM
varies with CPU placement; pinned to one CPU, `fork` rows repeat within a few percent). Both
libraries make the same system calls in the parent for each of these rows (`strace`): the
reference's `posix_spawn` maps a 36 KiB stack for a `CLONE_VM` child, Liella's maps a shared
page for the child's error word and creates the child without `CLONE_VM` (D-18.1, D-18.3); `system`
and `popen` add the same signal and pipe calls in both.

### Spawning, waiting, terminals (ns per call)

| Call | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `posix_spawn("/bin/true")` + `waitpid` | 505,103 | 592,139 | 1.17 |
| the same with three file actions (open, `dup2`, close) | 513,908 | 580,117 | 1.13 |
| `fork`, `_exit` in the child, `waitpid` | 211,617 | 170,283 | 0.80 |
| `fork`, `_exit`, `waitpid`, pinned to one CPU (`taskset`; a probe of 2,000 forks, median of five) | 109,424 | 135,237 | 1.24 |
| `vfork`, `_exit` in the child, `waitpid` | 68,307 | 203,108 | 2.97 |
| `system("exit 0")` | 577,672 | 705,813 | 1.22 |
| `popen("true", "r")` + `pclose` | 592,846 | 724,601 | 1.22 |
| `posix_spawn("/bin/true")` + `waitpid`, caller with 256 MiB of touched heap | 502,872 | 620,972 | 1.23 |
| `waitpid(-1, WNOHANG)`, no child (`ECHILD`) | 207.2 | 218.6 | 1.06 |
| `tcgetattr` | 264.2 | 274.8 | 1.04 |
| `tcsetattr(TCSANOW)`, the current settings | 1,131.5 | 895.6 | 0.79 |
| `cfsetspeed` + `cfgetospeed` | 19.04 | 11.87 | 0.62 |
| `openpty` + two `close`s | 28,935 | 28,878 | 1.00 |
| `ttyname_r` on a pty | 3,910 | 4,699 | 1.20 |
| `isatty`, pty | 270.2 | 267.3 | 0.99 |
| `isatty`, `/dev/null` | 125.9 | 126.1 | 1.00 |

`vfork` is meaningful to measure here because the spec defines it (D-18.1): the child gets its
own copy-on-write memory instead of the caller's, so a correct program behaves identically and an
incorrect one becomes safe, at the price this row shows (F24): the kernel copies the caller's
page tables, which the reference's `CLONE_VM` child does not. The `fork` row unpinned shows Liella
faster, which is placement noise; pinned, the reference is faster.

Profile (`perf`, `strace` and page-fault counts from `getrusage`, the Liella build):

* **`posix_spawn`, `system`, `popen`, `vfork`**: the child is created without `CLONE_VM`
  (D-18.1, D-18.3), so the kernel duplicates the caller's address space (its mappings and the
  page tables of every touched private page) and discards the copy at `execve`; the caller
  then takes a copy-on-write fault for each page it writes until the child has executed (3 per
  spawn, the reference 0). The system calls are otherwise the reference's, plus one `mmap` and
  `munmap` of the shared error page per spawn. The cost grows with the caller's resident
  memory: with 256 MiB of touched heap (65,536 pages, no transparent huge pages on this VM's
  `madvise` setting) Liella's spawn costs about 30–110 µs more than with a small heap across
  runs, the reference's nothing (D-18.3's performance note).
* **`fork`**: the same system calls; Liella's *prepare* step takes about twenty library locks (64
  heap shards among them) and releases them after `clone`, so the parent writes those pages
  while they are shared copy-on-write: 9 page faults per `fork` in the parent against the
  reference's 3, and the child's resets (thread list, timers, `getaddrinfo_a` queue, stdio locks,
  heap caches, `popen`/`system` state) write about 24 pages against the reference's 10.
* **`ttyname_r`**: the same four system calls as the reference (`TCGETS2`, `fstat`, `readlink`
  of `/proc/self/fd/N`, `stat` of the name); Liella also clears and copies three 4 KiB path
  buffers per call (`liella_mem::fill_u8` 3.5% of the whole benchmark's profile).
* **`tcsetattr`**: Liella reads the settings, sets them and, only when they changed, reads them
  again (D-19.1); the reference reads them after every set, so with unchanged settings Liella
  is faster.

### Expected after phase 12

No change to these rows. Phase 12 (directories, walks, `realpath`, `fnmatch`, `glob`, regular
expressions, `wordexp`) reaches them only through `wordexp`'s command substitution, which runs
the shell as `popen` does; its own measurements (directory streams, `nftw`, `realpath`,
`fnmatch`, `regexec`, `glob`) start in entry 10.

## Review before phase 12 (2026-10-03; plan rules P8, P9)

Same machine and method as entry 9; Liella before → after (after: medians of three
`cargo xtask bench` runs), glibc for reference (ns per call, from runs in the same session).
`cargo xtask bench`: all 61 budgets met in each of the three runs (the 15 new rows are sanity
bounds, about 5–10× Liella's value; phase 11 has no owner budgets); no pre-existing row moved
beyond its earlier range. Under `cargo xtask bench` the spawn rows are 5–10% slower than in the
shell runs of entry 9 (the children inherit Cargo's larger environment; the reference was not run
there). Every test file covering the changed code passes in both passes (plain and
`export,selftest`), with `process_products` and `terminals_products`.

| Gap | Before | After | glibc | Fix (or why deferred) |
|---|---:|---:|---:|---|
| `ttyname_r` | 4,699 | 3,945 | 3,910 | Fixed: the link is read into a 64-byte buffer first (a name that does not fit is read again into `PATH_MAX` bytes, `term::name_in`) and terminated in place for the `stat`, instead of clearing a 4 KiB buffer and copying the name into a second one (`KPathBuf`). Parity. `ttyname` shares the path. |
| `fork` + `_exit` + `waitpid`, pinned to one CPU | 135,237 | 122,388 | 109,424 | Fixed in part: while the caller is the process's only thread (`thread::alone`, checked after the *prepare* handlers), `fork` takes and releases no library lock (P5): no other thread can hold one, and the parent no longer writes about twenty lock words while their pages are shared copy-on-write (page faults in the parent per `fork`: 9 → 2; the reference 3). The unpinned row is within its noise (170,283 → 171,683; runs 168–235 µs on both sides). The rest is deferred: the child's resets write about 24 pages, the reference's child 10 (below). |
| `posix_spawn` + `waitpid` | 592,139 | 627,764 (`xtask`) | 505,103 | Deferred: no change on its path (the `xtask` column carries Cargo's environment). The cost is the child created without `CLONE_VM` (D-18.1, D-18.3): the address space duplicated and dropped at `execve`, 3 copy-on-write faults in the caller per spawn, and a shared page mapped and unmapped per spawn for the child's error. The remedy is D-18.3's performance target, a `CLONE_VM` child on its own stack, which touches the unsafe policy (the child would share the caller's memory): architectural. |
| `system`, `popen` + `pclose` | 705,813, 724,601 | 743,452, 749,146 (`xtask`) | 577,672, 592,846 | Deferred: as `posix_spawn` (the same system calls as the reference otherwise). |
| `posix_spawn`, 256 MiB caller | 620,972 | 693,286 (`xtask`) | 502,872 | Deferred: as `posix_spawn`; this is the cost that grows with the caller's resident memory (P8 below). |
| `vfork` | 203,108 | 160,849 | 68,307 | Deferred: no change on its path (runs 139–214 µs on both builds). The copy-on-write child is D-18.1's decision (F24); the `CLONE_VM` mechanism above would serve it too, but what `vfork` shares is the owner's decision. |
| `waitpid` (`ECHILD`), `tcgetattr` | 218.6, 274.8 | 217.9, 273.4 | 207.2, 264.2 | No action: within 5%, one system call each as in the reference (not profiled further). |

## Entry 10: after phase 12 (2026-10-03)

File system (high level) and patterns. The same machine as entries 5–9 (the owner's VM, AMD EPYC
7551, Ubuntu 24.04, glibc 2.39, kernel 6.8, 16 vCPUs). `bench/c/cbench_fstree.c` (new) and
`bench/c/cbench_patterns.c` (new `fnmatch`, `glob` and `wordexp` rows beside phase 12's regex
rows) build both against the products and against the host glibc (`cc -O2`), so the two columns
run identical code; medians of three runs, each row the best of seven rounds (five for the
pattern rows), both builds run in the same shell. Every call is checked once before it is timed.

**Scratch directories.** Each program creates its own directory under `/tmp` (`mkdtemp`) and
removes it at the end by the names it created; nothing outside it is touched. On this VM `/tmp`
is on the root file system: **ext4** (`rw,relatime`, `dir_index`, `metadata_csum`) on LVM.
Directory order and `getdents64` costs depend on it: with `dir_index` a directory is read in hash
order through the kernel's htree code, which dominates the directory rows (about 340 ns per entry
in both libraries). The tree is `flat` (1,000 empty files), `tree` (10 directories of 99 files:
1,000 entries below the root), `p/q/r` (with `/tmp/<scratch>`, a 5-component path) and a 50-line
mount table. `wordexp` runs no shell (`WRDE_NOCMD`; the arithmetic row has no command
substitution).

### Directories, walks, paths, mount tables, patterns (ns per call)

| Call | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `opendir` + `readdir` to the end + `closedir`, 1,000 files | 348,940 | 344,606 | 0.99 |
| `readdir`, per entry (an open stream, `rewinddir` then to the end) | 342.7 | 340.1 | 0.99 |
| `getcwd` | 488.8 | 523.4 | 1.07 |
| `realpath`, 5 components, no link | 3,999 | 14,591 | 3.65 |
| `scandir` + `alphasort`, 1,000 files (entries freed) | 552,568 | 695,716 | 1.26 |
| `nftw`, `FTW_PHYS`, 1,000 entries | 1,311,808 | 1,454,968 | 1.11 |
| `fts_open`/`fts_read`/`fts_close`, `FTS_PHYSICAL`, the same tree | 1,414,838 | 2,098,408 | 1.48 |
| `mkstemp` + `unlink` + `close` | 19,098 | 19,336 | 1.01 |
| `setmntent` + 50 × `getmntent` + `endmntent` | 21,475 | 49,934 | 2.33 |
| `fnmatch("*.c", 17 bytes)` | 42.26 | 842.7 | 19.9 |
| `fnmatch("[a-m]*_[!0-9]*.[ch]")` | 95.56 | 1,796 | 18.8 |
| `fnmatch("src/*/x*.c", FNM_PATHNAME)`, a match | 63.14 | 1,557 | 24.7 |
| `fnmatch("src/*.c", FNM_PATHNAME)`, no match | 32.36 | 1,093 | 33.8 |
| `glob("dir/f*")`, 1,000 matches, sorted (+ `globfree`) | 637,278 | 1,124,250 | 1.76 |
| the same, `GLOB_NOSORT` | 457,821 | 986,698 | 2.16 |
| `wordexp("~/a $HOME b", WRDE_NOCMD)` (+ `wordfree`) | 399.9 | 2,925 | 7.31 |
| `wordexp("x $((3*(4+5)-7)) y", WRDE_NOCMD)` | 446.0 | 3,616 | 8.11 |
| `regcomp` + `regexec` + `regfree`, `([a-z]+)@([a-z]+)\.com`, 1 KiB | 61,268 | 375,408 | 6.13 |
| `regexec`, a literal in 1 KiB, `REG_NOSUB` | 3,303 | 25,995 | 7.87 |
| `regexec`, the address pattern with three subexpressions, 1 KiB | 48,392 | 365,609 | 7.56 |
| `regexec`, `[[:digit:]]+` absent from 1 KiB | 783.4 | 2,619 | 3.34 |
| `regexec`, `\(b[a-z]*\)@\1` | 3,033 | 8,626 | 2.84 |
| `regexec`, `(a\|aa)*c` on 4,096 `a`s (no match) | 62,077,873 | 2,437,846 | 0.04 |

`wordexp` with `WRDE_NOCMD` crashed in Liella before these rows could run (fixed in the review
below): a successful call without `WRDE_DOOFFS` left `we_offs` as the caller's structure held it,
so `wordfree` freed from the wrong slots when the structure was not zeroed (here a stack slot that
had held `glob`'s `gl_pathc` of 1,000).

Profile (`perf` as root on the Liella build of per-row loop programs, no call graph; `strace -c`
for the system calls):

* **Directory streams, `getcwd`, `mkstemp`**: the same system calls as the reference; parity.
* **`realpath`**: 17 system calls against the reference's 5. Liella resolves through `O_PATH`
  descriptors (D-13.5, no length limit): per component `fstatat`, `openat` and `close`, the last
  directory opened too; the reference makes one `readlink` per component on the whole path.
  Kernel `memset` (zeroing a new `struct file` per `openat`) was the largest single symbol.
* **`getmntent`**: an `lseek` per entry (the stream's position recorded before each line in case
  `getmntent_r` must go back to it, which only its `ERANGE` path uses); then the line parsed into a
  vector grown byte by byte, and the four strings copied into the thread's result block.
* **`fnmatch`**: about 37% in the heap: every call decodes the pattern and the string into
  vectors of characters and byte offsets grown by `push`, compiles the pattern into items, and
  allocates a position set (a heap bit vector) per item; the rest is the position-set matcher
  (D-14.4 style: no backtracking) itself, where the reference matches the bytes directly.
* **`glob`**: the same `fnmatch` costs per directory entry, three allocations per match (the name,
  the path cloned then grown, the caller's copy), and the sort's `strcoll` (both strings measured
  first: the known `strcoll` target).
* **`scandir`**: the system calls match; the gap is `alphasort`'s `strcoll` (`strlen` of both
  names, then the comparison).
* **`nftw`**: the same 1,001 `fstatat`s and 22 `getdents64`; about 150 ns of user time per entry
  more than the reference.
* **`fts`**: the same system calls; about 550 ns of user time per entry more. Liella keeps the
  tree in Rust nodes and mirrors each into the caller-visible `FTSENT` block (`Fts::write`, written
  when built and again when visited, with the 144-byte `stat` copied), plus a path and a name vector
  per node.
* **`wordexp`**: the heap (about 33%), then the environment scans (`HOME` twice) and the expander:
  the words go through per-word segment vectors, piece vectors and field vectors, each a heap copy;
  arithmetic adds the operator search (string comparisons against 39 spellings per token and the
  10 precedence levels).
* **Regular expressions**: as recorded in phase 12 (the overall-match pass simulates threads per
  byte; see "Optimization targets").

### Expected after phase 13

No change to these rows. Phase 13 (syslog, message queues, shared memory, AIO, message catalogs,
allocator introspection) adds no hot path to them; its own measurements (`syslog` to a socket,
`mq_send`/`mq_receive`, `aio_read` completion, `gettext` lookups) start in entry 11.

## Review before phase 13 (2026-10-03; plan rules P8, P9)

Same machine and method as entry 10; Liella before → after (after: medians of three
`cargo xtask bench` runs; before: the phase-12 code, three runs of the same programs), glibc for
reference (ns per call). `cargo xtask bench`: all 93 budgets met in each of the three runs (the 17
new rows are sanity bounds, about 5–10× Liella's value; phase 12 has no owner budgets); no
pre-existing row moved beyond its earlier range. Under `cargo xtask bench` the `wordexp` rows are
slower than in the shell (3.7 and 4.0 µs against 2.9 and 3.6): `getenv` scans Cargo's larger
environment, as `localtime_r` did in entry 7. Every test file covering the changed code passes in
both passes (plain and `export,selftest`), five times, with `fstree_products`,
`patterns_products`, `mntent_products` and `stdio_products`.

| Gap | Before | After | glibc | Fix (or why deferred) |
|---|---:|---:|---:|---|
| `realpath`, 5 components | 14,591 | 8,634 | 3,999 | Fixed in part (`fs::canon::resolve`): a component with more to resolve is opened at once with `O_PATH\|O_DIRECTORY\|O_NOFOLLOW` (anything but a directory, a symbolic link included, is refused with `ENOTDIR` and then examined by `fstatat` as before; other errors are the ones `fstatat` gives), and a final directory is no longer opened: 17 system calls → 11. The rest is deferred: the descriptor walk itself (D-13.5) against the reference's one `readlink` per component. |
| `getmntent`, 50 lines | 49,934 | 32,935 | 21,475 | Fixed in part: no `lseek` per entry (`getmntent_r` goes back by the line's length, from the position after it, only on `ERANGE`); the line is parsed into a vector sized once and copied in runs between backslashes (`mnttab::parse`, `decode`). The rest is deferred: two vectors per line and the copy into the result block. |
| `fnmatch`, `*.c` | 842.7 | 530.7 | 42.26 | Fixed in part: the position sets of strings up to 255 characters live in place (`Bits`, no heap), the pattern and string vectors are sized once, the string's byte offsets (unused by the matcher) are no longer built, the item list is sized once. The rest is deferred: compiling and matching by position sets per call. |
| `fnmatch`, bracket | 1,796 | 1,056 | 95.56 | As above. |
| `fnmatch`, `FNM_PATHNAME` | 1,557 | 955.0 | 63.14 | As above. |
| `fnmatch`, `FNM_PATHNAME`, no match | 1,093 | 674.8 | 32.36 | As above. |
| `glob`, 1,000 matches, sorted | 1,124,250 | 948,142 | 637,278 | Fixed in part: the `fnmatch` change, and each path built in one allocation (`glob::Walk::expand`) instead of a clone that then grows. The rest is deferred: the per-entry `fnmatch` cost above, three allocations per match, and the sort's `strcoll` (the existing `strcoll` target). |
| `glob`, `GLOB_NOSORT` | 986,698 | 810,290 | 457,821 | As above. |
| `wordexp`, tilde and variable | 2,925 | 3,712 (`xtask`; 2,915 in the shell) | 399.9 | Deferred: no change on its path. The expander builds per-word segment, piece and field vectors, each a heap copy of its text; the remedy (fields as ranges into the input and one output buffer per call) is an engine change. |
| `wordexp`, arithmetic | 3,616 | 3,989 (`xtask`; 3,600 in the shell) | 446.0 | Deferred: as above, plus the operator search (39 spellings compared as strings per token, 10 precedence levels compared as strings); tokens as small integers is a rewrite of the evaluator's interface. |
| `fts`, 1,000 entries | 2,098,408 | 2,114,292 | 1,414,838 | Deferred: no change on its path. The tree lives in Rust nodes mirrored into the caller's `FTSENT` blocks (each written when built and again when visited, the `stat` copied each time), with a path and a name vector per node; building each `FTSENT` once, in place, is a change of the walk's data structure. |
| `scandir` + `alphasort` | 695,716 | 708,362 | 552,568 | Deferred: the system calls match; the gap is `strcoll` measuring both strings before comparing (the existing `strcmp`/`strcoll` target, a C-layer scanner under P7). |
| `nftw` | 1,454,968 | 1,445,587 | 1,311,808 | No action: 1.10×, the same system calls; the remaining user time (about 150 ns per entry) was not profiled further. |
| Regular expressions | 2.8–7.9× | unchanged | | Deferred (recorded in phase 12): the lazy DFA for the overall-match pass (D-14.4). |
| `getcwd`, directory streams, `mkstemp` | 0.99–1.07× | unchanged | | No action: parity, the same system calls. |

## Entry 11: after phase 13 (2026-10-04)

Remaining services: message catalogs, POSIX message queues and shared memory, the system logger,
POSIX AIO and the allocator's introspection. The same machine as entries 5–10 (the owner's VM, AMD
EPYC 7551, Ubuntu 24.04, glibc 2.39, kernel 6.8, 16 vCPUs). `bench/c/cbench_services.c` (new) and
`bench/c/cbench_aio.c` (phase 13's own rows) build both against the products and against the host
glibc (`cc -O2`), so the two columns run identical code; medians of three runs, each row the best
of seven rounds, both builds run in the same shell (the AIO rows: Liella under `cargo xtask bench`,
the reference in the shell). Every call is checked once before it is timed; a failure ends the
program.

**Scratch objects.** `cbench_services` makes its own directory under `/tmp` (`mkdtemp`) holding a
fixture `.mo` (Appendix D.2.1, revision 0, no hash table: the header entry with
`charset=UTF-8` and `Plural-Forms: nplurals=2; plural=n != 1;`, a plural entry and a singular one)
at `en_US/LC_MESSAGES/cbench.mo` and a fixture `.cat` (D.2.2, plane size and depth 1), plus a
message queue and a shared-memory object named after its process ID; an `atexit` handler removes
all of them, also after a failed check. The catalog rows run in `en_US.UTF-8` (the VM's locale
archive) with `LANGUAGE` unset, so the search tries `en_US.UTF-8`, `en_US.utf8`, then finds
`en_US`. **The logger rows send nothing**: an unprivileged program cannot point `syslog` at a
socket of its own (the channels are `/dev/log` and `/dev/console`), so the rows are `vsyslog` with a
priority the mask excludes (`setlogmask(LOG_MASK(LOG_EMERG))`, `LOG_DEBUG`: the masked fast path)
and `openlog` without `LOG_NDELAY` followed by `closelog` (no connection is made). `malloc_info`
writes to a stream on `/dev/null`.

### Catalogs, queues, shared memory, logger, allocator introspection, AIO (ns per call)

| Call | glibc | Liella | Ratio |
|---|---:|---:|---:|
| `gettext`, a msgid the catalog holds | 66.84 | 7,866 | 118 |
| `gettext`, a msgid the catalog lacks | 294.8 | 8,476 | 28.8 |
| `ngettext(…, 3)`, a plural entry | 87.39 | 7,925 | 90.7 |
| `catgets`, a message the `.cat` holds | 4.74 | 10.22 | 2.16 |
| `mq_send` + `mq_receive`, 4 bytes | 853.6 | 883.3 | 1.03 |
| `shm_open` + `ftruncate` + `mmap` + `munmap` + `close` + `shm_unlink`, 4 KiB | 12,122 | 13,692 | 1.13 |
| `vsyslog`, a priority the mask excludes | 22.76 | 10.81 | 0.47 |
| `openlog` + `closelog` (no connection) | 40.54 | 40.74 | 1.00 |
| `malloc_usable_size`, a 100-byte block | 3.05 | 7.81 | 2.56 |
| `mallinfo2` | 132.6 | 4,678 | 35.3 |
| `malloc_info` to `/dev/null` | 1,922 | 8,709 | 4.53 |
| `pread`, 4 KiB, cached | 495.1 | 512.3 | 1.03 |
| `aio_read` + `aio_suspend` + `aio_return`, 64 bytes | 22,815 | 34,509 | 1.51 |
| the same, 4 KiB | 23,072 | 26,119 | 1.13 |
| `lio_listio(LIO_WAIT)`, 16 × 4 KiB on one descriptor | 38,611 | 62,009 | 1.61 |

The AIO rows are dominated by thread wakes (Liella: the io_uring helper and back; the reference: a
worker and back) and vary by ±30% between runs on this VM on both sides (entry budgets record
23–53 µs for the reference); the list row is the known serialisation per descriptor (§11.11 order;
"linked submissions" in the optimization targets).

Profile (`perf` as root on the Liella build of per-row loop programs, no call graph; `strace -c`
for the system calls):

* **`gettext`, `ngettext`**: 74% in `liella::locale::name::lookup_alias`: every lookup scanned the
  whole alias file (`/usr/share/locale/locale.alias`, loaded once and kept) line by line for the
  locale name, splitting each line into fields; 8% clearing a `PATH_MAX` (4 KiB) buffer for each
  directory variant tried, and another in the boundary for the catalog's `stat`. One `newfstatat`
  per lookup (the catalog checked against its file, D-7.12: no stale answers), about 1.5 µs on this
  VM; the reference makes no system call per lookup.
* **`mallinfo2`**: 99% in `Heap::stats`, which counts the blocks in use from every owned slab's
  allocation bitmap: 256 words per slab whatever the size class (the fast paths keep no counter,
  D-3.10).
* **`malloc_info`**: `Heap::stats` (51%), then the line sink (`StreamLines::put`, 31%), which copied
  the XML text into its line buffer a byte at a time with a bounds check and a newline test per
  byte, and `fputs` per line.
* **`malloc_usable_size`**: the block is located through the segment registry and its liveness
  checked in the slab's bitmaps (an invalid or freed pointer is detected, D-3.4); the reference
  reads the chunk header.
* **`catgets`**: the descriptor's tag is checked and the catalog header parsed per call (D-7.12
  validation), 5 ns over the reference.
* **Queues, shared memory, logger**: the same system calls as the reference; parity (the masked
  `vsyslog` returns before the format is touched, §26.1).

### Expected after phase 14

No change to these rows. Phase 14 (dynamic linking) adds the loader, `dlopen` and the NSS plugin
modules; the static benchmarks do not reach them. Its own measurements (program start through
`ld.so`, `dlopen`/`dlsym`, symbol lookup, TLS access in a loaded object) start in entry 12, and
entries 1–11's rows can then be repeated dynamically linked.

## Review before phase 14 (2026-10-04; plan rules P8, P9)

Same machine and method as entry 11; Liella before → after (after: medians of three
`cargo xtask bench` runs; in the shell, for comparison with the before column, where it differs),
glibc for reference (ns per call). `cargo xtask bench`: all 107 budgets met in each of the three
runs (the 11 new `cbench_services` rows are sanity bounds, about 5–10× Liella's value; phase 13
has no owner budgets); the fixes touch no pre-existing row's path. Under `cargo xtask bench`
the catalog rows are 0.4 µs slower than in the shell (2.8 against 2.4 µs: `LANGUAGE` is looked up
through `secure_getenv` in Cargo's larger environment, as `getenv` in entry 7 and the review before
phase 13). Every test file covering the changed code passes in both passes (plain and
`export,selftest`), five times, with `catalogs_products` and `mallocdbg_products`.

| Gap | Before | After | glibc | Fix (or why deferred) |
|---|---:|---:|---:|---|
| `gettext`, found | 7,866 | 2,808 (`xtask`; 2,422 in the shell) | 66.84 | Fixed in part (`liella_legacy::intl`): the last alias lookup is kept (the name and the answer as a position in the alias table, which is loaded once and never replaced, so the answer cannot go stale; under the module's lock), and the catalog path of each directory variant is built in a 256-byte buffer when it fits (`join`; `PATH_MAX` only beyond), as is the path for the catalog's `stat` (`boundary::catalog::with_path`). The rest is deferred: the `stat` per lookup is D-7.12's no-stale-answers decision (about 1.5 µs here; see P8 below), and building candidate paths and finding the catalog record by path per lookup is the per-(domain, locale) cache already listed. |
| `gettext`, missing | 8,476 | 3,218 (2,820) | 294.8 | As above. |
| `ngettext` | 7,925 | 2,832 (2,470) | 87.39 | As above. |
| `mallinfo2` | 4,678 | 1,520 | 132.6 | Fixed in part (`liella_rt::heap::Heap::live_bits`): only the bitmap words that hold blocks of the slab's class are counted (bits past the last block are always set, `adopt`), so a slab of a large class costs a word or two instead of 256; the result is identical. The rest is deferred: the scan of every owned slab's bitmap remains (small classes use all 256 words); per-slab counts kept readable by others (the owner-only `R_USED` is not exact for the current slab) are a change of the slab protocol. |
| `malloc_info` | 8,709 | 5,485 | 1,922 | Fixed in part: the statistics as `mallinfo2`, and the line sink copies a run at a time up to the next newline or the buffer's end (`boundary::heap::StreamLines::put`; the same lines and the same `fputs` calls). The rest is deferred: an `fputs` per line and the formatting through the generic sink; writing the report in larger pieces would change how an unbuffered stream receives it. |
| `malloc_usable_size` | 7.81 | 7.81 | 3.05 | No action by decision: the pointer is validated (registry and bitmaps, D-3.4) where the reference trusts the chunk header. |
| `catgets` | 10.22 | 10.25 | 4.74 | No action: 5 ns, the descriptor and header validation per call (D-7.12). |
| `shm_open` cycle, `mq_send` + `mq_receive`, logger rows | 1.00–1.13× | unchanged | | No action: the same system calls; the masked `vsyslog` is faster than the reference. |
| AIO rows | 1.13–1.61× | unchanged | | No action: within the VM's variance for thread wakes; the list row is the existing "linked submissions" target. |

## Optimization targets (optimization pass, before the first release)

Gaps that meet every budget but are far from the reference library. The owner scheduled them
for an optimization pass (2026-10-01), moved before the first release on 2026-10-04 (plan P10). Each entry names the measurement, the
cause as far as it is known, and the likely remedy.

| Target | Entry | Gap | Known cause | Likely remedy |
|---|---|---:|---|---|
| `strtod` family, long or inexact inputs | 5 | ~19× (short exact inputs fixed: 0.75×) | Exact big arithmetic for every input beyond the fast path | A correctly rounded fast algorithm for the common inexact case (an Eisel–Lemire-style step), the exact path only as the fallback |
| `iconv`, non-ASCII text | 5 | 4.1–4.4× | A generic decode-then-encode step per character | Specialised loops for the common pairs (UTF-8 to and from UTF-16/32, 8-bit charmaps) |
| `isalpha` and the byte classifiers | 5 | 2.1× (was 4.0×) | The thread's locale found on every call | Keep the current tables' address in the thread block, as `__ctype_b_loc` does |
| `strcmp`, `strcoll` in the built-in locales | 5 | ~4.6× | Both strings measured before comparing | A C-layer single-pass compare scanner (P7, with D-4.14's exhaustive tests) |
| printf family | 6 | 5.6–7.8× | Per-call heap vectors for the parsed format; floats clear a 12 KB digit buffer | Parse into caller storage or render in one pass; an exact fast path for doubles |
| `putc`, `fputs`, `fwrite` | 6 | 1.6–3.8× (was 2.2–10.7×) | The stream's record found per call | Keep the record's address in the `FILE` word the window protocol already reads |
| `localtime_r` with POSIX rules, `mktime` | 7 | 4.5×, 2.1× | Six rule transitions computed per call | Precompute the transitions at load (P8) |
| `strftime` | 7 | 2.7–3.0× | The general field engine | A direct path for numeric directives |
| `malloc`/`free` of 1 MiB | 4 | 1.83× (budget 2×) | Not profiled | Profile the large-block cache path |
| Numeric `getaddrinfo` | 8 | 2.5–3.5× (was 38–45×) | The lookup context carries the 1.3 KB switch configuration by value; the result goes through four or five heap vectors (socket types, addresses, entries) before the caller's block; the node is parsed twice (the switch check, then the lookup) | Contexts that borrow the configuration; fixed-capacity results for the numeric case, written straight into the caller's block |
| Host lookups by name (`getaddrinfo`, `gethostbyname`) | 8 | 1.24× | Three `stat`s per lookup (`nsswitch.conf`, `host.conf`, `resolv.conf`), the switch decoded per lookup, the hosts file read and parsed per lookup | See the P8 rows below |
| `dn_comp`, `dn_expand` | 8 | 3.3×, 1.7× | The name goes through a 256-byte wire buffer and generic copies | Pack from the text and expand to the text in one pass |
| `res_mkquery` | 8 | 1.24× | A `getrandom` system call per query ID | A per-thread generator seeded from `getrandom` once (owner decision: the ID source is a security property) |
| `inet_pton` | 8 | 1.3–1.8× (layout-sensitive) | The text is measured before it is parsed | Parse the C string in one pass (P2) |
| `posix_spawn`, `system`, `popen` | 9 | 1.13–1.22×; 1.23× from a caller with 256 MiB touched, growing with the caller's resident memory | The child is created without `CLONE_VM` (D-18.1, D-18.3): the kernel duplicates the address space and discards it at `execve`, and the caller takes a copy-on-write fault for each page it writes meanwhile; one shared page is mapped and unmapped per spawn for the child's error | D-18.3's performance target: a `CLONE_VM` child on its own stack, with the error word in the caller's memory. Architectural: the child then shares the caller's memory, which the unsafe policy (S1–S2) and the child's code path (no allocation, no locks, nothing that writes the caller's state) must allow |
| `vfork` | 9 | 3.0× | Copy-on-write child by decision (D-18.1, F24) | The same `CLONE_VM` mechanism; not worked on in the optimization stage: it touches the unsafe policy, so it goes to the owner as a ruling with a recommendation (P10 (11), 2026-10-04) |
| `fork` | 9 | 1.12× pinned (was 1.24×; parity or better unpinned, within noise) | The child's resets write about 24 pages (the reference's child 10): unconditional stores of zero to lock words and lists in five modules, and the heap's cache parking, each a copy-on-write fault while the parent shares the page | Store only the words that change, or keep the state `fork` resets together on a few pages |
| Name-service cache attach (second lookup of a process) | nscache | +40 µs over uncached (region exists), +70 µs (created) | 13 system calls: four ID queries, open/fstat/mmap, the boot ID, five validation stats | Fold the ID queries into `getresuid`/`getresgid`; read the boot ID once per region, not per attach (D-16.10) |
| Regex overall-match pass | 10 | ~7× glibc on dense text | Breadth-first NFA simulation for pass 1 | A lazy DFA for the overall match (D-14.4) |
| `fnmatch` | 10 | 11.0–20.9× (was 18.8–33.8×) | Every call decodes both texts into character vectors, compiles the pattern into items and matches by position sets, item by item | A direct matcher on the bytes for patterns without extended groups (the classic single-star backtracking, linear for `*` and `?`), the position-set matcher kept for `FNM_EXTMATCH` (`glob` already compiles each component once per directory level) |
| `glob` | 10 | 1.5–1.8× (was 1.8–2.2×) | The `fnmatch` cost per directory entry; three allocations per match (name, path, the caller's copy); the sort's `strcoll` | The `fnmatch` remedy; names and paths written once into the caller's block; the `strcoll` remedy below |
| `wordexp` | 10 | 7.3–9.3× | Per-word segment, piece and field vectors, each a heap copy; arithmetic operators found by string comparison | Fields as ranges into the input, one output buffer per call; operators tokenized to small integers |
| `realpath` | 10 | 2.2× (was 3.6×) | An `O_PATH` descriptor walk (D-13.5): `openat` and `close` per directory where the reference makes one `readlink` per component | Resolve by path while the result is shorter than `PATH_MAX` (`readlinkat` on the prefix; `EINVAL` means not a link), descriptors only beyond it |
| `getmntent` | 10 | 1.5× (was 2.3×) | Two vectors per line (the line, the decoded strings) and a copy into the thread's result block | Parse in the stream's buffer and decode straight into the result block |
| `fts` | 10 | 1.5× | Rust nodes mirrored into the caller's `FTSENT` blocks (written when built and again when visited, `stat` copied each time), a path and a name vector per node | Build each `FTSENT` once in place, with paths shared through the parent's prefix |
| `scandir` with `alphasort` | 10 | 1.28× | `strcoll` measures both names before comparing | The `strcmp`/`strcoll` remedy (a single-pass C-layer compare) |
| gettext lookup | 11 | 36–42× found, 10–11× missing (was 118×, 29×) | One `stat` of each catalog used per lookup (D-7.12: no stale answers), about 1.5 µs on the VM; the candidate paths built and the catalog records searched by path per lookup (the alias lookup is now kept, review before phase 14) | Cache the resolved catalog per (domain, category, locale) behind the validity checks; the `stat` per lookup replaced by a change notification (P8 below) |
| `mallinfo2`, `malloc_info` | 11 | 11.5×, 2.9× (was 35×, 4.5×) | Every owned slab's allocation bitmap is counted (256 words for the small classes); `malloc_info` then formats through a per-line sink with one `fputs` per line | Per-slab in-use counts readable by any thread (summed on read, M2), so the statistics cost one word per slab; the report formatted into one buffer |
| AIO lists | 11 | 1.6× (`lio_listio`, 16 reads on one descriptor) | Requests on one descriptor run one after another (§11.11 order), each a helper round trip | Linked io_uring submissions for a descriptor's queue (D-11.11) |

### Targets from the hardware assumption (rule P8, 2026-10-02)

Where P8 suggests a better approach than code written before it, recorded here rather than
retrofitted:

| Area | Now | With P8 |
|---|---|---|
| Time zones | A zone is selected again when `TZ`/`TZDIR` change, and `/etc/localtime` is checked with `stat` at each implicit `tzset` while `TZ` is unset | Load once and pin; reload only on an explicit reset (`tzset`) |
| POSIX `TZ` rules | Transitions computed per call | A transition table precomputed at load (for example 1900–2200), searched as a TZif zone is |
| Locales, zone files, charmaps | Read into process memory per process (charmaps inflated from gzip at first use) | Memory-mapped read-only, and shared between processes (one load per machine) where the data is the system's |
| Character-set conversion | Range tables searched per character | Precomputed direct tables for the common charsets |
| Hot per-thread and per-stream state | Laid out as the ABI and the code grew | Cache-line-aligned, NUMA-aware placement (the heap already shards by node, D-3.12) |
| Name-service configuration (phase 10, entry 8) | `nsswitch.conf`, `host.conf`, `resolv.conf` (and `gai.conf` with `reload yes`) checked with `stat` on every lookup, about 1–1.5 µs each on the test VM; the switch decoded from atomics into a 1.3 KB value per lookup | Decoded once and pinned as a shared read-only snapshot, replaced only on an explicit reset (`res_init`, a new `__nss_configure_lookup`) or a change notification instead of a `stat` per call |
| `files` databases (phase 10, entry 8) | `passwd`, `group`, `services`, `hosts` opened, read and parsed on every lookup | Memory-mapped and indexed once (by name and number), the index kept until the file's identity changes; shared between processes where the data is the system's (an nscd-like per-machine cache) |
| Process creation (phase 11, entry 9) | A spawn child copies the caller's page tables, a cost that grows with the caller's resident memory; P8 expects large processes (abundant memory, data pinned in memory), for which this becomes the dominant cost of `posix_spawn`, `system` and `popen` | Creation independent of the caller's size: a `CLONE_VM` child (D-18.3's target) on a spawn stack mapped once and reused, rather than mapped per call, with the error word beside it |
| Message catalogs (phase 13, entry 11) | Each gettext lookup checks the catalog it uses against its file with one `stat` (D-7.12: a replaced catalog is read again, never a stale answer) | Catalogs mapped once and pinned; a replaced file noticed through a change notification (inotify on the catalog directories, or a shared per-machine generation) instead of a `stat` per lookup, keeping D-7.12's guarantee |
| State `fork` resets (phase 11, entry 9) | Lock words, lists and caches spread over the data of five modules and the heap, each page written after `clone` by the child (and, once threads exist, by the parent releasing the locks) | Laid out together (the page-placement side of the cache-line rule), so that `fork` touches a few pages on each side |


## Name-service cache (D-16.10, 2026-10-03)

`bench/c/cbench_nscache.c` (Liella only: it switches the cache with `liella_nscache_enable`, as
`LIELLA_NSCACHE=0` does at start), on the test VM, the shared region warm, `cargo xtask bench`
running it with a private `/dev/shm`. ns per call, best of seven rounds:

| Call | Cached | Uncached | glibc (entry 8) |
|---|---|---|---|
| `getpwnam_r("root")` | 882 | 5,286 | 5,200 |
| `getaddrinfo("localhost", AF_INET)` | 4,802 | 11,252 | 8,800 |
| `getservbyname_r("http", "tcp")` | 2,025 | 8,541 | 9,300 |
| First lookup of a fresh process (`execve`d; see "Cold start" below) | 84,430–95,528 | 88,268–95,378 | — |

`cbench_netdb`'s rows now run with the cache: `getpwnam_r_root` 832, `getgrgid_r_root` 722,
`getservbyname_r_http` 1,918, `getaddrinfo_localhost` 4,796. Budgets: sanity bounds and
cached/uncached ratios (0.4×, 0.7×, 0.5×), all met.

Where the remaining time goes, for the optimization pass:

* **Hits** pay a context only where the lookup's API builds one before the cache (services,
  protocols, networks, hosts: the switch configuration decoded from atomics, `host.conf`, and for
  hosts the thread's resolver state with `HOSTALIASES` read, about 1 µs and 3 µs); `getpwnam` and
  `getgrnam` build it only on a miss. A hit also allocates four small vectors (key, scratch
  words, value, line) and copies the value byte by byte. Targets: contexts built lazily
  (the "contexts that borrow the configuration" row above), a fixed-capacity key and value buffer.
### Cold start (owner follow-up, 2026-10-03)

A fresh process (`execve`d by the benchmark, best of 40, VM; three rounds), ns:

| | First lookup before | First lookup after | Second lookup after |
|---|---|---|---|
| Region exists and is valid | 150,642–152,452 | 84,430–95,528 | 50,239–57,939 |
| Region absent (created) | 171,682–172,532 | 88,248–95,938 | 80,399–93,828 |
| Cache off (`LIELLA_NSCACHE=0`) | 90,901–97,781 | 88,268–95,378 | 9,249–10,850 |
| Steady state, `getpwnam_r` | 841–882 | 852–867 | |

Where the 55–77 µs went (strace of the attaching lookup, page-fault counts from `getrusage`):
on this VM a system call costs about 1.5–2 µs and a first-touch fault about as much; attaching
made about 20 calls the uncached lookup does not (four ID queries, opening the region twice with
`O_EXCL` first, `fstat`, `mmap`, four for the boot ID, the account stamp's `passwd` read with
`nsswitch.conf` checked again, five validation `stat`s) and about ten more faults. Changes:

* **The first cacheable lookup of a process goes to the sources** and leaves the table alone
  (they read the current files, so nothing is validated or stored); the second attaches. A
  process that makes one lookup pays nothing for the cache.
* The region is opened before it is created (one `openat` when it exists), the boot ID is read
  with one `read`, and the account stamp is trusted without a `passwd` lookup when the header
  records that it was verified against the current identities of `passwd` and `nsswitch.conf`
  (words 70–79; any edit of either changes them) and only `files` can answer passwd lookups;
  those two `stat`s serve the first validation too.

The attaching (second) lookup still costs about 30 µs (a hit; about 45 µs with the miss and the
store of a second user) more than an uncached one: 13 system calls (four ID queries, `openat`,
`fstat`, `mmap`, three for the boot ID, the stamp's two `stat`s, three more validation `stat`s)
and three or four faults on the region. Kept by decision: the ID queries (credentials are never
cached, P3; `AT_*UID` from start-up would miss changes made by raw system calls), the boot ID
and the first validation (D-16.10). The cache pays for itself after about six hits (4.3 µs each
against the sources).

## Optimization stage (working numbers)

Plan rule P10 (owner, 2026-10-04): the optimization pass before the first release. These are
working numbers (P10 (9)); release-grade numbers follow later on a quiet machine.

**Machine.** Haswell-EP: Intel Xeon E5-2697 v3 (Haswell-EP, AVX2, ERMS), 16 vCPUs under KVM, alone on
its host, one NUMA node; Ubuntu 24.04.5, kernel 6.8.0-146, host C library Ubuntu glibc
2.39-0ubuntu8.9. Absolute numbers are not comparable with entries 1–11 (other machines); only the
same-run ratios are.

**Method** (P10 (9)). `cargo xtask bench --compare [--runs N] <cbench>...` builds each C
benchmark three times from the same source with the same flags (`-std=c17 -O2 -fno-builtin`):
against the products (static, as `cargo xtask bench`), against the host glibc dynamically linked
(the reference, as in entries 5–11) and statically linked (the fair comparison for start-up,
since the products are static). It runs the three alternately: one warm-up round, discarded, then
five rounds, each under `~/bin/timing` (nothing else ran on the machine). Every value in a round
is the program's own best of seven passes; the tables give the median of the five rounds with
min and max, and the ratio of the medians, Liella ÷ glibc (dynamic): the headline. Each call is
checked once before it is timed.

**Inputs.** `bench/c/cbench_opt.c` (new) holds the P10 targets with inputs typical of coreutils,
bash and sed, until the TEST thread's validation profiles arrive: about 100 real-world file names
(build trees, home and system directories, a few UTF-8 names) and paths made from them; ls-, du-
and date-style formats; 256 lines of syslog, configuration and C code, scanned with grep- and
sed-style expressions (a literal, an anchored class, an IPv4 address, an alternation, a
case-insensitive absent alternation, `.*` chains, and two `s///` patterns with subexpressions);
shell words (plain, quoted, tilde and variables, field splitting, parameter forms, arithmetic);
4 KiB of ASCII, Latin-1 and mixed-script prose for iconv. The microbenchmark rows of
`cbench_stdio`, `cbench_locale`, `cbench_time`, `cbench_patterns` and `cbench_services` were run
the same way.

**Cold start** (P10 (8)). `cold_exec_empty` and `cold_exec_once` are the median of 31 fresh
processes (this program again through `execve`), from just before `execve` (the time is sent
through the pipe by the child) to the parent's return from `waitpid`: a program that returns at
once, and one that calls every target once. `first_*`/`second_*` are the median cost of the first
and the second call of each target inside such a process, in the order listed (so
`first_strcoll` follows a `setlocale` of `en_US.UTF-8`; `TZ=America/New_York`). A `first_*` row
measures the library's first-call cost (its text and data page faults included) plus this binary's
own text layout: the process reads its own function-pointer table and inputs before the timed
section (HOT-ORDER), since a fault on an untouched page of the bench's `.data` once showed up as
`first_strcmp` 17-27x.

**Timebox** (P10 (13), set by the coordinator). The stage ends when every target more than about
2× glibc has had at least one algorithmic attempt, or at **2026-10-08 18:00 UTC**, whichever comes
first. Fuzzing follows as a separate assignment. The stopping rule (P10 (10), owner clarification
2026-10-04): about 10% of glibc is a goal, not a hard stop; a clear opportunity to do better
(including beating glibc) is taken, diminishing returns are not pursued. `bench/budgets.tsv`
stays the regression gate during the pass and is tightened to the achieved ratios at its end
(owner ruling 2026-10-04); no budget is loosened.

### Baseline (2026-10-04, before any optimization)

#### P10 targets, realistic and micro rows (`cbench_opt`), ns per operation

| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |
|---|---:|---:|---:|---:|
| `snprintf_d` | 45.62 (45.60-45.94) | 359.06 (358.70-375.20) | 7.87 | 47.59 (47.53-47.67) |
| `snprintf_ld_large` | 52.68 (52.64-55.26) | 406.53 (406.00-424.27) | 7.72 | 53.04 (52.94-55.16) |
| `snprintf_x08` | 49.12 (48.99-49.34) | 361.56 (361.09-367.49) | 7.36 | 46.44 (46.43-49.41) |
| `snprintf_s_path` | 35.22 (35.07-35.44) | 233.16 (232.49-237.32) | 6.62 | 35.35 (35.18-37.56) |
| `snprintf_s_d_colon` | 84.15 (83.77-84.18) | 565.52 (562.96-595.18) | 6.72 | 81.27 (81.24-81.30) |
| `snprintf_ls_line` | 263.20 (262.87-266.35) | 1318.39 (1310.39-1337.01) | 5.01 | 251.42 (247.52-267.92) |
| `snprintf_mixed` | 437.14 (436.64-456.11) | 2408.43 (2365.33-2426.13) | 5.51 | 438.19 (438.00-457.14) |
| `snprintf_f2` | 370.77 (370.59-383.68) | 2230.95 (2188.51-2367.77) | 6.02 | 371.74 (371.72-384.92) |
| `snprintf_g` | 355.73 (355.48-365.21) | 3510.33 (3475.17-3690.83) | 9.87 | 359.24 (359.07-360.93) |
| `snprintf_e` | 161.80 (161.55-161.92) | 3005.67 (2985.46-3261.04) | 18.58 | 161.70 (161.53-161.73) |
| `snprintf_f_large` | 16395.59 (16371.85-16412.78) | 6409.26 (6377.89-6470.37) | 0.39 | 16524.26 (16486.85-16660.38) |
| `fprintf_ls_line` | 273.39 (272.83-273.74) | 1494.55 (1489.17-1504.68) | 5.47 | 261.43 (260.59-261.51) |
| `fprintf_du_line` | 86.51 (86.31-86.58) | 543.46 (536.79-558.34) | 6.28 | 84.02 (83.94-84.23) |
| `strcmp_names` | 5.48 (5.48-5.67) | 11.12 (11.07-11.20) | 2.03 | 5.06 (5.06-5.18) |
| `strcmp_equal_16` | 2.23 (2.23-3.93) | 22.50 (22.41-22.67) | 10.09 | 2.79 (2.79-2.79) |
| `strcmp_late_256` | 7.34 (7.34-7.35) | 266.97 (264.24-267.66) | 36.37 | 9.03 (9.02-9.92) |
| `strcoll_c_names` | 7.58 (7.58-7.96) | 17.00 (16.91-17.97) | 2.24 | 6.99 (6.98-7.80) |
| `strcoll_c_utf8_names` | 7.58 (7.58-8.50) | 17.36 (17.31-18.35) | 2.29 | 6.99 (6.98-7.79) |
| `strcoll_en_us_names` | 30.68 (30.60-33.87) | 135.37 (135.20-140.27) | 4.41 | 30.73 (30.53-32.93) |
| `strcoll_en_us_equal_16` | 275.78 (274.98-283.67) | 2880.04 (2875.53-3056.78) | 10.44 | 275.52 (275.05-286.99) |
| `strcoll_en_us_utf8_16` | 3814.78 (3807.86-3827.93) | 6287.82 (6269.67-6332.33) | 1.65 | 3807.49 (3793.98-3846.44) |
| `iconv_utf8_utf16_ascii_4k` | 7166.44 (7164.64-7167.57) | 23343.66 (22901.89-23563.82) | 3.26 | 7164.44 (7161.52-7171.61) |
| `iconv_utf8_utf16_mixed_4k` | 7729.68 (7488.40-7802.45) | 42208.81 (42109.26-42279.70) | 5.46 | 7959.19 (7908.81-8049.94) |
| `iconv_utf16_utf8_mixed` | 8165.37 (8152.11-8554.62) | 44634.35 (44543.26-44795.41) | 5.47 | 8179.11 (8167.94-8514.57) |
| `iconv_utf8_utf32_mixed_4k` | 6758.17 (6746.86-7032.41) | 43384.97 (43334.66-43481.89) | 6.42 | 6977.43 (6964.93-7305.33) |
| `iconv_utf8_utf16_name` | 85.58 (84.93-88.26) | 127.41 (127.11-127.67) | 1.49 | 81.88 (81.70-83.63) |
| `iconv_open_conv_close_name` | 539.31 (538.18-556.20) | 210.29 (210.26-210.77) | 0.39 | 518.48 (518.04-564.78) |
| `iconv_utf8_8859_15_latin_4k` | 10348.65 (10342.03-10355.82) | 63188.35 (63098.51-63310.36) | 6.11 | 10789.08 (10784.80-10811.15) |
| `iconv_8859_15_utf8_latin` | 8853.34 (8842.30-8885.83) | 67654.17 (67457.22-67756.48) | 7.64 | 8851.47 (8849.19-8855.76) |
| `strftime_ls_iso` | 75.74 (75.62-75.85) | 313.74 (312.97-326.50) | 4.14 | 73.47 (73.45-73.53) |
| `strftime_ls_recent_en_us` | 60.36 (60.31-60.40) | 391.11 (390.90-393.05) | 6.48 | 59.21 (59.18-59.21) |
| `strftime_date_en_us` | 116.31 (116.26-116.33) | 719.10 (717.43-732.07) | 6.18 | 112.73 (112.66-112.78) |
| `strftime_iso8601_tz` | 107.96 (107.87-107.99) | 468.13 (467.55-468.45) | 4.34 | 105.32 (105.29-105.45) |
| `strftime_c_en_us` | 295.13 (294.86-307.74) | 1052.14 (1049.75-1054.92) | 3.57 | 183.63 (183.48-190.26) |
| `strftime_log` | 107.74 (107.69-110.37) | 633.58 (632.35-635.02) | 5.88 | 104.52 (104.47-107.17) |
| `strftime_ls_recent_c` | 60.30 (60.27-61.66) | 467.38 (466.26-468.06) | 7.75 | 59.21 (59.20-61.49) |
| `mktime_new_york` | 180.92 (178.55-206.30) | 412.19 (411.71-415.52) | 2.28 | 179.33 (178.69-188.56) |
| `localtime_r_new_york` | 502.08 (501.68-502.56) | 216.67 (216.47-219.44) | 0.43 | 500.18 (499.06-506.57) |
| `mktime_utc` | 145.30 (142.65-150.81) | 193.85 (193.46-196.30) | 1.33 | 144.73 (144.03-150.02) |
| `mktime_posix_rule` | 158.88 (156.85-167.22) | 753.53 (750.74-759.98) | 4.74 | 157.68 (157.10-162.63) |
| `localtime_r_posix_rule` | 58.52 (58.51-59.10) | 245.75 (245.17-246.64) | 4.20 | 60.23 (58.63-61.61) |
| `re_grep_literal` | 82.53 (81.79-82.98) | 2236.27 (2227.95-2314.21) | 27.10 | 82.65 (82.14-84.53) |
| `re_grep_comment` | 251.70 (251.39-261.60) | 3359.04 (3345.49-3466.70) | 13.35 | 232.27 (231.62-240.58) |
| `re_grep_ipv4` | 1555.23 (1552.31-1716.55) | 6345.31 (6328.88-6384.81) | 4.08 | 1488.36 (1483.59-1575.44) |
| `re_grep_alt` | 779.77 (779.19-781.08) | 5920.14 (5886.15-6127.77) | 7.59 | 721.60 (719.33-752.11) |
| `re_grep_icase_absent` | 634.10 (633.12-638.16) | 4927.01 (4890.39-5007.99) | 7.77 | 598.95 (597.22-616.95) |
| `re_grep_dotstar` | 259.39 (258.70-270.84) | 5918.01 (5896.18-5956.13) | 22.82 | 255.64 (255.28-257.63) |
| `re_sed_kv` | 1137.02 (1132.54-1183.01) | 12538.24 (12525.01-12990.93) | 11.03 | 1091.30 (1089.96-1097.65) |
| `re_sed_swap` | 3550.09 (3533.49-3627.92) | 15701.28 (15634.42-15780.53) | 4.42 | 3473.49 (3465.50-3624.03) |
| `regcomp_regfree_sed_kv` | 18293.47 (18190.91-19217.44) | 6872.58 (6838.57-7344.03) | 0.38 | 16535.06 (16459.48-16569.37) |
| `regcomp_regfree_ipv4` | 16918.70 (16844.92-17278.02) | 15521.39 (15465.35-15577.50) | 0.92 | 15350.58 (15331.35-15844.29) |
| `fnmatch_names_star_c` | 127.69 (127.17-131.01) | 564.72 (558.06-595.15) | 4.42 | 126.68 (126.52-127.08) |
| `fnmatch_names_ch` | 145.24 (145.02-148.08) | 687.62 (661.50-694.13) | 4.73 | 142.16 (141.98-142.62) |
| `fnmatch_names_lib_so` | 91.59 (91.17-92.72) | 571.34 (562.59-610.01) | 6.24 | 90.49 (89.86-92.31) |
| `fnmatch_names_period` | 77.48 (76.48-79.92) | 449.51 (438.35-480.37) | 5.80 | 75.52 (75.01-76.24) |
| `fnmatch_names_question` | 107.89 (107.54-114.69) | 715.41 (704.29-731.82) | 6.63 | 113.17 (113.02-114.21) |
| `fnmatch_names_class` | 151.52 (151.21-153.05) | 760.50 (758.13-763.87) | 5.02 | 147.17 (146.84-148.07) |
| `fnmatch_names_casefold` | 210.72 (209.82-217.25) | 863.16 (851.22-879.39) | 4.10 | 207.88 (207.07-215.81) |
| `fnmatch_paths_pathname` | 144.62 (143.84-148.32) | 821.30 (820.47-857.00) | 5.68 | 142.13 (141.83-142.63) |
| `fnmatch_paths_leading_dir` | 88.04 (87.50-91.10) | 548.30 (538.34-562.09) | 6.23 | 86.43 (86.02-86.47) |
| `wordexp_tilde_var` | 320.19 (318.18-326.96) | 2973.71 (2953.96-3158.49) | 9.29 | 293.03 (292.79-330.29) |
| `wordexp_arith` | 354.67 (354.55-360.98) | 2815.31 (2790.93-2938.42) | 7.94 | 336.26 (336.17-336.57) |
| `wordexp_plain_8` | 575.99 (566.22-601.03) | 3252.04 (3234.53-3442.99) | 5.65 | 524.16 (518.98-547.05) |
| `wordexp_quoted` | 255.29 (255.18-274.35) | 1907.38 (1906.18-2002.15) | 7.47 | 244.78 (243.52-244.87) |
| `wordexp_split_var` | 383.72 (383.48-430.39) | 2762.66 (2760.82-2909.64) | 7.20 | 365.06 (364.39-365.93) |
| `wordexp_param_forms` | 551.12 (550.73-605.05) | 3751.82 (3733.12-3880.83) | 6.81 | 521.41 (521.26-521.97) |
| `gettext_found` | 69.44 (68.22-79.76) | 2187.24 (2177.68-2193.36) | 31.50 | 68.08 (68.05-79.16) |
| `gettext_missing` | 324.16 (321.31-346.74) | 2420.98 (2417.73-2424.68) | 7.47 | 302.33 (298.93-311.39) |
| `ngettext_found` | 91.62 (91.51-95.05) | 2236.93 (2225.21-2307.59) | 24.42 | 85.18 (85.13-95.30) |
| `gettext_no_catalog` | 235.26 (235.13-239.06) | 1081.91 (1080.90-1084.19) | 4.60 | 231.86 (229.02-235.96) |
| `gettext_c_locale` | 92.68 (91.89-98.12) | 10.89 (10.89-11.24) | 0.12 | 93.35 (93.10-103.78) |
| `cold_exec_empty` | 627865.00 (615051.00-634910.00) | 309599.00 (301377.00-317813.00) | 0.49 | 444085.00 (425831.00-446913.00) |
| `cold_exec_once` | 1126288.00 (1086268.00-1132090.00) | 1184201.00 (1173661.00-1232983.00) | 1.05 | 1239441.00 (1195958.00-1265917.00) |
| `first_snprintf` | 16577.00 (14435.00-16747.00) | 107250.00 (105850.00-121920.00) | 6.47 | 4679.00 (4243.00-4721.00) |
| `second_snprintf` | 2006.00 (1822.00-2097.00) | 6597.00 (6397.00-7078.00) | 3.29 | 1889.00 (1791.00-1937.00) |
| `first_strcmp` | 101.00 (95.00-102.00) | 153.00 (149.00-164.00) | 1.51 | 67.00 (62.00-69.00) |
| `second_strcmp` | 63.00 (60.00-68.00) | 97.00 (92.00-103.00) | 1.54 | 47.00 (43.00-51.00) |
| `first_setlocale_en_US` | 55266.00 (49269.00-55944.00) | 161797.00 (161008.00-166079.00) | 2.93 | 52378.00 (47290.00-53832.00) |
| `second_setlocale_en_US` | 178.00 (166.00-182.00) | 77757.00 (77447.00-78108.00) | 436.84 | 164.00 (152.00-172.00) |
| `first_strcoll` | 386.00 (358.00-389.00) | 5912.00 (5563.00-5950.00) | 15.32 | 376.00 (344.00-390.00) |
| `second_strcoll` | 359.00 (347.00-367.00) | 741.00 (716.00-797.00) | 2.06 | 312.00 (299.00-333.00) |
| `first_iconv_open_conv_close` | 81767.00 (72334.00-83367.00) | 6489.00 (6222.00-6613.00) | 0.08 | 394253.00 (383742.00-404964.00) |
| `second_iconv_open_conv_close` | 6105.00 (5443.00-6269.00) | 1589.00 (1454.00-1646.00) | 0.26 | 6072.00 (5559.00-6225.00) |
| `first_localtime_r_new_york` | 27449.00 (25117.00-28049.00) | 62079.00 (57049.00-67114.00) | 2.26 | 27779.00 (25547.00-28201.00) |
| `second_localtime_r_new_york` | 538.00 (487.00-580.00) | 615.00 (578.00-671.00) | 1.14 | 601.00 (580.00-621.00) |
| `first_strftime_c` | 1811.00 (1679.00-1826.00) | 4036.00 (3917.00-4078.00) | 2.23 | 1303.00 (1202.00-1340.00) |
| `second_strftime_c` | 1341.00 (1231.00-1371.00) | 3157.00 (2936.00-3191.00) | 2.35 | 1099.00 (1033.00-1117.00) |
| `first_mktime_new_york` | 776.00 (718.00-792.00) | 1527.00 (1420.00-1571.00) | 1.97 | 735.00 (697.00-765.00) |
| `second_mktime_new_york` | 567.00 (538.00-578.00) | 1309.00 (1277.00-1355.00) | 2.31 | 533.00 (519.00-553.00) |
| `first_regcomp_regexec_regfree` | 81402.00 (76107.00-87682.00) | 78377.00 (76801.00-80350.00) | 0.96 | 75268.00 (71452.00-81784.00) |
| `second_regcomp_regexec_regfree` | 55869.00 (50197.00-55961.00) | 47638.00 (47406.00-48368.00) | 0.85 | 52663.00 (46127.00-52718.00) |
| `first_fnmatch` | 5390.00 (4652.00-5586.00) | 4827.00 (4699.00-4969.00) | 0.90 | 2782.00 (2501.00-2852.00) |
| `second_fnmatch` | 805.00 (738.00-823.00) | 1671.00 (1625.00-1723.00) | 2.08 | 752.00 (677.00-754.00) |
| `first_wordexp` | 4755.00 (4187.00-4799.00) | 22113.00 (21619.00-22388.00) | 4.65 | 2091.00 (1937.00-2111.00) |
| `second_wordexp` | 1684.00 (1527.00-1692.00) | 6354.00 (5841.00-6412.00) | 3.77 | 1548.00 (1443.00-1564.00) |
| `first_bindtextdomain_gettext` | 58559.00 (52151.00-60501.00) | 64823.00 (62524.00-66793.00) | 1.11 | 59781.00 (53410.00-60376.00) |
| `second_bindtextdomain_gettext` | 607.00 (554.00-650.00) | 11304.00 (11050.00-11622.00) | 18.62 | 609.00 (580.00-617.00) |
| `first_gettext_nocatalog` | 34922.00 (30589.00-35141.00) | 12914.00 (11603.00-13240.00) | 0.37 | 34512.00 (30389.00-34614.00) |
| `second_gettext_nocatalog` | 904.00 (819.00-912.00) | 10862.00 (10772.00-10978.00) | 12.02 | 748.00 (709.00-779.00) |

#### Streams and formatting (`cbench_stdio`), ns per operation

| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |
|---|---:|---:|---:|---:|
| `snprintf_d` | 45.70 (45.61-47.00) | 363.58 (361.30-379.23) | 7.96 | 46.35 (46.29-46.40) |
| `snprintf_mixed` | 453.17 (452.85-472.06) | 2458.70 (2434.90-2515.52) | 5.43 | 460.50 (460.16-461.49) |
| `fprintf_d` | 57.57 (57.41-59.96) | 399.84 (395.37-413.29) | 6.95 | 56.38 (56.34-56.70) |
| `fputs_32` | 13.70 (13.66-14.37) | 41.28 (41.24-41.42) | 3.01 | 13.43 (13.41-13.48) |
| `putc` | 2.62 (2.62-2.64) | 12.09 (12.08-12.10) | 4.61 | 2.62 (2.62-2.63) |
| `fwrite_4k` | 305.37 (303.64-308.11) | 371.38 (369.73-373.69) | 1.22 | 302.99 (301.78-306.38) |

#### Locale, conversion (`cbench_locale`), ns per operation

| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |
|---|---:|---:|---:|---:|
| `isalpha_c` | 1.96 (1.96-1.96) | 4.48 (4.47-4.57) | 2.29 | 1.68 (1.68-1.68) |
| `toupper_c` | 1.96 (1.96-1.96) | 3.91 (3.91-4.05) | 1.99 | 1.96 (1.96-1.96) |
| `strtod_c` | 101.30 (101.20-101.31) | 55.53 (55.50-56.95) | 0.55 | 99.78 (99.65-100.09) |
| `towupper_utf8` | 5.40 (5.40-5.42) | 5.03 (5.03-5.04) | 0.93 | 5.14 (5.13-5.32) |
| `iswalpha_utf8` | 6.30 (6.30-6.30) | 6.15 (6.15-6.40) | 0.98 | 5.77 (5.76-6.03) |
| `wcwidth_utf8` | 5.13 (5.12-5.24) | 4.75 (4.75-4.92) | 0.93 | 4.79 (4.79-4.99) |
| `mbrtowc_utf8_3` | 18.68 (18.61-20.08) | 35.41 (35.36-35.44) | 1.90 | 16.53 (16.48-18.53) |
| `strcoll_utf8_16` | 4.77 (4.74-5.31) | 17.45 (17.42-17.47) | 3.66 | 4.47 (4.47-4.48) |
| `strtod_utf8` | 102.00 (101.90-102.76) | 55.63 (55.60-55.68) | 0.55 | 98.94 (98.83-98.95) |
| `newlocale_free` | 4900.52 (4877.11-4947.61) | 2722.23 (2641.29-2790.30) | 0.56 | 4838.77 (4831.21-4857.93) |
| `iconv_utf8_utf16_4k` | 6950.43 (6941.92-6998.70) | 35796.39 (35747.55-35850.64) | 5.15 | 7154.46 (7151.18-7317.26) |
| `iconv_utf8_8859_15_4k` | 12891.39 (12853.99-13303.24) | 54610.77 (54518.44-55672.86) | 4.24 | 13608.88 (13461.54-13662.43) |
| `iconv_utf8_ascii_translit_4k` | 43684.94 (43489.32-44633.80) | 79247.61 (79220.99-79588.13) | 1.81 | 42889.40 (42662.58-43253.47) |

#### Time (`cbench_time`), ns per operation

| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |
|---|---:|---:|---:|---:|
| `gmtime_r` | 38.23 (38.16-38.84) | 39.20 (39.18-39.29) | 1.03 | 38.04 (38.01-38.38) |
| `timegm` | 63.67 (63.59-64.34) | 71.29 (71.23-73.66) | 1.12 | 63.55 (63.22-66.02) |
| `localtime_r_utc` | 59.30 (58.92-62.05) | 74.56 (74.35-80.61) | 1.26 | 59.16 (59.09-59.17) |
| `localtime_r_new_york` | 606.06 (605.55-609.82) | 245.64 (244.95-250.60) | 0.41 | 588.88 (587.68-591.92) |
| `localtime_r_new_york_2100` | 636.06 (635.11-639.24) | 255.46 (255.01-260.55) | 0.40 | 618.10 (617.22-618.41) |
| `mktime_new_york` | 186.34 (185.05-190.13) | 420.00 (417.23-424.34) | 2.25 | 178.50 (177.70-186.68) |
| `localtime_r_posix_rule` | 59.09 (58.73-61.26) | 247.91 (247.16-254.11) | 4.20 | 58.90 (58.88-58.94) |
| `strftime_iso` | 103.54 (103.48-104.07) | 478.36 (477.09-479.37) | 4.62 | 103.50 (103.42-105.93) |
| `strftime_c` | 187.41 (187.35-187.71) | 807.99 (803.57-809.59) | 4.31 | 187.51 (187.24-188.08) |

#### Patterns (`cbench_patterns`), ns per operation

| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |
|---|---:|---:|---:|---:|
| `regcomp_regexec_regfree_email` | 44867.39 (44468.59-46817.40) | 286178.24 (283437.22-298363.61) | 6.38 | 44381.43 (44078.55-44806.12) |
| `regexec_literal_1k` | 2227.46 (2213.20-2276.73) | 19505.32 (19382.54-19943.95) | 8.76 | 2233.31 (2217.93-2316.84) |
| `regexec_submatch_1k` | 34929.52 (34894.31-35296.24) | 278075.27 (275551.72-291960.54) | 7.96 | 35018.81 (34711.24-37453.73) |
| `regexec_class_nomatch_1k` | 630.32 (629.38-630.63) | 2114.99 (2108.47-2117.93) | 3.36 | 630.24 (630.10-632.24) |
| `regexec_backref` | 2189.39 (2186.93-2215.01) | 6396.70 (6282.73-6612.36) | 2.92 | 2112.94 (2110.94-2171.39) |
| `regexec_pathological_4k` | 50578395.15 (50275590.15-51016636.10) | 2159783.00 (2130148.40-2252194.50) | 0.04 | 50526160.10 (50297599.55-51156010.95) |
| `fnmatch_star` | 32.13 (32.00-37.51) | 402.66 (398.95-422.28) | 12.53 | 32.19 (31.85-33.77) |
| `fnmatch_bracket` | 73.20 (73.05-73.53) | 778.56 (772.85-831.15) | 10.64 | 72.98 (72.92-73.27) |
| `fnmatch_pathname` | 52.54 (51.95-56.65) | 722.25 (716.41-732.36) | 13.75 | 50.89 (50.73-51.27) |
| `fnmatch_pathname_nomatch` | 26.02 (25.98-28.59) | 506.15 (501.86-521.21) | 19.45 | 26.94 (26.86-27.02) |
| `glob_1000_sorted` | 465347.76 (463986.70-482113.64) | 695628.52 (692943.30-752947.08) | 1.49 | 457902.90 (455045.94-458735.30) |
| `glob_1000_nosort` | 322333.46 (321037.94-331121.16) | 581688.68 (574303.28-610119.22) | 1.80 | 316428.48 (313921.92-316787.76) |
| `wordexp_tilde_var` | 318.26 (315.00-369.61) | 2441.66 (2383.62-2464.50) | 7.67 | 299.39 (299.14-300.34) |
| `wordexp_arith` | 357.37 (357.28-371.98) | 2696.32 (2676.79-2779.09) | 7.54 | 337.88 (337.68-338.01) |

#### Catalogs and services (`cbench_services`), ns per operation

| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |
|---|---:|---:|---:|---:|
| `gettext_found` | 68.75 (68.69-69.27) | 2175.12 (2164.55-2191.06) | 31.64 | 67.99 (67.93-69.97) |
| `gettext_missing` | 265.41 (264.19-267.91) | 2381.05 (2368.97-2409.45) | 8.97 | 262.54 (257.17-267.31) |
| `ngettext_found` | 82.71 (82.41-83.84) | 2224.19 (2216.96-2254.29) | 26.89 | 84.66 (84.61-91.61) |
| `catgets_found` | 9.69 (9.60-9.74) | 11.46 (11.46-11.95) | 1.18 | 10.05 (10.03-10.51) |
| `mq_send_receive_4` | 1181.84 (1180.36-1185.31) | 1207.70 (1196.32-1212.68) | 1.02 | 1178.70 (1170.24-1179.65) |
| `shm_open_map_unlink` | 11891.08 (11696.88-11992.09) | 13816.68 (13589.25-13975.97) | 1.16 | 11574.26 (11438.20-11646.29) |
| `vsyslog_masked` | 17.89 (17.89-18.31) | 8.11 (8.10-8.41) | 0.45 | 17.89 (17.88-18.48) |
| `openlog_closelog` | 32.14 (32.09-33.30) | 30.46 (30.46-30.47) | 0.95 | 32.16 (32.13-34.17) |
| `malloc_usable_size` | 2.23 (2.23-2.23) | 8.38 (8.38-8.38) | 3.76 | 1.67 (1.67-1.73) |
| `mallinfo2` | 102.48 (102.44-105.46) | 1528.48 (1523.62-1558.95) | 14.91 | 103.26 (103.19-106.60) |
| `malloc_info_devnull` | 1299.63 (1295.35-1450.61) | 4373.00 (4361.94-4397.20) | 3.36 | 1286.17 (1248.25-1403.26) |

Notes on the baseline:

* **Already at or better than glibc:** `snprintf` of a huge `%f` (0.39), `localtime_r` with a
  TZif zone (0.41–0.43), `iconv_open` + convert + close of one name (0.39), `regcomp` (0.38–0.92),
  the pathological regex (0.04), `gettext` in the C locale (0.12), `strtod` (0.55), `newlocale`
  (0.56), the masked `vsyslog` (0.45); cold: process start (0.49 against the dynamic reference,
  0.70 against the static one), the first `iconv_open` (0.08) and the first `gettext` without a
  catalog (0.37). These are left alone (P10 (1)).
* **Cold-start outliers:** the first `snprintf` of a process costs 107 µs (glibc 16.6 µs
  dynamic, 4.7 µs static); `setlocale(LC_ALL, "en_US.UTF-8")` a second time, with the locale
  already selected, costs 78 µs (glibc 0.18 µs): every category is looked up and rebuilt again;
  the first `strcoll` in `en_US.UTF-8` 5.9 µs (glibc 0.39); the first `wordexp` 22 µs (glibc
  4.8). Inside a fresh process the second `gettext` costs 11 µs (glibc 0.6), against 2.2 µs in
  the steady state.
* **`cargo xtask bench` on Haswell-EP:** two budgets fail before any change, `bench_mem/memcpy_4k`
  (333 ns against 1.1 × 48 ns) and `memset_4k` (75 ns). Probed (a C program against `libc.a`):
  on this CPU `rep movsb` is 7–9× slower than glibc's copy when the destination starts 1–300
  bytes after the source modulo 4 KiB at an offset that is not a multiple of 32 (4 KiB at +16:
  344 ns, glibc 49 ns, the `liella-mem` loops 82 ns); the benchmark's two `Vec`s sit exactly so
  (+16). With other placements the products' `memcpy` is 1.0–1.4× glibc (4 KiB aligned: 51 against
  37 ns). `memset_4k` is `rep stosb` at a destination 16 bytes past a 64-byte boundary: 86 ns,
  glibc 82 ns at the same placement, so the budget (measured against an aligned `memcpy`) cannot
  be met by either library there. Recorded as a target below; the other 116 of the 118 budgets are met.

### Order of work

Largest ratio first, each with the algorithmic idea to try first:

| # | Target | Worst ratio (realistic rows) | Idea |
|---|---|---|---|
| 1 | `strcmp` (and `strcoll` in C and C.UTF-8) | 36× (256 bytes, late difference); 10× equal; 2.0× file names | The compare after measuring is a byte loop with two bounds checks per byte (`bytes::compare_text`): compare the measured slices with the C layer's `memcmp` (byte order of NUL-terminated strings is slice order); then avoid measuring both strings whole (bound the second by the first's length plus one) |
| 2 | gettext | 32× found, 27× `ngettext`, 9× missing, 4.6× no catalog | Resolve each (domain, category, locale) once to its catalog record and keep it (behind the existing validity checks), so a lookup is a hash probe; the `stat` per lookup (D-7.12) stays until the owner rules (below) |
| 3 | Regex overall-match pass | 27× literal, 23× `.*` chain, 13× anchored class, 11× sed `s///` | A lazy DFA for pass 1 (D-14.4), states built on demand and cached per compiled pattern with a bounded cache (falls back to the NFA when full); a required-literal prefilter (memchr/memmem) before it |
| 4 | printf family | 18.6× `%e`, 9.9× `%g`, 6–8× integers and strings, 5× an ls line | Parse the format into fixed caller-side storage instead of heap vectors; render integers straight into the output; an exact fast path for `%e`/`%f`/`%g` with a precision (fixed-precision digit generation instead of the big-number path and the 12 KB buffer cleared per call) |
| 5 | `fnmatch` | 19× micro, 4.1–6.6× over file names | A direct matcher on the bytes for patterns without extended groups (single-star backtracking), the position-set matcher kept for `FNM_EXTMATCH` |
| 6 | `strcoll` in `en_US.UTF-8` | 10.4× equal strings, 4.4× file names | Skip the common prefix with the C-layer compare before weights are looked up, then compare weights level by level lazily, stopping at the first difference |
| 7 | `wordexp` | 9.3× tilde and variable, 6.8–7.9× the rest | Fields as ranges into the input and one output buffer per call; arithmetic operators tokenized to integers |
| 8 | `strftime` | 7.8× `%b %e %H:%M` (C), 6.5× the same in en_US, 3.6–6.2× | A direct path for numeric and name directives writing into the caller's buffer; the locale's names found once per call, not per directive |
| 9 | iconv | 7.6× ISO-8859-15 to UTF-8, 5.5–6.4× Unicode forms, 3.3× ASCII | Specialised loops for UTF-8 ↔ UTF-16/32 (ASCII runs copied a word at a time) and direct 256-entry tables for 8-bit charmaps (precomputed at load, P8) |
| 10 | `mktime`, `localtime_r` with POSIX rules | 4.7×, 4.2×; `mktime` with a TZif zone 2.3× | The rule's transitions precomputed at load (P8) and searched like a TZif zone |
| 11 | Cold start and first calls | `setlocale` again 437×; first `strcoll` 15×; first `snprintf` 6.5× (23× static); first `wordexp` 4.7× | Profile each: an unchanged `setlocale` returns the current locale without lookups; the first-call initialisations made lazy or cheaper |
| 12 | Haswell `rep movsb` placement (`bench_mem` budgets on Haswell-EP) | 7–9× at 2–16 KiB for a short misaligned forward distance | Choose the loops for that placement up to a measured size (an attempt that switched all sizes cost 60% at 64 KiB, where `rep movsb` is fine; reverted), and faster loops for it |

Other gaps seen on the way (not P10 targets; existing optimization-target rows): `putc` 4.6×,
`fputs` 3.0×, `mallinfo2` 14.9×, `malloc_usable_size` 3.8×, `malloc_info` 3.4×, `glob` 1.5–1.8×,
`isalpha` 2.3×, `mbrtowc` 1.9×.

### Before and after

| Target | Row | Before (median, min–max) | After (median, min–max) | glibc | Ratio before → after | Commit |
|---|---|---:|---:|---:|---:|---|
| memchr (correctness, T7) | `bench_mem` `memchr_4k` (budget 53 ns) | 39.67 | 47.07–48.65 (six runs) | — | (cost of reading aligned blocks and stopping at the match) | b70552d |
| strcmp | `strcmp_late_256` | 266.97 (264.24–267.66) | 19.54 (19.53–19.98) | 7.34 | 36.4 → 2.66 | cf5aa1a |
| strcmp | `strcmp_equal_16` | 22.50 (22.41–22.67) | 8.69 (8.68–8.71) | 2.23 | 10.1 → 3.90 | cf5aa1a |
| strcmp | `strcmp_names` | 11.12 (11.07–11.20) | 12.37 (12.34–12.72) | 5.40 | 2.03 → 2.29 | cf5aa1a |
| gettext (once-per-second `stat`, owner ruling) | `gettext_found` | 2187.24 (2177.68–2193.36) | 930.61 (929.78–932.41) | 68.11 | 31.5 → 13.7 | 50b327f |
| gettext (resolution cache) | `gettext_found` | 930.61 (929.78–932.41) | 578.22 (568.58–632.48) | 68.11 | 13.7 → 8.5 | 047203b |
| gettext | `gettext_missing` | 2420.98 → 1129.97 (50b327f) | 509.12 (499.62–513.30) | 322.73 | 7.5 → 1.6 | 047203b |
| gettext | `ngettext_found` | 2236.93 → 965.01 (50b327f) | 604.63 (595.02–611.02) | 91.66 | 24.4 → 6.6 | 047203b |
| gettext | `gettext_no_catalog` | 1081.91 → 1040.26 (50b327f) | 468.76 (454.15–473.17) | 236.78 | 4.6 → 2.0 | 047203b |
| zones (once-per-second `stat`, owner ruling) | `mktime_new_york` | 412.19 (411.71–415.52) | 426.83 (426.53–429.82) | 181.88 | 2.28 → 2.35 (a coarse clock read per implicit tzset) | 50b327f |
| regex back-references (T6) | `\(a*\)\1`×10 … on 302 bytes (probe, one call) | ≈3,400 ms (Zen 1, in sed) | 126.6 ms | 4.3 ms | ≈800 → 29.7; growth now quadratic like glibc's | 9df2454 |
| regex back-references (T6) | the same on 1,025 bytes | (cubic: minutes) | 1,626 ms | 36.9 ms | → 44 | 9df2454 |
| memory | `bench_mem` `memcpy_4k` (+16 placement) | 333.19 | 79.46–80.58 (six runs) | (ref 48.07) | budget 53 not met: see "could not improve" | 390c317 |
| memory | `bench_mem` `memset_4k` (+16) | 75.29 | 45.30–45.59 | (ref 48.07) | budget met | 390c317 |
| memory (PJ-7 retune) | `bench_mem` `memcpy_4k` / `memcpy_4k_off`, Haswell-EP | 79.46 / 81.69 (p5b-before) | 81.96 / 81.47 (no AVX2, 3 runs) | (ref 47.8–48.1) | not closed: SSE2 loops either order; worst placement at 4 KiB on Zen 1 394 → 116 ns (aligned stores), +100 bytes 161 → 85 (Haswell-EP, descending order) | fb968a4 |
| memory (PJ-7 AVX2 copy) | `bench_mem` `memcpy_4k` / `memcpy_4k_off`, Haswell-EP | 81.96 / 81.47 | 50.85–53.84 / 50.85 | (ref 48.07–50.90 / 47.23–47.79) | budget 1.1 × ref met (1.06); Zen 1 memcpy_4k 99.0 → 91.0 (ref 91.4), 64k 1650 (ref 1479–1536, unchanged path class) | 5edc6f5 |
| memory | copy probe, 1,279 bytes aligned | 51.8 | 27.8 | 13.1 | 4.0 → 2.1 | 390c317 |
| memory | copy probe, 2 KiB at +16 | 194.2 | 47.8 | 22.1 | 8.8 → 2.2 | 390c317 |
| fnmatch (direct matcher) | `fnmatch_names_star_c` | 564.72 (558.06–595.15) | 100.00 (99.94–100.76) | 135.19 | 4.42 → 0.74 | 276af9d |
| fnmatch | `fnmatch_names_ch` | 687.62 (661.50–694.13) | 124.69 (124.14–124.77) | 150.13 | 4.73 → 0.83 | 276af9d |
| fnmatch | `fnmatch_names_lib_so` | 571.34 (562.59–610.01) | 89.76 (89.40–90.29) | 93.51 | 6.24 → 0.96 | 276af9d |
| fnmatch | `fnmatch_names_period` | 449.51 (438.35–480.37) | 106.52 (106.01–107.20) | 78.05 | 5.80 → 1.36 | 276af9d |
| fnmatch | `fnmatch_names_question` | 715.41 (704.29–731.82) | 102.87 (102.76–104.05) | 109.06 | 6.63 → 0.94 | 276af9d |
| fnmatch | `fnmatch_names_class` | 760.50 (758.13–763.87) | 253.02 (252.74–253.19) | 150.43 | 5.02 → 1.68 | 276af9d |
| fnmatch | `fnmatch_names_casefold` | 863.16 (851.22–879.39) | 351.03 (348.86–368.01) | 210.22 | 4.10 → 1.67 | 276af9d |
| fnmatch | `fnmatch_paths_pathname` | 821.30 (820.47–857.00) | 115.06 (114.94–119.70) | 147.61 | 5.68 → 0.78 | 276af9d |
| fnmatch | `fnmatch_paths_leading_dir` | 548.30 (538.34–562.09) | 91.50 (91.34–95.72) | 89.66 | 6.23 → 1.02 | 276af9d |
| fnmatch | `fnmatch_star` (micro) | 402.66 (398.95–422.28) | 30.06 (29.78–31.61) | 32.13 | 12.53 → 0.94 | 276af9d |
| fnmatch | `fnmatch_bracket` | 778.56 (772.85–831.15) | 84.76 (84.47–89.85) | 73.22 | 10.64 → 1.16 | 276af9d |
| fnmatch | `fnmatch_pathname` | 722.25 (716.41–732.36) | 47.40 (47.34–48.91) | 52.36 | 13.75 → 0.91 | 276af9d |
| fnmatch | `fnmatch_pathname_nomatch` | 506.15 (501.86–521.21) | 28.78 (28.68–33.79) | 26.01 | 19.45 → 1.11 | 276af9d |
| strcoll (first level directly, equal strings, elements kept per level) | `strcoll_en_us_names` | 135.37 (135.20–140.27) | 51.76 (50.56–52.15) | 30.71 | 4.41 → 1.69 | d498548 |
| strcoll | `strcoll_en_us_equal_16` | 2880.04 (2875.53–3056.78) | 13.38 (13.30–14.41) | 276.33 | 10.44 → 0.05 | d498548 |
| strcoll | `strcoll_en_us_utf8_16` (differs on the last level) | 6287.82 (6269.67–6332.33) | 4071.27 (4053.97–4114.20) | 3814.56 | 1.65 → 1.07 | d498548 |
| strcoll (code-point order as bytes) | `strcoll_c_names` | 17.00 (16.91–17.97) | 14.38 (14.36–14.90) | 7.63 | 2.24 → 1.88 | d498548 |
| strcoll | `strcoll_c_utf8_names` | 17.36 (17.31–18.35) | 15.04 (14.76–15.49) | 7.66 | 2.29 → 1.96 | d498548 |
| strcoll | `cbench_locale` `strcoll_utf8_16` (C.UTF-8) | 17.45 (17.42–17.47) | 15.97 (15.84–16.04) | 4.73 | 3.66 → 3.38 | d498548 |
| wordexp (one text buffer, reused pieces and fields, IFS byte set, operator codes) | `wordexp_tilde_var` | 2973.71 (2953.96–3158.49) | 1961.37 (1960.34–1974.71) | 333.66 | 9.29 → 5.88 | 1cbca1f |
| wordexp | `wordexp_arith` | 2815.31 (2790.93–2938.42) | 1842.52 (1838.66–1846.11) | 354.17 | 7.94 → 5.20 | 1cbca1f |
| wordexp | `wordexp_plain_8` | 3252.04 (3234.53–3442.99) | 2127.22 (2126.08–2228.68) | 586.34 | 5.65 → 3.63 | 1cbca1f |
| wordexp | `wordexp_quoted` | 1907.38 (1906.18–2002.15) | 1503.14 (1492.69–1566.54) | 257.41 | 7.47 → 5.84 | 1cbca1f |
| wordexp | `wordexp_split_var` | 2762.66 (2760.82–2909.64) | 1753.91 (1751.19–1817.90) | 382.70 | 7.20 → 4.58 | 1cbca1f |
| wordexp | `wordexp_param_forms` | 3751.82 (3733.12–3880.83) | 2532.84 (2527.80–2589.65) | 558.76 | 6.81 → 4.53 | 1cbca1f |
| iconv (no division or fill per run, byte tables for 8-bit charmaps, inline UTF-8, UTF-16/32 ASCII runs, small copies inline) | `iconv_utf8_utf16_ascii_4k` | 23343.66 (22901.89–23563.82) | 2231.32 (2230.82–2234.06) | 7174.21 | 3.26 → 0.31 | 449f663 |
| iconv | `iconv_utf8_utf16_mixed_4k` | 42208.81 (42109.26–42279.70) | 15482.68 (15477.21–15504.44) | 7715.57 | 5.46 → 2.01 | 449f663 |
| iconv | `iconv_utf16_utf8_mixed` | 44634.35 (44543.26–44795.41) | 19012.26 (19001.18–19870.95) | 8161.75 | 5.47 → 2.33 | 449f663 |
| iconv | `iconv_utf8_utf32_mixed_4k` | 43384.97 (43334.66–43481.89) | 17111.98 (17082.18–17140.19) | 6745.73 | 6.42 → 2.54 | 449f663 |
| iconv | `iconv_utf8_8859_15_latin_4k` | 63188.35 (63098.51–63310.36) | 16287.71 (16270.53–16849.31) | 10362.00 | 6.11 → 1.57 | 449f663 |
| iconv | `iconv_8859_15_utf8_latin` | 67654.17 (67457.22–67756.48) | 16073.01 (16063.77–16121.33) | 8857.36 | 7.64 → 1.81 | 449f663 |
| iconv | `iconv_utf8_utf16_name` | 127.41 (127.11–127.67) | 66.75 (66.68–69.53) | 84.61 | 1.49 → 0.79 | 449f663 |
| iconv | `cbench_locale` `iconv_utf8_utf16_4k` | 35796.39 (35747.55–35850.64) | 11336.89 (11322.30–11368.46) | 6949.26 | 5.15 → 1.63 | 449f663 |
| iconv | `cbench_locale` `iconv_utf8_8859_15_4k` | 54610.77 (54518.44–55672.86) | 22289.94 (22228.26–22869.59) | 12871.97 | 4.24 → 1.73 | 449f663 |
| iconv | `cbench_locale` `iconv_utf8_ascii_translit_4k` | 79247.61 (79220.99–79588.13) | 58818.88 (58771.31–59236.18) | 43501.20 | 1.81 → 1.35 | 449f663 |
| zones (rule changes per year precomputed, transitions by binary search) | `mktime_posix_rule` | 753.53 (750.74–759.98) | 332.80 (332.01–336.58) | 159.86 | 4.74 → 2.08 | ce1e8a5 |
| zones | `localtime_r_posix_rule` | 245.75 (245.17–246.64) | 111.23 (110.94–111.81) | 58.52 | 4.20 → 1.90 | ce1e8a5 |
| zones | `mktime_new_york` | 426.83 (426.53–429.82) (50b327f) | 279.56 (278.87–287.08) | 183.07 | 2.35 → 1.53 | ce1e8a5 |
| zones | `cbench_time` `mktime_new_york` | 420.00 (417.23–424.34) | 259.76 (259.11–268.34) | 189.79 | 2.25 → 1.37 | ce1e8a5 |
| zones | `cbench_time` `localtime_r_posix_rule` | 247.91 (247.16–254.11) | 110.88 (110.60–111.55) | 59.15 | 4.20 → 1.87 | ce1e8a5 |
| zones (rule years computed when first needed, not at load) | `first_localtime_r_new_york` (cold) | 80090.00 (79746.00–81899.00) | 63041.00 (62010.00–64446.00) | 27762.00 | 2.96 → 2.27 (the load-time table of ce1e8a5 had cost 18 µs) | 3096799 |
| zones | `mktime_posix_rule` | 332.80 (332.01–336.58) | 377.78 (375.09–390.81) | 160.57 | 2.08 → 2.35 (the price of the first-use check) | 3096799 |
| zones | `localtime_r_posix_rule` | 111.23 (110.94–111.81) | 124.38 (123.83–137.61) | 58.63 | 1.90 → 2.12 | 3096799 |
| printf (simple formats rendered directly) | `snprintf_d` | 159.01 (155.65–163.48) | 56.10 (56.07–58.28) | 45.70 | 3.47 → 1.23 | ffcd3b0 |
| printf | `snprintf_ld_large` | 163.29 (160.01–166.92) | 58.25 (58.24–60.39) | 52.53 | 3.11 → 1.11 | ffcd3b0 |
| printf | `snprintf_x08` | 167.32 (167.08–176.06) | 58.22 (58.19–58.35) | 50.46 | 3.32 → 1.15 | ffcd3b0 |
| printf | `snprintf_s_path` | 147.08 (147.01–151.18) | 54.78 (54.70–54.84) | 35.43 | 4.21 → 1.55 | ffcd3b0 |
| printf | `snprintf_s_d_colon` | 318.19 (318.02–333.90) | 130.45 (130.25–130.61) | 84.41 | 3.77 → 1.55 | ffcd3b0 |
| printf | `cbench_stdio` `fprintf_d` | — | 89.89 (89.79–90.09) | 57.56 | → 1.56 | ffcd3b0 |
| printf (length modifiers in simple formats) | `snprintf_ls_line` | 867.17 (866.99–875.48) | 348.91 (348.54–349.59) | 265.06 | 3.27 → 1.32 | 9bf9605 |
| printf | `snprintf_d` | 56.10 (56.07–58.28) | 60.82 (60.75–63.64) | 45.75 | 1.23 → 1.33 (the modifier scan) | 9bf9605 |
| strftime (fields below 10000 converted directly) | `strftime_iso` (`cbench_time`) | 284.98 (284.68–285.12) | 266.27 (265.86–266.81) | 103.48 | 2.75 → 2.57 | d1f411a |
| strftime | `strftime_ls_iso` | 207.62 (206.79–218.95) | 180.90 (180.83–181.68) | 75.80 | 2.74 → 2.39 | d1f411a |
| strftime | `strftime_date_en_us` | 366.36 (365.35–378.32) | 337.96 (337.08–343.97) | 116.08 | 3.15 → 2.91 | d1f411a |
| strftime | `strftime_c` (`cbench_time`) | 485.37 (481.98–485.78) | 450.31 (449.58–452.05) | 187.48 | 2.59 → 2.40 | d1f411a |
| regex (plain strings searched directly by regexec) | `re_grep_literal` | 1021.82 (1005.56–1102.67) | 96.63 (96.32–99.73) | 83.17 | 12.39 → 1.16 | 84fa1eb |
| regex | `regexec_literal_1k` | 6312.75 (6274.46–6527.79) | 1690.60 (1402.96–1739.98) | 2222.80 | 2.84 → 0.76 | 84fa1eb |
| regex (no match at once when the leading bytes are absent) | `re_grep_dotstar` | 2447.66 (2413.12–2567.06) | 1515.98 (1502.72–1558.74) | 259.10 | 9.43 → 5.85 | 7ff7d98 |
| regex | `regexec_literal_1k` | 1690.60 (1402.96–1739.98) | 1257.53 (1255.75–1624.16) | 2235.44 | 0.76 → 0.56 | 7ff7d98 |
| getenv and gettext (environment entries read only as far as the name, `AT_SECURE` read once, equal codeset names) | `gettext_found` | 558.97 (558.84–559.31) | 481.87 (481.54–565.65) | 71.86 | 8.18 → 6.71 | 9e9660b |
| getenv and gettext | `gettext_missing` | 474.90 (473.77–489.68) | 444.99 (444.07–455.28) | 322.70 | 1.47 → 1.38 | 9e9660b |
| getenv and gettext | `gettext_no_catalog` | 431.53 (430.56–444.56) | 396.56 (396.37–405.79) | 237.81 | 1.80 → 1.67 | 9e9660b |
| getenv and gettext | `wordexp_param_forms` | 2562.19 (2522.94–2594.77) | 2432.21 (2429.73–2451.23) | 559.73 | 4.61 → 4.35 | 9e9660b |


### Could not improve

| Target | Gap left | Class (ABI change; policy change: unsafe or assembly; architectural change; glibc uses assembly) | Reason |
|---|---:|---|---|
| `memcpy` 2–4 KiB on Haswell at a short misaligned source distance (`bench_mem` `memcpy_4k`, 80 ns against a 53 ns budget) | 1.6× glibc | Policy change (assembly): RULING NEEDED | The Rust loops are SSE2 (the baseline build has no AVX and Rust has no safe runtime dispatch, P4); `rep movsb` is several times slower at that placement; glibc uses 32-byte AVX2 moves. A C-layer AVX2 copy is a new instruction sequence. The budget is met on other placements and on Zen 1. |
| `strcmp` of short names | 2.3× | Policy change (assembly) | Two measuring scans and a compare: three calls into the C layer where glibc makes one pass. A single-pass C-layer `strcmp` scanner is new assembly (RULING NEEDED). |
| `fnmatch` with `FNM_CASEFOLD` (en_US.UTF-8) | 1.67× | Architectural change | A third of the time is the locale's lower-case mapping per compared character (the locale file's table walk). A per-locale cache of the mapping for ASCII would serve `towlower` too; deferred as a change to the `ctype` layer. |
| `strcoll` in the C and C.UTF-8 locales | 1.9–3.4× | Policy change (assembly) | The `strcmp` path: two measuring scans and a compare in the C layer (the `strcmp` row above; RULING NEEDED). |
| `strcoll` in `en_US.UTF-8`, file names | 1.69× | Architectural change | The first level is compared directly now; what remains is per-element table reads through bounds-checked slices and the two measuring scans. A per-file table of single-byte elements built at load (P8) would need a load-time hook in `LC_COLLATE` data; deferred. |
| `wordexp` | 3.6–5.9× | Architectural change | A fixed cost of about 1.3 µs a call: the parse builds a tree (a segment list per word, a heap value per expansion), each variable is a scan of `environ` that measures every entry, and the result is the caller's vector plus one allocation per word (as glibc). Parsing straight into the pieces, without a tree, is a rewrite of the expander; deferred. A bounded `getenv` scan was tried and gained nothing measurable (reverted). |
| `iconv` of mixed text (non-ASCII characters) | 1.6–2.5× | Architectural change | Each non-ASCII character goes through the general decode, encode and staged copy (one character at a time, every step bounds-checked); specialised loops per codec pair would be needed. |
| `localtime_r`/`mktime` with a POSIX rule | 1.9–2.1× | Architectural change | Each call places the instant among the six changes of three years (from the table now) and converts the calendar again; glibc keeps the last year's changes between calls. Keeping state across calls needs a per-zone cache with atomics; deferred. |
| `regexec` fixed cost for patterns that are not plain strings (≈575 ns a call on a 7-byte subject; glibc ≈70) and short subjects with groups (6.7 µs for `^([a-z]+)=([0-9]+)$` on `key=123`; glibc 0.9 µs) | 8× / 7.4× | Policy change (unsafe) and architectural change | Every call decodes the program from the `regex_t` block and derives its tables again (about 20 allocations), and the matcher sets up its frontiers afresh. Reading the block in place and sizing the vectors once was measured and gained nothing (not kept). Keeping decoded programs between calls needs a cache from block address to program, reached through new `unsafe` or a thread-block slot (RULING NEEDED); reusing the matcher's buffers across calls needs the same. |
| `gettext` of a translated message | 6.7× | Policy change (behaviour) and architectural change | Each lookup reads `LANGUAGE` from the environment (a scan of `environ`, one C-layer call per entry), builds and compares the resolution key, checks the catalog's file time, and parses the charset names. Remembering `LANGUAGE` between calls by the environment's identity would miss a program that writes into the `environ` array itself (RULING NEEDED); the rest needs per-catalog codecs stored at load. |
| `strftime` | 2.2–2.9× | Architectural change | Every directive goes through the general field engine (a dispatch over the directive and its flags, then a padded field through a `dyn` output); a direct path for unflagged numeric and name directives writing into the caller's buffer would remove most of it. |
| `fnmatch` with `[[:upper:]]` (en_US.UTF-8) | 1.68× | Architectural change | The class name is looked up and the class tested through the locale file on every call; the same `ctype` cache would serve it. |
| `setlocale` again with the name in force (`second_setlocale_en_US`) | 86× | Policy change (behaviour): RULING NEEDED | Every call resolves the name again (alias file, archive, candidates). A black-box probe shows glibc keeps the loaded data when the name equals the one in force, even after `LOCPATH` changes or the files go away (it reloads only after another name); Liella resolves again and picks up the new `LOCPATH`. Matching glibc (keep the category when its name is unchanged) would make the call a comparison, and is an observable change. |
| `bindtextdomain`+`gettext` and `gettext` without a catalog right after a setting change (`second_*` cold rows) | 12–19× | Owner ruling (D-7.12) | After `setlocale`, `bindtextdomain` and the other setting changes every catalog is checked against its file at once (the owner's rate-limit ruling), so the first lookups stat the catalog paths again; glibc never re-reads. |
| `malloc`/`free` of 1 MiB (`bench_heap` `malloc_free_1m`, 45 ns against glibc's 28 on Haswell-EP) | 1.66× | Architectural change | A kept large block is reused without a system call, but each pair finds the block twice in the shared large-block table and changes its cached flag with two atomic read-modify-writes: the cross-thread double-free check of D-3.13. glibc serves the pair from its main heap once its dynamic mmap threshold has risen. Halving it needs per-thread ownership of kept large blocks without the shared flag, a change to the allocator's design (not an optimization-pass target). |
| `putc` (`cbench_stdio`), stdio output (`fputs_32`, `fwrite_4k`) | 4.3×, 2.1×, 1.24× | Architectural change | Each call builds the stream's views (`stdio::parts`) and takes the stream lock through the general path; glibc's `putc` is a pointer compare and a store. A fast path on the buffer pointers needs a stream representation the boundary can test without the views. Not one of this pass's targets; the validation profiles show stdio output in `cut-tr-uniq` and `seq`. |
| `printf` with a floating conversion (`snprintf_e`, `snprintf_mixed`, `snprintf_g`, `snprintf_f2`) | 1.4–3.6× | Architectural change | Formats with floating conversions take the general parse and renderer; `%.6e` also pays the exact digit generation. |
| `regexec` with subexpressions on short subjects (`re_sed_kv` 5.2×, `regexec_submatch_1k` 3.3×) | 3.3–5.2× | Architectural change | The NFA simulation with captures (the frontiers and their buffers are set up per call); a lazy DFA for the overall match and buffers kept with the compiled program are the remedies (D-14.4). |

### Rows added at the end of the pass

The before-and-after rows above cover every commit with its own numbers; four earlier commits
of this pass were measured in their commit messages only: `e083cad` (regex overall match over
flat arrays), `ed4ecf6` (printf parsing into inline buffers, exact small floats), `d94e7eb`
(setlocale lookup cache) and `d37d74a` (strftime inline buffers). Their effect is in the final
ratios (`target/liella/bench/*.compare.tsv` of the final run, and the `ratio_host` rows of
`bench/budgets.tsv`).

### ratio_host limits per CPU (BUDGET-SPLIT, 2026-10-05)

The gate on a `ratio_host` row is the same-run MEDIAN Liella/glibc (dynamic) ratio over 5 rounds
(what `cargo xtask bench --compare` prints), checked against the limit for the machine's CPU model
(`model name` in /proc/cpuinfo) in `bench/host_limits.tsv`. A CPU without entries is measured and
reported, never gated. The worst ratio (slowest Liella round / fastest glibc round) is printed as a
diagnostic only. The target, a ratio of 1.10 or less, is separate and informational (82 of the 114
rows miss it on Zen 1, 84 on Haswell-EP).

Limits are 1.25 x the median measured on that CPU on the merged tree (main 22e1fca plus the
optimization pass), rounded up to 3 decimals. Haswell-EP: Intel Xeon E5-2697 v3; Zen 1: AMD EPYC 7551.
Reasons: a limit above the earlier (worst-based, Haswell-EP) bound is tagged "noise (DRIFT-CHECK)" in the
limits file, because DRIFT-CHECK found no slowdown outside the nine rows below; on Zen 1 the same
tag reads "first Zen 1 calibration" (the earlier bounds were Haswell-EP's). Nothing is accepted yet.

Nine rows are confirmed slowdowns since the merge (DRIFT-CHECK: worse by more than 5% in both paired
runs): `cold_exec_empty`, `snprintf_d`, `snprintf_ld_large`, `snprintf_x08`, `snprintf_s_d_colon`,
`snprintf_ls_line`, `wordexp_quoted`, `wordexp_split_var` (cbench_opt) and `fnmatch_pathname`
(cbench_patterns). On Haswell-EP they are `known_regression` rows with the limit 1.05 x their pre-merge
median (so that a 9% slowdown is visible; the earlier worst-based bounds, up to 1.25 x the worst
round, would let most of them pass); `cargo xtask bench` fails on them, labelled "known regression,
PERF-REGRESS", until PERF-REGRESS fixes them. On Zen 1 no pre-merge ratio was measured: report only.

| Benchmark | Row | Haswell-EP median | Haswell-EP limit | Zen 1 median | Zen 1 limit | note |
|---|---|---:|---:|---:|---:|---|
| cbench_opt | `snprintf_d` | 1.39 | 1.376 | 1.05 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `snprintf_ld_large` | 1.28 | 1.271 | 1.04 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `snprintf_x08` | 1.36 | 1.297 | 1.36 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `snprintf_s_path` | 1.80 | 2.252 | 1.71 | 2.139 |  |
| cbench_opt | `snprintf_s_d_colon` | 1.72 | 1.728 | 1.56 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `snprintf_ls_line` | 1.37 | 1.376 | 1.27 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `snprintf_mixed` | 1.67 | 2.093 | 1.94 | 2.424 |  |
| cbench_opt | `snprintf_f2` | 1.35 | 1.687 | 1.96 | 2.447 |  |
| cbench_opt | `snprintf_g` | 1.43 | 1.787 | 2.25 | 2.813 |  |
| cbench_opt | `snprintf_e` | 3.34 | 4.172 | 2.91 | 3.64 |  |
| cbench_opt | `snprintf_f_large` | 0.32 | 0.399 | 0.60 | 0.745 |  |
| cbench_opt | `fprintf_ls_line` | 1.66 | 2.079 | 1.57 | 1.965 |  |
| cbench_opt | `fprintf_du_line` | 1.79 | 2.237 | 1.78 | 2.224 |  |
| cbench_opt | `strcmp_names` | 2.27 | 2.834 | 3.40 | 4.252 |  |
| cbench_opt | `strcmp_equal_16` | 3.99 | 4.984 | 3.66 | 4.579 |  |
| cbench_opt | `strcmp_late_256` | 2.73 | 3.418 | 3.07 | 3.835 |  |
| cbench_opt | `strcoll_c_names` | 1.81 | 2.263 | 2.96 | 3.705 |  |
| cbench_opt | `strcoll_c_utf8_names` | 1.90 | 2.369 | 2.99 | 3.742 |  |
| cbench_opt | `strcoll_en_us_names` | 1.66 | 2.08 | 1.82 | 2.282 |  |
| cbench_opt | `strcoll_en_us_equal_16` | 0.05 | 0.059 | 0.06 | 0.069 |  |
| cbench_opt | `strcoll_en_us_utf8_16` | 1.15 | 1.438 | 1.19 | 1.485 |  |
| cbench_opt | `iconv_utf8_utf16_ascii_4k` | 0.31 | 0.389 | 0.26 | 0.32 |  |
| cbench_opt | `iconv_utf8_utf16_mixed_4k` | 2.05 | 2.567 | 2.45 | 3.067 |  |
| cbench_opt | `iconv_utf16_utf8_mixed` | 2.34 | 2.928 | 2.47 | 3.09 |  |
| cbench_opt | `iconv_utf8_utf32_mixed_4k` | 2.53 | 3.16 | 2.90 | 3.62 |  |
| cbench_opt | `iconv_utf8_utf16_name` | 0.79 | 0.987 | 0.80 | 0.994 |  |
| cbench_opt | `iconv_open_conv_close_name` | 0.27 | 0.34 | 0.25 | 0.317 |  |
| cbench_opt | `iconv_utf8_8859_15_latin_4k` | 1.57 | 1.959 | 1.88 | 2.355 |  |
| cbench_opt | `iconv_8859_15_utf8_latin` | 1.80 | 2.253 | 1.89 | 2.363 |  |
| cbench_opt | `strftime_ls_iso` | 2.39 | 2.988 | 1.95 | 2.438 |  |
| cbench_opt | `strftime_ls_recent_en_us` | 2.93 | 3.668 | 2.32 | 2.897 |  |
| cbench_opt | `strftime_date_en_us` | 2.88 | 3.598 | 2.29 | 2.864 |  |
| cbench_opt | `strftime_iso8601_tz` | 2.48 | 3.103 | 1.91 | 2.383 |  |
| cbench_opt | `strftime_c_en_us` | 2.17 | 2.708 | 1.38 | 1.728 |  |
| cbench_opt | `strftime_log` | 2.84 | 3.545 | 2.09 | 2.614 |  |
| cbench_opt | `strftime_ls_recent_c` | 2.83 | 3.539 | 2.20 | 2.754 |  |
| cbench_opt | `mktime_new_york` | 1.62 | 2.025 | 1.23 | 1.54 |  |
| cbench_opt | `localtime_r_new_york` | 0.24 | 0.295 | 0.21 | 0.268 |  |
| cbench_opt | `mktime_utc` | 1.40 | 1.75 | 1.14 | 1.42 |  |
| cbench_opt | `mktime_posix_rule` | 2.42 | 3.024 | 2.20 | 2.753 |  |
| cbench_opt | `localtime_r_posix_rule` | 2.12 | 2.648 | 2.59 | 3.235 |  |
| cbench_opt | `re_grep_literal` | 1.11 | 1.388 | 1.15 | 1.432 |  |
| cbench_opt | `re_grep_comment` | 5.01 | 6.268 | 5.47 | 6.842 |  |
| cbench_opt | `re_grep_ipv4` | 1.62 | 2.022 | 1.47 | 1.834 |  |
| cbench_opt | `re_grep_alt` | 3.14 | 3.93 | 3.14 | 3.924 |  |
| cbench_opt | `re_grep_icase_absent` | 4.11 | 5.14 | 3.63 | 4.537 |  |
| cbench_opt | `re_grep_dotstar` | 5.96 | 7.45 | 4.68 | 5.845 |  |
| cbench_opt | `re_sed_kv` | 5.12 | 6.399 | 4.81 | 6.008 |  |
| cbench_opt | `re_sed_swap` | 2.01 | 2.518 | 1.72 | 2.152 |  |
| cbench_opt | `regcomp_regfree_sed_kv` | 0.38 | 0.473 | 0.36 | 0.445 |  |
| cbench_opt | `regcomp_regfree_ipv4` | 0.90 | 1.123 | 0.91 | 1.133 |  |
| cbench_opt | `fnmatch_names_star_c` | 0.54 | 0.674 | 0.52 | 0.647 |  |
| cbench_opt | `fnmatch_names_ch` | 0.62 | 0.775 | 0.65 | 0.81 |  |
| cbench_opt | `fnmatch_names_lib_so` | 0.66 | 0.829 | 0.69 | 0.867 |  |
| cbench_opt | `fnmatch_names_period` | 0.97 | 1.219 | 1.03 | 1.292 |  |
| cbench_opt | `fnmatch_names_question` | 0.69 | 0.862 | 0.55 | 0.693 |  |
| cbench_opt | `fnmatch_names_class` | 1.39 | 1.742 | 1.30 | 1.625 |  |
| cbench_opt | `fnmatch_names_casefold` | 1.45 | 1.808 | 1.18 | 1.477 |  |
| cbench_opt | `fnmatch_paths_pathname` | 0.59 | 0.739 | 0.54 | 0.672 |  |
| cbench_opt | `fnmatch_paths_leading_dir` | 0.72 | 0.9 | 0.70 | 0.882 |  |
| cbench_opt | `wordexp_tilde_var` | 6.34 | 7.93 | 6.49 | 8.113 |  |
| cbench_opt | `wordexp_arith` | 5.27 | 6.59 | 5.53 | 6.918 |  |
| cbench_opt | `wordexp_plain_8` | 3.84 | 4.805 | 3.52 | 4.398 |  |
| cbench_opt | `wordexp_quoted` | 6.09 | 5.833 | 5.97 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `wordexp_split_var` | 4.77 | 4.657 | 5.07 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_opt | `wordexp_param_forms` | 4.64 | 5.805 | 4.69 | 5.867 |  |
| cbench_opt | `gettext_found` | 7.31 | 9.133 | 12.62 | 15.779 |  |
| cbench_opt | `gettext_missing` | 1.48 | 1.854 | 1.97 | 2.458 |  |
| cbench_opt | `ngettext_found` | 5.83 | 7.29 | 10.23 | 12.783 |  |
| cbench_opt | `gettext_no_catalog` | 1.83 | 2.29 | 2.56 | 3.207 |  |
| cbench_opt | `gettext_c_locale` | 0.12 | 0.157 | 0.13 | 0.167 |  |
| cbench_opt | `cold_exec_empty` | 0.68 | 0.52 | 0.77 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_patterns | `regcomp_regexec_regfree_email` | 2.77 | 3.468 | 2.04 | 2.548 |  |
| cbench_patterns | `regexec_literal_1k` | 0.56 | 0.705 | 0.47 | 0.585 |  |
| cbench_patterns | `regexec_submatch_1k` | 3.35 | 4.187 | 2.41 | 3.013 |  |
| cbench_patterns | `regexec_class_nomatch_1k` | 2.19 | 2.734 | 2.16 | 2.705 |  |
| cbench_patterns | `regexec_backref` | 3.16 | 3.95 | 3.02 | 3.774 |  |
| cbench_patterns | `regexec_pathological_4k` | 0.02 | 0.029 | 0.01 | 0.019 |  |
| cbench_patterns | `fnmatch_star` | 0.96 | 1.195 | 0.89 | 1.109 |  |
| cbench_patterns | `fnmatch_bracket` | 1.16 | 1.45 | 1.21 | 1.515 |  |
| cbench_patterns | `fnmatch_pathname` | 0.88 | 0.846 | 0.86 | report only | known regression, PERF-REGRESS (Haswell-EP: 1.05 x pre-merge median; Zen 1: report only) |
| cbench_patterns | `fnmatch_pathname_nomatch` | 1.08 | 1.352 | 1.02 | 1.275 |  |
| cbench_patterns | `glob_1000_sorted` | 1.37 | 1.709 | 1.39 | 1.735 |  |
| cbench_patterns | `glob_1000_nosort` | 1.62 | 2.024 | 1.58 | 1.982 |  |
| cbench_patterns | `wordexp_tilde_var` | 6.17 | 7.713 | 6.17 | 7.718 |  |
| cbench_patterns | `wordexp_arith` | 5.11 | 6.387 | 5.46 | 6.819 |  |
| cbench_time | `gmtime_r` | 1.03 | 1.283 | 1.35 | 1.684 |  |
| cbench_time | `timegm` | 1.12 | 1.397 | 0.93 | 1.157 |  |
| cbench_time | `localtime_r_utc` | 1.28 | 1.605 | 1.57 | 1.96 |  |
| cbench_time | `localtime_r_new_york` | 0.27 | 0.332 | 0.23 | 0.288 |  |
| cbench_time | `localtime_r_new_york_2100` | 0.21 | 0.262 | 0.18 | 0.232 |  |
| cbench_time | `mktime_new_york` | 1.45 | 1.812 | 1.18 | 1.469 |  |
| cbench_time | `localtime_r_posix_rule` | 2.11 | 2.64 | 2.57 | 3.217 |  |
| cbench_time | `strftime_iso` | 2.58 | 3.224 | 1.97 | 2.464 |  |
| cbench_time | `strftime_c` | 2.38 | 2.978 | 1.60 | 2.007 |  |
| cbench_locale | `isalpha_c` | 2.14 | 2.673 | 1.86 | 2.32 |  |
| cbench_locale | `toupper_c` | 1.85 | 2.315 | 2.16 | 2.704 |  |
| cbench_locale | `strtod_c` | 0.54 | 0.673 | 0.80 | 1.005 |  |
| cbench_locale | `towupper_utf8` | 1.07 | 1.339 | 0.98 | 1.222 |  |
| cbench_locale | `iswalpha_utf8` | 1.15 | 1.44 | 1.12 | 1.403 |  |
| cbench_locale | `wcwidth_utf8` | 1.07 | 1.334 | 1.81 | 2.265 |  |
| cbench_locale | `mbrtowc_utf8_3` | 1.87 | 2.343 | 1.85 | 2.315 |  |
| cbench_locale | `strcoll_utf8_16` | 3.35 | 4.184 | 3.61 | 4.512 |  |
| cbench_locale | `strtod_utf8` | 0.53 | 0.668 | 0.81 | 1.01 |  |
| cbench_locale | `newlocale_free` | 0.33 | 0.415 | 0.36 | 0.452 |  |
| cbench_locale | `iconv_utf8_utf16_4k` | 1.56 | 1.953 | 1.38 | 1.723 |  |
| cbench_locale | `iconv_utf8_8859_15_4k` | 1.76 | 2.2 | 1.93 | 2.409 |  |
| cbench_locale | `iconv_utf8_ascii_translit_4k` | 1.37 | 1.717 | 1.28 | 1.602 |  |
| cbench_stdio | `snprintf_d` | 1.37 | 1.708 | 1.07 | 1.344 |  |
| cbench_stdio | `snprintf_mixed` | 1.66 | 2.072 | 2.01 | 2.513 |  |
| cbench_stdio | `fprintf_d` | 1.65 | 2.062 | 1.36 | 1.697 |  |
| cbench_stdio | `fputs_32` | 2.05 | 2.562 | 2.36 | 2.947 |  |
| cbench_stdio | `putc` | 4.43 | 5.54 | 3.89 | 4.863 |  |
| cbench_stdio | `fwrite_4k` | 1.21 | 1.509 | 1.48 | 1.853 |  |

### Next step

The optimization pass ends here (stage report 2026-10-05). Open:
1. Done (BUDGET-SPLIT): `ratio_host` limits per CPU model on the median ratio (table above).
2. The rulings asked in the stage report (setlocale with an unchanged name, a program cache for
   `regexec`, `LANGUAGE` kept between gettext calls, the AVX2 copy and the single-pass `strcmp`
   scanner).
3. The regex oracle test (`pattern::regex_oracle_diff`) takes the TEST thread's harvested suite
   traces when they land.
