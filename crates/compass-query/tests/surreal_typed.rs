#![cfg(any(feature = "surreal-surrealkv", feature = "surreal-rocksdb"))]

mod support;

use std::fs;
use std::path::PathBuf;

use compass_graph::{GraphSnapshotBuilder, canonical_graph_json};
use compass_graphdb_surreal::{
    ProjectionPlan, SURREAL_DATABASE, SURREAL_NAMESPACE, SurrealEngine, SurrealProjection,
    SurrealRef,
};
use compass_model::code_graph::GraphDocument;
use compass_model::query_contract::{
    CallRequest, CodeQueryLimits, CodeQueryResponse, ExploreRequest, ImpactRequest,
    NodeTrailRequest, SearchRequest,
};
use compass_query::{
    CodeQueryEngine, EngineSelection, NaturalQueryRequest, QueryError, SurrealQueryEngine,
    open_with_engine,
};
use compass_store::{STORE_FILE_NAME, STORE_REF_FILE_NAME, SqliteStore};
use sha2::{Digest, Sha256};

fn engines() -> Vec<SurrealEngine> {
    vec![
        #[cfg(feature = "surreal-surrealkv")]
        SurrealEngine::SurrealKv,
        #[cfg(feature = "surreal-rocksdb")]
        SurrealEngine::RocksDb,
    ]
}

struct Fixture {
    directory: tempfile::TempDir,
    graph_path: PathBuf,
    json: CodeQueryEngine,
    sqlite: CodeQueryEngine,
    surreal: SurrealQueryEngine,
}

impl Fixture {
    async fn open(engine: SurrealEngine) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let graph_path = directory.path().join("graph.json");
        support::write_graph(&graph_path)?;
        let document = GraphDocument::load(&graph_path)?;
        let bytes = canonical_graph_json(&document)?;
        fs::write(&graph_path, &bytes)?;
        let document = GraphDocument::load(&graph_path)?;
        let store = SqliteStore::open(directory.path().join(STORE_FILE_NAME))?;
        let prepared = GraphSnapshotBuilder::new().prepare(&store, &document)?;
        GraphSnapshotBuilder::new().activate(&store, &prepared)?;
        fs::write(
            directory.path().join(STORE_REF_FILE_NAME),
            serde_json::to_vec(&store.snapshot_reference()?)?,
        )?;
        store.checkpoint()?;
        let json = open_with_engine(
            &graph_path,
            None,
            &directory.path().join("json-cache"),
            EngineSelection::Json,
        )?;
        let sqlite = open_with_engine(
            &graph_path,
            None,
            &directory.path().join("store-cache"),
            EngineSelection::Store,
        )?;
        let path = directory.path().join("surreal");
        let location = path.to_str().ok_or("non-UTF8 temporary path")?;
        let projection = match engine {
            #[cfg(feature = "surreal-surrealkv")]
            SurrealEngine::SurrealKv => {
                SurrealProjection::surrealkv(location, SURREAL_NAMESPACE, SURREAL_DATABASE).await?
            }
            #[cfg(feature = "surreal-rocksdb")]
            SurrealEngine::RocksDb => {
                SurrealProjection::rocksdb(location, SURREAL_NAMESPACE, SURREAL_DATABASE).await?
            }
            #[allow(unreachable_patterns)]
            _ => return Err("engine not enabled".into()),
        };
        let plan = ProjectionPlan::from_graph("typed-differential", &document)?;
        let reference = SurrealRef::from_plan(
            &plan,
            engine,
            &path,
            format!("sha256:{:x}", Sha256::digest(bytes)),
        )?;
        projection.stage_for_reference(&plan, &reference).await?;
        let surreal = SurrealQueryEngine::from_projection(reference, projection)
            .await?
            .with_graph_path(&graph_path);
        // Neither persisted query engine may rely on portable JSON after open.
        fs::remove_file(&graph_path)?;
        Ok(Self {
            directory,
            graph_path,
            json,
            sqlite,
            surreal,
        })
    }
}

fn equal(
    json: Result<CodeQueryResponse, QueryError>,
    sqlite: Result<CodeQueryResponse, QueryError>,
    surreal: Result<CodeQueryResponse, QueryError>,
    context: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match (json, sqlite, surreal) {
        (Ok(json), Ok(sqlite), Ok(surreal)) => {
            assert_eq!(
                serde_json::to_value(&sqlite)?,
                serde_json::to_value(&json)?,
                "SQLite: {context}"
            );
            assert_eq!(
                serde_json::to_value(&surreal)?,
                serde_json::to_value(&json)?,
                "Surreal: {context}"
            );
        }
        (Err(json), Err(sqlite), Err(surreal)) => {
            assert_eq!(
                (sqlite.kind(), sqlite.code(), sqlite.message()),
                (json.kind(), json.code(), json.message()),
                "SQLite: {context}"
            );
            assert_eq!(
                (surreal.kind(), surreal.code(), surreal.message()),
                (json.kind(), json.code(), json.message()),
                "Surreal: {context}"
            );
        }
        (json, sqlite, surreal) => {
            return Err(format!(
                "{context}: JSON {json:?}, SQLite {sqlite:?}, Surreal {surreal:?}"
            )
            .into());
        }
    }
    Ok(())
}

