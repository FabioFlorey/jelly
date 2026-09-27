use jelly::{BROWSER_MODE, BrowserSession, SCREENSHOT_DIR, Target};
use std::{
    env, fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const DEFAULT_NAME: &str = "latest.png";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(SCREENSHOT_DIR)?;
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|x| x == "--native-worker") {
        let out = args.get(1).ok_or("worker output missing")?;
        return native_viewport(out);
    }
    let output_flag = args
        .iter()
        .position(|x| x == "--output")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let positional: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(i, x)| {
            !x.starts_with("--") && !args.get(i.wrapping_sub(1)).is_some_and(|p| p == "--output")
        })
        .map(|(_, x)| x.clone())
        .collect();
    let target = positional.first().cloned();
    let out = output_flag
        .or_else(|| positional.get(1).cloned())
        .unwrap_or_else(|| format!("{SCREENSHOT_DIR}/{DEFAULT_NAME}"));
    if let Some(target) = target {
        return element(&target, &out);
    }

    let exe = env::current_exe()?;
    let mut child = Command::new(exe)
        .args(["--native-worker", &out])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() && fs::metadata(&out).is_ok() {
                println!("{}", out);
                return Ok(());
            }
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if fs::read_to_string(BROWSER_MODE).ok().as_deref() == Some("headed") {
        if desktop_fallback(&out)? {
            println!("{}", out);
            return Ok(());
        }
    }
    Err("Chromium viewport screenshot failed".into())
}

fn native_viewport(out: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = BrowserSession::connect()?;
    let v = c.call("Page.captureScreenshot", serde_json::json!({"format":"png","fromSurface":true,"captureBeyondViewport":false,"optimizeForSpeed":true}))?;
    let data = v["result"]["data"].as_str().ok_or("no screenshot")?;
    fs::write(out, base64_decode(data)?)?;
    Ok(())
}

fn element(target: &str, out: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = BrowserSession::connect()?;
    let r=c.eval(&format!(r#"(()=>{{const e={};if(!e)return null;e.scrollIntoView({{block:'center'}});const r=e.getBoundingClientRect();return {{x:r.x+scrollX,y:r.y+scrollY,width:r.width,height:r.height}}}})()"#,Target::parse(target)?.js_resolver()))?;
    if r.is_null()
        || r["width"].as_f64().unwrap_or(0.0) <= 0.0
        || r["height"].as_f64().unwrap_or(0.0) <= 0.0
    {
        return Err("element not found or has no size".into());
    }
    let v=c.call("Page.captureScreenshot",serde_json::json!({"format":"png","captureBeyondViewport":true,"clip":{"x":r["x"],"y":r["y"],"width":r["width"],"height":r["height"],"scale":1}}))?;
    fs::write(
        out,
        base64_decode(v["result"]["data"].as_str().ok_or("no screenshot")?)?,
    )?;
    println!("{}", out);
    Ok(())
}

fn desktop_fallback(out: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let aw = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()?;
    if !aw.status.success() {
        return Ok(false);
    }
    let v: serde_json::Value = serde_json::from_slice(&aw.stdout)?;
    let class = v["class"].as_str().unwrap_or("").to_ascii_lowercase();
    if !class.contains("chromium") {
        return Ok(false);
    }
    let at = v["at"].as_array().ok_or("active window has no position")?;
    let size = v["size"].as_array().ok_or("active window has no size")?;
    let geometry = format!("{},{} {}x{}", at[0], at[1], size[0], size[1]);
    Ok(Command::new("grim")
        .args(["-g", &geometry, out])
        .status()?
        .success())
}

fn base64_decode(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut p = Command::new("base64")
        .arg("-d")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    p.stdin.as_mut().unwrap().write_all(s.as_bytes())?;
    let o = p.wait_with_output()?;
    if !o.status.success() {
        return Err("base64 decode failed".into());
    }
    Ok(o.stdout)
}
