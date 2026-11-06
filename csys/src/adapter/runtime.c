/* Class adapter, runtime (phase 4; spec §1.1, §1.3, §1.4): what the process runtime needs from C.
 *
 * 1. The startup data objects C programs name. Several names share one object (`environ`,
 *    `__environ` and `_environ`; `program_invocation_name` and `__progname_full`;
 *    `program_invocation_short_name` and `__progname`), which Rust statics cannot express.
 *    Defined only in the shipped products (LIELLA_RUNTIME): in test builds the host C library
 *    owns them, and the Rust side reaches the host's objects under the same names (D-2.1).
 *    argp's program-wide variables (§1.7, D-1.7) are defined here too, weakly.
 * 2. The link-time symbols of a static executable's constructor and destructor lists, through
 *    weak references, so that every link resolves them (to 0 when the linker provides none).
 * 3. Where `backtrace` (§2.7, D-2.7) finds call-frame information: the ELF header of the image
 *    this library is linked into (`__ehdr_start`, which the linker defines), and the bounds of a
 *    static executable's .eh_frame. The start label is in the start object (machine/entry.S);
 *    the end is the terminator defined here, in a section named `.eh_frame.*`, which the
 *    linker's default script places after every `.eh_frame` input section. A zero length word
 *    ends a list of call-frame records (the LSB's .eh_frame).
 *
 * Each function is one statement returning an address; all logic is in Rust
 * (crates/boundary/src/runtime.rs, crates/boundary/src/backtrace.rs). */
#include <stddef.h>
#include <stdint.h>
#include "liella_csys.h"

#ifdef LIELLA_RUNTIME
char **environ = 0;
extern char **__environ __attribute__((alias("environ")));
extern char **_environ __attribute__((alias("environ")));
char *program_invocation_name = 0;
extern char *__progname_full __attribute__((alias("program_invocation_name")));
char *program_invocation_short_name = 0;
extern char *__progname __attribute__((alias("program_invocation_short_name")));
void *__libc_stack_end = 0;

/* The time-zone globals (spec §23.6): `tzname`, `timezone` and `daylight` are weak aliases of
 * the `__` objects, as in glibc. Before the first `tzset`, both names are "GMT". */
char *__tzname[2] = { (char *)"GMT", (char *)"GMT" };
extern char *tzname[2] __attribute__((weak, alias("__tzname")));
long __timezone = 0;
extern long timezone __attribute__((weak, alias("__timezone")));
int __daylight = 0;
extern int daylight __attribute__((weak, alias("__daylight")));

/* argp's program-wide variables (spec §1.7). Programs commonly define `argp_program_version`,
 * `argp_program_bug_address` and `argp_program_version_hook` themselves. They are GLOBAL in
 * `libc.so.6`, as in glibc (a program's definition interposes on them); `libc.a` makes them weak
 * in place (D-X.14), so a program's own definition takes their place in a static link. */
const char *argp_program_version = 0;
void (*argp_program_version_hook)(void *, void *) = 0;
const char *argp_program_bug_address = 0;
int argp_err_exit_status = 64;

/* Rust's precompiled `core` refers to its unwinding personality routine from landing pads. The
 * products are built with panic=abort, so no landing pad ever runs and the routine is never
 * called; it only has to exist for the link. Weak, so that a Rust program linking its own (std's)
 * takes precedence. */
void rust_eh_personality(void);
__attribute__((weak)) void rust_eh_personality(void) { return; }
#endif

extern void (*__preinit_array_start[])(void) __attribute__((weak));
extern void (*__preinit_array_end[])(void) __attribute__((weak));
extern void (*__init_array_start[])(void) __attribute__((weak));
extern void (*__init_array_end[])(void) __attribute__((weak));
extern void (*__fini_array_start[])(void) __attribute__((weak));
extern void (*__fini_array_end[])(void) __attribute__((weak));
extern void _init(void) __attribute__((weak));
extern void _fini(void) __attribute__((weak));

uintptr_t liella_link_preinit_start(void) { return (uintptr_t)__preinit_array_start; }
uintptr_t liella_link_preinit_end(void) { return (uintptr_t)__preinit_array_end; }
uintptr_t liella_link_init_start(void) { return (uintptr_t)__init_array_start; }
uintptr_t liella_link_init_end(void) { return (uintptr_t)__init_array_end; }
uintptr_t liella_link_fini_start(void) { return (uintptr_t)__fini_array_start; }
uintptr_t liella_link_fini_end(void) { return (uintptr_t)__fini_array_end; }
uintptr_t liella_link_init(void) { return (uintptr_t)_init; }
uintptr_t liella_link_fini(void) { return (uintptr_t)_fini; }

/* A static executable's IRELATIVE entries (`.rela.iplt`): the linker defines these two symbols
 * in a static link only, so elsewhere both are 0 and the range is empty. */
extern const char __rela_iplt_start[] __attribute__((weak, visibility("hidden")));
extern const char __rela_iplt_end[] __attribute__((weak, visibility("hidden")));
uintptr_t liella_link_rela_iplt_start(void) { return (uintptr_t)__rela_iplt_start; }
uintptr_t liella_link_rela_iplt_end(void) { return (uintptr_t)__rela_iplt_end; }

/* The x87 control word the program asks for in `__fpu_control` (D-10.5; the definition is
 * `fpu_control.c`). In `libc.so.6` the read goes through the GOT, so it sees a program's
 * interposing definition. In a static link a program's own definition satisfies the reference
 * (the member is then not pulled in, and no collision arises); start-up reads it only in a
 * dynamic program. */
extern unsigned short __fpu_control;
uint16_t liella_link_fpu_control(void) { return __fpu_control; }

/* Caller-owned memory (D-X.14): the library allocates what it hands to C callers, and frees
 * what they hand back, through the symbols `malloc`, `calloc`, `realloc` and `free`, which a
 * program may replace ("Replacing malloc" in the GNU C Library manual). Calling them from here
 * keeps each call a call to the symbol: inside the library's own optimised code it could be bound
 * to, or merged with, the library's definition. */
void *malloc(size_t n);
void *calloc(size_t k, size_t n);
void *realloc(void *p, size_t n);
void free(void *p);
void *liella_caller_malloc(size_t n) { return malloc(n); }
void *liella_caller_calloc(size_t k, size_t n) { return calloc(k, n); }
void *liella_caller_realloc(void *p, size_t n) { return realloc(p, n); }
void liella_caller_free(void *p) { free(p); }

extern const char __ehdr_start[] __attribute__((weak, visibility("hidden")));
extern const char liella_eh_frame_start[] __attribute__((weak, visibility("hidden")));
static const uint32_t liella_eh_frame_end __attribute__((section(".eh_frame.liella_end"), aligned(4))) = 0;

uintptr_t liella_link_ehdr(void) { return (uintptr_t)__ehdr_start; }
uintptr_t liella_link_eh_frame_start(void) { return (uintptr_t)liella_eh_frame_start; }
uintptr_t liella_link_eh_frame_end(void) { return (uintptr_t)&liella_eh_frame_end; }
