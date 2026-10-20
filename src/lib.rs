//! The modules the `claude-acc` binary is built from, as a library so other
//! front ends can reuse them.
//!
//! Not a published API: these are `pub` only because the binary is a separate
//! crate, and they change whenever the CLI needs them to.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod environment;
#[doc(hidden)]
pub mod identity;
#[doc(hidden)]
pub mod quoting;
#[doc(hidden)]
pub mod resolve;
#[doc(hidden)]
pub mod sessions;
