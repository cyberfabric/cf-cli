use anyhow::Context;
use clap::Args;
use module_parser::{CargoToml, Config, ConfigModuleMetadata, get_module_name_from_crate};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args)]
pub struct BuildRunArgs {
    /// Path to the config file
    #[arg(short = 'c', long, default_value = "./cyberfabric.yaml")]
    pub config: PathBuf,
    /// Path to the module
    #[arg(short = 'p', long, default_value = ".")]
    pub path: PathBuf,
    /// Use OpenTelemetry tracing
    #[arg(long)]
    pub otel: bool,
    /// Build/run in release mode
    #[arg(short = 'r', long)]
    pub release: bool,
}

pub const BASE_PATH: &str = ".cyberfabric";

const CARGO_CONFIG_TOML: &str = r#"[build]
target-dir = "../target"
build-dir = "../target"
"#;

const CARGO_SERVER_MAIN: &str = r#"
use anyhow::Result;
use modkit::bootstrap::{
    AppConfig, host::{init_logging_unified, init_panic_tracing}, /* run_migrate, */ run_server,
};
{{dependencies}}

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::load_or_default(&Some(std::path::PathBuf::from("{{config_path}}")))?;

    // Build OpenTelemetry layer before logging
    // Convert TracingConfig from modkit::bootstrap to modkit's type (they have identical structure)
    #[cfg(feature = "otel")]
    let otel_layer = if config.tracing.enabled {
        Some(modkit::telemetry::init::init_tracing(&config.tracing)?)
    } else {
        None
    };
    #[cfg(not(feature = "otel"))]
    let otel_layer = None;

    // Initialize logging + otel in one Registry
    init_logging_unified(&config.logging, &config.server.home_dir, otel_layer);

    // Register custom panic hook to reroute panic backtrace into tracing.
    init_panic_tracing();

    // One-time connectivity probe
    #[cfg(feature = "otel")]
    if config.tracing.enabled
        && let Err(e) = modkit::telemetry::init::otel_connectivity_probe(&config.tracing)
    {
        tracing::error!(error = %e, "OTLP connectivity probe failed");
    }

    tracing::info!("CyberFabric Server starting");

    run_server(config).await
}"#;

pub fn cargo_command(subcommand: &str, path: &Path, otel: bool, release: bool) -> Command {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = Command::new(cargo);
    cmd.arg(subcommand);
    if otel {
        cmd.arg("-F").arg("otel");
    }
    if release {
        cmd.arg("-r");
    }
    cmd.current_dir(path);
    cmd
}

pub fn get_config(path: &Path, config_path: &Path) -> anyhow::Result<Config> {
    let mut config = get_config_from_path(config_path)?;
    let mut members = get_module_name_from_crate(&path.to_path_buf())?;

    config.modules.iter_mut().for_each(|module| {
        if let Some(module_metadata) = members.remove(module.0.as_str()) {
            module.1.metadata = module_metadata.metadata;
        } else {
            eprintln!(
                "info: config module '{}' not found locally, retrieving it from the registry",
                module.0
            );
        }
    });

    Ok(config)
}

fn get_config_from_path(path: &Path) -> anyhow::Result<Config> {
    let config = fs::File::open(path).context("config not available")?;
    serde_saphyr::from_reader(config).context("config not valid")
}

fn create_features() -> HashMap<String, Vec<String>> {
    let mut res = HashMap::with_capacity(2);
    res.insert("default".to_owned(), vec![]);
    res.insert("otel".to_owned(), vec!["modkit/otel".to_owned()]);
    res
}

fn insert_required_deps(
    mut dependencies: HashMap<String, ConfigModuleMetadata>,
) -> HashMap<String, ConfigModuleMetadata> {
    dependencies.insert(
        "modkit".to_owned(),
        ConfigModuleMetadata {
            package: Some("cf-modkit".to_owned()),
            features: vec!["bootstrap".to_owned()],
            ..Default::default()
        },
    );
    dependencies.insert(
        "anyhow".to_owned(),
        ConfigModuleMetadata {
            package: Some("anyhow".to_owned()),
            version: Some("1".to_owned()),
            ..Default::default()
        },
    );
    dependencies.insert(
        "tokio".to_owned(),
        ConfigModuleMetadata {
            package: Some("tokio".to_owned()),
            features: vec!["full".to_owned()],
            version: Some("1".to_owned()),
            ..Default::default()
        },
    );
    dependencies.insert(
        "tracing".to_owned(),
        ConfigModuleMetadata {
            package: Some("tracing".to_owned()),
            version: Some("0.1".to_owned()),
            ..Default::default()
        },
    );
    dependencies
}

pub fn generate_server_structure(
    path: &Path,
    config_path: &Path,
    dependencies: &HashMap<String, ConfigModuleMetadata>,
) -> anyhow::Result<()> {
    let features = create_features();

    let cargo_toml = toml::to_string(&CargoToml {
        dependencies: insert_required_deps(dependencies.clone()),
        features,
        ..Default::default()
    })
    .context("something went wrong when transforming to toml")?;
    let main_template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(CARGO_SERVER_MAIN)?;

    create_file_structure(path, "Cargo.toml", &cargo_toml)?;
    create_file_structure(path, ".cargo/config.toml", CARGO_CONFIG_TOML)?;
    create_file_structure(
        path,
        "src/main.rs",
        &main_template.render(&prepare_cargo_server_main(config_path, dependencies))?,
    )?;

    Ok(())
}

fn create_file_structure(path: &Path, relative_path: &str, contents: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let path = PathBuf::from(path).join(BASE_PATH).join(relative_path);
    fs::create_dir_all(
        path.parent().context(
            "this should be unreachable, the parent for the file structure always exists",
        )?,
    )
    .context("can't create directory")?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .context("can't create file")?;
    file.write_all(contents.as_bytes())
        .context("can't write to file")
}

/// UNC paths are not supported like `\\server\share`, as we replace backslashes with forward slashes.
fn prepare_cargo_server_main(
    config_path: &Path,
    dependencies: &HashMap<String, ConfigModuleMetadata>,
) -> liquid::Object {
    use std::fmt::Write;
    let dependencies = dependencies.keys().fold(String::new(), |mut acc, name| {
        _ = writeln!(acc, "use {name} as _;");
        acc
    });
    let config_path = config_path.display().to_string().replace('\\', "/");

    liquid::object!({
        "dependencies": dependencies,
        "config_path": config_path,
    })
}
