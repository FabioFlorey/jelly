use crate::{Error, LOG_DIR};
use serde_json::json;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub fn log_primitive(
    name: &str,
    args: &[String],
    duration_ms: u128,
    error: Option<String>,
) -> Result<(), Error> {
    let trace_id = env::var("PBJ_TRACE_ID").unwrap_or_else(|_| new_id("trace"));
    let span_id = new_id("span");
    let ok = error.is_none();
    fs::create_dir_all(LOG_DIR)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{LOG_DIR}/actions.jsonl"))?;
    let v = json!({"timestamp":OffsetDateTime::now_utc().format(&Rfc3339)?,"level":if ok{"INFO"}else{"ERROR"},"event":if ok{"primitive.completed"}else{"primitive.failed"},"message":format!("agent-{name} {}",if ok{"completed"}else{"failed"}),"trace_id":trace_id,"span_id":span_id,"parent_span_id":env::var("PBJ_PARENT_SPAN_ID").ok(),"source":env::var("PBJ_SOURCE").unwrap_or_else(|_|"direct".into()),"tool":format!("agent-{name}"),"args":redact(args),"duration_ms":duration_ms,"ok":ok,"error":error.map(|message|json!({"kind":"primitive_error","message":message})),"process":{"pid":std::process::id()}});
    writeln!(f, "{v}")?;
    Ok(())
}

pub fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!(
        "{prefix}-{}-{}-{n}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
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
            secret = true
        } else if lower.contains("token=")
            || lower.contains("password=")
            || lower.contains("secret=")
            || lower.contains("api_key=")
        {
            out.push("<redacted>".into())
        } else {
            out.push(a.clone())
        }
    }
    out
}
