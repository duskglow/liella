#!/usr/bin/env python3
"""BINCOMPAT-SWEEP-STATIC (plan phase 15, LG-0c): the real-binary version-need sweep.

Every regular ELF file (x86_64, 64-bit, dynamic) under /usr/bin, /usr/sbin and /usr/lib* is read
with `readelf -W -l -d -V --dyn-syms` (files only: symlinks are not followed, so every file is read
once). Its version needs on the glibc ABI libraries (the `library` column of spec/abi/versions.tsv)
are compared with
  * the manifest: is each needed `sym@VER` the default or a compat version, or absent, and
  * Liella's built products (target/liella/*.so*): does Liella export it today.

Outputs (OUT directory; every file is sorted and host-independent in form, so two runs on one host
diff clean and two hosts diff only where their installed packages differ):
  summary.md          counts and the top compat needs
  compat-needs.tsv    every needed compat `sym@VER`, ranked by the number of ELFs that need it
  missing-needs.tsv   every needed `sym@VER` (any kind) that Liella does not export yet, ranked
  version-nodes.tsv   every needed (library, version node), with whether Liella defines it
  programs.tsv        every program (an ELF with PT_INTERP): direct / transitive status, first blocker
  liella-exports.tsv  the Liella export set used (reuse elsewhere with --liella-exports)
  run.txt             host, date, glibc, Liella product commit (the only file that differs per run)

Semantics (ELF symbol versioning as documented by the gABI/LSB): a version need names a file and a
version node; the file must define that node unless the need is flagged WEAK. A versioned symbol
reference is satisfied by a definition with the same name and version in the global scope, so a
`pthread_create@GLIBC_2.2.5` need recorded against libpthread.so.0 is met by a libc.so.6 definition.
A WEAK undefined reference that nothing defines resolves to 0 and is counted, not treated as a
blocker. Unversioned references to a glibc library (no version need on it) are not judged.
The program interpreter (ld.so) is not judged: Liella's ld.so is phase-14 work (DL-14).

Usage:
  python3 -I validation/bincompat_sweep.py --out DIR [--liella target/liella | --liella-exports TSV]
         [--manifest spec/abi/versions.tsv] [--root /usr/bin --root ...] [--jobs N]
  python3 -I validation/bincompat_sweep.py --self-test
"""

import argparse
import concurrent.futures
import datetime
import hashlib
import os
import re
import socket
import subprocess
import sys

ROOTS = ["/usr/bin", "/usr/sbin"]
STD_DIRS = ["/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib64", "/lib64",
            "/usr/lib", "/lib", "/usr/local/lib"]

RE_VERNEED_FILE = re.compile(r"^\s*0x?[0-9a-f]+:\s+Version:\s+\d+\s+File:\s+(\S+)\s+Cnt:")
RE_VERNEED_AUX = re.compile(r"^\s*0x[0-9a-f]+:\s+Name:\s+(\S+)\s+Flags:\s+(.*?)\s+Version:\s+(\d+)")
RE_VERDEF = re.compile(r"^\s*0x?[0-9a-f]+:\s+Rev:\s+\d+\s+Flags:\s+(\S+)\s+Index:\s+\d+\s+Cnt:\s+\d+\s+Name:\s+(\S+)")
RE_SYM = re.compile(r"^\s*\d+:\s+[0-9a-f]+\s+\S+\s+(\S+)\s+(\S+)\s+\S+\s+(\S+)\s+(\S+)(?:\s+\((\d+)\))?\s*$")
RE_NEEDED = re.compile(r"\(NEEDED\)\s+Shared library: \[(.+)\]")
RE_RUNPATH = re.compile(r"\((?:RUNPATH|RPATH)\)\s+Library (?:runpath|rpath): \[(.*)\]")
RE_SONAME = re.compile(r"\(SONAME\)\s+Library soname: \[(.+)\]")


