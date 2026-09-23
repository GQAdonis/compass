use std::error::Error;
use std::fs;

use compass_mcp::CompassMcp;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
    ReadResourceRequestParams,
};
use rmcp::{ClientLifecycleMode, ClientServiceExt, ServiceExt};

#[tokio::test]
async fn stdio_conformance_discovers_lists_invokes_reads_and_closes() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let graph = temp.path().join("graph.json");
    fs::write(
        &graph,
        r#"{
          "directed":true,"multigraph":false,"graph":{},
          "nodes":[{"id":"a","label":"Alpha","community":0}],
          "links":[]
        }"#,
    )?;
    fs::write(
        temp.path().join("GRAPH_REPORT.md"),
        "# Conformance fixture\n",
    )?;

    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        let running = CompassMcp::new(graph)
            .serve(server_transport)
            .await
            .map_err(|error| error.to_string())?;
        running.waiting().await.map_err(|error| error.to_string())
    });
    let client_info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("compass-conformance", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    let client = client_info
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await?;

    let peer = client.peer_info().ok_or("missing MCP peer information")?;
    assert_eq!(peer.protocol_version, ProtocolVersion::V_2026_07_28);
    let server_info = peer
        .server_info
        .as_ref()
        .ok_or("missing MCP server identity")?;
    assert_eq!(server_info.name, "compass");
    let tools = client.list_tools(None).await?;
    let tool_names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names,
        [
            "search_symbols",
            "get_callers",
            "get_callees",
            "get_impact",
            "explore_code",
            "get_node",
            "query_graph",
            "get_neighbors",
            "get_community",
            "god_nodes",
            "graph_stats",
            "shortest_path",
            "list_prs",
            "get_pr_impact",
            "triage_prs",
            "review_pull_request",
            "pr_readiness",
            "task_context",
        ]
    );
    let resources = client.list_resources(None).await?;
    let resource_uris = resources
        .resources
        .iter()
        .map(|resource| resource.uri.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        resource_uris,
        [
            "compass://orientation",
            "compass://report",
            "compass://stats",
            "compass://god-nodes",
            "compass://surprises",
            "compass://audit",
            "compass://questions",
            "compass://graph-insights",
        ]
    );

    let call = client
        .call_tool(CallToolRequestParams::new("graph_stats"))
        .await?;
    let text = call
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|content| content.text.as_str());
    assert!(text.is_some_and(|text| text.contains("Nodes: 1")));

    let stats = client
        .read_resource(ReadResourceRequestParams::new("compass://stats"))
        .await?;
    assert_eq!(stats.contents.len(), 1);
    let missing = client
        .read_resource(ReadResourceRequestParams::new("compass://missing"))
        .await
        .expect_err("missing resource unexpectedly succeeded");
    assert!(format!("{missing:?}").contains("compass://missing"));

    client.cancel().await?;
    server_task.await?.map_err(std::io::Error::other)?;
    Ok(())
}

/// Every advertised legacy revision must be accepted through `initialize`.
///
/// The Boss SDK requests `2025-11-25` and uses `initialize`. The server must
/// echo a supported requested revision and allow subsequent requests.
#[tokio::test]
async fn stdio_accepts_initialize_for_every_pre_discovery_revision() -> Result<(), Box<dyn Error>> {
    for version in [
        ProtocolVersion::V_2025_11_25,
        ProtocolVersion::V_2025_06_18,
        ProtocolVersion::V_2025_03_26,
    ] {
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let server_task = tokio::spawn(async move {
            let running = CompassMcp::new("missing.json")
                .serve(server_transport)
                .await
                .map_err(|error| error.to_string())?;
            running.waiting().await.map_err(|error| error.to_string())
        });
        let client_info = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("initialize-conformance-client", env!("CARGO_PKG_VERSION")),
        )
        .with_protocol_version(version.clone());
        let client = client_info
            .serve_with_lifecycle(client_transport, ClientLifecycleMode::Initialize)
            .await
            .map_err(|error| format!("initialize refused for {version}: {error}"))?;

        let peer = client.peer_info().ok_or("missing MCP peer information")?;
        assert_eq!(
            peer.protocol_version, version,
            "server did not echo the negotiated revision"
        );
        // A handshake alone proves little; a real request must work on the negotiated session.
        client.list_tools(None).await?;

        client.cancel().await?;
        server_task.await?.map_err(std::io::Error::other)?;
    }
    Ok(())
}

/// Compass requires `server/discover` (SEP-2575) when the client explicitly names
/// `2026-07-28`; rmcp's default would instead negotiate a legacy initialize version.
#[tokio::test]
async fn stdio_refuses_initialize_for_the_discovery_revision() -> Result<(), Box<dyn Error>> {
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server_task = tokio::spawn(async move {
        let running = CompassMcp::new("missing.json")
            .serve(server_transport)
            .await
            .map_err(|error| error.to_string())?;
        running.waiting().await.map_err(|error| error.to_string())
    });
    let client_info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("discovery-revision-client", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    let client_result = client_info
        .serve_with_lifecycle(client_transport, ClientLifecycleMode::Initialize)
        .await;

    let error = client_result
        .err()
        .ok_or("initialize was accepted for 2026-07-28")?;
    assert!(
        error
            .to_string()
            .contains("initialize is not available in MCP 2026-07-28"),
        "unexpected refusal: {error}"
    );
    assert!(server_task.await?.is_err());
    Ok(())
}
