use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use compass_files::BuildGuard;
use compass_graph::{GRAPH_SNAPSHOT_SELECTOR_SCHEMA_V1, GraphSnapshotReader, SnapshotSelector};
use compass_store::{
    STORE_FILE_NAME, STORE_REF_FILE_NAME, STORE_SCHEMA_V1, SqliteStore, StoreRef,
    local_sqlite_store_path,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
use compass_graphdb_surreal::{
    ProjectionBundle, SURREAL_REF_FILE_NAME, SurrealEngine, SurrealProjection, SurrealRef,
};

use crate::Outcome;

const BACKUP_SCHEMA_V1: &str = "compass.store.backup/1";
const SURREAL_BACKUP_SCHEMA_V1: &str = "compass.surreal.backup/1";
const MAX_BACKUP_MANIFEST_BYTES: u64 = 64 * 1024;
#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
const SURREAL_BUNDLE_FILE_NAME: &str = "surreal.bundle.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    schema: String,
    store_schema: String,
    adapter: String,
    graph_digest: String,
    store_digest: String,
    snapshot_id: String,
    manifest_digest: String,
    store_reference: StoreRef,
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SurrealBackupManifest {
    schema: String,
    adapter: String,
    graph_digest: String,
    bundle_digest: String,
    reference: SurrealRef,
}

pub(crate) fn command(args: &[String]) -> Outcome {
    let operation = args.first().map(String::as_str).unwrap_or("status");
    let result = match operation {
        "status" => status(args),
        "validate" => validate(args),
        "backup" => backup(args),
        "restore" => restore(args),
        _ => Err("usage: compass store <status|validate|backup|restore> [OPTIONS]".to_owned()),
    };
    match result {
        Ok(value) => match option(args, "--format").unwrap_or("text") {
            "json" => match serde_json::to_string_pretty(&value) {
                Ok(output) => Outcome::success(output),
                Err(error) => Outcome::failure(format!("error: {error}")),
            },
            "text" => Outcome::success(render_text(&value)),
            value => Outcome::failure(format!(
                "error: --format must be text or json (found {value})"
            )),
        },
        Err(error) => Outcome::failure(format!("error: {error}")),
    }
}

fn status(args: &[String]) -> Result<Value, String> {
    let output = output_root(args)?;
    let graph_path = output.join("graph.json");
    let store_path = local_sqlite_store_path(&graph_path);
    let reference_path = output.join(STORE_REF_FILE_NAME);
    let mut graph = if graph_path.is_file() {
        Some(graph_status(&graph_path)?)
    } else {
        None
    };

    let store = if store_path.is_file() {
        match SqliteStore::open_read_only(&store_path) {
            Ok(store) => match validate_store(&store, graph.as_mut(), &graph_path) {
                Ok((reference, snapshot_id, manifest_digest)) => Some(json!({
                    "present": true,
                    "valid": true,
                    "bytes": fs::metadata(&store_path).map(|metadata| metadata.len()).unwrap_or(0),
                    "sha256": digest_file(&store_path)?,
                    "adapter": "sqlite",
                    "storeSchema": STORE_SCHEMA_V1,
                    "snapshotId": snapshot_id,
                    "manifestDigest": manifest_digest,
                    "reference": reference,
                })),
                Err(error) => Some(json!({
                    "present": true,
                    "valid": false,
                    "adapter": "sqlite",
                    "error": error,
                })),
            },
            Err(error) => Some(json!({
                "present": true,
                "valid": false,
                "adapter": "sqlite",
                "error": error.to_string(),
            })),
        }
    } else {
        None
    };

    let reference = if reference_path.is_file() {
        let bytes =
            fs::read(&reference_path).map_err(|error| format!("read store.ref: {error}"))?;
        match serde_json::from_slice::<StoreRef>(&bytes) {
            Ok(reference) => match reference.validate() {
                Ok(()) => json!({ "present": true, "valid": true, "value": reference }),
                Err(error) => json!({
                    "present": true,
                    "valid": false,
                    "error": error.to_string(),
                }),
            },
            Err(error) => json!({
                "present": true,
                "valid": false,
                "error": error.to_string(),
            }),
        }
    } else {
        json!({ "present": false })
    };

    let surreal = surreal_status(&output);

    Ok(json!({
        "schema": "compass.store.status/1",
        "output": output,
        "graphJson": graph.unwrap_or_else(|| json!({ "present": false })),
        "store": store.unwrap_or_else(|| json!({ "present": false })),
        "storeRef": reference,
        "surrealStore": surreal,
        "rebuildCommand": "compass update --force",
    }))
}

fn validate(args: &[String]) -> Result<Value, String> {
    let output = output_root(args)?;
    if selects_surreal(args, &output)? {
        return validate_surreal(&output);
    }
    let graph_path = output.join("graph.json");
    let store_path = local_sqlite_store_path(&graph_path);
    if !store_path.is_file() {
        return Err(format!(
            "store sidecar is missing: {}",
            store_path.display()
        ));
    }
    let mut graph = if graph_path.is_file() {
        Some(graph_status(&graph_path)?)
    } else {
        None
    };
    let store = SqliteStore::open_read_only(&store_path).map_err(|error| error.to_string())?;
    let (reference, snapshot_id, manifest_digest) =
        validate_store(&store, graph.as_mut(), &graph_path)?;
    let reference_path = output.join(STORE_REF_FILE_NAME);
    if !reference_path.is_file() {
        return Err(format!(
            "store.ref is missing: {}",
            reference_path.display()
        ));
    }
    let on_disk: StoreRef = serde_json::from_slice(
        &fs::read(&reference_path).map_err(|error| format!("read store.ref: {error}"))?,
    )
    .map_err(|error| format!("decode store.ref: {error}"))?;
    on_disk.validate().map_err(|error| error.to_string())?;
    if on_disk != reference {
        return Err("store.ref does not match the active SQLite snapshot".to_owned());
    }
    Ok(json!({
        "schema": "compass.store.validation/1",
        "valid": true,
        "output": output,
        "graphJson": graph.unwrap_or_else(|| json!({ "present": false })),
        "adapter": "sqlite",
        "storeSchema": STORE_SCHEMA_V1,
        "snapshotId": snapshot_id,
        "manifestDigest": manifest_digest,
        "storeReference": reference,
    }))
}

fn backup(args: &[String]) -> Result<Value, String> {
    let output = output_root(args)?;
    let destination = option(args, "--output")
        .map(PathBuf::from)
        .ok_or_else(|| "store backup requires --output DIR".to_owned())?;
    if destination.exists() {
        return Err(format!(
            "backup destination already exists: {}",
            destination.display()
        ));
    }
    if selects_surreal(args, &output)? {
        return backup_surreal(&output, &destination);
    }
    let graph_path = output.join("graph.json");
    let store_path = local_sqlite_store_path(&graph_path);
    let reference_path = output.join(STORE_REF_FILE_NAME);
    let mut graph_value = graph_status(&graph_path)?;
    let reference_bytes =
        fs::read(&reference_path).map_err(|error| format!("read store.ref: {error}"))?;
    let reference: StoreRef = serde_json::from_slice(&reference_bytes)
        .map_err(|error| format!("decode store.ref: {error}"))?;
    reference.validate().map_err(|error| error.to_string())?;
    let store = SqliteStore::open(&store_path).map_err(|error| error.to_string())?;
    let (_, snapshot_id, manifest_digest) =
        validate_store(&store, Some(&mut graph_value), &graph_path)?;
    if reference.snapshot_id != snapshot_id || reference.manifest_digest != manifest_digest {
        return Err("store.ref does not match the active snapshot".to_owned());
    }

    fs::create_dir_all(&destination)
        .map_err(|error| format!("create backup destination: {error}"))?;
    let result = (|| {
        store
            .backup_to(destination.join(STORE_FILE_NAME))
            .map_err(|error| error.to_string())?;
        fs::copy(&graph_path, destination.join("graph.json"))
            .map_err(|error| format!("copy graph.json: {error}"))?;
        fs::copy(&reference_path, destination.join(STORE_REF_FILE_NAME))
            .map_err(|error| format!("copy store.ref: {error}"))?;
        let manifest = BackupManifest {
            schema: BACKUP_SCHEMA_V1.to_owned(),
            store_schema: STORE_SCHEMA_V1.to_owned(),
            adapter: "sqlite".to_owned(),
            graph_digest: graph_value
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| "graph status is missing its digest".to_owned())?
                .to_owned(),
            store_digest: digest_file(&destination.join(STORE_FILE_NAME))?,
            snapshot_id,
            manifest_digest,
            store_reference: reference,
        };
        fs::write(
            destination.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("write backup manifest: {error}"))?;
        Ok::<_, String>(manifest)
    })();
    match result {
        Ok(manifest) => Ok(json!({
            "schema": BACKUP_SCHEMA_V1,
            "backup": destination,
            "manifest": manifest,
        })),
        Err(error) => Err(with_restore_cleanup(error, &destination)),
    }
}

