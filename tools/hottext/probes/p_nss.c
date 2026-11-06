/* p_nss: an `ls -l`/`id`-shaped program: repeated user and group lookups, so the name-service
   cache's attach (second lookup), validation and hit paths are traced into the hot-text list
   (NSCACHE-SECOND-LOOKUP). */
#include <grp.h>
#include <pwd.h>
#include <stdio.h>
#include <unistd.h>

int main(void) {
    int found = 0;
    for (int i = 0; i < 3; i++) {
        found += getpwuid(getuid()) != NULL;
        found += getgrgid(getgid()) != NULL;
    }
    printf("%d\n", found);
    return 0;
}
