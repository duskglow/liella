// Smoke program for the reference packages: C++ iostream output (the C++ runtime creates its "C"
// locale through newlocale at start).
#include <iostream>

int main() {
    std::cout << "iostream: " << 42 << std::endl;
    return 0;
}
