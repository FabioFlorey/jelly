use crate::{
    ArgKind, BrowserSession, ErrorKind, classify_error, execute_browser_primitive,
    is_browser_primitive,
    mcp_auth::{AuthState, ConsentMode},
    primitive_specs, tool_specs,
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
use std::{env, fs, process::Command};

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
        "tools/list" => Ok(json!({"tools": mcp_tools()})),
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

pub fn mcp_tools() -> Vec<Value> {
    let mut tools = primitive_specs
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "description": spec.description,
                "inputSchema": primitive_input_schema(spec),
                "outputSchema": tool_output_schema(),
                "securitySchemes": [{"type":"oauth2","scopes":["jelly"]}],
                "_meta": {
                    "securitySchemes": [{"type":"oauth2","scopes":["jelly"]}]
                },
            })
        })
        .collect::<Vec<_>>();

    tools.extend(tool_specs.iter().filter_map(|spec| {
        system_input_schema(spec.name).map(|input_schema| {
            json!({
                "name": spec.name,
                "description": spec.description,
                "inputSchema": input_schema,
                "outputSchema": tool_output_schema(),
                "securitySchemes": [{"type":"oauth2","scopes":["jelly"]}],
                "_meta": {
                    "securitySchemes": [{"type":"oauth2","scopes":["jelly"]}]
                },
            })
        })
    }));
    tools
}

fn tool_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "ok":{"type":"boolean"},
            "data":{},
            "error":{
                "type":["object","null"],
                "properties":{
                    "kind":{"type":"string"},
                    "message":{"type":"string"},
                    "retryable":{"type":"boolean"}
                },
                "additionalProperties":false
            },
            "meta":{
                "type":"object",
                "properties":{"tool":{"type":"string"}},
                "required":["tool"],
                "additionalProperties":true
            }
        },
        "required":["ok","data","error","meta"],
        "additionalProperties":false
    })
}

fn primitive_input_schema(spec: &crate::PrimitiveSpec) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for arg in spec.args {
        let schema = match arg.kind {
            ArgKind::Integer => json!({"type":"integer","minimum":0}),
            ArgKind::Target => json!({
                "type":"string",
                "description":"Element target: stable @eN ref, css:<selector>, text:<exact text>, or plain exact text."
            }),
            ArgKind::String => json!({"type":"string"}),
        };
        properties.insert(arg.name.to_owned(), schema);
        if arg.required {
            required.push(Value::String(arg.name.to_owned()));
        }
    }
    json!({
        "type":"object",
        "properties":properties,
        "required":required,
        "additionalProperties":false
    })
}

fn system_input_schema(name: &str) -> Option<Value> {
    Some(match name {
        "open-browser" => json!({
            "type":"object",
            "properties":{
                "url":{"type":"string","description":"Optional URL to open after Chromium starts."}
            },
            "additionalProperties":false
        }),
        "close-browser" | "downloads" => json!({
            "type":"object","properties":{},"additionalProperties":false
        }),
        "verify-artifact" => json!({
            "type":"object",
            "properties":{
                "artifact":{"type":"string","description":"Artifact ID or file path."},
                "semantic_checks":{"type":"array","items":{"type":"string"},"description":"Optional evidence labels already established before capture."}
            },
            "required":["artifact"],
            "additionalProperties":false
        }),
        "wait-download" => json!({
            "type":"object",
            "properties":{
                "after_ms":{"type":"integer","minimum":0,"description":"Unix timestamp in milliseconds captured before triggering the download."},
                "seconds":{"type":"integer","minimum":1,"description":"Maximum wait time; defaults to 30."},
                "name_contains":{"type":"string","description":"Optional filename substring."}
            },
            "required":["after_ms"],
            "additionalProperties":false
        }),
        "browser-task" => json!({
            "type":"object",
            "properties":{
                "url":{"type":"string"},
                "tool":{"type":"string"},
                "args":{"type":"array","items":{"type":"string"}},
                "persist":{"type":"boolean"}
            },
            "required":["url","tool"],
            "additionalProperties":false
        }),
        "profile-import" => json!({
            "type":"object",
            "properties":{
                "source":{"type":"string","description":"Closed Chromium user-data directory to copy into Jelly runtime."},
                "force":{"type":"boolean","description":"Replace an existing Jelly profile."}
            },
            "required":["source"],
            "additionalProperties":false
        }),
        "screenshot" => json!({
            "type":"object",
            "properties":{
                "target":{"type":"string","description":"Optional browser target such as body, main, css:..., or @eN. Omit for the active page viewport."},
                "output":{"type":"string","description":"Optional output path."},
                "desktop_fallback":{"type":"boolean","description":"Explicitly allow headed desktop/window capture only if browser-native capture fails."}
            },
            "additionalProperties":false
        }),
        "record-browser" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["start","stop"]},
                "mode":{"type":"string","enum":["continuous","steps"],"description":"Recording mode for start. continuous streams frames; steps captures browser state after relevant actions."},
                "interval_ms":{"type":"integer","minimum":100,"description":"Frame interval for continuous mode; defaults to 500 ms."},
                "hold_ms":{"type":"integer","minimum":100,"description":"How long each captured action frame is shown in steps mode; defaults to 1000 ms."}
            },
            "required":["action"],
            "additionalProperties":false
        }),
        "inspect-network" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["start","stop","show"]},
                "filters":{"type":"array","items":{"type":"string"}}
            },
            "required":["action"],
            "additionalProperties":false
        }),
        "call-routine" => json!({
            "type":"object",
            "properties":{
                "name":{"type":"string","description":"Routine name when starting a routine."},
                "resume_id":{"type":"string","description":"Continuation ID when resuming a suspended routine."},
                "vars":{"type":"object","additionalProperties":{"type":"string"}}
            },
            "oneOf":[{"required":["name"]},{"required":["resume_id"]}],
            "additionalProperties":false
        }),
        "hitl" => json!({
            "type":"object",
            "properties":{
                "message":{"type":"string"},
                "screenshot_target":{"type":"string","description":"Optional browser element to attach instead of the viewport."},
                "no_screenshot":{"type":"boolean"},
                "desktop_fallback":{"type":"boolean","description":"Explicitly allow desktop/window capture if browser-native screenshot fails."}
            },
            "required":["message"],
            "additionalProperties":false
        }),
        _ => return None,
    })
}

