//! Parabuild is a Rust tool that helps you compile complex (single file) projects in parallel,
//! such as some C++/CUDA projects that heavily use templates (cannot achieve the best
//! performance through `make -j`).
//!
//! # Quick Start
//!
//! The following is an example of how to use parabuild-rust to compile a C++ project.
//!
//! We use [handlebars templating language](https://handlebarsjs.com/) to generate source file, here is an example:
//!
//! ```cpp
//! #include <iostream>
//!
//! template <int n>
//! void print()
//! {
//!     std::cout << n << std::endl;
//! }
//!
//! int main()
//! {
//!     print<{{N}}>();
//!     return 0;
//! }
//! ```
//!
//! Main body:
//!
//! ```rust
//! use parabuild::Parabuilder;
//! use serde_json::{json, to_string_pretty, Value as JsonValue};
//!
//! fn main() {
//!     let project_path = "tests/example_cmake_project"; // your project path
//!     let workspaces_path = "workspaces"; // where to store the workspaces, executables, etc.
//!     let template_path = "src/main.cpp.template"; // template file in the project
//!     let target_executable_file = "build/main"; // target executable file
//!     let datas = vec![json!({"N": "10"}), json!({"N": "20"})];
//!     let mut parabuilder = Parabuilder::new(
//!         project_path,
//!         workspaces_path,
//!         template_path,
//!         &[target_executable_file],
//!     );
//!     parabuilder.set_datas(datas).unwrap();
//!     parabuilder.init_workspace().unwrap();
//!     let (run_data, _compile_error_datas): (JsonValue, Vec<JsonValue>) = parabuilder.run().unwrap();
//!     println!("{}", to_string_pretty(&run_data).unwrap());
//!     /*
//!     [
//!         {
//!             "data": {
//!                 "N": "10"
//!             },
//!             "status": 0,
//!             "stderr": "",
//!             "stdout": "10\n"
//!         },
//!         {
//!             "data": {
//!                 "N": "20"
//!             },
//!             "status": 0,
//!             "stderr": "",
//!             "stdout": "20\n"
//!         }
//!     ]
//!      */
//! }
//! ```
//!
//! We return `compute_error_datas` to indicate the data with compilation errors. Compilation errors are common in debugging projects that heavily use templates.
//!
//! ## Advanced Usage
//！
//! For more advanced usage, please refer to the [documentation](https://docs.rs/parabuild) and [examples/complete_usage.rs](examples/complete_usage.rs).
//!
//! # Best Practices
//!
//! We mainly share how to make your normal work compatible with parabuild and avoid maintaining two sets of code at the same time.
//!
//! ## CMake-project
//!
//! You need to define a macro to use normal code when not parabuild.
//!
//! `CMakelists.txt`:
//!
//! ```CMakeLists.txt
//! cmake_minimum_required(VERSION 3.12)
//!
//! project(ExampleProject)
//!
//! set(CMAKE_CXX_STANDARD 11)
//!
//! if (PARABUILD STREQUAL "ON")
//!     add_compile_definitions(PARABUILD=ON)
//! endif()
//!
//! add_executable(main src/main.cpp)
//! ```
//!
//! `main.cpp`:
//!
//! ```cpp
//! #include <iostream>
//!
//! template <int n>
//! void print()
//! {
//!     std::cout << n << std::endl;
//! }
//!
//! int main()
//! {
//! #ifndef PARABUILD
//!     print<42>();
//! #else
//!     print<{{default N 42}}>();
//! #endif
//!     return 0;
//! }
//! ```
//!
//! run script:
//!
//! ```shell
//! parabuild \
//!     tests/example_cmake_project \
//!     src/main.cpp \
//!     build/main \
//!     --in-place-template \
//!     --data '[{"N": 10}, {"N": 20}]'
//! ```
//!
//! output:
//!
//! ```shell
//! [
//!   {
//!     "data": {
//!       "N": 10
//!     },
//!     "status": 0,
//!     "stderr": "",
//!     "stdout": "10\n"
//!   },
//!   {
//!     "data": {
//!       "N": 20
//!     },
//!     "status": 0,
//!     "stderr": "",
//!     "stdout": "20\n"
//!   }
//! ]
//! ```
//!
//! # Features
//!
//! - Use handlebars template language to generate source file.
//! - Ignore `.gitignore` files in the project, which may speed up the copying process.
//! - Support multi-threading compilation/executing, these two parts can share threads, meaning they can be executed immediately after compilation, or they can be separated. For example, four threads can be used for compilation and one thread for execution. This is suitable for scenarios where only one executable file should be active in the system, such as when testing GPU performance. In this case, multiple CPU threads compile in the background while one CPU thread is responsible for execution.
//! - TODO: Support better `force exclusive run`, which means only one executable thread is running, no compilation thread is running.
//! - TODO: Support multiple template files.
//!
//! # Notes
//!
//! Due to the fact that system time is not monotonous , when the program executes quickly, there may be older timestamps in subsequent file modifications, which may cause make to not be able to track program modifications correctly. Please be aware that when writing compilation scripts, try to forcefully ignore timestamp compilation.
//!
//! [SystemTime](https://doc.rust-lang.org/std/time/struct.SystemTime.html):
//!
//! > A measurement of the system clock, useful for talking to external entities like the file system or other processes.
//! >
//! > Distinct from the Instant type, this time measurement is not monotonic. This means that you can save a file to the file system, then save another file to the file system, and the second file has a SystemTime measurement earlier than the first. In other words, an operation that happens after another operation in real time may have an earlier SystemTime!

mod cuda_utils;
mod filesystem_utils;
mod handlebars_helper;
mod parabuilder;
pub use cuda_utils::get_cuda_device_uuids;
pub use parabuilder::{
    CompliationErrorHandlingMethod, Parabuilder, RunMethod, IGNORE_ON_ERROR_DEFAULT_RUN_FUNC,
    PANIC_ON_ERROR_DEFAULT_RUN_FUNC,
};

#[cfg(test)]
pub mod test_constants {
    pub const EXAMPLE_CMAKE_PROJECT_PATH: &str = "tests/example_cmake_project";
    pub const EXAMPLE_MAKEFILE_PROJECT_PATH: &str = "tests/example_makefile_project";
}
