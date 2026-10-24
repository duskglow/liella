/* dlsecure probe: what a (possibly set-group-ID) dynamic program sees at start.
 * Black-box probe of the reference loader (DL-PROBE-SECURE, plan DL-0a). Public interfaces only.
 *   probe env           AT_SECURE, ids, the loaded objects, the environment (sorted by the run script)
 *   probe tz            localtime of a fixed instant under the inherited TZ
 *   probe dlopen NAME   dlopen(NAME, RTLD_NOW): ok, or the dlerror text
 * Built with -DUSE_SGO it also calls sgo_value() from libsgo.so (a DT_NEEDED test). */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <time.h>
#include <unistd.h>

extern char **environ;
#ifdef USE_SGO
int sgo_value(void);
#endif

static int show_object(struct dl_phdr_info *info, size_t size, void *data)
{
    (void)size;
    (void)data;
    const char *name = info->dlpi_name;
    if (name == NULL || name[0] == '\0')
        return 0; /* the program itself */
    const char *base = strrchr(name, '/');
    base = base ? base + 1 : name;
    if (strncmp(base, "linux-vdso", 10) == 0 || strncmp(base, "ld-linux", 8) == 0 ||
        strncmp(base, "libc.so", 7) == 0)
        return 0; /* always present */
    printf("object %s\n", base);
    return 0;
}

int main(int argc, char **argv)
{
    const char *mode = argc > 1 ? argv[1] : "env";
    printf("secure %lu rgid_is_egid %d ruid_is_euid %d\n", getauxval(AT_SECURE),
           getgid() == getegid(), getuid() == geteuid());
#ifdef USE_SGO
    printf("sgo_value %d\n", sgo_value());
#endif
    if (strcmp(mode, "env") == 0) {
        dl_iterate_phdr(show_object, NULL);
        for (char **e = environ; *e != NULL; e++)
            printf("env %s\n", *e);
    } else if (strcmp(mode, "tz") == 0) {
        time_t t = 15552000; /* 1970-06-29, inside any northern summer rule */
        struct tm tm;
        localtime_r(&t, &tm);
        printf("tz gmtoff %ld zone %s\n", (long)tm.tm_gmtoff, tm.tm_zone ? tm.tm_zone : "(null)");
    } else if (strcmp(mode, "dlopen") == 0 && argc > 2) {
        void *h = dlopen(argv[2], RTLD_NOW);
        if (h != NULL) {
            printf("dlopen ok\n");
            dl_iterate_phdr(show_object, NULL);
        } else {
            const char *err = dlerror();
            printf("dlopen fail %s\n", err ? err : "(null)");
        }
    }
    fflush(stdout);
    return 0;
}
