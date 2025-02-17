<h1 align="center">
<img src="imgs/logo.png" width="200">
</h1><br>


[![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/panjd123/parabuild-rust/ci.yml?style=flat-square&logo=github)](https://github.com/panjd123/parabuild-rust/actions)
[![Crate informations](https://img.shields.io/crates/v/parabuild.svg?style=flat-square)](https://crates.io/crates/parabuild)
[![Crates.io MSRV](https://img.shields.io/crates/msrv/parabuild?style=flat-square)](https://crates.io/crates/parabuild)
[![License](https://img.shields.io/crates/l/parabuild.svg?style=flat-square)](https://github.com/panjd123/parabuild-rust#license)
[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square)](https://docs.rs/parabuild)

[English](README.md) | [简体中文](README_zh.md)

Parabuild 是一个用 Rust 编写的工具，它可以帮助你在需要编译多份不同编译期参数的 C++/CUDA 项目时，以并行的方式编译这些项目并执行，常见的情况是一个大量使用模板的单文件项目，你需要尝试多组模板参数时，`make -j` 无法达到最佳性能，这时候你可以使用 Parabuild 来发挥多核 CPU 的性能（还支持多 GPU，比如 MIG 或多卡），通常可以得到 10x 以上的性能提升。

Parabuild 同时提供了命令行工具和对应的 Rust 库，你可以根据需要使用，本 README 主要介绍命令行工具的使用方法。

![parabuild](./imgs/parabuild.gif)

## 命令行工具快速开始

以下是使用 `parabuild` 加速一个 C++ 项目调试的示例。

如果你是第一次接触 Rust，你可以通过以下命令安装 Rust：

```shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

然后你可以通过以下命令安装 parabuild-rust：

```shell
cargo install parabuild
```

你还需要安装 `lsof` 和 `rsync`，这两个工具是 parabuild-rust 所需要的：

```
sudo apt update
sudo apt install -y lsof rsync
```

我们使用 [handlebars 模板语言](https://handlebarsjs.com/) 来生成源文件，这里是一个示例：

```cpp
// src/main.cpp

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

我们将使用这个文件来组织一个 C++ 项目，目录结构如下：

```shell
example_project
├── CMakeLists.txt
├── src
│   └── main.cpp
```

假设我们想要使用不同的 `N` 值编译这个项目，我们可以使用以下命令，你可以使用 `xxx-bash-script` 参数来指定在工作区初始化、编译和运行时分别需要执行的内容：

```shell
parabuild \
    example_project \
    build/main \
    --init-bash-script "cmake -S . -B build" \
    --compile-bash-script "cmake --build build -- -B" \
    --run-bash-script "./build/main" \
    --template-file src/main.cpp \
    --data '[{"N": 10}, {"N": 20}]' \
    -j 1
```

你可以使用源码仓库中的示例项目来尝试这个过程：

```shell
git clone https://github.com/panjd123/parabuild-rust.git

cd parabuild-rust

cargo run -- \
    tests/example_cmake_project \
    build/main \
    --template-file src/main.cpp \
    --data '[{"N": 10}, {"N": 20}]'
```

命令行工具还提供了大量的自定义参数选项，请查看 `parabuild --help`。

### 高级用法

对于更高级的用法，请参考[文档](https://docs.rs/parabuild)和[完整示例](examples/complete_usage.rs)。

文档中提供了最佳实践（如何避免管理两份代码，一份用于 parabuild，一份用于正常开发），[`examples`](examples) 文件夹里还有更多的示例。

### Help

```shell
$ parabuild --help
A parallel build utility for template heavy projects.

Usage: parabuild [OPTIONS] <PROJECT_PATH> [TARGET_FILES]...

Arguments:
  <PROJECT_PATH>
          project path

  [TARGET_FILES]...
          target files in the project, which will be moved between build/run workspaces for further processing
          
          e.g. `build/main,data_generate_when_build`

Options:
  -t, --template-file <TEMPLATE_FILE>
          template file in the project

  -w, --workspaces-path <WORKSPACES_PATH>
          where to store the workspaces, executables, etc
          
          [default: .parabuild/workspaces]

      --data <DATA>
          json format data

  -d, --data-file <DATA_FILE>
          json format data file, when used together with the `--data` option, ignore this option

  -o, --output-file <OUTPUT_FILE>
          output the json format result to a file, default to stdout

      --init-bash-script <INIT_BASH_SCRIPT>
          init bash script
          
          Default to `cmake -S . -B build -DPARABUILD=ON`

      --init-bash-script-file <INIT_BASH_SCRIPT_FILE>
          init bash script file, when used together with the `--init-bash-script` option, ignore this option

  -i, --init-cmake-args <INIT_CMAKE_ARGS>
          init cmake args, when used together with the `--init-bash-script` or `--init-bash-script-file` option, ignore this option
          
          e.g. "-DCMAKE_BUILD_TYPE=Release"

      --compile-bash-script <COMPILE_BASH_SCRIPT>
          compile bash script
          
          Default to `cmake --build build --target all -- -B`

      --compile-bash-script-file <COMPILE_BASH_SCRIPT_FILE>
          compile bash script file, when used together with the `--compile-bash-script` option, ignore this option

  -m, --make-target <MAKE_TARGET>
          make target, when used together with the `--compile-bash-script` or `--compile-bash-script-file` option, ignore this option

      --run-bash-script <RUN_BASH_SCRIPT>
          run bash script
          
          If not provided, we will run the first target file in the `target_files` directly

      --run-bash-script-file <RUN_BASH_SCRIPT_FILE>
          run bash script file when used together with the `--run-bash-script` option, ignore this option

  -s, --silent
          do not show progress bar

  -j, --build-workers <BUILD_WORKERS>
          build workers

  -J, --run-workers <RUN_WORKERS>
          run workers
          
          We have four execution modes:
          
          1. separate and parallel
          
          2. separate and serial (by default)
          
          3. execute immediately in place
          
          4. do not execute, only compile, move all the TARGET_FILES to `workspaces/targets`
          
          The first one means we will move TARGET_FILES between build/run workspaces. Compile and run in parallel in different places like a pipeline.
          
          The second behavior is similar to the first, but the difference is that we only start running after all the compilation work is completed.
          
          The third method is quite unique, as it does not move the TARGET_FILES and immediately executes the compilation of a workspace in its original location.
          
          To specify these three working modes through the command line:
          
          1. positive numbers represent the first
          
          2. negative numbers represent the second
          
          3. pass `--run-in-place` to represent the third, we will ignore the value of this option
          
          4. 0 represent the fourth

      --run-in-place
          run in place, which means we will not move the TARGET_FILES between build/run workspaces

      --seperate-template
          seperate template file, as opposed to using the same file to render in place

      --no-cache
          Clear the contents in `workspaces` before running

      --without-rsync
          do not use rsync, which means you will not be able to use incremental replication, which may require you to use `--no-cache` every time you modify the project

      --makefile
          Mark that you are actually working on a makefile project
          
          pass `data` to `CPPFLAGS` environment variable in the compile bash script
          
          e.g. when data is `{"N": 10}`, `CPPFLAGS=-DN=10`

      --panic-on-compile-error
          panic on compile error

      --format-output
          format the output when printing to stdout (only valid when `--output-file` is not provided)

      --no-init
          do not run the init bash script, same as `--init-bash-script ""`

      --continue [<CONTINUE_FROM>]
          continue from which under the `autosave_dir`
          
          e.g. `2021-08-01_12-00-00`
          
          left empty to start from the latest one (--continue)

      --autosave <AUTOSAVE_INTERVAL>
          [default: 30m]

      --autosave-dir <AUTOSAVE_DIR>
          [default: .parabuild/autosave]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## License

本项目使用 MPL-2.0 许可证，详细信息请查看 [LICENSE](LICENSE) 文件。