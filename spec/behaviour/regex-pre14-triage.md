# Regex triage T28, T29, T30 (PRE14-REGEX-TRIAGE, 2026-10-23)

Black-box probes against glibc 2.39 (Ubuntu 2.39-0ubuntu8.9, x86_64, kanon), en_US.UTF-8.
Probe sources: `re_triage.c` and `re_icase2.c` in the task scratch (`~/work/pre14-regex-triage/` on kanon).
Test: `crates/core/tests/regex_pre14_triage.rs`.

**Clauses:** `s14_9_c_1_2`.

## T28: empty interval
* `a{}` gives "Invalid content of \{\}" (`REG_BADBR`) under POSIX_EGREP, EGREP, POSIX_EXTENDED,
  POSIX_MINIMAL_EXTENDED, GNU_AWK, POSIX_AWK, and with `RE_INVALID_INTERVAL_ORD` or `RE_NO_EMPTY_RANGES`.
  BRE `a\{\}` gives the same under POSIX_BASIC, GREP and ED|INVALID_INTERVAL_ORD.
* With `RE_INVALID_INTERVAL_ORD`, the other malformed intervals are literal: `a{x}`, `a{`, `a{ }`, `a{1,*}`.
  Without it, `a{x}`, `a{ }` and `a{1,*}` are `REG_BADBR` and `a{` is `REG_EBRACE`.
* `a{,}` and `a{,2}` compile under every syntax. `a{2,1}` is always `REG_BADBR`.

## T29: REG_ICASE folding
* Single characters and bracket lists: two characters match when their `towupper` is the same.
  Groups: {µ U+00B5, Μ, μ}, {I, i, ı U+0131}, {S, s, ſ U+017F}, {Σ, σ, ς}, {Ǆ, ǅ, ǆ}, {Θ, θ, ϑ}, {K, k}, {Å, å}.
  U+0130 İ, U+212A KELVIN SIGN, U+212B ANGSTROM SIGN and U+1E9E ẞ match only themselves. ß matches only itself.
  `[^c]` is the complement of these groups. Backreferences fold the same way (`(µ)\1` matches `μΜ`).
* `[[:upper:]]` and `[[:lower:]]` under `REG_ICASE` match all 19 cased characters probed (ß and the Kelvin sign included).
* Ranges in en_US.UTF-8 follow collation (`[a-z]` matches ǅ, ẞ, the Angstrom sign; `[j-l]` matches the Kelvin sign
  but `[k-k]` does not). Degenerate ranges such as `[i-i]` and `[µ-µ]` fold like the single character.
  Liella's range handling was not changed by this story.

## T30: deep nesting (bounded sizes only: 200 to 10,000 levels)
* Unbalanced `(`×n, `(`×n `a`, `\(`×n (BRE), `(a`×n `b`, `(a|`×n `b`, `(a*`×n `b`: `REG_EPAREN` at every size probed.
* Balanced `(`×n `a` `)`×n: compiles (rc 0) up to 1,000. `[`×n: `REG_EBRACK`.
* Liella keeps its bound of 256 levels. Past the bound it reports `REG_EPAREN` for an unbalanced pattern
  and `REG_EBRACK` for an open bracket. A balanced pattern past the bound is `REG_ESPACE`; this is the kept
  difference (the gain: bounded parser depth).
