use clap::Parser;
use parabuild::{CompliationErrorHandlingMethod, Parabuilder, RunMethod};
use serde_json::Value as JsonValue;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::{path::PathBuf, str::FromStr};

#[derive(Parser)]
#[command(version, author, about, long_about)]
struct Cli {
    /// project path
    project_path: PathBuf,

    /// target files in the project, which will be moved between build/run workspaces for further processing
    ///
    /// e.g. `build/main,data_generate_when_build`
    #[arg(value_delimiter = ',')]
    target_files: Vec<PathBuf>,

    /// template file in the project
    #[arg(short, long)]
    template_file: Option<PathBuf>,

    /// where to store the workspaces, executables, etc.
    #[arg(short, long, default_value = ".parabuild/workspaces")]
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

    /// init bash script
    ///
    /// Default to `cmake -S . -B build -DPARABUILD=ON`
    #[arg(long)]
    init_bash_script: Option<String>,

    /// init bash script file, when used together with the `--init-bash-script` option, ignore this option
    #[arg(long)]
    init_bash_script_file: Option<PathBuf>,

    /// init cmake args, when used together with the `--init-bash-script` or `--init-bash-script-file` option, ignore this option
    ///
    /// e.g. "-DCMAKE_BUILD_TYPE=Release"
    #[arg(short, long)]
    init_cmake_args: Option<String>,

    /// compile bash script
    ///
    /// Default to `cmake --build build --target all -- -B`
    #[arg(long)]
    compile_bash_script: Option<String>,

    /// compile bash script file, when used together with the `--compile-bash-script` option, ignore this option
    #[arg(long)]
    compile_bash_script_file: Option<PathBuf>,

    /// make target, when used together with the `--compile-bash-script` or `--compile-bash-script-file` option, ignore this option
    #[arg(short, long)]
    make_target: Option<String>,

    /// run bash script
    ///
    /// If not provided, we will run the first target file in the `target_files` directly
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
    ///
    /// We have four execution modes:
    ///
    /// 1. separate and parallel
    ///
    /// 2. separate and serial (by default)
    ///
    /// 3. execute immediately in place
    ///
    /// 4. do not execute, only compile, move all the TARGET_FILES to `workspaces/targets`
    ///
    /// The first one means we will move TARGET_FILES between build/run workspaces.
    /// Compile and run in parallel in different places like a pipeline.
    ///
    /// The second behavior is similar to the first,
    /// but the difference is that we only start running after all the compilation work is completed.
    ///
    /// The third method is quite unique, as it does not move the TARGET_FILES and
    /// immediately executes the compilation of a workspace in its original location.
    ///
    /// To specify these three working modes through the command line:
    ///
    /// 1. positive numbers represent the first
    ///
    /// 2. negative numbers represent the second
    ///
    /// 3. pass `--run-in-place` to represent the third, we will ignore the value of this option
    ///
    /// 4. 0 represent the fourth
    #[arg(short = 'J', long)]
    run_workers: Option<isize>,

    /// run in place, which means we will not move the TARGET_FILES between build/run workspaces
    #[arg(long)]
    run_in_place: bool,

    /// seperate template file, as opposed to using the same file to render in place
    #[arg(long)]
    seperate_template: bool,

    /// Clear the contents in `workspaces` before running
    #[arg(long)]
    no_cache: bool,

    /// do not use rsync, which means you will not be able to use incremental replication,
    /// which may require you to use `--no-cache` every time you modify the project
    #[arg(long)]
    without_rsync: bool,

    /// Mark that you are actually working on a makefile project
    ///
    /// pass `data` to `CPPFLAGS` environment variable in the compile bash script
    ///
    /// e.g. when data is `{"N": 10}`, `CPPFLAGS=-DN=10`
    #[arg(long)]
    makefile: bool,

    /// panic on compile error
    #[arg(long)]
    panic_on_compile_error: bool,

    /// format the output when printing to stdout (only valid when `--output-file` is not provided)
    #[arg(long)]
    format_output: bool,

    /// do not run the init bash script, same as `--init-bash-script ""`
    #[arg(long)]
    no_init: bool,

    #[arg(long, default_value = "1m")]
    autosave_interval: String,

