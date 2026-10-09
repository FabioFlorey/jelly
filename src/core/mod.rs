//! Deterministic domain decisions. The core must not perform I/O or access runtime globals.

pub(crate) mod downloads;
pub(crate) mod routines;
pub(crate) mod session;
pub(crate) mod targets;

pub(crate) mod oauth;
