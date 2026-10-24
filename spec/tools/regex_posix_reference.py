#!/usr/bin/env python3
"""Generate spec/behaviour/regex-posix.expected.txt: the POSIX XBD 9.1 submatch reading of every
regex line of the patterns record where it differs from glibc.

Owner ruling of 2026-10-03 (D-14.4, spec §14.9): submatch selection follows POSIX XBD 9.1 in every
case, including the categories I1-I4 and E2 of spec/behaviour/regex-submatch-analysis.md where
glibc differs, and Liella finds the matches glibc misses (O1). The products test
(crates/core/tests/patterns_products.rs) and the clause tests compare Liella with glibc line by
line; a regex line may differ only where this file gives the POSIX line, and then Liella must print
exactly that line.

The reference is the "POSIX subpattern reading" of spec/tools/regex_submatch_hypothesis.py
(imported from it, no regex engine source consulted), extended with regexec's reporting rule 3
(a group nested in a repetition is reported within the last iteration of the repetition, so a group
that the last iteration did not take is -1: carry=reset) as the ruling requires:

* Paths: the derivations of the pattern under the analysis's empty=stop rule (an iteration may match
  the empty string; no iteration follows an empty one beyond the minimum count; iterations needed
  for the minimum may be empty) and carry=reset (each new iteration of a repetition resets the
  groups inside it; a back-reference matches the text its group holds at that point of the path,
  and fails if the group is unset).
* Overall match: the smallest start s from which a path exists, then the largest end e.
* Submatches: among the paths from s to e, the capture vector that is greatest when compared group
  by group in the order of the opening parentheses: a participating group beats one that does not
  take part (a null string is longer than no match), then the earlier start, then the longer match.

Lines evaluated: the `rc` lines of the four corpora, the `re` lines of the "-- matching" block (C
locale), and the GNU `re_search` lines under POSIX_EXTENDED and EMACS syntax that search the whole
buffer forward. Lines the reference cannot evaluate (the step budget of the pathological patterns)
are left to glibc's line.

Each differing case is classified (the categories of the analysis):
  O1  glibc finds no match (or another overall match) where the path set has one: a back-reference to
      a group of a counted repetition that needs an empty iteration;
  I3  explained by the carry: the same reading with carry=keep gives glibc's vector;
  E2  glibc's vector needs a leading empty iteration followed by more iterations (empty=lead);
  I4  a counted repetition (minimum >= 2) whose last iteration glibc reports empty;
  I2  glibc leaves a group out (-1) where the null string can take part;
  I1  glibc follows alternation priority (its vector is the hypothesis's first path);
  OTHER anything else (reported; none is expected).

Usage:
    python -B spec/tools/regex_posix_reference.py [--record PATH] [--out PATH] [--check]

--check compares the generated text with the existing file and fails if they differ. Deterministic:
the output depends only on the record. Python 3, standard library only.
"""

from __future__ import annotations

import argparse
import gzip
import os
import re
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import regex_submatch_hypothesis as H  # noqa: E402

CATEGORIES = ("O1", "I3", "E2", "I4", "I2", "I1", "OTHER")


def key(v):
    return tuple(x for so, eo in v[1:] for x in ((0, 0, 0) if so < 0 else (1, -so, eo - so)))


def compiled(flags, pat):
    c = H.get_compiled(flags, pat)
    return None if isinstance(c, H.ParseError) else c


def matcher(flags, pat, empty, carry, eflags=""):
    node, ng = compiled(flags, pat)
    return H.Matcher(node, ng, flags, empty, carry, notbol="NOTBOL" in eflags, noteol="NOTEOL" in eflags)


def reading(flags, pat, subj, nmatch, carry, eflags=""):
    """The POSIX subpattern reading under `carry`: None (no match) or the vector; raises Budget."""
    mt = matcher(flags, pat, "stop", carry, eflags)
    r = mt.search(subj, nmatch)
    if r is None:
        return None
    _, _, s, e = r
    vecs = matcher(flags, pat, "stop", carry, eflags).all_vectors(subj, s, e, nmatch)
    return max(vecs, key=key)


def hypothesis(flags, pat, subj, nmatch, eflags=""):
    r = matcher(flags, pat, "stop", "keep", eflags).search(subj, nmatch)
    return None if r is None else tuple(r[0])


