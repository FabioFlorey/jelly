mod events;
mod perf;
mod runtime;
mod session;
mod target;
mod target_manager;
mod targets;
mod transport;

pub(crate) use events::EventState;
pub use events::{
    CdpEvent, CdpEventCursor, CdpEventFilter, CdpEventPoll, CdpEventRingStats,
    DEFAULT_CDP_EVENT_POLL_LIMIT, MAX_CDP_EVENT_POLL_LIMIT,
};
pub(crate) use runtime::{
    legacy_search_expression, legacy_snapshot_expression, page_runtime_bootstrap,
    search_expression, snapshot_expression,
};
pub use session::BrowserSession;
pub use target::Target;
pub(crate) use target_manager::TargetManager;
pub use targets::LogicalTarget;
pub(crate) use targets::TargetRegistry;

use crate::config::{RuntimePath, RuntimePathKind};

pub const STATE_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::State);
pub const PROFILE_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Profile);
pub const HEADLESS_PROFILE_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::HeadlessProfile);
pub const ARTIFACT_META_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::ArtifactMetadata);
pub const SCREENSHOT_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Screenshots);
pub const RECORDING_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Recordings);
pub const DOWNLOAD_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Downloads);
pub const NETWORK_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Network);
pub const LOG_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Logs);
pub const ROUTINE_STATE_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Routines);
pub const INJECTION_DIR: &RuntimePath = &RuntimePath(RuntimePathKind::Injections);
pub const ENDPOINT: &RuntimePath = &RuntimePath(RuntimePathKind::Endpoint);
pub const ACTIVE_TARGET: &RuntimePath = &RuntimePath(RuntimePathKind::ActiveTarget);
pub const LOGICAL_TARGETS: &RuntimePath = &RuntimePath(RuntimePathKind::LogicalTargets);
pub const LOGICAL_TARGETS_LOCK: &RuntimePath = &RuntimePath(RuntimePathKind::LogicalTargetsLock);
pub const PAGE_TARGET: &RuntimePath = &RuntimePath(RuntimePathKind::PageTarget);
pub const BROWSER_PID: &RuntimePath = &RuntimePath(RuntimePathKind::BrowserPid);
pub const BROWSER_STOP: &RuntimePath = &RuntimePath(RuntimePathKind::BrowserStop);
pub const BROWSER_MODE: &RuntimePath = &RuntimePath(RuntimePathKind::BrowserMode);
pub const BROWSER_READY: &RuntimePath = &RuntimePath(RuntimePathKind::BrowserReady);
