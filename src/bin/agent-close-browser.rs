use jelly::{
    ACTIVE_TARGET, BROWSER_MODE, BROWSER_PID, BROWSER_READY, BROWSER_STOP, ENDPOINT, INJECTION_DIR,
    PAGE_TARGET,
};
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = match fs::read_to_string(BROWSER_PID) {
        Ok(pid) => pid.trim().to_string(),
        Err(_) => {
            let _ = Command::new("systemctl")
                .args(["--user", "stop", "jelly-browser.service"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            cleanup_files();
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
            cleanup_files();
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
    cleanup_files();
    Ok(())
}

fn cleanup_files() {
    if let Ok(entries) = fs::read_dir(INJECTION_DIR) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                let _ = fs::remove_file(p);
            }
        }
    }
    for path in [
        ENDPOINT,
        PAGE_TARGET,
        ACTIVE_TARGET,
        BROWSER_PID,
        BROWSER_STOP,
        BROWSER_MODE,
        BROWSER_READY,
    ] {
        let _ = fs::remove_file(path);
    }
}
