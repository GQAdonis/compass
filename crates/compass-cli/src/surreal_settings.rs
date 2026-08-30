//! One startup configuration path for publication, query, MCP, and operations.
use compass_files::{ProjectConfig, ProjectStore, ProjectSurrealEngine, SurrealSettings};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn prepare_surreal_arguments(arguments: &mut Vec<OsString>) -> Result<(), String> {
    let Some(command) = arguments.first().and_then(|a| a.to_str()) else {
        return Ok(());
    };
    let publication = matches!(command, "init" | "extract" | "update" | "watch");
    if !publication
        && !matches!(
            command,
            "ask"
                | "search"
                | "callers"
                | "callees"
                | "impact"
                | "explore"
                | "node"
                | "query"
                | "context"
                | "serve"
                | "store"
        )
    {
        return Ok(());
    }
    if arguments
        .iter()
        .any(|a| a == "--at" || a.to_str().is_some_and(|v| v.starts_with("--at=")))
    {
        if arguments
            .iter()
            .any(|a| a.to_str().is_some_and(|v| v.starts_with("--surreal-")))
        {
            return Err(
                "historical --at queries cannot change their Surreal connection configuration"
                    .into(),
            );
        }
        return Ok(());
    }
    let root = project_root(arguments, publication)?;
    let mut settings = SurrealSettings::default();
    if let Some(config) = ProjectConfig::load(&root).map_err(|e| e.to_string())? {
        settings.store = config.storage.store.map(|s| {
            match s {
                ProjectStore::Json => "json",
                ProjectStore::Sqlite => "sqlite",
                ProjectStore::Surreal => "surreal",
            }
            .into()
        });
        settings.engine = config.storage.surreal_engine.map(|s| {
            match s {
                ProjectSurrealEngine::SurrealKv => "surrealkv",
                ProjectSurrealEngine::RocksDb => "rocksdb",
                ProjectSurrealEngine::Remote => "remote",
            }
            .into()
        });
        settings.path = config.storage.surreal_path;
        settings.endpoint = config.storage.surreal_endpoint;
        settings.namespace = config.storage.surreal_namespace;
        settings.database = config.storage.surreal_database;
    }
    let mut flags = SurrealSettings::default();
    let mut config_path = None;
    let mut retained = vec![arguments[0].clone()];
    let mut index = 1;
    while index < arguments.len() {
        let Some(argument) = arguments[index].to_str() else {
            retained.push(arguments[index].clone());
            index += 1;
            continue;
        };
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(n, v)| (n, Some(v)));
        if !name.starts_with("--surreal-") && !(publication && name == "--store") {
            retained.push(arguments[index].clone());
            index += 1;
            continue;
        }
        let value = if let Some(value) = inline {
            value.to_owned()
        } else {
            index += 1;
            arguments
                .get(index)
                .and_then(|v| v.to_str())
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| format!("{name} requires a value"))?
                .to_owned()
        };
        if value.is_empty() {
            return Err(format!("{name} requires a nonempty value"));
        }
        match name {
            "--store" => flags.store = Some(value),
            "--surreal-config" => config_path = Some(PathBuf::from(value)),
            "--surreal-engine" => flags.engine = Some(value),
            "--surreal-path" => flags.path = Some(PathBuf::from(value)),
            "--surreal-endpoint" => flags.endpoint = Some(value),
            "--surreal-namespace" => flags.namespace = Some(value),
            "--surreal-database" => flags.database = Some(value),
            "--surreal-auth-level" => flags.auth_level = Some(value),
            "--surreal-username" => flags.username = Some(value),
            "--surreal-password" => flags.password = Some(value),
            "--surreal-token" => flags.token = Some(value),
            "--surreal-password-env" => flags.password_env = Some(value),
            "--surreal-token-env" => flags.token_env = Some(value),
            _ => return Err("unknown --surreal-* option (value redacted)".into()),
        }
        index += 1;
    }
    let explicit_config =
        config_path.or_else(|| std::env::var_os("COMPASS_SURREAL_CONFIG").map(PathBuf::from));
    let default_config = crate::home_directory().map(|home| home.join(".compass/surreal.yaml"));
    if let Some(path) = explicit_config.or_else(|| default_config.filter(|p| p.is_file())) {
        settings.overlay(SurrealSettings::read_yaml(&path)?);
    }
    let environment = SurrealSettings::from_environment()?;
    // An explicit endpoint selects server mode unless that same higher-priority
    // source explicitly selected embedded mode.
    if environment.endpoint.is_some() && environment.engine.is_none() {
        settings.engine = None;
    }
    settings.overlay(environment);
    if flags.endpoint.is_some() && flags.engine.is_none() {
        settings.engine = None;
    }
    if flags.engine.as_deref() == Some("remote") || flags.endpoint.is_some() {
        settings.path = None;
    }
    if flags.path.is_some() && flags.engine.is_none() {
        settings.engine = Some("surrealkv".into());
    }
    settings.overlay(flags);
    let settings = settings.validated()?;
    if publication {
        if let Some(store) = &settings.store {
            retained.extend(["--store".into(), store.into()]);
        }
        if settings.store.as_deref() == Some("surreal") {
            if let Some(engine) = &settings.engine {
                retained.extend(["--surreal-engine".into(), engine.into()]);
            }
            if let Some(path) = &settings.path {
                retained.extend(["--surreal-path".into(), path.as_os_str().to_owned()]);
            }
        }
    }
    compass_files::configure_surreal(settings)?;
    *arguments = retained;
    Ok(())
}

fn project_root(arguments: &[OsString], publication: bool) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut candidate = cwd.clone();
    if publication {
        let mut index = 1;
        while index < arguments.len() {
            let value = arguments[index].to_string_lossy();
            if value.starts_with('-') {
                if !value.contains('=')
                    && !matches!(
                        value.as_ref(),
                        "--yes"
                            | "--force"
                            | "--timing"
                            | "--program"
                            | "--no-program"
                            | "--no-cluster"
                            | "--no-viz"
                            | "--no-gitignore"
                            | "--code-only"
                            | "--cargo"
                            | "--google-workspace"
                            | "--global"
                            | "--allow-partial"
                            | "--reuse-cache-on-force"
                            | "--dedup-llm"
                    )
                {
                    index += 1;
                }
            } else {
                candidate = PathBuf::from(&arguments[index]);
                if !candidate.is_absolute() {
                    candidate = cwd.join(candidate);
                }
                break;
            }
            index += 1;
        }
    } else {
        for (index, value) in arguments.iter().enumerate() {
            let text = value.to_string_lossy();
            let graph = if text == "--graph" {
                arguments.get(index + 1).map(PathBuf::from)
            } else {
                text.strip_prefix("--graph=").map(PathBuf::from)
            };
            if let Some(graph) = graph {
                candidate = if graph.is_absolute() {
                    graph
                } else {
                    cwd.join(graph)
                };
                candidate = candidate.parent().unwrap_or(Path::new(".")).to_path_buf();
            }
        }
        if let Some(root) = candidate.ancestors().find(|p| {
            p.join(compass_files::PROJECT_CONFIG_RELATIVE_PATH)
                .is_file()
        }) {
            return Ok(root.to_path_buf());
        }
        return Ok(cwd);
    }
    Ok(candidate)
}
