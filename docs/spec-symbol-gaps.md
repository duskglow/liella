# ABI names that no Liella product defines (2026-10-24)

This page is generated from `cargo xtask symcov` (README-COVERAGE-TRUTH, rulings #809/#811). It replaces the
SPEC-SYMBOLS-TRIAGE table, which started from the names listed one by one in `spec/symbols.tsv`: that file names a
family once (by its stem), so the triage missed 105 `libm` names of Track M milestone 5 (SPEC-GAPS-3). `symcov`
walks every row of `spec/abi/versions.tsv` (Appendix C: every library, name and version node of glibc 2.39). The
per-name table is [`spec-symbol-gaps.tsv`](spec-symbol-gaps.tsv) (class, symbol, library, version class, missing
versions, where it is missing, owner, spec section). Regenerate both with
`cargo xtask so && cargo xtask symcov --tsv docs/spec-symbol-gaps.tsv`.

**How it is checked.** A version row is present when the library's `target/liella/*.so*` defines `name@@version`
(default rows; an unversioned definition also counts) or `name@version` (compatibility rows), by
`nm -D --defined-only`. A default name must also be defined by the library's static archive (`nm --defined-only`);
`missing_in` says where it is missing (`so`, `a`, `so+a`). A library Liella ships only as an empty archive
(`libresolv.a`) and the loader's names count as present when any Liella archive defines them, since a static
program takes them from `libc.a`. Version class: `private` (`GLIBC_PRIVATE` only), `compat` (every missing version
is a compatibility version, so new programs cannot link against it), else `default`. Products scanned: job
`readme-coverage-truth-2-symcov` on Broadwell-EP host (`cargo xtask so` then `symcov` on the tree of run 1 merged onto next).

## Counts

4,305 names (4,600 version rows) in the table; 716 are not defined. Counts as of 2026-10-24, the release tree (`cargo xtask so` then `cargo xtask symcov --tsv docs/spec-symbol-gaps.tsv`, Zen 1 host). Earlier counts in the notes below are kept as history: 954 on the morning of 2026-10-24, before the compatibility, RPC and libm landings of that day.

| Class | Names | What it is | Owner in the plan |
|---|---:|---|---|
| (a) dynamic linking / ld.so | 113 | `ld-linux-x86-64.so.2` (3 default, 25 private, 1 placeholder); `libc.so.6` §27 (11: `dlopen` family, `_dl_mcount*`); `libthread_db.so.1` (40 `td_*`); `libc_malloc_debug.so.0` (28, compat); the version-node placeholders of `libpthread`, `libdl`, `librt`, `libutil`, `libanl` (5) | Phase 14 |
| (b) compat-only, in a library Liella ships | 176 | `libc.so.6` 128, `libm.so.6` 37, `libresolv.so.2` 11 (compatibility versions only: new programs cannot link against them) | Phase 15 |
| (c) library Liella does not replace yet | 127 | `libnsl.so.1` (121 compat, 6 private) | Phase 15 (§29.2) |
| (e) legacy tier, default version | 2 | `libc.so.6`: `__libc_sa_len`, `bindresvport` (the 86 other `_IO_*`, profiling and §29 names of 2026-10-22 are defined now) | Phase 15 |
| **milestone 5 (libm)** (m) | **18** | `libm.so.6` default names: the complex `_Float128` members (`cacosf128`, `cacoshf128`, `cargf128`, `casinf128`, ...; the other 87 names of 2026-10-22 are defined now) | Track M milestone 5 (plan/06-phases/04-track-m-parallel-from-p2.md, "Left for milestone 5") |
| (p) GLIBC_PRIVATE | 280 | `libc.so.6` 278, `libresolv.so.2` 2: interfaces between glibc's own libraries, not linkable by programs | Out of scope (ruling #822; see Notes) |
| **Total** | **716** | | |

The (m) row is why the README qualifies "Phases 1 to 13 implemented": those `libm` names (105 on 2026-10-22, 18 on 2026-10-24) are part of the math
library's claimed scope and are not yet implemented. Their `float`, `double` and `long double` (`l`, `f64x`)
real-valued counterparts and the `float`/`double` complex members are implemented.

**Correction (SPEC-GAPS-3, 2026-10-22).** Items 2 and 3 are not isolated gaps. Their base functions
(`lgammaf128`, `clog10l`) are not defined either. They were not in this scan because `spec/symbols.tsv` lists a
math family once, as its stem with a member count, and does not list each member. Comparing every default
`libm.so.6` name in `spec/abi/versions.tsv` with the defined dynamic symbols of a current `libm.so.6` build (Zen 1 host,
role n3) finds 105 missing names: these two plus 103 more. The source agrees: nothing in the tree defines them,
and `cargo xtask math-gen` refuses complex `long double`/`_Float128` signatures ("need forwarders, not generated
yet"). All 105 are what the Track M plan lists under "Left for milestone 5": the `_Float128` elementary and special
functions (34 names) and the complex `long double`/`f64x`/`_Float128` members of the 23 complex families (69
names, plus `__clog10l`). The 103 others:

`acosf128` `acoshf128` `asinf128` `asinhf128` `atan2f128` `atanf128` `atanhf128` `cabsf128` `cabsf64x` `cabsl`
`cacosf128` `cacosf64x` `cacoshf128` `cacoshf64x` `cacoshl` `cacosl` `cargf128` `cargf64x` `cargl` `casinf128`
`casinf64x` `casinhf128` `casinhf64x` `casinhl` `casinl` `catanf128` `catanf64x` `catanhf128` `catanhf64x`
`catanhl` `catanl` `cbrtf128` `ccosf128` `ccosf64x` `ccoshf128` `ccoshf64x` `ccoshl` `ccosl` `cexpf128` `cexpf64x`
`cexpl` `cimagf128` `cimagf64x` `cimagl` `clog10f128` `clog10f64x` `clog10l` `clogf128` `clogf64x` `clogl`
`conjf128` `conjf64x` `conjl` `cosf128` `coshf128` `cpowf128` `cpowf64x` `cpowl` `cprojf128` `cprojf64x` `cprojl`
`crealf128` `crealf64x` `creall` `csinf128` `csinf64x` `csinhf128` `csinhf64x` `csinhl` `csinl` `csqrtf128`
`csqrtf64x` `csqrtl` `ctanf128` `ctanf64x` `ctanhf128` `ctanhf64x` `ctanhl` `ctanl` `erfcf128` `erff128`
`exp10f128` `exp2f128` `expf128` `expm1f128` `j0f128` `j1f128` `jnf128` `lgammaf128` `log10f128` `log1pf128`
`log2f128` `logf128` `powf128` `sincosf128` `sinf128` `sinhf128` `tanf128` `tanhf128` `tgammaf128` `y0f128`
`y1f128` `ynf128`

So "Phases 1 to 13 implemented" does not hold for `libm` until milestone 5 is done, whether or not items 2 and 3
are added on their own. Adding them alone would mean a correctly rounded `_Float128` `lgamma` (the family's target,
D-10.20) and the complex `long double` forwarders, which is milestone-5 work. A default-version `libc.so.6` check
of the same kind finds no further gap outside this page except `GLIBC_PRIVATE` names and
`__x86_get_cpuid_feature_leaf` (X86-CPUID-FEATURE-LEAF-2).

**`__fpu_control` in glibc 2.39, settled with black-box probes** (Zen 1 host, Ubuntu GLIBC 2.39-0ubuntu8.9; a program
reads the control word with `fnstcw` at the start of `main`):

| Program | Static | Dynamic |
|---|---|---|
| reads `__fpu_control` only | value `0x37f`, control word `0x37f` | the same |
| defines its own `__fpu_control = 0x27f` | links; control word stays `0x37f` | control word `0x27f` at `main` |
| a constructor assigns `0x27f` (the constructor sees `0x37f`) | control word `0x37f` at `main` | the same |
| `main` assigns `0x27f`, then starts a thread | the thread's control word is `0x37f` | the same |

So the variable is read only at start-up, before the constructors, and only in a dynamic program. In a static
program it is not read at all, and a program's own definition must still link. In Liella that means a weak or
separate-member definition in `libc.a`, so that it never collides with the program's.

**Implemented (#812, D-10.5).** `unsigned short __fpu_control = 0x037f` is defined in an archive member of its own
(`csys/src/adapter/fpu_control.c`), exported as a 2-byte `OBJECT` at `GLIBC_2.2.5`. The start-up code
(`crates/boundary/src/runtime.rs`, `start()`) reads it through `liella_link_fpu_control` (`csys/src/adapter/runtime.c`)
on the dynamic path only, before the constructors, and loads it into the x87 control word when it differs from
`0x037f`. Tests: `sym___fpu_control` in `crates/boundary/tests/s10_math_symbols.rs` (symbol, size, archive member)
and the conformance programs `fpu_control_default` and `fpu_control_own` (static on Liella; dynamic on glibc as the
oracle). Their dynamic run on Liella waits for Liella's `ld.so` (phase 14, #814) and is a reported skip. So the
dynamic start-up hook has no Liella-side test yet. Class (d) is empty: `lgammaf128_r` and `__clog10l` are
milestone-5 names (row (m) above), left to milestone 5.

## Notes

* `symgap` compares against glibc's installed `libc.a` only; `symcov` covers every library of the ABI table.
* The (p) class is out of scope (ruling #822): `GLIBC_PRIVATE` names are glibc-internal, and programs cannot link
  them. Liella replaces those components together with its own internal interfaces. Phase 14 revisits them only if
  Liella's ld.so must interoperate with glibc libraries.
* The `libresolv` default names are defined in Liella's `libc.a` and its `libresolv.a` is empty; Liella does not
  ship `libresolv.so.2` (dynamic linking is phase 14).
