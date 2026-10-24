#!/usr/bin/env python3
"""Independent reference matcher for the owner's submatch hypothesis, run against the glibc record.

Hypothesis (owner's wording): the overall match is leftmost-longest; the submatches are chosen by
alternation priority among the paths that reach that same end.

This script is written from that definition only (no regex engine source was consulted). It parses
the POSIX BRE/ERE subset that the probe crates/core/tests/oracle/patterns_diff.c generates, plus the
constructs of the hand cases in the record's "-- matching" block, and compares its pmatch vectors
with spec/behaviour/patterns.glibc.txt.gz.

Definition implemented (see spec/behaviour/regex-submatch-analysis.md for the discussion):

* A *path* is one way the pattern consumes subject[s:e] (a derivation through the pattern).
* Start s: the smallest offset from which some path exists. End e: the largest end of a path from s.
* Priority order on paths = depth-first order in which
    - alternatives are tried left to right (earlier alternative first);
    - a repetition X{m,n} (also *, +, ?) tries one more iteration before stopping (greedy-first),
      i.e. more iterations are preferred at the first point where two paths differ;
    - concatenation is left to right, so an earlier choice dominates later ones.
  The chosen path is the first path in this order that starts at s and ends at e.
* Empty iterations (the construct the hypothesis leaves open), four named variants. In all of them
  iterations needed to reach the minimum count may be empty, and greedy-first order is kept:
    stop   an iteration that matches the empty string is allowed, but no further iteration follows
           it; this is the default.
    never  an iteration beyond the minimum count that matches the empty string is not allowed.
    first  beyond the minimum, an empty iteration is allowed only as the first iteration (when the
           loop would otherwise iterate zero times: "a null string is longer than no match"), and
           no iteration follows it.
    lead   as first, but further (non-empty) iterations may follow the leading empty iteration.
* Diagnostic only (not part of the hypothesis): --swap tries the second branch of an alternation
  before an empty first branch; used to classify residuals.
* Groups nested in a repeated group, two named variants:
    keep   captures are writes along the path, last write wins: an inner group not taken in the last
           iteration keeps its value from an earlier iteration (default).
    reset  each new iteration of a repeated expression resets the groups inside it to unset (-1).
* Reported offsets: pmatch[0] = (s, e); pmatch[i] = the last write of group i on the chosen path, or
  (-1, -1) if never written (or reset). A back-reference \\n matches the text group n holds at that
  point of the path; it fails if group n is unset.

Usage:
    python spec/tools/regex_submatch_hypothesis.py [--record PATH] [--examples N] [--json OUT]

Deterministic: the output depends only on the record. Python 3, standard library only.
"""

from __future__ import annotations

import argparse
import gzip
import json
import os
import re
import sys
from collections import defaultdict

sys.setrecursionlimit(20000)

EMPTY_VARIANTS = ("stop", "never", "first", "lead")
CARRY_VARIANTS = ("keep", "reset")
VARIANTS = [(e, c) for e in EMPTY_VARIANTS for c in CARRY_VARIANTS]
DEFAULT = ("stop", "keep")

# ----------------------------------------------------------------------------- pattern AST
# Nodes are tuples: ("chr", pred) ("assert", kind) ("bref", n) ("group", n, child)
# ("cat", [children]) ("alt", [children]) ("rep", child, min, max_or_None, inner_groups)


class ParseError(Exception):
    pass


class Unsupported(Exception):
    pass


def is_word(c):
    return c.isalnum() and ord(c) < 128 or c == "_"


CLASSES = {
    "alpha": lambda c: c.isascii() and c.isalpha(),
    "digit": lambda c: "0" <= c <= "9",
    "alnum": lambda c: c.isascii() and c.isalnum(),
    "upper": lambda c: "A" <= c <= "Z",
    "lower": lambda c: "a" <= c <= "z",
    "space": lambda c: c in " \t\n\v\f\r",
    "blank": lambda c: c in " \t",
    "punct": lambda c: 0x21 <= ord(c) <= 0x7E and not c.isalnum(),
    "print": lambda c: 0x20 <= ord(c) <= 0x7E,
    "graph": lambda c: 0x21 <= ord(c) <= 0x7E,
    "cntrl": lambda c: ord(c) < 0x20 or ord(c) == 0x7F,
    "xdigit": lambda c: c in "0123456789abcdefABCDEF",
}


def fold_pair(c):
    return {c, c.lower(), c.upper()} if c.isascii() else {c}


