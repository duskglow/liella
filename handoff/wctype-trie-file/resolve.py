#!/usr/bin/env python3
"""Map perf page-fault addresses (between the two marker faults) to symbols and sections.
usage: resolve.py <nm -n -S output> <readelf -SW output> <perf script output> <mark_a> <mark_b>"""
import bisect, re, sys

syms = []
for line in open(sys.argv[1]):
    p = line.split()
    if len(p) >= 3 and re.fullmatch(r'[0-9a-f]+', p[0]):
        name = p[-1]
        syms.append((int(p[0], 16), name))
syms.sort()
keys = [a for a, _ in syms]
secs = []
for line in open(sys.argv[2]):
    m = re.search(r'\]\s+(\S+)\s+\S+\s+([0-9a-f]{16})\s+[0-9a-f]+\s+([0-9a-f]+)', line)
    if m and int(m.group(2), 16):
        secs.append((int(m.group(2), 16), int(m.group(2), 16) + int(m.group(3), 16), m.group(1)))
ma, mb = int(sys.argv[4], 16) & ~0xfff, int(sys.argv[5], 16) & ~0xfff
inside = False
n = 0
for line in open(sys.argv[3]):
    m = re.search(r'\b([0-9a-f]{6,16})\s+([0-9a-f]{6,16})', line)
    if not m:
        continue
    addr, ip = int(m.group(1), 16), int(m.group(2), 16)
    if addr & ~0xfff == ma:
        inside = True
        continue
    if addr & ~0xfff == mb:
        break
    if not inside:
        continue
    n += 1
    def sym(x):
        i = bisect.bisect_right(keys, x) - 1
        return f'{syms[i][1]}+{x - syms[i][0]:#x}' if i >= 0 else '?'
    sec = next((s for lo, hi, s in secs if lo <= addr < hi), 'anon/stack/heap')
    print(f'{n:2} addr {addr:#x} [{sec}] {sym(addr)[:70]}  ip {sym(ip)[:70]}')
print(f'faults between markers: {n}')
