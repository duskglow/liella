/* Spec §12.7: formatted input through real C varargs (the C99 symbol set, as -std=c17
 * selects; `%b` through the C23 one). Self-checking: each clause ends with `end <clause>` on
 * standard error, each failed check prints `FAIL <clause>: line N: <check>`; the clause tests in
 * crates/core/tests/s12_stdio_clauses.rs read these lines. */
#define _GNU_SOURCE
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern int __isoc23_sscanf(const char *, const char *, ...);
/* The plain (GNU) and C99 symbol sets, by their symbols (the header would redirect sscanf). */
extern int plain_sscanf(const char *, const char *, ...) __asm__("sscanf");
extern int c99_sscanf(const char *, const char *, ...) __asm__("__isoc99_sscanf");

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

int main(void)
{
    begin("s12_7_c_0");
    {
        int n = 0;
        char s[8] = "";
        CHECK(sscanf("12 abc", "%d %s", &n, s) == 2 && n == 12 && strcmp(s, "abc") == 0);
    }
    end();

    begin("s12_7_c_1");
    {
        int a = 0, b = 0;
        char c = 0;
        CHECK(sscanf("1 \t\n\n   2", "%d %d", &a, &b) == 2 && a == 1 && b == 2);
        CHECK(sscanf("x", " \n %c", &c) == 1 && c == 'x'); /* including none */
        CHECK(sscanf("   \t y", " %c", &c) == 1 && c == 'y');
    }
    end();

    begin("s12_7_c_2");
    {
        int a = 0, b = 0;
        CHECK(sscanf("1,2", "%d,%d", &a, &b) == 2 && a == 1 && b == 2);
        b = -9;
        CHECK(sscanf("3;4", "%d,%d", &a, &b) == 1 && a == 3 && b == -9); /* a mismatch stops the scan */
    }
    end();

    begin("s12_7_c_3");
    {
        int i = 0, j = 0, k = 0, o = 0, h = 0, x = 0;
        unsigned u = 0;
        signed char c8 = 0;
        CHECK(sscanf("12345", "%3d%d", &i, &j) == 2 && i == 123 && j == 45);      /* a width */
        CHECK(sscanf("7 8", "%*d %d", &k) == 1 && k == 8);                      /* `*` assigns nothing */
        CHECK(sscanf("0x1f 017 9 -0X1A", "%i %o %u %x", &i, &o, &u, &h) == 4 && i == 31 && o == 15 && u == 9 && h == -26);
        CHECK(sscanf("-5 7", "%hhd %X", &c8, &x) == 2 && c8 == -5 && x == 7);   /* a length */
        float f = 0;
        double d = 0, e = 0;
        CHECK(sscanf("1.5 0x1p3 -2e2", "%f %la %lg", &f, &d, &e) == 3 && f == 1.5f && d == 8.0 && e == -200.0);
        char s[8] = "", cc[3] = "", set[8] = "";
        void *p = NULL;
        int n = -1;
        CHECK(sscanf("word xyz aabbc 0x10 %", "%s %3c %[ab]%*c %p%n %%", s, cc, set, &p, &n) == 4); /* %n is not counted */
        CHECK(strcmp(s, "word") == 0 && memcmp(cc, "xyz", 3) == 0 && strcmp(set, "aabb") == 0 && p == (void *)16 && n == 19);
        char *ms = NULL, *mc = NULL, *mset = NULL;
        CHECK(sscanf("alloc bc xyx", "%ms %2mc %m[xy]", &ms, &mc, &mset) == 3); /* `m` allocates */
        CHECK(ms && mc && mset && strcmp(ms, "alloc") == 0 && memcmp(mc, "bc", 2) == 0 && strcmp(mset, "xyx") == 0);
        free(ms);
        free(mc);
        free(mset);
        CHECK(__isoc23_sscanf("101", "%b", &i) == 1 && i == 5);                  /* C23 %b */
        CHECK(sscanf("", "%d", &i) == EOF);                                     /* input failure first */
        CHECK(sscanf("   ", "%d", &i) == EOF);
        CHECK(sscanf("x", "%d", &i) == 0);                                      /* a matching failure */
    }
    end();

    begin("s12_7_g_0");
    {
        FILE *f = tmpfile();
        fputs("12x 1e;", f);
        rewind(f);
        int n = 0;
        double d = -1;
        CHECK(fscanf(f, "%d", &n) == 1 && n == 12);
        CHECK(fgetc(f) == 'x');                       /* the mismatching character is left unread */
        CHECK(fscanf(f, "%lf", &d) == 0 && d == -1);  /* 1e is no number (D-12.13) */
        CHECK(fgetc(f) == ';');                       /* the prefix was consumed */
        fclose(f);
    }
    end();

    begin("s12_7_f_0");
    {
        /* Liella stores the clamped value without an error (D-12.6). */
        int n = 0;
        signed char c = 0;
        CHECK(sscanf("99999999999", "%d", &n) == 1 && n == INT_MAX);
        CHECK(sscanf("-99999999999", "%d", &n) == 1 && n == INT_MIN);
        CHECK(sscanf("300", "%hhd", &c) == 1 && c == 127);
    }
    end();

    begin("s12_7_f_1");
    {
        /* The plain symbols: %a allocates (GNU, the pre-C99 meaning). */
        char *p = NULL;
        CHECK(plain_sscanf("xy", "%as", &p) == 1 && p != NULL && strcmp(p, "xy") == 0);
        free(p);
    }
    end();

    begin("s12_7_f_2");
    {
        /* __isoc99_: %a is a floating conversion; %i reads no 0b prefix. */
        float x = 0;
        int i = -1;
        CHECK(c99_sscanf("0x1p3", "%a", &x) == 1 && x == 8.0f);
        CHECK(c99_sscanf("0b101", "%i", &i) == 1 && i == 0);
    }
    end();

    begin("s12_7_f_3");
    {
        /* __isoc23_: %i and %b accept a 0b prefix. */
        int i = 0;
        unsigned u = 0;
        CHECK(__isoc23_sscanf("0b101", "%i", &i) == 1 && i == 5);
        CHECK(__isoc23_sscanf("0b101", "%b", &u) == 1 && u == 5);
    }
    end();

    return failures ? 1 : 0;
}
