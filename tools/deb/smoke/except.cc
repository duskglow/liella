// Smoke program for the reference packages: a C++ exception thrown through two frames and caught.
// Output through printf: iostream is the separate smoke iostream.cc.
#include <cstdio>
#include <stdexcept>
#include <string>

static int depth2(int x) {
    if (x > 1) {
        throw std::runtime_error("thrown at depth " + std::to_string(x));
    }
    return x;
}

static int depth1(int x) { return depth2(x + 1) + 1; }

int main() {
    try {
        depth1(1);
    } catch (const std::runtime_error &e) {
        std::printf("except: caught: %s\n", e.what());
        return 0;
    }
    std::printf("except: not thrown\n");
    return 1;
}
