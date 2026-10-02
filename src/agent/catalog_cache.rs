use super::{
    McpSurface, RawCdpAccess, catalog::AgentToolCatalog, catalog_config::AgentCatalogConfig,
};
use std::sync::OnceLock;

#[cfg(test)]
pub fn large_surface_agent_catalog() -> &'static AgentToolCatalog {
    large_surface_agent_catalog_with_raw(RawCdpAccess::Disabled)
}

pub fn large_surface_agent_catalog_with_raw(raw_cdp: RawCdpAccess) -> &'static AgentToolCatalog {
    static DISABLED: OnceLock<AgentToolCatalog> = OnceLock::new();
    static ENABLED: OnceLock<AgentToolCatalog> = OnceLock::new();
    let slot = if raw_cdp.enabled() {
        &ENABLED
    } else {
        &DISABLED
    };
    slot.get_or_init(|| {
        AgentToolCatalog::large_surface_with_raw(raw_cdp)
            .unwrap_or_else(|error| panic!("invalid large-surface agent tool catalog: {error}"))
    })
}

pub fn small_surface_agent_catalog(raw_cdp: RawCdpAccess) -> &'static AgentToolCatalog {
    static DISABLED: OnceLock<AgentToolCatalog> = OnceLock::new();
    static ENABLED: OnceLock<AgentToolCatalog> = OnceLock::new();
    let slot = if raw_cdp.enabled() {
        &ENABLED
    } else {
        &DISABLED
    };
    slot.get_or_init(|| {
        AgentToolCatalog::small_surface(raw_cdp)
            .unwrap_or_else(|error| panic!("invalid small-surface agent tool catalog: {error}"))
    })
}

pub fn agent_catalog_for_surface(
    surface: McpSurface,
    raw_cdp: RawCdpAccess,
) -> &'static AgentToolCatalog {
    match surface {
        McpSurface::LargeSurface => large_surface_agent_catalog_with_raw(raw_cdp),
        McpSurface::SmallSurface => small_surface_agent_catalog(raw_cdp),
    }
}

pub fn agent_catalog_from_config(
    surface_value: Option<&str>,
    raw_cdp_value: Option<&str>,
) -> Result<&'static AgentToolCatalog, String> {
    let config = AgentCatalogConfig::parse(surface_value, raw_cdp_value)?;
    Ok(agent_catalog_for_surface(config.surface, config.raw_cdp))
}

pub fn active_agent_catalog() -> Result<&'static AgentToolCatalog, String> {
    static ACTIVE: OnceLock<Result<&'static AgentToolCatalog, String>> = OnceLock::new();
    match ACTIVE.get_or_init(|| {
        let config = AgentCatalogConfig::from_config()?;
        Ok(agent_catalog_for_surface(config.surface, config.raw_cdp))
    }) {
        Ok(catalog) => Ok(*catalog),
        Err(error) => Err(error.clone()),
    }
}
