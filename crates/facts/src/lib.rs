//! What a call's transcript says of the MCP servers, the requests and the tool
//! calls that failed: [`mcp_use`], [`request_use`] and [`tool_error`]
//!
//! Uses no other member of the workspace.

#![forbid(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

pub mod mcp_use;
pub mod request_use;
pub mod tool_error;
