use super::prepare::{PreparedBrowserCall, PreparedSemanticCall};
use super::raw::PreparedCdpCall;
use crate::structured_error;
use serde_json::{Value, json};
use std::error::Error as StdError;

pub(super) fn output_value(output: &str) -> Value {
    serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_owned()))
}

pub(super) fn semantic_success(call: &PreparedSemanticCall, data: Value) -> Value {
    json!({
        "index": call.index,
        "kind": "jelly",
        "operation": call.operation.name,
        "target": call.target,
        "ok": true,
        "data": data,
        "error": Value::Null
    })
}

pub(super) fn cdp_success(call: &PreparedCdpCall, data: Value) -> Value {
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
}

pub(super) fn semantic_failure(
    call: &PreparedSemanticCall,
    error: &(dyn StdError + 'static),
) -> Value {
    json!({
        "index": call.index,
        "kind": "jelly",
        "operation": call.operation.name,
        "target": call.target,
        "ok": false,
        "data": Value::Null,
        "error": structured_error(error)
    })
}

pub(super) fn cdp_failure(call: &PreparedCdpCall, error: &(dyn StdError + 'static)) -> Value {
    json!({
        "index": call.index,
        "kind": "cdp",
        "scope": call.scope.as_str(),
        "target": call.target,
        "method": call.method,
        "ok": false,
        "data": Value::Null,
        "error": structured_error(error)
    })
}

pub(super) fn batch_envelope(
    prepared: &PreparedBrowserCall,
    results: Vec<Value>,
    stopped_at: Option<usize>,
    semantic_count: usize,
    cdp_count: usize,
) -> Value {
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

    json!({
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::browser_call::prepare::{BatchFailurePolicy, PreparedCall};
    use crate::agent::browser_call::raw::{CdpScope, PreparedCdpCall};
    use crate::primitive_specs;
    use crate::{ErrorKind, jelly_error};

    #[test]
    fn batch_envelope_preserves_status_and_counts() {
        let operation = primitive_specs
            .iter()
            .find(|spec| spec.name == "read-page")
            .unwrap();
        let prepared = PreparedBrowserCall {
            policy: BatchFailurePolicy::Continue,
            calls: vec![
                PreparedCall::Semantic(PreparedSemanticCall {
                    index: 0,
                    target: None,
                    operation,
                    positional_args: Vec::new(),
                }),
                PreparedCall::Cdp(PreparedCdpCall {
                    index: 1,
                    scope: CdpScope::Browser,
                    target: None,
                    method: "Target.getTargets".into(),
                    params: json!({}),
                }),
            ],
        };
        let error = jelly_error(ErrorKind::ConditionFailed, "nope", false);
        let results = vec![
            semantic_success(
                match &prepared.calls[0] {
                    PreparedCall::Semantic(call) => call,
                    _ => unreachable!(),
                },
                json!({"ok":true}),
            ),
            cdp_failure(
                match &prepared.calls[1] {
                    PreparedCall::Cdp(call) => call,
                    _ => unreachable!(),
                },
                error.as_ref(),
            ),
        ];

        let value = batch_envelope(&prepared, results, None, 1, 1);
        assert_eq!(value["kind"], "mixed");
        assert_eq!(value["on_error"], "continue");
        assert_eq!(value["status"], "completed_with_errors");
        assert_eq!(value["calls_total"], 2);
        assert_eq!(value["calls_attempted"], 2);
        assert_eq!(value["calls_succeeded"], 1);
        assert_eq!(value["calls_failed"], 1);
    }
}
