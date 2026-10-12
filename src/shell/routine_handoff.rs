//! Optional handoff delivery to an operator-configured agent adapter.
//! The default is always the agent that invoked call-routine: no subprocess
//! or network request is needed for that path.
use crate::config::{RoutineAgentConfig, config};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

fn validate_profile(profile: &RoutineAgentConfig) -> Result<(), String> {
    if profile.name.is_empty()
        || profile.name == "caller"
        || profile.name.len() > 64
        || !profile
            .name
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-' || ch == b'_')
    {
        return Err("agent profile requires a name of 1-64 lowercase letters, digits, '-' or '_' (not caller)".into());
    }
    match profile.transport.as_str() {
        "local" => {
            if !profile
                .program
                .as_ref()
                .is_some_and(|program| program.is_absolute())
                || profile.url.is_some()
                || profile.bearer_token_env.is_some()
            {
                return Err("local agent requires absolute program and no remote fields".into());
            }
        }
        "remote" => {
            let url = profile
                .url
                .as_ref()
                .ok_or("remote agent requires HTTPS url")?;
            let parsed =
                url::Url::parse(url).map_err(|_| "remote agent requires valid HTTPS url")?;
            if parsed.scheme() != "https"
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.fragment().is_some()
                || profile.program.is_some()
                || !profile.args.is_empty()
            {
                return Err(
                    "remote agent requires HTTPS URL without credentials or fragment".into(),
                );
            }
            if let Some(key) = &profile.bearer_token_env
                && (key.is_empty()
                    || !key.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    }))
            {
                return Err(
                    "remote bearer_token_env must be an uppercase environment variable".into(),
                );
            }
        }
        _ => return Err("agent transport must be local or remote".into()),
    }
    Ok(())
}

pub(crate) fn select_target(name: &str) -> Result<Option<&'static RoutineAgentConfig>, String> {
    if name == "caller" {
        return Ok(None);
    }
    let mut matches = config()
        .routines
        .agents
        .iter()
        .filter(|profile| profile.name == name);
    let profile = matches
        .next()
        .ok_or_else(|| format!("unknown routine handoff agent: {name}"))?;
    if matches.next().is_some() {
        return Err(format!("duplicate routine handoff agent: {name}"));
    }
    validate_profile(profile)?;
    Ok(Some(profile))
}

fn wait_for_process(mut command: Command, payload: &[u8]) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("handoff adapter could not start: {error}"))?;
    let deadline = Instant::now() + DISPATCH_TIMEOUT;
    let write_result = child
        .stdin
        .take()
        .ok_or("handoff adapter has no stdin")?
        .write_all(payload);
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("handoff adapter stdin failed: {error}"));
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("handoff adapter exited with {status}")),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("handoff adapter exceeded 5 second delivery budget".into());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("handoff adapter wait failed: {error}"));
            }
        }
    }
}

struct TemporaryHeader(PathBuf);

