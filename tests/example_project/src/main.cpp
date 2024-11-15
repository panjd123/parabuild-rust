#include <iostream>
#include <fstream>

template <int n>
void print()
{
    std::cout << n << std::endl;
    std::ofstream file("output.txt");
    file << n << std::endl;
}

int main()
{
    print<42>();
    return 0;
}