use jelly::BrowserSession;
use std::{env, process::Command, thread, time::Duration};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = env::current_exe()?
        .parent()
        .ok_or("browser-task executable has no parent")?
        .join("agent-run");
    let mut args: Vec<String> = env::args().skip(1).collect();
    let persist = if let Some(i) = args.iter().position(|x| x == "--persist") {
        args.remove(i);
        true
    } else {
        false
    };
    let url = args
        .first()
        .ok_or("usage: browser-task [--persist] <url> <agent-tool> [args...]")?
        .clone();
    if args.len() < 2 {
        return Err("browser-task requires a tool".into());
    }
    let tool = args[1].clone();
    let tool_args = &args[2..];

    let status = Command::new(&run)
        .args(["open-browser", &url])
        .current_dir(ROOT)
        .status()?;
    if !status.success() {
        return Err("browser failed to start".into());
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut ready = false;
        for _ in 0..30 {
            if BrowserSession::connect().is_ok() {
                ready = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        if !ready {
            return Err("browser CDP did not become ready".into());
        }
        let status = Command::new(&run)
            .arg(&tool)
            .args(tool_args)
            .current_dir(ROOT)
            .status()?;
        if !status.success() {
            return Err(format!("agent-{tool} failed").into());
        }
        Ok(())
    })();

    if persist {
        println!("Browser persisted.");
    } else {
        let _ = Command::new(&run)
            .arg("close-browser")
            .current_dir(ROOT)
            .status();
    }
    result
}
