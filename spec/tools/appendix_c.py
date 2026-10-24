#!/usr/bin/env python3
"""Render spec Appendix C (ABI data) from spec/abi/*.tsv and splice it into the spec.

    python3 spec/tools/appendix_c.py          # rewrite the appendix in glibc-functional-spec.md

The appendix text between the markers `<!-- appendix-c:begin -->` and `<!-- appendix-c:end -->`
is generated; edit this script, not the spec, to change it.
"""
import csv, collections, os, re

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
ABI = os.path.join(ROOT, "spec", "abi")
SPEC = os.path.join(ROOT, "glibc-functional-spec.md")
BEGIN, END = "<!-- appendix-c:begin -->", "<!-- appendix-c:end -->"

def tsv(name):
    with open(os.path.join(ABI, name), newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))

def vkey(v):
    return [int(x) for x in re.findall(r"\d+", v)] or [0]

DETAILED = ["FILE", "mbstate_t", "sigset_t", "struct __jmp_buf_tag", "__pthread_unwind_buf_t",
            "struct sigaction", "struct tm", "struct termios", "struct dirent", "struct utmp",
            "struct passwd", "struct group", "struct hostent", "struct addrinfo", "struct lconv",
            "struct __locale_struct", "regex_t", "glob_t", "cookie_io_functions_t",
            "posix_spawnattr_t", "posix_spawn_file_actions_t", "struct option", "struct epoll_event",
            "struct dl_phdr_info", "struct r_debug", "struct link_map"]

