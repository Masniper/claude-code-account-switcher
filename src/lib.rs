//! The modules the `claude-acc` binary is built from, as a library.
//!
//! Not a published API. They are `pub` only because the binary is a separate
//! crate, and they change whenever the CLI needs them to.

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
