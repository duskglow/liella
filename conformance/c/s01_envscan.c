/* Spec §1.3: the environment scan reads the live `environ` array on every call (ENV-SCAN): raw
 * slot writes, in-place edits, a replaced array, duplicates (the first entry wins), names that
 * are prefixes of other names, entries without '=', long names, and every position of the target
 * relative to the scan's four-slot passes. getenv and wordexp's IFS use the same scan. */
/* RUN-ENV: DUP=1 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wordexp.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

extern char **environ;

static char *dup_str(const char *s)
{
    char *p = malloc(strlen(s) + 1);
    return p ? strcpy(p, s) : NULL;
}

/* The words of wordexp("$X") joined with '|', or "ERR". */
static const char *words(char *out, size_t n)
{
    wordexp_t w;
    if (wordexp("$X", &w, 0) != 0) {
        snprintf(out, n, "ERR");
        return out;
    }
    out[0] = 0;
    for (size_t i = 0; i < w.we_wordc; i++) {
        strncat(out, i ? "|" : "", n - strlen(out) - 1);
        strncat(out, w.we_wordv[i], n - strlen(out) - 1);
    }
    wordfree(&w);
    return out;
}

static int is(const char *name, const char *want)
{
    const char *v = getenv(name);
    return want ? v && strcmp(v, want) == 0 : v == NULL;
}

int main(void)
{
    char buf[256];

    /* A1-A6 through getenv. */
    static char put[] = "VAR=one";
    char *arr[8] = { dup_str("FILLER=x"), put, NULL };
    environ = arr;
    CHECK("s1_3_c_1", is("VAR", "one"));
    put[4] = 'T';                                   /* A2: in-place edit of a string in the array */
    CHECK("s1_3_c_1", is("VAR", "Tne"));
    arr[1] = dup_str("VAR=two");                    /* A3: a slot overwritten */
    CHECK("s1_3_c_1", is("VAR", "two"));
    char *arr2[4] = { dup_str("VAR=three"), NULL };
    environ = arr2;                                 /* A4: a new array */
    CHECK("s1_3_c_1", is("VAR", "three"));
    char *arr3[4] = { dup_str("OTHER=1"), NULL };
    environ = arr3;                                 /* A4b: a new array without the variable */
    CHECK("s1_3_c_1", is("VAR", NULL));
    arr3[0] = dup_str("VAR=back");                  /* A5: not set, then set by a slot write */
    CHECK("s1_3_c_1", is("VAR", "back"));
    static char renamed[] = "QQQ=zz";
    char *arr4[4] = { renamed, NULL };
    environ = arr4;
    CHECK("s1_3_c_1", is("VAR", NULL));
    memcpy(renamed, "VAR", 3);                      /* A5b: a string renamed in place */
    CHECK("s1_3_c_1", is("VAR", "zz"));
    char *dupa[4] = { dup_str("V=A"), dup_str("V=B"), NULL };
    environ = dupa;                                 /* A6: duplicates, the first wins */
    CHECK("s1_3_c_1", is("V", "A"));
    char *dupb[4] = { dup_str("V=B"), dup_str("V=A"), NULL };
    environ = dupb;
    CHECK("s1_3_c_1", is("V", "B"));
    environ = NULL;                                 /* no array */
    CHECK("s1_3_c_1", is("V", NULL));

    /* The same changes seen through an internal caller: IFS in wordexp. */
    char *iw[4] = { dup_str("X=a:b,c d"), dup_str("IFS=:"), NULL };
    environ = iw;
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a|b,c d") == 0);
    iw[1] = dup_str("IFS=,");                       /* slot overwritten */
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a:b|c d") == 0);
    iw[1] = NULL;                                   /* IFS removed: the default separators */
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a:b,c|d") == 0);
    iw[1] = dup_str("IFS=:");                       /* unset, then set by a slot write */
    iw[2] = NULL;
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a|b,c d") == 0);
    static char ifs[] = "IFS=:";
    iw[1] = ifs;
    ifs[4] = ',';                                   /* in-place edit */
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a:b|c d") == 0);
    char *iw2[4] = { dup_str("X=a:b,c d"), dup_str("IFS=:"), NULL };
    environ = iw2;                                  /* a new array */
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a|b,c d") == 0);
    char *id1[4] = { dup_str("IFS=,"), dup_str("X=a:b,c d"), dup_str("IFS=:"), NULL };
    environ = id1;                                  /* duplicates: the first wins */
    CHECK("s1_3_c_1", strcmp(words(buf, sizeof buf), "a:b|c d") == 0);

    /* Names that are prefixes of other names. */
    char *pf[8] = { dup_str("IFSX=1"), dup_str("I=2"), dup_str("IF=3"), dup_str("IFS=4"), NULL };
    environ = pf;
    CHECK("s1_3_c_1", is("IFS", "4") && is("I", "2") && is("IF", "3") && is("IFSX", "1"));
    CHECK("s1_3_c_1", is("IFSXX", NULL) && is("IFS=", NULL) && is("IFSY", NULL) && is("F", NULL));

    /* Empty values; entries without '='; an empty string entry. */
    char *em[8] = { dup_str("E="), dup_str("NOEQ"), dup_str(""), dup_str("NOEQ2=v"), NULL };
    environ = em;
    CHECK("s1_3_c_1", getenv("E") != NULL && getenv("E")[0] == 0);
    CHECK("s1_3_c_1", is("NOEQ", NULL) && is("NOE", NULL) && is("NOEQ2", "v") && is("E=", NULL));

    /* Very long names: a match, and a long entry that differs only in its last byte. */
    enum { L = 5000 };
    char *name = malloc(L + 1), *entry = malloc(L + 8), *entry2 = malloc(L + 8);
    memset(name, 'N', L);
    name[L] = 0;
    snprintf(entry, L + 8, "%s=ok", name);
    snprintf(entry2, L + 8, "%s", entry);
    entry2[L - 1] = 'M';
    char *lg[4] = { entry2, entry, NULL };
    environ = lg;
    CHECK("s1_3_c_1", is(name, "ok"));
    name[L - 1] = 'M';
    CHECK("s1_3_c_1", is(name, "ok"));              /* entry2 holds this name now (with "=ok") */
    name[L - 1] = 'K';
    CHECK("s1_3_c_1", is(name, NULL));
    name[L - 1] = 'N';
    name[L / 2] = 0;                                /* a shorter name: a prefix of the entries */
    CHECK("s1_3_c_1", is(name, NULL));

    /* The target at every position of arrays of 1..13 entries, among fillers that share the
     * target's first byte or do not; and absent. */
    for (int share = 0; share < 2; share++) {
        for (int n = 1; n <= 13; n++) {
            for (int pos = -1; pos < n; pos++) {
                char *a[16];
                for (int i = 0; i < n; i++) {
                    char tmp[32];
                    snprintf(tmp, sizeof tmp, share ? "TZX%d=f%d" : "F%d=f%d", i, i);
                    a[i] = dup_str(tmp);
                }
                if (pos >= 0)
                    a[pos] = dup_str("TZ=hit");
                a[n] = NULL;
                environ = a;
                CHECK("s1_3_c_1", is("TZ", pos >= 0 ? "hit" : NULL));
                for (int i = 0; i < n; i++)
                    if (i != pos)
                        free(a[i]);
                if (pos >= 0)
                    free(a[pos]);
            }
        }
    }
    return 0;
}
