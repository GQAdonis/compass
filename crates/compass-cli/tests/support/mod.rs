#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use compass_model::code_graph::{
    BuildMetadata, EdgeKind, EdgeRecord, ExtractionStatus, FileRecord, GraphDocument, NodeKind,
    NodeRecord,
};
use compass_model::identity::{edge_id, file_id};
use compass_model::provenance::{EvidenceConfidence, EvidenceOrigin, Provenance, SourceAnchor};
use sha2::{Digest, Sha256};

pub fn compass_executable() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_compass"))
}

pub fn compass_command() -> Command {
    command(compass_executable())
}

pub fn command(executable: &Path) -> Command {
    Command::new(executable)
}

pub fn write_typed_graph(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let graph_path = root.join("graph.json");
    let source_path = root.join("src/lib.rs");
    let source = b"code";
    fs::create_dir_all(source_path.parent().unwrap_or(root))?;
    fs::write(&source_path, source)?;
    let anchor = SourceAnchor {
        file: "src/lib.rs".to_owned(),
        start_byte: 0,
        end_byte: 4,
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 4,
    };
    let evidence = Provenance {
        origin: EvidenceOrigin::Ast,
        extractor: "cli-test".to_owned(),
        confidence: EvidenceConfidence::Exact,
        rule: None,
        anchors: vec![anchor.clone()],
        wiring_site: None,
        score: None,
        candidates: Vec::new(),
    };
    let mut graph = GraphDocument::empty_v1(BuildMetadata {
        builder_version: "test".to_owned(),
        schema_fingerprint: "sha256:test".to_owned(),
        source_tree_digest: "sha256:test".to_owned(),
        configuration_digest: "sha256:test".to_owned(),
        generation_id: "sha256:test".to_owned(),
        source_commit: None,
    });
    graph.graph.files.push(FileRecord {
        id: file_id("src/lib.rs"),
        path: "src/lib.rs".to_owned(),
        language: Some("rust".to_owned()),
        content_digest: format!("sha256:{:x}", Sha256::digest(source)),
        byte_size: 4,
        generated: false,
        extraction_status: ExtractionStatus::Extracted,
        extractor_versions: vec!["cli-test".to_owned()],
        coverage: Vec::new(),
        diagnostics: Vec::new(),
    });
    graph.nodes = ["Caller", "Target"]
        .into_iter()
        .map(|name| NodeRecord {
            id: format!("n:{}", name.to_ascii_lowercase()),
            kind: NodeKind::Function,
            roles: Vec::new(),
            name: name.to_owned(),
            qualified_name: format!("Fixture.{name}"),
            language: Some("rust".to_owned()),
            framework: None,
            source: Some(anchor.clone()),
            details: None,
            evidence: vec![evidence.clone()],
            coverage: Vec::new(),
            diagnostics: Vec::new(),
            community: None,
        })
        .collect();
    let id = edge_id("n:caller", EdgeKind::Calls, "n:target", Some(&anchor), None);
    graph.links.push(EdgeRecord {
        id: id.clone(),
        key: id,
        source: "n:caller".to_owned(),
        target: "n:target".to_owned(),
        kind: EdgeKind::Calls,
        occurrence_rule: None,
        relationship_site: Some(anchor),
        details: None,
        evidence: vec![evidence],
        weight: None,
        context: None,
        deferred: false,
        diagnostics: Vec::new(),
    });
    fs::write(&graph_path, serde_json::to_vec_pretty(&graph)?)?;
    Ok(graph_path)
}

/// Write a typed graph whose two nodes share one exact name.
pub fn write_typed_ambiguous_graph(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let graph_path = root.join("graph.json");
    let mut graph = GraphDocument::empty_v1(BuildMetadata {
        builder_version: "test".to_owned(),
        schema_fingerprint: "sha256:test".to_owned(),
        source_tree_digest: "sha256:test".to_owned(),
        configuration_digest: "sha256:test".to_owned(),
        generation_id: "sha256:test".to_owned(),
        source_commit: None,
    });
    for (id, file, qualified_name) in [
        ("n:alpha-run", "src/a.rs", "Alpha.run"),
        ("n:beta-run", "src/b.rs", "Beta.run"),
    ] {
        let source_path = root.join(file);
        fs::create_dir_all(source_path.parent().unwrap_or(root))?;
        fs::write(&source_path, b"code")?;
        let anchor = SourceAnchor {
            file: file.to_owned(),
            start_byte: 0,
            end_byte: 4,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 4,
        };
        graph.graph.files.push(FileRecord {
            id: file_id(file),
            path: file.to_owned(),
            language: Some("rust".to_owned()),
            content_digest: format!("sha256:{:x}", Sha256::digest(b"code")),
            byte_size: 4,
            generated: false,
            extraction_status: ExtractionStatus::Extracted,
            extractor_versions: vec!["cli-test".to_owned()],
            coverage: Vec::new(),
            diagnostics: Vec::new(),
        });
        graph.nodes.push(NodeRecord {
            id: id.to_owned(),
            kind: NodeKind::Function,
            roles: Vec::new(),
            name: "run".to_owned(),
            qualified_name: qualified_name.to_owned(),
            language: Some("rust".to_owned()),
            framework: None,
            source: Some(anchor.clone()),
            details: None,
            evidence: vec![Provenance {
                origin: EvidenceOrigin::Ast,
                extractor: "cli-test".to_owned(),
                confidence: EvidenceConfidence::Exact,
                rule: None,
                anchors: vec![anchor],
                wiring_site: None,
                score: None,
                candidates: Vec::new(),
            }],
            coverage: Vec::new(),
            diagnostics: Vec::new(),
            community: None,
        });
    }
    fs::write(&graph_path, serde_json::to_vec_pretty(&graph)?)?;
    Ok(graph_path)
}
