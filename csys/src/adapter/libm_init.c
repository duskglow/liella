/* Class adapter (OD-C1): the libm product's load-time constructor (D-10.50, #2920). libm.a and
 * libm.so.6 carry their own copy of the dispatch byte that chooses the FMA builds of the double
 * exp/log fast paths; the C library's startup never writes that copy, so this `.init_array` entry
 * has `liella_libm_start` (crates/boundary/src/dispatch.rs) decide it once, before the program's
 * code calls a math function, and no math call asks whether it has been decided. The loader (and
 * a static program's startup, `run_array`) calls it with `(argc, argv, envp)`. Linked only into
 * the libm product (feature `libm`, build.rs); the name is local in both libraries. */
#include "liella_csys.h"

#if defined(LIELLA_LIBM)
void liella_libm_start(int argc, char **argv, char **envp);

__attribute__((used, section(".init_array")))
static void (*const liella_libm_init_entry)(int, char **, char **) = liella_libm_start;
#endif
