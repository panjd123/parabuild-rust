use crate::filesystem_utils::copy_dir_with_ignore;
use crossbeam_channel::{unbounded, Receiver, Sender};
use handlebars::Handlebars;
use serde_json::{json, Value as JsonValue};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Parabuilder {
    project_path: PathBuf,
    workspaces_path: PathBuf,
    template_file: PathBuf,
    template_output_file: PathBuf,
    target_executable_file: PathBuf,
    target_executable_file_base: String,
    init_bash_script: String,
    compile_bash_script: String,
    build_workers: usize,
    run_workers: isize,
    to_target_executable_path_dir: PathBuf,
    run_func_data: fn(&PathBuf, &PathBuf, &JsonValue, &mut JsonValue) -> Result<(), Box<dyn Error>>,
    data_queue_receiver: Option<Receiver<(usize, JsonValue)>>,
    force_exclusive_run: bool,
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
        let template_output_file = template_file.with_extension("");
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
        cmake --build build --target all
        "#;
        let build_workers = 1;
        let run_workers = -1;
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
            template_output_file,
            target_executable_file,
            target_executable_file_base,
            init_bash_script: init_bash_script.to_string(),
            compile_bash_script: compile_bash_script.to_string(),
            build_workers,
            run_workers,
            to_target_executable_path_dir,
            run_func_data,
            data_queue_receiver: None,
            force_exclusive_run: false,
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
        self.run_workers = run_workers;
        self
    }

    pub fn run_func(
        mut self,
        run_func: fn(&PathBuf, &PathBuf, &JsonValue, &mut JsonValue) -> Result<(), Box<dyn Error>>,
    ) -> Self {
        self.run_func_data = run_func;
        self
    }

    pub fn force_exclusive_run(mut self, force_exclusive_run: bool) -> Self {
        self.force_exclusive_run = force_exclusive_run;
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
        {
            let mut handles = vec![];
            for destination in (0..self.build_workers).map(|i| format!("workspace_{}", i)) {
                let source = self.project_path.clone();
                let destination = self.workspaces_path.join(destination);
                let init_bash_script = self.init_bash_script.clone();
                let handle = std::thread::spawn(move || {
                    copy_dir_with_ignore(&source, &destination).unwrap();
                    Command::new("bash")
                        .arg("-c")
                        .arg(&init_bash_script)
                        .current_dir(&destination)
                        .output()
                        .unwrap();
                });
                handles.push(handle);
            }
            for handle in handles {
                handle.join().unwrap();
            }
        }
        if self.run_workers >= 0 {
            // only compile to executable when run_workers = 0
            let mut handles = vec![];
            std::fs::create_dir_all(self.workspaces_path.join("executable")).unwrap();
            for destination in (0..self.run_workers).map(|i| format!("workspace_exe_{}", i)) {
                let source = self.project_path.clone();
                let destination = self.workspaces_path.join(destination);
                let init_bash_script = self.init_bash_script.clone();
                let compile_bash_script = self.compile_bash_script.clone();
                let handle = std::thread::spawn(move || {
                    copy_dir_with_ignore(&source, &destination).unwrap();
                    Command::new("bash")
                        .arg("-c")
                        .arg(&init_bash_script)
                        .current_dir(&destination)
                        .output()
                        .unwrap();
                    Command::new("bash")
                        .arg("-c")
                        .arg(&compile_bash_script)
                        .current_dir(&destination)
                        .output()
                        .unwrap();
                });
                handles.push(handle);
            }
            for handle in handles {
                handle.join().unwrap();
            }
        } else {
            // run in the same workspace
        }
        Ok(())
    }

    pub fn run(&self) -> Result<JsonValue, Box<dyn Error>> {
        if self.build_workers == 1 && self.run_workers <= 0 {
            self.singlethreaded_run()
        } else {
            if self.force_exclusive_run {
                assert!(self.run_workers == 1);
            }
            self.multithreaded_run()
        }
    }

    pub fn singlethreaded_run(&self) -> Result<JsonValue, Box<dyn Error>> {
        let mut handlebars = Handlebars::new();
        let workspace_path = self.workspaces_path.join("workspace_0");
        let template_path = workspace_path.join(&self.template_file);
        let target_executable_path = workspace_path.join(&self.target_executable_file);
        handlebars
            .register_template_file("tpl", &template_path)
            .unwrap();
        let template_output_path = workspace_path.join(&self.template_output_file);
        let mut run_data = JsonValue::Null;
        if !self.data_queue_receiver.is_some() {
            return Err("Data queue receiver is not initialized".into());
        }
        let data_queue_receiver = self.data_queue_receiver.as_ref().unwrap();
        for (i, data) in data_queue_receiver.iter() {
            let template_output = std::fs::File::create(&template_output_path)
                .expect(format!("Failed to create {:?}", template_output_path).as_str());
            handlebars
                .render_to_write("tpl", &data, template_output)
                .expect(format!("Failed to render {:?}", template_output_path).as_str());
            Command::new("bash")
                .arg("-c")
                .arg(&self.compile_bash_script)
                .current_dir(&workspace_path)
                .output()
                .unwrap();

            if self.run_workers == 0 {
                let to_target_executable_path_file =
                    format!("{}_{}", &self.target_executable_file_base, i);
                let to_target_executable_path = self
                    .to_target_executable_path_dir
                    .join(&to_target_executable_path_file);
                let to_target_executable_metadata_path =
                    to_target_executable_path.with_extension("json");
                std::fs::rename(&target_executable_path, &to_target_executable_path).unwrap();
                std::fs::write(&to_target_executable_metadata_path, data.to_string()).unwrap();
            } else {
                // self.run_workers == -1
                let run_func = self.run_func_data;
                run_func(
                    &std::fs::canonicalize(&workspace_path).unwrap(),
                    &std::fs::canonicalize(&target_executable_path).unwrap(),
                    &data,
                    &mut run_data,
                )
                .unwrap();
            }
        }
        Ok(run_data)
    }

    pub fn multithreaded_run(&self) -> Result<JsonValue, Box<dyn Error>> {
        if self.run_workers <= 0 {
            self.multithreaded_run_in_place()
        } else {
            self.multithreaded_run_out_of_place()
        }
    }

    pub fn multithreaded_run_in_place(&self) -> Result<JsonValue, Box<dyn Error>> {
        let mut handles = vec![];
        for i in 0..self.build_workers {
            let workspace_path = self.workspaces_path.join(format!("workspace_{}", i));
            let template_path = workspace_path.join(&self.template_file);
            let target_executable_path = workspace_path.join(&self.target_executable_file);
            let template_output_path = workspace_path.join(&self.template_output_file);
            let mut handlebars = Handlebars::new();
            handlebars
                .register_template_file("tpl", &template_path)
                .unwrap();
            let compile_bash_script = self.compile_bash_script.clone();
            let target_executable_file_base = self.target_executable_file_base.clone();
            let to_target_executable_path_dir = self.to_target_executable_path_dir.clone();
            if !self.data_queue_receiver.is_some() {
                return Err("Data queue receiver is not initialized".into());
            }
            let data_queue_receiver = self.data_queue_receiver.as_ref().unwrap().clone();
            let run_workers = self.run_workers;
            let mut run_data = JsonValue::Null;
            let run_func = self.run_func_data;
            let handle = std::thread::spawn(move || {
                for (i, data) in data_queue_receiver.iter() {
                    let template_output = std::fs::File::create(&template_output_path)
                        .expect(format!("Failed to create {:?}", template_output_path).as_str());
                    handlebars
                        .render_to_write("tpl", &data, template_output)
                        .expect(format!("Failed to render {:?}", template_output_path).as_str());
                    Command::new("bash")
                        .arg("-c")
                        .arg(&compile_bash_script)
                        .current_dir(&workspace_path)
                        .output()
                        .unwrap();
                    if run_workers == 0 {
                        let to_target_executable_path_file =
                            format!("{}_{}", &target_executable_file_base, i);
                        let to_target_executable_path =
                            to_target_executable_path_dir.join(&to_target_executable_path_file);
                        let to_target_executable_metadata_path =
                            to_target_executable_path.with_extension("json");
                        std::fs::rename(&target_executable_path, &to_target_executable_path)
                            .unwrap();
                        std::fs::write(&to_target_executable_metadata_path, data.to_string())
                            .unwrap();
                    } else {
                        run_func(
                            &std::fs::canonicalize(&workspace_path).unwrap(),
                            &std::fs::canonicalize(&target_executable_path).unwrap(),
                            &data,
                            &mut run_data,
                        )
                        .unwrap();
                    }
                }
                run_data
            });
            handles.push(handle);
        }
        let run_data_array = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<JsonValue>>();
        if run_data_array.iter().all(|item| item.is_null()) {
            return Ok(JsonValue::Null);
        } else if run_data_array[0].is_array() {
            let mut run_data = Vec::new();
            for run_data_item in run_data_array {
                run_data.extend(run_data_item.as_array().unwrap().iter().cloned());
            }
            Ok(JsonValue::Array(run_data))
        } else {
            // just return array json
            Ok(JsonValue::Array(run_data_array))
        }
    }

    pub fn multithreaded_run_out_of_place(&self) -> Result<JsonValue, Box<dyn Error>> {
        let mut build_handles = vec![];
        let (executable_queue_sender, executable_queue_receiver) = unbounded();
        for i in 0..self.build_workers {
            let workspace_path = self.workspaces_path.join(format!("workspace_{}", i));
            let template_path = workspace_path.join(&self.template_file);
            let target_executable_path = workspace_path.join(&self.target_executable_file);
            let template_output_path = workspace_path.join(&self.template_output_file);
            let mut handlebars = Handlebars::new();
            handlebars
                .register_template_file("tpl", &template_path)
                .unwrap();
            let compile_bash_script = self.compile_bash_script.clone();
            let target_executable_file_base = self.target_executable_file_base.clone();
            let to_target_executable_path_dir = self.to_target_executable_path_dir.clone();
            if !self.data_queue_receiver.is_some() {
                return Err("Data queue receiver is not initialized".into());
            }
            let data_queue_receiver = self.data_queue_receiver.as_ref().unwrap().clone();
            let executable_queue_sender_clone = executable_queue_sender.clone();
            let handle = std::thread::spawn(move || {
                for (i, data) in data_queue_receiver.iter() {
                    let template_output = std::fs::File::create(&template_output_path)
                        .expect(format!("Failed to create {:?}", template_output_path).as_str());
                    handlebars
                        .render_to_write("tpl", &data, template_output)
                        .expect(format!("Failed to render {:?}", template_output_path).as_str());
                    Command::new("bash")
                        .arg("-c")
                        .arg(&compile_bash_script)
                        .current_dir(&workspace_path)
                        .output()
                        .unwrap();
                    let to_target_executable_path_file =
                        format!("{}_{}", &target_executable_file_base, i);
                    let to_target_executable_path =
                        to_target_executable_path_dir.join(&to_target_executable_path_file);
                    // let to_target_executable_metadata_path =
                    //     to_target_executable_path.with_extension("json");
                    std::fs::rename(&target_executable_path, &to_target_executable_path).unwrap();
                    // std::fs::write(&to_target_executable_metadata_path, data.to_string()).unwrap();
                    executable_queue_sender_clone
                        .send((to_target_executable_path, data))
                        .unwrap();
                }
            });
            build_handles.push(handle);
        }
        drop(executable_queue_sender);
        let mut handles = vec![];
        for i in 0..self.run_workers {
            let workspace_path = self.workspaces_path.join(format!("workspace_exe_{}", i));
            let target_executable_path = workspace_path.join(&self.target_executable_file);
            let run_func = self.run_func_data;
            let mut run_data = JsonValue::Null;
            let executable_queue_receiver = executable_queue_receiver.clone();
            let handle = std::thread::spawn(move || {
                for (to_target_executable_path, data) in executable_queue_receiver.iter() {
                    std::fs::rename(&to_target_executable_path, &target_executable_path).unwrap();
                    run_func(
                        &std::fs::canonicalize(&workspace_path).unwrap(),
                        &std::fs::canonicalize(&target_executable_path).unwrap(),
                        &data,
                        &mut run_data,
                    )
                    .unwrap();
                }
                run_data
            });
            handles.push(handle);
        }
        for handle in build_handles {
            handle.join().unwrap();
        }
        let run_data_array = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<JsonValue>>();
        if run_data_array.iter().all(|item| item.is_null()) {
            return Ok(JsonValue::Null);
        } else if self.run_workers == 1 {
            Ok(run_data_array[0].clone())
        } else if run_data_array[0].is_array() {
            let mut run_data = Vec::new();
            for run_data_item in run_data_array {
                run_data.extend(run_data_item.as_array().unwrap().iter().cloned());
            }
            Ok(JsonValue::Array(run_data))
        } else {
            // just return array json
            Ok(JsonValue::Array(run_data_array))
        }
    }

    // pub fn
}

