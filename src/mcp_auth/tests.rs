use super::*;

fn state() -> AuthState {
    AuthState::new(
        "01234567890123456789012345678901".into(),
        "0123456789abcdef".into(),
        "abcdef0123456789abcdef0123456789".into(),
        ConsentMode::Browser,
        true,
        "https://jelly.example".into(),
    )
    .unwrap()
}

#[test]
fn static_bearer_is_accepted() {
    let state = state();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer 01234567890123456789012345678901"),
    );
    assert!(state.authorized(&headers));
}

#[test]
fn oauth_application_tokens_do_not_grant_diagnostic_admin_access() {
    use super::storage::AccessGrant;
    let state = state();
    state.store_guard().tokens.insert(
        "oauth-token".into(),
        AccessGrant {
            resource: state.resource_url(),
            scope: SCOPE.into(),
            expires_at: u64::MAX,
            family_id: None,
        },
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer oauth-token"),
    );
    assert!(state.authorized(&headers));
    assert!(!state.has_admin_access(&headers));
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer abcdef0123456789abcdef0123456789"),
    );
    assert!(state.has_admin_access(&headers));
}

#[test]
fn chatgpt_connection_status_uses_only_chatgpt_client_refresh_grants() {
    use super::storage::RefreshGrant;
    let state = state();
    {
        let mut store = state.store_guard();
        store.clients.insert(
            "other".into(),
            Client {
                redirect_uris: vec!["https://example.com/redirect".into()],
            },
        );
        store.clients.insert(
            "chatgpt".into(),
            Client {
                redirect_uris: vec!["https://chatgpt.com/connector_platform_oauth_redirect".into()],
            },
        );
        store.refresh_tokens.insert(
            "other-refresh".into(),
            RefreshGrant {
                client_id: "other".into(),
                resource: state.resource_url(),
                scope: SCOPE.into(),
                family_id: "other-family".into(),
                expires_at: u64::MAX,
            },
        );
    }
    assert!(!state.chatgpt_authorized());
    state.store_guard().refresh_tokens.insert(
        "chatgpt-refresh".into(),
        RefreshGrant {
            client_id: "chatgpt".into(),
            resource: state.resource_url(),
            scope: SCOPE.into(),
            family_id: "chatgpt-family".into(),
            expires_at: u64::MAX,
        },
    );
    assert!(state.chatgpt_authorized());
    state
        .store_guard()
        .refresh_tokens
        .get_mut("chatgpt-refresh")
        .unwrap()
        .expires_at = 0;
    assert!(!state.chatgpt_authorized());
}

#[test]
fn pkce_matches_rfc7636_example() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        pkce_challenge(verifier),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn redirect_validation_rejects_plain_http_remote_hosts() {
    assert!(valid_redirect_uri(
        "https://chatgpt.com/connector/oauth/FjKFnVbRJLSh"
    ));
    assert!(valid_redirect_uri("http://127.0.0.1:1234/callback"));
    assert!(!valid_redirect_uri("http://example.com/callback"));
}

#[test]
fn public_dcr_is_chatgpt_only() {
    assert!(is_chatgpt_redirect(
        "https://chatgpt.com/connector/oauth/FjKFnVbRJLSh"
    ));
    assert!(is_chatgpt_redirect(
        "https://chatgpt.com/connector_platform_oauth_redirect"
    ));
    assert!(!is_chatgpt_redirect("https://example.com/callback"));
    assert!(!is_chatgpt_redirect(
        "https://chatgpt.com/other/oauth/FjKFnVbRJLSh"
    ));
    assert!(!is_chatgpt_redirect(
        "https://chatgpt.com/connector/oauth/FjKFnVbRJLSh/extra"
    ));
}

#[test]
fn consent_modes_match_pilink_shape() {
    assert_eq!(ConsentMode::parse("browser").unwrap(), ConsentMode::Browser);
    assert_eq!(ConsentMode::parse("paired").unwrap(), ConsentMode::Paired);
    assert!(ConsentMode::parse("auto").is_err());
}

#[test]
fn paired_mode_does_not_require_oauth_password() {
    assert!(
        AuthState::new(
            "01234567890123456789012345678901".into(),
            String::new(),
            "abcdef0123456789abcdef0123456789".into(),
            ConsentMode::Paired,
            false,
            "https://jelly.example".into(),
        )
        .is_ok()
    );
}

