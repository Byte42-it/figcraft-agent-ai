//! MCP over HTTP on a loopback port ("Streamable HTTP", MCP 2025-06-18, JSON replies only), plus the
//! agent guide at `/help`. Hand-written HTTP/1.1 on `std::net` (no async runtime), like the stdio
//! server it wraps.
//!
//! - `POST /mcp`: one JSON-RPC message (or an old-style batch) in the body → the reply as
//!   `application/json`; notifications and responses → `202 Accepted`. `GET` / `DELETE /mcp` → 405
//!   (no server-sent events, no session to end).
//! - `GET /help` (and `/`): [`crate::help::markdown`]. `GET /tools`: the tool schemas. `GET /health`.
//!
//! Only this computer reaches it: it binds 127.0.0.1, and refuses a `Host` other than the loopback
//! names with its port and any `Origin` other than those (a web page could otherwise make the
//! browser post to it, or reach it through DNS rebinding).
//!
//! One [`Server`] answers every connection, one message at a time, on a thread of its own: the
//! backend need not be `Send` (it is made on that thread).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::time::Duration;

use serde_json::json;

use crate::backend::Backend;
use crate::server::Server;

/// The largest request body (as the stdio server's longest line).
const MAX_BODY: usize = 32 * 1024 * 1024;
/// The largest request line plus headers.
const MAX_HEAD: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 16;
/// An idle kept-alive connection is closed after this long.
const IDLE: Duration = Duration::from_secs(120);
/// How long a request waits for the server (the app answers within 60 s; screenshots, renders).
const ANSWER: Duration = Duration::from_secs(150);

/// A message for the server thread and where its reply goes.
struct Job {
    body: String,
    reply: Sender<Option<String>>,
}

/// A running HTTP transport. Dropping it (or [`HttpServer::stop`]) stops accepting connections.
pub struct HttpServer {
    port: u16,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
}