class Parser:
    def __init__(self, pat, ere, icase, newline):
        self.p = pat
        self.i = 0
        self.ere = ere
        self.icase = icase
        self.newline = newline
        self.ngroups = 0
        self.closed = set()
        self.open_depth = 0

    def peek(self, k=0):
        j = self.i + k
        return self.p[j] if j < len(self.p) else None

    # literal and bracket predicates -------------------------------------------------
    def lit(self, ch):
        if self.icase:
            s = fold_pair(ch)
            return ("chr", lambda c, s=s: c in s)
        return ("chr", lambda c, ch=ch: c == ch)

    def dot(self):
        if self.newline:
            return ("chr", lambda c: c != "\n")
        return ("chr", lambda c: True)

    def bracket(self):
        # self.i is just past '['
        neg = False
        if self.peek() == "^":
            neg = True
            self.i += 1
        items = set()
        preds = []
        first = True

        def endpoint():
            c = self.peek()
            if c is None:
                raise ParseError("EBRACK")
            if c == "[" and self.peek(1) in (".", "="):
                kind = self.peek(1)
                j = self.p.find(kind + "]", self.i + 2)
                if j < 0:
                    raise ParseError("EBRACK")
                name = self.p[self.i + 2 : j]
                if len(name) != 1:
                    raise ParseError("ECOLLATE")
                self.i = j + 2
                return name, kind
            self.i += 1
            return c, None

        while True:
            c = self.peek()
            if c is None:
                raise ParseError("EBRACK")
            if c == "]" and not first:
                self.i += 1
                break
            first = False
            if c == "[" and self.peek(1) == ":":
                j = self.p.find(":]", self.i + 2)
                if j < 0:
                    raise ParseError("EBRACK")
                name = self.p[self.i + 2 : j]
                if name not in CLASSES:
                    raise ParseError("ECTYPE")
                preds.append(CLASSES[name])
                self.i = j + 2
                if self.peek() == "-" and self.peek(1) != "]":
                    raise ParseError("ERANGE")
                continue
            lo, kind = endpoint()
            if self.peek() == "-" and self.peek(1) not in ("]", None) and kind != "=":
                self.i += 1
                if self.peek() == "[" and self.peek(1) == ":":
                    raise ParseError("ERANGE")
                hi, _ = endpoint()
                if ord(hi) < ord(lo):
                    raise ParseError("ERANGE")
                for o in range(ord(lo), ord(hi) + 1):
                    items.add(chr(o))
            else:
                items.add(lo)

        def base(c, items=items, preds=preds):
            return c in items or any(f(c) for f in preds)

        if self.icase:
            def inset(c, base=base):
                return any(base(x) for x in fold_pair(c))
        else:
            inset = base
        if neg:
            nl = self.newline
            return ("chr", lambda c, inset=inset, nl=nl: not inset(c) and not (nl and c == "\n"))
        return ("chr", inset)

    # escapes shared by both syntaxes ---------------------------------------------
    def gnu_escape(self, c):
        if c in "123456789":
            n = int(c)
            if n > self.ngroups or n not in self.closed:
                raise ParseError("ESUBREG")
            return ("bref", n)
        table = {
            "w": ("chr", is_word),
            "W": ("chr", lambda ch: not is_word(ch)),
            "s": ("chr", lambda ch: ch in " \t\n\v\f\r"),
            "S": ("chr", lambda ch: ch not in " \t\n\v\f\r"),
            "b": ("assert", "wordb"),
            "B": ("assert", "notwordb"),
            "<": ("assert", "wordstart"),
            ">": ("assert", "wordend"),
            "`": ("assert", "bufstart"),
            "'": ("assert", "bufend"),
        }
        if c in table:
            return table[c]
        return self.lit(c)

    # ERE -------------------------------------------------------------------------
    def parse(self):
        node = self.ere_alt() if self.ere else self.bre_alt(top=True)
        if self.i != len(self.p):
            raise ParseError("trailing")
        return node, self.ngroups

    def ere_alt(self):
        branches = [self.ere_cat()]
        while self.peek() == "|":
            self.i += 1
            branches.append(self.ere_cat())
        return branches[0] if len(branches) == 1 else ("alt", branches)

    def ere_cat(self):
        items = []
        while True:
            c = self.peek()
            if c is None or c == "|" or (c == ")" and self.open_depth > 0):
                break
            atom = self.ere_atom()
            atom = self.postfix(atom, ere=True)
            items.append(atom)
        return ("cat", items)

    def ere_atom(self):
        c = self.peek()
        self.i += 1
        if c == "(":
            self.ngroups += 1
            n = self.ngroups
            self.open_depth += 1
            child = self.ere_alt()
            if self.peek() != ")":
                raise ParseError("EPAREN")
            self.i += 1
            self.open_depth -= 1
            self.closed.add(n)
            return ("group", n, child)
        if c in "*+?{":
            raise ParseError("BADRPT")
        if c == ".":
            return self.dot()
        if c == "[":
            return self.bracket()
        if c == "^":
            return ("assert", "bol")
        if c == "$":
            return ("assert", "eol")
        if c == "\\":
            d = self.peek()
            if d is None:
                raise ParseError("EESCAPE")
            self.i += 1
            return self.gnu_escape(d)
        return self.lit(c)

    def interval(self, close):
        # self.i is past the opening brace
        m = re.match(r"(\d*)(,(\d*))?", self.p[self.i :])
        lo, comma, hi = m.group(1), m.group(2), m.group(3)
        self.i += m.end()
        if not self.p.startswith(close, self.i):
            raise ParseError("EBRACE")
        self.i += len(close)
        mn = int(lo) if lo else 0
        if comma is None:
            if not lo:
                raise ParseError("BADBR")
            mx = mn
        else:
            mx = int(hi) if hi else None
        if mx is not None and mx < mn:
            raise ParseError("BADBR")
        return mn, mx

    def postfix(self, atom, ere):
        while True:
            c = self.peek()
            if ere:
                if c == "*":
                    self.i += 1
                    atom = mkrep(atom, 0, None)
                elif c == "+":
                    self.i += 1
                    atom = mkrep(atom, 1, None)
                elif c == "?":
                    self.i += 1
                    atom = mkrep(atom, 0, 1)
                elif c == "{":
                    self.i += 1
                    mn, mx = self.interval("}")
                    atom = mkrep(atom, mn, mx)
                else:
                    return atom
            else:
                if c == "*":
                    self.i += 1
                    atom = mkrep(atom, 0, None)
                elif c == "\\" and self.peek(1) == "+":
                    self.i += 2
                    atom = mkrep(atom, 1, None)
                elif c == "\\" and self.peek(1) == "?":
                    self.i += 2
                    atom = mkrep(atom, 0, 1)
                elif c == "\\" and self.peek(1) == "{":
                    self.i += 2
                    mn, mx = self.interval("\\}")
                    atom = mkrep(atom, mn, mx)
                else:
                    return atom

    # BRE -------------------------------------------------------------------------
    def bre_alt(self, top=False):
        branches = [self.bre_cat()]
        while self.p.startswith("\\|", self.i):
            self.i += 2
            branches.append(self.bre_cat())
        return branches[0] if len(branches) == 1 else ("alt", branches)

    def bre_cat(self):
        items = []
        start = True  # at the start of the RE, of a group, or of a \| branch
        while True:
            c = self.peek()
            if c is None:
                break
            if c == "\\" and self.peek(1) in (")", "|"):
                if self.peek(1) == ")" and self.open_depth == 0:
                    raise ParseError("EPAREN")
                break
            if c == "^" and start:
                self.i += 1
                items.append(("assert", "bol"))
                # '*' right after a leading '^' is literal
                if self.peek() == "*":
                    self.i += 1
                    items.append(self.postfix(self.lit("*"), ere=False))
                start = False
                continue
            if c == "*" and start:
                self.i += 1
                items.append(self.postfix(self.lit("*"), ere=False))
                start = False
                continue
            start = False
            if c == "$":
                rest = self.p[self.i + 1 :]
                if rest == "" or rest.startswith("\\)") or rest.startswith("\\|"):
                    self.i += 1
                    items.append(("assert", "eol"))
                    continue
                self.i += 1
                atom = self.lit("$")
            elif c == "\\":
                d = self.peek(1)
                if d is None:
                    raise ParseError("EESCAPE")
                self.i += 2
                if d == "(":
                    self.ngroups += 1
                    n = self.ngroups
                    self.open_depth += 1
                    child = self.bre_alt()
                    if not self.p.startswith("\\)", self.i):
                        raise ParseError("EPAREN")
                    self.i += 2
                    self.open_depth -= 1
                    self.closed.add(n)
                    atom = ("group", n, child)
                elif d == "{":
                    raise ParseError("BADRPT")
                else:
                    atom = self.gnu_escape(d)
            elif c == ".":
                self.i += 1
                atom = self.dot()
            elif c == "[":
                self.i += 1
                atom = self.bracket()
            else:
                self.i += 1
                atom = self.lit(c)
            atom = self.postfix(atom, ere=False)
            items.append(atom)
        return ("cat", items)


