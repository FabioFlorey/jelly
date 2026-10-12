use super::system_tools::execute_mcp_system_tool;
use crate::shell::session::{persistent_mcp_session_enabled, with_mcp_browser_session};
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

fn with_remote_browser_selector(mut input_schema: Value, remote: bool) -> Value {
    if !remote {
        return input_schema;
    }
    let id_field = json!({"type":"string", "minLength":1, "description":"Remote Chromium UUID or configured Jelly-* easy name; selects the browser, not a page target"});
    if let Some(properties) = input_schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        properties.insert("browser_id".into(), id_field.clone());
    }
    if let Some(branches) = input_schema.get_mut("oneOf").and_then(Value::as_array_mut) {
        for branch in branches {
            if let Some(properties) = branch.get_mut("properties").and_then(Value::as_object_mut) {
                properties.insert("browser_id".into(), id_field.clone());
            }
        }
    }
    input_schema
}

#[cfg(test)]
mod browser_selector_schema_tests {
    use super::*;
    #[test]
    fn optional_browser_id_is_injected_into_root_and_one_of_branches() {
        let object = json!({"type":"object","properties":{"action":{"type":"string"}},"additionalProperties":false});
        assert!(
            with_remote_browser_selector(object.clone(), false)["properties"]
                .get("browser_id")
                .is_none()
        );
        assert_eq!(
            with_remote_browser_selector(object, true)["properties"]["browser_id"]["minLength"],
            1
        );
        let variants = json!({"oneOf":[{"type":"object","properties":{"action":{"const":"search"}},"additionalProperties":false},{"type":"object","properties":{"action":{"const":"schema"}},"additionalProperties":false}]});
        let result = with_remote_browser_selector(variants, true);
        for branch in result["oneOf"].as_array().unwrap() {
            assert_eq!(branch["properties"]["browser_id"]["type"], "string");
        }
    }
}

pub(super) fn mcp_tools_from_catalog(catalog: &AgentToolCatalog) -> Vec<Value> {
    catalog
        .iter()
        .map(|tool| {
            let input_schema = with_remote_browser_selector(
                tool.input_schema().clone(),
                crate::config::config().browser.backend
                    == crate::browser_backend::BrowserBackendKind::RemoteCdp,
            );
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "inputSchema": input_schema,
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
        .map_err(ToolFailure::from_error)
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
    let browser_id = match object.get("browser_id") {
        Some(Value::String(id)) => Some(id.as_str()),
        Some(_) => {
            return Err(ToolFailure::new(
                ErrorKind::InvalidArguments,
                "browser_id must be a string",
                false,
            ));
        }
        None => None,
    };
    if let Some(id) = browser_id {
        let configured = &crate::config::config().browser;
        if configured.backend != crate::browser_backend::BrowserBackendKind::RemoteCdp
            || crate::config::resolve_remote_id(id).is_err()
        {
            return Err(ToolFailure::new(
                ErrorKind::InvalidArguments,
                "unknown or invalid remote browser_id",
                false,
            ));
        }
    }
    let mut clean_arguments = arguments.clone();
    if let Some(fields) = clean_arguments.as_object_mut() {
        fields.remove("browser_id");
    }
    crate::config::with_browser_id(browser_id, || {
        execute_tool_routed(catalog, name, &clean_arguments)
    })
}

fn execute_tool_routed(
    catalog: &AgentToolCatalog,
    name: &str,
    arguments: &Value,
) -> Result<String, ToolFailure> {
    let object = arguments.as_object().expect("validated object");
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
            builtin
                .preflight(arguments)
                .map_err(ToolFailure::from_error)?;
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
                    .map_err(ToolFailure::from_error),
                AgentBuiltinExecution::Browser => {
                    with_mcp_browser_session(|browser| builtin.execute_browser(browser, arguments))
                        .map_err(ToolFailure::from_error)
                }
            }
        }
    }
}
