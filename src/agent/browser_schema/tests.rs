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
    let value =
        execute_browser_schema(&json!({"action":"search","query":"upload local file"})).unwrap();
    let results = value["results"].as_array().unwrap();
    assert_eq!(results[0]["operation"], "upload");
    assert!(results.len() <= DEFAULT_SEARCH_LIMIT as usize);

    let again =
        execute_browser_schema(&json!({"action":"search","query":"upload local file"})).unwrap();
    assert_eq!(value, again);
}

#[test]
fn search_limit_is_bounded_and_defaults_explicitly() {
    let value =
        execute_browser_schema(&json!({"action":"search","query":"inspect links"})).unwrap();
    assert_eq!(value["limit"], DEFAULT_SEARCH_LIMIT);

    let value =
        execute_browser_schema(&json!({"action":"search","query":"inspect","limit":1})).unwrap();
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
