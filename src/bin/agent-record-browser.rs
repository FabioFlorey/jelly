use base64::{Engine as _, engine::general_purpose::STANDARD};
use jelly::{BrowserSession, RECORDING_DIR, new_id, register_recording};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const ACTIVE: &str = "/data/jelly-runtime/artifacts/recordings/active.json";

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(RECORDING_DIR)?;
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--worker") {
        let dir = args.get(1).ok_or("recording worker directory missing")?;
        let interval_ms = args
            .get(2)
            .ok_or("recording worker interval missing")?
            .parse::<u64>()?;
        return continuous_worker(Path::new(dir), interval_ms);
    }

    match args.first().map(String::as_str) {
        Some("start") => {
            let mut mode = "continuous".to_owned();
            let mut interval_ms = 500_u64;
            let mut hold_ms = 1000_u64;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--mode" => {
                        i += 1;
                        mode = args.get(i).ok_or("--mode requires continuous or steps")?.clone();
                    }
                    "--interval-ms" => {
                        i += 1;
                        interval_ms = args
                            .get(i)
                            .ok_or("--interval-ms requires a value")?
                            .parse::<u64>()?
                            .max(100);
                    }
                    "--hold-ms" => {
                        i += 1;
                        hold_ms = args
                            .get(i)
                            .ok_or("--hold-ms requires a value")?
                            .parse::<u64>()?
                            .max(100);
                    }
                    value if i == 1 && value.chars().all(|c| c.is_ascii_digit()) => {
                        interval_ms = value.parse::<u64>()?.max(100);
                    }
                    other => return Err(format!("unknown record-browser start option: {other}").into()),
                }
                i += 1;
            }
            start(&mode, interval_ms, hold_ms)
        }
        Some("stop") => stop(),
        _ => Err(
            "usage: record-browser start [--mode continuous|steps] [--interval-ms n] [--hold-ms n] | record-browser stop"
                .into(),
        ),
    }
}

fn ensure_not_active() -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(active) = fs::read_to_string(ACTIVE) {
        if let Ok(value) = serde_json::from_str::<Value>(&active) {
            let mode = value["mode"].as_str().unwrap_or("continuous");
            if mode == "steps" || value["pid"].as_u64().is_some_and(process_alive) {
                return Err("a browser recording is already active".into());
            }
        }
        let _ = fs::remove_file(ACTIVE);
    }
    Ok(())
}

fn source_context() -> (Option<String>, Option<String>, Option<String>) {
    let Ok(mut browser) = BrowserSession::connect() else {
        return (None, None, None);
    };
    let target_id = Some(browser.target_id().to_owned());
    let context = browser
        .eval("({url:location.href,title:document.title||''})")
        .unwrap_or(Value::Null);
    (
        target_id,
        context["url"].as_str().map(str::to_owned),
        context["title"].as_str().map(str::to_owned),
    )
}