def groups_in(node):
    t = node[0]
    if t == "group":
        return [node[1]] + groups_in(node[2])
    if t in ("cat", "alt"):
        out = []
        for ch in node[1]:
            out += groups_in(ch)
        return out
    if t == "rep":
        return groups_in(node[1])
    return []


def mkrep(atom, mn, mx):
    return ("rep", atom, mn, mx, tuple(groups_in(atom)))


def compile_pattern(pat, flags):
    ere = "E" in flags
    p = Parser(pat, ere, "I" in flags, "N" in flags)
    node, ngroups = p.parse()
    return node, ngroups


# ----------------------------------------------------------------------------- matcher


class Budget(Exception):
    pass


class Matcher:
    def __init__(self, node, ngroups, flags, empty, carry, notbol=False, noteol=False, budget=2_000_000,
                 swap_empty_first=False):
        self.swap = swap_empty_first
        self.node = node
        self.ng = ngroups
        self.icase = "I" in flags
        self.newline = "N" in flags
        self.empty = empty
        self.carry = carry
        self.notbol = notbol
        self.noteol = noteol
        self.budget = budget
        self.steps = 0

    def tick(self):
        self.steps += 1
        if self.steps > self.budget:
            raise Budget()

    def assert_ok(self, kind, s, i):
        n = len(s)
        prev = s[i - 1] if i > 0 else None
        nxt = s[i] if i < n else None
        if kind == "bol":
            if i == 0:
                return not self.notbol
            return self.newline and prev == "\n"
        if kind == "eol":
            if i == n:
                return not self.noteol
            return self.newline and nxt == "\n"
        pw = prev is not None and is_word(prev)
        nw = nxt is not None and is_word(nxt)
        if kind == "wordb":
            return pw != nw
        if kind == "notwordb":
            return pw == nw
        if kind == "wordstart":
            return nw and not pw
        if kind == "wordend":
            return pw and not nw
        if kind == "bufstart":
            return i == 0
        if kind == "bufend":
            return i == n
        raise Unsupported(kind)

    # m(node, i, caps, k) yields (end, caps) for every path, in priority order
    def m(self, node, i, caps, k):
        self.tick()
        t = node[0]
        s = self.s
        if t == "chr":
            if i < len(s) and node[1](s[i]):
                yield from k(i + 1, caps)
        elif t == "assert":
            if self.assert_ok(node[1], s, i):
                yield from k(i, caps)
        elif t == "bref":
            so, eo = caps[node[1]]
            if so < 0:
                return
            sub = s[so:eo]
            cand = s[i : i + len(sub)]
            if len(cand) == len(sub) and (cand == sub or (self.icase and cand.lower() == sub.lower())):
                yield from k(i + len(sub), caps)
        elif t == "group":
            n = node[1]

            def after(j, c, n=n, i=i):
                c2 = c[:n] + ((i, j),) + c[n + 1 :]
                yield from k(j, c2)

            yield from self.m(node[2], i, caps, after)
        elif t == "cat":
            yield from self.cat(node[1], 0, i, caps, k)
        elif t == "alt":
            brs = node[1]
            if self.swap and len(brs) >= 2 and brs[0] == ("cat", []):
                brs = [brs[1], brs[0]] + list(brs[2:])
            for br in brs:
                yield from self.m(br, i, caps, k)
        elif t == "rep":
            yield from self.rep(node, i, caps, k, 0)
        else:
            raise Unsupported(t)

    def cat(self, items, idx, i, caps, k):
        if idx == len(items):
            yield from k(i, caps)
            return

        def nxt(j, c):
            yield from self.cat(items, idx + 1, j, c, k)

        yield from self.m(items[idx], i, caps, nxt)

    def rep(self, node, i, caps, k, count):
        _, child, mn, mx, inner = node
        if mx is None or count < mx:
            c0 = caps
            if self.carry == "reset" and inner:
                c0 = list(caps)
                for g in inner:
                    c0[g] = (-1, -1)
                c0 = tuple(c0)

            def after(j, c):
                if j == i:  # this iteration matched the empty string
                    if count + 1 < mn:  # needed for the minimum: keep counting
                        yield from self.rep(node, j, c, k, count + 1)
                        return
                    allowed = {
                        "stop": True,
                        "never": count + 1 <= mn,
                        "first": count + 1 <= mn or count == 0,
                        "lead": count + 1 <= mn or count == 0,
                    }[self.empty]
                    if not allowed:
                        return
                    if self.empty == "lead":
                        yield from self.rep(node, j, c, k, count + 1)
                    else:
                        yield from k(j, c)  # no further iteration after an empty one
                    return
                yield from self.rep(node, j, c, k, count + 1)

            yield from self.m(child, i, c0, after)
        if count >= mn:
            yield from k(i, caps)

    def search(self, s, nmatch):
        """Return None (no match) or the pmatch list of nmatch (so, eo) pairs."""
        self.s = s
        empty = tuple([(-1, -1)] * (self.ng + 1))

        def done(j, c):
            yield (j, c)

        for start in range(len(s) + 1):
            paths = list(self.m(self.node, start, empty, done))
            if not paths:
                continue
            e = max(p[0] for p in paths)
            for j, c in paths:
                if j == e:
                    vec = [(start, e)] + list(c[1:])
                    vec += [(-1, -1)] * max(0, nmatch - len(vec))
                    return vec[:nmatch], paths, start, e
        return None

    def all_vectors(self, s, start, e, nmatch):
        """Every capture vector of a path from start to e (for realisability checks)."""
        self.s = s
        empty = tuple([(-1, -1)] * (self.ng + 1))

        def done(j, c):
            yield (j, c)

        out = []
        for j, c in self.m(self.node, start, empty, done):
            if j == e:
                vec = [(start, e)] + list(c[1:])
                vec += [(-1, -1)] * max(0, nmatch - len(vec))
                out.append(tuple(vec[:nmatch]))
        return out


