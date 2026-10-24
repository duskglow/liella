#!/usr/bin/env python3
"""Regenerate the machine-readable parts of spec Appendix C (ABI data).

Sources, per the owner's decision on OD-1 (interface declarations, not implementation):
  * the installed public headers (glibc 2.39, x86_64), read only through the compiler:
    type layouts (DWARF of a probe that declares one object per type), sizes and alignments,
    static-initializer bytes, function prototypes (gcc -aux-info), numeric macro values;
  * the dynamic symbol tables of the installed libraries (readelf): symbol versions,
    default-vs-compat binding, and object sizes of exported data symbols.

Run on the reference system (Ubuntu 24.04, glibc 2.39):  python3 spec/tools/abi_extract.py
Outputs go to spec/abi/. Nothing here reads glibc source code.
"""
import os, re, subprocess, sys, tempfile, collections

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "abi")
LIBDIR = "/lib/x86_64-linux-gnu"

HEADERS = """stdio.h stdio_ext.h stdlib.h string.h strings.h wchar.h wctype.h ctype.h locale.h
langinfo.h monetary.h iconv.h uchar.h math.h fenv.h complex.h setjmp.h signal.h ucontext.h
pthread.h threads.h semaphore.h time.h sys/time.h sys/times.h sys/timeb.h sys/timex.h
sys/resource.h sys/utsname.h sys/sysinfo.h sys/stat.h sys/statfs.h sys/statvfs.h dirent.h
fts.h ftw.h glob.h regex.h wordexp.h fnmatch.h search.h termios.h sys/ioctl.h pty.h utmp.h
utmpx.h pwd.h grp.h shadow.h gshadow.h netdb.h resolv.h arpa/inet.h arpa/nameser.h
netinet/in.h netinet/ether.h net/if.h ifaddrs.h sys/socket.h sys/un.h poll.h sys/epoll.h
sys/select.h sys/uio.h aio.h mqueue.h sys/ipc.h sys/msg.h sys/sem.h sys/shm.h spawn.h
sys/wait.h dlfcn.h link.h mntent.h fstab.h getopt.h argp.h obstack.h printf.h malloc.h err.h
error.h syslog.h fmtmsg.h sys/mman.h fcntl.h unistd.h sched.h sys/sysmacros.h sys/inotify.h
sys/signalfd.h sys/timerfd.h sys/eventfd.h sys/xattr.h sys/random.h sys/auxv.h execinfo.h
inttypes.h stdint.h stddef.h stdarg.h envz.h argz.h libgen.h nl_types.h libintl.h
thread_db.h sys/ptrace.h sys/prctl.h gnu/libc-version.h sys/single_threaded.h sys/rseq.h
stdbit.h sys/pidfd.h sys/mount.h sys/fanotify.h sys/quota.h sys/swap.h sys/klog.h
sys/reboot.h ttyent.h aliases.h rpc/netdb.h ulimit.h sys/profil.h sys/gmon.h mcheck.h nss.h
sys/platform/x86.h sys/fsuid.h sys/personality.h sys/io.h sys/file.h sys/sendfile.h
sys/user.h sys/procfs.h sys/vlimit.h sys/vtimes.h sys/ustat.h""".split()

