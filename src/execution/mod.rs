mod discovery;
mod registry;
mod tools;
mod tracing;

pub use discovery::{
    capabilities, category, lightweight_tools, search_tools, tool_schema, tools_in,
};
pub use registry::{
    ArgKind, ArgSpec, CATEGORIES as category_specs, CategorySpec, PRIMITIVES as primitive_specs,
    PrimitiveSpec,
};
pub use tools::{
    TOOL_CATEGORIES as tool_category_specs, TOOLS as tool_specs, ToolCategorySpec, ToolSpec,
    lookup_tool,
};
pub use tracing::{new_id, redact_args};

use crate::{BrowserSession, Error};
use std::{
    env,
    io::{self, Write},
    time::Instant,
};

pub fn is_browser_primitive(name: &str) -> bool {
    registry::lookup(name).is_some()
}
pub fn primitive_usage(name: &str) -> Option<&'static str> {
    registry::lookup(name).map(|x| x.usage)
}
pub fn primitive_schema(name: &str) -> Option<serde_json::Value> {
    registry::lookup(name).map(PrimitiveSpec::schema)
}
pub fn primitive_catalog() -> serde_json::Value {
    serde_json::Value::Array(primitive_specs.iter().map(PrimitiveSpec::schema).collect())
}

pub fn execute_browser_primitive(
    browser: &mut BrowserSession,
    name: &str,
    args: &[String],
) -> Result<String, Error> {
    let spec = registry::lookup(name).ok_or_else(|| {
        crate::jelly_error(
            crate::ErrorKind::Unsupported,
            format!("unsupported browser primitive: {name}"),
            false,
        )
    })?;
    let started = Instant::now();
    let result = spec
        .validate(args)
        .and_then(|_| (spec.handler)(browser, args));
    let _ = tracing::log_primitive(
        name,
        args,
        started.elapsed().as_millis(),
        result.as_ref().err(),
    );
    result
}

pub fn run_cli_primitive(name: &str) -> Result<(), Error> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut browser = BrowserSession::connect()?;
    let output = execute_browser_primitive(&mut browser, name, &args)?;
    if !output.is_empty() {
        let mut stdout = io::stdout().lock();
        if let Err(e) = writeln!(stdout, "{output}")
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(e.into());
        }
    }
    Ok(())
}
