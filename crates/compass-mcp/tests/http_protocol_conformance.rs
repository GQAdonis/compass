use std::error::Error;
use std::net::SocketAddr;
use std::time::Duration;

use compass_mcp::{HttpOptions, serve_http};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct ServerTask(tokio::task::JoinHandle<Result<(), String>>);

impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn legacy_http_negotiates_versions_and_rejects_malformed_headers()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = reservation.local_addr()?;
    let mut options = HttpOptions::new(temp.path().join("missing.json"));
    options.port = address.port();
    options.api_key = Some("protocol-test-key".to_owned());
    options.json_response = true;
    options.max_sessions = 3;
    drop(reservation);
    let server = ServerTask(tokio::spawn(serve_http(options)));
    tokio::time::timeout(Duration::from_secs(10), async {
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

    for version in ["2025-11-25", "2025-06-18", "2025-03-26"] {
        let initialize = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": version, "capabilities": {},
                "clientInfo": {"name": "http-conformance", "version": "1"}}
        });
        let initialized = post(address, b"", &initialize).await?;
        assert!(initialized.starts_with("HTTP/1.1 200 OK"));
        assert!(
            initialized
                .to_ascii_lowercase()
                .contains("content-type: text/event-stream")
        );
        assert_eq!(payload(&initialized)?["result"]["protocolVersion"], version);
        let session = header(&initialized, "mcp-session-id").ok_or("missing session id")?;
        let headers = format!("Mcp-Session-Id: {session}\r\nMcp-Protocol-Version: {version}\r\n");
        let notified = post(
            address,
            headers.as_bytes(),
            &json!({
                "jsonrpc": "2.0", "method": "notifications/initialized"
            }),
        )
        .await?;
        assert!(notified.starts_with("HTTP/1.1 202 Accepted"));
        let listed = post(
            address,
            headers.as_bytes(),
            &json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
            }),
        )
        .await?;
        assert!(listed.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(
            payload(&listed)?["result"]["tools"]
                .as_array()
                .map(Vec::len),
            Some(18)
        );
    }

    let initialize = json!({
        "jsonrpc": "2.0", "id": 3, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "invalid-header", "version": "1"}}
    });
    for headers in [
        b"Mcp-Protocol-Version:\r\n".as_slice(),
        b"Mcp-Protocol-Version:   \r\n".as_slice(),
        b"Mcp-Protocol-Version: \xff\r\n".as_slice(),
        b"Mcp-Protocol-Version: 1900-01-01\r\n".as_slice(),
    ] {
        let rejected = post(address, headers, &initialize).await?;
        assert!(rejected.starts_with("HTTP/1.1 400 Bad Request"));
        assert_eq!(payload(&rejected)?["error"]["code"], -32022);
    }
    let at_capacity = post(address, b"", &initialize).await?;
    assert!(at_capacity.starts_with("HTTP/1.1 429 Too Many Requests"));
    assert_eq!(payload(&at_capacity)?["error"]["code"], -32024);
    assert_eq!(payload(&at_capacity)?["error"]["data"]["max_sessions"], 3);
    Ok(())
}

async fn post(
    address: SocketAddr,
    headers: &[u8],
    message: &Value,
) -> Result<String, Box<dyn Error>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let body = serde_json::to_vec(message)?;
        let mut wire = format!(
            "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer protocol-test-key\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        ).into_bytes();
        wire.extend_from_slice(headers);
        wire.extend_from_slice(b"\r\n");
        wire.extend_from_slice(&body);
        let mut stream = TcpStream::connect(address).await?;
        stream.write_all(&wire).await?;
        let mut response = Vec::new();
        stream.take(1024 * 1024).read_to_end(&mut response).await?;
        Ok(String::from_utf8(response)?)
    }).await?
}

fn header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response
        .split_once("\r\n\r\n")?
        .0
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}

fn payload(response: &str) -> Result<Value, Box<dyn Error>> {
    let body = response
        .split_once("\r\n\r\n")
        .ok_or("missing HTTP body")?
        .1;
    if header(response, "content-type").is_some_and(|value| value.starts_with("text/event-stream"))
    {
        let data = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .find(|data| !data.is_empty())
            .ok_or("missing SSE payload")?;
        Ok(serde_json::from_str(data)?)
    } else {
        Ok(serde_json::from_str(body)?)
    }
}
