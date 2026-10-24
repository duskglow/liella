/* env-probe: how the reference library's environment readers see changes to the environment.
 * Readers: getenv("IFS"), wordexp("$X") (IFS splitting), gettext("hello") (LANGUAGE), localtime /
 * localtime_r (TZ). Every trial runs in a forked child (fresh process state).
 * usage: env_probe matrix            -> the A/C visibility matrix
 *        env_probe cost R N POS K    -> prime once, then K calls (R reader 0-4, N env size,
 *                                       POS 0 first / 1 last / 2 absent); use under perf stat
 * LOCDIR (env) = directory holding de/LC_MESSAGES/envp.mo and fr/LC_MESSAGES/envp.mo. */
#define _GNU_SOURCE
#include <libintl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <wordexp.h>

extern char **environ;

static const char *rname[] = {"getenv", "wordexp", "gettext", "localtime", "localtime_r"};
static const char *varof[] = {"IFS", "IFS", "LANGUAGE", "TZ", "TZ"};
static const char *valA[] = {":", ":", "de", "EST5", "EST5"};
static const char *valB[] = {",", ",", "fr", "PST8", "PST8"};
#define NR 5
#define XVAL "a:b,c d"

static void rd(int r, char *out, size_t n) {
    switch (r) {
    case 0: {
        const char *v = getenv("IFS");
        snprintf(out, n, "%s", v ? v : "(null)");
        break;
    }
    case 1: {
        wordexp_t w;
        int rc = wordexp("$X", &w, WRDE_NOCMD);
        if (rc) {
            snprintf(out, n, "rc=%d", rc);
            break;
        }
        size_t o = (size_t)snprintf(out, n, "n=%zu", w.we_wordc);
        for (size_t i = 0; i < w.we_wordc && o < n; i++)
            o += (size_t)snprintf(out + o, n - o, "[%s]", w.we_wordv[i]);
        wordfree(&w);
        break;
    }
    case 2:
        snprintf(out, n, "%s", gettext("hello"));
        break;
    case 3:
    case 4: {
        time_t t = 0;
        struct tm tmv, *p;
        if (r == 3)
            p = localtime(&t);
        else
            p = localtime_r(&t, &tmv);
        snprintf(out, n, "d%02d h%02d %s", p->tm_mday, p->tm_hour, p->tm_zone);
        break;
    }
    }
}

static void set_base(int r, const char *val) {
    clearenv();
    setenv("X", XVAL, 1);
    setenv("ZZ", "filler", 1);
    if (val) setenv(varof[r], val, 1);
}

static int find_slot(const char *name) {
    size_t l = strlen(name);
    for (int i = 0; environ[i]; i++)
        if (!strncmp(environ[i], name, l) && environ[i][l] == '=') return i;
    return -1;
}

static char *mk(const char *name, const char *val) {
    char *s = malloc(strlen(name) + strlen(val) + 2);
    sprintf(s, "%s=%s", name, val);
    return s;
}

/* change ids */
enum { REF_U = 0, REF_A, REF_B, C_SETENV = 10, C_UNSETENV, C_PUTENV, C_CLEARENV, C_SETENV_NEW, C_PUTENV_NEW,
       C_PUTBUF_MUT = 20, C_SLOT_PTR = 30, C_ENVIRON_ARR = 40, C_ENVIRON_ARR_U, C_SLOT_NEWVAR = 50,
       C_PUTBUF_RENAME = 51, C_DUP_AB = 60, C_DUP_BA };
static const struct { int id; const char *name; const char *exp; } changes[] = {
    {C_SETENV, "1a setenv overwrite A->B", "B"},
    {C_UNSETENV, "1b unsetenv A->unset", "U"},
    {C_PUTENV, "1c putenv(\"V=B\") over A", "B"},
    {C_CLEARENV, "1d clearenv A->unset", "U"},
    {C_SETENV_NEW, "1e setenv unset->B", "B"},
    {C_PUTENV_NEW, "1f putenv unset->B", "B"},
    {C_PUTBUF_MUT, "2  putenv'd buffer mutated in place A->B", "B"},
    {C_SLOT_PTR, "3  environ[i]=new ptr A->B", "B"},
    {C_ENVIRON_ARR, "4a environ=new array A->B", "B"},
    {C_ENVIRON_ARR_U, "4b environ=new array A->unset", "U"},
    {C_SLOT_NEWVAR, "5a slot ZZ overwritten by ptr: unset->B", "B"},
    {C_PUTBUF_RENAME, "5b putenv'd buf renamed in place: unset->B", "B"},
    {C_DUP_AB, "6a duplicates [V=A,V=B]", "?"},
    {C_DUP_BA, "6b duplicates [V=B,V=A]", "?"},
};

