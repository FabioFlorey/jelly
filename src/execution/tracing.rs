use crate::{BrowserSession, Error, LOG_DIR, Target, classify_error, sanitize_url};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub fn log_primitive(
    name: &str,
    args: &[String],
    duration_ms: u128,
    error: Option<&Error>,
    prepared: Option<&PreparedStep>,
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
    let redacted = redact_tool_args(name, args);
    let v = json!({"timestamp":OffsetDateTime::now_utc().format(&Rfc3339)?,"level":if ok{"INFO"}else{"ERROR"},"event":if ok{"primitive.completed"}else{"primitive.failed"},"message":format!("agent-{name} {}",if ok{"completed"}else{"failed"}),"trace_id":trace_id,"span_id":span_id,"parent_span_id":env::var("JELLY_PARENT_SPAN_ID").ok(),"source":env::var("JELLY_SOURCE").unwrap_or_else(|_|"direct".into()),"tool":format!("agent-{name}"),"args":redacted,"duration_ms":duration_ms,"ok":ok,"error":error,"process":{"pid":std::process::id()}});
    writeln!(f, "{v}")?;
    if ok && step_worthy_primitive(name) {
        let _ = record_step_prepared(
            &format!("agent-{name}"),
            &redacted,
            duration_ms,
            &trace_id,
            &span_id,
            prepared,
        );
    }
    Ok(())
}

const RECORDING_ACTIVE: &str = "/data/jelly-runtime/artifacts/recordings/active.json";

#[derive(Debug, Clone, Default)]
pub(crate) struct PreparedStep {
    subject: Option<String>,
}

pub(crate) fn prepare_step(
    browser: &mut BrowserSession,
    name: &str,
    args: &[String],
) -> Option<PreparedStep> {
    let active: Value = serde_json::from_slice(&fs::read(RECORDING_ACTIVE).ok()?).ok()?;
    if active["mode"].as_str() != Some("steps") || !step_worthy_primitive(name) {
        return None;
    }

    let target_index = match name {
        "click" | "check" | "select" | "drag" | "open-in-new-tab" | "upload" | "highlight" => {
            Some(0)
        }
        "fill" | "type-text" if args.len() > 1 => Some(1),
        "scroll"
            if args.first().is_some_and(|value| {
                !matches!(value.as_str(), "down" | "up" | "top" | "bottom")
            }) =>
        {
            Some(0)
        }
        _ => None,
    };

    let subject = target_index
        .and_then(|index| args.get(index))
        .and_then(|value| semantic_target_name(browser, value));

    Some(PreparedStep { subject })
}

fn semantic_target_name(browser: &mut BrowserSession, value: &str) -> Option<String> {
    let target = Target::parse(value).ok()?;
    let resolved = browser
        .eval(&format!(
            r#"(() => {{
                const e = {};
                if (!e) return null;
                const clean = value => (value || "").replace(/\s+/g, " ").trim();
                const labelText = label => {{
                    const clone = label.cloneNode(true);
                    clone.querySelectorAll("input, textarea, select, button").forEach(node => node.remove());
                    return clean(clone.textContent);
                }};
                const label = e.labels ? [...e.labels].map(labelText).find(Boolean) : "";
                const aria = clean(e.getAttribute("aria-label"));
                const text = /^(BUTTON|A|SUMMARY)$/.test(e.tagName) ? clean(e.innerText) : "";
                const placeholder = clean(e.getAttribute("placeholder"));
                const name = clean(e.getAttribute("name"));
                const role = clean(e.getAttribute("role"));
                return label || aria || text || placeholder || name || role || e.tagName.toLowerCase();
            }})()"#,
            target.js_resolver()
        ))
        .ok()?;
    let name = resolved.as_str()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(truncate_label(name, 48))
    }
}

fn step_worthy_primitive(name: &str) -> bool {
    matches!(
        name,
        "click"
            | "type-text"
            | "fill"
            | "press-key"
            | "select"
            | "check"
            | "dialog"
            | "drag"
            | "navigate"
            | "tab-history"
            | "scroll"
            | "switch-tab"
            | "close-tab"
            | "open-in-new-tab"
            | "upload"
            | "highlight"
            | "evaluate-js"
            | "inject-js"
    )
}

pub fn record_step(
    tool: &str,
    args: &[String],
    duration_ms: u128,
    trace_id: &str,
    span_id: &str,
) -> Result<(), Error> {
    record_step_prepared(tool, args, duration_ms, trace_id, span_id, None)
}

