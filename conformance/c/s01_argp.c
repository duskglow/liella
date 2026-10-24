/* Spec §1.7: the argp framework through the real C interface (option tables, callbacks, the
 * program-wide variables, the variadic argp_error and argp_failure). Self-checking: each clause
 * ends with `end <clause>` on standard error, each failed check prints `FAIL <clause>: line N:
 * <check>`; the clause tests in crates/core/tests/s01_startup_clauses.rs read these lines.
 *
 * Each parse runs in a child process (it may exit) whose standard output and error go to files;
 * the parsers write one line per call to standard output: the parser, then the key (a letter,
 * a number, or the name of a special key), then `=arg` when there is an argument. */
#define _GNU_SOURCE
#include <argp.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

const char *argp_program_version = "prog 1.0";
const char *argp_program_bug_address = "<bugs@example.org>";

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

/* ------------------------------------------------------------------ the parsers */

/* What the main parser does at ARGP_KEY_END. */
enum { END_OK, END_ERROR, END_FAILURE };
static int end_mode;

static void note(const char *who, int key, const char *arg)
{
    const char *name = NULL;
    switch (key) {
    case ARGP_KEY_ARG: name = "ARG"; break;
    case ARGP_KEY_ARGS: name = "ARGS"; break;
    case ARGP_KEY_END: name = "END"; break;
    case ARGP_KEY_NO_ARGS: name = "NO_ARGS"; break;
    case ARGP_KEY_INIT: name = "INIT"; break;
    case ARGP_KEY_SUCCESS: name = "SUCCESS"; break;
    case ARGP_KEY_ERROR: name = "ERROR"; break;
    case ARGP_KEY_FINI: name = "FINI"; break;
    }
    if (name)
        printf("%s %s", who, name);
    else if (key > ' ' && key < 127)
        printf("%s %c", who, key);
    else
        printf("%s %d", who, key);
    if (arg)
        printf("=%s", arg);
    printf("\n");
}

static error_t main_parser(int key, char *arg, struct argp_state *state)
{
    note("main", key, arg);
    switch (key) {
    case 'v': case 'o': case 'l': case 1000:
        return 0;
    case ARGP_KEY_ARG:
        if (strcmp(arg, "stop") == 0)
            return ARGP_ERR_UNKNOWN;
        if (strcmp(arg, "bad") == 0)
            return EINVAL;
        return 0;
    case ARGP_KEY_END:
        if (end_mode == END_ERROR)
            argp_error(state, "bad thing %d", 7);
        if (end_mode == END_FAILURE)
            argp_failure(state, 3, ENOENT, "failure %s", "here");
        return 0;
    case ARGP_KEY_INIT: case ARGP_KEY_NO_ARGS: case ARGP_KEY_SUCCESS: case ARGP_KEY_ERROR: case ARGP_KEY_FINI:
        return 0;
    default:
        return ARGP_ERR_UNKNOWN;
    }
}

static error_t child_parser(int key, char *arg, struct argp_state *state)
{
    (void)state;
    note("child", key, arg);
    if (key == 'c')
        return 0;
    if (key == ARGP_KEY_ARG && strcmp(arg, "stop") == 0)
        return ARGP_ERR_UNKNOWN;
    return key == ARGP_KEY_ARG || key == ARGP_KEY_ARGS ? ARGP_ERR_UNKNOWN : 0;
}

static const struct argp_option main_options[] = {
    { "verbose", 'v', 0, 0, "Produce verbose output", 0 },
    { "output", 'o', "FILE", 0, "Output to FILE instead of standard output, with a description long enough to wrap at the default margin", 0 },
    { "level", 'l', "N", OPTION_ARG_OPTIONAL, "Set the level", 0 },
    { 0, 0, 0, 0, "Advanced options:", 2 },
    { "extra", 1000, 0, 0, "An option in the second group", 0 },
    { 0 }
};

static const struct argp_option child_options[] = {
    { "child", 'c', 0, 0, "A child option", 0 },
    { 0 }
};

static const struct argp child_argp = { child_options, child_parser, 0, 0, 0, 0, 0 };
static const struct argp_child children[] = { { &child_argp, 0, "Child options:", 5 }, { 0 } };
static const struct argp main_argp = { main_options, main_parser, "ARG1 ARG2", "Program description.\vText after the options.", children, 0, 0 };

/* ------------------------------------------------------------------ running a parse */

