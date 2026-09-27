mod session;
mod target;

pub use session::BrowserSession;
pub use target::Target;

pub const ROOT: &str = env!("CARGO_MANIFEST_DIR");
pub const RUNTIME: &str = "/data/jelly-runtime";
pub const STATE_DIR: &str = "/data/jelly-runtime/state";
pub const PROFILE_DIR: &str = "/data/jelly-runtime/profiles/headed";
pub const HEADLESS_PROFILE_DIR: &str = "/data/jelly-runtime/profiles/headless";
pub const ARTIFACT_DIR: &str = "/data/jelly-runtime/artifacts";
pub const SCREENSHOT_DIR: &str = "/data/jelly-runtime/artifacts/screenshots";
pub const DOWNLOAD_DIR: &str = "/data/jelly-runtime/artifacts/downloads";
pub const NETWORK_DIR: &str = "/data/jelly-runtime/network";
pub const LOG_DIR: &str = "/data/jelly-runtime/logs";
pub const ROUTINE_STATE_DIR: &str = "/data/jelly-runtime/routines";
pub const INJECTION_DIR: &str = "/data/jelly-runtime/injections";
pub const ENDPOINT: &str = "/data/jelly-runtime/state/cdp_endpoint";
pub const ACTIVE_TARGET: &str = "/data/jelly-runtime/state/active_target_id";
pub const PAGE_TARGET: &str = "/data/jelly-runtime/state/page_target_id";
pub const BROWSER_PID: &str = "/data/jelly-runtime/state/browser.pid";
pub const BROWSER_STOP: &str = "/data/jelly-runtime/state/browser.stop";
pub const BROWSER_MODE: &str = "/data/jelly-runtime/state/browser_mode";
pub const BROWSER_READY: &str = "/data/jelly-runtime/state/browser_ready";
