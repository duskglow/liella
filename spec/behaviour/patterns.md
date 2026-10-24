# Patterns: fnmatch, glob, regular expressions, word expansion

**Spec:** §14.7 (`fnmatch`), §14.8 (`glob`, `glob64`, `globfree`, `globfree64`,
`glob_pattern_p`), §14.9 (`regcomp`, `regexec`, `regerror`, `regfree`, and the GNU and BSD
interfaces as far as §14.9 gives them behaviour: `re_compile_pattern`, `re_compile_fastmap`,
`re_search`, `re_search_2`, `re_match`, `re_match_2`, `re_set_registers`, `re_syntax_options`,
`re_comp`, `re_exec`), §14.10 (`wordexp`, `wordfree`). §14.1-14.6 are context only. Decisions
D-14.4 (regex engine; the plan's phase-12 bullet cites the linear-time guarantee as D-14.5, but it
is D-14.4) and D-14.5 (in-process `wordexp`); the plan's phase-12 test surface (the
leftmost-longest corpus, back-references, `REG_STARTEND` with embedded NUL, every `glob` flag and
both versioned contracts, the `wordexp` error codes). Public interfaces: `<fnmatch.h>`,
`<glob.h>`, `<regex.h>` (with `_REGEX_RE_COMP` for `re_comp`/`re_exec`), `<wordexp.h>`.
Normative references (plan §1.2): POSIX.1-2017 (`fnmatch`, `glob`, `regcomp`/`regexec`/
`regerror`/`regfree`, XBD 9 "Regular Expressions", XCU 2.6 "Word Expansions" and 2.13 "Pattern
Matching Notation", `wordexp`), the man pages `fnmatch(3)`, `glob(3)`, `glob(7)`, `regex(3)`,
`regex(7)`, `re_comp(3)`, `wordexp(3)`, and the glibc manual's "Pattern Matching" chapter.

**Clauses:** `s14_7_c_0`, `s14_7_c_1`, `s14_7_c_2`, `s14_7_c_2_1`, `s14_7_c_2_2`, `s14_7_c_2_3`,
`s14_7_c_2_4`, `s14_7_c_2_5`, `s14_7_c_2_6`, `s14_7_g_0`, `s14_7_f_0`, `s14_8_c_0`, `s14_8_c_1`,
`s14_8_c_2`, `s14_8_c_2_1`, `s14_8_c_2_2`, `s14_8_c_2_3`, `s14_8_c_3`, `s14_8_c_4`, `s14_8_f_0`,
`s14_9_c_1`, `s14_9_c_1_1`, `s14_9_c_1_2`, `s14_9_c_1_3`, `s14_9_c_2`, `s14_9_c_3`, `s14_9_g_0`,
`s14_9_f_0`, `s14_10_c_0`, `s14_10_c_1`, `s14_10_c_2`, `s14_10_c_3`, `s14_10_c_4`, `s14_10_f_0`.

**Probe:** `crates/core/tests/oracle/patterns_diff.c`, run by
`patterns_run.sh PROGRAM SCRATCH [COMPAT_PROGRAM]`. **Record:** `patterns.glibc.txt.gz` (gzip,
`gzip -9 -n`; 76,196 lines, 3.1 MB uncompressed; most of it is the two generated corpora). It
was made on the VM (Ubuntu 24.04, glibc 2.39-0ubuntu8.9, kernel 6.8, gcc 13.3) with
`cc -std=gnu17 -O1 -o p patterns_diff.c`, `cc -std=gnu17 -O1 -DPATTERNS_COMPAT_GLOB -o pc
patterns_diff.c` and `sh patterns_run.sh ./p SCRATCH ./pc`, twice, with different scratch
directories (`/tmp/patrec1`, `/tmp/patrec2`): the two outputs are byte-identical (SHA-256
`f3782279...79ed2` of the uncompressed text). During development an `-O0` build run from a
scratch directory under the home directory (same file system) printed the same bytes as the
`-O1` build. The whole run takes about 3 s.

`patterns_run.sh` makes the scratch directory, compiles the two locales the probe uses into
`SCRATCH/locales` with `localedef` (`C.UTF-8` from the `C` source and `en_US.UTF-8`; both are
also installed system-wide on the VM, `locale -a` lists `C.utf8` and `en_US.utf8`; nothing was
installed), and runs the program with `env -i PATH=/usr/bin:/bin LOCPATH=SCRATCH/locales` under
`timeout -s KILL`. The program writes its fixture under `SCRATCH/fx`. If `COMPAT_PROGRAM` is
given, its `compat` section is appended (see below). The scratch directory is removed at the end.

**Two parts.** The record's last section, `== glob compat`, comes from the `-DPATTERNS_COMPAT_GLOB`
build, which binds `glob@GLIBC_2.2.5` with `.symver` and so builds only against the host's
dynamic library. Liella's static products cannot select a symbol version (compat versions arrive
in phase 15), so the products comparison covers everything before `== glob compat`, and the
compat part is the oracle for the phase that adds `glob@GLIBC_2.2.5`.

## What the probe does

Strings are printed in brackets with every byte outside 0x20-0x7e, and the backslash, as `\xHH`
(so a pattern `a\*` shows as `[a\x5c*]`). The scratch directory is printed as `<dir>`, the
caller's passwd home directory as `<home>`, the caller's PID as `<pid>` and the program's
`argv[0]` as `<argv0>`. Each section runs in a forked child with an alarm, a parent-death signal
and `PR_SET_DUMPABLE 0` (so a crash writes no core file); calls that may abort or crash run in a
further child, and a line `LABEL: child killed by signal N` says how that child ended. The fixture
directory's file system is printed on `fs:` lines.

* **fnmatch** (`== fnmatch`, lines `fnmatch [PATTERN] [STRING] FLAGS = RESULT`): about 290
  hand-written cases in the C locale (basics, bracket expressions with classes, equivalence
  classes, collating symbols, ranges, invalid and unterminated brackets, escapes, and each flag:
  `NOESCAPE`, `PATHNAME`, `PERIOD`, `LEADING_DIR`, `CASEFOLD`, `EXTMATCH`, plus unknown flag
  bits); then 61 locale cases (ranges, classes, equivalence classes, case folding, invalid
  and truncated UTF-8 in pattern and string) under `C`, `C.UTF-8` and `en_US.UTF-8` (each block
  starts `-- locale NAME: ok`). Then a generated corpus (`fc` lines): every pattern of up to four
  tokens from `a * ? [a] / . [!a]` (2,801 patterns) under seven flag sets (`0`, `PATHNAME`,
  `PERIOD`, `PATHNAME|PERIOD`, `LEADING_DIR`, `PATHNAME|LEADING_DIR`, `NOESCAPE|PATHNAME|PERIOD`),
  each line giving the result for the 40 subjects over `{a, /, .}` of length 0-3 listed on the
  `subjects` line: `0` match, `1` `FNM_NOMATCH`, `E` anything else.
* **glob** (`== glob`, lines `glob [PATTERN] FLAGS -> RESULT pathc=N offs=N magchar=0|1 PATHV`):
  `PATHV` lists `gl_pathv` from index 0 through `gl_offs + gl_pathc`, `(null)` for a null slot and
  `$` for the terminating null pointer, or `pathv=NULL`; `magchar` is `GLOB_MAGCHAR` in
  `gl_flags`. Error-callback calls are printed as `  errfunc [PATH] errno=E -> RETURN` before the
  result line. The fixture (`tree/`, `sort/`, `tilde/`, `words/` under `<dir>`) has files,
  directories, hidden entries, a directory that is searchable but not readable (`noread`, mode
  0300), one that is readable but not searchable (`noexec`, 0600), symbolic links to a directory
  and a file, a dangling link, a self-loop, and names with brackets, `*`, `?`, a backslash,
  braces and `~`. Sections: basics; symbolic links; brackets and escapes; each flag
  (`MARK`, `PERIOD`, `ONLYDIR`, `NOCHECK`, `NOMAGIC`, `DOOFFS`, `APPEND` including appending after
  errors and with inconsistent `DOOFFS`, `ERR` with the callback returning 0 and 1, `BRACE`,
  `TILDE`, `TILDE_CHECK` with `HOME` set, empty and unset); `glob_pattern_p`; unknown flag bits;
  `GLOB_ALTDIRFUNC` over a virtual tree (`glob-alt` lines; `opened:` lists, sorted, the
  directories passed to `gl_opendir`, which is informative, not normative); sorting of 30 names
  chosen to order differently by bytes and by collation (case, accents, a decomposed accent,
  punctuation, digits) under `C`, `C.UTF-8` and `en_US.UTF-8`; directory order with
  `GLOB_NOSORT`; `glob64`.
