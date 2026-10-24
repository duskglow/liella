<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 8: Streams and formatting

* **Scope:**
  * §12 (340 symbols): `FILE`, buffering, memory and cookie streams, printf, scanf (all three
    dialects), wide streams, `stdio_ext`, printf registration, and the `_IO_*` main entry
    points;
  * §2.2–2.3 and §2.6–2.7, error text, `err`/`error`, `fmtmsg`, backtrace;
  * §1.6–1.7, `getopt`/`argp` (they need `stderr`);
  * §7.5, `strfmon`;
  * §8.4 remaining (`ecvt`/`gcvt` through the streams formatting core).
* **Symbols:** about 440.
* **Clauses:** about 190.
* **Needs:** P7, and Track M (for `%a`/`%e` corner cases and `strtod`/`printf` round-trips).
* **Builds:**
  * IP-22 (the typed format and parse engines).
  * IP-14 in full: the C variadic adapters for the printf, scanf, `err`/`error`, `syslog`
    and `argp` families, and `va_arg` accessors read by `abi::va` through
    `boundary::va_source`.
  * `abi::layout::FILE` (the buffer-window prefix of Appendix C.7/C.9, F10),
    `legacy::stdio_compat`, and IP-27 (the unwinder for backtrace).
* **Test surface:**
  * The buffering policy matrix: full/line/unbuffered, tty and non-tty, flush triggers
    including the ISO C input-flushes-line-buffered-output rule.
  * Every conversion, flag, width and precision combination against the spec grammar.
  * Positional arguments, `%n` rules (D-12.4), and `EOVERFLOW` at `INT_MAX`.
  * scanf dialect differences (`%a`, `0b`).
  * Orientation locking.
  * `fmemopen` in both versioned contracts.
  * `open_memstream` visibility at `fflush`.
  * C conformance tests that use the header-inline `getc`/`putc` macros against our `FILE`
    window.
* **Exit:** §12, §2.2/2.3/2.6/2.7, §1.6–1.7 and §7.5 green. Budgets (rule P6, §2.3): `printf("%d")` and `fputs` throughput targets set in the
  phase-0 benchmark table.
* **Delivered** (exit met, with the deviations listed):
  * **Clean-room exception (§1.4):** during this phase, glibc source was read and recalled in a
    continuous session. The nine affected items were removed and rewritten by fresh sessions from
    the spec and black-box behaviour documents (`spec/behaviour/`), beb7833. Everything written
    after that (argp, backtrace, the clause and symbol tests) was written the same way: a
    behaviour document first where the reference's behaviour mattered, then a fresh session
    with the spec, the documents and its own black-box probes.
  * **Streams (D-12.1, D-12.2, D-12.11, D-12.14):** `liella::stream`, one buffer whose role
    switches between reading and writing, over a descriptor, a fixed or growing memory buffer
    or caller callbacks; the C `FILE` object, whose buffer window is the stream's (header-inline
    `getc`/`putc` work on it, `conformance/c/s12_stdio_window.c`); `fmemopen` (the `GLIBC_2.22`
    contract; see the deviations), `open_memstream`/`open_wmemstream`, `fopencookie`, `<stdio_ext.h>`, stream
    locking, wide streams with `,ccs=` codecs and orientation locking, the standard streams and
    the `_IO_2_1_*_` objects.
  * **Formatting (IP-22, D-12.4, D-12.12, D-12.13, D-12.6):** the typed printf engine (positional
    arguments, `%n` rules, `EOVERFLOW` at `INT_MAX`), the scanf engine in its three dialects,
    printf customization (D-12.8), and the C variadic adapters for printf, scanf, `err`/`error`,
    `argp_error`/`argp_failure` and `strfmon` (IP-14).
  * **§1.6–1.7, §2.2–2.7, §7.5:** getopt/getsubopt, argp, error text (now with per-thread result
    slots, D-H3), `err`/`error`, `fmtmsg`, backtrace with Liella's own DWARF unwinder (IP-27,
    D-2.7), `strfmon` (D-7.11).
  * **Tests:**
    * Clause tests: §12 95 of 96, §1 all 68, §2 all 33, §7.5 both (the rest of §7 waits for phases
      12–13). Pending: `cuserid` (§17).
    * Symbol tests: §12 262 of 340, §1 all 48, §2 34 of 38, §7.5 all three `strfmon` forms.
      Variadic and `va_list` entry points are exercised through real C varargs in conformance
      programs. Pending: `cuserid`, `popen`/`pclose` and `_IO_popen`/`_IO_proc_*` (§17, §18), the
      72 libio internals (default versions at GLIBC_2.2.5, listed in `xtask/src/symgap.rs`) and
      `sys_errlist`/`sys_nerr` (phase 15, F32).
    * Differential tests against the host glibc: `stdio`, `wstream`, `printf`, `scanf`,
      `registry`, `diag`, `getopt`, `argp`, `monetary` and `backtrace` products; their recorded
      differences are in `docs/differences-from-glibc.md`.
    * `cargo xtask test`: plain, export+selftest, conformance 52/52; clippy clean.
  * **Defects the clause and symbol tests found, fixed in this phase:** `__fbufsize` before the
    first I/O; the line-buffered flush before input skipped while another thread opened a stream
    (D-12.1); `strerror`'s text shared between threads (D-H3); `freopen` with a NULL path
    (D-12.3); the fortified `fgets`/`fgetws`/`snprintf`/`swprintf` forms terminating on a bound
    rather than on an actual overflow, and the wide `%n` rule (D-C0.5); a flaky socket test and
    an environment race in the test harness.
  * **Budgets:** `benchmarks/progress.md` entry 6. The phase-0 table never had `printf`/`fputs`
    targets, so only sanity bounds were added (`cbench_stdio`); Liella is 6.1× glibc at
    `snprintf("%d")`, 4.5× at `fputs` and 10.7× at `putc`. Targets are for the owner; the gaps are
    listed as optimization targets.
  * **Deviations and open items for the owner:**
    * Spec over reference, recorded in `docs/differences-from-glibc.md`: among them `freopen` with
      a NULL path, the fortified bound rule, `fileno` setting `EBADF`, argp's `COLUMNS`, and
      backtrace's texts.
    * Spec gaps found: §2.7 describes the reference's `dlopen` of the unwinder; §2.3 is silent on
      several `error` details (D-2.3); §1.7 does not say that `ARGP_PARSE_ARGV0` needs
      `ARGP_NO_ERRS`; the argument layout `register_printf_type` handlers receive; the meaning of
      `_IO_vfscanf`'s fourth argument; the fortified forms' `flag` argument.
    * `snprintf` allocates (the format is parsed into heap vectors), so it fails when no memory
      can be mapped; the spec does not promise otherwise.
    * Historical glibc behaviours matched, candidates for a later cleanup: getopt's `optopt`
      overwrite and long-only ambiguity of equivalent entries, `__printf_fp`'s letter mapping,
      `parse_printf_format`'s `PA_INT` for `%Li`.
    * **Deviation from this phase's test surface:** `fmemopen@GLIBC_2.2.5`, the superseded
      contract, is not exported. Like every compat-only version it needs a `.symver` alias, which
      the product build (`xtask/src/so.rs`) leaves to phase 15; it moves there.
    * Out of this phase: `popen`/`pclose`/`system` (§18), `cuserid` (§17), the libio vtables and
      compat-only symbols (phase 15).
