use jelly::{DOWNLOAD_DIR, HEADLESS_PROFILE_DIR, PROFILE_DIR};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directories = [
        PathBuf::from(DOWNLOAD_DIR.as_str()),
        PathBuf::from(PROFILE_DIR.as_str()).join("Downloads"),
        PathBuf::from(HEADLESS_PROFILE_DIR.as_str()).join("Downloads"),
    ];

    for path in directories {
        if !path.exists() {
            continue;
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') {
                println!("{}", entry.path().display())
            }
        }
    }
    Ok(())
}