fn restore(args: &[String]) -> Result<Value, String> {
    let source = option(args, "--from")
        .map(PathBuf::from)
        .ok_or_else(|| "store restore requires --from DIR".to_owned())?;
    let destination = option(args, "--into")
        .map(PathBuf::from)
        .ok_or_else(|| "store restore requires --into DIR".to_owned())?;
    if destination.exists()
        && fs::read_dir(&destination)
            .map_err(|error| format!("inspect restore destination: {error}"))?
            .next()
            .is_some()
    {
        return Err(format!(
            "restore destination must be new or empty: {}",
            destination.display()
        ));
    }
    let manifest_bytes =
        compass_files::read_bytes_bounded(&source.join("manifest.json"), MAX_BACKUP_MANIFEST_BYTES)
            .map_err(|error| format!("read backup manifest: {error}"))?;
    let manifest_value: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("decode backup manifest: {error}"))?;
    if manifest_value.get("schema").and_then(Value::as_str) == Some(SURREAL_BACKUP_SCHEMA_V1) {
        return restore_surreal(&source, &destination, &manifest_bytes);
    }
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("decode backup manifest: {error}"))?;
    if manifest.schema != BACKUP_SCHEMA_V1
        || manifest.store_schema != STORE_SCHEMA_V1
        || manifest.adapter != "sqlite"
    {
        return Err("backup uses an unsupported Compass store format".to_owned());
    }
    manifest
        .store_reference
        .validate()
        .map_err(|error| error.to_string())?;
    let graph_path = source.join("graph.json");
    let mut graph_value = graph_status(&graph_path)?;
    if graph_value.get("sha256").and_then(Value::as_str) != Some(&manifest.graph_digest) {
        return Err("backup graph digest does not match manifest".to_owned());
    }
    let backup_store = source.join(STORE_FILE_NAME);
    let store = SqliteStore::open_read_only(&backup_store).map_err(|error| error.to_string())?;
    validate_store(&store, Some(&mut graph_value), &graph_path)?;
    if digest_file(&backup_store)? != manifest.store_digest {
        return Err("backup store digest does not match manifest".to_owned());
    }
    let backup_reference: StoreRef = serde_json::from_slice(
        &fs::read(source.join(STORE_REF_FILE_NAME))
            .map_err(|error| format!("read backup store.ref: {error}"))?,
    )
    .map_err(|error| format!("decode backup store.ref: {error}"))?;
    if backup_reference != manifest.store_reference {
        return Err("backup store.ref does not match manifest".to_owned());
    }

    fs::create_dir_all(&destination)
        .map_err(|error| format!("create restore destination: {error}"))?;
    let result = (|| {
        SqliteStore::restore_from(&backup_store, destination.join(STORE_FILE_NAME))
            .map_err(|error| error.to_string())?;
        fs::copy(&graph_path, destination.join("graph.json"))
            .map_err(|error| format!("restore graph.json: {error}"))?;
        fs::copy(
            source.join(STORE_REF_FILE_NAME),
            destination.join(STORE_REF_FILE_NAME),
        )
        .map_err(|error| format!("restore store.ref: {error}"))?;
        validate(&["validate".to_owned(), destination.display().to_string()])?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        return Err(with_restore_cleanup(error, &destination));
    }
    Ok(json!({
        "schema": "compass.store.restore/1",
        "restored": destination,
        "snapshotId": manifest.snapshot_id,
        "graphDigest": manifest.graph_digest,
    }))
}

