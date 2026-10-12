//! Conservatively convert a trace of *observed* browser primitives into an
//! immutable, reviewable auto-routine candidate. This never executes or promotes.
//! These log events are evidence of a past attempt, NOT proof of future safety.
use crate::core::routines::validate_routine_definition;
use serde_json::{Map, Value, json};
use std::{
    error::Error,
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

const MAX_LOG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TRACE_EVENTS: usize = 100;
const MAX_PAIRS: usize = 20;

/// Only operations with replayable, non-secret arguments are eligible here.
/// Form contents, script evaluation, network calls, navigation and uploads are
/// intentionally excluded until an explicit parameterization/review design.
fn action(name: &str) -> bool {
    matches!(name, "click" | "check" | "select" | "scroll")
}

fn verify(name: &str) -> bool {
    matches!(
        name,
        "assert-visible" | "assert-text" | "assert-title" | "assert-image-ready" | "wait-for"
    )
}

fn safe_args(name: &str, args: &Value) -> Result<Vec<String>, String> {
    let raw = args
        .as_array()
        .ok_or_else(|| format!("{name}: missing argument evidence"))?;
    let args = raw
        .iter()
        .map(|v| {
            v.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("{name}: non-string argument evidence"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let valid_count = match name {
        "click" | "check" | "assert-visible" | "assert-image-ready" => args.len() == 1,
        "scroll" => args.len() <= 1,
        "select" => args.len() == 2,
        "assert-title" => (1..=2).contains(&args.len()),
        "assert-text" | "wait-for" => (2..=3).contains(&args.len()),
        _ => false,
    };
    if !valid_count {
        return Err(format!("{name}: argument count cannot be replayed"));
    }
    if args.iter().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        arg.trim().is_empty()
            || arg.len() > 256
            || arg.starts_with('@') // ephemeral DOM refs are not stable selectors
            || arg.contains("{{")
            || arg.contains("}}")
            || lower.contains("redacted")
            || lower.contains("password=")
            || lower.contains("token=")
            || lower.contains("secret=")
            || lower.starts_with("http://")
            || lower.starts_with("https://")
    }) {
        return Err(format!(
            "{name}: trace includes unstable, redacted or sensitive arguments"
        ));
    }
    if name == "wait-for"
        && !matches!(
            args[0].as_str(),
            "exists" | "visible" | "hidden" | "text" | "title" | "image-ready"
        )
    {
        return Err("wait-for: unsupported verification predicate".into());
    }
    if matches!(name, "assert-title" | "assert-text")
        && args.last().is_some_and(|v| {
            args.len() == if name == "assert-title" { 2 } else { 3 }
                && !matches!(v.as_str(), "exact" | "contains")
        })
    {
        return Err(format!("{name}: invalid matching mode"));
    }
    if name == "wait-for"
        && args.len() == 3
        && args[2]
            .parse::<u64>()
            .ok()
            .is_none_or(|seconds| seconds == 0 || seconds > 60)
    {
        return Err("wait-for: invalid finite wait budget".into());
    }
    Ok(args)
}

#[derive(Debug)]
struct Step {
    tool: String,
    args: Vec<String>,
}

/// Read only Jelly's configured action log, and select a caller-supplied trace
/// ID. Reject all unrecognized events within that trace instead of silently
/// dropping operations that could have changed the page.
fn evidence(log: &Path, trace_id: &str) -> Result<Vec<Step>, Box<dyn Error>> {
    if trace_id.is_empty()
        || trace_id.len() > 128
        || !trace_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err("invalid trace ID".into());
    }
    let file = fs::File::open(log)?;
    if file.metadata()?.len() > MAX_LOG_BYTES {
        return Err("action log exceeds 32 MiB; rotate logs before crystallizing".into());
    }
    let mut steps = Vec::new();
    let mut trace_events = 0;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(&line)?;
        if entry["trace_id"].as_str() != Some(trace_id) {
            continue;
        }
        trace_events += 1;
        if trace_events > MAX_TRACE_EVENTS {
            return Err("trace exceeds 100 events".into());
        }
        let kind = entry["event"]
            .as_str()
            .ok_or("trace event is missing event kind")?;
        let name = entry["tool"]
            .as_str()
            .and_then(|v| v.strip_prefix("agent-"))
            .ok_or("trace event has unsupported tool identity")?;
        if !action(name) && !verify(name) {
            return Err(format!("trace contains unsupported operation: {name}").into());
        }
        if entry["ok"] != true || kind.ends_with(".failed") {
            return Err(format!("trace operation {name} failed; cannot crystallize").into());
        }
        match kind {
            // agent-run also emits a wrapper tool.completed for the same primitive.
            "tool.completed" => {}
            "primitive.completed" => {
                steps.push(Step {
                    tool: name.to_owned(),
                    args: safe_args(name, &entry["args"])?,
                });
            }
            _ => return Err(format!("trace contains unknown event kind: {kind}").into()),
        }
    }
    if steps.is_empty() {
        return Err("trace has no successful browser primitives".into());
    }
    Ok(steps)
}

