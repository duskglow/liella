/* Spec §27.1 (dlsym, RTLD_DEFAULT): a symbol exported by a shared object the program links
 * against is found by name in the global scope and calls through the returned address. Proves
 * the runner's dynamic-link mode (LINK-HELPERS); runs on the reference only until Liella has a
 * loader. Each failing check exits with its line number. */
/* LINK-HELPERS: dlh_add */
#define _GNU_SOURCE
#include <dlfcn.h>

int main(void)
{
    typedef int add_fn(int, int);
    void *p = dlsym(RTLD_DEFAULT, "dlh_add");
    if (p == 0) return __LINE__;
    add_fn *f = (add_fn *)p;
    if (f(2, 3) != 1005) return __LINE__;
    if (dlsym(RTLD_DEFAULT, "dlh_no_such_symbol") != 0) return __LINE__;
    return 0;
}
