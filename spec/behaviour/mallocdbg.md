# Allocator introspection and debugging: mallopt, mallinfo, mallinfo2, malloc_stats, malloc_info, malloc_trim, the hooks, mcheck, mtrace

**Spec:** §3.2 (`mallopt`, `mallinfo`, `mallinfo2`, `malloc_stats`, `malloc_info`, `malloc_trim`,
the `M_*` constants) and §3.3 (`__malloc_hook`, `__free_hook`, `__realloc_hook`,
`__memalign_hook`, `__after_morecore_hook`, `__malloc_initialize_hook`, `__morecore`,
`__default_morecore`, `malloc_get_state`, `malloc_set_state`, `mcheck`, `mcheck_pedantic`,
`mcheck_check_all`, `mprobe`, `mtrace`, `muntrace`, `mallwatch`, `tr_break`), with
`malloc_usable_size` (§3.1), the legacy `MALLOC_*_` variables and `GLIBC_TUNABLES`
`glibc.malloc.*` (§1.8), and the decision rows D-3.4 (introspection is implementation-neutral) and
D-3.5 (the hooks are inert compat data; `mcheck`/`mtrace` semantics come with the debug library in
phase 13; `malloc_get_state`/`malloc_set_state` fail) of implementation-plan.md. Public
interfaces: `<malloc.h>`, `<mcheck.h>`. Normative references (plan §1.2): the man pages
`mallopt(3)`, `mallinfo(3)`, `malloc_info(3)`, `malloc_stats(3)`, `malloc_trim(3)`,
`malloc_usable_size(3)`, `malloc_hook(3)`, `malloc_get_state(3)`, `mcheck(3)`, `mtrace(3)` (Linux
man-pages 6.7, as installed on the VM), and the glibc manual's Memory Allocation Tunables, Heap
Consistency Checking and Allocation Debugging (Tracing malloc) sections (the 2.39 edition for the
tunables, the current edition otherwise).

**Clauses:** `s1_8_c_0`, `s3_1_c_7`, `s3_2_c_1`, `s3_2_c_2`, `s3_2_c_3`, `s3_2_c_4`, `s3_2_c_5`,
`s3_2_c_6`, `s3_3_c_1`, `s3_3_c_2`, `s3_3_c_3`, `s3_3_c_4`.

No installed header declares the hook variables, `__morecore`, `__default_morecore`,
`malloc_get_state`, `malloc_set_state`, `mallwatch` or `tr_break` (a `-Werror` compile naming
`__malloc_hook` with `<malloc.h>` fails: "undeclared"). The probe reaches them only through
weak references of its own and through `dlvsym(handle, name, "GLIBC_2.2.5")`, both of which may be
absent; it prints what it finds, so it also builds statically against Liella's products.

**Probe:** `crates/core/tests/oracle/mallocdbg_diff.c`, run by
`mallocdbg_run.sh PROGRAM SCRATCH [DEBUGLIB]`.
**Record:** `mallocdbg.glibc.txt` (1108 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic, gcc 13.3.0, 31 GiB RAM, `vm.overcommit_memory` 0) with
`cc -std=gnu17 -O1 -o p mallocdbg_diff.c` and
`env -i PATH=/usr/bin:/bin sh mallocdbg_run.sh p SCRATCH /lib/x86_64-linux-gnu/libc_malloc_debug.so.0`,
twice, with scratch directories of different names (`~/mallocdbg/run-a`,
`~/mallocdbg/run-second-longer`): the two outputs are byte-identical, and identical to an `-O0`
build run with a third scratch name (SHA-256 `2c39f9d3...1d97` for all three). A run without the
third argument printed exactly the first 842 lines. Nothing was left behind: the trace file is
removed by the program, the scratch directories and the build were removed afterwards, the
probe's own core-file limit is 0 and `/var/crash` gained nothing.

The record has two parts.

1. **Lines 1 to 842, the plain library** (no preload; what the differential test compares):
   the `main` run, then the runs `mcheck-early`, `mcheck-null`, `pedantic`, `late`,
   `hooks-early i|m|f|imf`, `mtrace` (with and without `MALLOC_TRACE`), and 24 runs of the
   `env` mode, one per environment (`#### env NAME=VALUE`).
2. **From line 843 (`== reference only: LD_PRELOAD of the debug library`)**: the same probe with
   `LD_PRELOAD=libc_malloc_debug.so.0`, for the reference only (a static program ignores
   `LD_PRELOAD`; this part documents the semantics phase 13 is to provide, D-3.5). It is printed
   only when `DEBUGLIB` is given.

