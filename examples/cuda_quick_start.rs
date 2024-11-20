use parabuild::{Parabuilder, RunMethod};
use serde_json::Value as JsonValue;

fn main() {
    let project_path = "tests/example_cuda_project"; // your project path
    let workspaces_path = "workspaces"; // where to store the workspaces, executables, etc.
    let template_path = "src/main.cu"; // template file in the project
    let target_executable_file = "build/main"; // target executable file
    let datas = (0..5)
        .into_iter()
        .map(|_| JsonValue::Null)
        .collect::<Vec<JsonValue>>();
    let mut parabuilder = Parabuilder::new(
        project_path,
        workspaces_path,
        template_path,
        &[target_executable_file],
    )
    .in_place_template(true)
    .build_workers(2)
    .run_method(RunMethod::OutOfPlace(7));
    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    parabuilder.run().unwrap();
}