# (label, C type expression). Labels are the names used in Appendix C.
TYPES = [
 ("FILE", "FILE"), ("fpos_t", "fpos_t"), ("cookie_io_functions_t", "cookie_io_functions_t"),
 ("va_list", "va_list"), ("max_align_t", "max_align_t"),
 ("div_t", "div_t"), ("ldiv_t", "ldiv_t"), ("lldiv_t", "lldiv_t"), ("imaxdiv_t", "imaxdiv_t"),
 ("struct drand48_data", "struct drand48_data"), ("struct random_data", "struct random_data"),
 ("mbstate_t", "mbstate_t"), ("struct __locale_struct", "struct __locale_struct"),
 ("struct lconv", "struct lconv"),
 ("jmp_buf", "jmp_buf"), ("sigjmp_buf", "sigjmp_buf"), ("struct __jmp_buf_tag", "struct __jmp_buf_tag"),
 ("sigset_t", "sigset_t"), ("siginfo_t", "siginfo_t"), ("struct sigaction", "struct sigaction"),
 ("stack_t", "stack_t"), ("struct sigstack", "struct sigstack"), ("ucontext_t", "ucontext_t"),
 ("mcontext_t", "mcontext_t"), ("union sigval", "union sigval"), ("struct sigevent", "struct sigevent"),
 ("pthread_t", "pthread_t"), ("pthread_attr_t", "pthread_attr_t"), ("pthread_mutex_t", "pthread_mutex_t"),
 ("pthread_mutexattr_t", "pthread_mutexattr_t"), ("pthread_cond_t", "pthread_cond_t"),
 ("pthread_condattr_t", "pthread_condattr_t"), ("pthread_rwlock_t", "pthread_rwlock_t"),
 ("pthread_rwlockattr_t", "pthread_rwlockattr_t"), ("pthread_spinlock_t", "pthread_spinlock_t"),
 ("pthread_barrier_t", "pthread_barrier_t"), ("pthread_barrierattr_t", "pthread_barrierattr_t"),
 ("pthread_key_t", "pthread_key_t"), ("pthread_once_t", "pthread_once_t"),
 ("struct _pthread_cleanup_buffer", "struct _pthread_cleanup_buffer"),
 ("__pthread_unwind_buf_t", "__pthread_unwind_buf_t"), ("cpu_set_t", "cpu_set_t"),
 ("struct sched_param", "struct sched_param"), ("sem_t", "sem_t"),
 ("thrd_t", "thrd_t"), ("mtx_t", "mtx_t"), ("cnd_t", "cnd_t"), ("tss_t", "tss_t"), ("once_flag", "once_flag"),
 ("struct timespec", "struct timespec"), ("struct timeval", "struct timeval"), ("struct tm", "struct tm"),
 ("struct itimerval", "struct itimerval"), ("struct itimerspec", "struct itimerspec"),
 ("struct timezone", "struct timezone"), ("struct timex", "struct timex"),
 ("struct ntptimeval", "struct ntptimeval"), ("struct tms", "struct tms"), ("struct timeb", "struct timeb"),
 ("struct rusage", "struct rusage"), ("struct rlimit", "struct rlimit"), ("struct utsname", "struct utsname"),
 ("struct sysinfo", "struct sysinfo"), ("struct stat", "struct stat"), ("struct statx", "struct statx"),
 ("struct statfs", "struct statfs"), ("struct statvfs", "struct statvfs"),
 ("struct dirent", "struct dirent"), ("struct dirent64", "struct dirent64"),
 ("FTS", "FTS"), ("FTSENT", "FTSENT"), ("struct FTW", "struct FTW"), ("glob_t", "glob_t"),
 ("regex_t", "regex_t"), ("regmatch_t", "regmatch_t"), ("struct re_registers", "struct re_registers"),
 ("wordexp_t", "wordexp_t"), ("ENTRY", "ENTRY"), ("struct hsearch_data", "struct hsearch_data"),
 ("struct qelem", "struct qelem"),
 ("struct termios", "struct termios"), ("struct winsize", "struct winsize"),
 ("struct utmp", "struct utmp"), ("struct utmpx", "struct utmpx"), ("struct lastlog", "struct lastlog"),
 ("struct exit_status", "struct exit_status"),
 ("struct passwd", "struct passwd"), ("struct group", "struct group"), ("struct spwd", "struct spwd"),
 ("struct sgrp", "struct sgrp"), ("struct hostent", "struct hostent"), ("struct netent", "struct netent"),
 ("struct servent", "struct servent"), ("struct protoent", "struct protoent"), ("struct rpcent", "struct rpcent"),
 ("struct addrinfo", "struct addrinfo"), ("struct gaicb", "struct gaicb"), ("struct aliasent", "struct aliasent"),
 ("struct __res_state", "struct __res_state"), ("ns_msg", "ns_msg"), ("ns_rr", "ns_rr"),
 ("struct in_addr", "struct in_addr"), ("struct in6_addr", "struct in6_addr"),
 ("struct sockaddr", "struct sockaddr"), ("struct sockaddr_storage", "struct sockaddr_storage"),
 ("struct sockaddr_in", "struct sockaddr_in"), ("struct sockaddr_in6", "struct sockaddr_in6"),
 ("struct sockaddr_un", "struct sockaddr_un"), ("struct ether_addr", "struct ether_addr"),
 ("struct ifaddrs", "struct ifaddrs"), ("struct if_nameindex", "struct if_nameindex"),
 ("struct ifreq", "struct ifreq"), ("struct ip_mreq", "struct ip_mreq"),
 ("struct group_source_req", "struct group_source_req"), ("struct ip6_mtuinfo", "struct ip6_mtuinfo"),
 ("struct msghdr", "struct msghdr"), ("struct cmsghdr", "struct cmsghdr"), ("struct mmsghdr", "struct mmsghdr"),
 ("struct iovec", "struct iovec"), ("struct linger", "struct linger"), ("struct ucred", "struct ucred"),
 ("struct pollfd", "struct pollfd"), ("struct epoll_event", "struct epoll_event"), ("fd_set", "fd_set"),
 ("struct aiocb", "struct aiocb"), ("struct aioinit", "struct aioinit"),
 ("struct mq_attr", "struct mq_attr"), ("struct ipc_perm", "struct ipc_perm"),
 ("struct msqid_ds", "struct msqid_ds"), ("struct semid_ds", "struct semid_ds"), ("struct sembuf", "struct sembuf"),
 ("struct shmid_ds", "struct shmid_ds"),
 ("posix_spawnattr_t", "posix_spawnattr_t"), ("posix_spawn_file_actions_t", "posix_spawn_file_actions_t"),
 ("Dl_info", "Dl_info"), ("struct dl_phdr_info", "struct dl_phdr_info"), ("struct link_map", "struct link_map"),
 ("struct r_debug", "struct r_debug"), ("struct dl_find_object", "struct dl_find_object"),
 ("Dl_serinfo", "Dl_serinfo"),
 ("struct mntent", "struct mntent"), ("struct fstab", "struct fstab"), ("struct ttyent", "struct ttyent"),
 ("struct option", "struct option"), ("struct argp", "struct argp"), ("struct argp_option", "struct argp_option"),
 ("struct argp_state", "struct argp_state"), ("struct argp_child", "struct argp_child"),
 ("struct obstack", "struct obstack"), ("struct printf_info", "struct printf_info"),
 ("struct mallinfo", "struct mallinfo"), ("struct mallinfo2", "struct mallinfo2"),
 ("fenv_t", "fenv_t"), ("femode_t", "femode_t"), ("fexcept_t", "fexcept_t"),
 ("struct inotify_event", "struct inotify_event"), ("struct signalfd_siginfo", "struct signalfd_siginfo"),
 ("struct rseq", "struct rseq"), ("struct user_regs_struct", "struct user_regs_struct"),
 ("struct user_fpregs_struct", "struct user_fpregs_struct"), ("struct ustat", "struct ustat"),
 ("struct vtimes", "struct vtimes"), ("struct vlimit", "struct vlimit"), ("struct dqblk", "struct dqblk"),
 ("struct cpu_features", "struct cpu_features"),
]

