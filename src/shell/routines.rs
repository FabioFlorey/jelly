use crate::core::routines::{
    PlannedNode, PlannedToolOutcome, RoutineFailure, context_from_vars, error_transition,
    error_value, parse_output, plan_node, plan_tool_outcome, render_legacy, split,
    system_failure_kind, validate_graph, vars,
};
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

#[cfg(test)]
use crate::core::routines::{guard_matches, render_text};

const ROUTINES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.agent/routines");

fn routine_state() -> &'static str {
    ROUTINE_STATE_DIR.as_str()
}

impl RoutineFailure {
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

fn state_id() -> String {
    format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
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
        let kind = if matches!(name, "wait-download" | "download") && message.contains("timed out")
        {
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
    fs::create_dir_all(routine_state())?;
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
                fs::write(
                    format!("{}/{sid}.state", routine_state()),
                    serde_json::to_vec(&body)?,
                )?;
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
                fs::write(
                    format!("{}/{sid}.state", routine_state()),
                    serde_json::to_vec(&body)?,
                )?;
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
        remove_state_file(&id)?;
    }
    Ok(())
}

fn remove_state_file(id: &str) -> Result<(), std::io::Error> {
    let path = format!("{}/{id}.state", routine_state());
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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
            remove_state_file(id).map_err(|error| {
                RoutineFailure::new(
                    "internal",
                    format!("failed to remove routine state {id}: {error}"),
                    false,
                )
            })?;
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
            let _ = remove_state_file(id);
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
    fs::create_dir_all(routine_state())?;
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
        format!("{}/{id}.state", routine_state()),
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
    fs::create_dir_all(routine_state())?;
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

        match plan_node(&current, node, &context)? {
            PlannedNode::Terminal { success, message } => {
                if success {
                    finalizer
                        .finalize()
                        .map_err(|e| format!("cleanup failed: {}", e.message))?;
                    println!(
                        "{}",
                        json!({"status":"completed","message":message,"context":context})
                    );
                    return Ok(());
                }
                if let Err(cleanup) = finalizer.finalize() {
                    return Err(format!("{message}; cleanup failed: {}", cleanup.message).into());
                }
                return Err(message.into());
            }
            PlannedNode::Jump(next) => {
                current = next;
                continue;
            }
            PlannedNode::Hitl { message } => {
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
            PlannedNode::Tool { name, args } => {
                let result = execute_tool(&mut browser, &name, &args);
                match plan_tool_outcome(node, &name, result.as_ref().map(String::as_str)) {
                    PlannedToolOutcome::Success {
                        data,
                        save_as,
                        next,
                        browser_owned,
                    } => {
                        context.remove("_error");
                        context.insert("_last".into(), data.clone());
                        if let Some(save) = save_as {
                            context.insert(save, data);
                        }
                        if let Some(owned) = browser_owned {
                            finalizer.browser_owned = owned;
                        }
                        if let Some(next) = next {
                            current = next;
                        } else {
                            finalizer
                                .finalize()
                                .map_err(|e| format!("cleanup failed: {}", e.message))?;
                            println!("{}", json!({"status":"completed","context":context}));
                            return Ok(());
                        }
                    }
                    PlannedToolOutcome::Failure {
                        error,
                        recovery,
                        message,
                    } => {
                        context.insert("_error".into(), error);
                        if let Some(next) = recovery {
                            current = next;
                        } else {
                            return Err(message.into());
                        }
                    }
                }
            }
        }
    }
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
    let value: Value =
        serde_json::from_slice(&fs::read(format!("{}/{id}.state", routine_state()))?)?;
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

/// Run or resume a Jelly routine using the current process arguments and environment.
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
    fn system_tool_failure_kinds_match_mcp_classification() {
        assert_eq!(system_failure_kind("open-browser"), "browser_unavailable");
        assert_eq!(system_failure_kind("profile-import"), "interaction_failed");
        assert_eq!(system_failure_kind("record-browser"), "artifact_failed");
        assert_eq!(system_failure_kind("inspect-network"), "internal");
    }

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
