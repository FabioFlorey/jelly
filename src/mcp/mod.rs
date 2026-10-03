use crate::{
    BROWSER_PID, BROWSER_READY, RECORDING_DIR, active_agent_catalog,
    config::config,
    mcp_auth::{AuthState, ConsentMode},
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

mod browser_session;
mod dispatch;
mod protocol;
mod system_tools;

#[cfg(test)]
use crate::{AgentToolCatalog, ErrorKind};
use dispatch::call_tool;
#[cfg(test)]
use dispatch::{
    ToolFailure, builtin_requires_persistent_mcp_session, execute_tool_from_catalog,
    failure_envelope, mcp_tools_from_catalog, success_envelope,
};
pub use dispatch::{mcp_tools, mcp_tools_for_config};
#[cfg(test)]
use protocol::DEFAULT_PROTOCOL_VERSION;
use protocol::{McpRequest, SERVER_NAME, error_response, initialize, success_response};
#[cfg(test)]
use system_tools::system_cli_args;

/// Build the authenticated MCP HTTP router from explicit server configuration.
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
        .route("/", get(index_page))
        .route("/index", get(index_page))
        .route("/dashboard", get(dashboard))
        .route("/status", get(status_json))
        .route("/status.json", get(status_json))
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/admin/cleanup-inactive", post(cleanup_inactive))
        .route("/mcp", post(handle_mcp))
        .merge(crate::mcp_auth::routes())
        .fallback(not_found)
        .with_state(state))
}

fn process_tree_size(root_pid: u32) -> usize {
    #[cfg(target_os = "linux")]
    {
        use std::collections::{HashMap, VecDeque};
        use std::fs;

        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let Ok(entries) = fs::read_dir("/proc") else {
            return 0;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            let Some(after_name) = stat.rsplit_once(") ").map(|(_, tail)| tail) else {
                continue;
            };
            let mut fields = after_name.split_whitespace();
            let _state = fields.next();
            let Some(ppid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
                continue;
            };
            children.entry(ppid).or_default().push(pid);
        }

        let mut count = 0usize;
        let mut queue = VecDeque::from([root_pid]);
        while let Some(parent) = queue.pop_front() {
            if let Some(kids) = children.get(&parent) {
                for &pid in kids {
                    count += 1;
                    queue.push_back(pid);
                }
            }
        }
        count
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root_pid;
        0
    }
}

fn browser_pid() -> Option<u32> {
    std::fs::read_to_string(BROWSER_PID.path())
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn listening_socket_inodes() -> std::collections::HashSet<String> {
    #[cfg(target_os = "linux")]
    {
        use std::collections::HashSet;
        use std::fs;

        let mut out = HashSet::new();
        for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            for line in text.lines().skip(1) {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() > 9 && fields[3] == "0A" {
                    out.insert(fields[9].to_string());
                }
            }
        }
        out
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::collections::HashSet::new()
    }
}

fn process_has_listening_socket(pid: u32, listening: &std::collections::HashSet<String>) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let Ok(target) = std::fs::read_link(entry.path()) else {
                return false;
            };
            let text = target.to_string_lossy();
            let Some(inode) = text
                .strip_prefix("socket:[")
                .and_then(|v| v.strip_suffix(']'))
            else {
                return false;
            };
            listening.contains(inode)
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, listening);
        false
    }
}