# Static initializers whose bit patterns are ABI (programs embed them).
INITIALIZERS = [
 ("pthread_mutex_t", "PTHREAD_MUTEX_INITIALIZER"),
 ("pthread_mutex_t", "PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP"),
 ("pthread_mutex_t", "PTHREAD_ERRORCHECK_MUTEX_INITIALIZER_NP"),
 ("pthread_mutex_t", "PTHREAD_ADAPTIVE_MUTEX_INITIALIZER_NP"),
 ("pthread_rwlock_t", "PTHREAD_RWLOCK_INITIALIZER"),
 ("pthread_rwlock_t", "PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP"),
 ("pthread_cond_t", "PTHREAD_COND_INITIALIZER"),
 ("pthread_once_t", "PTHREAD_ONCE_INIT"),
 ("once_flag", "ONCE_FLAG_INIT"),
 ("mbstate_t", "{0}"),
]

LIBS = ["libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1", "libutil.so.1",
        "libanl.so.1", "libresolv.so.2", "libnsl.so.1", "libthread_db.so.1", "libBrokenLocale.so.1",
        "libc_malloc_debug.so.0", "ld-linux-x86-64.so.2"]

def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, check=True, **kw).stdout

def prelude():
    return "#define _GNU_SOURCE 1\n#define _DYNAMIC_STACK_SIZE_SOURCE 1\n" + \
           "".join(f"#include <{h}>\n" for h in HEADERS if os.path.exists("/usr/include/" + h) or
                   os.path.exists("/usr/include/x86_64-linux-gnu/" + h))