fn selects_surreal(args: &[String], output: &Path) -> Result<bool, String> {
    match option(args, "--engine") {
        Some("surreal") => Ok(true),
        Some("store" | "sqlite") => Ok(false),
        Some(value) => Err(format!(
            "--engine must be sqlite or surreal for store operations (found {value})"
        )),
        None => Ok(output.join("surreal.ref").is_file()),
    }
}

fn surreal_status(output: &Path) -> Value {
    let reference_path = output.join("surreal.ref");
    if !reference_path.is_file() {
        return json!({
            "present": false,
            "available": cfg!(any(feature = "surreal-surrealkv", feature = "surreal-rocksdb", feature = "surreal-remote")),
        });
    }
    #[cfg(any(
        feature = "surreal-surrealkv",
        feature = "surreal-rocksdb",
        feature = "surreal-remote"
    ))]
    {
        // The `return` keeps this cfg block and its `not(...)` counterpart
        // interchangeable; without it the two would have to be one expression.
        #[allow(clippy::needless_return)]
        return match validate_surreal(output) {
            Ok(value) => value,
            Err(error) => json!({
                "present": true,
                "available": true,
                "valid": false,
                "error": error,
            }),
        };
    }
    #[cfg(not(any(
        feature = "surreal-surrealkv",
        feature = "surreal-rocksdb",
        feature = "surreal-remote"
    )))]
    {
        let value = compass_files::read_bytes_bounded(&reference_path, 32 * 1024)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        json!({
            "present": true,
            "available": false,
            "valid": value.as_ref().is_some_and(|value| {
                value.get("schema").and_then(Value::as_str) == Some("compass.surreal.ref/1")
            }),
            "reference": value,
        })
    }
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
fn validate_surreal(output: &Path) -> Result<Value, String> {
    let reference = read_surreal_reference(output)?;
    let runtime = surreal_runtime()?;
    runtime.block_on(async {
        let projection = open_surreal_projection(&reference, false).await?;
        projection
            .validate_contents(&reference)
            .await
            .map_err(|error| error.to_string())?;
        let graph_path = output.join("graph.json");
        if graph_path.is_file() {
            let digest = format!("sha256:{}", digest_file(&graph_path)?);
            if digest != reference.graph_digest {
                return Err("surreal.ref graph digest does not match graph.json".to_owned());
            }
        }
        Ok(json!({
            "schema": "compass.surreal.validation/1",
            "present": true,
            "available": true,
            "valid": true,
            "adapter": "surreal",
            "engine": reference.engine.as_str(),
            "referenceContract": reference.schema,
            "generationId": reference.generation_id,
            "graphDigest": reference.graph_digest,
            "nodeCount": reference.node_count,
            "relationCount": reference.relation_count,
            "fileCount": reference.file_count,
            "location": reference.location,
            "reference": reference,
        }))
    })
}

