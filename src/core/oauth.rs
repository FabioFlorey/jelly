//! Deterministic OAuth grant policy. No clock, randomness, lock or persistence.
//!
//! All time values and grant facts are observations supplied by the HTTP shell.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub(crate) fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub(crate) fn token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub(crate) fn unexpired(expires_at: u64, current: u64) -> bool {
    expires_at > current
}

/// Facts of the *already consumed* authorization code, captured by the shell.
/// Keeping removal ahead of this decision preserves one-time use on failure.
pub(crate) struct CodeGrantView<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) redirect_uri: &'a str,
    pub(crate) resource: &'a str,
    pub(crate) code_challenge: &'a str,
    pub(crate) expires_at: u64,
}

pub(crate) struct CodeExchange<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) redirect_uri: &'a str,
    pub(crate) resource: &'a str,
    pub(crate) expected_resource: &'a str,
    pub(crate) verifier: &'a str,
    pub(crate) current: u64,
}

pub(crate) fn valid_code_grant(grant: &CodeGrantView<'_>, input: &CodeExchange<'_>) -> bool {
    unexpired(grant.expires_at, input.current)
        && grant.client_id == input.client_id
        && grant.redirect_uri == input.redirect_uri
        && grant.resource == input.resource
        && grant.resource == input.expected_resource
        && constant_time_eq(
            pkce_challenge(input.verifier).as_bytes(),
            grant.code_challenge.as_bytes(),
        )
}

pub(crate) struct RefreshGrantView<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) resource: &'a str,
    pub(crate) scope: &'a str,
    pub(crate) expires_at: u64,
}

pub(crate) struct RefreshExchange<'a> {
    pub(crate) client_id: &'a str,
    pub(crate) resource: Option<&'a str>,
    pub(crate) scope: Option<&'a str>,
    pub(crate) expected_resource: &'a str,
    pub(crate) current: u64,
}

/// Preserve the public OAuth error precedence: replay > missing/expired >
/// client/expiry mismatch > resource mismatch > scope mismatch > rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefreshDecision {
    RevokeFamily(String),
    InvalidOrExpired,
    InvalidGrant,
    InvalidTarget,
    InvalidScope,
    Rotate,
}

pub(crate) fn decide_refresh(
    consumed_family: Option<&str>,
    grant: Option<&RefreshGrantView<'_>>,
    input: &RefreshExchange<'_>,
) -> RefreshDecision {
    if let Some(family) = consumed_family {
        return RefreshDecision::RevokeFamily(family.to_owned());
    }
    let Some(grant) = grant else {
        return RefreshDecision::InvalidOrExpired;
    };
    if grant.client_id != input.client_id || !unexpired(grant.expires_at, input.current) {
        return RefreshDecision::InvalidGrant;
    }
    if input
        .resource
        .is_some_and(|resource| resource != grant.resource)
        || grant.resource != input.expected_resource
    {
        return RefreshDecision::InvalidTarget;
    }
    if input.scope.is_some_and(|scope| scope != grant.scope) {
        return RefreshDecision::InvalidScope;
    }
    RefreshDecision::Rotate
}

