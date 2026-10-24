# Message catalogs: the gettext family, catopen, catgets, catclose

**Spec:** §7.7 (`gettext`, `dgettext`, `dcgettext`, `ngettext`, `dngettext`, `dcngettext`,
`textdomain`, `bindtextdomain`, `bind_textdomain_codeset`, `__dcgettext`, `__dgettext`,
`_nl_default_dirname`, `_nl_domain_bindings`, `_nl_msg_cat_cntr`, `_libc_intl_domainname`) and
§7.8 (`catopen`, `catgets`, `catclose`), with the decision rows D-7.5 (msgid-keyed lookup, `.mo`
parsing after the GNU gettext file format, `_nl_msg_cat_cntr` bumped on catalog changes) and
D-7.6 (catgets reads the installed X/Open catalogs through their inspected format, Appendix D,
phase 13) of implementation-plan.md. Public interfaces: `<libintl.h>`, `<nl_types.h>`,
`<locale.h>`, `<limits.h>` (`NL_*` limits). Normative references (plan §1.2): the man pages
`gettext(3)`, `ngettext(3)`, `textdomain(3)`, `bindtextdomain(3)`, `bind_textdomain_codeset(3)`,
`catopen(3)`, `catgets(3)`; POSIX.1-2017 `catopen`, `catgets`, `catclose`, XBD 8.2 (`NLSPATH`) and
the `gencat` utility; the GNU gettext manual ("The Format of GNU MO Files", "Specifying a Priority
List of Languages", "Additional functions for plural forms", "Being a gettext grok").
The binary formats are recorded separately in [catalogs-format.md](catalogs-format.md), the draft
of spec Appendix D.2.

**Clauses:** `s7_7_c_0`, `s7_7_c_1`, `s7_7_c_2`, `s7_7_c_3`, `s7_7_c_4`, `s7_7_c_5`, `s7_7_g_0`,
`s7_8_c_0`, `s7_8_c_p1`, `s7_8_f_0`; no clause ID for `_nl_msg_cat_cntr` (§7.7; stated only in a ⚑
COMPAT note).

`_nl_msg_cat_cntr`, `_nl_default_dirname`, `_libc_intl_domainname` and `_nl_domain_bindings` are
exported data that no installed header declares; the probe declares them itself, as `gettext.h`
and autoconf tests do for `_nl_msg_cat_cntr` (`extern int`), and the two strings as `const char[]`
(symbol sizes 18 and 5), `_nl_domain_bindings` as `void *` (size 8; only whether it is NULL is
printed). `__dcgettext` and `__dgettext` are declared by `<libintl.h>`. The probe does not pass
NULL as a msgid: `<libintl.h>` declares the argument non-null.

**Probe:** `crates/core/tests/oracle/catalogs_diff.c`, run by `catalogs_run.sh PROGRAM SCRATCH`.
The harness first builds fixture catalogs into `SCRATCH/fix` with `msgfmt` (GNU gettext-tools
0.21, package `gettext` 0.21-14ubuntu2, installed on the VM with apt for this document; the
harness prints whether `msgfmt` and `gencat` are present) and `gencat` (libc-bin, run with
`LC_ALL=C.UTF-8`). Every other catalog is written by the probe below SCRATCH; nothing is written
into the system's locale directories (catalogs are reached through `bindtextdomain`, `NLSPATH`
and absolute names).
**Record:** `catalogs.glibc.txt` (1170 lines). It was made on the VM (Ubuntu 24.04, glibc
2.39-0ubuntu8.9, kernel 6.8.0-146-generic, scratch directory on ext4) with
`cc -std=gnu17 -O1 -o p catalogs_diff.c` and
`env -i PATH=/usr/bin:/bin LOCPATH=<repo>/target/test-locales sh catalogs_run.sh p SCRATCH`,
twice, with scratch directories of different names and lengths (`~/catprobe/run-a`,
`~/catprobe/run-second-longer`): the two outputs are byte-identical, and identical to an `-O0`
build run with a third scratch name (SHA-256 `d90f04d7...804728` for all three). Nothing was left
behind: the harness removes SCRATCH (`chmod -R u+rwx` first, for the mode-0000 fixtures).
The probe undefines `_FORTIFY_SOURCE`, calls the functions by name in parentheses where
`<libintl.h>` defines a macro of the same name, and runs as an ordinary user (`running as root:
no`).

**Unstable cases.** `catalogs_run.sh PROGRAM SCRATCH unstable` runs only the damaged-catalog
cases whose offsets or sizes point outside the file (marked `~` in the probe). They are not in
the record: an earlier `-O0` build gave garbage strings where the `-O1` build was killed by
`SIGSEGV` for two of them. In the final unstable run (both builds) every `.mo` case of this kind
was killed by `SIGSEGV` (big-endian magic with little-endian words, `N` 0x7fffffff and
0x20000000, `O` or `T` beyond the file, `O` 1, hash size 3 over the string area and 0xffffffff,
a key or translation offset beyond the file, hash entries N+1, N+5 and 0xffffffff); for `.cat`, a
byte-swapped magic alone and plane size 0x40000000 and plane depth 0x55555556 with size 3 (a 32-bit overflow) were killed by `SIGSEGV`,
and plane depth 0 opened and returned bytes of the header as messages (`\x03`, `\x10`, `\x02`).

## What the probe does

