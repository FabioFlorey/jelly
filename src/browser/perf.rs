use crate::LOG_DIR;
use serde_json::json;
use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        env::var("JELLY_PERF_LOG")
            .ok()
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "on"))
    })
}

pub(crate) fn record(event: &str, duration_ms: u128, detail: Option<&str>) {
    if !enabled() {
        return;
    }
    if fs::create_dir_all(LOG_DIR).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{LOG_DIR}/perf.jsonl"))
    else {
        return;
    };
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let _ = writeln!(
        file,
        "{}",
        json!({
            "timestamp_ms": timestamp_ms,
            "event": event,
            "duration_ms": duration_ms,
            "detail": detail
        })
    );
}
