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
mod tests {
    use super::*;
    use crate::{ErrorKind, classify_error, primitive_specs};

    fn assert_error(value: Result<Value, Error>, kind: ErrorKind, contains: &str) {
        let error = value.unwrap_err();
        let (actual, retryable) = classify_error(error.as_ref());
        assert_eq!(actual, kind);
        assert!(!retryable);
        assert!(
            error.to_string().contains(contains),
            "expected {:?} to contain {:?}",
            error.to_string(),
            contains
        );
    }

    #[test]
    fn input_schema_is_a_strict_action_union() {
        let schema = browser_schema_input_schema();
        let branches = schema["oneOf"].as_array().unwrap();
        assert_eq!(branches.len(), 3);
        assert!(
            branches
                .iter()
                .all(|branch| branch["additionalProperties"] == false)
        );
        assert_eq!(branches[0]["properties"]["action"]["const"], "capabilities");
        assert_eq!(branches[1]["properties"]["action"]["const"], "search");
        assert_eq!(branches[2]["properties"]["action"]["const"], "schema");
    }

    #[test]
    fn capabilities_are_browser_only_and_cover_the_registry_exactly() {
        let value = execute_browser_schema(&json!({"action":"capabilities"})).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["operation_count"], primitive_specs.len() as u64);
        let total = value["categories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|category| category["operation_count"].as_u64().unwrap())
            .sum::<u64>();
        assert_eq!(total, primitive_specs.len() as u64);
        assert!(
            !value.to_string().contains("browser-lifecycle"),
            "system categories must not leak into semantic browser discovery"
        );
    }

    #[test]
    fn search_reuses_semantic_ranking_and_is_deterministic() {
        let value = execute_browser_schema(&json!({"action":"search","query":"upload local file"}))
            .unwrap();
        let results = value["results"].as_array().unwrap();
        assert_eq!(results[0]["operation"], "upload");
        assert!(results.len() <= DEFAULT_SEARCH_LIMIT as usize);

        let again = execute_browser_schema(&json!({"action":"search","query":"upload local file"}))
            .unwrap();
        assert_eq!(value, again);
    }

    #[test]
    fn search_limit_is_bounded_and_defaults_explicitly() {
        let value =
            execute_browser_schema(&json!({"action":"search","query":"inspect links"})).unwrap();
        assert_eq!(value["limit"], DEFAULT_SEARCH_LIMIT);

        let value = execute_browser_schema(&json!({"action":"search","query":"inspect","limit":1}))
            .unwrap();
        assert_eq!(value["results"].as_array().unwrap().len(), 1);

        assert_error(
            execute_browser_schema(&json!({"action":"search","query":"x","limit":0})),
            ErrorKind::InvalidArguments,
            "between 1 and 50",
        );
        assert_error(
            execute_browser_schema(&json!({"action":"search","query":"x","limit":51})),
            ErrorKind::InvalidArguments,
            "between 1 and 50",
        );
    }

    #[test]
    fn schema_returns_named_json_contract_not_cli_usage() {
        let value =
            execute_browser_schema(&json!({"action":"schema","operation":"snapshot-interactive"}))
                .unwrap();
        let operation = &value["operation"];
        assert_eq!(operation["name"], "snapshot-interactive");
        assert_eq!(operation["category"], "inspect");
        assert_eq!(
            operation["input_schema"]["properties"]["limit"]["type"],
            "integer"
        );
        assert_eq!(
            operation["input_schema"]["dependentRequired"]["offset"],
            json!(["limit"])
        );
        assert!(operation.get("usage").is_none());
    }

    #[test]
    fn unknown_operation_is_a_typed_unsupported_failure() {
        assert_error(
            execute_browser_schema(&json!({"action":"schema","operation":"does-not-exist"})),
            ErrorKind::Unsupported,
            "unknown semantic browser operation",
        );
    }

    #[test]
    fn malformed_requests_are_rejected_before_discovery() {
        assert_error(
            execute_browser_schema(&json!([])),
            ErrorKind::InvalidArguments,
            "must be a JSON object",
        );
        assert_error(
            execute_browser_schema(&json!({"action":"search","query":""})),
            ErrorKind::InvalidArguments,
            "query must not be empty",
        );
        assert_error(
            execute_browser_schema(&json!({"action":"capabilities","query":"x"})),
            ErrorKind::InvalidArguments,
            "unknown browser-schema field: query",
        );
        assert_error(
            execute_browser_schema(&json!({"action":"wat"})),
            ErrorKind::InvalidArguments,
            "action must be capabilities, search, or schema",
        );
    }
}
