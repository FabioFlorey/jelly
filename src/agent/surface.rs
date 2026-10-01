use std::env;

pub const JELLY_MCP_SURFACE: &str = "JELLY_MCP_SURFACE";

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
                "{JELLY_MCP_SURFACE} must be large-surface or small-surface; got {other}"
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        match env::var(JELLY_MCP_SURFACE) {
            Ok(value) => Self::parse(Some(&value)),
            Err(env::VarError::NotPresent) => Ok(Self::SmallSurface),
            Err(env::VarError::NotUnicode(_)) => {
                Err(format!("{JELLY_MCP_SURFACE} must contain valid UTF-8"))
            }
        }
    }
}
