/* Translation tests for spec §1.6, §1.6a and §1.7 (crates/boundary/tests/s01_startup_symbols.rs):
 * each exported getopt and argp entry point and data object, reached from C through the installed
 * headers (the real data symbols, and the real C varargs of argp_error and argp_failure).
 * Self-checking: each symbol's checks end with `end <symbol>` on standard error, each failed check
 * prints `FAIL <symbol>: line N: <check>`; the exit status is 1 when any check failed.
 *
 * The program does not define argp's program-wide variables: the library's own definitions are
 * checked (their defaults, then values the program assigns). What may exit runs in a child
 * process whose standard output and error both go to one file. */
#define _GNU_SOURCE
#include <argp.h>
#include <errno.h>
#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

extern int __posix_getopt(int, char *const *, const char *);

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

static int eq(const char *got, const char *want)
{
    if (got != NULL && strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got ? got : "(null)", want);
    return 0;
}

struct run {
    int status; /* the exit status; 77 when the case returned; -1 for a signal */
    char out[4096];
};

/* Run `body` in a child whose standard output and error go to one file. */
static void run(struct run *r, void (*body)(void))
{
    fflush(NULL);
    FILE *f = tmpfile();
    pid_t pid = fork();
    if (pid == 0) {
        alarm(10);
        dup2(fileno(f), 1);
        dup2(fileno(f), 2);
        body();
        fflush(NULL);
        _exit(77);
    }
    int st = 0;
    syscall(SYS_wait4, pid, &st, 0, NULL);
    r->status = WIFEXITED(st) ? WEXITSTATUS(st) : -1;
    ssize_t n = pread(fileno(f), r->out, sizeof r->out - 1, 0);
    r->out[n > 0 ? n : 0] = 0;
    fclose(f);
}

/* A writable argument vector of up to 7 elements (getopt may permute it). */
struct argv {
    char text[8][16];
    char *v[8];
    int argc;
};

static char **args(struct argv *a, const char *const *items)
{
    a->argc = 0;
    for (; items[a->argc] != NULL; a->argc++) {
        strcpy(a->text[a->argc], items[a->argc]);
        a->v[a->argc] = a->text[a->argc];
    }
    a->v[a->argc] = NULL;
    return a->v;
}

#define ARGS(a, ...) args((a), (const char *const[]){ __VA_ARGS__, NULL })

/* ------------------------------------------------------------------ argp */

enum { PLAIN, STATE_HELP, USAGE, ERROR, FAILURE, REJECT };
static int mode;
static char record[256];
static FILE *help_stream;

static error_t parser(int key, char *arg, struct argp_state *state)
{
    char *end = record + strlen(record);
    size_t room = sizeof record - strlen(record);
    switch (key) {
    case 'v':
        snprintf(end, room, "v ");
        if (mode == STATE_HELP)
            argp_state_help(state, help_stream, ARGP_HELP_SHORT_USAGE);
        if (mode == USAGE)
            argp_usage(state);
        if (mode == ERROR)
            argp_error(state, "bad %s %d", "thing", 7);
        if (mode == FAILURE)
            argp_failure(state, 3, ENOENT, "failure %s", "here");
        return mode == REJECT ? EINVAL : 0;
    case 'o':
        snprintf(end, room, "o=%s ", arg);
        return 0;
    case ARGP_KEY_ARG:
        snprintf(end, room, "ARG=%s ", arg);
        return 0;
    default:
        return ARGP_ERR_UNKNOWN;
    }
}

static const struct argp_option options[] = {
    { "verbose", 'v', 0, 0, "Talk more", 0 },
    { "output", 'o', "FILE", 0, "Write to FILE", 0 },
    { 0 }
};
static const struct argp parse_tree = { options, parser, "IN", "A test program.", 0, 0, 0 };

/* argp_parse of `-v` with `flags`, in a child (the default flags may exit). */
static int child_flags;
static void parse_v(void)
{
    struct argv a;
    argp_parse(&parse_tree, 2, ARGS(&a, "prog", "-v"), (unsigned)child_flags, NULL, NULL);
}

static void parse_version(void)
{
    struct argv a;
    argp_parse(&parse_tree, 2, ARGS(&a, "prog", "--version"), 0, NULL, NULL);
}

static void version_hook(FILE *stream, struct argp_state *state)
{
    fprintf(stream, "HOOK %s\n", state != NULL ? "state" : "none");
}

