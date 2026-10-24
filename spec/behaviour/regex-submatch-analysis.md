# Regex submatch selection: what the spec says, and a test of the owner's hypothesis

**Status:** analysis only, for the owner's ruling. No implementation decision is made or recommended
here. Clean-room: written from glibc-functional-spec.md, the black-box record
(`patterns.glibc.txt.gz`, its probe `crates/core/tests/oracle/patterns_diff.c`, `patterns.md`), the
POSIX.1-2017 text (XBD chapter 9, `regcomp`/`regexec`), the man pages `regex(7)`/`regex(3)` read
on the VM, and a few extra black-box probes on the VM (glibc 2.39-0ubuntu8.9). No regex engine
source was read.

**Script:** `spec/tools/regex_submatch_hypothesis.py` (Python 3, standard library only).

**Clauses:** `s14_9_c_1_2`, `s14_9_g_0`.

## 1. What glibc-functional-spec.md says

§14.9 is the only clause about matching. The relevant lines, verbatim:

> * `regexec(&re, text, nmatch, pmatch, eflags)` finds the leftmost-longest match and fills
>   subexpression offsets (`regmatch_t`). [...]

> **Guarantees.** POSIX leftmost-longest semantics, and multibyte (locale) awareness.

> ⚑ HIST. Back-references make matching NP-hard. Exponential-time inputs are possible, and
> `re_max_failures` is a global limit on backtracking.

§0 and the cross-cutting conventions (§0.6, C0.1/C0.2) say nothing about regular expressions. The
only other mention is the H1 row of the historical-accident table (embedded NUL truncates
`regexec` input). Nothing else in the spec mentions subexpressions, submatches or `pmatch`.

Summary: the spec states the **overall** match rule explicitly (leftmost, then longest). For
subexpressions it says only that `regexec` "fills subexpression offsets". The Guarantee names
"POSIX leftmost-longest semantics" without saying whether that includes XBD 9.1's subexpression
rule. This document therefore treats the spec as **explicit on the overall match and ambiguous on
subexpression selection**.

(For reference, the behaviour record `patterns.md` already notes that glibc's subexpression
selection does not follow POSIX's subpattern rule, and that an inner group keeps a value from an
earlier iteration. It says that POSIX leaves the second point to the implementation. Section 2
questions that claim.)

## 2. What POSIX says (POSIX.1-2017)

* **XBD 9.1, definition of "matched".** The search finds the match that begins earliest. If several
  matches start there, it takes the longest. The same definition then adds a rule for
  subexpressions: "each subpattern, from left to right, shall match the longest possible string",
  consistent with the whole match being the longest of the leftmost ones. For this purpose a null
  string counts as longer than no match at all. This text says *shall*: it is normative, and it has
  no exceptions. Under it, `(a|ab)(c|bcd)(d*)` on `abcd` must give (0,2)(2,3)(3,4).
* **XBD 9.3.6 (BRE).** A subexpression repeated by `*` or by an interval must not match a null
  expression unless that is the only match for the repetition or is needed to reach the exact or
  minimum count.
* **XBD 9.4.6 (ERE).** The same restriction, but its wording covers only an ERE that matches a
  single character repeated by `*`, `?` or an interval. For ERE groups, only the 9.1 subpattern
  rule applies.
* **XBD 9.4.7.** Alternation matches a string that either side matches. It sets no priority
  between the alternatives.
* **`regexec` DESCRIPTION, the pmatch rules:**
  1. A top-level group that participated several times reports its **last** match.
  2. A top-level group that did not participate reports -1.
  3. A group *i* nested directly in group *j* is reported by rules 1 and 2 again, applied
     **within the substring reported for j** rather than the whole string.
  4. If j is -1, then i is -1.
  5. A zero-length match is reported at the offset just after it.

  Read literally, rule 3 makes `((a)|b)+` on `ab` report the inner group as -1, because the inner
  group did not participate within (1,2), the last match of the outer group. glibc gives (0,1).
