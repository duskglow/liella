/* No spec clause: a fixture of `cargo xtask affected` (D-X.27), not a check of the library.
 *
 * It passes (exit 0, no output) unless LIELLA_CONF_REPORT_FIXTURE is set in its environment. Then it
 * writes 45 numbered lines to standard output and a marker line to standard error and exits with
 * status 3, so a run of `LIELLA_CONF_REPORT_FIXTURE=1 cargo xtask affected --only
 * conformance:x_affected_report --run` shows that a failed program's status and the tails of its
 * output reach the console (the plant in handoff/XTASK-AFFECTED-CONFORMANCE-OUTPUT-plants.txt). */
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    if (getenv("LIELLA_CONF_REPORT_FIXTURE") == NULL) {
        return 0;
    }
    for (int i = 1; i <= 45; i++) {
        printf("fixture stdout line %d\n", i);
    }
    fprintf(stderr, "FIXTURE-STDERR-MARKER: the check this program stands for failed\n");
    return 3;
}