def available_types(tmp):
    """Keep only types the headers actually declare (probe each one)."""
    ok = []
    for label, expr in TYPES:
        src = prelude() + f"extern char probe[sizeof({expr})];\n"
        p = os.path.join(tmp, "t.c")
        open(p, "w").write(src)
        r = subprocess.run(["gcc", "-std=gnu17", "-fsyntax-only", p], capture_output=True, text=True)
        if r.returncode == 0:
            ok.append((label, expr))
        else:
            print(f"  skip: {label} (not declared by the installed headers)", file=sys.stderr)
    return ok

def sizes(tmp, types):
    src = prelude() + "#include <stdio.h>\nint main(void){\n"
    for label, expr in types:
        src += f'  printf("%s\\t%zu\\t%zu\\n", "{label}", sizeof({expr}), _Alignof({expr}));\n'
    src += "  return 0;}\n"
    p = os.path.join(tmp, "sz.c"); open(p, "w").write(src)
    sh(["gcc", "-std=gnu17", "-O0", p, "-o", os.path.join(tmp, "sz")])
    rows = [l.split("\t") for l in sh([os.path.join(tmp, "sz")]).splitlines()]
    return {r[0]: (int(r[1]), int(r[2])) for r in rows}

# --- DWARF member extraction -------------------------------------------------
def dwarf(tmp, types):
    src = prelude()
    for i, (label, expr) in enumerate(types):
        src += f"{expr} probe_{i};\n"
    p = os.path.join(tmp, "dw.c"); open(p, "w").write(src)
    o = os.path.join(tmp, "dw.o")
    sh(["gcc", "-std=gnu17", "-g", "-gdwarf-4", "-O0", "-fno-eliminate-unused-debug-types", "-c", p, "-o", o])
    text = sh(["readelf", "--debug-dump=info", "-W", o])
    dies = {}      # offset -> dict
    stack = []     # (depth, offset)
    cur = None
    for line in text.splitlines():
        m = re.match(r"\s*<(\d+)><([0-9a-f]+)>: Abbrev Number: (\d+)(?: \((\w+)\))?", line)
        if m:
            depth, off, tag = int(m.group(1)), int(m.group(2), 16), m.group(4)
            if tag is None:
                stack = [s for s in stack if s[0] < depth]
                cur = None
                continue
            cur = {"tag": tag, "children": [], "attrs": {}}
            dies[off] = cur
            stack = [s for s in stack if s[0] < depth]
            if stack:
                dies[stack[-1][1]]["children"].append(off)
            stack.append((depth, off))
            continue
        a = re.match(r"\s*<[0-9a-f]+>\s+(DW_AT_\w+)\s*:\s*(.*)$", line)
        if a and cur is not None:
            name, val = a.group(1), a.group(2).strip()
            # Strip DWARF form annotations: "(strp) (offset: 0x1a): _flags", "(string) int".
            if "(offset:" in val and "):" in val:
                val = val.split("):", 1)[1].strip()
            val = re.sub(r"^\((?:string|strp|line_strp|data\d|sdata|udata|ref\d|ref_udata|flag_present|exprloc)\)\s*", "", val)
            cur["attrs"][name] = val
    return dies

