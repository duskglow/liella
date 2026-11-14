/* validation/smoke/repro/readdir_removed.c: AQOURS-SMOKE finding 2 (spec section 13, readdir), from
   coreutils' test tests/ls/removed-directory.sh ("ls" in a directory that was removed under it).
   Opens a fresh directory, removes it, then reads it: prints readdir's result and errno.

   Build and run as validation/smoke/repro/regcomp_nesting.c says (gcc for the reference, liella-cc for
   Liella); run it in a writable directory. */
#include <dirent.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
    char name[64];
    snprintf(name, sizeof name, "readdir-removed.%ld", (long)getpid());
    if (mkdir(name, 0700) != 0)
        return 2;
    DIR *d = opendir(name);
    if (!d) {
        rmdir(name);
        return 2;
    }
    if (rmdir(name) != 0)
        return 2;
    errno = 0;
    struct dirent *e = readdir(d);
    int err = errno;
    printf("readdir %s errno %d (%s)\n", e ? "entry" : "NULL", err, err ? strerror(err) : "none");
    closedir(d);
    return 0;
}
