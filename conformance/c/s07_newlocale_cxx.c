/* C++17 source (LINK-CXX). Spec §7.3 (s7_3_c_1, s7_3_f_0; NEWLOCALE-LC-ALL-BIT, probes in
 * spec/behaviour/newlocale-mask.md): the C++ library makes its locale objects through
 * newlocale with the mask 1 << LC_ALL (every category) and the name "C", "" or a locale's name.
 * Checked: iostreams start and write; std::locale("C"), std::locale("") (the environment's
 * locale, a mixed one here) and std::locale("C.UTF-8") are made and named; numbers written
 * through a stream imbued with each; the ctype facet of the environment's locale; the call
 * newlocale(1 << LC_ALL, "C", 0) itself; an unknown name still throws std::runtime_error.
 * The same program passes against the reference library (handoff/NEWLOCALE-LC-ALL-BIT.md).
 * A failing CHECK exits with its line number. */
/* LINK-CXX: yes */
/* LINK-LIBM: yes */
/* LINK-UNWIND: yes */
/* RUN-ENV: LANG=C.UTF-8 LC_NUMERIC=C */
#include <locale.h>

#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <locale>
#include <sstream>
#include <stdexcept>
#include <string>

#define CHECK(c)                                                                          \
    do {                                                                                  \
        if (!(c)) {                                                                       \
            std::fprintf(stderr, "s07_newlocale_cxx: line %d: %s\n", __LINE__, #c);       \
            std::exit(__LINE__ % 250 + 1);                                                \
        }                                                                                 \
    } while (0)

static std::string written(const std::locale &l) {
    std::ostringstream os;
    os.imbue(l);
    os << 1234567 << ' ' << 2.5 << ' ' << -42L;
    return os.str();
}

int main() {
    std::cout << "s07_newlocale_cxx: iostreams started" << std::endl;
    CHECK(std::cout.good());

    std::locale c("C");
    CHECK(c.name() == "C");
    CHECK(written(c) == "1234567 2.5 -42");

    // LANG=C.UTF-8 with LC_NUMERIC=C: every category but one from LANG.
    std::locale env("");
    std::string n = env.name();
    CHECK(n.find("C.UTF-8") != std::string::npos);
    CHECK(written(env) == "1234567 2.5 -42");
    CHECK(std::use_facet<std::ctype<char>>(env).toupper('q') == 'Q');

    std::locale u("C.UTF-8");
    CHECK(u.name() == "C.UTF-8");
    CHECK(written(u) == "1234567 2.5 -42");

    std::cout.imbue(env);
    std::cout << "s07_newlocale_cxx: " << 42 << ' ' << 0.5 << std::endl;
    CHECK(std::cout.good());

    // The call as the C++ library makes it: every category from "C".
    locale_t l = newlocale(1 << LC_ALL, "C", (locale_t)0);
    CHECK(l != (locale_t)0);
    freelocale(l);

    bool threw = false;
    try {
        std::locale bad("xx_YY.NOPE");
    } catch (const std::runtime_error &) {
        threw = true;
    }
    CHECK(threw);

    std::cout << "s07_newlocale_cxx: ok" << std::endl;
    return 0;
}