fn record_step_prepared(
    tool: &str,
    args: &[String],
    duration_ms: u128,
    trace_id: &str,
    span_id: &str,
    prepared: Option<&PreparedStep>,
) -> Result<(), Error> {
    let active_bytes = match fs::read(RECORDING_ACTIVE) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(()),
    };
    let active: Value = serde_json::from_slice(&active_bytes)?;
    if active["mode"].as_str() != Some("steps") {
        return Ok(());
    }
    let dir = active["dir"]
        .as_str()
        .map(Path::new)
        .ok_or("step recording has no directory")?;
    fs::create_dir_all(dir)?;
    let mut browser = match BrowserSession::connect() {
        Ok(browser) => browser,
        Err(_) => return Ok(()),
    };
    if tool == "agent-open-in-new-tab" {
        for _ in 0..20 {
            let url = browser
                .eval("location.href")
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned));
            if url.as_deref().is_some_and(|value| value != "about:blank") {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    settle_step_capture(&mut browser);
    let context = browser
        .eval("({url:location.href,title:document.title||''})")
        .unwrap_or(Value::Null);
    let screenshot = browser.call(
        "Page.captureScreenshot",
        json!({
            "format":"png",
            "fromSurface":true,
            "captureBeyondViewport":false,
            "optimizeForSpeed":true
        }),
    )?;
    let data = screenshot["result"]["data"]
        .as_str()
        .ok_or("step screenshot returned no image")?;
    let bytes = STANDARD
        .decode(data)
        .map_err(|error| format!("step screenshot decode failed: {error}"))?;
    let created_at_ms = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    let frame_name = format!("{created_at_ms}-{span_id}.png");
    let frame_path = dir.join(&frame_name);
    fs::write(&frame_path, bytes)?;
    let hold_ms = active["hold_ms"].as_u64().unwrap_or(1000);
    let label = step_label(tool, args, &context, prepared);
    let safe_url = context["url"].as_str().map(sanitize_url);
    let entry = json!({
        "timestamp": OffsetDateTime::now_utc().format(&Rfc3339)?,
        "timestamp_ms": created_at_ms,
        "trace_id": trace_id,
        "span_id": span_id,
        "tool": tool,
        "args": args,
        "label": label,
        "duration_ms": duration_ms,
        "hold_ms": hold_ms,
        "url": safe_url,
        "title": context["title"],
        "frame": frame_name
    });
    let mut steps = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("steps.jsonl"))?;
    writeln!(steps, "{entry}")?;
    Ok(())
}

fn settle_step_capture(browser: &mut BrowserSession) {
    thread::sleep(Duration::from_millis(350));

    let deadline = Instant::now() + Duration::from_millis(1_600);
    let mut previous = None::<String>;
    let mut stable_samples = 0_u8;

    while Instant::now() < deadline {
        let state = browser.eval(
            r#"(() => {
                const visibleImages = [...document.images].filter(img => {
                    const rect = img.getBoundingClientRect();
                    return rect.width > 0 && rect.height > 0;
                });
                return {
                    ready: document.readyState,
                    url: location.href,
                    title: document.title || "",
                    scrollX: Math.round(scrollX),
                    scrollY: Math.round(scrollY),
                    width: innerWidth,
                    height: innerHeight,
                    documentHeight: document.documentElement.scrollHeight,
                    imagesReady: visibleImages.every(img => img.complete)
                };
            })()"#,
        );

        if let Ok(state) = state {
            let ready = state["ready"].as_str().unwrap_or("");
            let images_ready = state["imagesReady"].as_bool().unwrap_or(true);
            let signature = state.to_string();
            if ready != "loading" && images_ready {
                if previous.as_deref() == Some(signature.as_str()) {
                    stable_samples += 1;
                } else {
                    stable_samples = 0;
                }
                if stable_samples >= 1 {
                    break;
                }
            } else {
                stable_samples = 0;
            }
            previous = Some(signature);
        }

        thread::sleep(Duration::from_millis(120));
    }

    let _ = browser.eval(
        "(async()=>{await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));return true})()",
    );
    thread::sleep(Duration::from_millis(80));
}