## What the probe does

Each run is headed `#### <name>` and ends `#### exit status N` (128 + the signal number for a run
killed by a signal; the shell's own report of the signal is discarded). Statistics are never
printed as numbers: each field of `struct mallinfo2` is `grew`, `shrank` or `unchanged` across a
step, or `zero`/`nonzero`, plus explicit relations (`uordblks grew by at least 100000: yes`).
Text written by the library (`malloc_stats`, `malloc_info`, abort messages) is printed one line
per output line in brackets, every `0x<hex>` replaced by `0x?`; for `malloc_stats` and
`malloc_info` every decimal number is also replaced by `N` and the blank padding before a number
by one space. Risky calls run in forked children (the child's status follows its output, as
`child exited N` or `child killed by signal S`; a child's stderr is captured and printed as
`stderr: [...]`). Every allocation's result escapes through a volatile store, stores that corrupt
a block are volatile, and the hook variables and the counters changed inside `malloc`/`free` are
volatile: without this the compiler removes `malloc`/`free` pairs and the stores around them
(an earlier `-O1` build recorded hooks as never called while `-O0` called them).

* **Constants** (`== constants`): the `M_*` values, the `MCHECK_*` values, the two structure sizes.
* **`mallopt`** (`== mallopt`): every parameter of `<malloc.h>` with valid, boundary and invalid
  values, and seven unknown parameters (0, 5, 100, -9, -100, `INT_MIN`, `INT_MAX`), each in a
  fresh child, with `errno` preset to `EINTR`. **Effects** (`== mallopt effects`): `M_PERTURB` on
  fresh, `calloc`, mmapped and `realloc`-grown bytes; `M_MMAP_THRESHOLD` 4096 and 64 MiB;
  `M_MMAP_MAX` 0; `M_CHECK_ACTION` 0 before a double free.
* **`mallinfo`, `mallinfo2`** (`== mallinfo and mallinfo2`): the fields before the process's first
  allocation, after 100 × `malloc(1000)`, after an 8 MiB block, after freeing it and the rest;
  `mallinfo` against `mallinfo2` (only between two equal `mallinfo2` calls); a second thread's 200 ×
  `malloc(1000)`; more than 2 GiB of untouched blocks (3 × 1 GiB) through `mmap` and, with
  `M_MMAP_MAX` 0, through the program break, each field of `mallinfo` classified as `equal`, `INT_MAX
  (saturated)`, `low 32 bits (negative)` or `other`.
* **`malloc_usable_size`**: 14 sizes from 0 to 8 MiB, NULL, and blocks from `calloc`, `realloc`
  (grow, shrink), `aligned_alloc`, `memalign`.
* **`malloc_stats`**: in a fresh child; with a large, a small and another thread's block; with a
  fully buffered `stderr` holding pending text; `errno`; with descriptor 2 closed.
* **`malloc_info`**: options 0, 1, -1, `INT_MIN` into an `open_memstream` stream; with a second
  arena; a NULL stream (child); a read-only stream.
* **`malloc_trim`**: a sequence (fresh, 3000 live blocks, the first 2000 freed, again, all freed,
  again); pads of 1 TiB, 64 KiB and 0 with automatic trimming off; after freeing a 16 MiB block;
  after freeing a thread arena's only block.
* **Hooks and compat symbols** (`== hook variables and compat symbols in libc.so.6`): the weak
  references; 18 names looked up in `libc.so.6` (`dlopen` with `RTLD_NOLOAD`) by `dlvsym` at
  `GLIBC_2.2.5` and by unversioned `dlsym`; initial values of `mallwatch`, `__malloc_hook`,
  `__free_hook`; `tr_break()`; then, in children, each of `__malloc_hook`, `__free_hook`,
  `__realloc_hook`, `__memalign_hook`, `__after_morecore_hook` assigned a counting function around
  the matching call, `__morecore` replaced by a function that fails, `__default_morecore(0)`
  against `sbrk(0)`, `malloc_get_state()`, `malloc_set_state()` with NULL, a zeroed buffer, a
  buffer of 0xff bytes and a zeroed heap block (each in its own child). `hooks-early`: the hooks
  (`i` `__malloc_initialize_hook`, `m` `__malloc_hook`, `f` `__free_hook`) assigned before the
  process's first allocation.
* **`mcheck` family**: `late` (after allocations): `mcheck(abortfn)`, `mcheck_pedantic`,
  `mcheck(NULL)`, `mprobe` of a good and of an overrun block, `mcheck_check_all`, `mtrace` and
  `muntrace` without `MALLOC_TRACE`, `muntrace` twice. `mcheck-early` (`mcheck(abortfn)` as the
  first statement of `main`): a second call, tail and head corruption, a double free,
  `mcheck_check_all` with a clobbered tail, `realloc` of a clobbered block, `mprobe` of a live
  block, of NULL and of a static buffer, each in a child; `abortfn` prints the status it gets and
  returns. `mcheck-null` (`mcheck(NULL)` first): tail, head, double free with the default handler.
  `pedantic` (`mcheck_pedantic(abortfn)` first): a clobbered block, then another `malloc`.
* **`mtrace`** with `MALLOC_TRACE` unset, naming a file in SCRATCH, and (preloaded) naming a file in
  a missing directory: `errno` after `mtrace`, a fixed sequence (`malloc(10)`, `calloc(4, 8)`,
  `realloc` to 100, `memalign(64, 50)`, `posix_memalign(64, 70)`, three frees, `free(NULL)`,
  `realloc(NULL, 5)`, `realloc(z, 0)`), `muntrace`, then one more `malloc`/`free`; whether the file
  exists, its mode against the umask, and each log line with the caller shown as its shape
  (`<prog>:[0x?]` or `<prog>:(<sym>+<off>)[0x?]`), block addresses as `<addr>`, sizes kept.
* **`env`** (one run per environment): a fingerprint (fill of a fresh `malloc(200)`; whether the
  first allocation leaves 1 MiB free in the arena; whether 64 KiB and 1 MiB blocks are mmapped;
  40 × `malloc(60000)` freed in reverse: does the arena grow and shrink back; 7 × `malloc(32)`
  freed: does `uordblks` come back; how many heaps `malloc_info` lists after a second thread's
  `malloc(5000)`), then in children a one-byte overrun of a 24-byte block, a double free, and
  `free` of an interior pointer. Environments: nothing; `MALLOC_CHECK_` 0, 1, 3;
  `MALLOC_PERTURB_` 165, 0; `MALLOC_MMAP_THRESHOLD_=32768`; `MALLOC_MMAP_MAX_=0`;
  `MALLOC_TOP_PAD_=4194304`; `MALLOC_TRIM_THRESHOLD_=1000000000`; `MALLOC_ARENA_MAX=1`; and
  `GLIBC_TUNABLES` with `glibc.malloc.` `perturb=165`, `mmap_threshold=32768`, `mmap_max=0`,
  `top_pad=4194304`, `trim_threshold=1000000000`, `tcache_count=0`, `check=3`, `mxfast=0`,
  `nonexistent=1`, `perturb=junk`, `perturb=300`, `perturb=165:...mmap_threshold=32768`, and
  `perturb=165` together with `MALLOC_PERTURB_=1`. Preloaded: nothing, `MALLOC_CHECK_` 1, 2, 3,
  `glibc.malloc.check=3`.

## Observations the record shows directly

### Constants

* `M_MXFAST` 1, `M_NLBLKS` 2, `M_GRAIN` 3, `M_KEEP` 4, `M_TRIM_THRESHOLD` -1, `M_TOP_PAD` -2,
  `M_MMAP_THRESHOLD` -3, `M_MMAP_MAX` -4, `M_CHECK_ACTION` -5, `M_PERTURB` -6, `M_ARENA_TEST` -7,
  `M_ARENA_MAX` -8. `MCHECK_DISABLED` -1, `MCHECK_OK` 0, `MCHECK_FREE` 1, `MCHECK_HEAD` 2,
  `MCHECK_TAIL` 3. `sizeof(struct mallinfo)` 40, `sizeof(struct mallinfo2)` 80 (ten `int`, ten
  `size_t`).

### mallopt (§3.2)

* `mallopt` returns **1 for every call** in the record except `M_MXFAST` with a value above 160
  or negative (161, -1, `INT_MAX`: 0). In particular it returns 1 for `M_NLBLKS`, `M_GRAIN`,
  `M_KEEP` (which mallopt(3) says glibc does not implement), for every unknown parameter (0, 5,
  100, -9, -100, `INT_MIN`, `INT_MAX`), for `M_MMAP_THRESHOLD` above the documented upper limit
  (32 MiB + 1, 1 GiB, -1, `INT_MAX`), for negative `M_TOP_PAD`, `M_MMAP_MAX`, `M_ARENA_TEST`,
  `M_ARENA_MAX`, `M_PERTURB`, for `M_PERTURB` 256, and for `M_CHECK_ACTION` 8 and -1. `errno` is
  never changed (mallopt(3): "On error, errno is not set").
* Effects: `M_PERTURB` 0xa5 makes fresh blocks (small, mmapped, and the bytes a `realloc` adds)
  all `0x5a`, the complement; `calloc` still zero. `M_MMAP_MAX` 0: a 1 MiB block comes from the
  heap instead of `mmap` (the same request was mmapped before the call). `M_MMAP_THRESHOLD` 64 MiB
  is accepted and honoured (a 40 MiB block then comes from the heap), although mallopt(3) gives
  32 MiB as the upper limit. `M_MMAP_THRESHOLD` 4096 followed by `malloc(8192)`: that block is not
  mmapped (the heap had room). `M_CHECK_ACTION` 0 does not stop the abort on a double free
  (`free(): double free detected in tcache 2`, `SIGABRT`).

### mallinfo and mallinfo2 (§3.2, D-3.4)

* Before the process's first allocation all fields are zero except `ordblks` (nonzero).
* Small blocks: `arena`, `uordblks`, `fordblks`, `keepcost` grow; `uordblks` grows by at least the
  requests. `uordblks + fordblks == arena` holds; `keepcost <= fordblks <= arena`.
* An 8 MiB block (mmapped): `hblks` +1 and `hblkhd` grows by at least 8 MiB; **`uordblks` does not
  change** (mmapped blocks are not in it; `uordblks >= hblkhd` is false). Freeing it brings
  `hblks` and `hblkhd` back.
* After freeing the small blocks, `uordblks` shrinks but does not return to its value before them,
  and `fordblks`, `keepcost` grow. `usmblks` is always zero; `smblks` and `fsmblks` stayed zero in
  every step.
* A second thread's 200 × `malloc(1000)` raises `uordblks` by at least 200000 (and `ordblks`), and
  freeing them brings it down again: **`mallinfo2` includes the other arenas**, against
  mallinfo(3)'s BUGS ("Information is returned for only the main memory allocation area").
* `mallinfo` equals `mallinfo2` field by field while every value fits in `int`.
* Over 2 GiB: 3 × 1 GiB untouched blocks succeed (address space only). Through `mmap`,
  `mallinfo2.hblkhd >= 3 GiB` and `mallinfo.hblkhd` is **the low 32 bits, negative**; through the
  program break (`M_MMAP_MAX` 0), `arena` and `uordblks` exceed 3 GiB and `mallinfo` gives the low
  32 bits (negative) of each. No field saturates; the other fields stay equal. Freeing the break
  blocks shrinks `arena`.

### malloc_usable_size (§3.1)

* For every size 0 to 8 MiB and every allocation function probed the result is at least the
  request (and within 4096 + request/64 of it). `malloc_usable_size(NULL)` is 0, `errno`
  unchanged.

### malloc_stats (§3.2)

* Output on `stderr`, per arena: `Arena N:`, `system bytes     = N`, `in use bytes     = N`; then
  `Total (incl. mmap):`, `system bytes     = N`, `in use bytes     = N`, `max mmap regions = N`,
  `max mmap bytes   = N`. A second arena adds a second `Arena N:` block. (The numbers are
  right-aligned in a field of 10 characters before normalisation.)
* It writes through the `stderr` stream: text pending in a fully buffered `stderr` comes out
  first, on the same line (`[pending-before|Arena N:]`), and text written after it stays buffered
  until flushed. `errno` unchanged. With descriptor 2 closed it returns normally.
* `strace` of a separate small program (not part of the record): one `write(2, ...)` per line.

### malloc_info (§3.2)

* `malloc_info(0, fp)` returns 0 and writes (numbers replaced): `<malloc version="N">`; per arena
  `<heap nr="N">`, `<sizes>`, zero or more `  <size from="N" to="N" total="N" count="N"/>` or
  `  <unsorted from="N" to="N" total="N" count="N"/>` lines, `</sizes>`,
  `<total type="fast" count="N" size="N"/>`, `<total type="rest" count="N" size="N"/>`,
  `<system type="current" size="N"/>`, `<system type="max" size="N"/>`,
  `<aspace type="total" size="N"/>`, `<aspace type="mprotect" size="N"/>`, a non-main arena also
  `<aspace type="subheaps" size="N"/>`, `</heap>`; then for the whole process
  `<total type="fast" .../>`, `<total type="rest" .../>`, `<total type="mmap" count="N" size="N"/>`,
  `<system type="current" .../>`, `<system type="max" .../>`, `<aspace type="total" .../>`,
  `<aspace type="mprotect" .../>`, `</malloc>`. One element per line, no indentation except the
  two spaces before `size`/`unsorted`; no XML declaration.
* Options 1, -1, `INT_MIN`: **the return value is 22 (`EINVAL`) itself**, nothing is written and
  `errno` is unchanged; malloc_info(3) says -1 with `errno` `EINVAL`.
* A NULL stream: `SIGSEGV`. A read-only stream: returns 0, `errno` `EBADF`, the stream's error flag
  set.
* `strace` (separate program): one `write` of the whole text when the stream is flushed.

### malloc_trim (§3.2)

* Returns 1 at the start of a process that has not allocated, with 3000 live blocks, after freeing
  the first 2000 of them, again right after, and after freeing the rest; then 0 on the next call.
  After freeing a 16 MiB (mmapped) block: 1. A thread arena whose only block was freed: 0, `errno`
  unchanged.
* With automatic trimming off (`M_TRIM_THRESHOLD` -1) and 3 MB free at the top of the heap:
  `malloc_trim(1 TiB)` returns 1 but `arena` is unchanged; `malloc_trim(64 KiB)` returns 1 and
  `arena` shrinks (to at least 64 KiB); `malloc_trim(0)` returns 1 and shrinks it again; `keepcost`
  stays nonzero.
* `strace` (separate program, 2000 of 3000 blocks freed): one `madvise(..., MADV_DONTNEED)` over
  the freed pages and one `brk`, within the call.

### Hook variables and compat symbols, plain library (§3.3, D-3.5)

* `libc.so.6` has `__malloc_hook`, `__free_hook`, `__realloc_hook`, `__memalign_hook`,
  `__after_morecore_hook`, `__malloc_initialize_hook`, `__morecore`, `__default_morecore`,
  `mallwatch` and `tr_break` only at version `GLIBC_2.2.5` as non-default versions:
  `dlvsym(..., "GLIBC_2.2.5")` finds them, unversioned `dlsym` does not. Yet an unversioned weak
  reference from the program is bound to them at run time (`weak references ...: __malloc_hook
  bound, ...`).
* **`malloc_get_state` and `malloc_set_state` are not in `libc.so.6`** at all (`absent` at
  `GLIBC_2.2.5`, the weak references unbound).
* `mcheck`, `mcheck_pedantic`, `mcheck_check_all`, `mprobe`, `mtrace`, `muntrace` are present both
  ways (default version).
* `mallwatch`, `__malloc_hook`, `__free_hook` are NULL; `tr_break()` returns.
* Assigning `__malloc_hook`, `__free_hook`, `__realloc_hook`, `__memalign_hook`,
  `__after_morecore_hook` has no effect (each called 0 times, the allocation functions work);
  replacing `__morecore` with a failing function changes nothing (6.4 MB of heap growth succeeds,
  it is never called). Assigned before the first allocation (`hooks-early`), the malloc, free and
  initialize hooks are never called either. `__default_morecore(0)` returns the current break
  (`sbrk(0)`), `errno` unchanged.

### mcheck, mprobe, mtrace without the debug library (§3.3, D-3.5)

* `mcheck(abortfn)`, `mcheck_pedantic(abortfn)` and `mcheck(NULL)` return **-1**, also when called
  as the first statement of `main` before any allocation; `errno` unchanged.
* `mprobe` returns `MCHECK_DISABLED` for every block (good, overrun, freed, NULL, a static
  buffer); `mcheck_check_all()` returns without calling `abortfn`; no corruption is reported by
  the mcheck functions (a head corruption then reaches `free`, which aborts with
  `double free or corruption (out)`; a double free aborts with
  `free(): double free detected in tcache 2`; a tail overrun of a 10-byte block passes).
* `mtrace()` and `muntrace()` return, `errno` unchanged, and **no trace file is created** even with
  `MALLOC_TRACE` naming a writable path; `muntrace()` twice is harmless.

### The environment (§1.8)

Against the fingerprint of the run without environment (lines 434 to 450):

* `MALLOC_CHECK_` (0, 1, 3) and `glibc.malloc.check=3`: **no observable effect** (identical runs),
  as the manual says for 2.34+ without the debug library.
* `MALLOC_PERTURB_=165` and `glibc.malloc.perturb=165`: fresh blocks `all 0x5a`.
  `MALLOC_PERTURB_=0`: as without. When both are given (`perturb=165` and `MALLOC_PERTURB_=1`) the
  tunable wins (`0x5a`).
* `MALLOC_MMAP_THRESHOLD_=32768` / `mmap_threshold=32768`: the 40 × 60000-byte blocks no longer
  change `arena` (they are mmapped). The 64 KiB block of the fingerprint is not mmapped in any run.
* `MALLOC_MMAP_MAX_=0` / `mmap_max=0`: the 1 MiB block is not mmapped, and the arena shrinks back
  after the 60000-byte blocks are freed.
* `MALLOC_TOP_PAD_=4194304` / `top_pad=4194304`: the first allocation leaves at least 1 MiB free in
  the arena, and the later blocks need no growth (`arena unchanged then unchanged`, the 1 MiB block
  from the heap).
* `MALLOC_TRIM_THRESHOLD_=1000000000` / `trim_threshold=1000000000`: the arena does not shrink after
  the frees.
* `MALLOC_ARENA_MAX=1`: `malloc_info` lists one heap after a second thread's allocation (two
  otherwise).
