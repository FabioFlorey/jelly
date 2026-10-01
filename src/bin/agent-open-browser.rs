use jelly::{
    ACTIVE_TARGET, BROWSER_MODE, BROWSER_PID, BROWSER_READY, BROWSER_STOP, DOWNLOAD_DIR, ENDPOINT,
    HEADLESS_PROFILE_DIR, INJECTION_DIR, LOGICAL_TARGETS, NETWORK_DIR, PAGE_TARGET, PROFILE_DIR,
    RECORDING_DIR, ROUTINE_STATE_DIR, SCREENSHOT_DIR, STATE_DIR,
};
use rustwright::{GotoOptions, LaunchOptions, chromium};
use serde_json::{Value, json};
use std::{
    env, fs, process, thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, connect};

fn main() -> rustwright::Result<()> {
    if !std::env::args().any(|a| a == "--serve") {
        return launch_service();
    }
    for dir in [
        STATE_DIR,
        PROFILE_DIR,
        HEADLESS_PROFILE_DIR,
        INJECTION_DIR,
        NETWORK_DIR,
        ROUTINE_STATE_DIR,
        SCREENSHOT_DIR,
        RECORDING_DIR,
        DOWNLOAD_DIR,
    ] {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::remove_file(BROWSER_STOP);
    for stale in [
        BROWSER_READY,
        ENDPOINT,
        PAGE_TARGET,
        ACTIVE_TARGET,
        LOGICAL_TARGETS,
        BROWSER_PID,
    ] {
        let _ = fs::remove_file(stale);
    }
    let args: Vec<String> = env::args().skip(1).collect();
    let headless = args.iter().any(|arg| arg == "--headless");
    let url = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(String::as_str);

    let mut options = LaunchOptions::default()
        .headless(headless)
        .executable_path("/usr/bin/chromium");

    if !headless {
        options = options.arg("--start-maximized");
    }

    options.chromium_sandbox = true;
    options.ignore_default_args = vec!["--disable-blink-features=AutomationControlled".into()];
    options.user_data_dir = Some(
        if headless {
            HEADLESS_PROFILE_DIR
        } else {
            PROFILE_DIR
        }
        .into(),
    );

    let browser = chromium().launch(options)?;
    fs::write(BROWSER_MODE, if headless { "headless" } else { "headed" })?;
    fs::write(BROWSER_PID, process::id().to_string())?;
    fs::write(ENDPOINT, browser.ws_endpoint())?;

    if let Some(url) = url {
        let page = browser.new_page()?;
        page.goto(url, GotoOptions::default().wait_until("domcontentloaded"))?;
        fs::write(PAGE_TARGET, page.target_id())?;

        let (mut ws, _) = connect(browser.ws_endpoint().as_str())?;
        ws.send(Message::Text(
            json!({"id":1,"method":"Target.getTargets"})
                .to_string()
                .into(),
        ))?;
        loop {
            let Message::Text(text) = ws.read()? else {
                continue;
            };
            let value: Value = serde_json::from_str(&text)?;
            if value.get("id").and_then(Value::as_i64) != Some(1) {
                continue;
            }
            if let Some(target) = value["result"]["targetInfos"]
                .as_array()
                .and_then(|targets| {
                    targets.iter().find(|target| {
                        target["type"] == "page"
                            && target["url"] == "about:blank"
                            && target["targetId"].as_str() != Some(page.target_id().as_str())
                    })
                })
            {
                ws.send(Message::Text(
                    json!({
                        "id":2,
                        "method":"Target.closeTarget",
                        "params":{"targetId":target["targetId"]}
                    })
                    .to_string()
                    .into(),
                ))?;
            }
            break;
        }

        println!("Opened: {}", page.title(Default::default())?);
    }

    println!(
        "Chromium opened in {} mode.",
        if headless { "headless" } else { "headed" }
    );
    println!("CDP: {}", browser.ws_endpoint());
    println!("Use agent-close-browser to close the browser.");
    fs::write(BROWSER_READY, b"ready")?;

    loop {
        if fs::metadata(BROWSER_STOP).is_ok() {
            let _ = fs::remove_file(BROWSER_STOP);
            for stale in [
                ENDPOINT,
                PAGE_TARGET,
                ACTIVE_TARGET,
                LOGICAL_TARGETS,
                BROWSER_PID,
            ] {
                let _ = fs::remove_file(stale);
            }
            browser.close()?;
            let _ = fs::remove_file(ENDPOINT);
            let _ = fs::remove_file(PAGE_TARGET);
            let _ = fs::remove_file(ACTIVE_TARGET);
            let _ = fs::remove_file(LOGICAL_TARGETS);
            let _ = fs::remove_file(BROWSER_PID);
            let _ = fs::remove_file(BROWSER_MODE);
            let _ = fs::remove_file(BROWSER_READY);
            println!("Browser closed cleanly.");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn launch_service() -> rustwright::Result<()> {
    if let Ok(entries) = fs::read_dir(INJECTION_DIR) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                let _ = fs::remove_file(p);
            }
        }
    }

    use std::process::Command;
    let exe = std::env::current_exe().map_err(rustwright::Error::Io)?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "jelly-browser.service"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    for stale in [
        BROWSER_STOP,
        BROWSER_READY,
        ENDPOINT,
        PAGE_TARGET,
        ACTIVE_TARGET,
        BROWSER_PID,
        BROWSER_MODE,
    ] {
        let _ = fs::remove_file(stale);
    }
    let mut cmd = Command::new("systemd-run");
    cmd.args([
        "--user",
        "--unit=jelly-browser",
        "--collect",
        "--quiet",
        "--property=KillMode=control-group",
    ]);
    for key in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.arg(format!("--setenv={key}={v}"));
        }
    }
    cmd.arg(exe).arg("--serve").args(&args);
    if !cmd.status().map_err(rustwright::Error::Io)?.success() {
        return Err(rustwright::Error::Io(std::io::Error::other(
            "failed to start jelly browser service",
        )));
    }
    let startup_timeout_secs = env::var("JELLY_BROWSER_STARTUP_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30);
    let deadline = Instant::now() + Duration::from_secs(startup_timeout_secs);
    loop {
        if fs::metadata(BROWSER_READY).is_ok()
            && let Ok(ep) = fs::read_to_string(ENDPOINT)
            && tungstenite::connect(ep.trim()).is_ok()
        {
            println!("Browser ready.");
            println!("CDP: {}", ep.trim());
            return Ok(());
        }

        let state = Command::new("systemctl")
            .args([
                "--user",
                "show",
                "jelly-browser.service",
                "--property=ActiveState",
                "--value",
            ])
            .output()
            .map_err(rustwright::Error::Io)?;
        if !state.status.success() {
            return Err(rustwright::Error::Io(std::io::Error::other(
                "browser service disappeared before becoming ready",
            )));
        }
        let active_state = String::from_utf8_lossy(&state.stdout).trim().to_owned();
        if matches!(active_state.as_str(), "failed" | "inactive") {
            return Err(rustwright::Error::Io(std::io::Error::other(format!(
                "browser service exited before becoming ready (state: {active_state})"
            ))));
        }

        if Instant::now() >= deadline {
            return Err(rustwright::Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "browser service did not become ready within {startup_timeout_secs}s (state: {active_state})"
                ),
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
