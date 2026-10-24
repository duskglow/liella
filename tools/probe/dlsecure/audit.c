/* dlsecure audit module (rtld-audit(7)): writes a marker when the loader accepts it. */
#define _GNU_SOURCE
#include <link.h>
#include <unistd.h>

unsigned int la_version(unsigned int version)
{
    static const char msg[] = "audit la_version\n";
    (void)!write(2, msg, sizeof msg - 1);
    return version < LAV_CURRENT ? version : LAV_CURRENT;
}
