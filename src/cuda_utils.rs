use regex::Regex;
use std::process::Command;

pub fn get_cuda_device_uuids() -> Vec<String> {
    match Command::new("nvidia-smi").arg("-L").output() {
        Ok(output) => {
            let output = String::from_utf8(output.stdout).unwrap();
            let re = Regex::new(r"\(UUID: (MIG-[a-z0-9\-]+)\)").unwrap();
            re.captures_iter(&output)
                .map(|cap| cap[1].to_string())
                .collect()
        }
        Err(_) => Vec::new(),
    }
}