# ----------------------------------------------------------------------------- record parsing


def unesc(s):
    return re.sub(r"\\x([0-9a-f]{2})", lambda m: chr(int(m.group(1), 16)), s)


def show(s):
    out = []
    for c in s:
        if c == "\\" or ord(c) < 0x20 or ord(c) >= 0x7F:
            out.append("\\x%02x" % ord(c) if c != "\\" else "\\")
        else:
            out.append(c)
    return "".join(out)


CORPUS_NAMES = {
    "-- ERE: tokens": "ERE-1 (a b ( ) | * + ?; <=5 tokens)",
    "-- ERE with {m,n}": "ERE-2 (a b ( ) | {0,1} {2} \\1; <=4 tokens)",
    "-- BRE: tokens": "BRE (a b \\( \\) * \\1; <=5 tokens)",
    "-- ERE REG_ICASE": "ERE-ICASE (a B ( ) | * \\1; <=4 tokens)",
}


def load_record(path):
    with gzip.open(path, "rt", encoding="latin-1", newline="\n") as f:
        lines = f.read().split("\n")
    corpus = []  # (corpus, flags, pattern, nsub, subject, glibc_vec_or_None)
    hand = []  # (flags, pattern, subject, eflags, glibc result)
    section = None
    sub = None
    subjects = []
    cname = None
    for line in lines:
        if line.startswith("== "):
            section = line[3:]
            sub = None
            continue
        if line.startswith("-- "):
            sub = line
            for k, v in CORPUS_NAMES.items():
                if line.startswith(k):
                    cname = v
            continue
        if section == "regex corpus":
            if line.startswith("subjects"):
                subjects = [unesc(x[1:-1]) for x in line.split(" ")[1:]]
                continue
            if line.startswith("rc "):
                m = re.match(r"^rc (\S+) \[(.*)\] (\d+)(.*)$", line)
                flags, pat, code, rest = m.group(1), unesc(m.group(2)), int(m.group(3)), m.group(4).split()
                if code != 0:
                    continue
                nsub = int(rest[0])
                res = rest[1:]
                assert len(res) == len(subjects), line
                for subj, r in zip(subjects, res):
                    if r == "-":
                        vec = None
                    else:
                        vec = tuple(tuple(int(x) for x in pr.split(".")) for pr in r.split(","))
                    corpus.append((cname, flags, pat, nsub, subj, vec))
        elif section == "regex" and sub == "-- matching" and line.startswith("re "):
            head, _, result = line.partition(" -> ")
            parts = head.split(" ", 2)
            flags = parts[1]
            body, _, eflags = parts[2].rpartition(" ")
            k = body.rfind("] [")
            pat, subj = unesc(body[1:k]), unesc(body[k + 3 : -1])
            hand.append((flags, pat, subj, eflags, result))
    return corpus, hand


