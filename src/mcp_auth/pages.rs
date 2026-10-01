use super::oauth::now;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub(super) fn oauth_page(title: &str, content: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title}</title><link rel="icon" type="image/png" href="/brand/favicon.png"><style>
:root{{--bg:#f5f5f2;--fg:#2d2224;--border:#d9d4c8;--muted:#817a70;--honey:#ffc107;--honey-dark:#c58f00;--panel:#fafaf7;}}
*{{box-sizing:border-box}}html{{background:var(--bg)}}body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:var(--bg);color:var(--fg);font:15px/1.65 ui-monospace,"SFMono-Regular",Consolas,"Liberation Mono",Menlo,monospace;padding:32px 20px}}body::after{{content:"";position:fixed;inset:0;pointer-events:none;opacity:.035;background-image:url("data:image/svg+xml,%3Csvg viewBox='0 0 180 180' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.9' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")}}main{{position:relative;width:min(620px,100%);background:var(--panel);border:1px solid var(--border);padding:28px 30px 30px}}.logo{{display:block;width:min(390px,88%);height:auto;margin:0 auto 28px}}h1{{font-family:ui-monospace,"SFMono-Regular",Consolas,monospace;font-size:1.12rem;line-height:1.5;letter-spacing:.01em;margin:0 0 10px;font-weight:700}}h1::before{{content:"> ";color:var(--honey-dark)}}p{{margin:0 0 24px;color:var(--muted)}}form{{display:grid;gap:18px;border-top:1px solid var(--border);padding-top:20px}}label{{display:grid;gap:8px;font-weight:700;font-size:.9rem}}input{{width:100%;font:inherit;color:var(--fg);background:var(--bg);border:1px solid var(--border);border-radius:0;padding:11px 12px;outline:none}}input:focus-visible{{border-color:var(--honey-dark);box-shadow:0 0 0 2px rgba(255,193,7,.2)}}button,.button{{appearance:none;display:inline-block;border:1px solid var(--fg);border-radius:0;padding:10px 14px;background:var(--fg);color:var(--bg);font:700 .86rem/1.2 ui-monospace,"SFMono-Regular",Consolas,monospace;text-decoration:none;cursor:pointer}}button:hover,.button:hover{{border-color:var(--honey-dark);background:var(--honey);color:#211a00}}.actions{{display:flex;gap:9px;flex-wrap:wrap}}.secondary{{background:transparent;color:var(--muted);border-color:var(--border)}}.secondary:hover{{color:var(--fg);background:transparent;border-color:var(--fg)}}.success{{margin-top:20px;padding:12px 0;border-top:1px solid var(--border);border-bottom:1px solid var(--border);color:var(--honey-dark);font-weight:700}}::selection{{background:var(--honey);color:#211a00}}@media(max-width:520px){{body{{padding:18px 12px}}main{{padding:22px 18px 24px}}.logo{{width:92%;margin-bottom:22px}}}}@media(prefers-reduced-motion:reduce){{*{{scroll-behavior:auto!important}}}}
</style></head><body><main><img class="logo" src="/brand/full-logo.png" alt="Jelly">{content}</main></body></html>"#,
        title = html_escape(title),
        content = content
    )
}

pub(super) fn html_escape(value: &str) -> String {
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
