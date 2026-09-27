use crate::STATE_DIR;
use axum::{
    Form, Json, Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const SCOPE: &str = "jelly";
const CODE_TTL_SECS: u64 = 300;
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;
const OWNER_SESSION_TTL_SECS: u64 = 24 * 60 * 60;
const OAUTH_STATE_FILE: &str = "oauth.json";
const OWNER_COOKIE: &str = "jelly_owner";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentMode {
    Browser,
    Paired,
}

impl ConsentMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "browser" => Ok(Self::Browser),
            "paired" => Ok(Self::Paired),
            _ => Err("JELLY_OAUTH_CONSENT_MODE must be 'browser' or 'paired'".into()),
        }
    }
}

#[derive(Debug, Clone)]
struct Client {
    redirect_uris: Vec<String>,
}

#[derive(Debug, Clone)]
struct CodeGrant {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    resource: String,
    scope: String,
    expires_at: u64,
}

#[derive(Debug, Clone)]
struct AccessGrant {
    resource: String,
    scope: String,
    expires_at: u64,
}

#[derive(Default)]
struct AuthStore {
    clients: HashMap<String, Client>,
    codes: HashMap<String, CodeGrant>,
    tokens: HashMap<String, AccessGrant>,
}

#[derive(Clone)]
pub struct AuthState {
    static_token: Arc<str>,
    password: Arc<str>,
    bootstrap_secret: Arc<str>,
    consent_mode: ConsentMode,
    public_chatgpt_dcr: bool,
    public_url: Arc<str>,
    store: Arc<Mutex<AuthStore>>,
    owner_sessions: Arc<Mutex<HashMap<String, u64>>>,
}

