#!/usr/bin/env python3
"""Hot-text keys (PAGEFAULTS-2): function names -> build-stable keys, one per line, unique.

usage: keys.py <names file>... > keys              sorted
       keys.py --ordered <names file>... > keys    first occurrence order (files in argument order)
The same normalisation is in xtask/src/hottext.rs (`key`); keep the two in step:
  1. drop the library's own prefix `__liella_libc_` (D-X.14 renames; the section keeps the C name);
  2. drop ThinLTO promotion suffixes `.llvm.<digits>`;
  3. v0 mangling: drop crate disambiguators (`Cs<base62>_` -> `C`), which change with every build
     and with the checkout's path, and expand every back-reference (`B<base62>_`, a byte offset
     that moves when a disambiguator changes length: HOTTEXT-FAULT-ENV); a name that does not
     parse keeps the plain strip;
  4. legacy mangling: drop the hash (`17h<16 hex>E` -> `E`).
--ordered (HOT-ORDER) keeps the tracer's first-touch order: the hot-text list is a layout order.
"""
import re
import sys

OWN = "__liella_libc_"


class _Bad(Exception):
    pass


class _V0:
    """The v0 walker (xtask/src/hottext.rs `V0`): each method parses one production at byte i
    (offset after `_R`, the base of back-references), appends its canonical form, returns the end."""

    def __init__(self, b):
        self.b, self.out, self.depth = b, [], 0

    def at(self, i):
        if i >= len(self.b):
            raise _Bad
        return self.b[i]

    def decimal(self, i):
        j = i
        while j < len(self.b) and self.b[j].isdigit():
            j += 1
        if j == i:
            raise _Bad
        return int(self.b[i:j]), j

    def base62(self, i):
        if self.at(i) == "_":
            return 0, i + 1
        v = 0
        while True:
            c = self.at(i)
            i += 1
            if c == "_":
                return v + 1, i
            if "0" <= c <= "9":
                d = ord(c) - 48
            elif "a" <= c <= "z":
                d = ord(c) - 87
            elif "A" <= c <= "Z":
                d = ord(c) - 29
            else:
                raise _Bad
            v = v * 62 + d

    def copy(self, i, j):
        if j > len(self.b):
            raise _Bad
        self.out.append(self.b[i:j])
        return j

    def backref(self, i, f):
        target, end = self.base62(i + 1)
        if target >= i or self.depth > 64:
            raise _Bad
        self.depth += 1
        f(target)
        self.depth -= 1
        return end

    def ident(self, i):
        j = i + 1 if self.at(i) == "u" else i
        n, k = self.decimal(j)
        if k < len(self.b) and self.b[k] == "_":
            k += 1
        return self.copy(i, k + n)

    def disambiguated(self, i, keep):
        if self.at(i) == "s":
            end = self.base62(i + 1)[1]
            if keep:
                self.copy(i, end)
            i = end
        return self.ident(i)

    def impl_path(self, i):
        if self.at(i) == "s":
            i = self.copy(i, self.base62(i + 1)[1])
        return self.path(i)

    def path(self, i):
        c, o = self.at(i), self.out
        if c == "C":
            o.append("C")
            return self.disambiguated(i + 1, False)
        if c == "M":
            o.append("M")
            return self.ty(self.impl_path(i + 1))
        if c == "X":
            o.append("X")
            return self.path(self.ty(self.impl_path(i + 1)))
        if c == "Y":
            o.append("Y")
            return self.path(self.ty(i + 1))
        if c == "N":
            ns = self.at(i + 1)
            if not (ns.isascii() and ns.isalpha()):
                raise _Bad
            o.append("N" + ns)
            return self.disambiguated(self.path(i + 2), True)
        if c == "I":
            o.append("I")
            j = self.path(i + 1)
            while self.at(j) != "E":
                j = self.generic_arg(j)
            o.append("E")
            return j + 1
        if c == "B":
            return self.backref(i, self.path)
        raise _Bad

    def generic_arg(self, i):
        c = self.at(i)
        if c == "L":
            return self.copy(i, self.base62(i + 1)[1])
        if c == "K":
            self.out.append("K")
            return self.konst(i + 1)
        return self.ty(i)

    def konst(self, i):
        c = self.at(i)
        if c == "p":
            return self.copy(i, i + 1)
        if c == "B":
            return self.backref(i, self.konst)
        if "a" <= c <= "z":
            j = i + 1
            if self.at(j) == "n":
                j += 1
            while self.at(j) in "0123456789abcdefABCDEF":
                j += 1
            if self.at(j) != "_":
                raise _Bad
            return self.copy(i, j + 1)
        raise _Bad

    def opt(self, i, tag):
        return self.copy(i, self.base62(i + 1)[1]) if self.at(i) == tag else i

    def ty(self, i):
        c, o = self.at(i), self.out
        if c in "abcdefhijlmnopstuvxyz":
            return self.copy(i, i + 1)
        if c == "A":
            o.append("A")
            return self.konst(self.ty(i + 1))
        if c in "SPO":
            o.append(c)
            return self.ty(i + 1)
        if c in "RQ":
            o.append(c)
            return self.ty(self.opt(i + 1, "L"))
        if c == "T":
            o.append("T")
            j = i + 1
            while self.at(j) != "E":
                j = self.ty(j)
            o.append("E")
            return j + 1
        if c == "F":
            o.append("F")
            j = self.opt(i + 1, "G")
            if self.at(j) == "U":
                j = self.copy(j, j + 1)
            if self.at(j) == "K":
                o.append("K")
                j = self.copy(j + 1, j + 2) if self.at(j + 1) == "C" else self.ident(j + 1)
            while self.at(j) != "E":
                j = self.ty(j)
            o.append("E")
            return self.ty(j + 1)
        if c == "D":
            o.append("D")
            j = self.opt(i + 1, "G")
            while self.at(j) != "E":
                j = self.path(j)
                while self.at(j) == "p":
                    o.append("p")
                    j = self.ty(self.ident(j + 1))
            o.append("E")
            return self.opt(j + 1, "L")
        if c == "B":
            return self.backref(i, self.ty)
        return self.path(i)


def canon_v0(s):
    """A v0 symbol without crate-root disambiguators, back-references expanded; None if it does not parse."""
    if not s.startswith("_R"):
        return None
    d = _V0(s[2:])
    try:
        i = 0
        if d.b[:1].isdigit():
            i = d.decimal(0)[1]
            d.out.append(d.b[:i])
        i = d.path(i)
        if i < len(d.b) and d.b[i] != ".":
            i = d.path(i)
    except (_Bad, RecursionError):
        return None
    return "_R" + "".join(d.out) + d.b[i:]


def key(name):
    if name.startswith(OWN):
        name = name[len(OWN):]
    name = re.sub(r"\.llvm\.[0-9]+", "", name)
    if name.startswith("_R"):
        name = canon_v0(name) or re.sub(r"Cs[0-9A-Za-z]+_", "C", name)
    if name.startswith("_ZN"):
        name = re.sub(r"17h[0-9a-f]{16}E", "E", name)
    return name


def main():
    args = sys.argv[1:]
    ordered = bool(args) and args[0] == "--ordered"
    if ordered:
        args = args[1:]
    keys = {}
    for path in args:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line and line != "main":
                    keys.setdefault(key(line), None)
    for k in (keys if ordered else sorted(keys)):
        print(k)


if __name__ == "__main__":
    main()
