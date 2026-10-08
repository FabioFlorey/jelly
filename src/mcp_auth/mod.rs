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
mod local_approval;
mod oauth;
mod pages;
pub(crate) use pages::{html_escape, oauth_page};
mod state;
mod storage;

use dcr::{authorization_server_metadata, protected_resource_metadata, register_client};
#[cfg(test)]
use dcr::{is_chatgpt_redirect, valid_redirect_uri};
pub(crate) use local_approval::local_admin_routes;
#[cfg(test)]
use oauth::{
    authorize_action, pkce_challenge, revoke_refresh_family, token_hash, validate_authorize_request,
};
use oauth::{authorize_get, authorize_post, pair_code, pair_get, pair_post, pair_status, token};
pub use state::{AuthState, ConsentMode};
#[cfg(test)]
use storage::Client;

/// Brand assets are available on both the public and loopback-only routers.
pub(crate) fn brand_routes() -> Router<AuthState> {
    Router::new()
        .route("/brand/full-logo.png", get(brand_logo))
        .route("/brand/favicon.png", get(brand_favicon))
        .route("/brand/jelly.css", get(brand_css))
        .route("/brand/dynapuff.ttf", get(brand_dynapuff))
        .route("/brand/roboto-400.ttf", get(brand_roboto_400))
        .route("/brand/roboto-500.ttf", get(brand_roboto_500))
        .route("/brand/roboto-700.ttf", get(brand_roboto_700))
}

pub fn routes() -> Router<AuthState> {
    Router::new()
        .merge(brand_routes())
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
        .route("/register", post(register_client))
        .route("/authorize", get(authorize_get).post(authorize_post))
        .route("/pair", get(pair_get).post(pair_post))
        .route("/pair/code", post(pair_code))
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

async fn brand_css() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(include_str!("../../assets/jelly.css")))
        .unwrap()
}

async fn brand_dynapuff() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "font/ttf")
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(
            &include_bytes!("../../assets/fonts/DynaPuff-700.ttf")[..],
        ))
        .unwrap()
}

fn font_response(bytes: &'static [u8]) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "font/ttf")
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(bytes))
        .unwrap()
}

async fn brand_roboto_400() -> Response {
    font_response(&include_bytes!("../../assets/fonts/Roboto-400.ttf")[..])
}

async fn brand_roboto_500() -> Response {
    font_response(&include_bytes!("../../assets/fonts/Roboto-500.ttf")[..])
}

async fn brand_roboto_700() -> Response {
    font_response(&include_bytes!("../../assets/fonts/Roboto-700.ttf")[..])
}

#[cfg(test)]
mod tests;
