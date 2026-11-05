/* PERF-FNMATCH-BRACKET: fnmatch with bracket expressions in the C locale, per call. */
#include "percall.h"
#include <fnmatch.h>
#include <string.h>

CASE(fnmatch_bracket, 100000, 1) {
    for (long i = 0; i < n; i++) pc_sink += fnmatch("[a-m]*_[!0-9]*.[ch]", "main_helper_x.c", 0);
    PC_CHECKING { pc_check("%d %d", fnmatch("[a-m]*_[!0-9]*.[ch]", "main_helper_x.c", 0), fnmatch("[a-m]*_[!0-9]*.[ch]", "zmain_1.c", 0)); }
}

CASE(fnmatch_range_only, 100000, 1) {
    for (long i = 0; i < n; i++) pc_sink += fnmatch("[a-mA-M0-5][!x-z]", "k7", 0);
    PC_CHECKING { pc_check("%d %d", fnmatch("[a-mA-M0-5][!x-z]", "k7", 0), fnmatch("[a-mA-M0-5][!x-z]", "kz", 0)); }
}

CASE(fnmatch_star_only, 100000, 1) {
    for (long i = 0; i < n; i++) pc_sink += fnmatch("*_*.c", "main_helper_x.c", 0);
    PC_CHECKING { pc_check("%d %d", fnmatch("*_*.c", "main_helper_x.c", 0), fnmatch("*_*.c", "mainhelper.c", 0)); }
}
