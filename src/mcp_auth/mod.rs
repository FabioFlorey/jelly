#[cfg(test)]
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::{
    Router,
    body::Body,
    http::header,
    response::Response,
    routing::{get, post},
};
#[cfg(test)]
use std::collections::HashMap;

const SCOPE: &str = "jelly";
const OWNER_COOKIE: &str = "jelly_owner";

mod dcr;
mod oauth;
mod pages;
mod state;
mod storage;

use dcr::{authorization_server_metadata, protected_resource_metadata, register_client};
#[cfg(test)]
use dcr::{is_chatgpt_redirect, valid_redirect_uri};
use oauth::{authorize_get, authorize_post, pair_get, pair_post, pair_status, token};
#[cfg(test)]
use oauth::{pkce_challenge, validate_authorize_request};
pub use state::{AuthState, ConsentMode};
#[cfg(test)]
use storage::Client;

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