#[test]
fn authorize_validation_accepts_registered_pkce_request() {
    let state = state();
    state.store.lock().unwrap().clients.insert(
        "client-1".into(),
        Client {
            redirect_uris: vec!["https://client.example/callback".into()],
        },
    );
    let params = HashMap::from([
        ("response_type".into(), "code".into()),
        ("client_id".into(), "client-1".into()),
        (
            "redirect_uri".into(),
            "https://client.example/callback".into(),
        ),
        ("code_challenge".into(), "challenge".into()),
        ("code_challenge_method".into(), "S256".into()),
        ("resource".into(), state.resource_url()),
        ("scope".into(), SCOPE.into()),
    ]);
    assert!(validate_authorize_request(&state, &params).is_ok());
}

#[test]
fn authorize_validation_returns_boxed_bad_request() {
    let state = state();
    let params = HashMap::from([
        ("response_type".into(), "token".into()),
        ("client_id".into(), "missing".into()),
    ]);
    let response = validate_authorize_request(&state, &params).unwrap_err();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn poisoned_auth_store_lock_is_recoverable() {
    let state = state();
    let poisoned = state.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoned.store.lock().unwrap();
        panic!("poison auth store for recovery test");
    })
    .join();

    let mut store = state.store_guard();
    store.clients.clear();
}

#[test]
fn authorize_action_accepts_only_explicit_consent_values() {
    let approve = HashMap::from([("action".into(), "approve".into())]);
    let deny = HashMap::from([("action".into(), "deny".into())]);
    let unknown = HashMap::from([("action".into(), "later".into())]);
    let missing = HashMap::new();

    assert_eq!(authorize_action(&approve).unwrap(), "approve");
    assert_eq!(authorize_action(&deny).unwrap(), "deny");
    assert_eq!(
        authorize_action(&unknown).unwrap_err(),
        "action must be approve or deny"
    );
    assert_eq!(
        authorize_action(&missing).unwrap_err(),
        "action must be approve or deny"
    );
}

#[tokio::test]
async fn oauth_metadata_advertises_refresh_tokens() {
    let axum::Json(metadata) = authorization_server_metadata(axum::extract::State(state())).await;
    let grants = metadata["grant_types_supported"].as_array().unwrap();
    assert!(grants.iter().any(|grant| grant == "authorization_code"));
    assert!(grants.iter().any(|grant| grant == "refresh_token"));
}

#[test]
fn refresh_tokens_are_fingerprinted_before_storage() {
    let fingerprint = token_hash("example-refresh-token");
    assert_ne!(fingerprint, "example-refresh-token");
    assert_eq!(fingerprint, token_hash("example-refresh-token"));
}

#[test]
fn refresh_replay_revokes_only_its_token_family() {
    use super::storage::{AccessGrant, AuthStore, RefreshGrant};

    let mut store = AuthStore::default();
    store.tokens.insert(
        "access-family-a".into(),
        AccessGrant {
            resource: "https://jelly.example/mcp".into(),
            scope: SCOPE.into(),
            expires_at: u64::MAX,
            family_id: Some("family-a".into()),
        },
    );
    store.tokens.insert(
        "access-family-b".into(),
        AccessGrant {
            resource: "https://jelly.example/mcp".into(),
            scope: SCOPE.into(),
            expires_at: u64::MAX,
            family_id: Some("family-b".into()),
        },
    );
    store.refresh_tokens.insert(
        "refresh-family-a".into(),
        RefreshGrant {
            client_id: "client-a".into(),
            resource: "https://jelly.example/mcp".into(),
            scope: SCOPE.into(),
            family_id: "family-a".into(),
            expires_at: u64::MAX,
        },
    );
    store.refresh_tokens.insert(
        "refresh-family-b".into(),
        RefreshGrant {
            client_id: "client-b".into(),
            resource: "https://jelly.example/mcp".into(),
            scope: SCOPE.into(),
            family_id: "family-b".into(),
            expires_at: u64::MAX,
        },
    );

    revoke_refresh_family(&mut store, "family-a");

    assert!(!store.tokens.contains_key("access-family-a"));
    assert!(!store.refresh_tokens.contains_key("refresh-family-a"));
    assert!(store.tokens.contains_key("access-family-b"));
    assert!(store.refresh_tokens.contains_key("refresh-family-b"));
}

