#!/usr/bin/env python3
"""Seed corpora for the fuzz harnesses (implementation-plan.md section 6, "Fuzzing
infrastructure").

Writes fuzz/seeds/<target>/<sha1 prefix> from the repository's own tests and behaviour records:

* regex, glob, wordexp, scanf: the inputs of spec/behaviour/patterns.glibc.txt.gz and
  scanf.glibc.txt.gz (the probe records), re-encoded in each harness's input format;
* printf: the format strings of the printf and stdio probes (crates/core/tests/oracle);
* nsswitch, resolv_conf, hosts, netdb_files, passwd, group, shadow: the fixture texts the
  probes write (C string literals of crates/core/tests/oracle/*.c), sorted by their shape;
* dns, nscache, locale, charmap, iconv, digits_dots: small inputs built here (DNS messages,
  synthetic locale and charmap files; nothing copied from the system's data files).

The repository keeps these small (a cap per target). With --system DIR, it also writes seeds
made from the installed locale files and charmaps into DIR/<target>/corpus, outside the
repository: the campaign corpus root (`cargo xtask fuzz` uses ~/liella-fuzz-corpus).

Run from the repository root: python3 fuzz/harvest_seeds.py [--system DIR]
"""

import argparse
import gzip
import hashlib
import os
import re
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEEDS = os.path.join(ROOT, "fuzz", "seeds")
ORACLE = os.path.join(ROOT, "crates", "core", "tests", "oracle")
BEHAVIOUR = os.path.join(ROOT, "spec", "behaviour")
CAP = 160
MAX_SEED = 4096


PINNED = set()


def put(out, target, data, limit=MAX_SEED, pin=False):
    if len(data) > limit:
        return
    h = hashlib.sha1(data).hexdigest()[:16]
    out.setdefault(target, {})[h] = data
    if pin:
        PINNED.add(h)


def unbracket(line):
    """The [..] fields of a record line, with \\xHH escapes decoded."""
    fields = []
    i = 0
    while True:
        s = line.find("[", i)
        if s < 0:
            return fields
        e = line.find("] ", s + 1)
        if e < 0:
            e = line.rfind("]")
            if e <= s:
                return fields
        raw = line[s + 1:e]
        fields.append(re.sub(rb"\\x([0-9a-fA-F]{2})", lambda m: bytes([int(m.group(1), 16)]), raw.encode("latin-1")))
        i = e + 1


def c_literals(path):
    """The string literals of a C file, adjacent ones joined, escapes decoded."""
    src = open(path, encoding="latin-1").read()
    src = re.sub(r"/\*.*?\*/", "", src, flags=re.S)
    src = re.sub(r"//[^\n]*", "", src)
    out = []
    for m in re.finditer(r'"(?:[^"\\\n]|\\.)*"(?:\s*"(?:[^"\\\n]|\\.)*")*', src):
        parts = re.findall(r'"((?:[^"\\\n]|\\.)*)"', m.group(0))
        out.append(b"".join(c_unescape(p) for p in parts))
    return out


def c_unescape(s):
    b = bytearray()
    i = 0
    while i < len(s):
        c = s[i]
        if c != "\\":
            b += c.encode("latin-1")
            i += 1
            continue
        n = s[i + 1] if i + 1 < len(s) else ""
        simple = {"n": 10, "t": 9, "r": 13, "\\": 92, '"': 34, "'": 39, "a": 7, "b": 8, "f": 12, "v": 11, "?": 63}
        if n in simple:
            b.append(simple[n])
            i += 2
        elif n == "x":
            m = re.match(r"[0-9a-fA-F]+", s[i + 2:])
            b.append(int(m.group(0), 16) & 0xFF if m else 0)
            i += 2 + (len(m.group(0)) if m else 0)
        elif n.isdigit():
            m = re.match(r"[0-7]{1,3}", s[i + 1:])
            b.append(int(m.group(0), 8) & 0xFF)
            i += 1 + len(m.group(0))
        else:
            i += 2
    return bytes(b)