* `glibc.malloc.tcache_count=0`: `uordblks` returns to its level after 7 small blocks are freed;
  the double free is reported as `double free or corruption (fasttop)` and the interior pointer as
  `double free or corruption (out)` instead of `free(): double free detected in tcache 2` and
  `free(): invalid pointer`.
* `glibc.malloc.mxfast=0`, `glibc.malloc.nonexistent=1`, `glibc.malloc.perturb=300`: no observable
  effect, no message. `glibc.malloc.perturb=junk`: the loader prints
  ``WARNING: ld.so: invalid GLIBC_TUNABLES value `junk' for option `glibc.malloc.perturb': ignored.``
  on `stderr` (line 775) and the value is ignored.
* Two tunables separated by `:` are both honoured.
* Without any setting: a one-byte overrun of a 24-byte block is not detected; a double free
  aborts (`free(): double free detected in tcache 2`); `free` of an interior pointer aborts
  (`free(): invalid pointer`); fresh heap blocks read as zero (indeterminate by contract).

## Reference only: with libc_malloc_debug.so preloaded (line 843 on)

* **Where the names come from:** through the global scope, `__malloc_hook`, `__free_hook`,
  `__realloc_hook`, `__memalign_hook`, `malloc_get_state`, `malloc_set_state` and the six `mcheck`
  and `mtrace` functions are the debug library's; `__after_morecore_hook`,
  `__malloc_initialize_hook`, `__morecore`, `__default_morecore`, `mallwatch` and `tr_break` stay
  `libc.so.6`'s. The weak references to `malloc_get_state`/`malloc_set_state` are now bound.
