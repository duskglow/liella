/* Spec §3.3 mtrace in secure-execution mode (spec/security/threat-model.md): MALLOC_TRACE is
 * read through secure_getenv, so a set-ID program never creates or appends to a file its
 * caller's environment names, even when the variable is set after start; MALLOC_PERTURB_ is
 * removed at start. */
/* RUN-SECURE: yes */
/* RUN-ENV: MALLOC_TRACE=/nonexistent-liella/trace MALLOC_PERTURB_=165 */
#define _GNU_SOURCE
#include <mcheck.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#define CHECK(clause, cond) do { if (!(cond)) { unlink(path); return __LINE__; } } while (0)

static char path[512];

int main(void)
{
    snprintf(path, sizeof path, "/tmp/liella-mtrace-secure-%d", (int)getpid());
    CHECK("s3_3_c_3", getenv("MALLOC_TRACE") == NULL && getenv("MALLOC_PERTURB_") == NULL);
    setenv("MALLOC_TRACE", path, 1);
    mtrace();
    free(malloc(10));
    muntrace();
    CHECK("s3_3_c_3", access(path, F_OK) != 0);
    return 0;
}