def num(v, default=0):
    for tok in reversed((v or "").split()):
        try:
            return int(tok, 0)
        except ValueError:
            continue
    return default

def ref(v):
    m = re.search(r"<0x([0-9a-f]+)>", v or "")
    return int(m.group(1), 16) if m else None

def tname(dies, off, depth=0):
    if off is None:
        return "void"
    d = dies.get(off)
    if d is None or depth > 12:
        return "?"
    tag, at = d["tag"], d["attrs"]
    nm = at.get("DW_AT_name")
    if tag in ("DW_TAG_base_type", "DW_TAG_typedef"):
        return nm
    if tag in ("DW_TAG_structure_type", "DW_TAG_union_type", "DW_TAG_enumeration_type"):
        kw = {"DW_TAG_structure_type": "struct", "DW_TAG_union_type": "union", "DW_TAG_enumeration_type": "enum"}[tag]
        return f"{kw} {nm}" if nm else f"{kw} <anon>"
    if tag == "DW_TAG_pointer_type":
        return tname(dies, ref(at.get("DW_AT_type")), depth + 1) + " *"
    if tag in ("DW_TAG_const_type", "DW_TAG_volatile_type", "DW_TAG_restrict_type", "DW_TAG_atomic_type"):
        q = tag[7:-5]
        return f"{q} " + tname(dies, ref(at.get("DW_AT_type")), depth + 1)
    if tag == "DW_TAG_array_type":
        dims = ""
        for c in d["children"]:
            ca = dies[c]["attrs"]
            if "DW_AT_upper_bound" in ca:
                dims += f"[{num(ca['DW_AT_upper_bound']) + 1}]"
            elif "DW_AT_count" in ca:
                dims += f"[{num(ca['DW_AT_count'])}]"
            else:
                dims += "[]"
        return tname(dies, ref(at.get("DW_AT_type")), depth + 1) + dims
    if tag == "DW_TAG_subroutine_type":
        return "fn(...)"
    return tag

def resolve(dies, off, depth=0):
    """Follow typedef/cv chains to the underlying struct or union DIE."""
    while off is not None and depth < 20:
        d = dies[off]
        if d["tag"] in ("DW_TAG_structure_type", "DW_TAG_union_type"):
            return off
        if d["tag"] in ("DW_TAG_typedef", "DW_TAG_const_type", "DW_TAG_volatile_type"):
            off = ref(d["attrs"].get("DW_AT_type")); depth += 1; continue
        if d["tag"] == "DW_TAG_array_type":   # jmp_buf, sigjmp_buf are arrays of a struct
            off = ref(d["attrs"].get("DW_AT_type")); depth += 1; continue
        return None
    return None

def members(dies, off, prefix="", base=0, out=None, depth=0):
    out = [] if out is None else out
    d = dies[off]
    for c in d["children"]:
        m = dies[c]
        if m["tag"] != "DW_TAG_member":
            continue
        at = m["attrs"]
        name = at.get("DW_AT_name", "")
        o = num(at.get("DW_AT_data_member_location", "0"))
        t = ref(at.get("DW_AT_type"))
        full = f"{prefix}{name}" if name else prefix.rstrip(".") or "<anon>"
        bits = ""
        if "DW_AT_bit_size" in at:
            bits = f"bits {num(at['DW_AT_bit_size'])}@{num(at.get('DW_AT_data_bit_offset', '0'))}"
        out.append((base + o, full, tname(dies, t), bits))
        inner = resolve(dies, t)
        tag = dies[t]["tag"] if t in dies else ""
        # Descend into anonymous members, and one level into named struct members.
        if inner is not None and depth < 2 and tag != "DW_TAG_pointer_type" and not tname(dies, t).endswith("]"):
            members(dies, inner, f"{full}." if name else prefix, base + o, out, depth + 1)
    return out

