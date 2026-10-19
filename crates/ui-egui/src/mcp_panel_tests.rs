//! The left sidebar's MCP button opens the MCP panel, whose buttons start and stop the platform's
//! server and copy what agents need to connect.

use std::cell::RefCell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui::{Pos2, pos2};
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;
use crate::{LeftTab, McpServer, Services};

#[derive(Default)]
struct Log {
    state: Option<McpServer>,
    copied: Vec<String>,
    opened: Vec<String>,
    asked: Vec<bool>,
}

/// A platform whose server starts on port 4321.
struct Fake(Rc<RefCell<Log>>);

impl Services for Fake {
    fn mcp_server(&self) -> McpServer {
        self.0.borrow().state.clone().unwrap_or(McpServer::Stopped)
    }
    fn set_mcp_server(&mut self, on: bool) -> McpServer {
        let mut log = self.0.borrow_mut();
        log.asked.push(on);
        let s = if on { McpServer::Running { port: 4321, requests: 3 } } else { McpServer::Stopped };
        log.state = Some(s.clone());
        s
    }
    fn clipboard_set_text(&mut self, s: &str) {
        self.0.borrow_mut().copied.push(s.to_string());
    }
    fn open_url(&mut self, url: &str) {
        self.0.borrow_mut().opened.push(url.to_string());
    }
}

fn click(u: &mut Ui, p: Pos2) {
    u.button(p, true);
    u.button(p, false);
    u.frame(vec![]);
}

/// The middles of the widgets with `role` and `label`, top to bottom.
fn find(u: &mut Ui, role: Role, label: &str) -> Vec<Pos2> {
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update.expect("an AccessKit update").nodes;
    let mut at: Vec<Pos2> = nodes
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.role() == role && n.label() == Some(label))
        .filter_map(|n| n.bounds())
        .map(|b| pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
        .collect();
    at.sort_by(|a, b| a.y.total_cmp(&b.y));
    at
}

fn with_fake() -> (Ui, Rc<RefCell<Log>>) {
    let mut u = Ui::new(Session::new());
    let log = Rc::new(RefCell::new(Log::default()));
    u.app.services = Box::new(Fake(log.clone()));
    (u, log)
}

#[test]
fn the_rail_button_opens_the_mcp_panel() {
    let (mut u, _) = with_fake();
    u.app.ui.left_collapsed = true;
    let tab = *find(&mut u, Role::Tab, "MCP").first().expect("the MCP tab in the rail");
    click(&mut u, tab);
    assert_eq!(u.app.ui.left_tab, LeftTab::Mcp);
    assert!(!u.app.ui.left_collapsed);
    assert!(!find(&mut u, Role::Button, "Start server").is_empty(), "the panel offers to start the server");
}

#[test]
fn starting_shows_the_guide_and_copies_the_connection_details() {
    let (mut u, log) = with_fake();
    u.app.ui.left_tab = LeftTab::Mcp;
    let start = *find(&mut u, Role::Button, "Start server").first().expect("Start server");
    click(&mut u, start);
    assert_eq!(log.borrow().asked, vec![true]);
    u.frame(vec![]);
    let copies = find(&mut u, Role::Button, "Copy");
    assert_eq!(copies.len(), 4, "help, endpoint, Claude Code, Claude Desktop");
    // The last ones are below the 900-pixel window until the panel scrolls.
    for at in copies.iter().filter(|p| p.y < 890.0) {
        click(&mut u, *at);
    }
    let copied = log.borrow().copied.clone();
    assert!(copied.len() >= 3, "{copied:?}");
    assert_eq!(copied[0], "http://127.0.0.1:4321/help");
    assert_eq!(copied[1], "http://127.0.0.1:4321/mcp");
    assert_eq!(copied[2], "claude mcp add --transport http figcraft http://127.0.0.1:4321/mcp");
    let desktop: serde_json::Value =
        serde_json::from_str(&crate::mcp_panel::claude_desktop_config("http://127.0.0.1:4321/mcp")).expect("Claude Desktop's config is JSON");
    assert_eq!(desktop["mcpServers"]["figcraft"]["args"], json!(["-y", "mcp-remote", "http://127.0.0.1:4321/mcp"]));
    let open = *find(&mut u, Role::Button, "Open").first().expect("Open the guide");
    click(&mut u, open);
    assert_eq!(log.borrow().opened, vec!["http://127.0.0.1:4321/help".to_string()]);
    let stop = *find(&mut u, Role::Button, "Stop server").first().expect("Stop server");
    click(&mut u, stop);
    assert_eq!(log.borrow().asked, vec![true, false]);
    assert!(find(&mut u, Role::Button, "Copy").is_empty(), "nothing to copy once stopped");
}

#[test]
fn without_a_server_on_this_platform_nothing_starts() {
    let mut u = Ui::new(Session::new());
    u.app.ui.left_tab = LeftTab::Mcp;
    assert!(find(&mut u, Role::Button, "Start server").is_empty());
    assert!(find(&mut u, Role::Button, "Stop server").is_empty());
}

#[test]
fn the_help_command_opens_the_panel() {
    let (mut u, _) = with_fake();
    u.app.ui.left_collapsed = true;
    u.app.run("help.mcpServer", json!({}));
    u.frame(vec![]);
    assert_eq!(u.app.ui.left_tab, LeftTab::Mcp);
    assert!(!u.app.ui.left_collapsed);
}