def parse_readelf(text):
    """One readelf -W -l -d -V --dyn-syms output -> dict of the facts the sweep uses."""
    info = {"interp": False, "needed": [], "runpath": [], "soname": None, "verneed": {},
            "need_nodes": [], "defs": set(), "verdefs": set(), "und": []}
    cur_file = None
    section = None
    for line in text.splitlines():
        if "Requesting program interpreter" in line:
            info["interp"] = True
            continue
        m = RE_NEEDED.search(line)
        if m:
            info["needed"].append(m.group(1))
            continue
        m = RE_RUNPATH.search(line)
        if m:
            info["runpath"].extend(p for p in m.group(1).split(":") if p)
            continue
        m = RE_SONAME.search(line)
        if m:
            info["soname"] = m.group(1)
            continue
        if line.startswith("Version needs section"):
            section = "verneed"
            continue
        if line.startswith("Version definition section"):
            section = "verdef"
            continue
        if line.startswith("Symbol table"):
            section = "dynsym"
            continue
        if line.startswith("Version symbols section"):
            section = None
            continue
        if section == "verneed":
            m = RE_VERNEED_FILE.match(line)
            if m:
                cur_file = m.group(1)
                continue
            m = RE_VERNEED_AUX.match(line)
            if m and cur_file:
                weak = "WEAK" in m.group(2)
                info["verneed"][int(m.group(3))] = (cur_file, m.group(1))
                info["need_nodes"].append((cur_file, m.group(1), weak))
        elif section == "verdef":
            m = RE_VERDEF.match(line)
            if m and m.group(1) != "BASE":
                info["verdefs"].add(m.group(2))
        elif section == "dynsym":
            m = RE_SYM.match(line)
            if not m:
                continue
            _type, bind, ndx, name, vidx = m.groups()
            if ndx == "UND":
                if "@" not in name:
                    if name:
                        info["und"].append((name, None, bind == "WEAK", None))
                    continue
                sym, ver = name.split("@", 1)
                info["und"].append((sym, ver.lstrip("@"), bind == "WEAK",
                                    int(vidx) if vidx else None))
            elif "@" in name:
                sym, ver = name.split("@", 1)
                info["defs"].add((sym, ver.lstrip("@")))
    return info


def load_manifest(path):
    """versions.tsv -> ({(sym, ver): {(lib, binding)}}, {glibc library names})."""
    rows = {}
    libs = set()
    with open(path, encoding="utf-8") as f:
        header = f.readline().rstrip("\n").split("\t")
        col = {h: i for i, h in enumerate(header)}
        for line in f:
            p = line.rstrip("\n").split("\t")
            lib, sym, ver, binding = p[col["library"]], p[col["symbol"]], p[col["version"]], p[col["binding"]]
            libs.add(lib)
            rows.setdefault((sym, ver), set()).add((lib, binding))
    return rows, libs


def classify(manifest, sym, ver):
    """The manifest kind of one needed sym@VER: default, compat, private or absent."""
    if ver == "GLIBC_PRIVATE":
        return "private"
    hit = manifest.get((sym, ver))
    if hit:
        return "default" if any(b == "default" for _l, b in hit) else "compat"
    return "absent"


def judge(info, glibc_libs, exports):
    """Direct status of one ELF against Liella's exports.

    exports = {"libs": {soname: set(verdef names)}, "syms": {(sym, ver)}}.
    Returns (blockers, weak_missing, needs) where needs is the list of (lib, sym, ver) glibc needs.
    """
    blockers = []
    weak_missing = []
    needs = []
    for lib in info["needed"]:
        if lib in glibc_libs and lib not in exports["libs"]:
            blockers.append("lib:" + lib)
    for lib, node, weak in info["need_nodes"]:
        if lib not in glibc_libs or weak:
            continue
        have = exports["libs"].get(lib)
        if have is not None and node not in have:
            blockers.append("node:%s@%s" % (lib, node))
    for sym, ver, weak, vidx in info["und"]:
        if ver is None or vidx is None:
            continue
        lib = info["verneed"].get(vidx, (None, None))[0]
        if lib not in glibc_libs:
            continue
        needs.append((lib, sym, ver))
        if (sym, ver) not in exports["syms"]:
            if weak:
                weak_missing.append("%s@%s" % (sym, ver))
            else:
                blockers.append("sym:%s@%s" % (sym, ver))
    return blockers, weak_missing, needs


def glibc_component(info, glibc_libs):
    """True for glibc's own files: a glibc ABI soname, or a GLIBC_PRIVATE need on a glibc library.

    Liella replaces these files instead of serving them, so they are counted apart and judged not.
    """
    if info["soname"] in glibc_libs:
        return True
    return any(lib in glibc_libs and node == "GLIBC_PRIVATE" for lib, node, _w in info["need_nodes"])


