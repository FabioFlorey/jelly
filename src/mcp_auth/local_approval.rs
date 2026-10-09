use super::{
    SCOPE,
    dcr::is_chatgpt_redirect,
    html_escape,
    oauth::{complete_authorization, now, random_token},
    oauth_page,
    state::{AuthState, ConsentMode, LocalSetupSession, PendingApproval},
};
use axum::{
    Form, Json, Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde_json::json;
use std::collections::HashMap;
use url::Url;

const SETUP_WINDOW_TTL_SECS: u64 = 5 * 60;
const APPROVAL_TTL_SECS: u64 = 90;
const CHATGPT_URL: &str = "https://chatgpt.com/";

pub(crate) fn local_admin_routes() -> Router<AuthState> {
    Router::new()
        .merge(super::brand_routes())
        .route("/", get(local_home))
        .route("/dashboard", get(local_dashboard))
        .route("/connections", get(crate::mcp::connections_page))
        .route("/connect", get(connect_get))
        .route("/connect.js", get(connect_js))
        .route("/approve", get(approve_get).post(approve_post))
        .route("/admin/oauth/setup/open", post(open_setup_window))
        .route("/admin/oauth/setup/pending", get(pending_approval))
}

async fn local_home(State(state): State<AuthState>) -> Redirect {
    Redirect::to(&format!("{}/", state.public_url()))
}

async fn local_dashboard(State(state): State<AuthState>) -> Redirect {
    Redirect::to(&format!("{}/dashboard", state.public_url()))
}

async fn open_setup_window(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if !state.has_bootstrap_access(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let setup = match start_setup_session(&state) {
        Ok(setup) => setup,
        Err(response) => return *response,
    };
    let connect_url = connect_url(&state, &setup.token);
    Json(json!({
        "open": true,
        "expires_at": setup.expires_at,
        "expires_in": setup.expires_at.saturating_sub(now()),
        "connect_url": connect_url,
        "mcp_url": state.resource_url()
    }))
    .into_response()
}

async fn pending_approval(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if !state.has_bootstrap_access(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    cleanup_expired_state(&state);
    let pending = state.pending_approval_guard().clone();
    let setup_open = active_setup_session(&state).is_some();
    let (_, _, refresh_tokens) = state.oauth_counts();
    match pending {
        Some(pending) => Json(json!({
            "pending": true,
            "setup_open": setup_open,
            "connected": refresh_tokens > 0,
            "id": pending.id,
            "client_id": pending.client_id,
            "redirect_uri": pending.redirect_uri,
            "scope": pending.scope,
            "expires_at": pending.expires_at
        }))
        .into_response(),
        None => Json(json!({
            "pending": false,
            "setup_open": setup_open,
            "connected": refresh_tokens > 0
        }))
        .into_response(),
    }
}

async fn connect_get(
    State(state): State<AuthState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    cleanup_expired_state(&state);
    let setup_token = match params.get("setup") {
        Some(token) => token.clone(),
        None => {
            return local_page(
                StatusCode::OK,
                "Activate Jelly",
                "<h1>Activate Jelly</h1><p>Connection activation must be started by Jelly's installer, which creates a short-lived, authorized setup link. Visiting this page alone does not grant browser access.</p><div class=\"links\"><a class=\"button secondary\" href=\"/connections\">Connection methods</a></div><p class=\"hint\">Run <code>./scripts/dev.sh install</code> to open a new secure setup link.</p>",
                false,
            );
        }
    };
    if !setup_token_valid(&state, &setup_token) {
        return local_page(
            StatusCode::GONE,
            "Setup expired",
            "<h1>Setup expired</h1><p>This one-time Jelly setup link is no longer active. Run the Jelly installer again to start a new connection.</p>",
            false,
        );
    }

    let mcp_url = state.resource_url();
    let (chatgpt_url, direct_plugin) = chatgpt_destination();
    let status = if state.chatgpt_authorized() {
        "<div class=\"status ok\">ChatGPT OAuth authorization stored. Verify the active connection in ChatGPT.</div>"
    } else {
        "<div class=\"status\">Ready to connect.</div>"
    };
    let (intro, hint, button, copy_on_open) = if direct_plugin {
        (
            "Jelly is running locally. Open the Jelly plugin in ChatGPT to install or reconnect it. The bootstrap secret stays inside Jelly and is never placed in this page.",
            "The plugin already contains Jelly's MCP connection. When ChatGPT starts OAuth, Jelly redirects this browser through the local approval page and then back to ChatGPT.",
            "Open Jelly plugin in ChatGPT",
            "false",
        )
    } else {
        (
            "Jelly is running locally. Add this MCP URL to ChatGPT; the bootstrap secret stays inside Jelly and is never placed in this page.",
            "No Jelly plugin URL is configured yet, so this falls back to ChatGPT's custom MCP setup. Choose OAuth rather than a bearer token.",
            "Copy MCP URL & open ChatGPT",
            "true",
        )
    };
    let content = format!(
        "<h1>Connect Jelly to ChatGPT</h1>         <p>{intro}</p>         {status}         <label>MCP URL</label>         <div class=\"url-row\"><code id=\"mcp-url\">{mcp}</code><button type=\"button\" id=\"copy-mcp\">Copy</button></div>         <p class=\"hint\">{hint}</p>         <button class=\"primary\" type=\"button\" id=\"open-chatgpt\" data-copy-mcp=\"{copy_on_open}\" data-mcp-url=\"{mcp_attr}\" data-chatgpt-url=\"{chatgpt}\">{button}</button>         <p id=\"copy-status\" class=\"hint\" aria-live=\"polite\"></p>",
        intro = intro,
        status = status,
        mcp = html_escape(&mcp_url),
        hint = hint,
        copy_on_open = copy_on_open,
        mcp_attr = html_escape(&mcp_url),
        chatgpt = html_escape(&chatgpt_url),
        button = button,
    );
    local_page(StatusCode::OK, "Connect Jelly", &content, true)
}

async fn connect_js() -> Response {
    let script = r#"
(() => {
  const copy = async (value) => {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(value);
      return;
    }
    const input = document.createElement('textarea');
    input.value = value;
    input.setAttribute('readonly', '');
    input.style.position = 'fixed';
    input.style.opacity = '0';
    document.body.appendChild(input);
    input.select();
    document.execCommand('copy');
    input.remove();
  };

  const status = document.getElementById('copy-status');
  const copyButton = document.getElementById('copy-mcp');
  const openButton = document.getElementById('open-chatgpt');
  const mcp = document.getElementById('mcp-url');

  copyButton?.addEventListener('click', async () => {
    try {
      await copy(mcp.textContent);
      status.textContent = 'MCP URL copied.';
    } catch {
      status.textContent = 'Copy failed. Select the MCP URL above manually.';
    }
  });

  openButton?.addEventListener('click', async () => {
    if (openButton.dataset.copyMcp === 'true') {
      try {
        await copy(openButton.dataset.mcpUrl);
        status.textContent = 'MCP URL copied. Opening ChatGPT…';
      } catch {
        status.textContent = 'Opening ChatGPT. Copy the MCP URL above manually if needed.';
      }
    } else {
      status.textContent = 'Opening the Jelly plugin in ChatGPT…';
    }
    window.location.assign(openButton.dataset.chatgptUrl);
  });
})();
"#;
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )
        .header(header::CACHE_CONTROL, "no-store, max-age=0")
        .body(Body::from(script))
        .unwrap()
}

pub(super) fn redirect_to_local_chatgpt_approval(
    state: &AuthState,
    params: &HashMap<String, String>,
) -> Option<Response> {
    if !eligible_for_local_approval(state, params) {
        return None;
    }

    cleanup_expired_state(state);
    let setup = active_setup_session(state)?;
    let approval = PendingApproval {
        id: random_token(18),
        client_id: params["client_id"].clone(),
        redirect_uri: params["redirect_uri"].clone(),
        scope: params.get("scope").cloned().unwrap_or_else(|| SCOPE.into()),
        expires_at: now() + APPROVAL_TTL_SECS,
        setup_token: setup.token.clone(),
        params: params.clone(),
    };

    {
        let mut pending = state.pending_approval_guard();
        if pending.is_some() {
            return Some(
                (
                    StatusCode::CONFLICT,
                    "another local OAuth approval is already pending",
                )
                    .into_response(),
            );
        }
        *pending = Some(approval.clone());
    }

    let mut target = Url::parse(state.local_admin_url()).ok()?;
    target.set_path("/approve");
    target.set_query(None);
    {
        let mut query = target.query_pairs_mut();
        query.append_pair("setup", &setup.token);
        query.append_pair("id", &approval.id);
    }
    Some(Redirect::to(target.as_str()).into_response())
}

async fn approve_get(
    State(state): State<AuthState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    cleanup_expired_state(&state);
    let Some(setup_token) = params.get("setup") else {
        return local_page(
            StatusCode::BAD_REQUEST,
            "Approval link invalid",
            "<h1>Approval link invalid</h1><p>This approval URL is missing its one-time setup token.</p>",
            false,
        );
    };
    let Some(id) = params.get("id") else {
        return local_page(
            StatusCode::BAD_REQUEST,
            "Approval link invalid",
            "<h1>Approval link invalid</h1><p>This approval URL is missing its one-time request ID.</p>",
            false,
        );
    };
    if !setup_token_valid(&state, setup_token) {
        return local_page(
            StatusCode::GONE,
            "Approval expired",
            "<h1>Approval expired</h1><p>This Jelly setup session is no longer active.</p>",
            false,
        );
    }

    let pending = state.pending_approval_guard().clone();
    let Some(pending) = pending else {
        return local_page(
            StatusCode::GONE,
            "Approval expired",
            "<h1>Approval expired</h1><p>There is no pending ChatGPT connection request. Start the connection again from ChatGPT.</p>",
            false,
        );
    };
    if pending.id != *id || pending.setup_token != *setup_token {
        return local_page(
            StatusCode::UNAUTHORIZED,
            "Approval link invalid",
            "<h1>Approval link invalid</h1><p>This one-time approval URL does not match the pending request.</p>",
            false,
        );
    }

    let content = format!(
        "<h1>Allow ChatGPT to use Jelly?</h1>         <p>This is the connection request you just started in ChatGPT.</p>         <dl class=\"details\">           <dt>Client</dt><dd>{}</dd>           <dt>Callback</dt><dd>{}</dd>           <dt>Access</dt><dd>{}</dd>         </dl>         <form class=\"flow-actions\" method=\"post\" action=\"/approve\">           <input type=\"hidden\" name=\"setup\" value=\"{}\">           <input type=\"hidden\" name=\"id\" value=\"{}\">           <button class=\"primary\" type=\"submit\" name=\"action\" value=\"approve\">Yes, allow</button>           <button type=\"submit\" name=\"action\" value=\"deny\">No, deny</button>         </form>         <p class=\"hint\">After you choose, Jelly sends this browser directly back to ChatGPT.</p>",
        html_escape(&pending.client_id),
        html_escape(&pending.redirect_uri),
        html_escape(&pending.scope),
        html_escape(setup_token),
        html_escape(id),
    );
    local_page(StatusCode::OK, "Approve Jelly", &content, false)
}

fn approval_error(status: StatusCode, message: &str) -> Response {
    local_page(
        status,
        "Jelly approval",
        &format!(
            "<h1>Approval unavailable</h1><p>{}</p><a class=\"button secondary\" href=\"/connections\">Connections</a>",
            html_escape(message)
        ),
        false,
    )
}

async fn approve_post(
    State(state): State<AuthState>,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    cleanup_expired_state(&state);
    let Some(setup_token) = params.get("setup") else {
        return approval_error(StatusCode::BAD_REQUEST, "Setup is required.");
    };
    let Some(id) = params.get("id") else {
        return approval_error(StatusCode::BAD_REQUEST, "Request ID is required.");
    };
    let approved = match params.get("action").map(String::as_str) {
        Some("approve") => true,
        Some("deny") => false,
        _ => return approval_error(StatusCode::BAD_REQUEST, "Choose Allow or Deny."),
    };
    if !setup_token_valid(&state, setup_token) {
        return approval_error(
            StatusCode::GONE,
            "Setup session expired. Start the connection again from Jelly.",
        );
    }

    let pending = {
        let mut pending = state.pending_approval_guard();
        let Some(current) = pending.as_ref() else {
            return approval_error(StatusCode::GONE, "No OAuth approval is pending.");
        };
        if current.id != *id || current.setup_token != *setup_token {
            return approval_error(StatusCode::UNAUTHORIZED, "Approval request does not match.");
        }
        pending.take().unwrap()
    };

    *state.local_approval_window_guard() = 0;
    *state.local_setup_guard() = None;
    complete_authorization(&state, &pending.params, approved)
}

fn eligible_for_local_approval(state: &AuthState, params: &HashMap<String, String>) -> bool {
    if state.consent_mode != ConsentMode::Paired
        || !state.public_chatgpt_dcr
        || !local_approval_window_open(state)
        || active_setup_session(state).is_none()
    {
        return false;
    }

    let Some(client_id) = params.get("client_id") else {
        return false;
    };
    let Some(redirect_uri) = params.get("redirect_uri") else {
        return false;
    };
    if !is_chatgpt_redirect(redirect_uri) {
        return false;
    }

    let scope = params.get("scope").map(String::as_str).unwrap_or(SCOPE);
    if scope.split_whitespace().any(|item| item != SCOPE) {
        return false;
    }

    let store = state.store_guard();
    store.clients.get(client_id).is_some_and(|client| {
        client.redirect_uris.len() == 1 && client.redirect_uris[0] == *redirect_uri
    })
}

fn start_setup_session(state: &AuthState) -> Result<LocalSetupSession, Box<Response>> {
    if state.consent_mode != ConsentMode::Paired || !state.public_chatgpt_dcr {
        return Err(Box::new(
            (
                StatusCode::CONFLICT,
                "browser connect requires paired consent with public ChatGPT DCR enabled",
            )
                .into_response(),
        ));
    }

    cleanup_expired_state(state);
    if state.pending_approval_guard().is_some() {
        return Err(Box::new(
            (StatusCode::CONFLICT, "an OAuth approval is already pending").into_response(),
        ));
    }
    if let Some(existing) = active_setup_session(state) {
        return Ok(existing);
    }

    let expires_at = now() + SETUP_WINDOW_TTL_SECS;
    let setup = LocalSetupSession {
        token: random_token(24),
        expires_at,
    };
    *state.local_approval_window_guard() = expires_at;
    *state.local_setup_guard() = Some(setup.clone());
    Ok(setup)
}

fn chatgpt_destination() -> (String, bool) {
    let plugin_url = std::env::var("JELLY_CHATGPT_PLUGIN_URL")
        .ok()
        .filter(|value| {
            Url::parse(value).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str() == Some("chatgpt.com")
                    && url.path().starts_with("/plugins/")
            })
        });
    match plugin_url {
        Some(url) => (url, true),
        None => (CHATGPT_URL.into(), false),
    }
}

