#!/usr/bin/env python3
"""Unit test for limits_from_counters.py rounding and --check (run: python3 bench/test_limits_from_counters.py)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import limits_from_counters as L  # noqa: E402


def row(b, r, li, gi):
    return "\t".join([b, r, "1", "1", "1", "1", "-", "-", "-", "-", li, gi, "1%/1%", ""])


class T(unittest.TestCase):
    def test_rounding(self):
        self.assertEqual(L.limit_for(20.4, "2%"), 23)  # 2% = 0.41 < 2 -> +2 -> 22.4 -> 23
        self.assertEqual(L.limit_for(20.0, "2%"), 22)  # exact: no spurious +1
        self.assertEqual(L.limit_for(20.0000001, "2%"), 22)  # float noise does not round up
        self.assertEqual(L.limit_for(1000.0, "2%"), 1020)  # pct wins over 2 instr
        self.assertEqual(L.limit_for(1000.5, "2%"), 1021)  # 1020.51 -> 1021
        self.assertEqual(L.limit_for(20.4, "1"), 22)  # absolute N: exactly N
        self.assertEqual(L.limit_for(20.4, "0"), 21)

    def test_parse_and_select(self):
        t = row("cb", "a_16", "20.4", "19.4") + "\n" + row("cb", "b_16", "-", "5") + "\n" + row("cb", "c_16", "7.0", "-") + "\n"
        rows = L.parse_counters(t)
        self.assertEqual([r[:2] for r in rows], [("cb", "a_16"), ("cb", "c_16")])
        self.assertIsNone(rows[1][3])
        self.assertEqual(len(L.select(rows, ["cb/c_16"])), 1)
        self.assertEqual(len(L.select(rows, ["a_16"])), 1)
        p = L.propose(rows, "CPU", "2%")
        self.assertEqual(p[0], ("CPU", "cb", "a_16", 23, "Liella 20.4 vs glibc 19.4; headroom 2%"))

    def test_check(self):
        rows = [("cb", "a", 20.0, 19.0), ("cb", "b", 100.0, None), ("cb", "n", 5.0, None)]  # proposals 22 102 7
        lim = L.parse_limits("# c\nCPU\tcb\ta\t30\tx\n*\tcb\tb\t101\tx\nOTHER\tcb\tz\t9\tx\nCPU\tcb\tgone\t9\tx\n")
        got = {(k, n) for k, n, _ in L.check(rows, "2%", lim, "CPU")}
        # a: 30 vs 22 = loose; b: 101 < 102 but >= count 100 = fine; n: new; gone: no row; z: other cpu, ignored
        self.assertEqual(got, {("LOOSE", "cb/a"), ("NEW", "cb/n"), ("NO-ROW", "cb/gone")})
        lim2 = L.parse_limits("CPU\tcb\ta\t19\tx\n")
        self.assertIn(("WOULD-FAIL", "cb/a"), {(k, n) for k, n, _ in L.check(rows, "2%", lim2, "CPU")})


if __name__ == "__main__":
    unittest.main()
