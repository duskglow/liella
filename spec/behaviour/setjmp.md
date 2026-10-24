# setjmp and the signal mask

**Spec:** §21.1 (`setjmp`, `_setjmp`, `__sigsetjmp`, `sigsetjmp`, `longjmp`, `_longjmp`,
`siglongjmp`, `__longjmp_chk`; clauses `s21_1_c_1`, `s21_1_c_2`, `s21_1_c_3`, `s21_1_c_4`).
Plan decision D-21.4. Public interface: `<setjmp.h>`. Official documentation consulted:
setjmp(3) and longjmp(3) (Linux man-pages, as installed), POSIX.1-2017 `_longjmp`/`_setjmp`
and `sigsetjmp`/`siglongjmp`.

**Probe:** `crates/core/tests/oracle/setjmp_diff.c`. **Record:** `setjmp.glibc.txt` (the probe
built dynamically and with `-static`, both against glibc 2.39-0ubuntu8.9, kernel 6.8.0-146, on
Zen 1).

## What the probe does

Each line pairs one way of setting a jump buffer with one jump function. Before the set call
the signal mask is exactly {`SIGUSR2`}; between the set call and the jump the probe sets it to
exactly {`SIGUSR1`}. After the jump, `usr1` and `usr2` say whether each signal is blocked:

* `usr1=0 usr2=1`: the set call saved the mask and the jump restored it;
* `usr1=1 usr2=0`: the mask was left as the jump found it.

`ret` is the value the set call returned the second time (the probe jumps with 1).

The set forms: `setjmp(macro)` is `setjmp(env)` written in C with `<setjmp.h>` included;
`setjmp(pointer)` calls through the address of the function `setjmp` (no macro applies);
`setjmp(symbol)` calls the symbol `setjmp` directly through a declaration that bypasses the
header (`__asm__("setjmp")`, `returns_twice`); `_setjmp`; `sigsetjmp(0)` and `sigsetjmp(1)`
through the header's macro. The jumps are called by their symbols (`longjmp`, `_longjmp`,
`siglongjmp`, `__longjmp_chk`), so the result does not depend on whether the build is
fortified (with `_FORTIFY_SOURCE`, which Ubuntu's compiler enables at `-O1`, the header
redirects all three of `longjmp`, `_longjmp` and `siglongjmp` to `__longjmp_chk`).

## Observations

Dynamic and static linking give the same lines.

| Set form | Saves the mask | Then restored by |
|---|---|---|
| `setjmp(env)` through `<setjmp.h>` | no | (nothing to restore) |
| `setjmp` symbol, through a pointer | **yes** | `longjmp`, `_longjmp`, `siglongjmp`, `__longjmp_chk` |
| `setjmp` symbol, called directly | **yes** | all four |
| `_setjmp` | no | (nothing to restore) |
| `sigsetjmp(env, 0)` | no | (nothing to restore) |
| `sigsetjmp(env, 1)` | yes | all four, **`_longjmp` included** |

* The header's `setjmp(env)` does not save the mask (the installed `<setjmp.h>` defines the
  macro `setjmp(env)` as `_setjmp(env)`; setjmp(3): "Since glibc 2.19, `<setjmp.h>` exposes
  only the System V version of setjmp()").
* The function `setjmp` itself saves the mask (the BSD behaviour). A C program reaches it only
  by taking its address or by bypassing the header.
* Every jump function restores the mask exactly when the buffer saved one: `_longjmp` behaves
  as `longjmp` (the installed header's comment calls it an alias). With a buffer that saved
  nothing, no jump changes the mask.
* Each pairing returned 1 the second time.

## Against the spec and the documents

* §21.1 (`s21_1_c_3`): "`_setjmp`/`_longjmp` never save or restore the mask. `setjmp` on glibc
  does not save the mask either." The macro form agrees; the symbol does not (D-21.4, owner
  ruling 2026-10-01: the spec is followed). The `_longjmp` half disagrees with the reference
  only for a buffer `sigsetjmp(env, 1)` filled. POSIX says `_longjmp` "shall not manipulate the
  signal mask", and calls a `_longjmp` on a buffer that `_setjmp` did not fill undefined, so the
  documents do not settle that pairing.
