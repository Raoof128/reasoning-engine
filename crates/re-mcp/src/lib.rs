//! The reasoning engine's MCP server: the twenty tools of `server.py`, with
//! Python's argument coercion, validation messages and response JSON.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod args;
pub mod error;
#[cfg(feature = "local-host")]
pub mod http;
pub mod limits;
pub mod schema;
pub mod server;
#[cfg(feature = "tasks")]
pub mod tasks;
pub mod tools;
pub mod validate;

pub use error::ToolError;
pub use server::ReasoningServer;
pub use tools::Tools;
