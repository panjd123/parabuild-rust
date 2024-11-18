mod filesystem_utils;
mod handlebars_helper;
mod parabuilder;
pub use parabuilder::{
    CompliationErrorHandlingMethod, Parabuilder, RunMethod, IGNORE_ON_ERROR_DEFAULT_RUN_FUNC,
    PANIC_ON_ERROR_DEFAULT_RUN_FUNC,
};

#[cfg(test)]
pub mod test_constants {
    pub const EXAMPLE_CMAKE_PROJECT_PATH: &str = "tests/example_cmake_project";
    pub const EXAMPLE_MAKEFILE_PROJECT_PATH: &str = "tests/example_makefile_project";
}
