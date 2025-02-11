use crate::cuda_utils::get_cuda_mig_device_uuids;
use crate::filesystem_utils::{
    copy_dir, copy_dir_with_ignore, copy_dir_with_rsync, is_command_installed,
    wait_until_file_ready,
};
use crate::handlebars_helper::*;
use chrono::Local;
use crossbeam_channel::{unbounded, Receiver, Sender};
use handlebars::Handlebars;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use serde_json::{json, Value as JsonValue};
use std::borrow::Cow;
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use uuid::Uuid;

/// Method you want to when there is a compilation error
#[derive(PartialEq, Copy, Clone)]
pub enum CompliationErrorHandlingMethod {
    /// Just ignore this data
    Ignore,
    /// Collect this data into `compile_error_datas` returned by `run()`
    Collect,
    /// Panic when there is a compilation error
    Panic,
}

/// Method you want to run the your `run_bash_script`
#[derive(PartialEq, Copy, Clone)]
pub enum RunMethod {
    /// Just compile, do not run
    No,
    /// Compile and run in the same thread/workspace
    InPlace,
    /// Compile and run in different threads/workspaces, `usize` is the number of threads to run
    OutOfPlace(usize),
    /// After compile, run in a `usize` thread/workspace
    Exclusive(usize),
}

static CUDA_DEVICE_UUIDS: OnceLock<Vec<String>> = OnceLock::new();

fn get_cuda_device_uuid(id: usize) -> Option<String> {
    let cuda_device_uuids = CUDA_DEVICE_UUIDS.get_or_init(|| get_cuda_mig_device_uuids());
    if id < cuda_device_uuids.len() {
        Some(cuda_device_uuids[id].clone())
    } else {
        None
    }
}

/// The main body of building system
pub struct Parabuilder {
    project_path: PathBuf,
    workspaces_path: PathBuf,
    template_file: PathBuf,
    target_files: Vec<PathBuf>,
    target_files_base: Vec<String>,
    init_bash_script: String,
    compile_bash_script: String,
    run_bash_script: String,
    build_workers: usize,
    run_method: RunMethod,
    temp_target_path_dir: PathBuf,
    run_func_data: RunFunc,
    data_queue_receiver: Option<Receiver<(usize, JsonValue)>>,
    compilation_error_handling_method: CompliationErrorHandlingMethod,
    auto_gather_array_data: bool,
    in_place_template: bool,
    disable_progress_bar: bool,
    mpb: MultiProgress,
    no_cache: bool,
    without_rsync: bool,
    enable_cppflags: bool,
    autosave_interval: u64,
    autosave_dir: PathBuf,
    continue_from_start_time: Option<String>,
}

fn run_func_data_pre_(
    workspace_path: &PathBuf,
    run_script: &str,
    data: &JsonValue,
    _: &mut JsonValue,
) -> Result<(bool, JsonValue), Box<dyn Error>> {
    let workspace_id = workspace_path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .split('_')
        .last()
        .unwrap();
    let mut output = Command::new("bash");
    output
        .arg("-c")
        .arg(run_script)
        .env("PARABUILD_ID", workspace_id);
    if let Some(mig_uuid) = get_cuda_device_uuid(workspace_id.parse().unwrap()) {
        output.env("CUDA_VISIBLE_DEVICES", mig_uuid);
    }
    output.current_dir(&workspace_path);
    let output = output.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let this_data = json! {
        {
            "status": match output.status.code() {
                Some(code) => code,
                None => -1
            },
            "stdout": stdout,
            "stderr": stderr,
            "data": data
        }
    };
    Ok((output.status.success(), this_data))
}

fn run_func_data_post_(
    this_data: JsonValue,
    run_data: &mut JsonValue,
) -> Result<JsonValue, Box<dyn Error>> {
    if run_data.is_null() {
        *run_data = JsonValue::Array(vec![this_data.clone()]);
    } else {
        run_data.as_array_mut().unwrap().push(this_data.clone());
    }
    Ok(this_data)
}

fn run_func_data_panic_on_error(
    workspace_path: &PathBuf,
    run_script: &str,
    data: &JsonValue,
    run_data: &mut JsonValue,
    stop_flag: &Arc<AtomicBool>,
) -> Result<JsonValue, Box<dyn Error>> {
    let (success, this_data) = run_func_data_pre_(workspace_path, run_script, data, run_data)?;
    if !success {
        Err(format!("stderr: {}", this_data["stderr"]).as_str())?;
    }
    if stop_flag.load(Ordering::Relaxed) {
        Ok(JsonValue::Null)
    } else {
        run_func_data_post_(this_data, run_data)
    }
}

fn run_func_data_ignore_on_error(
    workspace_path: &PathBuf,
    run_script: &str,
    data: &JsonValue,
    run_data: &mut JsonValue,
    stop_flag: &Arc<AtomicBool>,
) -> Result<JsonValue, Box<dyn Error>> {
    let (_, this_data) = run_func_data_pre_(workspace_path, run_script, data, run_data)?;
    if stop_flag.load(Ordering::Relaxed) {
        Ok(JsonValue::Null)
    } else {
        run_func_data_post_(this_data, run_data)
    }
}

type RunFunc = fn(
    &PathBuf,
    &str,
    &JsonValue,
    &mut JsonValue,
    &Arc<AtomicBool>,
) -> Result<JsonValue, Box<dyn Error>>;

/// Default run function that panics when there is an error
pub const PANIC_ON_ERROR_DEFAULT_RUN_FUNC: RunFunc = run_func_data_panic_on_error;

/// Default run function that ignores when there is an error
pub const IGNORE_ON_ERROR_DEFAULT_RUN_FUNC: RunFunc = run_func_data_ignore_on_error;

impl Parabuilder {
    pub const TEMP_TARGET_PATH_DIR: &'static str = "targets";