def search_dirs(origin, runpath):
    return [d.replace("$ORIGIN", origin).replace("${ORIGIN}", origin) for d in runpath]


def resolve(soname, dirs, index):
    """The swept real path a NEEDED soname loads: the object's RUNPATH (with $ORIGIN), then its
    loaders' (a library its program already loaded from there matches by soname), then the
    standard dirs."""
    dirs = list(dirs) + STD_DIRS
    for d in dirs:
        cand = os.path.realpath(os.path.join(d, soname))
        if cand in index:
            return cand
    return None


def transitive(path, results, index, glibc_libs, memo, stack, inherited=()):
    """'ok' if this ELF and every non-glibc library it loads are ok directly, else the first blocker."""
    if path in memo:
        return memo[path]
    if path in stack:
        return "ok"
    r = results[path]
    if r["blockers"]:
        memo[path] = r["blockers"][0]
        return memo[path]
    stack.add(path)
    verdict = "ok"
    info = r["info"]
    dirs = search_dirs(os.path.dirname(path), info["runpath"]) + list(inherited)
    for lib in info["needed"]:
        if lib in glibc_libs:
            continue
        dep = resolve(lib, dirs, index)
        if dep is None:
            verdict = "unresolved:" + lib
            break
        sub = transitive(dep, results, index, glibc_libs, memo, stack, dirs)
        if sub != "ok":
            verdict = "via %s: %s" % (lib, sub)
            break
    stack.discard(path)
    memo[path] = verdict
    return verdict


def elf_kind(path):
    try:
        with open(path, "rb") as f:
            h = f.read(20)
    except OSError:
        return None
    if len(h) < 20 or h[:4] != b"\x7fELF":
        return None
    if h[4] != 2 or h[18:20] != b"\x3e\x00":
        return "foreign"
    return "x86_64"


def walk(roots):
    for root in roots:
        for d, subdirs, files in os.walk(root, followlinks=False):
            subdirs.sort()
            for n in sorted(files):
                p = os.path.join(d, n)
                if os.path.isfile(p) and not os.path.islink(p):
                    yield p


def readelf(path):
    try:
        out = subprocess.run(["readelf", "-W", "-l", "-d", "-V", "--dyn-syms", path],
                             capture_output=True, text=True, timeout=120,
                             env={"LC_ALL": "C", "PATH": "/usr/bin:/bin"}, errors="replace")
        return out.stdout
    except (subprocess.TimeoutExpired, OSError):
        return ""


def liella_exports(product_dir):
    """Liella's built shared objects -> exports dict, from readelf of each product."""
    exp = {"libs": {}, "syms": set()}
    for n in sorted(os.listdir(product_dir)):
        p = os.path.join(product_dir, n)
        if ".so" not in n or os.path.islink(p) or elf_kind(p) != "x86_64":
            continue
        info = parse_readelf(readelf(p))
        exp["libs"][info["soname"] or n] = set(info["verdefs"])
        exp["syms"] |= info["defs"]
    return exp


def write_exports(exp, path):
    with open(path, "w", encoding="utf-8") as f:
        f.write("kind\tlibrary_or_symbol\tversion\n")
        for lib in sorted(exp["libs"]):
            f.write("lib\t%s\t\n" % lib)
            for v in sorted(exp["libs"][lib]):
                f.write("node\t%s\t%s\n" % (lib, v))
        for sym, ver in sorted(exp["syms"]):
            f.write("sym\t%s\t%s\n" % (sym, ver))


def read_exports(path):
    exp = {"libs": {}, "syms": set()}
    with open(path, encoding="utf-8") as f:
        f.readline()
        for line in f:
            kind, a, b = line.rstrip("\n").split("\t")
            if kind == "lib":
                exp["libs"].setdefault(a, set())
            elif kind == "node":
                exp["libs"].setdefault(a, set()).add(b)
            elif kind == "sym":
                exp["syms"].add((a, b))
    return exp