#[tokio::test]
async fn every_typed_operation_preserves_values_ranking_limits_and_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    for engine in engines() {
        let fixture = Fixture::open(engine).await?;
        assert!(!fixture.graph_path.exists());
        for limits in [
            CodeQueryLimits::default(),
            CodeQueryLimits {
                max_candidates: 2,
                max_nodes: 2,
                max_edges: 1,
                ..CodeQueryLimits::default()
            },
        ] {
            for query in [
                "UserService.list",
                "list",
                "fetchUsers",
                "cafe",
                "resume",
                "cache key",
                "payment gateway",
                "unknown_symbol",
                "",
                "\"list\" OR * -",
            ] {
                let request = SearchRequest {
                    query: query.to_owned(),
                    limits: limits.clone(),
                };
                equal(
                    fixture.json.search(request.clone()),
                    fixture.sqlite.search(request.clone()),
                    fixture.surreal.search(request).await,
                    query,
                )?;
            }
            for include_heuristic in [false, true] {
                for symbol in ["n:caller", "Store.callee", "list", "missing"] {
                    let request = CallRequest {
                        symbol: symbol.to_owned(),
                        include_heuristic,
                        limits: limits.clone(),
                    };
                    equal(
                        fixture.json.callers(request.clone()),
                        fixture.sqlite.callers(request.clone()),
                        fixture.surreal.callers(request.clone()).await,
                        "callers",
                    )?;
                    equal(
                        fixture.json.callees(request.clone()),
                        fixture.sqlite.callees(request.clone()),
                        fixture.surreal.callees(request).await,
                        "callees",
                    )?;
                    let request = ImpactRequest {
                        symbol: symbol.to_owned(),
                        include_heuristic,
                        limits: limits.clone(),
                    };
                    equal(
                        fixture.json.impact(request.clone()),
                        fixture.sqlite.impact(request.clone()),
                        fixture.surreal.impact(request).await,
                        "impact",
                    )?;
                }
                let request = NodeTrailRequest {
                    source: "Api.caller".to_owned(),
                    target: "Store.callee".to_owned(),
                    include_heuristic,
                    limits: limits.clone(),
                };
                equal(
                    fixture.json.node_trail(request.clone()),
                    fixture.sqlite.node_trail(request.clone()),
                    fixture.surreal.node_trail(request).await,
                    "node trail",
                )?;
                let request = NaturalQueryRequest {
                    question: "who calls callee?".to_owned(),
                    include_heuristic,
                    limits: limits.clone(),
                };
                equal(
                    fixture.json.query_natural(request.clone()),
                    fixture.sqlite.query_natural(request.clone()),
                    fixture.surreal.query_natural(request).await,
                    "ask",
                )?;
            }
        }
        let invalid = SearchRequest {
            query: "caller".to_owned(),
            limits: CodeQueryLimits {
                max_candidates: 0,
                ..CodeQueryLimits::default()
            },
        };
        equal(
            fixture.json.search(invalid.clone()),
            fixture.sqlite.search(invalid.clone()),
            fixture.surreal.search(invalid).await,
            "invalid limits",
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn explore_verifies_indexed_source_digests_and_reports_stale_sources()
-> Result<(), Box<dyn std::error::Error>> {
    for engine in engines() {
        let fixture = Fixture::open(engine).await?;
        let request = ExploreRequest {
            symbols: vec!["Api.caller".to_owned(), "Store.callee".to_owned()],
            root: fixture.directory.path().to_string_lossy().into_owned(),
            include_heuristic: false,
            limits: CodeQueryLimits::default(),
        };
        let response = fixture.surreal.explore(request.clone()).await?;
        assert_eq!(response.files[0].source.as_deref(), Some("code"));
        equal(
            fixture.json.explore(request.clone()),
            fixture.sqlite.explore(request.clone()),
            Ok(response),
            "fresh source",
        )?;
        fs::write(fixture.directory.path().join("src/lib.rs"), "changed")?;
        equal(
            fixture.json.explore(request.clone()),
            fixture.sqlite.explore(request.clone()),
            fixture.surreal.explore(request).await,
            "stale source",
        )?;
    }
    Ok(())
}