def classify(lit, out):
    text = lit.decode("latin-1")
    lines = [l for l in text.split("\n") if l.strip()]
    if not lines or len(lit) < 4:
        return
    if re.search(r"^(nameserver|search|domain|options|sortlist)\b", text, re.M):
        put(out, "resolv_conf", lit + b"\0\0ndots:2\0host.example.test\0alias real.example.test\n\0alias")
    if re.search(r"^\s*(hosts|passwd|group|shadow|networks|services|protocols|netgroup|aliases|initgroups)\s*:", text, re.M):
        put(out, "nsswitch", lit + b"\0multi on\n\0\x00\x01\x02\x03\x04\x05")
    if any(re.match(r"^\s*[+-]?[^:\s]*:[^:]*:-?\w*:-?\w*(:|$)", l) for l in lines):
        put(out, "passwd", lit + b"\0alice")
        put(out, "group", lit + b"\0alice")
    if any(l.count(":") >= 7 for l in lines):
        put(out, "shadow", lit)
    if any(re.match(r"^\s*[0-9a-fA-F:.]+\s+\S", l) for l in lines):
        put(out, "hosts", lit + b"\0localhost")
    if any(re.match(r"^\s*\S+\s+\d+(/\w+)?", l) for l in lines):
        put(out, "netdb_files", lit + b"\0" + lines[0].split()[0].encode("latin-1"))


def harvest_patterns(out):
    path = os.path.join(BEHAVIOUR, "patterns.glibc.txt.gz")
    with gzip.open(path, "rt", encoding="latin-1") as f:
        for line in f:
            kind = line.split(" ", 1)[0]
            fs = unbracket(line)
            if kind in ("re", "re_search", "re-startend") and len(fs) >= 1:
                flags = line.split(" ", 2)[1] if kind == "re" else ""
                cflags = (1 if "E" in flags else 0) | (2 if "I" in flags else 0) | (4 if "N" in flags else 0)
                subject = fs[1] if len(fs) > 1 else b""
                put(out, "regex", bytes([cflags, 8]) + b"\0\0\0\0" + fs[0] + b"\0" + subject)
                put(out, "regex", bytes([cflags | 0x10, 8]) + b"\0\0\0\0" + fs[0] + b"\0" + subject)
            elif kind == "regcomp" and fs:
                put(out, "regex", b"\x01\x00\0\0\0\0" + fs[0] + b"\0abc")
            elif kind == "glob" and fs:
                rest = line.split("]", 1)[1].split()
                flags = int(rest[0], 0) if rest and re.match(r"^-?\d+$", rest[0]) else 0
                tree = b"tree/\ntree/a\ntree/b\ntree/d1/\ntree/d1/x\ntree/file1\ntree/file2\ntree/link_d1@\ntree/.hidden\n"
                put(out, "glob", struct.pack("<H", flags & 0xFFFF) + b"\0" + fs[0] + b"\0" + tree)
            elif kind == "fnmatch" and len(fs) >= 2:
                put(out, "glob", b"\0\0\0" + fs[0] + b"\0" + fs[1])
            elif kind == "wordexp" and fs:
                rest = line.split("]", 1)[1].split()
                flags = int(rest[0], 0) if rest and re.match(r"^-?\d+$", rest[0]) else 0
                put(out, "wordexp", bytes([flags & 0x7F]) + fs[0] + b"\0HOME=/home/me\nIFS= \t\nx=1\nv=a b\0one two")


def harvest_scanf(out):
    path = os.path.join(BEHAVIOUR, "scanf.glibc.txt.gz")
    with gzip.open(path, "rt", encoding="latin-1") as f:
        for line in f:
            parts = line.split(" ", 2)
            if len(parts) < 3 or parts[0] not in ("s", "ws"):
                continue
            fs = unbracket(line)
            if len(fs) < 2:
                continue
            sets = {"gnu": 0, "c99": 1, "c23": 2}
            header = (1 if parts[0] == "ws" else 0) | (sets.get(parts[1], 0) << 2)
            put(out, "scanf", bytes([header]) + fs[1] + b"\0" + fs[0])


def harvest_printf(out):
    for name in ("printf_diff.c", "stdio_diff.c", "registry_diff.c"):
        path = os.path.join(ORACLE, name)
        if not os.path.exists(path):
            continue
        for lit in c_literals(path):
            if b"%" in lit and len(lit) < 512:
                args = struct.pack("<qqd", 42, -7, 3.25) + b"\x00" * 6 + struct.pack("<Q", 0xC000000000000000) + b"\xff\x3f"
                put(out, "printf", b"\x00" + lit + b"\0" + args)
                put(out, "printf", b"\x03" + lit + b"\0" + args)