impl HttpServer {
    /// Listen on `127.0.0.1:port` (0: a free port the system picks) and serve MCP with the backend
    /// `make_backend` returns, called once on the server's thread.
    pub fn start<F>(port: u16, make_backend: F) -> std::io::Result<Self>
    where
        F: FnOnce() -> Box<dyn Backend> + Send + 'static,
    {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let (jobs, inbox) = channel::<Job>();
        let (ready, started) = sync_channel::<()>(1);
        std::thread::Builder::new().name("figcraft-mcp".into()).spawn(move || run_server(make_backend, inbox, ready))?;
        // The backend is made before the first connection is taken.
        let _ = started.recv_timeout(Duration::from_secs(10));
        let (flag, count) = (stop.clone(), requests.clone());
        std::thread::Builder::new().name("figcraft-mcp-http".into()).spawn(move || accept(listener, port, jobs, flag, count))?;
        Ok(Self { port, stop, requests })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:PORT`.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// How many MCP messages it has answered.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }

    /// Stop accepting connections; connections open now end after their current request.
    pub fn stop(&self) {
        if !self.stop.swap(true, Ordering::SeqCst) {
            // Wake the accept loop so it sees the flag.
            let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.port)), Duration::from_millis(300));
        }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_server<F: FnOnce() -> Box<dyn Backend>>(make_backend: F, inbox: Receiver<Job>, ready: SyncSender<()>) {
    let mut server = Server::new(make_backend());
    let _ = ready.send(());
    // Ends when the accept loop and every connection have let go of their senders.
    for job in inbox {
        let reply = server.handle_line(&job.body);
        let _ = job.reply.send(reply);
    }
}

fn accept(listener: TcpListener, port: u16, jobs: Sender<Job>, stop: Arc<AtomicBool>, requests: Arc<AtomicUsize>) {
    let open = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            let mut s = stream;
            let _ = write_response(&mut s, &Response::text(503, "too many connections"), true);
            continue;
        }
        open.fetch_add(1, Ordering::SeqCst);
        let (jobs, stop, requests, open2) = (jobs.clone(), stop.clone(), requests.clone(), open.clone());
        let spawned = std::thread::Builder::new().name("figcraft-mcp-conn".into()).spawn(move || {
            let _ = serve_connection(stream, port, &jobs, &stop, &requests);
            open2.fetch_sub(1, Ordering::SeqCst);
        });
        if let Err(e) = spawned {
            open.fetch_sub(1, Ordering::SeqCst);
            log::error!("mcp http: connection thread: {e}");
        }
    }
}

/// A parsed request.
#[derive(Debug)]
pub(crate) struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The client asked to close the connection after this request (or speaks HTTP/1.0).
    fn wants_close(&self) -> bool {
        self.header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"))
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub allow: Option<&'static str>,
}

impl Response {
    fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self { status, content_type, body: body.into(), allow: None }
    }
    fn text(status: u16, body: &str) -> Self {
        Self::new(status, "text/plain; charset=utf-8", format!("{body}\n"))
    }
    fn json(status: u16, v: &serde_json::Value) -> Self {
        Self::new(status, "application/json", serde_json::to_string_pretty(v).unwrap_or_default())
    }
}

/// Why a request couldn't be read: the reply to send before closing (`None`: just close).
enum ReadError {
    Closed,
    Reply(Response),
}

fn serve_connection(stream: TcpStream, port: u16, jobs: &Sender<Job>, stop: &AtomicBool, requests: &AtomicUsize) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IDLE))?;
    stream.set_write_timeout(Some(IDLE))?;
    stream.set_nodelay(true).ok();
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    loop {
        let req = match read_request(&mut reader) {
            Ok(r) => r,
            Err(ReadError::Closed) => return Ok(()),
            Err(ReadError::Reply(r)) => return write_response(&mut out, &r, true),
        };
        let close = req.wants_close() || stop.load(Ordering::SeqCst);
        let head = req.method == "HEAD";
        let mut resp = route(&req, port, &mut |body| {
            requests.fetch_add(1, Ordering::Relaxed);
            ask(jobs, body)
        });
        if head {
            resp.body.clear();
        }
        write_response(&mut out, &resp, close)?;
        if close {
            return Ok(());
        }
    }
}

/// Hand a body to the server thread and wait for its reply.
fn ask(jobs: &Sender<Job>, body: String) -> Result<Option<String>, String> {
    let (tx, rx) = channel();
    jobs.send(Job { body, reply: tx }).map_err(|_| "the MCP server stopped".to_string())?;
    rx.recv_timeout(ANSWER).map_err(|_| "timed out waiting for FigCraft".to_string())
}

fn read_request(reader: &mut impl BufRead) -> Result<Request, ReadError> {
    let mut head = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    loop {
        let mut line = Vec::new();
        let room = (MAX_HEAD + 1).saturating_sub(head.len()) as u64;
        let n = reader.take(room).read_until(b'\n', &mut line).map_err(|_| ReadError::Closed)?;
        if n == 0 {
            if lines.is_empty() {
                return Err(ReadError::Closed);
            }
            return Err(ReadError::Reply(Response::text(400, "incomplete request")));
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEAD {
            return Err(ReadError::Reply(Response::text(431, "request headers too large")));
        }
        let text = String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string();
        if text.is_empty() {
            // Blank lines before a request line are allowed (RFC 9112 §2.2).
            if lines.is_empty() {
                continue;
            }
            break;
        }
        lines.push(text);
    }
    let mut parts = lines.first().map(|l| l.split(' ')).into_iter().flatten();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ReadError::Reply(Response::text(400, "bad request line")));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(ReadError::Reply(Response::text(505, "HTTP/1.1 only")));
    }
    let mut headers = Vec::new();
    for l in lines.iter().skip(1) {
        let Some((k, v)) = l.split_once(':') else {
            return Err(ReadError::Reply(Response::text(400, "bad header line")));
        };
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let mut req = Request { method: method.to_string(), path: target.split(['?', '#']).next().unwrap_or("/").to_string(), headers, body: Vec::new() };
    if version == "HTTP/1.0" && !req.headers.iter().any(|(k, v)| k == "connection" && v.eq_ignore_ascii_case("keep-alive")) {
        req.headers.push(("connection".into(), "close".into()));
    }
    if req.header("transfer-encoding").is_some() {
        return Err(ReadError::Reply(Response::text(411, "send the body with a Content-Length")));
    }
    let len = match req.header("content-length") {
        None => 0,
        Some(v) => v.parse::<usize>().map_err(|_| ReadError::Reply(Response::text(400, "bad Content-Length")))?,
    };
    if len > MAX_BODY {
        return Err(ReadError::Reply(Response::text(413, "request body too large (32 MiB at most)")));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).map_err(|_| ReadError::Reply(Response::text(400, "incomplete body")))?;
    req.body = body;
    Ok(req)
}

/// Whether `host` (a `Host` header value, or an `Origin`'s host and port) names this server.
fn local_host(host: &str, port: u16) -> bool {
    let host = host.to_ascii_lowercase();
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|h| host == format!("{h}:{port}"))
}

/// The response to `req`. `mcp` hands a JSON-RPC body to the server and returns its reply line.
pub(crate) fn route(req: &Request, port: u16, mcp: &mut dyn FnMut(String) -> Result<Option<String>, String>) -> Response {
    // Only clients on this computer, through its own name for itself.
    if !req.header("host").is_some_and(|h| local_host(h, port)) {
        return Response::text(403, "this server only answers requests to 127.0.0.1 or localhost with its port");
    }
    if let Some(origin) = req.header("origin") {
        let ok = origin.strip_prefix("http://").is_some_and(|h| local_host(h, port));
        if !ok {
            return Response::text(403, "requests from web pages are refused");
        }
    }
    let base = format!("http://127.0.0.1:{port}");
    let get = req.method == "GET" || req.method == "HEAD";
    match req.path.as_str() {
        "/mcp" | "/mcp/" if req.method == "POST" => {
            let Ok(body) = String::from_utf8(req.body.clone()) else {
                return Response::json(
                    400,
                    &json!({"jsonrpc": "2.0", "id": null, "error": {"code": crate::server::PARSE_ERROR, "message": "the body is not UTF-8"}}),
                );
            };
            if body.trim().is_empty() {
                return Response::json(
                    400,
                    &json!({"jsonrpc": "2.0", "id": null, "error": {"code": crate::server::INVALID_REQUEST, "message": "empty body: POST one JSON-RPC message"}}),
                );
            }
            match mcp(body) {
                Ok(Some(reply)) => Response::new(200, "application/json", reply),
                Ok(None) => Response::new(202, "application/json", Vec::new()),
                Err(e) => Response::text(503, &e),
            }
        }
        "/mcp" | "/mcp/" => {
            let mut r = Response::text(405, "POST JSON-RPC messages here (this server sends no event streams)");
            r.allow = Some("POST");
            r
        }
        "/" | "/help" | "/help.md" | "/help/" if get => Response::new(200, "text/markdown; charset=utf-8", crate::help::markdown(&base)),
        "/tools" | "/tools.json" if get => Response::json(200, &json!({"tools": crate::tools::tool_definitions()})),
        "/health" if get => {
            Response::json(200, &json!({"ok": true, "name": "figcraft", "version": env!("CARGO_PKG_VERSION"), "mcp": format!("{base}/mcp")}))
        }
        "/" | "/help" | "/help.md" | "/help/" | "/tools" | "/tools.json" | "/health" => {
            let mut r = Response::text(405, "use GET");
            r.allow = Some("GET, HEAD");
            r
        }
        _ => Response::text(404, &format!("not found; see {base}/help")),
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

fn write_response(out: &mut impl Write, r: &Response, close: bool) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        r.status,
        reason(r.status),
        r.body.len()
    );
    if !r.body.is_empty() || r.status == 200 {
        head.push_str(&format!("Content-Type: {}\r\n", r.content_type));
    }
    if let Some(allow) = r.allow {
        head.push_str(&format!("Allow: {allow}\r\n"));
    }
    head.push_str(if close { "Connection: close\r\n\r\n" } else { "Connection: keep-alive\r\n\r\n" });
    out.write_all(head.as_bytes())?;
    out.write_all(&r.body)?;
    out.flush()
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
