/* Spec §1.2: exit runs atexit, on_exit and __cxa_atexit handlers in reverse order of
 * registration, including handlers registered during exit, then the static destructors; at
 * least 32 registrations succeed. The destructor (run last) checks the order and ends the
 * process with _exit: 0 if every handler ran as specified. */
#define _DEFAULT_SOURCE
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern int __cxa_atexit(void (*fn)(void *), void *arg, void *dso);

static char order[16];
static int n;
static int counted;
static int nine = 9;
static char letter = 'c';

static void mark(char c) { order[n++] = c; }
static void count(void) { counted++; }
static void a(void) { mark('a'); }
static void b(int status, void *arg) { mark(status == 3 && *(int *)arg == 9 ? 'b' : 'X'); }
static void c(void *arg) { mark(*(char *)arg); }
static void late(void) { mark('l'); }
static void registers_late(void) { mark('r'); atexit(late); }

__attribute__((destructor)) static void last(void)
{
    mark('d');
    order[n] = 0;
    _exit(strcmp(order, "rlcbad") == 0 && counted == 40 ? 0 : 100 + n);
}

int main(void)
{
    for (int i = 0; i < 40; i++)
        if (atexit(count) != 0)
            return 1;
    if (atexit(a) != 0)
        return 2;
    if (on_exit(b, &nine) != 0)
        return 3;
    if (__cxa_atexit(c, &letter, NULL) != 0)
        return 4;
    if (atexit(registers_late) != 0)
        return 5;
    exit(3);
}
