#!/usr/bin/env python3
"""Extract the x86_64 column of the GNU C Library manual's "Errors in Math Functions" table.

Usage: libm_ulps.py <Errors-in-Math-Functions.html> > spec/abi/libm-ulps.tsv

The page is official documentation (implementation-plan.md section 1.2): it lists, per function
and floating type, the largest error in ULPs that glibc's test suite measured on each
architecture. The table is split into several HTML tables of a few architectures each; the
column headed "x86_64" is the one for this ABI. "-" means no error was measured (the function
is correctly rounded on the tested inputs). Complex functions give "re + i im".

Output columns: function, type suffix (none, f, l, f128), ulps (as printed).
"""
import html
import re
import sys


def cells(row):
    return [html.unescape(re.sub(r"<[^>]+>", "", c)).strip() for c in re.findall(r"<td[^>]*>(.*?)</td>", row, re.S)]


def main(path):
    text = open(path, encoding="utf-8").read()
    out = {}
    order = []
    for table in re.findall(r"<table[^>]*>(.*?)</table>", text, re.S):
        rows = [cells(r) for r in re.findall(r"<tr[^>]*>(.*?)</tr>", table, re.S)]
        if not rows or not rows[0] or rows[0][0] != "Function":
            continue
        header = rows[0]
        if "x86_64" not in header:
            continue
        col = header.index("x86_64")
        for r in rows[1:]:
            if len(r) > col:
                if r[0] not in out:
                    order.append(r[0])
                out[r[0]] = r[col]
    print("function\ttype\tulps")
    for name in order:
        if name.endswith(("_advsimd", "_sve")):
            continue  # AArch64 vector variants: no x86_64 entry
        base, suffix = name, ""
        for s in ("f128", "f", "l"):
            # The table lists each function as base+f, base, base+l, base+f128 in a group.
            if name.endswith(s) and name[: -len(s)] in out:
                base, suffix = name[: -len(s)], s
                break
        print(f"{base}\t{suffix or '-'}\t{out[name]}")


if __name__ == "__main__":
    main(sys.argv[1])