def groups_of_bounded(node):
    """Group numbers inside a counted repetition with minimum >= 2 whose body can be empty."""
    out = set()

    def nullable(n):
        t = n[0]
        if t in ("assert",):
            return True
        if t == "chr":
            return False
        if t == "bref":
            return True
        if t == "group":
            return nullable(n[2])
        if t == "cat":
            return all(nullable(c) for c in n[1])
        if t == "alt":
            return any(nullable(c) for c in n[1])
        if t == "rep":
            return n[2] == 0 or nullable(n[1])
        return False

    def walk(n):
        t = n[0]
        if t == "rep":
            if n[2] >= 2 and nullable(n[1]):
                out.update(n[4])
            walk(n[1])
        elif t == "group":
            walk(n[2])
        elif t in ("cat", "alt"):
            for c in n[1]:
                walk(c)

    walk(node)
    return out


def classify(flags, pat, subj, nmatch, glibc, posix, eflags=""):
    node, _ = compiled(flags, pat)
    if glibc is None or posix is None or glibc[0] != posix[0]:
        try:
            keep = reading(flags, pat, subj, nmatch, "keep", eflags)
        except (H.Budget, RecursionError):
            keep = "budget"
        if keep == glibc:
            return "I3"
        has_bref = H.node_has(node, lambda n: n[0] == "bref")
        has_counted = H.node_has(node, lambda n: n[0] == "rep" and n[2] >= 2)
        return "O1" if has_bref and has_counted else "OTHER"
    try:
        keep = reading(flags, pat, subj, nmatch, "keep", eflags)
    except (H.Budget, RecursionError):
        keep = None
    if keep == glibc:
        return "I3"
    s, e = glibc[0]
    try:
        lead = matcher(flags, pat, "lead", "keep", eflags).all_vectors(subj, s, e, nmatch)
        stop = matcher(flags, pat, "stop", "keep", eflags).all_vectors(subj, s, e, nmatch)
    except (H.Budget, RecursionError):
        lead, stop = [], []
    if glibc in lead and glibc not in stop:
        return "E2"
    first = next(i for i in range(1, len(glibc)) if glibc[i] != posix[i])
    g, p = glibc[first], posix[first]
    if first in groups_of_bounded(node) and g[0] >= 0 and g[0] == g[1]:
        return "I4"
    if g == (-1, -1) and p[0] >= 0 and p[0] == p[1]:
        return "I2"
    try:
        hyp = hypothesis(flags, pat, subj, nmatch, eflags)
    except (H.Budget, RecursionError):
        hyp = None
    if hyp == glibc:
        return "I1"
    return "OTHER"


def fmt_rc(vec):
    return "-" if vec is None else ",".join("%d.%d" % p for p in vec)


def fmt_re(vec):
    return "".join("(%d,%d)" % p for p in vec)


