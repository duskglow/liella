<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 6. Phases

Phases are ordered by dependency. Each lists:

* **Scope**: the spec sections it covers.
* **Symbols**: exported names in scope, from `spec/symbols.tsv`.
* **Clauses**: clause stubs in scope.
* **Needs**: the phases and internal primitives it depends on.
* **Builds**: the primitives it creates.
* **Test surface**: what the tests look like.
* **Exit**: the gate for closing the phase.

The translation side of every phase is designed *in the same phase* as the internal API, so
impedance mismatches surface immediately. Each phase ends by adding its rows to the findings
table (§8.5).

Sections split across phases are marked *(part)*. Symbol counts by section:

| § | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| symbols | 48 | 38 | 60 | 174 | 115 | 37 | 35 | 212 | 34 | 1,163 | 141 | 340 | 173 | 43 | 98 |
| clauses | 68 | 33 | 46 | 66 | 7 | 37 | 34 | 44 | 24 | 56 | 114 | 96 | 71 | 56 | 35 |

| § | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | C0 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| symbols | 185 | 103 | 77 | 34 | 61 | 9 | 239 | 70 | 84 | 29 | 7 | 17 | 12 | 305 | – |
| clauses | 38 | 31 | 57 | 25 | 41 | 11 | 67 | 55 | 28 | 19 | 12 | 19 | 5 | 8 | 22 |

```
P0 ─▶ P1 ─▶ P2 ─▶ P3 ─▶ P4 ─▶ P5 ─▶ P6 ─▶ P7 ─▶ P8 ─▶ P9 ─▶ P10 ─▶ P11 ─▶ P12 ─▶ P13 ─▶ P14 ─▶ P15
              └────────────▶ M (libm track, parallel from P2; needed by P8 for printf/strtod checks; joins at P14: complete before P14 exits)
```
