/* `__fpu_control` (spec §10.6, D-10.5): the x87 control word a dynamically linked program starts
 * with, 0x037f (all exceptions masked, 64-bit precision, round to nearest).
 *
 * An archive member of its own, holding nothing else: a static program that defines its own
 * `__fpu_control` then links (its definition satisfies the start-up reader in `runtime.c`, so
 * this member stays out). A dynamic program's definition interposes on this
 * one, and the start-up hook (`runtime.rs`, `start()`) applies it; a static program's is ignored,
 * as glibc ignores it (probe-settled, #812).
 *
 * Defined only in the shipped products (LIELLA_RUNTIME): in test builds the host C library owns
 * the name. */
#include "liella_csys.h"

#ifdef LIELLA_RUNTIME
unsigned short __fpu_control = 0x037f;
#endif
