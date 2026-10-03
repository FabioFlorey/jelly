use super::oauth::now;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub(crate) fn oauth_page(title: &str, content: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title}</title><link rel="icon" type="image/png" href="/brand/favicon.png"><link rel="stylesheet" href="/brand/jelly.css"></head><body><main><a class="logo-link" href="/" aria-label="Jelly home"><img class="logo" src="/brand/full-logo.png" alt="Jelly"></a>{content}</main></body></html>"#,
        title = html_escape(title),
        content = content
    )
}

pub(crate) fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(super) fn client_registration_response(
    client_id: String,
    redirect_uris: Vec<String>,
) -> Response {
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

pub(super) fn oauth_json_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        Json(json!({
            "error": error,
            "error_description": description
        })),
    )
        .into_response()
}
