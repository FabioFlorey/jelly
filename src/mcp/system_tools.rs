use super::{ToolFailure, browser_session::reset_mcp_browser_session};
use crate::ErrorKind;
use serde_json::{Map, Value};
use std::{env, process::Command};

pub(super) fn execute_mcp_system_tool(
    name: &str,
    object: &Map<String, Value>,
) -> Result<String, ToolFailure> {
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

pub(super) fn system_cli_args(
    name: &str,
    object: &Map<String, Value>,
) -> Result<Vec<String>, String> {
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
