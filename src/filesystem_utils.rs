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

pub fn copy_dir_with_rsync(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    let from_ends_with_slash = if from.ends_with("/") {
        from.to_str().unwrap().to_string()
    } else {
        format!("{}/", from.to_str().unwrap())
    };
    let to_ends_with_slash = if to.ends_with("/") {
        to.to_str().unwrap().to_string()
    } else {
        format!("{}/", to.to_str().unwrap())
    };
    let gitignore_file = from.join(".gitignore");
    let mut output = Command::new("rsync");
    output.arg("-a");
    if gitignore_file.exists() {
        output.arg(format!(
            "--exclude-from={}",
            gitignore_file.to_str().unwrap()
        ));
    }
    let output = output
        .arg(from_ends_with_slash)
        .arg(to_ends_with_slash)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to copy directory: {:?}", output),
        ));
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
    Command::new(command).arg("--version").output().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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

    #[test]
    fn test_copy_dir_with_rsync() {
        fn get_mtime(path: &Path) -> std::io::Result<std::time::SystemTime> {
            std::fs::metadata(path).map(|meta| meta.modified())?
        }
        let example_project_dir = Path::new(crate::test_constants::EXAMPLE_CMAKE_PROJECT_PATH);
        let working_dir = tempdir().unwrap().into_path();
        copy_dir(example_project_dir, &working_dir).unwrap();
        let ignore_path = working_dir.join("src/example.ignore");
        assert!(ignore_path.exists());
        let file_path = working_dir.join("src/example.cpp");
        let main_path = working_dir.join("src/main.cpp");
        let main_old_mtime = get_mtime(&main_path).unwrap();
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "Hello, ").unwrap();
        file.sync_all().unwrap();
        let destination = tempdir().unwrap().into_path();
        copy_dir_with_rsync(&working_dir, &destination).unwrap();
        let ignore_destination = destination.join("src/example.ignore");
        let file_destination = destination.join("src/example.cpp");
        let main_destination = destination.join("src/main.cpp");
        assert!(!ignore_destination.exists());
        assert!(file_destination.exists());
        writeln!(file, "world!").unwrap();
        file.sync_all().unwrap();
        copy_dir_with_rsync(&working_dir, &destination).unwrap();
        assert_eq!(
            std::fs::read_to_string(file_destination).unwrap(),
            "Hello, world!\n"
        );
        assert_eq!(main_old_mtime, get_mtime(&main_destination).unwrap(),);
        std::fs::remove_dir_all(working_dir).unwrap();
        std::fs::remove_dir_all(destination).unwrap();
    }
}