fn step_label(
    tool: &str,
    args: &[String],
    context: &Value,
    prepared: Option<&PreparedStep>,
) -> String {
    let destination = concise_destination(context);
    let subject = prepared
        .and_then(|prepared| prepared.subject.as_deref())
        .unwrap_or("target");
    let label = match tool.strip_prefix("agent-").unwrap_or(tool) {
        "open-browser" => format!("opening browser at {destination}"),
        "close-browser" => "closing browser".into(),
        "navigate" => format!("navigating to {destination}"),
        "switch-tab" => format!("switching tab to {destination}"),
        "open-in-new-tab" => format!("opening {subject} in new tab"),
        "close-tab" => format!("closing tab, now at {destination}"),
        "click" => format!("clicking {subject}"),
        "scroll" => prepared
            .and_then(|prepared| prepared.subject.as_deref())
            .map(|subject| format!("scrolling to {subject}"))
            .unwrap_or_else(|| {
                format!(
                    "scrolling {}",
                    args.first().map(String::as_str).unwrap_or("page")
                )
            }),
        "fill" => format!("filling {subject}"),
        "type-text" => format!("typing into {subject}"),
        "press-key" => format!(
            "pressing {}",
            args.first().map(String::as_str).unwrap_or("key")
        ),
        "select" => format!(
            "selecting {} in {subject}",
            args.get(1).map(String::as_str).unwrap_or("option")
        ),
        "check" => format!("checking {subject}"),
        "upload" => format!("uploading file to {subject}"),
        "highlight" => format!(
            "highlighting {}",
            args.get(1).map(String::as_str).unwrap_or(subject)
        ),
        "tab-history" => format!("moving browser history to {destination}"),
        "drag" => format!("dragging {subject}"),
        "dialog" => "handling browser dialog".into(),
        "evaluate-js"
            if args.first().is_some_and(|script| {
                script.contains("data-color-mode") && script.contains("dark")
            }) =>
        {
            "switching page to dark mode".into()
        }
        "evaluate-js" => "evaluating page script".into(),
        "inject-js" => "injecting page script".into(),
        other => other.replace('-', " "),
    };
    truncate_label(&label, 64)
}

fn concise_destination(context: &Value) -> String {
    if let Some(title) = context["title"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        let mut title = title.trim().to_owned();
        if let Some(stripped) = title.strip_suffix(" · GitHub") {
            title = stripped.to_owned();
        }
        if let Some(stripped) = title.strip_prefix("GitHub - ") {
            title = stripped.to_owned();
        }
        if title.contains('/')
            && title.contains(':')
            && let Some((repo, _)) = title.split_once(':')
        {
            title = repo.to_owned();
        }
        return truncate_label(&title, 48);
    }

    if let Some(url) = context["url"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        && let Ok(parsed) = url::Url::parse(url)
        && let Some(host) = parsed.host_str()
    {
        let host = host.strip_prefix("www.").unwrap_or(host);
        let path = parsed.path().trim_matches('/');
        if path.is_empty() {
            return truncate_label(host, 48);
        }
        return truncate_label(&format!("{host}/{path}"), 48);
    }

    "current page".into()
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = normalized
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
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

pub fn redact_tool_args(tool: &str, args: &[String]) -> Vec<String> {
    let mut out = redact_args(args);
    match tool.strip_prefix("agent-").unwrap_or(tool) {
        "fill" | "type-text" => {
            if let Some(value) = out.first_mut() {
                *value = "<redacted-input>".into();
            }
        }
        "dialog" => {
            if let Some(value) = out.get_mut(1) {
                *value = "<redacted-input>".into();
            }
        }
        _ => {}
    }
    out
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

    #[test]
    fn tool_redaction_hides_text_entry_payloads() {
        assert_eq!(
            redact_tool_args("fill", &["secret text".into(), "@e2".into()]),
            vec!["<redacted-input>".to_owned(), "@e2".to_owned()]
        );
        assert_eq!(
            redact_tool_args("agent-type-text", &["private".into(), "@e1".into()]),
            vec!["<redacted-input>".to_owned(), "@e1".to_owned()]
        );
        assert_eq!(
            redact_tool_args("dialog", &["accept".into(), "private".into()]),
            vec!["accept".to_owned(), "<redacted-input>".to_owned()]
        );
    }

    #[test]
    fn step_labels_describe_the_visible_action_state() {
        let context = json!({
            "url": "https://example.com/complete",
            "title": "Complete"
        });
        let submit = PreparedStep {
            subject: Some("Submit".into()),
        };
        assert_eq!(
            step_label("agent-click", &["@e14".into()], &context, Some(&submit)),
            "clicking Submit"
        );

        let dropdown = PreparedStep {
            subject: Some("Country".into()),
        };
        assert_eq!(
            step_label(
                "agent-select",
                &["@e7".into(), "Italy".into()],
                &context,
                Some(&dropdown)
            ),
            "selecting Italy in Country"
        );
    }
}