def render():
    lay, ini, ver, pro, con = tsv("layouts.tsv"), tsv("initializers.tsv"), tsv("versions.tsv"), tsv("prototypes.tsv"), tsv("constants.tsv")
    prov = open(os.path.join(ABI, "PROVENANCE")).read().strip().splitlines()
    public = [r for r in ver if not r["version"].endswith("PRIVATE")]
    private = {r["symbol"] for r in ver if r["version"].endswith("PRIVATE")}
    by_name = collections.defaultdict(list)
    for r in public:
        by_name[r["symbol"]].append(r)
    multi = {n: v for n, v in by_name.items() if len({x["version"] for x in v}) > 1}
    compat_only = sorted(n for n, v in by_name.items() if all(x["binding"] == "compat" for x in v))
    libs = collections.OrderedDict()
    for r in public:
        libs.setdefault(r["library"], set()).add(r["version"])
    dup = collections.defaultdict(set)
    for r in public:
        dup[r["symbol"]].add(r["library"])
    shared = sorted(n for n, l in dup.items() if len(l) > 1)
    types = [r for r in lay if not r["member"]]
    fields = collections.defaultdict(list)
    for r in lay:
        if r["member"]:
            fields[r["type"]].append(r)

    o = []
    w = o.append
    w("## Appendix C. ABI data\n")
    w("*Normative. Added under open decision OD-1 (owner-approved): the public ABI data that binary")
    w("compatibility needs and that the body of this spec does not record.*\n")
    w("### C.0 Provenance and scope\n")
    w("| | |\n|---|---|")
    w("| Reference system | " + prov[1] + " (x86_64) |")
    w("| Sources | (1) the installed **public headers**, read only through the compiler: a probe declares one object of each type, and layouts come from its debug information; sizes and alignments from `sizeof`/`_Alignof`; initializer bytes from compiled initializers; prototypes from `gcc -aux-info`; numeric macros by evaluation. (2) The **dynamic symbol tables** of the installed libraries (`readelf --dyn-syms`): version nodes, default versus compatibility binding, and the sizes of exported data objects. |")
    w("| Not used | glibc source code. No function bodies, code sizes or dispatch mechanisms are recorded: a function's size and whether it is an IFUNC are implementation details and are excluded. |")
    w("| Excluded | `GLIBC_PRIVATE` symbols (" + str(len(private)) + " names): private interfaces between glibc's own libraries, which a replacement that ships its own `ld.so` and `libc.so.6` does not need to reproduce. |")
    w("| Reproduce | `python3 spec/tools/abi_extract.py && python3 spec/tools/appendix_c.py` on the reference system. |")
    w("")
    w("### C.1 Machine-readable tables (normative)\n")
    w("The TSV files are part of this appendix. The tables below reproduce the parts most often needed; the files are complete.\n")
    w("| File | Contents | Rows |\n|---|---|---|")
    w(f"| `spec/abi/versions.tsv` | library, symbol, version node, binding (`default` = `@@`, `compat` = `@`), type, scope, size (data objects only) | {len(ver)} |")
    w(f"| `spec/abi/layouts.tsv` | per type: size and alignment, then each member's offset, type and bit-field position | {len(lay)} ({len(types)} types) |")
    w(f"| `spec/abi/initializers.tsv` | static initializer bit patterns | {len(ini)} |")
    w(f"| `spec/abi/prototypes.tsv` | C declaration of every function the public headers declare, with its header | {len(pro)} |")
    w(f"| `spec/abi/constants.tsv` | value of every integer-constant macro the public headers define (names not starting with `__`) | {len(con)} |")
    w("")
    w("### C.2 Libraries and version nodes\n")
    w("| Library (soname) | Public version nodes |\n|---|---|")
    for lib, vs in libs.items():
        w(f"| `{lib}` | " + ", ".join(sorted(vs, key=vkey)) + " |")
    w("")
    agree = "the same count as Appendix B" if len(by_name) == 3943 else f"Appendix B lists 3943; the difference is an erratum to resolve"
    w(f"**Totals.** {len(by_name)} distinct public names ({agree}). {len(multi)} names have more than")
    w(f"one version (C.3). {len(shared)} names are exported by more than one library (C.4).")
    w(f"{len(compat_only)} names are **compatibility-only**: exported with `@` bindings only, so")
    w("old binaries can resolve them but new programs cannot link against them.\n")
    w("### C.3 Names with more than one version\n")
    w("Each row lists all public versions; **bold** is the default (`@@`) version, and the others are compatibility")
    w("versions with the older contracts that C0.6 describes. An implementation exports every listed version.\n")
    w("| Name | Library | Versions |\n|---|---|---|")
    for n in sorted(multi):
        rows = multi[n]
        by_lib = collections.defaultdict(list)
        for r in rows:
            by_lib[r["library"]].append(r)
        for lib, rs in sorted(by_lib.items()):
            vs = sorted(rs, key=lambda r: vkey(r["version"]))
            if len(vs) < 2:
                continue
            txt = ", ".join((f"**{r['version']}**" if r["binding"] == "default" else r["version"]) for r in vs)
            w(f"| `{n}` | `{lib}` | {txt} |")
    w("")
    w("### C.4 Names exported by more than one library\n")
    w("These names are exported, with the versions in `versions.tsv`, by every library listed. Most are `libm`")
    w("functions that `libc.so.6` also provides for historical reasons.\n")
    w("| Name | Libraries |\n|---|---|")
    for n in shared:
        w(f"| `{n}` | " + ", ".join(f"`{l}`" for l in sorted(dup[n])) + " |")
    w("")
    w("### C.5 Exported data objects\n")
    w("Data symbols are ABI in two ways: their layout (C.7) and their **size**, which old binaries capture through")
    w("copy relocations. An implementation must export each object with exactly this size per version.\n")
    w("| Name | Library | Version | Binding | Size (bytes) |\n|---|---|---|---|---|")
    for r in sorted((r for r in public if r["type"] != "FUNC"), key=lambda r: (r["symbol"], r["library"], vkey(r["version"]))):
        w(f"| `{r['symbol']}` | `{r['library']}` | {r['version']} | {r['binding']} | {r['size']} |")
    w("")
    w("### C.6 Type sizes and alignments\n")
    w("| Type | Size | Align |\n|---|---|---|")
    for r in types:
        w(f"| `{r['type']}` | {r['offset_or_size']} | {r['align']} |")
    w("")
    w("### C.7 Member layouts of the types most exposed to callers\n")
    w("Offsets in bytes from the start of the object. Nested members are shown with dotted names. The complete")
    w("layouts of all types in C.6 are in `layouts.tsv`.\n")
    for t in DETAILED:
        if t not in fields:
            continue
        size = next(r["offset_or_size"] for r in types if r["type"] == t)
        w(f"**`{t}`** ({size} bytes)\n")
        w("| Offset | Member | Type |\n|---|---|---|")
        for r in fields[t]:
            bits = f" ({r['bitfield']})" if r["bitfield"] else ""
            w(f"| {r['offset_or_size']} | `{r['member']}` | `{r['member_type']}`{bits} |")
        w("")
    w("### C.8 Static initializers\n")
    w("Programs embed these bit patterns in their data. An object holding one of these patterns must behave as")
    w("initialised by the corresponding init function. Bytes are listed in memory order.\n")
    w("| Type | Initializer | Size | All zero | Bytes (hex) |\n|---|---|---|---|---|")
    for r in ini:
        w(f"| `{r['type']}` | `{r['initializer']}` | {r['size']} | {r['zero']} | `{r['bytes_hex']}` |")
    w("")
    w(HEADER_PROTOCOLS)
    w("### C.10 Prototypes and constants\n")
    w(f"`prototypes.tsv` gives the declaration of {len(pro)} functions from the public headers. Names that Appendix B")
    w("marks `†` have no public declaration; their calling convention is fixed by their callers (compiler-emitted")
    w("calls such as `__stack_chk_fail`, `__tls_get_addr`, `__cxa_*`, or header macros) and is recorded in C.9.")
    w(f"`constants.tsv` gives {len(con)} macro values. Constants that are kernel ABI (open flags, error numbers, signal")
    w("numbers, ioctl codes) come from the kernel UAPI; they appear here as the headers expose them, and must agree.\n")
    w("### C.11 Remaining gaps\n")
    w("* **libm accuracy bounds.** The headers do not record ULP bounds. Accuracy targets are set by the implementation")
    w("  plan's own tests (decision D-10.3).")
    w("* **System file formats** named by this spec but not described (locale archive, iconv modules, nscd wire")
    w("  protocol, message catalogs, `ld.so.cache`): format inspection is approved (owner decision on formats) and is")
    w("  performed in the phase that needs each format. Findings are added to this spec as Appendix D.")
    return "\n".join(o) + "\n"

