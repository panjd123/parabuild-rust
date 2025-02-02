#include <fstream>
#include <iostream>

template <int n>
void print() {
    int id = 0;
    const char* value = std::getenv("PARABUILD_ID");
    if (value) {
        id = std::stoi(value);
    }
    std::cout << n << std::endl;
    std::ofstream file(std::to_string(id) + ".txt");
    if (!file.is_open()) {
        std::cerr << "Failed to open file" << std::endl;
        return;
    }
    file << n << std::endl;
}

int main() {
#ifndef PARABUILD
    print<42>();
#else
    print<{{default N 42}}>();
#endif
    return 0;
}