fn start(mode: &str, interval_ms: u64, hold_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(mode, "continuous" | "steps") {
        return Err("--mode must be continuous or steps".into());
    }
    ensure_not_active()?;

    let id = new_id("recording");
    let dir = Path::new(RECORDING_DIR).join(&id);
    fs::create_dir_all(&dir)?;
    let (target_id, url, title) = source_context();

    let mut state = json!({
        "recording_id": id,
        "mode": mode,
        "dir": dir,
        "target_id": target_id,
        "url": url,
        "title": title,
        "started_at_ms": now_ms()
    });

    if mode == "continuous" {
        if state["target_id"].is_null() {
            fs::remove_dir_all(&dir)?;
            return Err("continuous recording requires an active Chromium page".into());
        }
        let exe = env::current_exe()?;
        let child = Command::new(exe)
            .args([
                "--worker",
                dir.to_str().ok_or("invalid recording path")?,
                &interval_ms.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        state["pid"] = json!(child.id());
        state["interval_ms"] = json!(interval_ms);
    } else {
        state["hold_ms"] = json!(hold_ms);
    }

    fs::write(ACTIVE, serde_json::to_vec_pretty(&state)?)?;
    println!("{}", serde_json::to_string(&state)?);
    Ok(())
}

fn stop() -> Result<(), Box<dyn std::error::Error>> {
    let active: Value =
        serde_json::from_slice(&fs::read(ACTIVE).map_err(|_| "no browser recording is active")?)?;
    match active["mode"].as_str().unwrap_or("continuous") {
        "steps" => stop_steps(&active),
        "continuous" => stop_continuous(&active),
        _ => Err("active recording has an unknown mode".into()),
    }
}

fn stop_continuous(active: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let dir = active_dir(active)?;
    fs::write(dir.join("stop"), b"stop")?;
    let manifest = dir.join("manifest.json");
    for _ in 0..100 {
        if manifest.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if !manifest.is_file() {
        return Err("browser recording did not stop within 5 seconds".into());
    }
    finalize(active, &dir, &manifest)
}

fn stop_steps(active: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let dir = active_dir(active)?;
    let steps_path = dir.join("steps.jsonl");
    let raw =
        fs::read_to_string(&steps_path).map_err(|_| "step recording has no captured actions")?;
    let steps: Vec<Value> = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    if steps.is_empty() {
        return Err("step recording has no captured actions".into());
    }

    let video = dir.join("recording.mp4");
    encode_step_video(&dir, &steps, &video)?;

    let mut final_context = source_context();
    if final_context.1.is_none()
        && let Some(last) = steps.last()
    {
        final_context.1 = last["url"].as_str().map(str::to_owned);
        final_context.2 = last["title"].as_str().map(str::to_owned);
    }
    let manifest = dir.join("manifest.json");
    let value = json!({
        "version": 1,
        "kind": "browser_recording",
        "mode": "steps",
        "started_at_ms": active["started_at_ms"],
        "ended_at_ms": now_ms(),
        "frame_count": steps.len(),
        "video": video,
        "initial_target_id": active["target_id"],
        "initial_url": active["url"],
        "final_target_id": final_context.0,
        "final_url": final_context.1,
        "final_title": final_context.2,
        "steps": steps
    });
    fs::write(&manifest, serde_json::to_vec_pretty(&value)?)?;
    finalize(active, &dir, &manifest)
}

fn active_dir(active: &Value) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(
        active["dir"]
            .as_str()
            .ok_or("active recording has no directory")?,
    ))
}

fn finalize(active: &Value, dir: &Path, manifest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let recorded: Value = serde_json::from_slice(&fs::read(manifest)?)?;
    let frame_count = recorded["frame_count"].as_u64().unwrap_or(0);
    let video = dir.join("recording.mp4");
    let artifact = register_recording(
        &video,
        manifest,
        active["url"]
            .as_str()
            .or(recorded["initial_url"].as_str())
            .or(recorded["final_url"].as_str()),
        active["title"]
            .as_str()
            .or(recorded["final_title"].as_str()),
        active["target_id"]
            .as_str()
            .or(recorded["initial_target_id"].as_str())
            .or(recorded["final_target_id"].as_str()),
        frame_count,
    )?;
    let _ = fs::remove_file(ACTIVE);
    fs::remove_dir_all(dir)?;
    println!("{}", serde_json::to_string(&artifact)?);
    Ok(())
}

fn encode_step_video(
    dir: &Path,
    steps: &[Value],
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let first_frame = steps
        .first()
        .and_then(|step| step["frame"].as_str())
        .ok_or("step recording has no first frame")?;
    let (width, height) = probe_dimensions(&dir.join(first_frame))?;
    let font_size = ((height as f64 * 0.030).round() as u32).max(16);
    let margin_v = ((height as f64 * 0.035).round() as u32).max(12);
    let outline = ((height as f64 * 0.0025).round() as u32).max(1);
    let pad_x = ((width as f64 * 0.018).round() as u32).max(10);
    let pad_y = ((height as f64 * 0.010).round() as u32).max(6);
    let logo_h = ((height as f64 * 0.055).round() as u32).max(24);
    let logo_margin_x = ((width as f64 * 0.025).round() as u32).max(12);
    let logo_margin_y = ((height as f64 * 0.030).round() as u32).max(12);

    let concat_path = dir.join("concat.txt");
    let mut concat = String::new();
    let favicon = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/favicon.png");
    if !favicon.is_file() {
        return Err("Jelly favicon is missing".into());
    }

    for (index, step) in steps.iter().enumerate() {
        let frame = step["frame"].as_str().ok_or("step has no frame")?;
        let hold_ms = step["hold_ms"].as_u64().unwrap_or(1000).max(100);
        let label_path = dir.join(format!("label-{index:03}.txt"));
        let rendered_name = format!("rendered-{index:03}.png");
        let rendered_path = dir.join(&rendered_name);
        let label = step["label"]
            .as_str()
            .unwrap_or("browser action")
            .replace(['\r', '\n'], " ");
        fs::write(&label_path, label)?;

        let filter = format!(
            "[0:v]pad=ceil(iw/2)*2:ceil(ih/2)*2[base];[1:v]scale=-1:{logo_h}[logo];[base][logo]overlay=W-w-{logo_margin_x}:H-h-{logo_margin_y}[branded];[branded]drawtext=font='DejaVu Sans':textfile='label-{index:03}.txt':fontcolor=0xFFC107:fontsize={font_size}:borderw={outline}:bordercolor=black:box=1:boxcolor=black@0.56:boxborderw={pad_y}|{pad_x}|{pad_y}|{pad_x}:x=(w-text_w)/2:y=h-text_h-{margin_v}-{pad_y}[v]"
        );
        let status = Command::new("ffmpeg")
            .current_dir(dir)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                frame,
                "-i",
                favicon.to_str().ok_or("invalid favicon path")?,
                "-filter_complex",
                &filter,
                "-map",
                "[v]",
                "-frames:v",
                "1",
                rendered_name.as_str(),
            ])
            .status()?;
        if !status.success() || !rendered_path.is_file() || fs::metadata(&rendered_path)?.len() == 0
        {
            return Err(format!("ffmpeg failed to render branded step {index}").into());
        }

        concat.push_str(&format!(
            "file '{}'
",
            rendered_name.replace('\'', "'\\''")
        ));
        concat.push_str(&format!(
            "duration {:.3}
",
            hold_ms as f64 / 1000.0
        ));
    }
    if let Some(last_index) = steps.len().checked_sub(1) {
        concat.push_str(&format!(
            "file 'rendered-{last_index:03}.png'
"
        ));
    }
    fs::write(&concat_path, concat)?;

    let status = Command::new("ffmpeg")
        .current_dir(dir)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            "concat.txt",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-fps_mode",
            "vfr",
            "-movflags",
            "+faststart",
            output
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("invalid video path")?,
        ])
        .status()?;
    if !status.success() || !output.is_file() || fs::metadata(output)?.len() == 0 {
        return Err("ffmpeg failed to encode branded step recording".into());
    }
    Ok(())
}

