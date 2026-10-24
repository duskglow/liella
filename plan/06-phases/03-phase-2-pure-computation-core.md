<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 2: Pure computation core

* **Scope:**
  * §4 byte arrays and strings, all internal operations (174 symbols; locale-sensitive
    comparison and case folding use the built-in C/POSIX locale until P7);
  * §5 (part): C-locale classification, and the ctype table ABI for "C";
  * §6 (part): UTF-8/16/32 codecs with explicit state;
  * §8 numeric conversion (212 symbols; `_l` variants take the C locale until P7), integer
    and bit utilities (`<stdbit.h>`, byte order, `abs`/`div`, `ffs`, `l64a`), and the exact
    float engine behind `strtod`/`strfrom*`/`ecvt`;
  * §9 random numbers (34 symbols, including `getrandom`/`getentropy`/`arc4random` on the
    P1 kernel layer);
  * §14.1–14.6: sort, search, trees, hash tables, queues.
* **Symbols:** about 590.
* **Clauses:** about 220.
* **Needs:** P1; IP-11, IP-12, IP-15.
* **Builds:** IP-18 (exact float engine), IP-26 (CPU feature decoding and the hwcaps build
  variants, P4), `boundary::strz`, `abi::chk`, and safe SWAR/auto-vectorised kernels for
  `memchr`/`memcmp`/`strlen`-class operations. OD-C2 is decided here with benchmark data.
* **Internal API shape:** `bytes::{copy, fill, compare, find, find_any, span, split, dup}`
  over `&[u8]`/`&mut [u8]`. `num::{parse_int, parse_float, format_float, bits}` return typed
  outcomes. `random::{Csprng, Rng48, AdditiveFeedback, Lcg31}` are explicit-state values.
  `search::{sort_stable, binary_search, OrderedSet, HashMap}`.
* **Translation specifics:**
  * `str*` go through `strz`.
  * `_chk` forms pass the destination size as the slice bound (D-C0.5).
  * `strtok`, `strncpy` padding, XPG `basename` and the global generators come from
    `liella-legacy`.
  * `qsort` receives its comparator as an `Option<extern "C" fn>` parameter, which is safe
    to call, and the array as one category-*slice* conversion.
* **Test surface:**
  * Exhaustive tests over all byte values and lengths 0–256 at every alignment, for copy,
    compare and search.
  * Overlap tests (D-4.2).
  * `strtol`/`strtod` corpora: boundaries, every base, the C23 `0b` dialect, overflow
    clamping, hexadecimal floats, NaN payloads, and inputs of up to 10^5 digits for
    correct rounding.
  * Exact sequences for the `drand48`/`random`/`rand` algorithms, which are contracts
    (spec §9.3, §9.4).
  * Sort: stability (D-14.1), inconsistent comparators staying in bounds, allocation
    failure.
  * Translation: every symbol's argument mapping and return convention.
* **Exit:** all §4, §8, §9 and §14.1–14.6 clauses and symbols green, and §5/§6 at C-locale
  scope. Bench budgets: `memcpy`/`memset`/`strlen` within 10% of the hardware copy
  bandwidth at 4 KiB.
* **Delivered** (exit met except the three bench budgets, which await OD-C2):
  * **Exports:** 701 of the 709 phase-2 symbols (§4 171/174, §5 115, §6 34/37, §8 212, §9 34,
    §14.1–14.6 19), each a single-expression translation (`cargo xtask purity` clean). The
    26 `long double`/`_Float128` names of §8 are the OD-C4 forwarders in
    `csys/src/adapter/float.c` around `liella_*_bits` functions (version node
    `LIELLA_PRIVATE`). Not exported: `__mempcpy_small`, `__stpcpy_small`, `__strcpy_small`
    (ERRATA E6) and `iconv*` (with the conversion modules).
  * **Crates:** new leaf crate `liella-mem` (D-4.11); `liella-legacy` gains `cstring`,
    `mb_compat`, `prng_r`, `search_c`, `result_slots` and depends on `liella-abi` for the C
    structures it fills in place; `liella-abi` gains `callback` (D-14.6) and the `LdBits`/
    `F128Bits` bit structs.
  * **Tests:** 625 translation tests and 178 clause tests specified for §4–§9 and §14.1–14.6,
    plus exhaustive byte-primitive tests (every length 0–256 at every alignment 0–15, every
    byte value) and 10^5-digit `strtod` inputs. Fortify failures and the OD-6 abort are
    checked in child processes. Full workspace: 1,285 passed, 0 failed. `cargo clippy
    --workspace --all-targets` is clean.
  * **Pending clauses:** 26, each tied to a later phase: §4 Notes 5–6 (design commentary),
    §5.2's UTF-8 guarantee and §6.5's wide/combining widths (phase 7), §6.4 `iconv`, §8.8
    byte order (§15 exports), §9.1's cancellation point (phase 6).
  * **Bench (`cargo xtask bench`, `bench_mem`, fastest of 41 batches, any core):** at 4 KiB,
    against the host `memcpy` as the hardware copy reference: `memcpy` 1.55×, `memset`
    1.55×, `memcmp` 3.1×, `memchr` 6.5×, `strlen` 33×. The budgets for `memcpy`, `memset`
    and `strlen` (1.10×) are not met. `strlen` is the F1 limit itself (safe Rust must read a
    byte at a time); `memcpy`/`memset` are 64-byte block loops in baseline x86-64 (SSE2),
    against glibc's ERMS/AVX2 paths. See OD-C2 (§9.2) for the options.
  * **Deviations from the plan as written:**
    1. **Per-thread state is process-wide until phase 4** (interim, S9): the `strtok`
       cursor (D-4.5), the hidden `mbstate_t`s (D-6.2), the `ecvt`/`fcvt`/`l64a` buffers
       (D-8.5, D-8.8), the ctype table pointers (D-5.3), and the generators (D-9.3). All are
       atomics, so concurrent use is memory-safe (not meaningful, as in C). Phase 4 moves
       them to the thread block.
    2. **`arc4random`** takes every value from `getrandom` (the spec's 2.39 guarantee)
       instead of D-9.2's per-thread ChaCha20 buffer, which needs per-thread pages (phase 6).
    3. **argz/envz** (D-4.8) are byte-vector compositions in `legacy::cstring` rather than
       `TextList`/`TextMap` values through `abi::pack`: the packed form *is* the value
       here, and a second representation bought nothing.
    4. **`memcpy@GLIBC_2.2.5`** is the same function as `memcpy@GLIBC_2.14` (D-4.2); the
       phase-4 version script must export both names.
    5. The interim `errno` path stores through `__errno_location` (D-2.1), as approved.