def sweep(args):
    manifest, glibc_libs = load_manifest(args.manifest)
    exp = read_exports(args.liella_exports) if args.liella_exports else liella_exports(args.liella)
    roots = args.root or ROOTS + sorted(
        os.path.join("/usr", n) for n in os.listdir("/usr") if n.startswith("lib"))
    roots = [r for r in roots if os.path.isdir(r) and not os.path.islink(r)]
    counts = {"files": 0, "elf": 0, "foreign": 0, "nodyn": 0, "glibc_components": 0, "dynamic": 0,
              "programs": 0, "unversioned": 0}
    elves = []
    components = []
    for p in walk(roots):
        counts["files"] += 1
        k = elf_kind(p)
        if k is None:
            continue
        counts["elf"] += 1
        if k == "foreign":
            counts["foreign"] += 1
            continue
        elves.append(p)
    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for p, text in zip(elves, pool.map(readelf, elves)):
            info = parse_readelf(text)
            if not info["needed"] and not info["und"]:
                counts["nodyn"] += 1
                continue
            if glibc_component(info, glibc_libs):
                counts["glibc_components"] += 1
                components.append(p)
                continue
            counts["dynamic"] += 1
            if info["interp"]:
                counts["programs"] += 1
            versioned = {lib for lib, _n, _w in info["need_nodes"]}
            if any(lib in glibc_libs and lib not in versioned for lib in info["needed"]):
                counts["unversioned"] += 1
            blockers, weak_missing, needs = judge(info, glibc_libs, exp)
            results[p] = {"info": info, "blockers": blockers, "weak": weak_missing, "needs": needs}
    index = set(results)
    need_users = {}
    node_users = {}
    for p in sorted(results):
        r = results[p]
        for key in set(r["needs"]):
            u = need_users.setdefault(key, [0, 0, p])
            u[0] += 1
            u[1] += 1 if r["info"]["interp"] else 0
        for lib, node, _w in set(r["info"]["need_nodes"]):
            if lib in glibc_libs:
                node_users[(lib, node)] = node_users.get((lib, node), 0) + 1
    memo = {}
    progs = sorted(p for p, r in results.items() if r["info"]["interp"])
    trans = {p: transitive(p, results, index, glibc_libs, memo, set()) for p in progs}
    os.makedirs(args.out, exist_ok=True)

    def ranked(rows):
        return sorted(rows, key=lambda kv: (-kv[1][0], kv[0]))

    compat = [(k, u) for k, u in need_users.items() if classify(manifest, k[1], k[2]) == "compat"]
    with open(os.path.join(args.out, "compat-needs.tsv"), "w", encoding="utf-8") as f:
        f.write("library\tsymbol\tversion\telf_users\tprogram_users\tliella_exports\tfirst_user\n")
        for (lib, sym, ver), u in ranked(compat):
            f.write("%s\t%s\t%s\t%d\t%d\t%s\t%s\n" % (lib, sym, ver, u[0], u[1],
                                                      "yes" if (sym, ver) in exp["syms"] else "no", u[2]))
    missing = [(k, u) for k, u in need_users.items() if (k[1], k[2]) not in exp["syms"]]
    with open(os.path.join(args.out, "missing-needs.tsv"), "w", encoding="utf-8") as f:
        f.write("library\tsymbol\tversion\tkind\telf_users\tprogram_users\tfirst_user\n")
        for (lib, sym, ver), u in ranked(missing):
            f.write("%s\t%s\t%s\t%s\t%d\t%d\t%s\n" % (lib, sym, ver, classify(manifest, sym, ver),
                                                      u[0], u[1], u[2]))
    with open(os.path.join(args.out, "version-nodes.tsv"), "w", encoding="utf-8") as f:
        f.write("library\tversion\telf_users\tliella_defines\n")
        for (lib, node), n in sorted(node_users.items(), key=lambda kv: (-kv[1], kv[0])):
            have = exp["libs"].get(lib)
            f.write("%s\t%s\t%d\t%s\n" % (lib, node, n, "no-library" if have is None
                                          else ("yes" if node in have else "no")))
    with open(os.path.join(args.out, "programs.tsv"), "w", encoding="utf-8") as f:
        f.write("program\tdirect\ttransitive\tfirst_blocker\tweak_missing\n")
        for p in progs:
            r = results[p]
            f.write("%s\t%s\t%s\t%s\t%d\n" % (p, "ok" if not r["blockers"] else "blocked",
                                              "ok" if trans[p] == "ok" else "blocked",
                                              trans[p] if trans[p] != "ok" else "",
                                              len(r["weak"])))
    with open(os.path.join(args.out, "glibc-components.txt"), "w", encoding="utf-8") as f:
        f.write("".join(c + "\n" for c in components))
    if not args.liella_exports:
        write_exports(exp, os.path.join(args.out, "liella-exports.tsv"))
    kinds = {}
    for k in need_users:
        c = classify(manifest, k[1], k[2])
        kinds[c] = kinds.get(c, 0) + 1
    direct_ok = sum(1 for p in progs if not results[p]["blockers"])
    trans_ok = sum(1 for p in progs if trans[p] == "ok")
    lib_ok = sum(1 for p, r in results.items() if not r["info"]["interp"] and not r["blockers"])
    blocker_rank = {}
    for p in progs:
        for b in set(results[p]["blockers"]):
            blocker_rank[b] = blocker_rank.get(b, 0) + 1
    with open(os.path.join(args.out, "summary.md"), "w", encoding="utf-8") as f:
        f.write("# Binary-compatibility sweep (LG-0c)\n\n")
        f.write("Roots: %s\n\n" % ", ".join(roots))
        f.write("| count | value |\n|---|---|\n")
        for k in ["files", "elf", "foreign", "nodyn", "glibc_components", "dynamic", "programs",
                  "unversioned"]:
            f.write("| %s | %d |\n" % (k, counts[k]))
        f.write("| distinct glibc sym@VER needed | %d |\n" % len(need_users))
        for c in sorted(kinds):
            f.write("| ... of kind %s | %d |\n" % (c, kinds[c]))
        f.write("| distinct compat sym@VER needed | %d |\n" % len(compat))
        f.write("| distinct needed sym@VER Liella lacks | %d |\n" % len(missing))
        f.write("| programs ok directly | %d of %d |\n" % (direct_ok, len(progs)))
        f.write("| programs ok with their libraries | %d of %d |\n" % (trans_ok, len(progs)))
        f.write("| libraries/objects ok directly | %d of %d |\n" % (lib_ok, len(results) - len(progs)))
        f.write("\n## Top 25 compat needs (ELF users)\n\n")
        for (lib, sym, ver), u in ranked(compat)[:25]:
            f.write("- %s@%s (%s): %d ELFs, %d programs\n" % (sym, ver, lib, u[0], u[1]))
        f.write("\n## Top 25 program blockers (programs blocked directly)\n\n")
        for b, n in sorted(blocker_rank.items(), key=lambda kv: (-kv[1], kv[0]))[:25]:
            f.write("- %s: %d\n" % (b, n))
        trans_rank = {}
        for p in progs:
            if trans[p] != "ok" and not results[p]["blockers"]:
                key = re.sub(r"^(via \S+: )+", "via a library: ", trans[p])
                trans_rank[key] = trans_rank.get(key, 0) + 1
        f.write("\n## Top 25 blockers of programs that are ok directly but not with their libraries\n\n")
        for b, n in sorted(trans_rank.items(), key=lambda kv: (-kv[1], kv[0]))[:25]:
            f.write("- %s: %d\n" % (b, n))
    prods = []
    if not args.liella_exports:
        for n in sorted(os.listdir(args.liella)):
            p = os.path.join(args.liella, n)
            if ".so" in n and os.path.isfile(p) and not os.path.islink(p):
                with open(p, "rb") as fh:
                    prods.append("%s=%s" % (n, hashlib.sha256(fh.read()).hexdigest()[:16]))
    glibc = subprocess.run(["getconf", "GNU_LIBC_VERSION"], capture_output=True, text=True).stdout.strip()
    with open(os.path.join(args.out, "run.txt"), "w", encoding="utf-8") as f:
        f.write("host: %s\ndate: %s\nhost glibc: %s\nliella: %s\n" % (
            socket.gethostname(), datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
            glibc, " ".join([args.liella_exports or args.liella] + prods)))
    print("bincompat-sweep: %d ELFs, %d programs, %d compat needs, %d missing, %d/%d programs ok "
          "directly, %d transitively -> %s" % (counts["elf"], len(progs), len(compat), len(missing),
                                               direct_ok, len(progs), trans_ok, args.out))
    if counts["programs"] == 0 or not need_users:
        print("bincompat-sweep: FAIL - no programs or no needs found (parser broken?)")
        return 1
    return 0


