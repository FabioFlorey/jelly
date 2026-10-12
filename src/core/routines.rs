//! Pure routine parsing, graph validation, rendering and next-node planning.

use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug)]
pub(crate) struct RoutineFailure {
    pub(crate) kind: String,
    pub(crate) message: String,
    pub(crate) retryable: bool,
    pub(crate) details: Option<Value>,
}

impl RoutineFailure {
    pub(crate) fn new(
        kind: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            retryable,
            details: None,
        }
    }
}

pub(crate) fn vars(args: &[String]) -> HashMap<String, String> {
    args.iter()
        .filter_map(|x| x.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

pub(crate) fn system_failure_kind(name: &str) -> &'static str {
    match name {
        "open-browser" | "close-browser" | "browser-task" => "browser_unavailable",
        "profile-import" => "interaction_failed",
        "screenshot" | "record-browser" | "verify-artifact" => "artifact_failed",
        "download" | "downloads" | "wait-download" => "download_failed",
        "hitl" => "delivery_failed",
        _ => "internal",
    }
}

pub(crate) fn context_from_vars(vars: HashMap<String, String>) -> Map<String, Value> {
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

pub(crate) fn render_text(
    input: &str,
    context: &Map<String, Value>,
) -> Result<String, RoutineFailure> {
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

pub(crate) fn render_args(
    node: &Value,
    context: &Map<String, Value>,
) -> Result<Vec<String>, RoutineFailure> {
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

pub(crate) fn parse_output(output: &str) -> Value {
    serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_owned()))
}

pub(crate) fn error_value(error: &RoutineFailure) -> Value {
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

pub(crate) fn error_transition(node: &Value, kind: &str) -> Option<String> {
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

pub(crate) fn guard_matches(
    guard: &Value,
    context: &Map<String, Value>,
) -> Result<bool, RoutineFailure> {
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

pub(crate) fn valid_routine_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
        && name
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-' || ch == b'_')
        && !matches!(name, "save" | "list" | "describe" | "resume")
}

/// Saved definitions without a mode remain deterministic legacy graphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoutineMode {
    Auto,
    Hybrid,
    Jelly,
}

pub(crate) fn routine_mode(graph: &Value) -> Result<RoutineMode, &'static str> {
    match graph.get("mode") {
        None => Ok(RoutineMode::Auto),
        Some(Value::String(value)) if value == "auto" => Ok(RoutineMode::Auto),
        Some(Value::String(value)) if value == "hybrid" => Ok(RoutineMode::Hybrid),
        Some(Value::String(value)) if value == "jelly" => Ok(RoutineMode::Jelly),
        _ => Err("routine mode must be auto, hybrid or jelly"),
    }
}

pub(crate) fn valid_routine_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(i, byte)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        })
}

/// Validate a persistent, named JSON graph before saving or executing it.
pub(crate) fn validate_routine_definition(
    graph: &Value,
    expected_name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = graph
        .get("name")
        .and_then(Value::as_str)
        .ok_or("routine requires name")?;
    if !valid_routine_name(name) {
        return Err("routine name must start with a lowercase letter and contain only a-z, 0-9, '-' or '_' (max 64 characters)".into());
    }
    if expected_name.is_some_and(|expected| expected != name) {
        return Err("routine name does not match requested filename".into());
    }
    let description = graph
        .get("description")
        .and_then(Value::as_str)
        .ok_or("routine requires description")?;
    if description.trim().is_empty() || description.len() > 400 {
        return Err("routine description must contain 1-400 characters".into());
    }
    if let Some(id) = graph.get("id")
        && !id.as_str().is_some_and(valid_routine_id)
    {
        return Err("routine id must be a lowercase UUID v4".into());
    }
    validate_graph(graph)
}

