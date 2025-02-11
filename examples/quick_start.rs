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
    let (run_data, _compile_error_datas, _processed_data_ids): (
        JsonValue,
        Vec<JsonValue>,
        Vec<usize>,
    ) = parabuilder.run().unwrap();
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
