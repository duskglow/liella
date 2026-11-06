/* The bodies of libc_malloc_debug.so.0's own exports (D-27.9; design plan/ph14/design.md §16).
 *
 * Which names the library exports, at which versions, comes from spec/abi/versions.tsv
 * (xtask/src/placeholders.rs): every name libc.so.6 also has is reached through DT_NEEDED
 * libc.so.6, so only malloc_get_state and malloc_set_state, which libc.so.6 does not export,
 * need a body here. They fail as §3.3 and D-3.5 say ("compat stubs ... They now fail"), with the
 * same results as libc.a's definitions: get returns NULL, set returns -1, both with errno ENOSYS.
 * Preloading this library gives no hooks (libc.so.6 provides mcheck/mtrace itself, D-3.5).
 * Compiled by `cargo xtask so` only, never part of libc.a. */
#include <errno.h>
#include <stddef.h>

void *liella_ph_malloc_get_state(void);
int liella_ph_malloc_set_state(void *state);

void *liella_ph_malloc_get_state(void) {
    errno = ENOSYS;
    return NULL;
}

int liella_ph_malloc_set_state(void *state) {
    (void)state;
    errno = ENOSYS;
    return -1;
}
