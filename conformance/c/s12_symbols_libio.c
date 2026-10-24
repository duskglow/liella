/* Translation tests for spec §12.13 (crates/boundary/tests/s12_stdio_symbols.rs): the libio data
 * objects as compiled C code sees them. `_IO_2_1_stdin_`, `_IO_2_1_stdout_` and `_IO_2_1_stderr_`
 * are the `FILE` objects `stdin`, `stdout` and `stderr` point at; `_IO_list_all` heads the list
 * of open streams, most recently opened first, linked through `_chain`. Self-checking: each
 * symbol's checks end with `end <symbol>` on standard error, each failed check prints
 * `FAIL <symbol>: line N: <check>`; the exit status is 1 when any check failed. */
#define _GNU_SOURCE
#include <stdio.h>

/* Exported, not declared by the installed headers. */
extern FILE _IO_2_1_stdin_, _IO_2_1_stdout_, _IO_2_1_stderr_;
extern FILE *_IO_list_all;

static int failures;
static const char *current;

static void begin(const char *symbol) { current = symbol; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

int main(void)
{
    begin("_IO_2_1_stdin_");
    CHECK(stdin == &_IO_2_1_stdin_ && fileno(&_IO_2_1_stdin_) == 0);
    end();

    begin("_IO_2_1_stdout_");
    CHECK(stdout == &_IO_2_1_stdout_ && fileno(&_IO_2_1_stdout_) == 1);
    CHECK(fputs("through the object\n", &_IO_2_1_stdout_) >= 0 && fflush(stdout) == 0);
    end();

    begin("_IO_2_1_stderr_");
    CHECK(stderr == &_IO_2_1_stderr_ && fileno(&_IO_2_1_stderr_) == 2);
    CHECK(fputs("[through the object]\n", &_IO_2_1_stderr_) >= 0);
    end();

    begin("_IO_list_all");
    {
        FILE *a = tmpfile(), *b = tmpfile();
        CHECK(a != NULL && b != NULL);
        CHECK(_IO_list_all == b && b->_chain == a); /* the newest first */
        CHECK(fclose(b) == 0 && _IO_list_all == a);  /* a closed stream leaves the list */
        CHECK(fclose(a) == 0);
    }
    end();

    return failures ? 1 : 0;
}
