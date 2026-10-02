//! Jelly browser instrumentation and agent-facing execution library.
//!
//! The supported Rust surface is intentionally small and is primarily consumed by
//! Jelly's own binaries, generated-tool index, and regression probes. Implementation
//! modules stay private unless a package binary must enter through that namespace.

mod agent;
mod artifacts;
mod browser;
/// Browser process launcher used by the `agent-open-browser` binary.
pub mod browser_launcher;
pub mod config;
mod error;
mod execution;
/// MCP HTTP/JSON-RPC server integration.
pub mod mcp;
mod mcp_auth;
mod primitives;
/// Browser recording entrypoint used by the `agent-record-browser` binary.
pub mod recording;
/// Routine execution entrypoint used by the `agent-call-routine` binary.
pub mod routine;

pub(crate) use agent::active_agent_catalog;
/// Agent-facing catalog types and semantic execution entrypoints.
///
/// Catalog bindings expose builtin, primitive, and system-tool descriptor types
/// transitively so callers can inspect the published Agent API without private paths.
pub use agent::{
    AgentBuiltin, AgentBuiltinExecution, AgentCatalogError, AgentToolBinding, AgentToolCatalog,
    AgentToolEntry, AgentToolSpec, McpSurface, RawCdpAccess, agent_catalog_for_surface,
    agent_catalog_from_config, browser_call_input_schema, execute_browser_call,
    execute_browser_events, execute_browser_schema, validate_browser_call,
};

/// Artifact registration and verification operations used by Jelly command binaries.
pub use artifacts::{
    mark_artifact_verified, register_download, register_screenshot, verify_artifact,
};
pub(crate) use artifacts::{register_recording, sanitize_url};

/// Browser session, target, event, and runtime-path types required by package clients.
///
/// Event and logical-target value types remain public because they appear in
/// `BrowserSession` method signatures.
pub use browser::{
    ACTIVE_TARGET, BROWSER_MODE, BROWSER_PID, BROWSER_READY, BROWSER_STOP, BrowserSession,
    CdpEvent, CdpEventCursor, CdpEventFilter, CdpEventPoll, CdpEventRingStats, DOWNLOAD_DIR,
    ENDPOINT, HEADLESS_PROFILE_DIR, INJECTION_DIR, LOG_DIR, LOGICAL_TARGETS, LogicalTarget,
    NETWORK_DIR, PAGE_TARGET, PROFILE_DIR, SCREENSHOT_DIR, Target,
};
pub(crate) use browser::{
    ARTIFACT_META_DIR, DEFAULT_CDP_EVENT_POLL_LIMIT, LOGICAL_TARGETS_LOCK,
    MAX_CDP_EVENT_POLL_LIMIT, RECORDING_DIR, ROUTINE_STATE_DIR, STATE_DIR,
};

#[cfg(test)]
pub(crate) use error::CdpError;
/// Typed semantic error classification and construction for Jelly command clients.
pub use error::{ErrorKind, classify_error, jelly_error};
pub(crate) use error::{cdp_error, error_details, structured_error};

/// Browser/system registry descriptors, discovery functions, and execution helpers
/// consumed by Jelly binaries, generated documentation, and regression probes.
pub use execution::{
    ArgKind, ArgSpec, CategorySpec, PrimitiveSpec, ToolCategorySpec, ToolSpec, capabilities,
    category_specs, execute_named_browser_primitive, lightweight_tools, new_id, primitive_specs,
    record_step, redact_tool_args, run_cli_primitive, search_tools, tool_category_specs,
    tool_schema, tool_specs, tools_in,
};
pub(crate) use execution::{
    browser_capabilities, browser_operation_schema, execute_browser_primitive,
    is_browser_primitive, named_primitive_input_schema, prepare_named_primitive_args,
    search_browser_operations,
};

/// Error type shared by Jelly's command and browser execution surfaces.
pub type Error = Box<dyn std::error::Error>;