#[derive(Debug)]
struct ToolFailure {
    kind: ErrorKind,
    message: String,
    retryable: bool,
}

impl ToolFailure {
    fn new(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
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
    json!({
        "ok": false,
        "data": Value::Null,
        "error": {
            "kind": failure.kind.as_str(),
            "message": failure.message,
            "retryable": failure.retryable
        },
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

fn execute_tool(name: &str, arguments: &Value) -> Result<String, ToolFailure> {
    let object = arguments.as_object().ok_or_else(|| {
        ToolFailure::new(
            ErrorKind::InvalidArguments,
            "tool arguments must be a JSON object",
            false,
        )
    })?;
    if is_browser_primitive(name) {
        let spec = primitive_specs
            .iter()
            .find(|spec| spec.name == name)
            .ok_or_else(|| {
                ToolFailure::new(
                    ErrorKind::Unsupported,
                    format!("unknown browser primitive: {name}"),
                    false,
                )
            })?;
        let mut args = Vec::new();
        for arg in spec.args {
            match object.get(arg.name) {
                Some(Value::String(value)) => args.push(value.clone()),
                Some(Value::Number(value)) if matches!(arg.kind, ArgKind::Integer) => {
                    args.push(value.to_string())
                }
                Some(_) => {
                    return Err(ToolFailure::new(
                        ErrorKind::InvalidArguments,
                        format!("{} has the wrong type", arg.name),
                        false,
                    ));
                }
                None if arg.required => {
                    return Err(ToolFailure::new(
                        ErrorKind::InvalidArguments,
                        format!("missing required argument: {}", arg.name),
                        false,
                    ));
                }
                None => {}
            }
        }
        let mut browser = BrowserSession::connect()
            .map_err(|e| ToolFailure::new(ErrorKind::BrowserUnavailable, e.to_string(), true))?;
        return execute_browser_primitive(&mut browser, name, &args).map_err(|e| {
            let (kind, retryable) = classify_error(e.as_ref());
            ToolFailure::new(kind, e.to_string(), retryable)
        });
    }

    if !tool_specs.iter().any(|spec| spec.name == name) || system_input_schema(name).is_none() {
        return Err(ToolFailure::new(
            ErrorKind::Unsupported,
            format!("unknown tool: {name}"),
            false,
        ));
    }
    let args = system_cli_args(name, object)
        .map_err(|message| ToolFailure::new(ErrorKind::InvalidArguments, message, false))?;
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
            if bool_value("desktop_fallback")? {
                args.push("--desktop-fallback".into());
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
            if bool_value("desktop_fallback")? {
                args.push("--desktop-fallback".into());
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
        assert!(instructions.contains("Use call-routine"));
        assert!(instructions.contains("Use wait-for"));
        assert!(instructions.contains("Use assert-*"));
        assert!(instructions.contains("verified-screenshot"));
        assert!(instructions.contains("verified-download"));
        assert!(instructions.contains("inputSchema"));
        assert!(instructions.contains("agent-run <tool> [args...]"));
        assert!(instructions.contains("Do not automatically retry side-effecting operations"));
    }

    #[test]
    fn mcp_catalog_covers_both_registries() {
        let tools = mcp_tools();
        for spec in primitive_specs {
            assert!(tools.iter().any(|tool| tool["name"] == spec.name));
        }
        for spec in tool_specs {
            assert!(
                tools.iter().any(|tool| tool["name"] == spec.name),
                "system tool missing MCP mapping: {}",
                spec.name
            );
        }
    }

    #[test]
    fn primitive_schema_uses_named_arguments() {
        let click = primitive_specs
            .iter()
            .find(|spec| spec.name == "click")
            .unwrap();
        let schema = primitive_input_schema(click);
        assert_eq!(schema["properties"]["target"]["type"], "string");
        assert_eq!(schema["required"][0], "target");
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
    }

    #[test]
    fn every_tool_declares_object_output_schema() {
        for tool in mcp_tools() {
            assert_eq!(tool["outputSchema"]["type"], "object");
        }
    }
}
