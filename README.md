# parabuild-rust

This is a Rust tool that helps you compile complex (single file) projects in parallel, such as some C++ projects that heavily use templates (when you cannot achieve the best performance through `make -j`).

## Quick Start

The following is an example of how to use parabuild-rust to compile a C++ project.

We use [handlebars templating language](https://handlebarsjs.com/) to generate source file, here is an example:

```cpp
#include <iostream>

template <int n>
void print()
{
    std::cout << n << std::endl;
}

int main()
{
    print<{{N}}>();
    return 0;
}
```

Main body:

```rust
use parabuild::Parabuilder;
use serde_json::{json, to_string_pretty, Value as JsonValue};

fn main() {
    let project_path = "tests/example_cmake_project"; // your project path
    let workspaces_path = "workspaces"; // where to store the workspaces, executables, etc.
    let template_path = "src/main.cpp.template"; // template file in the project
    let target_executable_file = "build/main"; // target executable file
    let datas = vec![json!({"N": "10"}), json!({"N": "20"})];
    let mut parabuilder = Parabuilder::new(
        project_path,
        workspaces_path,
        template_path,
        &[target_executable_file],
    );
    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    let (run_data, _compile_error_datas): (JsonValue, Vec<JsonValue>) = parabuilder.run().unwrap();
    println!("{}", to_string_pretty(&run_data).unwrap());
    /*
    [
        {
            "data": {
                "N": "10"
            },
            "status": 0,
            "stderr": "",
            "stdout": "10\n"
        },
        {
            "data": {
                "N": "20"
            },
            "status": 0,
            "stderr": "",
            "stdout": "20\n"
        }
    ]
     */
}
```

We return `compute_error_datas` to indicate the data with compilation errors. Compilation errors are common in debugging projects that heavily use templates.

### Advanced Usage

Parabuild use

```shell
cmake -B build -S . -DPARABUILD=ON
```

and

```shell
cmake --build build --target all -- -B
```

as the default workspace initialization script and the script to be compiled each time.

## Command Line

We also provide a command line tool to compile the project. You can use `cargo install parabuild` to install it.

### Help

```shell
$ parabuild --help
A parallel build utility for template heavy projects.

Usage: parabuild [OPTIONS] <PROJECT_PATH> <TEMPLATE_PATH> <TARGET_EXECUTABLE_FILE>

Arguments:
  <PROJECT_PATH>            project path
  <TEMPLATE_PATH>           template file in the project
  <TARGET_EXECUTABLE_FILE>  target executable file in the project

Options:
  -w, --workspaces-path <WORKSPACES_PATH>
          where to store the workspaces, executables, etc [default: workspaces]
      --data <DATA>
          json format data
  -d, --data-file <DATA_FILE>
          json format data file, when used together with the `--data` option, ignore this option
  -o, --output-file <OUTPUT_FILE>
          output the json format result to a file, default to stdout
      --init-bash-script-file <INIT_BASH_SCRIPT_FILE>
          init bash script file
  -i, --init-cmake-args <INIT_CMAKE_ARGS>
          init cmake args e.g. "-DCMAKE_BUILD_TYPE=Release", when used together with the `--init-bash-script-file` option, ignore this option
      --compile-bash-script-file <COMPILE_BASH_SCRIPT_FILE>
          compile bash script file
  -t, --target <TARGET>
          make target, when used together with the `--compile-bash-script-file` option, ignore this option
  -p, --progress-bar
          enable progress bar
  -j, --build-workers <BUILD_WORKERS>
          build workers
  -J, --run-workers <RUN_WORKERS>
          run workers
      --in-place-template
          in place template
  -h, --help
          Print help
  -V, --version
          Print version
```

## Best Practices

We mainly share how to make your normal work compatible with parabuild and avoid maintaining two sets of code at the same time.

### CMake-project

You need to define a macro to use normal code when not parabuild.

`CMakelists.txt`:

```CMakeLists.txt
cmake_minimum_required(VERSION 3.26)

project(ExampleProject)

set(CMAKE_CXX_STANDARD 11)

if (PARABUILD STREQUAL "ON")
    add_compile_definitions(PARABUILD=ON)
endif()

add_executable(main src/main.cpp)
```

`main.cpp`:

```cpp
#include <iostream>

template <int n>
void print()
{
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
```

run script:

```shell
parabuild \
    tests/example_cmake_project \
    src/main.cpp \
    build/main \
    --in-place-template \
    --data '[{"N": 10}, {"N": 20}]'
```

output:

```shell
[
  {
    "data": {
      "N": 10
    },
    "status": 0,
    "stderr": "",
    "stdout": "10\n"
  },
  {
    "data": {
      "N": 20
    },
    "status": 0,
    "stderr": "",
    "stdout": "20\n"
  }
]
```

## Features

- Use handlebars template language to generate source file.
- Ignore `.gitignore` files in the project, which may speed up the copying process.
- Support multi-threading compilation/executing, these two parts can share threads, meaning they can be executed immediately after compilation, or they can be separated. For example, four threads can be used for compilation and one thread for execution. This is suitable for scenarios where only one executable file should be active in the system, such as when testing GPU performance. In this case, multiple CPU threads compile in the background while one CPU thread is responsible for execution.
- TODO: Support better `force exclusive run`, which means only one executable thread is running, no compilation thread is running.
- TODO: Support multiple template files.

## Notes

Due to the fact that system time is not monotonous , when the program executes quickly, there may be older timestamps in subsequent file modifications, which may cause make to not be able to track program modifications correctly. Please be aware that when writing compilation scripts, try to forcefully ignore timestamp compilation.

https://doc.rust-lang.org/std/time/struct.SystemTime.html

> A measurement of the system clock, useful for talking to external entities like the file system or other processes.
>
> Distinct from the Instant type, this time measurement is not monotonic. This means that you can save a file to the file system, then save another file to the file system, and the second file has a SystemTime measurement earlier than the first. In other words, an operation that happens after another operation in real time may have an earlier SystemTime!

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
