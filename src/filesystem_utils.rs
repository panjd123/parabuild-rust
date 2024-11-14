use fs_extra;
use ignore;
use std::path::Path;

#[allow(dead_code)]
pub fn copy_dir<P, Q>(from: P, to: Q) -> Result<(), fs_extra::error::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let mut options = fs_extra::dir::CopyOptions::new();
    options.overwrite = true;
    options.copy_inside = true;
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
                        println!("Creating parent directory: {:?}", parent);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::{Seek, Write};
    use std::time::Instant;

    static EXAMPLE_PROJECT: &str = "tests/example_project";
    static EXAMPLE_PROJECT_COPY: &str = "tests/example_project_copy";

    #[test]
    #[serial]
    fn test_copy_dir() {
        let source = Path::new(EXAMPLE_PROJECT);
        let destination = Path::new(EXAMPLE_PROJECT_COPY);
        fs_extra::dir::remove(destination).unwrap();
        let result = copy_dir(source, destination);
        assert!(result.is_ok());
        let main_file = destination.join("src/main.cpp.template");
        let ignore_file = destination.join("src/example.ignore");
        let gitignore_file = destination.join(".gitignore");
        assert!(main_file.exists());
        assert!(ignore_file.exists());
        assert!(gitignore_file.exists());
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    #[serial]
    fn test_copy_dir_with_ignore() {
        let source = Path::new(EXAMPLE_PROJECT);
        let destination = Path::new(EXAMPLE_PROJECT_COPY);
        fs_extra::dir::remove(destination).unwrap();
        let result = copy_dir_with_ignore(source, destination);
        assert!(result.is_ok());
        let main_file = destination.join("src/main.cpp.template");
        let ignore_file = destination.join("src/example.ignore");
        let gitignore_file = destination.join(".gitignore");
        assert!(main_file.exists());
        assert!(!ignore_file.exists());
        assert!(!gitignore_file.exists());
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    #[serial]
    fn test_multithreaded_copy_dir_with_ignore() {
        const NUM_THREADS: usize = 16;
        fn multithreaded_clean() {
            let mut handles = vec![];
            for destination in (0..NUM_THREADS).map(|i| format!("{}_{}", EXAMPLE_PROJECT_COPY, i)) {
                let handle = std::thread::spawn(move || {
                    fs_extra::dir::remove(&destination).unwrap();
                });
                handles.push(handle);
            }
            for handle in handles {
                handle.join().unwrap();
            }
        }
        let source = Path::new(EXAMPLE_PROJECT);
        // setup huge file
        let huge_file = source.join("src/huge_file");
        let mut file = std::fs::File::create(&huge_file).unwrap();
        file.seek(std::io::SeekFrom::End(16 * 1024 * 1024)).unwrap(); // 16 MB
        file.write_all(b"0").unwrap();
        file.flush().unwrap();

        multithreaded_clean();
        let start = Instant::now();
        let mut handles = vec![];
        for destination in (0..NUM_THREADS).map(|i| format!("{}_{}", EXAMPLE_PROJECT_COPY, i)) {
            let source = source.to_path_buf();
            let destination = Path::new(&destination).to_path_buf();
            let handle = std::thread::spawn(move || {
                copy_dir_with_ignore(&source, &destination).unwrap();
            });
            handles.push(handle);
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let duration = start.elapsed();
        println!(
            "Time elapsed in multithreaded copy_dir_with_ignore() is: {:?}",
            duration
        );
        multithreaded_clean();
        let start2 = Instant::now();
        for destination in (0..NUM_THREADS).map(|i| format!("{}_{}", EXAMPLE_PROJECT_COPY, i)) {
            copy_dir_with_ignore(&source, &destination).unwrap();
        }
        let duration2 = start2.elapsed();
        println!(
            "Time elapsed in single-threaded copy_dir_with_ignore() is: {:?}",
            duration2
        );
        multithreaded_clean();

        fs_extra::file::remove(huge_file).unwrap();
    }
}