def harvest_fixtures(out):
    for name in sorted(os.listdir(ORACLE)):
        if name.endswith(".c"):
            for lit in c_literals(os.path.join(ORACLE, name)):
                classify(lit, out)


# ---------------------------------------------------------------- built here

def dns_name(n):
    b = b""
    for label in n.split("."):
        if label:
            b += bytes([len(label)]) + label.encode()
    return b + b"\0"


def dns_msg(qname, qtype, answers, rcode=0, authority=()):
    hdr = struct.pack(">HHHHHH", 0x1234, 0x8180 | rcode, 1, len(answers), len(authority), 0)
    body = dns_name(qname) + struct.pack(">HH", qtype, 1)
    for owner, rtype, data in list(answers) + list(authority):
        o = b"\xc0\x0c" if owner == qname else dns_name(owner)
        body += o + struct.pack(">HHIH", rtype, 1, 300, len(data)) + data
    return hdr + body


def built(out):
    # DNS responses: A, AAAA, a CNAME chain, PTR, NXDOMAIN with an SOA, a pointer loop.
    put(out, "dns", dns_msg("www.example.com", 1, [("www.example.com", 1, bytes([93, 184, 216, 34]))]))
    put(out, "dns", dns_msg("www.example.com", 28, [("www.example.com", 28, bytes(range(16)))]))
    put(out, "dns", dns_msg("a.example.com", 1, [("a.example.com", 5, dns_name("b.example.com")), ("b.example.com", 1, bytes([10, 0, 0, 1])), ("b.example.com", 1, bytes([10, 0, 0, 2]))]))
    put(out, "dns", dns_msg("4.3.2.1.in-addr.arpa", 12, [("4.3.2.1.in-addr.arpa", 12, dns_name("host.example.com")), ("4.3.2.1.in-addr.arpa", 12, b"\x05other\xc0\x0c")]))
    soa = dns_name("ns.example.com") + dns_name("admin.example.com") + struct.pack(">IIIII", 1, 2, 3, 4, 60)
    put(out, "dns", dns_msg("nx.example.com", 1, [], rcode=3, authority=[("example.com", 6, soa)]))
    put(out, "dns", struct.pack(">HHHHHH", 1, 0x8180, 1, 0, 0, 0) + b"\xc0\x0c\x00\x01\x00\x01")
    put(out, "dns", dns_msg("t.example.com", 16, [("t.example.com", 16, b"\x05hello\x05world")]))

    # The name-service cache: patched tables (mode odd) and a raw header (mode even).
    put(out, "nscache", b"\x01" + b"\x02\x01\x00\x00\x00\x04root\0\x00root:x:0:0:root:/root:/bin/sh\0host\0\x00value\0\0")
    put(out, "nscache", b"\x01key\0value\0\0" + struct.pack("<HQ", 7, 1) + struct.pack("<HQ", 80, 0x1000000001))
    hdr = struct.pack("<8Q", int.from_bytes(b"LIELLANC", "little"), 1, 4096, 1000, 0x1234, 0x5678, 0x9ABC, 0)
    put(out, "nscache", b"\x00" + hdr + struct.pack("<QQ", 3, 3) + b"\0" * 48 + struct.pack("<Q", 256))

    # Locale: synthetic category files (a header, item offsets, NUL-terminated strings).
    def category(magic, items):
        n = len(items)
        off = 8 + 4 * n
        table = b""
        data = b""
        for it in items:
            table += struct.pack("<I", off + len(data))
            data += it
        return struct.pack("<II", magic, n) + table + data

    numeric = category(0x20031011, [b".\0", b",\0", b"\x03\x03\0", b"\0\0\0\0", b"\0\0\0\0", b"UTF-8\0"])
    days = [d.encode() + b"\0" for d in ("Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat")]
    time_items = days + days + [m.encode() + b"\0" for m in ("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")] * 2
    time_items += [b"AM\0", b"PM\0", b"%a %b %e %H:%M:%S %Y\0", b"%m/%d/%y\0", b"%H:%M:%S\0", b"%I:%M:%S %p\0"]
    time_items += [b"+:1:2000/01/01:+*:Era:%EC %Ey\0-:0:1999/12/31:-*:Old:%EC%Ey\0", b"\0"]
    lc_time = category(0x20031012, time_items)
    monetary = category(0x20031014, [b"EUR \0", b"\xe2\x82\xac\0", b",\0", b".\0", b"\x03\0", b"\0", b"-\0"] + [b"\x02"] * 7)

    def locale_seed(name, flags=0, alias=b"", archive=b"", cats=None):
        cats = cats or {}
        order = ["CTYPE", "NUMERIC", "TIME", "COLLATE", "MONETARY", "MESSAGES", "PAPER", "NAME", "ADDRESS", "TELEPHONE", "MEASUREMENT", "IDENTIFICATION"]
        b = bytes([flags]) + struct.pack("<I", len(name)) + name + struct.pack("<I", len(alias)) + alias + struct.pack("<I", len(archive)) + archive
        for c in order:
            d = cats.get(c, b"")
            b += struct.pack("<I", len(d)) + d
        return b

    cats = {"NUMERIC": numeric, "TIME": lc_time, "MONETARY": monetary}
    put(out, "locale", locale_seed(b"xx_XX.UTF-8", cats=cats), limit=1 << 16)
    put(out, "locale", locale_seed(b"alias1", alias=b"# comment\nalias1 xx_XX.UTF-8\n", cats=cats), limit=1 << 16)
    put(out, "locale", locale_seed(b"xx_XX", flags=1, cats=cats), limit=1 << 16)
    # A one-locale archive holding the same categories.
    name = b"xx_XX.utf8\0"
    rec = b""
    blobs = b""
    base = 24 + 12 + len(name) + 4 + 13 * 8
    for i in range(13):
        d = {1: numeric, 2: lc_time, 4: monetary}.get(i, b"")
        rec += struct.pack("<II", base + len(blobs) if d else 0, len(d))
        blobs += d
    archive = struct.pack("<IIIIII", 0xDE020109, 0, 24, 0, 1, 0) + struct.pack("<III", 1, 36, 36 + len(name)) + name + b"\0\0\0\0" + rec + blobs
    put(out, "locale", locale_seed(b"xx_XX.UTF-8", archive=archive), limit=1 << 16)

    # Charmaps: a small one in text and gzip form.
    cm = (b"<code_set_name> FUZZ-8\n<comment_char> %\n<escape_char> /\n<mb_cur_max> 2\n<mb_cur_min> 1\n"
          b"% alias FUZZ8\nCHARMAP\n<U0000>..<U007F> /x00 BASIC\n<U00E9> /xe9 E ACUTE\n"
          b"<U4E00>..<U4E0F> /x81/x40 CJK\n<U20AC> /d128 EURO\n<U00A0> /o240 NBSP\nEND CHARMAP\n")
    put(out, "charmap", b"\x00" + cm + b"\0abc\xe9\x81\x40\x81\x4f\x80\xff")
    put(out, "charmap", b"\x01" + gzip.compress(cm, mtime=0))

    # iconv.
    for f, t, d in ((b"UTF-8", b"UTF-16", "héllo € \U0001f600".encode()), (b"UTF-16LE", b"ISO-8859-1//TRANSLIT", "xé€".encode("utf-16-le")),
                    (b"UCS-4", b"ASCII//IGNORE", "aéb".encode("utf-32-be")), (b"UTF-32", b"UTF-8", b"\xff\xfe\x00\x00A\x00\x00\x00"), (b"latin1", b"utf8", b"\xe9\xff")):
        put(out, "iconv", b"\x07" + f + b"\0" + t + b"\0" + d)

    # Regressions: D-12.18 (positions beyond NL_ARGMAX took memory in proportion to them).
    for f in (b"%142949672950$s", b"%100000000$s", b"%4096$d%1$d", b"%1$*5000$d"):
        put(out, "printf", b"\x00" + f + b"\0" + struct.pack("<q", 7), pin=True)
    # FZ-1: stacked repetitions of a nullable body (`b*++++...`) grew the matcher's states past
    # 2 GB; now REG_ESPACE beyond 2^21 states (D-14.4).
    put(out, "regex", bytes.fromhex("110800000061292b2b2b2b622a" + "2b" * 17 + "68"), pin=True)
    # RX-STARS: `((|)a` + ~60 stacked `*`/`?` + `(b))+` compiles to ~440 KB (D-14.8, under the 2^21 state bound);
    # the capture pass costs ~18 ms per subject byte, so the harness bounds program x subject for it.
    put(out, "regex", bytes.fromhex(
        "01080000002e28287c29612a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a2e2a2a2a2a3f2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2e2a2a2a2a3f2a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a286229292b002a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a286229292b00ffffff"), pin=True)
    put(out, "regex", bytes.fromhex(
        "01080000002e28287c29612a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a2e2a2a2a2a3f2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2e2a2a2a2a3f2a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a286229292b002a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2e2a2a2a2a3f2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2e2a2a2a2a3f2a2a2a2a"
        "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a286229292b002a2a2a2a2a2a2a2a2a2a2a2a"
        "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a286229292b00ffffff"), pin=True)
    # FZ-2: `${1#pat}` over a long `$1` with invalid bytes took a match per prefix, each decoding the
    # whole value (quadratic; the fuzz run hit libFuzzer's 20 s timeout). Shaped on the minimized input.
    put(out, "wordexp", b"\x80" + b"${1#\xe5\x85}${1#}\x85:${1#}" * 52 + b"\0\0" + b"#\"pa\xff\xff\x85\xe5" * 175, pin=True)

    for t in (b"1.2.3.4", b"1.2.3", b"123", b"0x7f.1", b"010.0.0.1", b"::1", b"fe80::1%1", b"::ffff:1.2.3.4", b"1:2:3:4:5:6:7:8", b"1.2.3.4.", b"255.255.255.255 x", b"10/8", b"192.168.1.0/24"):
        put(out, "digits_dots", t)