Strings are printed in brackets with every byte outside printable ASCII as `\xNN` and a
backslash as `\\`; a run of more than 60 equal bytes as `<c>*<n>`; the scratch directory as
`<dir>`. Nothing prints an address, a descriptor number or the value of `_nl_msg_cat_cntr`.
"(the argument)", `S`/`P` and "(default)" mean that the function returned the very pointer it was
given (msgid, msgid / msgid_plural, catgets' default). Each case runs in a forked child (fresh
library state, an alarm of 3 to 60 s, core dumps off; a child killed by a signal is reported as
`child killed by signal S` after its own output). Every `.mo` the probe writes has no hash table
unless it is a copy of a `msgfmt` fixture.

* **Initial state** (`== initial state`, before any other call): the exported strings,
  `textdomain(NULL)`, `bindtextdomain`/`bind_textdomain_codeset` queries, the C locale's
  answers, `gettext("")`, the `NL_*` constants.
* **textdomain** (`== textdomain`): copying of the argument, the pointer returned by later
  queries, `""`, names with `/`, a 5000-byte name, `errno`; then which domain `gettext`,
  `dgettext(NULL)`, `dcgettext(NULL)`, `ngettext`, `dngettext(NULL)`, `dcngettext(NULL)`,
  `__dcgettext`, `__dgettext` use.
* **bindtextdomain** (`== bindtextdomain`): the default directory, copying, relative directories
  (`rel`, `./dot`) with a `chdir` between binding and lookup, NULL and empty domain and
  directory, trailing `/`, `//` and `/.`, a 5000-byte directory, rebinding after a catalog was
  loaded, a catalog file replaced on disk after loading (then the same directory rebound, the
  directory spelled differently, `setlocale` to the same and to another name and back).
* **Codesets** (`== bind_textdomain_codeset and conversion`): argument checks; a UTF-8 catalog
  (`ascii`, `e-acute`, `euro`, `mixed` (`naïve – €5`), `cyr` (`Жа`), `kana` (`あ`), `invalid`
  (bytes `\xff\xfe`), a plural entry) read with 15 bound codesets (none, `UTF-8`, `utf8`,
  `ISO-8859-1`, `iso-8859-1`, `ISO-8859-15`, `ASCII`, `//TRANSLIT` and `//IGNORE` forms, `KOI8-R`,
  `EUC-JP`, `UTF-16LE`, `NOPE`, `""`); the codeset changed after a lookup; `OUTPUT_CHARSET`; the
  locale's codeset (fr_FR.ISO-8859-1, `LC_CTYPE` C, C.UTF-8, ru_RU.KOI8-R, ja_JP.EUC-JP); catalogs
  in ISO-8859-1, without a header, without `charset=`, with `CHARSET`, `NOPE`, a lowercase name,
  `charset=` on another line or outside `Content-Type`.
* **Locale selection** (`== locale selection`): 30 combinations of `LC_ALL`, `LC_MESSAGES`, `LANG`
  and `LANGUAGE` with `setlocale(LC_ALL, "")` (or no `setlocale`, or only `LC_MESSAGES`, or a
  category reset afterwards), against catalogs for de_DE.UTF-8, fr_FR.UTF-8, cs_CZ.UTF-8, `C`,
  `POSIX`, `C.UTF-8`, `it`, `nl` (which alone has the msgid `only-nl`) and a corrupt `bad`; then
  changes at run time (`LANGUAGE`, the environment, `setlocale`, `uselocale`).
* **Name variants** (`== locale name variants`): for 17 locale names (through `LANGUAGE` with
  `LC_ALL` hu_HU.UTF-8, or through `setlocale`), catalog directories for every plausible variant
  of the name and some decoys; the probe repeatedly looks up a fresh domain and removes the
  directory found, which prints the complete preference order (`order:`) and the directories
  never used (`never:`).
* **Categories** (`== dcgettext categories`): `dcgettext` and `dcngettext` with each of the 12
  categories, `LC_ALL`, -1, 13, 99, `INT_MAX`, `INT_MIN`, against `<locale>/<CATEGORY>/catd.mo` for
  five locales; with and without `LANGUAGE`, and with `LC_MESSAGES` C.
* **Plural forms** (`== plural forms`): 42 headers (none, without `Plural-Forms`, nine real
  languages, mismatched `nplurals`, syntax errors, missing parts, division by zero, wrap-around,
  precedence, `!`, unary minus, constants beyond 32 bits, `?:` chains, `&&`/`||`, octal-looking
  constants, spacing, trailing text, 200 and 4000 nested parentheses, the header field's spelling
  and position), each with `n` = 0, 1, 2, 3, 4, 5, 7, 10, 11, 12, 14, 19, 21, 22, 25, 100, 101, 102,
  103, 111, 10^6, 2^32-1, 2^32, 2^32+1, 2^64-1; a singular-only entry under `ngettext`; the
  untranslated case.
* **Contexts and special msgids** (`== contexts and special msgids`): `ctx\x04id` keys, an empty
  context or id, two separators, a plural with a context, bytes outside ASCII, near misses, an
  empty translation, `gettext("")` with and without a header entry and without a catalog,
  pointer identity, a 20000-byte msgid.
* **msgfmt catalogs** (`== msgfmt catalogs, hash table and search`): `a.mo` with and without a
  hash table and big-endian, the probe's writer in both byte orders, 500 entries with and
  without a hash table (all found, misses), `big.mo` with its hash table zeroed or every entry
  set to a wrong index, a Russian catalog from `msgfmt`, a revision-1 file with a
  system-dependent string, an ISO-8859-1 catalog.
* **Damaged .mo files** (`== damaged .mo files`): 33 cases (truncations, wrong magic, revisions,
  `N` 0 and 2, tables overlapping, lengths, missing NULs, unsorted and duplicate keys, a
  directory, mode 0000, a FIFO, symbolic links) and `LANGUAGE=bad:good`.
