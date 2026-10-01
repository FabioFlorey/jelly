pub mod agent;
pub mod artifacts;
pub mod browser;
pub mod error;
pub mod execution;
pub mod mcp;
mod mcp_auth;
pub mod primitives;

pub use agent::{
    AgentBuiltin, AgentBuiltinExecution, AgentCatalogError, AgentToolBinding, AgentToolCatalog,
    AgentToolEntry, AgentToolSpec, JELLY_MCP_SURFACE, McpSurface, RawCdpAccess,
    active_agent_catalog, agent_catalog_for_surface, agent_catalog_from_config,
    browser_call_input_schema, browser_events_input_schema, browser_schema_input_schema,
    cdp_call_input_schema, execute_browser_call, execute_browser_events, execute_browser_schema,
    execute_cdp_call, large_surface_agent_catalog, large_surface_agent_catalog_with_raw,
    small_surface_agent_catalog, validate_browser_call, validate_browser_events, validate_cdp_call,
};
pub use artifacts::{
    mark_artifact_verified, register_download, register_recording, register_screenshot,
    sanitize_url, verify_artifact,
};
pub use browser::{
    ACTIVE_TARGET, ARTIFACT_DIR, ARTIFACT_META_DIR, BROWSER_MODE, BROWSER_PID, BROWSER_READY,
    BROWSER_STOP, BrowserSession, CdpEvent, CdpEventCursor, CdpEventFilter, CdpEventPoll,
    CdpEventRingStats, DEFAULT_CDP_EVENT_MAX_BYTES, DEFAULT_CDP_EVENT_MAX_COUNT,
    DEFAULT_CDP_EVENT_POLL_LIMIT, DOWNLOAD_DIR, ENDPOINT, HEADLESS_PROFILE_DIR, INJECTION_DIR,
    LOG_DIR, LOGICAL_TARGETS, LOGICAL_TARGETS_LOCK, LogicalTarget, MAX_CDP_EVENT_POLL_LIMIT,
    MAX_CDP_EVENT_SUBSCRIPTIONS, NETWORK_DIR, PAGE_TARGET, PROFILE_DIR, RECORDING_DIR, ROOT,
    ROUTINE_STATE_DIR, RUNTIME, SCREENSHOT_DIR, STATE_DIR, Target, TargetRegistry,
};
pub use error::{
    CdpError, ErrorKind, JellyError, cdp_error, classify_error, error_details, jelly_error,
    structured_error,
};
pub use execution::{
    ArgKind, ArgSpec, CategorySpec, PrimitiveSpec, ToolCategorySpec, ToolSpec,
    browser_capabilities, browser_operation_schema, capabilities, category, category_specs,
    execute_browser_primitive, execute_named_browser_primitive, is_browser_primitive,
    lightweight_tools, named_primitive_input_schema, new_id, prepare_named_primitive_args,
    primitive_catalog, primitive_schema, primitive_specs, primitive_usage, record_step,
    redact_args, redact_tool_args, run_cli_primitive, search_browser_operations, search_tools,
    tool_category_specs, tool_schema, tool_specs, tools_in, validate_named_primitive_contract,
};

pub type Error = Box<dyn std::error::Error>;
