cmake_minimum_required(VERSION 3.12)

project(ExampleGPUTest)

set(CMAKE_CXX_STANDARD 11)

if (PARABUILD STREQUAL "ON")
    add_compile_definitions(PARABUILD=ON)
endif()

add_executable(main src/main.cpp)
