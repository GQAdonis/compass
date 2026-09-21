#![cfg(any(feature = "surrealkv", feature = "rocksdb"))]

use std::fs;
use std::path::Path;

use compass_graphdb_surreal::{
    InterruptAfter, ProjectionError, ProjectionPlan, SurrealEngine, SurrealProjection, SurrealRef,
};
use compass_model::code_graph::{BuildMetadata, EdgeKind, GraphDocument, NodeKind};
use compass_model::identity::{edge_id, file_id, symbol_id};
use compass_model::provenance::SourceAnchor;
use compass_model::query_contract::{
    CallRequest, CodeQueryLimits, ExploreRequest, ImpactRequest, NodeTrailRequest, SearchRequest,
};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
enum Backend {
    #[cfg(feature = "surrealkv")]
    SurrealKv,
    #[cfg(feature = "rocksdb")]
    RocksDb,
}

impl Backend {
    fn engine(self) -> SurrealEngine {
        match self {
            #[cfg(feature = "surrealkv")]
            Self::SurrealKv => SurrealEngine::SurrealKv,
            #[cfg(feature = "rocksdb")]
            Self::RocksDb => SurrealEngine::RocksDb,
        }
    }

    async fn open(self, path: &Path) -> Result<SurrealProjection, ProjectionError> {
        let path = path.to_str().ok_or_else(|| {
            ProjectionError::InvalidPlan("test storage path is not UTF-8".to_owned())
        })?;
        match self {
            #[cfg(feature = "surrealkv")]
            Self::SurrealKv => SurrealProjection::surrealkv(path, "compass", "graph").await,
            #[cfg(feature = "rocksdb")]
            Self::RocksDb => SurrealProjection::rocksdb(path, "compass", "graph").await,
        }
    }

    async fn open_existing(self, path: &Path) -> Result<SurrealProjection, ProjectionError> {
        let path = path.to_str().ok_or_else(|| {
            ProjectionError::InvalidPlan("test storage path is not UTF-8".to_owned())
        })?;
        match self {
            #[cfg(feature = "surrealkv")]
            Self::SurrealKv => {
                SurrealProjection::surrealkv_existing(path, "compass", "graph").await
            }
            #[cfg(feature = "rocksdb")]
            Self::RocksDb => SurrealProjection::rocksdb_existing(path, "compass", "graph").await,
        }
    }
}

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn graph(generation: char, suffix: &str) -> Result<GraphDocument, serde_json::Error> {
    let path = "src/lib.rs";
    let caller = symbol_id(
        "rust",
        path,
        NodeKind::Function,
        &format!("caller{suffix}"),
        "",
    );
    let callee = symbol_id(
        "rust",
        path,
        NodeKind::Function,
        &format!("callee{suffix}"),
        "",
    );
    let leaf = symbol_id(
        "rust",
        path,
        NodeKind::Function,
        &format!("leaf{suffix}"),
        "",
    );
    let anchor = SourceAnchor {
        file: path.to_owned(),
        start_byte: 0,
        end_byte: 4,
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 4,
    };
    let evidence = json!([{
        "origin": "ast",
        "extractor": "persistent-contract",
        "confidence": "exact",
        "anchors": [anchor]
    }]);
    let mut graph = GraphDocument::empty_v1(BuildMetadata {
        builder_version: "integration".to_owned(),
        schema_fingerprint: digest('1'),
        source_tree_digest: digest('2'),
        configuration_digest: digest('3'),
        generation_id: digest(generation),
        source_commit: None,
    });
    graph.graph.files = serde_json::from_value(json!([{
        "id": file_id(path),
        "path": path,
        "contentDigest": digest('5'),
        "byteSize": 4,
        "generated": false,
        "extractionStatus": "extracted"
    }]))?;
    graph.nodes = serde_json::from_value(json!([
        {
            "id": caller,
            "kind": "function",
            "name": format!("caller{suffix}"),
            "qualifiedName": format!("fixture::caller{suffix}"),
            "language": "rust",
            "source": anchor,
            "evidence": evidence
        },
        {
            "id": callee,
            "kind": "function",
            "name": format!("callee{suffix}"),
            "qualifiedName": format!("fixture::callee{suffix}"),
            "language": "rust",
            "source": anchor,
            "evidence": evidence
        },
        {
            "id": leaf,
            "kind": "function",
            "name": format!("leaf{suffix}"),
            "qualifiedName": format!("fixture::leaf{suffix}"),
            "language": "rust",
            "source": anchor,
            "evidence": evidence
        }
    ]))?;
    let relation = |source: &str, target: &str, rule: &str| {
        let id = edge_id(source, EdgeKind::Calls, target, Some(&anchor), Some(rule));
        json!({
            "id": id,
            "key": id,
            "source": source,
            "target": target,
            "kind": "calls",
            "occurrenceRule": rule,
            "relationshipSite": anchor,
            "evidence": evidence
        })
    };
    graph.links = serde_json::from_value(json!([
        relation(&caller, &callee, "caller-to-callee"),
        relation(&callee, &leaf, "callee-to-leaf")
    ]))?;
    Ok(graph)
}