def parse_hand_result(result):
    if result.startswith("regcomp="):
        return "regcomp", None, None
    m = re.match(r"^(\d+) nsub=(\d+) ?(.*)$", result)
    rc, nsub, rest = int(m.group(1)), int(m.group(2)), m.group(3)
    if rc != 0:
        return "nomatch", nsub, None
    vec = tuple((int(a), int(b)) for a, b in re.findall(r"\((-?\d+),(-?\d+)\)", rest))
    return "match", nsub, vec


# ----------------------------------------------------------------------------- comparison


def fmt_vec(v):
    if v is None:
        return "no match"
    return "".join("(%d,%d)" % p for p in v)


class Case:
    __slots__ = ("corpus", "flags", "pat", "subj", "nsub", "glibc", "eflags")

    def __init__(self, corpus, flags, pat, subj, nsub, glibc, eflags=""):
        self.corpus, self.flags, self.pat, self.subj = corpus, flags, pat, subj
        self.nsub, self.glibc, self.eflags = nsub, glibc, eflags


_compiled = {}


def get_compiled(flags, pat):
    key = (flags, pat)
    if key not in _compiled:
        try:
            _compiled[key] = compile_pattern(pat, flags)
        except ParseError as e:
            _compiled[key] = e
    return _compiled[key]


