# parabuild-rust

This is a Rust tool that helps you compile complex (single file) projects in parallel, such as some C++ projects that heavily use templates (when you cannot achieve the best performance through make - j).

## Quick Start

The following is an example of how to use parabuild-rust to compile a C++ project.

We use [handlebars templating language](https://handlebarsjs.com/) to generate source file, here is an example:

```cpp
#include <iostream>

#define N {{N}}

template <int n>
void print() {
    std::cout << n << std::endl;
}

int main() {
    print<N>();
    return 0;
}
```

Main body:

```rust
use parabuild::Parabuilder;

fn main() {
    let project_path = "tests/example_project";     // your project path
    let workspaces_path = "workspaces";             // where to store the workspaces, executables, etc.
    let template_path = "src/main.cpp.template";    // template file in the project
    let build_path = "build/main";                  // target executable file
    let build_command = r#"
    cmake -B build -S .
    "#;
    let run_command = r#"
    cmake --build build --target all
    "#;
    let thread_num = 1;
    let mut parabuilder = Parabuilder::new(
        project_path,
        workspaces_path,
        template_path,
        build_path,
        build_command,
        run_command,
        thread_num,
    );
    let datas = vec![
        serde_json::json!({"N": "10"}),
        serde_json::json!({"N": "20"}),
    ];
    parabuilder.add_datas(datas);
    parabuilder.init_workspace().unwrap();
    parabuilder.run().unwrap();
    println!("Check the executable files in workspaces/executable");
    // std::fs::remove_dir_all("workspaces").unwrap();
}
```

Then you can find `main_0`, `main_0.json`, `main_1`, `main_1.json` in the `workspaces/executable` directory, which are the executables and the corresponding json data files.

## Features

- Use handlebars template language to generate source file.
- Ignore `.gitignore` files in the project, which may speed up the copying process.
- TODO: Support multi-threading.
- TODO: Support multiple template files.