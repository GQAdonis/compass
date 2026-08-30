#![cfg(any(feature = "surreal-surrealkv", feature = "surreal-rocksdb"))]

mod support;

use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use compass_cypher::{CompileLimits, CompileRequest, ParameterTypes, Parameters, compile};
use compass_graph::{GraphSnapshotBuilder, canonical_graph_json};
use compass_graphdb_surreal::{
    ProjectionPlan, SURREAL_DATABASE, SURREAL_NAMESPACE, SURREAL_REF_FILE_NAME, SurrealEngine,
    SurrealProjection, SurrealRef,
};
use compass_model::Graph;
use compass_model::code_graph::GraphDocument as CodeGraphDocument;
use compass_query::{
    GraphEngine, QueryError, QueryLimits, QueryRequest, QueryResult, StoreGraphEngine,
    SurrealCqlRequest, SurrealQueryEngine, SurrealQueryEngineCache, execute,
};
use compass_store::SqliteStore;
use sha2::{Digest, Sha256};

const MATCH_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/clauses/match/Match1.feature");
const RETURN_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/clauses/return/Return1.feature");
const UNION_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/clauses/union/Union1.feature");
const UNWIND_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/clauses/unwind/Unwind1.feature");
const WITH_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/clauses/with/With1.feature");
const AGGREGATION_FEATURE: &str = include_str!(
    "../../../tests/opencypher-tck/features/expressions/aggregation/Aggregation1.feature"
);
const BOOLEAN_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/expressions/boolean/Boolean1.feature");
const LIST_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/expressions/list/List1.feature");
const PATH_FEATURE: &str =
    include_str!("../../../tests/opencypher-tck/features/expressions/path/Path1.feature");
const EXISTS_FEATURE: &str = include_str!(
    "../../../tests/opencypher-tck/features/expressions/existentialSubqueries/ExistentialSubquery1.feature"
);

struct DifferentialFixture {
    _directory: tempfile::TempDir,
    graph_path: std::path::PathBuf,
    graph: Graph,
    sqlite: Graph,
    surreal: SurrealQueryEngine,
    projection: SurrealProjection,
}

impl DifferentialFixture {
    async fn open() -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_extra_nodes(0).await
    }

    async fn with_extra_nodes(extra: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let graph_path = directory.path().join("graph.json");
        support::write_graph(&graph_path)?;
        // Compare the same canonical published artifact consumed by all three
        // engines, not the fixture builder's intentionally unordered input.
        let mut source = CodeGraphDocument::load(&graph_path)?;
        let template = source.nodes.first().cloned().ok_or("fixture has no node")?;
        for index in 0..extra {
            let mut node = template.clone();
            node.id = format!("n:unrelated:{index:06}");
            node.name = format!("unrelated_{index}");
            node.qualified_name = node.name.clone();
            source.nodes.push(node);
        }
        fs::write(&graph_path, canonical_graph_json(&source)?)?;
        let typed = CodeGraphDocument::load(&graph_path)?;
        let plan = ProjectionPlan::from_graph("surreal-cql-differential", &typed)?;
        let store_path = directory.path().join("surreal");
        #[cfg(feature = "surreal-surrealkv")]
        let engine = SurrealEngine::SurrealKv;
        #[cfg(not(feature = "surreal-surrealkv"))]
        let engine = SurrealEngine::RocksDb;
        #[cfg(feature = "surreal-surrealkv")]
        let projection = SurrealProjection::surrealkv(
            store_path
                .to_str()
                .ok_or("Surreal test path is not UTF-8")?,
            SURREAL_NAMESPACE,
            SURREAL_DATABASE,
        )
        .await?;
        #[cfg(not(feature = "surreal-surrealkv"))]
        let projection = SurrealProjection::rocksdb(
            store_path
                .to_str()
                .ok_or("Surreal test path is not UTF-8")?,
            SURREAL_NAMESPACE,
            SURREAL_DATABASE,
        )
        .await?;
        let reference = SurrealRef::from_plan(
            &plan,
            engine,
            &store_path,
            format!("sha256:{:x}", Sha256::digest(fs::read(&graph_path)?)),
        )?;
        projection.stage_for_reference(&plan, &reference).await?;
        fs::write(
            directory.path().join(SURREAL_REF_FILE_NAME),
            reference.encode()?,
        )?;
        let surreal = SurrealQueryEngine::from_projection(reference, projection.clone())
            .await?
            .with_graph_path(&graph_path);
        let store = SqliteStore::open(directory.path().join("store.sqlite3"))?;
        let prepared = GraphSnapshotBuilder::new().prepare(&store, &typed)?;
        GraphSnapshotBuilder::new().activate(&store, &prepared)?;
        let store_engine = StoreGraphEngine::from_store(&store)?;
        let sqlite = Graph::from_document(store_engine.graph().clone().into_legacy_document()?)?;
        let graph = Graph::from_document(typed.into_legacy_document()?)?;
        Ok(Self {
            _directory: directory,
            graph_path,
            graph,
            sqlite,
            surreal,
            projection,
        })
    }

    async fn execute_all(
        &self,
        source: &str,
        parameters: &Parameters,
        limits: QueryLimits,
        cancellation: &AtomicBool,
    ) -> Result<
        (
            Result<QueryResult, QueryError>,
            Result<QueryResult, QueryError>,
            Result<QueryResult, QueryError>,
        ),
        Box<dyn std::error::Error>,
    > {
        let parameter_types = parameters
            .iter()
            .map(|(name, value)| (name.clone(), value.compass_type()))
            .collect::<ParameterTypes>();
        let compiled = compile(CompileRequest {
            source_name: "surreal-differential.cypher",
            source,
            parameter_types: &parameter_types,
            schema: &self.graph.schema_fingerprint(),
            limits: CompileLimits::default(),
        })?;
        let json = execute(QueryRequest {
            compiled: &compiled,
            graph: &self.graph,
            parameters: &parameters,
            limits,
            cancellation,
        });
        let sqlite = execute(QueryRequest {
            compiled: &compiled,
            graph: &self.sqlite,
            parameters,
            limits,
            cancellation,
        });
        let surreal = self
            .surreal
            .execute_cql(SurrealCqlRequest {
                compiled: &compiled,
                parameters: &parameters,
                limits,
                cancellation,
            })
            .await;
        Ok((json, sqlite, surreal))
    }
}

