use crate::core::routines::{
    PlannedNode, PlannedToolOutcome, RoutineFailure, context_from_vars, error_transition,
    error_value, parse_output, plan_node, plan_tool_outcome, routine_mode, system_failure_kind,
    valid_routine_id, valid_routine_name, validate_routine_definition, vars,
};
use crate::shell::routine_handoff::{dispatch, select_target};
use crate::{
    BrowserSession, ROUTINE_STATE_DIR, classify_error, error_details, execute_browser_primitive,
    is_browser_primitive,
};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    env, fs,
    io::Write,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use crate::core::routines::{guard_matches, render_text, validate_graph};

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
    // Keep continuation IDs decimal for existing resume callers, but avoid
    // collisions when two graph runs suspend in the same millisecond.
    format!(
        "{}{:010}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        rand::random::<u32>()
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
    let mut command = if name == "call-routine" {
        // A chained graph executes through the same validated routine binary;
        // it does not need an agent-run wrapper process in between.
        let mut command = Command::new(&target);
        command.args(args);
        command
    } else {
        let mut command = Command::new(bin_dir.join("agent-run"));
        command.arg(name).args(args);
        command
    };
    if name == "screenshot" && !args.iter().any(|arg| arg == "--json") {
        command.arg("--json");
    }
    if name == "call-routine" {
        let depth = env::var("JELLY_ROUTINE_CALL_DEPTH")
            .ok()
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(0);
        if depth >= 8 {
            return Err(RoutineFailure::new(
                "invalid_arguments",
                "routine chain depth limit (8) exceeded",
                false,
            ));
        }
        command.env("JELLY_ROUTINE_CALL_DEPTH", (depth + 1).to_string());
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

struct RunContinuation {
    browser_owned: bool,
    state: Option<String>,
    handoff_to: String,
    pending_child: Option<String>,
}

fn save_graph_state(
    id: &str,
    graph: &Value,
    current: &str,
    context: &Map<String, Value>,
    steps: u64,
    visits: &HashMap<String, u64>,
    continuation: &RunContinuation,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(routine_state())?;
    let body = json!({
        "kind":"graph",
        "graph":graph,
        "current":current,
        "context":context,
        "steps":steps,
        "visits":visits,
        "browser_owned":continuation.browser_owned,
        "handoff_to":continuation.handoff_to,
        "pending_child":continuation.pending_child
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
    mut continuation: RunContinuation,
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
    let mut finalizer = GraphFinalizer::new(
        continuation.browser_owned,
        keep_browser,
        continuation.state.take(),
    );

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
                        continuation.browser_owned = finalizer.browser_owned;
                        continuation.pending_child = None;
                        save_graph_state(
                            &id,
                            &graph,
                            resume,
                            &context,
                            steps,
                            &visits,
                            &continuation,
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
            PlannedNode::Agent { instruction } => {
                // The selected agent performs work outside this process. On resume,
                // the next node is a validated verification tool, not the next action.
                let verify = node["resume"]
                    .as_str()
                    .ok_or("agent node requires verification resume target")?;
                let id = finalizer.state.clone().unwrap_or_else(state_id);
                continuation.browser_owned = finalizer.browser_owned;
                continuation.pending_child = None;
                save_graph_state(&id, &graph, verify, &context, steps, &visits, &continuation)?;
                finalizer.state = Some(id.clone());
                finalizer.suspend();
                let mut handoff = json!({
                    "status":"suspended",
                    "reason":"agent_required",
                    "resume_id":id,
                    "routine":graph["name"],
                    "node":current,
                    "instruction":instruction,
                    "verify":verify,
                    "error":context.get("_error")
                });
                let delivery = dispatch(&continuation.handoff_to, &handoff);
                handoff["handoff"] = delivery;
                println!("{handoff}");
                return Ok(());
            }
            PlannedNode::Tool { name, args } => {
                let result = if let Some(child_id) = continuation.pending_child.take() {
                    if name != "call-routine" {
                        return Err("pending child continuation requires call-routine node".into());
                    }
                    system_tool("call-routine", &["resume".into(), child_id])
                } else {
                    execute_tool(&mut browser, &name, &args)
                };
                // Nested calls execute in a separate process. Their suspension is
                // not a success: checkpoint the parent and relay the instruction
                // outward, so only the original caller (or selected adapter)
                // receives the actionable handoff.
                if name == "call-routine"
                    && let Ok(output) = &result
                    && let Ok(mut child) = serde_json::from_str::<Value>(output)
                    && child["status"] == "suspended"
                {
                    let child_id = child["resume_id"]
                        .as_str()
                        .ok_or("suspended child routine lacks resume_id")?;
                    if !child_id.bytes().all(|ch| ch.is_ascii_digit()) {
                        return Err("suspended child has invalid resume_id".into());
                    }
                    let parent_id = finalizer.state.clone().unwrap_or_else(state_id);
                    continuation.browser_owned = finalizer.browser_owned;
                    continuation.pending_child = Some(child_id.to_owned());
                    save_graph_state(
                        &parent_id,
                        &graph,
                        &current,
                        &context,
                        steps,
                        &visits,
                        &continuation,
                    )?;
                    finalizer.state = Some(parent_id.clone());
                    finalizer.suspend();
                    child["resume_id"] = json!(parent_id);
                    child["parent_routine"] = graph["name"].clone();
                    if child["reason"] == "agent_required" {
                        if let Some(object) = child.as_object_mut() {
                            object.remove("handoff");
                        }
                        child["handoff"] = dispatch(&continuation.handoff_to, &child);
                    }
                    println!("{child}");
                    return Ok(());
                }
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

fn new_routine_id() -> String {
    // UUID v4 using the existing rand dependency; avoid adding a UUID crate.
    let value = rand::random::<u128>();
    let value =
        (value & !(0xf_u128 << 76) & !(0x3_u128 << 62)) | (0x4_u128 << 76) | (0x2_u128 << 62);
    let hex = format!("{value:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn saved_routines() -> Result<Vec<(String, Value)>, Box<dyn std::error::Error>> {
    let mut routines = Vec::new();
    for entry in fs::read_dir(ROUTINES)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|part| part.to_str()) else {
            continue;
        };
        if !valid_routine_name(name) {
            continue;
        }
        let Ok(data) = fs::read(&path) else { continue };
        let Ok(graph) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
        if validate_routine_definition(&graph, Some(name)).is_ok() {
            routines.push((name.to_owned(), graph));
        }
    }
    routines.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(routines)
}

fn routine_summary(name: &str, graph: &Value) -> Value {
    let mode = match routine_mode(graph).expect("validated routine") {
        crate::core::routines::RoutineMode::Auto => "auto",
        crate::core::routines::RoutineMode::Hybrid => "hybrid",
        crate::core::routines::RoutineMode::Jelly => "jelly",
    };
    json!({"name":name,"id":graph.get("id"),"mode":mode,"description":graph["description"]})
}

fn load_routine(identifier: &str) -> Result<Value, Box<dyn std::error::Error>> {
    // A UUID beginning with a-f is also a syntactically valid routine name.
    // Resolve UUIDs first so that every generated ID remains callable.
    if valid_routine_id(identifier) {
        for (_, graph) in saved_routines()? {
            if graph["id"] == identifier {
                return Ok(graph);
            }
        }
        return Err("routine UUID not found".into());
    }
    if valid_routine_name(identifier) {
        let graph: Value =
            serde_json::from_slice(&fs::read(format!("{ROUTINES}/{identifier}.json"))?)?;
        validate_routine_definition(&graph, Some(identifier))?;
        return Ok(graph);
    }
    Err("invalid routine name or UUID".into())
}

/// Saving a routine never runs it. Refuse overwrites and unsafe names.
fn save_routine(json_source: &str) -> Result<(), Box<dyn std::error::Error>> {
    if json_source.len() > 64 * 1024 {
        return Err("routine graph exceeds the 64 KiB size limit".into());
    }
    let mut graph: Value = serde_json::from_str(json_source)?;
    validate_routine_definition(&graph, None)?;
    if graph.get("id").is_none() {
        graph
            .as_object_mut()
            .unwrap()
            .insert("id".into(), json!(new_routine_id()));
    }
    for (name, existing) in saved_routines()? {
        if existing["id"] == graph["id"] {
            return Err(format!("routine UUID already belongs to {name}").into());
        }
    }
    let name = graph["name"].as_str().unwrap();
    let path = format!("{ROUTINES}/{name}.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(serde_json::to_string_pretty(&graph)?.as_bytes())?;
    file.write_all(b"\n")?;
    println!(
        "{}",
        json!({"status":"saved","name":name,"id":graph["id"],"mode":routine_summary(name, &graph)["mode"],"description":graph["description"],"path":path})
    );
    Ok(())
}

fn list_routines() -> Result<(), Box<dyn std::error::Error>> {
    let routines: Vec<_> = saved_routines()?
        .iter()
        .map(|(name, graph)| routine_summary(name, graph))
        .collect();
    println!("{}", json!({"routines":routines}));
    Ok(())
}

fn search_routines(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let query = query.trim();
    if query.is_empty() || query.len() > 200 {
        return Err("routine search query must contain 1-200 characters".into());
    }
    let query = query.to_lowercase();
    let routines: Vec<_> = saved_routines()?
        .iter()
        .filter(|(name, graph)| {
            name.to_lowercase().contains(&query)
                || graph["description"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&query)
        })
        .map(|(name, graph)| routine_summary(name, graph))
        .collect();
    println!("{}", json!({"routines":routines}));
    Ok(())
}

fn describe_routine(identifier: &str) -> Result<(), Box<dyn std::error::Error>> {
    let graph = load_routine(identifier)?;
    println!("{}", serde_json::to_string_pretty(&graph)?);
    Ok(())
}

fn start_graph(
    name: &str,
    extra: HashMap<String, String>,
    handoff_to: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    select_target(handoff_to)?;
    let graph = load_routine(name)?;
    let entry = graph["entry"].as_str().unwrap().to_owned();
    graph_run(
        graph,
        entry,
        context_from_vars(extra),
        0,
        HashMap::new(),
        RunContinuation {
            browser_owned: false,
            state: None,
            handoff_to: handoff_to.to_owned(),
            pending_child: None,
        },
    )
}

fn resume(id: &str, extra: HashMap<String, String>) -> Result<(), Box<dyn std::error::Error>> {
    if id.is_empty() || !id.bytes().all(|ch| ch.is_ascii_digit()) {
        return Err("invalid routine resume ID".into());
    }
    let value: Value =
        serde_json::from_slice(&fs::read(format!("{}/{id}.state", routine_state()))?)?;
    if value["kind"] == "graph" {
        let graph = value["graph"].clone();
        validate_routine_definition(&graph, None)?;
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
        // Old HITL states had no handoff routing field.
        let handoff_to = value["handoff_to"].as_str().unwrap_or("caller");
        let pending_child = value["pending_child"].as_str().map(str::to_owned);
        return graph_run(
            graph,
            current,
            context,
            steps,
            visits,
            RunContinuation {
                browser_owned,
                state: Some(id.to_owned()),
                handoff_to: handoff_to.to_owned(),
                pending_child,
            },
        );
    }

    Err("legacy Jinja routine state is no longer supported; rerun a named JSON graph".into())
}

/// Run or resume a Jelly routine using the current process arguments and environment.
pub fn run_from_env() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let handoff_to = if args.first().is_some_and(|a| a == "--handoff-to") {
        if args.len() < 3 {
            return Err(
                "usage: call-routine --handoff-to <configured-agent> <name> [key=value]".into(),
            );
        }
        let target = args[1].clone();
        args.drain(..2);
        target
    } else {
        "caller".to_owned()
    };
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => return list_routines(),
        Some("--search") if args.len() == 2 => return search_routines(&args[1]),
        Some("describe") if args.len() == 2 => return describe_routine(&args[1]),
        Some("save") if args.len() == 2 => return save_routine(&args[1]),
        _ => {}
    }
    if args.first().map(String::as_str) == Some("resume") {
        if handoff_to != "caller" {
            return Err(
                "handoff_to is captured in the checkpoint and cannot change on resume".into(),
            );
        }
        return resume(
            args.get(1)
                .ok_or("usage: call-routine resume <id> [key=value]")?,
            vars(&args[2..]),
        );
    }

    let name = args
        .first()
        .ok_or("usage: call-routine <name> [key=value]")?;
    if !valid_routine_name(name) && !valid_routine_id(name) {
        return Err("invalid routine name or UUID".into());
    }
    start_graph(name, vars(&args[1..]), &handoff_to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_ids_remain_decimal_and_have_entropy() {
        let first = state_id();
        let second = state_id();
        assert!(first.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(second.bytes().all(|byte| byte.is_ascii_digit()));
        assert_ne!(first, second);
    }

    #[test]
    fn saved_routine_ids_are_valid_uuid_v4() {
        let first = new_routine_id();
        let second = new_routine_id();
        assert!(valid_routine_id(&first));
        assert!(valid_routine_id(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn saved_routine_summary_reports_legacy_mode() {
        let graph = json!({"name":"legacy","description":"Old","entry":"done",
            "nodes":{"done":{"terminal":"success"}}});
        assert_eq!(routine_summary("legacy", &graph)["mode"], "auto");
        assert!(routine_summary("legacy", &graph)["id"].is_null());
    }

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
            validate_routine_definition(&graph, Some(name)).unwrap();
        }
    }
}