* **Hooks:** `__malloc_hook`, `__free_hook`, `__realloc_hook` are each called once for their call;
  `__memalign_hook` twice (for `memalign` and for `aligned_alloc`). `__after_morecore_hook` and a
  replaced `__morecore` are still never called. Before the first allocation:
  `__malloc_initialize_hook` alone is called once; `__malloc_hook` and `__free_hook` are called
  for the first `malloc`/`free`; with all three assigned the two hooks are called and the
  initialize hook is not.
* **`malloc_get_state()`** returns NULL with `errno` `ENOSYS`. **`malloc_set_state`**: NULL
  crashes (`SIGSEGV`); a zeroed buffer, a buffer of 0xff bytes and a zeroed heap block return -1
  with `errno` left 0.
* **`mcheck`:** 0 when called before the first allocation, -1 when called again or after
  allocations (then `mprobe` gives `MCHECK_DISABLED`). After a successful `mcheck(abortfn)`:
  `mprobe` of a fresh block `MCHECK_OK`; after writing one byte past the end `MCHECK_TAIL`; after
  changing the byte before the block `MCHECK_HEAD`, and `MCHECK_OK` again once it is restored.
  `mprobe` calls `abortfn` itself whenever it finds a problem, and returns the status when
  `abortfn` returns. `free` of a clobbered block calls `abortfn` (`MCHECK_TAIL`) and then
  returns. `realloc` of a clobbered block calls it once and returns a block.
  `mcheck_check_all()` reports the clobbered tail once. A freed block is reported as
  **`MCHECK_HEAD`** by `mprobe`, and a second `free` calls `abortfn` with `MCHECK_HEAD` and then
  crashes (`SIGSEGV`) when `abortfn` returns; `MCHECK_FREE` never appears. `mprobe(NULL)` is
  `MCHECK_OK`; `mprobe` of a static buffer is `MCHECK_HEAD`.
