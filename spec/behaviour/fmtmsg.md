# fmtmsg and addseverity (item 4)

**Spec:** §2.6 (`fmtmsg`, `addseverity`; `MSGVERB`, `SEV_LEVEL`). Public interface: `<fmtmsg.h>`.

**Clauses:** `s2_6_c_0`, `s2_6_c_1`, `s2_6_c_2`, `s2_6_c_3`, `s2_6_c_4`, `s2_6_f_0`.

**Probe:** `crates/core/tests/oracle/diag_diff.c`, the `fmt_*` children. **Record:**
`diag.glibc.txt`, the lines from `label0=` through the `fmt_sev` child.

## What the probe does

Each configuration runs in a child process, which sets the environment before its first
`fmtmsg` call. The child's standard output lines come first, then a line
`<child> exit=<status> stderr=[...]` with everything it wrote to standard error.

* `fmt_default`: no `MSGVERB`. `fmt_verb`: `MSGVERB=text:action`. `fmt_verb_order`:
  `MSGVERB=tag:severity:label`. `fmt_badverb`: `MSGVERB=text:bogus`.
* Each of these runs the same cases (with `MM_PRINT` unless stated):
  * `label0`…`label6`: the labels `UX:cat`, NULL, `nocolon`, `ABCDEFGHIJK:x` (11 bytes before
    the colon), `ABCDEFGHIJ:abcdefghijklmn` (10 and 14), `a:abcdefghijklmno` (15 after), `:`,
    each with `MM_ERROR`, text `text`, action `act`, tag `UX:cat:001`;
  * `sev-1`…`sev10`: severities -1 to 10 with label `UX:cat` and text `t`;
  * `fields`: no fields at all; a label alone; a severity and an action; text and a tag;
    `MM_PRINT|MM_SOFT|MM_UTIL` with label, `MM_HALT` and tag; classification 0 (nothing
    printed);
  * `add`: `addseverity(6, "SIX")`, `fmtmsg` with 6, `addseverity(6, NULL)`, `fmtmsg` with 6,
    `addseverity(4, "X")`, `addseverity(8, NULL)`. The six calls are arguments of one `printf`,
    so the compiler evaluates them right to left: read the results in that order.
* `fmt_sev`: `SEV_LEVEL=x,5,FIVE:y,9,NINE:bad:z,3,LOW:w,0x7,SEVEN:v,12`, an `addseverity(9,
  "EARLY")` before the first `fmtmsg`, then severities 3 to 12.

`MM_CONSOLE` is not probed. The spec names `/dev/console` for it.
