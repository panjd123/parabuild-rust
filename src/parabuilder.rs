use crate::filesystem_utils::{copy_dir, copy_dir_with_ignore};
use crate::handlebars_helper::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use handlebars::Handlebars;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use serde_json::{json, Value as JsonValue};
use std::borrow::Cow;
use std::env;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

#[derive(PartialEq, Copy, Clone)]
pub enum CompliationErrorHandlingMethod {
    Ignore,
    Collect,
    Panic,
}

#[derive(PartialEq, Copy, Clone)]
pub enum RunMethod {
    No,
    InPlace,
    OutOfPlace(usize),
    Exclusive,
}

pub struct Parabuilder {
    project_path: PathBuf,
    workspaces_path: PathBuf,
    template_file: PathBuf,
    target_executable_file: PathBuf,
    target_executable_file_base: String,
    init_bash_script: String,
    compile_bash_script: String,
    build_workers: usize,
    run_method: RunMethod,
    to_target_executable_path_dir: PathBuf,
    run_func_data: fn(&PathBuf, &PathBuf, &JsonValue, &mut JsonValue) -> Result<(), Box<dyn Error>>,
    data_queue_receiver: Option<Receiver<(usize, JsonValue)>>,
    compilation_error_handling_method: CompliationErrorHandlingMethod,
    auto_gather_array_data: bool,
    in_place_template: bool,
    enable_progress_bar: bool,
    mpb: MultiProgress,
}

impl Parabuilder {
    pub fn new<P, Q, R, S>(
        project_path: P,
        workspaces_path: Q,
        template_file: R,
        target_executable_file: S,
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
        let target_executable_file = target_executable_file.as_ref().to_path_buf();
        let target_executable_file_base = target_executable_file
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let to_target_executable_path_dir = workspaces_path.join("executable");
        let init_bash_script = r#"
        cmake -B build -S .
        "#;
        let compile_bash_script = r#"
        cmake --build build --target all -- -B
        "#;
        let build_workers = 1;
        fn run_func_data(
            workspace_path: &PathBuf,
            target_executable_path: &PathBuf,
            data: &JsonValue,
            run_data: &mut JsonValue,
        ) -> Result<(), Box<dyn Error>> {
            let output = Command::new(&target_executable_path)
                .current_dir(&workspace_path)
                .output()
                .unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            if output.status.success() {
                if run_data.is_null() {
                    *run_data = JsonValue::Array(vec![json! {
                        {
                            "stdout": stdout,
                            "data": data
                        }
                    }]);
                } else {
                    run_data.as_array_mut().unwrap().push(json! {
                        {
                            "stdout": stdout,
                            "data": data
                        }
                    });
                }
            } else {
                let stderr = String::from_utf8(output.stderr).unwrap();
                Err(format!("stderr: {}", stderr).as_str())?;
            }
            Ok(())
        }
        Self {
            project_path,
            workspaces_path,
            template_file,
            target_executable_file,
            target_executable_file_base,
            init_bash_script: init_bash_script.to_string(),
            compile_bash_script: compile_bash_script.to_string(),
            build_workers,
            run_method: RunMethod::Exclusive,
            to_target_executable_path_dir,
            run_func_data,
            data_queue_receiver: None,
            compilation_error_handling_method: CompliationErrorHandlingMethod::Panic,
            auto_gather_array_data: true,
            in_place_template: false,
            enable_progress_bar: false,
            mpb: MultiProgress::new(),
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
            if self.build_workers == -run_workers as usize {
                self.run_method = RunMethod::InPlace;
            } else {
                self.run_method = RunMethod::Exclusive;
            }
        }
        self
    }

    pub fn run_workers_exclusive(mut self) -> Self {
        self.run_method = RunMethod::Exclusive;
        self
    }

    pub fn run_method(mut self, run_method: RunMethod) -> Self {
        self.run_method = run_method;
        self
    }

    pub fn run_func(
        mut self,
        run_func: fn(&PathBuf, &PathBuf, &JsonValue, &mut JsonValue) -> Result<(), Box<dyn Error>>,
    ) -> Self {
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

    pub fn enable_progress_bar(mut self, enable_progress_bar: bool) -> Self {
        self.enable_progress_bar = enable_progress_bar;
        self
    }

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

    pub fn get_data_queue_sender(&mut self) -> Result<Sender<(usize, JsonValue)>, Box<dyn Error>> {
        if self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is already initialized".into());
        }
        let (data_queue_sender, data_queue_receiver) = unbounded();
        self.data_queue_receiver = Some(data_queue_receiver);
        Ok(data_queue_sender)
    }