impl AuthState {
    pub fn new(
        static_token: String,
        password: String,
        bootstrap_secret: String,
        consent_mode: ConsentMode,
        public_chatgpt_dcr: bool,
        public_url: String,
    ) -> Result<Self, String> {
        if static_token.len() < 32 {
            return Err("JELLY_MCP_TOKEN must be at least 32 bytes".into());
        }
        if consent_mode == ConsentMode::Browser && password.len() < 16 {
            return Err(
                "JELLY_OAUTH_PASSWORD must be at least 16 bytes in browser consent mode".into(),
            );
        }
        if bootstrap_secret.len() < 32 {
            return Err("JELLY_BOOTSTRAP_SECRET must be at least 32 bytes".into());
        }

        let public_url = public_url.trim_end_matches('/').to_owned();
        let parsed =
            Url::parse(&public_url).map_err(|e| format!("invalid JELLY_PUBLIC_URL: {e}"))?;
        if parsed.scheme() != "https"
            && !(parsed.scheme() == "http"
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")))
        {
            return Err(
                "JELLY_PUBLIC_URL must use https, except localhost development URLs".into(),
            );
        }

        let mut store = load_store().unwrap_or_default();
        let now = now();
        store.tokens.retain(|_, grant| grant.expires_at > now);

        Ok(Self {
            static_token: Arc::from(static_token),
            password: Arc::from(password),
            bootstrap_secret: Arc::from(bootstrap_secret),
            consent_mode,
            public_chatgpt_dcr,
            public_url: Arc::from(public_url),
            store: Arc::new(Mutex::new(store)),
            owner_sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub fn resource_url(&self) -> String {
        format!("{}/mcp", self.public_url)
    }

    pub fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(token) = bearer(headers) else {
            return false;
        };

        if constant_time_eq(token.as_bytes(), self.static_token.as_bytes()) {
            return true;
        }

        let now = now();
        let mut store = self.store.lock().unwrap();
        store.tokens.retain(|_, grant| grant.expires_at > now);
        store.tokens.get(token).is_some_and(|grant| {
            grant.resource == self.resource_url()
                && grant.scope.split_whitespace().any(|scope| scope == SCOPE)
                && grant.expires_at > now
        })
    }

    pub fn unauthorized(&self) -> Response {
        let metadata = format!(
            "{}/.well-known/oauth-protected-resource/mcp",
            self.public_url
        );
        let challenge = format!("Bearer resource_metadata=\"{metadata}\", scope=\"{SCOPE}\"");
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        if let Ok(value) = HeaderValue::from_str(&challenge) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        response
    }

    fn has_bootstrap_access(&self, headers: &HeaderMap) -> bool {
        bearer(headers).is_some_and(|token| {
            constant_time_eq(token.as_bytes(), self.bootstrap_secret.as_bytes())
        })
    }

    fn has_owner_session(&self, headers: &HeaderMap) -> bool {
        let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let Some(token) = cookie.split(';').find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (name == OWNER_COOKIE).then_some(value)
        }) else {
            return false;
        };

        let now = now();
        let mut sessions = self.owner_sessions.lock().unwrap();
        sessions.retain(|_, expiry| *expiry > now);
        sessions.get(token).is_some_and(|expiry| *expiry > now)
    }
}

pub fn routes() -> Router<AuthState> {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route("/brand/full-logo.png", get(brand_logo))
        .route("/brand/favicon.png", get(brand_favicon))
        .route("/register", post(register_client))
        .route("/authorize", get(authorize_get).post(authorize_post))
        .route("/pair", get(pair_get).post(pair_post))
        .route("/pair/status", get(pair_status))
        .route("/token", post(token))
}

async fn protected_resource_metadata(State(state): State<AuthState>) -> Json<Value> {
    Json(json!({
        "resource": state.resource_url(),
        "authorization_servers": [state.public_url()],
        "scopes_supported": [SCOPE]
    }))
}

async fn authorization_server_metadata(State(state): State<AuthState>) -> Json<Value> {
    let mut metadata = json!({
        "issuer": state.public_url(),
        "authorization_endpoint": format!("{}/authorize", state.public_url()),
        "token_endpoint": format!("{}/token", state.public_url()),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": [SCOPE],
        "token_endpoint_auth_methods_supported": ["none"],
        "authorization_response_iss_parameter_supported": true
    });
    if state.public_chatgpt_dcr {
        metadata["registration_endpoint"] = json!(format!("{}/register", state.public_url()));
    }
    Json(metadata)
}

async fn register_client(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(redirects) = body.get("redirect_uris").and_then(Value::as_array) else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "redirect_uris is required",
        );
    };

    let mut redirect_uris = Vec::new();
    for redirect in redirects {
        let Some(redirect) = redirect.as_str() else {
            return oauth_json_error(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "redirect_uris must contain strings",
            );
        };
        if !valid_redirect_uri(redirect) {
            return oauth_json_error(
                StatusCode::BAD_REQUEST,
                "invalid_redirect_uri",
                "redirect URI must use https or localhost http",
            );
        }
        redirect_uris.push(redirect.to_owned());
    }
    if redirect_uris.is_empty() {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "at least one redirect URI is required",
        );
    }

    let public_chatgpt_registration = state.public_chatgpt_dcr
        && redirect_uris.len() == 1
        && is_chatgpt_redirect(&redirect_uris[0]);

    if !public_chatgpt_registration && !state.has_bootstrap_access(&headers) {
        let mut response = oauth_json_error(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "bootstrap bearer token required for client registration",
        );
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }

    let client_id = {
        let mut store = state.store.lock().unwrap();
        if public_chatgpt_registration {
            if let Some((id, _)) = store
                .clients
                .iter()
                .find(|(_, client)| client.redirect_uris == redirect_uris)
            {
                return client_registration_response(id.clone(), redirect_uris);
            }
            let public_count = store
                .clients
                .values()
                .filter(|client| {
                    client.redirect_uris.len() == 1 && is_chatgpt_redirect(&client.redirect_uris[0])
                })
                .count();
            if public_count >= 64 {
                return oauth_json_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "registration_limit_reached",
                    "too many public ChatGPT clients",
                );
            }
        }

        let client_id = random_token(24);
        store.clients.insert(
            client_id.clone(),
            Client {
                redirect_uris: redirect_uris.clone(),
            },
        );
        if let Err(error) = persist_store(&store) {
            return oauth_json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", &error);
        }
        client_id
    };

    client_registration_response(client_id, redirect_uris)
}

async fn brand_logo() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(&include_bytes!("../assets/full-logo.png")[..]))
        .unwrap()
}

async fn brand_favicon() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(&include_bytes!("../assets/favicon.png")[..]))
        .unwrap()
}

async fn pair_get(State(state): State<AuthState>) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }
    Html(oauth_page(
        "Pair Jelly",
        "<h1>Pair this browser</h1><p>Establish this browser as the owner for OAuth approvals.</p><form method=\"post\" action=\"/pair\"><label>Bootstrap secret<input type=\"password\" name=\"secret\" autocomplete=\"current-password\" required autofocus></label><button type=\"submit\">Pair browser</button></form>",
    ))
    .into_response()
}

