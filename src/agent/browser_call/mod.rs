use crate::{
    BrowserSession, Error, ErrorKind, PrimitiveSpec, classify_error, execute_browser_primitive,
    jelly_error, prepare_named_primitive_args, primitive_specs, structured_error,
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub(super) const MAX_BATCH_CALLS: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchFailurePolicy {
    Stop,
    Continue,
}

impl BatchFailurePolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Continue => "continue",
        }
    }
}

#[derive(Debug)]
struct PreparedSemanticCall {
    index: usize,
    target: Option<String>,
    operation: &'static PrimitiveSpec,
    positional_args: Vec<String>,
}

#[derive(Debug)]
enum PreparedCall {
    Semantic(PreparedSemanticCall),
    Cdp(PreparedCdpCall),
}

impl PreparedCall {
    const fn index(&self) -> usize {
        match self {
            Self::Semantic(call) => call.index,
            Self::Cdp(call) => call.index,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Semantic(call) => call.operation.name.to_owned(),
            Self::Cdp(call) => format!("{} [{}]", call.method, call.scope.as_str()),
        }
    }

    fn logical_target(&self) -> Option<&str> {
        match self {
            Self::Semantic(call) => call.target.as_deref(),
            Self::Cdp(call) => call.target.as_deref(),
        }
    }
}

#[derive(Debug)]
struct PreparedBrowserCall {
    policy: BatchFailurePolicy,
    calls: Vec<PreparedCall>,
}

enum PreparedInvocation<'a> {
    Semantic {
        target: Option<&'a str>,
        operation: &'static PrimitiveSpec,
        positional_args: &'a [String],
    },
    Cdp {
        scope: CdpScope,
        target: Option<&'a str>,
        method: &'a str,
        params: &'a Value,
    },
}

pub mod schema;
pub use schema::{browser_call_input_schema, cdp_call_input_schema};
mod raw;
pub use raw::RawCdpAccess;
#[cfg(test)]
use raw::validate_cdp_method;
use raw::{CdpScope, PreparedCdpCall, execute_cdp, prepare_cdp_call};

pub fn validate_browser_call(arguments: &Value, raw_cdp: RawCdpAccess) -> Result<(), Error> {
    prepare_browser_call(arguments, raw_cdp).map(|_| ())
}

pub fn validate_cdp_call(arguments: &Value) -> Result<(), Error> {
    prepare_cdp_only_call(arguments).map(|_| ())
}

pub fn execute_cdp_call(browser: &mut BrowserSession, arguments: &Value) -> Result<Value, Error> {
    let prepared = prepare_cdp_only_call(arguments)?;
    let mut seen_targets = HashSet::new();
    let logical_targets = prepared
        .calls
        .iter()
        .filter_map(PreparedCall::logical_target)
        .filter(|target| seen_targets.insert(*target))
        .collect::<Vec<_>>();
    browser.validate_logical_targets(&logical_targets)?;

    execute_prepared_browser_call(&prepared, |invocation| match invocation {
        PreparedInvocation::Cdp {
            scope,
            target,
            method,
            params,
        } => execute_cdp(browser, scope, target, method, params),
        PreparedInvocation::Semantic { .. } => {
            unreachable!("cdp-call preflight rejects semantic calls")
        }
    })
}

pub fn execute_browser_call(
    browser: &mut BrowserSession,
    arguments: &Value,
    raw_cdp: RawCdpAccess,
) -> Result<Value, Error> {
    let prepared = prepare_browser_call(arguments, raw_cdp)?;
    let mut seen_targets = HashSet::new();
    let logical_targets = prepared
        .calls
        .iter()
        .filter_map(PreparedCall::logical_target)
        .filter(|target| seen_targets.insert(*target))
        .collect::<Vec<_>>();
    browser.validate_logical_targets(&logical_targets)?;

    execute_prepared_browser_call(&prepared, |invocation| match invocation {
        PreparedInvocation::Semantic {
            target,
            operation,
            positional_args,
        } => {
            if let Some(target) = target {
                browser.switch_logical_target(target)?;
            }
            execute_browser_primitive(browser, operation.name, positional_args)
                .map(|output| output_value(&output))
        }
        PreparedInvocation::Cdp {
            scope,
            target,
            method,
            params,
        } => execute_cdp(browser, scope, target, method, params),
    })
}

