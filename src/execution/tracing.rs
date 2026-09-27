use crate::{Error, LOG_DIR, classify_error};
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
    error: Option<&Error>,
) -> Result<(), Error> {
    let trace_id = env::var("JELLY_TRACE_ID").unwrap_or_else(|_| new_id("trace"));
    let span_id = new_id("span");
    let ok = error.is_none();
    fs::create_dir_all(LOG_DIR)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{LOG_DIR}/actions.jsonl"))?;
    let error = error.map(|error| {
        let (kind, retryable) = classify_error(error.as_ref());
        json!({"kind":kind.as_str(),"message":error.to_string(),"retryable":retryable})
    });
    let v = json!({"timestamp":OffsetDateTime::now_utc().format(&Rfc3339)?,"level":if ok{"INFO"}else{"ERROR"},"event":if ok{"primitive.completed"}else{"primitive.failed"},"message":format!("agent-{name} {}",if ok{"completed"}else{"failed"}),"trace_id":trace_id,"span_id":span_id,"parent_span_id":env::var("JELLY_PARENT_SPAN_ID").ok(),"source":env::var("JELLY_SOURCE").unwrap_or_else(|_|"direct".into()),"tool":format!("agent-{name}"),"args":redact_args(args),"duration_ms":duration_ms,"ok":ok,"error":error,"process":{"pid":std::process::id()}});
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

pub fn redact_args(args: &[String]) -> Vec<String> {
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
        } else if let Ok(mut parsed) = url::Url::parse(a) {
            if matches!(parsed.scheme(), "http" | "https") {
                let _ = parsed.set_username("");
                let _ = parsed.set_password(None);
                parsed.set_query(None);
                parsed.set_fragment(None);
                out.push(parsed.to_string());
            } else {
                out.push(a.clone())
            }
        } else {
            out.push(a.clone())
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_redaction_removes_url_credentials_query_and_fragment() {
        let args = vec!["https://user:pass@example.com/path?view=1#frag".to_owned()];
        assert_eq!(
            redact_args(&args),
            vec!["https://example.com/path".to_owned()]
        );
    }

    #[test]
    fn trace_redaction_fully_hides_secret_bearing_arguments() {
        let args = vec!["https://example.com/path?token=secret".to_owned()];
        assert_eq!(redact_args(&args), vec!["<redacted>".to_owned()]);
    }

    #[test]
    fn trace_redaction_keeps_secret_flags_but_hides_values() {
        let args = vec!["--token".to_owned(), "secret".to_owned()];
        assert_eq!(
            redact_args(&args),
            vec!["--token".to_owned(), "<redacted>".to_owned()]
        );
    }
}
