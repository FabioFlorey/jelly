use base64::{Engine as _, engine::general_purpose::STANDARD};
use jelly::{BrowserSession, SCREENSHOT_DIR, Target, new_id, register_screenshot};
use std::{
    env, fs,
    path::Path,
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

    for (i, arg) in args.iter().enumerate() {
        let is_output_value = i > 0 && args[i - 1] == "--output";
        if arg.starts_with("--") && arg != "--json" && arg != "--output" && !is_output_value {
            return Err(format!("unknown option: {arg}").into());
        }
    }

    let json_output = args.iter().any(|x| x == "--json");
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
    ensure_parent(&out)?;

    if let Some(target) = target.as_deref() {
        capture_element(target, &out)?;
        return finish(&out, Some(target), json_output);
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
                return finish(&out, None, json_output);
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

    Err("Chromium viewport screenshot failed".into())
}

fn ensure_parent(out: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = Path::new(out)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn source_context() -> (Option<String>, Option<String>) {
    let Ok(mut browser) = BrowserSession::connect() else {
        return (None, None);
    };
    let Ok(value) = browser.eval("({url:location.href,title:document.title||''})") else {
        return (None, None);
    };
    (
        value["url"].as_str().map(str::to_owned),
        value["title"].as_str().map(str::to_owned),
    )
}

fn finish(
    out: &str,
    target: Option<&str>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let out_path = Path::new(out);
    let artifact_path = Path::new(SCREENSHOT_DIR).join(format!("{}.png", new_id("screenshot")));
    fs::copy(out_path, &artifact_path)?;
    let (url, title) = source_context();
    let artifact = register_screenshot(&artifact_path, target, url.as_deref(), title.as_deref())?;
    if json_output {
        println!("{}", serde_json::to_string(&artifact)?);
    } else {
        println!("{out}");
    }
    Ok(())
}

fn native_viewport(out: &str) -> Result<(), Box<dyn std::error::Error>> {
    ensure_parent(out)?;
    let mut c = BrowserSession::connect()?;
    let v = c.call(
        "Page.captureScreenshot",
        serde_json::json!({
            "format":"png",
            "fromSurface":true,
            "captureBeyondViewport":false,
            "optimizeForSpeed":true
        }),
    )?;
    let data = v["result"]["data"].as_str().ok_or("no screenshot")?;
    fs::write(out, base64_decode(data)?)?;
    Ok(())
}

fn capture_element(target: &str, out: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = BrowserSession::connect()?;
    let r = c.eval(&format!(
        r#"(()=>{{const e={};if(!e)return null;const r=e.getBoundingClientRect(),s=getComputedStyle(e);if(r.width<=0||r.height<=0||s.display==='none'||s.visibility==='hidden')return null;return {{x:r.x+scrollX,y:r.y+scrollY,width:r.width,height:r.height}}}})()"#,
        Target::parse(target)?.js_resolver()
    ))?;
    if r.is_null()
        || r["width"].as_f64().unwrap_or(0.0) <= 0.0
        || r["height"].as_f64().unwrap_or(0.0) <= 0.0
    {
        return Err("element not found or not visible".into());
    }
    let v = c.call(
        "Page.captureScreenshot",
        serde_json::json!({
            "format":"png",
            "captureBeyondViewport":true,
            "clip":{
                "x":r["x"],
                "y":r["y"],
                "width":r["width"],
                "height":r["height"],
                "scale":1
            }
        }),
    )?;
    fs::write(
        out,
        base64_decode(v["result"]["data"].as_str().ok_or("no screenshot")?)?,
    )?;
    Ok(())
}

fn base64_decode(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(STANDARD.decode(s)?)
}
