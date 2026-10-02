#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSurface {
    LargeSurface,
    SmallSurface,
}

impl McpSurface {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LargeSurface => "large-surface",
            Self::SmallSurface => "small-surface",
        }
    }

    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let Some(value) = value else {
            return Ok(Self::SmallSurface);
        };
        match value.trim().to_ascii_lowercase().as_str() {
            "large-surface" => Ok(Self::LargeSurface),
            "small-surface" => Ok(Self::SmallSurface),
            other => Err(format!(
                "mcp.surface must be large-surface or small-surface; got {other}"
            )),
        }
    }

    pub fn from_config() -> Result<Self, String> {
        Self::parse(Some(&crate::config::config().mcp.surface))
    }
}