fn connect_url(state: &AuthState, setup_token: &str) -> String {
    let mut url = Url::parse(state.local_admin_url()).expect("validated local admin URL");
    url.set_path("/connect");
    url.set_query(None);
    url.query_pairs_mut().append_pair("setup", setup_token);
    url.into()
}

fn active_setup_session(state: &AuthState) -> Option<LocalSetupSession> {
    let current = now();
    let mut setup = state.local_setup_guard();
    if setup
        .as_ref()
        .is_some_and(|session| session.expires_at <= current)
    {
        *setup = None;
        *state.local_approval_window_guard() = 0;
    }
    setup.clone()
}

fn setup_token_valid(state: &AuthState, token: &str) -> bool {
    active_setup_session(state).is_some_and(|session| session.token == token)
}

fn local_approval_window_open(state: &AuthState) -> bool {
    let current = now();
    let mut expires_at = state.local_approval_window_guard();
    if *expires_at <= current {
        *expires_at = 0;
        return false;
    }
    true
}

fn cleanup_expired_state(state: &AuthState) {
    let current = now();
    let mut pending = state.pending_approval_guard();
    if pending
        .as_ref()
        .is_some_and(|approval| approval.expires_at <= current)
    {
        *pending = None;
    }
    drop(pending);
    let _ = active_setup_session(state);
}

