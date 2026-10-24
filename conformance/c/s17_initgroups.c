/* Spec §17.2 `initgroups(user, g)`: the supplementary list becomes the groups the group database
 * lists `user` in, plus `g` (phase 10; the group database is the `files` source, D-15.6). Run as
 * root in a user namespace with several IDs mapped. The user is the session's login user when
 * the user database knows it, else root; the expected list comes from the group enumeration. */
/* The name-service cache is off (LIELLA_NSCACHE=0, D-16.10): the system's /dev/shm is shared, and a
 * conformance run must not leave a region there. */
/* RUN-WRAP: unshare -U --map-auto --map-root-user env LIELLA_NSCACHE=0 */
#define _GNU_SOURCE
#include <grp.h>
#include <pwd.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

enum { MAX = 256 };

static int has(const gid_t *list, int n, gid_t g)
{
    for (int i = 0; i < n; i++)
        if (list[i] == g)
            return 1;
    return 0;
}

int main(void)
{
    if (getuid() != 0)
        return 1;
    unsigned login = 4294967295u;
    FILE *f = fopen("/proc/self/loginuid", "r");
    if (f) {
        if (fscanf(f, "%u", &login) != 1)
            login = 4294967295u;
        fclose(f);
    }
    char name[256] = "root";
    struct passwd *pw = login != 4294967295u ? getpwuid(login) : NULL;
    if (pw)
        snprintf(name, sizeof name, "%s", pw->pw_name);
    gid_t want[MAX];
    int n = 0;
    want[n++] = 4242;
    setgrent();
    for (struct group *g; (g = getgrent()) != NULL;) {
        if (g->gr_name[0] == '+' || g->gr_name[0] == '-')
            continue;
        for (char **m = g->gr_mem; *m; m++)
            if (strcmp(*m, name) == 0 && !has(want, n, g->gr_gid) && n < MAX)
                want[n++] = g->gr_gid;
    }
    endgrent();
    if (initgroups(name, 4242) != 0)
        return 2;
    gid_t got[MAX];
    int k = getgroups(MAX, got);
    if (k != n)
        return 3;
    for (int i = 0; i < n; i++)
        if (!has(got, k, want[i]))
            return 4;
    /* An unknown user: the base group alone. */
    if (initgroups("no-such-user-here", 77) != 0)
        return 5;
    if (getgroups(MAX, got) != 1 || got[0] != 77)
        return 6;
    return 0;
}
