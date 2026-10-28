//! MCP `2024-11-05` HTTP+SSE transport.
//!
//! The first MCP revision predates Streamable HTTP. A client opens `GET <sse>` and
//! receives an `endpoint` event naming the URL it must `POST` JSON-RPC messages to
//! (`<messages>?sessionId=<id>`). Each POST is acknowledged with `202 Accepted`;
//! the JSON-RPC reply arrives later as a `message` event on the open stream.
//!
//! Each connection runs the same [`CompassMcp`] handler as stdio, over an in-memory
//! line-delimited pipe, so protocol behavior cannot drift between transports. Every
//! queue, session count, and message is bounded; the caller applies the shared
//! authentication, host, and write-capability gate before these handlers run.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::channel::{Channel, Sender};
use rand::RngCore;
use rand::rngs::OsRng;
use rmcp::ServiceExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::CompassMcp;

/// Largest single JSON-RPC message accepted from or emitted to a legacy client.
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// Client messages queued per session before `POST` answers `503`.
const SESSION_QUEUE_DEPTH: usize = 64;
/// Buffered SSE frames between the session pump and the HTTP body.
const EVENT_BUFFER: usize = 16;
/// Capacity of each in-memory pipe between the SSE session and the server.
const PIPE_BYTES: usize = 64 * 1024;
/// Idle interval after which a comment frame keeps proxies from closing the stream.
/// It is also the only way to notice a client that vanished while idle, so it bounds
/// how long a dead connection can hold one of the limited session slots.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(5);
/// Bytes of entropy in a session identifier.
const SESSION_ID_BYTES: usize = 16;
/// Longest `sessionId` query value this adapter will look up.
const MAX_SESSION_ID_CHARS: usize = 64;

const JSON_RPC_INVALID_REQUEST: i64 = -32600;
const JSON_RPC_PARSE_ERROR: i64 = -32700;
const SESSION_LIMIT_CODE: i64 = -32024;
const SESSION_NOT_FOUND_CODE: i64 = -32001;
const SESSION_BUSY_CODE: i64 = -32025;

/// Shared state for the `2024-11-05` endpoints.
#[derive(Clone)]
pub(crate) struct LegacySseState {
    inner: Arc<Inner>,
}

struct Inner {
    sessions: Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>,
    server: CompassMcp,
    max_sessions: usize,
    messages_path: String,
    cancellation: CancellationToken,
}

impl LegacySseState {
    pub(crate) fn new(
        server: CompassMcp,
        max_sessions: usize,
        messages_path: String,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                sessions: Mutex::new(HashMap::new()),
                server,
                max_sessions,
                messages_path,
                cancellation,
            }),
        }
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, mpsc::Sender<Vec<u8>>>> {
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn remove(&self, session_id: &str) {
        self.sessions().remove(session_id);
    }
}

/// `GET <sse>`: open a session and stream its events.
pub(crate) async fn open_stream(State(state): State<LegacySseState>) -> Response {
    let session_id = new_session_id();
    let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(SESSION_QUEUE_DEPTH);
    {
        // Counting and inserting under one lock keeps concurrent connects from
        // all passing the limit before any of them registers.
        let mut sessions = state.sessions();
        if sessions.len() >= state.inner.max_sessions {
            return json_rpc_error(
                StatusCode::TOO_MANY_REQUESTS,
                SESSION_LIMIT_CODE,
                "MCP legacy session limit reached",
            );
        }
        sessions.insert(session_id.clone(), input_tx);
    }
    let (events, body) = Channel::<Bytes, Infallible>::new(EVENT_BUFFER);
    tokio::spawn(run_session(state, session_id, input_rx, events));
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::new(body),
    )
        .into_response()
}

/// `POST <messages>?sessionId=<id>`: queue one client message for its session.
pub(crate) async fn post_message(
    State(state): State<LegacySseState>,
    request: Request,
) -> Response {
    let Some(session_id) = session_id_from_query(request.uri().query()) else {
        return json_rpc_error(
            StatusCode::BAD_REQUEST,
            JSON_RPC_INVALID_REQUEST,
            "missing or malformed sessionId",
        );
    };
    let queue = state.sessions().get(&session_id).cloned();
    let Some(queue) = queue else {
        return json_rpc_error(
            StatusCode::NOT_FOUND,
            SESSION_NOT_FOUND_CODE,
            "unknown MCP legacy session",
        );
    };
    let bytes = match axum::body::to_bytes(request.into_body(), MAX_MESSAGE_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return json_rpc_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                JSON_RPC_INVALID_REQUEST,
                "MCP message exceeds the configured limit",
            );
        }
    };
    let line = match frame_message(&bytes) {
        Ok(line) => line,
        Err(message) => {
            return json_rpc_error(StatusCode::BAD_REQUEST, JSON_RPC_PARSE_ERROR, message);
        }
    };
    match queue.try_send(line) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(mpsc::error::TrySendError::Full(_)) => json_rpc_error(
            StatusCode::SERVICE_UNAVAILABLE,
            SESSION_BUSY_CODE,
            "MCP legacy session is busy; retry the message",
        ),
        Err(mpsc::error::TrySendError::Closed(_)) => json_rpc_error(
            StatusCode::NOT_FOUND,
            SESSION_NOT_FOUND_CODE,
            "unknown MCP legacy session",
        ),
    }
}