* **`_nl_msg_cat_cntr`** (`== _nl_msg_cat_cntr`): whether it changed after each of 31 steps.
* **errno** (`== errno across the gettext family`): 15 calls (found, not found, no catalog,
  unreadable and truncated catalogs, failed conversion, `ngettext`, `LC_ALL`, `textdomain`,
  `bindtextdomain`, `bind_textdomain_codeset`) with `errno` preset to 0, `EINTR`, `ENOMEM`
  and 12345.
* **X/Open catalogs**: the probe's `.cat` writer against `gencat` (byte identity); a `gencat`
  catalog `main.cat` read with 34 (set, message) pairs including 0, negatives, `INT_MAX` and
  `INT_MIN`, `errno`, pointer stability, a NULL default, a second `catopen`, `catclose`, and the
  invalid descriptor; 59 `catopen` cases for `NLSPATH` (every conversion, empty templates,
  unknown conversions, trailing `%`, candidates that are directories, unreadable, empty or not
  catalogs, absolute and relative names, an empty name, a name with `%`, `LANG` with `/`, a path
  over `PATH_MAX`) and for the locale sources (flags 0, `NL_CAT_LOCALE`, 2, -1 against
  `setlocale`, `LANG`, `LC_ALL`, `LC_MESSAGES`); `NLSPATH` changed between calls, the file
  replaced or removed after `catopen`; 23 damaged `.cat` files.

## Observations the record shows directly

### Initial state, exported data

* `_nl_default_dirname` is `/usr/share/locale`, `_libc_intl_domainname` `libc`.
  `_nl_domain_bindings` is NULL until the first `bindtextdomain` that sets a directory (queries
  with NULL do not change it) and non-NULL afterwards.
* `textdomain(NULL)` is `messages`; `bindtextdomain(d, NULL)` is `/usr/share/locale` for any
  domain never bound (also `messages`); `bind_textdomain_codeset(d, NULL)` is NULL.
* In the C locale `gettext` returns its argument; `ngettext` returns `msgid` for `n == 1` and
  `msgid_plural` otherwise (0, 2, `ULONG_MAX`); `gettext("")` returns `""`.
* `NL_SETD` 1, `NL_CAT_LOCALE` 1, `sizeof(nl_catd)` 8; `NL_SETMAX`, `NL_MSGMAX`, `NL_TEXTMAX`,
  `NL_NMAX` 2147483647, `NL_ARGMAX` 4096, `NL_LANGMAX` 2048.

### textdomain, bindtextdomain (§7.7)

* `textdomain(name)` copies the name and returns the library's copy (not the argument);
  `textdomain(NULL)` returns that same pointer. `textdomain("")` sets the domain back to
  `messages` and returns `messages`. Any other string is accepted as a domain (`a/b`, `../x`, 5000
  bytes). `textdomain(textdomain(NULL))` works. `errno` is unchanged.
* The default domain is what `gettext`, `ngettext` and the `d*` functions with a NULL domain use;
  `__dcgettext` and `__dgettext` behave as `dcgettext` and `dgettext`. `dgettext("", id)` does not
  use the default domain: it returns `id`.
* A domain's catalogs are found only under its own binding (or `/usr/share/locale`): a catalog
  for `other` lying in the tree bound for `tdom` is not used until `other` is bound there.
* `bindtextdomain(domain, dir)` copies `dir` and returns the copy; a later query returns the same
  pointer. The directory is kept as given: `rel`, `./dot`, `<dir>/b4/`, `<dir>//b5/.` are
  returned verbatim, and a relative directory is resolved against the working directory **at
  lookup time** (`relative to the cwd at lookup time`). `""` is accepted and makes paths relative
  to the working directory (`found relative to the cwd`). A 5000-byte directory is accepted.
* `bindtextdomain` with a NULL or empty domain returns NULL and leaves `errno` 0 (also with a
  NULL directory). `bind_textdomain_codeset` likewise.
* A domain bound only through `bind_textdomain_codeset` still reports the default directory.
* Rebinding to another directory takes effect at the next lookup, also after a catalog was
  loaded, and rebinding back reuses the first catalog. A catalog **file replaced on disk** is not
  seen: the old translation stays 1.1 s later, after a rebind to the same directory string, after `setlocale`
  to the same name and after `setlocale("C")` and back; a rebind with the directory spelled
  differently (`<dir>/b7/`) loads the new file.

### Codesets and conversion (§7.7)

* Translations are converted from the catalog's `charset=` to the codeset bound with
  `bind_textdomain_codeset`, else to the codeset of the calling thread's `LC_CTYPE`. Bound
  `UTF-8`, `utf8` and `""` and a UTF-8 locale return the bytes unchanged.
* Characters the target cannot represent are transliterated where the target knows how, else
  replaced by `?`, for every target **with or without** `//TRANSLIT`: `€` gives `EUR` (and `\xa4`
  in ISO-8859-15), `–` `-`, `é` `e` in ASCII and KOI8-R, `Жа` `??` in ISO-8859-1 and `ZHa` in a C
  `LC_CTYPE`, `あ` `?`; `é` in a C `LC_CTYPE` is `?`. `//IGNORE` gives the same results.
  `UTF-16LE` returns the converted bytes, which the caller sees cut at the first NUL (`ascii` ->
  `p`).
