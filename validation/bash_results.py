#!/usr/bin/env python3
"""Compare two runs of bash's test suite (`make check` output), test by test.

    validation/bash_results.py GLIBC_CHECK_LOG LIELLA_CHECK_LOG [--show NAME]

Bash's suite has no pass/fail counter: tests/run-all runs each `run-*` script, which prints
its name and then a diff against the expected output (empty when the output matches) and
some advisory `warning:` lines. A test "differs" when anything but warnings follows its name.
The baseline itself differs in a few tests (no terminal, inherited signal dispositions), so
the comparison that matters is per test: the same output under both libraries, or not.
"""
import re
import sys


def parse(path):
    tests, name = {}, None
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.rstrip("\n")
            if re.fullmatch(r"run-[A-Za-z0-9_.+-]+", line):
                name = line
                tests[name] = []
            elif name is not None and not line.startswith("warning:"):
                tests[name].append(line)
    return tests


def main():
    base, other = parse(sys.argv[1]), parse(sys.argv[2])
    if len(sys.argv) > 4 and sys.argv[3] == "--show":
        n = sys.argv[4]
        print("--- glibc\n" + "\n".join(base.get(n, ["(not run)"])))
        print("--- liella\n" + "\n".join(other.get(n, ["(not run)"])))
        return
    names = sorted(set(base) | set(other))
    same = changed = 0
    for n in names:
        b, o = base.get(n), other.get(n)
        if b == o:
            same += 1
            continue
        changed += 1
        state = lambda t: "not run" if t is None else ("clean" if not t else f"{len(t)} diff lines")
        print(f"{n}\tglibc: {state(b)}\tliella: {state(o)}")
    print(f"tests: {len(names)}; identical output: {same}; different: {changed}")
    clean = lambda t: sum(1 for v in t.values() if not v)
    print(f"clean (no diff) under glibc: {clean(base)}/{len(base)}; under liella: {clean(other)}/{len(other)}")


main()
