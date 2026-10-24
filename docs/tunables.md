# Tunables (`GLIBC_TUNABLES`)

Spec §1.8; decision D-1.8 (policy R15-6, LEGACY-TUNABLES-POLICY, 2026-10-23); code
`crates/core/src/config.rs`; tests `s1_8_c_0`, `s1_8_c_0_syntax`, `s3_2_c_1`.

`GLIBC_TUNABLES=name=value:name=value` is read once at startup, before any user code runs.
Liella parses the whole syntax as glibc 2.39 does (black-box probes on the host glibc,
2026-10-23), honours the entries that map to a Liella setting, and ignores every other entry
silently. Each tunable also has a `liella.*` name (a Liella extension; glibc ignores those names).

## Secure mode

In a secure-execution process (`AT_SECURE`) no tunable is honoured and the variable is removed
from the environment at start (`spec/security/threat-model.md`, the `GLIBC_TUNABLES` row and
loader rule 1; phase 14 applies the same rule in the dynamic loader).

## Honoured

| Name (alias) | Range | Effect |
|---|---|---|
| `glibc.cpu.hwcaps` (`liella.cpu.hwcaps`) | comma list | `-AVX2`, `-ERMS` forbid those machine paths; `AVX2`, `ERMS` ask for them (used only when the CPU has the feature, otherwise refused). The 2.26-era `_Usable` suffix is accepted. Other items are ignored |
| `glibc.cpu.x86_rep_movsb_threshold` (`liella.cpu.rep_movsb_threshold`) | 0x100 and up | bytes from which copies use `rep movsb` |
| `glibc.cpu.x86_rep_stosb_threshold` (`liella.cpu.rep_stosb_threshold`) | 1 and up | bytes from which fills use `rep stosb` |
| `glibc.pthread.rseq` (`liella.pthread.rseq`) | 0 or 1 | 0: threads register no rseq area (§22.9) |
| `glibc.malloc.perturb` (`liella.malloc.perturb`) | 0 to 255 | `M_PERTURB` of the C `malloc` family (§3.2); wins over `MALLOC_PERTURB_` |

The ranges are the ones the host loader lists (`ld.so --list-tunables`).

## Ignored

Every other name, silently: the rest of the 41 tunables glibc 2.39 lists (`glibc.malloc.*`
arena, tcache, threshold and `check` settings, `glibc.elision.*`, `glibc.rtld.*`,
`glibc.mem.*`, `glibc.gmon.*`, the other `glibc.cpu.*` and `glibc.pthread.*` entries) have no
neutral meaning in Liella's design (D-3.4 for the allocator), and unknown names are ignored by
glibc too. Phase 14 and later stories add a row here when a tunable gains a meaning.

## Syntax (probe-settled)

* Entries are separated by `:`; a value runs from the first `=` to the next `:`.
* An entry without `=` is skipped, except the last: when the text after the last `:` has no
  `=` (`a=1:`, `a=1:b`, `:`, `foo`), the whole string is malformed and nothing is honoured.
  An empty string sets nothing.
* A number: spaces or tabs, an optional `+`/`-`, then `0x`/`0X` and hexadecimal digits, `0` and
  octal digits, or decimal digits. The digit run may be empty (`=`, `=+`, `=0x` read 0).
  Anything after it (`5x`, `7 `, `09`, `0xg`; also a newline, vertical tab or carriage return
  in place of the leading blank) or a value above 2^64 - 1 makes the value malformed.
  `-0` reads 0; any other negative value is out of range (`perturb=-1` and
  `x86_rep_movsb_threshold=-1` were ignored without a warning).
* A malformed or out-of-range value is ignored; a later valid setting replaces an earlier one.
* Names are case-sensitive.

Not probed (the model above is assumed): a form feed as the leading blank, a sign before
`0x`, a value containing `=`.

## Differences

* glibc writes `WARNING: ld.so: invalid GLIBC_TUNABLES ...: ignored.` to standard error for a
  malformed string or a malformed value of a name it knows (static programs too); out-of-range
  values are silent. Liella writes the same lines (ruling #1096, TUNABLES-WARNING:
  `liella::config::warnings`, one line per malformed value of the 38 numeric names, or one
  line for a malformed string; none in secure mode, as glibc). A dynamic program started by
  another `ld.so` gets that loader's lines only.
* glibc applies a CPU-dependent minimum to `glibc.cpu.x86_rep_movsb_threshold` above the listed
  0x100 (0x100 itself was not taken on the probe host, 0x1000 was). Liella takes the listed
  range; the threshold chooses between two correct copy paths, so only timing can differ.
