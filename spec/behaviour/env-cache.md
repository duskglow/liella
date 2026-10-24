# env-cache

**Spec:** §1.3 (`getenv`, `secure_getenv`, `setenv`, `unsetenv`, `putenv`, `clearenv`, `environ`),
§14.10 (`wordexp`: `IFS`), §7.7 (`gettext`: `LANGUAGE`), §23.5 and §23.6 (`localtime`, `tzset`: `TZ`).
**Clauses:** `s1_3_c_0`, `s1_3_c_1`, `s1_3_c_5`, `s1_3_f_3`, `s1_3_t_0`, `s7_7_c_4`, `s14_10_c_3`, `s23_6_c_0`, `s23_6_g_0`.

**Question (owner, 2026-10-09):** Liella may cache environment lookups (`IFS` in `wordexp`,
`LANGUAGE` in `gettext`, `TZ` in `localtime`/`tzset`) behind a process-wide generation counter
bumped by `setenv`/`unsetenv`/`putenv`/`clearenv`. What does the reference library do? This is a
fact record, not a decision.

**Probe:** `env-cache.probe.c` (one program, three modes). **Record:** `env-cache.glibc.txt`
(`matrix`, `gt`, then the cost table). Built with `cc -std=gnu17 -O1`, run on Zen 1 (glibc 2.39,
Ubuntu 24.04) with `LANG=en_US.utf8` and `LOCDIR` pointing at two throw-away catalogs
(`de`/`fr`, domain `envp`, `hello` -> `hallo`/`bonjour`, `bye` -> `tschuess`/`au revoir`, made with
`msgfmt`). Every trial runs in a forked child, so no process state leaks between trials. Public
interfaces only; aggregate `perf stat -e instructions:u` for the cost table.

## Method

Readers: `getenv("IFS")`; `wordexp("$X")` with `X="a:b,c d"` (IFS `:` -> `[a][b,c d]`, IFS `,` ->
`[a:b][c d]`, IFS unset -> `[a:b,c][d]`); `gettext("hello")` (LANGUAGE `de`/`fr`); `localtime`
and `localtime_r` of time 0 (TZ `EST5`/`PST8`, unset -> UTC). For each reader and each change kind
the child sets the old value A (or leaves the variable unset), calls the reader once (primes any
cache), applies the change, calls the reader again (`after`), then calls `tzset()` and reads
again (`after_tzset`, TZ readers only). "Expected" is a fresh process with the new state built
through `setenv`. SEEN = `after` equals expected.

Change kinds: 1a `setenv` overwrite, 1b `unsetenv`, 1c `putenv` over, 1d `clearenv`, 1e `setenv` of
a variable that was unset, 1f `putenv` of one that was unset; 2 in-place mutation (no call) of a
buffer earlier given to `putenv`; 3 `environ[i]` overwritten with a new pointer; 4a `environ`
assigned a new array, 4b a new array lacking the variable; 5a an unrelated slot (`ZZ=filler`)
overwritten with `"IFS=,"` (variable was NOT set), 5b a `putenv`'d buffer `QQQ=,` renamed in place
to `IFS=,`; 6a/6b duplicate entries `[V=A,V=B]` / `[V=B,V=A]` built directly in `environ`.

## A and C: visibility (recorded in `env-cache.glibc.txt`, section matrix)

| Change | getenv | wordexp (IFS) | gettext (LANGUAGE), msgid already translated once | localtime (TZ) | localtime_r (TZ), with or without a prior tzset |
|---|---|---|---|---|---|
| 1a-1d setenv/unsetenv/putenv/clearenv | seen | seen | NOT seen | seen | not seen |
| 1e/1f variable was unset | seen | seen | seen | seen | not seen |
| 2 putenv'd buffer mutated in place | seen | seen | NOT seen | seen | not seen |
| 3 `environ[i]` = new pointer | seen | seen | NOT seen | seen | not seen |
| 4a/4b `environ` = new array | seen | seen | NOT seen | seen | not seen |
| 5a slot overwritten, var was unset | seen | seen | seen | seen | not seen |
| 5b putenv'd buffer renamed in place | seen | seen | seen | seen | not seen |
| 6 duplicate names | first entry wins | first wins | first wins | first wins | first wins (after tzset) |

Findings, one line each:

