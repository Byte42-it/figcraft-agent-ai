use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;

fn headless() -> HttpServer {
    HttpServer::start(0, || Box::new(crate::Headless::new())).unwrap()
}

/// One request on a fresh connection → (status, headers, body).
fn send(port: u16, raw: &str) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    read_response(&mut BufReader::new(s))
}

fn read_response(r: &mut impl BufRead) -> (u16, String, Vec<u8>) {
    let mut head = String::new();
    loop {
        let mut line = String::new();
        assert!(r.read_line(&mut line).unwrap() > 0, "response ended early: {head}");
        if line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    let status: u16 = head.split(' ').nth(1).unwrap().parse().unwrap();
    let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap();
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    (status, head, body)
}

fn post(port: u16, body: &Value) -> (u16, Value) {
    let b = body.to_string();
    let (status, _, body) = send(
        port,
        &format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        ),
    );
    (status, if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() })
}

#[test]
fn help_names_the_live_endpoint() {
    let srv = headless();
    let port = srv.port();
    let (status, head, body) = send(port, &format!("GET /help HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"));
    assert_eq!(status, 200);
    assert!(head.contains("text/markdown"));
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains(&format!("http://127.0.0.1:{port}/mcp")));
    assert!(text.contains("claude mcp add --transport http"));
}

#[test]
fn an_agent_initializes_lists_tools_and_draws() {
    let srv = headless();
    let port = srv.port();
    let (s, init) = post(
        port,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}),
    );
    assert_eq!(s, 200);
    assert_eq!(init["result"]["serverInfo"]["name"], "figcraft");
    let (s, none) = post(port, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_eq!((s, none), (202, Value::Null));
    let (_, tools) = post(port, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() >= 10);
    let (_, made) = post(
        port,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "create_node", "arguments": {"type": "rectangle", "x": 0, "y": 0, "width": 50, "height": 40}}}),
    );
    assert_eq!(made["result"]["isError"], false, "{made}");
    let (_, doc) =
        post(port, &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "inspect_document", "arguments": {"depth": 2}}}));
    assert!(doc["result"]["content"][0]["text"].as_str().unwrap().contains("\"width\": 50"));
    assert!(srv.requests() >= 5);
}

#[test]
fn a_kept_alive_connection_carries_several_requests() {
    let srv = headless();
    let port = srv.port();
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    for id in 1..=3 {
        let b = json!({"jsonrpc": "2.0", "id": id, "method": "ping"}).to_string();
        write!(w, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{b}", b.len()).unwrap();
        let (status, head, body) = read_response(&mut r);
        assert_eq!(status, 200);
        assert!(head.contains("keep-alive"));
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["id"], id);
    }
}

#[test]
fn other_hosts_and_web_pages_are_refused() {
    let srv = headless();
    let port = srv.port();
    let b = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    for headers in [
        format!("Host: evil.example:{port}"),
        "Host: 127.0.0.1:1".to_string(),
        String::new(),
        format!("Host: 127.0.0.1:{port}\r\nOrigin: https://evil.example"),
        format!("Host: 127.0.0.1:{port}\r\nOrigin: null"),
    ] {
        let (status, _, _) = send(port, &format!("POST /mcp HTTP/1.1\r\n{headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()));
        assert_eq!(status, 403, "{headers}");
    }
    let (status, _, _) = send(
        port,
        &format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        ),
    );
    assert_eq!(status, 200);
}

#[test]
fn wrong_methods_paths_and_bodies_get_errors() {
    let srv = headless();
    let port = srv.port();
    let host = format!("Host: 127.0.0.1:{port}\r\nConnection: close");
    let (s, head, _) = send(port, &format!("GET /mcp HTTP/1.1\r\n{host}\r\n\r\n"));
    assert_eq!(s, 405);
    assert!(head.contains("Allow: POST"));
    assert_eq!(send(port, &format!("GET /nope HTTP/1.1\r\n{host}\r\n\r\n")).0, 404);
    assert_eq!(send(port, &format!("POST /mcp HTTP/1.1\r\n{host}\r\nContent-Length: 0\r\n\r\n")).0, 400);
    assert_eq!(send(port, &format!("POST /mcp HTTP/1.1\r\n{host}\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")).0, 411);
    assert_eq!(send(port, &format!("POST /mcp HTTP/1.1\r\n{host}\r\nContent-Length: 99999999999\r\n\r\n")).0, 413);
    let (s, _, body) = send(port, &format!("POST /mcp HTTP/1.1\r\n{host}\r\nContent-Length: 5\r\n\r\nnope!"));
    assert_eq!(s, 200);
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"], crate::server::PARSE_ERROR);
    assert_eq!(send(port, &format!("GET /health HTTP/1.1\r\n{host}\r\n\r\n")).0, 200);
    let (s, _, body) = send(port, &format!("GET /tools HTTP/1.1\r\n{host}\r\n\r\n"));
    assert_eq!(s, 200);
    assert!(serde_json::from_slice::<Value>(&body).unwrap()["tools"].is_array());
}

#[test]
fn stopping_closes_the_port() {
    let srv = headless();
    let port = srv.port();
    drop(srv);
    // The accept loop has gone: nothing answers there any more.
    let mut refused = false;
    for _ in 0..50 {
        match TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200)) {
            Err(_) => {
                refused = true;
                break;
            }
            Ok(mut s) => {
                s.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
                let _ = write!(s, "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
                let mut buf = [0u8; 16];
                if matches!(s.read(&mut buf), Ok(0) | Err(_)) {
                    refused = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(refused, "the server still answers after stop");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    /// Junk never panics the request reader or the router.
    #[test]
    fn junk_requests_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600), path in "[ -~]{0,40}") {
        let mut r = BufReader::new(&bytes[..]);
        if let Ok(req) = read_request(&mut r) {
            let _ = route(&req, 9, &mut |_| Ok(None));
        }
        let raw = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n");
        let mut r = BufReader::new(raw.as_bytes());
        if let Ok(req) = read_request(&mut r) {
            let _ = route(&req, 9, &mut |_| Ok(Some("{}".into())));
        }
    }
}
