mod events;
mod perf;
mod runtime;
mod session;
mod target;
mod targets;

pub use events::{
    CdpEvent, CdpEventCursor, CdpEventFilter, CdpEventPoll, CdpEventRingStats,
    DEFAULT_CDP_EVENT_MAX_BYTES, DEFAULT_CDP_EVENT_MAX_COUNT, DEFAULT_CDP_EVENT_POLL_LIMIT,
    MAX_CDP_EVENT_POLL_LIMIT, MAX_CDP_EVENT_SUBSCRIPTIONS,
};
pub(crate) use events::{CdpEventRing, CdpEventSubscriptions};
pub(crate) use runtime::{
    legacy_search_expression, legacy_snapshot_expression, page_runtime_bootstrap,
    search_expression, snapshot_expression,
};
pub use session::BrowserSession;
pub use target::Target;
pub use targets::{LogicalTarget, TargetRegistry};

pub const ROOT: &str = env!("CARGO_MANIFEST_DIR");
pub const RUNTIME: &str = "/data/jelly-runtime";
pub const STATE_DIR: &str = "/data/jelly-runtime/state";
pub const PROFILE_DIR: &str = "/data/jelly-runtime/profiles/headed";
pub const HEADLESS_PROFILE_DIR: &str = "/data/jelly-runtime/profiles/headless";
pub const ARTIFACT_DIR: &str = "/data/jelly-runtime/artifacts";
pub const ARTIFACT_META_DIR: &str = "/data/jelly-runtime/artifacts/metadata";
pub const SCREENSHOT_DIR: &str = "/data/jelly-runtime/artifacts/screenshots";
pub const RECORDING_DIR: &str = "/data/jelly-runtime/artifacts/recordings";
pub const DOWNLOAD_DIR: &str = "/data/jelly-runtime/artifacts/downloads";
pub const NETWORK_DIR: &str = "/data/jelly-runtime/network";
pub const LOG_DIR: &str = "/data/jelly-runtime/logs";
pub const ROUTINE_STATE_DIR: &str = "/data/jelly-runtime/routines";
pub const INJECTION_DIR: &str = "/data/jelly-runtime/injections";
pub const ENDPOINT: &str = "/data/jelly-runtime/state/cdp_endpoint";
pub const ACTIVE_TARGET: &str = "/data/jelly-runtime/state/active_target_id";
pub const LOGICAL_TARGETS: &str = "/data/jelly-runtime/state/logical_targets.json";
pub const LOGICAL_TARGETS_LOCK: &str = "/data/jelly-runtime/state/logical_targets.lock";
pub const PAGE_TARGET: &str = "/data/jelly-runtime/state/page_target_id";
pub const BROWSER_PID: &str = "/data/jelly-runtime/state/browser.pid";
pub const BROWSER_STOP: &str = "/data/jelly-runtime/state/browser.stop";
pub const BROWSER_MODE: &str = "/data/jelly-runtime/state/browser_mode";
pub const BROWSER_READY: &str = "/data/jelly-runtime/state/browser_ready";