* **Default handler** (`mcheck(NULL)`): the messages `memory clobbered past end of allocated
  block` (tail), `memory clobbered before allocated block` (head, and also the double free), each a
  line on `stderr`, then `SIGABRT`.
* **`mcheck_pedantic`:** returns 0 before the first allocation; the next `malloc` after a block is
  clobbered calls `abortfn` (`MCHECK_TAIL`) once, although the clobbered block is not involved in
  the call. Once the byte is restored, freeing both blocks reports nothing more.
  `mcheck_check_all()` after a plain `mcheck` likewise reports a clobbered tail once, and nothing
  after the byte is restored.
* **`mtrace`** with `MALLOC_TRACE` naming a writable file: the file is created (mode 0666 less the
  umask) and holds `= Start`, one line per event, `= End` (after `muntrace`; the `malloc`/`free`
  after `muntrace` are not logged). Event lines are `@ <caller> <op> <addr> [<size>]` where the
  caller is the program and offset (`<prog>:[0x?]`, or `<prog>:(<sym>+<off>)[0x?]` when the
  program exports symbols, seen with `-rdynamic` in a separate check), `<op>` is `+` (allocation,
  with the size in hex: `0xa` for `malloc(10)`, `0x20` for `calloc(4, 8)`, `0x32` for `memalign`,
  `0x46` for `posix_memalign`, `0x5` for `realloc(NULL, 5)`), `-` (free), and a `realloc` that
  moves a block is the pair `< <old addr>` then `> <new addr> <size>`. `free(NULL)` is not logged;
  `realloc(p, 0)` is logged as `- <addr>`. With `MALLOC_TRACE` unset nothing happens; naming a file
  in a missing directory: no file, and `errno` is left `ENOENT` by `mtrace`.
