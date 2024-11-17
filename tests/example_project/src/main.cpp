#include <fstream>
#include <iostream>

template <int n>
void print()
{
    std::cout << n << std::endl;
    std::ofstream file("output.txt");
    file << n << std::endl;
}

int main()
{
#ifndef PROFILING
    print<42>();
#else
    print<{{default N 42}}>();
#endif
    return 0;
}