async fn pair_status(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !state.has_bootstrap_access(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let now = now();
    let mut sessions = state.owner_sessions.lock().unwrap();
    sessions.retain(|_, expiry| *expiry > now);
    Json(json!({"paired": !sessions.is_empty()})).into_response()
}

async fn pair_post(
    State(state): State<AuthState>,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }
    let secret = params.get("secret").map(String::as_str).unwrap_or("");
    if !constant_time_eq(secret.as_bytes(), state.bootstrap_secret.as_bytes()) {
        return (StatusCode::UNAUTHORIZED, "invalid bootstrap secret").into_response();
    }

    let token = random_token(32);
    state
        .owner_sessions
        .lock()
        .unwrap()
        .insert(token.clone(), now() + OWNER_SESSION_TTL_SECS);
    let secure = if state.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{OWNER_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={OWNER_SESSION_TTL_SECS}{secure}"
    );
    let mut response = Html(oauth_page(
        "Jelly paired",
        "<h1>Browser paired</h1><p>This browser can now approve Jelly OAuth requests. You can return to ChatGPT.</p><div class=\"success\">✓ Owner session active</div>",
    ))
    .into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn authorize_get(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = validate_authorize_request(&state, &params) {
        return *response;
    }
    if state.consent_mode == ConsentMode::Paired && !state.has_owner_session(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Html(oauth_page(
                "Pairing required",
                &format!(
                    "<h1>Owner pairing required</h1><p>Pair this browser first, then retry authorization.</p><a class=\"button\" href=\"{0}/pair\">Open pairing</a>",
                    html_escape(state.public_url())
                ),
            )),
        )
            .into_response();
    }

    let hidden = params
        .iter()
        .map(|(key, value)| {
            format!(
                "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
                html_escape(key),
                html_escape(value)
            )
        })
        .collect::<String>();

    let password = if state.consent_mode == ConsentMode::Browser {
        "<label>Authorization password <input type=\"password\" name=\"password\" autocomplete=\"current-password\" required></label>"
    } else {
        ""
    };

    Html(oauth_page(
        "Authorize Jelly",
        &format!(
            "<h1>Authorize access</h1><p>Allow this client to control your local Chromium browser through Jelly.</p><form method=\"post\" action=\"/authorize\">{hidden}{password}<div class=\"actions\"><button type=\"submit\" name=\"action\" value=\"approve\">Allow</button><button class=\"secondary\" type=\"submit\" name=\"action\" value=\"deny\">Deny</button></div></form>"
        ),
    ))
    .into_response()
}

async fn authorize_post(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    if let Err(response) = validate_authorize_request(&state, &params) {
        return *response;
    }

    if state.consent_mode == ConsentMode::Paired {
        if !state.has_owner_session(&headers) {
            return (
                StatusCode::FORBIDDEN,
                "owner pairing is required for OAuth consent",
            )
                .into_response();
        }
    } else {
        let password = params.get("password").map(String::as_str).unwrap_or("");
        if !constant_time_eq(password.as_bytes(), state.password.as_bytes()) {
            return (StatusCode::UNAUTHORIZED, "invalid authorization password").into_response();
        }
    }

    let redirect_uri = params["redirect_uri"].clone();
    if params.get("action").map(String::as_str) == Some("deny") {
        let mut redirect = Url::parse(&redirect_uri).unwrap();
        {
            let mut query = redirect.query_pairs_mut();
            query.append_pair("error", "access_denied");
            if let Some(value) = params.get("state") {
                query.append_pair("state", value);
            }
            query.append_pair("iss", state.public_url());
        }
        return Redirect::to(redirect.as_str()).into_response();
    }

    let client_id = params["client_id"].clone();
    let code_challenge = params["code_challenge"].clone();
    let resource = params["resource"].clone();
    let scope = params.get("scope").cloned().unwrap_or_else(|| SCOPE.into());
    let code = random_token(32);

    state.store.lock().unwrap().codes.insert(
        code.clone(),
        CodeGrant {
            client_id,
            redirect_uri: redirect_uri.clone(),
            code_challenge,
            resource,
            scope,
            expires_at: now() + CODE_TTL_SECS,
        },
    );

    let mut redirect = Url::parse(&redirect_uri).unwrap();
    {
        let mut query = redirect.query_pairs_mut();
        query.append_pair("code", &code);
        if let Some(value) = params.get("state") {
            query.append_pair("state", value);
        }
        query.append_pair("iss", state.public_url());
    }
    Redirect::to(redirect.as_str()).into_response()
}

