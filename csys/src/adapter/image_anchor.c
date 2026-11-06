/*
 * Class adapter: the start-up anchor of a static program's own image (D-27.10).
 *
 * The static-PIE start reads this address before the image is relocated, so the reference must
 * be PC-relative: `__ehdr_start` is declared hidden and NOT weak, and the compiler then takes its
 * address with one RIP-relative instruction instead of loading it from a GOT slot (which, in a
 * static PIE, holds an unrelocated link-time value until the relocator has run). The linker
 * defines `__ehdr_start` in every executable and shared object whose ELF header lies in a loaded
 * segment (the default layouts). This file holds nothing else: `cargo xtask so` checks that the
 * member defining `liella_link_image_header` has no GOT-indirect relocation at all.
 */
#include <stdint.h>

#ifdef LIELLA_RUNTIME
extern const char __ehdr_start[] __attribute__((visibility("hidden")));

/* The run-time address of the ELF header of the image this object is linked into. */
uintptr_t liella_link_image_header(void) { return (uintptr_t)__ehdr_start; }
#endif
