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

pub(crate) fn render_legacy(mut s: String, vars: &HashMap<String, String>) -> String {
    for (k, v) in vars {
        s = s
            .replace(&format!("{{{{ {k} }}}}"), v)
            .replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

pub(crate) fn split(line: &str) -> Vec<String> {
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

pub(crate) fn validate_graph(graph: &Value) -> Result<(), Box<dyn std::error::Error>> {
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

/// A single graph-node decision; no browser, clock, persistence, or child processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlannedNode {
    Terminal { success: bool, message: String },
    Jump(String),
    Hitl { message: String },
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
    fn parse_and_template_helpers_preserve_original_values() {
        assert_eq!(parse_output("{\"ok\":true}"), json!({"ok":true}));
        assert_eq!(parse_output("not-json"), json!("not-json"));
        assert_eq!(
            render_legacy(
                "{{ hello }}".into(),
                &HashMap::from([("hello".into(), "there".into())])
            ),
            "there"
        );
        assert_eq!(
            split("fill 'hello world' text"),
            vec!["fill", "hello world", "text"]
        );
    }
}
