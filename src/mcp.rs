use crate::{
    AgentBuiltinExecution, AgentToolBinding, AgentToolCatalog, BrowserSession, ErrorKind,
    active_agent_catalog, classify_error, error_details, execute_named_browser_primitive,
    mcp_auth::{AuthState, ConsentMode},
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use std::{
    env, fs,
    process::Command,
    sync::{Mutex, OnceLock},
};

static MCP_BROWSER_SESSION: OnceLock<Mutex<Option<BrowserSession>>> = OnceLock::new();

const SERVER_NAME: &str = "jelly";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_INSTRUCTIONS: &str = include_str!("../.agent/instructions/mcp.md");

pub fn router(
    token: String,
    oauth_password: String,
    bootstrap_secret: String,
    consent_mode: String,
    public_chatgpt_dcr: bool,
    public_url: String,
) -> Result<Router, String> {
    active_agent_catalog()
        .map_err(|error| format!("invalid MCP tool surface configuration: {error}"))?;
    let consent_mode = ConsentMode::parse(&consent_mode)?;
    let state = AuthState::new(
        token,
        oauth_password,
        bootstrap_secret,
        consent_mode,
        public_chatgpt_dcr,
        public_url,
    )?;
    Ok(Router::new()
        .route("/health", get(health))
        .route("/mcp", post(handle_mcp))
        .merge(crate::mcp_auth::routes())
        .with_state(state))
}

async fn health() -> Json<Value> {
    Json(json!({
        "name": SERVER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "status": "ok"
    }))
}

async fn handle_mcp(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !state.authorized(&headers) {
        return state.unauthorized();
    }

    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    if id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let id = id.unwrap_or(Value::Null);

    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => mcp_tools()
            .map(|tools| json!({"tools":tools}))
            .map_err(|message| (-32603, message)),
        "tools/call" => call_tool(&params).await,
        _ => Err((-32601, format!("method not found: {method}"))),
    };

    match result {
        Ok(result) => Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response(),
        Err((code, message)) => Json(json!({
            "jsonrpc":"2.0",
            "id":id,
            "error":{"code":code,"message":message}
        }))
        .into_response(),
    }
}

fn initialize(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {"listChanged": false}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": SERVER_INSTRUCTIONS
    })
}

pub fn mcp_tools() -> Result<Vec<Value>, String> {
    Ok(mcp_tools_from_catalog(active_agent_catalog()?))
}

