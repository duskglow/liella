#!/usr/bin/env python3
"""Per-test results of the suites that do not print automake's lines (validation/README.md).

    validation/results_other.py PACKAGE CHECK_LOG > OUT.results

Writes automake-style lines (PASS:, FAIL:, SKIP:, XFAIL:) so every package's *.results file
reads the same. Only the harness's own output on standard output (the check log) is read:
  tar   autotest's per-test lines: "  12: name    ok", "FAILED (file.at:N)", "skipped (...)",
        "expected failure (...)";
  make  run_make_tests.pl's per-category lines: "category ..... ok (N passed)",
        "FAILED (M/N passed)", "N/A";
  gawk  the test Makefile prints each test's bare name inside its "Starting ... tests"
        sections; after its summary ("ALL TESTS PASSED" or "N TESTS FAILED") it shows each
        failure's diff under "============== _name =============".
"""
import re
import sys


def tar(lines):
    for line in lines:
        m = re.match(r"^\s*(\d+): (.*?)\s{2,}(ok|FAILED|skipped|expected failure|UNEXPECTED PASS)\b", line)
        if not m:
            continue
        num, name, status = m.groups()
        tag = {"ok": "PASS", "FAILED": "FAIL", "skipped": "SKIP",
               "expected failure": "XFAIL", "UNEXPECTED PASS": "XPASS"}[status]
        print(f"{tag}: {num} {name.strip()}")


def make(lines):
    for line in lines:
        m = re.match(r"^(\S+) \.+ (ok|FAILED|N/A)", line)
        if not m:
            continue
        name, status = m.groups()
        tag = {"ok": "PASS", "FAILED": "FAIL", "N/A": "SKIP"}[status]
        print(f"{tag}: {name}")


def gawk(lines):
    # Inside "======== Starting ... tests ========" sections each test prints its bare name;
    # after the summary ("N TESTS FAILED") the Makefile shows each failure's diff under
    # "============== _name =============".
    seen, failed = [], set()
    inside = False
    for line in lines:
        if re.match(r"^======== Starting .* ========$", line):
            inside = True
            continue
        if re.match(r"^======== Done with .* ========$", line):
            inside = False
            continue
        m = re.match(r"^============== _(\S+) =============$", line)
        if m:
            failed.add(m.group(1))
            continue
        if inside and re.fullmatch(r"[A-Za-z][A-Za-z0-9_+-]*", line) and line not in seen:
            seen.append(line)
    for name in seen:
        print(f"{'FAIL' if name in failed else 'PASS'}: {name}")
    for name in sorted(failed - set(seen)):
        print(f"FAIL: {name}")


def main():
    pkg, log = sys.argv[1], sys.argv[2]
    with open(log, encoding="utf-8", errors="replace") as f:
        lines = f.read().splitlines()
    {"tar": tar, "make": make, "gawk": gawk}[pkg](lines)


if __name__ == "__main__":
    main()
