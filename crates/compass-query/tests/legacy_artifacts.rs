use std::fs;

use compass_graph::{GraphSnapshotBuilder, GraphSnapshotReader};
use compass_model::code_graph::{BuildMetadata, GraphDocument};
use compass_query::{
    DirectGraphEngine, EngineSelection, JsonGraphEngine, QueryEngineCache, QueryError,
    QueryErrorKind, StoreGraphEngine, open_with_engine, open_with_store_selector,
};
use compass_store::{STORE_FILE_NAME, STORE_REF_FILE_NAME, SqliteStore};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const LEGACY: &[u8] = include_bytes!("../../../fixtures/compatibility/compass-0.3.6/graph.json");

fn document(version: &str) -> GraphDocument {
    GraphDocument::empty_v1(BuildMetadata {
        builder_version: version.into(),
        schema_fingerprint: "schema".into(),
        source_tree_digest: "tree".into(),
        configuration_digest: "config".into(),
        generation_id: format!("generation-{version}"),
        source_commit: None,
    })
}

fn rejected<T>(
    result: std::result::Result<T, QueryError>,
    root: Option<&std::path::Path>,
) -> Result {
    let error = result.err().ok_or("legacy engine unexpectedly loaded")?;
    assert_eq!(error.kind(), QueryErrorKind::UnsupportedSchema);
    assert_eq!(error.code(), "legacy_graph_artifact");
    assert!(
        error
            .to_string()
            .contains("Compass 0.3.6; minimum supported builder version is 0.3.23"),
        "{error}"
    );
    if let Some(root) = root {
        assert!(
            error
                .to_string()
                .ends_with(&format!("compass update \"{}\" --force", root.display())),
            "{error}"
        );
    } else {
        assert!(
            error
                .to_string()
                .contains("supply the project root explicitly"),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn authentic_json_fails_when_opening_engines_not_during_adjacency() -> Result {
    let dir = tempfile::tempdir()?;
    let graph = dir.path().join("graph.json");
    fs::write(&graph, LEGACY)?;
    let root = fs::canonicalize(dir.path())?;
    fs::write(
        dir.path().join("source-root.txt"),
        root.to_str().ok_or("root UTF8")?,
    )?;
    rejected(JsonGraphEngine::open(&graph), Some(&root))?;
    for engine in [EngineSelection::Default, EngineSelection::Json] {
        rejected(
            open_with_engine(&graph, None, &dir.path().join("cache"), engine),
            Some(&root),
        )?;
    }
    assert!(!dir.path().join("cache").exists());
    Ok(())
}

#[test]
fn sqlite_checks_pinned_metadata_without_portable_json() -> Result {
    let dir = tempfile::tempdir()?;
    let graph = dir.path().join("graph.json");
    let root = fs::canonicalize(dir.path())?;
    fs::write(
        dir.path().join("source-root.txt"),
        root.to_str().ok_or("root UTF8")?,
    )?;
    let store = SqliteStore::open(dir.path().join(STORE_FILE_NAME))?;
    let prepared = GraphSnapshotBuilder::new().prepare(&store, &document("0.3.6"))?;
    GraphSnapshotBuilder::new().activate(&store, &prepared)?;
    let selector = GraphSnapshotReader::open_active(&store)?
        .ok_or("snapshot missing")?
        .selector()
        .clone();
    fs::write(
        dir.path().join(STORE_REF_FILE_NAME),
        serde_json::to_vec(&store.snapshot_reference()?)?,
    )?;
    store.checkpoint()?;
    assert!(!graph.exists());
    rejected(StoreGraphEngine::from_store(&store), None)?;
    rejected(
        open_with_store_selector(&store, selector, &graph, None, &dir.path().join("cache")),
        Some(&root),
    )?;
    drop(store);
    for engine in [EngineSelection::Default, EngineSelection::Store] {
        rejected(
            open_with_engine(&graph, None, &dir.path().join("cache"), engine),
            Some(&root),
        )?;
    }
    Ok(())
}

#[test]
fn in_memory_and_verified_cache_paths_cannot_bypass_compatibility() -> Result {
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir()?;
    let graph = dir.path().join("graph.json");
    let modern = document("0.3.23");
    let bytes = serde_json::to_vec(&modern)?;
    let identity = format!("{:x}", Sha256::digest(&bytes));
    fs::write(&graph, bytes)?;
    let cache = QueryEngineCache::default();
    cache.open_verified_document(&modern, &identity, &graph, &dir.path().join("cache"))?;
    rejected(
        cache.open_verified_document(
            &document("0.3.6"),
            &identity,
            &graph,
            &dir.path().join("cache"),
        ),
        None,
    )?;
    rejected(DirectGraphEngine::from_document(document("0.3.6")), None)?;
    Ok(())
}

#[cfg(feature = "surreal-surrealkv")]
#[tokio::test]
async fn surreal_checks_generation_metadata_and_cached_repin_without_json() -> Result {
    use compass_graph::canonical_graph_json;
    use compass_graphdb_surreal::{
        ProjectionPlan, SURREAL_DATABASE, SURREAL_NAMESPACE, SURREAL_REF_FILE_NAME, SurrealEngine,
        SurrealProjection, SurrealRef,
    };
    use compass_query::{SurrealQueryEngine, SurrealQueryEngineCache};
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir()?;
    let graph = dir.path().join("graph.json");
    let root = fs::canonicalize(dir.path())?;
    fs::write(
        dir.path().join("source-root.txt"),
        root.to_str().ok_or("root UTF8")?,
    )?;
    let store = dir.path().join("surreal");
    let projection = SurrealProjection::surrealkv(
        store.to_str().ok_or("store UTF8")?,
        SURREAL_NAMESPACE,
        SURREAL_DATABASE,
    )
    .await?;
    let reference = |version| -> Result<SurrealRef> {
        let graph = document(version);
        Ok(SurrealRef::from_plan(
            &ProjectionPlan::from_graph("legacy-test", &graph)?,
            SurrealEngine::SurrealKv,
            &store,
            format!("sha256:{:x}", Sha256::digest(canonical_graph_json(&graph)?)),
        )?)
    };
    let modern = reference("0.3.23")?;
    let old = reference("0.3.6")?;
    for (version, reference) in [("0.3.23", &modern), ("0.3.6", &old)] {
        projection
            .stage_for_reference(
                &ProjectionPlan::from_graph("legacy-test", &document(version))?,
                reference,
            )
            .await?;
    }
    rejected(
        SurrealQueryEngine::from_projection(old.clone(), projection.clone()).await,
        None,
    )?;
    let engine = SurrealQueryEngine::from_projection(modern.clone(), projection.clone())
        .await?
        .with_graph_path(&graph);
    let cache = SurrealQueryEngineCache::from_engine(engine)?;
    fs::write(
        dir.path().join(SURREAL_REF_FILE_NAME),
        serde_json::to_vec(&modern)?,
    )?;
    cache.get(&graph).await?;
    fs::write(
        dir.path().join(SURREAL_REF_FILE_NAME),
        serde_json::to_vec(&old)?,
    )?;
    assert!(!graph.exists());
    rejected(cache.get(&graph).await, Some(&root))?;
    // Rejecting a new reference must not replace the good cached generation.
    fs::write(
        dir.path().join(SURREAL_REF_FILE_NAME),
        serde_json::to_vec(&modern)?,
    )?;
    assert_eq!(
        cache.get(&graph).await?.reference().generation_id,
        modern.generation_id
    );
    drop(cache);
    drop(projection);
    fs::write(
        dir.path().join(SURREAL_REF_FILE_NAME),
        serde_json::to_vec(&old)?,
    )?;
    rejected(SurrealQueryEngine::open(&graph).await, Some(&root))?;
    Ok(())
}
