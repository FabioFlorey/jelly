pub mod artifacts;
pub mod browser;
pub mod error;
pub mod execution;
pub mod mcp;
mod mcp_auth;
pub mod primitives;

pub use artifacts::{
    mark_artifact_verified, register_download, register_screenshot, verify_artifact,
};
pub use browser::{
    ACTIVE_TARGET, ARTIFACT_DIR, ARTIFACT_META_DIR, BROWSER_MODE, BROWSER_PID, BROWSER_READY,
    BROWSER_STOP, BrowserSession, DOWNLOAD_DIR, ENDPOINT, HEADLESS_PROFILE_DIR, INJECTION_DIR,
    LOG_DIR, NETWORK_DIR, PAGE_TARGET, PROFILE_DIR, ROOT, ROUTINE_STATE_DIR, RUNTIME,
    SCREENSHOT_DIR, STATE_DIR, Target,
};
pub use error::{ErrorKind, JellyError, classify_error, jelly_error};
pub use execution::{
    ArgKind, ArgSpec, CategorySpec, PrimitiveSpec, ToolCategorySpec, ToolSpec, capabilities,
    category, category_specs, execute_browser_primitive, is_browser_primitive, lightweight_tools,
    new_id, primitive_catalog, primitive_schema, primitive_specs, primitive_usage, redact_args,
    run_cli_primitive, search_tools, tool_category_specs, tool_schema, tool_specs, tools_in,
};

pub type Error = Box<dyn std::error::Error>;
