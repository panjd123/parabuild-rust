use parabuild::{
    CompliationErrorHandlingMethod, Parabuilder, RunMethod, IGNORE_ON_ERROR_DEFAULT_RUN_FUNC,
};
use serde_json::{json, Value as JsonValue};

fn main() {
    let project_path = "tests/example_run_time_consuming_project"; // your project path
    let workspaces_path = "workspaces"; // where to store the workspaces, executables, etc.
    let template_path = "src/main.cpp.template"; // template file in the project
    let target_executable_file = "build/main"; // target executable file
    let mut datas = (1..=100)
        .map(|i| json!({"N": i}))
        .collect::<Vec<JsonValue>>();
    let error_data = json!({"N": "a"});
    datas.push(error_data.clone());
    let init_bash_script = r#"cmake -B build -S . -DPARABUILD=ON"#;
    let compile_bash_script = r#"cmake --build build --target all -- -B"#;
    let mut parabuilder = Parabuilder::new(
        project_path,
        workspaces_path,
        template_path,
        &[target_executable_file],
    )
    .init_bash_script(init_bash_script)
    .compile_bash_script(compile_bash_script)
    // .run_bash_script(format!(r#"./{}"#, target_executable_file).as_str())
    .build_workers(4)
    .run_method(RunMethod::OutOfPlace(2)) // 4 threads compile, 1 thread run
    // .run_method(RunMethod::Exclusive) // 4 threads compile, 1 thread run
    .compilation_error_handling_method(CompliationErrorHandlingMethod::Collect) // collect data that has compilation error
    .auto_gather_array_data(true) // when each run thread finishes, gather the data into one array when every thread returns an array
    .run_func(IGNORE_ON_ERROR_DEFAULT_RUN_FUNC)
    .in_place_template(false)
    .disable_progress_bar(false);
    // let sender = parabuilder.get_data_queue_sender().unwrap();
    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    let (run_data, compile_error_datas): (JsonValue, Vec<JsonValue>) = parabuilder.run().unwrap();
    println!(
        "run_data: {}",
        serde_json::to_string_pretty(&run_data).unwrap()
    );
    println!("compile_error_datas: {:?}", compile_error_datas);
}