fn local_page(status: StatusCode, title: &str, content: &str, with_script: bool) -> Response {
    let script = if with_script {
        "<script src=\"/connect.js\" defer></script>"
    } else {
        ""
    };
    let html = oauth_page(title, &format!("{content}{script}"));
    let mut response = (status, Html(html)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    let csp = if with_script {
        "default-src 'none'; style-src 'self'; img-src 'self' data:; font-src 'self'; script-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
    } else {
        "default-src 'none'; style-src 'self'; img-src 'self' data:; font-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
    };
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(csp),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_auth::storage::Client;

    fn paired_state() -> AuthState {
        AuthState::new(
            "01234567890123456789012345678901".into(),
            String::new(),
            "abcdef0123456789abcdef0123456789".into(),
            ConsentMode::Paired,
            true,
            "https://jelly.example".into(),
        )
        .unwrap()
    }

    fn request(state: &AuthState) -> HashMap<String, String> {
        let redirect = "https://chatgpt.com/connector_platform_oauth_redirect".to_owned();
        state.store_guard().clients.insert(
            "client-1".into(),
            Client {
                redirect_uris: vec![redirect.clone()],
            },
        );
        HashMap::from([
            ("response_type".into(), "code".into()),
            ("client_id".into(), "client-1".into()),
            ("redirect_uri".into(), redirect),
            ("code_challenge".into(), "challenge".into()),
            ("code_challenge_method".into(), "S256".into()),
            ("resource".into(), state.resource_url()),
            ("scope".into(), SCOPE.into()),
            ("state".into(), "chatgpt-state".into()),
        ])
    }

    fn open_setup(state: &AuthState) -> String {
        let token = "setup-token".to_owned();
        let expires_at = now() + 60;
        *state.local_approval_window_guard() = expires_at;
        *state.local_setup_guard() = Some(LocalSetupSession {
            token: token.clone(),
            expires_at,
        });
        token
    }

    #[test]
    fn local_approval_requires_an_explicit_setup_window() {
        let state = paired_state();
        let params = request(&state);
        assert!(!eligible_for_local_approval(&state, &params));

        open_setup(&state);
        assert!(eligible_for_local_approval(&state, &params));
    }

    #[test]
    fn local_approval_rejects_non_chatgpt_redirects() {
        let state = paired_state();
        state.store_guard().clients.insert(
            "client-1".into(),
            Client {
                redirect_uris: vec!["https://example.com/callback".into()],
            },
        );
        open_setup(&state);
        let params = HashMap::from([
            ("client_id".into(), "client-1".into()),
            ("redirect_uri".into(), "https://example.com/callback".into()),
        ]);
        assert!(!eligible_for_local_approval(&state, &params));
    }

    #[test]
    fn local_authorize_redirect_targets_loopback_approve() {
        let state = paired_state();
        let params = request(&state);
        let setup = open_setup(&state);

        let response = redirect_to_local_chatgpt_approval(&state, &params).unwrap();
        assert!(response.status().is_redirection());
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        assert!(location.starts_with("http://127.0.0.1:8788/approve?"));
        assert!(location.contains(&format!("setup={setup}")));
        assert!(location.contains("id="));
    }

    #[tokio::test]
    async fn bare_connect_does_not_start_an_oauth_setup_session() {
        let state = paired_state();
        let response = connect_get(State(state.clone()), Query(HashMap::new())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("must be started by Jelly"));
        assert!(state.local_setup_guard().is_none());
    }

    #[tokio::test]
    async fn only_bootstrap_authorized_requests_can_open_setup() {
        let state = paired_state();
        let denied = open_setup_window(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert!(state.local_setup_guard().is_none());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", state.bootstrap_secret)
                .parse()
                .unwrap(),
        );
        let approved = open_setup_window(State(state.clone()), headers).await;
        assert_eq!(approved.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(approved.into_body(), 128 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("connect_url"));
        assert!(state.local_setup_guard().is_some());
    }

    #[tokio::test]
    async fn local_pages_share_jelly_brand_assets_and_secure_policy() {
        let response = local_page(StatusCode::OK, "Jelly page", "<h1>Jelly</h1>", true);
        assert_eq!(response.status(), StatusCode::OK);
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(csp.contains("style-src 'self'"));
        assert!(csp.contains("font-src 'self'"));
        assert!(csp.contains("script-src 'self'"));
        let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("/brand/full-logo.png"));
        assert!(html.contains("/brand/jelly.css"));
        assert!(html.contains("/connections"));
        assert!(html.contains("/connect.js"));
    }

    #[tokio::test]
    async fn connect_page_exposes_mcp_url_without_bootstrap_secret() {
        let state = paired_state();
        let setup = open_setup(&state);
        let response = connect_get(
            State(state),
            Query(HashMap::from([("setup".into(), setup)])),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("https://jelly.example/mcp"));
        assert!(html.contains("id=\"open-chatgpt\""));
        assert!(!html.contains("abcdef0123456789abcdef0123456789"));
    }

    #[tokio::test]
    async fn approve_page_exposes_yes_and_no_controls_for_matching_request() {
        let state = paired_state();
        let setup = open_setup(&state);
        let params = request(&state);
        *state.pending_approval_guard() = Some(PendingApproval {
            id: "approval-2".into(),
            client_id: "client-1".into(),
            redirect_uri: "https://chatgpt.com/connector_platform_oauth_redirect".into(),
            scope: SCOPE.into(),
            expires_at: now() + 60,
            setup_token: setup.clone(),
            params,
        });

        let response = approve_get(
            State(state),
            Query(HashMap::from([
                ("setup".into(), setup),
                ("id".into(), "approval-2".into()),
            ])),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("Yes, allow"));
        assert!(html.contains("No, deny"));
        assert!(html.contains("class=\"details\""));
        assert!(html.contains("class=\"flow-actions\""));
        assert!(html.contains("/brand/jelly.css"));
        assert!(html.contains("After you choose"));
    }

    #[tokio::test]
    async fn approve_post_consumes_request_and_redirects_back_to_chatgpt() {
        let state = paired_state();
        let setup = open_setup(&state);
        let params = request(&state);
        *state.pending_approval_guard() = Some(PendingApproval {
            id: "approval-3".into(),
            client_id: "client-1".into(),
            redirect_uri: "https://chatgpt.com/connector_platform_oauth_redirect".into(),
            scope: SCOPE.into(),
            expires_at: now() + 60,
            setup_token: setup.clone(),
            params,
        });

        let response = approve_post(
            State(state.clone()),
            Form(HashMap::from([
                ("setup".into(), setup),
                ("id".into(), "approval-3".into()),
                ("action".into(), "approve".into()),
            ])),
        )
        .await;

        assert!(response.status().is_redirection());
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        assert!(location.starts_with("https://chatgpt.com/connector_platform_oauth_redirect?"));
        assert!(location.contains("code="));
        assert!(location.contains("state=chatgpt-state"));
        assert!(state.pending_approval_guard().is_none());
        assert!(state.local_setup_guard().is_none());
    }

    #[test]
    fn expired_pending_approval_is_cleaned_up() {
        let state = paired_state();
        let setup = open_setup(&state);
        *state.pending_approval_guard() = Some(PendingApproval {
            id: "pending".into(),
            client_id: "client".into(),
            redirect_uri: "https://chatgpt.com/connector_platform_oauth_redirect".into(),
            scope: SCOPE.into(),
            expires_at: now().saturating_sub(1),
            setup_token: setup,
            params: HashMap::new(),
        });

        cleanup_expired_state(&state);
        assert!(state.pending_approval_guard().is_none());
    }
}
