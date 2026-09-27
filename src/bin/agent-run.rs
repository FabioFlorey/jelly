use jelly::{ACTIVE_TARGET, BROWSER_MODE, LOG_DIR, PAGE_TARGET, new_id};
use serde_json::json;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let tool = args.first().ok_or("usage: agent-run <tool> [args...]")?;
    let tool = tool.strip_prefix("agent-").unwrap_or(tool);
    if tool == "run" {
        return Err("agent-run cannot invoke itself".into());
    }
    let bin_dir = env::current_exe()?
        .parent()
        .ok_or("agent-run executable has no parent")?
        .to_path_buf();
    let bin = bin_dir.join(format!("agent-{tool}"));
    if !Path::new(&bin).is_file() {
        return Err(format!("tool not found: agent-{tool}").into());
    }

    let started = SystemTime::now();
    let timer = Instant::now();
    let trace_id = env::var("PBJ_TRACE_ID").unwrap_or_else(|_| new_id("trace"));
    let span_id = new_id("span");
    let source = env::var("PBJ_SOURCE").unwrap_or_else(|_| "agent-run".into());
    let status = Command::new(&bin)
        .args(&args[1..])
        .current_dir(ROOT)
        .env("PBJ_TRACE_ID", &trace_id)
        .env("PBJ_PARENT_SPAN_ID", &span_id)
        .env(
            "PBJ_SOURCE",
            if tool == "call-routine" {
                "routine"
            } else {
                "cli"
            },
        )
        .status();
    let duration_ms = timer.elapsed().as_millis();
    let (ok, exit_code, error) = match &status {
        Ok(s) => (s.success(), s.code(), None),
        Err(e) => (false, None, Some(e.to_string())),
    };
    let level = if ok { "INFO" } else { "ERROR" };
    let event = if ok { "tool.completed" } else { "tool.failed" };
    let message = if ok {
        format!("agent-{tool} completed")
    } else {
        format!("agent-{tool} failed")
    };
    append(json!({
        "timestamp": OffsetDateTime::now_utc().format(&Rfc3339)?,
        "timestamp_ms": started.duration_since(UNIX_EPOCH)?.as_millis(),
        "level": level,
        "event": event,
        "message": message,
        "trace_id": trace_id,
        "span_id": span_id,
        "parent_span_id": env::var("PBJ_PARENT_SPAN_ID").ok(),
        "source": source,
        "tool": format!("agent-{tool}"),
        "args": redact(&args[1..]),
        "duration_ms": duration_ms,
        "ok": ok,
        "exit_code": exit_code,
        "error": error.as_ref().map(|message| json!({"kind":"spawn_error","message":message})),
        "process": {"pid": std::process::id()},
        "browser": browser_context(),
    }))?;
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => Err(e.into()),
    }
}

fn append(v: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(LOG_DIR)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{LOG_DIR}/actions.jsonl"))?;
    writeln!(f, "{}", v)?;
    Ok(())
}

fn browser_context() -> serde_json::Value {
    json!({
        "mode": fs::read_to_string(BROWSER_MODE).ok().map(|x|x.trim().to_owned()),
        "target_id": fs::read_to_string(ACTIVE_TARGET).ok().or_else(||fs::read_to_string(PAGE_TARGET).ok()).map(|x|x.trim().to_owned()),
    })
}

fn redact(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut secret = false;
    for a in args {
        if secret {
            out.push("<redacted>".into());
            secret = false;
            continue;
        }
        let lower = a.to_ascii_lowercase();
        if ["--token", "--password", "--secret", "--api-key", "--apikey"].contains(&lower.as_str())
        {
            out.push(a.clone());
            secret = true;
        } else if lower.contains("token=")
            || lower.contains("password=")
            || lower.contains("secret=")
            || lower.contains("api_key=")
        {
            out.push("<redacted>".into());
        } else {
            out.push(a.clone());
        }
    }
    out
}
