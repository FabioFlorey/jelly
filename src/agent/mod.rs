mod browser_call;
mod browser_events;
mod browser_schema;
mod builtin;
mod catalog;
mod catalog_build;
mod catalog_cache;
mod catalog_config;
mod schema;
mod surface;

pub use browser_call::{
    RawCdpAccess, browser_call_input_schema, execute_browser_call, validate_browser_call,
};
pub use browser_events::execute_browser_events;
pub use browser_schema::execute_browser_schema;
pub use builtin::{AgentBuiltin, AgentBuiltinExecution};
pub use catalog::{
    AgentCatalogError, AgentToolBinding, AgentToolCatalog, AgentToolEntry, AgentToolSpec,
};
pub use catalog_cache::{
    active_agent_catalog, agent_catalog_for_surface, agent_catalog_from_config,
};

pub use surface::McpSurface;