fn limits() -> QueryLimits {
    QueryLimits {
        deadline: Instant::now() + Duration::from_secs(5),
        max_rows: 1_000,
        max_path_depth: 32,
        max_expanded_relationships: 100_000,
        max_memory_bytes: 32 * 1024 * 1024,
    }
}

fn assert_same_result(json: QueryResult, surreal: QueryResult) {
    assert_eq!(surreal.columns, json.columns);
    assert_eq!(surreal.rows, json.rows);
    assert_eq!(surreal.explain.is_some(), json.explain.is_some());
    if let (Some(json), Some(surreal)) = (&json.explain, &surreal.explain) {
        assert_eq!(surreal.schema_fingerprint, json.schema_fingerprint);
        assert_eq!(surreal.operators, json.operators);
        assert_eq!(surreal.optimizations, json.optimizations);
    }
    match (json.profile, surreal.profile) {
        (Some(json), Some(surreal)) => {
            assert_eq!(surreal.candidate_nodes, json.candidate_nodes);
            assert_eq!(surreal.expanded_relationships, json.expanded_relationships);
            assert_eq!(surreal.peak_memory_bytes, json.peak_memory_bytes);
            assert_eq!(surreal.cancellation_checks, json.cancellation_checks);
            assert_eq!(surreal.operators.len(), json.operators.len());
            for (json, surreal) in json.operators.iter().zip(&surreal.operators) {
                assert_eq!(surreal.name, json.name);
                assert_eq!(surreal.input_rows, json.input_rows);
                assert_eq!(surreal.output_rows, json.output_rows);
                assert_eq!(surreal.candidate_nodes, json.candidate_nodes);
                assert_eq!(surreal.expanded_relationships, json.expanded_relationships);
                assert_eq!(surreal.peak_memory_bytes, json.peak_memory_bytes);
                assert_eq!(surreal.cancellation_checks, json.cancellation_checks);
            }
        }
        (None, None) => {}
        (json, surreal) => assert_eq!(surreal.is_some(), json.is_some()),
    }
}

fn scenario_query(feature: &str, id: usize) -> Result<&str, Box<dyn std::error::Error>> {
    let marker = format!("Scenario: [{id}]");
    let scenario = feature
        .split_once(&marker)
        .ok_or_else(|| format!("scenario {id} is absent"))?
        .1;
    let body = scenario
        .split_once("When executing query:")
        .ok_or_else(|| format!("scenario {id} has no query"))?
        .1;
    let quoted = body
        .split_once("\"\"\"")
        .ok_or_else(|| format!("scenario {id} has no opening query delimiter"))?
        .1;
    Ok(quoted
        .split_once("\"\"\"")
        .ok_or_else(|| format!("scenario {id} has no closing query delimiter"))?
        .0
        .trim())
}

