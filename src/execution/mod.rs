mod discovery;
mod named;
mod registry;
mod tools;
mod tracing;

pub use discovery::{
    browser_capabilities, browser_operation_schema, capabilities, lightweight_tools,
    search_browser_operations, search_tools, tool_schema, tools_in,
};
pub use named::{
    execute_named_browser_primitive, named_primitive_input_schema, prepare_named_primitive_args,
};
pub use registry::{
    ArgKind, ArgSpec, CATEGORIES as category_specs, CategorySpec, PRIMITIVES as primitive_specs,
    PrimitiveSpec,
};
pub use tools::{
    TOOL_CATEGORIES as tool_category_specs, TOOLS as tool_specs, ToolCategorySpec, ToolSpec,
};
pub use tracing::{new_id, record_step, redact_tool_args};

use crate::{BrowserSession, Error};
use std::{
    env,
    io::{self, Write},
    time::Instant,
};

pub fn is_browser_primitive(name: &str) -> bool {
    registry::lookup(name).is_some()
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
    let validation = spec.validate(args);
    let prepared = if validation.is_ok() {
        tracing::prepare_step(browser, name, args)
    } else {
        None
    };
    let result = validation.and_then(|_| (spec.handler)(browser, args));
    let _ = tracing::log_primitive(
        name,
        args,
        started.elapsed().as_millis(),
        result.as_ref().err(),
        prepared.as_ref(),
    );
    result
}

fn normalize_cli_primitive_args(args: Vec<String>) -> Result<Vec<String>, &'static str> {
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => Err("help"),
        Some("--") => Ok(args.into_iter().skip(1).collect()),
        _ => Ok(args),
    }
}

fn print_cli_primitive_help(name: &str) -> Result<(), Error> {
    let spec = registry::lookup(name).ok_or_else(|| {
        crate::jelly_error(
            crate::ErrorKind::Unsupported,
            format!("unsupported browser primitive: {name}"),
            false,
        )
    })?;
    let args = spec.usage.strip_prefix(name).unwrap_or(spec.usage);
    println!(
        "{}\n\nUsage: agent-{name}{args}\n\nUse `--` before arguments to pass a literal value beginning with `-`.",
        spec.description
    );
    Ok(())
}

pub fn run_cli_primitive(name: &str) -> Result<(), Error> {
    let args: Vec<String> = env::args().skip(1).collect();
    let args = match normalize_cli_primitive_args(args) {
        Ok(args) => args,
        Err("help") => return print_cli_primitive_help(name),
        Err(_) => unreachable!(),
    };
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

#[cfg(test)]
mod cli_tests {
    use super::normalize_cli_primitive_args;

    #[test]
    fn help_flags_are_not_forwarded_to_browser_primitives() {
        assert_eq!(
            normalize_cli_primitive_args(vec!["--help".into()]),
            Err("help")
        );
        assert_eq!(normalize_cli_primitive_args(vec!["-h".into()]), Err("help"));
    }

    #[test]
    fn separator_allows_literal_help_text() {
        assert_eq!(
            normalize_cli_primitive_args(vec!["--".into(), "--help".into()]),
            Ok(vec!["--help".into()])
        );
    }
}