def generate(record):
    with gzip.open(record, "rt", encoding="latin-1", newline="\n") as f:
        lines = f.read().split("\n")
    out = []
    counts = Counter()
    lines_per_cat = Counter()
    others = []
    section = sub = None
    subjects = []
    for line in lines:
        if line.startswith("== "):
            section, sub = line[3:], None
            continue
        if line.startswith("-- "):
            sub = line
            continue
        cats = []
        new = None
        if section == "regex corpus" and line.startswith("subjects"):
            subjects = [H.unesc(x[1:-1]) for x in line.split(" ")[1:]]
            continue
        if section == "regex corpus" and line.startswith("rc "):
            m = re.match(r"^rc (\S+) \[(.*)\] (\d+)(.*)$", line)
            flags, pat, code, rest = m.group(1), H.unesc(m.group(2)), int(m.group(3)), m.group(4).split()
            if code != 0 or compiled(flags, pat) is None:
                continue
            nsub = int(rest[0])
            nmatch = min(nsub + 1, 10)
            res = rest[1:]
            fields = []
            changed = False
            for subj, r in zip(subjects, res):
                glibc = None if r == "-" else tuple(tuple(int(x) for x in pr.split(".")) for pr in r.split(","))
                try:
                    posix = reading(flags, pat, subj, nmatch, "reset")
                except (H.Budget, RecursionError):
                    posix = glibc
                if posix != glibc:
                    c = classify(flags, pat, subj, nmatch, glibc, posix)
                    cats.append(c)
                    if c == "OTHER":
                        others.append((flags, pat, subj, glibc, posix))
                    changed = True
                fields.append(fmt_rc(posix))
            if changed:
                new = "rc %s [%s] %d %d %s" % (m.group(1), m.group(2), code, nsub, " ".join(fields))
        elif section == "regex" and sub == "-- matching" and line.startswith("re "):
            head, _, result = line.partition(" -> ")
            parts = head.split(" ", 2)
            flags = parts[1]
            body, _, eflags = parts[2].rpartition(" ")
            k = body.rfind("] [")
            pat, subj = H.unesc(body[1:k]), H.unesc(body[k + 3 : -1])
            kind, nsub, glibc = H.parse_hand_result(result)
            if kind == "regcomp" or compiled(flags, pat) is None:
                continue
            ef = "" if eflags == "-" else eflags
            nmatch = min(nsub + 1, 10)
            try:
                posix = reading(flags, pat, subj, nmatch, "reset", ef)
            except (H.Budget, RecursionError):
                continue
            if posix != glibc:
                c = classify(flags, pat, subj, nmatch, glibc, posix, ef)
                cats.append(c)
                if c == "OTHER":
                    others.append((flags, pat, subj, glibc, posix))
                tail = "0 nsub=%d %s" % (nsub, fmt_re(posix)) if posix is not None else "1 nsub=%d " % nsub
                new = "%s -> %s" % (head, tail)
        elif section == "GNU regex interface" and line.startswith("re_search "):
            m = re.match(r"^re_search (POSIX_EXTENDED|EMACS) \[(.*)\] \[(.*)\] start=0 range=(\d+) -> (\d+) nsub=(\d+) regs=(\d+)((?: \(-?\d+,-?\d+\))*) (re_match@start=-?\d+)$", line)
            if not m:
                continue
            flags = "E" if m.group(1) == "POSIX_EXTENDED" else "B"
            pat, subj = H.unesc(m.group(2)), H.unesc(m.group(3))
            if int(m.group(4)) != len(subj) or compiled(flags, pat) is None:
                continue
            nsub = int(m.group(6))
            nmatch = min(nsub + 1, 10)
            glibc = tuple((int(a), int(b)) for a, b in re.findall(r"\((-?\d+),(-?\d+)\)", m.group(8)))
            try:
                posix = reading(flags, pat, subj, nmatch, "reset")
            except (H.Budget, RecursionError):
                continue
            if posix is not None and posix != glibc and posix[0] == glibc[0]:
                c = classify(flags, pat, subj, nmatch, glibc, posix)
                cats.append(c)
                if c == "OTHER":
                    others.append((flags, pat, subj, glibc, posix))
                regs = "".join(" (%d,%d)" % p for p in posix)
                new = "re_search %s [%s] [%s] start=0 range=%s -> %s nsub=%s regs=%s%s %s" % (
                    m.group(1), m.group(2), m.group(3), m.group(4), m.group(5), m.group(6), m.group(7), regs, m.group(9))
        if new is not None:
            cs = Counter(cats)
            tag = ",".join("%s:%d" % (c, cs[c]) for c in CATEGORIES if cs[c])
            out.append("%s\t%s\t%s" % (tag, line, new))
            counts.update(cats)
            for c in cs:
                lines_per_cat[c] += 1
    return out, counts, lines_per_cat, others


HEADER = """# The POSIX XBD 9.1 submatch reading of the regex lines of spec/behaviour/patterns.glibc.txt.gz
# where it differs from glibc (owner ruling 2026-10-03, D-14.4, spec 14.9). Generated by
# spec/tools/regex_posix_reference.py from the record; do not edit.
# Each line: CATEGORIES <TAB> glibc's line <TAB> the line POSIX gives. CATEGORIES counts the
# differing cases (corpus subjects) per category of spec/behaviour/regex-submatch-analysis.md.
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--record", default=os.path.join(HERE, "..", "behaviour", "patterns.glibc.txt.gz"))
    ap.add_argument("--out", default=os.path.join(HERE, "..", "behaviour", "regex-posix.expected.txt"))
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    out, counts, per_line, others = generate(args.record)
    text = HEADER + "".join(l + "\n" for l in out)
    print("lines that differ: %d" % len(out))
    for c in CATEGORIES:
        if counts[c]:
            print("  %-5s %6d cases on %5d lines" % (c, counts[c], per_line[c]))
    for flags, pat, subj, g, p in others[:20]:
        print("  OTHER %s [%s] on [%s]: glibc %s, POSIX %s" % (flags, H.show(pat), H.show(subj), H.fmt_vec(g), H.fmt_vec(p)))
    if args.check:
        with open(args.out, encoding="latin-1", newline="\n") as f:
            if f.read() != text:
                print("regex-posix.expected.txt is out of date")
                return 1
        return 0
    with open(args.out, "w", encoding="latin-1", newline="\n") as f:
        f.write(text)
    return 1 if others else 0


if __name__ == "__main__":
    sys.exit(main())