fn limits() -> CodeQueryLimits {
    CodeQueryLimits {
        max_depth: 8,
        max_nodes: 64,
        max_edges: 64,
        max_paths: 32,
        max_candidates: 16,
        max_source_bytes: 1_024,
        max_response_bytes: 1_048_576,
    }
}

async fn exercise(backend: Backend) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let store_path = temporary.path().join("store");
    let repository = "repository";
    let first_graph = graph('6', "")?;
    let first_plan = ProjectionPlan::from_graph(repository, &first_graph)?;
    let first_ref = SurrealRef::from_plan(
        &first_plan,
        backend.engine(),
        &store_path,
        format!("sha256:{:x}", Sha256::digest(b"first graph")),
    )?;
    let projection = backend.open(&store_path).await?;

    let empty_graph = GraphDocument::empty_v1(BuildMetadata {
        builder_version: "integration".to_owned(),
        schema_fingerprint: digest('1'),
        source_tree_digest: digest('2'),
        configuration_digest: digest('3'),
        generation_id: digest('4'),
        source_commit: None,
    });
    let empty_plan = ProjectionPlan::from_graph("empty-repository", &empty_graph)?;
    let empty_ref = SurrealRef::from_plan(
        &empty_plan,
        backend.engine(),
        &store_path,
        format!("sha256:{:x}", Sha256::digest(b"empty graph")),
    )?;
    projection
        .stage_for_reference(&empty_plan, &empty_ref)
        .await?;
    projection.validate_reference(&empty_ref).await?;

    let unbound = ProjectionPlan::from_graph("unbound-library", &graph('0', "_unbound")?)?;
    projection.stage(&unbound).await?;
    let unbound_ref = SurrealRef::from_plan(&unbound, backend.engine(), &store_path, digest('a'))?;
    assert!(matches!(
        projection.validate_reference(&unbound_ref).await,
        Err(ProjectionError::InvalidReference(_))
    ));

    projection
        .stage_for_reference(&first_plan, &first_ref)
        .await?;
    projection.validate_reference(&first_ref).await?;
    for field in [
        "namespace",
        "database",
        "engine",
        "location",
        "graph_digest",
    ] {
        let mut wrong = first_ref.clone();
        match field {
            "namespace" => wrong.namespace = "other".to_owned(),
            "database" => wrong.database = "other".to_owned(),
            "engine" => {
                wrong.engine = match wrong.engine {
                    SurrealEngine::SurrealKv => SurrealEngine::RocksDb,
                    SurrealEngine::RocksDb => SurrealEngine::SurrealKv,
                };
            }
            "location" => wrong.location = temporary.path().to_string_lossy().into_owned(),
            _ => wrong.graph_digest = digest('f'),
        }
        assert!(matches!(
            projection.validate_reference(&wrong).await,
            Err(ProjectionError::InvalidReference(_))
        ));
    }
    assert_eq!(projection.active_generation(repository).await?, None);
    let mut different_digest = first_ref.clone();
    different_digest.graph_digest = digest('f');
    assert!(matches!(
        projection
            .stage_for_reference(&first_plan, &different_digest)
            .await,
        Err(ProjectionError::InvalidPlan(_))
    ));
    assert_eq!(
        projection.read_generation_projection(&first_ref).await?,
        first_plan
    );

    let search = projection
        .search_at(
            &first_ref,
            SearchRequest {
                query: "CALLEE".to_owned(),
                limits: limits(),
            },
        )
        .await?;
    assert_eq!(search.nodes.len(), 1);
    let callee = search.nodes[0].id.clone();
    let caller = first_graph.nodes[0].id.clone();
    let leaf = first_graph.nodes[2].id.clone();
    assert_eq!(
        projection
            .callers_at(
                &first_ref,
                CallRequest {
                    symbol: callee.clone(),
                    include_heuristic: false,
                    limits: limits(),
                },
            )
            .await?
            .edges
            .len(),
        1
    );
    assert_eq!(
        projection
            .callees_at(
                &first_ref,
                CallRequest {
                    symbol: callee.clone(),
                    include_heuristic: false,
                    limits: limits(),
                },
            )
            .await?
            .edges
            .len(),
        1
    );
    assert!(
        projection
            .impact_at(
                &first_ref,
                ImpactRequest {
                    symbol: leaf.clone(),
                    include_heuristic: false,
                    limits: limits(),
                },
            )
            .await?
            .nodes
            .iter()
            .any(|node| node.id == caller)
    );
    assert_eq!(
        projection
            .node_trail_at(
                &first_ref,
                NodeTrailRequest {
                    source: caller.clone(),
                    target: leaf,
                    include_heuristic: false,
                    limits: limits(),
                },
            )
            .await?
            .paths[0]
            .edge_ids
            .len(),
        2
    );
    assert!(
        projection
            .structural_subgraph_at(
                &first_ref,
                ExploreRequest {
                    symbols: vec![caller, callee],
                    root: String::new(),
                    include_heuristic: false,
                    limits: limits(),
                },
            )
            .await?
            .edges
            .len()
            >= 1
    );

    let bundle = projection.export_bundle(&first_ref).await?;
    bundle.validate()?;
    let restored_path = temporary.path().join("restored");
    let restored = backend.open(&restored_path).await?;
    restored.restore_bundle(&bundle).await?;
    let restored_ref = SurrealRef::from_plan(
        &bundle.plan,
        backend.engine(),
        &restored_path,
        bundle.reference.graph_digest.clone(),
    )?;
    restored.validate_reference(&restored_ref).await?;

    let second_plan = ProjectionPlan::from_graph(repository, &graph('7', "_new")?)?;
    let second_ref = SurrealRef::from_plan(
        &second_plan,
        backend.engine(),
        &store_path,
        format!("sha256:{:x}", Sha256::digest(b"second graph")),
    )?;
    projection
        .stage_for_reference(&second_plan, &second_ref)
        .await?;
    projection.validate_contents(&first_ref).await?;
    projection.validate_contents(&second_ref).await?;
    assert_eq!(first_ref.file_count, 1);
    assert_eq!(
        projection.metadata_at(&first_ref).await?.build,
        first_graph.graph.build
    );
    assert_eq!(
        projection.file_at(&first_ref, "src/lib.rs").await?.as_ref(),
        first_graph.graph.files.first()
    );
    let bundle = projection.export_bundle(&first_ref).await?;
    let mut corrupt = bundle.clone();
    corrupt.plan.files[0].payload_json = "{}".to_owned();
    assert!(corrupt.validate().is_err());
    let mut corrupt = bundle;
    corrupt.plan.metadata_json = "{}".to_owned();
    assert!(corrupt.validate().is_err());
    projection.validate_reference(&first_ref).await?;
    projection.validate_reference(&second_ref).await?;
    assert!(
        projection
            .search_at(
                &first_ref,
                SearchRequest {
                    query: "callee_new".to_owned(),
                    limits: limits(),
                },
            )
            .await?
            .nodes
            .is_empty()
    );
    assert_eq!(
        projection
            .search_at(
                &second_ref,
                SearchRequest {
                    query: "callee_new".to_owned(),
                    limits: limits(),
                },
            )
            .await?
            .nodes
            .len(),
        1
    );

    let interrupted = ProjectionPlan::from_graph(repository, &graph('8', "_interrupted")?)?;
    assert!(matches!(
        projection
            .stage_with_interrupt(&interrupted, Some(InterruptAfter(1)))
            .await,
        Err(ProjectionError::Interrupted { .. })
    ));
    // Retained and foreign-repository manifests sort before the orphans. A
    // one-generation batch must still make progress without touching either.
    let first_gc = projection
        .garbage_collect_generations(repository, std::slice::from_ref(&first_ref), 1)
        .await?;
    assert_eq!(first_gc.generations_deleted, 1);
    projection.validate_contents(&first_ref).await?;
    projection.validate_contents(&empty_ref).await?;
    let gc = projection
        .garbage_collect_generations(repository, std::slice::from_ref(&first_ref), 16)
        .await?;
    assert_eq!(gc.generations_deleted + first_gc.generations_deleted, 2);
    assert_eq!(
        gc.incomplete_generations_deleted + first_gc.incomplete_generations_deleted,
        1
    );
    projection.validate_contents(&empty_ref).await?;
    projection.validate_reference(&first_ref).await?;
    assert!(projection.validate_reference(&second_ref).await.is_err());

    let reference_path = temporary.path().join("surreal.ref");
    fs::write(&reference_path, first_ref.encode()?)?;
    assert_eq!(SurrealRef::decode(&fs::read(reference_path)?)?, first_ref);
    Ok(())
}

