use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use figcraft_ui_egui::{ControlReply, McpServer};
use serde_json::{Value, json};

use super::McpHost;

/// A stand-in for the UI thread: answers every queued method with its name and params.
fn ui_thread(queue: super::Queue, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let reqs: Vec<_> = std::mem::take(&mut *queue.lock().unwrap());
            for r in reqs {
                let _ = r.reply.send(ControlReply { id: r.id, result: Ok(json!({"method": r.method, "params": r.params})) });
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })
}

fn post(port: u16, body: &Value) -> Value {
    let b = body.to_string();
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (_, body) = out.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(body).unwrap()
}

#[test]
fn agents_reach_the_window_through_the_control_queue() {
    let queue: super::Queue = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let ui = ui_thread(queue.clone(), stop.clone());
    let mut host = McpHost::new(queue, Arc::new(Mutex::new(None)));
    assert_eq!(host.state(), McpServer::Stopped);
    let McpServer::Running { port, .. } = host.set(true) else { panic!("the server starts") };
    let reply = post(
        port,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "run_command", "arguments": {"command": "page.add", "params": {"name": "Agent"}}}}),
    );
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("engine.execute") && text.contains("page.add") && text.contains("Agent"), "{text}");
    assert!(matches!(host.state(), McpServer::Running { requests: 1, .. }));
    // Started again, it takes the same port.
    assert_eq!(host.set(false), McpServer::Stopped);
    std::thread::sleep(Duration::from_millis(100));
    assert!(matches!(host.set(true), McpServer::Running { port: p, .. } if p == port));
    host.set(false);
    stop.store(true, Ordering::Relaxed);
    ui.join().unwrap();
}