fn probe_dimensions(path: &Path) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
            path.to_str().ok_or("invalid frame path")?,
        ])
        .output()
        .map_err(|error| format!("failed to start ffprobe: {error}"))?;
    if !output.status.success() {
        return Err("ffprobe failed to inspect step frame".into());
    }
    let dimensions = String::from_utf8(output.stdout)?;
    let (width, height) = dimensions
        .trim()
        .split_once('x')
        .ok_or("ffprobe returned invalid frame dimensions")?;
    Ok((width.parse()?, height.parse()?))
}

fn continuous_worker(dir: &Path, interval_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
    let mut browser = BrowserSession::connect()?;
    let target_id = browser.target_id().to_owned();
    let started_at_ms = now_ms();
    let mut frame_count = 0_u64;
    let fps = 1000.0 / interval_ms as f64;
    let video = dir.join("recording.mp4");

    let mut ffmpeg = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-framerate",
            &format!("{fps:.6}"),
            "-i",
            "-",
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            video.to_str().ok_or("invalid recording output path")?,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to start ffmpeg: {error}"))?;

    let mut ffmpeg_stdin = ffmpeg.stdin.take().ok_or("ffmpeg stdin unavailable")?;

    while !dir.join("stop").exists() {
        let value = browser.call(
            "Page.captureScreenshot",
            json!({
                "format": "png",
                "fromSurface": true,
                "captureBeyondViewport": false,
                "optimizeForSpeed": true
            }),
        );
        if let Ok(value) = value
            && let Some(data) = value["result"]["data"].as_str()
            && let Ok(bytes) = STANDARD.decode(data)
        {
            ffmpeg_stdin.write_all(&bytes)?;
            frame_count += 1;
        }
        thread::sleep(Duration::from_millis(interval_ms));
    }

    drop(ffmpeg_stdin);
    let status = ffmpeg.wait()?;
    if !status.success() {
        return Err("ffmpeg failed to encode browser recording".into());
    }
    if !video.is_file() || fs::metadata(&video)?.len() == 0 || frame_count == 0 {
        return Err("browser recording produced no usable video".into());
    }

    let context = browser
        .eval("({url:location.href,title:document.title||''})")
        .unwrap_or(Value::Null);
    let manifest = json!({
        "version": 1,
        "kind": "browser_recording",
        "mode": "continuous",
        "target_id": target_id,
        "started_at_ms": started_at_ms,
        "ended_at_ms": now_ms(),
        "interval_ms": interval_ms,
        "frame_count": frame_count,
        "video": video,
        "final_url": context["url"],
        "final_title": context["title"]
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn process_alive(pid: u64) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}
