/* dlsecure test library: built as libsgo.so (DT_NEEDED of the USE_SGO probes), libpre.so
 * (LD_PRELOAD and /etc/ld.so.preload cases) and libsgc.so.1 (found only through a private cache).
 * The constructor writes a marker to stderr so the record shows whether the library was loaded. */
#include <string.h>
#include <unistd.h>

#ifndef LIBNAME
#define LIBNAME "lib"
#endif

__attribute__((constructor)) static void marker(void)
{
    static const char msg[] = "ctor " LIBNAME "\n";
    (void)!write(2, msg, sizeof msg - 1);
}

int sgo_value(void)
{
    return 42;
}
