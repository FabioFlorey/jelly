pub mod files;
pub mod input;
pub mod inspect;
pub mod navigation;
pub mod script;
pub mod tabs;
pub mod verify;

use crate::{Error, ErrorKind, Target, jelly_error};

pub(crate) fn missing_target(target: &Target) -> Error {
    match target {
        Target::Ref(reference) => jelly_error(
            ErrorKind::TargetStale,
            format!("stable target @{reference} is no longer present; inspect the page again"),
            true,
        ),
        _ => jelly_error(ErrorKind::TargetNotFound, "target not found", true),
    }
}

pub(crate) fn target(args: &[String], index: usize, usage: &str) -> Result<Target, Error> {
    let value = args
        .get(index)
        .ok_or_else(|| jelly_error(ErrorKind::InvalidArguments, usage, false))?;
    Target::parse(value)
}
pub(crate) fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}
pub(crate) fn js(s: &str) -> String {
    serde_json::to_string(s).expect("string serialization")
}
