use super::{
    MAX_BATCH_CALLS,
    raw::{PreparedCdpCall, RawCdpAccess, prepare_cdp_call},
};
use crate::{
    Error, ErrorKind, PrimitiveSpec, classify_error, jelly_error, prepare_named_primitive_args,
    primitive_specs,
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BatchFailurePolicy {
    Stop,
    Continue,
}

impl BatchFailurePolicy {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Continue => "continue",
        }
    }
}

#[derive(Debug)]
pub(super) struct PreparedSemanticCall {
    pub(super) index: usize,
    pub(super) target: Option<String>,
    pub(super) operation: &'static PrimitiveSpec,
    pub(super) positional_args: Vec<String>,
}

#[derive(Debug)]
pub(super) enum PreparedCall {
    Semantic(PreparedSemanticCall),
    Cdp(PreparedCdpCall),
}

impl PreparedCall {
    pub(super) const fn index(&self) -> usize {
        match self {
            Self::Semantic(call) => call.index,
            Self::Cdp(call) => call.index,
        }
    }

    pub(super) fn label(&self) -> String {
        match self {
            Self::Semantic(call) => call.operation.name.to_owned(),
            Self::Cdp(call) => format!("{} [{}]", call.method, call.scope.as_str()),
        }
    }

    pub(super) fn logical_target(&self) -> Option<&str> {
        match self {
            Self::Semantic(call) => call.target.as_deref(),
            Self::Cdp(call) => call.target.as_deref(),
        }
    }
}

#[derive(Debug)]
pub(super) struct PreparedBrowserCall {
    pub(super) policy: BatchFailurePolicy,
    pub(super) calls: Vec<PreparedCall>,
}

pub(super) fn prepare_cdp_only_call(arguments: &Value) -> Result<PreparedBrowserCall, Error> {
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

pub(super) fn prepare_browser_call(
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

pub(super) fn required_nonempty_string<'a>(
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

pub(super) fn optional_nonempty_string<'a>(
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

pub(super) fn invalid_arguments(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::InvalidArguments, message, false)
}
