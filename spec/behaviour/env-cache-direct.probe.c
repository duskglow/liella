/* env-cache-direct: does gettext see LANGUAGE changed by a direct store into `environ` (no library
 * call)? Black-box probe, public interfaces only (GETTEXT-ENV-SCAN, ruling #504).
 *
 * Build: cc -std=gnu17 -O1 env-cache-direct.probe.c -o probe
 * Run:   LOCDIR=<dir with de/ and fr/ LC_MESSAGES/envp.mo> LANG=en_US.utf8 ./probe
 * Catalogs (domain envp): hello -> hallo / bonjour, bye -> tschuess / au revoir.
 *
 * Each trial runs in a forked child: setlocale(LC_ALL, ""), bind envp to LOCDIR, set ZZ=filler
 * (so ZZ's slot comes before LANGUAGE's), then the start state (LANGUAGE unset, or LANGUAGE=de by
 * setenv or putenv), one priming gettext("hello"), the change, and one gettext of the query.
 */
#define _GNU_SOURCE
#include <libintl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static int slot(const char *name) {
    size_t n = strlen(name);
    for (int i = 0; environ[i]; i++)
        if (!strncmp(environ[i], name, n) && environ[i][n] == '=')
            return i;
    return -1;
}

enum { UNSET, SET_DE, PUT_DE };
static const char *startname[] = {"LANGUAGE unset", "LANGUAGE=de (setenv)", "LANGUAGE=de (putenv)"};

static const struct { const char *name; int start; } changes[] = {
    {"none (control)", SET_DE},
    {"setenv LANGUAGE=fr (control)", SET_DE},
    {"a: ZZ's slot = \"LANGUAGE=fr\"", UNSET},
    {"b: LANGUAGE's slot = \"LANGUAGE=fr\"", SET_DE},
    {"c: ZZ's slot (before LANGUAGE's) = \"LANGUAGE=fr\"", SET_DE},
    {"d: putenv'd \"QQQQQQQQ=fr\" renamed in place to LANGUAGE", UNSET},
    {"e: putenv'd \"LANGUAGE=de\" value edited in place to fr", PUT_DE},
    {"f: LANGUAGE's slot = NULL (array truncated there)", SET_DE},
};

static char fr_entry[] = "LANGUAGE=fr";
static char q_entry[] = "QQQQQQQQ=fr";
static char de_entry[] = "LANGUAGE=de";

static void child(int c, const char *query, char *out, size_t n) {
    setlocale(LC_ALL, "");
    bindtextdomain("envp", getenv("LOCDIR"));
    textdomain("envp");
    setenv("ZZ", "filler", 1);
    unsetenv("LANGUAGE");
    if (changes[c].start == SET_DE)
        setenv("LANGUAGE", "de", 1);
    if (changes[c].start == PUT_DE)
        putenv(de_entry);
    if (c == 5)
        putenv(q_entry);
    char primed[64];
    snprintf(primed, sizeof primed, "%s", gettext("hello"));
    switch (c) {
    case 1: setenv("LANGUAGE", "fr", 1); break;
    case 2: environ[slot("ZZ")] = fr_entry; break;
    case 3: environ[slot("LANGUAGE")] = fr_entry; break;
    case 4: environ[slot("ZZ")] = fr_entry; break;
    case 5: memcpy(q_entry, "LANGUAGE", 8); break;
    case 6: memcpy(de_entry + 9, "fr", 2); break;
    case 7: environ[slot("LANGUAGE")] = NULL; break;
    }
    snprintf(out, n, "primed [%s] -> %s(%s) [%s]", primed, "gettext", query, gettext(query));
}

int main(void) {
    static const char *queries[] = {"hello", "bye"};
    for (size_t c = 0; c < sizeof changes / sizeof changes[0]; c++)
        for (int q = 0; q < 2; q++) {
            int fd[2];
            if (pipe(fd))
                return 2;
            fflush(NULL);
            pid_t p = fork();
            if (p == 0) {
                char line[300];
                child((int)c, queries[q], line, sizeof line);
                if (write(fd[1], line, strlen(line)) < 0)
                    _exit(3);
                _exit(0);
            }
            close(fd[1]);
            char b[300] = "";
            ssize_t k = read(fd[0], b, sizeof b - 1);
            b[k < 0 ? 0 : k] = 0;
            close(fd[0]);
            waitpid(p, NULL, 0);
            printf("%-22s | %-55s | %s\n", startname[changes[c].start], changes[c].name, b);
        }
    return 0;
}
