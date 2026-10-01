use crate::{
    Error, ErrorKind, browser_capabilities, browser_operation_schema, jelly_error,
    search_browser_operations,
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

const DEFAULT_SEARCH_LIMIT: u64 = 8;
const MAX_SEARCH_LIMIT: u64 = 50;

pub fn browser_schema_input_schema() -> Value {
    json!({
        "oneOf": [
            {
                "type":"object",
                "properties":{
                    "action":{"const":"capabilities"}
                },
                "required":["action"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "action":{"const":"search"},
                    "query":{"type":"string","minLength":1},
                    "limit":{
                        "type":"integer",
                        "minimum":1,
                        "maximum":MAX_SEARCH_LIMIT,
                        "default":DEFAULT_SEARCH_LIMIT
                    }
                },
                "required":["action","query"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "action":{"const":"schema"},
                    "operation":{"type":"string","minLength":1}
                },
                "required":["action","operation"],
                "additionalProperties":false
            }
        ]
    })
}

pub fn execute_browser_schema(arguments: &Value) -> Result<Value, Error> {
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid_arguments("browser-schema arguments must be a JSON object"))?;
    let action = required_nonempty_string(object, "action")?;

    match action {
        "capabilities" => {
            reject_unknown_fields(object, &["action"])?;
            let capabilities = browser_capabilities();
            Ok(json!({
                "schema_version": 1,
                "action": "capabilities",
                "operation_count": capabilities["operation_count"],
                "categories": capabilities["categories"]
            }))
        }
        "search" => {
            reject_unknown_fields(object, &["action", "query", "limit"])?;
            let query = required_nonempty_string(object, "query")?;
            let limit = optional_limit(object)?;
            Ok(json!({
                "schema_version": 1,
                "action": "search",
                "query": query,
                "limit": limit,
                "results": search_browser_operations(query, limit as usize)
            }))
        }
        "schema" => {
            reject_unknown_fields(object, &["action", "operation"])?;
            let operation = required_nonempty_string(object, "operation")?;
            let schema = browser_operation_schema(operation)
                .map_err(|reason| {
                    jelly_error(
                        ErrorKind::Internal,
                        format!(
                            "invalid named schema contract for browser operation {operation}: {reason}"
                        ),
                        false,
                    )
                })?
                .ok_or_else(|| {
                    jelly_error(
                        ErrorKind::Unsupported,
                        format!("unknown semantic browser operation: {operation}"),
                        false,
                    )
                })?;
            Ok(json!({
                "schema_version": 1,
                "action": "schema",
                "operation": schema
            }))
        }
        other => Err(jelly_error(
            ErrorKind::InvalidArguments,
            format!("browser-schema action must be capabilities, search, or schema; got {other}"),
            false,
        )),
    }
}

fn required_nonempty_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, Error> {
    let value = object
        .get(key)
        .ok_or_else(|| invalid_arguments(format!("browser-schema requires {key}")))?;
    let value = value
        .as_str()
        .ok_or_else(|| invalid_arguments(format!("{key} must be a string")))?;
    if value.trim().is_empty() {
        return Err(invalid_arguments(format!("{key} must not be empty")));
    }
    Ok(value)
}

fn optional_limit(object: &Map<String, Value>) -> Result<u64, Error> {
    let Some(value) = object.get("limit") else {
        return Ok(DEFAULT_SEARCH_LIMIT);
    };
    let limit = value
        .as_u64()
        .ok_or_else(|| invalid_arguments("limit must be a positive integer"))?;
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(invalid_arguments(format!(
            "limit must be between 1 and {MAX_SEARCH_LIMIT}"
        )));
    }
    Ok(limit)
}

fn reject_unknown_fields(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), Error> {
    let allowed = allowed.iter().copied().collect::<HashSet<_>>();
    let mut unknown = object
        .keys()
        .filter(|key| !allowed.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();

    if unknown.is_empty() {
        return Ok(());
    }

    Err(invalid_arguments(format!(
        "unknown browser-schema field{}: {}",
        if unknown.len() == 1 { "" } else { "s" },
        unknown.join(", ")
    )))
}

fn invalid_arguments(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::InvalidArguments, message, false)
}

#[cfg(test)]
mod tests;