fn prepare_cdp_only_call(arguments: &Value) -> Result<PreparedBrowserCall, Error> {
    let prepared = prepare_browser_call(arguments, RawCdpAccess::Enabled)?;
    if prepared
        .calls
        .iter()
        .any(|call| matches!(call, PreparedCall::Semantic(_)))
    {
        return Err(invalid_arguments(
            "cdp-call accepts only raw CDP method entries; semantic jelly calls are not allowed",
        ));
    }
    Ok(prepared)
}

fn prepare_browser_call(
    arguments: &Value,
    raw_cdp: RawCdpAccess,
) -> Result<PreparedBrowserCall, Error> {
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid_arguments("browser-call arguments must be a JSON object"))?;
    reject_unknown_fields(object, &["calls", "on_error"], "browser-call")?;

    let policy = match object.get("on_error") {
        None => BatchFailurePolicy::Stop,
        Some(Value::String(value)) if value == "stop" => BatchFailurePolicy::Stop,
        Some(Value::String(value)) if value == "continue" => BatchFailurePolicy::Continue,
        Some(Value::String(value)) => {
            return Err(invalid_arguments(format!(
                "browser-call on_error must be stop or continue; got {value}"
            )));
        }
        Some(_) => {
            return Err(invalid_arguments("browser-call on_error must be a string"));
        }
    };

    let calls = object
        .get("calls")
        .ok_or_else(|| invalid_arguments("browser-call requires calls"))?
        .as_array()
        .ok_or_else(|| invalid_arguments("browser-call calls must be an array"))?;

    if calls.is_empty() {
        return Err(invalid_arguments(
            "browser-call calls must contain at least one call",
        ));
    }
    if calls.len() > MAX_BATCH_CALLS {
        return Err(invalid_arguments(format!(
            "browser-call supports at most {MAX_BATCH_CALLS} calls per batch"
        )));
    }

    let mut prepared = Vec::with_capacity(calls.len());
    for (index, value) in calls.iter().enumerate() {
        prepared.push(prepare_call(index, value, raw_cdp)?);
    }

    Ok(PreparedBrowserCall {
        policy,
        calls: prepared,
    })
}

fn prepare_call(index: usize, value: &Value, raw_cdp: RawCdpAccess) -> Result<PreparedCall, Error> {
    let item = value.as_object().ok_or_else(|| {
        invalid_arguments(format!("browser-call calls[{index}] must be an object"))
    })?;
    reject_unknown_fields(
        item,
        &["scope", "target", "call"],
        &format!("browser-call calls[{index}]"),
    )?;

    let call = item
        .get("call")
        .ok_or_else(|| invalid_arguments(format!("browser-call calls[{index}] requires call")))?
        .as_object()
        .ok_or_else(|| {
            invalid_arguments(format!(
                "browser-call calls[{index}].call must be an object"
            ))
        })?;

    let has_jelly = call.contains_key("jelly");
    let has_method = call.contains_key("method");

    match (has_jelly, has_method) {
        (true, false) => prepare_semantic_call(index, item, call),
        (false, true) => prepare_cdp_call(index, item, call, raw_cdp),
        (true, true) => Err(invalid_arguments(format!(
            "browser-call calls[{index}].call must contain exactly one of jelly or method"
        ))),
        (false, false) => Err(invalid_arguments(format!(
            "browser-call calls[{index}].call requires exactly one of jelly or method"
        ))),
    }
}

fn prepare_semantic_call(
    index: usize,
    item: &Map<String, Value>,
    call: &Map<String, Value>,
) -> Result<PreparedCall, Error> {
    reject_unknown_fields(
        item,
        &["target", "call"],
        &format!("semantic browser-call calls[{index}]"),
    )?;
    reject_unknown_fields(
        call,
        &["jelly", "params"],
        &format!("browser-call calls[{index}].call"),
    )?;

    let operation_name =
        required_nonempty_string(call, "jelly", &format!("browser-call calls[{index}].call"))?;
    let operation = primitive_specs
        .iter()
        .find(|primitive| primitive.name == operation_name)
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Unsupported,
                format!("unknown semantic browser operation at calls[{index}]: {operation_name}"),
                false,
            )
        })?;

    let empty = json!({});
    let params = call.get("params").unwrap_or(&empty);
    let positional_args = prepare_named_primitive_args(operation, params).map_err(|error| {
        let (kind, retryable) = classify_error(error.as_ref());
        jelly_error(
            kind,
            format!("invalid params for browser-call calls[{index}] ({operation_name}): {error}"),
            retryable,
        )
    })?;

    let target = optional_nonempty_string(
        item,
        "target",
        &format!("semantic browser-call calls[{index}]"),
    )?
    .map(str::to_owned);

    Ok(PreparedCall::Semantic(PreparedSemanticCall {
        index,
        target,
        operation,
        positional_args,
    }))
}