def run_case(case, empty, carry, swap=False):
    comp = get_compiled(case.flags, case.pat)
    if isinstance(comp, ParseError):
        return ("parse-error", str(comp))
    node, ng = comp
    nmatch = min(case.nsub + 1, 10)
    mt = Matcher(node, ng, case.flags, empty, carry, notbol="NOTBOL" in case.eflags,
                 noteol="NOTEOL" in case.eflags, swap_empty_first=swap)
    try:
        r = mt.search(case.subj, nmatch)
    except (Budget, RecursionError):
        return ("budget", None)
    if r is None:
        return ("ok", None)
    return ("ok", tuple(r[0]))


def realisable(case, empty, carry, start, e):
    node, ng = get_compiled(case.flags, case.pat)
    nmatch = min(case.nsub + 1, 10)
    mt = Matcher(node, ng, case.flags, empty, carry, notbol="NOTBOL" in case.eflags, noteol="NOTEOL" in case.eflags)
    try:
        vecs = mt.all_vectors(case.subj, start, e, nmatch)
    except (Budget, RecursionError):
        return None
    return case.glibc in vecs


# ----------------------------------------------------------------------------- classification

def node_has(node, pred):
    if pred(node):
        return True
    t = node[0]
    if t == "group":
        return node_has(node[2], pred)
    if t in ("cat", "alt"):
        return any(node_has(c, pred) for c in node[1])
    if t == "rep":
        return node_has(node[1], pred)
    return False


CATEGORY_TEXT = {
    "O1": "overall match (pmatch[0]) differs: back-reference to a group whose bounded repetition "
          "needs an empty iteration",
    "O2": "overall match (pmatch[0]) differs: other",
    "E1": "trailing empty iteration: the hypothesis adds a final empty iteration after a non-empty one, "
          "glibc does not (fixed by empty=first)",
    "E2": "leading empty iteration followed by non-empty iterations (fixed only by empty=lead)",
    "A1": "empty first alternative: glibc prefers the second alternative over an empty first one "
          "(fixed by the swap diagnostic)",
    "A1+E": "both A1 and an empty-iteration choice (fixed only by swap plus empty=first/lead)",
    "M1": "glibc's vector is produced by some path, but not by the first one under any variant",
    "N1": "glibc's vector is not produced by any path under any variant",
}


def classify(case, hyp, carry="keep"):
    """Category code for a residual of the default variant, using the variants as diagnostics."""
    g = case.glibc
    node, ng = get_compiled(case.flags, case.pat)
    if (g is None) != (hyp is None) or (g is not None and g[0] != hyp[0]):
        bounded_bref = node_has(node, lambda n: n[0] == "bref") and node_has(
            node, lambda n: n[0] == "rep" and n[2] >= 2)
        return "O1" if bounded_bref else "O2"

    def fixed(em, swap):
        return run_case(case, em, carry, swap)[1] == g

    if fixed("first", False):
        return "E1"
    if fixed("lead", False):
        return "E2"
    if fixed("stop", True):
        return "A1"
    if fixed("first", True) or fixed("lead", True):
        return "A1+E"
    start, e = g[0]
    for em in EMPTY_VARIANTS:
        for ca in CARRY_VARIANTS:
            if realisable(case, em, ca, start, e):
                return "M1"
    return "N1"


# ----------------------------------------------------------------------------- main


