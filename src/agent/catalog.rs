use super::{
    AgentBuiltin, RawCdpAccess,
    catalog_build::{large_surface_specs, small_surface_specs},
    schema::{primitive_input_schema, system_input_schema, tool_output_schema},
};
use crate::{PrimitiveSpec, ToolSpec};
#[cfg(test)]
use crate::{primitive_specs, tool_specs};
use serde_json::Value;
use std::{collections::HashMap, error::Error as StdError, fmt};

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
        Self::large_surface_with_raw(RawCdpAccess::Disabled)
    }

    pub fn large_surface_with_raw(raw_cdp: RawCdpAccess) -> Result<Self, AgentCatalogError> {
        Self::try_new(large_surface_specs(raw_cdp))
    }

    pub fn small_surface(raw_cdp: RawCdpAccess) -> Result<Self, AgentCatalogError> {
        Self::try_new(small_surface_specs(raw_cdp))
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

#[cfg(test)]
mod tests;
