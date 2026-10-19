//! The MCP server the left sidebar's MCP panel starts: MCP over HTTP on a free loopback port
//! (`figcraft_mcp::HttpServer`, with the agent guide at `/help`), answered by this window. Its
//! backend queues control-channel methods for the UI thread exactly like `--control` and
//! `figcraft-cli mcp --attach` do, so an agent sees and changes what the user sees.

use figcraft_mcp::{Backend, HttpServer};
use figcraft_ui_egui::McpServer;
use serde_json::{Value, json};

use crate::control_server::{self, CtxSlot, Queue, TIMEOUT};

/// Control methods queued for this window.
struct InApp {
    queue: Queue,
    ctx: CtxSlot,
    next_id: u64,
}

impl Backend for InApp {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        control_server::call(&self.queue, &self.ctx, json!(format!("mcp-{}", self.next_id)), method.to_string(), params, TIMEOUT)
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        "the FigCraft app the user has open (changes show in its window live)".into()
    }
}

/// The server, when it runs, and what to say when it couldn't start.
pub struct McpHost {
    queue: Queue,
    ctx: CtxSlot,
    server: Option<HttpServer>,
    /// The port it last ran on: started again, it takes the same one if it is free, so agents set
    /// up for it keep working.
    last_port: u16,
    failed: Option<String>,
}

impl McpHost {
    pub fn new(queue: Queue, ctx: CtxSlot) -> Self {
        Self { queue, ctx, server: None, last_port: 0, failed: None }
    }

    pub fn state(&self) -> McpServer {
        match (&self.server, &self.failed) {
            (Some(s), _) => McpServer::Running { port: s.port(), requests: s.requests() },
            (None, Some(why)) => McpServer::Failed(why.clone()),
            (None, None) => McpServer::Stopped,
        }
    }

    pub fn set(&mut self, on: bool) -> McpServer {
        if !on {
            if let Some(s) = self.server.take() {
                s.stop();
            }
            self.failed = None;
            return self.state();
        }
        if self.server.is_some() {
            return self.state();
        }
        let start = |port: u16| {
            let (queue, ctx) = (self.queue.clone(), self.ctx.clone());
            HttpServer::start(port, move || Box::new(InApp { queue, ctx, next_id: 0 }))
        };
        let started = match self.last_port {
            0 => start(0),
            p => start(p).or_else(|_| start(0)),
        };
        match started {
            Ok(s) => {
                self.last_port = s.port();
                eprintln!("FigCraft MCP server: {}/mcp (guide: {}/help)", s.base_url(), s.base_url());
                self.server = Some(s);
                self.failed = None;
            }
            Err(e) => {
                log::error!("MCP server: {e}");
                self.failed = Some(e.to_string());
            }
        }
        self.state()
    }
}

#[cfg(test)]
#[path = "mcp_host_tests.rs"]
mod tests;