#[cfg(not(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
)))]
fn validate_surreal(_output: &Path) -> Result<Value, String> {
    Err("this Compass binary was built without a SurrealDB engine".to_owned())
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
fn backup_surreal(output: &Path, destination: &Path) -> Result<Value, String> {
    let reference = read_surreal_reference(output)?;
    let graph_path = output.join("graph.json");
    let graph_digest = format!("sha256:{}", digest_file(&graph_path)?);
    if graph_digest != reference.graph_digest {
        return Err("surreal.ref graph digest does not match graph.json".to_owned());
    }
    let runtime = surreal_runtime()?;
    let bundle = runtime.block_on(async {
        let projection = open_surreal_projection(&reference, false).await?;
        projection
            .export_bundle(&reference)
            .await
            .map_err(|error| error.to_string())
    })?;
    bundle.validate().map_err(|error| error.to_string())?;
    let manifest = SurrealBackupManifest {
        schema: SURREAL_BACKUP_SCHEMA_V1.to_owned(),
        adapter: format!("surreal-{}", reference.engine.as_str()),
        graph_digest,
        bundle_digest: bundle.digest.clone(),
        reference,
    };
    fs::create_dir_all(destination)
        .map_err(|error| format!("create backup destination: {error}"))?;
    let result = (|| {
        fs::copy(&graph_path, destination.join("graph.json"))
            .map_err(|error| format!("copy graph.json: {error}"))?;
        fs::write(
            destination.join(SURREAL_BUNDLE_FILE_NAME),
            serde_json::to_vec_pretty(&bundle).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("write Surreal projection bundle: {error}"))?;
        fs::write(
            destination.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("write backup manifest: {error}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        return Err(with_restore_cleanup(error, destination));
    }
    Ok(json!({
        "schema": SURREAL_BACKUP_SCHEMA_V1,
        "backup": destination,
        "manifest": manifest,
    }))
}

#[cfg(not(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
)))]
fn backup_surreal(_output: &Path, _destination: &Path) -> Result<Value, String> {
    Err("this Compass binary was built without a SurrealDB engine".to_owned())
}