* Input that is not valid in the catalog's codeset (`\xff\xfe` in UTF-8) makes that lookup
  return the msgid when a conversion is needed; with no conversion the bytes are returned as
  stored. An unknown target (`NOPE`) makes every lookup in the domain return the msgid (or
  `msgid_plural`). In every case `errno` is unchanged.
* An ISO-8859-1 catalog is converted to UTF-8 in a UTF-8 locale and returned as stored in an
  ISO-8859-1 locale or with `ISO-8859-1` bound. A catalog without a header entry or whose header
  has no `charset=` is never converted; `charset=CHARSET` or `charset=NOPE` makes every lookup
  untranslated, in any locale. The header parsing is described in catalogs-format.md.
* The codeset can be changed after lookups: the next lookup returns the new conversion, and an
  earlier result keeps its bytes (`first result still reads`). Two lookups of the same msgid
  return the same pointer. `OUTPUT_CHARSET` in the environment has no effect.
* `bind_textdomain_codeset` copies its argument (`result is the argument: no`); `""` is stored and
  returned as `""`.

### Locale selection (§7.7; gettext(3); the GNU manual's LANGUAGE section)

* The locale used is the name of the calling thread's `LC_MESSAGES` category (as `setlocale` or
  `uselocale` set it), not the environment: without a `setlocale` call nothing is translated,
  whatever `LC_ALL`, `LANG` or `LANGUAGE` say, and changing `LC_MESSAGES` or `LANG` in the
  environment after `setlocale` changes nothing. `setlocale(LC_ALL, "")` applies
  `LC_ALL` > `LC_MESSAGES` > `LANG`, empty values counting as unset; when one named locale is not
  installed `setlocale` fails and everything stays C (untranslated).
* `LANGUAGE` (a colon-separated list) replaces the locale name for `LC_MESSAGES` lookups when the
  `LC_MESSAGES` locale is not `C`, `POSIX` or **`C.UTF-8`** (all three ignore it); `LC_CTYPE`
  set to C does not matter. Empty `LANGUAGE` is ignored; empty list elements are skipped
  (`:::it:`). Each element is tried in order, **per msgid**: with `it:nl`, `where` comes from
  `it` and `only-nl` from `nl`. A corrupt catalog is skipped (`bad:it` gives `it`).
* The `LC_MESSAGES` locale is **not** a fall-back after the `LANGUAGE` list: `LANGUAGE=zz`
  (no catalog) gives untranslated text although a de_DE.UTF-8 catalog exists.
* A list element `C` or `POSIX` ends the search with the untranslated text (`C:it` and
  `POSIX:it` untranslated; `it:C` gives `it`). Elements with `/` are used as path components
  (`../sel/it` and `/it` find `<dir>/sel/it`).
* The thread locale counts: under `uselocale` of a locale with `LC_MESSAGES` fr_FR.UTF-8 the
  French catalog is used, and a thread locale `C` ignores `LANGUAGE`.
* **Caching**: after `LANGUAGE` changes, a msgid already looked up in the domain keeps its
  previous translation (`LANGUAGE=nl, nothing else` still gives `it`) while a msgid not looked
  up before uses the new list (`only-nl` from `nl`). `textdomain(textdomain(NULL))` (which bumps
  `_nl_msg_cat_cntr`) makes the new value effective; `setlocale(LC_MESSAGES, current name)` does
  not bump it and changes nothing. Unsetting `LANGUAGE` afterwards, or setting it back to `it`,
  or changing `LANG`, still gives the cached `nl`; a `setlocale` to another name gives fresh
  results.

### Locale name variants (catalog directory order)

For a name `language[_territory][.codeset][@modifier]` the directories tried are all
combinations that keep the language, from most to least specific, in this order (`LANGUAGE=
xx_YY.ISO-8859-1@mod`): `xx_YY.ISO-8859-1@mod`, `xx_YY.iso88591@mod`, `xx_YY@mod`,
`xx.ISO-8859-1@mod`, `xx.iso88591@mod`, `xx@mod`, `xx_YY.ISO-8859-1`, `xx_YY.iso88591`, `xx_YY`,
`xx.ISO-8859-1`, `xx.iso88591`, `xx`. That is: the modifier is dropped last, then the territory,
then the codeset; a codeset is tried as written, then **normalized**. Directories named by any
other spelling are never used (`ISO8859-1`, `iso-8859-1`, `@MOD`, `XX`, another territory, the
modifier alone, the `LC_ALL` locale `hu`).

* **Normalization:** letters lowercased, everything but letters and digits removed (`UTF-8` ->
  `utf8`, `EUC-JP` -> `eucjp`, `utf-8` -> `utf8`), and **`iso` prefixed when only digits remain**
  (`8859-1` -> `iso88591`; `88591` is never tried). When the normalized form equals the written
  one (`xx.utf8`), it is tried once. A codeset is never added (`xx_YY` does not find
  `xx_YY.utf8`), and `xx.utf8` does not find `xx.UTF-8`.
* An empty codeset (`xx_YY.@mod`) is treated as absent: `xx_YY@mod`, `xx@mod`, `xx_YY`, `xx`
  (`xx_YY.` and `xx_YY.@mod` are never tried). `_YY` (an empty language) finds only `_YY`.
  `xx_YY_ZZ` tries `xx_YY_ZZ` (the territory `YY_ZZ`), then `xx`.
* **Aliases:** `LANGUAGE=german` and `GERMAN` are replaced through
  `/usr/share/locale/locale.alias` (`german` -> `de_DE.ISO-8859-1`) and the replacement's
  variants are tried; the directory `german` itself is never tried.
