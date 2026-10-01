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
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const SCOPE: &str = "jelly";
const CODE_TTL_SECS: u64 = 300;
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;
const OWNER_SESSION_TTL_SECS: u64 = 24 * 60 * 60;
const OWNER_COOKIE: &str = "jelly_owner";

mod dcr;
mod pages;
mod state;
mod storage;

use dcr::{authorization_server_metadata, protected_resource_metadata, register_client};
#[cfg(test)]
use dcr::{is_chatgpt_redirect, valid_redirect_uri};
use pages::{html_escape, oauth_json_error, oauth_page};
pub use state::{AuthState, ConsentMode};
#[cfg(test)]
use storage::Client;
use storage::{AccessGrant, CodeGrant, persist_store};

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

async fn brand_logo() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(
            &include_bytes!("../../assets/full-logo.png")[..],
        ))
        .unwrap()
}

async fn brand_favicon() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(&include_bytes!("../../assets/favicon.png")[..]))
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
