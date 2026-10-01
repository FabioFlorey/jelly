use super::{
    browser_call::{
        RawCdpAccess, browser_call_input_schema, cdp_call_input_schema, execute_browser_call,
        execute_cdp_call, validate_browser_call, validate_cdp_call,
    },
    browser_events::{
        browser_events_input_schema, execute_browser_events, validate_browser_events,
    },
    browser_schema::{browser_schema_input_schema, execute_browser_schema},
};
use crate::{BrowserSession, Error, ErrorKind, jelly_error};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentBuiltinExecution {
    Stateless,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentBuiltin {
    BrowserSchema,
    BrowserCall { raw_cdp: RawCdpAccess },
    CdpCall,
    BrowserEvents,
}

impl AgentBuiltin {
    pub const fn browser_call(raw_cdp: RawCdpAccess) -> Self {
        Self::BrowserCall { raw_cdp }
    }

    pub fn browser_call_from_env() -> Result<Self, String> {
        Ok(Self::browser_call(RawCdpAccess::from_env()?))
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::BrowserSchema => "browser-schema",
            Self::BrowserCall { .. } => "browser-call",
            Self::CdpCall => "cdp-call",
            Self::BrowserEvents => "browser-events",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::BrowserSchema => {
                "Discover Jelly semantic browser capabilities, search operations, and load the named JSON argument schema for one operation."
            }
            Self::BrowserCall { raw_cdp } if raw_cdp.enabled() => {
                "Execute ordered Jelly semantic browser operations plus explicitly scoped privileged raw CDP calls. Raw CDP has broader authority than Jelly semantic operations or page JavaScript and can bypass Jelly-level interaction, verification, and future browser-policy guards; enable it only for trusted workflows."
            }
            Self::BrowserCall { .. } => {
                "Execute an ordered batch of Jelly semantic browser operations through one persistent browser session, with strict preflight and structured per-call outcomes."
            }
            Self::CdpCall => {
                "Execute explicitly scoped privileged raw CDP calls on page targets or the browser connection. Raw CDP bypasses Jelly semantic-operation policy and should be enabled only for trusted workflows."
            }
            Self::BrowserEvents => {
                "Subscribe to retained CDP notifications, poll them through bounded logical-target/method filters, and explicitly observe cursor loss, drops, and stream resets."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::BrowserSchema => browser_schema_input_schema(),
            Self::BrowserCall { raw_cdp } => browser_call_input_schema(raw_cdp),
            Self::CdpCall => cdp_call_input_schema(),
            Self::BrowserEvents => browser_events_input_schema(),
        }
    }

    pub const fn execution(self) -> AgentBuiltinExecution {
        match self {
            Self::BrowserSchema => AgentBuiltinExecution::Stateless,
            Self::BrowserCall { .. } | Self::CdpCall | Self::BrowserEvents => {
                AgentBuiltinExecution::Browser
            }
        }
    }

    pub fn preflight(self, arguments: &Value) -> Result<(), Error> {
        match self {
            Self::BrowserSchema => Ok(()),
            Self::BrowserCall { raw_cdp } => validate_browser_call(arguments, raw_cdp),
            Self::CdpCall => validate_cdp_call(arguments),
            Self::BrowserEvents => validate_browser_events(arguments),
        }
    }

    pub fn execute_stateless(self, arguments: &Value) -> Result<String, Error> {
        let value = match self {
            Self::BrowserSchema => execute_browser_schema(arguments)?,
            Self::BrowserCall { .. } => {
                return Err(jelly_error(
                    ErrorKind::Internal,
                    "browser-call requires browser execution",
                    false,
                ));
            }
            Self::CdpCall => {
                return Err(jelly_error(
                    ErrorKind::Internal,
                    "cdp-call requires browser execution",
                    false,
                ));
            }
            Self::BrowserEvents => {
                return Err(jelly_error(
                    ErrorKind::Internal,
                    "browser-events requires browser execution",
                    false,
                ));
            }
        };
        serde_json::to_string(&value).map_err(Into::into)
    }

    pub fn execute_browser(
        self,
        browser: &mut BrowserSession,
        arguments: &Value,
    ) -> Result<String, Error> {
        let value = match self {
            Self::BrowserCall { raw_cdp } => execute_browser_call(browser, arguments, raw_cdp)?,
            Self::CdpCall => execute_cdp_call(browser, arguments)?,
            Self::BrowserEvents => execute_browser_events(browser, arguments)?,
            Self::BrowserSchema => {
                return Err(jelly_error(
                    ErrorKind::Internal,
                    "browser-schema does not require browser execution",
                    false,
                ));
            }
        };
        serde_json::to_string(&value).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_execution_scope_and_preflight_are_explicit() {
        assert_eq!(
            AgentBuiltin::BrowserSchema.execution(),
            AgentBuiltinExecution::Stateless
        );

        let semantic_only = AgentBuiltin::browser_call(RawCdpAccess::Disabled);
        assert_eq!(semantic_only.execution(), AgentBuiltinExecution::Browser);
        assert_eq!(
            AgentBuiltin::BrowserEvents.execution(),
            AgentBuiltinExecution::Browser
        );
        assert!(
            AgentBuiltin::BrowserEvents
                .preflight(&serde_json::json!({"action":"subscribe","target":"main"}))
                .is_ok()
        );
        assert!(
            semantic_only
                .preflight(&serde_json::json!({
                    "calls":[{"call":{"jelly":"read-page"}}]
                }))
                .is_ok()
        );
        assert!(
            semantic_only
                .preflight(&serde_json::json!({
                    "calls":[{
                        "scope":"target",
                        "call":{"method":"Runtime.evaluate"}
                    }]
                }))
                .is_err()
        );

        let raw = AgentBuiltin::browser_call(RawCdpAccess::Enabled);
        assert!(
            raw.preflight(&serde_json::json!({
                "calls":[{
                    "scope":"target",
                    "call":{"method":"Runtime.evaluate"}
                }]
            }))
            .is_ok()
        );
    }

    #[test]
    fn raw_cdp_policy_changes_both_description_and_schema() {
        let disabled = AgentBuiltin::browser_call(RawCdpAccess::Disabled);
        let enabled = AgentBuiltin::browser_call(RawCdpAccess::Enabled);

        assert!(!disabled.description().contains("raw CDP"));
        assert!(enabled.description().contains("raw CDP"));
        assert!(enabled.description().contains("broader authority"));
        assert!(
            enabled
                .description()
                .contains("future browser-policy guards")
        );
        assert!(!disabled.input_schema().to_string().contains("\"method\""));
        assert!(enabled.input_schema().to_string().contains("\"method\""));
    }
}
