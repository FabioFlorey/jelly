use super::pages::{html_escape, oauth_json_error, oauth_page};
use super::state::{AuthState, ConsentMode};
use super::storage::{
    AccessGrant, AuthStore, CodeGrant, ConsumedRefreshGrant, RefreshGrant, persist_store,
};
use super::{OWNER_COOKIE, SCOPE};
use axum::{
    Form, Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
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

const CODE_TTL_SECS: u64 = 300;
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;
const REFRESH_TOKEN_TTL_SECS: u64 = 30 * 24 * 60 * 60;
const OWNER_SESSION_TTL_SECS: u64 = 24 * 60 * 60;
const PAIR_CODE_TTL_SECS: u64 = 5 * 60;

pub(super) async fn pair_code(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !state.has_bootstrap_access(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let code = random_token(32);
    let expires_at = now() + PAIR_CODE_TTL_SECS;
    {
        let mut codes = state.pair_codes_guard();
        let current = now();
        codes.retain(|_, expiry| *expiry > current);
        codes.insert(code.clone(), expires_at);
    }

    Json(json!({
        "pair_url": format!("{}/pair?code={}", state.public_url(), code),
        "expires_in": PAIR_CODE_TTL_SECS
    }))
    .into_response()
}

pub(super) async fn pair_get(
    State(state): State<AuthState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(code) = params.get("code") {
        let valid = {
            let mut codes = state.pair_codes_guard();
            let current = now();
            codes.retain(|_, expiry| *expiry > current);
            codes.remove(code).is_some_and(|expiry| expiry > current)
        };
        if !valid {
            return (
                StatusCode::UNAUTHORIZED,
                Html(oauth_page(
                    "Pairing link expired",
                    "<h1>Pairing link invalid or expired</h1><p>Generate a new pairing link from the Jelly installer.</p>",
                )),
            )
                .into_response();
        }
        return paired_response(&state);
    }

    Html(oauth_page(
        "Pair Jelly",
        "<h1>Pair this browser</h1><p>Establish this browser as the owner for OAuth approvals.</p><form method=\"post\" action=\"/pair\"><label>Bootstrap secret<input type=\"password\" name=\"secret\" autocomplete=\"current-password\" required autofocus></label><button type=\"submit\">Pair browser</button></form>",
    ))
    .into_response()
}

pub(super) async fn pair_status(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !state.has_bootstrap_access(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let now = now();
    let mut sessions = state.owner_sessions_guard();
    sessions.retain(|_, expiry| *expiry > now);
    Json(json!({"paired": !sessions.is_empty()})).into_response()
}

fn oauth_ui_error(status: StatusCode, title: &str, message: &str) -> Response {
    (
        status,
        Html(oauth_page(
            title,
            &format!("<h1>{}</h1><p>{}</p><div class=\"links\"><a class=\"button secondary\" href=\"/connections\">Connections</a></div>", html_escape(title), html_escape(message)),
        )),
    )
        .into_response()
}

pub(super) async fn pair_post(
    State(state): State<AuthState>,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    if state.consent_mode != ConsentMode::Paired {
        return StatusCode::NOT_FOUND.into_response();
    }
    let secret = params.get("secret").map(String::as_str).unwrap_or("");
    if !constant_time_eq(secret.as_bytes(), state.bootstrap_secret.as_bytes()) {
        return oauth_ui_error(
            StatusCode::UNAUTHORIZED,
            "Pairing failed",
            "Invalid bootstrap secret. Please try again.",
        );
    }

    paired_response(&state)
}

fn paired_response(state: &AuthState) -> Response {
    let token = random_token(32);
    state
        .owner_sessions_guard()
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

pub(super) async fn authorize_get(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = validate_authorize_request(&state, &params) {
        return oauth_ui_error(
            response.status(),
            "Invalid OAuth request",
            "Authorization parameters are missing or invalid. Start the connection again from your MCP client.",
        );
    }
    if state.consent_mode == ConsentMode::Paired && !state.has_owner_session(&headers) {
        if let Some(response) =
            super::local_approval::redirect_to_local_chatgpt_approval(&state, &params)
        {
            return response;
        }
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

pub(super) async fn authorize_post(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    if let Err(response) = validate_authorize_request(&state, &params) {
        return oauth_ui_error(
            response.status(),
            "Invalid OAuth request",
            "Authorization parameters are missing or invalid. Start the connection again from your MCP client.",
        );
    }

    let action = match authorize_action(&params) {
        Ok(action) => action,
        Err(message) => {
            return oauth_ui_error(StatusCode::BAD_REQUEST, "Invalid decision", message);
        }
    };

    if state.consent_mode == ConsentMode::Paired {
        if !state.has_owner_session(&headers) {
            return oauth_ui_error(
                StatusCode::FORBIDDEN,
                "Owner pairing required",
                "Pair this browser as Jelly's owner before approving OAuth access.",
            );
        }
    } else {
        let password = params.get("password").map(String::as_str).unwrap_or("");
        if !constant_time_eq(password.as_bytes(), state.password.as_bytes()) {
            return oauth_ui_error(
                StatusCode::UNAUTHORIZED,
                "Authorization failed",
                "Invalid authorization password. Please try again.",
            );
        }
    }

    complete_authorization(&state, &params, action == "approve")
}

pub(super) fn complete_authorization(
    state: &AuthState,
    params: &HashMap<String, String>,
    approved: bool,
) -> Response {
    let redirect_uri = params["redirect_uri"].clone();
    if !approved {
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

    state.store_guard().codes.insert(
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

pub(super) async fn token(
    State(state): State<AuthState>,
    Form(params): Form<HashMap<String, String>>,
) -> Response {
    match params.get("grant_type").map(String::as_str) {
        Some("authorization_code") => authorization_code_token(&state, &params),
        Some("refresh_token") => refresh_token(&state, &params),
        _ => oauth_json_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "grant_type must be authorization_code or refresh_token",
        ),
    }
}

fn authorization_code_token(state: &AuthState, params: &HashMap<String, String>) -> Response {
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

    let grant = state.store_guard().codes.remove(code);
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

    let current = now();
    let access_token = random_token(32);
    let refresh_token = random_token(32);
    let refresh_token_hash = token_hash(&refresh_token);
    let family_id = random_token(18);
    let access_expires_at = current + TOKEN_TTL_SECS;
    let refresh_expires_at = current + REFRESH_TOKEN_TTL_SECS;

    {
        let mut store = state.store_guard();
        store.tokens.insert(
            access_token.clone(),
            AccessGrant {
                resource: grant.resource.clone(),
                scope: grant.scope.clone(),
                expires_at: access_expires_at,
                family_id: Some(family_id.clone()),
            },
        );
        store.refresh_tokens.insert(
            refresh_token_hash,
            RefreshGrant {
                client_id: grant.client_id,
                resource: grant.resource,
                scope: grant.scope.clone(),
                family_id,
                expires_at: refresh_expires_at,
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
        "refresh_token": refresh_token,
        "refresh_token_expires_in": REFRESH_TOKEN_TTL_SECS,
        "scope": grant.scope
    }))
    .into_response()
}

fn refresh_token(state: &AuthState, params: &HashMap<String, String>) -> Response {
    let Some(client_id) = params.get("client_id") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_id is required",
        );
    };
    let Some(presented) = params.get("refresh_token") else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "refresh_token is required",
        );
    };

    let presented_hash = token_hash(presented);
    let current = now();
    let mut store = state.store_guard();
    store.tokens.retain(|_, grant| grant.expires_at > current);
    store
        .refresh_tokens
        .retain(|_, grant| grant.expires_at > current);
    store
        .consumed_refresh_tokens
        .retain(|_, grant| grant.expires_at > current);

    if let Some(consumed) = store.consumed_refresh_tokens.get(&presented_hash).cloned() {
        let family_id = consumed.family_id;
        revoke_refresh_family(&mut store, &family_id);
        if let Err(error) = persist_store(&store) {
            return oauth_json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", &error);
        }
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "refresh token replay detected; token family revoked",
        );
    }

    let Some(grant) = store.refresh_tokens.get(&presented_hash).cloned() else {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "refresh token is invalid or expired",
        );
    };

    if grant.client_id != *client_id || grant.expires_at <= current {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "refresh token validation failed",
        );
    }
    if params
        .get("resource")
        .is_some_and(|resource| resource != &grant.resource)
        || grant.resource != state.resource_url()
    {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_target",
            "refresh token resource does not match",
        );
    }
    if params
        .get("scope")
        .is_some_and(|scope| scope != &grant.scope)
    {
        return oauth_json_error(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "refresh token cannot broaden or change scope",
        );
    }

    let replacement = random_token(32);
    let replacement_hash = token_hash(&replacement);
    let access_token = random_token(32);
    let access_expires_at = current + TOKEN_TTL_SECS;
    let refresh_expires_at = current + REFRESH_TOKEN_TTL_SECS;

    store.refresh_tokens.remove(&presented_hash);
    store.consumed_refresh_tokens.insert(
        presented_hash,
        ConsumedRefreshGrant {
            family_id: grant.family_id.clone(),
            expires_at: grant.expires_at,
        },
    );
    store.refresh_tokens.insert(
        replacement_hash,
        RefreshGrant {
            client_id: grant.client_id,
            resource: grant.resource.clone(),
            scope: grant.scope.clone(),
            family_id: grant.family_id.clone(),
            expires_at: refresh_expires_at,
        },
    );
    store.tokens.insert(
        access_token.clone(),
        AccessGrant {
            resource: grant.resource,
            scope: grant.scope.clone(),
            expires_at: access_expires_at,
            family_id: Some(grant.family_id),
        },
    );
    if let Err(error) = persist_store(&store) {
        return oauth_json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", &error);
    }

    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": TOKEN_TTL_SECS,
        "refresh_token": replacement,
        "refresh_token_expires_in": REFRESH_TOKEN_TTL_SECS,
        "scope": grant.scope
    }))
    .into_response()
}

pub(super) fn authorize_action(params: &HashMap<String, String>) -> Result<&str, &'static str> {
    match params.get("action").map(String::as_str) {
        Some(action @ ("approve" | "deny")) => Ok(action),
        _ => Err("action must be approve or deny"),
    }
}

pub(super) fn validate_authorize_request(
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

    let store = state.store_guard();
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

pub(super) fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

pub(super) fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub(super) fn token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

pub(super) fn revoke_refresh_family(store: &mut AuthStore, family_id: &str) {
    store
        .refresh_tokens
        .retain(|_, grant| grant.family_id != family_id);
    store
        .tokens
        .retain(|_, grant| grant.family_id.as_deref() != Some(family_id));
}

pub(super) fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
