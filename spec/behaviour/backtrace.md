# backtrace

**Spec:** §2.7 (`backtrace`, `backtrace_symbols`, `backtrace_symbols_fd`, and the `__`-prefixed
names). Public interface: `<execinfo.h>`. Normative references for the mechanism (plan §1.2): the
x86-64 psABI (stack frames and DWARF call-frame information) and DWARF (the `.eh_frame` call-frame
instructions); the `PT_GNU_EH_FRAME` program header and its `.eh_frame_hdr` search table.

**Clauses:** `s2_7_c_1`, `s2_7_c_2`, `s2_7_c_3`, `s2_7_f_1`, `s2_7_f_2`, `s2_7_f_3`.

**Probe:** `crates/core/tests/oracle/backtrace_diff.c`. **Record:** `backtrace.glibc.txt`.

## What the probe does

The probe's own functions are placed in sections named `.text.sorted.liella_probe_NN`, which the
default linker script lays out in name order, so each function's address range ends where the next
one starts (`layout_ok=1` checks it). Each return address in a trace is shown as the name of the
probe function containing it; a run of addresses outside them (the library's own frames, the
signal trampoline, start-up code) is shown as one `~`. A trace is shown up to the probe's `driver`
function; `after_driver` says whether frames follow it.

* `nested`: `driver` calls `f1`, `f2`, `f3`, `leaf`; `leaf` calls `backtrace` with room for 64.
* `limit N`: the same with room for 0, 1, 2, 3 and 5 (the count is shown, since the limit falls
  inside the probe's own frames).
* `signal handler`: `driver` calls `g1`, `g2`, `g3`; `g3` raises `SIGUSR1`; the handler
  `on_signal` calls `leaf`. The trace must pass through the signal trampoline to `g3` and on.
* `symbols`: `backtrace_symbols` of a 4-frame trace, each text with every hexadecimal number shown
  as `H` and the program's name as `P`; `one block` checks that the texts lie inside the same
  allocation as the pointer array (§2.7 HIST); `symbols n=0` whether a 0-frame call returns
  non-NULL.
* `symbols_fd`: `backtrace_symbols_fd` of the same trace, written to a pipe, normalised the same
  way.

## Observations the record shows directly

* The reference's output (a dynamically linked position-independent program) has the form
  `./P(+H) [H]` from `backtrace_symbols` and `./P(+H)[H]` (no space) from `backtrace_symbols_fd`
  for these functions, which have no dynamic symbols. The differential test links the Liella
  build statically, so the object name and offset form of those lines may differ legitimately.
