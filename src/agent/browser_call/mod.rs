use crate::Error;
use serde_json::Value;

#[cfg(test)]
use crate::{ErrorKind, classify_error, jelly_error, primitive_specs};
#[cfg(test)]
use serde_json::{Map, json};
#[cfg(test)]
use std::collections::HashSet;

pub(super) const MAX_BATCH_CALLS: usize = 64;
pub mod schema;
pub use schema::{browser_call_input_schema, cdp_call_input_schema};
mod execute;
mod prepare;
mod raw;
mod result;
#[cfg(test)]
use execute::{PreparedInvocation, execute_prepared_browser_call};
pub use execute::{execute_browser_call, execute_cdp_call};
#[cfg(test)]
use prepare::{PreparedBrowserCall, PreparedCall};
use prepare::{prepare_browser_call, prepare_cdp_only_call};
#[cfg(test)]
use raw::CdpScope;
pub use raw::RawCdpAccess;
#[cfg(test)]
use raw::validate_cdp_method;

pub fn validate_browser_call(arguments: &Value, raw_cdp: RawCdpAccess) -> Result<(), Error> {
    prepare_browser_call(arguments, raw_cdp).map(|_| ())
}

pub fn validate_cdp_call(arguments: &Value) -> Result<(), Error> {
    prepare_cdp_only_call(arguments).map(|_| ())
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
