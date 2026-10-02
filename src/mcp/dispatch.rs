use super::{
    browser_session::{
        map_browser_failure, persistent_mcp_session_enabled, with_mcp_browser_session,
    },
    system_tools::execute_mcp_system_tool,
};
use crate::{
    AgentBuiltinExecution, AgentToolBinding, AgentToolCatalog, ErrorKind, active_agent_catalog,
    agent_catalog_from_config, classify_error, error_details, execute_named_browser_primitive,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use std::fs;

/// Return the validated active MCP `tools/list` projection.
pub fn mcp_tools() -> Result<Vec<Value>, String> {
    Ok(mcp_tools_from_catalog(active_agent_catalog()?))
}

pub fn mcp_tools_for_config(
    surface: Option<&str>,
    raw_cdp: Option<&str>,
) -> Result<Vec<Value>, String> {
    Ok(mcp_tools_from_catalog(agent_catalog_from_config(
        surface, raw_cdp,
    )?))
}

pub(super) fn mcp_tools_from_catalog(catalog: &AgentToolCatalog) -> Vec<Value> {
    catalog
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "inputSchema": tool.input_schema(),
                "outputSchema": tool.output_schema(),
                "securitySchemes": [{"type":"oauth2","scopes":["jelly"]}],
                "_meta": {
                    "securitySchemes": [{"type":"oauth2","scopes":["jelly"]}]
                },
            })
        })
        .collect()
}

#[derive(Debug)]
pub(super) struct ToolFailure {
    pub(super) kind: ErrorKind,
    pub(super) message: String,
    pub(super) retryable: bool,
    pub(super) details: Option<Value>,
}

impl ToolFailure {
    pub(super) fn new(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
            details: None,
        }
    }

    pub(super) fn from_error(error: crate::Error) -> Self {
        let (kind, retryable) = classify_error(error.as_ref());
        Self {
            kind,
            message: error.to_string(),
            retryable,
            details: error_details(error.as_ref()),
        }
    }
}

fn output_value(output: &str) -> Value {
    serde_json::from_str::<Value>(output).unwrap_or_else(|_| Value::String(output.to_owned()))
}

pub(super) fn success_envelope(name: &str, output: &str) -> Value {
    json!({
        "ok": true,
        "data": output_value(output),
        "error": Value::Null,
        "meta": {"tool": name}
    })
}

pub(super) fn failure_envelope(name: &str, failure: &ToolFailure) -> Value {
    let mut error = Map::from_iter([
        (
            "kind".to_owned(),
            Value::String(failure.kind.as_str().to_owned()),
        ),
        ("message".to_owned(), Value::String(failure.message.clone())),
        ("retryable".to_owned(), Value::Bool(failure.retryable)),
    ]);
    if let Some(details) = &failure.details {
        error.insert("details".to_owned(), details.clone());
    }

    json!({
        "ok": false,
        "data": Value::Null,
        "error": Value::Object(error),
        "meta": {"tool": name}
    })
}

pub(super) async fn call_tool(params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "tools/call requires params.name".to_owned()))?
        .to_owned();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let worker_name = name.clone();

    let result = tokio::task::spawn_blocking(move || execute_tool(&worker_name, &arguments))
        .await
        .map_err(|e| (-32603, format!("tool worker failed: {e}")))?;

    let (envelope, is_error) = match result {
        Ok(output) => (success_envelope(&name, &output), false),
        Err(error) => (failure_envelope(&name, &error), true),
    };
    let text = serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| envelope.to_string());
    let mut content = vec![json!({"type":"text","text":text})];
    if !is_error
        && name == "screenshot"
        && let Some(path) = envelope["data"]["path"].as_str()
        && let Ok(bytes) = fs::read(path)
    {
        content.push(json!({
            "type": "image",
            "data": STANDARD.encode(bytes),
            "mimeType": "image/png"
        }));
    }
    Ok(json!({
        "content":content,
        "structuredContent":envelope,
        "isError":is_error
    }))
}

fn execute_mcp_browser_primitive(name: &str, arguments: &Value) -> Result<String, ToolFailure> {
    with_mcp_browser_session(|browser| execute_named_browser_primitive(browser, name, arguments))
}

fn execute_tool(name: &str, arguments: &Value) -> Result<String, ToolFailure> {
    let catalog = active_agent_catalog().map_err(|message| {
        ToolFailure::new(
            ErrorKind::Internal,
            format!("invalid MCP tool surface configuration: {message}"),
            false,
        )
    })?;
    execute_tool_from_catalog(catalog, name, arguments)
}

pub(super) fn builtin_requires_persistent_mcp_session(builtin: crate::AgentBuiltin) -> bool {
    matches!(builtin, crate::AgentBuiltin::BrowserEvents)
}

pub(super) fn execute_tool_from_catalog(
    catalog: &AgentToolCatalog,
    name: &str,
    arguments: &Value,
) -> Result<String, ToolFailure> {
    let object = arguments.as_object().ok_or_else(|| {
        ToolFailure::new(
            ErrorKind::InvalidArguments,
            "tool arguments must be a JSON object",
            false,
        )
    })?;
    let tool = catalog.get(name).ok_or_else(|| {
        ToolFailure::new(
            ErrorKind::Unsupported,
            format!("unknown agent tool: {name}"),
            false,
        )
    })?;

    match tool.binding() {
        AgentToolBinding::BrowserPrimitive(spec) => {
            execute_mcp_browser_primitive(spec.name, arguments)
        }
        AgentToolBinding::SystemTool(spec) => execute_mcp_system_tool(spec.name, object),
        AgentToolBinding::Builtin(builtin) => {
            builtin.preflight(arguments).map_err(map_browser_failure)?;
            if builtin_requires_persistent_mcp_session(builtin) && !persistent_mcp_session_enabled()
            {
                return Err(ToolFailure::new(
                    ErrorKind::Unsupported,
                    "browser-events requires config/jelly.toml [mcp].persistent_session = true",
                    false,
                ));
            }
            match builtin.execution() {
                AgentBuiltinExecution::Stateless => builtin
                    .execute_stateless(arguments)
                    .map_err(map_browser_failure),
                AgentBuiltinExecution::Browser => {
                    with_mcp_browser_session(|browser| builtin.execute_browser(browser, arguments))
                }
            }
        }
    }
}
