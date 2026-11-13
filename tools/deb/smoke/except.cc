// Smoke program for the reference packages: a C++ exception thrown through two frames and caught,
// plus a std::string and iostream output.
#include <iostream>
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
        std::cout << "except: caught: " << e.what() << std::endl;
        return 0;
    }
    std::cout << "except: not thrown" << std::endl;
    return 1;
}