fn tck_parameters(feature: &str, id: usize) -> Parameters {
    let mut parameters = Parameters::new();
    if feature == LIST_FEATURE {
        if matches!(id, 3 | 5) {
            parameters.insert(
                "expr".to_owned(),
                compass_cypher::CompassValue::List(
                    vec![compass_cypher::CompassValue::String("Apa".into())].into(),
                ),
            );
        }
        if matches!(id, 3 | 4 | 5) {
            parameters.insert("idx".to_owned(), compass_cypher::CompassValue::Integer(0));
        }
    }
    parameters
}

#[tokio::test(flavor = "multi_thread")]
async fn supported_compassql_operators_are_differentially_identical()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DifferentialFixture::open().await?;
    let queries = [
        "MATCH (a:Function)-[r:CALLS]->(b:Function) RETURN a.id AS caller, b.id AS callee ORDER BY caller, callee",
        "MATCH (n:Function) WHERE n.name STARTS WITH 'c' OPTIONAL MATCH (n)-[:CALLS]->(m) RETURN n.id AS id, coalesce(m.id, 'none') AS next ORDER BY id, next",
        "MATCH (n) WHERE EXISTS { MATCH (n)-[:CALLS]->(m) } RETURN count(DISTINCT n) AS callers",
        "MATCH p=(a {id:'n:caller'})-[:CALLS*1..3]->(b) RETURN b.id AS id, length(p) AS hops ORDER BY hops, id",
        "MATCH p=shortestPath((a {id:'n:caller'})-[:CALLS*1..3]->(b {id:'n:callee'})) RETURN length(p) AS hops, size(nodes(p)) AS nodes",
        "UNWIND [1,2,2] AS value RETURN count(DISTINCT value) AS count, sum(value) AS total, avg(value) AS average",
        "RETURN CASE WHEN 2 IN [1,2] THEN 'yes' ELSE 'no' END AS verdict, [true,null][1] AS unknown UNION ALL RETURN 'second' AS verdict, null AS unknown",
        "PROFILE MATCH (n) RETURN labels(n) AS labels, properties(n).source_file AS file ORDER BY file",
        "MATCH (n:Function) RETURN n ORDER BY n",
        "MATCH (a:Function)-[r:CALLS]->(b:Function) RETURN r ORDER BY r",
        "MATCH (n) WITH n MATCH (m:Function) RETURN n.id AS node, m.id AS function ORDER BY node, function",
        "MATCH (n:Function) RETURN id(n) AS local, n.id AS portable ORDER BY portable",
        "MATCH (n:function) RETURN n.id AS id",
        "MATCH (a)-[:calls]->(b) RETURN a.id AS source, b.id AS target",
        "MATCH (a)-[:CALLS]->(a) RETURN a.id AS id",
        "MATCH (a)<-[r:CALLS]-(b) RETURN id(r) AS relation, a.id AS target ORDER BY relation",
        "MATCH (a)-[r:CALLS]-(b) RETURN id(r) AS relation, a.id AS source ORDER BY relation, source",
        "MATCH (n) RETURN n.id AS id ORDER BY id SKIP 1 LIMIT 3",
        "RETURN all(x IN [true,null] WHERE x) AS every, any(x IN [false,true] WHERE x) AS some, none(x IN [] WHERE x) AS none",
        "RETURN [1,2,3][1] AS item, [1,2,3][1..3] AS tail, coalesce(null,'yes') AS value",
        "RETURN [null] = [1] AS unknown, 5 / 2 AS quotient",
        "RETURN all(x IN [true,null] WHERE x) AS every, any(x IN [false,null] WHERE x) AS some, none(x IN [false,null] WHERE x) AS none_value, single(x IN [true,null] WHERE x) AS one, toUpper(trim(' compass ')) AS name, 'compass' =~ 'comp.*' AS regex",
        "RETURN 1 AS value UNION RETURN 1 AS value UNION ALL RETURN 2 AS value",
        "MATCH p=(a {id:'n:caller'})-[r:CALLS]->(b) RETURN id(a) AS id, labels(a) AS labels, type(r) AS relation, length(p) AS hops, size(nodes(p)) AS nodes, properties(a).source_file AS file",
    ];
    let cancellation = AtomicBool::new(false);
    for source in queries {
        let parameters = Parameters::new();
        let (json, sqlite, surreal) = fixture
            .execute_all(source, &parameters, limits(), &cancellation)
            .await?;
        let json = json?;
        assert_same_result(json.clone(), sqlite?);
        assert_same_result(json, surreal?);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn semantic_limits_and_errors_are_identical_without_charging_storage_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DifferentialFixture::open().await?;
    fs::remove_file(&fixture.graph_path)?;
    let cancellation = AtomicBool::new(false);
    let queries = [
        "MATCH (n {id:'n:caller'}) RETURN n.id AS id",
        "MATCH p=(a)-[:CALLS*1..2]->(b) RETURN a.id AS source, b.id AS target ORDER BY source, target",
        "PROFILE MATCH (n) RETURN n.id AS id ORDER BY id",
        "RETURN 1 / 0 AS value",
    ];
    for memory in [1, 1_200, 4_096, 32 * 1024 * 1024] {
        for source in queries {
            for max_rows in [1, 1_000] {
                let bounded = QueryLimits {
                    max_memory_bytes: memory,
                    max_rows,
                    ..limits()
                };
                let (json, sqlite, surreal) = fixture
                    .execute_all(source, &Parameters::new(), bounded, &cancellation)
                    .await?;
                for actual in [sqlite, surreal] {
                    match (&json, actual) {
                        (Ok(expected), Ok(actual)) => assert_same_result(expected.clone(), actual),
                        (Err(expected), Err(actual)) => {
                            assert_eq!(actual.kind(), expected.kind(), "{source}; memory={memory}");
                            assert_eq!(actual.code(), expected.code(), "{source}; memory={memory}");
                            assert_eq!(actual.message(), expected.message(), "{source}; memory={memory}");
                        }
                        (expected, actual) => return Err(format!("backend limit mismatch for {source}; memory={memory}; rows={max_rows}: expected {expected:?}, got {actual:?}").into()),
                    }
                }
            }
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn selective_cql_uses_indexed_rows_without_materializing_the_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DifferentialFixture::with_extra_nodes(1_500).await?;
    assert_eq!(
        fixture.surreal.schema_fingerprint().await?,
        fixture.graph.schema_fingerprint()
    );
    fs::remove_file(&fixture.graph_path)?;
    let mut bounded = limits();
    bounded.max_rows = 1;
    bounded.max_memory_bytes = 64 * 1024;
    let (json, sqlite, surreal) = fixture
        .execute_all(
            "PROFILE MATCH (n {id:'n:callee'}) RETURN id(n) AS local, n.name AS name",
            &Parameters::new(),
            bounded,
            &AtomicBool::new(false),
        )
        .await?;
    let json = json?;
    assert_same_result(json.clone(), sqlite?);
    assert_same_result(json, surreal?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn exact_surreal_execution_survives_json_removal_and_matches_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DifferentialFixture::open().await?;
    fs::remove_file(&fixture.graph_path)?;
    let mut tampered = fixture.surreal.reference().clone();
    tampered.graph_digest = format!("sha256:{}", "f".repeat(64));
    assert!(SurrealQueryEngine::open_reference(tampered).await.is_err());
    let cancellation = AtomicBool::new(false);
    let (_, _, surreal) = fixture
        .execute_all(
            "MATCH (a:Function)-[:CALLS]->(b:Function) RETURN a.id AS caller, b.id AS callee ORDER BY caller, callee",
            &Parameters::new(),
            limits(),
            &cancellation,
        )
        .await?;
    assert!(!surreal?.rows.is_empty());

    let cancellation = AtomicBool::new(true);
    let (json, sqlite, surreal) = fixture
        .execute_all(
            "MATCH (n) RETURN n.id",
            &Parameters::new(),
            limits(),
            &cancellation,
        )
        .await?;
    let json = json.expect_err("JSON cancellation must fail");
    let sqlite = sqlite.expect_err("SQLite cancellation must fail");
    let surreal = surreal.expect_err("Surreal cancellation must fail");
    assert_eq!(sqlite.kind(), json.kind());
    assert_eq!(sqlite.code(), json.code());
    assert_eq!(sqlite.message(), json.message());
    assert_eq!(surreal.kind(), json.kind());
    assert_eq!(surreal.code(), json.code());
    assert_eq!(surreal.message(), json.message());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn cached_readers_share_one_connection_and_pin_distinct_generations()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DifferentialFixture::open().await?;
    let cache = SurrealQueryEngineCache::from_engine(fixture.surreal.clone())?;
    let (first, simultaneous) = tokio::join!(
        cache.get(&fixture.graph_path),
        cache.get(&fixture.graph_path)
    );
    let first = first?;
    assert!(Arc::ptr_eq(&first, &simultaneous?));

    let mut next_graph = CodeGraphDocument::load(&fixture.graph_path)?;
    next_graph.graph.build.generation_id = format!("sha256:{}", "9".repeat(64));
    let node = next_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == "n:callee")
        .ok_or("fixture callee is missing")?;
    node.name = "renamed_callee".to_owned();
    node.qualified_name = "renamed_callee".to_owned();
    let bytes = canonical_graph_json(&next_graph)?;
    let plan = ProjectionPlan::from_graph(first.reference().repository_id.clone(), &next_graph)?;
    let reference = SurrealRef::from_plan(
        &plan,
        first.reference().engine,
        std::path::Path::new(&first.reference().location),
        format!("sha256:{:x}", Sha256::digest(&bytes)),
    )?;
    fixture
        .projection
        .stage_for_reference(&plan, &reference)
        .await?;
    fs::write(&fixture.graph_path, bytes)?;
    fs::write(
        fixture.graph_path.with_file_name(SURREAL_REF_FILE_NAME),
        reference.encode()?,
    )?;
    let next = cache.get(&fixture.graph_path).await?;
    assert_ne!(
        first.reference().generation_id,
        next.reference().generation_id
    );
    assert!(Arc::ptr_eq(&next, &cache.get(&fixture.graph_path).await?));

    let request = || compass_model::query_contract::SearchRequest {
        query: "renamed_callee".to_owned(),
        limits: compass_model::query_contract::CodeQueryLimits::default(),
    };
    fs::remove_file(&fixture.graph_path)?;
    assert!(first.search(request()).await?.nodes.is_empty());
    assert_eq!(next.search(request()).await?.nodes.len(), 1);
    assert!(Arc::ptr_eq(&next, &cache.get(&fixture.graph_path).await?));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn every_selected_opencypher_tck_scenario_is_differentially_identical()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DifferentialFixture::open().await?;
    let selected = [
        (MATCH_FEATURE, &[1, 4, 5][..]),
        (RETURN_FEATURE, &[1, 2][..]),
        (UNION_FEATURE, &[1, 2, 3, 5][..]),
        (UNWIND_FEATURE, &[1, 3, 7, 8, 9, 10, 11, 13][..]),
        (WITH_FEATURE, &[5][..]),
        (AGGREGATION_FEATURE, &[1][..]),
        (BOOLEAN_FEATURE, &[1, 2, 3, 4, 5][..]),
        (LIST_FEATURE, &[1, 2, 3, 4, 5][..]),
        (PATH_FEATURE, &[1][..]),
        (EXISTS_FEATURE, &[1, 2, 3, 4][..]),
    ];
    let cancellation = AtomicBool::new(false);
    for (feature, ids) in selected {
        for id in ids {
            let source = scenario_query(feature, *id)?;
            let parameters = tck_parameters(feature, *id);
            match fixture
                .execute_all(source, &parameters, limits(), &cancellation)
                .await
            {
                Ok((Ok(json), Ok(sqlite), Ok(surreal))) => {
                    assert_same_result(json.clone(), sqlite);
                    assert_same_result(json, surreal);
                }
                Ok((Err(json), Err(sqlite), Err(surreal))) => {
                    assert_eq!(sqlite.kind(), json.kind(), "scenario {id}");
                    assert_eq!(sqlite.code(), json.code(), "scenario {id}");
                    assert_eq!(sqlite.message(), json.message(), "scenario {id}");
                    assert_eq!(surreal.kind(), json.kind(), "scenario {id}");
                    assert_eq!(surreal.code(), json.code(), "scenario {id}");
                    assert_eq!(surreal.message(), json.message(), "scenario {id}");
                }
                Ok((json, sqlite, surreal)) => {
                    return Err(format!(
                        "scenario {id} differed between JSON ({json:?}), SQLite ({sqlite:?}), and Surreal ({surreal:?})"
                    )
                    .into());
                }
                Err(error) => {
                    // Compile-time errors are backend-independent because both
                    // executors consume the same compiled logical plan.
                    let parameter_types = parameters
                        .iter()
                        .map(|(name, value)| (name.clone(), value.compass_type()))
                        .collect::<ParameterTypes>();
                    assert!(
                        compile(CompileRequest {
                            source_name: "selected-openCypher-TCK.feature",
                            source,
                            parameter_types: &parameter_types,
                            schema: &fixture.graph.schema_fingerprint(),
                            limits: CompileLimits::default(),
                        })
                        .is_err(),
                        "unexpected pre-execution error for scenario {id}: {error}"
                    );
                }
            }
        }
    }
    Ok(())
}