    pub fn new<P, Q, R, S>(
        project_path: P,
        workspaces_path: Q,
        template_file: R,
        target_files: &[S],
    ) -> Self
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
        R: AsRef<Path>,
        S: AsRef<Path>,
    {
        let project_path = project_path.as_ref().to_path_buf();
        let workspaces_path = workspaces_path.as_ref().to_path_buf();
        let template_file = template_file.as_ref().to_path_buf();
        let target_files: Vec<PathBuf> = target_files
            .into_iter()
            .map(|target_file| target_file.as_ref().to_path_buf())
            .collect();
        let target_files_base = target_files
            .iter()
            .map(|target_file| {
                target_file
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        let default_run_bash_script = if target_files.len() > 0 {
            format!(
                r#"
                ./{}
                "#,
                target_files[0].to_string_lossy()
            )
        } else {
            "".to_string()
        };

        let temp_target_path_dir = workspaces_path.join(Self::TEMP_TARGET_PATH_DIR);
        let init_bash_script = r#"
        cmake -B build -S . -DPARABUILD=ON
        "#;
        let compile_bash_script = r#"
        cmake --build build --target all -- -B
        "#;
        let build_workers = 1;
        Self {
            project_path,
            workspaces_path,
            template_file,
            target_files,
            target_files_base,
            init_bash_script: init_bash_script.to_string(),
            compile_bash_script: compile_bash_script.to_string(),
            run_bash_script: default_run_bash_script,
            build_workers,
            run_method: RunMethod::Exclusive(1),
            temp_target_path_dir,
            run_func_data: IGNORE_ON_ERROR_DEFAULT_RUN_FUNC,
            data_queue_receiver: None,
            compilation_error_handling_method: CompliationErrorHandlingMethod::Collect,
            auto_gather_array_data: true,
            in_place_template: false,
            disable_progress_bar: false,
            mpb: MultiProgress::new(),
            no_cache: false,
            without_rsync: false,
            enable_cppflags: false,
            autosave_interval: 0,
            autosave_dir: PathBuf::from(".parabuild/autosave"),
            continue_from_start_time: None,
        }
    }

    pub fn init_bash_script(mut self, init_bash_script: &str) -> Self {
        self.init_bash_script = init_bash_script.to_string();
        self
    }

    pub fn compile_bash_script(mut self, compile_bash_script: &str) -> Self {
        self.compile_bash_script = compile_bash_script.to_string();
        self
    }

    pub fn run_bash_script(mut self, run_bash_script: &str) -> Self {
        self.run_bash_script = run_bash_script.to_string();
        self
    }

    pub fn build_workers(mut self, build_workers: usize) -> Self {
        self.build_workers = build_workers;
        self
    }

    pub fn run_workers(mut self, run_workers: isize) -> Self {
        if run_workers > 0 {
            self.run_method = RunMethod::OutOfPlace(run_workers as usize);
        } else if run_workers == 0 {
            self.run_method = RunMethod::No;
        } else if run_workers < 0 {
            self.run_method = RunMethod::Exclusive(-run_workers as usize);
        }
        self
    }

    pub fn run_workers_exclusive(mut self, run_workers: isize) -> Self {
        self.run_method = RunMethod::Exclusive(run_workers as usize);
        self
    }

    pub fn run_method(mut self, run_method: RunMethod) -> Self {
        self.run_method = run_method;
        self
    }

    pub fn run_func(mut self, run_func: RunFunc) -> Self {
        self.run_func_data = run_func;
        self
    }

    pub fn compilation_error_handling_method(
        mut self,
        compilation_error_handling_method: CompliationErrorHandlingMethod,
    ) -> Self {
        self.compilation_error_handling_method = compilation_error_handling_method;
        self
    }

    pub fn auto_gather_array_data(mut self, auto_gather_array_data: bool) -> Self {
        self.auto_gather_array_data = auto_gather_array_data;
        self
    }

    pub fn in_place_template(mut self, in_place_template: bool) -> Self {
        self.in_place_template = in_place_template;
        self
    }

    pub fn disable_progress_bar(mut self, disable_progress_bar: bool) -> Self {
        self.disable_progress_bar = disable_progress_bar;
        self
    }

    pub fn no_cache(mut self, no_cache: bool) -> Self {
        self.no_cache = no_cache;
        self
    }

    pub fn without_rsync(mut self, without_rsync: bool) -> Self {
        self.without_rsync = without_rsync;
        self
    }

    pub fn enable_cppflags(mut self, enable_cppflags: bool) -> Self {
        self.enable_cppflags = enable_cppflags;
        self
    }

    pub fn autosave_interval(mut self, autosave_interval: u64) -> Self {
        self.autosave_interval = autosave_interval;
        self
    }

    pub fn autosave_dir<S: AsRef<Path>>(mut self, autosave_dir: S) -> Self {
        self.autosave_dir = autosave_dir.as_ref().to_path_buf();
        self
    }

    /// Set datas to be rendered into the template
    pub fn set_datas(&mut self, datas: Vec<JsonValue>) -> Result<(), Box<dyn Error>> {
        if self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is already initialized".into());
        }
        let (data_queue_sender, data_queue_receiver) = unbounded();
        self.data_queue_receiver = Some(data_queue_receiver);
        for id_data in datas.into_iter().enumerate() {
            data_queue_sender.send(id_data).unwrap();
        }
        Ok(())
    }

    /// Set datas to be rendered into the template
    pub fn set_datas_with_processed_data_ids_set(
        &mut self,
        datas: Vec<JsonValue>,
        processed_data_ids_set: HashSet<usize>,
    ) -> Result<(), Box<dyn Error>> {
        if self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is already initialized".into());
        }
        let (data_queue_sender, data_queue_receiver) = unbounded();
        self.data_queue_receiver = Some(data_queue_receiver);
        for id_data in datas.into_iter().enumerate() {
            if !processed_data_ids_set.contains(&id_data.0) {
                data_queue_sender.send(id_data).unwrap();
            }
        }
        Ok(())
    }

