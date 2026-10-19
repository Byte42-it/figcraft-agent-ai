//! The left sidebar's MCP panel: start and stop the local MCP server AI agents design through
//! (Claude Code, Claude Desktop, any MCP client), and copy what they need to connect. The server
//! itself is the platform's ([`crate::Services::set_mcp_server`]): the desktop app serves MCP over
//! HTTP on a free loopback port, with a guide for agents at `/help`.

use egui::{Align2, RichText, Sense, Ui, pos2, vec2};

use crate::i18n::{tr, trf, trn};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{self, PAD};
use crate::{FigApp, McpServer};

/// The Claude Code command that adds the server at `mcp`.
pub fn claude_code_command(mcp: &str) -> String {
    format!("claude mcp add --transport http figcraft {mcp}")
}

/// Claude Desktop's `claude_desktop_config.json` entry for the server at `mcp` (through the
/// `mcp-remote` bridge: Claude Desktop starts local servers as commands).
pub fn claude_desktop_config(mcp: &str) -> String {
    format!(
        "{{\n  \"mcpServers\": {{\n    \"figcraft\": {{\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"mcp-remote\", \"{mcp}\"]\n    }}\n  }}\n}}"
    )
}

pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let state = app.services.mcp_server();
    egui::ScrollArea::vertical().id_salt("mcp_panel").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(8.0);
        widgets::note(
            ui,
            tr("Start a local server so AI agents (Claude Code, Claude Desktop or any MCP client) can design in this window as you would: make and open files, add pages, draw, and use every tool and command. What they do shows here live and can be undone."),
            Tokens::get(ui.ctx()).text_secondary,
        );
        status(ui, &state);
        ui.horizontal(|ui| {
            ui.add_space(PAD);
            match state {
                McpServer::Unavailable => {}
                McpServer::Running { .. } => {
                    if widgets::button(ui, tr("Stop server"), false, true).clicked() {
                        app.services.set_mcp_server(false);
                    }
                }
                McpServer::Stopped | McpServer::Failed(_) => {
                    if widgets::button(ui, tr("Start server"), true, true).clicked()
                        && let McpServer::Running { port, .. } = app.services.set_mcp_server(true)
                    {
                        app.toast(trf("MCP server running on port {port}", &[("port", &port)]));
                    }
                }
            }
        });
        if let McpServer::Running { port, requests } = state {
            running(app, ui, port, requests);
            // The request count changes while the panel shows.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        ui.add_space(16.0);
    });
}

fn status(ui: &mut Ui, state: &McpServer) {
    let t = Tokens::get(ui.ctx());
    let (color, text) = match state {
        McpServer::Unavailable => (t.text_secondary, tr("Not available here: the desktop app runs the MCP server").to_string()),
        McpServer::Stopped => (t.text_secondary, tr("Stopped").to_string()),
        McpServer::Running { port, .. } => (t.success, trf("Running on 127.0.0.1:{port}", &[("port", port)])),
        McpServer::Failed(why) => (t.danger, trf("Couldn't start: {why}", &[("why", why)])),
    };
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    ui.painter().circle_filled(pos2(r.min.x + PAD + 4.0, r.center().y), 4.0, color);
    let shown = widgets::elide_font(ui, &text, (w - PAD * 2.0 - 14.0).max(0.0), regular(11.0));
    ui.painter().text(pos2(r.min.x + PAD + 14.0, r.center().y), Align2::LEFT_CENTER, &shown, regular(11.0), t.text);
    if shown != text {
        ui.interact(r, ui.id().with("mcp_status"), Sense::hover()).on_hover_text(text);
    }
}

fn running(app: &mut FigApp, ui: &mut Ui, port: u16, requests: usize) {
    let t = Tokens::get(ui.ctx());
    let base = format!("http://127.0.0.1:{port}");
    let help = format!("{base}/help");
    let mcp = format!("{base}/mcp");
    ui.add_space(4.0);
    widgets::note(ui, &trn("{n} requests answered", requests, &[("n", &requests)]), t.text_secondary);

    heading(ui, tr("Guide for agents"));
    widgets::note(ui, tr("Give this address to an agent: it explains how to connect, the tools and the commands."), t.text_secondary);
    code(ui, &help);
    ui.horizontal(|ui| {
        ui.add_space(PAD);
        if widgets::button(ui, tr("Open"), false, true).clicked() {
            app.services.open_url(&help);
        }
        if widgets::button(ui, tr("Copy"), false, true).clicked() {
            copy(app, &help);
        }
    });

    heading(ui, tr("MCP endpoint"));
    code(ui, &mcp);
    copy_button(app, ui, &mcp);

    heading(ui, tr("Claude Code"));
    code(ui, &claude_code_command(&mcp));
    copy_button(app, ui, &claude_code_command(&mcp));

    heading(ui, tr("Claude Desktop"));
    widgets::note(ui, tr("Settings ▸ Developer ▸ Edit Config, then restart Claude Desktop (needs Node.js):"), t.text_secondary);
    code(ui, &claude_desktop_config(&mcp));
    copy_button(app, ui, &claude_desktop_config(&mcp));

    heading(ui, tr("Other agents"));
    widgets::note(ui, tr("Add an HTTP (Streamable HTTP) MCP server with the endpoint above."), t.text_secondary);
    ui.add_space(4.0);
    widgets::note(
        ui,
        tr("Only apps on this computer can connect. The port is new each time the server starts: update the agent's settings when it changes."),
        t.text_secondary,
    );
}

fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(10.0);
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
    ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, text, semibold(11.0), t.text);
}

/// Monospaced text the user can select, in a box.
fn code(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.outer_margin(egui::Margin::symmetric(PAD as i8, 4)).inner_margin(egui::Margin::same(8)).fill(t.hover).corner_radius(5).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).monospace().size(10.5).color(t.text)).wrap().selectable(true));
        },
    );
}

fn copy_button(app: &mut FigApp, ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(PAD);
        if widgets::button(ui, tr("Copy"), false, true).clicked() {
            copy(app, text);
        }
    });
}

fn copy(app: &mut FigApp, text: &str) {
    app.services.clipboard_set_text(text);
    app.toast(tr("Copied to the clipboard"));
}

#[cfg(test)]
#[path = "mcp_panel_tests.rs"]
mod tests;