pub(crate) fn candidate(log: &Path, trace_id: &str, base: &Value) -> Result<Value, Box<dyn Error>> {
    let steps = evidence(log, trace_id)?;
    let mut nodes = Map::new();
    let mut last_kind: Option<bool> = None; // true: verified, false: action
    let mut actions = 0;
    let mut verifications = 0;
    for (i, step) in steps.iter().enumerate() {
        let is_check = verify(&step.tool);
        if is_check && (last_kind.is_none() || last_kind == Some(true)) {
            return Err("verification without an immediately preceding action".into());
        }
        if !is_check && last_kind == Some(false) {
            return Err("every action needs an explicit successful verification".into());
        }
        if is_check {
            verifications += 1;
        } else {
            actions += 1;
        }
        if actions > MAX_PAIRS {
            return Err("trace exceeds 20 action/verification pairs".into());
        }
        let current = format!("step_{i:03}");
        let next = format!("step_{:03}", i + 1);
        nodes.insert(
            current,
            json!({
                "tool":step.tool,
                "args":step.args,
                "next": if i + 1 == steps.len() { "done".to_owned() } else { next },
                "on_error":"failed"
            }),
        );
        last_kind = Some(is_check);
    }
    if last_kind != Some(true) || verifications != actions {
        return Err("trace must end with a verified action".into());
    }
    nodes.insert("done".into(), json!({"terminal":"success"}));
    nodes.insert("failed".into(), json!({"terminal":"failure"}));
    let name = base["name"].as_str().ok_or("saved routine has no name")?;
    let graph = json!({
        "name":name,
        "description":format!("Reviewed candidate from trace {trace_id}; verify before promotion"),
        "mode":"auto",
        "entry":"step_000",
        "max_steps":steps.len() + 2,
        "nodes":nodes,
        "crystallization":{
            "trace_id":trace_id,
            "action_count":actions,
            "verification_count":verifications,
            "reviewed":false,
            "source":"logs/actions.jsonl",
            "limits":"allowlisted actions and one explicit check per action"
        }
    });
    validate_routine_definition(&graph, Some(name))?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn with_log<T>(records: &[Value], f: impl FnOnce(&Path) -> T) -> T {
        let path = std::env::temp_dir().join(format!(
            "jelly-crystallize-test-{:032x}.jsonl",
            rand::random::<u128>()
        ));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        for entry in records {
            writeln!(file, "{entry}").unwrap();
        }
        drop(file);
        let result = f(&path);
        let _ = fs::remove_file(path);
        result
    }

    fn event(trace: &str, tool: &str, args: &[&str], ok: bool) -> Value {
        json!({
            "trace_id":trace,
            "event":if ok {"primitive.completed"} else {"primitive.failed"},
            "tool":format!("agent-{tool}"),
            "args":args,"ok":ok
        })
    }

    fn base() -> Value {
        json!({"name":"sample","description":"Original",
            "entry":"agent","mode":"jelly",
            "nodes":{"agent":{"agent":"Find button","resume":"check"},
                "check":{"tool":"assert-visible","args":["text:Done"],"next":"done"},
                "done":{"terminal":"success"}}})
    }

    #[test]
    fn only_replayable_verified_pairs_generate_an_auto_candidate() {
        with_log(
            &[
                event("other", "fill", &["private"], true),
                event("t1", "click", &["text:Submit"], true),
                json!({"trace_id":"t1","event":"tool.completed","tool":"agent-click","ok":true}),
                event("t1", "assert-visible", &["text:Confirmation"], true),
            ],
            |path| {
                let graph = candidate(path, "t1", &base()).unwrap();
                assert_eq!(graph["mode"], "auto");
                assert_eq!(graph["nodes"]["step_000"]["tool"], "click");
                assert_eq!(graph["nodes"]["step_001"]["tool"], "assert-visible");
                assert_eq!(graph["nodes"]["step_001"]["next"], "done");
                assert_eq!(graph["crystallization"]["reviewed"], false);
                assert!(graph.get("revision").is_none());
            },
        );
    }

    #[test]
    fn crystallization_stages_a_revision_without_activating_it() {
        struct TestRoot(std::path::PathBuf);
        impl Drop for TestRoot {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let dir = TestRoot(std::env::temp_dir().join(format!(
            "jelly-crystallize-stage-{:032x}",
            rand::random::<u128>()
        )));
        fs::create_dir(&dir.0).unwrap();
        fs::write(
            dir.0.join("sample.json"),
            serde_json::to_vec(&base()).unwrap(),
        )
        .unwrap();
        with_log(
            &[
                event("trace-A", "click", &["css:#submit"], true),
                event("trace-A", "assert-visible", &["text:Confirmed"], true),
            ],
            |path| {
                let graph = candidate(path, "trace-A", &base()).unwrap();
                let staged =
                    crate::shell::routine_versions::stage(&dir.0, "sample", 1, graph).unwrap();
                assert_eq!(staged["revision"], 2);
                let old = crate::shell::routine_versions::exact(&dir.0, "sample", 1).unwrap();
                assert_eq!(old["mode"], "jelly");
                let new = crate::shell::routine_versions::exact(&dir.0, "sample", 2).unwrap();
                assert_eq!(new["mode"], "auto");
                assert_eq!(new["crystallization"]["trace_id"], "trace-A");
                assert_eq!(new["crystallization"]["reviewed"], false);
                let versions = crate::shell::routine_versions::versions(&dir.0, "sample").unwrap();
                assert_eq!(versions["active_revision"], 1);
                assert!(crate::shell::routine_versions::promote(&dir.0, "sample", 2, 99).is_err());
            },
        );
    }

    #[test]
    fn unsafe_or_unverified_traces_are_rejected() {
        for events in [
            vec![event("t1", "click", &["text:Submit"], true)],
            vec![event("t1", "assert-visible", &["text:Done"], true)],
            vec![
                event("t1", "click", &["@e3"], true),
                event("t1", "assert-visible", &["text:Done"], true),
            ],
            vec![
                event("t1", "fill", &["s3cr3t", "css:input"], true),
                event("t1", "assert-visible", &["text:Done"], true),
            ],
            vec![
                event("t1", "click", &["text:Submit"], true),
                event("t1", "assert-visible", &["text:Done"], false),
            ],
            vec![
                event("t1", "click", &["text:Submit"], true),
                event("t1", "click", &["text:Again"], true),
                event("t1", "assert-visible", &["text:Done"], true),
            ],
            vec![
                event("t1", "click", &["text:Submit"], true),
                event("t1", "wait-for", &["js", "true"], true),
            ],
            vec![
                event("t1", "click", &["text:Submit"], true),
                event("t1", "assert-visible", &["text:Done"], true),
                json!({"trace_id":"t1","event":"tool.failed","tool":"agent-click","ok":false}),
            ],
            vec![
                event("t1", "click", &["<redacted-input>"], true),
                event("t1", "assert-visible", &["text:Done"], true),
            ],
        ] {
            with_log(&events, |path| {
                assert!(candidate(path, "t1", &base()).is_err())
            });
        }
    }
}
