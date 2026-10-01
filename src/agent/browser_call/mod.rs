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
mod tests;