* The same order holds for the `setlocale` name (`de_DE.UTF-8@euro`, `fr_FR.ISO-8859-1`). Note
  that this differs from `setlocale`'s own locale search in D.1 (which normalizes `8859-1` to
  `88591` and prefers other orders); see "Disagreements".

### dcgettext categories (§7.7; gettext(3))

* `dcgettext(domain, id, category)` looks in `dir/<locale>/<CATEGORY>/domain.mo`, where
  `<locale>` is that category's locale name and `<CATEGORY>` its name (`LC_TIME`,
  `LC_NUMERIC`, ...), for each of the 12 categories. `dcngettext` alike.
* `LANGUAGE` applies to every category, and to each category whose own locale is not C even when
  `LC_MESSAGES` is C (`LC_TIME` -> `it/LC_TIME`, `LC_MESSAGES` untranslated).
* `LC_ALL`, -1, 13, 99, `INT_MAX` and `INT_MIN` return the msgid (`dcngettext`: msgid_plural for
  n 2); nothing crashes and `errno` is unchanged.

### Plural forms (§7.7; ngettext(3); the GNU manual's plural-forms sections)

* The form index is the value of the `plural=` expression of the header's `Plural-Forms`, a C-like
  expression in `n` (unsigned long): `?:` (right-associative), `||`, `&&`, `==`, `!=`, `<`, `>`,
  `<=`, `>=`, `+`, `-`, `*`, `/`, `%`, `!`, parentheses and decimal constants, with C precedence
  (`1+2*n%3` is `1+((2*n)%3)`). The record's nine real rules (`n != 1`, French `n>1`,
  Japanese, Russian, Polish, Czech, Slovenian, Irish, Arabic) give the expected forms for every
  `n`, and evaluation is 64-bit: `n>4294967295` and `n==4294967297` distinguish 2^32-1, 2^32 and
  2^32+1; Arabic's `n%100` for 2^32 gives form 4. The `msgfmt`-built Russian catalog agrees with
  the probe-written one.
* **Default:** with no header entry, a header without `Plural-Forms`, or a `Plural-Forms` that
  cannot be used (syntax error `n !=`, `nplurals=x`, missing `plural=`, missing `nplurals=`), the
  rule is `n != 1` (form 0 for 1, form 1 otherwise, also for 0).
