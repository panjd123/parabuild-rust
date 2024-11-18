#include <iostream>
#include <unistd.h>

template <int n>
void print()
{
    std::cout << "Sleeping for 0.3 second" << std::endl;
    usleep(300000);
    std::cout << n << std::endl;
}

int main()
{
#ifndef PARABUILD
    print<42>();
#else
    print<{{default N 42}}>();
#endif
    return 0;
}