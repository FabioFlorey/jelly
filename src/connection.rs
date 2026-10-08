//! Provider-agnostic MCP connection profiles.
//!
//! Profiles describe supported onboarding choices. They do not change the
//! existing MCP server's authorization policy or start additional listeners.
//! The HTTP server currently accepts both its static bearer token and OAuth
//! access tokens; each connection's `auth` is the intended client setup mode.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use url::Url;

/// The client integration preset. Unrecognized MCP clients use `GenericMcp`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[serde(rename = "chatgpt")]
    ChatGPT,
    #[default]
    GenericMcp,
}

/// Transport as seen by the MCP client, not the internal HTTP listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionMethod {
    /// Reserved for a future local MCP stdio adapter.
    Stdio,
    LocalHttp,
    /// HTTPS (typically forwarded to Jelly's local HTTP listener by a proxy).
    RemoteHttp,
}

/// Desired client authentication mechanism, not a server-wide auth switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    /// Reserved for a future, explicitly scoped local transport.
    None,
    BearerToken,
    #[serde(rename = "oauth")]
    OAuth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConnection {
    pub provider: Provider,
    pub method: ConnectionMethod,
    pub auth: AuthMethod,
}

impl Provider {
    /// Suggested onboarding choice, not an enforced provider restriction.
    pub const fn preset(self) -> ProviderConnection {
        match self {
            Self::ChatGPT => ProviderConnection {
                provider: self,
                method: ConnectionMethod::RemoteHttp,
                auth: AuthMethod::OAuth,
            },
            Self::GenericMcp => ProviderConnection {
                provider: self,
                method: ConnectionMethod::LocalHttp,
                auth: AuthMethod::BearerToken,
            },
        }
    }
}

impl Default for ProviderConnection {
    fn default() -> Self {
        Provider::default().preset()
    }
}

impl ProviderConnection {
    /// Reject transports/auth choices that Jelly cannot securely serve today.
    pub fn validate(self) -> Result<(), String> {
        match (self.method, self.auth) {
            (ConnectionMethod::Stdio, _) => {
                Err("stdio MCP transport is not implemented; use local-http or remote-http".into())
            }
            (_, AuthMethod::None) => Err(
                "unauthenticated MCP connections are not implemented; use bearer-token or oauth"
                    .into(),
            ),
            _ => Ok(()),
        }
    }

    /// Validate deployment boundaries for explicit connection profiles.
    /// Jelly itself serves plain HTTP, so its listener must remain loopback-only
    /// even when a TLS reverse proxy exposes a remote HTTPS endpoint.
    pub fn validate_deployment(self, bind: SocketAddr, public_url: &str) -> Result<(), String> {
        self.validate()?;
        if !bind.ip().is_loopback() {
            return Err("Jelly's plain HTTP MCP listener must bind to a loopback address".into());
        }
        match self.method {
            ConnectionMethod::RemoteHttp => {
                let url = Url::parse(public_url)
                    .map_err(|error| format!("invalid remote MCP public URL: {error}"))?;
                if url.scheme() != "https" || url.host_str().is_none() {
                    return Err("remote-http requires a public HTTPS URL".into());
                }
                if !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                {
                    return Err(
                        "remote-http public URL must not contain credentials or a fragment".into(),
                    );
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_generic_local_bearer() {
        assert_eq!(ProviderConnection::default(), Provider::GenericMcp.preset());
        assert_eq!(
            Provider::GenericMcp.preset().method,
            ConnectionMethod::LocalHttp
        );
        assert_eq!(Provider::ChatGPT.preset().auth, AuthMethod::OAuth);
    }

    #[test]
    fn serialized_values_are_stable() {
        let chatgpt: ProviderConnection =
            toml::from_str("provider = 'chatgpt'\nmethod = 'remote-http'\nauth = 'oauth'").unwrap();
        assert_eq!(chatgpt, Provider::ChatGPT.preset());
        assert_eq!(
            toml::to_string(&chatgpt).unwrap(),
            "provider = \"chatgpt\"\nmethod = \"remote-http\"\nauth = \"oauth\"\n"
        );
        assert!(
            toml::from_str::<ProviderConnection>(
                "provider = 'unknown'\nmethod = 'local-http'\nauth = 'bearer-token'"
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_unimplemented_modes() {
        let base = ProviderConnection::default();
        assert!(
            ProviderConnection {
                method: ConnectionMethod::Stdio,
                ..base
            }
            .validate()
            .is_err()
        );
        assert!(
            ProviderConnection {
                auth: AuthMethod::None,
                ..base
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn enforces_local_binding_and_remote_tls() {
        let loopback = "127.0.0.1:8787".parse().unwrap();
        let exposed = "0.0.0.0:8787".parse().unwrap();
        assert!(
            Provider::GenericMcp
                .preset()
                .validate_deployment(loopback, "http://127.0.0.1:8787")
                .is_ok()
        );
        assert!(
            Provider::GenericMcp
                .preset()
                .validate_deployment(exposed, "http://127.0.0.1:8787")
                .is_err()
        );
        assert!(
            Provider::ChatGPT
                .preset()
                .validate_deployment(loopback, "https://jelly.example")
                .is_ok()
        );
        assert!(
            Provider::ChatGPT
                .preset()
                .validate_deployment(loopback, "http://jelly.example")
                .is_err()
        );
        assert!(
            Provider::ChatGPT
                .preset()
                .validate_deployment(exposed, "https://jelly.example")
                .is_err()
        );
        assert!(
            Provider::ChatGPT
                .preset()
                .validate_deployment(loopback, "https://user:secret@jelly.example")
                .is_err()
        );
    }
}
