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

pub fn jelly_error(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> crate::Error {
    Box::new(JellyError::new(kind, message, retryable))
}

pub fn classify_error(error: &(dyn StdError + 'static)) -> (ErrorKind, bool) {
    if let Some(error) = error.downcast_ref::<JellyError>() {
        return (error.kind(), error.retryable());
    }
    (ErrorKind::Internal, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_errors_keep_machine_readable_kind() {
        let error = JellyError::new(ErrorKind::TargetNotFound, "missing", true);
        assert_eq!(error.kind().as_str(), "target_not_found");
        assert!(error.retryable());
        assert_eq!(error.to_string(), "missing");
    }
}
