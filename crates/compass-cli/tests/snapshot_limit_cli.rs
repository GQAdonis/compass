use std::error::Error;
use std::fs;
use std::process::Command;

use compass_files::BuildGuard;
use compass_graph::{
    GRAPH_SNAPSHOT_ACTIVE_KEY, GRAPH_SNAPSHOT_CATALOG_PARTITION, GRAPH_SNAPSHOT_OBJECT_PARTITION,
    GraphSnapshotReader, SnapshotSelector, graph_snapshot_manifest_key,
};
use compass_store::{
    Key, MAX_GRAPH_BYTES, NamespaceId, PartitionKey, SqliteStore, Store, StoreRef, WriteCondition,
    local_sqlite_store_path,
};
use sha2::{Digest, Sha256};

#[test]
fn source_size_prediction_cannot_reject_an_admissible_graph() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("sparse.rs");
    let mut admitted = vec![b' '; 32 * 1024];
    let declaration = b"pub fn admitted() -> u64 { 1 }\n";
    admitted[..declaration.len()].copy_from_slice(declaration);
    fs::write(&source, admitted)?;

    let built = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args([
            "update",
            ".",
            "--force",
            "--store",
            "json",
            "--max-source-bytes",
            "65536",
            "--no-cluster",
            "--no-viz",
            "--no-program",
        ])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .env("COMPASS_MAX_GRAPH_BYTES", "1048576")
        .output()?;
    assert!(
        built.status.success(),
        "actual-under-cap graph was rejected: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let graph = BuildGuard::resolve_artifact(&root.path().join("compass-out"), "graph.json")?;
    assert!(graph.metadata()?.len() < 1_048_576);
    Ok(())
}

#[test]
fn actual_delta_overrun_preserves_the_active_snapshot() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("sample.rs");
    fs::write(&source, "pub fn sample() -> u64 { 1 }\n// before\n")?;
    let initial = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args([
            "update",
            ".",
            "--force",
            "--store",
            "json",
            "--no-cluster",
            "--no-viz",
            "--no-program",
        ])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .env_remove("COMPASS_MAX_GRAPH_BYTES")
        .output()?;
    assert!(
        initial.status.success(),
        "initial publication failed: {}",
        String::from_utf8_lossy(&initial.stderr)
    );

    let output = root.path().join("compass-out");
    let active_graph = BuildGuard::resolve_artifact(&output, "graph.json")?;
    let prior_bytes = fs::read(&active_graph)?;
    let maximum = prior_bytes.len().to_string();
    fs::write(
        &source,
        format!(
            "pub fn sample() -> u64 {{ 1 }}\n// {}\n",
            "after".repeat(200)
        ),
    )?;

    let rejected = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args([
            "update",
            ".",
            "--store",
            "json",
            "--no-cluster",
            "--no-viz",
            "--no-program",
        ])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .env("COMPASS_MAX_GRAPH_BYTES", maximum)
        .output()?;
    assert_eq!(rejected.status.code(), Some(1));
    let stderr = String::from_utf8(rejected.stderr)?;
    assert!(stderr.contains("canonical graph exceeds"), "{stderr}");
    assert!(stderr.contains("COMPASS_MAX_GRAPH_BYTES"), "{stderr}");

    let still_active = BuildGuard::resolve_artifact(&output, "graph.json")?;
    assert_eq!(fs::read(still_active)?, prior_bytes);
    Ok(())
}