FIXTURE_PROG = """
Program Headers:
      [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]
Dynamic section at offset 0x1000 contains 3 entries:
 0x0000000000000001 (NEEDED)             Shared library: [libfoo.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000001d (RUNPATH)            Library runpath: [$ORIGIN/../lib]
Symbol table '.dynsym' contains 6 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
     1: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND memcpy@GLIBC_2.14 (3)
     2: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND memcpy_old@GLIBC_2.2.5 (2)
     3: 0000000000000000     0 FUNC    WEAK   DEFAULT  UND __cxa_finalize@GLIBC_2.2.5 (2)
     4: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND foo_init@FOO_1 (4)
     5: 0000000000000000     0 NOTYPE  WEAK   DEFAULT  UND _ITM_deregisterTMCloneTable
Version symbols section '.gnu.version' contains 6 entries:
  000:   0 (*local*)       3 (GLIBC_2.14)    2 (GLIBC_2.2.5)
Version needs section '.gnu.version_r' contains 2 entries:
 Addr: 0x0000000000000660  Offset: 0x00000660  Link: 7 (.dynstr)
  000000: Version: 1  File: libfoo.so.1  Cnt: 1
  0x0010:   Name: FOO_1  Flags: none  Version: 4
  0x0020: Version: 1  File: libc.so.6  Cnt: 3
  0x0030:   Name: GLIBC_2.2.5  Flags: none  Version: 2
  0x0040:   Name: GLIBC_2.14  Flags: none  Version: 3
  0x0050:   Name: GLIBC_2.99  Flags: WEAK  Version: 5
"""