struct run {
    int status;   /* the exit status; 77 when argp_parse returned; -1 for a signal */
    int result;   /* what argp_parse returned */
    int index;    /* *arg_index (-1 when not stored) */
    char out[8192];
    char err[8192];
};

static unsigned run_flags;
static char **run_argv;
static int run_argc;
static int want_index;
static const char *run_env;

static void slurp(int fd, char *out, size_t cap)
{
    ssize_t n = pread(fd, out, cap - 1, 0);
    out[n > 0 ? n : 0] = 0;
}

/* Parse `argv` (NULL-terminated) with `flags` in a child; `env` is NAME=VALUE or NULL. */
static void parse(struct run *r, unsigned flags, const char *env, char **argv)
{
    run_flags = flags;
    run_argv = argv;
    for (run_argc = 0; argv[run_argc]; run_argc++)
        ;
    run_env = env;
    fflush(NULL);
    FILE *fo = tmpfile(), *fe = tmpfile(), *fr = tmpfile();
    pid_t pid = fork();
    if (pid == 0) {
        alarm(10);
        dup2(fileno(fo), 1);
        dup2(fileno(fe), 2);
        if (run_env)
            putenv((char *)run_env);
        int index = -1;
        int result = argp_parse(&main_argp, run_argc, run_argv, run_flags, want_index ? &index : NULL, NULL);
        fflush(NULL);
        int pair[2] = { result, index };
        if (pwrite(fileno(fr), pair, sizeof pair, 0) != (ssize_t)sizeof pair)
            _exit(78);
        _exit(77);
    }
    int st = 0;
    syscall(SYS_wait4, pid, &st, 0, NULL);
    r->status = WIFEXITED(st) ? WEXITSTATUS(st) : -1;
    int pair[2] = { -100, -100 };
    if (pread(fileno(fr), pair, sizeof pair, 0) != (ssize_t)sizeof pair)
        pair[0] = pair[1] = -100;
    r->result = pair[0];
    r->index = pair[1];
    slurp(fileno(fo), r->out, sizeof r->out);
    slurp(fileno(fe), r->err, sizeof r->err);
    fclose(fo);
    fclose(fe);
    fclose(fr);
}

/* The calls one parser received, as `KEY KEY=arg ...`. */
static void calls(const struct run *r, const char *who, char *dst, size_t cap)
{
    size_t wl = strlen(who), n = 0;
    dst[0] = 0;
    for (const char *l = r->out; *l;) {
        const char *e = strchr(l, '\n');
        size_t len = e ? (size_t)(e - l) : strlen(l);
        if (len > wl && strncmp(l, who, wl) == 0 && l[wl] == ' ' && n + len - wl + 1 < cap) {
            if (n)
                dst[n++] = ' ';
            memcpy(dst + n, l + wl + 1, len - wl - 1);
            n += len - wl - 1;
            dst[n] = 0;
        }
        l += len + (e != NULL);
    }
}

