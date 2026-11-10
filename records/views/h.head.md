<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 7. Design decision register

Every ⚑ HIST, COMPAT, PATH and ARB annotation in the spec is either an opportunity for a
cleaner internal design, or a constraint the translation layer must reproduce at the
boundary. It is usually both. This register records the decision for each. ⚑ KERN items
are honoured unchanged, and are listed only where they shape a decision.

**Reading the tables.**

* *Internal* is the clean API in `liella`.
* *Boundary* is what the translation layer (plus `liella-legacy`) reproduces for C callers.
* A decision that changes behaviour observable through the C ABI says so explicitly, and has
  a test citing its ID.

### 7.1 Cross-cutting decisions (spec §30, patterns H1–H18)