/* The text argp_help(flags) writes for `name`, in `buf`. */
static const char *help_text(char *buf, size_t cap, unsigned flags, char *name)
{
    char *p = NULL;
    size_t n = 0;
    FILE *m = open_memstream(&p, &n);
    argp_help(&parse_tree, m, flags, name);
    fclose(m);
    snprintf(buf, cap, "%s", p != NULL ? p : "");
    free(p);
    return buf;
}

/* ------------------------------------------------------------------ getopt */

static void unknown_option(void)
{
    struct argv a;
    optind = 0;
    printf("[%d]", getopt(2, ARGS(&a, "prog", "-q"), "ab"));
}

int main(void)
{
    static struct run r;
    static struct argv a;
    char b[1024];
    /* The initial values, before any parse. */
    int optind0 = optind, opterr0 = opterr, optopt0 = optopt;
    char *optarg0 = optarg;
    program_invocation_name = "/usr/bin/prog";
    program_invocation_short_name = "prog";
    opterr = 0;

    begin("getopt");
    {
        char **v = ARGS(&a, "prog", "-a", "x", "-bval");
        optind = 0;
        CHECK(getopt(a.argc, v, "ab:") == 'a' && optind == 2);
        CHECK(getopt(a.argc, v, "ab:") == 'b' && eq(optarg, "val") && optind == 4);
        CHECK(getopt(a.argc, v, "ab:") == -1 && optind == 3 && eq(v[3], "x")); /* permuted */
        optind = 0;
        CHECK(getopt(2, ARGS(&a, "prog", "-z"), "ab:") == '?');
    }
    end();

    begin("__posix_getopt");
    {
        char **v = ARGS(&a, "prog", "-a", "x", "-b");
        optind = 0;
        CHECK(__posix_getopt(a.argc, v, "ab") == 'a');
        CHECK(__posix_getopt(a.argc, v, "ab") == -1 && optind == 2 && eq(v[2], "x"));
    }
    end();

    begin("getopt_long");
    {
        int flag = 0, li = -1;
        const struct option lo[] = { { "verbose", no_argument, NULL, 'v' }, { "file", required_argument, NULL, 'f' }, { "flag", no_argument, &flag, 42 }, { 0 } };
        char **v = ARGS(&a, "prog", "--verb", "--file", "n", "--flag");
        optind = 0;
        CHECK(getopt_long(a.argc, v, "", lo, &li) == 'v' && li == 0);
        CHECK(getopt_long(a.argc, v, "", lo, &li) == 'f' && li == 1 && eq(optarg, "n") && optind == 4);
        CHECK(getopt_long(a.argc, v, "", lo, NULL) == 0 && flag == 42);
        CHECK(getopt_long(a.argc, v, "", lo, &li) == -1 && optind == 5);
    }
    end();

    begin("getopt_long_only");
    {
        int li = -1;
        const struct option lo[] = { { "verbose", no_argument, NULL, 'v' }, { "file", required_argument, NULL, 'f' }, { 0 } };
        char **v = ARGS(&a, "prog", "-file=x", "-verbose");
        optind = 0;
        CHECK(getopt_long_only(a.argc, v, "", lo, &li) == 'f' && li == 1 && eq(optarg, "x"));
        CHECK(getopt_long_only(a.argc, v, "", lo, &li) == 'v' && li == 0);
        CHECK(getopt_long_only(a.argc, v, "", lo, &li) == -1);
    }
    end();

    begin("getsubopt");
    {
        char buf[] = "ro,size=10,bogus";
        char *const tokens[] = { "ro", "rw", "size", NULL };
        char *p = buf, *val = buf;
        CHECK(getsubopt(&p, tokens, &val) == 0 && val == NULL && p == buf + 3);
        CHECK(getsubopt(&p, tokens, &val) == 2 && eq(val, "10"));
        CHECK(getsubopt(&p, tokens, &val) == -1 && eq(val, "bogus"));
        CHECK(*p == 0 && memcmp(buf, "ro\0size\00010\0bogus", 17) == 0);
    }
    end();

    begin("optarg");
    {
        char **v = ARGS(&a, "prog", "-b", "arg", "-a");
        CHECK(optarg0 == NULL);
        optind = 0;
        CHECK(getopt(a.argc, v, "ab:") == 'b' && optarg == v[2]);
        CHECK(getopt(a.argc, v, "ab:") == 'a' && optarg == NULL);
    }
    end();

    begin("optind");
    {
        char **v = ARGS(&a, "prog", "-a", "-b", "-c");
        CHECK(optind0 == 1);
        optind = 0;
        CHECK(getopt(a.argc, v, "abc") == 'a' && optind == 2);
        optind = 3; /* skip -b */
        CHECK(getopt(a.argc, v, "abc") == 'c' && optind == 4);
    }
    end();

    begin("opterr");
    CHECK(opterr0 == 1);
    opterr = 0;
    run(&r, unknown_option);
    CHECK(r.status == 77 && eq(r.out, "[63]"));
    opterr = 1;
    run(&r, unknown_option);
    CHECK(r.status == 77 && eq(r.out, "prog: invalid option -- 'q'\n[63]"));
    opterr = 0;
    end();

    begin("optopt");
    CHECK(optopt0 == '?');
    optind = 0;
    CHECK(getopt(2, ARGS(&a, "prog", "-q"), "ab") == '?' && optopt == 'q');
    end();

    begin("argp_parse");
    {
        int idx = -1;
        mode = PLAIN;
        record[0] = 0;
        CHECK(argp_parse(&parse_tree, 4, ARGS(&a, "prog", "-v", "--output=f", "in"), ARGP_NO_EXIT, &idx, NULL) == 0);
        CHECK(idx == 4 && eq(record, "v o=f ARG=in "));
        mode = REJECT;
        CHECK(argp_parse(&parse_tree, 2, ARGS(&a, "prog", "-v"), ARGP_NO_EXIT, NULL, NULL) == EINVAL);
    }
    end();

    begin("argp_help");
    help_text(b, sizeof b, ARGP_HELP_USAGE, "tprog");
    CHECK(strncmp(b, "Usage: tprog ", 13) == 0 && strstr(b, "[--output=FILE]") != NULL && strstr(b, "[--verbose]") != NULL);
    help_text(b, sizeof b, ARGP_HELP_SHORT_USAGE, "tprog");
    CHECK(eq(b, "Usage: tprog [OPTION...] IN\n"));
    end();

    begin("argp_state_help");
    {
        char *p = NULL;
        size_t n = 0;
        help_stream = open_memstream(&p, &n);
        mode = STATE_HELP;
        CHECK(argp_parse(&parse_tree, 2, ARGS(&a, "prog", "-v"), ARGP_NO_EXIT, NULL, NULL) == 0);
        fclose(help_stream);
        CHECK(eq(p, "Usage: prog [OPTION...] IN\n"));
        free(p);
    }
    end();

    begin("argp_usage");
    mode = USAGE;
    child_flags = 0;
    run(&r, parse_v);
    CHECK(r.status == 64 && eq(r.out, "Usage: prog [OPTION...] IN\nTry `prog --help' or `prog --usage' for more information.\n"));
    end();

    begin("argp_error");
    mode = ERROR;
    child_flags = 0;
    run(&r, parse_v);
    CHECK(r.status == 64 && eq(r.out, "prog: bad thing 7\nTry `prog --help' or `prog --usage' for more information.\n"));
    child_flags = ARGP_NO_EXIT;
    run(&r, parse_v);
    CHECK(r.status == 77 && eq(r.out, "prog: bad thing 7\nTry `prog --help' or `prog --usage' for more information.\n"));
    end();

    begin("argp_failure");
    mode = FAILURE;
    child_flags = 0;
    run(&r, parse_v);
    CHECK(r.status == 3 && eq(r.out, "prog: failure here: No such file or directory\n"));
    end();

    begin("argp_err_exit_status");
    CHECK(argp_err_exit_status == 64);
    argp_err_exit_status = 9;
    mode = USAGE;
    child_flags = 0;
    run(&r, parse_v);
    CHECK(r.status == 9);
    argp_err_exit_status = 64;
    end();

    begin("argp_program_version");
    CHECK(argp_program_version == NULL);
    argp_program_version = "tprog 2.0";
    run(&r, parse_version);
    CHECK(r.status == 0 && eq(r.out, "tprog 2.0\n"));
    argp_program_version = NULL;
    end();

    begin("argp_program_version_hook");
    CHECK(argp_program_version_hook == NULL);
    argp_program_version_hook = version_hook;
    run(&r, parse_version);
    CHECK(r.status == 0 && eq(r.out, "HOOK state\n"));
    argp_program_version_hook = NULL;
    end();

    begin("argp_program_bug_address");
    CHECK(argp_program_bug_address == NULL);
    CHECK(strstr(help_text(b, sizeof b, ARGP_HELP_BUG_ADDR, "tprog"), "Report bugs") == NULL);
    argp_program_bug_address = "<bugs@example.org>";
    CHECK(eq(help_text(b, sizeof b, ARGP_HELP_BUG_ADDR, "tprog"), "Report bugs to <bugs@example.org>.\n"));
    argp_program_bug_address = NULL;
    end();

    return failures ? 1 : 0;
}
