/* validation/smoke/selftest/plugin.c: a shared object built by liella-cc -shared (smoke.sh selftest); the
   check requires its DT_NEEDED to name Liella's C library by path, never the system's by soname. */
#include <string.h>

size_t plugin_len(const char *s) { return strlen(s); }