#[cfg(feature = "surrealkv")]
#[tokio::test(flavor = "multi_thread")]
async fn surrealkv_persistent_generation_contract_is_complete()
-> Result<(), Box<dyn std::error::Error>> {
    exercise(Backend::SurrealKv).await
}

#[cfg(feature = "rocksdb")]
#[tokio::test(flavor = "multi_thread")]
async fn rocksdb_persistent_generation_contract_is_complete()
-> Result<(), Box<dyn std::error::Error>> {
    exercise(Backend::RocksDb).await
}

#[test]
fn embedded_sessions_survive_replacement_of_publication_runtimes()
-> Result<(), Box<dyn std::error::Error>> {
    let backends = [
        #[cfg(feature = "surrealkv")]
        Backend::SurrealKv,
        #[cfg(feature = "rocksdb")]
        Backend::RocksDb,
    ];
    for backend in backends {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("store");
        let first = ProjectionPlan::from_graph("runtime-replacement", &graph('6', "")?)?;
        let first_ref = SurrealRef::from_plan(&first, backend.engine(), &path, digest('a'))?;
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                // Query-only open must not manufacture an absent datastore.
                assert!(backend.open_existing(&path).await.is_err());
                assert!(!path.exists());
                let writer = backend.open(&path).await?;
                writer.stage_for_reference(&first, &first_ref).await?;
                writer.validate_contents(&first_ref).await
            })?;
        }
        // The first runtime and every caller-owned session are now gone.
        // A watch update and an independently opened query session still share
        // the live physical owner; exact references retain their generations.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let reader = backend.open_existing(&path).await?;
            reader.validate_contents(&first_ref).await?;
            let second = ProjectionPlan::from_graph("runtime-replacement", &graph('7', "_new")?)?;
            let second_ref = SurrealRef::from_plan(&second, backend.engine(), &path, digest('b'))?;
            let writer = backend.open(&path).await?;
            writer.stage_for_reference(&second, &second_ref).await?;
            reader.validate_contents(&first_ref).await?;
            reader.validate_contents(&second_ref).await
        })?;
    }
    Ok(())
}

/// A store path containing `://` must fail closed.
///
/// Embedded addresses are rendered as `scheme://path`, and the SDK splits on the
/// first `://`. A path carrying its own `://` would move that split point and
/// open a different datastore than the caller named, so reject it instead of
/// silently retargeting the store. Windows paths reach this same code with a
/// verbatim `\\?\C:\...` prefix, which must keep working.
#[tokio::test(flavor = "multi_thread")]
async fn embedded_store_paths_reject_embedded_scheme_separators()
-> Result<(), Box<dyn std::error::Error>> {
    let backends = [
        #[cfg(feature = "surrealkv")]
        Backend::SurrealKv,
        #[cfg(feature = "rocksdb")]
        Backend::RocksDb,
    ];
    for backend in backends {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("store://nested");
        let error = backend
            .open(&path)
            .await
            .expect_err("a store path containing \"://\" must be rejected");
        assert!(
            error.to_string().contains("://"),
            "error should name the rejected separator, got: {error}"
        );
    }
    Ok(())
}