def system(out_dir):
    """Seeds from the installed locale files and charmaps, outside the repository."""
    out = {}
    base = "/usr/lib/locale/C.utf8"
    order = ["LC_CTYPE", "LC_NUMERIC", "LC_TIME", "LC_COLLATE", "LC_MONETARY", "LC_MESSAGES/SYS_LC_MESSAGES", "LC_PAPER", "LC_NAME", "LC_ADDRESS", "LC_TELEPHONE", "LC_MEASUREMENT", "LC_IDENTIFICATION"]
    if os.path.isdir(base):
        for skip_ctype in (False, True):
            name = b"C.utf8" if not skip_ctype else b"xx_XX.UTF-8"
            b = b"\0" + struct.pack("<I", len(name)) + name + struct.pack("<I", 0) + struct.pack("<I", 0)
            for c in order:
                p = os.path.join(base, c)
                d = open(p, "rb").read() if os.path.exists(p) and not (skip_ctype and c == "LC_CTYPE") else b""
                b += struct.pack("<I", len(d)) + d
            put(out, "locale", b, limit=1 << 20)
    cmdir = "/usr/share/i18n/charmaps"
    if os.path.isdir(cmdir):
        for name in sorted(os.listdir(cmdir)):
            if name.endswith(".gz"):
                data = open(os.path.join(cmdir, name), "rb").read()
                put(out, "charmap", b"\x01" + data, limit=1 << 16)
    for target, files in out.items():
        d = os.path.join(out_dir, target, "corpus")
        os.makedirs(d, exist_ok=True)
        for h, data in files.items():
            with open(os.path.join(d, h), "wb") as f:
                f.write(data)
        print(f"{target}: {len(files)} system seeds in {d}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--system", metavar="DIR", help="also write seeds from installed data into DIR/<target>/corpus")
    args = ap.parse_args()
    out = {}
    harvest_patterns(out)
    harvest_scanf(out)
    harvest_printf(out)
    harvest_fixtures(out)
    built(out)
    for target in sorted(out):
        files = out[target]
        d = os.path.join(SEEDS, target)
        os.makedirs(d, exist_ok=True)
        for old in os.listdir(d):
            os.remove(os.path.join(d, old))
        # A deterministic sample when there are more than the cap; regression seeds always.
        pinned = sorted(h for h in files if h in PINNED)
        rest = sorted(h for h in files if h not in PINNED)
        keep = pinned + rest[: max(0, CAP - len(pinned))]
        for h in keep:
            with open(os.path.join(d, h), "wb") as f:
                f.write(files[h])
        total = sum(len(files[h]) for h in keep)
        print(f"{target}: {len(keep)} seeds, {total} bytes")
    if args.system:
        system(args.system)
    return 0


if __name__ == "__main__":
    sys.exit(main())