fn inactive_jelly_mcp_pids() -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;

        let current_pid = std::process::id();
        let Ok(current_exe) = fs::read_link("/proc/self/exe") else {
            return Vec::new();
        };
        let listening = listening_socket_inodes();
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        let mut pids = Vec::new();
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == current_pid {
                continue;
            }
            let Ok(exe) = fs::read_link(entry.path().join("exe")) else {
                continue;
            };
            if exe != current_exe {
                continue;
            }
            if !process_has_listening_socket(pid, &listening) {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        pids
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

fn runtime_status(state: &AuthState) -> Value {
    let browser_ready = BROWSER_READY.path().exists();
    let recording_active = RECORDING_DIR.path().join("active.json").exists();
    let (oauth_clients, oauth_tokens) = state.oauth_counts();
    let mcp_pid = std::process::id();
    let live_children = process_tree_size(mcp_pid);
    let browser_pid = browser_pid();
    let browser_tree = browser_pid.map(process_tree_size).unwrap_or(0);
    let inactive_mcp = inactive_jelly_mcp_pids();
    json!({
        "name": SERVER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "status": "ok",
        "ready": true,
        "runtime_root": config().paths.runtime_root.display().to_string(),
        "browser_ready": browser_ready,
        "recording_active": recording_active,
        "processes": {
            "mcp_pid": mcp_pid,
            "live_children": live_children,
            "browser_pid": browser_pid,
            "browser_descendants": browser_tree,
            "browser_tree_total": browser_pid.map(|_| browser_tree + 1).unwrap_or(0),
            "inactive_mcp_instances": inactive_mcp.len()
        },
        "oauth": {
            "configured": true,
            "consent_mode": state.consent_mode_name(),
            "public_chatgpt_dcr": state.public_chatgpt_dcr(),
            "owner_sessions": state.owner_session_count(),
            "clients": oauth_clients,
            "active_tokens": oauth_tokens
        },
        "endpoints": {
            "mcp": "/mcp",
            "health": "/health",
            "ready": "/ready",
            "status": "/status",
            "dashboard": "/dashboard",
            "pair": "/pair",
            "authorize": "/authorize"
        }
    })
}

async fn health() -> Json<Value> {
    Json(json!({
        "name": SERVER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "status": "ok"
    }))
}

async fn ready() -> Json<Value> {
    Json(json!({
        "name": SERVER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "ready": true
    }))
}

async fn status_json(State(state): State<AuthState>) -> Json<Value> {
    Json(runtime_status(&state))
}

async fn index_page() -> Html<String> {
    let content = r#"<h1>Jelly</h1><p>Local-first browser instrumentation and MCP service.</p>
<div class="grid">
<a class="card" href="/dashboard"><span class="k">Service</span><span class="v">Dashboard →</span></a>
<a class="card" href="/pair"><span class="k">OAuth</span><span class="v">Pair this browser →</span></a>
<a class="card" href="https://github.com/FabioFlorey/jelly"><span class="k">Source</span><span class="v">GitHub repository ↗</span></a>
<a class="card" href="https://fabioflorey.com/en/"><span class="k">Writing</span><span class="v">Fabio Florey blog ↗</span></a>
</div>
<div class="links"><a class="button secondary" href="/health">health</a><a class="button secondary" href="/ready">ready</a><a class="button secondary" href="/status">status</a></div>"#;
    Html(crate::mcp_auth::oauth_page("Jelly", content))
}

async fn cleanup_inactive(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if !state.has_owner_session(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"owner pairing required"})),
        )
            .into_response();
    }

    let candidates = inactive_jelly_mcp_pids();
    let current_exe = std::fs::read_link("/proc/self/exe").ok();
    let mut killed = Vec::new();
    for pid in candidates {
        let same_exe = current_exe
            .as_ref()
            .and_then(|exe| {
                std::fs::read_link(format!("/proc/{pid}/exe"))
                    .ok()
                    .map(|candidate| candidate == *exe)
            })
            .unwrap_or(false);
        if !same_exe {
            continue;
        }
        let status = std::process::Command::new("kill")
            .args(["-TERM", "--", &pid.to_string()])
            .status();
        if status.is_ok_and(|status| status.success()) {
            killed.push(pid);
        }
    }

    Json(json!({"killed":killed,"count":killed.len()})).into_response()
}