def pct(a, b):
    return 100.0 * a / max(b, 1)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--record", default=os.path.join(here, "..", "behaviour", "patterns.glibc.txt.gz"))
    ap.add_argument("--examples", type=int, default=3, help="examples per category (distinct patterns)")
    ap.add_argument("--json", help="also write the numbers and examples as JSON to this file")
    args = ap.parse_args()

    corpus_rows, hand_rows = load_record(args.record)
    cases = [Case(c, f, p, s, n, v) for (c, f, p, n, s, v) in corpus_rows]
    hand = []
    hand_excluded = 0
    for flags, pat, subj, eflags, result in hand_rows:
        kind, nsub, vec = parse_hand_result(result)
        if kind == "regcomp":
            hand_excluded += 1
            continue
        hand.append(Case("hand", flags, pat, subj, nsub, vec, "" if eflags == "-" else eflags))

    corpus_order = list(CORPUS_NAMES.values())
    npat = defaultdict(set)
    for c in cases:
        npat[c.corpus].add((c.flags, c.pat))

    report = {"variants": {}, "categories": {}, "keep_vs_reset": {}}
    print("record: spec/behaviour/patterns.glibc.txt.gz")
    for cn in corpus_order:
        print("  %-42s %5d compiled patterns" % (cn, len(npat[cn])))
    print("  corpus patterns total %d; corpus cases (pattern x subject) %d" % (
        sum(len(v) for v in npat.values()), len(cases)))
    print("  hand cases ('-- matching' block): %d evaluated, %d excluded (regcomp error in glibc)" % (
        len(hand), hand_excluded))

    results = {}
    allc = cases + hand
    for em, ca in VARIANTS:
        stats = defaultdict(lambda: [0, 0, 0, 0, 0])  # total, pm0 agree, full agree, parse errors, budget
        for idx, c in enumerate(allc):
            st, vec = run_case(c, em, ca)
            results[(em, ca, idx)] = (st, vec)
            row = stats[c.corpus]
            row[0] += 1
            if st == "parse-error":
                row[3] += 1
                continue
            if st == "budget":
                row[4] += 1
                continue
            g = c.glibc
            row[1] += (vec is None and g is None) or (vec is not None and g is not None and vec[0] == g[0])
            row[2] += vec == g
        name = "empty=%s carry=%s" % (em, ca)
        print()
        print("== %s%s" % (name, "  (default)" if (em, ca) == DEFAULT else ""))
        print("  %-42s %7s  %18s  %18s  %s" % ("corpus", "cases", "pmatch[0] agree", "full vector agree",
                                               "not evaluated"))
        tot = [0, 0, 0, 0, 0]
        report["variants"][name] = {}
        for cn in corpus_order + ["ALL CORPORA", "hand"]:
            r = tot if cn == "ALL CORPORA" else stats[cn]
            print("  %-42s %7d  %7d (%6.2f%%)  %7d (%6.2f%%)  %s" % (
                cn, r[0], r[1], pct(r[1], r[0]), r[2], pct(r[2], r[0]),
                ("budget %d" % r[4]) if r[4] else ("parse %d" % r[3]) if r[3] else "-"))
            report["variants"][name][cn] = {"cases": r[0], "pmatch0": r[1], "full": r[2], "budget": r[4],
                                            "parse_error": r[3]}
            if cn in corpus_order:
                for j in range(5):
                    tot[j] += r[j]

    # For comparison only (not the hypothesis): the POSIX subpattern reading. Among the paths from s to
    # e (path set of empty=stop carry=keep), take the vector that maximises, group by group in the order
    # of their opening parentheses: participation (a null string beats no match), then the earliest
    # start, then the longest length.
    print()
    print("== for comparison: POSIX subpattern reading (each subexpression, left to right, longest)")
    pstats = defaultdict(lambda: [0, 0, 0])
    pex = []
    for idx, c in enumerate(allc):
        st, vec = results[(DEFAULT[0], DEFAULT[1], idx)]
        if st != "ok":
            continue
        row = pstats[c.corpus]
        row[0] += 1
        if vec is None:
            row[1] += c.glibc is None
            continue
        node, ng = get_compiled(c.flags, c.pat)
        mt = Matcher(node, ng, c.flags, "stop", "keep", notbol="NOTBOL" in c.eflags, noteol="NOTEOL" in c.eflags)
        vecs = mt.all_vectors(c.subj, vec[0][0], vec[0][1], min(c.nsub + 1, 10))

        def key(v):
            return tuple(x for so, eo in v[1:] for x in ((0, 0, 0) if so < 0 else (1, -so, eo - so)))

        pv = max(vecs, key=key)
        row[1] += pv == c.glibc
        if vec == c.glibc and pv != c.glibc:
            row[2] += 1
            pex.append((len(c.pat), len(c.subj), c.pat, c.subj, c.flags, pv, c.glibc))
    tot = [0, 0, 0]
    for cn in corpus_order + ["ALL CORPORA", "hand"]:
        r = tot if cn == "ALL CORPORA" else pstats[cn]
        print("  %-42s %7d  full vector agree %7d (%6.2f%%)  hypothesis right where this reading is wrong %d"
              % (cn, r[0], r[1], pct(r[1], r[0]), r[2]))
        report.setdefault("posix_subpattern_reading", {})[cn] = {"cases": r[0], "full": r[1], "hyp_only": r[2]}
        if cn in corpus_order:
            for j in range(3):
                tot[j] += r[j]
    seen = set()
    for _, _, pat, subj, flags, pv, g in sorted(pex):
        if pat in seen:
            continue
        seen.add(pat)
        print("     e.g. %s [%s] on [%s]: glibc and hypothesis %s, POSIX subpattern reading %s" % (
            flags, show(pat), show(subj), fmt_vec(g), fmt_vec(pv)))
        if len(seen) >= args.examples:
            break

    # keep versus reset: where they differ, which one glibc matches
    print()
    print("== carry: cases where keep and reset give different vectors, and which one glibc matches")
    for em in EMPTY_VARIANTS:
        n = gk = gr = 0
        for idx, c in enumerate(allc):
            vk = results[(em, "keep", idx)][1]
            vr = results[(em, "reset", idx)][1]
            if vk != vr:
                n += 1
                gk += vk == c.glibc
                gr += vr == c.glibc
        print("  empty=%-6s differ %4d  glibc=keep %4d  glibc=reset %4d  neither %4d" % (em, n, gk, gr, n - gk - gr))
        report["keep_vs_reset"][em] = {"differ": n, "glibc_keep": gk, "glibc_reset": gr}
    diffs = [(idx, c) for idx, c in enumerate(allc)
             if results[("first", "keep", idx)][1] != results[("first", "reset", idx)][1]]
    diffs.sort(key=lambda ic: (len(ic[1].pat), len(ic[1].subj), ic[1].pat, ic[1].subj))
    seen = set()
    for idx, c in diffs:
        if c.pat in seen:
            continue
        seen.add(c.pat)
        print("     e.g. %s [%s] on [%s]: glibc %s, reset gives %s" % (
            c.flags, show(c.pat), show(c.subj), fmt_vec(c.glibc), fmt_vec(results[("first", "reset", idx)][1])))
        if len(seen) >= args.examples:
            break

    # classify the residuals of the default variant
    for label, pool, offset in (("corpora", cases, 0), ("hand", hand, len(cases))):
        cats = defaultdict(list)
        notev = []
        for idx, c in enumerate(pool):
            st, vec = results[(DEFAULT[0], DEFAULT[1], offset + idx)]
            if st != "ok":
                notev.append(c)
                continue
            if vec != c.glibc:
                cats[classify(c, vec)].append((c, vec))
        total = sum(len(v) for v in cats.values())
        print()
        print("== residuals of the default variant (%s): %d" % (label, total))
        for c in notev:
            print("  not evaluated (step budget): %s [%s] on [%s] (glibc %s)" % (
                c.flags, show(c.pat), show(c.subj), fmt_vec(c.glibc)))
        report["categories"][label] = {}
        for cat in sorted(cats):
            items = sorted(cats[cat], key=lambda cv: (len(cv[0].pat), len(cv[0].subj), cv[0].corpus, cv[0].pat,
                                                      cv[0].subj))
            per = defaultdict(int)
            pats = set()
            for c, _ in items:
                per[c.corpus.split(" ")[0]] += 1
                pats.add((c.flags, c.pat))
            print("  %-5s %5d cases, %4d patterns  %s" % (cat, len(items), len(pats), dict(sorted(per.items()))))
            print("        %s" % CATEGORY_TEXT[cat])
            ex = []
            seen = set()
            for c, vec in items:
                if c.pat in seen:
                    continue
                seen.add(c.pat)
                print("        %s [%s] on [%s]: glibc %s, hypothesis %s" % (
                    c.flags, show(c.pat), show(c.subj), fmt_vec(c.glibc), fmt_vec(vec)))
                ex.append({"flags": c.flags, "pattern": c.pat, "subject": c.subj,
                           "glibc": fmt_vec(c.glibc), "hypothesis": fmt_vec(vec)})
                if len(ex) >= args.examples:
                    break
            report["categories"][label][cat] = {"cases": len(items), "patterns": len(pats),
                                                "per_corpus": dict(per), "examples": ex}

    if args.json:
        with open(args.json, "w", encoding="utf-8", newline="\n") as f:
            json.dump(report, f, indent=1, sort_keys=True)


if __name__ == "__main__":
    main()


