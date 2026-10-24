#!/usr/bin/env python3
"""ldcache_dump.py - read and write ld.so.cache files from the layout in spec Appendix D.3.

  ldcache_dump.py dump FILE...        strict parse; prints the header, every entry and the extensions
  ldcache_dump.py write OUT SPEC      writes a new-format cache from SPEC (tab-separated lines:
                                      flags-hex  soname  path  [hwcaps-subdirectory-name]), in the
                                      order given, with its own string layout (no shared tails)

Written from the hypothesis in Appendix D.3 (format inspection of ldconfig's output); no loader or
ldconfig source was read. Exits 1 on any structural error, so a run proves the layout holds.
"""
import struct
import sys

NEW_MAGIC = b"glibc-ld.so.cache1.1"
OLD_MAGIC = b"ld.so-1.7.0\0"
EXT_MAGIC = 0xEAA42174
HWCAP_EXT = 1 << 62


class Bad(Exception):
    pass


def cstr(buf, off, base_end):
    if off >= base_end:
        raise Bad(f"string offset {off:#x} outside 0..{base_end:#x}")
    end = buf.find(b"\0", off, base_end)
    if end < 0:
        raise Bad(f"string at {off:#x} not NUL-terminated")
    return buf[off:end].decode("utf-8", "replace")


def parse_old(buf):
    n, = struct.unpack_from("<I", buf, 12)
    ents_end = 16 + 12 * n
    if ents_end > len(buf):
        raise Bad("old: entries past end of file")
    out = []
    for i in range(n):
        fl, k, v = struct.unpack_from("<iII", buf, 16 + 12 * i)
        out.append((fl, cstr(buf, ents_end + k, len(buf)), cstr(buf, ents_end + v, len(buf))))
    return n, ents_end, out


def parse_new(buf, base):
    """base = file offset of the new header; offsets in it are relative to base."""
    b = buf[base:]
    if b[:20] != NEW_MAGIC:
        raise Bad(f"no new magic at {base:#x}")
    nlibs, len_strings = struct.unpack_from("<II", b, 20)
    flags = b[28]
    pad1 = b[29:32]
    ext_off, = struct.unpack_from("<I", b, 32)
    unused = struct.unpack_from("<III", b, 36)
    ents_end = 48 + 24 * nlibs
    str_end = ents_end + len_strings
    if str_end > len(b):
        raise Bad("new: entries + strings past end of file")
    hdr = dict(nlibs=nlibs, len_strings=len_strings, flags=flags, pad=pad1.hex(),
               ext_off=ext_off, unused=unused, strings=(ents_end, str_end))
    ents = []
    for i in range(nlibs):
        fl, k, v, osv, hw = struct.unpack_from("<iIIIQ", b, 48 + 24 * i)
        for o in (k, v):
            if not ents_end <= o < str_end:
                raise Bad(f"entry {i}: offset {o:#x} outside the string table")
        ents.append(dict(flags=fl, key=cstr(b, k, str_end), path=cstr(b, v, str_end),
                         osversion=osv, hwcap=hw))
    exts = []
    hwnames = None
    if ext_off:
        # The extension's own offsets count from the start of the FILE, even inside a compat
        # file whose other offsets count from the new header (observed; Appendix D.3).
        eb = buf
        if ext_off % 4 or ext_off + 8 > len(eb):
            raise Bad(f"extension offset {ext_off:#x}")
        magic, count = struct.unpack_from("<II", eb, ext_off)
        if magic != EXT_MAGIC:
            raise Bad(f"extension magic {magic:#x}")
        for j in range(count):
            tag, xfl, off, size = struct.unpack_from("<IIII", eb, ext_off + 8 + 16 * j)
            if off + size > len(eb):
                raise Bad(f"extension {j} past end of file")
            exts.append((tag, xfl, off, size))
            if tag == 1:
                if size % 4:
                    raise Bad("hwcaps extension size not a multiple of 4")
                offs = struct.unpack_from(f"<{size // 4}I", eb, off)
                hdr["hwcaps_name_offsets"] = [hex(o) for o in offs]
                # The name offsets themselves count from the new header, like the entries'.
                hwnames = [cstr(b, o, str_end) for o in offs]
    for e in ents:
        if e["hwcap"] & HWCAP_EXT:
            idx = e["hwcap"] & 0xFFFFFFFF
            if hwnames is None or idx >= len(hwnames):
                raise Bad(f"{e['key']}: hwcaps index {idx} without a name")
            if e["hwcap"] & ~(HWCAP_EXT | 0xFFFFFFFF):
                raise Bad(f"{e['key']}: other hwcap bits with the subdirectory flag")
            e["subdir"] = hwnames[idx]
    return hdr, ents, exts, hwnames