HEADER_PROTOCOLS = """### C.9 Header-inline protocols

Public headers expand some interfaces into inline code or macros that compiled programs contain. A
replacement library must support these protocols exactly, because the code that relies on them is
inside the caller's binary.

| Interface | What compiled callers do | Obligation on the library |
|---|---|---|
| `getc_unlocked`, `fgetc_unlocked`, `getchar_unlocked` (`bits/types/struct_FILE.h`, `bits/stdio.h`) | `fp->_IO_read_ptr >= fp->_IO_read_end ? __uflow(fp) : *(unsigned char *)fp->_IO_read_ptr++` | Keep the read window `[_IO_read_ptr, _IO_read_end)` of every `FILE` valid at the offsets in C.7. `__uflow(fp)` refills and returns the next byte (advancing the window) or `EOF`. |
| `putc_unlocked`, `fputc_unlocked`, `putchar_unlocked` | `fp->_IO_write_ptr >= fp->_IO_write_end ? __overflow(fp, (unsigned char)c) : (unsigned char)(*fp->_IO_write_ptr++ = c)` | Keep the write window valid. A stream that must see every byte (unbuffered, line-buffered, wide, closed) sets `_IO_write_ptr == _IO_write_end` so that every byte reaches `__overflow`. |
| `feof_unlocked`, `ferror_unlocked` | test `fp->_flags & 0x0010` (`_IO_EOF_SEEN`) and `& 0x0020` (`_IO_ERR_SEEN`) | Maintain those two bits in `_flags`. `_IO_USER_LOCK` (`0x8000`) is also public. Other `_flags` bits are unconstrained. |
| `stdin`, `stdout`, `stderr` | read the exported `FILE *` variables | Export them as data (C.5) and read them on every implicit use (they are assignable). |
| `is*` classification (`ctype.h`, `__isctype`) | `(*__ctype_b_loc())[(int)c] & mask`, with masks `_ISupper` … `_ISalnum` = `_ISbit(0..11)`, where on little-endian `_ISbit(b) = b < 8 ? (1 << b) << 8 : (1 << b) >> 8` | `__ctype_b_loc()` returns a per-thread pointer to a pointer to an array of `unsigned short`, indexable from -128 to 255, for the thread's current locale. |
| `tolower`/`toupper` inline forms | `(*__ctype_tolower_loc())[c]`, `(*__ctype_toupper_loc())[c]` (`int32_t` tables, index -128..255) | As above, for the case tables. |
| `errno` | `(*__errno_location())` | Return the calling thread's errno cell, stable for the thread's lifetime. |
| `assert` | `__assert_fail(#expr, __FILE__, __LINE__, __func__)` | Signature per `prototypes.tsv`, no return. |
| `setjmp(env)` | `_setjmp(env)` (no mask save) | `setjmp` is a macro. `_setjmp` and `__sigsetjmp(env, savemask)` are the called entry points, with `jmp_buf` per C.7. |
| `sigsetjmp(env, save)` | `__sigsetjmp(env, save)` | As above. |
| `FD_SET`/`FD_CLR`/`FD_ISSET`/`FD_ZERO` | inline bit operations on `fd_set.fds_bits` (`long` words, bit `d % 64` of word `d / 64`); fortified builds call `__fdelt_chk(d)` | `fd_set` is 128 bytes (1024 bits). `__fdelt_chk` returns `d / 64` or aborts for `d` outside 0..1023. |
| `MB_CUR_MAX` | `__ctype_get_mb_cur_max()` | Per current locale. |
| `SIGRTMIN`/`SIGRTMAX` | `__libc_current_sigrtmin()` / `__libc_current_sigrtmax()` | Runtime values. |
| `PTHREAD_STACK_MIN`, `MINSIGSTKSZ`, `SIGSTKSZ` (with `_GNU_SOURCE` / `_DYNAMIC_STACK_SIZE_SOURCE`) | `sysconf(_SC_THREAD_STACK_MIN)`, `sysconf(_SC_MINSIGSTKSZ)`, `sysconf(_SC_SIGSTKSZ)` | Answer those `sysconf` names. |
| `pthread_cleanup_push`/`pop` (C, non-exception build) | allocate `__pthread_unwind_buf_t` on the caller's stack, call `__sigsetjmp` on its `__cancel_jmp_buf`, then `__pthread_register_cancel(&buf)`; on pop `__pthread_unregister_cancel(&buf)`; on cancellation the library `__pthread_unwind_next(&buf)`s through the chain | Layout of `__pthread_unwind_buf_t` per C.7. Cancellation must `longjmp` into each registered buffer in LIFO order. The `_defer`/`_restore` variants also switch the cancellation type. |
| `CMSG_NXTHDR` | `__cmsg_nxthdr(mhdr, cmsg)` | Signature per `prototypes.tsv`. |
| Fortified calls (`_FORTIFY_SOURCE`) | `__name_chk(args…, destlen)`: the compiler-known destination size is appended (for `read`: `__read_chk(fd, buf, nbytes, buflen)`); printf-family `_chk` forms insert a `flag` argument after the stream or buffer argument | Signatures per `prototypes.tsv`. Overflow calls `__chk_fail`. |
| Language-mode redirects | headers bind `scanf`-family calls to `__isoc99_*` (C99+) or `__isoc23_*` (C2X/C23) names, and `strtol`-family calls to `__isoc23_*` in C23 mode, through asm labels | Export all three name sets with their distinct contracts (§12.7, §8.1). |
| Math classification macros | `fpclassify`/`isinf`/`isnan`/`signbit`/`issignaling`/`iseqsig` may compile to builtins or call `__fpclassify*`, `__isinf*`, `__isnan*`, `__signbit*`, `__issignaling*`, `__iseqsig*` | Export those helpers (Appendix B.10). |
"""

def main():
    text = open(SPEC, encoding="utf-8").read()
    block = BEGIN + "\n" + render() + END + "\n"
    if BEGIN in text:
        text = re.sub(re.escape(BEGIN) + ".*?" + re.escape(END) + "\n?", lambda m: block, text, flags=re.S)
    else:
        text = text.rstrip("\n") + "\n\n---\n\n" + block
    if "[Appendix C. ABI data]" not in text:
        text = text.replace("- [Appendix B. Exported symbol index](#appendix-b-exported-symbol-index)\n",
                            "- [Appendix B. Exported symbol index](#appendix-b-exported-symbol-index)\n"
                            "- [Appendix C. ABI data](#appendix-c-abi-data)\n", 1)
    open(SPEC, "w", encoding="utf-8", newline="\n").write(text)
    print("Appendix C written:", block.count("\n"), "lines")

if __name__ == "__main__":
    main()