* **regex** (`== regex`): the constants; `regerror` for codes -1 to 18 and 1000 (each in a
  child), with small and null buffers; `regcomp` results for 71 invalid or borderline
  patterns and an unknown `cflags` bit (`regcomp FLAGS [PATTERN] = CODE`); 317 matching cases
  (`re FLAGS [PATTERN] [SUBJECT] EFLAGS -> RESULT nsub=N (so,eo)...` with every `pmatch` entry up
  to `re_nsub + 1`, flags `B`/`E` plus `I` `REG_ICASE`, `N` `REG_NEWLINE`, `S` `REG_NOSUB`):
  leftmost-longest and subexpression selection, nested and repeated groups, empty matches,
  bounded repetition, back-references, BRE versus ERE syntax, bracket expressions, `REG_ICASE`,
  `REG_NEWLINE` with `REG_NOTBOL`/`REG_NOTEOL`, the GNU operators (`\b \B \< \> \w \W \s \S \` \'`),
  and five pathological patterns without back-references; `re-all` lines, every successive match
  of a search loop (restart at the end of the match, one further after an empty match, with
  `REG_NOTBOL`); `REG_NOSUB` and `nmatch` handling (entries prefilled with 77);
  `re-startend` lines (`REG_STARTEND` on `xa\0ab\0c` and others, `pmatch` after the call);
  the twelve character classes over bytes 1-255 with and without `REG_ICASE` in `C` and
  `C.UTF-8`; 34 cases per locale under `C`, `C.UTF-8` and `en_US.UTF-8`.
* **regex corpus** (`== regex corpus`, lines `rc FLAGS [PATTERN] CODE NSUB RESULTS`): every
  pattern generated from a token set, compiled; `CODE` is `regcomp`'s result and, when it is 0,
  `RESULTS` gives for each subject on the preceding `subjects` line either `-` (no match) or the
  `so.eo` pairs of all `re_nsub + 1` entries joined by commas. Four corpora: ERE over
  `a b ( ) | * + ?` up to five tokens (37,449 patterns, 9,809 compile) against the 15 subjects
  over `{a, b}` of length 0-3; ERE over `a b ( ) | {0,1} {2} \1` up to four tokens (4,681; 1,010
  compile) against the same subjects; BRE over `a b \( \) * \1` up to five tokens (9,331; 753
  compile) against the 31 subjects of length 0-4; ERE with `REG_ICASE` over `a B ( ) | * \1` up to
  four tokens (2,801; 685 compile) against the same 31 subjects.
* **GNU and BSD interfaces** (`== GNU regex interface`): `re_syntax_options` and the `RE_SYNTAX_*`
  values; `re_search` (with the start, range and returned registers) and `re_match` at the start
  position under several syntaxes, compile error strings, backward and out-of-range searches,
  NUL in the buffer, anchors and word operators; `re_search_2`/`re_match_2` across split buffers;
  `re_set_registers` with caller arrays; `re_compile_fastmap`; `re_comp`/`re_exec`, including
  before any pattern and after a failed `re_comp` (in children).
* **wordexp** (`== wordexp`, lines `wordexp [WORDS] FLAGS -> RESULT wordc=N offs=N WORDV`, then
  `  stderr [TEXT]` with what the call wrote to descriptor 2): runs in `<dir>/words` (files `w1`,
  `w2`, `.w3`, `w*`) with `HOME=<dir>/tilde`, `PATH=/usr/bin:/bin` and variables `X="a b"`,
  `Y=""`, `S="  lead  trail  "`, `C="x:y::z"`, `G="w*"`, `N=3`, `P=path/to/file.tar.gz`,
  `Q="'q' \"r\""`, `M="xéyéz"`, `D=" a : b :: c "`, `U` unset, `IFS` unset unless said.
  Sections: words and quoting; parameter expansion (all POSIX forms, special parameters, bash
  forms); empty and unset variables inside double quotes; arithmetic; tilde (also with `HOME`
  empty and unset); field splitting under eight `IFS` values (`IFS=` lines); pathname expansion;
  command substitution; `WRDE_SHOWERR`; `WRDE_NOCMD` with every form of command substitution;
  `WRDE_BADCHAR` for each of `| & ; < > ( ) { }` and newline (bare, inside a word, single- and
  double-quoted, escaped, in `${U:-...}`, in `$(...)`, with `NOCMD`); `WRDE_SYNTAX`;
  `WRDE_UNDEF`/`WRDE_BADVAL`; the state left in the `wordexp_t` after each kind of error
  (`wordexp-state` lines: `we_wordc` starts at 777 and `we_wordv` at NULL; then `wordfree` runs
  in a child, `wordfree afterwards: ok` meaning it returned normally), `WRDE_APPEND` after errors
  (`append-err#N`), `WRDE_DOOFFS` with `APPEND` (`doofs-append#N`), `WRDE_REUSE` (`reuse#N`),
  `wordfree` twice and on a zeroed structure, and an unknown flag bit.
* **glob compat** (`== glob compat`, from the compat build): `glob@GLIBC_2.2.5` (`glob@2.2.5` and
  `glob-alt@2.2.5` lines) and then the default `glob` on the same pattern: dangling links, loops
  and links on the real file system, then through `GLOB_ALTDIRFUNC` with a virtual dangling link
  `v/dang` for which `gl_lstat` succeeds and `gl_stat` fails with `ENOENT`
  (`v/dang: gl_stat called|not called, gl_lstat called|not called`).

## Observations the record shows directly

### fnmatch (§14.7)

* Return values are 0 and `FNM_NOMATCH` (1) only: no input in the record, including unterminated
  brackets, unknown classes, invalid UTF-8 and unknown flag bits, gives another value (no `E` in
  the `fc` lines). Unknown flag bits are ignored (`fnmatch [a] [a] 0x400 = 0`).
* `*` matches the empty string; `?` needs one character; `[!...]` and `[^...]` both negate; `]`
  first (also after `!`/`^`) is literal; `-` first or last is literal; a reversed range `[c-a]`
  matches nothing (not even its end points); `[a-c-e]` is `a-c`, `-`, `e`.
* An unterminated bracket makes `[` an ordinary character: `[` matches `[`, `[a` matches `[a`,
  `[!` matches `[!`, `[]` matches `[]`, `[!]` matches `[!]`; `*[` and `[*` match `x[`/`[x`.
* Classes: `[[:alpha:]]` etc. as in the locale. An unknown class name `[[:foo:]]` or
  `[[:ALPHA:]]` matches nothing (not an error); `[[:alpha:]` (missing `]`) matches nothing;
  `[[:alpha]` is the set of the characters `[:alph` (it matches `a`, not `:]`); a class as a range end point (`[a-[:alpha:]]`)
  never matches, while `[[:alpha:]-z]` matches `-`.
* Equivalence classes and collating symbols in `C`: `[[=a=]]` and `[[.a.]]` are `a`; `[[.-.]]`
  is `-`; multi-character or named symbols (`[[.ab.]]`, `[[.hyphen.]]`, `[[.space.]]`,
  `[[=ab=]]`) match nothing; `[[.a.]-c]` and `[a-[.c.]]` are ranges. Unterminated forms: `[[=]`
  and `[[:]` match `[`, `[[.]` does not.