* **`MALLOC_CHECK_` 1, 2, 3 and `glibc.malloc.check=3`** (all the same): the one-byte overrun, the
  double free and the interior pointer each abort with `free(): invalid pointer` (written without
  a trailing newline) and `SIGABRT`. The fingerprint changes: freed small blocks leave `uordblks`
  (no per-thread cache), the arena shrinks back, and a second thread's block is in the same heap
  (`malloc_info` lists one). Preloaded without `MALLOC_CHECK_` the behaviour equals the plain
  library's.

## Where the spec, the record and the documentation disagree

* **`mallopt`'s return** (§3.2 "returns 1 on success and 0 on error"; mallopt(3) the same): the
  reference returns 1 for unknown parameters, unimplemented SVID parameters and out-of-range values
  of every parameter except `M_MXFAST`; mallopt(3)'s BUGS section concedes that an invalid
  parameter is not an error. D-3.4 (1 only for the neutral parameters, 0 otherwise) and Liella's
  `s3_2_c_1` therefore differ from the record for `M_MMAP_MAX`, `M_CHECK_ACTION`, `M_MXFAST` within
  range, `M_ARENA_TEST`, the SVID constants and unknown parameters: a deliberate difference.
* **`M_MMAP_THRESHOLD`'s upper limit:** mallopt(3) gives 32 MiB on 64-bit systems; the reference
  accepts and honours 64 MiB.
