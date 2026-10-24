/* Class adapter (proposed, OD-C1): va_arg accessors, one per C type class, each a single
 * `va_arg` of the next argument of a va_list Rust holds. Kept apart from variadic.c, whose
 * entry points name the products' exports: a test executable that reads a va_list (the
 * formatting engines) links these without the variadic entry points. */
#include <stdarg.h>
#include "liella_csys.h"

/* The next argument as a register-sized value. */
long liella_va_long(void *ap) { return va_arg(*(va_list *)ap, long); }

/* The formatted-I/O families' floating arguments (phase 8): a double, and a long double stored
 * at `out` (16 bytes, 16-aligned). */
double liella_va_double(void *ap) { return va_arg(*(va_list *)ap, double); }
void liella_va_ldouble(void *ap, long double *out) { *out = va_arg(*(va_list *)ap, long double); }
