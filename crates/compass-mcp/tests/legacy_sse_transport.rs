//! MCP `2024-11-05` HTTP+SSE transport: `GET /sse` opens a session and announces
//! its message endpoint, `POST /messages` is acknowledged with `202`, and replies
//! arrive as SSE `message` events.

use std::error::Error;
use std::net::SocketAddr;
use std::time::Duration;

use compass_mcp::{HttpOptions, serve_http};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const API_KEY: &str = "legacy-sse-key";
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// A vanished client is noticed on the next keep-alive frame (5 s), so allow a few.
const RECLAIM_TIMEOUT: Duration = Duration::from_secs(20);

struct ServerTask(tokio::task::JoinHandle<Result<(), String>>);

impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn start(max_sessions: usize) -> Result<(SocketAddr, ServerTask), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = reservation.local_addr()?;
    let mut options = HttpOptions::new(temp.path().join("missing.json"));
    options.port = address.port();
    options.api_key = Some(API_KEY.to_owned());
    options.max_sessions = max_sessions;
    drop(reservation);
    let server = ServerTask(tokio::spawn(serve_http(options)));
    tokio::time::timeout(STEP_TIMEOUT, async {
        loop {
            if TcpStream::connect(address).await.is_ok() {
                return Ok::<(), Box<dyn Error>>(());
            }
            if server.0.is_finished() {
                return Err("HTTP server exited before accepting connections".into());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    // The tempdir may drop now: tools/list does not read the graph file.
    Ok((address, server))
}

/// An open `GET /sse` connection that decodes chunked SSE frames as they arrive.
struct SseClient {
    stream: TcpStream,
    raw: Vec<u8>,
    decoded: String,
    headers_done: bool,
    head: String,
}

impl SseClient {
    async fn open(address: SocketAddr, key: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let mut stream = TcpStream::connect(address).await?;
        let auth = key.map_or_else(String::new, |key| format!("Authorization: Bearer {key}\r\n"));
        let request = format!(
            "GET /sse HTTP/1.1\r\nHost: {address}\r\nAccept: text/event-stream\r\n{auth}\r\n"
        );
        stream.write_all(request.as_bytes()).await?;
        Ok(Self {
            stream,
            raw: Vec::new(),
            decoded: String::new(),
            headers_done: false,
            head: String::new(),
        })
    }

    /// Read until the response head is complete and return its status line.
    async fn status_line(&mut self) -> Result<String, Box<dyn Error>> {
        while !self.headers_done {
            self.fill().await?;
        }
        Ok(self.head.lines().next().unwrap_or_default().to_owned())
    }

    async fn fill(&mut self) -> Result<(), Box<dyn Error>> {
        let mut chunk = [0_u8; 4096];
        let count = tokio::time::timeout(STEP_TIMEOUT, self.stream.read(&mut chunk)).await??;
        if count == 0 {
            return Err("SSE connection closed".into());
        }
        self.raw.extend_from_slice(&chunk[..count]);
        if !self.headers_done {
            if let Some(end) = find(&self.raw, b"\r\n\r\n") {
                self.head = String::from_utf8_lossy(&self.raw[..end]).into_owned();
                self.raw.drain(..end + 4);
                self.headers_done = true;
            }
        }
        if self.headers_done {
            self.decode_chunks();
        }
        Ok(())
    }

    /// Consume every complete HTTP/1.1 chunk currently buffered.
    fn decode_chunks(&mut self) {
        loop {
            let Some(line_end) = find(&self.raw, b"\r\n") else {
                return;
            };
            let size_text = String::from_utf8_lossy(&self.raw[..line_end]).into_owned();
            let Ok(size) = usize::from_str_radix(size_text.trim(), 16) else {
                return;
            };
            let total = line_end + 2 + size + 2;
            if self.raw.len() < total {
                return;
            }
            let body = &self.raw[line_end + 2..line_end + 2 + size];
            self.decoded.push_str(&String::from_utf8_lossy(body));
            self.raw.drain(..total);
        }
    }

    /// Next `event:`/`data:` frame, skipping comment-only keep-alive frames.
    async fn next_event(&mut self) -> Result<(String, String), Box<dyn Error>> {
        loop {
            while let Some(end) = self.decoded.find("\n\n") {
                let frame: String = self.decoded.drain(..end + 2).collect();
                let mut event = String::new();
                let mut data = String::new();
                for line in frame.lines() {
                    if let Some(value) = line.strip_prefix("event: ") {
                        event = value.to_owned();
                    } else if let Some(value) = line.strip_prefix("data: ") {
                        data = value.to_owned();
                    }
                }
                if !event.is_empty() {
                    return Ok((event, data));
                }
            }
            self.fill().await?;
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// POST `body` to `target` with the API key and return the status line.
async fn post(
    address: SocketAddr,
    target: &str,
    key: Option<&str>,
    body: &str,
) -> Result<String, Box<dyn Error>> {
    let mut stream = TcpStream::connect(address).await?;
    let auth = key.map_or_else(String::new, |key| format!("Authorization: Bearer {key}\r\n"));
    let request = format!(
        "POST {target} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    tokio::time::timeout(STEP_TIMEOUT, stream.read_to_end(&mut response)).await??;
    let text = String::from_utf8_lossy(&response);
    Ok(text.lines().next().unwrap_or_default().to_owned())
}

fn session_target(endpoint: &str) -> Result<&str, Box<dyn Error>> {
    if endpoint.starts_with("/messages?sessionId=") {
        Ok(endpoint)
    } else {
        Err(format!("unexpected endpoint event data: {endpoint}").into())
    }
}

#[tokio::test]
async fn legacy_sse_session_completes_the_2024_11_05_handshake_and_lists_tools()
-> Result<(), Box<dyn Error>> {
    let (address, _server) = start(4).await?;
    let mut sse = SseClient::open(address, Some(API_KEY)).await?;
    assert!(sse.status_line().await?.starts_with("HTTP/1.1 200"));
    assert!(
        sse.head
            .to_ascii_lowercase()
            .contains("content-type: text/event-stream")
    );
    let (event, endpoint) = sse.next_event().await?;
    assert_eq!(event, "endpoint");
    let target = session_target(&endpoint)?.to_owned();

    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "legacy-sse-conformance", "version": "1"}}
    });
    let status = post(address, &target, Some(API_KEY), &initialize.to_string()).await?;
    assert!(status.starts_with("HTTP/1.1 202"), "initialize: {status}");
    let (event, data) = sse.next_event().await?;
    assert_eq!(event, "message");
    let reply: Value = serde_json::from_str(&data)?;
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");

    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let status = post(address, &target, Some(API_KEY), &initialized.to_string()).await?;
    assert!(status.starts_with("HTTP/1.1 202"), "initialized: {status}");

    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let status = post(address, &target, Some(API_KEY), &list.to_string()).await?;
    assert!(status.starts_with("HTTP/1.1 202"), "tools/list: {status}");
    let (event, data) = sse.next_event().await?;
    assert_eq!(event, "message");
    let reply: Value = serde_json::from_str(&data)?;
    assert_eq!(reply["id"], 2);
    let tools = reply["result"]["tools"]
        .as_array()
        .ok_or("tools/list result has no tools array")?;
    assert!(!tools.is_empty());
    Ok(())
}

#[tokio::test]
async fn legacy_sse_enforces_authentication_session_identity_and_message_shape()
-> Result<(), Box<dyn Error>> {
    let (address, _server) = start(4).await?;

    let mut anonymous = SseClient::open(address, None).await?;
    assert!(anonymous.status_line().await?.starts_with("HTTP/1.1 401"));
    let status = post(address, "/messages?sessionId=00", None, "{}").await?;
    assert!(status.starts_with("HTTP/1.1 401"), "unauthenticated post: {status}");

    let unknown = "/messages?sessionId=00000000000000000000000000000000";
    let status = post(address, unknown, Some(API_KEY), "{}").await?;
    assert!(status.starts_with("HTTP/1.1 404"), "unknown session: {status}");
    let status = post(address, "/messages", Some(API_KEY), "{}").await?;
    assert!(status.starts_with("HTTP/1.1 400"), "missing session: {status}");
    let status = post(address, "/messages?sessionId=not-hex!", Some(API_KEY), "{}").await?;
    assert!(status.starts_with("HTTP/1.1 400"), "malformed session: {status}");

    let mut sse = SseClient::open(address, Some(API_KEY)).await?;
    let (_, endpoint) = sse.next_event().await?;
    let target = session_target(&endpoint)?.to_owned();
    let status = post(address, &target, Some(API_KEY), "[{}]").await?;
    assert!(status.starts_with("HTTP/1.1 400"), "batch: {status}");
    let status = post(address, &target, Some(API_KEY), "not json").await?;
    assert!(status.starts_with("HTTP/1.1 400"), "malformed json: {status}");
    Ok(())
}

#[tokio::test]
async fn legacy_sse_bounds_concurrent_sessions_and_frees_closed_ones() -> Result<(), Box<dyn Error>>
{
    let (address, _server) = start(1).await?;
    let mut first = SseClient::open(address, Some(API_KEY)).await?;
    assert_eq!(first.next_event().await?.0, "endpoint");

    let mut refused = SseClient::open(address, Some(API_KEY)).await?;
    assert!(
        refused.status_line().await?.starts_with("HTTP/1.1 429"),
        "second session must exceed the limit"
    );

    drop(first);
    tokio::time::timeout(RECLAIM_TIMEOUT, async {
        loop {
            let mut retry = SseClient::open(address, Some(API_KEY)).await?;
            if retry.status_line().await?.starts_with("HTTP/1.1 200") {
                return Ok::<(), Box<dyn Error>>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await??;
    Ok(())
}
