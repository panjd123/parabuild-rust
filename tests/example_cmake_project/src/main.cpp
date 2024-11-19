#include <fstream>
#include <iostream>

template <int n>
void print(int id = 0)
{
    std::cout << n << std::endl;
    std::ofstream file(std::to_string(id) + ".txt");
    if (!file.is_open())
    {
        std::cerr << "Failed to open file" << std::endl;
        return;
    }
    file << n << std::endl;
}

int main(int argc, char *argv[])
{
    int id = 0;
    if (argc > 1)
    {
        id = std::stoi(argv[1]);
    }
#ifndef PARABUILD
    print<42>(id);
#else
    print<{{default N 42}}>(id);
#endif
    return 0;
}