#include <iostream>

#define N 42

template <int n>
void print() {
    std::cout << n << std::endl;
}

int main() {
    print<N>();
    return 0;
}