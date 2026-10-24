# argp

**Spec:** §1.7 (`argp_parse`, `argp_help`, `argp_state_help`, `argp_usage`, `argp_error`,
`argp_failure`, `argp_program_version`, `argp_program_version_hook`, `argp_program_bug_address`,
`argp_err_exit_status`). Public interface: `<argp.h>` (`struct argp`, `struct argp_option`,
`struct argp_child`, `struct argp_state`, the `ARGP_*` and `OPTION_*` constants, the `ARGP_KEY_*`
keys, the `ARGP_HELP_*` flags).

**Clauses:** `s1_7_c_0`, `s1_7_c_1`, `s1_7_c_2`, `s1_7_c_3`, `s1_7_i_0`, `s1_7_g_0`, `s1_7_f_0`.

**Probe:** `crates/core/tests/oracle/argp_diff.c`. **Record:** `argp.glibc.txt`.

## What the probe does

The program defines `argp_program_version` (`prog 1.0`) and `argp_program_bug_address`
(`<bugs@example.org>`), and sets `program_invocation_name` to `/usr/bin/prog` and
`program_invocation_short_name` to `prog`. `argv[0]` is `prog`.

The parser tree: a main parser with the options `--verbose`/`-v`; `--output`/`-o FILE` (a long
description); `--level`/`-l [N]` (optional argument) with the aliases `--quiet` (no key) and
`--silent`/`-s`; `--hidden`/`-H` (hidden); a group header "Advanced options:" (group 2) with
`--long-only` (key 1000, no short name) and `-x` (no long name); a documentation entry `FORMAT`
(`OPTION_DOC | OPTION_NO_USAGE`, group 3). Its `args_doc` is `ARG1 ARG2\n-x ARG`; its `doc` is
`Program description.` and, after a vertical tab, `Text after the options.`. One child parser
(group 0) with a header "Child options:" (group 5), `--child`/`-c` and `--child-long[=VAL]` (key
1001), and its own `doc`.

The parser callbacks print every call to standard output: which parser (`main`, `child`), the key
(a letter, a number, or `ARG`, `ARGS`, `END`, `NO_ARGS`, `INIT`, `SUCCESS`, `ERROR`, `FINI`), the
argument, `state->arg_num` and `state->next`. The main parser accepts its own keys; it returns
`ARGP_ERR_UNKNOWN` for the operand `stop` and for keys it does not know. At `ARGP_KEY_END`,
depending on the case, it calls `argp_usage` when fewer operands than required arrived, calls
`argp_error(state, "bad thing %d", 7)`, calls `argp_failure(state, 3, ENOENT, "failure %s",
"here")`, returns `EINVAL`, or calls `argp_state_help(state, stdout, ARGP_HELP_SHORT_USAGE |
ARGP_HELP_SEE)`.

Each case runs `argp_parse` in a child process (standard input closed, a 5-second alarm), then
prints `-> <result> index=<index>`. The record shows, per case, `== <case> exit=<status>` (77
when `argp_parse` returned; -1 when the child was killed by a signal), then the child's standard
output and standard error with `\n` for newlines (each read up to 8192 bytes).

The cases (name: flags; environment; arguments): options short and long, attached and separate
arguments, optional arguments, abbreviations, an ambiguous abbreviation, the child's options,
unknown options, a missing and an unexpected argument, `--`, an operand the parser does not want,
`ARGP_IN_ORDER`, permutation, `ARGP_PARSE_ARGV0`, too few operands, `argp_error`, `argp_failure`,
a parser error, `argp_state_help`, `ARGP_NO_EXIT`, `ARGP_NO_ERRS`, `ARGP_SILENT`, `ARGP_NO_HELP`,
`--help`, `-?`, `--usage`, `--version`, `-V`, `--program-name`, and `--help`/`--usage` under
`ARGP_HELP_FMT` (`rmargin=40`, `no-dup-args`, the column settings, an unknown name, a bad value)
and under `COLUMNS=50`.

## Observations the record shows directly

* The help output's layout (columns, wrapping, the order of groups and headers, how aliases and
  optional arguments are shown, the `Usage:` lines, the bug-address line) is shown in full for
  every case that prints it.
* `ARGP_HELP_FMT=rmargin=x` (a value that is not a number): the reference prints
  `prog: x: Unknown ARGP_HELP_FMT parameter` and then writes blank lines without end until the
  alarm kills it (`exit=-1`). This is a defect of the reference; it is not behaviour to reproduce.
