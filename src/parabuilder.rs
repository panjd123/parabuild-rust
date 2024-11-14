use crate::filesystem_utils::copy_dir_with_ignore;
use handlebars::Handlebars;
use serde_json::Value as JsonValue;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
// use std::sync::{Arc, Mutex};

pub struct Parabuilder {
    project_path: PathBuf,
    workspaces_path: PathBuf,
    template_file: PathBuf,
    template_output_file: PathBuf,
    target_executable_file: PathBuf,
    target_executable_file_base: String,
    init_bash_script: String,
    compile_bash_script: String,
    num_threads: usize,
    datas: Vec<JsonValue>,
}

impl Parabuilder {
    pub fn new<P>(
        project_path: P,
        workspaces_path: P,
        template_file: P,
        target_executable_file: P,
        init_bash_script: &str,
        compile_bash_script: &str,
        num_threads: usize,
    ) -> Self
    where
        P: AsRef<Path>,
    {
        let project_path = project_path.as_ref().to_path_buf();
        let workspaces_path = workspaces_path.as_ref().to_path_buf();
        let template_file = template_file.as_ref().to_path_buf();
        let template_output_file = template_file.with_extension("");
        let target_executable_file = target_executable_file.as_ref().to_path_buf();
        let target_executable_file_base =target_executable_file.file_name().unwrap().to_string_lossy().to_string();
        Self {
            project_path,
            workspaces_path,
            template_file,
            template_output_file,
            target_executable_file,
            target_executable_file_base,
            init_bash_script: init_bash_script.to_string(),
            compile_bash_script: compile_bash_script.to_string(),
            num_threads,
            datas: vec![],
        }
    }

    pub fn add_data(&mut self, data: JsonValue) {
        self.datas.push(data);
    }

    pub fn add_datas(&mut self, datas: Vec<JsonValue>) {
        self.datas.extend(datas);
    }

    pub fn init_workspace(&self) -> Result<(), Box<dyn Error>> {
        let mut handles = vec![];
        for destination in (0..self.num_threads).map(|i| format!("workspace_{}", i)) {
            let source = self.project_path.clone();
            let destination = self.workspaces_path.join(destination);
            println!("Copying from {:?} to {:?}", source, destination);
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
        std::fs::create_dir_all(self.workspaces_path.join("executable")).expect(
            format!(
                "Failed to create {:?}",
                self.workspaces_path.join("executable")
            )
            .as_str(),
        );
        Ok(())
    }

    pub fn run(&self) -> Result<(), Box<dyn Error>> {
        if self.num_threads == 1 {
            self.singlethreaded_run()?;
        } else {
            unimplemented!();
        }
        Ok(())
    }

    pub fn singlethreaded_run(&self) -> Result<(), Box<dyn Error>> {
        let mut handlebars = Handlebars::new();
        let workspace_path = self.workspaces_path.join("workspace_0");
        let template_path = workspace_path.join(&self.template_file);
        let target_executable_path = workspace_path.join(&self.target_executable_file);
        let to_target_executable_path = self
            .workspaces_path
            .join("executable")
            .join(&self.target_executable_file_base)
            .to_string_lossy()
            .to_string();
        handlebars
            .register_template_file("tpl", &template_path)
            .unwrap();
        let template_output_path = workspace_path.join(&self.template_output_file);
        for (i, data) in self.datas.iter().enumerate() {
            let template_output = std::fs::File::create(&template_output_path)
                .expect(format!("Failed to create {:?}", template_output_path).as_str());
            handlebars
                .render_to_write("tpl", data, template_output)
                .expect(format!("Failed to render {:?}", template_output_path).as_str());
            Command::new("bash")
                .arg("-c")
                .arg(&self.compile_bash_script)
                .current_dir(&workspace_path)
                .output()
                .unwrap();
            let to_target_executable_path = format!("{}_{}", to_target_executable_path, i);
            let to_target_executable_path_metadata = format!("{}.json", to_target_executable_path);
            std::fs::rename(&target_executable_path, &to_target_executable_path).expect(
                format!(
                    "Failed to rename {:?} to {}",
                    target_executable_path, &to_target_executable_path
                )
                .as_str(),
            );
            std::fs::write(&to_target_executable_path_metadata, data.to_string()).expect(
                format!("Failed to write {:?}", to_target_executable_path_metadata).as_str(),
            );
        }
        Ok(())
    }

    // pub fn
}

#[cfg(test)]
mod tests {
    use super::*;
    // use std::time::Instant;
    use serde_json::json;
    use serial_test::serial;
    use std::path::Path;

    const EXAMPLE_PROJECT: &str = "tests/example_project";
    const EXAMPLE_WORKSPACE: &str = "tests/workspaces";
    const EXAMPLE_TEMPLATE_FILE: &str = "src/main.cpp.template";
    const EXAMPLE_TARGET_EXECUTABLE_FILE: &str = "build/main";
    const EXAMPLE_INIT_BASH_SCRIPT: &str = r#"
        cmake -B build -S .
        "#;
    const EXAMPLE_COMPILE_BASH_SCRIPT: &str = r#"
        cmake --build build --target all
        "#;

    #[test]
    #[serial]
    fn test_singlethreaded_parabuild() {
        let datas = vec![json!({"N": "10"}), json!({"N": "20"})];
        let mut parabuild = Parabuilder::new(
            EXAMPLE_PROJECT,
            EXAMPLE_WORKSPACE,
            EXAMPLE_TEMPLATE_FILE,
            EXAMPLE_TARGET_EXECUTABLE_FILE,
            EXAMPLE_INIT_BASH_SCRIPT,
            EXAMPLE_COMPILE_BASH_SCRIPT,
            1,
        );
        parabuild.add_datas(datas);
        parabuild.init_workspace().unwrap();
        parabuild.singlethreaded_run().unwrap();
        assert!(Path::new("tests/workspaces/executable/main_0").exists());
        assert!(Path::new("tests/workspaces/executable/main_1").exists());
        assert!(Path::new("tests/workspaces/executable/main_0.json").exists());
        assert!(Path::new("tests/workspaces/executable/main_1.json").exists());
        std::fs::remove_dir_all("tests/workspaces").unwrap();
    }
}