static void trial(int r, int c, int pre_tzset, char *before, char *after, char *after2, size_t n) {
    const char *var = varof[r], *A = valA[r], *B = valB[r];
    int init_unset = (c == C_SETENV_NEW || c == C_PUTENV_NEW || c == C_SLOT_NEWVAR || c == C_PUTBUF_RENAME);
    char *buf = NULL;
    if (c == C_DUP_AB || c == C_DUP_BA) {
        static char *arr[4];
        arr[0] = mk("X", XVAL);
        arr[1] = mk(var, c == C_DUP_AB ? A : B);
        arr[2] = mk(var, c == C_DUP_AB ? B : A);
        arr[3] = NULL;
        environ = arr;
        before[0] = 0;
        if (r >= 3) tzset();
        rd(r, after, n);
        after2[0] = 0;
        return;
    }
    set_base(r, init_unset ? NULL : A);
    if (c == C_PUTBUF_MUT) {
        unsetenv(var);
        buf = mk(var, A);
        putenv(buf);
    }
    if (c == C_PUTBUF_RENAME) {
        char *q = malloc(strlen(var) + strlen(B) + 2);
        memset(q, 'Q', strlen(var));
        sprintf(q + strlen(var), "=%s", B);
        buf = q;
        putenv(buf);
    }
    if (pre_tzset && r >= 3) tzset();
    rd(r, before, n);
    switch (c) {
    case C_SETENV: setenv(var, B, 1); break;
    case C_UNSETENV: unsetenv(var); break;
    case C_PUTENV: putenv(mk(var, B)); break;
    case C_CLEARENV: clearenv(); setenv("X", XVAL, 1); break;
    case C_SETENV_NEW: setenv(var, B, 1); break;
    case C_PUTENV_NEW: putenv(mk(var, B)); break;
    case C_PUTBUF_MUT: memcpy(buf + strlen(var) + 1, B, strlen(B)); break;
    case C_SLOT_PTR: environ[find_slot(var)] = mk(var, B); break;
    case C_ENVIRON_ARR: {
        static char *arr[3];
        arr[0] = mk("X", XVAL);
        arr[1] = mk(var, B);
        arr[2] = NULL;
        environ = arr;
        break;
    }
    case C_ENVIRON_ARR_U: {
        static char *arr[2];
        arr[0] = mk("X", XVAL);
        arr[1] = NULL;
        environ = arr;
        break;
    }
    case C_SLOT_NEWVAR: environ[find_slot("ZZ")] = mk(var, B); break;
    case C_PUTBUF_RENAME: memcpy(buf, var, strlen(var)); break;
    }
    rd(r, after, n);
    if (r >= 3) {
        tzset();
        rd(r, after2, n);
    } else
        strcpy(after2, "-");
}

static void ref(int r, int st, char *out, size_t n) {
    set_base(r, st == REF_A ? valA[r] : st == REF_B ? valB[r] : NULL);
    if (r >= 3) tzset();
    rd(r, out, n);
}

typedef struct { char s[256]; } str;

/* run fn-ish in a child; the child writes its 3 strings separated by \x01 to a pipe */
static str spawn(int r, int c, int pre, int refmode) {
    int fd[2];
    if (pipe(fd)) exit(2);
    fflush(NULL);
    pid_t p = fork();
    if (p == 0) {
        close(fd[0]);
        char b[80] = "", a[80] = "", a2[80] = "", line[256];
        if (refmode)
            ref(r, c, a, sizeof a);
        else
            trial(r, c, pre, b, a, a2, sizeof a);
        snprintf(line, sizeof line, "%s\x01%s\x01%s", b, a, a2);
        if (write(fd[1], line, strlen(line)) < 0) _exit(3);
        _exit(0);
    }
    close(fd[1]);
    str o = {{0}};
    ssize_t k = read(fd[0], o.s, sizeof o.s - 1);
    if (k < 0) k = 0;
    o.s[k] = 0;
    close(fd[0]);
    int st;
    waitpid(p, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st)) snprintf(o.s, sizeof o.s, "CHILD-FAILED(%d)\x01\x01", st);
    return o;
}

static void split(str *s, char **f) {
    f[0] = s->s;
    int i = 1;
    for (char *q = s->s; *q && i < 3; q++)
        if (*q == 1) {
            *q = 0;
            f[i++] = q + 1;
        }
    while (i < 3) f[i++] = s->s + strlen(s->s);
}