    pub fn get_data_queue_sender(&mut self) -> Result<Sender<(usize, JsonValue)>, Box<dyn Error>> {
        if self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is already initialized".into());
        }
        let (data_queue_sender, data_queue_receiver) = unbounded();
        self.data_queue_receiver = Some(data_queue_receiver);
        Ok(data_queue_sender)
    }

    /// Initialize workspaces
    pub fn init_workspace(&self) -> Result<(), Box<dyn Error>> {
        if !is_command_installed("rsync") {
            if !self.without_rsync {
                return Err("rsync is not installed, set `without_rsync` to true to ignore".into());
            }
        }
        let out_of_place_run_workers = match self.run_method {
            RunMethod::OutOfPlace(run_workers) => run_workers,
            RunMethod::Exclusive(run_workers) => run_workers,
            _ => 0,
        };
        let workspaces_path = if self.workspaces_path.is_absolute() {
            self.workspaces_path.clone()
        } else {
            env::current_dir().unwrap().join(&self.workspaces_path)
        };
        if self.no_cache {
            if self.workspaces_path.exists() {
                std::fs::remove_dir_all(&self.workspaces_path).unwrap();
            }
        }
        std::fs::create_dir_all(&workspaces_path).unwrap();
        let mut project_path = self.project_path.clone();
        let move_to_temp_dir = workspaces_path
            .starts_with(std::fs::canonicalize(&self.project_path).unwrap())
            && self.without_rsync;
        let mut build_handles = vec![];
        if move_to_temp_dir {
            self.add_spinner("copying to temp dir");
            project_path = tempdir().unwrap().into_path();
            copy_dir_with_ignore(&self.project_path, &project_path).unwrap();
        }
        for (i, destination) in (0..self.build_workers).map(|i| (i, format!("workspace_{}", i))) {
            let source = project_path.clone();
            let destination = self.workspaces_path.join(destination);
            let init_bash_script = self.init_bash_script.clone();
            let mpb = self.mpb.clone();
            let disable_progress_bar = self.disable_progress_bar;
            let without_rsync = self.without_rsync;
            let handle = std::thread::spawn(move || {
                let sp = Self::add_spinner2(
                    disable_progress_bar,
                    &mpb,
                    format!("init workspace {}: copying", i),
                );
                if move_to_temp_dir {
                    copy_dir(&source, &destination).unwrap();
                } else {
                    if without_rsync {
                        copy_dir_with_ignore(&source, &destination).unwrap();
                    } else {
                        copy_dir_with_rsync(&source, &destination).unwrap();
                    }
                }
                sp.set_message(format!("init workspace {}: init", i));
                Command::new("bash")
                    .arg("-c")
                    .arg(&init_bash_script)
                    .current_dir(&destination)
                    .output()
                    .unwrap();
            });
            build_handles.push(handle);
        }
        let mut run_handles = vec![];
        if out_of_place_run_workers > 0 {
            // only compile to executable when run_workers = 0
            std::fs::create_dir_all(self.workspaces_path.join(Self::TEMP_TARGET_PATH_DIR)).unwrap();
            for (i, destination) in
                (0..out_of_place_run_workers).map(|i| (i, format!("workspace_exe_{}", i)))
            {
                let source = project_path.clone();
                let destination = self.workspaces_path.join(destination);
                let init_bash_script = self.init_bash_script.clone();
                // let compile_bash_script = self.compile_bash_script.clone();
                // let in_place_template = self.in_place_template;
                let mpb = self.mpb.clone();
                let disable_progress_bar = self.disable_progress_bar;
                let without_rsync = self.without_rsync;
                let handle = std::thread::spawn(move || {
                    let sp = Self::add_spinner2(
                        disable_progress_bar,
                        &mpb,
                        format!("init workspace_run {}: copying", i),
                    );
                    if move_to_temp_dir {
                        copy_dir(&source, &destination).unwrap();
                    } else {
                        if without_rsync {
                            copy_dir_with_ignore(&source, &destination).unwrap();
                        } else {
                            copy_dir_with_rsync(&source, &destination).unwrap();
                        }
                    }
                    sp.set_message(format!("init workspace_run {}: init", i));
                    match Command::new("bash")
                        .arg("-c")
                        .arg(&init_bash_script)
                        .current_dir(&destination)
                        .output()
                    {
                        Ok(output) => {
                            if !output.status.success() {
                                panic!(
                                    "Init bash script failed in workspace_run_{}: {:?}",
                                    i, output
                                );
                            }
                        }
                        Err(e) => {
                            panic!("Init bash script failed in workspace_run_{}: {:?}", i, e);
                        }
                    }
                    // assert!(output.status.success());
                    // let output = Command::new("bash")
                    //     .arg("-c")
                    //     .arg(&compile_bash_script)
                    //     .current_dir(&destination)
                    //     .output()
                    //     .unwrap();
                    // assert!(output.status.success());
                });
                run_handles.push(handle);
            }
        }

        for handle in build_handles {
            handle.join().unwrap();
        }
        for handle in run_handles {
            handle.join().unwrap();
        }

        std::fs::create_dir_all(&self.temp_target_path_dir).unwrap();

        Ok(())
    }

    fn latest_folder<P: AsRef<Path>>(dir: P) -> Option<PathBuf> {
        if !dir.as_ref().exists() {
            return None;
        }
        let mut latest = None;
        let mut latest_time = None;
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                let time = metadata.modified().unwrap();
                if latest_time.is_none() || time > latest_time.unwrap() {
                    latest = Some(path);
                    latest_time = Some(time);
                }
            }
        }
        latest
    }

    /// Load autosave data (run_datas, compile_error_datas, processed_data_ids)
    pub fn autosave_load(&mut self, start_time: String) -> (JsonValue, Vec<JsonValue>, Vec<usize>) {
        let autosave_dir = if start_time.is_empty() {
            let latest = Self::latest_folder(&self.autosave_dir);
            if latest.is_none() {
                panic!("No autosave data found, consider running without --continue");
            }
            latest.unwrap()
        } else {
            self.autosave_dir.join(start_time)
        };
        println!("Loading autosave data from {:?}", autosave_dir);
        self.continue_from_start_time = Some(
            autosave_dir
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
        );
        let datas = std::fs::read_dir(&autosave_dir).unwrap().into_iter().fold(
            (vec![], vec![], vec![]),
            |(mut run_datas_array, mut compile_error_datas_array, mut processed_data_ids_array),
             entry| {
                let entry = entry.unwrap();
                let path = entry.path();
                let run_datas_file = path.join("run_datas.json");
                let compile_error_datas_file = path.join("compile_error_datas.json");
                let processed_data_ids_file = path.join("processed_data_ids.json");
                let run_datas: JsonValue =
                    serde_json::from_reader(std::fs::File::open(&run_datas_file).unwrap()).unwrap();
                let compile_error_datas: Vec<JsonValue> = serde_json::from_reader(
                    std::fs::File::open(&compile_error_datas_file).unwrap(),
                )
                .unwrap();
                let processed_data_ids: Vec<usize> =
                    serde_json::from_reader(std::fs::File::open(&processed_data_ids_file).unwrap())
                        .unwrap();
                run_datas_array.push(run_datas);
                compile_error_datas_array.extend(compile_error_datas);
                processed_data_ids_array.extend(processed_data_ids);
                (
                    run_datas_array,
                    compile_error_datas_array,
                    processed_data_ids_array,
                )
            },
        );
        self.gather_data(datas.0, datas.1, datas.2).unwrap()
    }

    /// Save autosave data
    fn autosave_save<P: AsRef<Path>>(
        autosave_dir: P,
        start_time: &str,
        run_datas: &JsonValue,
        compile_error_datas: &Vec<JsonValue>,
        processed_data_ids: &Vec<usize>,
        workspace_id: Uuid,
    ) {
        // 包含当前时间的文件名
        let autosave_dir = autosave_dir
            .as_ref()
            .to_path_buf()
            .join(start_time)
            .join(workspace_id.to_string());
        if !autosave_dir.exists() {
            std::fs::create_dir_all(&autosave_dir).expect("Failed to create autosave dir");
        }
        let run_datas_file = autosave_dir.join("run_datas.json");
        let run_datas_file1 = autosave_dir.join("run_datas.json.1");
        let compile_error_datas_file = autosave_dir.join("compile_error_datas.json");
        let compile_error_datas_file1 = autosave_dir.join("compile_error_datas.json.1");
        let processed_data_ids_file = autosave_dir.join("processed_data_ids.json");
        let processed_data_ids_file1 = autosave_dir.join("processed_data_ids.json.1");
        if run_datas_file.exists() {
            std::fs::rename(&run_datas_file, &run_datas_file1).unwrap();
        }
        if compile_error_datas_file.exists() {
            std::fs::rename(&compile_error_datas_file, &compile_error_datas_file1).unwrap();
        }
        if processed_data_ids_file.exists() {
            std::fs::rename(&processed_data_ids_file, &processed_data_ids_file1).unwrap();
        }
        let run_datas_file = std::fs::File::create(&run_datas_file).unwrap();
        let compile_error_datas_file = std::fs::File::create(&compile_error_datas_file).unwrap();
        let processed_data_ids_file = std::fs::File::create(&processed_data_ids_file).unwrap();
        serde_json::to_writer(run_datas_file, &run_datas).unwrap();
        serde_json::to_writer(compile_error_datas_file, &compile_error_datas).unwrap();
        serde_json::to_writer(processed_data_ids_file, &processed_data_ids).unwrap();
    }

    /// run the build system
    pub fn run(&self) -> Result<(JsonValue, Vec<JsonValue>, Vec<usize>), Box<dyn Error>> {
        let start_time = if let Some(start_time) = &self.continue_from_start_time {
            start_time.clone()
        } else {
            Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
        };
        println!("Start from: {}", start_time);
        if !self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is not initialized".into());
        }
        if !is_command_installed("bash") {
            return Err("bash is not installed".into());
        }
        if !is_command_installed("lsof") {
            return Err("lsof is not installed, which may lead to strange problems that are difficult to reproduce".into());
        }
        let mut build_handles = vec![];
        let mut run_handles = Vec::new();
        let (executable_queue_sender, executable_queue_receiver) = unbounded();
        let data_size = self.data_queue_receiver.as_ref().unwrap().len() as u64;
        let build_pb = self.add_progress_bar("Building", data_size, "All builds done");
        let run_pb = if !matches!(self.run_method, RunMethod::No) {
            if matches!(self.run_method, RunMethod::Exclusive(_)) {
                self.add_progress_bar("Waiting to run (exclusive)", data_size, "All runs done")
            } else {
                self.add_progress_bar("Running", data_size, "All runs done")
            }
        } else {
            ProgressBar::hidden()
        };
        let stop_flag = Arc::new(AtomicBool::new(false));
        if !cfg!(test) {
            ctrlc::set_handler({
                let stop_flag = Arc::clone(&stop_flag);
                move || {
                    println!("Ctrl-C received, stopping...");
                    stop_flag.store(true, Ordering::Relaxed);
                }
            })
            .expect("Error setting Ctrl-C handler");
        }
        build_pb.tick();
        run_pb.tick();
        let spawn_build_workers = || {
            for i in 0..self.build_workers {
                let workspace_path = self.workspaces_path.join(format!("workspace_{}", i));
                let build_handle = self.build_worker(
                    workspace_path,
                    executable_queue_sender.clone(),
                    build_pb.clone(),
                    run_pb.clone(),
                    Arc::clone(&stop_flag),
                    start_time.clone(),
                );
                build_handles.push(build_handle);
            }
            drop(executable_queue_sender);
        };
        let spawn_run_workers = || {
            let run_workers = match self.run_method {
                RunMethod::OutOfPlace(run_workers) => run_workers,
                RunMethod::Exclusive(run_workers) => run_workers,
                _ => 0,
            };
            for i in 0..run_workers {
                let workspace_path = self.workspaces_path.join(format!("workspace_exe_{}", i));
                let run_handle = self.run_worker(
                    workspace_path,
                    executable_queue_receiver.clone(),
                    run_pb.clone(),
                    Arc::clone(&stop_flag),
                    start_time.clone(),
                );
                run_handles.push(run_handle);
            }
            drop(executable_queue_receiver);
        };
        let gather_build_handlers =
            |build_handles: Vec<JoinHandle<(JsonValue, Vec<JsonValue>, Vec<usize>)>>| {
                build_handles.into_iter().fold(
                    (vec![], vec![], vec![]),
                    |(
                        mut run_datas_array,
                        mut compile_error_datas_array,
                        mut processed_data_ids_array,
                    ),
                     handle| {
                        let (run_datas, compile_error_datas, processed_data_ids) =
                            handle.join().unwrap();
                        run_datas_array.push(run_datas);
                        compile_error_datas_array.extend(compile_error_datas);
                        processed_data_ids_array.extend(processed_data_ids);
                        (
                            run_datas_array,
                            compile_error_datas_array,
                            processed_data_ids_array,
                        )
                    },
                )
            };
        let gather_run_handlers = |run_handles: Vec<JoinHandle<(JsonValue, Vec<usize>)>>| {
            run_handles.into_iter().fold(
                (vec![], vec![]),
                |(mut run_datas_array, mut processed_data_ids_array), handle| {
                    let (run_datas, processed_data_ids) = handle.join().unwrap();
                    run_datas_array.push(run_datas);
                    processed_data_ids_array.extend(processed_data_ids);
                    (run_datas_array, processed_data_ids_array)
                },
            )
        };
        spawn_build_workers();
        drop(build_pb);
        match self.run_method {
            RunMethod::No | RunMethod::InPlace => {
                let (run_datas, compile_error_datas, processed_data_ids) =
                    gather_build_handlers(build_handles);
                self.gather_data(run_datas, compile_error_datas, processed_data_ids)
            }
            RunMethod::Exclusive(_) => {
                let (_, compile_error_datas, mut processed_data_ids) =
                    gather_build_handlers(build_handles);
                run_pb.set_message("Running");
                spawn_run_workers();
                let (run_datas, run_processed_data_ids) = gather_run_handlers(run_handles);
                processed_data_ids.extend(run_processed_data_ids);
                self.gather_data(run_datas, compile_error_datas, processed_data_ids)
            }
            RunMethod::OutOfPlace(_) => {
                spawn_run_workers();
                let (_, compile_error_datas, mut processed_data_ids) =
                    gather_build_handlers(build_handles);
                let (run_datas, run_processed_data_ids) = gather_run_handlers(run_handles);
                processed_data_ids.extend(run_processed_data_ids);
                self.gather_data(run_datas, compile_error_datas, processed_data_ids)
            }
        }
    }

    fn build_worker(
        &self,
        workspace_path: PathBuf,
        executable_queue_sender: Sender<(usize, JsonValue)>,
        build_pb: ProgressBar,
        run_pb: ProgressBar,
        stop_flag: Arc<AtomicBool>,
        start_time: String,
    ) -> std::thread::JoinHandle<(JsonValue, Vec<JsonValue>, Vec<usize>)> {
        let template_path = self.project_path.join(&self.template_file);
        let targets_path: Vec<PathBuf> = self
            .target_files
            .iter()
            .map(|target_file| workspace_path.join(target_file).to_path_buf())
            .collect();
        let compile_bash_script = self.compile_bash_script.clone();
        let template_output_file = if self.in_place_template {
            self.template_file.clone()
        } else {
            self.template_file.with_extension("")
        };
        let target_files_base = self.target_files_base.clone();
        let temp_target_path_dir = self.temp_target_path_dir.clone();
        let data_queue_receiver = self.data_queue_receiver.as_ref().unwrap().clone();
        let run_method = self.run_method;
        let run_func = self.run_func_data;
        let compilation_error_handling_method = self.compilation_error_handling_method;

        let template_output_path = workspace_path.join(&template_output_file);
        let mut handlebars = Handlebars::new();
        if template_path.exists() && template_path.is_file() {
            handlebars.register_helper("default", Box::new(default_value_helper));
            handlebars
                .register_template_string("tpl", std::fs::read_to_string(&template_path).unwrap())
                .unwrap();
        }
        let mut run_data = JsonValue::Null;
        let mut compile_error_datas = Vec::new();
        let run_bash_script = self.run_bash_script.clone();
        let enable_cppflags = self.enable_cppflags;
        let disable_progress_bar = self.disable_progress_bar;
        let mpb = self.mpb.clone();
        let autosave_dir = self.autosave_dir.clone();
        let autosave_interval = self.autosave_interval;
        std::thread::spawn(move || {
            let uuid = Uuid::new_v4();
            let mut processed_data_ids = Vec::new();
            let sp = Self::add_spinner2(
                disable_progress_bar || !matches!(run_method, RunMethod::InPlace),
                &mpb,
                serde_json::to_string_pretty(&JsonValue::Null).unwrap(),
            );
            let mut autosave_last_time = Instant::now();
            for (i, data) in data_queue_receiver.iter() {
                let mut cppflags_val = "-DPARABUILD=ON ".to_string();
                if enable_cppflags {
                    /* {"key":value} => -Dkey=value*/
                    for (key, value) in data.as_object().unwrap().iter() {
                        cppflags_val.push_str(&format!("-D{}={} ", key, value));
                    }
                }
                if handlebars.has_template("tpl") {
                    let mut template_output = std::fs::File::create(&template_output_path)
                        .expect(format!("Failed to create {:?}", template_output_path).as_str());
                    handlebars
                        .render_to_write("tpl", &data, &template_output)
                        .expect(format!("Failed to render {:?}", template_output_path).as_str());
                    template_output.flush().unwrap();
                }
                let mut output = Command::new("bash");
                let mut output = output
                    .arg("-c")
                    .arg(&compile_bash_script)
                    .current_dir(&workspace_path);
                if enable_cppflags {
                    output = output.env("CPPFLAGS", cppflags_val);
                }
                let output = output.output();
                build_pb.inc(1);
                if output.is_err() || output.is_ok() && !output.as_ref().unwrap().status.success() {
                    if stop_flag.load(Ordering::Relaxed) {
                        // current data should be saved, ignore here
                    } else {
                        processed_data_ids.push(i);
                        if compilation_error_handling_method
                            == CompliationErrorHandlingMethod::Panic
                        {
                            if let Ok(output) = output {
                                panic!(
                                    "Compilation script failed in data: {:?} with output: {:?}",
                                    data, output
                                );
                            } else {
                                panic!("Compilation script failed in data: {:?}", data);
                            }
                        } else {
                            if !matches!(run_method, RunMethod::No) {
                                run_pb.inc(1);
                            }
                            match compilation_error_handling_method {
                                CompliationErrorHandlingMethod::Collect => {
                                    compile_error_datas.push(data.clone());
                                    continue;
                                }
                                CompliationErrorHandlingMethod::Ignore => {
                                    continue;
                                }
                                CompliationErrorHandlingMethod::Panic => {
                                    panic!("Compilation script failed in data: {:?}", data);
                                }
                            }
                        }
                    }
                }
                if stop_flag.load(Ordering::Relaxed) {
                    Self::autosave_save(
                        &autosave_dir,
                        &start_time,
                        &run_data,
                        &compile_error_datas,
                        &processed_data_ids,
                        uuid,
                    );
                    break;
                }
                match run_method {
                    RunMethod::InPlace => {
                        // run
                        let last_data = run_func(
                            &std::fs::canonicalize(&workspace_path).unwrap(),
                            &run_bash_script,
                            &data,
                            &mut run_data,
                            &stop_flag,
                        )
                        .unwrap();
                        sp.set_message(serde_json::to_string_pretty(&last_data).unwrap());
                        run_pb.inc(1);
                    }
                    RunMethod::No | RunMethod::Exclusive(_) | RunMethod::OutOfPlace(_) => {
                        for (target_path, target_file_base) in
                            targets_path.iter().zip(target_files_base.iter())
                        {
                            let to_target_executable_path_file =
                                format!("{}_{}", &target_file_base, i);
                            let to_target_executable_path =
                                temp_target_path_dir.join(&to_target_executable_path_file);
                            std::fs::copy(&target_path, &to_target_executable_path).unwrap();
                        }
                        match run_method {
                            RunMethod::No => {
                                let to_metadata_path =
                                    temp_target_path_dir.join(format!("data_{}.json", i));
                                std::fs::write(&to_metadata_path, data.to_string()).unwrap();
                            }
                            RunMethod::OutOfPlace(_) | RunMethod::Exclusive(_) => {
                                executable_queue_sender.send((i, data.clone())).unwrap();
                            }
                            _ => panic!("Unexpected run method"),
                        }
                    }
                }
                if stop_flag.load(Ordering::Relaxed) {
                    Self::autosave_save(
                        &autosave_dir,
                        &start_time,
                        &run_data,
                        &compile_error_datas,
                        &processed_data_ids,
                        uuid,
                    );
                    break;
                }
                match run_method {
                    RunMethod::InPlace | RunMethod::No => {
                        processed_data_ids.push(i);
                    }
                    _ => {}
                }
                if autosave_interval > 0
                    && autosave_last_time.elapsed().as_secs() > autosave_interval
                {
                    Self::autosave_save(
                        &autosave_dir,
                        &start_time,
                        &run_data,
                        &compile_error_datas,
                        &processed_data_ids,
                        uuid,
                    );
                    autosave_last_time = Instant::now();
                }
            }
            (run_data, compile_error_datas, processed_data_ids)
        })
    }

    fn run_worker(
        &self,
        workspace_path: PathBuf,
        executable_queue_receiver: Receiver<(usize, JsonValue)>,
        run_pb: ProgressBar,
        stop_flag: Arc<AtomicBool>,
        start_time: String,
    ) -> std::thread::JoinHandle<(JsonValue, Vec<usize>)> {
        let uuid = Uuid::new_v4();
        let targets_path: Vec<PathBuf> = self
            .target_files
            .iter()
            .map(|target_file| workspace_path.join(target_file).to_path_buf())
            .collect();
        let target_files_base = self.target_files_base.clone();
        let run_func = self.run_func_data;
        let mut run_data = JsonValue::Null;
        let disable_progress_bar = self.disable_progress_bar;
        let mpb = self.mpb.clone();
        let run_bash_script = self.run_bash_script.clone();
        let temp_target_path_dir = self.temp_target_path_dir.clone();
        let autosave_dir = self.autosave_dir.clone();
        let autosave_interval = self.autosave_interval;
        std::thread::spawn(move || {
            let mut processed_data_ids = Vec::new();
            let mut autosave_last_time = Instant::now();
            let sp = Self::add_spinner2(
                disable_progress_bar,
                &mpb,
                serde_json::to_string_pretty(&JsonValue::Null).unwrap(),
            );
            for (i, data) in executable_queue_receiver.iter() {
                for (target_path, target_file_base) in
                    targets_path.iter().zip(target_files_base.iter())
                {
                    let to_target_path_file = format!("{}_{}", &target_file_base, i);
                    let to_target_executable_path = temp_target_path_dir.join(&to_target_path_file);
                    std::fs::rename(&to_target_executable_path, &target_path).unwrap();
                }
                for target_path in targets_path.iter() {
                    wait_until_file_ready(&target_path).unwrap();
                }
                let last_data = run_func(
                    &std::fs::canonicalize(&workspace_path).unwrap(),
                    &run_bash_script,
                    &data,
                    &mut run_data,
                    &stop_flag,
                )
                .unwrap();
                if stop_flag.load(Ordering::Relaxed) {
                    Self::autosave_save(
                        &autosave_dir,
                        &start_time,
                        &run_data,
                        &vec![],
                        &processed_data_ids,
                        uuid,
                    );
                    break;
                }
                sp.set_message(serde_json::to_string_pretty(&last_data).unwrap());
                run_pb.inc(1);
                processed_data_ids.push(i);
                if autosave_interval > 0
                    && autosave_last_time.elapsed().as_secs() > autosave_interval
                {
                    Self::autosave_save(
                        &autosave_dir,
                        &start_time,
                        &run_data,
                        &vec![],
                        &processed_data_ids,
                        uuid,
                    );
                    autosave_last_time = Instant::now();
                }
            }
            (run_data, processed_data_ids)
        })
    }

    pub fn gather_data(
        &self,
        run_data_array: Vec<JsonValue>,
        compile_error_datas: Vec<JsonValue>,
        processed_data_ids: Vec<usize>,
    ) -> Result<(JsonValue, Vec<JsonValue>, Vec<usize>), Box<dyn Error>> {
        let run_data_array: Vec<JsonValue> = run_data_array
            .into_iter()
            .filter(|item| !item.is_null())
            .collect();
        let run_datas = if self.run_method == RunMethod::No {
            JsonValue::Null
        } else if self.auto_gather_array_data && run_data_array.iter().all(|item| item.is_array()) {
            let mut run_data = Vec::new();
            for run_data_item in run_data_array {
                run_data.extend(run_data_item.as_array().unwrap().iter().cloned());
            }
            JsonValue::Array(run_data)
        } else {
            // just return array json
            JsonValue::Array(run_data_array)
        };
        Ok((run_datas, compile_error_datas, processed_data_ids))
    }

    fn add_progress_bar<S: Into<String>, F: Into<Cow<'static, str>>>(
        &self,
        message: S,
        total: u64,
        finish_message: F,
    ) -> ProgressBar {
        if self.disable_progress_bar {
            return ProgressBar::hidden();
        }
        let sty = ProgressStyle::with_template(
            "[{elapsed_precise}  ETA: {eta_precise}] [{per_sec}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap();
        self.mpb.add(
            ProgressBar::new(total)
                .with_message(message.into())
                .with_style(sty)
                .with_finish(ProgressFinish::AbandonWithMessage(finish_message.into())),
        )
    }

    fn add_spinner<S: Into<String>>(&self, message: S) -> ProgressBar {
        if self.disable_progress_bar {
            return ProgressBar::hidden();
        }
        let sp = self
            .mpb
            .add(ProgressBar::new_spinner().with_message(message.into()));
        sp.enable_steady_tick(Duration::from_millis(100));
        sp
    }

    fn add_spinner2<S: Into<String>>(
        disable_progress_bar: bool,
        mpb: &MultiProgress,
        message: S,
    ) -> ProgressBar {
        if disable_progress_bar {
            return ProgressBar::hidden();
        }
        let sp = mpb.add(ProgressBar::new_spinner().with_message(message.into()));
        sp.enable_steady_tick(Duration::from_millis(100));
        sp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use std::time::Instant;
    use serde_json::json;

    const EXAMPLE_PROJECT: &str = crate::test_constants::EXAMPLE_CMAKE_PROJECT_PATH;
    const EXAMPLE_TEMPLATE_FILE: &str = "src/main.cpp.template";
    const EXAMPLE_IN_PLACE_TEMPLATE_FILE: &str = "src/main.cpp";
    const EXAMPLE_TARGET_EXECUTABLE_FILE: &str = "build/main";
    const EXAMPLE_INIT_BASH_SCRIPT: &str = r#"
        cmake -B build -S .
        "#;
    const EXAMPLE_IN_PLACE_INIT_BASH_SCRIPT: &str = r#"
        cmake -B build -S . -DPARABUILD=ON
        "#;
    const EXAMPLE_COMPILE_BASH_SCRIPT: &str = r#"
        cmake --build build --target all -- -B
        "#;

    #[test]
    fn test_workspaces_under_project_path() {
        let example_project_path = std::fs::canonicalize(EXAMPLE_PROJECT).unwrap();
        let workspaces_path = PathBuf::from("workspaces_under_project_path");
        let parabuilder = Parabuilder::new(
            ".",
            &workspaces_path,
            example_project_path.join("src/main.cpp.template"),
            &["build/main"],
        )
        .init_bash_script(
            format!(
                r#"
                cmake -B build -S {}
                "#,
                EXAMPLE_PROJECT
            )
            .as_str(),
        )
        .compile_bash_script(
            r#"
            cmake --build build --target all -- -B
            "#,
        )
        .disable_progress_bar(true)
        .no_cache(true);
        parabuilder.init_workspace().unwrap();
        assert!(workspaces_path
            .join(format!(
                "workspace_0/{}/src/main.cpp.template",
                EXAMPLE_PROJECT
            ))
            .exists());
        assert!(workspaces_path.join("workspace_0/build").exists());
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    const SINGLETHREADED_N: i64 = 20;
    const MULTITHREADED_N: i64 = 100;

    fn parabuild_tester(
        name: &str,
        size: i64,
        build_workers: usize,
        run_method: RunMethod,
        in_place_template: bool,
        is_makefile_project: bool,
    ) {
        let project = if !is_makefile_project {
            EXAMPLE_PROJECT
        } else {
            crate::test_constants::EXAMPLE_MAKEFILE_PROJECT_PATH
        };
        let mut datas = (1..=size)
            .map(|i| json!({"N": i}))
            .collect::<Vec<JsonValue>>();
        if size == 0 {
            datas.push(json!({}));
        }
        let error_data = json!({"N": "a"});
        datas.push(error_data.clone());
        let workspaces_path = PathBuf::from(format!("tests/workspaces_{}", name));
        let mut parabuilder = Parabuilder::new(
            project,
            &workspaces_path,
            if !is_makefile_project {
                if in_place_template {
                    EXAMPLE_IN_PLACE_TEMPLATE_FILE
                } else {
                    EXAMPLE_TEMPLATE_FILE
                }
            } else {
                ""
            },
            if !is_makefile_project {
                &[EXAMPLE_TARGET_EXECUTABLE_FILE]
            } else {
                &["main"]
            },
        )
        .init_bash_script(if !is_makefile_project {
            if in_place_template {
                EXAMPLE_IN_PLACE_INIT_BASH_SCRIPT
            } else {
                EXAMPLE_INIT_BASH_SCRIPT
            }
        } else {
            ""
        })
        .compile_bash_script(if !is_makefile_project {
            EXAMPLE_COMPILE_BASH_SCRIPT
        } else {
            "make -B"
        })
        .build_workers(build_workers)
        .run_method(run_method)
        .run_func(PANIC_ON_ERROR_DEFAULT_RUN_FUNC)
        .disable_progress_bar(true)
        .compilation_error_handling_method(CompliationErrorHandlingMethod::Collect)
        .in_place_template(in_place_template)
        .auto_gather_array_data(true)
        .enable_cppflags(is_makefile_project);

        parabuilder.set_datas(datas).unwrap();
        parabuilder.init_workspace().unwrap();
        let (run_data, compile_error_datas, processed_data_ids) = parabuilder.run().unwrap();
        assert!(
            compile_error_datas == vec![error_data],
            "got: {:?} {:?}",
            run_data,
            compile_error_datas
        );
        assert!(
            processed_data_ids.len() == if size == 0 { 2 } else { size as usize + 1 }, // default
            "got: {:?}",
            processed_data_ids
        );
        if matches!(run_method, RunMethod::No) {
            assert!(run_data.is_null(), "got: {}", run_data);
            for i in 0..size {
                assert!(workspaces_path
                    .join(format!("{}/main_{}", Parabuilder::TEMP_TARGET_PATH_DIR, i))
                    .exists());
                assert!(workspaces_path
                    .join(format!(
                        "{}/data_{}.json",
                        Parabuilder::TEMP_TARGET_PATH_DIR,
                        i
                    ))
                    .exists());
            }
        } else {
            let ground_truth = if size > 0 {
                (1..=size).sum::<i64>()
            } else {
                42
            };
            assert!(run_data.is_array());
            let sum = run_data.as_array().unwrap().iter().fold(0, |acc, item| {
                acc + item["stdout"]
                    .as_str()
                    .unwrap()
                    .trim()
                    .parse::<i64>()
                    .unwrap()
            });
            assert!(
                sum == ground_truth,
                "expected: {}, got: {}, run_data: {}",
                ground_truth,
                sum,
                run_data
            );
        }
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    #[test]
    fn test_singlethreaded_parabuild_without_run() {
        parabuild_tester(
            "test_singlethreaded_parabuild_without_run",
            SINGLETHREADED_N,
            1,
            RunMethod::No,
            false,
            false,
        );
    }

    #[test]
    fn test_singlethreaded_parabuild_in_place_run() {
        parabuild_tester(
            "test_singlethreaded_parabuild_in_place_run",
            SINGLETHREADED_N,
            1,
            RunMethod::InPlace,
            false,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_without_run() {
        parabuild_tester(
            "test_multithreaded_parabuild_without_run",
            MULTITHREADED_N,
            4,
            RunMethod::No,
            false,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_in_place_run() {
        parabuild_tester(
            "test_multithreaded_parabuild_in_place_run",
            MULTITHREADED_N,
            4,
            RunMethod::InPlace,
            false,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_out_of_place_single_run() {
        parabuild_tester(
            "test_multithreaded_parabuild_out_of_place_single_run",
            MULTITHREADED_N,
            4,
            RunMethod::OutOfPlace(1),
            false,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_out_of_place_run() {
        parabuild_tester(
            "test_multithreaded_parabuild_out_of_place_run",
            MULTITHREADED_N,
            4,
            RunMethod::OutOfPlace(2),
            false,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_exclusive_run() {
        parabuild_tester(
            "test_multithreaded_parabuild_exclusive_run",
            MULTITHREADED_N,
            4,
            RunMethod::Exclusive(2),
            false,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_out_of_place_run_in_place_template() {
        parabuild_tester(
            "test_multithreaded_parabuild_out_of_place_run_in_place_template",
            MULTITHREADED_N,
            4,
            RunMethod::OutOfPlace(2),
            true,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_out_of_place_run_default_template() {
        parabuild_tester(
            "test_multithreaded_parabuild_out_of_place_run_default_template",
            0,
            4,
            RunMethod::OutOfPlace(2),
            true,
            false,
        );
    }

    #[test]
    fn test_multithreaded_parabuild_makefile_project() {
        parabuild_tester(
            "test_multithreaded_parabuild_makefile_project",
            10,
            4,
            RunMethod::OutOfPlace(2),
            true,
            true,
        );
    }

    // #[test]
    // fn test_multithreaded_parabuild_out_of_place_run_in_place_template_heavy() {
    //     parabuild_tester(
    //         "test_multithreaded_parabuild_out_of_place_run_in_place_template",
    //         1000,
    //         20,
    //         RunMethod::OutOfPlace(4),
    //         true,
    //     );
    // }
}
