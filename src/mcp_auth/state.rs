use super::oauth::{bearer, constant_time_eq, now};
use super::storage::{AuthStore, load_store};
use super::{OWNER_COOKIE, SCOPE};
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentMode {
    Browser,
    Paired,
}

impl ConsentMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "browser" => Ok(Self::Browser),
            "paired" => Ok(Self::Paired),
            _ => Err("JELLY_OAUTH_CONSENT_MODE must be 'browser' or 'paired'".into()),
        }
    }
}

#[derive(Clone)]
pub struct AuthState {
    pub(super) static_token: Arc<str>,
    pub(super) password: Arc<str>,
    pub(super) bootstrap_secret: Arc<str>,
    pub(super) consent_mode: ConsentMode,
    pub(super) public_chatgpt_dcr: bool,
    pub(super) public_url: Arc<str>,
    pub(super) store: Arc<Mutex<AuthStore>>,
    pub(super) owner_sessions: Arc<Mutex<HashMap<String, u64>>>,
    pub(super) pair_codes: Arc<Mutex<HashMap<String, u64>>>,
}

impl AuthState {
    pub fn new(
        static_token: String,
        password: String,
        bootstrap_secret: String,
        consent_mode: ConsentMode,
        public_chatgpt_dcr: bool,
        public_url: String,
    ) -> Result<Self, String> {
        if static_token.len() < 32 {
            return Err("JELLY_MCP_TOKEN must be at least 32 bytes".into());
        }
        if consent_mode == ConsentMode::Browser && password.len() < 16 {
            return Err(
                "JELLY_OAUTH_PASSWORD must be at least 16 bytes in browser consent mode".into(),
            );
        }
        if bootstrap_secret.len() < 32 {
            return Err("JELLY_BOOTSTRAP_SECRET must be at least 32 bytes".into());
        }

        let public_url = public_url.trim_end_matches('/').to_owned();
        let parsed =
            Url::parse(&public_url).map_err(|e| format!("invalid JELLY_PUBLIC_URL: {e}"))?;
        if parsed.scheme() != "https"
            && !(parsed.scheme() == "http"
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")))
        {
            return Err(
                "JELLY_PUBLIC_URL must use https, except localhost development URLs".into(),
            );
        }

        let mut store = load_store()?;
        let now = now();
        store.tokens.retain(|_, grant| grant.expires_at > now);

        Ok(Self {
            static_token: Arc::from(static_token),
            password: Arc::from(password),
            bootstrap_secret: Arc::from(bootstrap_secret),
            consent_mode,
            public_chatgpt_dcr,
            public_url: Arc::from(public_url),
            store: Arc::new(Mutex::new(store)),
            owner_sessions: Arc::new(Mutex::new(HashMap::new())),
            pair_codes: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(super) fn store_guard(&self) -> MutexGuard<'_, AuthStore> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn owner_sessions_guard(&self) -> MutexGuard<'_, HashMap<String, u64>> {
        self.owner_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn pair_codes_guard(&self) -> MutexGuard<'_, HashMap<String, u64>> {
        self.pair_codes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub fn resource_url(&self) -> String {
        format!("{}/mcp", self.public_url)
    }

    pub fn consent_mode_name(&self) -> &'static str {
        match self.consent_mode {
            ConsentMode::Browser => "browser",
            ConsentMode::Paired => "paired",
        }
    }

    pub fn public_chatgpt_dcr(&self) -> bool {
        self.public_chatgpt_dcr
    }

    pub fn owner_session_count(&self) -> usize {
        let now = now();
        let mut sessions = self.owner_sessions_guard();
        sessions.retain(|_, expiry| *expiry > now);
        sessions.len()
    }

    pub fn oauth_counts(&self) -> (usize, usize) {
        let now = now();
        let mut store = self.store_guard();
        store.tokens.retain(|_, grant| grant.expires_at > now);
        (store.clients.len(), store.tokens.len())
    }

    pub fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(token) = bearer(headers) else {
            return false;
        };

        if constant_time_eq(token.as_bytes(), self.static_token.as_bytes()) {
            return true;
        }

        let now = now();
        let mut store = self.store_guard();
        store.tokens.retain(|_, grant| grant.expires_at > now);
        store.tokens.get(token).is_some_and(|grant| {
            grant.resource == self.resource_url()
                && grant.scope.split_whitespace().any(|scope| scope == SCOPE)
                && grant.expires_at > now
        })
    }

    pub fn unauthorized(&self) -> Response {
        let metadata = format!(
            "{}/.well-known/oauth-protected-resource/mcp",
            self.public_url
        );
        let challenge = format!("Bearer resource_metadata=\"{metadata}\", scope=\"{SCOPE}\"");
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        if let Ok(value) = HeaderValue::from_str(&challenge) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        response
    }

    pub(super) fn has_bootstrap_access(&self, headers: &HeaderMap) -> bool {
        bearer(headers).is_some_and(|token| {
            constant_time_eq(token.as_bytes(), self.bootstrap_secret.as_bytes())
        })
    }

    pub(crate) fn has_owner_session(&self, headers: &HeaderMap) -> bool {
        let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let Some(token) = cookie.split(';').find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (name == OWNER_COOKIE).then_some(value)
        }) else {
            return false;
        };

        let now = now();
        let mut sessions = self.owner_sessions_guard();
        sessions.retain(|_, expiry| *expiry > now);
        sessions.get(token).is_some_and(|expiry| *expiry > now)
    }
}
