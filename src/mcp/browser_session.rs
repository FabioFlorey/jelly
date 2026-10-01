use super::ToolFailure;
use crate::{BrowserSession, ErrorKind};
use std::{
    env,
    sync::{Mutex, OnceLock},
};

static MCP_BROWSER_SESSION: OnceLock<Mutex<Option<BrowserSession>>> = OnceLock::new();

pub(super) fn persistent_mcp_session_enabled() -> bool {
    !env::var("JELLY_MCP_PERSISTENT_SESSION")
        .ok()
        .is_some_and(|value| matches!(value.trim(), "0" | "false" | "off"))
}

pub(super) fn reset_mcp_browser_session() {
    if let Some(sessions) = MCP_BROWSER_SESSION.get()
        && let Ok(mut guard) = sessions.lock()
    {
        *guard = None;
    }
}

pub(super) fn map_browser_failure(error: crate::Error) -> ToolFailure {
    ToolFailure::from_error(error)
}

pub(super) fn with_mcp_browser_session<F>(operation: F) -> Result<String, ToolFailure>
where
    F: FnOnce(&mut BrowserSession) -> Result<String, crate::Error>,
{
    if !persistent_mcp_session_enabled() {
        let mut browser = BrowserSession::connect().map_err(map_browser_failure)?;
        browser.sync_active_target().map_err(map_browser_failure)?;
        return operation(&mut browser).map_err(map_browser_failure);
    }

    let sessions = MCP_BROWSER_SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = sessions.lock().map_err(|_| {
        ToolFailure::new(
            ErrorKind::Internal,
            "persistent browser session lock is poisoned",
            true,
        )
    })?;
    if guard.is_none() {
        *guard = Some(BrowserSession::connect().map_err(map_browser_failure)?);
    }

    let browser = guard.as_mut().expect("session initialized");
    if let Err(error) = browser.sync_active_target() {
        let failure = map_browser_failure(error);
        if failure.kind == ErrorKind::BrowserUnavailable {
            *guard = None;
        }
        return Err(failure);
    }

    let result = operation(browser).map_err(map_browser_failure);
    if result
        .as_ref()
        .err()
        .is_some_and(|failure| failure.kind == ErrorKind::BrowserUnavailable)
    {
        *guard = None;
    }
    result
}