def layouts(tmp, types, szmap):
    dies = dwarf(tmp, types)
    varmap = {}
    for off, d in dies.items():
        if d["tag"] == "DW_TAG_variable" and d["attrs"].get("DW_AT_name", "").startswith("probe_"):
            varmap[int(d["attrs"]["DW_AT_name"][6:])] = ref(d["attrs"].get("DW_AT_type"))
    rows = []
    for i, (label, expr) in enumerate(types):
        size, align = szmap[label]
        rows.append((label, "", size, align, "", ""))
        s = resolve(dies, varmap.get(i))
        if s is None:
            continue
        for o, name, ty, bits in members(dies, s):
            rows.append((label, name, o, "", ty, bits))
    return rows

def initializers(tmp):
    src = prelude() + "#include <stdio.h>\n#include <string.h>\nint main(void){\n"
    for ty, init in INITIALIZERS:
        var = re.sub(r"\W", "_", init)
        rhs = init if init.startswith("{") or init.startswith("*") else init
        src += f"  {{ {ty} v = {rhs}; const unsigned char *p = (const unsigned char *)&v; size_t n = sizeof v, z = 1;\n"
        src += f'    for (size_t i = 0; i < n; i++) z &= p[i] == 0;\n'
        src += f'    printf("%s\\t%s\\t%zu\\t%s\\t", "{ty}", "{init}", n, z ? "all-zero" : "nonzero");\n'
        src += '    for (size_t i = 0; i < n; i++) printf("%02x", p[i]); printf("\\n"); }\n'
    src += "  return 0;}\n"
    p = os.path.join(tmp, "init.c"); open(p, "w").write(src)
    sh(["gcc", "-std=gnu17", p, "-o", os.path.join(tmp, "init")])
    return [l.split("\t") for l in sh([os.path.join(tmp, "init")]).splitlines()]

def prototypes(tmp):
    p = os.path.join(tmp, "proto.c"); open(p, "w").write(prelude())
    aux = os.path.join(tmp, "aux.txt")
    subprocess.run(["gcc", "-std=gnu17", "-c", p, "-o", os.path.join(tmp, "proto.o"), "-aux-info", aux], check=True)
    rows = {}
    for line in open(aux, errors="replace"):
        m = re.match(r"/\* (\S+):(\d+):\S+ \*/ (.*);$", line.strip())
        if not m:
            continue
        decl = m.group(3)
        n = re.search(r"(\w+)\s*\((?!\*)", decl)
        if not n:
            continue
        name = n.group(1)
        hdr = m.group(1).replace("/usr/include/x86_64-linux-gnu/", "").replace("/usr/include/", "")
        rows.setdefault(name, (hdr, re.sub(r"\s+", " ", decl)))
    return rows

def symbols():
    out = []
    for lib in LIBS:
        path = os.path.join(LIBDIR, lib)
        if not os.path.exists(path):
            continue
        text = sh(["readelf", "--dyn-syms", "-W", path])
        for line in text.splitlines():
            f = line.split()
            if len(f) < 8 or not f[0].rstrip(":").isdigit():
                continue
            num, value, size, typ, bind, vis, ndx, name = f[:8]
            if ndx == "UND" or typ in ("SECTION", "FILE"):
                continue
            if "@" not in name:
                continue
            base, _, ver = name.partition("@")
            default = ver.startswith("@")
            ver = ver.lstrip("@")
            if base == ver:        # version-definition symbols (GLIBC_2.2.5 ABS)
                continue
            # Interface metadata only: a function's code size and whether it is an IFUNC are
            # implementation details, so functions are reported as FUNC with no size. Object
            # sizes are ABI (old binaries have them baked in via copy relocations).
            is_obj = typ in ("OBJECT", "TLS")
            nbytes = (int(size, 0) if size.startswith("0x") else int(size)) if is_obj else ""
            out.append((lib, base, ver, "default" if default else "compat", typ if is_obj else "FUNC", bind, nbytes))
    return out