pub(crate) fn validate_graph(graph: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let mode = routine_mode(graph)?;
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
        if let Some(instruction) = node.get("agent") {
            if mode == RoutineMode::Auto {
                return Err(format!("node {name} requires hybrid or jelly mode").into());
            }
            if !instruction
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && text.len() <= 2000)
            {
                return Err(
                    format!("node {name} agent must be nonempty text (max 2000 bytes)").into(),
                );
            }
            if ["tool", "hitl", "terminal", "guard", "goto", "next"]
                .iter()
                .any(|key| node.get(*key).is_some())
            {
                return Err(format!("node {name} agent cannot combine other actions").into());
            }
            let verify = node
                .get("resume")
                .and_then(Value::as_str)
                .and_then(|target| nodes.get(target))
                // plan_node prioritizes these actions over tool execution.
                // A mixed node could otherwise bypass verification entirely.
                .filter(|target| {
                    ["terminal", "goto", "guard", "hitl", "agent"]
                        .iter()
                        .all(|key| target.get(*key).is_none())
                })
                .and_then(|target| target.get("tool"))
                .and_then(Value::as_str);
            if !verify.is_some_and(|tool| {
                matches!(
                    tool,
                    "assert-visible"
                        | "assert-text"
                        | "assert-title"
                        | "assert-url"
                        | "assert-image-ready"
                        | "wait-for"
                        | "verify-artifact"
                        | "wait-download"
                )
            }) {
                return Err(
                    format!("node {name} agent resume must target a verification tool").into(),
                );
            }
        }
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

/// A single graph-node decision; no browser, clock, persistence, or child processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlannedNode {
    Terminal { success: bool, message: String },
    Jump(String),
    Hitl { message: String },
    Agent { instruction: String },
    Tool { name: String, args: Vec<String> },
}

pub(crate) fn plan_node(
    current: &str,
    node: &Value,
    context: &Map<String, Value>,
) -> Result<PlannedNode, String> {
    // Keep the original precedence: terminal > goto > guard > hitl > tool.
    if let Some(outcome) = node.get("terminal").and_then(Value::as_str) {
        let message = node
            .get("message")
            .and_then(Value::as_str)
            .map(|message| render_text(message, context))
            .transpose()
            .map_err(|error| format!("{}: {}", error.kind, error.message))?
            .unwrap_or_else(|| outcome.to_owned());
        return Ok(PlannedNode::Terminal {
            success: outcome == "success",
            message,
        });
    }
    if let Some(next) = node.get("goto").and_then(Value::as_str) {
        return Ok(PlannedNode::Jump(next.to_owned()));
    }
    if let Some(guard) = node.get("guard") {
        let matched = guard_matches(guard, context)
            .map_err(|error| format!("{}: {}", error.kind, error.message))?;
        let next = node
            .get(if matched { "then" } else { "else" })
            .and_then(Value::as_str)
            .ok_or("guard node requires then and else")?;
        return Ok(PlannedNode::Jump(next.to_owned()));
    }
    if let Some(message) = node.get("hitl").and_then(Value::as_str) {
        let message = render_text(message, context)
            .map_err(|error| format!("{}: {}", error.kind, error.message))?;
        // Intentionally do not resolve the resume/next edge here: the original
        // contract evaluates it only *after* the HITL delivery succeeds.
        return Ok(PlannedNode::Hitl { message });
    }
    if let Some(instruction) = node.get("agent").and_then(Value::as_str) {
        let instruction = render_text(instruction, context)
            .map_err(|error| format!("{}: {}", error.kind, error.message))?;
        return Ok(PlannedNode::Agent { instruction });
    }
    let name = node
        .get("tool")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("routine node {current} has no tool/guard/hitl/terminal/goto"))?;
    let args =
        render_args(node, context).map_err(|error| format!("{}: {}", error.kind, error.message))?;
    Ok(PlannedNode::Tool {
        name: name.to_owned(),
        args,
    })
}

/// Interpret an observed tool result without performing the tool's effects.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlannedToolOutcome {
    Success {
        data: Value,
        save_as: Option<String>,
        next: Option<String>,
        browser_owned: Option<bool>,
    },
    Failure {
        error: Value,
        recovery: Option<String>,
        message: String,
    },
}

