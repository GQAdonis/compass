#![cfg(any(feature = "surreal-surrealkv", feature = "surreal-rocksdb"))]

use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Output, Stdio};

use compass_files::{BuildGuard, ProjectConfig, ProjectStore, ProjectSurrealEngine};
use compass_graphdb_surreal::{SURREAL_REF_FILE_NAME, SurrealEngine, SurrealRef};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn run(root: &std::path::Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_compass"))
        .args(args)
        .current_dir(root)
        .env_remove("COMPASS_OUT")
        .output()?)
}

fn require_success(output: &Output, operation: &str) -> Result<(), Box<dyn Error>> {
    if !output.status.success() {
        return Err(format!(
            "{operation} failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn graph_digest(path: &std::path::Path) -> Result<String, Box<dyn Error>> {
    Ok(format!("sha256:{:x}", Sha256::digest(fs::read(path)?)))
}

#[cfg(feature = "surreal-surrealkv")]
#[test]
fn surreal_cli_is_wired_through_publication_queries_compassql_and_operations()
-> Result<(), Box<dyn Error>> {
    let container = tempfile::tempdir()?;
    let root = tempfile::tempdir_in(container.path())?;
    fs::write(
        root.path().join("lib.rs"),
        "pub fn caller() { callee(); }\npub fn callee() {}\n",
    )?;
    let init = run(
        root.path(),
        &[
            "init",
            ".",
            "--yes",
            "--store",
            "surreal",
            "--surreal-engine",
            "surrealkv",
            "--surreal-path",
            "compass-out/custom-surreal",
        ],
    )?;
    require_success(&init, "surreal init")?;

    let config = ProjectConfig::load(root.path())?.ok_or("missing project configuration")?;
    assert_eq!(config.version, 2);
    assert_eq!(config.storage.store, Some(ProjectStore::Surreal));
    assert_eq!(
        config.storage.surreal_engine,
        Some(ProjectSurrealEngine::SurrealKv)
    );
    assert_eq!(
        config.storage.surreal_path.as_deref(),
        Some(std::path::Path::new("compass-out/custom-surreal"))
    );

    let output = root.path().join("compass-out");
    let active = BuildGuard::resolve_current_snapshot_directory(&output)?;
    let graph_path = active.join("graph.json");
    let reference = SurrealRef::decode(&fs::read(active.join(SURREAL_REF_FILE_NAME))?)?;
    assert_eq!(reference.engine, SurrealEngine::SurrealKv);
    assert_eq!(reference.graph_digest, graph_digest(&graph_path)?);
    assert!(std::path::Path::new(&reference.location).is_dir());

    let unsafe_location = run(
        root.path(),
        &[
            "update",
            ".",
            "--store",
            "surreal",
            "--surreal-path",
            "compass-out/snapshots/unsafe-store",
            "--no-cluster",
            "--no-viz",
        ],
    )?;
    assert!(!unsafe_location.status.success());
    assert!(
        String::from_utf8_lossy(&unsafe_location.stderr).contains("outside immutable snapshots")
    );
    assert_eq!(
        BuildGuard::resolve_current_snapshot_directory(&output)?,
        active,
        "a rejected embedded-store location must preserve the active snapshot"
    );

    let typed_commands: &[&[&str]] = &[
        &[
            "ask",
            "who calls callee?",
            "--engine",
            "surreal",
            "--format",
            "json",
        ],
        &[
            "search", "callee", "--engine", "surreal", "--format", "json",
        ],
        &[
            "callers", "callee", "--engine", "surreal", "--format", "json",
        ],
        &[
            "callees", "caller", "--engine", "surreal", "--format", "json",
        ],
        &[
            "impact", "callee", "--engine", "surreal", "--format", "json",
        ],
        &[
            "explore", "caller", "callee", "--engine", "surreal", "--format", "json",
        ],
        &[
            "node", "caller", "callee", "--engine", "surreal", "--format", "json",
        ],
    ];
    for command in typed_commands {
        let result = run(root.path(), command)?;
        require_success(&result, command[0])?;
        let value: Value = serde_json::from_slice(&result.stdout)?;
        assert_eq!(value["schema"], "compass.query/1", "command: {command:?}");
    }

    let default_search = run(root.path(), &["search", "callee", "--format", "json"])?;
    require_success(&default_search, "default Surreal selection")?;
    let default_value: Value = serde_json::from_slice(&default_search.stdout)?;
    assert_eq!(default_value["schema"], "compass.query/1");

    let cql = run(
        root.path(),
        &[
            "query",
            "--cql",
            "PROFILE MATCH (n:Function) RETURN n.name AS name ORDER BY name",
            "--engine",
            "surreal",
            "--format",
            "json",
            "--graph",
            graph_path.to_str().ok_or("graph path is not UTF-8")?,
        ],
    )?;
    require_success(&cql, "native Surreal CompassQL")?;
    let cql_value: Value = serde_json::from_slice(&cql.stdout)?;
    assert_eq!(cql_value["schema"], "compass.cql.result/1");
    assert!(
        cql_value["rows"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
    );
    assert!(cql_value["profile"].is_object());

    let portable = container.path().join("portable-graph.json");
    fs::rename(&graph_path, &portable)?;
    for command in typed_commands {
        require_success(
            &run(root.path(), command)?,
            "typed query without canonical JSON",
        )?;
    }
    require_success(
        &run(
            root.path(),
            &[
                "query",
                "--cql",
                "MATCH (n:Function) RETURN n.name AS name",
                "--engine",
                "surreal",
                "--format",
                "json",
            ],
        )?,
        "CompassQL without canonical JSON",
    )?;
    require_success(
        &run(root.path(), &["search", "callee", "--format", "json"])?,
        "default query without canonical JSON",
    )?;
    fs::rename(&portable, &graph_path)?;

    let capabilities = run(root.path(), &["capabilities", "--format", "json"])?;
    require_success(&capabilities, "Surreal capabilities")?;
    let capabilities: Value = serde_json::from_slice(&capabilities.stdout)?;
    assert_eq!(capabilities["features"]["surreal_store"], true);
    assert_eq!(capabilities["features"]["surreal_cql"], true);
    assert_eq!(capabilities["engines"]["surrealkv"], true);
    assert_eq!(
        capabilities["contracts"]["surreal_reference"],
        "compass.surreal.ref/1"
    );
    assert_eq!(
        capabilities["contracts"]["surreal_projection"],
        "compass.graph.surreal/2"
    );

    for operation in ["status", "validate"] {
        let result = run(
            root.path(),
            &[
                "store",
                operation,
                output.to_str().ok_or("output path is not UTF-8")?,
                "--engine",
                "surreal",
                "--format",
                "json",
            ],
        )?;
        require_success(&result, operation)?;
        let value: Value = serde_json::from_slice(&result.stdout)?;
        if operation == "status" {
            assert_eq!(value["surrealStore"]["valid"], true);
        } else {
            assert_eq!(value["valid"], true);
        }
    }

    let backup = container.path().join("surreal-backup");
    let backup_result = run(
        root.path(),
        &[
            "store",
            "backup",
            output.to_str().ok_or("output path is not UTF-8")?,
            "--engine",
            "surreal",
            "--output",
            backup.to_str().ok_or("backup path is not UTF-8")?,
            "--format",
            "json",
        ],
    )?;
    require_success(&backup_result, "Surreal backup")?;
    assert!(backup.join("surreal.bundle.json").is_file());
    assert!(!backup.join("surreal").exists());

    let restored = container.path().join("restored");
    let restore_result = run(
        root.path(),
        &[
            "store",
            "restore",
            "--from",
            backup.to_str().ok_or("backup path is not UTF-8")?,
            "--into",
            restored.to_str().ok_or("restore path is not UTF-8")?,
            "--format",
            "json",
        ],
    )?;
    require_success(&restore_result, "Surreal restore")?;
    let restored_validation = run(
        root.path(),
        &[
            "store",
            "validate",
            restored.to_str().ok_or("restore path is not UTF-8")?,
            "--engine",
            "surreal",
            "--format",
            "json",
        ],
    )?;
    require_success(&restored_validation, "restored Surreal validation")?;
    let restored_ref = SurrealRef::decode(&fs::read(restored.join(SURREAL_REF_FILE_NAME))?)?;
    assert_eq!(restored_ref.graph_digest, reference.graph_digest);
    assert_eq!(restored_ref.generation_id, reference.generation_id);
    assert_ne!(restored_ref.location, reference.location);
    let restored_graph = restored.join("graph.json");
    fs::remove_file(&restored_graph)?;
    require_success(
        &run(
            root.path(),
            &[
                "search",
                "callee",
                "--graph",
                restored_graph
                    .to_str()
                    .ok_or("restored graph path is not UTF-8")?,
                "--engine",
                "surreal",
                "--format",
                "json",
            ],
        )?,
        "restored generation query without canonical JSON",
    )?;

    let original_digest = graph_digest(&graph_path)?;
    fs::remove_dir_all(&reference.location)?;
    let missing_store_query = run(
        root.path(),
        &[
            "search", "callee", "--engine", "surreal", "--format", "json",
        ],
    )?;
    assert!(!missing_store_query.status.success());
    assert!(
        !std::path::Path::new(&reference.location).exists(),
        "a query must not recreate missing embedded storage"
    );
    // Keep init's default clustering/viewer profile: changing either flag is
    // a different build, not an unchanged projection repair.
    let repair = run(root.path(), &["update", "."])?;
    require_success(&repair, "unchanged Surreal repair")?;
    assert!(String::from_utf8_lossy(&repair.stdout).contains("(0 extracted,"));
    let repaired = BuildGuard::resolve_current_snapshot_directory(&output)?;
    assert_eq!(graph_digest(&repaired.join("graph.json"))?, original_digest);
    let repaired_ref = SurrealRef::decode(&fs::read(repaired.join(SURREAL_REF_FILE_NAME))?)?;
    assert_eq!(repaired_ref.graph_digest, original_digest);

    let mut stale_ref = repaired_ref;
    stale_ref.graph_digest = format!("sha256:{}", "0".repeat(64));
    fs::write(repaired.join(SURREAL_REF_FILE_NAME), stale_ref.encode()?)?;
    for engine in ["default", "surreal"] {
        let rejected = run(
            root.path(),
            &["search", "callee", "--engine", engine, "--format", "json"],
        )?;
        assert!(
            !rejected.status.success(),
            "changed graph digest must fail at engine load"
        );
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("Surreal"));
    }
    let stale_repair = run(root.path(), &["update", "."])?;
    require_success(&stale_repair, "stale Surreal reference repair")?;
    assert!(String::from_utf8_lossy(&stale_repair.stdout).contains("(0 extracted,"));
    let repaired = BuildGuard::resolve_current_snapshot_directory(&output)?;
    let repaired_ref = SurrealRef::decode(&fs::read(repaired.join(SURREAL_REF_FILE_NAME))?)?;
    assert_eq!(repaired_ref.graph_digest, original_digest);

    let explicit_json = run(
        root.path(),
        &["update", ".", "--store", "json", "--no-cluster", "--no-viz"],
    )?;
    require_success(&explicit_json, "explicit storage override")?;
    let json_active = BuildGuard::resolve_current_snapshot_directory(&output)?;
    assert!(!json_active.join(SURREAL_REF_FILE_NAME).exists());
    let configured_surreal = run(root.path(), &["update", ".", "--no-cluster", "--no-viz"])?;
    require_success(&configured_surreal, "configured Surreal precedence")?;
    let repaired = BuildGuard::resolve_current_snapshot_directory(&output)?;
    assert!(repaired.join(SURREAL_REF_FILE_NAME).is_file());

    fs::write(repaired.join(SURREAL_REF_FILE_NAME), b"{\"corrupt\":true}")?;
    for args in [
        vec!["search", "callee", "--format", "json"],
        vec![
            "search", "callee", "--engine", "surreal", "--format", "json",
        ],
    ] {
        let failed = run(root.path(), &args)?;
        assert!(
            !failed.status.success(),
            "corrupt surreal.ref must fail closed"
        );
        assert!(
            String::from_utf8_lossy(&failed.stderr)
                .to_ascii_lowercase()
                .contains("surreal")
        );
    }
    Ok(())
}

#[cfg(feature = "surreal-surrealkv")]
#[test]
fn extract_uses_the_same_surreal_publication_path() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("main.rs"), "fn main() {}\n")?;
    let extract = run(
        root.path(),
        &[
            "extract",
            ".",
            "--store",
            "surreal",
            "--surreal-engine",
            "surrealkv",
            "--no-cluster",
            "--no-viz",
        ],
    )?;
    require_success(&extract, "Surreal extract")?;
    let active = BuildGuard::resolve_current_snapshot_directory(&root.path().join("compass-out"))?;
    assert!(active.join("graph.json").is_file());
    assert!(active.join(SURREAL_REF_FILE_NAME).is_file());
    Ok(())
}

#[cfg(unix)]
#[cfg(feature = "surreal-surrealkv")]
#[test]
fn watch_publishes_surreal_and_persists_changed_generations() -> Result<(), Box<dyn Error>> {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    struct WatchChild(std::process::Child);
    impl Drop for WatchChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("main.rs"), "fn watched() {}\n")?;
    let mut child = WatchChild(
        Command::new(env!("CARGO_BIN_EXE_compass"))
            .args([
                "watch",
                ".",
                "--poll",
                "--store",
                "surreal",
                "--surreal-engine",
                "surrealkv",
                "--no-cluster",
                "--no-viz",
            ])
            .current_dir(root.path())
            .env_remove("COMPASS_OUT")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = child.0.stdout.take().ok_or("missing watch stdout")?;
    let stderr = child.0.stderr.take().ok_or("missing watch stderr")?;
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout.take(256 * 1024)).lines().take(256) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.take(64 * 1024).read_to_string(&mut text);
        text
    });
    let mut transcript = String::new();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let line = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
        transcript.push_str(&line);
        if line.contains("Watching for changes") || line.contains("Watching ") {
            break;
        }
    }
    let output = root.path().join("compass-out");
    let first = BuildGuard::resolve_current_snapshot_directory(&output)?;
    let first_ref = SurrealRef::decode(&fs::read(first.join(SURREAL_REF_FILE_NAME))?)?;
    fs::write(
        root.path().join("main.rs"),
        "fn watched_after_change() {}\n",
    )?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let current = BuildGuard::resolve_current_snapshot_directory(&output)?;
        if current != first && current.join(SURREAL_REF_FILE_NAME).is_file() {
            let reference = SurrealRef::decode(&fs::read(current.join(SURREAL_REF_FILE_NAME))?)?;
            if reference.generation_id != first_ref.generation_id {
                break;
            }
        }
        if Instant::now() >= deadline {
            child.0.kill()?;
            child.0.wait()?;
            let errors = errors.join().map_err(|_| "watch stderr reader failed")?;
            return Err(format!(
                "watch did not publish changed Surreal generation: {transcript}; stderr: {errors}"
            )
            .into());
        }
        if let Ok(line) = receiver.recv_timeout(Duration::from_millis(100)) {
            transcript.push_str(&line?);
        }
    }
    let status = Command::new("kill")
        .args(["-INT", &child.0.id().to_string()])
        .status()?;
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("watch did not stop after SIGINT".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    reader.join().map_err(|_| "watch stdout reader failed")?;
    let errors = errors.join().map_err(|_| "watch stderr reader failed")?;
    assert!(
        status.success(),
        "watch transcript: {transcript}; stderr: {errors}"
    );
    let query = run(
        root.path(),
        &[
            "search",
            "watched_after_change",
            "--engine",
            "surreal",
            "--format",
            "json",
        ],
    )?;
    require_success(&query, "reopened watch generation")?;
    let value: Value = serde_json::from_slice(&query.stdout)?;
    assert!(!value["nodes"].as_array().ok_or("nodes absent")?.is_empty());
    Ok(())
}

#[cfg(feature = "surreal-rocksdb")]
#[test]
fn rocksdb_publication_and_explicit_query_are_available() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("main.rs"), "fn searchable() {}\n")?;
    let update = run(
        root.path(),
        &[
            "update",
            ".",
            "--store",
            "surreal",
            "--surreal-engine",
            "rocksdb",
            "--no-cluster",
            "--no-viz",
        ],
    )?;
    require_success(&update, "RocksDB publication")?;
    let active = BuildGuard::resolve_current_snapshot_directory(&root.path().join("compass-out"))?;
    let reference = SurrealRef::decode(&fs::read(active.join(SURREAL_REF_FILE_NAME))?)?;
    assert_eq!(reference.engine, SurrealEngine::RocksDb);
    let search = run(
        root.path(),
        &[
            "search",
            "searchable",
            "--engine",
            "surreal",
            "--format",
            "json",
        ],
    )?;
    require_success(&search, "RocksDB native query")?;
    let response: Value = serde_json::from_slice(&search.stdout)?;
    assert_eq!(response["schema"], "compass.query/1");
    Ok(())
}
