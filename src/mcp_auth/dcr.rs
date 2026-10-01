use super::SCOPE;
use super::oauth::random_token;
use super::pages::{client_registration_response, oauth_json_error};
use super::state::AuthState;
use super::storage::{Client, persist_store};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use url::Url;

pub(super) async fn protected_resource_metadata(State(state): State<AuthState>) -> Json<Value> {
    Json(json!({
        "resource": state.resource_url(),
        "authorization_servers": [state.public_url()],
        "scopes_supported": [SCOPE]
    }))
}

pub(super) async fn authorization_server_metadata(State(state): State<AuthState>) -> Json<Value> {
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

pub(super) async fn register_client(
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
        let mut store = state.store_guard();
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

pub(super) fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        || (url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost")))
}

pub(super) fn is_chatgpt_redirect(value: &str) -> bool {
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