* **`mallinfo` over 2 GiB** (§3.2 ⚑ COMPAT "truncates"; mallinfo(3) "may wrap around zero"): the
  record shows the low 32 bits, negative, which is what "wrap" means; D-3.4 makes Liella saturate
  at `INT_MAX` instead (deliberate, stricter).
* **`uordblks`:** mallinfo(3) calls it "the total number of bytes used by in-use allocations"; the
  reference excludes mmapped blocks from it. Liella's `s3_2_c_2` (D-3.4) requires it to include the
  8 MiB block: a deliberate difference.
* **Other arenas in `mallinfo`:** mallinfo(3)'s BUGS says only the main arena is reported; the
  record shows a second thread's allocations in `uordblks`.
* **`malloc_info` with nonzero options:** malloc_info(3) says -1 and `errno` `EINVAL`; the
  reference returns 22 and leaves `errno` alone.
* **`malloc_trim`'s return** (§3.2 and malloc_trim(3) "1 if memory was actually released"): the
  reference returns 1 in a process that has allocated nothing, with every block live, and with a
  pad larger than the heap while `arena` does not change.
* **`malloc_get_state`/`malloc_set_state`** (§3.3 "compat stubs ... now fail"; spec/symbols.tsv
  lists them at 2.2.5 in §3): `libc.so.6` 2.39 does not export them at all; only the debug library
  does, where `get` fails with `ENOSYS` and `set` returns -1 without setting `errno` (malloc_get_state(3)
  documents -1 and -2 but no `errno`) and crashes on NULL. malloc_get_state(3) says they were
  removed in 2.25. D-3.5 ("fail") is compatible with the debug library's behaviour.
