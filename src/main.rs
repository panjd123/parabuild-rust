use clap::Parser;
use parabuild::Parabuilder;
use serde_json::Value as JsonValue;
use std::{path::PathBuf, str::FromStr};

#[derive(Parser)]
#[command(version, author, about, long_about = None)]
struct Cli {
    /// project path
    project_path: PathBuf,

    /// template file in the project
    template_path: PathBuf,

    /// target executable file in the project
    target_executable_file: PathBuf,

    /// where to store the workspaces, executables, etc.
    #[arg(short, long, default_value = "workspaces")]
    workspaces_path: PathBuf,

    /// json format data
    #[arg(long)]
    data: Option<String>,

    /// json format data file, when used together with the `--data` option, ignore this option
    #[arg(short, long)]
    data_file: Option<PathBuf>,

    /// output the json format result to a file, default to stdout
    #[arg(short, long)]
    output_file: Option<PathBuf>,

    /// init bash script file
    #[arg(long)]
    init_bash_script_file: Option<PathBuf>,

    /// init cmake args
    /// e.g. "-DCMAKE_BUILD_TYPE=Release", when used together with the `--init-bash-script-file` option, ignore this option
    #[arg(short, long)]
    init_cmake_args: Option<String>,

    /// compile bash script file
    #[arg(long)]
    compile_bash_script_file: Option<PathBuf>,

    /// make target, when used together with the `--compile-bash-script-file` option, ignore this option
    #[arg(short, long)]
    target: Option<String>,

    /// enable progress bar
    #[arg(short, long)]
    progress_bar: bool,

    /// build workers
    #[arg(short = 'j', long)]
    build_workers: Option<usize>,

    /// run workers
    #[arg(short = 'J', long)]
    run_workers: Option<isize>,

    /// in place template
    #[arg(long)]
    in_place_template: bool,
}

fn main() {
    let args = Cli::parse();
    let data = if let Some(data_str) = args.data {
        JsonValue::from_str(&data_str).unwrap()
    } else if let Some(data_path) = args.data_file {
        let data_str = std::fs::read_to_string(data_path).unwrap();
        JsonValue::from_str(&data_str).unwrap()
    } else {
        panic!("either `--data` or `--data-file` must be provided");
    };

    if !data.is_array() {
        panic!("data must be an array");
    }
    let datas = data.as_array().unwrap().to_owned();

    let init_bash_script = if let Some(init_bash_script_file) = args.init_bash_script_file {
        Some(std::fs::read_to_string(init_bash_script_file).unwrap())
    } else if let Some(init_cmake_args) = args.init_cmake_args {
        Some(format!(
            r#"cmake -S . -B build {} -DPROFILING=ON"#,
            init_cmake_args
        ))
    } else {
        None
    };

    let mut parabuilder = Parabuilder::new(
        args.project_path,
        args.workspaces_path,
        args.template_path,
        args.target_executable_file,
    )
    .in_place_template(args.in_place_template)
    .enable_progress_bar(args.progress_bar);

    if let Some(init_bash_script) = init_bash_script {
        parabuilder = parabuilder.init_bash_script(&init_bash_script);
    }

    let compile_bash_script = if let Some(compile_bash_script_file) = args.compile_bash_script_file
    {
        Some(std::fs::read_to_string(compile_bash_script_file).unwrap())
    } else if let Some(target) = args.target {
        Some(format!(r#"cmake --build build --target {} -- -B"#, target))
    } else {
        None
    };

    if let Some(compile_bash_script) = compile_bash_script {
        parabuilder = parabuilder.compile_bash_script(&compile_bash_script);
    }

    if let Some(build_workers) = args.build_workers {
        parabuilder = parabuilder.build_workers(build_workers);
    }

    if let Some(run_workers) = args.run_workers {
        parabuilder = parabuilder.run_workers(run_workers);
    }

    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    let (run_data, _compile_error_datas): (JsonValue, Vec<JsonValue>) = parabuilder.run().unwrap();

    if let Some(output_file) = args.output_file {
        std::fs::write(
            output_file,
            serde_json::to_string_pretty(&run_data).unwrap(),
        )
        .unwrap();
    } else {
        println!("{}", serde_json::to_string_pretty(&run_data).unwrap());
    }
}