#[test]
fn rejected_authorization_code_cannot_be_reused_after_consume() {
    use super::storage::{AuthStore, CodeGrant};
    use crate::core::oauth::{CodeExchange, CodeGrantView, valid_code_grant};

    // Isolated in-memory simulation of the production ordering. No persist_store.
    let mut store = AuthStore::default();
    store.codes.insert(
        "one-time-code".into(),
        CodeGrant {
            client_id: "client-a".into(),
            redirect_uri: "https://app.test/redirect".into(),
            resource: "https://jelly.test/mcp".into(),
            code_challenge: pkce_challenge("correct-verifier"),
            scope: SCOPE.into(),
            expires_at: 200,
        },
    );
    let grant = store.codes.remove("one-time-code").unwrap();
    assert!(!valid_code_grant(
        &CodeGrantView {
            client_id: &grant.client_id,
            redirect_uri: &grant.redirect_uri,
            resource: &grant.resource,
            code_challenge: &grant.code_challenge,
            expires_at: grant.expires_at,
        },
        &CodeExchange {
            client_id: "client-a",
            redirect_uri: "https://app.test/redirect",
            resource: "https://jelly.test/mcp",
            expected_resource: "https://jelly.test/mcp",
            verifier: "wrong-verifier",
            current: 100,
        },
    ));
    assert!(store.codes.remove("one-time-code").is_none());
}

#[test]
fn replay_decision_and_revocation_preserve_consumed_marker_and_other_families() {
    use super::storage::{AccessGrant, AuthStore, ConsumedRefreshGrant, RefreshGrant};
    use crate::core::oauth::{RefreshDecision, RefreshExchange, decide_refresh};

    let mut store = AuthStore::default();
    for family in ["family-a", "family-b"] {
        store.tokens.insert(
            format!("access-{family}"),
            AccessGrant {
                resource: "https://jelly.test/mcp".into(),
                scope: SCOPE.into(),
                expires_at: 1000,
                family_id: Some(family.into()),
            },
        );
        store.refresh_tokens.insert(
            format!("refresh-{family}"),
            RefreshGrant {
                client_id: format!("client-{family}"),
                resource: "https://jelly.test/mcp".into(),
                scope: SCOPE.into(),
                family_id: family.into(),
                expires_at: 1000,
            },
        );
    }
    store.consumed_refresh_tokens.insert(
        "old-refresh".into(),
        ConsumedRefreshGrant {
            family_id: "family-a".into(),
            expires_at: 1000,
        },
    );
    let decision = decide_refresh(
        store
            .consumed_refresh_tokens
            .get("old-refresh")
            .map(|g| g.family_id.as_str()),
        None,
        &RefreshExchange {
            client_id: "wrong-client",
            resource: Some("wrong-resource"),
            scope: Some("wrong-scope"),
            expected_resource: "https://jelly.test/mcp",
            current: 100,
        },
    );
    assert_eq!(decision, RefreshDecision::RevokeFamily("family-a".into()));
    if let RefreshDecision::RevokeFamily(family) = decision {
        revoke_refresh_family(&mut store, &family);
    }
    assert!(!store.tokens.contains_key("access-family-a"));
    assert!(!store.refresh_tokens.contains_key("refresh-family-a"));
    assert!(store.tokens.contains_key("access-family-b"));
    assert!(store.refresh_tokens.contains_key("refresh-family-b"));
    assert!(store.consumed_refresh_tokens.contains_key("old-refresh"));
}

#[test]
fn expired_replay_markers_are_pruned_before_decision_and_cannot_revoke_family() {
    use super::storage::{AuthStore, ConsumedRefreshGrant};
    use crate::core::oauth::{RefreshDecision, RefreshExchange, decide_refresh, unexpired};

    let mut store = AuthStore::default();
    store.consumed_refresh_tokens.insert(
        "expired-marker".into(),
        ConsumedRefreshGrant {
            family_id: "family-a".into(),
            expires_at: 200,
        },
    );
    store
        .consumed_refresh_tokens
        .retain(|_, g| unexpired(g.expires_at, 200));
    assert_eq!(
        decide_refresh(
            store
                .consumed_refresh_tokens
                .get("expired-marker")
                .map(|g| g.family_id.as_str()),
            None,
            &RefreshExchange {
                client_id: "client",
                resource: None,
                scope: None,
                expected_resource: "https://jelly.test/mcp",
                current: 200,
            },
        ),
        RefreshDecision::InvalidOrExpired
    );
}
