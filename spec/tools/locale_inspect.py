#!/usr/bin/env python3
"""Inspect glibc's compiled locale files (format inspection, implementation-plan.md D-7.2;
findings recorded in spec Appendix D).

  python3 spec/tools/locale_inspect.py items  DIR/LC_CTYPE      list items, offsets, sizes
  python3 spec/tools/locale_inspect.py item   DIR/LC_CTYPE N    hex dump of item N
  python3 spec/tools/locale_inspect.py archive ARCHIVE           list the archive's header words

Item names come from the public <langinfo.h> (an nl_item is category << 16 | index); the
category is taken from the file name.
"""
import re
import struct
import sys

CATEGORY_OF_FILE = {
    'LC_CTYPE': 0, 'LC_NUMERIC': 1, 'LC_TIME': 2, 'LC_COLLATE': 3, 'LC_MONETARY': 4,
    'LC_MESSAGES': 5, 'LC_PAPER': 7, 'LC_NAME': 8, 'LC_ADDRESS': 9, 'LC_TELEPHONE': 10,
    'LC_MEASUREMENT': 11, 'LC_IDENTIFICATION': 12,
}


def item_names(category):
    """Names of the items of one category, in index order, from <langinfo.h>."""
    text = open('/usr/include/langinfo.h', encoding='utf-8').read()
    cat_macro = {0: '__LC_CTYPE', 1: '__LC_NUMERIC', 2: '__LC_TIME', 3: '__LC_COLLATE',
                 4: '__LC_MONETARY', 5: '__LC_MESSAGES', 7: '__LC_PAPER', 8: '__LC_NAME',
                 9: '__LC_ADDRESS', 10: '__LC_TELEPHONE', 11: '__LC_MEASUREMENT',
                 12: '__LC_IDENTIFICATION'}[category]
    body = text[text.index('enum'):]
    names, index, current = {}, None, False
    for raw in body.splitlines():
        line = raw.split('/*')[0].strip().rstrip(',')
        if not line or line.startswith('#'):
            continue
        m = re.match(r'(\w+)\s*=\s*_NL_ITEM\s*\(\s*(\w+)\s*,\s*(\d+)\s*\)', line)
        if m:
            current = m.group(2) == cat_macro
            index = int(m.group(3))
            if current:
                names.setdefault(index, []).append(m.group(1))
            continue
        m = re.match(r'(\w+)\s*=\s*(\w+)$', line)
        if m and current:  # an alias of the previous item, e.g. CODESET = _NL_CTYPE_CODESET_NAME
            names.setdefault(index, []).append(m.group(1))
            continue
        m = re.match(r'(\w+)$', line)
        if m and current and index is not None:
            if m.group(1).startswith('_NL_NUM_'):
                current = False
                continue
            index += 1
            names.setdefault(index, []).append(m.group(1))
    return names


def read_items(path):
    data = open(path, 'rb').read()
    magic, n = struct.unpack_from('<II', data, 0)
    offsets = list(struct.unpack_from('<%dI' % n, data, 8))
    ends = offsets[1:] + [len(data)]
    return data, magic, list(zip(offsets, ends))


def main():
    cmd, path = sys.argv[1], sys.argv[2]
    if cmd == 'archive':
        data = open(path, 'rb').read()
        words = struct.unpack_from('<16I', data, 0)
        print('header words:', ' '.join('%#x' % w for w in words))
        return
    data, magic, items = read_items(path)
    cat = CATEGORY_OF_FILE[path.rstrip('/').split('/')[-1]]
    names = item_names(cat)
    if cmd == 'items':
        print('magic %#010x, %d items, %d bytes' % (magic, len(items), len(data)))
        for i, (a, b) in enumerate(items):
            raw = data[a:b]
            text = ''
            if raw and raw[-1:] == b'\0' and all(32 <= c < 127 for c in raw[:-1].rstrip(b'\0')):
                text = repr(raw.rstrip(b'\0').decode())[:60]
            print('%3d %-34s off %8d len %8d %s' % (i, '/'.join(names.get(i, ['?'])), a, b - a, text))
    elif cmd == 'item':
        a, b = items[int(sys.argv[3])]
        raw = data[a:b]
        for k in range(0, min(len(raw), 512), 16):
            chunk = raw[k:k + 16]
            print('%06x  %-48s %s' % (k, ' '.join('%02x' % c for c in chunk),
                                      ''.join(chr(c) if 32 <= c < 127 else '.' for c in chunk)))


if __name__ == '__main__':
    main()