* Escapes: `\x` matches `x`; a trailing `\` matches nothing (`[\x5c] [\x5c] 0 = 1`,
  `[a\x5c] [a\x5c] 0 = 1`); inside brackets `\` escapes (`[\]]` matches `]`, `[a\-c]` is `a`, `-`,
  `c`). With `FNM_NOESCAPE` the backslash is ordinary everywhere, and a trailing `\` matches `\`.
* `FNM_PATHNAME`: `*`, `?` and bracket expressions (`[/]`, `[!x]`, `[a/]`, `[[:punct:]]`) never
  match `/`; an escaped `\/` matches `/`; slashes must correspond one to one (`a//b` against
  `a/b` fails both ways).
* `FNM_PERIOD`: a leading `.` is matched only by a literal `.` or `\.` (not `*`, `?`, `[.]`,
  `[!a]`, `[[:punct:]]`); without `FNM_PATHNAME` only the first character of the string is
  leading (`[a/*] [a/.b] PERIOD = 0`); with it, a `.` after `/` is leading too.
* `FNM_LEADING_DIR`: the pattern may match a prefix of the string that ends just before a `/`
  (`a` against `a/b`, `a/`, `a//`; `a/b` against `a/b/c/d`; the empty pattern against `/a`), not
  one ending elsewhere (`a` against `ab`); `a/` against `a/b` fails. Without `FNM_PATHNAME`, `*`
  may cross slashes before the leading-directory cut (`[*b] [a/b/c] LEADING_DIR = 0`, but 1 with
  `PATHNAME`).
* `FNM_CASEFOLD`: literals, ranges and escapes fold (`[a-c]` matches `B`, `[A-C]` matches `b`,
  `\A` matches `a`); classes do not (`[[:upper:]]` against `a` and `[[:lower:]]` against `A` are
  1); `[!a]` excludes `A`; `[[=a=]]` and `[[.a.]]` do not fold; `[Z-a]` matches `_` without
  `CASEFOLD` and neither `_` nor `z` with it.
* `FNM_EXTMATCH`: `?()` zero or one, `*()` zero or more, `+()` one or more, `@()` exactly one,
  `!()` anything except the alternatives (including the empty string and longer strings); nested
  and empty alternatives work (`@()`, `@(|a)`, `!(!(a))`, `*(*(a)b)`). Without the flag the
  operators are literal characters followed by a group (`[*(a)] [x(a)] 0 = 0`). An unterminated
  `?(a` is literal (`[?(a] [?(a] EXTMATCH = 0`). `*(a/b)` with `FNM_PATHNAME` matches `a/b`;
  `!(x)` matches `a/b` with `PATHNAME`, and `!(a)` matches `/`. `!(x)` matches `.a` under
  `PERIOD`, `*(x)` does not. Inside a group, `|` is not escapable (`@(a\|b)` matches neither
  `a|b` nor `a`), and `\)` does not escape `)` (`@(a\)b)` does not match `a)b`); bracketed
  `[|]` and `[)]` work.
* Locales (`-- locale` blocks). `C`: bytes; `?` against a two-byte UTF-8 character fails and `??`
  matches. `C.UTF-8` and `en_US.UTF-8`: `?` is one character; classes are Unicode-aware
  (`[[:alpha:]]` matches `é`, `[[:upper:]]` matches `É`); `CASEFOLD` folds `é`/`É`.
* Ranges by locale: in `C` and `C.UTF-8` ranges are code-point order (`[A-z]` matches `_` and `b`,
  `[a-Z]` matches nothing, `[Z-a]` matches `_`); in `en_US.UTF-8` they follow collation order
  (`[a-z]` matches `é` but not `B`, `[a-Z]` matches `b`, `[A-z]` matches neither `_` nor `b`,
  `[Z-a]` does not match `_`, `[a-A]` matches `a`). `[à-ÿ]` matches `é` in both UTF-8 locales
  (not in `C`, where the pattern is bytes). `[b-é]` matches `c` and none of `ê`, `B`, `C` in all
  three locales; it matches `à` in `C.UTF-8` (code-point order: U+00E0 lies between `b` and `é`)
  and not in `C` or `en_US.UTF-8` (corrected 2026-10-03: an earlier version said `à` was excluded
  in all three; the `[[b-\xc3\xa9]]` lines).
* Equivalence classes: `C.UTF-8` treats `[[=e=]]` as `e` only, but `[[=é=]]` matches `é` and not
  `e`; `en_US.UTF-8` makes `e`, `E`, `é`, `É` one class (`[[=e=]]` matches `é` and `E`,
  `[[=E=]]` matches `e`). `[[.é.]]` matches `é` in both UTF-8 locales; `[[.ch.]]` matches nothing
  in all three.
* Invalid multibyte text in a UTF-8 locale (no error is reported): `*` and `?` match a lone
  `\xff` byte and a lone `\xc3`; `\xff` in the pattern matches itself; `[!a]` matches `\xff`; but
  `?` does not match the truncated three-byte sequence `\xe2\x82`, and `[[:alpha:]]` does not
  match `\xff`. A pattern holding only the first byte of a character (`\xc3*`, `\xc3?`) matches
  `é`, and `[é]` matches the lone byte `\xc3` (in all three locales).
* The corpus (`fc` lines) is the exact matrix for the six basic tokens: it fixes, for example,
  that `*` with `PATHNAME|PERIOD` matches `` (the empty string) and `a.` but not `.`, `/` or
  `a/`, and the `LEADING_DIR` cut points.

### glob (§14.8)

* Results are sorted and the vector is terminated by a null pointer; `gl_pathc` excludes the
  `gl_offs` slots. `GLOB_NOMATCH` (3) leaves `gl_pathc` 0 and `gl_pathv` NULL (without
  `DOOFFS`); with `DOOFFS` the vector holds the null slots and the terminator.
* `GLOB_MAGCHAR` in `gl_flags` is set only when the **last** pattern component has a
  metacharacter: `tree/*/../file1` and `t*/d2` report `magchar=0`; `tree/\x5c[ab\x5c]` (only
  escaped metacharacters) reports 1, as does `tree/no\x5cmatch` with `NOCHECK` (a backslash
  alone), but not with `NOESCAPE`.
* `""` is `GLOB_NOMATCH` (and `[""]` with `NOCHECK`); `/` and `//` give `/`; `tree//file1` and
  `tree//file*` keep the double slash; `./` and `..` are kept as written.
* A trailing `/` matches directories only and is kept (`tree/*/` gives `d1/ d2/ link_d1/
  noexec/ noread/`); but a pattern without metacharacters ending in `/` that names a file or a
  dangling link returns the name **without** the slash (`glob [tree/file1/] 0 -> 0 ...
  [tree/file1]`, `[tree/dangling/] ... [tree/dangling]`).
* Existence checks for patterns without metacharacters do not follow links: `tree/dangling` and
  `tree/loop` are returned (`magchar=0`). Wildcard patterns list dangling links and loops.
  Directories are entered through a symbolic link (`tree/*/*` includes `link_d1/...`).
* Brackets and escapes: `[ab]` is a set; `\[ab\]` and `\[ab]` match the file `[ab]`, as does
  `[[]ab]`; an unterminated `[x` matches the file `[x` literally (and `[x*` matches it too),
  `tree/[` is a literal name (no such file: NOMATCH), and `[*` matches `[ab]` and `[x`.
  A trailing backslash makes the pattern match nothing (`tree/file1\x5c`, `tree/file*\x5c`).
  `[^a]` negates like `[!a]`. `*(1)` is not an extended pattern.