async fn dashboard(State(state): State<AuthState>) -> Html<String> {
    let status = runtime_status(&state);
    let browser_ready = status["browser_ready"].as_bool().unwrap_or(false);
    let recording_active = status["recording_active"].as_bool().unwrap_or(false);
    let oauth = &status["oauth"];
    let processes = &status["processes"];
    let content = format!(
        r#"<h1>Jelly dashboard</h1><p>Live runtime, browser and OAuth state.</p>
<section class="section">
<div class="section-head"><h2>Overview</h2><span class="section-note">service identity</span></div>
<div class="metrics">
<div class="card"><span class="k">Service</span><span id="service-status" class="v ok">online</span></div>
<div class="card"><span class="k">Version</span><span id="version" class="v">{version}</span></div>
<div class="card"><span class="k">Runtime root</span><span class="v"><code id="runtime">{runtime}</code></span></div>
</div>
</section>
<section class="section">
<div class="section-head"><h2>Browser & recording</h2><span class="section-note">interactive runtime</span></div>
<div class="metrics two">
<div class="card"><span class="k">Browser</span><span id="browser" class="v {browser_class}">{browser}</span></div>
<div class="card"><span class="k">Recording</span><span id="recording" class="v">{recording}</span></div>
</div>
</section>
<section class="section">
<div class="section-head"><h2>Processes</h2><span class="section-note">live process tree</span></div>
<div class="metrics">
<div class="card"><span class="k">MCP PID</span><span id="mcp-pid" class="v">{mcp_pid}</span></div>
<div class="card"><span class="k">Live child processes</span><span id="live-children" class="v">{live_children}</span></div>
<div class="card"><span class="k">Browser process tree</span><span id="browser-tree" class="v">{browser_tree}</span></div>
<div class="card"><span class="k">Inactive Jelly MCP</span><span id="inactive-mcp" class="v">{inactive_mcp}</span></div>
</div>
<div class="links"><button id="cleanup-inactive" class="button secondary" type="button">Kill inactive Jelly instances</button></div>
<p id="cleanup-state" class="refresh-state">Owner pairing required for cleanup.</p>
</section>
<section class="section">
<div class="section-head"><h2>OAuth & access</h2><span class="section-note">authorization state</span></div>
<div class="metrics">
<div class="card"><span class="k">OAuth</span><span id="oauth" class="v ok">configured · {consent}</span></div>
<div class="card"><span class="k">Owner sessions</span><span id="owners" class="v">{owners}</span></div>
<div class="card"><span class="k">OAuth clients</span><span id="clients" class="v">{clients}</span></div>
<div class="card"><span class="k">Active OAuth tokens</span><span id="tokens" class="v">{tokens}</span></div>
<div class="card"><span class="k">ChatGPT DCR</span><span id="dcr" class="v">{dcr}</span></div>
</div>
<div class="links"><a class="button secondary" href="/pair">Pair this browser</a></div>
</section>
<section class="section">
<div class="section-head"><h2>Endpoints</h2><span class="section-note">machine-readable state</span></div>
<div class="links"><a class="button" href="/health">health JSON</a><a class="button secondary" href="/ready">ready JSON</a><a class="button secondary" href="/status">status JSON</a></div>
<p id="refresh-state" class="refresh-state">Auto-refreshing every 2 seconds.</p>
</section>
<script>
const setText=(id,value)=>{{const el=document.getElementById(id);if(el)el.textContent=String(value)}};
async function refreshDashboard(){{
  const state=document.getElementById('refresh-state');
  try{{
    const response=await fetch('/status',{{cache:'no-store'}});
    if(!response.ok)throw new Error(`HTTP ${{response.status}}`);
    const data=await response.json();
    setText('service-status',data.status==='ok'?'online':data.status);
    setText('version',data.version);
    const browser=document.getElementById('browser');
    if(browser){{browser.textContent=data.browser_ready?'ready':'not running';browser.className=`v ${{data.browser_ready?'ok':'warn'}}`}}
    setText('recording',data.recording_active?'active':'idle');
    setText('mcp-pid',data.processes?.mcp_pid ?? 0);
    setText('live-children',data.processes?.live_children ?? 0);
    setText('browser-tree',data.processes?.browser_tree_total ?? 0);
    setText('inactive-mcp',data.processes?.inactive_mcp_instances ?? 0);
    setText('oauth',`configured · ${{data.oauth?.consent_mode ?? 'unknown'}}`);
    setText('owners',data.oauth?.owner_sessions ?? 0);
    setText('clients',data.oauth?.clients ?? 0);
    setText('tokens',data.oauth?.active_tokens ?? 0);
    setText('dcr',data.oauth?.public_chatgpt_dcr?'enabled':'disabled');
    setText('runtime',data.runtime_root ?? '');
    if(state)state.textContent=`Auto-refreshing every 2 seconds · updated ${{new Date().toLocaleTimeString()}}`;
  }}catch(error){{if(state)state.textContent=`Auto-refresh failed: ${{error.message}}`}}
}}
document.getElementById('cleanup-inactive')?.addEventListener('click',async()=>{{
  const state=document.getElementById('cleanup-state');
  if(!confirm('Kill Jelly MCP processes that are not listening on any socket?'))return;
  if(state)state.textContent='Cleaning up…';
  try{{
    const response=await fetch('/admin/cleanup-inactive',{{method:'POST',headers:{{'accept':'application/json'}}}});
    const data=await response.json().catch(()=>({{}}));
    if(!response.ok)throw new Error(data.error || `HTTP ${{response.status}}`);
    if(state)state.textContent=`Killed ${{data.killed?.length ?? 0}} inactive instance(s).`;
    await refreshDashboard();
  }}catch(error){{if(state)state.textContent=`Cleanup failed: ${{error.message}}`}}
}});
refreshDashboard();setInterval(refreshDashboard,2000);
</script>"#,
        version = env!("CARGO_PKG_VERSION"),
        browser_class = if browser_ready { "ok" } else { "warn" },
        browser = if browser_ready {
            "ready"
        } else {
            "not running"
        },
        recording = if recording_active { "active" } else { "idle" },
        mcp_pid = processes["mcp_pid"].as_u64().unwrap_or(0),
        live_children = processes["live_children"].as_u64().unwrap_or(0),
        browser_tree = processes["browser_tree_total"].as_u64().unwrap_or(0),
        inactive_mcp = processes["inactive_mcp_instances"].as_u64().unwrap_or(0),
        consent = oauth["consent_mode"].as_str().unwrap_or("unknown"),
        owners = oauth["owner_sessions"].as_u64().unwrap_or(0),
        clients = oauth["clients"].as_u64().unwrap_or(0),
        tokens = oauth["active_tokens"].as_u64().unwrap_or(0),
        dcr = if oauth["public_chatgpt_dcr"].as_bool().unwrap_or(false) {
            "enabled"
        } else {
            "disabled"
        },
        runtime = crate::mcp_auth::html_escape(status["runtime_root"].as_str().unwrap_or("")),
    );
    Html(crate::mcp_auth::oauth_page("Jelly dashboard", &content))
}

async fn not_found() -> (StatusCode, Html<String>) {
    (
        StatusCode::NOT_FOUND,
        Html(crate::mcp_auth::oauth_page(
            "Jelly · 404",
            r#"<h1>404</h1><p>This Jelly route does not exist.</p><div class="links"><a class="button" href="/">index</a><a class="button secondary" href="/dashboard">dashboard</a></div>"#,
        )),
    )
}

async fn handle_mcp(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !state.authorized(&headers) {
        return state.unauthorized();
    }

    let request = McpRequest::parse(&request);
    let Some(id) = request.id else {
        return StatusCode::ACCEPTED.into_response();
    };

    let result = match request.method.as_str() {
        "initialize" => Ok(initialize(&request.params)),
        "ping" => Ok(json!({})),
        "tools/list" => mcp_tools()
            .map(|tools| json!({"tools":tools}))
            .map_err(|message| (-32603, message)),
        "tools/call" => call_tool(&request.params).await,
        _ => Err((-32601, format!("method not found: {}", request.method))),
    };

    match result {
        Ok(result) => Json(success_response(id, result)).into_response(),
        Err((code, message)) => Json(error_response(id, code, message)).into_response(),
    }
}

#[cfg(test)]
mod tests;
