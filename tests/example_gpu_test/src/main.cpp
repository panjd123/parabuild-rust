#include <iostream>
#include <cstdlib>

int main()
{
    const char* cuda_visible_devices = std::getenv("CUDA_VISIBLE_DEVICES");
    const char* parabuild_id = std::getenv("PARABUILD_ID");

    std::cout << "PARABUILD_ID=" << (parabuild_id ? parabuild_id : "NOT_SET") << std::endl;
    std::cout << "CUDA_VISIBLE_DEVICES=" << (cuda_visible_devices ? cuda_visible_devices : "NOT_SET") << std::endl;

    return 0;
}