* **The hooks** (§3.3 "assigning them does nothing unless `libc_malloc_debug.so` is preloaded"):
  true for the four allocation hooks and `__malloc_initialize_hook`; `__after_morecore_hook` and
  `__morecore` have no effect even with the debug library. malloc_hook(3) says the hook variables
  were removed from the API in 2.34 and still documents them as working.
* **`mcheck` without the debug library** (mcheck(3) "0 on success"; the manual: -1 "if called
  after initial allocations"): plain `libc.so.6` returns -1 always, before any allocation too.
* **`MCHECK_FREE`** (mcheck(3), the manual, §3.3): never produced; a double free is reported as
  `MCHECK_HEAD` (or the default handler's "memory clobbered before allocated block"), and returning
  from `abortfn` after it crashes.
* **`mprobe` calls `abortfn`:** mcheck(3) describes `abortfn` as called when an inconsistency is
  detected and `mprobe` as returning a status; the record shows both, the call first.
* **`MALLOC_CHECK_`**: mallopt(3) describes value 1 as "print a detailed error message and continue"
  and the message as starting with `*** glibc detected ***`; with the debug library every nonzero
  value aborts with `free(): invalid pointer`, matching the manual ("Any detected heap corruption
  results in immediate termination of the process"). mallopt(3) also says `MALLOC_CHECK_` works
  through the hooks and needs nothing else; the manual and the record agree that without the debug
  library it has no effect. `M_CHECK_ACTION` 0 likewise does not prevent the abort.
* **`glibc.malloc.mxfast`:** the current manual says it "has no effect since the fastbins have been
  removed" (a later version); the 2.39 manual documents it; `mxfast=0` showed no effect in this
  fingerprint, which does not separate the two.
* **mtrace(3)** says `mtrace` installs hooks for `malloc`, `realloc`, `memalign`, `free`; the record
  also shows `calloc` and `posix_memalign` logged as `+`.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* `mallopt` accepting (1) anything it does not reject explicitly; `M_MXFAST` the only checked
  parameter.
* `mallinfo` wrapping to negative values instead of saturating.
* `malloc_info` returning an error number instead of -1.
* `malloc_trim` returning 1 without any visible release.
* `malloc_stats` going through the `stderr` stream (pending text first, on the same line).
* Unversioned weak references binding to the non-default `GLIBC_2.2.5` hook symbols.
* With the debug library: `malloc_set_state(NULL)` crashing; `mprobe(NULL)` being `MCHECK_OK`;
  a double free reported as a head corruption and then crashing; `mtrace` leaving `errno`
  `ENOENT` after failing to open the file.

## What the record does not settle

* The numbers themselves: `malloc_stats`, `malloc_info` and `mallinfo` values are glibc's
  bookkeeping (D-3.4 makes Liella's neutral); only the line and element structure, the field
  relations and the return values are recorded. The number and kind of `<size>`/`<unsorted>`
  lines in `malloc_info` depend on the free chunks present.
* Which allocations are served by `mmap` (the 64 KiB block with a 32 KiB threshold, the 8192-byte
  block with a 4096 threshold were not), and the dynamic threshold adjustment mallopt(3) describes:
  the record shows outcomes for these sequences only.
* `M_ARENA_TEST`, `glibc.malloc.arena_test`, `tcache_max`, `hugetlb`, `mxfast`: no observable probe;
  `M_ARENA_MAX` only through `MALLOC_ARENA_MAX=1`.
* The free-side effect of `M_PERTURB` (freed bytes set to the value): reading freed memory is
  undefined, so it is not probed.
* Secure-execution (set-user-ID) handling of `MALLOC_*_`, `MALLOC_TRACE` and `/etc/suid-debug`.
* Concurrency: `mallinfo`/`malloc_stats`/`malloc_trim` while other threads allocate.
* With the debug library: whether `free` of a clobbered block actually releases it when `abortfn`
  returns (the record shows only that it returns), and how often `mcheck_pedantic` reports a
  corruption that persists over several calls (the probe restores the byte after one call). The
  reference-only part is not compared with Liella until phase 13.
* `mallinfo` saturation for the count fields (`hblks`, `ordblks`): not reachable here.
