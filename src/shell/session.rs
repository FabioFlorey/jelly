//! MCP browser session registry, isolated by browser identity.
use crate::{
    BrowserSession, Error, ErrorKind, classify_error,
    core::session::{SessionPlan, invalidate_cached_session, plan_session},
    jelly_error,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    thread::JoinHandle,
};

type SessionSlot = Arc<Mutex<Option<BrowserSession>>>;
static MCP_BROWSER_SESSIONS: OnceLock<Mutex<HashMap<String, SessionSlot>>> = OnceLock::new();
static TRACKED_BROWSERS: OnceLock<Mutex<HashMap<String, JoinHandle<()>>>> = OnceLock::new();

fn current_key() -> String {
    if crate::config::config().browser.backend
        == crate::browser_backend::BrowserBackendKind::RemoteCdp
    {
        crate::config::selected_remote_id().expect("validated remote browser selector")
    } else {
        String::new()
    }
}

pub(crate) fn ensure_remote_tracker() -> Result<(), Error> {
    if crate::config::config().browser.backend
        != crate::browser_backend::BrowserBackendKind::RemoteCdp
    {
        return Ok(());
    }
    let remote = crate::config::selected_remote()
        .map_err(|reason| jelly_error(ErrorKind::InvalidArguments, reason, false))?;
    if remote.download_path.is_none() {
        return Ok(());
    }
    let tracker_registry = TRACKED_BROWSERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut tracked = tracker_registry
        .lock()
        .map_err(|_| jelly_error(ErrorKind::Internal, "remote tracker lock poisoned", false))?;
    let key = current_key();
    if !tracked
        .get(&key)
        .is_some_and(|handle| !handle.is_finished())
    {
        let endpoint = crate::config::selected_remote_endpoint()
            .map_err(|reason| jelly_error(ErrorKind::InvalidArguments, reason, false))?;
        tracked.remove(&key);
        let handle = crate::shell::downloads::start_download_tracker(&endpoint)?;
        tracked.insert(key, handle);
    }
    Ok(())
}

pub(crate) fn persistent_mcp_session_enabled() -> bool {
    crate::config::config().mcp.persistent_session
}

pub(crate) fn reset_mcp_browser_session() {
    if let Some(sessions) = MCP_BROWSER_SESSIONS.get()
        && let Ok(mut guard) = sessions.lock()
    {
        guard.remove(&current_key());
    }
}

fn invalidates_cache(error: &Error) -> bool {
    invalidate_cached_session(classify_error(error.as_ref()).0)
}

pub(crate) fn with_mcp_browser_session<F>(operation: F) -> Result<String, Error>
where
    F: FnOnce(&mut BrowserSession) -> Result<String, Error>,
{
    ensure_remote_tracker()?;
    if plan_session(persistent_mcp_session_enabled(), false) == SessionPlan::Ephemeral {
        let mut browser = BrowserSession::connect()?;
        browser.sync_active_target()?;
        return operation(&mut browser);
    }

    let registry = MCP_BROWSER_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let key = current_key();
    let slot = {
        let mut map = registry.lock().map_err(|_| {
            jelly_error(
                ErrorKind::Internal,
                "browser session registry poisoned",
                true,
            )
        })?;
        Arc::clone(map.entry(key).or_insert_with(|| Arc::new(Mutex::new(None))))
    };
    // Lock one browser only: requests for different IDs can execute concurrently.
    let mut guard = slot
        .lock()
        .map_err(|_| jelly_error(ErrorKind::Internal, "browser session lock poisoned", true))?;
    if guard.is_none() {
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