* **`regexec` RATIONALE** discusses `nmatch`, `REG_NEWLINE`, `REG_ICASE`, offsets and `regoff_t`.
  It does **not** discuss how subexpressions are chosen, or how nested groups behave in repeated
  groups. `patterns.md` says POSIX leaves the carried-over-group behaviour to the implementation.
  I could not find text that says so. On the literal reading of rule 3 the behaviour is
  constrained. The owner should rule on which reading applies.
* **`regex(7)` (man-pages 6.7, from Henry Spencer's package)** paraphrases the same rule:
  subexpressions match the longest possible substrings, subject to the whole match being as long as
  possible, and earlier subexpressions take priority over later ones. Its BUGS section calls
  back-references vaguely defined. `regex(3)` adds nothing about selection: unused entries are -1.

## 3. The hypothesis as implemented

The owner's wording: *the overall match is leftmost-longest; submatches are chosen by alternation
priority among the paths that reach that same end.* The script implements it from that definition:

* A **path** is one derivation of the pattern over `subject[s:e]`.
* **Overall:** s is the smallest offset that has a path. e is the largest end of a path from s.
* **Priority order** is the depth-first order in which:
  * alternatives are tried left to right;
  * a repetition `X{m,n}` (also `*` = {0,}, `+` = {1,}, `?` = {0,1}) tries one more iteration
    before stopping (greedy-first);
  * concatenation is ordered left to right, so the earliest point where two paths differ decides.

  The chosen path is the first path in this order that runs from s to e.
* **Reported offsets:** pmatch[0] = (s,e). pmatch[i] is the last write of group i on the chosen path,
  or (-1,-1). A back-reference matches the text its group holds at that point of the path, folding
  case under `REG_ICASE`. It fails if the group is unset.

The hypothesis leaves two things open. Both were run as named variants:

| Variant | Choice |
|---|---|
| empty=**stop** (default) | An iteration may match the empty string. No iteration follows an empty one. |
| empty=**never** | An iteration beyond the minimum count may not match the empty string. |
| empty=**first** | Beyond the minimum, an empty iteration is allowed only as the first iteration (so a null string beats zero iterations), and nothing follows it. |
| empty=**lead** | As `first`, but non-empty iterations may follow the leading empty one. |
| carry=**keep** (default) | Captures are writes along the path, so an inner group not taken in the last iteration keeps its earlier value. |
| carry=**reset** | Each new iteration resets the groups inside the repeated expression to -1. This is the literal rule-3 reading. |

In every variant, iterations needed to reach the minimum count may be empty.

**Diagnostic only, not part of the hypothesis:** a `swap` switch tries the second branch of an
alternation before an *empty* first branch. It is used only to classify residuals.

**Syntax covered.** The script covers what the probe's generator emits:
* ERE: `a b ( ) | * + ?`, `{0,1}`, `{2}`, `\1`;
* BRE: `a b \( \) * \1`;
* ERE with `REG_ICASE`: `a B ( ) | * \1`;
* the extra constructs in the hand cases: `.`, bracket expressions in the C locale, `{m,n}`, BRE
  `\{ \} \+ \? \|`, the anchor rules, `REG_NEWLINE`, `REG_NOTBOL`/`REG_NOTEOL`, and the GNU
  operators `\b \B \< \> \w \W \s \S \` \'`.

Every corpus and hand pattern that glibc compiled was parsed.

## 4. Method

The script reads `spec/behaviour/patterns.glibc.txt.gz`:
* **Corpora:** every `rc` line with regcomp code 0, crossed with its `subjects` line.
* **Hand cases:** every `re` line of the `-- matching` block in the C locale. 62 of these are
  glibc regcomp errors and were excluded.

For each case and variant it compares:
* `pmatch[0]` agreement: match versus no match, and the (so,eo) of the match;
* full agreement: all `min(re_nsub+1, 10)` entries.

Five hand cases (the pathological patterns on 30-100 characters) exceed the script's step budget.
They are reported as not evaluated. Four of them are no-match cases in glibc.

Each residual of the default variant was classified by the first diagnostic change that makes it
agree, in this order:
1. empty=first;
2. empty=lead;
3. swap;
4. swap together with first or lead.

If none of these works, the case is classed by whether glibc's vector is produced by any path under
any variant. The script also measures the **POSIX subpattern reading**, for comparison only. Among
the paths from s to e, it takes group by group in opening-parenthesis order:
* a participating group beats a non-participating one;
* then the earliest start wins;
* then the longest length wins.

Extra black-box probes on the VM (`cc` plus a 30-line `regexec` driver in `/tmp`, since removed)
checked the empty-alternative and back-reference findings in §6. They are quoted as "probe".

## 5. Numbers

12,257 corpus patterns compile, giving 206,863 corpus cases:

| Corpus | Patterns | Subjects | Cases |
|---|---|---|---|
| ERE-1 | 9,809 | 15 | 147,135 |
| ERE-2 | 1,010 | 15 | 15,150 |
| BRE | 753 | 31 | 23,343 |
| ERE-ICASE | 685 | 31 | 21,235 |

There are also 255 evaluated hand cases.

Agreement (pmatch[0] / full vector):

| Variant | ERE-1 | ERE-2 | BRE | ERE-ICASE | **All corpora** | Hand (255; 5 not evaluated) |
|---|---|---|---|---|---|---|
| stop/keep (default) | 100.00 / 99.50 | 99.90 / 99.31 | 100.00 / 99.87 | 100.00 / 99.27 | **99.99 / 99.50** (205,837) | 98.04 / 93.33 |
| stop/reset | 100.00 / 99.46 | 99.90 / 99.31 | 100.00 / 99.87 | 100.00 / 99.27 | 99.99 / 99.48 | 96.86 / 88.63 |
| never/keep | 100.00 / 97.88 | 99.80 / 98.32 | 99.07 / 96.17 | 99.85 / 98.05 | 99.87 / 97.74 | 98.04 / 94.51 |
| never/reset | 100.00 / 97.88 | 99.80 / 98.32 | 99.07 / 96.17 | 99.85 / 98.05 | 99.87 / 97.74 | 96.86 / 89.80 |
| first/keep | 100.00 / 99.58 | 99.90 / 99.31 | 100.00 / 100.00 | 100.00 / 99.27 | 99.99 / 99.57 (205,979) | 98.04 / 96.86 |
| first/reset | 100.00 / 99.58 | 99.90 / 99.31 | 100.00 / 100.00 | 100.00 / 99.27 | 99.99 / 99.57 | 96.86 / 92.16 |
| lead/keep | 100.00 / 99.58 | 99.90 / 99.31 | 100.00 / 100.00 | 100.00 / 99.27 | 99.99 / 99.57 | 98.04 / **97.65** |
| lead/reset | 100.00 / 99.58 | 99.90 / 99.31 | 100.00 / 100.00 | 100.00 / 99.27 | 99.99 / 99.57 | 96.86 / 92.16 |

The hand-case percentages count the 5 cases that were not evaluated as disagreements. Among the 250
cases that were evaluated, the default variant disagrees on 12, and lead/keep on 1.

**keep versus reset.** Wherever the two give different vectors, glibc matches **keep** every time:
68 of 68 cases under empty=stop, 12/12 under never, 12/12 under first and 14/14 under lead.
Examples:
* `((a)|b)*` on `ab`: glibc (0,2)(1,2)(0,1); reset gives (1,2)(-1,-1).
* `(a(b)?)+` on `aba`: glibc (0,3)(2,3)(1,2).

**POSIX subpattern reading (comparison only).** It agrees on the full vector in 99.83% of corpus
cases (206,507) and in 242 of the 250 evaluated hand cases. This is higher than the default
hypothesis on the corpora, because the corpora rarely separate the two rules:
* The corpora contain no pattern like `(a|ab)(c|bcd)`; their tokens are only `a` and `b`, and
  patterns have at most 5 tokens.
* The 341 corpus cases where the hypothesis is right and the POSIX reading is wrong are all of the
  `||()` kind (category I2 below).
* The hand cases that do separate them all go the hypothesis's way: `(a|ab)(c|bcd)(d*)`,
  `(a|ab)(bcd|c)(d*)`, `(a|ab)(b*)`, `(a*){2}`, `(a?){3}` and `\(a*\)\{2\}`.

**Residuals of the default variant:**
* Corpora: 1,026 cases, which are A1 869 + E1 142 + O1 15.
* Hand cases: 12, which are E1 9 + E2 2 + M1 1.

With empty=first and the swap diagnostic together, only O1 (15 cases) is left in the corpora.

## 6. Residual classification and three-way triage

How each outcome is assigned:
* The spec is taken as ambiguous on subexpressions (§1).
* Where the spec is ambiguous and (b) and (c) agree, the clause cannot be right as it stands, so
  the outcome is "spec wrong: correct the clause". Here the correction means stating the rule
  explicitly.
* Where the spec is ambiguous and (b) and (c) differ, the outcome is "owner ruling needed".

### Residual categories (hypothesis ≠ glibc)

| Cat. | Cases | Minimal examples (glibc vs hypothesis) | (a) spec | (b) glibc probe | (c) POSIX text | Outcome |
|---|---|---|---|---|---|---|
| **A1** empty first alternative. glibc prefers the 2nd branch over an empty 1st branch when the 2nd branch sets a group. | 869 corpus (58 patterns: ERE-1 624, ERE-2 90, ICASE 155); 0 hand | `\|()` on `""`: (0,0)(0,0) vs (0,0)(-1,-1). `\|(a*)` on `""`: same. `(\|())` on `""`: (0,0)(0,0)(0,0) vs (0,0)(0,0)(-1,-1). | Ambiguous (silent on subexpressions) | The 2nd branch wins over an empty 1st branch (probe: `x(\|())` on `x` gives (1,1)(1,1)). Only the 2nd branch moves: `\|(a)\|()`, `\|\|()` and `(\|(b)\|())` keep the empty branch. | **Forced, agrees with (b).** Under 9.1 the group must take the null string, since a null string beats no match. | **Spec wrong: correct the clause.** The hypothesis's plain alternation priority conflicts with both (b) and (c). |
| **E1** trailing empty iteration. The hypothesis adds a final empty iteration after a non-empty one; glibc does not. Variant `first` fixes it. | 142 corpus (18 patterns: ERE-1 112, BRE 30); 9 hand | `(a*)*` on `a`: (0,1)(0,1) vs (0,1)(1,1). `(a\|)+` on `a`: same. `\(a*\)*` on `a`: same. Hand: `(a*)*\1` on `aa`: (0,2)(0,1) vs (0,2)(2,2). | Ambiguous | No empty iteration after a non-empty one (`(a*)*` on `aa` gives (0,2)(0,2)) | **Forced, agrees with (b).** BRE: 9.3.6 forbids the null iteration. ERE: 9.4.6 does not cover groups, but 9.1 (longest subpattern) gives (0,1). | **Spec wrong: correct the clause.** The hypothesis's default reading (`stop`) conflicts with (b) and (c); `first`/`lead` agree. |
| **E2** leading empty iteration followed by non-empty iterations. Only variant `lead` fixes it. | 0 corpus; 2 hand | `(()\|a)+` on `a`: (0,1)(0,1)(0,0) vs (0,1)(1,1)(1,1). `(()\|a)*` on `aa`: (0,2)(1,2)(0,0) vs (0,2)(2,2)(2,2). | Ambiguous | The inner `()` keeps (0,0) from a leading empty iteration (probe: `(()\|a){1,3}` on `aa` gives (0,2)(1,2)(0,0); `(a\|())+` on `a` gives -1 for the inner group) | **Contradicts (b).** 9.3.6 and 9.1 do not allow the unneeded empty iteration. Rule 3 reports the inner group within (1,2), which gives -1. | **Owner ruling needed.** |
| **M1** interval with an empty-matching body. glibc's path exists but is not first under any variant. | 0 corpus; 1 hand | `(a*){2,3}` on `aa`: (0,2)(0,2) vs (0,2)(2,2) | Ambiguous | Not uniform. Probe: `{2,3}`, `{2,}` and `{3,4}` give (0,2); `{2}` and `{1,3}` give (2,2); `(a?){3}` on `aa` gives (2,2). | **Forced to (0,2) by 9.1.** This residual case agrees with (b); other members of the family do not (I4). | **Spec wrong: correct the clause** for this case. glibc is not uniform across the family; see I4. |
| **O1** overall match differs. glibc reports **no match** for a back-reference to a group whose interval repetition ends on an empty iteration. | 15 corpus (1 pattern, ERE-2); 0 hand | `(){2}\1` on `""`: no match vs (0,0)(0,0) | **Explicit:** "finds the leftmost-longest match", and here a match exists | Probe: no match for `(){2}\1`, `(){0,2}\1`, `(){2,}\1`, `(()){2}\2`, `(a*){2}\1` on `aa`, `(a\|){2}\1` on `aa` and `(b*){2}\1` on `bbb`. Match for `(){1}\1`, `()*\1`, `(){1,}\1`, `(a){2}\1` on `aaa` and `(a)*\1` on `aa`. | **Forced:** a match exists under 9.1, 9.3.6/9.4.6 (null allowed to reach the count) and the back-reference rule, so `regexec` must succeed | **glibc odd one out: Liella follows the spec, differences entry.** |

### Related contrasts where the hypothesis agrees with glibc but POSIX differs (not residuals)

These carry the same decision and are listed so the ruling covers them.

| Cat. | Size | Example | (a) spec | (b) glibc | (c) POSIX | Outcome |
|---|---|---|---|---|---|---|
| **I1** alternation priority versus longest subpattern: the core of the hypothesis | 3 hand cases (`(a\|ab)(c\|bcd)(d*)`, `(a\|ab)(bcd\|c)(d*)`, `(a\|ab)(b*)`); the corpora cannot separate the two readings | `(a\|ab)(c\|bcd)(d*)` on `abcd`: glibc (0,1)(1,4)(4,4) | Ambiguous: "POSIX leftmost-longest semantics" | Alternation order decides; the swapped pattern gives (0,2)(2,3)(3,4) | **Contradicts (b):** 9.1 requires (0,2)(2,3)(3,4) | **Owner ruling needed** |
| **I2** an empty branch that is not followed directly by a participating branch | 341 corpus cases | `\|\|()` on `""`: glibc (0,0)(-1,-1) | Ambiguous | The group does not participate | **Contradicts (b):** under 9.1 a null string beats no match, giving (0,0)(0,0) | **Owner ruling needed** |
| **I3** inner group carried over from an earlier iteration (keep versus reset) | 68 cases differ under stop, 12 under first; glibc is keep in all | `((a)\|b)+` on `ab`: glibc inner (0,1) | Ambiguous | keep | Read literally, rule 3 gives -1 and **contradicts (b)**. `patterns.md` says POSIX leaves this open; I did not find text that says so. | **Owner ruling needed.** The ruling also covers the reading of rule 3. |
| **I4** interval with an empty last iteration | 3 hand cases | `(a*){2}` on `aa`: glibc (2,2). `(a?){3}` on `aa`: glibc (2,2). `\(a*\)\{2\}` on `aaaa`: glibc (4,4). | Ambiguous | An empty last iteration is reported | **Contradicts (b):** 9.1 prefers the longer last iteration, e.g. (0,2) for `(a*){2}` | **Owner ruling needed** |

### Options the owner could rule between (no recommendation)

* **A1, E1, M1** ((b) and (c) agree): either state the agreed behaviour in §14.9, or keep the
  hypothesis's plain reading and record the difference.
* **E2, I1, I2, I3, I4** ((b) and (c) differ): follow POSIX 9.1 and regexec rule 3, or follow
  glibc. Per category, the glibc behaviour can be expressed as:
  * alternation priority (I1);
  * a second branch beating an empty first branch (A1 and I2 together);
  * no trailing empty iteration (E1), but a leading one allowed (E2);
  * keep (I3);
  * a non-uniform interval rule (M1 and I4).
* **O1** (spec and POSIX agree, glibc does not): a differences entry.

## 7. How to rerun

```
python -B spec/tools/regex_submatch_hypothesis.py [--record spec/behaviour/patterns.glibc.txt.gz] \
    [--examples N] [--json OUT.json]
```

The run is deterministic: its output depends only on the record. It takes about one minute and
prints the following, in order:
* the corpus sizes;
* the agreement table for each variant;
* the POSIX-reading comparison;
* the keep-versus-reset contrast;
* the classified residuals of the default variant, with the smallest example patterns.

`--json` writes the same numbers and examples as machine-readable data.