/// Roll a failed restore back, reporting cleanup that could not complete.
///
/// Embedded store owners are retained for the process lifetime, so the files
/// under `destination` may still be open and locked when a restore fails.
/// POSIX unlinks them anyway; Windows refuses, which used to leave a partially
/// populated directory that the next attempt rejected as non-empty. Report both
/// the original failure and the manual step that unblocks a retry rather than
/// discarding the cleanup result.
fn with_restore_cleanup(error: String, destination: &Path) -> String {
    match fs::remove_dir_all(destination) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => format!(
            "{error} (could not remove the partially restored directory {}: {cleanup}; \
remove it manually before retrying, closing any process still using the store)",
            destination.display()
        ),
    }
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
fn restore_surreal(
    source: &Path,
    destination: &Path,
    manifest_bytes: &[u8],
) -> Result<Value, String> {
    let manifest: SurrealBackupManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|error| format!("decode Surreal backup manifest: {error}"))?;
    if manifest.schema != SURREAL_BACKUP_SCHEMA_V1
        || manifest.adapter != format!("surreal-{}", manifest.reference.engine.as_str())
    {
        return Err("backup uses an unsupported Compass Surreal format".to_owned());
    }
    let bundle_path = source.join(SURREAL_BUNDLE_FILE_NAME);
    let bundle_bytes = bounded_read(
        &bundle_path,
        compass_model::DEFAULT_GRAPH_SIZE_CAP_BYTES,
        "Surreal projection bundle",
    )?;
    let bundle: ProjectionBundle = serde_json::from_slice(&bundle_bytes)
        .map_err(|error| format!("decode Surreal projection bundle: {error}"))?;
    bundle.validate().map_err(|error| error.to_string())?;
    if bundle.digest != manifest.bundle_digest || bundle.reference != manifest.reference {
        return Err("Surreal projection bundle does not match backup manifest".to_owned());
    }
    let source_graph = source.join("graph.json");
    let graph_digest = format!("sha256:{}", digest_file(&source_graph)?);
    if graph_digest != manifest.graph_digest || graph_digest != bundle.reference.graph_digest {
        return Err("backup graph digest does not match its Surreal projection".to_owned());
    }

    let location = destination.join("surreal");
    let settings = compass_files::surreal_settings().ok();
    let selected_engine = settings.and_then(|settings| settings.engine.as_deref());
    let restore_engine = match selected_engine {
        Some("remote") => SurrealEngine::Remote,
        Some("surrealkv") => SurrealEngine::SurrealKv,
        Some("rocksdb") => SurrealEngine::RocksDb,
        _ => bundle.reference.engine,
    };
    let new_reference = if restore_engine == SurrealEngine::Remote {
        SurrealRef::from_remote_plan(&bundle.plan, graph_digest.clone())
    } else {
        SurrealRef::from_plan(
            &bundle.plan,
            restore_engine,
            &location,
            graph_digest.clone(),
        )
    }
    .map_err(|error| error.to_string())?;
    let runtime = surreal_runtime()?;
    fs::create_dir_all(destination)
        .map_err(|error| format!("create restore destination: {error}"))?;
    let result = runtime.block_on(async {
        let projection = open_surreal_projection(&new_reference, true).await?;
        projection
            .stage_for_reference(&bundle.plan, &new_reference)
            .await
            .map_err(|error| error.to_string())?;
        projection
            .validate_reference(&new_reference)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<SurrealProjection, String>(projection)
    });
    let projection = match result {
        Ok(projection) => projection,
        Err(error) => {
            return Err(with_restore_cleanup(error, destination));
        }
    };
    let publication = (|| {
        fs::copy(&source_graph, destination.join("graph.json"))
            .map_err(|error| format!("restore graph.json: {error}"))?;
        fs::write(
            destination.join(SURREAL_REF_FILE_NAME),
            new_reference.encode().map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("write surreal.ref: {error}"))?;
        if read_surreal_reference(destination)? != new_reference
            || format!("sha256:{}", digest_file(&destination.join("graph.json"))?)
                != new_reference.graph_digest
        {
            return Err(
                "restored artifacts do not match their validated Surreal generation".to_owned(),
            );
        }
        runtime
            .block_on(projection.validate_reference(&new_reference))
            .map_err(|error| error.to_string())?;
        Ok::<(), String>(())
    })();
    if let Err(error) = publication {
        return Err(with_restore_cleanup(error, destination));
    }
    Ok(json!({
        "schema": "compass.surreal.restore/1",
        "restored": destination,
        "generationId": new_reference.generation_id,
        "graphDigest": graph_digest,
        "engine": new_reference.engine.as_str(),
    }))
}