fn execute_prepared_browser_call<F>(
    prepared: &PreparedBrowserCall,
    mut execute: F,
) -> Result<Value, Error>
where
    F: FnMut(PreparedInvocation<'_>) -> Result<Value, Error>,
{
    let mut results = Vec::with_capacity(prepared.calls.len());
    let mut stopped_at = None;
    let mut semantic_count = 0usize;
    let mut cdp_count = 0usize;

    for call in &prepared.calls {
        let outcome = match call {
            PreparedCall::Semantic(call) => {
                semantic_count += 1;
                execute(PreparedInvocation::Semantic {
                    target: call.target.as_deref(),
                    operation: call.operation,
                    positional_args: &call.positional_args,
                })
                .map(|data| {
                    json!({
                        "index": call.index,
                        "kind": "jelly",
                        "operation": call.operation.name,
                        "target": call.target,
                        "ok": true,
                        "data": data,
                        "error": Value::Null
                    })
                })
            }
            PreparedCall::Cdp(call) => {
                cdp_count += 1;
                execute(PreparedInvocation::Cdp {
                    scope: call.scope,
                    target: call.target.as_deref(),
                    method: &call.method,
                    params: &call.params,
                })
                .map(|data| {
                    json!({
                        "index": call.index,
                        "kind": "cdp",
                        "scope": call.scope.as_str(),
                        "target": call.target,
                        "method": call.method,
                        "ok": true,
                        "data": data,
                        "error": Value::Null
                    })
                })
            }
        };

        match outcome {
            Ok(result) => results.push(result),
            Err(error) => {
                let (kind, retryable) = classify_error(error.as_ref());
                if kind == ErrorKind::BrowserUnavailable {
                    return Err(jelly_error(
                        kind,
                        format!(
                            "browser unavailable during browser-call calls[{}] ({}) after {} completed call(s): {}",
                            call.index(),
                            call.label(),
                            results.len(),
                            error
                        ),
                        retryable,
                    ));
                }

                let failure = match call {
                    PreparedCall::Semantic(call) => json!({
                        "index": call.index,
                        "kind": "jelly",
                        "operation": call.operation.name,
                        "target": call.target,
                        "ok": false,
                        "data": Value::Null,
                        "error": structured_error(error.as_ref())
                    }),
                    PreparedCall::Cdp(call) => json!({
                        "index": call.index,
                        "kind": "cdp",
                        "scope": call.scope.as_str(),
                        "target": call.target,
                        "method": call.method,
                        "ok": false,
                        "data": Value::Null,
                        "error": structured_error(error.as_ref())
                    }),
                };
                results.push(failure);

                if prepared.policy == BatchFailurePolicy::Stop {
                    stopped_at = Some(call.index());
                    break;
                }
            }
        }
    }

    let attempted = results.len();
    let succeeded = results.iter().filter(|result| result["ok"] == true).count();
    let failed = attempted - succeeded;
    let status = if stopped_at.is_some() {
        "stopped"
    } else if failed > 0 {
        "completed_with_errors"
    } else {
        "completed"
    };
    let kind = match (semantic_count > 0, cdp_count > 0) {
        (true, true) => "mixed",
        (true, false) => "semantic",
        (false, true) => "cdp",
        (false, false) => unreachable!("validated browser-call batch cannot be empty"),
    };

    Ok(json!({
        "schema_version": 1,
        "kind": kind,
        "on_error": prepared.policy.as_str(),
        "status": status,
        "calls_total": prepared.calls.len(),
        "calls_attempted": attempted,
        "calls_succeeded": succeeded,
        "calls_failed": failed,
        "stopped_at": stopped_at,
        "results": results
    }))
}

fn required_nonempty_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, Error> {
    let value = object
        .get(key)
        .ok_or_else(|| invalid_arguments(format!("{context} requires {key}")))?;
    let value = value
        .as_str()
        .ok_or_else(|| invalid_arguments(format!("{context}.{key} must be a string")))?;
    if value.trim().is_empty() {
        return Err(invalid_arguments(format!(
            "{context}.{key} must not be empty"
        )));
    }
    Ok(value)
}

fn optional_nonempty_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<&'a str>, Error> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| invalid_arguments(format!("{context}.{key} must be a string")))?;
    if value.trim().is_empty() {
        return Err(invalid_arguments(format!(
            "{context}.{key} must not be empty"
        )));
    }
    Ok(Some(value))
}

