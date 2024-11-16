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
    let (run_data, _compile_error_datas): (JsonValue, Vec<JsonValue>) = parabuilder.run().unwrap();
    println!("{:?}", run_data);
    // Array [Object {"data": Object {"N": String("10")}, "stdout": String("10\n")}, Object {"data": Object {"N": String("20")}, "stdout": String("20\n")}]
}
