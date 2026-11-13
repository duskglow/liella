/* wl_percall setlocale rows (SETLOCALE-POSIX-ATTR, SETLOCALE-C-ROWS, SETLOCALE-CTYPE-ENV): repeated calls that change
 * nothing. The harness runs every case in the --locale given (default en_US.UTF-8, through LC_ALL) after
 * setlocale(LC_ALL, ""), so the "" rows re-select the locale already in force. */
#include "percall.h"
#include <locale.h>

CASE(setlocale_ctype_posix, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_CTYPE, "POSIX");
    PC_CHECKING pc_check("%s", setlocale(LC_CTYPE, "POSIX"));
}
CASE(setlocale_ctype_c, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_CTYPE, "C");
    PC_CHECKING pc_check("%s", setlocale(LC_CTYPE, "C"));
}
CASE(setlocale_all_c, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, "C");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, "C"));
}
CASE(setlocale_all_posix, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, "POSIX");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, "POSIX"));
}
CASE(setlocale_query, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, NULL);
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, NULL));
}
CASE(setlocale_ctype_empty, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_CTYPE, "");
    PC_CHECKING pc_check("%s", setlocale(LC_CTYPE, ""));
}
CASE(setlocale_all_empty, 10000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, "");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, ""));
}