* **Accepted oddities:** a missing `;` after the expression, spaces around `;` (but a blank
  before `=` makes the whole `Plural-Forms` unusable, so the default rule applies: the record's
  `-- spaces:` case gives form 0 for 1 and form 1 otherwise, which is `n != 1`, not its
  `n == 1 ? 1 : 0`; corrected 2026-10-04 after the implementation's probe), no
  spaces, trailing text after the `;`, 200 and 4000 nested parentheses, a lowercase field name
  `plural-forms:`, the field after `Content-Type`, inside another line (`X-Note: Plural-Forms:`),
  and `plural=` before `nplurals=`. `010` is decimal 10, not octal 8. Unary minus is not
  accepted (`-n` falls back to the default rule).
* **Index out of range:** when the expression's value is not below `nplurals`, form 0 is
  returned (`nplurals=2; plural=n;`: n 0 -> f0, 1 -> f1, 2 and more -> f0; `nplurals=1000;
  plural=n;` alike; `n-5` wraps to a huge value -> f0, except n 7 -> f2). When the value is below
  `nplurals` but beyond the forms stored, form 0 is returned too (`nplurals=3; plural=n%3;` with 2
  forms: 2 -> f0); extra stored forms beyond `nplurals` are not used (3 forms, `nplurals=2`:
  n%3 = 2 -> f0). `nplurals=0` always gives f0.
* **Division by zero** in the expression (`n/0`, `n%0`, and `10/n` evaluated for n 0) kills the
  process with `SIGFPE`.
* A singular-only entry is returned for every `n` (`Solo` for n 2); `gettext("file")` on a
  plural entry returns form 0; `ngettext("file", "other", 2)` finds the entry keyed
  `file\0files` (only the singular is the key).
* Untranslated: `msgid` for n 1 and `msgid_plural` for every other n (0 and 2^64-1 included).

### Contexts and special msgids

* A msgid containing `\x04` is matched as a whole key: `menu\x04open` finds its context entry,
  `open` finds the context-free entry, `menu\x04missing` returns the argument itself (the
  pointer test `pgettext` in `gettext.h` relies on). An empty context (`\x04bare`), an empty id
  in a context (`menu\x04`), two separators and plurals with contexts are ordinary keys. There is
  no fall-back from a context key to the bare msgid.
* Matching is byte-exact (`Open`, `open `, ` open` are not found); UTF-8 bytes match.
* An entry whose translation is empty returns `""` (not the msgid).
* `gettext("")` returns the header entry's text when the catalog has one, the argument itself
  otherwise (and without a catalog). Found translations come back at the same address on every
  call; not-found lookups return the argument itself (also for a 20000-byte msgid).

### Hash table and search; damaged .mo files (§7.7 guarantees)

* `msgfmt` catalogs with and without a hash table, in either byte order, and the probe's
  table-less writer give the same results; 500 of 500 entries are found in both 500-entry
  catalogs and near misses (`key-500`, `key-`, `key-0000`, `Key-001`, `key-001 `) are not.
* With a hash table present, the table decides alone: zeroed, every lookup fails; with every
  entry naming a valid but wrong index, the first lookup never returns (`SIGALRM` after 3 s).
  Without one, keys are found by a search that assumes the sorted order (unsorted keys: `hello`
  found, `zz` and `file` not). Details in catalogs-format.md.
* A revision-1 file with a system-dependent string (`count %<PRIu64> x`) matches `count %lu x`.
* Files that are not usable catalogs are ignored (untranslated, `errno` unchanged): empty, 4, 20,
  27 and 28 bytes, a wrong magic, revisions 1, 2, 0x20000 and 0xffffffff, `N` 0, `O` = `T`, a
  directory, mode 0000, a dangling link. Revision 0x10000 is used normally. The other damaged
  files that stay inside the file give partial or odd results listed in catalogs-format.md
  (`N` 2, half a file, lengths, a missing NUL, duplicate keys).
* **A FIFO in place of a `.mo` blocks the lookup** (killed by the 3 s alarm): the file is opened
  without `O_NONBLOCK` and nothing checks its type first.
* `LANGUAGE=bad:good` with a wrong-magic catalog under `bad` translates from `good`.

### `_nl_msg_cat_cntr` (§7.7 ⚑ COMPAT; D-7.5)

It changed after: `textdomain(name)` (also the same name again, and `""`), `bindtextdomain` with
a new directory (twice: the first binding and a different directory), `bind_textdomain_codeset`
with a new codeset, and every successful `setlocale` that changes a category's name
(`LC_ALL`, `LC_MESSAGES`, also `LC_CTYPE` and `LC_NUMERIC`). It did not change after: queries
(`textdomain(NULL)`, `bindtextdomain(d, NULL)`, `bind_textdomain_codeset(d, NULL)`,
`setlocale(c, NULL)`), the same directory or codeset again, `bindtextdomain(NULL, dir)`,
`setlocale` to the current name, a failed `setlocale`, lookups (including the one that loads a
catalog and a lookup after a rebinding), `newlocale`, `uselocale`, `setenv("LANGUAGE")`,
`catopen`.

### errno (§7.7 guarantee)

All 15 calls leave `errno` exactly as preset (0, `EINTR`, `ENOMEM`, 12345), including lookups
that find no catalog, an unreadable one, a truncated one, and a failed conversion.

### catopen, catgets, catclose (§7.8; POSIX)

* **Descriptor.** `catopen` reads the whole catalog at once (the file is mapped and its
  descriptor closed: `descriptors added 0`); later changes to the file are not seen, and the
  catalog stays readable after the file is removed. Two `catopen` calls give distinct
  descriptors, each closed independently. A change of `LC_MESSAGES` after `catopen` does not
  affect an open catalog.
* **catgets** returns a pointer into the catalog (the same pointer on every call) or the
  default pointer itself. Found: `errno` unchanged. Missing message with set and message
  numbers in 0..`INT_MAX-1` of an open catalog: the default with **`ENOMSG`** (`1,0`, `0,1`,
  `1,18`, `2,2`, `3,1`, `1,INT_MAX`). Negative set or message numbers and set `INT_MAX`: the
  default with `errno` **0** (cleared: the probe sets it to 0 before each call, and the found
  case shows `errno` is not otherwise touched). A NULL default is returned as NULL (`ENOMSG`).
  `catgets((nl_catd)-1, ...)` returns the default with `errno` 0.
* **catclose** returns 0 and leaves `errno` alone; `catclose((nl_catd)-1)` returns -1 with
  `EBADF`.
* **Message text.** `catgets` returns the bytes `gencat` stored. The fixture shows `gencat`'s
  processing (not the library's): `\t`, `\n`, `\v`, `\b`, `\r`, `\f`, `\\` and octal escapes are
  decoded (`\0101` gives `A`: four octal digits were taken), an unknown escape drops the
  backslash (`\q` -> `q`), a trailing backslash continues the line, one blank (space or tab)
  separates the number from the text and further blanks are kept, `$quote "` enables quotes
  (`""` is empty, `\"` a quote, trailing spaces kept) and `$quote` alone disables them, a number
  with a blank and no text **and a number alone** both store `""`, message numbers 32767 and
  100000 are accepted, and `$delset 7` after set 7 in the same source deletes nothing.
  `gencat` in the C locale rejects bytes outside ASCII and octal escapes above `\177` ("invalid
  character", "invalid escape sequence") and exits 1; the harness runs it in C.UTF-8.
* **Name with `/`**: used as a path, `NLSPATH` ignored (absolute and relative names). Errors are
  the open's: missing `ENOENT`, below a file `ENOTDIR`, unreadable `EACCES`; a directory gives
  **`EINVAL`**.
* **NLSPATH** (read at each `catopen`): templates separated by `:`; `%N` the name, `%L` the
  locale name, `%l` the language, `%t` the territory, `%c` the codeset, `%%` a `%`. As observed:
  `de_DE.UTF-8@euro` gives `%l` `de`, `%t` `DE`, `%c` **`UTF-8@euro`** (the modifier stays with
  the last element present: `de_DE@euro` gives `%t` `DE@euro`, `de@euro` gives `%l` `de@euro`);
  `de_DE_X.Y` gives `%t` `DE_X`, `%c` `Y`; `C.UTF-8` gives `%l` `C`, `%c` `UTF-8`. `LANG` unset
  or empty gives `%L` and `%l` `C`; `POSIX` stays `POSIX`.
* Empty templates (leading, adjacent or trailing `:`) stand for `%N` (relative to the working
  directory). An empty `NLSPATH` does not: the name is not found in the working directory.
* A template with an unknown conversion (`%X`) is skipped; a template ending in a lone `%`
  ruins the search: `<T>/%N%` and `<T>/x%:<T>/ok/%N` both fail with `ENOENT` (strace shows that
  the template after it is not tried either).
* After the `NLSPATH` templates (or without `NLSPATH`), the default templates are tried
  (system calls, below). A template without `%N` names a fixed file, which is opened for any
  name.
* **Search and errors:** a missing or unreadable candidate moves on to the next (an unreadable
  last candidate gives `ENOENT`, not `EACCES`); a candidate that is a directory or an empty file
  **stops the search** with `EINVAL`; a candidate that is not a catalog stops it with `-1` and
  **`errno` 0**. A path over `PATH_MAX` gives `ENOENT`. An empty name fails with `EINVAL` when the
  template then names a directory (`<T>/n/`, and `/usr/share/locale/de/` by default) and opens
  `<T>/x` for the template `<T>/x%N`. A `%L` in the name is not substituted. `LANG` is
  substituted unchecked, as a path: `LANG=sub/de` opens `<T>/sub/de/app`, and `LANG=../x` with
  the template `<T>/in/%L/%N` opens `<T>/x/app`, outside the template's directory.
* **Locale source:** flag 0 uses `LANG` only (not `LC_ALL` or `LC_MESSAGES` from the
  environment, not the `setlocale` state; `LANG` unset gives `C`); `NL_CAT_LOCALE` uses the
  `LC_MESSAGES` category's name (`C` without a `setlocale` call; the full requested name such as
  `de_DE.UTF-8@euro`). Flags 2 and -1 behave as 0.

### Damaged .cat files

Summarized in catalogs-format.md (D.2.2): `catopen` rejects files shorter than 12 bytes and
directories with `EINVAL`, wrong magic numbers, files shorter than their tables, big-endian
files and string offsets beyond the file with `errno` 0; plane size 0 kills the process with
`SIGFPE`; a FIFO blocks `catopen` (killed by the 3 s alarm); mode 0000 gives `EACCES`.

### System calls

A separate small program (not part of the record) was traced with `strace` on the VM:

* A `gettext` lookup that finds `<dir>/de_DE.UTF-8/LC_MESSAGES/dom.mo`:
  `openat(path, O_RDONLY)` (**no `O_CLOEXEC`**), `fstat`, `mmap(NULL, size, PROT_READ,
  MAP_PRIVATE, fd, 0)`, `close`. A second lookup makes no system call. Candidates that do not
  exist are each one failed `openat`, in the order above.
* For a domain bound to `/usr/share/locale` (explicitly or by default) Ubuntu's library also
  tries every candidate below **`/usr/share/locale-langpack`** after those below
  `/usr/share/locale`. This is a distribution addition; a directory bound elsewhere is searched
  alone.
* `catopen`: `openat(path, O_RDONLY|O_CLOEXEC)`, `fstat`, `mmap(PROT_READ, MAP_PRIVATE)`,
  `close`; `catgets` makes no system call; `catclose`: `munmap`.
* The default `catopen` templates after `NLSPATH`'s are `/usr/share/locale/%L/%N`,
  `/usr/share/locale/%L/LC_MESSAGES/%N`, `/usr/share/locale/%l/%N`,
  `/usr/share/locale/%l/LC_MESSAGES/%N` (with `LANG=de` the last two repeat the first two). With
  an `NLSPATH` template ending in `%`, the first default template is not tried either.

## Disagreements between the spec, the record and the documentation

* **§7.7 Guarantees "It never fails visibly"** vs the record: a `Plural-Forms` expression that
  divides by zero kills the process with `SIGFPE`; a FIFO in place of a catalog blocks the
  lookup forever; a hash table naming wrong indices loops forever; damaged offsets crash
  (unstable cases). The spec should either qualify the guarantee (well-formed catalogs) or
  Liella should decide to be robust (and record the difference).
* **§7.8 Failure "`catgets` returns the default and sets `errno`"** and POSIX (`ENOMSG` "shall
  fail") vs the record: negative numbers, set `INT_MAX` and the invalid descriptor give the
  default with `errno` 0. POSIX's optional `EBADF` for an invalid descriptor is not reported.
* **§7.8 / POSIX catopen "returns -1 and sets errno"** vs the record: a candidate file that is
  not a catalog (wrong magic, short tables, string offsets beyond the file, big-endian) gives -1
  with `errno` **0**. A directory gives `EINVAL` (POSIX lists no such case; `EISDIR` is not
  used). POSIX says an empty name gives `ENOENT`; the record shows `EINVAL` (the default
  template names a directory) or a successful open (`<T>/x%N`).
* **POSIX XBD 8.2 `%t`/`%c`**: "the codeset element" vs the record: the modifier is appended to
  the last element present (`%c` `UTF-8@euro`, `%t` `DE@euro`, `%l` `de@euro`). POSIX does not
  describe an empty `NLSPATH`; read as one empty template it would mean `%N`, which the record
  does not do.
* **POSIX catopen: search the default path when the catalog "cannot be found in any of the
  components"** vs the record: a candidate that is a directory, an empty file or not a catalog
  ends the search with a failure instead of continuing.
* **D-7.5 "`_nl_msg_cat_cntr` ... bumped on every catalog change"** vs the record: it is bumped by
  domain, binding, codeset and locale changes, not by loading a catalog, and not by a
  `LANGUAGE` change; and `textdomain` bumps it even when the name does not change. The spec's
  wording should say "bumped when a setting that selects catalogs changes".
* **The GNU gettext manual ("Being a gettext grok") says `dcgettext` determines the language
  from `LANGUAGE`, `LC_ALL`, `LC_xxx`, `LANG` at every call** vs the record: the reference library
  reads `LANGUAGE` from the environment but takes the locale from the `LC_MESSAGES` category
  (`setlocale`/`uselocale`), never from `LC_ALL`/`LC_xxx`/`LANG` directly, and it returns cached
  translations for msgids already looked up until something bumps `_nl_msg_cat_cntr`. The
  manual's remedy (bump the counter after `setenv("LANGUAGE")`) matches the record's
  `textdomain(textdomain(NULL))` case. That chapter is marked outdated by its own text.
* **The GNU manual: "LANGUAGE is ignored if the locale is set to 'C'"** and gettext(3) ("not the
  C locale") vs the record: also ignored for `POSIX` and for `C.UTF-8`; and a `C`/`POSIX`
  element inside `LANGUAGE` ends the list untranslated, which neither text says.
* **The GNU manual (LANGUAGE): "you still need to have LANG (or LC_ALL) set to the primary
  language"** is consistent with the record; but the locale is not used as a fall-back after the
  list (`LANGUAGE=zz` untranslated with a de_DE.UTF-8 catalog present), which a reader might
  expect.
* **Appendix D.1 (setlocale name resolution)** vs the record for catalog directories: D.1
  normalizes `8859-1` to `88591` and orders candidates by weights; gettext's candidate list
  prefixes `iso` to an all-digit codeset and uses the order above. These are two different
  procedures; the spec should state gettext's separately (it is not D.1's).
* **bindtextdomain(3): "domainname must be a non-empty string"**; textdomain(3) likewise. The
  record: `textdomain("")` is accepted and means `messages`; `bindtextdomain("", dir)` returns
  NULL with `errno` 0 (no error indication other than NULL, while the man page says NULL means
  `ENOMEM`).
* **bind_textdomain_codeset(3): the codeset "must be a valid encoding name"**: an invalid one is
  accepted and makes the domain's lookups untranslated; `""` is accepted and means no conversion.
* **POSIX gencat** (not the library; recorded because the fixture shows it): a message number
  without a separator or text should delete the message, and `$delset` should delete a set; the
  installed `gencat` stores `""` and deletes nothing within one source. POSIX allows one to three
  octal digits; `gencat` took four (`\0101` -> `A`). POSIX makes characters outside the portable
  set locale-dependent; `gencat` in C rejects them.
* §7.7 Kernel lists `read`; the record's traces show `mmap` and no `read` of catalog data
  (harmless).

## Observations that look like historical artifacts

The implementer may decide these differently (and record the difference).

* The translation cache ignores `LANGUAGE` changes until `_nl_msg_cat_cntr` changes, and a catalog
  file replaced on disk is never re-read unless the binding string changes.
* `nplurals` is not used to bound the form index beyond falling back to form 0; an invalid
  `Plural-Forms` silently becomes `n != 1`; division by zero raises `SIGFPE`.
* With a hash table present there is no fall-back search; a bad table loops or crashes.
* `catopen` stops at a directory or a non-catalog candidate instead of trying the next one, and
  reports a non-catalog with `errno` 0.
* `catgets` clears `errno` for negative numbers and set `INT_MAX` (likely an overflow of
  `set + 1` and a range check that does not set `ENOMSG`).
* `NLSPATH` `%c`/`%t`/`%l` keep the `@modifier`; a trailing lone `%` disables the remaining search.
* A FIFO as a catalog blocks both `gettext` and `catopen`.
* Locale names are used as path components unchecked: `LANG=../x` in `%L` and `LANGUAGE=../sel/it`
  or `/it` reach directories outside the template or binding (relevant to the secure-execution
  rules, which this record does not probe).
* Ubuntu's `/usr/share/locale-langpack` fall-back: probed 2026-10-22 (DIFF-AUDIT-FOLLOW-GLIBC) and followed (#411): `LC_MESSAGES` only, after the domain's directory for each `LANGUAGE` element, for any binding (default, `/usr/share/locale/`, another absolute or a relative directory); a catalog without the msgid falls through to it.

## What the record does not settle

* **The `.mo` hash function** is not documented in the manual and was not derived. An
  implementation that always searches the sorted originals agrees with the record for every
  well-formed catalog and the zero-hash case does not (the reference finds nothing). Decision
  for the coordinator/owner: accept the difference for damaged hash tables, or obtain the hash
  function from a permitted source (the gettext manual says it is "fairly dependent on GNU
  gettext code").
* The full syntax of system-dependent strings (other `<PRI...>` macros, the `I` flag) and the
  minor-1 header with zero system-dependent strings: only one msgfmt file was probed.
* The unstable cases (offsets and sizes outside the file): the reference library crashes or
  reads foreign memory; the record leaves them out, and Liella should not crash (to be recorded
  as a deliberate difference).
* Lookups concurrent with `setlocale`, `textdomain`, `bindtextdomain` or `LANGUAGE` changes in
  other threads; `catgets` from several threads.
* `ENOMEM` paths of `textdomain`, `bindtextdomain`, `bind_textdomain_codeset` and `catopen`.
* The secure-execution behaviour (`NLSPATH` cleared, spec/security/threat-model.md; `LANGUAGE`
  and relative or `/`-containing locale names in set-user-ID programs): not probed.
* Whether `gettext` reuses a mapping when two domains name the same file, and when mappings
  are released (never, as far as the traces show).
* `catopen` with `NLSPATH` set in a privileged process (POSIX: undefined).
* The `.cat` files of other byte orders on a big-endian host, and what `catopen` does with a
  byte-swapped magic beyond the cases above (`SIGSEGV`, unstable).
