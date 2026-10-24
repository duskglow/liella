# System calls on aarch64 (ledger A-3, ARCH-SYSCALLS)

`csys/syscalls.tsv` has an optional fifth column, `aarch64`. `cargo xtask csys-gen` reads the
kernel's generic table (UAPI `asm-generic/unistd.h`, which arm64 includes after defining
`__ARCH_WANT_RENAMEAT`, `_NEW_STAT`, `_SET_GET_RLIMIT`, `_TIME32_SYSCALLS`, `_SYS_CLONE3` and
`_MEMFD_SECRET`). It evaluates the header's `#if` conditions for a 64-bit arm64 and writes:

| File | Contents |
|---|---|
| `csys/include/aarch64/liella_sysnr.h` | `LIELLA_NR_*` for aarch64, the absent calls as comments, `LIELLA_CLONE_BACKWARDS` |
| `crates/boundary/src/nr_aarch64.rs` | the same numbers for Rust, `CLONE_BACKWARDS` |
| `crates/boundary/src/ffi_sys_aarch64.rs` | `ffi_sys.rs` without the x86-only wrappers |

None of these is built yet. The aarch64 C layer (ledger A-6) and the cross check (A-2) wire them
in. The x86_64 outputs did not change: `csys-gen --check` passes on the new table, and git shows
no change to any x86_64 file it writes.

**What the generator enforces.** Every call in the table must have a number in the generic table
or be marked absent (`none:`). A call marked absent must have no number there. A replacement
(`none:<call>`) must be a call of the table that has an aarch64 number. When a kernel header tree
is installed (`/usr/src/linux-headers-*/arch/arm64`), the generator also checks the `__ARCH_WANT_*`
list against arm64's `uapi/asm/unistd.h`, and checks `=backwards` on clone against
`select CLONE_BACKWARDS` in `arch/arm64/Kconfig`. Any condition the reader does not understand is
an error.

## The calls aarch64 lacks

| Call | aarch64 | Replacement (A-6 writes it) | Black-box probe of the reference on x86_64 (Broadwell-EP host, 2026-10-23) |
|---|---|---|---|
| `dup2` | `none:dup3` | `dup3(old, new, 0)`. When `old == new`, return `old` if `fcntl(old, F_GETFD)` succeeds, else `-EBADF` | `dup2(1,1)` = 1. `dup2(77,77)` on a closed fd = -1 EBADF. A raw `dup3(1,1,0)` = -1 EINVAL, so the equal-fd case needs the extra check |
| `pause` | `none:ppoll` | `ppoll(NULL, 0, NULL, NULL)` | With an `SA_RESTART` handler, both `pause` and the raw `ppoll` return -1 EINTR (neither restarts) |
| `alarm` | `none:setitimer` | `setitimer(ITIMER_REAL)`. The old value rounds up when `tv_usec >= 500000`, or when `tv_sec == 0` and `tv_usec != 0` | The reference and the replacement agree: 5 s after 300 ms gives 5, 5 s after 700 ms gives 4, 1 s after 950 ms gives 1, idle gives 0 |
| `time` | `none:clock_gettime` | `CLOCK_REALTIME` `tv_sec` (vDSO on aarch64 too) | Equal |
| `getpgrp` | `none:getpgid` | `getpgid(0)` | Equal |
| `arch_prctl`, `ioperm`, `iopl`, `modify_ldt` | `none:-` | No wrapper. On aarch64 the thread pointer is `TPIDR_EL0` (A-4/A-6) | Not probed (x86-only) |
| `ustat` | `none:-` | No wrapper | Not probed. The export follows the reference's aarch64 list (RL-8, read in A-5) |
| `create_module`, `get_kernel_syms`, `query_module`, `uselib` | `none:-` | No wrapper. On x86_64 the LG-5 compat stubs return `ENOSYS` (LEGACY-STUBS) | Not probed on aarch64 (removed calls, absent from asm-generic) |

**clone.** arm64 selects `CONFIG_CLONE_BACKWARDS` (`arch/arm64/Kconfig`). On x86 only `X86_32`
selects it. The aarch64 kernel therefore takes `(flags, newsp, parent_tid, tls, child_tid)`, while
x86_64 takes `(flags, newsp, parent_tid, child_tid, tls)`. The table marks clone `=backwards`, and
the aarch64 outputs define `LIELLA_CLONE_BACKWARDS`. The A-6 wrapper swaps the last two arguments.

**Open for A-5/A-6 (needs an aarch64 toolchain or the reference, RL-2/RL-3).** Whether the
reference exports `ustat`, `ioperm`, `iopl`, `arch_prctl` and `modify_ldt` on aarch64. The C
bodies of the five replacements. The include order that picks `csys/include/aarch64/` before
`csys/include/` (or moving the x86_64 header to `csys/include/x86_64/`). Wiring
`nr_aarch64.rs`/`ffi_sys_aarch64.rs` with `#[path]` under `cfg(target_arch = "aarch64")`.
