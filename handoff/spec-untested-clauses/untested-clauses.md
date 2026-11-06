# Spec clauses with no traced test

Produced by `cargo xtask trace --untested --out <this file>` (report only: it never fails the build). A clause counts as tested when a Rust `#[test]` (`crates/*/tests`, `crates/*/src`) names its ID in its own text (name, leading comment block or body), or a program under `conformance/` names it. A test named only after a symbol (`sym_`, `fam_`) credits no clause, so a clause listed here may still be exercised by a test that lacks its tag.

trace --untested: 1269 clauses, 1205 tested (1205 by Rust tests, 0 more by conformance programs), 2 commentary (not testable), 62 untested in 21 of 216 sections.

## Per section

| Section | Clauses | Untested |
|---|---|---|
| 0.6 | 24 | 1 |
| 1.1 | 12 | 0 |
| 1.2 | 16 | 0 |
| 1.3 | 15 | 0 |
| 1.4 | 1 | 0 |
| 1.5 | 4 | 0 |
| 1.6 | 11 | 0 |
| 1.6a | 1 | 0 |
| 1.7 | 7 | 0 |
| 1.8 | 1 | 0 |
| 2.1 | 2 | 0 |
| 2.2 | 10 | 0 |
| 2.3 | 5 | 0 |
| 2.4 | 1 | 0 |
| 2.5 | 3 | 0 |
| 2.6 | 6 | 0 |
| 2.7 | 6 | 0 |
| 3.1 | 26 | 0 |
| 3.2 | 6 | 0 |
| 3.3 | 4 | 0 |
| 3.4 | 2 | 2 |
| 3.5 | 6 | 0 |
| 3.7 | 2 | 0 |
| 4 | 10 | 8 |
| 4.1 | 14 | 0 |
| 4.2 | 3 | 0 |
| 4.3 | 10 | 0 |
| 4.4 | 1 | 0 |
| 4.5 | 7 | 0 |
| 4.6 | 2 | 0 |
| 4.7 | 4 | 0 |
| 4.8 | 1 | 0 |
| 4.9 | 3 | 0 |
| 4.10 | 2 | 0 |
| 4.11 | 5 | 0 |
| 4.13 | 9 | 0 |
| 4.14 | 1 | 0 |
| 5.1 | 4 | 0 |
| 5.2 | 3 | 0 |
| 5.3 | 1 | 0 |
| 6 | 2 | 2 |
| 6.1 | 1 | 0 |
| 6.2 | 18 | 0 |
| 6.3 | 1 | 0 |
| 6.4 | 15 | 0 |
| 6.5 | 5 | 0 |
| 7.1 | 9 | 0 |
| 7.2 | 8 | 0 |
| 7.3 | 5 | 0 |
| 7.4 | 3 | 0 |
| 7.5 | 2 | 0 |
| 7.6 | 1 | 0 |
| 7.7 | 7 | 0 |
| 7.8 | 3 | 0 |
| 8 | 2 | 2 |
| 8.1 | 13 | 0 |
| 8.2 | 9 | 0 |
| 8.3 | 1 | 0 |
| 8.4 | 6 | 0 |
| 8.6 | 2 | 0 |
| 8.7 | 8 | 0 |
| 8.8 | 1 | 0 |
| 8.9 | 1 | 0 |
| 8.10 | 1 | 0 |
| 9.1 | 11 | 0 |
| 9.2 | 2 | 0 |
| 9.3 | 3 | 0 |
| 9.4 | 8 | 0 |
| 10.0 | 14 | 1 |
| 10.1 | 4 | 0 |
| 10.2 | 18 | 0 |
| 10.3 | 8 | 0 |
| 10.4 | 2 | 0 |
| 10.5 | 1 | 0 |
| 10.6 | 9 | 0 |
| 11 | 7 | 7 |
| 11.1 | 14 | 0 |
| 11.2 | 3 | 0 |
| 11.3 | 13 | 0 |
| 11.4 | 5 | 0 |
| 11.5 | 14 | 0 |
| 11.6 | 1 | 0 |
| 11.7 | 13 | 0 |
| 11.8 | 8 | 0 |
| 11.9 | 6 | 0 |
| 11.10 | 21 | 0 |
| 11.11 | 3 | 0 |
| 11.12 | 3 | 0 |
| 11.13 | 3 | 0 |
| 12.1 | 10 | 0 |
| 12.2 | 9 | 0 |
| 12.3 | 3 | 0 |
| 12.4 | 10 | 0 |
| 12.5 | 10 | 0 |
| 12.5a | 3 | 0 |
| 12.6 | 26 | 0 |
| 12.7 | 9 | 0 |
| 12.8 | 7 | 0 |
| 12.9 | 1 | 0 |
| 12.10 | 1 | 0 |
| 12.11 | 5 | 0 |
| 12.12 | 4 | 0 |
| 12.13 | 1 | 0 |
| 13 | 3 | 3 |
| 13.1 | 8 | 0 |
| 13.2 | 10 | 0 |
| 13.3 | 9 | 0 |
| 13.4 | 7 | 0 |
| 13.5 | 1 | 0 |
| 13.6 | 4 | 0 |
| 13.7 | 6 | 0 |
| 13.8 | 4 | 0 |
| 13.9 | 7 | 0 |
| 13.10 | 2 | 0 |
| 13.11 | 5 | 0 |
| 13.12 | 3 | 0 |
| 13.13 | 4 | 0 |
| 13.14 | 1 | 0 |
| 13.15 | 1 | 0 |
| 14.1 | 5 | 0 |
| 14.2 | 1 | 0 |
| 14.3 | 1 | 0 |
| 14.4 | 9 | 0 |
| 14.5 | 5 | 0 |
| 14.6 | 1 | 0 |
| 14.7 | 11 | 0 |
| 14.8 | 9 | 0 |
| 14.9 | 8 | 0 |
| 14.10 | 6 | 0 |
| 15 | 2 | 2 |
| 15.1 | 11 | 0 |
| 15.2 | 7 | 0 |
| 15.3 | 8 | 0 |
| 15.4 | 3 | 0 |
| 15.5 | 1 | 0 |
| 15.6 | 1 | 0 |
| 15.7 | 1 | 0 |
| 15.8 | 4 | 0 |
| 16.1 | 10 | 0 |
| 16.2 | 13 | 0 |
| 16.3 | 2 | 0 |
| 16.4 | 14 | 0 |
| 16.5 | 2 | 0 |
| 16.6 | 1 | 0 |
| 17.1 | 2 | 0 |
| 17.2 | 9 | 0 |
| 17.3 | 9 | 0 |
| 17.4 | 1 | 0 |
| 17.5 | 4 | 0 |
| 17.5a | 1 | 0 |
| 17.6 | 8 | 0 |
| 18.1 | 1 | 0 |
| 18.2 | 9 | 0 |
| 18.3 | 8 | 0 |
| 18.4 | 13 | 0 |
| 18.5 | 5 | 0 |
| 18.6 | 8 | 0 |
| 18.7 | 3 | 0 |
| 18.8 | 5 | 0 |
| 18.9 | 5 | 0 |
| 19.1 | 10 | 0 |
| 19.2 | 2 | 0 |
| 19.3 | 4 | 0 |
| 19.4 | 8 | 0 |
| 19.5 | 1 | 0 |
| 20.1 | 3 | 0 |
| 20.2 | 13 | 0 |
| 20.3 | 5 | 0 |
| 20.4 | 7 | 0 |
| 20.5 | 6 | 0 |
| 20.6 | 1 | 0 |
| 20.7 | 2 | 0 |
| 20.8 | 3 | 0 |
| 20.9 | 1 | 0 |
| 21.1 | 6 | 0 |
| 21.2 | 5 | 0 |
| 22 | 3 | 3 |
| 22.1 | 12 | 0 |
| 22.2 | 6 | 0 |
| 22.3 | 6 | 0 |
| 22.4 | 9 | 0 |
| 22.5 | 7 | 0 |
| 22.6 | 5 | 0 |
| 22.7 | 7 | 0 |
| 22.8 | 5 | 0 |
| 22.9 | 1 | 0 |
| 22.10 | 6 | 6 |
| 23.1 | 17 | 0 |
| 23.2 | 4 | 0 |
| 23.3 | 5 | 0 |
| 23.4 | 9 | 0 |
| 23.5 | 11 | 0 |
| 23.6 | 5 | 0 |
| 23.7 | 7 | 0 |
| 24.1 | 6 | 0 |
| 24.2 | 1 | 0 |
| 24.3 | 5 | 0 |
| 24.4 | 11 | 0 |
| 24.5 | 5 | 0 |
| 24.6 | 6 | 0 |
| 25.1 | 5 | 0 |
| 25.2 | 6 | 0 |
| 25.3 | 7 | 0 |
| 25.4 | 1 | 0 |
| 26.1 | 12 | 0 |
| 27.1 | 14 | 13 |
| 27.2 | 1 | 1 |
| 27.3 | 2 | 2 |
| 27.4 | 1 | 1 |
| 27.5 | 1 | 1 |
| 28.1 | 4 | 2 |
| 28.3 | 1 | 1 |
| 29.1 | 2 | 2 |
| 29.2 | 1 | 1 |
| 29.3 | 1 | 0 |
| 29.4 | 4 | 1 |

