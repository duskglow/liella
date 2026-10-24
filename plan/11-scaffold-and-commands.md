<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 11. Scaffold map and commands

```
glib-rewrite/
├── Cargo.toml                 workspace, lint policy (unsafe_code = forbid), profiles; zero deps
├── rust-toolchain.toml        stable, clippy, rustfmt
├── LICENSE (LGPL-2.1), README.md, .gitattributes (LF)
├── scripts/dev.sh             run cargo from WSL/Linux with a Linux-native target dir
├── glibc-functional-spec.md   the contract; Appendix C = ABI data
├── implementation-plan.md     this document
├── spec/
│   ├── clauses.tsv  symbols.tsv  primitives.tsv     generated from the spec
│   ├── schedule.tsv           the owner of each pending test (§4.3)
│   ├── abi/                   Appendix C tables: versions, layouts, initializers, prototypes, constants
│   ├── tools/                 abi_extract.py, appendix_c.py (regenerate Appendix C)
│   └── ERRATA.md
├── csys/                      ARTIFACT 2: the C syscall layer (freestanding C + asm)
│   ├── README.md              classes and rules (K1–K6)
│   ├── include/liella_csys.h
│   └── src/ version.c · sys/*.c (19 areas) · machine/*.S (12 primitives) · adapter/*.c
├── crates/                    ARTIFACT 3: the Rust library
│   ├── kernel/   (liella-kernel)   Sys/Machine traits, Errno, Fd, KPath, auxv, vdso, uapi, testing  [forbid]
│   ├── rt/       (liella-rt)       start tls futex sync heap thread signal context fpu cpu fatal panic [forbid]
│   ├── abi/      (liella-abi)      ctypes layout ret pack callback va chk                              [forbid]
│   │   └── tests/appendix_c_layouts.rs                                           171 pending
│   ├── core/     (liella)          the clean capability API (30 modules)                                [forbid]
│   │   └── tests/sNN_*_clauses.rs                                                1,147 pending
│   ├── legacy/   (liella-legacy)   quarantined historical semantics (26 modules)                        [forbid]
│   ├── math/     (liella-math)     libm                                                                 [forbid]
│   │   └── tests/s10_math_clauses.rs                                             56 pending
│   ├── ld/, thread-db/            ld.so and libthread_db logic                                         [forbid]
│   ├── spectest/                  pending-test macros                                                   [forbid]
│   ├── boundary/ (liella-boundary) THE ONLY UNSAFE: ffi sys conv strz heap export data caller_alloc
│   │   │                          va_source, and posix/ = ARTIFACT 4, the translation layer   [deny + 4 categories]
│   │   ├── build.rs               compiles csys/ into libliella_csys.a
│   │   └── tests/                 s00 conventions (22), sNN symbols (3,112), appendix A primitives (281),
│   │                              csys_link.rs (1 specified: the C layer links and is callable)
│   └── libc-so/                   product crate (separate build from phase 4)                         [forbid]
├── conformance/               C conformance suite (SKIPPED until phase 4)
├── fuzz/                      fuzz harnesses: own workspace, nightly, libfuzzer-sys; seeds/ (§4.5)
└── xtask/                     stubs · status · purity · unsafe-audit · csys-audit · test · ctest · fuzz [forbid]
```

**Commands** (from WSL or Linux):

```bash
scripts/dev.sh xtask test
```

This runs `purity`, then `unsafe-audit`, then `csys-audit`, then `cargo test --workspace`,
then the C conformance suite, then the status report with the schedule report (§4.3; `scripts/dev.sh
xtask status --schedule` lists the pending tests by owner and fails on flagged work).

```bash
scripts/dev.sh xtask stubs
```

This regenerates the pending tests from the spec and Appendix C, and checks the coverage of
files that already contain specified tests.

```bash
python3 spec/tools/abi_extract.py && python3 spec/tools/appendix_c.py
```

This regenerates Appendix C on the reference system.

```bash
scripts/dev.sh clippy --workspace --all-targets -- -D warnings
```

This runs the lint policy.

**Verified state at the end of phase 0.**

* `cargo xtask test`:
  * `purity`: ok (29 files).
  * `unsafe-audit`: ok (232 files; boundary unsafe sites: extern=1).
  * `csys-audit`: ok (35 files).
  * All suites compile and run: 4,789 pending, 1 specified and passing (`csys_link`).
  * Conformance: SKIPPED until phase 4.
* Clippy at `-D warnings`: clean.
* Injected-violation check: an undocumented unsafe site in the boundary, an undocumented
  `#[allow(unsafe_code)]`, and `unsafe` in `liella` are each rejected by the audit. The last
  is also rejected by the compiler.
* No implementation code yet. The only functional code is the phase-0 link proof: one C
  function returning the C layer's ABI version, and its declaration.
