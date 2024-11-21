# parabuild-rust

[![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/panjd123/parabuild-rust/ci.yml?style=flat-square&logo=github)](https://github.com/panjd123/parabuild-rust/actions)
[![Crate informations](https://img.shields.io/crates/v/parabuild.svg?style=flat-square)](https://crates.io/crates/parabuild)
[![Crates.io MSRV](https://img.shields.io/crates/msrv/parabuild?style=flat-square)](https://crates.io/crates/parabuild)
[![License](https://img.shields.io/crates/l/parabuild.svg?style=flat-square)](https://github.com/panjd123/parabuild-rust#license)
[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square)](https://docs.rs/parabuild)

Parabuild is a Rust tool that helps you compile complex (single file) projects in parallel, such as some C++/CUDA projects that heavily use templates (when you cannot achieve the best performance through `make -j`).

## Quick Start

The following is an example of how to use parabuild-rust to compile a C++ project.

We suggest that you install `lsof` and `rsync`.

```
sudo apt update
sudo apt install -y lsof rsync
```

We use [handlebars templating language](https://handlebarsjs.com/) to generate source file, here is an example:

```cpp
#include <iostream>

template <int n>
void print(){
    std::cout << n << std::endl;
}

int main(){
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

For more advanced usage, please refer to the [documentation](https://docs.rs/parabuild) and [complete example](examples/complete_usage.rs).

## Command Line

We also provide a command line tool to compile the project. You can use `cargo install parabuild` to install it.

### Simple Example

```shell
parabuild \
    tests/example_cmake_project \
    src/main.cpp \
    build/main \
    --data '[{"N": 10}, {"N": 20}]'
```

### Help

```shell
$ parabuild --help
A parallel build utility for template heavy projects.

Usage: parabuild [OPTIONS] <PROJECT_PATH> <TEMPLATE_FILE> [TARGET_FILES]...

Arguments:
  <PROJECT_PATH>     project path
  <TEMPLATE_FILE>    template file in the project
  [TARGET_FILES]...  target files in the project, which will be moved between build/run workspaces for further processing

Options:
  -w, --workspaces-path <WORKSPACES_PATH>
          where to store the workspaces, executables, etc [default: workspaces]
      --data <DATA>
          json format data
  -d, --data-file <DATA_FILE>
          json format data file, when used together with the `--data` option, ignore this option
  -o, --output-file <OUTPUT_FILE>
          output the json format result to a file, default to stdout
      --init-bash-script <INIT_BASH_SCRIPT>
          init bash script
      --init-bash-script-file <INIT_BASH_SCRIPT_FILE>
          init bash script file, when used together with the `--init-bash-script` option, ignore this option
  -i, --init-cmake-args <INIT_CMAKE_ARGS>
          init cmake args, when used together with the `--init-bash-script` or `--init-bash-script-file` option, ignore this option
      --compile-bash-script <COMPILE_BASH_SCRIPT>
          compile bash script
      --compile-bash-script-file <COMPILE_BASH_SCRIPT_FILE>
          compile bash script file, when used together with the `--compile-bash-script` option, ignore this option
  -m, --make-target <MAKE_TARGET>
          make target, when used together with the `--compile-bash-script` or `--compile-bash-script-file` option, ignore this option
      --run-bash-script <RUN_BASH_SCRIPT>
          run bash script
      --run-bash-script-file <RUN_BASH_SCRIPT_FILE>
          run bash script file when used together with the `--run-bash-script` option, ignore this option
  -s, --silent
          do not show progress bar
  -j, --build-workers <BUILD_WORKERS>
          build workers
  -J, --run-workers <RUN_WORKERS>
          run workers
      --seperate-template
          seperate template file, as opposed to using the same file to render in place
      --no-cache
          Clear the contents in `workspaces` before running
      --without-rsync
          do not use rsync, which means you will not be able to use incremental replication, which may require you to use `--no-cache` every time you modify the project
  -h, --help
          Print help (see more with '--help')
  -V, --version
          Print version
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