* **getenv, wordexp, localtime read the live `environ` array at every call.** All six change
  kinds, including ones that make no library call (2, 3, 4, 5), are seen by the very next call, and
  a variable that was unset and becomes set through a raw slot write (5) is seen. Nothing is
  remembered between calls. Duplicates: the first entry in array order wins in every reader.
* **localtime** re-reads TZ on every call (no `tzset` needed): `after` already shows the new zone
  for every kind, and `after_tzset` equals `after`. A prior `tzset()` makes no difference
  (`localtime+tzset-primed` rows identical).
* **localtime_r reads TZ only on its first call (initialisation) and on `tzset`**: `before` shows
  the TZ in force at the first call (EST), but after the change `after` is still the old zone for
  every kind, and only an explicit `tzset()` brings the new value (`after_tzset` column). So for
  `localtime_r` a later environment change matters only through `tzset`.
* **gettext is sticky for messages it has already translated.** With `de` found for `hello`,
  changing LANGUAGE to `fr` (any of 1a-4b) still returns `hallo`. A message that was a *miss*
  earlier (LANGUAGE unset) does see a new LANGUAGE (1e, 1f, 5a, 5b), so misses are not remembered.
  The stickiness is per message, not per catalog selection (next section).

### gettext stickiness (section `gt` of the record)

Child: `LANGUAGE=de`, `gettext("hello")` -> `hallo`; then `LANGUAGE=fr`; then:

| Operation after the change | Result |
|---|---|
| `gettext("hello")` again | `hallo` (old) |
| `gettext("bye")` (never looked up before) | `au revoir` (new LANGUAGE honoured) |
| `dgettext("envp","hello")` | `hallo` (old) |
| `bindtextdomain("envp", <same string>)` then `gettext("hello")` | `hallo` (old) |
| `textdomain("envp")` then `gettext("hello")` | `bonjour` (new) |
| `bindtextdomain("envp", <different string for the same dir>)` then `gettext("hello")` | `bonjour` (new) |
| `setlocale(LC_ALL,"")` (LANG set) then `gettext("hello")` | `bonjour` (new) |
| `setlocale(LC_MESSAGES,"C")` then `setlocale(LC_MESSAGES,"")` | `bonjour` (new) |

(Only a `setenv` change was tried for the not-yet-remembered message `bye`; the matrix columns
for 1e, 1f, 5a, 5b are also misses, which were seen.) Finding: a translation already obtained is remembered per message and is dropped by `textdomain`,
`bindtextdomain` with a changed directory string, and `setlocale`, but not by environment changes
or by a `bindtextdomain` with the same string. The LANGUAGE value is consulted again for
messages not yet remembered. (Observed behaviour only; the mechanism is not inferred further.)

## B: cost (instructions:u per call; method: (11000 calls - 1000 calls)/10000, so process start
and the priming call cancel; the harness `snprintf` of the result is included, about 450-480
instructions per call, the same for every row of a reader)

Columns are the position of the variable in the array of N entries (`X` is always entry 0):
first (entry 1), last, absent. Raw numbers: record section "cost".

| Reader | N=10 first / last / absent | N=100 first / last / absent | N=1000 first / last / absent | Per-entry slope |
|---|---|---|---|---|
| getenv("IFS") | 507 / 555 / 525 | 507 / 1094 / 1065 | 507 / 6504 / 6465 | 6.0 instr per entry scanned |
| wordexp("$X") | 3456 / 3504 / 3586 | 3456 / 4053 / 4126 | 3456 / 9453 / 9526 | 6.0 (IFS scan), flat when IFS is first |
| gettext("hello"), found (LANGUAGE=de) | 856 / 856 / - | 855 / 856 / - | 856 / 855 / - | flat: no scan on a remembered hit |
| gettext("hello"), miss (LANGUAGE absent) | - / - / 2409 | - / - / 2969 | - / - / 8372 | 6.0 per entry (scans every call) |
| localtime | 1877 / 1934 / 2152 | 1877 / 2465 / 2692 | 1899 / 7896 / 8092 | 6.0 for TZ, plus some more when absent |
| localtime_r | 1741 / 1741 / 1748 | 1741 / 1741 / 1748 | 1741 / 1741 / 1748 | flat: never reads TZ |

