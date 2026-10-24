# The toolchain

This document gives every build parameter of Liella's products and of the programs that use
them, so that a third party can build a compatible binary without Liella's build system
(`cargo xtask`). Every command below is the one `cargo xtask` runs, written out; where a
command here was checked to give the same bytes as `cargo xtask`, it says so.

Which way into Liella a program should use, and what each way guarantees, is in
[interfaces.md](interfaces.md). The design is decision D-X.10 of
[implementation-plan.md](../implementation-plan.md).

## Contents

1. [The reference system](#1-the-reference-system)
2. [What depends on the exact compiler](#2-what-depends-on-the-exact-compiler)
3. [The products: libc.a, libc.so.6, liella-crt1.o](#3-the-products-libca-libcso6-liella-crt1o)
4. [Programs of the native C ABI](#4-programs-of-the-native-c-abi)
5. [Static Rust programs](#5-static-rust-programs)
6. [The Rust dylib and its programs](#6-the-rust-dylib-and-its-programs)
7. [Introducing a new Rust ABI version](#7-introducing-a-new-rust-abi-version)
8. [What cannot work yet](#8-what-cannot-work-yet)

## 1. The reference system

The toolchain of record is the one on the test machine. All commands in this document were
run there.

| Item | Value |
|---|---|
| System | Ubuntu 24.04, x86_64, Linux 6.8 |
| Rust (the workspace) | rustup, channel `1.99.1` (pinned exactly in `rust-toolchain.toml`), components `clippy` and `rustfmt` |
| Rust (the dylib lr1) | rustup toolchain `1.99.0`, pinned (section 6): `rustup toolchain install 1.99.0 --profile minimal --target x86_64-unknown-linux-gnu` |
| `rustc -vV` | `rustc 1.99.0 (b940084d7 2026-09-28)`, commit-hash `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, commit-date `2026-09-28`, host `x86_64-unknown-linux-gnu`, LLVM version `23.1.1` |
| `cargo -V` | `cargo 1.99.0 (5f94df478 2026-08-27)` |
| Target | `x86_64-unknown-linux-gnu` only |
| C compiler | `cc` = GCC 13.3.0 (`cc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0`) |
| Linker | GNU ld (BFD) 2.42 (`GNU ld (GNU Binutils for Ubuntu) 2.42`), through `cc` |
| Other tools | `ar`, `nm`, `readelf` from the same binutils |

The workspace's minimum Rust version is 1.85 (`rust-version` in `Cargo.toml`), edition 2024.
`rust-toolchain.toml` names the channel `stable`, not a version, so the workspace's compiler
moves with each stable release. That is harmless for everything except the Rust dylib
(section 2), which is therefore never built by the workspace's toolchain: each Rust ABI
version names a **pinned toolchain**, a rustup toolchain installed beside it (for lr1,
`1.99.0`), and `cargo xtask rust-dylib` runs every rustc command through that one. The two
are independent: updating `stable` changes neither the pinned toolchain nor the dylib.

Liella has no external dependencies: no crate from crates.io, no C library other than its
own. Building needs only the tools above.

## 2. What depends on the exact compiler

| Product | Interface it offers | Bound to the exact rustc? |
|---|---|---|
| `libc.a`, `libc.so.6` | The C ABI (glibc's symbols and versions; the native C ABI) | No. Any rustc that builds the workspace produces a compatible library; only its bytes differ. |
| `liella-crt1.o` | The C start-up convention | No (it is assembly). |
| A static Rust program (`liella-native`, feature `static`) | Nothing: one compiler compiles all of its Rust code in one link | No. |
| A dynamic Rust program compiling `liella-native` in (feature `dynamic`) | It calls only the native C ABI | No. |
| The Rust dylib `libliella_rust.so.<abi>` and its programs | Rust's own ABI, which is not stable | **Yes**: the exact rustc (version and commit), the target, the target features, the panic strategy, and the dylib's other options. This is what the Rust ABI version records. |

The C ABI is defined by the psABI and the version scripts, not by the compiler, so the first
four rows stay compatible across compilers. A Rust dylib exports Rust items with Rust
layouts, Rust calling conventions and Rust symbol names, all of which may change with any
compiler release; and a program compiled against it has already inlined and instantiated
parts of it. Hence the versioning of section 6.

## 3. The products: libc.a, libc.so.6, liella-crt1.o

`cargo xtask so` builds the three products into `target/liella/` (never into
`target/release`, which Cargo puts on `LD_LIBRARY_PATH`).

### 3.1 libc.a

The product crate `crates/libc-so` is a separate workspace (a `no_std` library with the
runtime, the global allocator and the panic handler). From the checkout root:

```sh
cargo rustc -q --release --crate-type staticlib \
    --manifest-path crates/libc-so/Cargo.toml --target-dir target/product
cp target/product/release/libc.a target/liella/libc.a
```

* Rust options: the product crate's `[profile.release]`: `panic = "abort"`, `lto = "fat"`,
  `codegen-units = 1`, and Cargo's release defaults (`opt-level = 3`, no debug information,
  no debug assertions, no overflow checks). Target CPU: the target's baseline (`x86-64`);
  the `glibc-hwcaps` builds of P4 are not built yet. The crate enables
  `liella-boundary/runtime` (which enables `export`).
* The C layer (`csys/`) is compiled by `crates/boundary/build.rs` with `$CC` (default `cc`)
  and archived with `$AR` (default `ar`), each file as
  `cc -std=c17 -O2 -fPIC -ffreestanding -fno-builtin -nostdlib -fno-stack-protector -Wall
  -Wextra -Werror -c -DLIELLA_EXPORT -DLIELLA_RUNTIME -I csys/include <file> -o <object>`,
  every source under `csys/src` except `machine/entry.S`. The archive is linked into
  `libc.a`.
* Cross-language LTO (P1) is not part of this build today.

### 3.2 libc.so.6

The shared library is linked from the archive with a version script that `cargo xtask so`
generates (`target/product/libc.map`):

* For every symbol the archive defines that `spec/abi/versions.tsv` gives a **default**
  version in `libc.so.6`: the symbol in that node. Nodes are ordered numerically
  (`GLIBC_2.2.5`, `GLIBC_2.3`, ...) with `GLIBC_PRIVATE` last; each node names the previous
  one as its parent; the first carries `local: *;`, so everything else is local.
  Compatibility-only versions (`sym@VER` without `@@`) are not produced yet (phase 15).
* Then the native C ABI: every `fn` row of `spec/abi/native.tsv` in its node
  (`LIELLA_NATIVE_1`, then `LIELLA_NATIVE_2`, ...; each names the previous as parent). The
  archive must define each one.

```sh
cc -shared -nostdlib -Wl,--gc-sections -Wl,-z,now -Wl,-z,relro -Wl,--hash-style=gnu \
    -Wl,--version-script=target/product/libc.map -Wl,-soname,libc.so.6 \
    -Wl,--whole-archive target/liella/libc.a -Wl,--no-whole-archive -lgcc \
    -o target/liella/libc.so.6
```

`cargo xtask so` then checks with `readelf --dyn-syms` that every native function is
exported as `name@@LIELLA_NATIVE_<n>` and no other `liella_` name is exported.

### 3.3 liella-crt1.o and the libpthread placeholders

```sh
cc -c csys/src/machine/entry.S -o target/liella/liella-crt1.o
```

The start object must be the first object of every program's link: its `.eh_frame` label
marks the start of the program's call-frame information (D-2.7). Two placeholders let
`-lpthread` and `DT_NEEDED libpthread.so.0` resolve (libpthread is part of libc.so.6 since
glibc 2.34): `libpthread.so.0`, an empty shared object with that soname
(`cc -shared -nostdlib -Wl,-soname,libpthread.so.0`), and `libpthread.a`, an empty archive.

## 4. Programs of the native C ABI

The native C ABI is a set of C functions, `liella_<area>_<name>`, in `libc.a` and in
`libc.so.6` under the version nodes `LIELLA_NATIVE_1`, `LIELLA_NATIVE_2`, ... Any compiler
that follows the x86_64 psABI can call it: C, C++, or another language through its C
interface. Nothing about it depends on Rust.

* **Header:** `include/liella/native.h`, generated by `cargo xtask native-gen` from
  `spec/abi/native.tsv`. It states the conventions (slices as pointer and length, results as
  a value or a negated error number, no `errno`), declares every constant, structure and
  function grouped by node, and asserts every structure's size, alignment and offsets. It
  needs only `<stddef.h>` and `<stdint.h>`.
* **Version:** `LIELLA_NATIVE_VERSION` in the header is the newest node it describes. A
  published node never changes; additions go in a new node.

Static link (the conformance suite's line; `conformance/c/native_v1.c` is such a program):

```sh
cc -std=c17 -O1 -static -nostdlib -fno-builtin -I include \
    target/liella/liella-crt1.o program.c target/liella/libc.a -lgcc -o program
```

Dynamic link:

```sh
cc -std=c17 -O1 -nostdlib -fno-builtin -I include \
    target/liella/liella-crt1.o program.c target/liella/libc.so.6 -lgcc -o program
```

`readelf --dyn-syms -W program` then shows each native function the program calls at its
node, for example `liella_process_id@LIELLA_NATIVE_1`; `readelf -d` shows
`NEEDED libc.so.6`. Such a program cannot run yet (section 8).

## 5. Static Rust programs

A Rust program can use the facade crate `liella-native` with feature `static`: the clean
API compiled into it, the stateful services resolved to `libc.a`. The whole program is
compiled by one compiler and linked once, so there is no Rust ABI between separately
compiled objects and no compiler version to match: any stable rustc that builds the
workspace will do. `cargo xtask native` builds and runs the example
`crates/native/examples/native_static.rs` this way; its module documentation
(`xtask/src/native_examples.rs`) has the same commands.

Two link helpers first:

```sh
# The linker driver: cc with Liella's start object first.
printf '#!/bin/sh\nexec cc %s "$@"\n' "$PWD/target/liella/liella-crt1.o" > liella-cc
chmod +x liella-cc
# The no-unwind helper.
cc -c -O2 -fno-asynchronous-unwind-tables crates/native/link/nounwind.c -o nounwind.o
```

Then the program (here the example; a program of your own depends on `liella-native` with
`features = ["static"]` and is built with the same arguments after `--`):

```sh
cargo rustc -p liella-native --example native_static --features static -- \
    -C linker=$PWD/liella-cc -C linker-features=-lld \
    -C link-arg=-nostartfiles -C link-arg=-nostdlib -C link-arg=$PWD/nounwind.o \
    -C relocation-model=static -C target-feature=+crt-static \
    -C link-arg=$PWD/target/liella/libc.a -C link-arg=-lgcc
```

Why each argument:

* **`-C linker=liella-cc`**: rustc places its link arguments after its own objects, but the
  start object must come first (section 3.3), so a driver puts it there.
* **`-C linker-features=-lld`**: link with GNU ld, not rust-lld (rustc's default linker on
  this target in current releases). rust-lld places the start object's end label, the
  section `.eh_frame.liella_end`, outside the output's `.eh_frame`, so Liella's unwinder
  (`backtrace`) would not find its terminator. Every Liella link uses GNU ld.
* **`-nostartfiles -nostdlib`**: no host start files, no host C library.
* **`nounwind.o`**: Rust's precompiled `core` and `alloc` are built for unwinding: their
  cleanup paths call `_Unwind_Resume`, and their call-frame information names
  `rust_eh_personality`. A `panic = "abort"` program never calls either, but the link needs
  both. `nounwind.c` defines both as weak functions that abort, so a real unwinder (libgcc_eh,
  which uses the C library's `dl_iterate_phdr` and `_dl_find_object`, D-27.4/D-27.5) or `std`
  replaces them.
* **`-C relocation-model=static -C target-feature=+crt-static`**: a static, non-PIE
  executable with no program interpreter and no dynamic section.
* **`libc.a -lgcc`**: the library, then libgcc for the compiler's helpers.

The program must be `panic = "abort"` (the workspace's profiles are), `#![no_std]` and
`#![no_main]`, name its entry point with `liella_native::entry!`, set
`liella_native::heap::Allocator` as its global allocator and define a panic handler (the
examples show all four).

**Feature `dynamic`** (path 2 in Rust form, `examples/native_dynamic.rs`): the same, without
`relocation-model=static` and `+crt-static`, and with `target/liella/libc.so.6` in place of
`libc.a`. The program records `NEEDED libc.so.6` and references the native functions at
`LIELLA_NATIVE_1`; it compiles the clean API in, so it is not bound to a compiler either.

**What blocks `std`.** Not known: no `std` program has been linked against Liella yet. The
C symbols `std` and libgcc_eh need that the library lacked (`getcwd`, `realpath`, `dl_iterate_phdr`,
`_dl_find_object`) are now defined by `libc.a` and `libc.so.6`, and `libc.a`'s
`rust_eh_personality` (`csys/src/adapter/runtime.c`) is weak, so `std`'s replaces it. C++
programs that throw already link statically against `libc.a` with libgcc_eh
(`crates/core/tests/cxx_throw_products.rs`, `conformance/c/s22_once_cxx.c`).

## 6. The Rust dylib and its programs

`libliella_rust.so.<abi>` is `liella-native` with feature `dylib` (which includes `dynamic`),
built as a Rust `dylib`: the facade and the whole clean API (`liella`, `liella-math` and
their dependencies) compiled once, plus `core` and `alloc` from the toolchain. A Rust
program built by the same toolchain links it instead of compiling the crates in. Its
stateful services call `libc.so.6`'s native C ABI, so the dylib holds no state of its own.

The version `<abi>` is a **Rust ABI version**, `lr<N>`: a named, frozen set of build
parameters, defined in [`spec/abi/rust-abi.toml`](../spec/abi/rust-abi.toml). The version
identifier is part of the public interface: it is the file name and soname, an ELF note and
an exported string (section 6.3).

### 6.1 The parameters of lr1

Every field of `[lr1]` and its subtables. "Used" fields are turned into the commands of
section 6.2 by `cargo xtask rust-dylib`; "checked" fields are compared with the running
toolchain or with the build's output; "recorded" fields document the reference build.

| Field | lr1 | Meaning | Use |
|---|---|---|---|
| `identifier` | `lr1` | The version's name: `lr` and a number (a test may add `-<suffix>`). At most 31 bytes of `a-z`, `0-9`, `-`. Equal to the table name. | Used, checked |
| `status` | `current` | `current` (built by default; exactly one), `supported` (still built and shipped), `retired`. Not part of the hash. | Used |
| `native_c_abi` | `LIELLA_NATIVE_1` | The newest native C ABI node the dylib may reference. Its programs need a `libc.so.6` that has this node. | Checked |
| `hash` | `b84ac90a5c7fa1e1` | FNV-1a 64 of the canonical form (section 6.3). | Checked, embedded |
| `toolchain.rustup_toolchain` | `1.99.0` | The pinned toolchain: the rustup channel name it is installed under (`rustup toolchain install 1.99.0 --profile minimal --target x86_64-unknown-linux-gnu`). Every rustc command of the version runs its rustc (`rustup which --toolchain 1.99.0-x86_64-unknown-linux-gnu rustc`). Independent of `rust-toolchain.toml`. | Used |
| `toolchain.rustc_release` | `1.99.0` | `release:` of `rustc -vV`. | Checked |
| `toolchain.rustc_commit_hash` | `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4` | `commit-hash:` of `rustc -vV`. Two compilers with the same release number but different commits are different toolchains. | Checked |
| `toolchain.rustc_commit_date` | `2026-09-28` | `commit-date:` of `rustc -vV`. | Recorded |
| `toolchain.rustc_host` | `x86_64-unknown-linux-gnu` | `host:` of `rustc -vV`. | Checked |
| `toolchain.llvm_version` | `23.1.1` | `LLVM version:` of `rustc -vV`. | Checked |
| `toolchain.sysroot_crates` | `core`, `alloc`, `compiler_builtins` | The toolchain's precompiled crates (rustup component `rust-std` for the target) compiled into the dylib. | Recorded |
| `toolchain.std` | `false` | `std` is not used: every crate is `#![no_std]`. | Recorded |
| `toolchain.linker` | `cc` | `-C linker=cc`: the C compiler driver, running GNU ld. | Used |
| `toolchain.linker_features` | `-lld` | `-C linker-features=-lld`: not rust-lld (section 5). | Used |
| `toolchain.reference_cc`, `reference_ld` | GCC 13.3.0, GNU ld 2.42 | The reference build's C tools. They do not change the Rust ABI; other versions of GNU ld and a GCC-compatible driver give a compatible dylib, possibly not byte-identical. | Recorded |
| `target.triple` | `x86_64-unknown-linux-gnu` | `--target`. | Used, checked |
| `target.target_cpu` | `x86-64` | `-C target-cpu`: the baseline, so the dylib runs on every x86_64 CPU. | Used |
| `target.target_feature_flags` | none | `-C target-feature` arguments. | Used |
| `target.target_features` | `fxsr`, `sse`, `sse2` | The features in effect, as `rustc --print cfg --target x86_64-unknown-linux-gnu -C target-cpu=x86-64` lists them. A toolchain that changes a default changes this list. | Checked |
| `target.relocation_model` | `pic` | `-C relocation-model`. | Used |
| `target.code_model` | `small` | `-C code-model`. | Used |
| `codegen.edition` | `2024` | `--edition` (every crate of the workspace). | Used |
| `codegen.opt_level` | `3` | `-C opt-level`. | Used |
| `codegen.debug_assertions` | `false` | `-C debug-assertions=off`. | Used |
| `codegen.overflow_checks` | `false` | `-C overflow-checks=off`. | Used |
| `codegen.panic` | `abort` | `-C panic=abort`. Nothing unwinds; a panic runs the dylib's panic handler. | Used |
| `codegen.codegen_units` | `1` | `-C codegen-units=1`: a deterministic partitioning. | Used |
| `codegen.lto` | `off` | `-C lto=off`. rustc cannot apply LTO to a dylib. | Used |
| `codegen.debuginfo` | `0` | `-C debuginfo=0`. | Used |
| `codegen.metadata` | `lr1` | `-C metadata` of every crate: the identifier. It enters every Rust symbol name (the crate disambiguator), so two versions in one process never share a symbol of Liella's crates. Checked equal to `identifier`. | Used, checked |
| `codegen.remap_path_prefix` | `/liella` | `--remap-path-prefix=<checkout>=/liella`: the checkout's absolute path, which rustc records, is written as `/liella`, so the dylib is the same wherever it is built. | Used |
| `crates.order` | `liella_kernel`, `liella_rt`, `liella_mem`, `liella`, `liella_math`, `liella_native` | The crates compiled, in order. All but the last are rlibs. | Used |
| `crates.<name>` | for example `liella_rt = ["crates/rt/src/lib.rs", "liella_kernel"]` | The crate root, then the crates given with `--extern`. | Used |
| `crates.liella_native_features` | `dynamic`, `dylib` | `--cfg feature="..."` for the last crate. The other crates have no features. | Used |
| `crates.global_allocator` | `dylib` | The dylib defines the global allocator (`liella_native::heap::Allocator`, over `liella_heap_*`); its programs must not. rustc requires a dylib to have one. | Recorded |
| `crates.panic_handler` | `dylib` | The dylib defines the panic handler (message on standard error, then `exit(101)`); its programs must not. | Recorded |
| `product.file` | `libliella_rust.so.lr1` | The file name. Checked equal to `libliella_rust.so.<identifier>`. | Used, checked |
| `product.soname` | `libliella_rust.so.lr1` | `DT_SONAME`; what programs record as `DT_NEEDED`. | Checked |
| `product.link_name` | `libliella_native.so` | The name rustc's `--extern` accepts for a dylib (`lib<crate>.so`): a symbolic link to `file`, used only to build programs. | Used |
| `product.needed` | `libc.so.6` | The dylib's only `DT_NEEDED`. | Checked |
| `product.link_args` | `-nostartfiles`, `-nostdlib`, `nounwind.o`, `libc.so.6`, `-lgcc`, `-Wl,-soname,libliella_rust.so.lr1` | `-C link-arg` each, after rustc's own (the file names stand for those files' paths). | Used |
| `product.note_section`, `note_owner`, `note_type` | `.note.liella.rustabi`, `Liella`, `0x4c520001` | The ELF note (section 6.3). | Checked |
| `product.symbol` | `LIELLA_RUST_ABI` | The exported string (section 6.3). | Checked |
| `consumer.*` | | What a program of the dylib must do (section 6.4). | Recorded |

### 6.2 Building lr1

`cargo xtask rust-dylib` (or `cargo xtask rust-dylib lr1`) does all of this and the checks
of section 6.5. It runs the pinned toolchain's rustc; if that toolchain is not installed it
fails and names the install command (it never installs it, and never falls back to another
rustc). It refuses unless that rustc's `rustc -vV` matches `toolchain` and its target
features match `target.target_features` exactly, and unless the recorded `hash` matches the
fields. The same build by hand, from the root of a Liella checkout, with the pinned
toolchain installed and Liella's `libc.so.6` built (section 3):

```sh
#!/bin/sh
# Rust ABI version lr1 (spec/abi/rust-abi.toml), by hand. Run from the checkout root.
set -eu
OUT=build/lr1                              # where the dylib and its build-time files go
LIBC=target/liella/libc.so.6               # Liella's libc.so.6
NOUNWIND=build/nounwind.o
mkdir -p "$OUT"
cc -c -O2 -fno-asynchronous-unwind-tables crates/native/link/nounwind.c -o "$NOUNWIND"

lr1() {   # the pinned toolchain's rustc with every option of lr1
    rustc +1.99.0 "$@" --edition 2024 --target x86_64-unknown-linux-gnu \
        -C opt-level=3 -C debug-assertions=off -C overflow-checks=off -C panic=abort \
        -C codegen-units=1 -C lto=off -C debuginfo=0 -C target-cpu=x86-64 \
        -C relocation-model=pic -C code-model=small -C metadata=lr1 \
        --remap-path-prefix="$PWD=/liella" --out-dir "$OUT" -L "dependency=$OUT"
}
lr1 --crate-name liella_kernel crates/kernel/src/lib.rs --crate-type rlib
lr1 --crate-name liella_rt crates/rt/src/lib.rs --crate-type rlib \
    --extern liella_kernel="$OUT/libliella_kernel.rlib"
lr1 --crate-name liella_mem crates/mem/src/lib.rs --crate-type rlib
lr1 --crate-name liella crates/core/src/lib.rs --crate-type rlib \
    --extern liella_kernel="$OUT/libliella_kernel.rlib" \
    --extern liella_mem="$OUT/libliella_mem.rlib" \
    --extern liella_rt="$OUT/libliella_rt.rlib"
lr1 --crate-name liella_math crates/math/src/lib.rs --crate-type rlib \
    --extern liella_rt="$OUT/libliella_rt.rlib"
LIELLA_RUST_ABI=lr1 LIELLA_RUST_ABI_HASH=b84ac90a5c7fa1e1 \
lr1 --crate-name liella_native crates/native/src/lib.rs --crate-type dylib \
    --cfg 'feature="dynamic"' --cfg 'feature="dylib"' \
    --extern liella="$OUT/libliella.rlib" --extern liella_math="$OUT/libliella_math.rlib" \
    -C linker=cc -C linker-features=-lld \
    -C link-arg=-nostartfiles -C link-arg=-nostdlib -C link-arg="$NOUNWIND" \
    -C link-arg="$LIBC" -C link-arg=-lgcc -C link-arg=-Wl,-soname,libliella_rust.so.lr1
mv "$OUT/libliella_native.so" "$OUT/libliella_rust.so.lr1"
ln -s libliella_rust.so.lr1 "$OUT/libliella_native.so"
```

On the reference system this script and `cargo xtask rust-dylib` produce byte-identical
files (the dylib and the five rlibs), and the dylib was also byte-identical when built from
a checkout at another path. The inputs that decide the bytes: the source, the toolchain, the
options above, the C tools, and the `libc.so.6` linked against (only its symbol versions
matter to the result).

* `LIELLA_RUST_ABI` and `LIELLA_RUST_ABI_HASH` are read by `crates/native/src/dylib.rs` at
  compile time; without them the build calls itself `unversioned` (with a zero hash) and is
  not a Rust ABI version. A malformed value stops the compilation.
* The rlibs are build-time files: programs need them to compile (section 6.4), not to run.
* At run time only `libliella_rust.so.lr1` is needed, with `libc.so.6`.

### 6.3 How the version is embedded, and how to read it

The identifier is in three places, all part of the public interface:

1. **The file name and soname**, `libliella_rust.so.lr1`. A program built against the dylib
   records it as `DT_NEEDED`, so the dynamic loader gives it that version and no other.
2. **The ELF note** in section `.note.liella.rustabi` (in a `PT_NOTE` segment), in the
   System V gABI note format: `namesz` = 7, `descsz`, `type` = `0x4c520001`, the name
   `Liella\0` padded to 8 bytes, then the descriptor: the identifier, NUL, the definition's
   hash as 16 lowercase hexadecimal digits, NUL (`descsz` counts these bytes), padded with
   zeros to a multiple of 4. The type is `LR` (0x4c52) in its upper 16 bits and the
   descriptor's format (1) in its lower 16; it is not a small number, so `readelf` does not
   show it as one of the generic note types.
3. **The exported symbol** `LIELLA_RUST_ABI`: an object holding the identifier and a NUL
   (`"lr1\0"`, 4 bytes), for `dlsym(handle, "LIELLA_RUST_ABI")`. Rust programs read it as
   `liella_native::LIELLA_RUST_ABI`; `liella_native::RUST_ABI` is the identifier the program
   was compiled against (a constant compiled into it).

Reading them with `readelf` (the reference build of lr1):

```text
$ readelf -dW libliella_rust.so.lr1 | grep -E 'NEEDED|SONAME'
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000000e (SONAME)             Library soname: [libliella_rust.so.lr1]

$ readelf -nW libliella_rust.so.lr1     # the note (other notes left out)
Displaying notes found in: .note.liella.rustabi
  Owner                Data size 	Description
  Liella               0x00000015	Unknown note type: (0x4c520001)	   description data: 6c 72 31 00 62 38 34 61 63 39 30 61 35 63 37 66 61 31 65 31 00

$ readelf -x .note.liella.rustabi libliella_rust.so.lr1
Hex dump of section '.note.liella.rustabi':
  0x0000025c 07000000 15000000 0100524c 4c69656c ..........RLLiel
  0x0000026c 6c610000 6c723100 62383461 63393061 la..lr1.b84ac90a
  0x0000027c 35633766 61316531 00000000          5c7fa1e1....

$ readelf --dyn-syms -W libliella_rust.so.lr1 | grep LIELLA_RUST_ABI
   528: 000000000008d000     4 OBJECT  GLOBAL DEFAULT   12 LIELLA_RUST_ABI
```

The description data is `lr1\0` followed by `b84ac90a5c7fa1e1\0`. The symbol's contents can
be read with `readelf -x .rodata` (or the section its `Ndx` names) at its address.

For a program, `readelf -d` names the version it needs:

```text
$ readelf -dW native_rust_dylib | grep NEEDED
 0x0000000000000001 (NEEDED)             Shared library: [libliella_rust.so.lr1]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
```

**The hash.** The canonical form of a version is one line per field of its tables except
`hash` and `status`: `<table>.<key> = <value>`, where `<table>` is the full table name
(`lr1.codegen`) and `<value>` the value exactly as written in the file (after removing a
trailing comment and surrounding blanks), each line ended by a newline (0x0a), the lines
sorted bytewise. The hash is FNV-1a 64 over these bytes (offset basis 0xcbf29ce484222325,
prime 0x100000001b3), written as 16 lowercase hexadecimal digits. It identifies the
definition, so two builds that carry the same identifier but differ in a field are told
apart; it is not a security measure. `cargo xtask rust-dylib` recomputes it and refuses a
version whose recorded hash differs, which is how an edit of a defined version is caught.

### 6.4 Programs of the dylib

A program of lr1 must be compiled by **the same toolchain** (`rustc +1.99.0`; rustc refuses
crate metadata from another compiler, error E0514), for **the same target**, with **`-C panic=abort`**
(rustc refuses `panic=unwind` without `std`), must **not define a global allocator or a
panic handler** (the dylib defines both; a second is a compile error), and links
`libc.so.6`. Its other options (`opt-level`, debug assertions, the target CPU at or above
the baseline) are its own choice: they change its own code, not the dylib's interface.

At compile time it needs the dylib under the name `libliella_native.so` and the rlibs of the
same build, in one directory (`$OUT` of section 6.2). The example
`crates/native/examples/native_rust_dylib.rs`, by hand (with `liella-cc` and `nounwind.o`
from section 5):

```sh
rustc +1.99.0 --crate-name native_rust_dylib crates/native/examples/native_rust_dylib.rs \
    --crate-type bin --edition 2024 --target x86_64-unknown-linux-gnu -C panic=abort \
    --extern liella_native="$OUT/libliella_native.so" -L "dependency=$OUT" \
    -C linker=$PWD/liella-cc -C linker-features=-lld \
    -C link-arg=-nostartfiles -C link-arg=-nostdlib -C link-arg=$PWD/nounwind.o \
    -C link-arg=$PWD/target/liella/libc.so.6 -C link-arg=-lgcc -o native_rust_dylib
```

The program records `NEEDED libliella_rust.so.lr1` (the soname, not the file name it was
linked through) and `NEEDED libc.so.6`. Its calls into Liella go through the dylib's Rust
symbols; a native function it references directly (if rustc inlined a wrapper into it) is
versioned at its node. `cargo xtask rust-dylib` builds it with all of lr1's codegen options;
with only the options above (and `-C opt-level=2`) the link was also checked.

### 6.5 What `cargo xtask rust-dylib` checks

On the dylib: the soname; `DT_NEEDED` exactly `libc.so.6`; the note (owner, type,
descriptor); `LIELLA_RUST_ABI` an exported object holding the identifier and a NUL (read from
the file); every native function with a safe use referenced at its node, none newer than
`native_c_abi`. On its program: `DT_NEEDED` exactly the version's file and `libc.so.6`; any
native reference at its node; `LIELLA_RUST_ABI` referenced; no allocator of its own. The
program is not run (section 8).

It then builds a synthetic second version, `lr999-test` (lr1's tables renamed,
`opt_level` changed to `1`, the hash recomputed; written to
`target/liella/rust-test/rust-abi.toml`), with its own program, checks it the same way, and
places both files side by side in `target/liella/rust-test/lib`: the sonames, notes and
strings differ, and the only dynamic symbols both define are `LIELLA_RUST_ABI` and the
toolchain's `core`, `alloc` and allocator shims (which the same toolchain compiles
identically), none of Liella's crates. The variant is built by the same toolchain: it shows
that versions coexist on a system, not that a second toolchain works. It is never
installed.

`cargo xtask test` runs this command too, the same way: it always builds and checks the
current version with its pinned toolchain, whatever `stable` is at the time, and fails if
the pinned toolchain is not installed.

## 7. Introducing a new Rust ABI version

**lr versions are immutable. A released lr version is never rebuilt from changed source. Any
change, however small, requires a new lr version.** (Owner ruling, 2026-10-02.) The rule begins at
Liella's first release, Butterfly Wing, and is not yet binding: until then `lr1` is open and is
rebuilt as the source changes, and it freezes at that release together with `LIELLA_NATIVE_1`
(owner, 2026-10-03). Once released,
an lr version is frozen permanently: its binary is what it is. A new toolchain, a change to any
field of a version, or any change to the Liella source compiled into it makes a new version.

1. Install the new toolchain as a pinned one, by its rustup channel name (for example
   `rustup toolchain install 1.100.0 --profile minimal --target x86_64-unknown-linux-gnu`).
   Keep the old version's toolchain installed.
2. In `spec/abi/rust-abi.toml`, copy the newest version's tables to `[lr<N+1>]`,
   `[lr<N+1>.toolchain]`, ... Change `identifier`, `codegen.metadata`, `product.file`,
   `product.soname` and the `-Wl,-soname,` argument of `product.link_args` to the new
   identifier; set `toolchain.rustup_toolchain` to the channel name, the other `toolchain`
   fields from its `rustc +<channel> -vV` and `target.target_features` from its
   `rustc --print cfg`; make any option change. Set its `status` to `current` and the old
   version's to `supported`.
3. Run `cargo xtask rust-dylib lr<N+1>`. It refuses the placeholder `hash` and prints the
   computed one; record that value. The second run builds `target/liella/rust/lr<N+1>/`.
4. The old version still builds, unchanged, with its own pinned toolchain:
   `cargo xtask rust-dylib lr1`. The gate builds the current version; a supported version
   can be added to it the same way.
5. Ship both runtime files. They have different sonames, notes and Rust symbols, so they
   can be installed in one directory and loaded into one process (by two components built
   against different versions).

**Existing programs keep working**: their `DT_NEEDED` names the version they were built
against, and that file stays installed while the version is `supported`. To move a program
to the new version, rebuild it with the new toolchain against the new version's build
directory; nothing else changes in its source. A version is `retired` (no longer shipped)
only by a decision recorded in the plan, since its programs stop loading.

**A change of Liella's own source.** The Rust ABI version names the toolchain and the
options, and its binary is fixed by the source it was built from. A dylib built from different
Liella source by the same toolchain would keep the symbol names but possibly change layouts and
inlined code, and a program compiled against the older build would not notice at load time.
That is why the rule above has no exception: a Liella release that changes the dylib's contents
in any way ships them as a new version, and the released file is never replaced.

## 8. What cannot work yet

* **Running a dynamically linked program.** Every program linked against `libc.so.6` (the C
  programs of section 4, the `dynamic` example, the dylib's programs) is linked and checked
  but not run: that needs Liella's dynamic loader (phase 14). Their program interpreter is
  the host's, which would load the host's `libc.so.6` and stop at the missing
  `LIELLA_NATIVE_1` node; the host's loader must never load Liella's `libc.so.6`.
* **Rust `std`.** Not tried yet (section 5: the C symbols it needs are defined now; a link
  has not been tested). Every Rust program is `#![no_std]` until then, and the
  dylib's `toolchain.std` is `false`; a version with `std` would be a new version.
* **Unwinding.** Everything is `panic = "abort"`; `nounwind.o` stands in for the unwinder.
* **Several CPU levels.** The dylib is built for the baseline `x86-64` only, like the
  static products; the `glibc-hwcaps` builds (P4) do not exist yet.
* **A second real toolchain** has not been exercised: `lr999-test` uses lr1's.
* **Building on Windows.** The library targets Linux x86_64 only; build inside Linux.