fn mcp_tools_from_catalog(catalog: &AgentToolCatalog) -> Vec<Value> {
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
struct ToolFailure {
    kind: ErrorKind,
    message: String,
    retryable: bool,
    details: Option<Value>,
}

impl ToolFailure {
    fn new(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
            details: None,
        }
    }

    fn from_error(error: crate::Error) -> Self {
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

fn success_envelope(name: &str, output: &str) -> Value {
    json!({
        "ok": true,
        "data": output_value(output),
        "error": Value::Null,
        "meta": {"tool": name}
    })
}

fn failure_envelope(name: &str, failure: &ToolFailure) -> Value {
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

async fn call_tool(params: &Value) -> Result<Value, (i64, String)> {
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

fn persistent_mcp_session_enabled() -> bool {
    !env::var("JELLY_MCP_PERSISTENT_SESSION")
        .ok()
        .is_some_and(|value| matches!(value.trim(), "0" | "false" | "off"))
}

fn reset_mcp_browser_session() {
    if let Some(sessions) = MCP_BROWSER_SESSION.get()
        && let Ok(mut guard) = sessions.lock()
    {
        *guard = None;
    }
}

fn map_browser_failure(error: crate::Error) -> ToolFailure {
    ToolFailure::from_error(error)
}

fn with_mcp_browser_session<F>(operation: F) -> Result<String, ToolFailure>
where
    F: FnOnce(&mut BrowserSession) -> Result<String, crate::Error>,
{
    if !persistent_mcp_session_enabled() {
        let mut browser = BrowserSession::connect().map_err(map_browser_failure)?;
        browser.sync_active_target().map_err(map_browser_failure)?;
        return operation(&mut browser).map_err(map_browser_failure);
    }

    let sessions = MCP_BROWSER_SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = sessions.lock().map_err(|_| {
        ToolFailure::new(
            ErrorKind::Internal,
            "persistent browser session lock is poisoned",
            true,
        )
    })?;
    if guard.is_none() {
        *guard = Some(BrowserSession::connect().map_err(map_browser_failure)?);
    }

    let browser = guard.as_mut().expect("session initialized");
    if let Err(error) = browser.sync_active_target() {
        let failure = map_browser_failure(error);
        if failure.kind == ErrorKind::BrowserUnavailable {
            *guard = None;
        }
        return Err(failure);
    }

    let result = operation(browser).map_err(map_browser_failure);
    if result
        .as_ref()
        .err()
        .is_some_and(|failure| failure.kind == ErrorKind::BrowserUnavailable)
    {
        *guard = None;
    }
    result
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

fn builtin_requires_persistent_mcp_session(builtin: crate::AgentBuiltin) -> bool {
    matches!(builtin, crate::AgentBuiltin::BrowserEvents)
}

fn execute_tool_from_catalog(
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
                    "browser-events requires persistent MCP browser sessions; JELLY_MCP_PERSISTENT_SESSION=0 is incompatible with runtime-scoped subscriptions",
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

fn execute_mcp_system_tool(name: &str, object: &Map<String, Value>) -> Result<String, ToolFailure> {
    let args = system_cli_args(name, object)
        .map_err(|message| ToolFailure::new(ErrorKind::InvalidArguments, message, false))?;
    if matches!(
        name,
        "open-browser" | "close-browser" | "browser-task" | "profile-import"
    ) {
        reset_mcp_browser_session();
    }
    run_system_tool(name, &args).map_err(|message| {
        let kind = if name == "wait-download" && message.contains("timed out") {
            ErrorKind::ConditionTimeout
        } else {
            match name {
                "open-browser" | "close-browser" | "browser-task" => ErrorKind::BrowserUnavailable,
                "profile-import" => ErrorKind::InteractionFailed,
                "screenshot" | "record-browser" | "verify-artifact" => ErrorKind::ArtifactFailed,
                "downloads" | "wait-download" => ErrorKind::DownloadFailed,
                "hitl" => ErrorKind::DeliveryFailed,
                _ => ErrorKind::Internal,
            }
        };
        ToolFailure::new(
            kind,
            message,
            matches!(
                kind,
                ErrorKind::BrowserUnavailable
                    | ErrorKind::DeliveryFailed
                    | ErrorKind::ConditionTimeout
            ),
        )
    })
}

fn system_cli_args(name: &str, object: &Map<String, Value>) -> Result<Vec<String>, String> {
    let string = |key: &str| -> Result<Option<String>, String> {
        match object.get(key) {
            Some(Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(format!("{key} must be a string")),
            None => Ok(None),
        }
    };
    let bool_value = |key: &str| -> Result<bool, String> {
        match object.get(key) {
            Some(Value::Bool(value)) => Ok(*value),
            Some(_) => Err(format!("{key} must be a boolean")),
            None => Ok(false),
        }
    };
    let integer = |key: &str| -> Result<Option<u64>, String> {
        match object.get(key) {
            Some(Value::Number(value)) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("{key} must be a non-negative integer")),
            Some(_) => Err(format!("{key} must be an integer")),
            None => Ok(None),
        }
    };
    let strings = |key: &str| -> Result<Vec<String>, String> {
        match object.get(key) {
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("{key} must contain only strings"))
                })
                .collect(),
            Some(_) => Err(format!("{key} must be an array of strings")),
            None => Ok(Vec::new()),
        }
    };

    match name {
        "open-browser" => Ok(string("url")?.into_iter().collect()),
        "close-browser" | "downloads" => Ok(Vec::new()),
        "browser-task" => {
            let url = string("url")?.ok_or("browser-task requires url")?;
            let tool = string("tool")?.ok_or("browser-task requires tool")?;
            let mut args = Vec::new();
            if bool_value("persist")? {
                args.push("--persist".into());
            }
            args.extend([url, tool]);
            args.extend(strings("args")?);
            Ok(args)
        }
        "profile-import" => {
            let source = string("source")?.ok_or("profile-import requires source")?;
            let mut args = vec![source];
            if bool_value("force")? {
                args.push("--force".into());
            }
            Ok(args)
        }
        "screenshot" => {
            let mut args = Vec::new();
            if let Some(target) = string("target")? {
                args.push(target);
            }
            if let Some(output) = string("output")? {
                args.extend(["--output".into(), output]);
            }
            args.push("--json".into());
            Ok(args)
        }
        "record-browser" => {
            let action = string("action")?.ok_or("record-browser requires action")?;
            let mut args = vec![action.clone()];
            if action == "start" {
                if let Some(mode) = string("mode")? {
                    args.extend(["--mode".into(), mode]);
                }
                if let Some(interval_ms) = integer("interval_ms")? {
                    args.extend(["--interval-ms".into(), interval_ms.max(100).to_string()]);
                }
                if let Some(hold_ms) = integer("hold_ms")? {
                    args.extend(["--hold-ms".into(), hold_ms.max(100).to_string()]);
                }
            }
            Ok(args)
        }
        "verify-artifact" => {
            let mut args = vec![string("artifact")?.ok_or("verify-artifact requires artifact")?];
            let checks = strings("semantic_checks")?;
            if !checks.is_empty() {
                args.push("--semantic".into());
                args.extend(checks);
            }
            Ok(args)
        }
        "wait-download" => {
            let after_ms = integer("after_ms")?.ok_or("wait-download requires after_ms")?;
            let mut args = vec![after_ms.to_string()];
            if let Some(seconds) = integer("seconds")? {
                args.push(seconds.to_string());
            }
            if let Some(name) = string("name_contains")? {
                if args.len() == 1 {
                    args.push("30".into());
                }
                args.push(name);
            }
            Ok(args)
        }
        "inspect-network" => {
            let action = string("action")?.ok_or("inspect-network requires action")?;
            let mut args = vec![action];
            args.extend(strings("filters")?);
            Ok(args)
        }
        "call-routine" => {
            let name = string("name")?;
            let resume_id = string("resume_id")?;
            if name.is_some() == resume_id.is_some() {
                return Err("provide exactly one of name or resume_id".into());
            }
            let mut args = if let Some(id) = resume_id {
                vec!["resume".into(), id]
            } else {
                vec![name.unwrap()]
            };
            if let Some(vars) = object.get("vars") {
                let vars = vars.as_object().ok_or("vars must be an object")?;
                for (key, value) in vars {
                    let value = value
                        .as_str()
                        .ok_or("routine variable values must be strings")?;
                    args.push(format!("{key}={value}"));
                }
            }
            Ok(args)
        }
        "hitl" => {
            let mut args = vec![string("message")?.ok_or("hitl requires message")?];
            if let Some(target) = string("screenshot_target")? {
                args.extend(["--screenshot-target".into(), target]);
            }
            if bool_value("no_screenshot")? {
                args.push("--no-screenshot".into());
            }
            Ok(args)
        }
        _ => Err(format!("tool is not MCP-exposed: {name}")),
    }
}

fn run_system_tool(name: &str, args: &[String]) -> Result<String, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let bin_dir = exe
        .parent()
        .ok_or("MCP executable has no parent directory")?;
    let direct = bin_dir.join(format!("agent-{name}"));
    let output = if direct.is_file() {
        Command::new(direct).args(args).output()
    } else {
        Command::new("cargo")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["run", "--quiet", "--bin", &format!("agent-{name}"), "--"])
            .args(args)
            .output()
    }
    .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if stderr.is_empty() {
            format!("{name} exited with {}", output.status)
        } else {
            stderr
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_includes_operating_instructions() {
        let response = initialize(&json!({"protocolVersion": DEFAULT_PROTOCOL_VERSION}));
        let instructions = response["instructions"].as_str().unwrap();
        assert!(instructions.contains("Inspect before mutation"));
        assert!(instructions.contains("Reject nonessential cookies by default"));
        assert!(instructions.contains("active ChatGPT conversation"));
        assert!(instructions.contains("through Telegram"));
        assert!(instructions.contains("exhaust legitimate automatable paths"));
        assert!(instructions.contains(".agent/tools/index.md"));
        assert!(instructions.contains("docs/DISCOVERY.md"));
        assert!(instructions.contains("agent-discover schema <tool>"));
        assert!(instructions.contains("Detect the active surface from `tools/list`"));
        assert!(instructions.contains("Prefer a **semantic Jelly operation**"));
        assert!(instructions.contains("use **browser-schema** to discover it"));
        assert!(instructions.contains("Execute semantic operations through **browser-call**"));
        assert!(instructions.contains("Use **browser-events** only when"));
        assert!(instructions.contains("Use **raw CDP** only when it is published"));
        assert!(instructions.contains("Do not supply Chromium `targetId` or `sessionId` values"));
        assert!(instructions.contains("Use `call-routine`"));
        assert!(instructions.contains("published system artifact tools"));
        assert!(instructions.contains("inputSchema"));
        assert!(instructions.contains("agent-run <tool> [args...]"));
        assert!(instructions.contains("Do not automatically retry side-effecting operations"));
        assert!(!instructions.contains("Use direct primitives for short, local interactions"));
        assert!(!instructions.contains("Use snapshot-interactive before clicking"));
    }

    #[test]
    fn large_surface_agent_catalog_preserves_the_current_mcp_surface() {
        let tools = mcp_tools_from_catalog(crate::large_surface_agent_catalog());
        for spec in crate::primitive_specs {
            assert!(tools.iter().any(|tool| tool["name"] == spec.name));
        }
        for spec in crate::tool_specs {
            assert!(
                tools.iter().any(|tool| tool["name"] == spec.name),
                "system tool missing large-surface MCP mapping: {}",
                spec.name
            );
        }
    }

    #[test]
    fn mcp_schema_generation_accepts_an_intentional_projection() {
        let click = crate::primitive_specs
            .iter()
            .find(|spec| spec.name == "click")
            .unwrap();
        let catalog =
            AgentToolCatalog::try_new(vec![crate::AgentToolSpec::browser_primitive(click)])
                .unwrap();
        let tools = mcp_tools_from_catalog(&catalog);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "click");
        assert_eq!(tools[0]["description"], click.description);
        assert_eq!(tools[0]["inputSchema"]["required"][0], "target");
        assert!(crate::primitive_specs.len() > tools.len());
    }

    #[test]
    fn projected_catalog_is_also_the_execution_allowlist() {
        let read_page = crate::primitive_specs
            .iter()
            .find(|spec| spec.name == "read-page")
            .unwrap();
        let catalog =
            AgentToolCatalog::try_new(vec![crate::AgentToolSpec::browser_primitive(read_page)])
                .unwrap();

        let error = execute_tool_from_catalog(&catalog, "click", &json!({"target":"css:button"}))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert!(error.message.contains("unknown agent tool: click"));
    }

    #[test]
    fn browser_schema_builtin_can_be_published_and_executed_without_browser_state() {
        let catalog = AgentToolCatalog::try_new(vec![crate::AgentToolSpec::builtin(
            crate::AgentBuiltin::BrowserSchema,
        )])
        .unwrap();

        let tools = mcp_tools_from_catalog(&catalog);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "browser-schema");
        assert_eq!(
            tools[0]["inputSchema"]["oneOf"].as_array().unwrap().len(),
            3
        );

        let output = execute_tool_from_catalog(
            &catalog,
            "browser-schema",
            &json!({"action":"capabilities"}),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["action"], "capabilities");
        assert_eq!(
            value["operation_count"],
            crate::primitive_specs.len() as u64
        );
    }

    #[test]
    fn browser_call_builtin_is_publishable_and_preflights_before_browser_connection() {
        let catalog = AgentToolCatalog::try_new(vec![crate::AgentToolSpec::builtin(
            crate::AgentBuiltin::browser_call(crate::RawCdpAccess::Disabled),
        )])
        .unwrap();

        let tools = mcp_tools_from_catalog(&catalog);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "browser-call");
        assert_eq!(
            tools[0]["inputSchema"]["properties"]["calls"]["minItems"],
            1
        );

        let error =
            execute_tool_from_catalog(&catalog, "browser-call", &json!({"calls":[]})).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidArguments);
        assert!(error.message.contains("at least one call"));
    }

    #[test]
    fn large_surface_does_not_publish_small_surface_browser_builtins() {
        let tools = mcp_tools_from_catalog(crate::large_surface_agent_catalog());
        for name in ["browser-schema", "browser-call", "browser-events"] {
            assert!(
                !tools.iter().any(|tool| tool["name"] == name),
                "large-surface unexpectedly publishes {name}"
            );
        }
    }

    #[test]
    fn small_surface_publishes_facade_and_system_tools_but_not_browser_primitives() {
        let catalog = crate::agent_catalog_for_surface(
            crate::McpSurface::SmallSurface,
            crate::RawCdpAccess::Disabled,
        );
        let tools = mcp_tools_from_catalog(catalog);
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();

        for name in ["browser-schema", "browser-call", "browser-events"] {
            assert!(names.contains(name), "small-surface missing {name}");
        }
        for spec in crate::tool_specs {
            assert!(
                names.contains(spec.name),
                "small-surface missing system tool {}",
                spec.name
            );
        }
        for spec in crate::primitive_specs {
            assert!(
                !names.contains(spec.name),
                "small-surface unexpectedly publishes primitive {}",
                spec.name
            );
        }
        assert_eq!(tools.len(), crate::tool_specs.len() + 3);

        let error = execute_tool_from_catalog(catalog, "click", &json!({"target":"css:button"}))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert!(error.message.contains("unknown agent tool: click"));

        let schema =
            execute_tool_from_catalog(catalog, "browser-schema", &json!({"action":"capabilities"}))
                .unwrap();
        let value: Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(value["action"], "capabilities");
    }

    #[test]
    fn small_surface_raw_cdp_disabled_rejects_method_form_before_browser_acquisition() {
        let catalog = crate::agent_catalog_for_surface(
            crate::McpSurface::SmallSurface,
            crate::RawCdpAccess::Disabled,
        );
        let tool = catalog.get("browser-call").unwrap();
        assert!(
            !tool.input_schema().to_string().contains("\"method\""),
            "raw CDP method form must not be published when disabled"
        );

        let error = execute_tool_from_catalog(
            catalog,
            "browser-call",
            &json!({
                "calls":[{
                    "scope":"browser",
                    "call":{"method":"Target.getTargets","params":{}}
                }]
            }),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert!(!error.retryable);
        assert!(error.message.contains("raw CDP is disabled"));
    }

    #[test]
    fn browser_events_builtin_is_publishable_and_requires_persistent_mcp_state() {
        let catalog = AgentToolCatalog::try_new(vec![crate::AgentToolSpec::builtin(
            crate::AgentBuiltin::BrowserEvents,
        )])
        .unwrap();
        let tools = mcp_tools_from_catalog(&catalog);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "browser-events");
        assert_eq!(
            tools[0]["inputSchema"]["oneOf"].as_array().unwrap().len(),
            3
        );
        assert!(builtin_requires_persistent_mcp_session(
            crate::AgentBuiltin::BrowserEvents
        ));
        assert!(!builtin_requires_persistent_mcp_session(
            crate::AgentBuiltin::BrowserSchema
        ));

        let error = execute_tool_from_catalog(
            &catalog,
            "browser-events",
            &json!({"action":"poll","subscription_id":"","limit":1}),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidArguments);
    }

    #[test]
    fn system_args_are_mapped_to_existing_cli_shape() {
        let open = json!({"url":"https://example.com"});
        assert_eq!(
            system_cli_args("open-browser", open.as_object().unwrap()).unwrap(),
            vec!["https://example.com"]
        );
        let hitl = json!({"message":"approve"});
        assert_eq!(
            system_cli_args("hitl", hitl.as_object().unwrap()).unwrap(),
            vec!["approve"]
        );
    }

    #[test]
    fn tool_results_are_always_top_level_objects() {
        let array = success_envelope("inspect-images", "[1,2,3]");
        assert!(array.is_object());
        assert_eq!(array["ok"], true);
        assert!(array["data"].is_array());

        let text = success_envelope("click", "performed");
        assert!(text.is_object());
        assert_eq!(text["data"], "performed");

        let failure = ToolFailure::new(ErrorKind::TargetNotFound, "missing", true);
        let error = failure_envelope("assert-visible", &failure);
        assert!(error.is_object());
        assert_eq!(error["ok"], false);
        assert_eq!(error["error"]["kind"], "target_not_found");
        assert!(error["error"].get("details").is_none());

        let cdp = ToolFailure::from_error(crate::cdp_error(
            "Runtime.missing",
            -32601,
            "Method not found",
            None,
        ));
        let error = failure_envelope("browser-call", &cdp);
        assert_eq!(error["error"]["kind"], "cdp_failed");
        assert_eq!(error["error"]["retryable"], false);
        assert_eq!(
            error["error"]["details"],
            json!({
                "protocol":"cdp",
                "method":"Runtime.missing",
                "code":-32601,
                "message":"Method not found"
            })
        );
    }

    #[test]
    fn every_tool_declares_object_output_schema() {
        for catalog in [
            crate::agent_catalog_for_surface(
                crate::McpSurface::LargeSurface,
                crate::RawCdpAccess::Disabled,
            ),
            crate::agent_catalog_for_surface(
                crate::McpSurface::SmallSurface,
                crate::RawCdpAccess::Disabled,
            ),
        ] {
            for tool in mcp_tools_from_catalog(catalog) {
                assert_eq!(tool["outputSchema"]["type"], "object");
            }
        }
    }
}