Finding: getenv, wordexp (IFS), localtime (TZ) and the gettext miss path scan the environment
array on every call (linear in N and in the variable's position, about 6 instructions per entry,
which is the cost of a plain `getenv`). A remembered gettext hit and `localtime_r` do not scan
at all. Nothing in the first three is a "flat cost = cached" case; they are as cheap as getenv
itself because they just call it (a 10-entry environment costs well under 100 instructions of
scanning).

## IMPLICATIONS FOR A GENERATION CACHE

A cache validated only by a counter bumped in `setenv`/`unsetenv`/`putenv`/`clearenv` would differ
from the reference library as follows:

| Case | Reference library | Counter-validated cache |
|---|---|---|
| wordexp IFS / localtime TZ after 1a-1f | new value seen | same |
| wordexp IFS / localtime TZ after 2 (in-place mutation of a putenv'd string) | seen | STALE (no call, no bump) |
| after 3 (`environ[i]` overwritten) | seen | STALE |
| after 4 (`environ` replaced) | seen | STALE |
| after 5 (unset -> set through a slot write; a cached "not set" answer) | seen | STALE, the hard case |
| duplicates | first entry wins | same if the cache is filled with the same lookup |
| localtime_r | never reads TZ except via `tzset` | no difference (tzset is the only trigger either way) |
| gettext, found message, after 1a-1d | NOT seen (sticky) | seen if the counter invalidates; matches glibc only if the counter does NOT invalidate remembered hits |
| gettext, found message, after 2-4 | NOT seen | matches glibc (stale) |
| gettext, message not remembered, any change (1-5) | seen | seen only if misses are not cached or the counter invalidates them |

Options (facts, not a recommendation to adopt any):

1. No cache: scan per call. This is exactly the reference behaviour and cost class for
   `wordexp`/`localtime`/the gettext miss path (about 6 instructions per environment entry). Exact
   for every change kind including raw writes.
2. Counter-validated cache: saves the scan, but is stale for kinds 2, 3, 4, 5 on `wordexp` and
   `localtime`, which the reference sees. Adding a check of the `environ` pointer catches 4 only;
   3, 2 and 5 are invisible without re-reading the array (which is the scan). The deviation would
   need a row in `docs/differences-from-glibc.md` and is observable by a program that edits
   `environ` directly.
3. gettext only: reproducing the reference's per-message memory of hits (invalidated by
   `textdomain`, `bindtextdomain` with a changed string, `setlocale`) gives glibc's observable
   behaviour for free; invalidating on the env counter instead would be fresher than glibc.

Recommendation (one line): for IFS and TZ keep the live scan (glibc does too, the saving is a few
dozen instructions at typical environment sizes), and use remembered hits for gettext as above.

## D: direct stores into `environ`, then a message not yet translated (GETTEXT-ENV-SCAN, 2026-10-21)

**Probe:** `env-cache-direct.probe.c`; **record:** `env-cache-direct.glibc.txt` (glibc 2.39
Ubuntu 2.39-0ubuntu8.9, `cc -std=gnu17 -O1`, `LANG=en_US.utf8`, the same two catalogs). Each trial:
`ZZ=filler` set first, then LANGUAGE unset or `de` (setenv or putenv), one `gettext("hello")`, a
change made without a library call, then `gettext("hello")` (already translated) or `gettext("bye")`
(never looked up).

| Change (no library call) | `bye` afterwards (glibc) |
|---|---|
| a: LANGUAGE unset; ZZ's slot = `"LANGUAGE=fr"` | seen (`au revoir`; `hello` too, it was a miss) |
| b: LANGUAGE's own slot = `"LANGUAGE=fr"` | seen |
| c: ZZ's slot, before LANGUAGE's, = `"LANGUAGE=fr"` | seen (the earlier entry wins) |
| d: LANGUAGE unset; a putenv'd `"QQQQQQQQ=fr"` renamed in place | seen |
| e: a putenv'd `"LANGUAGE=de"` edited in place to `fr` | seen |
| f: LANGUAGE's slot = NULL (the array ends there) | seen (`bye`: untranslated) |

`hello`, already translated, keeps `hallo` in b, c, e and f (section `gt`). Finding: glibc reads the
live array for every message not yet translated. Liella (ruling #504) remembers where LANGUAGE
was found and sees b, e and f at once, and a, c and d at the next `setenv`/`putenv`/`unsetenv`/
`clearenv` or new `environ` (docs/differences-from-glibc.md).
