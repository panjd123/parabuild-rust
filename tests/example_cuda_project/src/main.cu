#include <iostream>
#include <cuda_runtime.h>

__global__ void sleep_kernel(int64_t num_cycles, int64_t clock_rate) {
    int64_t start = clock64();
    int64_t next_tick = start + clock_rate;
    while (clock64() - start < num_cycles) {
        if (clock64() >= next_tick) {
            next_tick += clock_rate;
            printf("Slept for %f seconds\n", (clock64() - start) / static_cast<float>(clock_rate));
        }
        // sleep
    }
}

int main() {
    int device_count;
    cudaGetDeviceCount(&device_count);
    std::cout << "Number of CUDA devices: " << device_count << std::endl;

    cudaDeviceProp prop;
    cudaGetDeviceProperties(&prop, 0);
    int64_t clock_rate = prop.clockRate * 1000;
    float sleep_seconds = 10.0f;
    int64_t num_cycles = static_cast<int64_t>(sleep_seconds * clock_rate);

    sleep_kernel<<<1, 1>>>(num_cycles, clock_rate);
    cudaDeviceSynchronize();
    
    std::cout << "Slept for " << sleep_seconds << " seconds" << std::endl;
    return 0;
}