#[test]
fn actual_full_overrun_preserves_the_active_sqlite_snapshot() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("sample.rs");
    fs::write(&source, "pub fn sample() -> u64 { 1 }\n")?;
    let initial = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args([
            "update",
            ".",
            "--force",
            "--store",
            "sqlite",
            "--no-cluster",
            "--no-viz",
            "--no-program",
        ])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .env_remove("COMPASS_MAX_GRAPH_BYTES")
        .output()?;
    assert!(
        initial.status.success(),
        "initial SQLite publication failed: {}",
        String::from_utf8_lossy(&initial.stderr)
    );

    let output = root.path().join("compass-out");
    let active_graph = BuildGuard::resolve_artifact(&output, "graph.json")?;
    let prior_graph = fs::read(&active_graph)?;
    let active_store_ref = BuildGuard::resolve_artifact(&output, "store.ref")?;
    let prior_store_ref = fs::read(&active_store_ref)?;
    let expanded = (0..200)
        .map(|index| format!("pub fn added_{index}() -> usize {{ {index} }}\n"))
        .collect::<String>();
    fs::write(&source, expanded)?;

    let rejected = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args([
            "update",
            ".",
            "--store",
            "sqlite",
            "--no-cluster",
            "--no-viz",
            "--no-program",
        ])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .env("COMPASS_MAX_GRAPH_BYTES", prior_graph.len().to_string())
        .output()?;
    assert_eq!(rejected.status.code(), Some(1));
    let stderr = String::from_utf8(rejected.stderr)?;
    assert!(stderr.contains("canonical graph exceeds"), "{stderr}");

    let still_active_graph = BuildGuard::resolve_artifact(&output, "graph.json")?;
    let still_active_ref = BuildGuard::resolve_artifact(&output, "store.ref")?;
    assert_eq!(fs::read(still_active_graph)?, prior_graph);
    assert_eq!(fs::read(still_active_ref)?, prior_store_ref);
    Ok(())
}

#[test]
fn oversized_snapshot_limit_error_names_scope_remedies_and_exits_one() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("sample.rs"), "pub fn sample() {}\n")?;
    let built = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args(["update", ".", "--code-only", "--no-viz"])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .output()?;
    assert!(
        built.status.success(),
        "initial build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let output_root = root.path().join("compass-out");
    let graph_path = BuildGuard::resolve_artifact(&output_root, "graph.json")?;
    let store_path = local_sqlite_store_path(&graph_path);
    let store = SqliteStore::open(&store_path)?;
    let namespace = NamespaceId::graph();
    let catalog = PartitionKey::new(GRAPH_SNAPSHOT_CATALOG_PARTITION)?;
    let objects = PartitionKey::new(GRAPH_SNAPSHOT_OBJECT_PARTITION)?;
    let active = Key::new(GRAPH_SNAPSHOT_ACTIVE_KEY)?;
    let active_entry = store
        .get(&namespace, &catalog, &active)?
        .ok_or("store has no active graph snapshot")?;
    let mut selector: SnapshotSelector = serde_json::from_slice(&active_entry.value)?;
    let reader = GraphSnapshotReader::open_selector(&store, selector.clone())?;
    let mut manifest = reader.manifest().clone();
    manifest.graph_bytes = MAX_GRAPH_BYTES as u64 + 1;
    drop(reader);
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let manifest_digest = format!("{:x}", Sha256::digest(&manifest_bytes));
    let manifest_key = graph_snapshot_manifest_key(&manifest_digest)?;
    store.put(
        &namespace,
        &objects,
        &manifest_key,
        &manifest_bytes,
        WriteCondition::Missing,
    )?;

    selector.manifest_digest = manifest_digest;
    let selector_bytes = serde_json::to_vec(&selector)?;
    store.put(
        &namespace,
        &catalog,
        &active,
        &selector_bytes,
        WriteCondition::Version(active_entry.version),
    )?;
    store.checkpoint()?;
    drop(store);

    let reference_path = graph_path
        .parent()
        .ok_or("resolved graph has no parent directory")?
        .join("store.ref");
    let mut reference: StoreRef = serde_json::from_slice(&fs::read(&reference_path)?)?;
    reference.manifest_digest = selector.manifest_digest;
    fs::write(&reference_path, serde_json::to_vec_pretty(&reference)?)?;

    let queried = Command::new(env!("CARGO_BIN_EXE_compass"))
        .args(["search", "sample", "--graph"])
        .arg(&graph_path)
        .args(["--engine", "store"])
        .current_dir(root.path())
        .env_remove("COMPASS_OUT")
        .output()?;
    assert_eq!(queried.status.code(), Some(1));
    assert!(queried.stdout.is_empty());
    let stderr = String::from_utf8(queried.stderr)?;
    assert!(stderr.contains("snapshot limit exceeded"), "{stderr}");
    assert!(stderr.contains("--exclude <pattern>"), "{stderr}");
    assert!(stderr.contains(".compassignore"), "{stderr}");
    assert!(stderr.contains("COMPASS_MAX_GRAPH_BYTES"), "{stderr}");
    Ok(())
}
