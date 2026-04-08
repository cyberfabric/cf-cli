use crate::config::validate_name;
use anyhow::Context;
use clap::{Args, ValueEnum};
use module_parser::{
    CargoToml, CargoTomlDependencies, CargoTomlDependency, Config, ConfigModuleMetadata, Package,
    get_dependencies, get_module_name_from_crate,
};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

#[derive(Args)]
pub struct PathConfigArgs {
    /// Path to the module workspace root
    #[arg(short = 'p', long, value_parser = parse_and_chdir)]
    pub path: Option<PathBuf>,
    /// Path to the config file
    #[arg(short = 'c', long)]
    pub config: PathBuf,
}

fn parse_and_chdir(s: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(s);

    if !path.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }

    env::set_current_dir(&path)
        .map_err(|e| format!("failed to change directory to {}: {e}", path.display()))?;

    Ok(path)
}

impl PathConfigArgs {
    pub fn resolve_config(&self) -> anyhow::Result<PathBuf> {
        self.config
            .canonicalize()
            .context("can't canonicalize config")
    }
}

pub fn workspace_root() -> anyhow::Result<PathBuf> {
    env::current_dir().context("can't determine current working directory")
}

#[derive(Args)]
pub struct BuildRunArgs {
    #[command(flatten)]
    pub path_config: PathConfigArgs,
    /// Use OpenTelemetry tracing
    #[arg(long)]
    pub otel: bool,
    /// Build/run in release mode
    #[arg(short = 'r', long)]
    pub release: bool,
    /// Remove Cargo.lock at the start of the execution
    #[arg(long)]
    pub clean: bool,
    /// Override the generated server and binary name
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum Registry {
    #[default]
    #[value(name = "crates.io")]
    CratesIo,
}

impl Registry {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CratesIo => "crates.io",
        }
    }
}

impl Display for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BuildRunArgs {
    pub fn resolve_config_and_name(&self) -> anyhow::Result<(PathBuf, String)> {
        let config_path = self.path_config.resolve_config()?;
        let project_name = resolve_generated_project_name(&config_path, self.name.as_deref())?;
        if self.clean {
            remove_from_file_structure(&project_name, "Cargo.lock")?;
        }

        Ok((config_path, project_name))
    }
}

pub const BASE_PATH: &str = ".cyberfabric";

const CONFIG_PATH_ENV_VAR: &str = "CF_CLI_CONFIG";

const CARGO_CONFIG_TOML: &str = r#"[build]
target-dir = "../../target"
build-dir = "../../target"
"#;

const CARGO_SERVER_MAIN: &str = r#"
use anyhow::{Context, Result};
use modkit::bootstrap::{AppConfig, /* run_migrate, */ run_server};
{{dependencies}}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::var_os("CF_CLI_CONFIG")
        .map(std::path::PathBuf::from)
        .context("CF_CLI_CONFIG is not set")?;
    let config = AppConfig::load_or_default(Some(&config_path))?;

    run_server(config).await
}"#;

pub fn cargo_command(
    subcommand: &str,
    path: &Path,
    config_path: &Path,
    otel: bool,
    release: bool,
) -> Command {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = Command::new(cargo);
    cmd.arg(subcommand);
    cmd.env(CONFIG_PATH_ENV_VAR, config_path.as_os_str());
    if otel {
        cmd.arg("-F").arg("otel");
    }
    if release {
        cmd.arg("-r");
    }
    cmd.current_dir(path);
    cmd
}

