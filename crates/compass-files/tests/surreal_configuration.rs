use compass_files::{
    BuildScope, ProjectConfig, ProjectStorage, ProjectStore, ProjectSurrealEngine, SurrealSettings,
};
use std::error::Error;
use std::fs;

#[test]
fn yaml_connection_round_trip_to_secret_free_project_configuration() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let yaml = root.path().join("connection.yaml");
    fs::write(
        &yaml,
        "store: surreal\nengine: remote\nendpoint: http://127.0.0.1:28000/rpc\nnamespace: compass\ndatabase: graph\nusername: fixture\npassword: fixture-secret\n",
    )?;
    let config = SurrealSettings::read_yaml(&yaml)?.validated()?;
    assert_eq!(config.endpoint.as_deref(), Some("ws://127.0.0.1:28000"));
    assert!(!format!("{config:?}").contains("fixture-secret"));
    let project = ProjectConfig {
        version: 2,
        build: BuildScope::default(),
        storage: ProjectStorage {
            store: Some(ProjectStore::Surreal),
            surreal_engine: Some(ProjectSurrealEngine::Remote),
            surreal_endpoint: config.endpoint.clone(),
            surreal_namespace: config.namespace.clone(),
            surreal_database: config.database.clone(),
            ..ProjectStorage::default()
        },
    };
    let path = project.write(root.path())?;
    assert_eq!(ProjectConfig::load(root.path())?, Some(project));
    assert!(!fs::read_to_string(path)?.contains("fixture-secret"));
    config.authorize_target("ws://127.0.0.1:28000", "compass", "graph")?;
    assert!(
        config
            .authorize_target("ws://127.0.0.1:28001", "compass", "graph")
            .is_err()
    );
    assert!(
        config
            .authorize_target("ws://127.0.0.1:28000", "compass", "other")
            .is_err()
    );
    Ok(())
}

#[test]
fn malformed_oversized_and_unsafe_connection_files_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let yaml = root.path().join("connection.yaml");
    for contents in [
        "password: [fixture-secret",
        "unknown: fixture-secret",
        "engine: remote\nengine: rocksdb\n",
    ] {
        fs::write(&yaml, contents)?;
        let error = SurrealSettings::read_yaml(&yaml)
            .err()
            .ok_or("invalid YAML admitted")?;
        assert!(!error.contains("fixture-secret"));
    }
    fs::write(&yaml, vec![b' '; 65537])?;
    assert!(SurrealSettings::read_yaml(&yaml).is_err());
    for endpoint in [
        "ws://example.com:8000",
        "wss://user:secret@example.com",
        "wss://example.com?token=secret",
        "file:///tmp/database",
        "ws://127.0.0.1:8000/other",
    ] {
        fs::write(&yaml, format!("engine: remote\nendpoint: '{endpoint}'\n"))?;
        assert!(SurrealSettings::read_yaml(&yaml)?.validated().is_err());
    }
    Ok(())
}

#[test]
fn higher_priority_sources_select_remote_or_embedded_without_stale_paths()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let yaml = root.path().join("connection.yaml");
    fs::write(
        &yaml,
        "store: surreal\nengine: surrealkv\npath: compass-out/surreal\n",
    )?;
    let mut settings = SurrealSettings::read_yaml(&yaml)?;
    settings.overlay(SurrealSettings {
        endpoint: Some("ws://127.0.0.1:28000".into()),
        ..SurrealSettings::default()
    });
    let mut settings = settings.validated()?;
    assert_eq!(settings.engine.as_deref(), Some("remote"));
    assert!(settings.path.is_none());
    settings.overlay(SurrealSettings {
        engine: Some("surrealkv".into()),
        path: Some("compass-out/new-store".into()),
        ..SurrealSettings::default()
    });
    let settings = settings.validated()?;
    assert_eq!(settings.engine.as_deref(), Some("surrealkv"));
    assert!(
        settings
            .authorize_target("ws://127.0.0.1:28000", "compass", "graph")
            .is_err()
    );
    Ok(())
}
