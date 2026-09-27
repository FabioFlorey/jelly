use jelly::mcp;
use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")).ok();

    let token = env::var("JELLY_MCP_TOKEN").map_err(
        |_| "JELLY_MCP_TOKEN is not set; refusing to start an unauthenticated MCP server",
    )?;
    let oauth_password = env::var("JELLY_OAUTH_PASSWORD").unwrap_or_default();
    let bootstrap_secret =
        env::var("JELLY_BOOTSTRAP_SECRET").map_err(|_| "JELLY_BOOTSTRAP_SECRET is not set")?;
    let consent_mode = env::var("JELLY_OAUTH_CONSENT_MODE").unwrap_or_else(|_| "browser".into());
    let public_chatgpt_dcr = env::var("JELLY_OAUTH_PUBLIC_CHATGPT_DCR")
        .map(|value| value == "true")
        .unwrap_or(false);

    let address = env::var("JELLY_MCP_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let address: SocketAddr = address.parse()?;
    let public_url = env::var("JELLY_PUBLIC_URL").unwrap_or_else(|_| format!("http://{address}"));

    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("jelly MCP listening on http://{address}/mcp");
    println!("OAuth issuer: {public_url}");
    println!("OAuth resource: {public_url}/mcp");
    println!(
        "OAuth consent={} public_chatgpt_dcr={}",
        consent_mode, public_chatgpt_dcr
    );

    axum::serve(
        listener,
        mcp::router(
            token,
            oauth_password,
            bootstrap_secret,
            consent_mode,
            public_chatgpt_dcr,
            public_url,
        )?,
    )
    .await?;
    Ok(())
}
