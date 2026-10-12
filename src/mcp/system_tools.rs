use super::dispatch::ToolFailure;
use crate::ErrorKind;
use crate::shell::session::reset_mcp_browser_session;
use serde_json::{Map, Value};
use std::{env, process::Command};

pub(super) fn execute_mcp_system_tool(
    name: &str,
    object: &Map<String, Value>,
) -> Result<String, ToolFailure> {
    if crate::config::config().browser.backend
        == crate::browser_backend::BrowserBackendKind::RemoteCdp
        && matches!(name, "download" | "downloads" | "wait-download")
    {
        let remote = crate::config::selected_remote()
            .map_err(|reason| ToolFailure::new(ErrorKind::InvalidArguments, reason, false))?;
        if remote.download_path.is_none() {
            return Err(ToolFailure::new(
                ErrorKind::Unsupported,
                "remote downloads require download_path",
                false,
            ));
        }
    }
    if crate::config::config().browser.backend
        == crate::browser_backend::BrowserBackendKind::RemoteCdp
        && matches!(
            name,
            "open-browser" | "download" | "downloads" | "wait-download"
        )
    {
        crate::shell::session::ensure_remote_tracker().map_err(ToolFailure::from_error)?;
    }
    let args = system_cli_args(name, object)
        .map_err(|message| ToolFailure::new(ErrorKind::InvalidArguments, message, false))?;
    if matches!(
        name,
        "open-browser" | "close-browser" | "browser-task" | "profile-import"
    ) {
        reset_mcp_browser_session();
    }
    run_system_tool(name, &args).map_err(|message| {
        let kind = if matches!(name, "wait-download" | "download") && message.contains("timed out")
        {
            ErrorKind::ConditionTimeout
        } else {
            match name {
                "open-browser" | "close-browser" | "browser-task" => ErrorKind::BrowserUnavailable,
                "profile-import" => ErrorKind::InteractionFailed,
                "screenshot" | "record-browser" | "verify-artifact" => ErrorKind::ArtifactFailed,
                "download" | "downloads" | "wait-download" => ErrorKind::DownloadFailed,
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
        "download" => {
            let action = string("action")?.ok_or("download requires action")?;
            match action.as_str() {
                "list" => {
                    if object.len() != 1 {
                        return Err("download list accepts only action".into());
                    }
                    Ok(vec![action])
                }
                "status" | "cancel" => {
                    let id =
                        string("id")?.ok_or_else(|| format!("download {action} requires id"))?;
                    if object
                        .keys()
                        .any(|key| !matches!(key.as_str(), "action" | "id"))
                    {
                        return Err(format!("download {action} accepts only action and id"));
                    }
                    Ok(vec![action, id])
                }
                "transfer" => {
                    let id = string("id")?.ok_or("download transfer requires id")?;
                    let destination = string("destination")?;
                    let collision = string("collision")?;
                    if collision.is_some() && destination.is_none() {
                        return Err("download collision requires destination".into());
                    }
                    if let Some(policy) = collision.as_deref()
                        && !matches!(policy, "fail" | "overwrite" | "uniquify")
                    {
                        return Err("invalid collision policy".into());
                    }
                    let mut args = vec![action, id];
                    if let Some(destination) = destination {
                        args.push(destination);
                        if let Some(collision) = collision {
                            args.push(collision);
                        }
                    }
                    Ok(args)
                }
                "wait" => {
                    let id = string("id")?.ok_or("download wait requires id")?;
                    let seconds = integer("seconds")?.unwrap_or(30).max(1);
                    let destination = string("destination")?;
                    let collision = string("collision")?;
                    if collision.is_some() && destination.is_none() {
                        return Err("download collision requires destination".into());
                    }
                    if let Some(policy) = collision.as_deref()
                        && !matches!(policy, "fail" | "overwrite" | "uniquify")
                    {
                        return Err(
                            "download collision must be fail, overwrite, or uniquify".into()
                        );
                    }
                    let mut args = vec![action, id, seconds.to_string()];
                    if let Some(destination) = destination {
                        args.push(destination);
                        if let Some(collision) = collision {
                            args.push(collision);
                        }
                    }
                    Ok(args)
                }
                _ => Err("download action must be list, status, wait, transfer, or cancel".into()),
            }
        }
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
            if let Some(action) = string("action")? {
                return match action.as_str() {
                    "list" if object.len() == 1 => Ok(vec!["list".into()]),
                    "capture" => {
                        let operation = string("operation")?.ok_or("capture requires operation")?;
                        let id = string("capture_id")?;
                        let scope = string("scope")?;
                        if operation == "start"
                            && scope.as_deref() == Some("isolated")
                            && object.len() == 3
                        {
                            return Ok(vec!["--capture".into(), "start-isolated".into()]);
                        }
                        if scope.is_some() {
                            return Err("capture scope is supported only for start".into());
                        }
                        match (operation.as_str(), id) {
                            ("start", None) if object.len() == 2 =>
                                Ok(vec!["--capture".into(), "start".into()]),
                            ("status", None) if object.len() == 2 =>
                                Ok(vec!["--capture".into(), "status".into()]),
                            ("status", Some(id)) | ("stop", Some(id))
                                if object.len() == 3 =>
                                Ok(vec!["--capture".into(), operation, id]),
                            _ => Err("capture requires start, status [capture_id], or stop with capture_id".into()),
                        }
                    }

                    "crystallize" if (4..=5).contains(&object.len()) => {
                        let name = string("name")?.ok_or("crystallize requires name")?;
                        let expected = integer("expected_revision")?
                            .ok_or("crystallize requires expected_revision")?;
                        let trace_id =
                            string("trace_id")?.ok_or("crystallize requires trace_id")?;
                        if expected < 1 {
                            return Err("expected_revision must be positive".into());
                        }
                        let mut args =
                            vec!["--crystallize".into(), name, expected.to_string(), trace_id];
                        if let Some(parameters) = object.get("parameters") {
                            if !parameters.is_object() {
                                return Err("parameters must be an object".into());
                            }
                            args.push(
                                serde_json::to_string(parameters).map_err(|e| e.to_string())?,
                            );
                        }
                        Ok(args)
                    }
                    "versions" if object.len() == 2 => Ok(vec![
                        "--versions".into(),
                        string("name")?.ok_or("versions requires name")?,
                    ]),
                    "revise" if object.len() == 4 => {
                        let name = string("name")?.ok_or("revise requires name")?;
                        let expected = integer("expected_revision")?
                            .ok_or("revise requires expected_revision")?;
                        if expected < 1 {
                            return Err("expected_revision must be positive".into());
                        }
                        let graph = object.get("graph").ok_or("revise requires graph")?;
                        if !graph.is_object() {
                            return Err("graph must be an object".into());
                        }
                        Ok(vec![
                            "--revise".into(),
                            name,
                            expected.to_string(),
                            serde_json::to_string(graph).map_err(|e| e.to_string())?,
                        ])
                    }
                    "promote" if object.len() == 4 => {
                        let name = string("name")?.ok_or("promote requires name")?;
                        let revision = integer("revision")?.ok_or("promote requires revision")?;
                        let expected = integer("expected_revision")?
                            .ok_or("promote requires expected_revision")?;
                        if revision < 1 || expected < 1 {
                            return Err("revision numbers must be positive".into());
                        }
                        Ok(vec![
                            "--promote".into(),
                            name,
                            revision.to_string(),
                            expected.to_string(),
                        ])
                    }
                    "status" if object.len() == 2 => Ok(vec![
                        "--status".into(),
                        string("resume_id")?.ok_or("status requires resume_id")?,
                    ]),
                    "claim" => {
                        let id = string("resume_id")?.ok_or("claim requires resume_id")?;
                        let owner = string("owner")?.ok_or("claim requires owner")?;
                        let seconds = integer("ttl_seconds")?;
                        let takeover = string("takeover_reason")?;
                        let expected =
                            3 + usize::from(seconds.is_some()) + usize::from(takeover.is_some());
                        if object.len() != expected {
                            return Err("unexpected claim fields".into());
                        }
                        let mut args = vec!["--claim".into(), id, owner];
                        if let Some(ttl) = seconds {
                            args.push(ttl.to_string());
                        }
                        if let Some(reason) = takeover {
                            args.extend(["--takeover".into(), reason]);
                        }
                        Ok(args)
                    }
                    "renew" => {
                        let id = string("resume_id")?.ok_or("renew requires resume_id")?;
                        let token = string("lease_token")?.ok_or("renew requires lease_token")?;
                        let seconds = integer("ttl_seconds")?;
                        if object.len() != 3 + usize::from(seconds.is_some()) {
                            return Err("unexpected renew fields".into());
                        }
                        let mut args = vec!["--renew".into(), id, token];
                        if let Some(ttl) = seconds {
                            args.push(ttl.to_string());
                        }
                        Ok(args)
                    }
                    "release" if object.len() == 3 => Ok(vec![
                        "--release".into(),
                        string("resume_id")?.ok_or("release requires resume_id")?,
                        string("lease_token")?.ok_or("release requires lease_token")?,
                    ]),
                    "search" if object.len() == 2 => Ok(vec![
                        "--search".into(),
                        string("query")?.ok_or("search requires query")?,
                    ]),
                    "describe" if object.len() == 2 => Ok(vec![
                        "describe".into(),
                        string("name")?.ok_or("describe requires name")?,
                    ]),
                    "save" if object.len() == 2 => {
                        let graph = object.get("graph").ok_or("save requires graph")?;
                        if !graph.is_object() {
                            return Err("graph must be an object".into());
                        }
                        let text = serde_json::to_string(graph).map_err(|e| e.to_string())?;
                        Ok(vec!["save".into(), text])
                    }
                    _ => Err("invalid call-routine management action or extra fields".into()),
                };
            }
            let name = string("name")?;
            let resume_id = string("resume_id")?;
            if name.is_some() == resume_id.is_some() {
                return Err("provide exactly one of name or resume_id".into());
            }
            let handoff_to = string("handoff_to")?;
            let lease_token = string("lease_token")?;
            if resume_id.is_some() && handoff_to.is_some() {
                return Err("handoff_to is fixed by the checkpoint on resume".into());
            }
            if name.is_some() && lease_token.is_some() {
                return Err("lease_token may only accompany resume_id".into());
            }
            let mut args = if let Some(id) = resume_id {
                vec!["resume".into(), id]
            } else {
                vec![name.unwrap()]
            };
            if let Some(target) = handoff_to {
                args.splice(0..0, ["--handoff-to".into(), target]);
            }
            if let Some(token) = lease_token {
                args.extend(["--lease".into(), token]);
            }
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
            if let Some(video) = string("video")? {
                args.extend(["--video".into(), video]);
            }
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
    let browser_id = crate::config::active_browser_id();
    let output = if direct.is_file() {
        let mut cmd = Command::new(direct);
        cmd.args(args);
        if let Some(id) = &browser_id { cmd.env("JELLY_BROWSER_ID", id); }
        cmd.output()
    } else if crate::config::config().mcp.allow_cargo_fallback {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["run", "--quiet", "--bin", &format!("agent-{name}"), "--"])
            .args(args);
        if let Some(id) = &browser_id { cmd.env("JELLY_BROWSER_ID", id); }
        cmd.output()
    } else {
        return Err(format!(
            "required MCP system tool binary is missing: {} (enable [mcp].allow_cargo_fallback in config/jelly.toml only for development)",
            direct.display()
        ));
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
    use serde_json::json;

    #[test]
    fn download_system_args_preserve_lifecycle_contract() {
        assert_eq!(
            system_cli_args("download", json!({"action":"list"}).as_object().unwrap()).unwrap(),
            vec!["list".to_owned()]
        );
        assert_eq!(
            system_cli_args(
                "download",
                json!({
                    "action":"wait",
                    "id":"guid-1",
                    "seconds":12,
                    "destination":"/tmp/out",
                    "collision":"uniquify"
                })
                .as_object()
                .unwrap()
            )
            .unwrap(),
            vec![
                "wait".to_owned(),
                "guid-1".to_owned(),
                "12".to_owned(),
                "/tmp/out".to_owned(),
                "uniquify".to_owned()
            ]
        );
        assert!(
            system_cli_args(
                "download",
                json!({"action":"wait","id":"guid-1","collision":"overwrite"})
                    .as_object()
                    .unwrap()
            )
            .unwrap_err()
            .contains("requires destination")
        );
    }
}
