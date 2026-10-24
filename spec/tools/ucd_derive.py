#!/usr/bin/env python3
"""Derive the built-in C.UTF-8 character tables from the UCD and compare them with a compiled
LC_CTYPE (D-5.4). The rules found here are what `cargo xtask unicode-gen` implements.

  python3 spec/tools/ucd_derive.py /usr/lib/locale/C.utf8/LC_CTYPE
"""
import struct
import sys

sys.path.insert(0, __file__.rsplit('/', 1)[0])
import locale_inspect as li  # noqa: E402

UCD = __file__.rsplit('/', 2)[0] + '/unicode/15.1.0/'
CLASSES = ['upper', 'lower', 'alpha', 'digit', 'xdigit', 'space', 'print', 'graph', 'blank',
           'cntrl', 'punct', 'alnum']
MAX = 0x110000


def ranges(path, want=None):
    """{property: set(code points)} from a UCD property file (cp or a..b ; prop)."""
    out = {}
    for line in open(UCD + path, encoding='utf-8'):
        line = line.split('#')[0].strip()
        if not line:
            continue
        cps, prop = [x.strip() for x in line.split(';')[:2]]
        if want and prop not in want:
            continue
        a, _, b = cps.partition('..')
        out.setdefault(prop, set()).update(range(int(a, 16), int(b or a, 16) + 1))
    return out


def unicode_data():
    """cp -> (category, upper, lower, title), expanding <..., First>/<..., Last> ranges."""
    data, first = {}, None
    for line in open(UCD + 'UnicodeData.txt', encoding='utf-8'):
        f = line.rstrip('\n').split(';')
        cp = int(f[0], 16)
        rec = (f[2], int(f[12], 16) if f[12] else None, int(f[13], 16) if f[13] else None,
               int(f[14], 16) if f[14] else None)
        if f[1].endswith(', First>'):
            first = cp
            continue
        if f[1].endswith(', Last>'):
            for c in range(first, cp + 1):
                data[c] = rec
            continue
        data[cp] = rec
    return data


class Compiled:
    """The classes, maps and widths of a compiled LC_CTYPE (Appendix D.1.2-D.1.3)."""

    def __init__(self, path):
        d, _, items = li.read_items(path)
        raw = lambda i: d[items[i][0]:items[i][1]]
        names = [n.decode() for n in raw(10).split(b'\0') if n]
        mnames = [n.decode() for n in raw(11).split(b'\0') if n]
        co = struct.unpack('<I', raw(17))[0]
        mo = struct.unpack('<I', raw(18))[0]
        self.cls = {n: raw(co + k) for k, n in enumerate(names)}
        self.maps = {n: raw(mo + k) for k, n in enumerate(mnames)}
        self.width = raw(12)

    @staticmethod
    def look(t, wc, kind):
        shift1, bound, shift2, mask2, mask3 = struct.unpack_from('<5I', t, 0)
        i1 = wc >> shift1
        if i1 >= bound:
            return None
        l2 = struct.unpack_from('<I', t, 20 + 4 * i1)[0]
        if not l2:
            return None
        l3 = struct.unpack_from('<I', t, l2 + 4 * ((wc >> shift2) & mask2))[0]
        if not l3:
            return None
        if kind == 'bit':
            w = struct.unpack_from('<I', t, l3 + 4 * ((wc >> 5) & mask3))[0]
            return (w >> (wc & 31)) & 1
        if kind == 'map':
            return struct.unpack_from('<i', t, l3 + 4 * (wc & mask3))[0]
        return t[l3 + (wc & mask3)]

    def member(self, name, wc):
        return bool(self.look(self.cls[name], wc, 'bit'))

    def mapped(self, name, wc):
        return wc + (self.look(self.maps[name], wc, 'map') or 0)

    def wcwidth(self, wc):
        w = self.look(self.width, wc, 'width')
        return -1 if w is None or w == 0xff else w


NOBREAK = {0x00A0, 0x2007, 0x202F}