def macros(tmp):
    p = os.path.join(tmp, "m.c"); open(p, "w").write(prelude())
    defs = sh(["gcc", "-std=gnu17", "-dM", "-E", p])
    names = sorted({m.group(1) for m in re.finditer(r"^#define (\w+) ", defs, re.M) if not m.group(1).startswith("__")})
    # Evaluate each name that expands to an integer constant expression, or to a pointer with a
    # fixed integer value (SIG_ERR, MAP_FAILED, RTLD_NEXT). The type check rejects floating
    # macros, which would record a truncated value; running each probe twice (below) rejects
    # string and function-name macros, which would record an address. A chunk that does not compile is bisected until
    # the offending names are isolated, so a bad name never drops the others (an earlier
    # version skipped such a chunk whole, losing e.g. the _SC_* enum names; ERRATA E10).
    def evaluate(part, tag):
        src = prelude() + "#include <stdio.h>\n"
        for n in part:
            src += (f"#ifdef {n}\n_Static_assert(__builtin_classify_type(({n}) + 0) == 1 || __builtin_classify_type({n}) == 5, \"integer\");\n"
                    f"static const long long v_{n} = (long long)({n});\n#endif\n")
        src += "int main(void){\n"
        for n in part:
            src += f'#ifdef {n}\n printf("%s\\t%lld\\n", "{n}", v_{n});\n#endif\n'
        src += "return 0;}\n"
        q = os.path.join(tmp, f"mv{tag}.c"); open(q, "w").write(src)
        r = subprocess.run(["gcc", "-std=gnu17", "-w", q, "-o", q[:-2]], capture_output=True, text=True)
        if r.returncode == 0:
            # Run twice: under ASLR an address (a function name such as `basename`) differs
            # between runs, a constant does not.
            first = [l.split("\t") for l in sh([q[:-2]]).splitlines()]
            again = dict(l.split("\t") for l in sh([q[:-2]]).splitlines())
            return [row for row in first if again.get(row[0]) == row[1]]
        if len(part) == 1:
            return []
        half = len(part) // 2
        return evaluate(part[:half], tag + "a") + evaluate(part[half:], tag + "b")
    rows = []
    chunk = 400
    for i in range(0, len(names), chunk):
        rows += evaluate(names[i:i + chunk], str(i))
    return rows

def write(name, header, rows):
    with open(os.path.join(OUT, name), "w", newline="\n") as f:
        f.write("\t".join(header) + "\n")
        for r in rows:
            f.write("\t".join(str(x) for x in r) + "\n")

def main():
    os.makedirs(OUT, exist_ok=True)
    ver = sh(["ldd", "--version"]).splitlines()[0]
    if "--constants-only" in sys.argv:
        # Regenerate constants.tsv alone (the other tables are unchanged by the bisection fix).
        with tempfile.TemporaryDirectory() as tmp:
            mac = macros(tmp)
        write("constants.tsv", ["macro", "value"], mac)
        print(f"{ver}\nconstants: {len(mac)}")
        return
    with tempfile.TemporaryDirectory() as tmp:
        types = available_types(tmp)
        szmap = sizes(tmp, types)
        lay = layouts(tmp, types, szmap)
        write("layouts.tsv", ["type", "member", "offset_or_size", "align", "member_type", "bitfield"], lay)
        ini = initializers(tmp)
        write("initializers.tsv", ["type", "initializer", "size", "zero", "bytes_hex"], ini)
        pro = prototypes(tmp)
        write("prototypes.tsv", ["name", "header", "declaration"], [(k, *v) for k, v in sorted(pro.items())])
        mac = macros(tmp)
        write("constants.tsv", ["macro", "value"], mac)
    syms = symbols()
    write("versions.tsv", ["library", "symbol", "version", "binding", "type", "scope", "size"], syms)
    with open(os.path.join(OUT, "PROVENANCE"), "w", newline="\n") as f:
        f.write(f"Generated by spec/tools/abi_extract.py on the reference system.\n{ver}\n"
                f"Types: {len(types)}  layout rows: {len(lay)}  initializers: {len(ini)}\n"
                f"Prototypes: {len(pro)}  constants: {len(mac)}  versioned symbol rows: {len(syms)}\n")
    print(open(os.path.join(OUT, "PROVENANCE")).read())

if __name__ == "__main__":
    main()
