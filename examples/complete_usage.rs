use parabuild::{CompliationErrorHandlingMethod, Parabuilder, RunMethod};
use serde_json::{json, Value as JsonValue};
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn run_func(
    workspace_path: &PathBuf,
    target_executable_path: &PathBuf,
    _data: &JsonValue,
    run_data: &mut JsonValue,
) -> Result<(), Box<dyn Error>> {
    let output = Command::new(&target_executable_path)
        .current_dir(&workspace_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    if output.status.success() {
        assert!(workspace_path.join("output.txt").exists());
        let output_number = stdout.trim().parse::<i64>().unwrap();
        if run_data.is_null() {
            *run_data = json!(output_number);
        } else {
            *run_data = json!(run_data.as_i64().unwrap() + output_number);
        }
    } else {
        let stderr = String::from_utf8(output.stderr).unwrap();
        Err(format!("stderr: {}", stderr).as_str())?;
    }
    Ok(())
}

fn main() {
    let project_path = "tests/example_project"; // your project path
    let workspaces_path = "workspaces"; // where to store the workspaces, executables, etc.
    let template_path = "src/main.cpp.template"; // template file in the project
    let build_path = "build/main"; // target executable file
    let mut datas = (1..=100)
        .map(|i| json!({"N": i}))
        .collect::<Vec<JsonValue>>();
    let error_data = json!({"N": "a"});
    datas.push(error_data.clone());
    let init_bash_script = r#"
        cmake -B build -S .
        "#;
    let compile_bash_script = r#"
        cmake --build build --target all -- -B
        "#;
    let mut parabuilder =
        Parabuilder::new(project_path, workspaces_path, template_path, build_path)
            .init_bash_script(init_bash_script)
            .compile_bash_script(compile_bash_script)
            .build_workers(4)
            .run_method(RunMethod::OutOfPlace(2)) // 4 threads compile, 1 thread run
            .compilation_error_handling_method(CompliationErrorHandlingMethod::Collect) // collect data that has compilation error
            .auto_gather_array_data(true) // when each run thread finishes, gather the data into one array when every thread returns an array
            .run_func(run_func); // use custom run function
    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    let (run_data, compile_error_datas): (JsonValue, Vec<JsonValue>) = parabuilder.run().unwrap();
    println!("run_data: {:?}", run_data);
    println!("compile_error_datas: {:?}", compile_error_datas);
}
