pub mod files;
pub mod input;
pub mod inspect;
pub mod navigation;
pub mod script;
pub mod tabs;

use crate::{Error, Target};

pub(crate) fn target(args: &[String], index: usize, usage: &str) -> Result<Target, Error> {
    Target::parse(args.get(index).ok_or_else(|| usage.to_owned())?)
}
pub(crate) fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}
pub(crate) fn js(s: &str) -> String {
    serde_json::to_string(s).expect("string serialization")
}
