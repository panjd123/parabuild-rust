#include <iostream>

#ifndef N
#define N 42
#endif

template <int n>
void print() {
    std::cout << n << std::endl;
}

int main() {
    print<N>();
    return 0;
}