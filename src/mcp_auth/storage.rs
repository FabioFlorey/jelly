use crate::STATE_DIR;
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
};

const OAUTH_STATE_FILE: &str = "oauth.json";

#[derive(Debug, Clone)]
pub(super) struct Client {
    pub(super) redirect_uris: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CodeGrant {
    pub(super) client_id: String,
    pub(super) redirect_uri: String,
    pub(super) code_challenge: String,
    pub(super) resource: String,
    pub(super) scope: String,
    pub(super) expires_at: u64,
}

#[derive(Debug, Clone)]
pub(super) struct AccessGrant {
    pub(super) resource: String,
    pub(super) scope: String,
    pub(super) expires_at: u64,
    pub(super) family_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct RefreshGrant {
    pub(super) client_id: String,
    pub(super) resource: String,
    pub(super) scope: String,
    pub(super) family_id: String,
    pub(super) expires_at: u64,
}

#[derive(Debug, Clone)]
pub(super) struct ConsumedRefreshGrant {
    pub(super) family_id: String,
    pub(super) expires_at: u64,
}

#[derive(Default)]
pub(super) struct AuthStore {
    pub(super) clients: HashMap<String, Client>,
    pub(super) codes: HashMap<String, CodeGrant>,
    pub(super) tokens: HashMap<String, AccessGrant>,
    pub(super) refresh_tokens: HashMap<String, RefreshGrant>,
    pub(super) consumed_refresh_tokens: HashMap<String, ConsumedRefreshGrant>,
}

fn store_path() -> String {
    format!("{STATE_DIR}/{OAUTH_STATE_FILE}")
}

pub(super) fn load_store() -> Result<AuthStore, String> {
    let path = store_path();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AuthStore::default());
        }
        Err(error) => return Err(format!("failed to read OAuth state: {error}")),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse OAuth state: {error}"))?;
    let mut store = AuthStore::default();

    if let Some(clients) = value.get("clients").and_then(Value::as_object) {
        for (id, client) in clients {
            let redirects = client
                .get("redirect_uris")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !redirects.is_empty() {
                store.clients.insert(
                    id.clone(),
                    Client {
                        redirect_uris: redirects,
                    },
                );
            }
        }
    }

    if let Some(tokens) = value.get("tokens").and_then(Value::as_object) {
        for (token, grant) in tokens {
            let Some(resource) = grant.get("resource").and_then(Value::as_str) else {
                continue;
            };
            let Some(scope) = grant.get("scope").and_then(Value::as_str) else {
                continue;
            };
            let Some(expires_at) = grant.get("expires_at").and_then(Value::as_u64) else {
                continue;
            };
            let family_id = grant
                .get("family_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            store.tokens.insert(
                token.clone(),
                AccessGrant {
                    resource: resource.to_owned(),
                    scope: scope.to_owned(),
                    expires_at,
                    family_id,
                },
            );
        }
    }

    if let Some(refresh_tokens) = value.get("refresh_tokens").and_then(Value::as_object) {
        for (token_hash, grant) in refresh_tokens {
            let Some(client_id) = grant.get("client_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(resource) = grant.get("resource").and_then(Value::as_str) else {
                continue;
            };
            let Some(scope) = grant.get("scope").and_then(Value::as_str) else {
                continue;
            };
            let Some(family_id) = grant.get("family_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(expires_at) = grant.get("expires_at").and_then(Value::as_u64) else {
                continue;
            };
            store.refresh_tokens.insert(
                token_hash.clone(),
                RefreshGrant {
                    client_id: client_id.to_owned(),
                    resource: resource.to_owned(),
                    scope: scope.to_owned(),
                    family_id: family_id.to_owned(),
                    expires_at,
                },
            );
        }
    }

    if let Some(consumed) = value
        .get("consumed_refresh_tokens")
        .and_then(Value::as_object)
    {
        for (token_hash, grant) in consumed {
            let Some(family_id) = grant.get("family_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(expires_at) = grant.get("expires_at").and_then(Value::as_u64) else {
                continue;
            };
            store.consumed_refresh_tokens.insert(
                token_hash.clone(),
                ConsumedRefreshGrant {
                    family_id: family_id.to_owned(),
                    expires_at,
                },
            );
        }
    }

    Ok(store)
}

pub(super) fn persist_store(store: &AuthStore) -> Result<(), String> {
    fs::create_dir_all(STATE_DIR)
        .map_err(|error| format!("failed to create OAuth state dir: {error}"))?;
    let clients = store
        .clients
        .iter()
        .map(|(id, client)| (id.clone(), json!({"redirect_uris":client.redirect_uris})))
        .collect::<Map<String, Value>>();
    let tokens = store
        .tokens
        .iter()
        .map(|(token, grant)| {
            (
                token.clone(),
                json!({
                    "resource":grant.resource,
                    "scope":grant.scope,
                    "expires_at":grant.expires_at,
                    "family_id":grant.family_id
                }),
            )
        })
        .collect::<Map<String, Value>>();
    let refresh_tokens = store
        .refresh_tokens
        .iter()
        .map(|(token_hash, grant)| {
            (
                token_hash.clone(),
                json!({
                    "client_id": grant.client_id,
                    "resource": grant.resource,
                    "scope": grant.scope,
                    "family_id": grant.family_id,
                    "expires_at": grant.expires_at
                }),
            )
        })
        .collect::<Map<String, Value>>();
    let consumed_refresh_tokens = store
        .consumed_refresh_tokens
        .iter()
        .map(|(token_hash, grant)| {
            (
                token_hash.clone(),
                json!({
                    "family_id": grant.family_id,
                    "expires_at": grant.expires_at
                }),
            )
        })
        .collect::<Map<String, Value>>();
    let bytes = serde_json::to_vec_pretty(&json!({
        "clients": clients,
        "tokens": tokens,
        "refresh_tokens": refresh_tokens,
        "consumed_refresh_tokens": consumed_refresh_tokens
    }))
    .map_err(|error| format!("failed to serialize OAuth state: {error}"))?;
    let path = store_path();
    let temp = format!("{path}.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| format!("failed to open OAuth state: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("failed to write OAuth state: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("failed to sync OAuth state: {error}"))?;
    fs::rename(&temp, &path).map_err(|error| format!("failed to install OAuth state: {error}"))?;
    Ok(())
}