    #[arg(long, default_value = ".parabuild/autosave")]
    autosave_dir: PathBuf,
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

fn is_empty(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => true,
        JsonValue::Array(arr) => arr.is_empty(),
        JsonValue::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn main() {
    let args = Cli::parse();
    let data = if let Some(data_str) = args.data {
        if data_str.is_empty() {
            panic!("data must not be empty");
        }
        JsonValue::from_str(&data_str).unwrap()
    } else if let Some(data_path) = args.data_file {
        if !data_path.exists() {
            panic!("data file not exists");
        }
        let data_str = std::fs::read_to_string(data_path).unwrap();
        JsonValue::from_str(&data_str).unwrap()
    } else {
        panic!("either `--data` or `--data-file` must be provided");
    };

    let datas = data.as_array().expect("data must be an array").to_owned();

    let init_bash_script = if args.no_init {
        Some("".to_string())
    } else {
        if let Some(init_bash_script) = args.init_bash_script {
            Some(init_bash_script)
        } else if let Some(init_bash_script_file) = args.init_bash_script_file {
            Some(std::fs::read_to_string(init_bash_script_file).unwrap())
        } else if let Some(init_cmake_args) = args.init_cmake_args {
            Some(format!(
                r#"cmake -S . -B build {} -DPARABUILD=ON"#,
                init_cmake_args
            ))
        } else {
            if !args.makefile {
                None
            } else {
                Some("".to_string()) // do nothing when using makefile by default
            }
        }
    };

    let autosave_interval_secs = humantime::parse_duration(&args.autosave_interval)
        .expect("invalid autosave interval")
        .as_secs();

    let mut parabuilder = Parabuilder::new(
        args.project_path,
        args.workspaces_path,
        args.template_file.unwrap_or_else(|| PathBuf::from("")),
        &args.target_files,
    )
    .in_place_template(!args.seperate_template)
    .disable_progress_bar(args.silent)
    .no_cache(args.no_cache)
    .without_rsync(args.without_rsync)
    .enable_cppflags(args.makefile)
    .autosave_interval(autosave_interval_secs)
    .autosave_dir(args.autosave_dir)
    .compilation_error_handling_method(if args.panic_on_compile_error {
        CompliationErrorHandlingMethod::Panic
    } else {
        CompliationErrorHandlingMethod::Collect
    });

    if let Some(init_bash_script) = init_bash_script {
        parabuilder = parabuilder.init_bash_script(&init_bash_script);
    }

    let compile_bash_script = if let Some(compile_bash_script) = args.compile_bash_script {
        Some(compile_bash_script)
    } else if let Some(compile_bash_script_file) = args.compile_bash_script_file {
        Some(std::fs::read_to_string(compile_bash_script_file).unwrap())
    } else if let Some(target) = args.make_target {
        if !args.makefile {
            Some(format!(r#"cmake --build build --target {} -- -B"#, target))
        } else {
            Some(format!(r#"make {} -B"#, target))
        }
    } else {
        if !args.makefile {
            None
        } else {
            Some("make -B".to_string())
        }
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
        if !args.run_in_place {
            parabuilder = parabuilder.run_workers(run_workers);
        }
    }

    if args.run_in_place {
        parabuilder = parabuilder.run_method(RunMethod::InPlace);
    }

    let datas_len = datas.len();
    parabuilder.set_datas(datas).unwrap();
    parabuilder.init_workspace().unwrap();
    let (run_data, compile_error_datas, unprocessed_datas): (
        JsonValue,
        Vec<JsonValue>,
        Vec<JsonValue>,
    ) = parabuilder.run().unwrap();

    if let Some(output_file) = args.output_file {
        std::fs::write(
            output_file,
            serde_json::to_string_pretty(&run_data).unwrap(),
        )
        .unwrap();
    } else {
        if args.format_output {
            for data in run_data.as_array().unwrap().iter() {
                println!("data:");
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        data.as_object().unwrap()["data"].as_object().unwrap()
                    )
                    .unwrap()
                );
                let stdout = data.as_object().unwrap()["stdout"].as_str().unwrap();
                println!("stdout:");
                println!("{}", stdout);
                println!();
            }
        } else {
            println!("{}", serde_json::to_string_pretty(&run_data).unwrap());
        }
    }

    if !unprocessed_datas.is_empty() {
        println!("Unprocessed: {}", unprocessed_datas.len());
        println!();
    }

    println!("Compilation Summary");
    println!("===================");
    println!(
        "Success: {}\tFailed: {}",
        datas_len - unprocessed_datas.len() - compile_error_datas.len(),
        compile_error_datas.len()
    );
    println!();
    println!("Execution Summary");
    println!("===================");
    if run_data.is_array()
        && run_data.as_array().unwrap().len() > 0
        && run_data.as_array().unwrap()[0].is_object()
        && !run_data.as_array().unwrap()[0]["status"].is_null()
    {
        let success = run_data
            .as_array()
            .unwrap()
            .iter()
            .filter(|data| data["status"].as_i64().unwrap() == 0)
            .count();
        let failed = run_data.as_array().unwrap().len() - success;
        println!("Success: {}\tFailed: {}", success, failed);
    } else {
        if is_empty(&run_data) {
            println!("Empty run_data");
        } else {
            println!("Unknown run_data format, please check the output");
        }
    }

    // write compile error datas to current directory
    std::fs::write(
        "compile_error_datas.json",
        serde_json::to_string_pretty(&compile_error_datas).unwrap(),
    )
    .unwrap();
}