impl Drop for TemporaryHeader {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn remote_header(token_env: &str) -> Result<TemporaryHeader, String> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    let token = env::var(token_env)
        .map_err(|_| format!("handoff token environment variable {token_env} is missing"))?;
    if token.is_empty() || token.contains(['\r', '\n']) {
        return Err("handoff bearer token is empty or contains a newline".into());
    }
    let path = env::temp_dir().join(format!(
        "jelly-handoff-{:032x}.headers",
        rand::random::<u128>()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| format!("handoff header cannot be created: {error}"))?;
    let guard = TemporaryHeader(path);
    file.write_all(format!("Authorization: Bearer {token}\n").as_bytes())
        .map_err(|error| format!("handoff header cannot be written: {error}"))?;
    Ok(guard)
}

fn deliver_profile(profile: &RoutineAgentConfig, payload: &Value) -> Result<(), String> {
    validate_profile(profile)?;
    let bytes = serde_json::to_vec(payload).map_err(|error| error.to_string())?;
    match profile.transport.as_str() {
        "local" => {
            let mut command = Command::new(profile.program.as_ref().unwrap());
            command.args(&profile.args);
            wait_for_process(command, &bytes)
        }
        "remote" => {
            let header = profile
                .bearer_token_env
                .as_ref()
                .map(|key| remote_header(key))
                .transpose()?;
            let mut command = Command::new("curl");
            command.args([
                "--fail",
                "--silent",
                "--show-error",
                "--output",
                "/dev/null",
                "--connect-timeout",
                "2",
                "--max-time",
                "5",
                "--proto",
                "=https",
                "--request",
                "POST",
                "--header",
                "Content-Type: application/json",
                "--data-binary",
                "@-",
            ]);
            if let Some(header) = &header {
                command
                    .arg("--header")
                    .arg(format!("@{}", header.0.display()));
            }
            command.arg("--").arg(profile.url.as_ref().unwrap());
            wait_for_process(command, &bytes)
        }
        _ => unreachable!("validated adapter"),
    }
}

/// Persist the checkpoint *before* calling this. Delivery only acknowledges
/// receipt; it does not mean the work was performed or verified.
pub(crate) fn dispatch(target: &str, payload: &Value) -> Value {
    if target == "caller" {
        return json!({"target":"caller","delivery":"returned","action":"execute_instruction_then_resume"});
    }
    match select_target(target).and_then(|profile| deliver_profile(profile.unwrap(), payload)) {
        Ok(()) => {
            json!({"target":target,"delivery":"delivered","action":"await_external_agent_then_resume"})
        }
        Err(error) => json!({
            "target":target,"delivery":"failed","fallback":"caller",
            "action":"execute_instruction_then_resume","error":error
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_profile(program: &str, args: Vec<String>) -> RoutineAgentConfig {
        RoutineAgentConfig {
            name: "worker".into(),
            transport: "local".into(),
            program: Some(program.into()),
            args,
            url: None,
            bearer_token_env: None,
        }
    }

    #[test]
    fn routing_is_opt_in_and_invalid_remotes_cannot_be_used() {
        assert!(select_target("caller").unwrap().is_none());
        let payload = json!({"resume_id":"123","instruction":"Click"});
        assert_eq!(dispatch("caller", &payload)["delivery"], "returned");
        let mut profile = local_profile("relative/program", vec![]);
        assert!(validate_profile(&profile).is_err());
        profile.program = Some("/bin/cat".into());
        assert!(validate_profile(&profile).is_ok());
        profile.name = "caller".into();
        assert!(validate_profile(&profile).is_err());
        profile.name = "worker".into();
        profile.transport = "remote".into();
        profile.program = None;
        profile.url = Some("http://example.com/agent".into());
        assert!(validate_profile(&profile).is_err());
        profile.url = Some("https://user:pass@example.com/agent".into());
        assert!(validate_profile(&profile).is_err());
        profile.url = Some("https://example.com/agent".into());
        profile.bearer_token_env = Some("WORKER_TOKEN".into());
        assert!(validate_profile(&profile).is_ok());
        profile.bearer_token_env = Some("bad\nTOKEN".into());
        assert!(validate_profile(&profile).is_err());
    }

    #[test]
    fn local_adapter_receives_structured_handoff_on_stdin() {
        let path = env::temp_dir().join(format!(
            "jelly-handoff-test-{:032x}.json",
            rand::random::<u128>()
        ));
        assert!(!path.exists());
        let profile = local_profile("/usr/bin/tee", vec![path.to_string_lossy().into_owned()]);
        let payload = json!({"status":"suspended","resume_id":"12345678",
            "instruction":"Click the changed button","verify":"assert"});
        let result = deliver_profile(&profile, &payload);
        let written = fs::read(&path);
        let _ = fs::remove_file(&path);
        result.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&written.unwrap()).unwrap(),
            payload
        );
    }
}
