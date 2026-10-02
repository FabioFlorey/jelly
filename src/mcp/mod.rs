use crate::{
    active_agent_catalog,
    mcp_auth::{AuthState, ConsentMode},
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
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
