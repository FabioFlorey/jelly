//! Pure decisions for the MCP browser-session cache.

use crate::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionPlan {
    /// No cache: connect once for this call and drop the session afterwards.
    Ephemeral,
    /// Persistent mode with no live cached session.
    ConnectCached,
    /// Persistent mode with a live cached session.
    ReuseCached,
}

pub(crate) fn plan_session(persistent: bool, has_cached_session: bool) -> SessionPlan {
    match (persistent, has_cached_session) {
        (false, _) => SessionPlan::Ephemeral,
        (true, false) => SessionPlan::ConnectCached,
        (true, true) => SessionPlan::ReuseCached,
    }
}

pub(crate) fn invalidate_cached_session(error: ErrorKind) -> bool {
    error == ErrorKind::BrowserUnavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ephemeral_sessions_never_consult_the_cache() {
        for cached in [false, true] {
            assert_eq!(plan_session(false, cached), SessionPlan::Ephemeral);
        }
    }

    #[test]
    fn persistent_sessions_connect_once_then_reuse() {
        assert_eq!(plan_session(true, false), SessionPlan::ConnectCached);
        assert_eq!(plan_session(true, true), SessionPlan::ReuseCached);
    }

    #[test]
    fn only_browser_unavailable_invalidates_a_cached_session() {
        assert!(invalidate_cached_session(ErrorKind::BrowserUnavailable));
        assert!(!invalidate_cached_session(ErrorKind::TargetNotFound));
        assert!(!invalidate_cached_session(ErrorKind::InvalidArguments));
        assert!(!invalidate_cached_session(ErrorKind::Internal));
    }
}