pub(super) fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    context: &str,
) -> Result<(), Error> {
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
        "unknown field{} in {context}: {}",
        if unknown.len() == 1 { "" } else { "s" },
        unknown.join(", ")
    )))
}

fn output_value(output: &str) -> Value {
    serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_owned()))
}

pub(super) fn invalid_arguments(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::InvalidArguments, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(arguments: Value, raw_cdp: RawCdpAccess) -> PreparedBrowserCall {
        prepare_browser_call(&arguments, raw_cdp).unwrap()
    }

    fn assert_error(value: Result<PreparedBrowserCall, Error>, kind: ErrorKind, contains: &str) {
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
    fn raw_cdp_configuration_is_strict_and_defaults_disabled() {
        assert_eq!(RawCdpAccess::parse(None).unwrap(), RawCdpAccess::Disabled);
        for value in ["1", "true", "TRUE", " on "] {
            assert_eq!(
                RawCdpAccess::parse(Some(value)).unwrap(),
                RawCdpAccess::Enabled
            );
        }
        for value in ["0", "false", "FALSE", " off "] {
            assert_eq!(
                RawCdpAccess::parse(Some(value)).unwrap(),
                RawCdpAccess::Disabled
            );
        }
        assert!(RawCdpAccess::parse(Some("yes")).is_err());
        assert!(RawCdpAccess::parse(Some("")).is_err());
    }

    #[test]
    fn input_schema_exposes_raw_branch_only_when_enabled() {
        let disabled = browser_call_input_schema(RawCdpAccess::Disabled);
        let disabled_item = &disabled["properties"]["calls"]["items"];
        assert!(disabled_item.get("oneOf").is_none());
        assert!(disabled_item.to_string().contains("\"jelly\""));
        assert!(!disabled_item.to_string().contains("\"method\""));

        let enabled = browser_call_input_schema(RawCdpAccess::Enabled);
        let branches = enabled["properties"]["calls"]["items"]["oneOf"]
            .as_array()
            .unwrap();
        assert_eq!(branches.len(), 3);
        assert!(branches[0].to_string().contains("\"jelly\""));
        assert_eq!(branches[1]["properties"]["scope"]["const"], "target");
        assert!(branches[1]["properties"].get("target").is_some());
        assert!(
            branches[1]["description"]
                .as_str()
                .unwrap()
                .contains("Privileged raw CDP")
        );
        assert!(
            branches[1]["description"]
                .as_str()
                .unwrap()
                .contains("bypasses Jelly semantic-operation policy")
        );
        assert_eq!(branches[2]["properties"]["scope"]["const"], "browser");
        assert!(branches[2]["properties"].get("target").is_none());
        assert!(
            branches[2]["description"]
                .as_str()
                .unwrap()
                .contains("browser-level connection")
        );
        assert!(
            branches[2]["description"]
                .as_str()
                .unwrap()
                .contains("broader than page JavaScript")
        );
    }

    #[test]
    fn whole_mixed_batch_is_preflighted_before_execution() {
        let request = json!({
            "calls":[
                {"call":{"jelly":"read-page","params":{}}},
                {
                    "scope":"target",
                    "call":{"method":"Runtime.evaluate","params":{"expression":"document.title"}}
                },
                {"call":{"jelly":"click","params":{"target":"css:"}}}
            ]
        });
        let error = prepare_browser_call(&request, RawCdpAccess::Enabled).unwrap_err();
        assert!(error.to_string().contains("calls[2] (click)"));
    }

    #[test]
    fn semantic_contract_remains_available_when_raw_cdp_is_disabled() {
        let prepared = prepared(
            json!({"calls":[{"call":{"jelly":"read-page","params":{}}}]}),
            RawCdpAccess::Disabled,
        );
        assert!(matches!(prepared.calls[0], PreparedCall::Semantic(_)));

        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{
                        "scope":"target",
                        "call":{"method":"Runtime.evaluate","params":{}}
                    }]
                }),
                RawCdpAccess::Disabled,
            ),
            ErrorKind::Unsupported,
            "raw CDP is disabled",
        );
    }

    #[test]
    fn raw_calls_require_exclusive_method_form_and_explicit_scope() {
        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{
                        "scope":"target",
                        "call":{"jelly":"read-page","method":"Runtime.evaluate"}
                    }]
                }),
                RawCdpAccess::Enabled,
            ),
            ErrorKind::InvalidArguments,
            "exactly one of jelly or method",
        );
        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{"call":{"method":"Runtime.evaluate","params":{}}}]
                }),
                RawCdpAccess::Enabled,
            ),
            ErrorKind::InvalidArguments,
            "requires explicit scope",
        );
        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{
                        "scope":"page",
                        "call":{"method":"Runtime.evaluate","params":{}}
                    }]
                }),
                RawCdpAccess::Enabled,
            ),
            ErrorKind::InvalidArguments,
            "scope must be target or browser",
        );
        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{
                        "scope":"target",
                        "call":{"method":"Runtime.evaluate","params":[]}
                    }]
                }),
                RawCdpAccess::Enabled,
            ),
            ErrorKind::InvalidArguments,
            "params must be an object",
        );
    }

    #[test]
    fn logical_targets_are_preflighted_as_explicit_per_call_routing() {
        let prepared = prepared(
            json!({
                "calls":[
                    {"target":"main","call":{"jelly":"read-page"}},
                    {
                        "scope":"target",
                        "target":"tab-2",
                        "call":{"method":"Runtime.evaluate","params":{"expression":"document.title"}}
                    }
                ]
            }),
            RawCdpAccess::Enabled,
        );

        let PreparedCall::Semantic(semantic) = &prepared.calls[0] else {
            panic!("expected semantic call");
        };
        assert_eq!(semantic.target.as_deref(), Some("main"));

        let PreparedCall::Cdp(raw) = &prepared.calls[1] else {
            panic!("expected raw CDP call");
        };
        assert_eq!(raw.scope, CdpScope::Target);
        assert_eq!(raw.target.as_deref(), Some("tab-2"));

        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{
                        "scope":"browser",
                        "target":"main",
                        "call":{"method":"Target.getTargets"}
                    }]
                }),
                RawCdpAccess::Enabled,
            ),
            ErrorKind::InvalidArguments,
            "cannot specify target when scope is browser",
        );
        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{"target":"","call":{"jelly":"read-page"}}]
                }),
                RawCdpAccess::Disabled,
            ),
            ErrorKind::InvalidArguments,
            "target must not be empty",
        );
    }

    #[test]
    fn prepared_batch_exposes_unique_logical_targets_for_browser_state_preflight() {
        let prepared = prepared(
            json!({
                "calls":[
                    {"target":"main","call":{"jelly":"read-page"}},
                    {"target":"main","call":{"jelly":"assert-title","params":{"expected":"x"}}},
                    {
                        "scope":"target",
                        "target":"tab-2",
                        "call":{"method":"Runtime.evaluate"}
                    },
                    {
                        "scope":"browser",
                        "call":{"method":"Target.getTargets"}
                    }
                ]
            }),
            RawCdpAccess::Enabled,
        );

        let mut seen = HashSet::new();
        let targets = prepared
            .calls
            .iter()
            .filter_map(PreparedCall::logical_target)
            .filter(|target| seen.insert(*target))
            .collect::<Vec<_>>();
        assert_eq!(targets, vec!["main", "tab-2"]);
    }

    #[test]
    fn cdp_method_syntax_is_validated_before_browser_execution() {
        for invalid in [
            "",
            "Runtime",
            ".evaluate",
            "Runtime.",
            "Runtime.evaluate.extra",
            "Runtime.eval-uate",
            "1Runtime.evaluate",
        ] {
            assert!(
                validate_cdp_method(invalid).is_err(),
                "{invalid:?} unexpectedly accepted"
            );
        }
        for valid in [
            "Runtime.evaluate",
            "Target.getTargets",
            "HeadlessExperimental.beginFrame",
            "DOMSnapshot.captureSnapshot",
        ] {
            validate_cdp_method(valid).unwrap();
        }
    }

    #[test]
    fn raw_cdp_enabled_uses_scope_and_syntax_validation_not_a_method_allowlist() {
        validate_cdp_method("FutureDomain.futureCommand").unwrap();
        let prepared = prepared(
            json!({
                "calls":[{
                    "scope":"browser",
                    "call":{"method":"FutureDomain.futureCommand","params":{}}
                }]
            }),
            RawCdpAccess::Enabled,
        );
        let PreparedCall::Cdp(call) = &prepared.calls[0] else {
            panic!("expected raw CDP call");
        };
        assert_eq!(call.scope, CdpScope::Browser);
        assert_eq!(call.method, "FutureDomain.futureCommand");
    }

    #[test]
    fn mixed_semantic_and_raw_execution_preserves_order_and_result_shape() {
        let prepared = prepared(
            json!({
                "calls":[
                    {"call":{"jelly":"read-page","params":{}}},
                    {
                        "scope":"target",
                        "call":{"method":"Runtime.evaluate","params":{"expression":"document.title"}}
                    },
                    {
                        "scope":"browser",
                        "call":{"method":"Target.getTargets","params":{}}
                    }
                ]
            }),
            RawCdpAccess::Enabled,
        );

        let mut seen = Vec::new();
        let result = execute_prepared_browser_call(&prepared, |invocation| match invocation {
            PreparedInvocation::Semantic { operation, .. } => {
                seen.push(format!("jelly:{}", operation.name));
                Ok(json!({"title":"Example"}))
            }
            PreparedInvocation::Cdp {
                scope,
                method,
                params,
                ..
            } => {
                seen.push(format!("cdp:{}:{}", scope.as_str(), method));
                if method == "Runtime.evaluate" {
                    assert_eq!(params["expression"], "document.title");
                    Ok(json!({"result":{"type":"string","value":"Example"}}))
                } else {
                    Ok(json!({"targetInfos":[]}))
                }
            }
        })
        .unwrap();

        assert_eq!(
            seen,
            vec![
                "jelly:read-page",
                "cdp:target:Runtime.evaluate",
                "cdp:browser:Target.getTargets"
            ]
        );
        assert_eq!(result["kind"], "mixed");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["calls_succeeded"], 3);
        assert_eq!(result["results"][1]["kind"], "cdp");
        assert_eq!(result["results"][1]["scope"], "target");
        assert_eq!(result["results"][1]["method"], "Runtime.evaluate");
        assert_eq!(result["results"][1]["data"]["result"]["value"], "Example");
    }

    #[test]
    fn stop_and_continue_apply_to_raw_failures_too() {
        for (policy, expected_attempted, expected_status) in [
            ("stop", 2, "stopped"),
            ("continue", 3, "completed_with_errors"),
        ] {
            let prepared = prepared(
                json!({
                    "on_error":policy,
                    "calls":[
                        {"call":{"jelly":"read-page"}},
                        {
                            "scope":"target",
                            "call":{"method":"Runtime.evaluate","params":{}}
                        },
                        {"call":{"jelly":"assert-title","params":{"expected":"Done"}}}
                    ]
                }),
                RawCdpAccess::Enabled,
            );
            let result = execute_prepared_browser_call(&prepared, |invocation| match invocation {
                PreparedInvocation::Semantic { .. } => Ok(Value::String("ok".into())),
                PreparedInvocation::Cdp { method, .. } => Err(crate::cdp_error(
                    method,
                    -32601,
                    "Method not found",
                    Some(json!({"hint":"unknown command"})),
                )),
            })
            .unwrap();
            assert_eq!(result["calls_attempted"], expected_attempted);
            assert_eq!(result["status"], expected_status);
            assert_eq!(result["results"][1]["kind"], "cdp");
            assert_eq!(result["results"][1]["error"]["kind"], "cdp_failed");
            assert_eq!(
                result["results"][1]["error"]["details"],
                json!({
                    "protocol":"cdp",
                    "method":"Runtime.evaluate",
                    "code":-32601,
                    "message":"Method not found",
                    "data":{"hint":"unknown command"}
                })
            );
        }
    }

    #[test]
    fn browser_unavailable_is_batch_fatal_for_raw_calls() {
        let prepared = prepared(
            json!({
                "on_error":"continue",
                "calls":[
                    {"call":{"jelly":"read-page"}},
                    {
                        "scope":"browser",
                        "call":{"method":"Target.getTargets","params":{}}
                    },
                    {"call":{"jelly":"assert-title","params":{"expected":"Done"}}}
                ]
            }),
            RawCdpAccess::Enabled,
        );
        let error = execute_prepared_browser_call(&prepared, |invocation| match invocation {
            PreparedInvocation::Semantic { .. } => Ok(Value::String("ok".into())),
            PreparedInvocation::Cdp { .. } => Err(jelly_error(
                ErrorKind::BrowserUnavailable,
                "CDP socket closed",
                true,
            )),
        })
        .unwrap_err();

        let (kind, retryable) = classify_error(error.as_ref());
        assert_eq!(kind, ErrorKind::BrowserUnavailable);
        assert!(retryable);
        assert!(error.to_string().contains("Target.getTargets [browser]"));
        assert!(error.to_string().contains("after 1 completed call(s)"));
    }

    #[test]
    fn every_semantic_primitive_can_be_preflighted_with_raw_disabled() {
        for primitive in primitive_specs {
            let mut params = Map::new();
            for arg in primitive.args.iter().filter(|arg| arg.required) {
                let value = match arg.kind {
                    crate::ArgKind::String => Value::String("x".into()),
                    crate::ArgKind::Target => Value::String("css:#target".into()),
                    crate::ArgKind::Integer => json!(1),
                };
                params.insert(arg.name.to_owned(), value);
            }

            let request = json!({
                "calls":[{
                    "call":{
                        "jelly":primitive.name,
                        "params":Value::Object(params)
                    }
                }]
            });
            prepare_browser_call(&request, RawCdpAccess::Disabled).unwrap_or_else(|error| {
                panic!("{} failed browser-call preflight: {error}", primitive.name)
            });
        }
    }

    #[test]
    fn document_scoped_refs_are_preserved_verbatim_during_preflight() {
        let prepared = prepared(
            json!({
                "calls":[{"call":{"jelly":"click","params":{"target":"@eabc123-7"}}}]
            }),
            RawCdpAccess::Disabled,
        );
        let PreparedCall::Semantic(call) = &prepared.calls[0] else {
            panic!("expected semantic call");
        };
        assert_eq!(call.positional_args, vec!["@eabc123-7".to_owned()]);
    }

    #[test]
    fn positional_normalization_occurs_during_preflight() {
        let prepared = prepared(
            json!({
                "calls":[
                    {"call":{"jelly":"fill","params":{"target":"css:#email","text":"a b"}}},
                    {"call":{"jelly":"accessibility-tree","params":{"max":25}}}
                ]
            }),
            RawCdpAccess::Disabled,
        );

        let PreparedCall::Semantic(first) = &prepared.calls[0] else {
            panic!("expected semantic call");
        };
        let PreparedCall::Semantic(second) = &prepared.calls[1] else {
            panic!("expected semantic call");
        };
        assert_eq!(
            first.positional_args,
            vec!["a b".to_owned(), "css:#email".to_owned()]
        );
        assert_eq!(second.positional_args, vec!["25".to_owned()]);
    }

    #[test]
    fn batch_policy_and_bounds_are_strict() {
        assert_error(
            prepare_browser_call(
                &json!({
                    "calls":[{"call":{"jelly":"read-page"}}],
                    "on_error":"sometimes"
                }),
                RawCdpAccess::Disabled,
            ),
            ErrorKind::InvalidArguments,
            "on_error must be stop or continue",
        );

        let calls = (0..=MAX_BATCH_CALLS)
            .map(|_| json!({"call":{"jelly":"read-page"}}))
            .collect::<Vec<_>>();
        assert_error(
            prepare_browser_call(&json!({"calls":calls}), RawCdpAccess::Disabled),
            ErrorKind::InvalidArguments,
            "at most 64 calls",
        );
    }

    #[test]
    fn semantic_outputs_are_normalized_without_losing_json_structure() {
        let prepared = prepared(
            json!({
                "calls":[
                    {"call":{"jelly":"read-page"}},
                    {"call":{"jelly":"press-key","params":{"key":"Escape"}}}
                ]
            }),
            RawCdpAccess::Disabled,
        );
        let result = execute_prepared_browser_call(&prepared, |invocation| match invocation {
            PreparedInvocation::Semantic { operation, .. } => {
                if operation.name == "read-page" {
                    Ok(json!({"title":"Example"}))
                } else {
                    Ok(Value::String("performed".into()))
                }
            }
            PreparedInvocation::Cdp { .. } => unreachable!(),
        })
        .unwrap();

        assert_eq!(result["kind"], "semantic");
        assert_eq!(result["results"][0]["data"]["title"], "Example");
        assert_eq!(result["results"][1]["data"], "performed");
    }
}