#[cfg(test)]
mod tests {
    use super::*;
    // use std::time::Instant;
    use serde_json::json;
    use std::path::Path;

    const EXAMPLE_PROJECT: &str = "tests/example_project";
    const EXAMPLE_TEMPLATE_FILE: &str = "src/main.cpp.template";
    const EXAMPLE_TARGET_EXECUTABLE_FILE: &str = "build/main";
    const EXAMPLE_INIT_BASH_SCRIPT: &str = r#"
        cmake -B build -S .
        "#;
    const EXAMPLE_COMPILE_BASH_SCRIPT: &str = r#"
        cmake --build build --target all
        "#;

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

    #[test]
    fn test_singlethreaded_parabuild_without_run() {
        let datas = vec![json!({"N": "10"}), json!({"N": "20"})];
        let workspaces_path =
            Path::new("tests/workspaces_test_singlethreaded_parabuild_without_run");
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            workspaces_path,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(EXAMPLE_INIT_BASH_SCRIPT)
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(1)
        .run_workers(0);
        parabuild.set_datas(datas).unwrap();
        parabuild.init_workspace().unwrap();
        parabuild.run().unwrap();
        assert!(workspaces_path.join("executable/main_0").exists());
        assert!(workspaces_path.join("executable/main_1").exists());
        assert!(workspaces_path.join("executable/main_0.json").exists());
        assert!(workspaces_path.join("executable/main_1.json").exists());
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    #[test]
    fn test_singlethreaded_parabuild_run() {
        let datas = vec![json!({"N": "10"}), json!({"N": "20"})];
        let workspaces_path = Path::new("tests/workspaces_test_singlethreaded_parabuild_run");
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            workspaces_path,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(EXAMPLE_INIT_BASH_SCRIPT)
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(1)
        .run_workers(-1)
        .run_func(run_func);
        parabuild.set_datas(datas).unwrap();
        parabuild.init_workspace().unwrap();
        let run_data = parabuild.run().unwrap();
        assert!(run_data.is_i64());
        assert!(run_data.as_i64().unwrap() == 30);
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    #[test]
    fn test_multithreaded_parabuild_without_run() {
        let datas = (1..=20)
            .map(|i| json!({"N": i}))
            .collect::<Vec<JsonValue>>();
        let workspaces_path =
            Path::new("tests/workspaces_test_multithreaded_parabuild_without_run");
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            workspaces_path,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(EXAMPLE_INIT_BASH_SCRIPT)
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(4)
        .run_workers(0);
        parabuild.set_datas(datas).unwrap();
        parabuild.init_workspace().unwrap();
        let run_data = parabuild.run().unwrap();
        assert!(run_data.is_null());
        assert!(workspaces_path.join("executable/main_0").exists());
        assert!(workspaces_path.join("executable/main_0.json").exists());
        assert!(workspaces_path.join("executable/main_1").exists());
        assert!(workspaces_path.join("executable/main_1.json").exists());
        assert!(workspaces_path.join("executable/main_2").exists());
        assert!(workspaces_path.join("executable/main_2.json").exists());
        assert!(workspaces_path.join("executable/main_3").exists());
        assert!(workspaces_path.join("executable/main_3.json").exists());
        assert!(workspaces_path.join("executable/main_10").exists());
        assert!(workspaces_path.join("executable/main_10.json").exists());
        assert!(workspaces_path.join("executable/main_11").exists());
        assert!(workspaces_path.join("executable/main_11.json").exists());
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    #[test]
    fn test_multithreaded_parabuild_in_place_run() {
        let datas = (1..=20)
            .map(|i| json!({"N": i}))
            .collect::<Vec<JsonValue>>();
        let workspaces_path =
            Path::new("tests/workspaces_test_multithreaded_parabuild_in_place_run");
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
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            workspaces_path,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(EXAMPLE_INIT_BASH_SCRIPT)
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(4)
        .run_workers(-1)
        .run_func(run_func);
        parabuild.set_datas(datas).unwrap();
        parabuild.init_workspace().unwrap();
        let run_data = parabuild.run().unwrap();
        assert!(run_data.is_array());
        let ground_truth = (1..=20).sum::<i64>();
        let sum = run_data
            .as_array()
            .unwrap()
            .iter()
            .fold(0, |acc, item| acc + item.as_i64().unwrap());
        assert!(sum == ground_truth);
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    #[test]
    fn test_multithreaded_parabui_out_of_place_single_run() {
        let datas = (1..=20)
            .map(|i| json!({"N": i}))
            .collect::<Vec<JsonValue>>();
        let workspaces_path =
            Path::new("tests/workspaces_test_multithreaded_parabui_out_of_place_single_run");
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
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            workspaces_path,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(EXAMPLE_INIT_BASH_SCRIPT)
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(4)
        .run_workers(1)
        .run_func(run_func);
        parabuild.set_datas(datas).unwrap();
        parabuild.init_workspace().unwrap();
        let run_data = parabuild.run().unwrap();
        assert!(run_data.is_i64());
        let ground_truth = (1..=20).sum::<i64>();
        assert!(run_data.as_i64().unwrap() == ground_truth);
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }

    #[test]
    fn test_multithreaded_parabui_out_of_place_run() {
        let datas = (1..=20)
            .map(|i| json!({"N": i}))
            .collect::<Vec<JsonValue>>();
        let workspaces_path =
            Path::new("tests/workspaces_test_multithreaded_parabui_out_of_place_run");
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
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            workspaces_path,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
        )
        .init_bash_script(EXAMPLE_INIT_BASH_SCRIPT)
        .compile_bash_script(EXAMPLE_COMPILE_BASH_SCRIPT)
        .build_workers(4)
        .run_workers(2)
        .run_func(run_func);
        parabuild.set_datas(datas).unwrap();
        parabuild.init_workspace().unwrap();
        let run_data = parabuild.run().unwrap();
        assert!(run_data.is_array());
        let ground_truth = (1..=20).sum::<i64>();
        let sum = run_data
            .as_array()
            .unwrap()
            .iter()
            .fold(0, |acc, item| acc + item.as_i64().unwrap());
        assert!(sum == ground_truth);
        std::fs::remove_dir_all(workspaces_path).unwrap();
    }
}
