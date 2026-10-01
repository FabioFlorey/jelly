use super::{
    prepare::{
        BatchFailurePolicy, PreparedBrowserCall, PreparedCall, prepare_browser_call,
        prepare_cdp_only_call,
    },
    raw::{CdpScope, RawCdpAccess, execute_cdp},
    result::{
        batch_envelope, cdp_failure, cdp_success, output_value, semantic_failure, semantic_success,
    },
};
use crate::{
    BrowserSession, Error, ErrorKind, PrimitiveSpec, classify_error, execute_browser_primitive,
    jelly_error,
};
use serde_json::Value;
use std::collections::HashSet;

pub(super) enum PreparedInvocation<'a> {
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

pub(super) fn execute_prepared_browser_call<F>(
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
                .map(|data| semantic_success(call, data))
            }
            PreparedCall::Cdp(call) => {
                cdp_count += 1;
                execute(PreparedInvocation::Cdp {
                    scope: call.scope,
                    target: call.target.as_deref(),
                    method: &call.method,
                    params: &call.params,
                })
                .map(|data| cdp_success(call, data))
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
                    PreparedCall::Semantic(call) => semantic_failure(call, error.as_ref()),
                    PreparedCall::Cdp(call) => cdp_failure(call, error.as_ref()),
                };
                results.push(failure);

                if prepared.policy == BatchFailurePolicy::Stop {
                    stopped_at = Some(call.index());
                    break;
                }
            }
        }
    }

    Ok(batch_envelope(
        prepared,
        results,
        stopped_at,
        semantic_count,
        cdp_count,
    ))
}