FIXTURE_LIB = """
Dynamic section at offset 0x1000 contains 2 entries:
 0x000000000000000e (SONAME)             Library soname: [libc.so.6]
Symbol table '.dynsym' contains 4 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     1: 0000000000001000    10 FUNC    GLOBAL DEFAULT   9 memcpy@@GLIBC_2.14
     2: 0000000000002000    10 FUNC    GLOBAL DEFAULT   9 memcpy@GLIBC_2.2.5
     3: 0000000000003000    10 FUNC    WEAK   DEFAULT   9 abs@@GLIBC_2.2.5
Version definition section '.gnu.version_d' contains 3 entries:
 Addr: 0x0000000000000500  Offset: 0x00000500  Link: 4 (.dynstr)
  000000: Rev: 1  Flags: BASE  Index: 1  Cnt: 1  Name: libc.so.6
  0x001c: Rev: 1  Flags: none  Index: 2  Cnt: 1  Name: GLIBC_2.2.5
  0x0038: Rev: 1  Flags: none  Index: 3  Cnt: 2  Name: GLIBC_2.14
  0x0054: Parent 1: GLIBC_2.2.5
"""


def self_test():
    """Fixed readelf texts through the parser, the classifier and the judge; exits 1 on a mismatch."""
    fails = []

    def check(name, got, want):
        if got != want:
            fails.append("self-test %s: got %r, want %r" % (name, got, want))

    p = parse_readelf(FIXTURE_PROG)
    check("interp", p["interp"], True)
    check("needed", p["needed"], ["libfoo.so.1", "libc.so.6"])
    check("runpath", p["runpath"], ["$ORIGIN/../lib"])
    check("verneed", p["verneed"], {4: ("libfoo.so.1", "FOO_1"), 2: ("libc.so.6", "GLIBC_2.2.5"),
                                    3: ("libc.so.6", "GLIBC_2.14"), 5: ("libc.so.6", "GLIBC_2.99")})
    check("weak-node", [n for n in p["need_nodes"] if n[2]], [("libc.so.6", "GLIBC_2.99", True)])
    check("und", p["und"][:2], [("memcpy", "GLIBC_2.14", False, 3), ("memcpy_old", "GLIBC_2.2.5", False, 2)])
    check("und-weak-unversioned", p["und"][4], ("_ITM_deregisterTMCloneTable", None, True, None))
    lib = parse_readelf(FIXTURE_LIB)
    check("soname", lib["soname"], "libc.so.6")
    check("verdefs", lib["verdefs"], {"GLIBC_2.2.5", "GLIBC_2.14"})
    check("defs", lib["defs"], {("memcpy", "GLIBC_2.14"), ("memcpy", "GLIBC_2.2.5"), ("abs", "GLIBC_2.2.5")})
    manifest = {("memcpy", "GLIBC_2.14"): {("libc.so.6", "default")},
                ("memcpy", "GLIBC_2.2.5"): {("libc.so.6", "compat")},
                ("abs", "GLIBC_2.2.5"): {("libc.so.6", "default")}}
    check("classify-default", classify(manifest, "memcpy", "GLIBC_2.14"), "default")
    check("classify-compat", classify(manifest, "memcpy", "GLIBC_2.2.5"), "compat")
    check("classify-private", classify(manifest, "_dl_x", "GLIBC_PRIVATE"), "private")
    check("classify-absent", classify(manifest, "nope", "GLIBC_2.2.5"), "absent")
    manifest[("_dl_y", "GLIBC_PRIVATE")] = {("libc.so.6", "compat")}
    check("classify-private-in-manifest", classify(manifest, "_dl_y", "GLIBC_PRIVATE"), "private")
    check("component-no", glibc_component(p, {"libc.so.6"}), False)
    check("component-soname", glibc_component(lib, {"libc.so.6"}), True)
    priv = parse_readelf(FIXTURE_PROG.replace("Name: GLIBC_2.99", "Name: GLIBC_PRIVATE"))
    check("component-private", glibc_component(priv, {"libc.so.6"}), True)
    exp = {"libs": {"libc.so.6": lib["verdefs"]}, "syms": lib["defs"]}
    glibc_libs = {"libc.so.6", "libpthread.so.0"}
    b, w, n = judge(p, glibc_libs, exp)
    check("judge-blockers", b, ["sym:memcpy_old@GLIBC_2.2.5"])
    check("judge-weak", w, ["__cxa_finalize@GLIBC_2.2.5"])
    check("judge-needs", n, [("libc.so.6", "memcpy", "GLIBC_2.14"), ("libc.so.6", "memcpy_old", "GLIBC_2.2.5"),
                             ("libc.so.6", "__cxa_finalize", "GLIBC_2.2.5")])
    p2 = parse_readelf(FIXTURE_PROG.replace("[libfoo.so.1]", "[libpthread.so.0]")
                       .replace("File: libc.so.6  Cnt: 3", "File: libc.so.6  Cnt: 2")
                       .replace("  0x0050:   Name: GLIBC_2.99  Flags: WEAK  Version: 5", "  0x0050:   Name: GLIBC_2.99  Flags: none  Version: 5"))
    b2, _w, _n = judge(p2, glibc_libs, exp)
    check("judge-missing-lib", b2[0], "lib:libpthread.so.0")
    check("judge-missing-node", "node:libc.so.6@GLIBC_2.99" in b2, True)
    results = {"/x/bin/p": {"info": p, "blockers": [], "weak": []},
               "/x/lib/libfoo.so.1": {"info": parse_readelf(""), "blockers": ["sym:bar@GLIBC_2.2.5"], "weak": []}}
    t = transitive("/x/bin/p", results, set(results), glibc_libs, {}, set())
    check("transitive-via", t, "via libfoo.so.1: sym:bar@GLIBC_2.2.5")
    results["/x/lib/libfoo.so.1"]["blockers"] = []
    t = transitive("/x/bin/p", results, set(results), glibc_libs, {}, set())
    check("transitive-ok", t, "ok")
    results["/x/bin/p"]["info"] = parse_readelf(FIXTURE_PROG.replace("[$ORIGIN/../lib]", "[/x/inner]"))
    results["/x/inner/libfoo.so.1"] = results.pop("/x/lib/libfoo.so.1")
    results["/x/inner/libfoo.so.1"]["info"] = {"needed": ["libbar.so.2"], "runpath": []}
    results["/x/inner/libbar.so.2"] = {"info": parse_readelf(""), "blockers": [], "weak": []}
    t = transitive("/x/bin/p", results, set(results), glibc_libs, {}, set())
    check("transitive-inherited-runpath", t, "ok")
    for f in fails:
        print(f)
    print("bincompat-sweep self-test: %s (%d failures)" % ("FAIL" if fails else "PASS", len(fails)))
    return 1 if fails else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out")
    ap.add_argument("--manifest", default="spec/abi/versions.tsv")
    ap.add_argument("--liella", default="target/liella")
    ap.add_argument("--liella-exports")
    ap.add_argument("--root", action="append")
    ap.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    if not args.out:
        ap.error("--out is required")
    return sweep(args)


if __name__ == "__main__":
    sys.exit(main())