async fn token(
    State(state): State<AuthState>,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    if params.get("grant_type").map(String::as_str) != Some("authorization_code") {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "only authorization_code is supported",
        );
    }

    let Some(code) = params.get("code") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "code is required",
        );
    };
    let Some(client_id) = params.get("client_id") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_id is required",
        );
    };
    let Some(redirect_uri) = params.get("redirect_uri") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect_uri is required",
        );
    };
    let Some(verifier) = params.get("code_verifier") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "code_verifier is required",
        );
    };
    let Some(resource) = params.get("resource") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "resource is required",
        );
    };

    let grant = state.store.lock().unwrap().codes.remove(code);
    let Some(grant) = grant else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "authorization code is invalid or already used",
        );
    };

    if grant.expires_at <= now()
        || grant.client_id != *client_id
        || grant.redirect_uri != *redirect_uri
        || grant.resource != *resource
        || grant.resource != state.resource_url()
        || !constant_time_eq(
            pkce_challenge(verifier).as_bytes(),
            grant.code_challenge.as_bytes(),
        )
    {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "authorization code validation failed",
        );
    }

    let access_token = random_token(32);
    let expires_at = now() + TOKEN_TTL_SECS;
    {
        let mut store = state.store.lock().unwrap();
        store.tokens.insert(
            access_token.clone(),
            AccessGrant {
                resource: grant.resource,
                scope: grant.scope.clone(),
                expires_at,
            },
        );
        if let Err(error) = persist_store(&store) {
            return oauth_json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", &error);
        }
    }

    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": TOKEN_TTL_SECS,
        "scope": grant.scope
    }))
    .into_response()
}

fn validate_authorize_request(
    state: &AuthState,
    params: &HashMap<String, String>,
) -> Result<(), Box<Response>> {
    if params.get("response_type").map(String::as_str) != Some("code") {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "response_type must be code").into_response(),
        ));
    }
    if params.get("code_challenge_method").map(String::as_str) != Some("S256") {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "PKCE S256 is required").into_response(),
        ));
    }
    let Some(client_id) = params.get("client_id") else {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "client_id is required").into_response(),
        ));
    };
    let Some(redirect_uri) = params.get("redirect_uri") else {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "redirect_uri is required").into_response(),
        ));
    };
    if params.get("code_challenge").is_none() {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "code_challenge is required").into_response(),
        ));
    }
    let resource_url = state.resource_url();
    if params.get("resource").map(String::as_str) != Some(resource_url.as_str()) {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "invalid resource").into_response(),
        ));
    }
    let scope = params.get("scope").map(String::as_str).unwrap_or(SCOPE);
    if !scope.split_whitespace().all(|item| item == SCOPE) {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "unsupported scope").into_response(),
        ));
    }

    let store = state.store.lock().unwrap();
    let Some(client) = store.clients.get(client_id) else {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "unknown client_id").into_response(),
        ));
    };
    if !client.redirect_uris.iter().any(|uri| uri == redirect_uri) {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "redirect_uri is not registered").into_response(),
        ));
    }
    Ok(())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        || (url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost")))
}

