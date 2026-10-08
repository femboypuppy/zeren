//! zeren-mcp — a Model Context Protocol server over the running engine.
//!
//! `zeren mcp` speaks MCP (JSON-RPC 2.0, newline-delimited) on stdin/stdout
//! and proxies every tool into the engine's localhost IPC — the same
//! `zeren_rpc` WebSocket the headed app and `zeren sync` dial. Nothing here
//! talks to the edge or touches the filesystem: the engine stays the single
//! authority for chats, devices, projects, and the command plane.
//!
//! The server is hand-rolled rather than an SDK: the MCP surface a stdio
//! tool server needs is five methods (`initialize`, `ping`, `tools/list`,
//! `tools/call`, and the `initialized` notification), and the repo already
//! owns JSON-RPC framing for the Codex and ACP drivers. Zero new crates.
//!
//! When the engine launches a harness it can inject this server into the
//! agent's MCP config with `ZEREN_CHAT_ID` / `ZEREN_DEVICE_ID` in the
//! environment. Every send from such an agent is then attributed to its
//! originating chat, and a chat can never message itself.

mod jsonrpc;
mod tools;
mod transcript;
mod zeren;

pub use jsonrpc::serve_stdio;
pub use tools::{ToolDef, Tools};
pub use transcript::{RenderOptions, RenderedMessage, render_entries};
pub use zeren::{Origin, Zeren};

/// How `zeren mcp` finds the engine and who it speaks for.
#[derive(Debug, Clone)]
pub struct McpConfig {
    /// Loopback IPC port of the engine to proxy (`ZEREN_IPC_PORT`, default 27654).
    pub ipc_port: u16,
    /// The chat whose agent spawned this server, when injected by the engine.
    pub origin: Origin,
}

impl McpConfig {
    /// Resolve from the process environment: `ZEREN_IPC_PORT` for the engine,
    /// `ZEREN_CHAT_ID` / `ZEREN_DEVICE_ID` for the originating chat.
    pub fn from_env() -> Self {
        let ipc_port = std::env::var("ZEREN_IPC_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(27654);
        Self {
            ipc_port,
            origin: Origin::from_env(),
        }
    }
}

/// Run the MCP server on this process's stdin/stdout until stdin closes.
pub async fn run(config: McpConfig) -> anyhow::Result<()> {
    let zeren = Zeren::new(format!("ws://127.0.0.1:{}", config.ipc_port), config.origin);
    let tools = Tools::new(std::sync::Arc::new(zeren));
    serve_stdio(std::sync::Arc::new(tools)).await
}
