#!/usr/bin/env python3
"""Prototype of collation over glibc's compiled LC_COLLATE (format inspection, D-7.2/D-4.18).

Derives, and checks against the host glibc's strxfrm, how a compiled table turns a string
into a key. The Rust implementation follows what this prototype establishes; Appendix D
records the format.

  python3 spec/tools/collate_proto.py LC_COLLATE elements STRING...   per-element weights
  python3 spec/tools/collate_proto.py LC_COLLATE xfrm < lines          keys, one per line (hex)
"""
import struct
import sys

sys.path.insert(0, __file__.rsplit('/', 1)[0])
import locale_inspect as li  # noqa: E402


class Table:
    def __init__(self, path):
        d, _, items = li.read_items(path)
        raw = lambda i: d[items[i][0]:items[i][1]]
        self.nrules = struct.unpack('<I', raw(0))[0]
        self.rulesets = raw(1)
        self.table = struct.unpack('<256i', raw(2)) if self.nrules else None
        self.weights = raw(3)
        self.extra = raw(4)
        self.indirect = raw(5)
        self.tablewc = raw(9)
        self.weightswc = raw(10)
        n = len(self.weightswc) // 4
        self.wwc = struct.unpack('<%dI' % n, self.weightswc)
        self.extrawc = struct.unpack('<%di' % (len(raw(11)) // 4), raw(11))
        self.indirectwc = struct.unpack('<%di' % (len(raw(12)) // 4), raw(12))

    def table3(self, wc):
        """The 3-level wide table: an int32 per code point (0 when absent)."""
        t = self.tablewc
        shift1, bound, shift2, mask2, mask3 = struct.unpack_from('<5I', t, 0)
        i1 = wc >> shift1
        if i1 >= bound:
            return 0
        l2 = struct.unpack_from('<I', t, 20 + 4 * i1)[0]
        if not l2:
            return 0
        l3 = struct.unpack_from('<I', t, l2 + 4 * ((wc >> shift2) & mask2))[0]
        if not l3:
            return 0
        return struct.unpack_from('<i', t, l3 + 4 * (wc & mask3))[0]

    def find_wc(self, ws, pos):
        i = self.table3(ws[pos])
        if i >= 0:
            return i >> 24, i & 0xffffff, 1
        p = -i
        rest = ws[pos + 1:]
        ex = self.extrawc
        while True:
            idx, ln = ex[p], ex[p + 1]
            if idx >= 0:
                if list(rest[:ln]) == list(ex[p + 2:p + 2 + ln]):
                    return idx >> 24, idx & 0xffffff, 1 + ln
                p += 2 + ln
            else:
                lo, hi = ex[p + 2:p + 2 + ln], ex[p + 2 + ln:p + 2 + 2 * ln]
                cand = list(rest[:ln])
                if len(cand) == ln and list(lo) <= cand <= list(hi):
                    off = 0
                    for k in range(ln):
                        off = off * (hi[k] - lo[k] + 1) + (cand[k] - lo[k])
                    v = self.indirectwc[-idx + off]
                    return v >> 24, v & 0xffffff, 1 + ln
                p += 2 + 2 * ln

    def levels_wc(self, off):
        out = []
        for _ in range(self.nrules):
            n = self.wwc[off]
            out.append(list(self.wwc[off + 1:off + 1 + n]))
            off += 1 + n
        return out

    def elements_wc(self, ws):
        pos, out = 0, []
        while pos < len(ws):
            rs, off, n = self.find_wc(ws, pos)
            out.append((ws[pos:pos + n], rs, self.levels_wc(off)))
            pos += n
        return out

    def find(self, s, pos):
        """(ruleset, weight offset, bytes consumed) of the element at s[pos]."""
        i = self.table[s[pos]]
        if i >= 0:
            return i >> 24, i & 0xffffff, 1
        p = -i
        rest = s[pos + 1:]
        while True:
            idx, ln = struct.unpack_from('<iB', self.extra, p)
            if idx >= 0:
                seq = self.extra[p + 5:p + 5 + ln]
                if rest[:ln] == seq:
                    return idx >> 24, idx & 0xffffff, 1 + ln
                p += 5 + ln
            else:
                lo = self.extra[p + 5:p + 5 + ln]
                hi = self.extra[p + 5 + ln:p + 5 + 2 * ln]
                cand = rest[:ln]
                if len(cand) == ln and lo <= cand <= hi:
                    # mixed-radix offset of cand within [lo, hi], byte by byte
                    off = 0
                    for k in range(ln):
                        off = off * (hi[k] - lo[k] + 1) + (cand[k] - lo[k])
                    v = struct.unpack_from('<i', self.indirect, 4 * (-idx + off))[0]
                    return v >> 24, v & 0xffffff, 1 + ln
                p += 5 + 2 * ln
            p = (p + 3) & ~3

    def levels(self, off):
        """The weights of one element, per level: a list of byte strings."""
        out = []
        for _ in range(self.nrules):
            n = self.weights[off]
            out.append(self.weights[off + 1:off + 1 + n])
            off += 1 + n
        return out

    def elements(self, s):
        pos, out = 0, []
        while pos < len(s):
            rs, off, n = self.find(s, pos)
            out.append((s[pos:pos + n], rs, self.levels(off)))
            pos += n
        return out


FORWARD, BACKWARD, POSITION = 1, 2, 4


def xfrm(t, s):
    """The strxfrm key of byte string s."""
    if not t.nrules:
        return bytes(s)  # no rules: byte order (C, C.UTF-8)
    if not s:
        return b''
    return bytes(assemble(t, t.elements(s)))


def wxfrm(t, ws):
    """The wcsxfrm key (a list of wide units) of code point list ws."""
    if not t.nrules:
        return list(ws)
    if not ws:
        return []
    return assemble(t, t.elements_wc(ws))


def assemble(t, els):
    """The key from elements (sequence, ruleset, per-level weights), in units (bytes or wide)."""
    out = []
    for level in range(t.nrules):
        rule = lambda e: t.rulesets[e[1] * t.nrules + level]
        # Runs of consecutive elements whose rule at this level is backward are reversed.
        order, k = [], 0
        while k < len(els):
            if rule(els[k]) & BACKWARD:
                j = k
                while j < len(els) and rule(els[j]) & BACKWARD:
                    j += 1
                order.extend(reversed(els[k:j]))
                k = j
            else:
                order.append(els[k])
                k += 1
        start, skipped, position = len(out), 0, False
        for e in order:
            w = e[2][level]
            if rule(e) & POSITION:
                position = True
                # An element without weight here emits nothing; the next weight's separator
                # counts it (1 + elements skipped).
                if not w:
                    skipped += 1
                    continue
                out.append(1 + skipped)
                skipped = 0
            out += list(w)
        # Each level ends with a separator, except an empty position level.
        if not (position and len(out) == start):
            out.append(1)
    # The last separator is dropped.
    if out and out[-1] == 1:
        del out[-1]
    return out


def main():
    t = Table(sys.argv[1])
    if sys.argv[2] == 'xfrm':
        for line in sys.stdin.buffer:
            print(xfrm(t, line.rstrip(b'\n')).hex())
        return
    if sys.argv[2] == 'wxfrm':  # input UTF-8 lines; output as the wcsxfrm oracle prints
        for line in sys.stdin.buffer:
            try:
                ws = [ord(c) for c in line.rstrip(b'\n').decode('utf-8')]
            except UnicodeDecodeError:
                print('invalid')
                continue
            print(','.join('%x' % u for u in wxfrm(t, ws)))
        return
    if sys.argv[2] == 'elements':
        for arg in sys.argv[3:]:
            print(repr(arg))
            for seq, rs, lv in t.elements(arg.encode()):
                print('   %-10s rs%d %s' % (seq.hex(), rs, ' | '.join(x.hex() for x in lv)))


if __name__ == '__main__':
    main()
