use jelly::{DOWNLOAD_DIR, HEADLESS_PROFILE_DIR, PROFILE_DIR};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for d in [
        DOWNLOAD_DIR,
        &format!("{PROFILE_DIR}/Downloads"),
        &format!("{HEADLESS_PROFILE_DIR}/Downloads"),
    ] {
        let p = Path::new(d);
        if !p.exists() {
            continue;
        }
        for e in fs::read_dir(p)? {
            let e = e?;
            let n = e.file_name().to_string_lossy().to_string();
            if !n.starts_with('.') {
                println!("{}", e.path().display())
            }
        }
    }
    Ok(())
}
