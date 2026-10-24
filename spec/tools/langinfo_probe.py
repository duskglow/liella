#!/usr/bin/env python3
"""Record the reference library's nl_langinfo items for the built-in "C" and "C.UTF-8" locales
(D-7.2: the built-ins never read files, so their data is part of the implementation; as with
spec/abi/sysconf.tsv, D-24.6, the values are recorded from the reference system).

  python3 spec/tools/langinfo_probe.py PROBE > spec/abi/langinfo.tsv

PROBE is target/probe/probe_langinfo (prints W <value> for word items, P <hex> otherwise).
Item boundaries come from the reference system's compiled C.utf8 (/usr/lib/locale/C.utf8),
which holds glibc's C.UTF-8 data; a file-loaded copy of it (under LOCPATH) tells word items
from pointer items. Large LC_CTYPE and LC_COLLATE tables are not recorded: the built-ins
generate their own (D-5.4).

Columns: category, index, name, kind (W word / P bytes), C value, C.UTF-8 value (decimal for W,
hex for P). "C" differs from "C.UTF-8" in MB_CUR_MAX, the codeset items and the transliteration
table; its LC_IDENTIFICATION is recorded as C.UTF-8's (D-7.10).
"""
import os
import shutil
import struct
import subprocess
import sys

sys.path.insert(0, __file__.rsplit('/', 1)[0])
import locale_inspect as li  # noqa: E402

SRC = '/usr/lib/locale/C.utf8'
COPY = '/tmp/liella-langinfo-probe'
CATS = [('LC_CTYPE', 0), ('LC_NUMERIC', 1), ('LC_TIME', 2), ('LC_COLLATE', 3), ('LC_MONETARY', 4),
        ('LC_MESSAGES', 5), ('LC_PAPER', 7), ('LC_NAME', 8), ('LC_ADDRESS', 9), ('LC_TELEPHONE', 10),
        ('LC_MEASUREMENT', 11), ('LC_IDENTIFICATION', 12)]
# LC_CTYPE items recorded (the others are tables or table offsets).
CTYPE_ITEMS = set([13, 14] + list(range(19, 61)) + [61, 66, 67, 68, 69, 70, 71])
COLLATE_ITEMS = {0, 13, 18}


def probe(exe, loc, specs, env=None):
    r = subprocess.run([exe, loc] + specs, capture_output=True, text=True, env=env)
    lines = [l.split('\t') for l in r.stdout.split('\n') if l]
    if len(lines) != len(specs):
        raise SystemExit('probe %s failed (%d of %d)' % (loc, len(lines), len(specs)))
    return [(l[2], l[3]) for l in lines]


def cut(kind, hexval, length):
    """A pointer item's bytes: `length` bytes, extended for strings that run longer."""
    return hexval[:2 * length]


def main():
    exe = sys.argv[1]
    if os.path.exists(COPY):
        shutil.rmtree(COPY)
    os.makedirs(COPY)
    shutil.copytree(SRC, COPY + '/xx_XX.UTF-8')
    env = dict(os.environ, LOCPATH=COPY)
    print('category\tindex\tname\tkind\tC\tC.UTF-8')
    for name, c in CATS:
        f = SRC + '/' + ('LC_MESSAGES/SYS_LC_MESSAGES' if name == 'LC_MESSAGES' else name)
        data, _, items = li.read_items(f)
        idx = [i for i in range(len(items)) if (c != 0 or i in CTYPE_ITEMS) and (c != 3 or i in COLLATE_ITEMS)]
        names = li.item_names(c)
        kinds = probe(exe, 'xx_XX.UTF-8', ['%d:%d:0' % (c, i) for i in idx], env)
        # Read generously: strings in "C" may be longer than in C.UTF-8 (the codeset names).
        # Word items are never dereferenced (their high half may be garbage in the built-ins).
        is_word = [kinds[k][0] == 'W' for k in range(len(idx))]
        specs = ['%d:%d:%d' % (c, i, 0 if is_word[k] else max(items[i][1] - items[i][0], 1) + 64) for k, i in enumerate(idx)]
        vc = probe(exe, 'C', specs)
        vu = probe(exe, 'C.UTF-8', specs)
        for k, i in enumerate(idx):
            a, b = items[i]
            fv = data[a:b]
            kind = 'W' if kinds[k][0] == 'W' and int(kinds[k][1]) == (struct.unpack('<I', fv[:4])[0] if len(fv) >= 4 else -1) else 'P'
            label = '/'.join(names.get(i, ['?']))
            if kind == 'W':
                wc = struct.unpack('<I', fv[:4])[0]
                cval = vc[k][1] if vc[k][0] == 'W' else str(wc)
                print('%d\t%d\t%s\tW\t%s\t%d' % (c, i, label, cval, wc))
                continue
            uval = fv.hex()
            craw = bytes.fromhex(vc[k][1]) if vc[k][0] == 'P' else b''
            first = lambda b: b.split(b'\0', 1)[0]
            if label.endswith('CODESET'):
                # The codeset differs in "C": one string.
                cval = (craw.split(b'\0')[0] + b'\0').hex()
            elif c == 12:
                # LC_IDENTIFICATION is descriptive metadata. The reference "C" names a person
                # (address, e-mail, telephone); Liella's "C" carries the C.UTF-8 values instead
                # (D-7.10), so no personal contact data is copied.
                cval = uval
            else:
                # Everything else is shared; a probe read runs past short items, so only the
                # first string is compared, and a difference is reported for review.
                if first(craw) != first(fv):
                    sys.stderr.write('note: %s differs in C: %r vs %r\n' % (label, first(craw)[:40], first(fv)[:40]))
                cval = uval
            print('%d\t%d\t%s\tP\t%s\t%s' % (c, i, label, cval, uval))


if __name__ == '__main__':
    main()