* `GLOB_NOESCAPE`: `\` is literal (`tree/b\x5cs` then matches the file `b\s`; `q\x5c*` matches
  nothing). Without it `tree/b\x5cs` matches `bs` and `b\x5c\x5cs` matches `b\s`.
* `GLOB_MARK`: a `/` is appended to directories, symbolic links to directories (`link_d1/`), and
  `tree/.` (`tree/./`), not to files, dangling links or a `NOCHECK` result; a name already ending
  in `/` is not doubled.
* `GLOB_PERIOD`: wildcards match a leading `.`, and the lists then include `.` and `..`
  (`tree/*` with `PERIOD`, `tree/*/*` with `PERIOD`). A pattern starting with a literal `.`
  includes `.` and `..` with or without the flag (`tree/.*`, `tree/d1/.*`, `tree/.?` gives
  `..`).
* `GLOB_ONLYDIR`: with a wildcard, only directories and links to directories are returned
  (`tree/*`, `tree/[dl]*`); a pattern without metacharacters naming a file is still returned
  (`glob [tree/file1] ONLYDIR -> 0 ... [tree/file1]`), as is a `NOCHECK` result.
* `GLOB_NOCHECK` returns the pattern exactly as given, backslashes included
  (`[tree/no\x5cmatch*]`, `[~/nomatch]` with `TILDE`), with `pathc=1`; `""` gives `[""]`.
  `GLOB_NOMAGIC` returns the pattern only if it has no `*`, `?` or `[`, and no backslash
  (`tree/no\x5cmatch` with `NOMAGIC` is NOMATCH; `tree/no[` too); `""` with `NOMAGIC` is NOMATCH.
* `gl_offs` is ignored without `GLOB_DOOFFS` and reset to 0 (`glob [tree/file*] 0` with
  `gl_offs=5` prints `offs=0`). `GLOB_APPEND` appends the new matches, sorted among themselves
  only (`append-sorting#2`, `#3`); a call that finds nothing returns 3 and leaves the vector as it
  was; `APPEND` on a structure that holds a NOMATCH result works. `abort-append#2`: an aborted
  `APPEND` call frees the previous results (`pathc=0 ... pathv=NULL`). `globfree` sets
  `gl_pathv` to NULL and accepts a zeroed structure.
* Errors: a directory that cannot be read gives the error callback the directory path and
  `errno` (`[tree/noread] errno=13`); without `GLOB_ERR` and with a callback returning 0 the walk
  goes on and the directory contributes nothing; with `GLOB_ERR`, or a callback returning
  nonzero, the result is `GLOB_ABORTED` (2) with no partial results. A missing directory is
  reported too (`[tree/nodir] errno=2`, `[tree/dangling] errno=2`, `[tree/loop] errno=40`), and
  aborts with `GLOB_ERR`; a path component that is a file is not reported (`tree/file1/*` gives
  NOMATCH even with `ERR`). The unreadable-but-searchable directory still resolves names without
  metacharacters (`tree/noread/z`, `tree/*/z` succeed without a callback), and the
  unsearchable-but-readable one lists entries (`tree/noexec/*` gives `noexec/w`) but a literal
  `tree/noexec/w` is NOMATCH, also without a callback.
* `GLOB_BRACE`: alternatives expand left to right, each alternative's matches sorted on their own
  and the groups concatenated in the order written (`{d2,d1}` gives `d2 d1`; `{file*,d*}` gives
  the `file` matches first); nested braces, `{}` (gives `tree/`), empty alternatives
  (`{,file1}` gives `tree/` and `file1`), duplicates (`{file1,file1}` gives two) are all
  expanded. An alternative without metacharacters that does not exist is dropped
  (`{d1,nomatch}` gives `d1`); if none matches, NOMATCH, or the whole unexpanded pattern with
  `NOCHECK` or `NOMAGIC`. Unbalanced braces are NOMATCH; `\{a,b}` matches the file `{a,b}`;
  `{a\,b}` and `[{]a,b}` are NOMATCH (braces are found before brackets are considered).
* `GLOB_TILDE`: `~` and `~/...` use `HOME` (`<dir>/tilde`); with `HOME` empty or unset, the
  passwd home of the caller (`[<home>]`). `~root` is `/root`; an unknown user leaves the pattern
  unexpanded and **it is returned with result 0 without any existence check and without
  `NOCHECK`** (`glob [~nosuchuser_zq] TILDE -> 0 pathc=1`; no such file exists);
  `GLOB_TILDE_CHECK` makes that `GLOB_NOMATCH` (also with `NOCHECK`),
  and `TILDE_CHECK` alone enables tilde expansion. A tilde not at the start, `\~`, and `~` without
  the flags are literal; `~\x5c/t1` expands; `~*` and `~ro*` are not expanded; `~` inside braces
  is expanded (`{~,tree}`). `~/x` with `HOME` empty and `NOCHECK` returns `[~/x]`.
* `glob_pattern_p` is 1 for `*`, `?` and a terminated `[...]`, 0 for an unterminated `[`, `]`,
  braces and `~`; with `quote=1` an escaped metacharacter does not count (`a\*` 0), with
  `quote=0` it does.
* Unknown flag bits (`1<<20`, any bit above `GLOB_TILDE_CHECK`) make `glob` return -1 with
  `errno` `EINVAL`, which is none of the documented return values.
* `GLOB_ALTDIRFUNC`: results come only from the callbacks. Entries whose `d_type` is
  `DT_UNKNOWN` are listed by wildcards and resolved through the stat callbacks for `MARK` and
  `ONLYDIR` (`v/ud/` gets the slash). An `EACCES` from `gl_opendir` reaches the error callback
  and gives NOMATCH, or `GLOB_ABORTED` with `GLOB_ERR`. `v/dang` (a link for `gl_lstat`, missing
  for `gl_stat`) is returned for `v/dang`, `v/dang` with `MARK` (no slash), `v/da*`, and
  `v/{dang,x}`; it is dropped with `ONLYDIR`.
* Sorting (`-- sorting under each locale`, the `glob [sort/*] 0` lines): `C` and `C.UTF-8` sort
  by bytes (`#a -a 1 10 9 A Ab B E Z _a a a\x20b a-b a.b a1 a10 a9 aB a_b ab b e e\xcc\x81 f z ~a`
  then `É ä é` by their UTF-8 bytes); `en_US.UTF-8` sorts by collation (`1 10 9 #a -a _a ~a a A
  ä a1 a10 a9 a\x20b a-b a.b a_b ab aB Ab b B e E e\xcc\x81 é É f z Z`), and with `PERIOD` it
  puts `.` and `..` first but `.x` between `f` and `z` (punctuation ignored at the first
  level). Bracket matching in the same lines follows the locale (`sort/[a-z]` includes `ä` and `é` only in `en_US.UTF-8`;
  `[[=e=]]` gives four names there and one elsewhere; `?` matches two-byte names only in the UTF-8
  locales). Brace groups are not merged even when sorted (`sort/{z,a,Z,A}` keeps that order).
* **Directory order** (`GLOB_NOSORT`): the lines after `-- directory order` and the
  `glob [tree/{d*,file*}] NOSORT|BRACE` line in the `GLOB_BRACE` block follow the directory
  order of the scratch file system, which the record names: `fs: scratch directory
  f_type=0xef53 (ext2/ext3)`, an ext4 file system on the VM (`stat -f -c %T` prints
  `ext2/ext3` for ext4). These orders are a property of that file system (on ext4, of the hash
  order of the names in that file system), not of the library; with `NOSORT` and `BRACE`, the
  alternatives still come in the written order and only the entries within one alternative are in
  directory order. `tree/*/*` with `NOSORT` lists each subdirectory's entries together, the
  subdirectories in directory order.
* `glob64`/`globfree64` behave as `glob`/`globfree` (`glob64 [tree/file*]` line).

### glob@GLIBC_2.2.5 versus the default version (§14.8 ⚑ COMPAT)

* On the real file system the two versions give identical results for every case in the
  `compat` section (dangling links with and without `NOCHECK`, `MARK`, `NOMAGIC`, wildcards,
  `ONLYDIR`, loops, braces, escapes).
* The difference shows through `GLOB_ALTDIRFUNC`: for a pattern without metacharacters, the old
  version decides existence with `gl_stat` and the default version with `gl_lstat`. So
  `v/dang` is NOMATCH for `glob@2.2.5` and found by `glob` (`v/dang: gl_stat called, gl_lstat not
  called` versus `gl_stat not called, gl_lstat called`); the same for `v/dang` with `MARK` (the
  default version then calls both: `gl_lstat` for existence, `gl_stat` for the mark) and for the
  `v/dang` alternative of `v/{dang,x}`. With `NOCHECK` neither is called and both return the
  pattern. Wildcard patterns (`v/da*`, `v/*`) behave identically in both versions.

### Regular expressions (§14.9)

* `regerror` returns the length of the full message plus one whatever the buffer size, writes a
  truncated, terminated message into a small buffer (`size=5` gives `Unma` and a NUL, `size=1` a
  NUL only), and accepts `NULL, 0`. The texts for codes 0-16 are on the `regerror N` lines
  (`Success`, `No match`, `Invalid regular expression`, ..., `Unmatched ) or \)`). **Codes outside
  0-16 (-1, 17, 18, 1000) abort the process** (`regerror -1: child killed by signal 6`). The
  `preg` argument does not change the text.
* `regcomp` error codes (`regcomp` lines): unknown class `REG_ECTYPE`; trailing backslash
  `REG_EESCAPE`; back-reference to a group that does not exist or is not closed yet
  (`(a)\2`, `\1`, `(\1)`, `(a\1)`, `\1(a)`) and `(a)|\1` `REG_ESUBREG`; unterminated bracket
  (`[a`, `[]`, `[^]`, `[[:alpha:]`, `[[:alpha]`, `[[=]`, `[[.]`, `[a-`) `REG_EBRACK`; unmatched
  `(` or BRE `\)` `REG_EPAREN` (`REG_ERPAREN` is never returned); unterminated `{`/`\{`
  `REG_EBRACE`; bad interval contents (`{2,1}`, `{x}`, `{-1}`, `{1,2,3}`, `{}`, `{1,x}`)
  `REG_BADBR`; reversed range and a class as a range end point `REG_ERANGE`; an unknown or
  multi-character collating element or equivalence class (`[[.foo.]]`, `[[.ab.]]`,
  `[[=ab=]]`, `[[.hyphen.]]`, `[[.space.]]`) `REG_ECOLLATE`; a count above `RE_DUP_MAX` (32767)
  `REG_ESIZE` (`a{32768}`, `a{1,32768}`, `a{65536}`, `a{99999999999}`), while `a{32767}`
  compiles; ERE `[` alone `REG_BADPAT`. `REG_BADRPT` for an ERE repetition operator with nothing
  before it (`*a`, `+`, `?`, `{1}`, `^*`, `a|*`, `(*)`, `(+a)`), for BRE `\{1\}` at the start,
  for BRE `a**`, and for BRE intervals that follow another repetition (`a\{1\}\{2\}`,
  `a*\{2\}`). Perl syntax `(?:` and `(?i)` is `REG_BADRPT`. An unknown `cflags` bit (0x100) is
  ignored.
* Accepted in ERE: `a**`, `a+*`, `a*+`, `a?*`, `a{1}{2}`, `a*{2}`, `a{,}` (as `a*`), `a{,1}`,
  `a)` and `)` (literal `)`), `]`, `}`, `a{1}}`, `[--a]`, empty patterns and empty alternatives
  (`a||b`, `(|a)`, `|a`, `a|`), `()`. ERE `a{`, `a{x`, `a{1` are `REG_EBRACE`, `{1}` `REG_BADRPT`.
  BRE: `*` first, after `\(`, and after `^` is literal; `\+`, `\?` and `\|` are operators;
  `a\{,2\}` is accepted.
* `re_nsub` counts every `(`/`\(` group, also under `REG_NOSUB` (`nsub=2`).
* Unmatched subexpressions are (-1,-1): an untaken alternative (`(a)|b` on `b`), an optional
  group not taken (`(a)?b`), a group under `{0}` (`(a){0}`), a starred group that iterated zero
  times (`(a|b)*` on `""`).
* `regexec` writes exactly `nmatch` entries: `nmatch=1` leaves the others untouched, entries
  beyond `re_nsub` are set to -1; on no match nothing is written; under `REG_NOSUB` nothing is
  written on a match either, and `nmatch=0` with a NULL `pmatch` is accepted.
* Overall match: leftmost, then longest (`(.*)c(.*)` on `abcdcef` is (0,7); `x*` on `yxx` is
  (0,0) at the leftmost position, not the longer later one; `x*$` on `yxx` is (1,3)).
* **Subexpressions** (the `re` lines under `-- matching` and the whole corpus are the oracle;
  the main facts):
  * A repeated group reports its last iteration: `(a|b)*` on `abab` gives (3,4); `(ab){1,3}` on
    `ababab` gives (4,6); `((a)|b)+` on `ab` gives (1,2) for the outer group and (0,1) for the
    inner one, i.e. **an inner group keeps its value from an earlier iteration when the last
    iteration does not take it** (also `(a(b)?)+` on `aba`: (2,3)(1,2); `((a)|(b))*` on `ab`:
    (1,2)(0,1)(1,2)).
  * Alternation does not maximise the earlier subexpression: `(a|ab)(c|bcd)(d*)` on `abcd`
    gives (0,1)(1,4)(4,4), and the same pattern with the alternatives swapped,
    `(ab|a)(c|bcd)(d*)`, gives (0,2)(2,3)(3,4); `(a|ab)(b*)` gives (0,1)(1,3) and `(ab|a)(b*)`
    gives (0,2)(2,3). The overall match is the same; the split depends on the order of the
    alternatives. Concatenated stars do maximise the earlier one (`(a*)(a*)` gives (0,3)(3,3),
    `(.?)(.?)(.?)` on `ab` gives (0,1)(1,2)(2,2)).
  * Empty iterations: `(a*)*` and `(a*)+` on `b` give (0,0)(0,0); on `aa` (0,2)(0,2); `()*` gives
    (0,0)(0,0); `(a?)?b` on `b` gives (0,0) for the group (an empty iteration is recorded), while
    `(a)?(b)?` on `""` gives -1 for both; `(()|a)*` on `aa` gives (1,2)(0,0) but `(a|())*` on `aa`
    gives (1,2)(-1,-1); `(a*)*(b)` on `aab` gives (0,2)(2,3); `((a*)*)*` on `a` gives (0,1) for
    all three.
  * Bounded repetition: `(a){2}` on `aaa` (1,2); `(a*){2}` on `aa` gives (2,2) (second
    iteration empty) but `(a*){2,3}` on `aa` gives (0,2); `(a?){3}` on `aa` gives (2,2);
    `(a|ab){2}` on `abab` gives (2,4); `(a|ab){1,2}(c)?` on `abac` gives (2,3)(3,4).
  * Back-references: `(a*)\1` on `aaaa` gives (0,4)(0,2), on `aaa` (0,2)(0,1); `(a*)\1b` on `aaab`
    starts at 1; `((a)|b)\2` fails on `ba` (the inner group is unset); `((a)|b)*\2` on `aba`
    matches (0,3) with the inner group (0,1) from an earlier iteration; `(a|b)*\1` matches `abb`
    (0,3)(1,2) but not `aba`; `(a)?\1` fails on `""` and `(x)?(a)\1` fails on `a` (a reference to
    an unset group never matches); `(a*)*\1` on `aa` gives (0,2)(0,1); `(a*)(a*)\2` on `aaaa`
    gives (0,4)(4,4) and `(a*)(a*)\1` gives (0,4)(0,2)(2,2); `(.)\1` finds `bb` at 1; `\9` works.
    BRE back-references behave the same (`\(a*\)\{2\}` on `aaaa` gives (4,4) for the group).
* Anchors: in ERE `^` and `$` are anchors anywhere (`a^`, `$a`, `a$b` never match; `x(^a)` does
  not match `xa`; `(^a)` matches); `\$` is literal. In BRE, `^` is an anchor only at the start
  of the pattern or of a group (`\(^a\)` matches `a`, `x\(^a\)` does not match `x^a`, `a^b`
  matches literally) and `$` only at the end or before `\)`.
* `REG_NEWLINE`: `.` and non-matching lists (`[^x]`, `[^[:alpha:]]`) do not match newline, `^`
  and `$` match after and before it; a newline in the pattern or in a bracket matches;
  `[[:space:]]` and `[[:cntrl:]]` still match it. `REG_NOTBOL`/`REG_NOTEOL` affect only the
  string's start and end: `^` still matches after a newline with `REG_NEWLINE|REG_NOTBOL`;
  `^$` on `""` with both fails. `\`` and `\'` ignore `REG_NOTBOL`, `REG_NOTEOL` and
  `REG_NEWLINE` (buffer anchors).
* `REG_ICASE`: literals, ranges and back-references fold (`(a)\1` matches `aA`); `[[:lower:]]`
  matches `A` and `[[:upper:]]` matches `a`; negated sets exclude both cases (`[^a]` does not match
  `A`, `[^A-Z]` does not match `q`, `[^[:lower:]]` does not match `a`); `[Z-a]` is `REG_ERANGE`
  under `ICASE` but valid without it. The class lines show `upper` and `lower` becoming
  `41-5a 61-7a` under `ICASE`, the other classes unchanged.
* GNU operators through `regcomp` (both syntaxes): `\b \B \< \> \w \W \s \S \` \'` work as word,
  space and buffer anchors; `\b` and `\<` do not match the empty string, `\B` does. Other escaped
  ordinary characters in ERE stand for themselves (`\a`, `\n` is `n`, `\t` is `t`, `\%`); `\.`
  is a literal dot. In brackets `\` is literal (`[\]` matches `\`).
* Bracket expressions in `regcomp`: `]` first is literal, `[a-]` and `[]-a]` as written
  (`[]-a]` is the range `]`-`a`, so `-` does not match); `[c-a]` and `[a-c-e]` are `REG_ERANGE`;
  `[[]` and `[a[]` match `[`; `[[.].]]` matches `]`; `[[.^.]a]` matches `^`.
* `REG_STARTEND` (`re-startend` lines): the search covers `[so, eo)` of the buffer, NUL bytes
  included (`(a)(b)` finds `ab` after a NUL, `[^x]` and `[[:cntrl:]]` match NUL), except that
  **`.` does not match NUL** (`a.a`, `a(.)a` fail; `.*` from 0 stops at the NUL, (0,2)); and a
  back-reference across a NUL (`\(a\).\1`) fails for the same reason. Offsets in `pmatch` are
  from the start of the buffer, not from `so`. `^` does not match at `so > 0` and `\`` does not
  either (`so=3`), but `REG_NOTBOL` is irrelevant there; `$` matches at `eo` (unless
  `REG_NOTEOL`); `\<` matches at `so` after a NUL, and `\b`/`\B` at `so=1` look at the byte
  before `so` (`xa`: `\ba` fails, `\Ba` matches). `so > eo` gives `REG_NOMATCH`. On no match,
  `pmatch[0]` keeps the caller's `so`/`eo`. With `REG_NEWLINE`, `^` matches after a newline
  inside the range.
* Locales (`-- locale dependent`): in `C`, `.` and bracket expressions work on bytes (`.` on `é`
  is (0,1), `^.$` fails, a `\xff` byte matches `.`). In `C.UTF-8` and `en_US.UTF-8`, `.` and
  brackets match whole characters ((0,2) for `é`), offsets are in bytes, `REG_ICASE` folds
  `é`/`É`, classes and `\w` are Unicode-aware; an invalid byte (`\xff`) matches neither `.` nor
  `[^a]` nor `[\xff]`, but a literal `\xff` in the pattern matches it, and a pattern holding a
  lone lead byte `\xc3` matches the first byte of `é` while `\xc3.` does not match `é`.
  `[[.é.]]` and `[[.ch.]]` are `REG_ECOLLATE` everywhere. Ranges: `C.UTF-8` rejects a range with a
  non-ASCII end point (`[\xc3\xa0-\xc3\xbf]` and `[b-é]` are `REG_ECOLLATE`) and accepts `[A-z]`;
  `en_US.UTF-8` uses collation order (`[a-z]` matches `é`, `[A-z]` is `REG_ERANGE`, `[a-Z]`
  matches `b`); `[[=e=]]` matches `é` and `E` only in `en_US.UTF-8`.
* `re-all` lines: an empty match right after a non-empty one is found (`x*` on `axxbx`: (0,0)
  (1,3) (3,3) (4,5) (5,5)); `^a` with `REG_NOTBOL` matches only once; `\<` on `ab cd` matches at
  0, 1, 3 and 4: `REG_NOTBOL` does not stop `\<` at the start of a restarted string (the probe
  passes `subject + pos`, so the library cannot see the preceding character).
* Pathological patterns without back-references (`(a*)*b`, `(x+x+)+y`, `(a|aa)*c` on 40
  characters, `(a?){30}a{30}`, `(.*)*x` on 100 characters) give their results; outside the
  record, the whole regex section (about 600 calls) runs in about 30 ms on the VM, so none of them
  is slow on the reference.

### GNU and BSD interfaces (§14.9)

* `re_syntax_options` starts at 0 (`RE_SYNTAX_EMACS`); the `syntaxes` line gives the
  `RE_SYNTAX_*` values. `re_compile_pattern` returns NULL or the `regerror` text of the error
  (`Unmatched ( or \(`).
* `re_search` returns the start position of the leftmost match and fills the registers
  (`num_regs` is `re_nsub + 2` on first allocation: `regs=2` for no group, 5 for three groups);
  -1 for no match or a start outside the buffer; a negative range searches backwards from
  `start` (`start=5 range=-5` finds 4); a range past the end is clipped. Subexpressions follow
  the same rules as `regexec` under every syntax (`(a|ab)(c|bcd)(d*)` gives (0,1)(1,4)(4,4) under
  `POSIX_EXTENDED` and `EMACS`). The overall match is the longest (`a|ab` on `xab` gives (1,3)).
* `re_match` returns the length matched at exactly `start`, or -1.
* With `RE_SYNTAX_POSIX_EXTENDED`, `.` does not match NUL (`a.b` on `a\0b` fails) and `\1` is a
  back-reference; with `RE_NO_BK_REFS` added it is not (`(a)\1` does not match `aa`). `RE_ICASE`
  folds case. Without `RE_DOT_NEWLINE`, `.` does not match a newline. `EMACS` syntax takes `|`
  literally and `\|` as alternation, with `+` an operator (`a+` on `aa+` gives (0,2)); `GREP`
  takes `\|`; `EGREP` takes `+` and `|`.
* `^` and `\`` do not match at a `start` inside the buffer (`^b` on `ab` from 1 fails); `$` and
  `\'` match at the end; `\<`, `\>`, `\b`, `\w` work.
* `re_search_2` treats the two buffers as one string (`xab`+`bcy`: match at 1, registers (1,5) and
  (2,4) in the joined offsets); `stop` limits the match end (`stop=4` gives -1); `re_match_2`
  matches across the boundary, also with an empty first buffer.
* `re_set_registers` with caller-allocated arrays of 3: `re_search` fills them and `num_regs`
  stays 3. `re_compile_fastmap` returns 0 with a caller fastmap, searching still works, and
  `regfree` frees the fastmap and sets the pointer to NULL.
* `re_comp(NULL)` before any pattern returns `No previous regular expression`; after a
  successful `re_comp`, `re_comp(NULL)` returns NULL and keeps the pattern. `re_comp` uses the
  current `re_syntax_options` (here 0, Emacs syntax: `a|b` matches the text `a|b`, `a\|b` matches
  `b`); `re_exec` returns 1 or 0. `re_comp("")` succeeds and matches everything. A failed
  `re_comp` returns the message, after which `re_comp(NULL)` again says `No previous regular
  expression`. **`re_exec` with no compiled pattern (before any `re_comp`, or after a failed
  one) crashes with `SIGSEGV`** (`re_exec before any re_comp: child killed by signal 11`).

### wordexp (§14.10)

* Words: blanks separate words; quotes and backslashes are removed; `''` and `""` give an empty
  word; `\` at the end is `WRDE_SYNTAX`; backslash-newline is removed (also inside double
  quotes); `#` is not a comment (`a #b` gives two words, `#b` included); a lone `$`, `$ X` and a
  trailing `$` are literal; multibyte text passes through.
* Parameter expansion: `$X`, `${X}`, `${X:-w}`, `${X-w}`, `${X:+w}`, `${X+w}`, `${#X}`, `${X%p}`,
  `${X%%p}`, `${X#p}`, `${X##p}` (quoted patterns work), `${X:=w}` and `${X=w}`, which **set the
  variable in the caller's environment** (`after ${U:=set}: getenv(U)=set`; a later `$U` sees it).
  Results of unquoted expansions are field-split but **not** pathname-expanded (`$G` with
  `G=w*` gives the word `w*` while `w1` and `w2` exist; corrected 2026-10-03: an earlier version
  said they were pathname-expanded); `"$X"` is one word. A
  word that expands to nothing unquoted disappears (`$U`, `$Y`, `$Y$Y`). Unquoted text in the
  default value is split (`${U:-a b}` two words; `${U:-'a b'}` and `${U:-"x y"}` also two),
  `~` in a default is expanded, and `|`, `)`, newline and `{` are allowed inside `${...}`;
  `${U:-}}` is `WRDE_BADCHAR` (the first `}` closes) while `${U:-\}}` works. Braces nest inside
  `${...}`: `${U:-a{b}` and `${U:-{}` (the outer `${` never closed) succeed with the words `a{b}`
  and `{}`, `${U:-{}}` gives `{}`, `${U:-a{b` is `WRDE_SYNTAX`. An unbalanced quote in the
  default is not an error: `${U:-'}` and `${U:-"}` give `}`, and `${X:-'}`, `${X#'}` with `X` set
  give `X`.
* Bash forms (`${X:2}`, `${X/a/z}`, `${!X}`, `${X^}`, `${X,}`), `${}`, `${ X}`, `${X:}`, `${#}`
  and an unterminated `${` are `WRDE_SYNTAX`; `${#X:-}` gives `3`; `${X#}` and `${X:-}` give `X`.
* `${X:?msg}` and `${X?msg}` with `X` unset or null: the message `NAME: msg` (default
  `NAME: parameter null or not set`) goes to standard error (with or without `WRDE_SHOWERR`) and
  `wordexp` **returns 0 with no words**.
* Special parameters (POSIX leaves them unspecified for `wordexp`): **the positional parameters
  are the calling program's own arguments**: `$1` is `argv[1]` (`[<dir>]`), `$#` is `argc - 1`
  (1 in the record, which passes one argument), `$@` and `$*` are the arguments, `$0` is
  `argv[0]` (`[<argv0>]`); `${10}` is empty. `$$` is the caller's PID. `$?`, `$-`, `$!` are left
  as the literal text `$?`, `$-`, `$!`; `$_` is empty.
* Arithmetic `$((...))` and the old form `$[...]`: decimal, octal (`010` is 8), hex (`0x10`),
  `+ - * /` and parentheses, with 64-bit wrap-around (`9223372036854775807+1` gives the minimum,
  `4294967296*4294967296` gives 0) and truncating division (`-3/2` is -1, `7/2` 3). Variables
  must be written with `$` (`$(($N*2))` is 6, `$((N*2))` is `WRDE_SYNTAX`); `$((1/0))`, `~`, `!`,
  `**`, `+=`, `++`, a dangling operator and unknown names are `WRDE_SYNTAX`. Other operators are
  not evaluated and **the expression stops at the first unknown operator**: `1<2`, `1<<4`,
  `1==1`, `1?2:3`, `1&&0`, `1%0`, `1,2` all give `1`; `5&3`, `5|3`, `5^3` give `5`; `-3%2` gives
  -3; `08` gives 0 and `0x` gives 0. Command substitution inside arithmetic is evaluated
  (`$((1+$(echo 2)))` is 3).
* Tilde: `~`, `~/x`, `~root`, `~root/x`; an unknown user leaves the word unchanged; `x=~` and
  `x=~/a` are expanded (as in an assignment) but `a:~` is not, while `~:x` is; quoted or escaped
  `~` is literal; `~$X` is left as is. `HOME` empty gives `WRDE_NOSPACE` (1) with no words
  (`wordexp [~] 0 -> 1`, also `~/x`); `HOME` unset uses the passwd home.
* Field splitting (`IFS=` blocks): `IFS` unset or ` ` splits on blanks only; `:` splits `$C`
  into `x y "" z` (an empty field between two `:`), keeps leading and trailing blanks of `$S`
  (whitespace is not in `IFS`); `" :"` combines both, and `$D` (` a : b :: c `) gives `a b "" c`;
  `IFS=""` never splits; `IFS="\t\n"` leaves spaces; `:x` makes `x` a separator too, so `$C`
  starts with two empty fields. Splitting applies to expansion results only (`a:b c` is split on
  the blank only); command-substitution output is split with `IFS` too. A multibyte `IFS` (`é`)
  splits on each of its bytes (`$M` gives `x "" y "" z`).
* Pathname expansion: unquoted wildcards in the text and in expansion results are expanded,
  sorted; `.*` includes `.` and `..`; a pattern without a match stays as written; quoted or
  escaped wildcards are literal; `{w1,w2}` is `WRDE_BADCHAR` (no brace expansion); `w*/` gives the
  single word `w*` (no directory matches; the trailing slash is lost, as with `glob` on
  `tree/file1/`).
* Command substitution (`$(...)` and backquotes, nested in both forms) runs `/bin/sh`; trailing
  newlines are removed and inner ones split; an exit status other than 0 is not an error; the
  output is split and globbed unless quoted; `$()`, ``` `` ``` and `$( )` give nothing. Standard
  error of the command goes to `/dev/null` without `WRDE_SHOWERR` and to the caller's descriptor 2
  with it. A `)` inside quotes in `$(...)` is handled, but **`$(case a in a) echo c;; esac)` is
  `WRDE_SYNTAX`** (parentheses are counted).
* `WRDE_NOCMD` (`-- WRDE_NOCMD`): `$(...)`, backquotes, nested forms, forms inside double quotes,
  inside `$((...))`, in `${U:-$(...)}`, `${X:+$(...)}`, `${P#$(...)}` and after other expansions
  all give `WRDE_CMDSUB` (4) with nothing returned; single-quoted and escaped forms are literal.
  The check happens where the substitution would be evaluated: `${X:-$(echo x)}` with `X` set
  succeeds (the default is not expanded), and **a backquote inside `${...}` is not recognised at
  all**, with or without `NOCMD` (`${U:-\`echo x\`}` gives the words `` `echo `` and `` x` ``,
  and inside double quotes the literal text), while `$(...)` there is substituted. ``` `` ``` is
  `WRDE_CMDSUB` but `$()` succeeds. `$((1+2))`, `$X`, `~` and globbing still work. An unbalanced
  `$(` is `WRDE_SYNTAX`; `\$(echo x)` is `WRDE_BADCHAR`.
* `WRDE_BADCHAR` (2): each of `| & ; < > ( ) { }` and newline unquoted, alone or inside a word, is
  `WRDE_BADCHAR` (also with `NOCMD`); quoted, escaped, inside `$(...)` or inside `${...}` it is
  accepted (except `}` closing the `${`). The checks run left to right: `| $U` with `UNDEF` is
  `BADCHAR`, `$U |` is `BADVAL`; `$(echo x)|` is `BADCHAR` without `NOCMD` and `CMDSUB` with it,
  `x|$(echo x)` is `BADCHAR` either way.
* `WRDE_SYNTAX` (5): unbalanced `'`, `"`, `` ` ``, `$(`, `$((`, `${`, a trailing backslash (also
  inside `"`). Unbalanced quotes inside `${...}` are **not** errors (see parameter expansion).
* `WRDE_UNDEF`: an unset variable in `$U`, `${U}`, `"$U"`, `${#U}`, `${U%x}`, and also in
  `${U:-x}`, `${U-x}`, `${U:+x}` and `${U:=d}` is `WRDE_BADVAL` (3); inside `$((...))` it is
  `WRDE_SYNTAX`; inside a command substitution it is not checked; quoted or escaped `$U` is
  literal; set-but-empty `$Y` is fine; positional parameters are set (see above) so `$1` is fine.
* **`WRDE_NOSPACE` without memory exhaustion**: a double-quoted string that starts with an
  expansion of a set but empty variable (`"$Y"`, `"${Y}"`, `"$Y"a`, `"$Y "`, `"$Y$Y"`, `"$Y"''`,
  `"${Y%x}"`) returns 1 (`WRDE_NOSPACE`); `a"$Y"`, `"x$Y"`, `''$Y`, `"$U"` (unset), `"${Y:-}"`,
  `"$(true)"` work. This is the only way the record provokes `WRDE_NOSPACE`.
* **State after an error** (`wordexp-state` lines, owner's question): for `WRDE_BADCHAR`,
  `WRDE_SYNTAX`, `WRDE_BADVAL` and `WRDE_CMDSUB` on a fresh call, `we_wordc` and `we_wordv` are not
  written (777 and NULL stay), with or without `WRDE_DOOFFS`, and `we_offs` is unchanged. For
  `WRDE_NOSPACE`, the words expanded before the failure are returned (`a b "$Y"` gives
  `wordc=2 [a] [b]`; with `DOOFFS` the null slots are present). `wordfree` afterwards returns
  normally in every case.
* `WRDE_APPEND` after an error (`append-err#N`): `BADCHAR`, `BADVAL`, `CMDSUB` and `SYNTAX` leave
  the previous words untouched (the new words of the failing call are not added); `NOSPACE` adds
  the words expanded before the failure (`three` appears in `#6` and again after `#7`).
  `doofs-append`: `DOOFFS|APPEND` keeps the null slots; a later `APPEND` without `DOOFFS` still
  appends after them and keeps `offs=2`.
* `WRDE_REUSE` frees the previous result first: `REUSE|APPEND` gives only the new words
  (`reuse#3`); a failing `REUSE` call leaves `wordc=0 wordv=NULL` (`reuse#4`); with `DOOFFS` the
  slots are rebuilt. `wordfree` sets `we_wordv` to NULL and leaves `we_wordc`; calling it twice,
  or on a zeroed structure, is safe. An unknown flag bit (0x400) is ignored.

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference); POSIX does not force
any of them.

* `regerror` aborts the process for an error code outside 0-16 instead of returning a generic
  message (POSIX leaves unknown codes unspecified).
* `re_exec` crashes when no pattern is compiled (before any `re_comp`, or after a failed one).
* Subexpression selection does not follow POSIX's rule that each subexpression, left to right,
  takes the longest match consistent with the whole: `(a|ab)(c|bcd)(d*)` on `abcd` gives
  `(0,1)(1,4)(4,4)` (POSIX would give `(0,2)(2,3)(3,4)`), and the result changes with the order of
  the alternatives. Concatenated stars and the overall match do follow leftmost-longest.
* An inner group keeps its value from an earlier iteration of an enclosing repeated group when
  the last iteration does not take it (`((a)|b)+` on `ab`: inner (0,1)); this is not left to the
  implementation: `regexec`'s rules report a group nested in a repetition within the substring its
  parent reports, so a group the last iteration did not take is -1 (corrected 2026-10-03 by the
  analysis in `regex-submatch-analysis.md`, category I3; an earlier version of this document said
  POSIX left it to the implementation). The
  asymmetry between `(()|a)*` (inner (0,0)) and `(a|())*` (inner -1) on `aa` follows from the same
  choice.
* `(a*){2}` on `aa` reports an empty second iteration (2,2) while `(a*){2,3}` reports (0,2).
* BRE `a**` is `REG_BADRPT` while ERE `a**` is accepted; BRE `\{` after another repetition is
  `REG_BADRPT`. ERE `a)` and `)` are accepted as literals. `REG_ERPAREN` exists but is never
  returned (`REG_EPAREN` is).
* `GLOB_MAGCHAR` describes only the last pattern component.
* `glob` returns a file or dangling link named by a pattern with a trailing slash, without the
  slash (`tree/file1/`), and `wordexp` does the same through globbing (`w*/`).
* Unknown `glob` flag bits give -1 with `EINVAL`, outside the documented return values;
  `fnmatch`, `regcomp` and `wordexp` ignore unknown bits.
* `GLOB_ONLYDIR` returns a non-directory named without metacharacters (the man page allows it: the
  flag is a hint).
* `GLOB_NOMAGIC` treats a backslash as magic (`tree/no\match` is NOMATCH) while `GLOB_NOCHECK`
  returns patterns verbatim; `glob_pattern_p` and `GLOB_MAGCHAR` disagree on a lone backslash.
* An aborted `GLOB_APPEND` call discards the results of earlier calls.
* `GLOB_APPEND` without the `GLOB_DOOFFS` of the first call (misuse: the flags must agree)
  resets `gl_offs` to 0, counts the old null slots in `gl_pathc` and overwrites the last two
  earlier results (`append#5`: `tree/dangling` and `tree/nomatch` are gone). The record shows it
  only so that the line is explained; it is not a contract.
* The compat `glob@GLIBC_2.2.5` decides existence with `gl_stat` (follows links), the current
  version with `gl_lstat`; on the real file system the two agree on every case recorded.
* fnmatch: a trailing backslash never matches; `[[:alpha]` is a set, not an error; inside
  extended-pattern groups `\|` does not escape `|`; `?` does not match a truncated multibyte
  sequence but does match a lone invalid byte.
* `wordexp`: `${X:?msg}` prints the message and returns success with no words (a shell would
  fail); `"$Y"` with `Y` set and empty is `WRDE_NOSPACE`; `HOME=""` makes `~` `WRDE_NOSPACE`;
  `${X:=w}` changes the caller's environment; the positional parameters are the program's own
  `argv`; `$?`, `$-`, `$!` are left literal; arithmetic silently ignores everything after an
  unsupported operator (`1<<4` is 1) and supports only `+ - * /`; `$[...]` is accepted; a
  backquote inside `${...}` is not command substitution; `${X:-$(cmd)}` with `X` set escapes the
  `WRDE_NOCMD` check; `$()` passes `WRDE_NOCMD` but ``` `` ``` does not; `$(case ... esac)` with
  an unbalanced `)` is a syntax error; `WRDE_UNDEF` applies inside `${U:-x}` (where the shell's
  `set -u` would not complain); a multibyte `IFS` is used byte by byte; quotes in an unquoted
  `${U:-...}` default do not protect it from field splitting (`${U:-'a b'}` gives two words, a
  POSIX shell gives one); braces nest inside `${...}` so `${U:-a{b}` succeeds without closing
  the `${`, and unbalanced quotes inside `${...}` are accepted.

## What the record does not settle

* `GLOB_NOSPACE`, `REG_ESPACE` and a real `WRDE_NOSPACE` (memory exhaustion) are not provoked:
  the probe does not exhaust memory on the test machine. `REG_EEND` is never returned by the
  cases recorded.
* Directory order with `GLOB_NOSORT` on any file system other than the VM's ext4 (deferred by
  the owner), and any convention for comparing unordered results (deferred).
* `glob` with `GLOB_TILDE` for users whose home directory needs a lookup through a service other
  than `files`, and `~user` in `wordexp` for such users (phase 10 covers NSS).
* Which callbacks (`gl_stat`, `gl_lstat`, `gl_readdir`'s `d_type` use) the default `glob` calls in
  each case, beyond the `compat` lines: the `opened:` lines are informative only.
* The exact behaviour of `fnmatch` in multibyte locales for every class and every invalid
  sequence beyond the cases listed; `FNM_CASEFOLD` with `[[=...=]]` in `en_US.UTF-8`.
* Collation order beyond the 30 names in `sort/`, and `glob` sorting in other locales.
* `regexec` time and memory bounds (D-14.4's linear-time guarantee): the record holds only
  results; the timing note above is outside it.
* The GNU `re_*` interfaces beyond what §14.9 describes: `re_max_failures`, `translate` tables,
  `not_bol`/`not_eol`/`newline_anchor` fields, `RE_SYNTAX_*` bits individually, `REGS_FIXED`.
  `re_search`'s `-2` (internal error) return was not provoked.
* `wordexp` with a large positional-parameter list, `$@` with several arguments, and `"$@"`
  splitting (the record runs with one argument); `$!` and `$?` after the caller ran children.
* `wordexp`'s use of `/bin/sh` beyond the output and exit status (environment passed, signals,
  descriptors), and cancellation; `WRDE_SHOWERR` for errors other than command substitution.
* Thread safety and cancellation of all four interfaces, and `fnmatch`/`regexec` in a locale
  changed by another thread.
