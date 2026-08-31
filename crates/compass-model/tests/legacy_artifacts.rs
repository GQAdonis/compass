use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use compass_model::code_graph::{BuildMetadata, GraphDocument as CodeGraph};
use compass_model::{GraphDocument, GraphError, MAX_GRAPH_PREAMBLE_BYTES, validate_graph_preamble};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const LEGACY: &[u8] = include_bytes!("../../../fixtures/compatibility/compass-0.3.6/graph.json");

fn legacy(error: GraphError) -> Result<String> {
    assert!(
        matches!(&error, GraphError::LegacyArtifact { found, minimum, .. }
        if found == "0.3.6" && *minimum == "0.3.23"),
        "{error}"
    );
    Ok(error.to_string())
}

#[test]
fn authentic_release_fails_at_every_public_file_loader() -> Result {
    let dir = tempfile::tempdir()?;
    let graph = dir.path().join("graph.json");
    fs::write(&graph, LEGACY)?;
    let root = fs::canonicalize(dir.path())?;
    fs::write(
        dir.path().join("source-root.txt"),
        root.to_str().ok_or("root UTF8")?,
    )?;
    let errors = [
        CodeGraph::load(&graph).err(),
        CodeGraph::load_for_affected(&graph).err(),
        CodeGraph::load_for_recluster(&graph).err(),
        CodeGraph::load_with_artifact_digest(&graph).err(),
        CodeGraph::load_for_recluster_with_artifact_digest(&graph).err(),
        GraphDocument::load(&graph).err(),
        GraphDocument::load_for_affected(&graph).err(),
        GraphDocument::load_for_traversal(&graph).err(),
        GraphDocument::load_for_recluster(&graph).err(),
    ];
    for error in errors {
        assert_eq!(
            legacy(error.ok_or("legacy graph loaded")?)?,
            format!(
                "graph artifact was built by Compass 0.3.6; minimum supported builder version is 0.3.23. Rebuild with: compass update \"{}\" --force",
                root.display()
            )
        );
    }
    assert!(!dir.path().join("cache").exists());
    Ok(())
}

#[test]
fn malformed_large_record_tail_is_never_decoded_to_reject_legacy() -> Result {
    let dir = tempfile::tempdir()?;
    let graph = dir.path().join("graph.json");
    let mut bytes = br#"{"graph":{"build":{"builderVersion":"0.3.6"}},"nodes":["#.to_vec();
    bytes.extend(vec![b'!'; 4 * MAX_GRAPH_PREAMBLE_BYTES as usize]);
    fs::write(&graph, bytes)?;
    legacy(
        CodeGraph::load_with_artifact_digest(&graph)
            .err()
            .ok_or("loaded malformed legacy")?,
    )?;
    legacy(
        GraphDocument::load_for_traversal(&graph)
            .err()
            .ok_or("traversed malformed legacy")?,
    )?;
    Ok(())
}

struct CountingReader {
    inner: Cursor<Vec<u8>>,
    bytes: usize,
}
impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.bytes += count;
        Ok(count)
    }
}

#[test]
fn preamble_stops_at_builder_and_never_reads_over_its_budget() -> Result {
    let mut reader = CountingReader {
        inner: Cursor::new(LEGACY.to_vec()),
        bytes: 0,
    };
    legacy(
        validate_graph_preamble(&mut reader, Path::new("graph.json"))
            .err()
            .ok_or("accepted legacy")?,
    )?;
    assert!(reader.bytes < 256, "read {} bytes", reader.bytes);
    let mut reader = CountingReader {
        inner: Cursor::new(
            format!(
                "{{\"padding\":\"{}\"}}",
                "x".repeat(2 * MAX_GRAPH_PREAMBLE_BYTES as usize)
            )
            .into_bytes(),
        ),
        bytes: 0,
    };
    assert!(matches!(
        validate_graph_preamble(&mut reader, Path::new("graph.json")),
        Err(GraphError::GraphPreambleLimit { .. })
    ));
    assert_eq!(reader.bytes, MAX_GRAPH_PREAMBLE_BYTES as usize);
    Ok(())
}

#[test]
fn release_floor_and_current_cache_behavior_remain_explicit() -> Result {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("graph.json");
    for version in ["0.3.6", "0.3.22", "0.3.23-rc.1", "v0.3.6+build"] {
        let bytes = format!(r#"{{"graph":{{"build":{{"builderVersion":"{version}"}}}}}}"#);
        assert!(
            matches!(
                validate_graph_preamble(bytes.as_bytes(), &path),
                Err(GraphError::LegacyArtifact { .. })
            ),
            "{version}"
        );
    }
    for version in ["0.3.23", "0.3.23+build", "0.3.24", "1.0.0", "test"] {
        let document = CodeGraph::empty_v1(BuildMetadata {
            builder_version: version.into(),
            schema_fingerprint: "schema".into(),
            source_tree_digest: "tree".into(),
            configuration_digest: "config".into(),
            generation_id: "generation".into(),
            source_commit: None,
        });
        fs::write(&path, serde_json::to_vec(&document)?)?;
        CodeGraph::load(&path)?;
        CodeGraph::load(&path)?;
        GraphDocument::load(&path)?;
        GraphDocument::load_for_traversal(&path)?;
        GraphDocument::load_for_affected(&path)?;
    }
    fs::write(&path, LEGACY)?;
    legacy(
        CodeGraph::load(&path)
            .err()
            .ok_or("reused modern content cache")?,
    )?;
    legacy(
        GraphDocument::load(&path)
            .err()
            .ok_or("reused modern query cache")?,
    )?;
    Ok(())
}

#[test]
fn absent_or_invalid_provenance_requires_an_explicit_root() -> Result {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("graph.json");
    fs::write(&path, LEGACY)?;
    let check = || -> Result {
        let error = legacy(CodeGraph::load(&path).err().ok_or("accepted legacy")?)?;
        assert!(
            error.contains("provenance is absent or invalid; supply the project root explicitly")
        );
        assert!(error.ends_with("compass update \"<source-root>\" --force"));
        Ok(())
    };
    check()?;
    for value in [
        "relative/root".to_owned(),
        "\n".into(),
        "x".repeat(16 * 1024 + 1),
        format!("{}/../elsewhere", dir.path().display()),
        format!("{}\nextra", dir.path().display()),
        dir.path().join("absent").display().to_string(),
        path.display().to_string(),
    ] {
        fs::write(dir.path().join("source-root.txt"), value)?;
        check()?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn provenance_symlinks_are_rejected_and_shell_metacharacters_are_escaped() -> Result {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("graph.json");
    fs::write(&path, LEGACY)?;
    let root = dir
        .path()
        .join("project $name `command` \"quoted\" \\ path");
    fs::create_dir(&root)?;
    let root = fs::canonicalize(root)?;
    let text = root.to_str().ok_or("root UTF8")?;
    let provenance = dir.path().join("source-root.txt");
    fs::write(&provenance, format!("{text}\r\n"))?;
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    let error = legacy(CodeGraph::load(&path).err().ok_or("accepted legacy")?)?;
    assert!(
        error.ends_with(&format!("compass update \"{escaped}\" --force")),
        "{error}"
    );
    let target = dir.path().join("other-provenance");
    fs::rename(&provenance, &target)?;
    std::os::unix::fs::symlink(&target, &provenance)?;
    let error = legacy(CodeGraph::load(&path).err().ok_or("accepted legacy")?)?;
    assert!(error.contains("supply the project root explicitly"));
    Ok(())
}
