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