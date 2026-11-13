/* wl_percall setlocale rows (SETLOCALE-POSIX-ATTR, SETLOCALE-C-ROWS, SETLOCALE-CTYPE-ENV): repeated calls that change
 * nothing. The harness runs every case in the --locale given (default en_US.UTF-8, through LC_ALL) after
 * setlocale(LC_ALL, ""), so the "" rows re-select the locale already in force. */
#include "percall.h"
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>

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
/* The common environment of a program: LANG only (LC_ALL unset), the harness's locale moved there. */
CASE(setlocale_ctype_empty_lang, 10000, 1) {
    char *l = getenv("LC_ALL"), lang[64];
    snprintf(lang, sizeof lang, "%s", l ? l : "C");
    unsetenv("LC_ALL");
    setenv("LANG", lang, 1);
    setlocale(LC_CTYPE, "");
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_CTYPE, "");
    PC_CHECKING pc_check("%s", setlocale(LC_CTYPE, ""));
    setenv("LC_ALL", lang, 1);
}
CASE(setlocale_all_empty_lang, 10000, 1) {
    char *l = getenv("LC_ALL"), lang[64];
    snprintf(lang, sizeof lang, "%s", l ? l : "C");
    unsetenv("LC_ALL");
    setenv("LANG", lang, 1);
    setlocale(LC_ALL, "");
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, "");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, ""));
    setenv("LC_ALL", lang, 1);
}
/* LC_ALL set but empty, which selects as if unset (POSIX), with LANG holding the locale. */
CASE(setlocale_all_empty_lcall_empty, 10000, 1) {
    char *l = getenv("LC_ALL"), lang[64];
    snprintf(lang, sizeof lang, "%s", l ? l : "C");
    setenv("LC_ALL", "", 1);
    setenv("LANG", lang, 1);
    setlocale(LC_ALL, "");
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, "");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, ""));
    setenv("LC_ALL", lang, 1);
}