/// Validate one client message and render it as a single pipe line.
///
/// `2024-11-05` has no JSON-RPC batching, so only a single object is accepted.
/// Re-serializing compactly guarantees the message contains no raw newline,
/// which the line-delimited pipe would otherwise split.
fn frame_message(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "request body is not valid JSON")?;
    if !value.is_object() {
        return Err("MCP 2024-11-05 accepts one JSON-RPC object per request");
    }
    let mut line = serde_json::to_vec(&value).map_err(|_| "request body cannot be re-encoded")?;
    line.push(b'\n');
    Ok(line)
}

fn session_id_from_query(query: Option<&str>) -> Option<String> {
    let value = query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("sessionId="))?;
    let valid = !value.is_empty()
        && value.len() <= MAX_SESSION_ID_CHARS
        && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    valid.then(|| value.to_owned())
}

fn new_session_id() -> String {
    let mut bytes = [0_u8; SESSION_ID_BYTES];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json_rpc_error(status: StatusCode, code: i64, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": serde_json::Value::Null,
            "error": {"code": code, "message": message},
        })
        .to_string(),
    )
        .into_response()
}

/// Drive one session until the client disconnects, the server stops, or the
/// process is cancelled, then release every resource it held.
async fn run_session(
    state: LegacySseState,
    session_id: String,
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    mut events: Sender<Bytes, Infallible>,
) {
    let (mut to_server, server_input) = tokio::io::duplex(PIPE_BYTES);
    let (server_output, from_server) = tokio::io::duplex(PIPE_BYTES);
    let server = state.inner.server.clone();
    // `serve` completes the initialize handshake, which needs POSTs that arrive
    // only after the endpoint event below, so it must not block this task.
    let service = tokio::spawn(async move {
        let running = server
            .serve((server_input, server_output))
            .await
            .map_err(|error| error.to_string())?;
        running
            .waiting()
            .await
            .map(drop)
            .map_err(|error| error.to_string())
    });
    let writer = tokio::spawn(async move {
        while let Some(line) = input_rx.recv().await {
            if to_server.write_all(&line).await.is_err() {
                break;
            }
        }
    });
    let (lines_tx, mut lines_rx) = mpsc::channel::<Result<Vec<u8>, String>>(EVENT_BUFFER);
    let reader = tokio::spawn(read_server_lines(from_server, lines_tx));

    let outcome = pump_events(&state, &session_id, &mut events, &mut lines_rx).await;
    state.remove(&session_id);
    service.abort();
    writer.abort();
    reader.abort();
    if let Err(reason) = outcome {
        // The stream is the only channel to the client; tell it why it ended.
        // A disconnected client simply makes this final send fail.
        let _ = send(&mut events, &format!(": session closed: {reason}\n\n")).await;
    }
}

/// Forward complete server lines. Reading in its own task keeps the pump's
/// `select!` free of partially read lines, which a cancelled read would lose.
async fn read_server_lines(
    pipe: tokio::io::DuplexStream,
    lines: mpsc::Sender<Result<Vec<u8>, String>>,
) {
    let mut reader = BufReader::new(pipe);
    loop {
        let mut line = Vec::new();
        let read = (&mut reader)
            .take((MAX_MESSAGE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .await;
        let item = match read {
            Ok(0) => return,
            Ok(_) if line.len() > MAX_MESSAGE_BYTES => {
                Err(format!("message exceeds {MAX_MESSAGE_BYTES} bytes"))
            }
            Ok(_) => Ok(line),
            Err(error) => Err(error.to_string()),
        };
        let failed = item.is_err();
        if lines.send(item).await.is_err() || failed {
            return;
        }
    }
}

async fn pump_events(
    state: &LegacySseState,
    session_id: &str,
    events: &mut Sender<Bytes, Infallible>,
    lines: &mut mpsc::Receiver<Result<Vec<u8>, String>>,
) -> Result<(), String> {
    let endpoint = format!(
        "event: endpoint\ndata: {}?sessionId={session_id}\n\n",
        state.inner.messages_path
    );
    send(events, &endpoint).await?;
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    // An interval's first tick is immediate; the endpoint event just counted.
    keep_alive.tick().await;
    loop {
        tokio::select! {
            () = state.inner.cancellation.cancelled() => return Ok(()),
            _ = keep_alive.tick() => send(events, ": keep-alive\n\n").await?,
            item = lines.recv() => {
                let Some(item) = item else { return Ok(()) };
                let line = item?;
                let text = String::from_utf8(line)
                    .map_err(|_| "server emitted a non-UTF-8 message".to_owned())?;
                let message = text.trim();
                if !message.is_empty() {
                    send(events, &format!("event: message\ndata: {message}\n\n")).await?;
                }
            }
        }
    }
}

async fn send(events: &mut Sender<Bytes, Infallible>, frame: &str) -> Result<(), String> {
    events
        .send_data(Bytes::from(frame.to_owned()))
        .await
        .map_err(|_| "client disconnected".to_owned())
}
