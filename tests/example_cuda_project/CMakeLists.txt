cmake_minimum_required(VERSION 3.12)

project(ExampleProject LANGUAGES CXX CUDA)

set(CMAKE_CUDA_STANDARD 11)

set(CMAKE_CUDA_ARCHITECTURES 80) # A100

if (PARABUILD STREQUAL "ON")
    add_compile_definitions(PARABUILD=ON)
endif()

add_executable(main src/main.cu)