def derive():
    """The C.UTF-8 tables from the UCD: {class: set}, {map: dict}, width function."""
    ud = unicode_data()
    core = ranges('DerivedCoreProperties.txt', {'Alphabetic', 'Lowercase', 'Uppercase', 'Default_Ignorable_Code_Point'})
    cat = lambda c: ud.get(c, ('Cn', None, None, None))[0]
    upper_map = {c: r[1] for c, r in ud.items() if r[1] is not None}
    lower_map = {c: r[2] for c, r in ud.items() if r[2] is not None}
    title_map = {c: r[3] for c, r in ud.items() if r[3] is not None}
    cls = {k: set() for k in CLASSES + ['combining', 'combining_level3']}
    ccc = {}
    for line in open(UCD + 'UnicodeData.txt', encoding='utf-8'):
        f = line.split(';')
        ccc[int(f[0], 16)] = int(f[3])
    for c in range(MAX):
        g = cat(c)
        if (c in lower_map and lower_map[c] != c) or c in core['Uppercase']:
            cls['upper'].add(c)
        if (c in upper_map and upper_map[c] != c) or c in core['Lowercase']:
            cls['lower'].add(c)
        if c in core['Alphabetic'] or (g == 'Nd' and not 0x30 <= c <= 0x39):
            cls['alpha'].add(c)
        if 0x30 <= c <= 0x39:
            cls['digit'].add(c)
        if 0x30 <= c <= 0x39 or 0x41 <= c <= 0x46 or 0x61 <= c <= 0x66:
            cls['xdigit'].add(c)
        if (g == 'Zs' and c not in NOBREAK) or g in ('Zl', 'Zp') or 0x09 <= c <= 0x0D:
            cls['space'].add(c)
        if (g == 'Zs' and c not in NOBREAK) or c == 0x09:
            cls['blank'].add(c)
        if g in ('Cc', 'Zl', 'Zp'):
            cls['cntrl'].add(c)
        if g not in ('Cc', 'Zl', 'Zp', 'Cs', 'Cn'):
            cls['print'].add(c)
            if not (g == 'Zs' and c not in NOBREAK):
                cls['graph'].add(c)
        if g in ('Mn', 'Mc', 'Me'):
            cls['combining'].add(c)
            if ccc.get(c, 0) < 200:
                cls['combining_level3'].add(c)
    cls['alnum'] = cls['alpha'] | cls['digit']
    cls['punct'] = cls['graph'] - cls['alnum']
    return cls, {'toupper': upper_map, 'tolower': lower_map, 'totitle': title_map}, ud


def compare(ref, cls, maps):
    total = 0
    for name, members in cls.items():
        if name not in ref.cls:
            continue
        missing = [c for c in range(MAX) if ref.member(name, c) and c not in members]
        extra = [c for c in members if not ref.member(name, c)]
        total += len(missing) + len(extra)
        print('%-17s missing %5d extra %5d   e.g. missing %s extra %s' % (
            name, len(missing), len(extra), ' '.join('%04X' % c for c in missing[:6]),
            ' '.join('%04X' % c for c in sorted(extra)[:6])))
    for name, m in maps.items():
        bad = [c for c in range(MAX) if ref.mapped(name, c) != m.get(c, c)]
        total += len(bad)
        print('%-17s differ %5d  e.g. %s' % (name, len(bad), ' '.join(
            '%04X:%04X/%04X' % (c, ref.mapped(name, c), m.get(c, c)) for c in bad[:5])))
    return total


def derive_width(ud):
    """wcwidth per code point: a dict holding only the values other than 1."""
    eaw = ranges('EastAsianWidth.txt')
    wide = eaw.get('W', set()) | eaw.get('F', set())
    hst = ranges('HangulSyllableType.txt')
    jamo0 = hst.get('V', set()) | hst.get('T', set())
    pcm = ranges('PropList.txt', {'Prepended_Concatenation_Mark'}).get('Prepended_Concatenation_Mark', set())
    out = {}
    for c in range(MAX):
        g = ud.get(c, ('Cn',))[0]
        if c == 0:
            w = 0
        elif g in ('Cc', 'Cs', 'Zl', 'Zp', 'Cn'):
            w = -1
        elif c == 0x00AD or c in pcm:
            w = 1
        elif g in ('Mn', 'Me', 'Cf') or c in jamo0:
            w = 0
        elif c in wide or (0x2E80 <= c <= 0xA4CF and c != 0x303F):
            w = 2
        else:
            w = 1
        if w != 1:
            out[c] = w
    return out


def main():
    ref = Compiled(sys.argv[1])
    if len(sys.argv) > 2 and sys.argv[2] == 'width':
        ud = unicode_data()
        wd = derive_width(ud)
        bad = [c for c in range(MAX) if ref.wcwidth(c) != wd.get(c, 1)]
        from collections import Counter
        print('width differ', len(bad), Counter((ref.wcwidth(c), wd.get(c, 1), ud.get(c, ('Cn',))[0]) for c in bad).most_common(12))
        print(' '.join('%04X' % c for c in bad[:40]))
        return
    if len(sys.argv) > 2 and sys.argv[2] == 'compare':
        cls, maps, _ = derive()
        print('total differences:', compare(ref, cls, maps))
        return
    ud = unicode_data()
    print('loaded', len(ud), 'code points')
    # Report each class's membership statistics by general category, to guide the rules.
    for name in CLASSES + [n for n in ref.cls if n not in CLASSES]:
        by_cat = {}
        for cp in range(MAX):
            if ref.member(name, cp):
                cat = ud.get(cp, ('Cn',))[0]
                by_cat[cat] = by_cat.get(cat, 0) + 1
        print('%-17s %s' % (name, ' '.join('%s:%d' % kv for kv in sorted(by_cat.items()))))


if __name__ == '__main__':
    main()
