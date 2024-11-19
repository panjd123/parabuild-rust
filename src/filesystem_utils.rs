use fs_extra;
use ignore;
use std::{path::Path, process::Command};

pub fn copy_dir<P, Q>(from: P, to: Q) -> Result<(), fs_extra::error::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    fs_extra::dir::create_all(&to, false)?;
    let options = fs_extra::dir::CopyOptions::new()
        .overwrite(true)
        .copy_inside(true)
        .content_only(true);
    fs_extra::dir::copy(from, to, &options)?;
    Ok(())
}

pub fn copy_dir_with_ignore<P, Q>(from: P, to: Q) -> Result<(), std::io::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    for entry in ignore::WalkBuilder::new(&from).git_ignore(true).build() {
        match entry {
            Ok(ref entry) => {
                let path = entry.path();
                if path.is_file() {
                    let relative_path = path
                        .strip_prefix(from.as_ref())
                        .expect("Failed to strip prefix");
                    let destination = to.as_ref().join(relative_path);
                    if let Some(parent) = destination.parent() {
                        std::fs::create_dir_all(parent).expect("Failed to create parent directory");
                    }
                    std::fs::copy(path, destination).expect("Failed to copy file");
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
    Ok(())
}

pub fn wait_until_file_ready(file_path: &Path) -> Result<(), std::io::Error> {
    use std::thread::sleep;
    use std::time::Duration;
    let mut attempts = 0;
    fn ready(file_path: &Path) -> bool {
        if !file_path.exists() {
            return false;
        }
        let output = Command::new("lsof").arg(file_path).output().unwrap();
        if output.stdout.is_empty() {
            return true;
        }
        eprintln!(
            "Waiting for file to be ready: {:?}, {:?}",
            file_path, output.stdout
        );
        false
    }
    while !ready(file_path) {
        attempts += 1;
        if attempts > 100 {
            if !file_path.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found: {:?}", file_path),
                ));
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("File is not ready: {:?}", file_path),
                ));
            }
        }
        sleep(Duration::from_millis(100));
    }
    Ok(())
}

pub fn is_command_installed(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const EXAMPLE_PROJECT: &str = crate::test_constants::EXAMPLE_CMAKE_PROJECT_PATH;

    #[test]
    fn test_copy_dir() {
        let source = Path::new(EXAMPLE_PROJECT);
        let destination = &tempdir().unwrap().into_path();
        println!("source: {:?}", source);
        println!("destination: {:?}", destination);
        copy_dir(source, destination).unwrap();
        let main_file = destination.join("src/main.cpp.template");
        let ignore_file = destination.join("src/example.ignore");
        let gitignore_file = destination.join(".gitignore");
        assert!(main_file.exists());
        assert!(ignore_file.exists());
        assert!(gitignore_file.exists());
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn test_copy_dir_with_ignore() {
        let source = Path::new(EXAMPLE_PROJECT);
        let destination = &tempdir().unwrap().into_path();
        println!("destination: {:?}", destination);
        fs_extra::dir::remove(destination).unwrap();
        copy_dir_with_ignore(source, destination).unwrap();
        let main_file = destination.join("src/main.cpp.template");
        let ignore_file = destination.join("src/example.ignore");
        let gitignore_file = destination.join(".gitignore");
        assert!(main_file.exists());
        assert!(!ignore_file.exists());
        assert!(!gitignore_file.exists());
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn test_is_command_installed() {
        assert!(is_command_installed("ls"));
        assert!(!is_command_installed("ls_not_exist"));
    }
}