fn is_chatgpt_redirect(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || url.host_str() != Some("chatgpt.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }

    if url.path() == "/connector_platform_oauth_redirect" {
        return true;
    }

    let Some(suffix) = url.path().strip_prefix("/connector/oauth/") else {
        return false;
    };
    (6..=160).contains(&suffix.len())
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn oauth_page(title: &str, content: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title}</title><link rel="icon" type="image/png" href="/brand/favicon.png"><style>
:root{{--bg:#f5f5f2;--fg:#2d2224;--border:#d9d4c8;--muted:#817a70;--honey:#ffc107;--honey-dark:#c58f00;--panel:#fafaf7;}}
*{{box-sizing:border-box}}html{{background:var(--bg)}}body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:var(--bg);color:var(--fg);font:15px/1.65 ui-monospace,"SFMono-Regular",Consolas,"Liberation Mono",Menlo,monospace;padding:32px 20px}}body::after{{content:"";position:fixed;inset:0;pointer-events:none;opacity:.035;background-image:url("data:image/svg+xml,%3Csvg viewBox='0 0 180 180' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.9' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")}}main{{position:relative;width:min(620px,100%);background:var(--panel);border:1px solid var(--border);padding:28px 30px 30px}}.logo{{display:block;width:min(390px,88%);height:auto;margin:0 auto 28px}}h1{{font-family:ui-monospace,"SFMono-Regular",Consolas,monospace;font-size:1.12rem;line-height:1.5;letter-spacing:.01em;margin:0 0 10px;font-weight:700}}h1::before{{content:"> ";color:var(--honey-dark)}}p{{margin:0 0 24px;color:var(--muted)}}form{{display:grid;gap:18px;border-top:1px solid var(--border);padding-top:20px}}label{{display:grid;gap:8px;font-weight:700;font-size:.9rem}}input{{width:100%;font:inherit;color:var(--fg);background:var(--bg);border:1px solid var(--border);border-radius:0;padding:11px 12px;outline:none}}input:focus-visible{{border-color:var(--honey-dark);box-shadow:0 0 0 2px rgba(255,193,7,.2)}}button,.button{{appearance:none;display:inline-block;border:1px solid var(--fg);border-radius:0;padding:10px 14px;background:var(--fg);color:var(--bg);font:700 .86rem/1.2 ui-monospace,"SFMono-Regular",Consolas,monospace;text-decoration:none;cursor:pointer}}button:hover,.button:hover{{border-color:var(--honey-dark);background:var(--honey);color:#211a00}}.actions{{display:flex;gap:9px;flex-wrap:wrap}}.secondary{{background:transparent;color:var(--muted);border-color:var(--border)}}.secondary:hover{{color:var(--fg);background:transparent;border-color:var(--fg)}}.success{{margin-top:20px;padding:12px 0;border-top:1px solid var(--border);border-bottom:1px solid var(--border);color:var(--honey-dark);font-weight:700}}::selection{{background:var(--honey);color:#211a00}}@media(max-width:520px){{body{{padding:18px 12px}}main{{padding:22px 18px 24px}}.logo{{width:92%;margin-bottom:22px}}}}@media(prefers-reduced-motion:reduce){{*{{scroll-behavior:auto!important}}}}
</style></head><body><main><img class="logo" src="/brand/full-logo.png" alt="Jelly">{content}</main></body></html>"#,
        title = html_escape(title),
        content = content
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn client_registration_response(client_id: String, redirect_uris: Vec<String>) -> Response {
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "client_id_issued_at": now(),
            "redirect_uris": redirect_uris,
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        })),
    )
        .into_response()
}

fn oauth_json_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        Json(json!({
            "error": error,
            "error_description": description
        })),
    )
        .into_response()
}

fn store_path() -> String {
    format!("{STATE_DIR}/{OAUTH_STATE_FILE}")
}

fn load_store() -> Result<AuthStore, String> {
    let path = store_path();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AuthStore::default());
        }
        Err(error) => return Err(format!("failed to read OAuth state: {error}")),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse OAuth state: {error}"))?;
    let mut store = AuthStore::default();

    if let Some(clients) = value.get("clients").and_then(Value::as_object) {
        for (id, client) in clients {
            let redirects = client
                .get("redirect_uris")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !redirects.is_empty() {
                store.clients.insert(
                    id.clone(),
                    Client {
                        redirect_uris: redirects,
                    },
                );
            }
        }
    }

    if let Some(tokens) = value.get("tokens").and_then(Value::as_object) {
        for (token, grant) in tokens {
            let Some(resource) = grant.get("resource").and_then(Value::as_str) else {
                continue;
            };
            let Some(scope) = grant.get("scope").and_then(Value::as_str) else {
                continue;
            };
            let Some(expires_at) = grant.get("expires_at").and_then(Value::as_u64) else {
                continue;
            };
            store.tokens.insert(
                token.clone(),
                AccessGrant {
                    resource: resource.to_owned(),
                    scope: scope.to_owned(),
                    expires_at,
                },
            );
        }
    }
    Ok(store)
}

