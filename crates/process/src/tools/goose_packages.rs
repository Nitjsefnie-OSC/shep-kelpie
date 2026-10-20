//! The MCP server a researcher on goose has its web from, installed beside the sandbox runtime
//!
//! `tools install` puts it at the version `package.json` pins, in the same `node_modules`.
//! goose has no search or fetch tool, and a pi package cannot be loaded into it, so the call's
//! reach names the server's file as a stdio server that runs under `node`.

/// The package that gives a researcher on goose its web, as `package.json` names it
pub const WEB_SERVER: &str = "duckduckgo-websearch";

/// The file in the package's folder that starts the server
pub const WEB_SERVER_FILE: &str = "build/index.js";
