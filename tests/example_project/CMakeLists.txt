cmake_minimum_required(VERSION 3.10)

project(ExampleProject)

set(CMAKE_CXX_STANDARD 11)

if (PROFILING STREQUAL "ON")
    add_definitions(-DPROFILING=ON)
endif()

add_executable(main src/main.cpp)