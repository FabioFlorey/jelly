pub mod files;
pub mod input;
pub mod inspect;
pub mod navigation;
pub mod script;
pub mod tabs;
pub mod verify;
pub mod visual;

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

pub(crate) fn arg<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str, Error> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| jelly_error(ErrorKind::InvalidArguments, usage, false))
}

pub(crate) fn target(args: &[String], index: usize, usage: &str) -> Result<Target, Error> {
    Target::parse(arg(args, index, usage)?)
}

pub(crate) fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}
pub(crate) fn js(s: &str) -> String {
    serde_json::to_string(s).expect("string serialization")
}