pub fn get_config(config_path: &Path) -> anyhow::Result<Config> {
    let mut config = get_config_from_path(config_path)?;
    let mut members = get_module_name_from_crate()?;

    config.modules.iter_mut().for_each(|module| {
        if let Some(module_metadata) = members.remove(module.0.as_str()) {
            let config_metadata = std::mem::take(&mut module.1.metadata);
            module.1.metadata = merge_module_metadata(config_metadata, module_metadata.metadata);
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

fn merge_module_metadata(
    config_metadata: ConfigModuleMetadata,
    local_metadata: ConfigModuleMetadata,
) -> ConfigModuleMetadata {
    let features = if config_metadata.features.is_empty() {
        local_metadata.features
    } else {
        config_metadata.features
    };

    ConfigModuleMetadata {
        package: config_metadata.package.or(local_metadata.package),
        version: config_metadata.version.or(local_metadata.version),
        features,
        default_features: config_metadata
            .default_features
            .or(local_metadata.default_features),
        path: config_metadata.path.or(local_metadata.path),
        deps: local_metadata.deps,
        capabilities: local_metadata.capabilities,
    }
}

static FEATURES: LazyLock<HashMap<String, Vec<String>>> = LazyLock::new(|| {
    let mut res = HashMap::with_capacity(2);
    res.insert("default".to_owned(), vec![]);
    res.insert("otel".to_owned(), vec!["modkit/otel".to_owned()]);
    res
});

static CARGO_DEPS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let mut res = HashMap::with_capacity(5);
    res.insert("cf-modkit".to_owned(), "modkit".to_owned());
    res.insert("modkit".to_owned(), "modkit".to_owned()); // just in case there's a renamed
    res.insert("anyhow".to_owned(), "anyhow".to_owned());
    res.insert("tokio".to_owned(), "tokio".to_owned());
    res.insert("tracing".to_owned(), "tracing".to_owned());
    res
});

fn create_required_deps() -> anyhow::Result<CargoTomlDependencies> {
    let workspace_path = workspace_root()?;
    let mut deps = get_dependencies(&workspace_path, &CARGO_DEPS)?;
    if let Some(modkit) = deps.get_mut("modkit") {
        modkit.features.insert("bootstrap".to_owned());
    } else {
        deps.insert(
            "modkit".to_owned(),
            CargoTomlDependency {
                package: Some("cf-modkit".to_owned()),
                features: BTreeSet::from(["bootstrap".to_owned()]),
                ..Default::default()
            },
        );
    }
    if let Some(tokio) = deps.get_mut("tokio") {
        tokio.features.insert("full".to_owned());
    } else {
        deps.insert(
            "tokio".to_owned(),
            CargoTomlDependency {
                features: BTreeSet::from(["full".to_owned()]),
                version: Some("1".to_owned()),
                ..Default::default()
            },
        );
    }
    Ok(deps)
}

pub fn generate_server_structure(
    project_name: &str,
    current_dependencies: &CargoTomlDependencies,
) -> anyhow::Result<()> {
    let mut dependencies = current_dependencies.clone();
    dependencies.extend(create_required_deps()?);
    let cargo_toml = CargoToml {
        package: Package {
            name: project_name.to_owned(),
            ..Default::default()
        },
        dependencies,
        features: FEATURES.clone(),
        ..Default::default()
    };
    let cargo_toml_str =
        toml::to_string(&cargo_toml).context("something went wrong when transforming to toml")?;
    let main_rs = prepare_cargo_server_main(current_dependencies);

    create_file_structure(project_name, "Cargo.toml", &cargo_toml_str)?;
    create_file_structure(project_name, ".cargo/config.toml", CARGO_CONFIG_TOML)?;
    create_file_structure(project_name, "src/main.rs", &main_rs)?;

    Ok(())
}

pub fn generated_project_dir(project_name: &str) -> anyhow::Result<PathBuf> {
    Ok(workspace_root()?.join(BASE_PATH).join(project_name))
}

fn create_file_structure(
    project_name: &str,
    relative_path: &str,
    contents: &str,
) -> anyhow::Result<()> {
    use std::io::Write;
    let path = generated_project_dir(project_name)?.join(relative_path);
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

fn remove_from_file_structure(project_name: &str, relative_path: &str) -> anyhow::Result<()> {
    let path = generated_project_dir(project_name)?.join(relative_path);
    if path.exists() {
        fs::remove_file(path).context("can't remove file")?;
    }
    Ok(())
}

fn resolve_generated_project_name(
    config_path: &Path,
    override_name: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(name) = override_name {
        validate_name(name, "server")?;
        return Ok(name.to_owned());
    }

    let file_stem = config_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("config filename is not valid UTF-8")?;
    validate_name(file_stem, "server").with_context(|| {
        format!(
            "invalid generated server name '{file_stem}' from config file {}; use --name to override",
            config_path.display()
        )
    })?;

    Ok(file_stem.to_owned())
}

fn prepare_cargo_server_main(dependencies: &CargoTomlDependencies) -> String {
    use std::fmt::Write;

    let dependencies = dependencies.keys().fold(String::new(), |mut acc, name| {
        let rust_name = name.replace('-', "_");
        _ = writeln!(acc, "use {rust_name} as _;");
        acc
    });

    CARGO_SERVER_MAIN.replace("{{dependencies}}", &dependencies)
}

#[cfg(test)]
mod tests {
    use super::{merge_module_metadata, prepare_cargo_server_main, resolve_generated_project_name};
    use module_parser::{Capability, CargoTomlDependencies, ConfigModuleMetadata};
    use std::path::Path;

    #[test]
    fn merge_module_metadata_preserves_config_overrides() {
        let config_metadata = ConfigModuleMetadata {
            package: None,
            version: None,
            features: vec!["grpc".to_owned(), "otel".to_owned()],
            default_features: Some(false),
            path: Some("modules/custom-path".to_owned()),
            deps: vec![],
            capabilities: vec![],
        };
        let local_metadata = ConfigModuleMetadata {
            package: Some("cf-demo".to_owned()),
            version: Some("0.5.0".to_owned()),
            features: vec![],
            default_features: None,
            path: Some("modules/demo".to_owned()),
            deps: vec!["authz".to_owned()],
            capabilities: vec![Capability::Grpc],
        };

        let merged = merge_module_metadata(config_metadata, local_metadata);
        assert_eq!(merged.package.as_deref(), Some("cf-demo"));
        assert_eq!(merged.version.as_deref(), Some("0.5.0"));
        assert_eq!(merged.features, vec!["grpc", "otel"]);
        assert_eq!(merged.default_features, Some(false));
        assert_eq!(merged.path.as_deref(), Some("modules/custom-path"));
        assert_eq!(merged.deps, vec!["authz"]);
        assert_eq!(merged.capabilities, vec![Capability::Grpc]);
    }

    #[test]
    fn generated_project_name_defaults_to_config_file_stem() {
        let name = resolve_generated_project_name(Path::new("/tmp/quickstart.yml"), None)
            .expect("config stem should resolve to a project name");

        assert_eq!(name, "quickstart");
    }

    #[test]
    fn generated_project_name_prefers_explicit_override() {
        let name = resolve_generated_project_name(Path::new("/tmp/quickstart.yml"), Some("demo"))
            .expect("explicit override should resolve to a project name");

        assert_eq!(name, "demo");
    }

    #[test]
    fn generated_server_main_reads_config_from_env_and_includes_dependencies() {
        let dependencies = CargoTomlDependencies::from([
            ("module_a".to_owned(), Default::default()),
            ("module_b".to_owned(), Default::default()),
            ("api-db-handler".to_owned(), Default::default()),
        ]);

        let main_rs = prepare_cargo_server_main(&dependencies);

        assert!(main_rs.contains("std::env::var_os(\"CF_CLI_CONFIG\")"));
        assert!(main_rs.contains("use module_a as _;"));
        assert!(main_rs.contains("use module_b as _;"));
        assert!(main_rs.contains("use api_db_handler as _;"));
        assert!(!main_rs.contains("use api-db-handler as _;"));
        assert!(!main_rs.contains("{{dependencies}}"));
    }
}
