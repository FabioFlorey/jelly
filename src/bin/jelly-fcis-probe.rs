//! Isolated FC/IS integration. Builds/profiles/state must be under a fresh sandbox.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn require(test: bool, msg: &str) -> Result<()> {
    if test {
        Ok(())
    } else {
        Err(msg.to_owned().into())
    }
}
fn port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}
struct Children(Vec<Child>);
impl Children {
    fn launch(
        &mut self,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        envs: &[(String, String)],
        log: &Path,
    ) -> Result<usize> {
        let logfile = fs::File::create(log)?;
        let stderr = logfile.try_clone()?;
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .current_dir(cwd)
            .stdout(Stdio::from(logfile))
            .stderr(Stdio::from(stderr))
            .process_group(0);
        for (key, _) in env::vars().filter(|(key, _)| key.starts_with("JELLY_")) {
            cmd.env_remove(key);
        }
        for (key, value) in envs {
            cmd.env(key, value);
        }
        self.0.push(cmd.spawn()?);
        Ok(self.0.len() - 1)
    }
    fn stop(&mut self, index: usize) {
        if let Some(child) = self.0.get_mut(index)
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", child.id())])
                .output();
            let timeout = Instant::now() + Duration::from_secs(6);
            while Instant::now() < timeout {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(70))
            }
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}
impl Drop for Children {
    fn drop(&mut self) {
        for i in (0..self.0.len()).rev() {
            self.stop(i)
        }
    }
}
fn exec(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    envs: &[(String, String)],
) -> Result<(bool, String, String)> {
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(cwd);
    for (key, _) in env::vars().filter(|(key, _)| key.starts_with("JELLY_")) {
        cmd.env_remove(key);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let result = cmd.output()?;
    Ok((
        result.status.success(),
        String::from_utf8_lossy(&result.stdout).into_owned(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
    ))
}
fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
    ctype: &str,
    bearer: Option<&str>,
) -> Result<(u16, String, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(8)))?;
    let data = body.unwrap_or("");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        data.len()
    );
    if body.is_some() {
        request.push_str(&format!("Content-Type: {ctype}\r\n"));
    }
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(data);
    stream.write_all(request.as_bytes())?;
    // DevTools can keep HTTP/1.1 sockets open even with Connection: close.
    // Honor Content-Length or decode chunked framing instead of waiting for EOF.
    let mut response = Vec::new();
    let mut chunk = [0u8; 16384];
    let mut header_end = None;
    let mut expected_len = None;
    let mut chunked = false;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&chunk[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Err(format!(
                    "HTTP read timed out for {method} {path} after {} bytes",
                    response.len()
                )
                .into());
            }
            Err(e) => return Err(e.into()),
        }
        require(
            response.len() <= 10_000_000,
            "HTTP response exceeds test limit",
        )?;
        if header_end.is_none()
            && let Some(end) = response.windows(4).position(|w| w == b"\r\n\r\n")
        {
            header_end = Some(end + 4);
            let head = String::from_utf8_lossy(&response[..end]);
            for line in head.lines() {
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("Content-Length") {
                        expected_len = Some(value.trim().parse::<usize>()?);
                    }
                    if name.eq_ignore_ascii_case("Transfer-Encoding")
                        && value.to_ascii_lowercase().contains("chunked")
                    {
                        chunked = true;
                    }
                }
            }
        }
        if let Some(start) = header_end {
            let body = &response[start..];
            if chunked && decode_chunks(body).is_some() {
                break;
            }
            if let Some(size) = expected_len
                && body.len() >= size
            {
                break;
            }
        }
    }
    let response = String::from_utf8(response)?;
    let (headers, body) = response.split_once("\r\n\r\n").ok_or("bad HTTP response")?;
    let contents = if chunked {
        let decoded = decode_chunks(body.as_bytes()).ok_or("truncated chunked HTTP response")?;
        String::from_utf8(decoded)?
    } else if let Some(size) = expected_len {
        body.get(..size)
            .ok_or("truncated HTTP Content-Length")?
            .to_owned()
    } else {
        body.to_owned()
    };
    let status = headers
        .lines()
        .next()
        .and_then(|x| x.split_whitespace().nth(1))
        .ok_or("HTTP status missing")?
        .parse()?;
    let location = headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("Location"))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_default();
    Ok((status, location, contents))
}
fn decode_chunks(input: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    let mut result = Vec::new();
    loop {
        let end = input[pos..].windows(2).position(|w| w == b"\r\n")? + pos;
        let hex = std::str::from_utf8(&input[pos..end])
            .ok()?
            .split(';')
            .next()?
            .trim();
        let length = usize::from_str_radix(hex, 16).ok()?;
        pos = end + 2;
        if length == 0 {
            return Some(result);
        }
        if input.len() < pos + length + 2 || &input[pos + length..pos + length + 2] != b"\r\n" {
            return None;
        }
        result.extend_from_slice(&input[pos..pos + length]);
        pos += length + 2;
    }
}
fn form(fields: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish()
}
fn post_form(port: u16, path: &str, fields: &[(&str, &str)]) -> Result<(u16, String, Value)> {
    let (status, location, body) = http(
        port,
        "POST",
        path,
        Some(&form(fields)),
        "application/x-www-form-urlencoded",
        None,
    )?;
    Ok((
        status,
        location,
        serde_json::from_str(&body).unwrap_or(Value::Null),
    ))
}
fn post_json(port: u16, path: &str, value: Value, bearer: Option<&str>) -> Result<(u16, Value)> {
    let (status, _, body) = http(
        port,
        "POST",
        path,
        Some(&value.to_string()),
        "application/json",
        bearer,
    )?;
    Ok((status, serde_json::from_str(&body).unwrap_or(Value::Null)))
}
fn rpc(port: u16, method: &str, params: Value, bearer: &str) -> Result<Value> {
    let (status, body) = post_json(
        port,
        "/mcp",
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        Some(bearer),
    )?;
    require(
        status == 200 && body.get("result").is_some(),
        &format!("MCP {method} error: {status} {body}"),
    )?;
    Ok(body["result"].clone())
}
fn call(port: u16, tool: &str, params: Value, bearer: &str) -> Result<Value> {
    rpc(
        port,
        "tools/call",
        json!({"name":"browser-call","arguments":{"calls":[{"call":{"jelly":tool,"params":params}}]}}),
        bearer,
    )
}
fn record(checks: &mut Vec<String>, label: &str, success: bool) -> Result<()> {
    require(success, label)?;
    println!("PASS {label}");
    checks.push(label.to_owned());
    Ok(())
}
fn browser(
    children: &mut Children,
    repo: &Path,
    sandbox: &Path,
    fixture: &str,
    number: usize,
    envs: &[(String, String)],
) -> Result<(usize, PathBuf)> {
    let profile = sandbox.join(format!("fcis-browser-{}-{number}", std::process::id()));
    fs::create_dir(&profile)?;
    let args = [
        "--headless=new",
        "--no-sandbox",
        "--disable-gpu",
        "--disable-background-networking",
        "--disable-sync",
        "--no-first-run",
        "--disable-dev-shm-usage",
        "--remote-allow-origins=*",
        "--remote-debugging-port=0",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([
        format!("--user-data-dir={}", profile.display()),
        fixture.to_owned(),
    ])
    .collect::<Vec<_>>();
    let index = children.launch(
        Path::new("/usr/bin/chromium"),
        &args,
        repo,
        envs,
        &sandbox.join(format!("chromium-{number}.log")),
    )?;
    Ok((index, profile))
}
fn set_cdp(profile: &Path, runtime: &Path) -> Result<()> {
    let file = profile.join("DevToolsActivePort");
    let timeout = Instant::now() + Duration::from_secs(20);
    let lines = loop {
        if let Ok(text) = fs::read_to_string(&file) {
            let lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
            if lines.len() == 2 {
                break lines;
            }
        }
        if Instant::now() > timeout {
            return Err("Chromium CDP start timeout".into());
        }
        thread::sleep(Duration::from_millis(100));
    };
    let port = lines[0].parse::<u16>()?;
    fs::write(
        runtime.join("state/cdp_endpoint"),
        format!("ws://127.0.0.1:{port}{}", lines[1]),
    )?;
    let (_, _, body) = http(port, "GET", "/json/list", None, "", None)?;
    let value: Value = serde_json::from_str(&body)?;
    let target = value
        .as_array()
        .ok_or("CDP pages not array")?
        .iter()
        .find(|page| {
            page["type"] == "page"
                && page["url"]
                    .as_str()
                    .is_some_and(|v| v.ends_with("browser-perf.html"))
        })
        .ok_or("target page missing")?;
    let id = target["id"].as_str().ok_or("CDP target ID missing")?;
    fs::write(runtime.join("state/page_target_id"), id)?;
    fs::write(runtime.join("state/active_target_id"), id)?;
    Ok(())
}
fn wait_mcp(port: u16) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if http(port, "GET", "/health", None, "", None).is_ok_and(|(status, _, _)| status == 200) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(120))
    }
    Err("isolated MCP failed health check".into())
}
fn main() -> Result<()> {
    let sandbox = fs::canonicalize(
        env::args()
            .nth(1)
            .ok_or("usage: jelly-fcis-probe SANDBOX_ROOT")?,
    )?;
    require(
        sandbox.parent() == Some(Path::new("/data"))
            && sandbox
                .file_name()
                .is_some_and(|v| v.to_string_lossy().starts_with("jelly-fcis-isolated-")),
        "refusing unsafe FCIS sandbox path",
    )?;
    let repo = sandbox.join("repo");
    let runtime = sandbox.join("runtime");
    let bin = sandbox.join("build/debug");
    require(
        !repo.join(".env").exists() && repo != Path::new("/data/github/jelly"),
        "credentials or production source detected",
    )?;
    require(
        fs::read_to_string(repo.join("config/jelly.toml"))?
            .contains(&format!("runtime_root = \"{}\"", runtime.display())),
        "sandbox runtime config incorrect",
    )?;
    for name in [
        "jelly-mcp",
        "jelly-agent-api-probe",
        "agent-evaluate-js",
        "agent-find-interactive",
        "agent-call-routine",
    ] {
        require(
            bin.join(name).is_file(),
            &format!("missing isolated binary: {name}"),
        )?;
    }
    fs::create_dir_all(runtime.join("state"))?;
    fs::create_dir_all(runtime.join("artifacts/downloads"))?;
    let envs = vec![
        ("HOME".into(), sandbox.display().to_string()),
        (
            "XDG_CONFIG_HOME".into(),
            sandbox.join("xdg").display().to_string(),
        ),
        (
            "CARGO_TARGET_DIR".into(),
            sandbox.join("build").display().to_string(),
        ),
    ];
    let mut children = Children(vec![]);
    let mut checks = vec![];
    let fixture = format!(
        "file://{}",
        repo.join("tests/fixtures/browser-perf.html").display()
    );
    let (chrome_idx, profile) = browser(&mut children, &repo, &sandbox, &fixture, 1, &envs)?;
    set_cdp(&profile, &runtime)?;
    record(
        &mut checks,
        "isolated Chromium CDP/profile and page target",
        true,
    )?;
    for mode in [
        "semantic-read",
        "semantic-mutate-verify",
        "preflight-atomic",
        "failure-stop",
        "failure-continue",
        "stale-ref",
        "logical-target",
        "raw-target",
        "raw-browser",
        "mixed-batch",
        "events-lifecycle",
        "subscription-stale",
    ] {
        let (ok, out, err) = exec(&bin.join("jelly-agent-api-probe"), &[mode], &repo, &envs)?;
        record(
            &mut checks,
            &format!("Agent API/BrowserSession {mode}"),
            ok && serde_json::from_str::<Value>(&out).is_ok_and(|v| v["ok"] == true),
        )
        .map_err(|e| format!("{e}: {err}"))?;
    }
    let second_page = format!(
        "file://{}",
        repo.join("tests/fixtures/semantic-targets.html").display()
    );
    let (_, out, err) = exec(
        &bin.join("agent-evaluate-js"),
        &[&format!("location.href={}", json!(second_page))],
        &repo,
        &envs,
    )?;
    require(
        err.is_empty() || !out.is_empty(),
        "cannot navigate to isolated semantic fixture",
    )?;
    let config_path = repo.join("config/jelly.toml");
    let original = fs::read_to_string(&config_path)?;
    let parity = (|| -> Result<()> {
        let (ok, out, _) = exec(
            &bin.join("agent-find-interactive"),
            &["Duplicate action", "10"],
            &repo,
            &envs,
        )?;
        record(
            &mut checks,
            "Jelly CLI enabled-first ranking",
            ok && serde_json::from_str::<Value>(&out).is_ok_and(|v| v[0]["disabled"] == false),
        )?;
        fs::write(
            &config_path,
            original.replace("runtime = true", "runtime = false"),
        )?;
        let (ok, out, _) = exec(
            &bin.join("agent-find-interactive"),
            &["Duplicate action", "10"],
            &repo,
            &envs,
        )?;
        record(
            &mut checks,
            "Jelly CLI legacy rank priority",
            ok && serde_json::from_str::<Value>(&out).is_ok_and(|v| v[0]["disabled"] == false),
        )?;
        Ok(())
    })();
    fs::write(&config_path, &original)?;
    parity?;
    let routine_dir = repo.join(".agent/routines");
    let tool = routine_dir.join("fcis-integration-tool.json");
    let loop_path = routine_dir.join("fcis-integration-loop.json");
    let hitl = routine_dir.join("fcis-integration-hitl.json");
    for p in [&tool, &loop_path, &hitl] {
        require(!p.exists(), "refusing to overwrite existing routine")?;
    }
    fs::write(&tool,json!({"entry":"tool","nodes":{"tool":{"tool":"evaluate-js","args":["21*2"],"save":"answer","next":"decide"},"decide":{"guard":{"path":"answer","op":"equals","value":42},"then":"done","else":"bad"},"done":{"terminal":"success","message":"browser tool passed"},"bad":{"terminal":"failure","message":"wrong browser output"}}}).to_string())?;
    fs::write(
        &loop_path,
        json!({"entry":"loop","max_steps":10,"nodes":{"loop":{"goto":"loop","max_visits":2}}})
            .to_string(),
    )?;
    fs::write(&hitl,json!({"entry":"review","nodes":{"review":{"hitl":"Approve {{name}}","resume":"check"},"check":{"guard":{"path":"verdict","op":"equals","value":"approved"},"then":"done","else":"bad"},"done":{"terminal":"success","message":"approved {{name}}"},"bad":{"terminal":"failure","message":"rejected {{name}}"}}}).to_string())?;
    let routine = (|| -> Result<()> {
        let (ok, out, _) = exec(
            &bin.join("agent-call-routine"),
            &["fcis-integration-tool"],
            &repo,
            &envs,
        )?;
        record(
            &mut checks,
            "routine actual browser tool and core transition",
            ok && serde_json::from_str::<Value>(&out)
                .is_ok_and(|v| v["status"] == "completed" && v["context"]["answer"] == 42),
        )?;
        let (ok, _, err) = exec(
            &bin.join("agent-call-routine"),
            &["fcis-integration-loop"],
            &repo,
            &envs,
        )?;
        record(
            &mut checks,
            "routine max_visits protection and failure cleanup",
            !ok && err.contains("exceeded max_visits=2"),
        )?;
        let fake_run = bin.join("agent-run");
        let fake_hitl = bin.join("agent-hitl");
        require(
            !fake_run.exists() && !fake_hitl.exists(),
            "refusing to replace genuine HITL executable",
        )?;
        fs::write(
            &fake_run,
            "#!/bin/sh\n[ \"$1\" = hitl ] || exit 91\nprintf '{\"delivered\":true}\\n'\n",
        )?;
        fs::write(&fake_hitl, "#!/bin/sh\nexit 0\n")?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_run, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&fake_hitl, fs::Permissions::from_mode(0o700))?;
        let inner = (|| -> Result<()> {
            let (ok, out, err) = exec(
                &bin.join("agent-call-routine"),
                &["fcis-integration-hitl", "name=Alice"],
                &repo,
                &envs,
            )?;
            require(ok, &format!("HITL suspend failed: {err}"))?;
            let result: Value = serde_json::from_str(&out)?;
            let resume = result["resume_id"].as_str().ok_or("missing resume id")?;
            let file = runtime.join("routines").join(format!("{resume}.state"));
            let state: Value = serde_json::from_slice(&fs::read(&file)?)?;
            record(
                &mut checks,
                "routine HITL suspend persists next step and delivery",
                result["status"] == "suspended"
                    && state["current"] == "check"
                    && state["context"]["_hitl_delivery"]["delivered"] == true,
            )?;
            let (ok, out, _) = exec(
                &bin.join("agent-call-routine"),
                &["resume", resume, "verdict=approved"],
                &repo,
                &envs,
            )?;
            record(
                &mut checks,
                "routine HITL resume and persisted state cleanup",
                ok && serde_json::from_str::<Value>(&out)
                    .is_ok_and(|v| v["status"] == "completed" && v["message"] == "approved Alice")
                    && !file.exists(),
            )?;
            Ok(())
        })();
        let _ = fs::remove_file(fake_run);
        let _ = fs::remove_file(fake_hitl);
        inner
    })();
    for path in [&tool, &loop_path, &hitl] {
        let _ = fs::remove_file(path);
    }
    routine?;
    let mut disk_env = envs.clone();
    disk_env.push((
        "JELLY_FCIS_ISOLATION_ROOT".into(),
        sandbox.display().to_string(),
    ));
    let (ok, out, err) = exec(
        Path::new("cargo"),
        &[
            "test",
            "--lib",
            "fcis_real_download_persistence_and_artifact_finalize",
            "--quiet",
            "--locked",
            "--",
            "--ignored",
        ],
        &repo,
        &disk_env,
    )?;
    record(
        &mut checks,
        "downloads real persisted lifecycle, artifact registration and collisions",
        ok && out.contains("1 passed"),
    )
    .map_err(|e| format!("{e}: {err}"))?;
    oauth_integration(
        &mut children,
        &mut checks,
        &repo,
        &runtime,
        &bin,
        &sandbox,
        &envs,
        chrome_idx,
        &fixture,
    )?;
    require(
        checks.len() == 41,
        &format!("expected 41 checks, got {}", checks.len()),
    )?;
    fs::write(
        sandbox.join("integration-result.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":checks.len(),"checks":checks,"runtime":runtime,"uses_systemd":false}),
        )?,
    )?;
    println!(
        "SUMMARY {} integrated checks PASS; no systemd actions",
        checks.len()
    );
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn oauth_integration(
    children: &mut Children,
    checks: &mut Vec<String>,
    repo: &Path,
    runtime: &Path,
    bin: &Path,
    sandbox: &Path,
    envs: &[(String, String)],
    chrome_idx: usize,
    fixture: &str,
) -> Result<()> {
    let port = port()?;
    let token = "fcis-test-token-0123456789012345678901234567";
    let bootstrap = "fcis-bootstrap-012345678901234567890123456789";
    let password = "fcis-test-password-012345678901234567890123";
    let resource = "https://jelly-fcis-test.invalid/mcp";
    let mut mcp_env = envs.to_vec();
    for (k, v) in [
        ("JELLY_MCP_ADDR", format!("127.0.0.1:{port}")),
        ("JELLY_PUBLIC_URL", "https://jelly-fcis-test.invalid".into()),
        ("JELLY_OAUTH_CONSENT_MODE", "browser".into()),
        ("JELLY_OAUTH_PASSWORD", password.into()),
        ("JELLY_OAUTH_PUBLIC_CHATGPT_DCR", "true".into()),
        ("JELLY_MCP_TOKEN", token.into()),
        ("JELLY_BOOTSTRAP_SECRET", bootstrap.into()),
    ] {
        mcp_env.push((k.into(), v));
    }
    let start = |children: &mut Children| {
        children.launch(
            &bin.join("jelly-mcp"),
            &[],
            repo,
            &mcp_env,
            &sandbox.join("isolated-mcp.log"),
        )
    };
    let mut mcp_idx = start(children)?;
    wait_mcp(port)?;
    record(checks, "isolated MCP loopback server", true)?;
    let tools = rpc(port, "tools/list", json!({}), token)?;
    let names = tools["tools"].as_array().ok_or("MCP tools missing")?;
    record(
        checks,
        "MCP tools/list small-surface",
        names.iter().any(|v| v["name"] == "browser-call")
            && names.iter().any(|v| v["name"] == "browser-schema"),
    )?;
    let output = call(
        port,
        "evaluate-js",
        json!({"expression":"document.title"}),
        token,
    )?;
    record(
        checks,
        "MCP browser-call over HTTP",
        output["isError"] == false && output["structuredContent"]["ok"] == true,
    )?;
    let redirect = "http://127.0.0.1/fcis/callback";
    let (status, client) = post_json(
        port,
        "/register",
        json!({"client_name":"FCIS integration","redirect_uris":[redirect]}),
        Some(bootstrap),
    )?;
    let client_id = client["client_id"]
        .as_str()
        .ok_or("DCR client id missing")?
        .to_owned();
    record(
        checks,
        "OAuth DCR registration isolated",
        matches!(status, 200 | 201) && !client_id.is_empty(),
    )?;
    let verifier = "isolated-code-verifier-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let authorize = || -> Result<String> {
        let (status, location, _) = post_form(
            port,
            "/authorize",
            &[
                ("response_type", "code"),
                ("client_id", &client_id),
                ("redirect_uri", redirect),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge),
                ("resource", resource),
                ("scope", "jelly"),
                ("action", "approve"),
                ("password", password),
            ],
        )?;
        require(
            matches!(status, 302 | 303 | 307 | 308),
            "consent did not redirect",
        )?;
        let url = url::Url::parse(&location)?;
        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .ok_or("missing authorization code")?
            .1
            .into_owned();
        Ok(code)
    };
    let exchange = |code: &str, proof: &str| -> Result<(u16, Value)> {
        let (s, _, v) = post_form(
            port,
            "/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &client_id),
                ("redirect_uri", redirect),
                ("resource", resource),
                ("code", code),
                ("code_verifier", proof),
            ],
        )?;
        Ok((s, v))
    };
    let rotate = |refresh: &str, extra: &[(&str, &str)]| -> Result<(u16, Value)> {
        let mut args = vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh),
        ];
        args.extend(extra);
        let (s, _, v) = post_form(port, "/token", &args)?;
        Ok((s, v))
    };
    let bad = authorize()?;
    let (s, v) = exchange(&bad, "wrong-verifier")?;
    record(
        checks,
        "OAuth PKCE rejects invalid verifier",
        s == 400 && v["error"] == "invalid_grant",
    )?;
    let (s, v) = exchange(&bad, verifier)?;
    record(
        checks,
        "OAuth authorization code one-time use",
        s == 400 && v["error"] == "invalid_grant",
    )?;
    let (s, a) = exchange(&authorize()?, verifier)?;
    record(
        checks,
        "OAuth authorization_code exchange persisted",
        s == 200 && a["refresh_token"].is_string(),
    )?;
    let (s, b) = exchange(&authorize()?, verifier)?;
    record(
        checks,
        "OAuth second independent token family",
        s == 200 && b["refresh_token"].is_string(),
    )?;
    let ra = a["refresh_token"]
        .as_str()
        .ok_or("missing family A refresh")?;
    let rb = b["refresh_token"]
        .as_str()
        .ok_or("missing family B refresh")?;
    let (s, v) = rotate(ra, &[("scope", "not-jelly")])?;
    record(
        checks,
        "OAuth refresh scope binding",
        s == 400 && v["error"] == "invalid_scope",
    )?;
    let (s, v) = rotate(ra, &[("resource", "https://invalid.invalid/mcp")])?;
    record(
        checks,
        "OAuth refresh resource binding",
        s == 400 && v["error"] == "invalid_target",
    )?;
    let (s, rotated) = rotate(ra, &[])?;
    record(
        checks,
        "OAuth refresh token rotation",
        s == 200 && rotated["refresh_token"].as_str() != Some(ra),
    )?;
    let (s, v) = rotate(ra, &[])?;
    record(
        checks,
        "OAuth consumed-refresh replay detection",
        s == 400 && v["error"] == "invalid_grant",
    )?;
    let (s, v) = rotate(
        rotated["refresh_token"]
            .as_str()
            .ok_or("rotated refresh missing")?,
        &[],
    )?;
    record(
        checks,
        "OAuth replay revokes entire token family",
        s == 400 && v["error"] == "invalid_grant",
    )?;
    let (s, other) = rotate(rb, &[])?;
    record(
        checks,
        "OAuth unrelated family survives replay",
        s == 200 && other["refresh_token"].is_string(),
    )?;
    let persisted: Value = serde_json::from_slice(&fs::read(runtime.join("state/oauth.json"))?)?;
    record(
        checks,
        "OAuth durable store only under isolated runtime",
        persisted["clients"]
            .as_object()
            .is_some_and(|a| !a.is_empty())
            && persisted["refresh_tokens"]
                .as_object()
                .is_some_and(|a| !a.is_empty()),
    )?;
    let access = other["access_token"]
        .as_str()
        .ok_or("OAuth access token missing")?;
    record(
        checks,
        "MCP authorized with isolated OAuth access token",
        rpc(port, "tools/list", json!({}), access)?["tools"].is_array(),
    )?;
    children.stop(chrome_idx);
    let response = call(port, "read-page", json!({}), token)?;
    record(
        checks,
        "MCP cached browser session becomes unavailable after CDP loss",
        response["isError"] == true
            && response["structuredContent"]["error"]["kind"] == "browser_unavailable",
    )?;
    let (new_idx, profile) = browser(children, repo, sandbox, fixture, 2, envs)?;
    set_cdp(&profile, runtime)?;
    let response = call(port, "read-page", json!({}), token)?;
    record(
        checks,
        "MCP cached session reconnects to replacement isolated Chromium",
        response["isError"] == false && response["structuredContent"]["ok"] == true,
    )?;
    children.stop(mcp_idx);
    mcp_idx = start(children)?;
    wait_mcp(port)?;
    record(
        checks,
        "OAuth access token remains valid after isolated MCP restart",
        rpc(port, "tools/list", json!({}), access)?["tools"].is_array(),
    )?;
    let (s, v) = rotate(ra, &[])?;
    record(
        checks,
        "OAuth consumed refresh replay remains revoked after restart",
        s == 400 && v["error"] == "invalid_grant",
    )?;
    let (s, v) = rotate(
        other["refresh_token"]
            .as_str()
            .ok_or("independent refresh missing")?,
        &[],
    )?;
    record(
        checks,
        "OAuth independent refresh family survives store reload",
        s == 200 && v["refresh_token"].is_string(),
    )?;
    children.stop(new_idx);
    children.stop(mcp_idx);
    Ok(())
}
