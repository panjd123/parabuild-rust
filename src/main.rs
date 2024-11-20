use clap::Parser;
use parabuild::Parabuilder;
use serde_json::Value as JsonValue;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::{path::PathBuf, str::FromStr};

#[derive(Parser)]
#[command(version, author, about, long_about = None)]
struct Cli {
    /// project path
    project_path: PathBuf,

    /// template file in the project
    template_file: PathBuf,

    /// target files in the project, which will be moved between build/run workspaces for further processing
    ///
    /// e.g. `build/main,data_generate_when_build`
    #[arg(value_delimiter = ',')]
    target_files: Vec<PathBuf>,

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
    ///
    /// e.g. "-DCMAKE_BUILD_TYPE=Release", when used together with the `--init-bash-script-file` option, ignore this option
    #[arg(short, long)]
    init_cmake_args: Option<String>,

    /// compile bash script file
    #[arg(long)]
    compile_bash_script_file: Option<PathBuf>,

    /// make target, when used together with the `--compile-bash-script-file` option, ignore this option
    #[arg(short, long)]
    make_target: Option<String>,

    /// run bash script
    #[arg(long)]
    run_bash_script: Option<String>,

    /// run bash script file
    /// when used together with the `--run-bash-script` option, ignore this option
    #[arg(long)]
    run_bash_script_file: Option<PathBuf>,

    /// do not show progress bar
    #[arg(short, long)]
    silent: bool,

    /// build workers
    #[arg(short = 'j', long)]
    build_workers: Option<usize>,

    /// run workers
    #[arg(short = 'J', long)]
    run_workers: Option<isize>,

    /// seperate template file, as opposed to using the same file to render in place
    #[arg(long)]
    seperate_template: bool,

    /// Clear the contents in `workspaces` before running
    #[arg(long)]
    no_cache: bool,

    /// do not use rsync, which means you will not be able to use incremental replication,
    /// which may require you to use -- no cache every time you modify the project
    #[arg(long)]
    without_rsync: bool,
}

fn _command_platform_specific_behavior_check() {
    fn create_file_with_executable_permission(file_path: &str, msg: &str) {
        if let Some(parent) = std::path::Path::new(file_path).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut file = std::fs::File::create(file_path).unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        file.set_permissions(perms).unwrap();
        write!(file, "{}", "#!/bin/bash\n").unwrap();
        writeln!(file, "{}", msg).unwrap();
    }
    create_file_with_executable_permission("test.sh", "echo parent");
    create_file_with_executable_permission("tmp/test.sh", "echo current");
    let output = Command::new("./test.sh")
        .current_dir("tmp")
        .output()
        .unwrap()
        .stdout;
    if output != b"current\n" {
        println!("Waning: command run under parent process, you should use custom run function")
    } else {
        println!(
            "command run under `current_dir`, free to use relative path in your `run_bash_script`"
        );
    }
    std::fs::remove_file("test.sh").unwrap();
    std::fs::remove_file("tmp/test.sh").unwrap();
    std::fs::remove_dir("tmp").unwrap();
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
            r#"cmake -S . -B build {} -DPARABUILD=ON"#,
            init_cmake_args
        ))
    } else {
        None
    };

    let mut parabuilder = Parabuilder::new(
        args.project_path,
        args.workspaces_path,
        args.template_file,
        &args.target_files,
    )
    .in_place_template(!args.seperate_template)
    .disable_progress_bar(args.silent)
    .no_cache(args.no_cache)
    .without_rsync(args.without_rsync);

    if let Some(init_bash_script) = init_bash_script {
        parabuilder = parabuilder.init_bash_script(&init_bash_script);
    }

    let compile_bash_script = if let Some(compile_bash_script_file) = args.compile_bash_script_file
    {
        Some(std::fs::read_to_string(compile_bash_script_file).unwrap())
    } else if let Some(target) = args.make_target {
        Some(format!(r#"cmake --build build --target {} -- -B"#, target))
    } else {
        None
    };

    if let Some(compile_bash_script) = compile_bash_script {
        parabuilder = parabuilder.compile_bash_script(&compile_bash_script);
    }

    if let Some(run_bash_script) = args.run_bash_script {
        parabuilder = parabuilder.run_bash_script(&run_bash_script);
    } else if let Some(run_bash_script_file) = args.run_bash_script_file {
        let run_bash_script = std::fs::read_to_string(run_bash_script_file).unwrap();
        parabuilder = parabuilder.run_bash_script(&run_bash_script);
    } else {
        println!(
            "Warning: no run bash script provided, we will run {} directly",
            args.target_files[0].to_str().unwrap()
        );
        parabuilder = parabuilder
            .run_bash_script(&format!(r#"./{}"#, args.target_files[0].to_str().unwrap()));
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