static int eq(const char *got, const char *want)
{
    if (strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
    return 0;
}

static int has(const char *text, const char *part)
{
    if (strstr(text, part))
        return 1;
    fprintf(stderr, "  [%s] lacks [%s]\n", text, part);
    return 0;
}

/* The longest line of `text`, ignoring the parsers' own lines. */
static size_t widest(const char *text)
{
    size_t w = 0;
    for (const char *l = text; *l;) {
        const char *e = strchr(l, '\n');
        size_t len = e ? (size_t)(e - l) : strlen(l);
        if (strncmp(l, "main ", 5) != 0 && strncmp(l, "child ", 6) != 0 && len > w)
            w = len;
        l += len + (e != NULL);
    }
    return w;
}

#define ARGV(...) ((char *[]){ __VA_ARGS__, NULL })
#define TRY "Try `prog --help' or `prog --usage' for more information.\n"

int main(void)
{
    static struct run r;
    char seq[1024];
    program_invocation_name = "prog";
    program_invocation_short_name = "prog";

    begin("s1_7_c_0");
    /* A tree: the main parser's options reach it, the child's options reach the child. */
    parse(&r, 0, NULL, ARGV("prog", "-v", "-c", "--child", "a"));
    CHECK(r.status == 77 && r.result == 0);
    calls(&r, "main", seq, sizeof seq);
    CHECK(has(seq, "v") && has(seq, "ARG=a"));
    calls(&r, "child", seq, sizeof seq);
    CHECK(has(seq, "INIT c c "));
    end();

    begin("s1_7_c_1");
    /* One call per option and per operand, between the initialization and finalization events. */
    parse(&r, 0, NULL, ARGV("prog", "-v", "a", "--output", "f", "b"));
    CHECK(r.status == 77 && r.result == 0);
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT v o=f ARG=a ARG=b END SUCCESS FINI"));
    calls(&r, "child", seq, sizeof seq);
    CHECK(strncmp(seq, "INIT", 4) == 0 && strlen(seq) >= 4 && strcmp(seq + strlen(seq) - 4, "FINI") == 0);
    /* No operands at all: ARGP_KEY_NO_ARGS. */
    parse(&r, 0, NULL, ARGV("prog", "-v"));
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT v NO_ARGS END SUCCESS FINI"));
    end();

    begin("s1_7_c_2");
    parse(&r, 0, NULL, ARGV("prog", "--help"));
    CHECK(r.status == 0 && r.err[0] == 0);
    CHECK(has(r.out, "Usage: prog [OPTION...] ARG1 ARG2\n") && has(r.out, "Program description.\n"));
    CHECK(has(r.out, "  -v, --verbose ") && has(r.out, "Produce verbose output\n"));
    CHECK(has(r.out, "  -o, --output=FILE ") && has(r.out, "  -l, --level[=N] "));
    CHECK(has(r.out, " Advanced options:\n") && has(r.out, "      --extra ") && has(r.out, " Child options:\n"));
    CHECK(has(r.out, "  -?, --help ") && has(r.out, "      --usage ") && has(r.out, "  -V, --version "));
    CHECK(has(r.out, "Text after the options.\n") && has(r.out, "Report bugs to <bugs@example.org>.\n"));
    CHECK(strstr(r.out, "--verbose") < strstr(r.out, "Advanced options:"));
    CHECK(strstr(r.out, "Advanced options:") < strstr(r.out, "Child options:"));
    size_t normal = widest(r.out);
    CHECK(normal > 40 && normal <= 79);
    parse(&r, 0, NULL, ARGV("prog", "-?"));
    CHECK(r.status == 0 && has(r.out, "Usage: prog [OPTION...] ARG1 ARG2\n"));
    parse(&r, 0, NULL, ARGV("prog", "--usage"));
    CHECK(r.status == 0 && has(r.out, "Usage: prog [-vc?V] ") && has(r.out, "[-l[N]]") && has(r.out, "[-o FILE]"));
    CHECK(has(r.out, "[--verbose]") && has(r.out, "[--output=FILE]") && has(r.out, "[--level[=N]]") && has(r.out, "ARG1 ARG2\n"));
    parse(&r, 0, NULL, ARGV("prog", "--version"));
    CHECK(r.status == 0 && has(r.out, "\nprog 1.0\n") && r.err[0] == 0);
    parse(&r, 0, NULL, ARGV("prog", "-V"));
    CHECK(r.status == 0 && has(r.out, "\nprog 1.0\n"));
    /* The width: ARGP_HELP_FMT's right margin, and the terminal's (COLUMNS, read as the spec's
     * Kernel line says: the right margin is COLUMNS - 1, D-1.7; the reference ignores it). */
    parse(&r, 0, "ARGP_HELP_FMT=rmargin=40", ARGV("prog", "--help"));
    CHECK(r.status == 0 && widest(r.out) <= 40 && has(r.out, "  -v, --verbose "));
    parse(&r, 0, "COLUMNS=50", ARGV("prog", "--help"));
    CHECK(r.status == 0 && widest(r.out) <= 49 && has(r.out, "  -v, --verbose "));
    end();

    begin("s1_7_c_3");
    parse(&r, 0, NULL, ARGV("prog", "-z"));
    CHECK(eq(r.err, "prog: invalid option -- 'z'\n" TRY));
    parse(&r, 0, NULL, ARGV("prog", "--bogus"));
    CHECK(eq(r.err, "prog: unrecognized option '--bogus'\n" TRY));
    parse(&r, 0, NULL, ARGV("prog", "-o"));
    CHECK(eq(r.err, "prog: option requires an argument -- 'o'\n" TRY));
    end_mode = END_ERROR;
    parse(&r, 0, NULL, ARGV("prog"));
    CHECK(eq(r.err, "prog: bad thing 7\n" TRY));
    end_mode = END_FAILURE;
    parse(&r, 0, NULL, ARGV("prog"));
    CHECK(r.status == 3 && eq(r.err, "prog: failure here: No such file or directory\n"));
    end_mode = END_OK;
    end();

    begin("s1_7_i_0");
    /* Descriptors: name, key, argument (required, optional), group, documentation. */
    parse(&r, 0, NULL, ARGV("prog", "-ofile1", "-o", "file2", "--output=file3", "--out", "file4", "-l", "-l3", "--level=4", "--extra"));
    CHECK(r.status == 77 && r.result == 0);
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT o=file1 o=file2 o=file3 o=file4 l l=3 l=4 1000 NO_ARGS END SUCCESS FINI"));
    /* ARGP_IN_ORDER: operands in place; by default they follow the options. */
    parse(&r, ARGP_IN_ORDER, NULL, ARGV("prog", "a", "-v", "b"));
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT ARG=a v ARG=b END SUCCESS FINI"));
    parse(&r, 0, NULL, ARGV("prog", "a", "-v", "b"));
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT v ARG=a ARG=b END SUCCESS FINI"));
    /* ARGP_PARSE_ARGV0: the first element is parsed too; <argp.h> documents that the flag
     * counts only with ARGP_NO_ERRS (otherwise argv[0] names the program in messages). */
    parse(&r, ARGP_PARSE_ARGV0 | ARGP_NO_ERRS, NULL, ARGV("-v", "a"));
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT v ARG=a END SUCCESS FINI"));
    parse(&r, ARGP_PARSE_ARGV0, NULL, ARGV("-v", "a"));
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT ARG=a END SUCCESS FINI"));
    parse(&r, 0, NULL, ARGV("-v", "a"));
    calls(&r, "main", seq, sizeof seq);
    CHECK(eq(seq, "INIT ARG=a END SUCCESS FINI"));
    /* ARGP_NO_HELP: --help is not an option. */
    parse(&r, ARGP_NO_HELP, NULL, ARGV("prog", "--help"));
    CHECK(r.status == 64 && has(r.err, "prog: unrecognized option '--help'\n"));
    /* ARGP_NO_EXIT: the error is reported and returned. */
    parse(&r, ARGP_NO_EXIT, NULL, ARGV("prog", "-z"));
    CHECK(r.status == 77 && r.result != 0 && has(r.err, "prog: invalid option -- 'z'\n"));
    /* ARGP_SILENT: nothing printed, no exit. */
    parse(&r, ARGP_SILENT, NULL, ARGV("prog", "-z"));
    CHECK(r.status == 77 && r.result != 0 && r.err[0] == 0);
    parse(&r, ARGP_SILENT, NULL, ARGV("prog", "--help"));
    CHECK(r.status == 77 && r.result != 0 && r.err[0] == 0 && strstr(r.out, "Usage") == NULL);
    end();

    begin("s1_7_g_0");
    want_index = 1;
    parse(&r, 0, NULL, ARGV("prog", "-v", "a", "b"));
    CHECK(r.status == 77 && r.result == 0 && r.index == 4);
    /* An operand no parser takes: success, and the index of the first unparsed element. */
    parse(&r, 0, NULL, ARGV("prog", "a", "stop", "b"));
    CHECK(r.status == 77 && r.result == 0 && r.index == 2);
    want_index = 0;
    /* A usage error exits with argp_err_exit_status: EX_USAGE (64) by default. */
    CHECK(argp_err_exit_status == 64);
    parse(&r, 0, NULL, ARGV("prog", "-z"));
    CHECK(r.status == 64);
    argp_err_exit_status = 9;
    parse(&r, 0, NULL, ARGV("prog", "--bogus"));
    CHECK(r.status == 9);
    end_mode = END_ERROR;
    parse(&r, 0, NULL, ARGV("prog"));
    CHECK(r.status == 9);
    end_mode = END_OK;
    argp_err_exit_status = 64;
    end();

    begin("s1_7_f_0");
    /* An error from the callback ends the parse and is returned; later operands are not seen. */
    parse(&r, 0, NULL, ARGV("prog", "a", "bad", "c"));
    CHECK(r.status == 77 && r.result == EINVAL);
    calls(&r, "main", seq, sizeof seq);
    CHECK(has(seq, "ARG=a ARG=bad") && strstr(seq, "ARG=c") == NULL && strstr(seq, "SUCCESS") == NULL);
    /* ARGP_ERR_UNKNOWN: the operand is offered to the child parser. */
    want_index = 1;
    parse(&r, 0, NULL, ARGV("prog", "stop"));
    want_index = 0;
    calls(&r, "child", seq, sizeof seq);
    CHECK(has(seq, "ARG=stop"));
    end();

    return failures ? 1 : 0;
}