pub(crate) fn plan_tool_outcome(
    node: &Value,
    tool_name: &str,
    outcome: Result<&str, &RoutineFailure>,
) -> PlannedToolOutcome {
    match outcome {
        Ok(output) => PlannedToolOutcome::Success {
            data: parse_output(output),
            save_as: node.get("save").and_then(Value::as_str).map(str::to_owned),
            next: node.get("next").and_then(Value::as_str).map(str::to_owned),
            browser_owned: match tool_name {
                "open-browser" => Some(true),
                "close-browser" => Some(false),
                _ => None,
            },
        },
        Err(failure) => PlannedToolOutcome::Failure {
            error: error_value(failure),
            recovery: error_transition(node, &failure.kind),
            message: format!("{}: {}", failure.kind, failure.message),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> Map<String, Value> {
        json!({
            "artifact":{"id":"artifact-12","ready":true},
            "_error":{"kind":"target_stale"},
            "name":"Alice",
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn modes_preserve_legacy_graphs_and_reject_unhandled_agent_steps() {
        let graph = json!({
            "name":"legacy",
            "description":"An existing deterministic routine",
            "entry":"done",
            "nodes":{"done":{"terminal":"success"}}
        });
        assert_eq!(routine_mode(&graph).unwrap(), RoutineMode::Auto);
        validate_routine_definition(&graph, None).unwrap();
        let mut invalid = graph.clone();
        invalid["mode"] = json!("unknown");
        assert!(validate_routine_definition(&invalid, None).is_err());
        invalid["mode"] = json!("auto");
        invalid["nodes"]["done"] = json!({"agent":"Click button","resume":"verify"});
        assert!(validate_routine_definition(&invalid, None).is_err());
    }

    #[test]
    fn hybrid_and_jelly_agent_handoffs_require_real_verification() {
        let graph = json!({
            "mode":"hybrid", "name":"recover-click",
            "description":"Recover a missing selector and verify the outcome",
            "entry":"click",
            "nodes":{
                "click":{"tool":"click","args":["text:Download"],
                         "next":"done","on_error":{"target_not_found":"repair"}},
                "repair":{"agent":"Find and click the new Download button for {{name}}",
                          "resume":"verify"},
                "verify":{"tool":"assert-visible","args":["text:Downloaded"],"next":"done"},
                "done":{"terminal":"success"}
            }
        });
        validate_routine_definition(&graph, None).unwrap();
        assert_eq!(
            plan_node("repair", &graph["nodes"]["repair"], &context()).unwrap(),
            PlannedNode::Agent {
                instruction: "Find and click the new Download button for Alice".into()
            }
        );
        let mut jelly = graph.clone();
        jelly["mode"] = json!("jelly");
        jelly["entry"] = json!("repair");
        validate_routine_definition(&jelly, None).unwrap();
        jelly["nodes"]["verify"]["tool"] = json!("click");
        assert!(validate_routine_definition(&jelly, None).is_err());
        jelly["nodes"]["verify"]["tool"] = json!("assert-visible");
        jelly["nodes"]["verify"]["terminal"] = json!("success");
        assert!(validate_routine_definition(&jelly, None).is_err());
        jelly["nodes"]["verify"]
            .as_object_mut()
            .unwrap()
            .remove("terminal");
        jelly["nodes"]["repair"]["resume"] = json!("done");
        assert!(validate_routine_definition(&jelly, None).is_err());
        jelly["nodes"]["repair"]["resume"] = json!("verify");
        jelly["nodes"]["repair"]["agent"] = json!("");
        assert!(validate_routine_definition(&jelly, None).is_err());
    }

    #[test]
    fn routine_ids_are_strict_uuid_v4_and_optional_on_legacy_graphs() {
        let graph = json!({"name":"check","description":"Check","entry":"done",
            "nodes":{"done":{"terminal":"success"}}});
        validate_routine_definition(&graph, None).unwrap();
        let mut identified = graph.clone();
        identified["id"] = json!("72dddbd7-f9a0-4d7f-93cc-d927bd35ec09");
        validate_routine_definition(&identified, None).unwrap();
        identified["id"] = json!("72dddbd7-f9a0-3d7f-93cc-d927bd35ec09");
        assert!(validate_routine_definition(&identified, None).is_err());
        assert!(valid_routine_name("search"));
    }

    #[test]
    fn terminal_precedes_other_fields_and_uses_rendered_message() {
        let action = plan_node(
            "finish",
            &json!({
                "terminal":"success", "message":"Saved {{artifact.id}}",
                "goto":"bad", "tool":"never", "hitl":"never"
            }),
            &context(),
        )
        .unwrap();
        assert_eq!(
            action,
            PlannedNode::Terminal {
                success: true,
                message: "Saved artifact-12".into(),
            }
        );
        assert_eq!(
            plan_node("end", &json!({"terminal":"failure"}), &context()).unwrap(),
            PlannedNode::Terminal {
                success: false,
                message: "failure".into()
            }
        );
        assert_eq!(
            plan_node("end", &json!({"terminal":"pending"}), &context()).unwrap(),
            PlannedNode::Terminal {
                success: false,
                message: "pending".into()
            }
        );
    }

    #[test]
    fn goto_takes_priority_over_guards_and_actions() {
        let action = plan_node(
            "start",
            &json!({
                "goto":"final", "guard":{}, "hitl":"never", "tool":"never"
            }),
            &context(),
        )
        .unwrap();
        assert_eq!(action, PlannedNode::Jump("final".into()));
    }

    #[test]
    fn guard_matches_nested_evidence_and_routes_both_branches() {
        let node = json!({
            "guard":{"path":"artifact.ready", "op":"true"},
            "then":"ready", "else":"wait", "tool":"never"
        });
        assert_eq!(
            plan_node("decide", &node, &context()).unwrap(),
            PlannedNode::Jump("ready".into())
        );
        let mut missing = context();
        missing.remove("artifact");
        assert_eq!(
            plan_node("decide", &node, &missing).unwrap(),
            PlannedNode::Jump("wait".into())
        );
    }

    #[test]
    fn typed_error_guard_and_wildcard_error_edges_are_preserved() {
        let node = json!({"guard":{"op":"error_kind","value":"target_stale"},
            "then":"recover", "else":"bail"});
        assert_eq!(
            plan_node("decide", &node, &context()).unwrap(),
            PlannedNode::Jump("recover".into())
        );
        assert_eq!(
            error_transition(
                &json!({"on_error":{"target_stale":"retry","*":"default"}}),
                "target_stale"
            ),
            Some("retry".into())
        );
        assert_eq!(
            error_transition(
                &json!({"on_error":{"target_stale":"retry","*":"default"}}),
                "internal"
            ),
            Some("default".into())
        );
        assert_eq!(
            error_transition(&json!({"on_error":"stop"}), "internal"),
            Some("stop".into())
        );
        assert_eq!(error_transition(&json!({"on_error":{}}), "internal"), None);
    }

    #[test]
    fn hitl_is_rendered_but_resume_edge_is_not_prechecked() {
        // The legacy execution only examines the resume edge after a successful delivery.
        let node = json!({"hitl":"Review {{name}}"});
        assert_eq!(
            plan_node("review", &node, &context()).unwrap(),
            PlannedNode::Hitl {
                message: "Review Alice".into()
            }
        );
    }

    #[test]
    fn tool_arguments_use_context_and_preserve_argument_types() {
        let node = json!({"tool":"fill", "args":["{{name}}", "{{artifact.id}}", "literal"]});
        assert_eq!(
            plan_node("fill", &node, &context()).unwrap(),
            PlannedNode::Tool {
                name: "fill".into(),
                args: vec!["Alice".into(), "artifact-12".into(), "literal".into()]
            }
        );
        assert_eq!(
            plan_node("run", &json!({"tool":"wait"}), &context()).unwrap(),
            PlannedNode::Tool {
                name: "wait".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn planner_reports_same_missing_fields_and_template_errors() {
        assert_eq!(
            plan_node("broken", &json!({}), &context()).unwrap_err(),
            "routine node broken has no tool/guard/hitl/terminal/goto"
        );
        assert_eq!(
            plan_node(
                "g",
                &json!({"guard":{"path":"name"}, "then":"ok"}),
                &context()
            )
            .unwrap_err(),
            "guard node requires then and else"
        );
        assert_eq!(
            plan_node(
                "t",
                &json!({"tool":"fill","args":["{{missing}}"]}),
                &context()
            )
            .unwrap_err(),
            "invalid_arguments: routine context value not found: missing"
        );
        assert_eq!(
            plan_node(
                "t",
                &json!({"terminal":"success", "message":"{{missing}}"}),
                &context()
            )
            .unwrap_err(),
            "invalid_arguments: routine context value not found: missing"
        );
    }

    #[test]
    fn validation_preserves_error_edges_and_cycle_semantics() {
        assert!(
            validate_graph(&json!({"entry":"start","nodes":{
                "start":{"tool":"ping","on_error":{"*":"start"},"next":"done"},
                "done":{"terminal":"success"}
            }}))
            .is_ok()
        );
        assert_eq!(
            validate_graph(&json!({"entry":"start","nodes":{
                "start":{"tool":"ping","on_error":{"invalid_arguments":"notfound"}}
            }}))
            .unwrap_err()
            .to_string(),
            "node start on_error points to missing node notfound"
        );
    }

    #[test]
    fn successful_tool_outcome_keeps_output_and_next_edge() {
        let node = json!({"save":"snapshot", "next":"verify"});
        assert_eq!(
            plan_tool_outcome(&node, "screenshot", Ok("{\"ok\":true}")),
            PlannedToolOutcome::Success {
                data: json!({"ok":true}),
                save_as: Some("snapshot".into()),
                next: Some("verify".into()),
                browser_owned: None,
            }
        );
        assert_eq!(
            plan_tool_outcome(&json!({}), "open-browser", Ok("plain text")),
            PlannedToolOutcome::Success {
                data: json!("plain text"),
                save_as: None,
                next: None,
                browser_owned: Some(true),
            }
        );
        assert!(matches!(
            plan_tool_outcome(&json!({}), "close-browser", Ok("")),
            PlannedToolOutcome::Success {
                browser_owned: Some(false),
                ..
            }
        ));
    }

    #[test]
    fn failed_tool_outcome_preserves_typed_error_and_recovery() {
        let mut failure = RoutineFailure::new("target_stale", "lost ref", true);
        failure.details = Some(json!({"target":"e12"}));
        let node = json!({"on_error":{"target_stale":"refresh", "*":"abort"}});
        assert_eq!(
            plan_tool_outcome(&node, "click", Err(&failure)),
            PlannedToolOutcome::Failure {
                error: json!({"kind":"target_stale","message":"lost ref",
                    "retryable":true,"details":{"target":"e12"}}),
                recovery: Some("refresh".into()),
                message: "target_stale: lost ref".into(),
            }
        );
        assert!(matches!(
            plan_tool_outcome(&json!({}), "click", Err(&failure)),
            PlannedToolOutcome::Failure { recovery: None, .. }
        ));
    }

    #[test]
    fn parse_output_preserves_original_values() {
        assert_eq!(parse_output("{\"ok\":true}"), json!({"ok":true}));
        assert_eq!(parse_output("not-json"), json!("not-json"));
    }

    #[test]
    fn graph_definitions_require_safe_names_and_descriptions() {
        let graph = json!({
            "name":"verified-task",
            "description":"Verify the task outcome before returning.",
            "entry":"done",
            "nodes":{"done":{"terminal":"success"}}
        });
        validate_routine_definition(&graph, Some("verified-task")).unwrap();
        assert!(validate_routine_definition(&graph, Some("wrong-name")).is_err());
        assert!(
            validate_routine_definition(
                &json!({
                    "name":"../escape", "description":"Unsafe", "entry":"done",
                    "nodes":{"done":{"terminal":"success"}}
                }),
                None
            )
            .is_err()
        );
        assert!(
            validate_routine_definition(
                &json!({
                    "name":"missing-description", "entry":"done",
                    "nodes":{"done":{"terminal":"success"}}
                }),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn routine_chaining_interpolates_variables_for_child_graph() {
        let node = json!({
            "tool":"call-routine",
            "args":["verified-screenshot","url={{ url }}","target={{ target }}"]
        });
        let context = json!({"url":"https://example.com/", "target":"css:main"})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            plan_node("call", &node, &context).unwrap(),
            PlannedNode::Tool {
                name: "call-routine".into(),
                args: vec![
                    "verified-screenshot".into(),
                    "url=https://example.com/".into(),
                    "target=css:main".into()
                ]
            }
        );
    }
}
