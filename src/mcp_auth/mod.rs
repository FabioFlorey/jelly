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
#[cfg(test)]
use oauth::{authorize_action, pkce_challenge, validate_authorize_request};
use oauth::{authorize_get, authorize_post, pair_get, pair_post, pair_status, token};
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
mod tests;