## Untested clauses

### 0.6 (1 of 24)

- `s0_6_c0_6_0` (Convention): Every exported name carries a version node (`GLIBC_2.2.5` is the x86_64 baseline). Some names have several versions with different contracts (for example `memcpy@GLIBC_2.2.5` permits overlapping argum ...

### 3.4 (2 of 2)

- `s3_4_c_0` (Contract): Allocate `n` bytes in the caller's stack frame. The storage is released when the function returns.
- `s3_4_f_0` (Failure): There is no failure indication. Exceeding the stack is undefined, and in practice triggers `SIGSEGV` on the guard page.

### 4 (8 of 10)

- `s4_n_1` (Notes): All functions in this section are pure computations. **Kernel: none.**
- `s4_n_2` (Notes): All are MT-Safe and AS-Safe, except where stated (`strtok`, `strerror`, locale-dependent collation).
- `s4_n_3` (Notes): Overlapping source and destination is undefined behaviour unless the entry says otherwise.
- `s4_n_4` (Notes): ⚑ HIST (section-wide). Almost every function here exists in two or more forms whose only difference is *how the extent of an input is delimited*:
- `s4_n_4_1` (Notes): byte count (`mem*`);
- `s4_n_4_2` (Notes): NUL terminator (`str*`);
- `s4_n_4_3` (Notes): NUL terminator *or* a count, whichever comes first (`strn*`);
- `s4_n_4_4` (Notes): a destination capacity (`strl*`, `*_chk`).

### 6 (2 of 2)

- `s6_n_1` (Notes): **Kernel: none** for all conversion functions, except `iconv_open`, which loads conversion modules. That needs `openat`, `mmap` and `dlopen` of `gconv` modules, and reads **Config** `gconv-modules` /  ...
- `s6_n_2` (Notes): The *multibyte encoding* of the locale (`LC_CTYPE` codeset, usually UTF-8) is what these functions convert from and to. On glibc the *wide* representation is always UCS-4 (`__STDC_ISO_10646__` is defi ...

### 8 (2 of 2)

- `s8_n_1` (Notes): **Kernel: none** throughout this section.
- `s8_n_2` (Notes): Locale sensitivity (the radix character, thousands grouping with the GNU `'` flag, and locale digits with `I`) comes from `LC_NUMERIC`.

### 10.0 (1 of 14)

- `s10_0_c2_0` (Contract): `__<name>_finite` (for example `__exp_finite`) were emitted by `-ffinite-math-only` builds before 2.31. They remain as compat symbols with the same contract as `<name>`, restricted to finite arguments ...

### 11 (7 of 7)

- `s11_n_1` (Notes): Most functions in this section are **thin wrappers**. The contract *is* the kernel's, and glibc adds only:
- `s11_n_1_1` (Notes): the errno convention (C0.1);
- `s11_n_1_2` (Notes): thread cancellation (C0.2), for blocking calls;
- `s11_n_1_3` (Notes): 64-bit aliases (C0.4);
- `s11_n_1_4` (Notes): occasionally, emulation for older kernels.
- `s11_n_2` (Notes): Such entries are marked **[thin]**. For them, "Kernel" names the syscall, and the contract is the kernel's documented behaviour (man-pages section 2). Only the caller-visible facts that a reimplementa ...
- `s11_n_3` (Notes): ⚑ KERN (section-wide): descriptor numbering, open-file-description sharing, the offset model, and the semantics of flags are all kernel ABI.

### 13 (3 of 3)

- `s13_n_1` (Notes): Most operations are **[thin]** wrappers over the `*at` system calls. The non-`at` forms pass `AT_FDCWD`.
- `s13_n_2` (Notes): ⚑ KERN. Path resolution, permission checks, symlink following, the `*at` directory-fd model, and inode metadata are all kernel ABI.
- `s13_n_3` (Notes): ⚑ PATH. Every path function exists twice (`f(path)` and `fat(dirfd, path, flags)`), because the directory-relative form was added in 2006 to fix time-of-check to time-of-use races in the path-based fo ...

### 15 (2 of 2)

- `s15_n_1` (Notes): The socket calls are **[thin]**. ⚑ KERN: the address structures (`sockaddr_in`, `sockaddr_in6`, `sockaddr_un`, `sockaddr_ll`, `sockaddr_nl`, ...), the option levels and names, and the message-header l ...
- `s15_n_2` (Notes): ⚑ HIST. The generic `struct sockaddr` (a 2-byte family plus 14 bytes) is too small for most address families. That is why `struct sockaddr_storage` exists, and why every call passes an explicit length ...

### 22 (3 of 3)

- `s22_n_1` (Notes): Since 2.34 all of `libpthread` is in `libc.so.6`. `libpthread.so.0` remains as an empty placeholder (⚑ COMPAT).
- `s22_n_2` (Notes): Pthread functions return **error numbers** and do not set `errno` (C0.1). The exceptions are `sem_*`, which use `errno`.
- `s22_n_3` (Notes): **Kernel (section-wide).** Thread synchronization rests on one primitive, the **futex** (`futex`/`futex_time64`), with operations `FUTEX_WAIT`, `FUTEX_WAKE`, `FUTEX_WAIT_BITSET` (absolute timeouts on  ...

### 22.10 (6 of 6)

- `s22_10_c_0` (Contract): A library that a *debugger* loads to interpret the thread structures of a target process. The debugger supplies memory and register access callbacks (`ps_pdread`, `ps_pdwrite`, `ps_lgetregs`, `ps_pglo ...
- `s22_10_c_1` (Contract): the thread list;
- `s22_10_c_2` (Contract): the LWP ↔ thread mapping;
- `s22_10_c_3` (Contract): TLS addresses;
- `s22_10_c_4` (Contract): TSD values;
- `s22_10_c_5` (Contract): creation and death event breakpoints.

### 27.1 (13 of 14)

- `s27_1_c_1` (Contract): **`dlopen(path, mode)`** loads an ELF shared object and its dependencies into the process and runs their constructors. It returns a handle, and repeated opens are reference-counted.
- `s27_1_c_1_1` (Contract): **Path search** applies when `path` contains no slash: `DT_RPATH`, `LD_LIBRARY_PATH`, `DT_RUNPATH`, `/etc/ld.so.cache`, then the default directories.
- `s27_1_c_1_2` (Contract): **Modes:** `RTLD_LAZY`/`RTLD_NOW` (binding time), `RTLD_GLOBAL`/`RTLD_LOCAL` (symbol visibility to later loads), `RTLD_NODELETE`, `RTLD_NOLOAD` (only return an existing handle), `RTLD_DEEPBIND` (prefe ...
- `s27_1_c_1_3` (Contract): **`dlopen(NULL)`** returns the global scope.
- `s27_1_c_2` (Contract): **`dlmopen(lmid, ...)`** loads into a separate *link-map namespace* (`LM_ID_NEWLM`), giving an isolated copy of the libraries.
- `s27_1_c_3` (Contract): **`dlsym(handle, name)`** returns the address of a symbol. The special handles are `RTLD_DEFAULT` (global lookup order) and `RTLD_NEXT` (the next occurrence after the caller's object, for interpositio ...
- `s27_1_c_4` (Contract): **`dlclose`** decrements the reference count. At zero, it runs destructors and unmaps the object, unless the object is `NODELETE` or has unique symbols or TLS in use.
- `s27_1_c_5` (Contract): **`dlerror()`** returns a Text describing the last failure on this thread, and clears it. It returns `NULL` if there was no error. ⚑ HIST: this is yet another error channel. `dlsym` returning `NULL` i ...
- `s27_1_c_6` (Contract): **`dladdr(addr, &info)`** / **`dladdr1`** map an address to its object and nearest symbol.
- `s27_1_c_7` (Contract): **`dlinfo(handle, request, arg)`** queries a handle: its link map, origin, search paths, TLS module ID and block, and namespace.
- `s27_1_c_8` (Contract): **`dl_iterate_phdr(cb, data)`** calls `cb` for each loaded object, with its load address and program headers. The loader lock is held during the callback.
- `s27_1_g_0` (Guarantees): Symbol resolution follows ELF rules (global scope order, symbol versioning, `STB_GNU_UNIQUE`, IFUNC resolvers run at bind time). Constructors run in dependency order.
- `s27_1_f_0` (Failure): `NULL` plus `dlerror` text: file not found, wrong ELF class, undefined symbol (with `RTLD_NOW`), static TLS exhaustion, or a version mismatch.

### 27.2 (1 of 1)

- `s27_2_c_0` (Contract): Libraries named in `LD_AUDIT` (or `DT_AUDIT`) receive callbacks from the dynamic linker for library search, object load and unload, symbol binding (they can redirect bindings), and PLT entry and exit.

### 27.3 (2 of 2)

- `s27_3_c_0` (Contract): Return the address of a TLS variable, given a module ID and an offset, for the calling thread. The block is allocated on first access for modules loaded by `dlopen`.
- `s27_3_f_0` (Failure): None. Allocation failure aborts the process. ⚑ ARB: there is no error channel for a lazy allocation.

### 27.4 (1 of 1)

- `s27_4_c_0` (Contract): `_r_debug` and `_dl_debug_state` implement the debugger rendezvous. The debugger sets a breakpoint on `_dl_debug_state` and walks the `r_debug.r_map` list after each change. Since 2.35 `r_version` is  ...

### 27.5 (1 of 1)

- `s27_5_c_0` (Contract): Report CPU features as detected by the dynamic linker (CPUID plus OS support via XCR0, masked by `glibc.cpu.hwcaps` tunables): "present" means the CPU supports the feature, and "active" means it is al ...

### 28.1 (2 of 4)

- `s28_1_c_3` (Contract): **`profil(buf, size, offset, scale)`** samples the PC into a histogram on every `ITIMER_PROF` tick. **`sprofil`** takes multiple regions.
- `s28_1_c_4` (Contract): **`__cyg_profile_func_enter/_exit`** are empty hooks called by `-finstrument-functions` code, for the application to interpose.

### 28.3 (1 of 1)

- `s28_3_c_0` (Contract): `__fentry__` is the profiling counter call that `-pg -mfentry` code makes before the function prologue; it records the call arc as `mcount` does (§28.1). The others are covered in their sections.

### 29.1 (2 of 2)

- `s29_1_c_1` (Contract): **XDR** serializes C data to and from the External Data Representation (RFC 4506): encode, decode and free, for primitive types, arrays, strings, unions, pointers, and record-marking streams.
- `s29_1_c_2` (Contract): **The RPC runtime** provides client handles over UDP, TCP and Unix sockets, server dispatch, portmapper registration, and authentication flavours (`AUTH_NONE`, `AUTH_UNIX`/`AUTH_SYS`, `AUTH_DES`).

### 29.2 (1 of 1)

- `s29_2_c_0` (Contract): Client functions for Sun's Network Information Service (`yp_match`, `yp_first`, `yp_next`, `yp_all`, `yp_order`, `yp_master`, `yp_get_default_domain`, `yp_bind`, ...) and for NIS+.

### 29.4 (1 of 4)

- `s29_4_c_0` (Contract): Each keeps the contract it had when it was removed. Most are aliases of current functions, or stubs that fail with `ENOSYS`. Three are special:
