use super::{
    AgentBuiltin,
    schema::{primitive_input_schema, system_input_schema, tool_output_schema},
};
use crate::{PrimitiveSpec, ToolSpec, primitive_specs, tool_specs};
use serde_json::Value;
use std::{collections::HashMap, env, error::Error as StdError, fmt, sync::OnceLock};

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

#[derive(Debug, Clone, Copy)]
pub enum AgentToolBinding {
    BrowserPrimitive(&'static PrimitiveSpec),
    SystemTool(&'static ToolSpec),
    Builtin(AgentBuiltin),
}

impl AgentToolBinding {
    pub const fn source_name(self) -> &'static str {
        match self {
            Self::BrowserPrimitive(spec) => spec.name,
            Self::SystemTool(spec) => spec.name,
            Self::Builtin(builtin) => builtin.name(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AgentToolSpec {
    name: &'static str,
    description: &'static str,
    binding: AgentToolBinding,
}

impl AgentToolSpec {
    pub const fn projected(
        name: &'static str,
        description: &'static str,
        binding: AgentToolBinding,
    ) -> Self {
        Self {
            name,
            description,
            binding,
        }
    }

    pub const fn browser_primitive(spec: &'static PrimitiveSpec) -> Self {
        Self::projected(
            spec.name,
            spec.description,
            AgentToolBinding::BrowserPrimitive(spec),
        )
    }

    pub const fn system_tool(spec: &'static ToolSpec) -> Self {
        Self::projected(
            spec.name,
            spec.description,
            AgentToolBinding::SystemTool(spec),
        )
    }

    pub const fn builtin(builtin: AgentBuiltin) -> Self {
        Self::projected(
            builtin.name(),
            builtin.description(),
            AgentToolBinding::Builtin(builtin),
        )
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn description(&self) -> &'static str {
        self.description
    }

    pub const fn binding(&self) -> AgentToolBinding {
        self.binding
    }
}

#[derive(Debug, Clone)]
pub struct AgentToolEntry {
    spec: AgentToolSpec,
    input_schema: Value,
    output_schema: Value,
}

impl AgentToolEntry {
    pub const fn name(&self) -> &'static str {
        self.spec.name()
    }

    pub const fn description(&self) -> &'static str {
        self.spec.description()
    }

    pub const fn binding(&self) -> AgentToolBinding {
        self.spec.binding()
    }

    pub const fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    pub const fn output_schema(&self) -> &Value {
        &self.output_schema
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCatalogError {
    EmptyCatalog,
    EmptyName,
    EmptyDescription { name: &'static str },
    DuplicateName { name: &'static str },
    MissingInputSchema { name: &'static str },
    InvalidBrowserPrimitiveContract { name: &'static str, reason: String },
    InvalidInputSchema { name: &'static str },
    InvalidOutputSchema { name: &'static str },
}

impl fmt::Display for AgentCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCatalog => f.write_str("agent tool catalog must not be empty"),
            Self::EmptyName => f.write_str("agent tool name must not be empty"),
            Self::EmptyDescription { name } => {
                write!(f, "agent tool {name} must have a nonempty description")
            }
            Self::DuplicateName { name } => write!(f, "duplicate agent tool name: {name}"),
            Self::MissingInputSchema { name } => {
                write!(f, "agent tool {name} has no input schema")
            }
            Self::InvalidBrowserPrimitiveContract { name, reason } => {
                write!(
                    f,
                    "agent tool {name} binds to an invalid named primitive contract: {reason}"
                )
            }
            Self::InvalidInputSchema { name } => {
                write!(f, "agent tool {name} input schema must be an object schema")
            }
            Self::InvalidOutputSchema { name } => {
                write!(
                    f,
                    "agent tool {name} output schema must be an object schema"
                )
            }
        }
    }
}

impl StdError for AgentCatalogError {}

#[derive(Debug, Clone)]
pub struct AgentToolCatalog {
    entries: Vec<AgentToolEntry>,
    by_name: HashMap<&'static str, usize>,
}

impl AgentToolCatalog {
    pub fn try_new(specs: Vec<AgentToolSpec>) -> Result<Self, AgentCatalogError> {
        if specs.is_empty() {
            return Err(AgentCatalogError::EmptyCatalog);
        }

        let mut entries = Vec::with_capacity(specs.len());
        let mut by_name = HashMap::with_capacity(specs.len());

        for spec in specs {
            if spec.name().trim().is_empty() {
                return Err(AgentCatalogError::EmptyName);
            }
            if spec.description().trim().is_empty() {
                return Err(AgentCatalogError::EmptyDescription { name: spec.name() });
            }
            if by_name.contains_key(spec.name()) {
                return Err(AgentCatalogError::DuplicateName { name: spec.name() });
            }

            let input_schema = match spec.binding() {
                AgentToolBinding::BrowserPrimitive(primitive) => primitive_input_schema(primitive)
                    .map_err(
                        |reason| AgentCatalogError::InvalidBrowserPrimitiveContract {
                            name: spec.name(),
                            reason,
                        },
                    )?,
                AgentToolBinding::SystemTool(tool) => system_input_schema(tool.name)
                    .ok_or(AgentCatalogError::MissingInputSchema { name: spec.name() })?,
                AgentToolBinding::Builtin(builtin) => builtin.input_schema(),
            };
            if !is_object_schema(&input_schema) {
                return Err(AgentCatalogError::InvalidInputSchema { name: spec.name() });
            }

            let output_schema = tool_output_schema();
            if !is_object_schema(&output_schema) {
                return Err(AgentCatalogError::InvalidOutputSchema { name: spec.name() });
            }

            let index = entries.len();
            by_name.insert(spec.name(), index);
            entries.push(AgentToolEntry {
                spec,
                input_schema,
                output_schema,
            });
        }

        Ok(Self { entries, by_name })
    }

    pub fn large_surface() -> Result<Self, AgentCatalogError> {
        Self::large_surface_with_raw(crate::RawCdpAccess::Disabled)
    }

    pub fn large_surface_with_raw(raw_cdp: crate::RawCdpAccess) -> Result<Self, AgentCatalogError> {
        let mut specs = primitive_specs
            .iter()
            .map(AgentToolSpec::browser_primitive)
            .collect::<Vec<_>>();
        if raw_cdp.enabled() {
            specs.push(AgentToolSpec::builtin(AgentBuiltin::CdpCall));
        }
        specs.extend(tool_specs.iter().map(AgentToolSpec::system_tool));
        Self::try_new(specs)
    }

    pub fn small_surface(raw_cdp: crate::RawCdpAccess) -> Result<Self, AgentCatalogError> {
        let mut specs = vec![
            AgentToolSpec::builtin(AgentBuiltin::BrowserSchema),
            AgentToolSpec::builtin(AgentBuiltin::browser_call(raw_cdp)),
            AgentToolSpec::builtin(AgentBuiltin::BrowserEvents),
        ];
        specs.extend(tool_specs.iter().map(AgentToolSpec::system_tool));
        Self::try_new(specs)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &AgentToolEntry> {
        self.entries.iter()
    }

    pub fn get(&self, name: &str) -> Option<&AgentToolEntry> {
        self.by_name.get(name).map(|index| &self.entries[*index])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn is_object_schema(schema: &Value) -> bool {
    if schema.get("type").and_then(Value::as_str) == Some("object")
        && schema.get("properties").is_some_and(Value::is_object)
    {
        return true;
    }

    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .is_some_and(|branches| !branches.is_empty() && branches.iter().all(is_object_schema))
}

pub fn large_surface_agent_catalog() -> &'static AgentToolCatalog {
    large_surface_agent_catalog_with_raw(crate::RawCdpAccess::Disabled)
}

pub fn large_surface_agent_catalog_with_raw(
    raw_cdp: crate::RawCdpAccess,
) -> &'static AgentToolCatalog {
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

pub fn small_surface_agent_catalog(raw_cdp: crate::RawCdpAccess) -> &'static AgentToolCatalog {
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
    raw_cdp: crate::RawCdpAccess,
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
    let surface = McpSurface::parse(surface_value)?;
    let raw_cdp = crate::RawCdpAccess::parse(raw_cdp_value)?;
    Ok(agent_catalog_for_surface(surface, raw_cdp))
}

pub fn active_agent_catalog() -> Result<&'static AgentToolCatalog, String> {
    static ACTIVE: OnceLock<Result<&'static AgentToolCatalog, String>> = OnceLock::new();
    match ACTIVE.get_or_init(|| {
        let surface = McpSurface::from_env()?;
        let raw_cdp = crate::RawCdpAccess::from_env()?;
        Ok(agent_catalog_for_surface(surface, raw_cdp))
    }) {
        Ok(catalog) => Ok(*catalog),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_spec() -> &'static PrimitiveSpec {
        primitive_specs
            .iter()
            .find(|spec| spec.name == "click")
            .unwrap()
    }

    #[test]
    fn large_surface_catalog_preserves_registry_order_and_metadata() {
        let catalog = AgentToolCatalog::large_surface().unwrap();
        assert_eq!(catalog.len(), primitive_specs.len() + tool_specs.len());

        let mut expected = primitive_specs
            .iter()
            .map(|spec| (spec.name, spec.description))
            .chain(tool_specs.iter().map(|spec| (spec.name, spec.description)));

        for tool in catalog.iter() {
            let (name, description) = expected.next().unwrap();
            assert_eq!(tool.name(), name);
            assert_eq!(tool.description(), description);
        }
        assert!(expected.next().is_none());
    }

    #[test]
    fn catalog_can_be_an_intentional_projection_with_agent_facing_metadata() {
        let click = click_spec();
        let catalog = AgentToolCatalog::try_new(vec![AgentToolSpec::projected(
            "browser-action",
            "Agent-facing projection of one semantic browser capability.",
            AgentToolBinding::BrowserPrimitive(click),
        )])
        .unwrap();

        assert_eq!(catalog.len(), 1);
        assert!(primitive_specs.len() > catalog.len());
        let tool = catalog.get("browser-action").unwrap();
        assert_eq!(tool.name(), "browser-action");
        assert_eq!(tool.binding().source_name(), "click");
        assert_eq!(tool.input_schema()["required"][0], "target");
    }

    #[test]
    fn builtin_agent_tools_are_first_class_catalog_entries() {
        let catalog = AgentToolCatalog::try_new(vec![
            AgentToolSpec::builtin(AgentBuiltin::BrowserSchema),
            AgentToolSpec::builtin(AgentBuiltin::browser_call(crate::RawCdpAccess::Disabled)),
            AgentToolSpec::builtin(AgentBuiltin::BrowserEvents),
        ])
        .unwrap();

        let schema = catalog.get("browser-schema").unwrap();
        assert_eq!(schema.binding().source_name(), "browser-schema");
        assert_eq!(schema.input_schema()["oneOf"].as_array().unwrap().len(), 3);
        assert_eq!(schema.output_schema()["type"], "object");

        let call = catalog.get("browser-call").unwrap();
        assert_eq!(call.binding().source_name(), "browser-call");
        assert_eq!(call.input_schema()["properties"]["calls"]["minItems"], 1);
        assert!(
            call.input_schema()["properties"]["calls"]["items"]
                .get("oneOf")
                .is_none()
        );
        assert_eq!(call.output_schema()["type"], "object");

        let events = catalog.get("browser-events").unwrap();
        assert_eq!(events.binding().source_name(), "browser-events");
        assert_eq!(events.input_schema()["oneOf"].as_array().unwrap().len(), 3);
        assert_eq!(events.output_schema()["type"], "object");

        let raw_catalog = AgentToolCatalog::try_new(vec![AgentToolSpec::builtin(
            AgentBuiltin::browser_call(crate::RawCdpAccess::Enabled),
        )])
        .unwrap();
        let raw_call = raw_catalog.get("browser-call").unwrap();
        assert_eq!(
            raw_call.input_schema()["properties"]["calls"]["items"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn duplicate_public_names_are_rejected() {
        let click = click_spec();
        let error = AgentToolCatalog::try_new(vec![
            AgentToolSpec::browser_primitive(click),
            AgentToolSpec::browser_primitive(click),
        ])
        .unwrap_err();
        assert_eq!(error, AgentCatalogError::DuplicateName { name: "click" });
    }

    #[test]
    fn empty_catalog_is_rejected() {
        assert_eq!(
            AgentToolCatalog::try_new(Vec::new()).unwrap_err(),
            AgentCatalogError::EmptyCatalog
        );
    }

    #[test]
    fn invalid_named_primitive_contract_is_rejected_before_publication() {
        fn no_op(
            _browser: &mut crate::BrowserSession,
            _args: &[String],
        ) -> Result<String, crate::Error> {
            Ok(String::new())
        }

        static BAD_ARGS: &[crate::ArgSpec] = &[
            crate::ArgSpec::opt("first", crate::ArgKind::String),
            crate::ArgSpec::req("second", crate::ArgKind::String),
        ];
        static BAD: PrimitiveSpec = PrimitiveSpec {
            name: "bad-named-shape",
            description: "Test-only invalid positional grammar.",
            usage: "bad-named-shape [first] <second>",
            category: "test",
            args: BAD_ARGS,
            max_args: Some(2),
            handler: no_op,
        };

        let error =
            AgentToolCatalog::try_new(vec![AgentToolSpec::browser_primitive(&BAD)]).unwrap_err();
        match error {
            AgentCatalogError::InvalidBrowserPrimitiveContract { name, reason } => {
                assert_eq!(name, "bad-named-shape");
                assert!(reason.contains("required argument second after an optional"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn missing_system_schema_is_rejected_instead_of_silently_dropping_the_tool() {
        static UNKNOWN: ToolSpec = ToolSpec {
            name: "unknown-agent-system-tool",
            description: "Test-only system tool with no agent schema mapping.",
            usage: "unknown-agent-system-tool",
            category: "test",
        };
        let error =
            AgentToolCatalog::try_new(vec![AgentToolSpec::system_tool(&UNKNOWN)]).unwrap_err();
        assert_eq!(
            error,
            AgentCatalogError::MissingInputSchema {
                name: "unknown-agent-system-tool"
            }
        );
    }

    #[test]
    fn name_lookup_uses_the_validated_catalog_index() {
        let catalog =
            AgentToolCatalog::try_new(vec![AgentToolSpec::browser_primitive(click_spec())])
                .unwrap();
        assert_eq!(
            catalog.get("click").unwrap().binding().source_name(),
            "click"
        );
        assert!(catalog.get("fill").is_none());
    }

    #[test]
    fn mcp_surface_parser_accepts_only_canonical_names() {
        assert_eq!(McpSurface::parse(None).unwrap(), McpSurface::SmallSurface);
        assert_eq!(McpSurface::SmallSurface.as_str(), "small-surface");
        assert_eq!(McpSurface::LargeSurface.as_str(), "large-surface");
        assert!(McpSurface::parse(Some("")).is_err());
        assert_eq!(
            McpSurface::parse(Some("  LARGE-SURFACE ")).unwrap(),
            McpSurface::LargeSurface
        );
        assert_eq!(
            McpSurface::parse(Some(" Small-Surface ")).unwrap(),
            McpSurface::SmallSurface
        );
        for unsupported in ["legacy", "compact", "both"] {
            let error = McpSurface::parse(Some(unsupported)).unwrap_err();
            assert!(error.contains("must be large-surface or small-surface"));
        }
    }

    #[test]
    fn rollout_config_applies_raw_cdp_policy_to_both_surfaces() {
        let large_disabled = agent_catalog_from_config(Some("large-surface"), None).unwrap();
        assert!(large_disabled.get("cdp-call").is_none());

        let large_enabled = agent_catalog_from_config(Some("large-surface"), Some("true")).unwrap();
        assert!(large_enabled.get("click").is_some());
        assert!(large_enabled.get("cdp-call").is_some());
        assert!(large_enabled.get("browser-call").is_none());

        let small_disabled = agent_catalog_from_config(Some("small-surface"), None).unwrap();
        assert!(
            small_disabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]
                ["items"]
                .get("oneOf")
                .is_none()
        );

        let small_enabled = agent_catalog_from_config(Some("small-surface"), Some("true")).unwrap();
        assert_eq!(
            small_enabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]
                ["items"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        assert!(agent_catalog_from_config(Some("large-surface"), Some("invalid")).is_err());
        assert!(agent_catalog_from_config(Some("small-surface"), Some("invalid")).is_err());
        assert!(agent_catalog_from_config(Some("both"), None).is_err());
    }

    #[test]
    fn small_surface_catalog_replaces_browser_primitives_but_preserves_system_tools() {
        let catalog = AgentToolCatalog::small_surface(crate::RawCdpAccess::Disabled).unwrap();
        assert_eq!(catalog.len(), tool_specs.len() + 3);

        for name in ["browser-schema", "browser-call", "browser-events"] {
            assert!(
                catalog.get(name).is_some(),
                "small-surface catalog missing {name}"
            );
        }
        for spec in tool_specs {
            assert!(
                catalog.get(spec.name).is_some(),
                "small-surface catalog missing system tool {}",
                spec.name
            );
        }
        for spec in primitive_specs {
            assert!(
                catalog.get(spec.name).is_none(),
                "small-surface catalog unexpectedly publishes primitive {}",
                spec.name
            );
        }
    }

    #[test]
    fn system_tool_surface_is_identical_between_large_and_small_surfaces() {
        let large = large_surface_agent_catalog();
        let small = small_surface_agent_catalog(crate::RawCdpAccess::Disabled);

        let large_system = large
            .iter()
            .filter_map(|entry| match entry.binding() {
                AgentToolBinding::SystemTool(spec) => Some((entry.name(), spec.name)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let small_system = small
            .iter()
            .filter_map(|entry| match entry.binding() {
                AgentToolBinding::SystemTool(spec) => Some((entry.name(), spec.name)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let registry = tool_specs
            .iter()
            .map(|spec| (spec.name, spec.name))
            .collect::<Vec<_>>();

        assert_eq!(large_system, registry);
        assert_eq!(small_system, registry);
    }

    #[test]
    fn hitl_and_profile_import_remain_explicit_system_tools_during_browser_rollout() {
        for catalog in [
            large_surface_agent_catalog(),
            small_surface_agent_catalog(crate::RawCdpAccess::Disabled),
        ] {
            let hitl = catalog.get("hitl").expect("hitl must remain published");
            assert!(
                matches!(hitl.binding(), AgentToolBinding::SystemTool(spec) if spec.name == "hitl")
            );

            let profile = catalog
                .get("profile-import")
                .expect("profile-import must remain published during step 14");
            assert!(
                matches!(profile.binding(), AgentToolBinding::SystemTool(spec) if spec.name == "profile-import")
            );
        }
    }

    #[test]
    fn small_surface_raw_cdp_policy_is_reflected_in_browser_call_schema() {
        let disabled = small_surface_agent_catalog(crate::RawCdpAccess::Disabled);
        assert!(
            disabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]["items"]
                .get("oneOf")
                .is_none()
        );

        let enabled = small_surface_agent_catalog(crate::RawCdpAccess::Enabled);
        assert_eq!(
            enabled.get("browser-call").unwrap().input_schema()["properties"]["calls"]["items"]
                ["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}