#[cfg(not(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
)))]
fn restore_surreal(
    _source: &Path,
    _destination: &Path,
    _manifest_bytes: &[u8],
) -> Result<Value, String> {
    Err("this Compass binary was built without a SurrealDB engine".to_owned())
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
fn read_surreal_reference(output: &Path) -> Result<SurrealRef, String> {
    let path = output.join(SURREAL_REF_FILE_NAME);
    let bytes = bounded_read(
        &path,
        compass_graphdb_surreal::MAX_SURREAL_REF_BYTES,
        "surreal.ref",
    )?;
    SurrealRef::decode(&bytes).map_err(|error| error.to_string())
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
fn surreal_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start Surreal runtime: {error}"))
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
async fn open_surreal_projection(
    reference: &SurrealRef,
    initialize_schema: bool,
) -> Result<SurrealProjection, String> {
    SurrealProjection::open_reference(reference, initialize_schema)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
fn bounded_read(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    compass_files::read_bytes_bounded(path, limit).map_err(|error| format!("read {label}: {error}"))
}

fn validate_store(
    store: &SqliteStore,
    graph: Option<&mut Value>,
    graph_path: &Path,
) -> Result<(StoreRef, String, String), String> {
    let reference_path = graph_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(STORE_REF_FILE_NAME);
    let reference: StoreRef = serde_json::from_slice(
        &fs::read(&reference_path)
            .map_err(|error| format!("read {}: {error}", reference_path.display()))?,
    )
    .map_err(|error| format!("decode store.ref: {error}"))?;
    reference.validate().map_err(|error| error.to_string())?;
    let reader = GraphSnapshotReader::open_selector_for_maintenance(
        store,
        SnapshotSelector {
            schema: GRAPH_SNAPSHOT_SELECTOR_SCHEMA_V1.to_owned(),
            snapshot_id: reference.snapshot_id.clone(),
            manifest_digest: reference.manifest_digest.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    let manifest = reader.manifest();
    reader
        .validate_integrity()
        .map_err(|error| error.to_string())?;
    if let Some(graph) = graph {
        let graph_bytes = graph
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| "graph status is missing its byte count".to_owned())?;
        let graph_digest = graph
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| "graph status is missing its digest".to_owned())?;
        if graph_bytes != manifest.graph_bytes || graph_digest != manifest.graph_digest {
            return Err(format!(
                "store manifest does not match {}",
                graph_path.display()
            ));
        }
        graph["nodes"] = json!(manifest.node_count);
        graph["edges"] = json!(manifest.edge_count);
    }
    let actual = store
        .graph_snapshot_reference_for(&reference.snapshot_id, &reference.manifest_digest)
        .map_err(|error| error.to_string())?;
    if actual != reference {
        return Err("store.ref does not match the selected immutable snapshot".to_owned());
    }
    let manifest_digest = reference.manifest_digest.clone();
    Ok((reference, manifest.snapshot_id.clone(), manifest_digest))
}

fn graph_status(path: &Path) -> Result<Value, String> {
    let bytes = fs::metadata(path)
        .map_err(|error| format!("inspect {}: {error}", path.display()))?
        .len();
    Ok(json!({
        "present": true,
        "bytes": bytes,
        "sha256": digest_file(path)?,
    }))
}

fn output_root(args: &[String]) -> Result<PathBuf, String> {
    let candidate = positional(args)
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("compass-out"));
    if candidate.file_name().and_then(|name| name.to_str()) == Some("graph.json") {
        let graph = BuildGuard::resolve_requested_artifact(&candidate)
            .map_err(|error| error.to_string())?;
        return Ok(graph.parent().unwrap_or(Path::new(".")).to_path_buf());
    }
    let output_container = if candidate.join("compass-out").is_dir() {
        candidate.join("compass-out")
    } else {
        candidate.clone()
    };
    if output_container.join("current-snapshot").is_file()
        || output_container.join("snapshots").is_dir()
    {
        return BuildGuard::resolve_current_snapshot_directory(&output_container)
            .map_err(|error| error.to_string());
    }
    if candidate.join("graph.json").is_file() || candidate.join(STORE_FILE_NAME).is_file() {
        return Ok(candidate);
    }
    BuildGuard::resolve_current_snapshot_directory(&output_container)
        .map_err(|error| error.to_string())
}

fn positional(args: &[String]) -> Vec<String> {
    let value_options = ["--format", "--output", "--from", "--into", "--engine"];
    let mut values = Vec::new();
    let mut skip = false;
    for argument in args.iter().skip(1) {
        if skip {
            skip = false;
        } else if value_options.contains(&argument.as_str()) {
            skip = true;
        } else if !argument.starts_with("--") {
            values.push(argument.clone());
        }
    }
    values
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(index, argument)| {
        if argument == name {
            args.get(index + 1).map(String::as_str)
        } else {
            argument
                .strip_prefix(name)
                .and_then(|value| value.strip_prefix('='))
        }
    })
}

fn digest_file(path: &Path) -> Result<String, String> {
    let mut reader =
        File::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn render_text(value: &Value) -> String {
    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let output = value
        .get("output")
        .or_else(|| value.get("backup"))
        .or_else(|| value.get("restored"))
        .map(Value::to_string)
        .unwrap_or_default();
    let valid = value.get("valid").and_then(Value::as_bool);
    match valid {
        Some(valid) => format!("{schema}: valid={valid} {output}"),
        None => format!("{schema}: {output}"),
    }
}