def dump(path):
    buf = open(path, "rb").read()
    print(f"== {path} ({len(buf)} bytes)")
    base = 0
    if buf.startswith(OLD_MAGIC):
        n, ents_end, old = parse_old(buf)
        print(f"old: nlibs={n} strings at {ents_end:#x}")
        for fl, k, v in old:
            print(f"  old {fl:#06x} {k} => {v}")
        i = buf.find(NEW_MAGIC)
        if i < 0:
            return
        print(f"compat: new header at {i:#x} (old part {i:#x} bytes, align {i % 8})")
        base = i
    hdr, ents, exts, hwnames = parse_new(buf, base)
    print("new: " + " ".join(f"{k}={v}" for k, v in hdr.items()))
    for e in ents:
        sub = f" hwcaps={e['subdir']}" if "subdir" in e else (
            f" hwcap={e['hwcap']:#x}" if e["hwcap"] else "")
        osv = f" osversion={e['osversion']:#x}" if e["osversion"] else ""
        print(f"  {e['flags']:#06x} {e['key']} => {e['path']}{sub}{osv}")
    for tag, xfl, off, size in exts:
        val = buf[off:off + size]
        shown = val.decode("utf-8", "replace") if tag == 0 else val.hex()
        print(f"  ext tag={tag} flags={xfl} off={off:#x} size={size}: {shown}")
    if hwnames is not None:
        print(f"  hwcaps names: {hwnames}")
    flags = sorted({e["flags"] for e in ents})
    print(f"  flag values: {[hex(f) for f in flags]}")


def write(out, spec):
    rows = []
    for line in open(spec):
        line = line.rstrip("\n")
        if line and not line.startswith("#"):
            f = line.split("\t")
            rows.append((int(f[0], 16), f[1], f[2], f[3] if len(f) > 3 else None))
    subdirs = []
    for r in rows:
        if r[3] and r[3] not in subdirs:
            subdirs.append(r[3])
    n = len(rows)
    strings = bytearray()
    base = 48 + 24 * n

    def add(s):
        off = base + len(strings)
        strings.extend(s.encode() + b"\0")
        return off
    ents = bytearray()
    sub_off = [add(s) for s in subdirs]
    for fl, key, path, sub in rows:
        k, v = add(key), add(path)
        hw = (HWCAP_EXT | subdirs.index(sub)) if sub else 0
        ents += struct.pack("<iIIIQ", fl, k, v, 0, hw)
    end = base + len(strings)
    ext_off = (end + 3) & ~3
    ext = struct.pack("<II", EXT_MAGIC, 1 if subdirs else 0)
    arr_off = ext_off + 8 + (16 if subdirs else 0)
    if subdirs:
        ext += struct.pack("<IIII", 1, 0, arr_off, 4 * len(subdirs))
        ext += struct.pack(f"<{len(subdirs)}I", *sub_off)
    hdr = NEW_MAGIC + struct.pack("<II", n, len(strings)) + bytes([2, 0, 0, 0])
    hdr += struct.pack("<IIII", ext_off, 0, 0, 0)
    data = hdr + ents + strings + b"\0" * (ext_off - end) + ext
    open(out, "wb").write(data)


def main():
    if len(sys.argv) >= 3 and sys.argv[1] == "dump":
        rc = 0
        for p in sys.argv[2:]:
            try:
                dump(p)
            except (Bad, struct.error) as e:
                print(f"BAD {p}: {e}")
                rc = 1
        sys.exit(rc)
    if len(sys.argv) == 4 and sys.argv[1] == "write":
        write(sys.argv[2], sys.argv[3])
        sys.exit(0)
    sys.exit(__doc__)


main()
