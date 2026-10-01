use jelly::{
    ACTIVE_TARGET, BROWSER_MODE, BROWSER_PID, BROWSER_READY, BROWSER_STOP, ENDPOINT, INJECTION_DIR,
    LOGICAL_TARGETS, PAGE_TARGET,
};
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn remove_if_exists(path: impl AsRef<std::path::Path>) -> Result<(), std::io::Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = match fs::read_to_string(BROWSER_PID) {
        Ok(pid) => pid.trim().to_string(),
        Err(_) => {
            let _ = Command::new("systemctl")
                .args(["--user", "stop", "jelly-browser.service"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            cleanup_files()?;
            println!("No jelly browser running.");
            return Ok(());
        }
    };

    fs::write(BROWSER_STOP, b"close")?;

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let proc_path = format!("/proc/{pid}");
        let exited = !std::path::Path::new(&proc_path).exists()
            || fs::read_to_string(format!("{proc_path}/stat"))
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(')')
                        .map(|(_, rest)| rest.trim_start().starts_with('Z'))
                })
                .unwrap_or(false);
        if exited {
            cleanup_files()?;
            println!("Browser closed.");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    eprintln!("Graceful shutdown timed out; stopping browser service.");
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "jelly-browser.service"])
        .status();
    thread::sleep(Duration::from_millis(500));
    cleanup_files()?;
    Ok(())
}

fn cleanup_files() -> Result<(), Box<dyn std::error::Error>> {
    match fs::read_dir(INJECTION_DIR) {
        Ok(entries) => {
            for entry in entries {
                let p = entry?.path();
                if p.is_file() {
                    remove_if_exists(&p)?;
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    for path in [
        ENDPOINT,
        PAGE_TARGET,
        ACTIVE_TARGET,
        LOGICAL_TARGETS,
        BROWSER_PID,
        BROWSER_STOP,
        BROWSER_MODE,
        BROWSER_READY,
    ] {
        remove_if_exists(path)?;
    }
    Ok(())
}
