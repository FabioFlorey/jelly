use serde_json::{Map, Value};
use std::{error::Error as StdError, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidArguments,
    BrowserUnavailable,
    NavigationFailed,
    NavigationTimeout,
    TargetNotFound,
    TargetNotVisible,
    TargetStale,
    InteractionFailed,
    ConditionFailed,
    ConditionTimeout,
    JavascriptFailed,
    ArtifactFailed,
    DownloadFailed,
    DeliveryFailed,
    HumanInterventionRequired,
    AuthenticationRequired,
    Unsupported,
    SubscriptionNotFound,
    CdpFailed,
    Internal,
}

impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::BrowserUnavailable => "browser_unavailable",
            Self::NavigationFailed => "navigation_failed",
            Self::NavigationTimeout => "navigation_timeout",
            Self::TargetNotFound => "target_not_found",
            Self::TargetNotVisible => "target_not_visible",
            Self::TargetStale => "target_stale",
            Self::InteractionFailed => "interaction_failed",
            Self::ConditionFailed => "condition_failed",
            Self::ConditionTimeout => "condition_timeout",
            Self::JavascriptFailed => "javascript_failed",
            Self::ArtifactFailed => "artifact_failed",
            Self::DownloadFailed => "download_failed",
            Self::DeliveryFailed => "delivery_failed",
            Self::HumanInterventionRequired => "human_intervention_required",
            Self::AuthenticationRequired => "authentication_required",
            Self::Unsupported => "unsupported",
            Self::SubscriptionNotFound => "subscription_not_found",
            Self::CdpFailed => "cdp_failed",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug)]
pub struct JellyError {
    kind: ErrorKind,
    message: String,
    retryable: bool,
}

impl JellyError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub const fn retryable(&self) -> bool {
        self.retryable
    }
}

impl fmt::Display for JellyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for JellyError {}

#[derive(Debug, Clone)]
pub struct CdpError {
    method: String,
    code: i64,
    message: String,
    data: Option<Value>,
}

impl CdpError {
    pub fn new(
        method: impl Into<String>,
        code: i64,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            method: method.into(),
            code,
            message: message.into(),
            data,
        }
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub const fn code(&self) -> i64 {
        self.code
    }

    pub fn protocol_message(&self) -> &str {
        &self.message
    }

    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    pub fn details(&self) -> Value {
        let mut details = Map::from_iter([
            ("protocol".to_owned(), Value::String("cdp".to_owned())),
            ("method".to_owned(), Value::String(self.method.clone())),
            ("code".to_owned(), Value::Number(self.code.into())),
            ("message".to_owned(), Value::String(self.message.clone())),
        ]);
        if let Some(data) = &self.data {
            details.insert("data".to_owned(), data.clone());
        }
        Value::Object(details)
    }
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CDP {} failed ({}): {}",
            self.method, self.code, self.message
        )
    }
}

impl StdError for CdpError {}

pub fn jelly_error(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> crate::Error {
    Box::new(JellyError::new(kind, message, retryable))
}

pub fn cdp_error(
    method: impl Into<String>,
    code: i64,
    message: impl Into<String>,
    data: Option<Value>,
) -> crate::Error {
    Box::new(CdpError::new(method, code, message, data))
}

pub fn classify_error(error: &(dyn StdError + 'static)) -> (ErrorKind, bool) {
    if let Some(error) = error.downcast_ref::<JellyError>() {
        return (error.kind(), error.retryable());
    }
    if error.downcast_ref::<CdpError>().is_some() {
        return (ErrorKind::CdpFailed, false);
    }
    (ErrorKind::Internal, false)
}

pub fn error_details(error: &(dyn StdError + 'static)) -> Option<Value> {
    error.downcast_ref::<CdpError>().map(CdpError::details)
}

pub fn structured_error(error: &(dyn StdError + 'static)) -> Value {
    let (kind, retryable) = classify_error(error);
    let mut value = Map::from_iter([
        ("kind".to_owned(), Value::String(kind.as_str().to_owned())),
        ("message".to_owned(), Value::String(error.to_string())),
        ("retryable".to_owned(), Value::Bool(retryable)),
    ]);
    if let Some(details) = error_details(error) {
        value.insert("details".to_owned(), details);
    }
    Value::Object(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_errors_keep_machine_readable_kind() {
        let error = JellyError::new(ErrorKind::TargetNotFound, "missing", true);
        assert_eq!(error.kind().as_str(), "target_not_found");
        assert!(error.retryable());
        assert_eq!(error.to_string(), "missing");
    }

    #[test]
    fn cdp_errors_have_a_first_class_structured_contract() {
        let error = CdpError::new(
            "Runtime.evaluate",
            -32601,
            "Method not found",
            Some(json!({"hint":"test"})),
        );
        assert_eq!(classify_error(&error), (ErrorKind::CdpFailed, false));
        assert_eq!(
            error.to_string(),
            "CDP Runtime.evaluate failed (-32601): Method not found"
        );
        assert_eq!(
            error.details(),
            json!({
                "protocol":"cdp",
                "method":"Runtime.evaluate",
                "code":-32601,
                "message":"Method not found",
                "data":{"hint":"test"}
            })
        );
        assert_eq!(
            structured_error(&error),
            json!({
                "kind":"cdp_failed",
                "message":"CDP Runtime.evaluate failed (-32601): Method not found",
                "retryable":false,
                "details":{
                    "protocol":"cdp",
                    "method":"Runtime.evaluate",
                    "code":-32601,
                    "message":"Method not found",
                    "data":{"hint":"test"}
                }
            })
        );
    }

    #[test]
    fn ordinary_jelly_errors_do_not_invent_details() {
        let error = JellyError::new(ErrorKind::ConditionFailed, "no", false);
        assert!(error_details(&error).is_none());
        assert_eq!(
            structured_error(&error),
            json!({
                "kind":"condition_failed",
                "message":"no",
                "retryable":false
            })
        );
    }
}
