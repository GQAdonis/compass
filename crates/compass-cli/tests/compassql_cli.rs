use std::error::Error;
use std::process::Command;

use compass_model::{
    code_graph::EdgeKind,
    identity::{edge_id, file_id},
    provenance::SourceAnchor,
};
use serde_json::json;

fn seed_graph(directory: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = directory.join("graph.json");
    let anchor = SourceAnchor {
        file: "src/lib.rs".to_owned(),
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 1,
    };
    let evidence = json!([{
        "origin": "ast",
        "extractor": "compass-cli-test",
        "confidence": "exact",
        "anchors": [anchor.clone()]
    }]);
    let relationship_id = edge_id("a", EdgeKind::Calls, "b", Some(&anchor), None);
    let digest = format!("sha256:{}", "0".repeat(64));
    let document = json!({
        "directed": true,
        "multigraph": true,
        "graph": {
            "schema": "compass.graph/1",
            "build": {
                "builderVersion": env!("CARGO_PKG_VERSION"),
                "schemaFingerprint": digest,
                "sourceTreeDigest": digest,
                "configurationDigest": digest,
                "generationId": digest
            },
            "files": [{
                "id": file_id("src/lib.rs"),
                "path": "src/lib.rs",
                "language": "rust",
                "contentDigest": digest,
                "byteSize": 1,
                "generated": false,
                "extractionStatus": "extracted"
            }]
        },
        "nodes": [
            {"id":"a","kind":"function","name":"a","qualifiedName":"fixture::a","evidence":evidence},
            {"id":"b","kind":"function","name":"b","qualifiedName":"fixture::b","evidence":evidence}
        ],
        "links": [{
            "id": relationship_id,
            "key": relationship_id,
            "source": "a",
            "target": "b",
            "kind": "calls",
            "relationshipSite": anchor,
            "evidence": evidence
        }]
    });
    std::fs::write(&path, serde_json::to_vec(&document)?)?;
    Ok(path)
}

#[test]
fn compassql_cli_supports_typed_output_files_and_limits() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let graph = seed_graph(directory.path())?;
    let output_path = directory.path().join("result.json");
    let compass = env!("CARGO_BIN_EXE_compass");
    let output = Command::new(compass)
        .args([
            "query",
            "--cql",
            "PROFILE MATCH (a)-[:CALLS]->(b) RETURN a.id AS caller, b.id AS callee",
            "--format=json",
            "--timeout-ms=2000",
            "--max-rows=10",
            "--max-path-depth=4",
            "--max-expanded-relationships=100",
            "--max-memory-bytes=1048576",
            "--graph",
        ])
        .arg(&graph)
        .args(["--output"])
        .arg(&output_path)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&output_path)?)?;
    assert_eq!(value["schema"], "compass.cql.result/1");
    assert_eq!(value["rows"][0]["caller"]["value"], "a");
    assert_eq!(value["profile"]["plan_cache_hit"], false);

    Ok(())
}
