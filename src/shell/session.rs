//! Imperative MCP session adapter: cache, CDP connection, and target synchronization.
use crate::{
    BrowserSession, Error, ErrorKind, classify_error,
    core::session::{SessionPlan, invalidate_cached_session, plan_session},
    jelly_error,
};
use std::sync::{Mutex, OnceLock};

static MCP_BROWSER_SESSION: OnceLock<Mutex<Option<BrowserSession>>> = OnceLock::new();

pub(crate) fn persistent_mcp_session_enabled() -> bool {
    crate::config::config().mcp.persistent_session
}

pub(crate) fn reset_mcp_browser_session() {
    if let Some(sessions) = MCP_BROWSER_SESSION.get()
        && let Ok(mut guard) = sessions.lock()
    {
        *guard = None;
    }
}

fn invalidates_cache(error: &Error) -> bool {
    invalidate_cached_session(classify_error(error.as_ref()).0)
}

pub(crate) fn with_mcp_browser_session<F>(operation: F) -> Result<String, Error>
where
    F: FnOnce(&mut BrowserSession) -> Result<String, Error>,
{
    if plan_session(persistent_mcp_session_enabled(), false) == SessionPlan::Ephemeral {
        let mut browser = BrowserSession::connect()?;
        browser.sync_active_target()?;
        return operation(&mut browser);
    }

    let sessions = MCP_BROWSER_SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = sessions.lock().map_err(|_| {
        jelly_error(
            ErrorKind::Internal,
            "persistent browser session lock is poisoned",
            true,
        )
    })?;

    if plan_session(true, guard.is_some()) == SessionPlan::ConnectCached {
        *guard = Some(BrowserSession::connect()?);
    }

    let browser = guard.as_mut().expect("session initialized");
    if let Err(error) = browser.sync_active_target() {
        if invalidates_cache(&error) {
            *guard = None;
        }
        return Err(error);
    }

    let result = operation(browser);
    if result.as_ref().err().is_some_and(invalidates_cache) {
        *guard = None;
    }
    result
}
