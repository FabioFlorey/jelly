use super::{McpSurface, RawCdpAccess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentCatalogConfig {
    pub(crate) surface: McpSurface,
    pub(crate) raw_cdp: RawCdpAccess,
}

impl AgentCatalogConfig {
    pub(crate) fn parse(
        surface_value: Option<&str>,
        raw_cdp_value: Option<&str>,
    ) -> Result<Self, String> {
        Ok(Self {
            surface: McpSurface::parse(surface_value)?,
            raw_cdp: RawCdpAccess::parse(raw_cdp_value)?,
        })
    }

    pub(crate) fn from_env() -> Result<Self, String> {
        Ok(Self {
            surface: McpSurface::from_env()?,
            raw_cdp: RawCdpAccess::from_env()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parses_surface_and_raw_policy_together() {
        assert_eq!(
            AgentCatalogConfig::parse(Some("large-surface"), Some("true")).unwrap(),
            AgentCatalogConfig {
                surface: McpSurface::LargeSurface,
                raw_cdp: RawCdpAccess::Enabled,
            }
        );
        assert_eq!(
            AgentCatalogConfig::parse(None, None).unwrap(),
            AgentCatalogConfig {
                surface: McpSurface::SmallSurface,
                raw_cdp: RawCdpAccess::Disabled,
            }
        );
        assert!(AgentCatalogConfig::parse(Some("legacy"), None).is_err());
        assert!(AgentCatalogConfig::parse(None, Some("yes")).is_err());
    }
}
