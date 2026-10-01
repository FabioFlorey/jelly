use crate::{
    BrowserSession, ROUTINE_STATE_DIR, classify_error, error_details, execute_browser_primitive,
    is_browser_primitive,
};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    env, fs,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ROUTINES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.agent/routines");
const STATE: &str = ROUTINE_STATE_DIR;

#[derive(Debug)]
struct RoutineFailure {
    kind: String,
    message: String,
    retryable: bool,
    details: Option<Value>,
}

impl RoutineFailure {
    fn new(kind: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            retryable,
            details: None,
        }
    }

    fn from_browser(error: crate::Error) -> Self {
        let (kind, retryable) = classify_error(error.as_ref());
        Self {
            kind: kind.as_str().to_owned(),
            message: error.to_string(),
            retryable,
            details: error_details(error.as_ref()),
        }
    }
}

fn vars(args: &[String]) -> HashMap<String, String> {
    args.iter()
        .filter_map(|x| x.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn render_legacy(mut s: String, vars: &HashMap<String, String>) -> String {
    for (k, v) in vars {
        s = s
            .replace(&format!("{{{{ {k} }}}}"), v)
            .replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur)
    }
    out
}

fn state_id() -> String {
    format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

fn system_failure_kind(name: &str) -> &'static str {
    match name {
        "open-browser" | "close-browser" | "browser-task" => "browser_unavailable",
        "screenshot" | "verify-artifact" => "artifact_failed",
        "downloads" | "wait-download" => "download_failed",
        "hitl" => "delivery_failed",
        _ => "internal",
    }
}

fn system_tool(name: &str, args: &[String]) -> Result<String, RoutineFailure> {
    let bin_dir = env::current_exe()
        .map_err(|e| RoutineFailure::new("internal", e.to_string(), false))?
        .parent()
        .ok_or_else(|| {
            RoutineFailure::new("internal", "call-routine executable has no parent", false)
        })?
        .to_path_buf();
    let target = bin_dir.join(format!("agent-{name}"));
    if !target.exists() {
        return Err(RoutineFailure::new(
            "unsupported",
            format!("unknown/unbuilt tool: {name}"),
            false,
        ));
    }
    let bin = bin_dir.join("agent-run");
    let mut command = Command::new(bin);
    command.arg(name).args(args);
    if name == "screenshot" && !args.iter().any(|arg| arg == "--json") {
        command.arg("--json");
    }
    let out = command
        .output()
        .map_err(|e| RoutineFailure::new("internal", e.to_string(), false))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            format!("agent-{name} failed with {}", out.status)
        } else {
            stderr
        };
        let kind = if name == "wait-download" && message.contains("timed out") {
            "condition_timeout"
        } else {
            system_failure_kind(name)
        };
        return Err(RoutineFailure::new(
            kind,
            message,
            matches!(
                kind,
                "browser_unavailable" | "delivery_failed" | "condition_timeout"
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn execute_tool(
    browser: &mut Option<BrowserSession>,
    name: &str,
    args: &[String],
) -> Result<String, RoutineFailure> {
    if is_browser_primitive(name) {
        if browser.is_none() {
            *browser = Some(BrowserSession::connect().map_err(RoutineFailure::from_browser)?);
        }
        execute_browser_primitive(browser.as_mut().unwrap(), name, args)
            .map_err(RoutineFailure::from_browser)
    } else {
        system_tool(name, args)
    }
}

fn legacy_run(
    lines: &[String],
    start: usize,
    mut vars: HashMap<String, String>,
    id: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(STATE)?;
    let mut browser: Option<BrowserSession> = None;
    for (i, raw) in lines.iter().enumerate().skip(start) {
        let line = render_legacy(raw.trim().to_owned(), &vars);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts = split(&line);
        if parts.is_empty() {
            continue;
        }
        match parts[0].as_str() {
            "signal" => {
                let cmd = parts.get(1).ok_or("signal requires a tool")?;
                let output = execute_tool(&mut browser, cmd, &parts[2..])
                    .map_err(|e| format!("{}: {}", e.kind, e.message))?;
                println!(
                    "[signal:{cmd}]
{}",
                    output.trim()
                );
            }
            "handoff" => {
                let sid = id.clone().unwrap_or_else(state_id);
                let message = parts[1..].join(" ");
                let body = json!({"kind":"legacy","next":i+1,"lines":lines,"vars":vars});
                fs::write(format!("{STATE}/{sid}.state"), serde_json::to_vec(&body)?)?;
                println!("handoff {sid} {message}");
                return Ok(());
            }
            "hitl" => {
                let message = parts.get(1..).unwrap_or_default().join(" ");
                if message.is_empty() {
                    return Err("hitl requires a message".into());
                }
                system_tool("hitl", &parts[1..])
                    .map_err(|e| format!("{}: {}", e.kind, e.message))?;
                let sid = id.clone().unwrap_or_else(state_id);
                let body = json!({"kind":"legacy","next":i+1,"lines":lines,"vars":vars});
                fs::write(format!("{STATE}/{sid}.state"), serde_json::to_vec(&body)?)?;
                println!("hitl {sid} {message}");
                return Ok(());
            }
            "set" => {
                let kv = parts.get(1).ok_or("set requires name=value")?;
                let (k, v) = kv.split_once('=').ok_or("set requires name=value")?;
                vars.insert(k.to_owned(), v.to_owned());
            }
            cmd => {
                let output = execute_tool(&mut browser, cmd, &parts[1..])
                    .map_err(|e| format!("{}: {}", e.kind, e.message))?;
                if !output.trim().is_empty() {
                    println!("{}", output.trim())
                }
            }
        }
    }
    if let Some(id) = id {
        let _ = fs::remove_file(format!("{STATE}/{id}.state"));
    }
    Ok(())
}

fn context_from_vars(vars: HashMap<String, String>) -> Map<String, Value> {
    vars.into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

fn lookup<'a>(context: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = context.get(first)?;
    for part in parts {
        value = value.as_object()?.get(part)?;
    }
    Some(value)
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => value.to_string(),
    }
}

fn render_text(input: &str, context: &Map<String, Value>) -> Result<String, RoutineFailure> {
    let trimmed = input.trim();
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        let path = trimmed[2..trimmed.len() - 2].trim();
        if !path.contains("}}") && !path.contains("{{") {
            return lookup(context, path).map(scalar_text).ok_or_else(|| {
                RoutineFailure::new(
                    "invalid_arguments",
                    format!("routine context value not found: {path}"),
                    false,
                )
            });
        }
    }

    let mut out = input.to_owned();
    let mut cursor = 0;
    while let Some(start_rel) = out[cursor..].find("{{") {
        let start = cursor + start_rel;
        let Some(end_rel) = out[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end_rel;
        let path = out[start + 2..end].trim();
        let value = lookup(context, path).ok_or_else(|| {
            RoutineFailure::new(
                "invalid_arguments",
                format!("routine context value not found: {path}"),
                false,
            )
        })?;
        let replacement = scalar_text(value);
        out.replace_range(start..end + 2, &replacement);
        cursor = start + replacement.len();
    }
    Ok(out)
}

fn render_args(node: &Value, context: &Map<String, Value>) -> Result<Vec<String>, RoutineFailure> {
    node.get("args")
        .and_then(Value::as_array)
        .map(|args| {
            args.iter()
                .map(|arg| {
                    let value = arg.as_str().ok_or_else(|| {
                        RoutineFailure::new(
                            "invalid_arguments",
                            "routine args must be strings",
                            false,
                        )
                    })?;
                    render_text(value, context)
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn parse_output(output: &str) -> Value {
    serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_owned()))
}

fn error_value(error: &RoutineFailure) -> Value {
    let mut value = Map::from_iter([
        ("kind".to_owned(), Value::String(error.kind.clone())),
        ("message".to_owned(), Value::String(error.message.clone())),
        ("retryable".to_owned(), Value::Bool(error.retryable)),
    ]);
    if let Some(details) = &error.details {
        value.insert("details".to_owned(), details.clone());
    }
    Value::Object(value)
}

fn error_transition(node: &Value, kind: &str) -> Option<String> {
    let on_error = node.get("on_error")?;
    if let Some(next) = on_error.as_str() {
        return Some(next.to_owned());
    }
    let object = on_error.as_object()?;
    object
        .get(kind)
        .or_else(|| object.get("*"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn guard_matches(guard: &Value, context: &Map<String, Value>) -> Result<bool, RoutineFailure> {
    let object = guard.as_object().ok_or_else(|| {
        RoutineFailure::new("invalid_arguments", "guard must be an object", false)
    })?;
    let op = object.get("op").and_then(Value::as_str).unwrap_or("equals");
    if op == "error_kind" {
        let expected = object.get("value").and_then(Value::as_str).ok_or_else(|| {
            RoutineFailure::new(
                "invalid_arguments",
                "error_kind guard requires value",
                false,
            )
        })?;
        return Ok(lookup(context, "_error.kind").and_then(Value::as_str) == Some(expected));
    }

    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| RoutineFailure::new("invalid_arguments", "guard requires path", false))?;
    let value = lookup(context, path);
    Ok(match op {
        "exists" => value.is_some() && !value.is_some_and(Value::is_null),
        "true" => value.and_then(Value::as_bool) == Some(true),
        "false" => value.and_then(Value::as_bool) == Some(false),
        "equals" => value == object.get("value"),
        "not_equals" => value != object.get("value"),
        _ => {
            return Err(RoutineFailure::new(
                "invalid_arguments",
                format!("unsupported guard operation: {op}"),
                false,
            ));
        }
    })
}

struct GraphFinalizer {
    browser_owned: bool,
    keep_browser: bool,
    suspended: bool,
    state: Option<String>,
}

impl GraphFinalizer {
    fn new(browser_owned: bool, keep_browser: bool, state: Option<String>) -> Self {
        Self {
            browser_owned,
            keep_browser,
            suspended: false,
            state,
        }
    }

    fn finalize(&mut self) -> Result<(), RoutineFailure> {
        if self.browser_owned && !self.keep_browser {
            system_tool("close-browser", &[])?;
            self.browser_owned = false;
        }
        if let Some(id) = self.state.as_deref() {
            let _ = fs::remove_file(format!("{STATE}/{id}.state"));
        }
        Ok(())
    }

    fn suspend(&mut self) {
        self.suspended = true;
    }
}

impl Drop for GraphFinalizer {
    fn drop(&mut self) {
        if self.suspended {
            return;
        }
        if self.browser_owned && !self.keep_browser {
            let _ = system_tool("close-browser", &[]);
            self.browser_owned = false;
        }
        if let Some(id) = self.state.as_deref() {
            let _ = fs::remove_file(format!("{STATE}/{id}.state"));
        }
    }
}

fn save_graph_state(
    id: &str,
    graph: &Value,
    current: &str,
    context: &Map<String, Value>,
    steps: u64,
    visits: &HashMap<String, u64>,
    browser_owned: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(STATE)?;
    let body = json!({
        "kind":"graph",
        "graph":graph,
        "current":current,
        "context":context,
        "steps":steps,
        "visits":visits,
        "browser_owned":browser_owned
    });
    fs::write(
        format!("{STATE}/{id}.state"),
        serde_json::to_vec_pretty(&body)?,
    )?;
    Ok(())
}

fn graph_run(
    graph: Value,
    start: String,
    mut context: Map<String, Value>,
    mut steps: u64,
    mut visits: HashMap<String, u64>,
    browser_owned: bool,
    state: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(STATE)?;
    let max_steps = graph
        .get("max_steps")
        .and_then(Value::as_u64)
        .unwrap_or(100);
    let max_duration_ms = graph
        .get("max_duration_ms")
        .and_then(Value::as_u64)
        .unwrap_or(300_000);
    let started = Instant::now();
    let keep_browser = graph
        .get("keep_browser")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let nodes = graph
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or("graph routine requires nodes")?;
    let mut current = start;
    let mut browser: Option<BrowserSession> = None;
    let mut finalizer = GraphFinalizer::new(browser_owned, keep_browser, state);

    loop {
        if started.elapsed() > Duration::from_millis(max_duration_ms) {
            return Err(
                format!("routine execution time budget exceeded ({max_duration_ms} ms)").into(),
            );
        }
        steps += 1;
        if steps > max_steps {
            return Err(format!("routine execution budget exceeded ({max_steps} steps)").into());
        }
        let count = visits.entry(current.clone()).or_insert(0);
        *count += 1;

        let node = nodes
            .get(&current)
            .ok_or_else(|| format!("routine node not found: {current}"))?;
        if let Some(max_visits) = node.get("max_visits").and_then(Value::as_u64)
            && *count > max_visits
        {
            return Err(format!("routine node {current} exceeded max_visits={max_visits}").into());
        }

        if let Some(outcome) = node.get("terminal").and_then(Value::as_str) {
            let message = node
                .get("message")
                .and_then(Value::as_str)
                .map(|m| render_text(m, &context))
                .transpose()
                .map_err(|e| format!("{}: {}", e.kind, e.message))?
                .unwrap_or_else(|| outcome.to_owned());
            if outcome == "success" {
                finalizer
                    .finalize()
                    .map_err(|e| format!("cleanup failed: {}", e.message))?;
                println!(
                    "{}",
                    json!({"status":"completed","message":message,"context":context})
                );
                return Ok(());
            }
            let _ = finalizer.finalize();
            return Err(message.into());
        }

        if let Some(next) = node.get("goto").and_then(Value::as_str) {
            current = next.to_owned();
            continue;
        }

        if let Some(guard) = node.get("guard") {
            let matched =
                guard_matches(guard, &context).map_err(|e| format!("{}: {}", e.kind, e.message))?;
            current = node
                .get(if matched { "then" } else { "else" })
                .and_then(Value::as_str)
                .ok_or("guard node requires then and else")?
                .to_owned();
            continue;
        }

        if let Some(message) = node.get("hitl").and_then(Value::as_str) {
            let message =
                render_text(message, &context).map_err(|e| format!("{}: {}", e.kind, e.message))?;
            match system_tool("hitl", std::slice::from_ref(&message)) {
                Ok(delivery) => {
                    context.insert("_hitl_delivery".into(), parse_output(&delivery));
                    let resume = node
                        .get("resume")
                        .or_else(|| node.get("next"))
                        .and_then(Value::as_str)
                        .ok_or("hitl node requires resume or next")?;
                    let id = finalizer.state.clone().unwrap_or_else(state_id);
                    save_graph_state(
                        &id,
                        &graph,
                        resume,
                        &context,
                        steps,
                        &visits,
                        finalizer.browser_owned,
                    )?;
                    finalizer.state = Some(id.clone());
                    finalizer.suspend();
                    println!(
                        "{}",
                        json!({"status":"suspended","resume_id":id,"message":message})
                    );
                    return Ok(());
                }
                Err(error) => {
                    context.insert("_error".into(), error_value(&error));
                    if let Some(next) = error_transition(node, &error.kind) {
                        current = next;
                        continue;
                    }
                    return Err(format!("{}: {}", error.kind, error.message).into());
                }
            }
        }

        let tool_name = node.get("tool").and_then(Value::as_str).ok_or_else(|| {
            format!("routine node {current} has no tool/guard/hitl/terminal/goto")
        })?;
        let args = render_args(node, &context).map_err(|e| format!("{}: {}", e.kind, e.message))?;

        match execute_tool(&mut browser, tool_name, &args) {
            Ok(output) => {
                context.remove("_error");
                let data = parse_output(&output);
                context.insert("_last".into(), data.clone());
                if let Some(save) = node.get("save").and_then(Value::as_str) {
                    context.insert(save.to_owned(), data);
                }
                if tool_name == "open-browser" {
                    finalizer.browser_owned = true;
                } else if tool_name == "close-browser" {
                    finalizer.browser_owned = false;
                }
                if let Some(next) = node.get("next").and_then(Value::as_str) {
                    current = next.to_owned();
                } else {
                    finalizer
                        .finalize()
                        .map_err(|e| format!("cleanup failed: {}", e.message))?;
                    println!("{}", json!({"status":"completed","context":context}));
                    return Ok(());
                }
            }
            Err(error) => {
                context.insert("_error".into(), error_value(&error));
                if let Some(next) = error_transition(node, &error.kind) {
                    current = next;
                } else {
                    return Err(format!("{}: {}", error.kind, error.message).into());
                }
            }
        }
    }
}

fn validate_graph(graph: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let entry = graph
        .get("entry")
        .and_then(Value::as_str)
        .ok_or("graph routine requires entry")?;
    let nodes = graph
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or("graph routine requires nodes")?;
    if !nodes.contains_key(entry) {
        return Err(format!("entry node does not exist: {entry}").into());
    }
    for (name, node) in nodes {
        for field in ["next", "then", "else", "resume", "goto"] {
            if let Some(target) = node.get(field).and_then(Value::as_str)
                && !nodes.contains_key(target)
            {
                return Err(format!("node {name} {field} points to missing node {target}").into());
            }
        }
        if let Some(on_error) = node.get("on_error") {
            if let Some(target) = on_error.as_str() {
                if !nodes.contains_key(target) {
                    return Err(
                        format!("node {name} on_error points to missing node {target}").into(),
                    );
                }
            } else if let Some(map) = on_error.as_object() {
                for target in map.values().filter_map(Value::as_str) {
                    if !nodes.contains_key(target) {
                        return Err(format!(
                            "node {name} on_error points to missing node {target}"
                        )
                        .into());
                    }
                }
            } else {
                return Err(
                    format!("node {name} on_error must be a node name or error map").into(),
                );
            }
        }
    }
    Ok(())
}

fn start_graph(
    name: &str,
    extra: HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("{ROUTINES}/{name}.json");
    let graph: Value = serde_json::from_slice(&fs::read(path)?)?;
    validate_graph(&graph)?;
    let entry = graph["entry"].as_str().unwrap().to_owned();
    graph_run(
        graph,
        entry,
        context_from_vars(extra),
        0,
        HashMap::new(),
        false,
        None,
    )
}

fn resume(id: &str, extra: HashMap<String, String>) -> Result<(), Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_slice(&fs::read(format!("{STATE}/{id}.state"))?)?;
    if value["kind"] == "graph" {
        let graph = value["graph"].clone();
        validate_graph(&graph)?;
        let current = value["current"]
            .as_str()
            .ok_or("bad graph state")?
            .to_owned();
        let mut context = value["context"]
            .as_object()
            .cloned()
            .ok_or("bad graph state")?;
        for (key, value) in extra {
            context.insert(key, Value::String(value));
        }
        let steps = value["steps"].as_u64().unwrap_or(0);
        let visits = value["visits"]
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
                    .collect()
            })
            .unwrap_or_default();
        let browser_owned = value["browser_owned"].as_bool().unwrap_or(false);
        return graph_run(
            graph,
            current,
            context,
            steps,
            visits,
            browser_owned,
            Some(id.to_owned()),
        );
    }

    let start = value["next"].as_u64().ok_or("bad state")? as usize;
    let lines = value["lines"]
        .as_array()
        .ok_or("bad state")?
        .iter()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let mut saved_vars = value["vars"]
        .as_object()
        .ok_or("bad state")?
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
        .collect::<HashMap<_, _>>();
    saved_vars.extend(extra);
    legacy_run(&lines, start, saved_vars, Some(id.to_owned()))
}

pub fn run_from_env() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("resume") {
        return resume(
            args.get(1)
                .ok_or("usage: call-routine resume <id> [key=value]")?,
            vars(&args[2..]),
        );
    }

    let name = args
        .first()
        .ok_or("usage: call-routine <name> [key=value]")?;
    let graph_path = format!("{ROUTINES}/{name}.json");
    if std::path::Path::new(&graph_path).is_file() {
        return start_graph(name, vars(&args[1..]));
    }

    let path = format!("{ROUTINES}/{name}.jinja");
    let lines = fs::read_to_string(path)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    legacy_run(&lines, 0, vars(&args[1..]), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_validation_allows_cycles_and_jumps() {
        let graph = json!({
            "entry":"inspect",
            "nodes":{
                "inspect":{"guard":{"path":"ready","op":"true"},"then":"done","else":"wait"},
                "wait":{"goto":"inspect"},
                "done":{"terminal":"success"}
            }
        });
        validate_graph(&graph).unwrap();
    }

    #[test]
    fn graph_validation_rejects_missing_edges() {
        let graph = json!({
            "entry":"a",
            "nodes":{"a":{"goto":"missing"}}
        });
        assert!(validate_graph(&graph).is_err());
    }

    #[test]
    fn guards_read_nested_evidence() {
        let mut context = Map::new();
        context.insert("target".into(), json!({"visible":true,"tag":"img"}));
        assert!(guard_matches(&json!({"path":"target.visible","op":"true"}), &context).unwrap());
        assert!(
            guard_matches(
                &json!({"path":"target.tag","op":"equals","value":"img"}),
                &context
            )
            .unwrap()
        );
    }

    #[test]
    fn typed_error_guards_branch_on_error_kind() {
        let mut context = Map::new();
        context.insert(
            "_error".into(),
            json!({"kind":"target_stale","message":"stale","retryable":true}),
        );
        assert!(
            guard_matches(&json!({"op":"error_kind","value":"target_stale"}), &context).unwrap()
        );
        assert!(
            !guard_matches(
                &json!({"op":"error_kind","value":"target_not_found"}),
                &context
            )
            .unwrap()
        );
    }

    #[test]
    fn routine_failures_preserve_cdp_details() {
        let failure = RoutineFailure::from_browser(crate::cdp_error(
            "Runtime.missing",
            -32601,
            "Method not found",
            None,
        ));
        assert_eq!(failure.kind, "cdp_failed");
        assert!(!failure.retryable);
        assert_eq!(
            error_value(&failure)["details"],
            json!({
                "protocol":"cdp",
                "method":"Runtime.missing",
                "code":-32601,
                "message":"Method not found"
            })
        );
    }

    #[test]
    fn rendering_uses_nested_context() {
        let mut context = Map::new();
        context.insert("artifact".into(), json!({"artifact_id":"artifact-1"}));
        assert_eq!(
            render_text("{{ artifact.artifact_id }}", &context).unwrap(),
            "artifact-1"
        );
    }

    #[test]
    fn bundled_graph_routines_validate() {
        for name in ["verified-screenshot", "verified-download"] {
            let path = format!("{ROUTINES}/{name}.json");
            let graph: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            validate_graph(&graph).unwrap();
        }
    }
}