    pub fn init_workspace(&self) -> Result<(), Box<dyn Error>> {
        let workspaces_path = if self.workspaces_path.is_absolute() {
            self.workspaces_path.clone()
        } else {
            env::current_dir().unwrap().join(&self.workspaces_path)
        };
        let mut project_path = self.project_path.clone();
        let move_to_temp_dir =
            workspaces_path.starts_with(std::fs::canonicalize(&self.project_path).unwrap());

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
            let enable_progress_bar = self.enable_progress_bar;
            let handle = std::thread::spawn(move || {
                let sp = Self::add_spinner2(
                    enable_progress_bar,
                    &mpb,
                    format!("init workspace {}: copying", i),
                );
                if move_to_temp_dir {
                    copy_dir(&source, &destination).unwrap();
                } else {
                    copy_dir_with_ignore(&source, &destination).unwrap();
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

        let out_of_place_run_workers = match self.run_method {
            RunMethod::OutOfPlace(run_workers) => run_workers,
            RunMethod::Exclusive => 1,
            _ => 0,
        };
        let mut run_handles = vec![];
        if out_of_place_run_workers > 0 {
            // only compile to executable when run_workers = 0
            std::fs::create_dir_all(self.workspaces_path.join("executable")).unwrap();
            for (i, destination) in
                (0..out_of_place_run_workers).map(|i| (i, format!("workspace_exe_{}", i)))
            {
                let source = project_path.clone();
                let destination = self.workspaces_path.join(destination);
                let init_bash_script = self.init_bash_script.clone();
                let compile_bash_script = self.compile_bash_script.clone();
                let in_place_template = self.in_place_template;
                let mpb = self.mpb.clone();
                let enable_progress_bar = self.enable_progress_bar;
                let handle = std::thread::spawn(move || {
                    let sp = Self::add_spinner2(
                        enable_progress_bar,
                        &mpb,
                        format!("init workspace_run {}: copying", i),
                    );
                    if move_to_temp_dir {
                        copy_dir(&source, &destination).unwrap();
                    } else {
                        copy_dir_with_ignore(&source, &destination).unwrap();
                    }
                    sp.set_message(format!("init workspace_run {}: init", i));
                    Command::new("bash")
                        .arg("-c")
                        .arg(&init_bash_script)
                        .current_dir(&destination)
                        .output()
                        .unwrap();
                    if !in_place_template {
                        Command::new("bash")
                            .arg("-c")
                            .arg(&compile_bash_script)
                            .current_dir(&destination)
                            .output()
                            .unwrap();
                    }
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

        std::fs::create_dir_all(&self.to_target_executable_path_dir).unwrap();

        Ok(())
    }

    pub fn run(&self) -> Result<(JsonValue, Vec<JsonValue>), Box<dyn Error>> {
        if !self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is not initialized".into());
        }
        let mut build_handles = vec![];
        let mut run_handles = Vec::new();
        let (executable_queue_sender, executable_queue_receiver) = unbounded();
        let data_size = self.data_queue_receiver.as_ref().unwrap().len() as u64;
        let build_pb = self.add_progress_bar("Building", data_size, "All builds done");
        let run_pb = if !matches!(self.run_method, RunMethod::No) {
            if matches!(self.run_method, RunMethod::Exclusive) {
                self.add_progress_bar("Waiting", data_size, "All runs done")
            } else {
                self.add_progress_bar("Running", data_size, "All runs done")
            }
        } else {
            ProgressBar::hidden()
        };
        let spawn_build_workers = || {
            for i in 0..self.build_workers {
                let workspace_path = self.workspaces_path.join(format!("workspace_{}", i));
                let build_handle = self.build_worker(
                    workspace_path,
                    executable_queue_sender.clone(),
                    build_pb.clone(),
                    run_pb.clone(),
                );
                build_handles.push(build_handle);
            }
            drop(executable_queue_sender);
        };
        let spawn_run_workers = || {
            if let RunMethod::OutOfPlace(run_workers) = self.run_method {
                for i in 0..run_workers {
                    let workspace_path = self.workspaces_path.join(format!("workspace_exe_{}", i));
                    let run_handle = self.run_worker(
                        workspace_path,
                        executable_queue_receiver.clone(),
                        run_pb.clone(),
                    );
                    run_handles.push(run_handle);
                }
            } else if matches!(self.run_method, RunMethod::Exclusive) {
                let workspace_path = self.workspaces_path.join("workspace_exe_0");
                let run_handle = self.run_worker(
                    workspace_path,
                    executable_queue_receiver.clone(),
                    run_pb.clone(),
                );
                run_handles.push(run_handle);
            }
            drop(executable_queue_receiver);
        };
        spawn_build_workers();
        drop(build_pb);
        if matches!(self.run_method, RunMethod::Exclusive) {
            let compile_error_datas =
                build_handles
                    .into_iter()
                    .fold(vec![], |mut compile_error_datas_array, handle| {
                        let (_, compile_error_datas) = handle.join().unwrap();
                        compile_error_datas_array.extend(compile_error_datas);
                        compile_error_datas_array
                    });
            run_pb.set_message("Running");
            spawn_run_workers(); // spawn after build workers are done
            let run_data_array = run_handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect();
            self.gather_data(run_data_array, compile_error_datas)
        } else {
            if matches!(self.run_method, RunMethod::OutOfPlace(_)) {
                spawn_run_workers(); // spawn before build workers are done
            }
            let (mut run_data_array, compile_error_datas) = build_handles.into_iter().fold(
                (vec![], vec![]),
                |(mut run_data_array, mut compile_error_datas_array), handle| {
                    let (run_data, compile_error_datas) = handle.join().unwrap();
                    run_data_array.push(run_data);
                    compile_error_datas_array.extend(compile_error_datas);
                    (run_data_array, compile_error_datas_array)
                },
            );
            if matches!(self.run_method, RunMethod::OutOfPlace(_)) {
                run_data_array = run_handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect();
            } // else run InPlace or No, use run_data_array from build workers
            self.gather_data(run_data_array, compile_error_datas)
        }
    }

    fn build_worker(
        &self,
        workspace_path: PathBuf,
        executable_queue_sender: Sender<(PathBuf, JsonValue)>,
        build_pb: ProgressBar,
        run_pb: ProgressBar,
    ) -> std::thread::JoinHandle<(JsonValue, Vec<JsonValue>)> {
        let template_path = self.project_path.join(&self.template_file);
        let target_executable_path = workspace_path.join(&self.target_executable_file);
        let compile_bash_script = self.compile_bash_script.clone();
        let template_output_file = if self.in_place_template {
            self.template_file.clone()
        } else {
            self.template_file.with_extension("")
        };
        let target_executable_file_base = self.target_executable_file_base.clone();
        let to_target_executable_path_dir = self.to_target_executable_path_dir.clone();
        let data_queue_receiver = self.data_queue_receiver.as_ref().unwrap().clone();
        let run_method = self.run_method;
        let run_func = self.run_func_data;
        let compilation_error_handling_method = self.compilation_error_handling_method;

        let template_output_path = workspace_path.join(&template_output_file);
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("default", Box::new(default_value_helper));
        handlebars
            .register_template_string("tpl", std::fs::read_to_string(&template_path).unwrap())
            .unwrap();
        let mut run_data = JsonValue::Null;
        let mut compile_error_datas = Vec::new();
        std::thread::spawn(move || {
            for (i, data) in data_queue_receiver.iter() {
                let mut template_output = std::fs::File::create(&template_output_path)
                    .expect(format!("Failed to create {:?}", template_output_path).as_str());
                handlebars
                    .render_to_write("tpl", &data, &template_output)
                    .expect(format!("Failed to render {:?}", template_output_path).as_str());
                template_output.flush().unwrap();
                let output = Command::new("bash")
                    .arg("-c")
                    .arg(&compile_bash_script)
                    .current_dir(&workspace_path)
                    .output();
                build_pb.inc(1);
                if output.is_err() || output.is_ok() && !output.as_ref().unwrap().status.success() {
                    if compilation_error_handling_method == CompliationErrorHandlingMethod::Panic {
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
                        if compilation_error_handling_method
                            == CompliationErrorHandlingMethod::Collect
                        {
                            compile_error_datas.push(data.clone());
                            continue;
                        } else if compilation_error_handling_method
                            == CompliationErrorHandlingMethod::Ignore
                        {
                            continue;
                        } else {
                            panic!("Compilation error handling method not implemented");
                        }
                    }
                }
                if matches!(run_method, RunMethod::No) {
                    let to_target_executable_path_file =
                        format!("{}_{}", &target_executable_file_base, i);
                    let to_target_executable_path =
                        to_target_executable_path_dir.join(&to_target_executable_path_file);
                    let to_target_executable_metadata_path =
                        to_target_executable_path.with_extension("json");
                    std::fs::rename(&target_executable_path, &to_target_executable_path).expect(
                        format!(
                            "Failed to rename {:?} to {:?}",
                            target_executable_path, to_target_executable_path
                        )
                        .as_str(),
                    );
                    std::fs::write(&to_target_executable_metadata_path, data.to_string()).unwrap();
                } else {
                    if matches!(run_method, RunMethod::InPlace) {
                        run_func(
                            &std::fs::canonicalize(&workspace_path).unwrap(),
                            &std::fs::canonicalize(&target_executable_path).unwrap(),
                            &data,
                            &mut run_data,
                        )
                        .unwrap();
                        run_pb.inc(1);
                    } else if matches!(run_method, RunMethod::OutOfPlace(_))
                        || matches!(run_method, RunMethod::Exclusive)
                    {
                        let to_target_executable_path_file =
                            format!("{}_{}", &target_executable_file_base, i);
                        let to_target_executable_path =
                            to_target_executable_path_dir.join(&to_target_executable_path_file);
                        std::fs::rename(&target_executable_path, &to_target_executable_path)
                            .unwrap();
                        executable_queue_sender
                            .send((to_target_executable_path, data))
                            .unwrap();
                    } else {
                        panic!("Run method not implemented");
                    }
                }
            }
            (run_data, compile_error_datas)
        })
    }

    fn run_worker(
        &self,
        workspace_path: PathBuf,
        executable_queue_receiver: Receiver<(PathBuf, JsonValue)>,
        run_pb: ProgressBar,
    ) -> std::thread::JoinHandle<JsonValue> {
        let target_executable_path = workspace_path.join(&self.target_executable_file);
        let run_func = self.run_func_data;
        let mut run_data = JsonValue::Null;
        std::thread::spawn(move || {
            for (to_target_executable_path, data) in executable_queue_receiver.iter() {
                std::fs::rename(&to_target_executable_path, &target_executable_path).unwrap();
                run_func(
                    &std::fs::canonicalize(&workspace_path).unwrap(),
                    &std::fs::canonicalize(&target_executable_path).unwrap(),
                    &data,
                    &mut run_data,
                )
                .unwrap();
                run_pb.inc(1);
            }
            run_data
        })
    }

    fn gather_data(
        &self,
        run_data_array: Vec<JsonValue>,
        compile_error_datas: Vec<JsonValue>,
    ) -> Result<(JsonValue, Vec<JsonValue>), Box<dyn Error>> {
        let run_data_array: Vec<JsonValue> = run_data_array
            .into_iter()
            .filter(|item| !item.is_null())
            .collect();
        if self.run_method == RunMethod::No {
            return Ok((JsonValue::Null, compile_error_datas));
        } else if self.auto_gather_array_data && run_data_array.iter().all(|item| item.is_array()) {
            let mut run_data = Vec::new();
            for run_data_item in run_data_array {
                run_data.extend(run_data_item.as_array().unwrap().iter().cloned());
            }
            Ok((JsonValue::Array(run_data), compile_error_datas))
        } else {
            // just return array json
            Ok((JsonValue::Array(run_data_array), compile_error_datas))
        }
    }

    fn add_progress_bar<S: Into<String>, F: Into<Cow<'static, str>>>(
        &self,
        message: S,
        total: u64,
        finish_message: F,
    ) -> ProgressBar {
        if !self.enable_progress_bar {
            return ProgressBar::hidden();
        }
        let sty = ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap();
        self.mpb.add(
            ProgressBar::new(total)
                .with_message(message.into())
                .with_style(sty)
                .with_finish(ProgressFinish::WithMessage(finish_message.into())),
        )
    }

    fn add_spinner<S: Into<String>>(&self, message: S) -> ProgressBar {
        if !self.enable_progress_bar {
            return ProgressBar::hidden();
        }
        let sp = self
            .mpb
            .add(ProgressBar::new_spinner().with_message(message.into()));
        sp.enable_steady_tick(Duration::from_millis(100));
        sp
    }

    fn add_spinner2<S: Into<String>>(
        enable_progress_bar: bool,
        mpb: &MultiProgress,
        message: S,
    ) -> ProgressBar {
        if !enable_progress_bar {
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

    #[test]
    fn test_workspaces_under_project_path() {
        println!("{}", std::fs::canonicalize(".").unwrap().display());
        let workspaces_path = PathBuf::from("workspaces_under_project_path");
        let parabuilder = Parabuilder::new(
            ".",
            &workspaces_path,
            "tests/example_project/src/main.cpp.template",
            "build/main",
        )
        .init_bash_script(
            r#"
            cmake -B build -S tests/example_project
            "#,
        )
        .compile_bash_script(
            r#"
            cmake --build build --target all -- -B
            "#,
        );
        parabuilder.init_workspace().unwrap();
        assert!(workspaces_path
            .join("workspace_0/tests/example_project/src/main.cpp.template")
            .exists());
        assert!(workspaces_path.join("workspace_0/build").exists());
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

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

    const EXAMPLE_PROJECT: &str = "tests/example_project";
    const EXAMPLE_TEMPLATE_FILE: &str = "src/main.cpp.template";
    const EXAMPLE_IN_PLACE_TEMPLATE_FILE: &str = "src/main.cpp";
    const EXAMPLE_TARGET_EXECUTABLE_FILE: &str = "build/main";
    const EXAMPLE_INIT_BASH_SCRIPT: &str = r#"
        cmake -B build -S .
        "#;
    const EXAMPLE_IN_PLACE_INIT_BASH_SCRIPT: &str = r#"
        cmake -B build -S . -DPROFILING=ON
        "#;
    const EXAMPLE_COMPILE_BASH_SCRIPT: &str = r#"
        cmake --build build --target all -- -B
        "#;

    const SINGLETHREADED_N: i64 = 20;
    const MULTITHREADED_N: i64 = 100;

    fn parabuild_tester(
        name: &str,
        size: i64,
        build_workers: usize,
        run_method: RunMethod,
        in_place_template: bool,
    ) {
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
            EXAMPLE_PROJECT,
            &workspaces_path,
            if in_place_template {
                EXAMPLE_IN_PLACE_TEMPLATE_FILE
            } else {
                EXAMPLE_TEMPLATE_FILE
            },
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(if in_place_template {
            EXAMPLE_IN_PLACE_INIT_BASH_SCRIPT
        } else {
            EXAMPLE_INIT_BASH_SCRIPT
        })
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(build_workers)
        .run_method(run_method)
        .run_func(run_func)
        .compilation_error_handling_method(CompliationErrorHandlingMethod::Collect)
        .in_place_template(in_place_template);
        parabuilder.set_datas(datas).unwrap();
        parabuilder.init_workspace().unwrap();
        let (run_data, compile_error_datas) = parabuilder.run().unwrap();
        assert!(
            compile_error_datas == vec![error_data],
            "got: {:?} {:?}",
            run_data,
            compile_error_datas
        );
        if matches!(run_method, RunMethod::No) {
            assert!(run_data.is_null(), "got: {}", run_data);
            for i in 0..size {
                assert!(workspaces_path
                    .join(format!("executable/main_{}", i))
                    .exists());
                assert!(workspaces_path
                    .join(format!("executable/main_{}.json", i))
                    .exists());
            }
        } else {
            let ground_truth = if size > 0 {
                (1..=size).sum::<i64>()
            } else {
                42
            };
            assert!(run_data.is_array());
            let sum = run_data
                .as_array()
                .unwrap()
                .iter()
                .fold(0, |acc, item| acc + item.as_i64().unwrap());
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
        );
    }

    #[test]
    fn test_multithreaded_parabuild_exclusive_run() {
        parabuild_tester(
            "test_multithreaded_parabuild_exclusive_run",
            MULTITHREADED_N,
            4,
            RunMethod::Exclusive,
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
        );
    }

    #[test]
    fn test_multithreaded_parabuild_out_of_place_run_default_template() {
        parabuild_tester(
            "est_multithreaded_parabuild_out_of_place_run_default_template",
            0,
            4,
            RunMethod::OutOfPlace(2),
            true,
        );
    }
}
