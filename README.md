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
use serde_json::{json, Value as JsonValue};

fn main() {
    let project_path = "tests/example_project"; // your project path
    let workspaces_path = "workspaces"; // where to store the workspaces, executables, etc.
    let template_path = "src/main.cpp.template"; // template file in the project
    let build_path = "build/main"; // target executable file
    let datas = vec![json!({"N": "10"}), json!({"N": "20"})];
    let mut parabuilder =
        Parabuilder::new(project_path, workspaces_path, template_path, build_path);
    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    let run_data: JsonValue = parabuilder.run().unwrap();
    println!("{:?}", run_data);
    // Array [Object {"data": Object {"N": String("10")}, "stdout": String("10\n")}, Object {"data": Object {"N": String("20")}, "stdout": String("20\n")}]
}
```

## Features

- Use handlebars template language to generate source file.
- Ignore `.gitignore` files in the project, which may speed up the copying process.
- Support multi-threading compilation/executing, these two parts can share threads, meaning they can be executed immediately after compilation, or they can be separated. For example, four threads can be used for compilation and one thread for execution. This is suitable for scenarios where only one executable file should be active in the system, such as when testing GPU performance. In this case, multiple CPU threads compile in the background while one CPU thread is responsible for execution.
- TODO: Support `force exclusive run`, which means only one executable thread is running, no compilation thread is running.
- TODO: Support multiple template files.

## Notes

Due to the fact that system time is not monotonous , when the program executes quickly, there may be older timestamps in subsequent file modifications, which may cause make to not be able to track program modifications correctly. Please be aware that when writing compilation scripts, try to forcefully ignore timestamp compilation.

https://doc.rust-lang.org/std/time/struct.SystemTime.html

> A measurement of the system clock, useful for talking to external entities like the file system or other processes.
>
> Distinct from the Instant type, this time measurement is not monotonic. This means that you can save a file to the file system, then save another file to the file system, and the second file has a SystemTime measurement earlier than the first. In other words, an operation that happens after another operation in real time may have an earlier SystemTime!