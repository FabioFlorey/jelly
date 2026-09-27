use jelly::{BROWSER_PID, PROFILE_DIR};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn browser_running() -> bool {
    let Ok(pid) = fs::read_to_string(BROWSER_PID) else {
        return false;
    };
    Path::new(&format!("/proc/{}", pid.trim())).exists()
}

fn ignored(name: &str) -> bool {
    matches!(
        name,
        "SingletonLock" | "SingletonSocket" | "SingletonCookie" | "DevToolsActivePort"
    )
}

fn copy_tree(source: &Path, destination: &Path) -> Result<u64, Box<dyn std::error::Error>> {
    fs::create_dir_all(destination)?;
    let mut files = 0_u64;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if ignored(&name_text) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            files += copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
            files += 1;
        }
    }
    Ok(files)
}

fn nonempty(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let source = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(PathBuf::from)
        .ok_or("usage: profile-import <chromium-user-data-dir> [--force]")?;
    let force = args.iter().any(|arg| arg == "--force");

    if browser_running() {
        return Err("close Jelly's browser before importing a profile".into());
    }
    if !source.is_dir() {
        return Err(format!("profile source is not a directory: {}", source.display()).into());
    }

    let destination = Path::new(PROFILE_DIR);
    if destination.exists() && source.canonicalize().ok() == destination.canonicalize().ok() {
        return Err("profile source is already Jelly's managed profile".into());
    }
    if nonempty(destination) {
        if !force {
            return Err(format!(
                "Jelly profile already contains data at {}; rerun with --force to replace it",
                destination.display()
            )
            .into());
        }
        fs::remove_dir_all(destination)?;
    }

    let files = copy_tree(&source, destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    }
    println!(
        "{}",
        json!({
            "imported": true,
            "source": source,
            "destination": destination,
            "files": files,
            "note": "Profile reuse can preserve session state; it does not guarantee anti-bot or CAPTCHA behavior."
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "jelly-profile-import-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn copy_tree_preserves_files_and_skips_runtime_locks_and_symlinks() {
        let source = temp_dir("source");
        let destination = temp_dir("destination");
        fs::write(source.join("Preferences"), b"prefs").unwrap();
        fs::write(source.join("SingletonLock"), b"lock").unwrap();
        fs::create_dir_all(source.join("Default")).unwrap();
        fs::write(source.join("Default/Cookies"), b"cookies").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(source.join("Preferences"), source.join("linked")).unwrap();

        let files = copy_tree(&source, &destination).unwrap();
        assert_eq!(files, 2);
        assert_eq!(fs::read(destination.join("Preferences")).unwrap(), b"prefs");
        assert_eq!(
            fs::read(destination.join("Default/Cookies")).unwrap(),
            b"cookies"
        );
        assert!(!destination.join("SingletonLock").exists());
        assert!(!destination.join("linked").exists());

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn nonempty_detects_profile_contents() {
        let path = temp_dir("nonempty");
        assert!(!nonempty(&path));
        fs::write(path.join("Preferences"), b"prefs").unwrap();
        assert!(nonempty(&path));
        let _ = fs::remove_dir_all(path);
    }
}