static void matrix(void) {
    for (int r = 0; r < NR; r++) {
        for (int pre = 0; pre < (r >= 3 ? 2 : 1); pre++) {
            char label[40];
            snprintf(label, sizeof label, "%s%s", rname[r], pre ? "+tzset-primed" : "");
            char *f[3];
            str su = spawn(r, REF_U, 0, 1), sa = spawn(r, REF_A, 0, 1), sb = spawn(r, REF_B, 0, 1);
            split(&su, f);
            char U[80], A[80], B[80];
            snprintf(U, sizeof U, "%s", f[1]);
            split(&sa, f);
            snprintf(A, sizeof A, "%s", f[1]);
            split(&sb, f);
            snprintf(B, sizeof B, "%s", f[1]);
            printf("== reader %s (var %s): fresh unset=[%s] A=%s=[%s] B=%s=[%s]\n", label, varof[r], U, valA[r], A,
                   valB[r], B);
            for (size_t i = 0; i < sizeof changes / sizeof *changes; i++) {
                str s = spawn(r, changes[i].id, pre, 0);
                split(&s, f);
                const char *exp = changes[i].exp[0] == 'B' ? B : changes[i].exp[0] == 'U' ? U : NULL;
                const char *verdict;
                if (exp)
                    verdict = !strcmp(f[1], exp) ? "SEEN" : "NOT-SEEN";
                else
                    verdict = !strcmp(f[1], A) ? "first-wins(A-listed-first)" : !strcmp(f[1], B) ? "B-value" : "neither";
                if (!exp && changes[i].id == C_DUP_BA)
                    verdict = !strcmp(f[1], B) ? "first-wins" : !strcmp(f[1], A) ? "last-wins" : "neither";
                if (!exp && changes[i].id == C_DUP_AB)
                    verdict = !strcmp(f[1], A) ? "first-wins" : !strcmp(f[1], B) ? "last-wins" : "neither";
                printf("%-12s | %-44s | before=[%s] after=[%s] after_tzset=[%s] | %s\n", label, changes[i].name, f[0], f[1],
                       f[2], verdict);
            }
        }
    }
}


/* gettext stickiness: prime with LANGUAGE=de, switch to fr, then try operations */
static void gtmode(void) {
    static const char *ops[] = {"gettext(hello) again", "gettext(bye) (never looked up)", "dgettext(envp,hello)",
        "bindtextdomain(same dir) + gettext(hello)", "textdomain(envp) + gettext(hello)",
        "setlocale(LC_ALL,\"\") + gettext(hello)", "bindtextdomain(other dir string) + gettext(hello)",
        "setlocale(LC_MESSAGES,\"C\") then setlocale(LC_MESSAGES,\"\") + gettext(hello)"};
    const char *d = getenv("LOCDIR");
    for (int o = 0; o < 8; o++) {
        int fd[2];
        if (pipe(fd)) exit(2);
        fflush(NULL);
        pid_t p = fork();
        if (p == 0) {
            char line[200], alt[400];
            set_base(2, "de");
            gettext("hello");
            setenv("LANGUAGE", "fr", 1);
            const char *res = NULL;
            switch (o) {
            case 0: res = gettext("hello"); break;
            case 1: res = gettext("bye"); break;
            case 2: res = dgettext("envp", "hello"); break;
            case 3: bindtextdomain("envp", d); res = gettext("hello"); break;
            case 4: textdomain("envp"); res = gettext("hello"); break;
            case 5: setenv("LANG", "en_US.utf8", 1); setlocale(LC_ALL, ""); res = gettext("hello"); break;
            case 6: snprintf(alt, sizeof alt, "%s/.", d); bindtextdomain("envp", alt); res = gettext("hello"); break;
            case 7: setenv("LANG", "en_US.utf8", 1); setlocale(LC_MESSAGES, "C"); setlocale(LC_MESSAGES, ""); res = gettext("hello"); break;
            }
            snprintf(line, sizeof line, "%s", res);
            if (write(fd[1], line, strlen(line)) < 0) _exit(3);
            _exit(0);
        }
        close(fd[1]);
        char b[200] = "";
        ssize_t k = read(fd[0], b, sizeof b - 1);
        if (k < 0) k = 0;
        b[k] = 0;
        close(fd[0]);
        waitpid(p, NULL, 0);
        printf("primed LANGUAGE=de (hello->hallo), then LANGUAGE=fr; %-70s -> [%s]\n", ops[o], b);
    }
}

static void cost(int r, int N, int pos, int K) {
    char *arr[N + 3];
    int n = 0;
    arr[n++] = mk("X", XVAL);
    int vslot = pos == 0 ? 1 : -1;
    if (pos == 0) arr[n++] = mk(varof[r], valA[r]);
    for (int i = 0; n < N - (pos == 1 ? 1 : 0); i++) {
        char nm[16];
        snprintf(nm, sizeof nm, "V%04d", i);
        arr[n++] = mk(nm, "some filler value 0123456789");
    }
    if (pos == 1) arr[n++] = mk(varof[r], valA[r]);
    (void)vslot;
    arr[n] = NULL;
    environ = arr;
    char out[200];
    rd(r, out, sizeof out); /* prime */
    for (int i = 0; i < K; i++) rd(r, out, sizeof out);
    fprintf(stderr, "cost r=%d N=%d(n=%d) pos=%d K=%d last=[%s]\n", r, N, n, pos, K, out);
}

int main(int argc, char **argv) {
    setlocale(LC_ALL, "");
    const char *d = getenv("LOCDIR");
    bindtextdomain("envp", d ? d : ".");
    textdomain("envp");
    if (argc >= 2 && !strcmp(argv[1], "matrix")) {
        matrix();
        return 0;
    }
    if (argc >= 2 && !strcmp(argv[1], "gt")) {
        gtmode();
        return 0;
    }
    if (argc == 6 && !strcmp(argv[1], "cost")) {
        cost(atoi(argv[2]), atoi(argv[3]), atoi(argv[4]), atoi(argv[5]));
        return 0;
    }
    fprintf(stderr, "usage\n");
    return 2;
}
