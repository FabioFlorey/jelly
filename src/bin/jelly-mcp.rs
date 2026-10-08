use jelly::{config::config, mcp};
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

    let local_approval_enabled = consent_mode == "paired" && public_chatgpt_dcr;
    let address = env::var("JELLY_MCP_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let address: SocketAddr = address.parse()?;
    let admin_address = if local_approval_enabled {
        env::var("JELLY_MCP_ADMIN_ADDR").unwrap_or_else(|_| "127.0.0.1:8788".into())
    } else {
        "127.0.0.1:8788".into()
    };
    let admin_address: SocketAddr = admin_address.parse()?;
    if local_approval_enabled && !admin_address.ip().is_loopback() {
        return Err("JELLY_MCP_ADMIN_ADDR must bind to a loopback address".into());
    }
    let local_admin_url = format!("http://{admin_address}");
    let public_url = env::var("JELLY_PUBLIC_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("http://{address}"));

    // Explicit connection profiles are validated before opening any listener.
    // No profile configured means the existing MCP/authorization setup is unchanged.
    for connection in &config().mcp.connections {
        connection.validate_deployment(address, &public_url)?;
    }

    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("jelly MCP listening on http://{address}/mcp");
    println!("OAuth issuer: {public_url}");
    println!("OAuth resource: {public_url}/mcp");
    println!(
        "OAuth consent={} public_chatgpt_dcr={}",
        consent_mode, public_chatgpt_dcr
    );

    let (public_router, admin_router) = mcp::server_routers(
        token,
        oauth_password,
        bootstrap_secret,
        consent_mode,
        public_chatgpt_dcr,
        public_url,
        local_admin_url,
    )?;

    if local_approval_enabled {
        let admin_listener = tokio::net::TcpListener::bind(admin_address).await?;
        println!("jelly local admin listening on http://{admin_address}");
        tokio::try_join!(
            axum::serve(listener, public_router),
            axum::serve(admin_listener, admin_router)
        )?;
    } else {
        axum::serve(listener, public_router).await?;
    }
    Ok(())
}