/// A revoked refresh family also revokes its access tokens, but not tokens
/// belonging to other families or legacy ungrouped access tokens.
pub(crate) fn retain_other_family(grant_family: Option<&str>, revoked_family: &str) -> bool {
    grant_family != Some(revoked_family)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code<'a>(challenge: &'a str) -> CodeGrantView<'a> {
        CodeGrantView {
            client_id: "client-1",
            redirect_uri: "https://app.test/callback",
            resource: "https://jelly.test/mcp",
            code_challenge: challenge,
            expires_at: 200,
        }
    }

    fn exchange<'a>(verifier: &'a str) -> CodeExchange<'a> {
        CodeExchange {
            client_id: "client-1",
            redirect_uri: "https://app.test/callback",
            resource: "https://jelly.test/mcp",
            expected_resource: "https://jelly.test/mcp",
            verifier,
            current: 100,
        }
    }

    fn refresh() -> RefreshGrantView<'static> {
        RefreshGrantView {
            client_id: "client-1",
            resource: "https://jelly.test/mcp",
            scope: "jelly",
            expires_at: 200,
        }
    }

    fn refresh_request() -> RefreshExchange<'static> {
        RefreshExchange {
            client_id: "client-1",
            resource: None,
            scope: None,
            expected_resource: "https://jelly.test/mcp",
            current: 100,
        }
    }

    #[test]
    fn pkce_matches_rfc7636_example_and_token_hash_is_stable() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(token_hash("secret"), token_hash("secret"));
        assert_ne!(token_hash("secret"), "secret");
    }

    #[test]
    fn code_validation_requires_all_binding_claims_and_pkce() {
        let challenge = pkce_challenge("verifier");
        let valid = code(&challenge);
        let original = exchange("verifier");
        assert!(valid_code_grant(&valid, &original));
        assert!(!valid_code_grant(
            &valid,
            &CodeExchange {
                client_id: "other",
                ..original
            }
        ));
        assert!(!valid_code_grant(
            &valid,
            &CodeExchange {
                redirect_uri: "https://other.test",
                ..exchange("verifier")
            }
        ));
        assert!(!valid_code_grant(
            &valid,
            &CodeExchange {
                resource: "https://other.test",
                ..exchange("verifier")
            }
        ));
        assert!(!valid_code_grant(
            &valid,
            &CodeExchange {
                expected_resource: "https://other.test",
                ..exchange("verifier")
            }
        ));
        assert!(!valid_code_grant(&valid, &exchange("incorrect")));
        assert!(!valid_code_grant(&code("incorrect"), &exchange("verifier")));
    }

    #[test]
    fn exact_expiry_is_expired_for_code_and_refresh() {
        let challenge = pkce_challenge("verifier");
        assert!(!valid_code_grant(
            &code(&challenge),
            &CodeExchange {
                current: 200,
                ..exchange("verifier")
            }
        ));
        assert!(valid_code_grant(
            &code(&challenge),
            &CodeExchange {
                current: 199,
                ..exchange("verifier")
            }
        ));
        assert!(!unexpired(200, 200));
        assert!(unexpired(200, 199));
        assert_eq!(
            decide_refresh(
                None,
                Some(&refresh()),
                &RefreshExchange {
                    current: 200,
                    ..refresh_request()
                }
            ),
            RefreshDecision::InvalidGrant
        );
    }

    #[test]
    fn refresh_replay_precedes_other_errors_even_without_a_live_grant() {
        assert_eq!(
            decide_refresh(Some("family-a"), None, &refresh_request()),
            RefreshDecision::RevokeFamily("family-a".into())
        );
        let bad_request = RefreshExchange {
            client_id: "other",
            resource: Some("invalid"),
            scope: Some("invalid"),
            ..refresh_request()
        };
        assert_eq!(
            decide_refresh(Some("family-a"), Some(&refresh()), &bad_request),
            RefreshDecision::RevokeFamily("family-a".into())
        );
        assert_eq!(
            decide_refresh(None, None, &refresh_request()),
            RefreshDecision::InvalidOrExpired
        );
    }

    #[test]
    fn refresh_failure_precedence_preserves_client_resource_scope_order() {
        assert_eq!(
            decide_refresh(
                None,
                Some(&refresh()),
                &RefreshExchange {
                    client_id: "bad-client",
                    resource: Some("bad"),
                    scope: Some("bad"),
                    ..refresh_request()
                }
            ),
            RefreshDecision::InvalidGrant
        );
        assert_eq!(
            decide_refresh(
                None,
                Some(&refresh()),
                &RefreshExchange {
                    resource: Some("https://other.test/mcp"),
                    scope: Some("bad"),
                    ..refresh_request()
                }
            ),
            RefreshDecision::InvalidTarget
        );
        assert_eq!(
            decide_refresh(
                None,
                Some(&refresh()),
                &RefreshExchange {
                    expected_resource: "https://other.test/mcp",
                    scope: Some("bad"),
                    ..refresh_request()
                }
            ),
            RefreshDecision::InvalidTarget
        );
        assert_eq!(
            decide_refresh(
                None,
                Some(&refresh()),
                &RefreshExchange {
                    scope: Some("bad"),
                    ..refresh_request()
                }
            ),
            RefreshDecision::InvalidScope
        );
        assert_eq!(
            decide_refresh(
                None,
                Some(&refresh()),
                &RefreshExchange {
                    scope: Some("jelly"),
                    resource: Some("https://jelly.test/mcp"),
                    ..refresh_request()
                }
            ),
            RefreshDecision::Rotate
        );
        assert_eq!(
            decide_refresh(None, Some(&refresh()), &refresh_request()),
            RefreshDecision::Rotate
        );
    }

    #[test]
    fn family_revoke_preserves_independent_and_ungrouped_tokens() {
        assert!(!retain_other_family(Some("family-a"), "family-a"));
        assert!(retain_other_family(Some("family-b"), "family-a"));
        assert!(retain_other_family(None, "family-a"));
    }

    #[test]
    fn constant_time_compare_accepts_equal_only() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"other!"));
        assert!(!constant_time_eq(b"secret", b"short"));
    }
}
