/* Spec §1.2: quick_exit runs only the at_quick_exit handlers, most recent first, then _Exit;
 * no atexit handler and no destructor runs. */
/* EXPECT-EXIT: 5 */
#include <stdlib.h>
#include <string.h>

static char order[8];
static int n;

static void q1(void)
{
    order[n++] = '1';
    order[n] = 0;
    _Exit(strcmp(order, "21") == 0 ? 5 : 60);
}
static void q2(void) { order[n++] = '2'; }
static void never(void) { _Exit(70); }
__attribute__((destructor)) static void dtor(void) { _Exit(71); }

int main(void)
{
    if (at_quick_exit(q1) != 0 || at_quick_exit(q2) != 0 || atexit(never) != 0)
        return 1;
    quick_exit(9);
}
