use super::prepare::{
    PreparedCall, invalid_arguments, optional_nonempty_string, reject_unknown_fields,
    required_nonempty_string,
};
use crate::{BrowserSession, Error, ErrorKind, jelly_error};
use serde_json::{Map, Value, json};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawCdpAccess {
    Disabled,
    Enabled,
}

impl RawCdpAccess {
    pub const fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn from_config() -> Self {
        if crate::config::config().mcp.raw_cdp {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let Some(value) = value else {
            return Ok(Self::Disabled);
        };
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" => Ok(Self::Enabled),
            "0" | "false" | "off" => Ok(Self::Disabled),
            other => Err(format!(
                "mcp.raw_cdp must be one of 1,true,on,0,false,off; got {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CdpScope {
    Target,
    Browser,
}

impl CdpScope {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Browser => "browser",
        }
    }
}

#[derive(Debug)]
pub(super) struct PreparedCdpCall {
    pub(super) index: usize,
    pub(super) scope: CdpScope,
    pub(super) target: Option<String>,
    pub(super) method: String,
    pub(super) params: Value,
}

pub(super) fn prepare_cdp_call(
    index: usize,
    item: &Map<String, Value>,
    call: &Map<String, Value>,
    raw_cdp: RawCdpAccess,
) -> Result<PreparedCall, Error> {
    if !raw_cdp.enabled() {
        return Err(jelly_error(
            ErrorKind::Unsupported,
            "raw CDP is disabled by config/jelly.toml [mcp].raw_cdp".to_owned(),
            false,
        ));
    }

    reject_unknown_fields(
        item,
        &["scope", "target", "call"],
        &format!("raw browser-call calls[{index}]"),
    )?;
    reject_unknown_fields(
        call,
        &["method", "params"],
        &format!("browser-call calls[{index}].call"),
    )?;

    let scope = match item.get("scope") {
        Some(Value::String(value)) if value == "target" => CdpScope::Target,
        Some(Value::String(value)) if value == "browser" => CdpScope::Browser,
        Some(Value::String(value)) => {
            return Err(invalid_arguments(format!(
                "browser-call calls[{index}].scope must be target or browser; got {value}"
            )));
        }
        Some(_) => {
            return Err(invalid_arguments(format!(
                "browser-call calls[{index}].scope must be a string"
            )));
        }
        None => {
            return Err(invalid_arguments(format!(
                "raw browser-call calls[{index}] requires explicit scope"
            )));
        }
    };

    let target =
        optional_nonempty_string(item, "target", &format!("raw browser-call calls[{index}]"))?
            .map(str::to_owned);
    if scope == CdpScope::Browser && target.is_some() {
        return Err(invalid_arguments(format!(
            "browser-call calls[{index}] cannot specify target when scope is browser"
        )));
    }

    let method =
        required_nonempty_string(call, "method", &format!("browser-call calls[{index}].call"))?;
    validate_cdp_method(method).map_err(|message| {
        invalid_arguments(format!(
            "invalid CDP method for browser-call calls[{index}]: {message}"
        ))
    })?;

    let params = call.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(invalid_arguments(format!(
            "browser-call calls[{index}].call.params must be an object"
        )));
    }

    Ok(PreparedCall::Cdp(PreparedCdpCall {
        index,
        scope,
        target,
        method: method.to_owned(),
        params,
    }))
}

pub(super) fn validate_cdp_method(method: &str) -> Result<(), String> {
    let mut parts = method.split('.');
    let Some(domain) = parts.next() else {
        return Err("method is empty".into());
    };
    let Some(command) = parts.next() else {
        return Err("method must use Domain.command syntax".into());
    };
    if parts.next().is_some() {
        return Err("method must contain exactly one dot".into());
    }
    if !valid_cdp_identifier(domain) || !valid_cdp_identifier(command) {
        return Err(format!(
            "method must use ASCII identifier segments in Domain.command syntax; got {method}"
        ));
    }
    Ok(())
}

fn valid_cdp_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(super) fn execute_cdp(
    browser: &mut BrowserSession,
    scope: CdpScope,
    target: Option<&str>,
    method: &str,
    params: &Value,
) -> Result<Value, Error> {
    let response = match (scope, target) {
        (CdpScope::Target, Some(target)) => {
            browser.call_on_logical_target(target, method, params.clone())?
        }
        (CdpScope::Target, None) => browser.call(method, params.clone())?,
        (CdpScope::Browser, None) => browser.browser_call(method, params.clone())?,
        (CdpScope::Browser, Some(_)) => {
            unreachable!("validated browser-scoped CDP calls cannot contain a logical target")
        }
    };
    response.get("result").cloned().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            format!(
                "CDP {method} [{}] returned a success response without result",
                scope.as_str()
            ),
            false,
        )
    })
}