fn persist_store(store: &AuthStore) -> Result<(), String> {
    fs::create_dir_all(STATE_DIR)
        .map_err(|error| format!("failed to create OAuth state dir: {error}"))?;
    let clients = store
        .clients
        .iter()
        .map(|(id, client)| (id.clone(), json!({"redirect_uris":client.redirect_uris})))
        .collect::<Map<String, Value>>();
    let tokens = store
        .tokens
        .iter()
        .map(|(token, grant)| {
            (
                token.clone(),
                json!({
                    "resource":grant.resource,
                    "scope":grant.scope,
                    "expires_at":grant.expires_at
                }),
            )
        })
        .collect::<Map<String, Value>>();
    let bytes = serde_json::to_vec_pretty(&json!({"clients":clients,"tokens":tokens}))
        .map_err(|error| format!("failed to serialize OAuth state: {error}"))?;
    let path = store_path();
    let temp = format!("{path}.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| format!("failed to open OAuth state: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("failed to write OAuth state: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("failed to sync OAuth state: {error}"))?;
    fs::rename(&temp, &path).map_err(|error| format!("failed to install OAuth state: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AuthState {
        AuthState::new(
            "01234567890123456789012345678901".into(),
            "0123456789abcdef".into(),
            "abcdef0123456789abcdef0123456789".into(),
            ConsentMode::Browser,
            true,
            "https://jelly.example".into(),
        )
        .unwrap()
    }

    #[test]
    fn static_bearer_is_accepted() {
        let state = state();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer 01234567890123456789012345678901"),
        );
        assert!(state.authorized(&headers));
    }

    #[test]
    fn pkce_matches_rfc7636_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn redirect_validation_rejects_plain_http_remote_hosts() {
        assert!(valid_redirect_uri(
            "https://chatgpt.com/connector/oauth/FjKFnVbRJLSh"
        ));
        assert!(valid_redirect_uri("http://127.0.0.1:1234/callback"));
        assert!(!valid_redirect_uri("http://example.com/callback"));
    }

    #[test]
    fn public_dcr_is_chatgpt_only() {
        assert!(is_chatgpt_redirect(
            "https://chatgpt.com/connector/oauth/FjKFnVbRJLSh"
        ));
        assert!(is_chatgpt_redirect(
            "https://chatgpt.com/connector_platform_oauth_redirect"
        ));
        assert!(!is_chatgpt_redirect("https://example.com/callback"));
        assert!(!is_chatgpt_redirect(
            "https://chatgpt.com/other/oauth/FjKFnVbRJLSh"
        ));
        assert!(!is_chatgpt_redirect(
            "https://chatgpt.com/connector/oauth/FjKFnVbRJLSh/extra"
        ));
    }

    #[test]
    fn consent_modes_match_pilink_shape() {
        assert_eq!(ConsentMode::parse("browser").unwrap(), ConsentMode::Browser);
        assert_eq!(ConsentMode::parse("paired").unwrap(), ConsentMode::Paired);
        assert!(ConsentMode::parse("auto").is_err());
    }

    #[test]
    fn paired_mode_does_not_require_oauth_password() {
        assert!(
            AuthState::new(
                "01234567890123456789012345678901".into(),
                String::new(),
                "abcdef0123456789abcdef0123456789".into(),
                ConsentMode::Paired,
                false,
                "https://jelly.example".into(),
            )
            .is_ok()
        );
    }

    #[test]
    fn authorize_validation_accepts_registered_pkce_request() {
        let state = state();
        state.store.lock().unwrap().clients.insert(
            "client-1".into(),
            Client {
                redirect_uris: vec!["https://client.example/callback".into()],
            },
        );
        let params = HashMap::from([
            ("response_type".into(), "code".into()),
            ("client_id".into(), "client-1".into()),
            (
                "redirect_uri".into(),
                "https://client.example/callback".into(),
            ),
            ("code_challenge".into(), "challenge".into()),
            ("code_challenge_method".into(), "S256".into()),
            ("resource".into(), state.resource_url()),
            ("scope".into(), SCOPE.into()),
        ]);
        assert!(validate_authorize_request(&state, &params).is_ok());
    }

    #[test]
    fn authorize_validation_returns_boxed_bad_request() {
        let state = state();
        let params = HashMap::from([
            ("response_type".into(), "token".into()),
            ("client_id".into(), "missing".into()),
        ]);
        let response = validate_authorize_request(&state, &params).unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
