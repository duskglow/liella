/* Spec §1.3: setenv, unsetenv and putenv find an entry by its name and the '=' after it
 * (ENVIRON-FIND-PREFIX): names that are prefixes of other names, entries without '=', empty
 * entries, an entry starting with '=', duplicates, a very long entry, and a putenv string the
 * caller changes afterwards. The array is the program's own, installed through `environ`. */
/* RUN-ENV: KEEP=k */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

extern char **environ;

static int count(const char *prefix)
{
    int n = 0;
    size_t len = strlen(prefix);
    for (char **e = environ; e && *e; e++)
        n += strncmp(*e, prefix, len) == 0;
    return n;
}

static char long_entry[8192];

int main(void)
{
    static char e_ab[] = "AB=prefix", e_abc[] = "ABC", e_abcd[] = "ABCD=1", e_eq[] = "=x", e_empty[] = "";
    static char e_dup1[] = "DUP=1", e_dup2[] = "DUP=2", e_q[] = "Q=1";
    memset(long_entry, 'z', sizeof long_entry - 1);
    memcpy(long_entry, "LONG=", 5);
    static char *mine[16];
    char *init[] = { e_ab, e_abc, e_abcd, e_eq, e_empty, e_dup1, e_dup2, e_q, long_entry, NULL };
    memcpy(mine, init, sizeof init);
    environ = mine;

    CHECK("s1_3_c_1", strcmp(getenv("AB"), "prefix") == 0 && getenv("ABC") == NULL && getenv("A") == NULL);
    CHECK("s1_3_c_1", strlen(getenv("LONG")) == sizeof long_entry - 6);
    /* "ABC" without '=' is not an entry for ABC: setenv with overwrite 0 still adds one. */
    CHECK("s1_3_c_3", setenv("ABC", "v", 0) == 0 && strcmp(getenv("ABC"), "v") == 0);
    CHECK("s1_3_c_3", count("ABC") == 3 && strcmp(getenv("ABCD"), "1") == 0 && strcmp(getenv("AB"), "prefix") == 0);
    /* Duplicates: setenv replaces the value getenv sees. */
    CHECK("s1_3_f_3", setenv("DUP", "3", 1) == 0 && strcmp(getenv("DUP"), "3") == 0);
    CHECK("s1_3_c_3", setenv("LONG", "short", 0) == 0 && strlen(getenv("LONG")) == sizeof long_entry - 6);
    CHECK("s1_3_c_3", setenv("LONG", "short", 1) == 0 && strcmp(getenv("LONG"), "short") == 0);
    /* A putenv string changed afterwards is found by its current name. */
    static char p[] = "P=x";
    CHECK("s1_3_c_5", putenv(p) == 0 && strcmp(getenv("P"), "x") == 0);
    p[0] = 'Q';
    CHECK("s1_3_c_5", getenv("P") == NULL && strcmp(getenv("Q"), "1") == 0);
    CHECK("s1_3_c_3", setenv("P", "new", 0) == 0 && strcmp(getenv("P"), "new") == 0);
    CHECK("s1_3_c_5", count("Q=") == 2);
    static char q2[] = "Q=2";
    CHECK("s1_3_c_5", putenv(q2) == 0 && strcmp(getenv("Q"), "2") == 0);
    CHECK("s1_3_c_5", putenv((char *)"Q") == 0 && getenv("Q") == NULL && count("Q=") == 0);
    /* unsetenv removes AB only: the longer names, the entry without '=' and the empty and
     * '='-first entries stay. */
    CHECK("s1_3_c_4", unsetenv("AB") == 0 && getenv("AB") == NULL && count("AB=") == 0);
    CHECK("s1_3_c_4", count("ABC") == 3 && count("=x") == 1 && strcmp(getenv("ABCD"), "1") == 0);
    CHECK("s1_3_c_4", unsetenv("DUP") == 0 && count("DUP") == 0);
    errno = 0;
    CHECK("s1_3_i_1", setenv("", "x", 1) == -1 && errno == EINVAL);
    errno = 0;
    CHECK("s1_3_i_1", unsetenv("A=B") == -1 && errno == EINVAL);
    CHECK("s1_3_c_4", count("=x") == 1);

    /* ENVIRON-PUTENV-SLOPE: the slot a replacement lands in. A large array (2,000 entries, like
     * a big real environment) whose last entry is replaced in place; names that are prefixes of
     * it; one string stored in two slots (only the first slot is replaced); duplicates by name. */
    static char *big[2010];
    static char names[2000][16];
    for (int i = 0; i < 2000; i++) {
        snprintf(names[i], sizeof names[i], "V%d=x", i);
        big[i] = names[i];
    }
    static char same[] = "SAME=old";
    big[3] = same;
    big[7] = same;
    big[2000] = NULL;
    environ = big;
    static char last[] = "V1999=new", pre[] = "V199=p", same_new[] = "SAME=new";
    CHECK("s1_3_c_5", putenv(last) == 0 && environ == big && big[1999] == last && big[2000] == NULL);
    CHECK("s1_3_c_5", strcmp(getenv("V1999"), "new") == 0 && strcmp(getenv("V199"), "x") == 0);
    CHECK("s1_3_c_5", putenv(pre) == 0 && big[199] == pre && big[1999] == last && strcmp(getenv("V19"), "x") == 0);
    CHECK("s1_3_c_5", putenv(same_new) == 0 && big[3] == same_new && big[7] == same && strcmp(getenv("SAME"), "new") == 0);
    CHECK("s1_3_c_3", setenv("SAME", "s", 1) == 0 && big[7] == same && strcmp(big[3], "SAME=s") == 0);
    CHECK("s1_3_c_3", setenv("V1998", "y", 1) == 0 && environ == big && strcmp(big[1998], "V1998=y") == 0);
    CHECK("s1_3_c_3", setenv("V1998", "z", 0) == 0 && strcmp(big[1998], "V1998=y") == 0);
    /* A name in no slot: added after the last entry; the old ones keep their slots. */
    static char fresh[] = "V2000=f";
    CHECK("s1_3_c_5", putenv(fresh) == 0 && environ[2000] == fresh && environ[2001] == NULL && environ[1999] == last);

    /* ENVIRON-UNSET-SCAN: removal from a large array. Every copy of the name goes (first slot,
     * adjacent copies, one string in two slots, the last slot), the others keep their order and
     * the array's own storage; an absent name changes no slot; names it is a prefix of stay. */
    static char *rm[2010];
    static char d_first[] = "RM=0", d_a[] = "RM=a", d_b[] = "RM=b", d_last[] = "RM=z", d_pre[] = "RMX=1", d_bare[] = "RM";
    for (int i = 0; i < 2000; i++)
        rm[i] = names[i];
    rm[0] = d_first;
    rm[500] = d_a;
    rm[501] = d_b;
    rm[502] = d_pre;
    rm[503] = d_bare;
    rm[1200] = d_a;
    rm[1999] = d_last;
    rm[2000] = NULL;
    environ = rm;
    CHECK("s1_3_c_4", unsetenv("RMXY") == 0 && unsetenv("NOPE") == 0 && environ == rm && rm[1999] == d_last && rm[0] == d_first);
    CHECK("s1_3_c_4", unsetenv("RM") == 0 && environ == rm && getenv("RM") == NULL && count("RM=") == 0);
    /* 2,000 - 5 entries stay, in order: V1..V499, RMX, RM (no '='), V504..V1199, V1201..V1998. */
    CHECK("s1_3_c_4", rm[1995] == NULL && rm[0] == names[1] && rm[498] == names[499] && rm[499] == d_pre && rm[500] == d_bare);
    CHECK("s1_3_c_4", rm[501] == names[504] && rm[1197] == names[1201] && rm[1994] == names[1998]);
    /* putenv without '=' removes the same way: two copies, one of them the new last entry. */
    static char p_mid[] = "PQ=1", p_end[] = "PQ=2", p_name[] = "PQ";
    rm[700] = p_mid;
    rm[1994] = p_end;
    CHECK("s1_3_c_5", putenv(p_name) == 0 && environ == rm && getenv("PQ") == NULL && rm[1992] == names[1997] && rm[1993] == NULL);
    CHECK("s1_3_c_5", rm[699] == names[702] && rm[700] == names[704] && strcmp(getenv("V1997"), "x") == 0);
    return 0;
}
