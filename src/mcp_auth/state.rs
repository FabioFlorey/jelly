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

#[derive(Debug, Clone)]
pub(super) struct PendingApproval {
    pub(super) id: String,
    pub(super) client_id: String,
    pub(super) redirect_uri: String,
    pub(super) scope: String,
    pub(super) expires_at: u64,
    pub(super) setup_token: String,
    pub(super) params: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(super) struct LocalSetupSession {
    pub(super) token: String,
    pub(super) expires_at: u64,
}

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
    pub(super) local_admin_url: Arc<str>,
    pub(super) store: Arc<Mutex<AuthStore>>,
    pub(super) owner_sessions: Arc<Mutex<HashMap<String, u64>>>,
    pub(super) pair_codes: Arc<Mutex<HashMap<String, u64>>>,
    pub(super) local_approval_window_expires_at: Arc<Mutex<u64>>,
    pub(super) local_setup_session: Arc<Mutex<Option<LocalSetupSession>>>,
    pub(super) pending_approval: Arc<Mutex<Option<PendingApproval>>>,
}

impl AuthState {
    #[cfg(test)]
    pub fn new(
        static_token: String,
        password: String,
        bootstrap_secret: String,
        consent_mode: ConsentMode,
        public_chatgpt_dcr: bool,
        public_url: String,
    ) -> Result<Self, String> {
        Self::new_with_admin_url(
            static_token,
            password,
            bootstrap_secret,
            consent_mode,
            public_chatgpt_dcr,
            public_url,
            "http://127.0.0.1:8788".into(),
        )
    }

    pub fn new_with_admin_url(
        static_token: String,
        password: String,
        bootstrap_secret: String,
        consent_mode: ConsentMode,
        public_chatgpt_dcr: bool,
        public_url: String,
        local_admin_url: String,
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

        let local_admin_url = local_admin_url.trim_end_matches('/').to_owned();
        let admin_parsed =
            Url::parse(&local_admin_url).map_err(|e| format!("invalid local admin URL: {e}"))?;
        if admin_parsed.scheme() != "http"
            || !matches!(
                admin_parsed.host_str(),
                Some("127.0.0.1" | "localhost" | "::1")
            )
        {
            return Err("local admin URL must use http on a loopback host".into());
        }

        let mut store = load_store()?;
        let now = now();
        store.tokens.retain(|_, grant| grant.expires_at > now);
        store
            .refresh_tokens
            .retain(|_, grant| grant.expires_at > now);
        store
            .consumed_refresh_tokens
            .retain(|_, grant| grant.expires_at > now);

        Ok(Self {
            static_token: Arc::from(static_token),
            password: Arc::from(password),
            bootstrap_secret: Arc::from(bootstrap_secret),
            consent_mode,
            public_chatgpt_dcr,
            public_url: Arc::from(public_url),
            local_admin_url: Arc::from(local_admin_url),
            store: Arc::new(Mutex::new(store)),
            owner_sessions: Arc::new(Mutex::new(HashMap::new())),
            pair_codes: Arc::new(Mutex::new(HashMap::new())),
            local_approval_window_expires_at: Arc::new(Mutex::new(0)),
            local_setup_session: Arc::new(Mutex::new(None)),
            pending_approval: Arc::new(Mutex::new(None)),
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

    pub(super) fn local_approval_window_guard(&self) -> MutexGuard<'_, u64> {
        self.local_approval_window_expires_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn local_setup_guard(&self) -> MutexGuard<'_, Option<LocalSetupSession>> {
        self.local_setup_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn pending_approval_guard(&self) -> MutexGuard<'_, Option<PendingApproval>> {
        self.pending_approval
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub fn resource_url(&self) -> String {
        format!("{}/mcp", self.public_url)
    }

    pub(crate) fn local_admin_url(&self) -> &str {
        &self.local_admin_url
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

    pub fn oauth_counts(&self) -> (usize, usize, usize) {
        let now = now();
        let mut store = self.store_guard();
        store.tokens.retain(|_, grant| grant.expires_at > now);
        store
            .refresh_tokens
            .retain(|_, grant| grant.expires_at > now);
        (
            store.clients.len(),
            store.tokens.len(),
            store.refresh_tokens.len(),
        )
    }

    /// A stored, unexpired ChatGPT OAuth refresh grant (not proof that the
    /// ChatGPT client is currently online).
    pub fn chatgpt_authorized(&self) -> bool {
        let now = now();
        let mut store = self.store_guard();
        store
            .refresh_tokens
            .retain(|_, grant| grant.expires_at > now);
        store.refresh_tokens.values().any(|grant| {
            store.clients.get(&grant.client_id).is_some_and(|client| {
                client
                    .redirect_uris
                    .iter()
                    .any(|uri| super::dcr::is_chatgpt_redirect(uri))
            })
        })
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

    /// Owner-only diagnostics: OAuth application tokens do not grant admin access.
    pub(crate) fn has_admin_access(&self, headers: &HeaderMap) -> bool {
        self.has_owner_session(headers)
            || self.has_bootstrap_access(headers)
            || bearer(headers).is_some_and(|token| {
                constant_time_eq(token.as_bytes(), self.static_token.as_bytes())
            })
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
