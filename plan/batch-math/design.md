<!-- Batch math design (BATCH-MATH-DESIGN, ledger story X-7, 2026-10-23). Docs only: no code, no exports. -->
<!-- Sources: the owner direction in plan/06-phases/20-post-release-ledger-and-experiments.md (OD ledger, 2026-10-04;
     GPU optional, 2026-10-22), D-X.4, D-X.10, D-10.1..3, D-10.9, D-10.10, D-10.13, docs/architecture.md §3,
     docs/interfaces.md §3 and §6, crates/math (signatures only), crates/rt/src/cpu.rs (exists), IEEE 754-2019,
     and the textbook error analysis of floating-point summation (Higham, "Accuracy and Stability of Numerical
     Algorithms", ch. 3). No libm or libmvec source was read. -->

# Batch math: a companion crate for math over slices

Status: PROPOSED (RL-7). This note designs the crate the owner direction asks for: "native interface
only, opt-in, never behind standard symbols ... a companion crate, with a complete CPU fallback,
documented determinism differences, a reported crossover size, and no effect on the core's
zero-dependency policy". Nothing here is implemented or exported. The provisional ruling RL-7 says
that the interface is approved only when a named workload asks for it. Until then this note is the
only artefact, and X-8 (CPU) and X-9 (GPU) wait. Local decisions are numbered **B-n**. Questions
for the owner are numbered **BQ-n** and listed in section 9.

## 0. What exists to build on

* `liella-math` (`crates/math`, `no_std`, depends only on `liella-rt`) has one generic scalar
  function per operation, `fn f<F: Small>(x: F, mode: Rounding) -> (F, Status)` (for example
  `exp_log::exp`, `trig::sincos`). The rounding direction is an argument (rule C2). `Status`
  carries the IEEE flags and the error (D-10.2). Errno is set only by the C translation.
* For `float` and `double`, the elementary functions are **correctly rounded in every mode**
  (D-10.13, double-double evaluation), and so are the special functions (D-10.15). Their NaN
  conventions follow glibc (D-10.10, D-10.16). So a correctly rounded result is unique, and the
  scalar function is a bit-exact oracle for any faster path.
* `liella-rt::cpu` holds the CPU feature detection, and `liella-mathtools` holds the
  arbitrary-precision oracle (D-10.12, test code only).
* `liella-native` (D-X.10) is the facade for Rust programs. Only static Rust programs run today;
  `std` and `dlopen` wait for phase 14 (`docs/interfaces.md` §6).

## 1. Shape (B-1, B-2)

| Part | Where | Depends on | Built by |
|---|---|---|---|
| `liella-batch` | `crates/batch`, in the workspace | `liella-math`, `liella-rt` (CPU features only); **no external crate** | the normal workspace build and gate |
| GPU backend `liella-batch-gpu` | `batch-gpu/`, a **separate workspace** excluded from the root one (like `fuzz/`) | `liella-batch` (path), `std`, and the GPU API (BQ-2) | its own job on the GPU host only |

* **B-1.** `liella-batch` is `#![no_std]` with **no `alloc`**. Every element-wise operation
  writes into a slice the caller provides. It contains the interface, the complete scalar
  fallback and the CPU SIMD kernels (X-8). It has no exports and no `#[no_mangle]`, no C header,
  and no symbol version. `libc.so.6`, `libm.so.6` and their `.a` forms never contain it.
* **B-2.** The GPU backend lives outside the product workspace, so neither the product build nor
  the gate, `cargo xtask static` or a default `cargo build` ever compiles it or resolves its
  dependencies. It implements a `Backend` trait defined in `liella-batch`, and a program opts in
  by depending on both crates and passing the backend in. `liella-batch` knows of no GPU.
* `liella-native` re-exports `liella-batch` behind a feature `batch` only after RL-7 approves
  the interface. Until then, a program depends on the crate directly.
* **Not libmvec.** M-1's `_ZGV*` placeholder is the C ABI that compilers call, and its contract
  is the reference's. This crate is a Rust interface with its own contract. M-1 may later reuse
  the X-8 kernels for functions whose contract here is `Exact` (section 3), and for nothing else.

## 2. Interface over slices (B-3 .. B-7)

Sketch (names are proposals; the final names are part of the RL-7 approval):

```rust
pub struct Opts<'a> { pub mode: Rounding, pub backend: Choice<'a>, pub accept: Accept }
pub enum Choice<'a> { Auto, Scalar, Cpu, Backend(&'a dyn Backend) } // Cpu = best SIMD level detected
pub enum Accept { Exact, Ulp(u32) }                                 // the weakest contract the caller accepts
pub struct Report { pub flags: Flags, pub error: Option<(usize, MathError)>, pub used: Used }

pub fn exp<F: Small>(x: &[F], out: &mut [F], o: &Opts) -> Result<Report, BatchError>;
pub fn exp_in_place<F: Small>(x: &mut [F], o: &Opts) -> Result<Report, BatchError>;
pub fn pow<F: Small>(x: &[F], y: &[F], out: &mut [F], o: &Opts) -> Result<Report, BatchError>;
pub fn pow_y<F: Small>(x: &[F], y: F, out: &mut [F], o: &Opts) -> Result<Report, BatchError>;
pub fn sincos<F: Small>(x: &[F], s: &mut [F], c: &mut [F], o: &Opts) -> Result<Report, BatchError>;
```

* **B-3. Formats.** `f32` and `f64` only (`F: Small`). x87 `long double` and `_Float128` have
  no SIMD or GPU gain, so a batch form would be only a loop the caller can write.
* **B-4. Explicit context.** The rounding mode is an argument (C2), default `Nearest`. The
  crate never takes the rounding mode from MXCSR, and it never leaves a flag raised in hardware.
  The evaluation needs round-to-nearest in hardware (D-10.13), so, like the C translation
  (`mathret::elem`), a call switches MXCSR to its default and restores the caller's word
  (flags included) before it returns. The flags come back in `Report`.
* **B-5. Status per batch.** `Report.flags` is the OR of every element's flags, the same value a
  scalar loop would accumulate. `Report.error` is the first element (lowest index) with a domain
  or range error, if any. There is no errno. Per-element status is not kept: a caller who needs
  it runs the scalar functions.
* **B-6. Errors are values.** Mismatched lengths, or a backend that cannot meet `accept` or
  `mode`, return `BatchError`. Nothing panics, and empty slices are valid. Rust's borrow rules
  rule out overlap between inputs and outputs, so in-place work has its own `_in_place` forms.
* **B-7. First function set: none until a workload names one (RL-7).** The candidate set is
  `exp`, `log`, `sin`, `cos`, `sincos`, `pow`, `pow_y`, `sqrt`, `fma`, `hypot` and `atan2`, and
  matrix multiplication `gemm` (row-major with explicit strides, `C = alpha*A*B + beta*C`) in a
  module `linalg`. X-8 implements only the functions the named workload uses.
* **Sizes.** Every element-wise function treats its elements independently, so the result of
  element `i` cannot depend on the slice length, the alignment, the chunking or the thread count.
  Section 3 makes this a contract. The crate starts no threads (it is `no_std`). A caller can
  split a slice across threads and get the same bits.

## 3. The determinism contract (B-8 .. B-11)

Each (function, format, backend) has **one** published contract. It is recorded in a generated
table, `docs/batch-math-contract.tsv`, with columns function, format, backend, contract, bound,
modes, and evidence job.

* **B-8. `Exact`.** The result is bit-identical to the scalar `liella-math` function in the same
  rounding mode, including the NaN sign and payload rules (D-10.10) and the OR of the flags.
  Because the scalar result is correctly rounded (D-10.13, D-10.15), any correctly rounded
  algorithm meets `Exact`. The scalar fallback is `Exact` by construction. The X-8 SIMD kernels
  must be `Exact`: a lane whose rounding test is ambiguous re-runs that element through the
  scalar function (a fast path with an error bound, then a fallback per lane), so hard cases cost
  time, never bits.
* **B-9. `Ulp(b)`.** |result - correctly rounded value| <= b ulp, measured against the
  `mathtools` oracle on the random sample plus the hard-case corpus that phase M uses
  (D-10.3). The special values must still match exactly: +-0, +-inf, NaN in and out, and
  subnormal inputs and outputs. A backend that flushes subnormals to zero cannot offer the
  function at all. `Ulp` results may differ between devices and driver versions. The table
  names the device and driver the bound was measured on, and a driver change re-runs the
  measurement. Round-to-nearest only.
* **B-10. `Auto` never changes the bits.** `Choice::Auto` may move between backends only when
  **every** backend it may pick is `Exact` for that function, format and mode. Otherwise the
  result of one element would depend on the length of the slice it is in (a size threshold
  picks the backend), which would make results non-reproducible in a way callers cannot see.
  For an `Ulp` backend the caller names the backend explicitly and sets `accept: Ulp(b)` with
  `b` at least the table's bound. A request the backend cannot meet is a `BatchError`, never a
  silent downgrade.
* **B-11. `gemm` and reductions.** Floating-point sums depend on order. `Exact` for `gemm` means
  one summation order fixed by the interface: for each output element, `beta*C` first, then the
  `k` products added in increasing `k`, each product rounded and then added (no fused
  multiply-add). This order is slow on every backend. So `gemm` has a second contract,
  `Bound`: the standard forward error bound for an inner product of length `k`,
  |C - C_exact| <= gamma_k |A||B| with gamma_k = k*u/(1 - k*u) (Higham ch. 3), which any order and
  any use of FMA meets. The table states which one each backend gives. Deterministic run to run
  on one backend means a fixed blocking that does not depend on the thread count, and X-8/X-9
  test it.
* Directed rounding modes: `Exact` covers all four modes where the scalar function does. A GPU
  backend is expected to offer round-to-nearest only, so `Auto` falls back to the CPU for the
  other modes (B-10).

## 4. The CPU fallback (B-12, B-13)

* **B-12. Complete.** Every function and format in the interface has a scalar path: a loop
  over `liella-math` in the requested mode, which ORs the `Status`. This is what `Choice::Scalar`
  runs, and it is the reference that every other path is tested against. A build with no SIMD and
  no GPU is fully functional.
* **B-13. SIMD (X-8).** The kernels for x86_64 (SSE2 baseline, plus AVX2 and FMA selected at run
  time through `liella-rt::cpu`) vectorise the double-double evaluation of D-10.13. With FMA, the
  Dekker product becomes one instruction, and the result is still `Exact` because of the final
  rounding test (B-8). Table lookups use gathers or a scalar index loop, whichever measures
  faster. Tails shorter than a vector run the scalar path. aarch64 (ASIMD, RL-1) follows in the A
  track with the same contract.

## 5. How the crossover size is measured (B-14, B-15)

* **B-14. Definition.** For one function, format, backend pair and machine, the crossover is
  the smallest `n` at which the backend's median wall time beats the best CPU path's at that
  `n` **and at every larger `n` measured**. The backend's time includes the copy of the inputs
  to the device, the kernel and the copy back. The one-time setup (driver load, context, kernel
  compile) is reported as its own number and is not part of the crossover. `n` runs over
  powers of two from 2^10 to 2^28 (or up to the device memory), with 11 runs per point after
  2 warm-ups. The CPU side runs one thread and, as a second row, every thread of the host with
  the slice split evenly, so the report shows both crossovers.
* **B-15. Where and how it is reported.** The bench runs on the GPU host under a timing booking
  (`ops timing-book`), in a job `batch-math-gpu-crossover`. The CPU-only part (scalar against
  SIMD, which locates the SIMD path's small-`n` break-even) runs on a timing host in X-8. The
  result is a table in `docs/benchmarks.md` (new section "Batch math") naming the CPU model,
  thread count, GPU model, driver version and the backend's contract. `Auto`'s threshold is the
  measured crossover for that device, set at backend construction from a table compiled into
  the GPU crate, or by an explicit `calibrate()` call. Under B-10 the threshold changes only the
  time, never the result.

## 6. The dependency boundary (B-16 .. B-18)

* **B-16.** The core stays zero-dependency (D-X.4): `liella-batch` has no external crate, and no
  library crate or product depends on `liella-batch`. A check in `cargo xtask static` (X-8) fails
  if any `[dependencies]` of `kernel`, `abi`, `rt`, `mem`, `core`, `legacy`, `math`, `boundary`,
  `ld`, `thread-db`, `libc-so` or `libm-so` names `liella-batch`, or if `crates/batch/Cargo.toml`
  names a crate outside the workspace.
* **B-17.** The GPU workspace is the only place an external crate may appear, and only the one
  the owner approves (BQ-2). It has its own `Cargo.lock` and its own toolchain file, like
  `fuzz/`. The root workspace's `exclude` list names it.
* **B-18. GPU stays optional** (owner 2026-10-22). Nothing in the product, the gate or CI needs a
  GPU, a GPU driver or the GPU crate. The CPU fallback is complete (B-12). The GPU backend needs
  `std` and loading the vendor driver at run time. On Liella that means phase 14 (`dlopen`) and
  the `std` symbols of `docs/interfaces.md` §6. Before then it runs only in programs linked
  against the host C library, which is enough to measure the crossover but not to ship it on
  Liella.

## 7. Tests the implementation stories owe (X-8, X-9)

* Bit-for-bit comparison against the scalar function on the phase-M random sample plus the
  hard-case corpus, in every mode that the contract lists. The tests use lengths 0 to 67 and
  every start offset 0 to 7 inside a buffer (vector tails and alignment), the same input
  through `Scalar`, `Cpu` and each SIMD level forced, and equal `Report.flags`.
* `Ulp` backends: the oracle's maximum error must be at or below the table's bound, and every
  special value must match.
* Plants (X-8): swap two lanes, skip the tail element, run a kernel in the wrong rounding mode,
  drop one lane's flags, and leave the hard-case lanes without the scalar fallback. Each one must be
  caught by a named test.
* `gemm`: the `Exact` order against a naive triple loop, bit for bit. `Bound`: the error against
  an exact (`mathtools`) inner product, within gamma_k |A||B|, and the same bits for 1 and N
  threads.

## 8. What this note does not decide

The final names, the first function set (B-7, which needs a named workload), the GPU API and its
crate (BQ-2), and whether a C ABI for batch math ever exists. A C ABI would be new native
interface (`LIELLA_NATIVE_*`) and needs its own ruling. None is proposed.

## 9. Questions for the owner (each with a recommendation)

* **BQ-1 (RL-7) Shape.** An in-workspace `no_std` crate with no dependencies, plus a GPU backend
  in its own excluded workspace (B-1, B-2, B-16, B-17). Recommend yes. It keeps D-X.4 intact
  without a special case.
* **BQ-2 GPU API and its crate (X-9).** Option (a): one external API crate in `batch-gpu/` only,
  chosen when the GPU host is known (its vendor decides the API). Option (b): no crate at all,
  with the backend loading the vendor's driver library itself and declaring the few C entry
  points it uses from the vendor's public API documentation. Recommend (b) if the API surface the
  kernels need stays small (one buffer type, one kernel launch, copies), else (a). Either way it
  stays outside the product workspace.
* **BQ-3 Default contract.** `accept: Exact` is the default, so a program gets the scalar
  function's bits unless it opts into `Ulp` by name (B-10). Recommend yes: the owner direction
  asks for documented determinism differences, and opt-in keeps them out of the default.
* **BQ-4 Trigger.** Which workload, if any, asks for batch math. None of the current workloads
  was checked for it in this story. Recommend: X-8 starts when one is named, with that
  workload's functions only.
