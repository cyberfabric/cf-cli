use super::{SYSTEM_REGISTRY_MODULES, SystemRegistryModule, load_config, resolve_modules_context};
use crate::common::PathConfigArgs;
use anyhow::{Context, bail};
use clap::Args;
use flate2::read::GzDecoder;
use module_parser::{Capability, ConfigModule, get_module_name_from_crate, parse_module_rs_source};
use reqwest::Client;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Args)]
pub struct ListArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Verbose output. Fetches registry metadata for system crates.
    #[arg(short = 'v', long)]
    verbose: bool,
    /// Registry to query when verbose mode is enabled.
    #[arg(long, default_value = "crates.io")]
    registry: String,
}

impl ListArgs {
    pub(super) fn run(&self) -> anyhow::Result<()> {
        let context = resolve_modules_context(&self.path_config)?;
        let local_modules = discover_workspace_modules(&context.workspace_path)?;
        let config = load_config(&context.config_path)?;
        let enabled_modules: BTreeSet<_> = config.modules.keys().map(String::as_str).collect();

        println!("System crates:");
        if self.verbose {
            if self.registry != "crates.io" {
                let registry = &self.registry;
                bail!("unsupported registry '{registry}'. Only 'crates.io' is currently supported");
            }

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime for registry queries")?;

            let metadata_by_crate = runtime.block_on(fetch_all_crates_io_metadata())?;

            for module in SYSTEM_REGISTRY_MODULES {
                let Some(metadata) = metadata_by_crate.get(module.crate_name) else {
                    bail!("missing fetched metadata for '{}'", module.crate_name);
                };

                println!("  - {}", module.module_name);
                println!("      crate: {}", module.crate_name);
                println!("      latest_version: {}", metadata.latest_version);

                if metadata.features.is_empty() {
                    println!("      features: (none)");
                } else {
                    println!("      features:");
                    for feature in &metadata.features {
                        println!("        - {feature}");
                    }
                }

                if metadata.deps.is_empty() {
                    println!("      deps: (none)");
                } else {
                    println!("      deps:");
                    for dep in &metadata.deps {
                        println!("        - {dep}");
                    }
                }

                if metadata.capabilities.is_empty() {
                    println!("      capabilities: (none)");
                } else {
                    println!("      capabilities:");
                    for capability in &metadata.capabilities {
                        println!("        - {capability}");
                    }
                }
            }
        } else {
            for module in SYSTEM_REGISTRY_MODULES {
                println!("  - {}", module.module_name);
            }
        }

        println!();
        println!("Workspace modules ({}):", context.workspace_path.display());
        if local_modules.is_empty() {
            println!("  (none)");
        } else {
            let mut local_entries: Vec<_> = local_modules.iter().collect();
            local_entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
            for (module_name, module) in local_entries {
                let enabled_label = if enabled_modules.contains(module_name.as_str()) {
                    " (enabled in config)"
                } else {
                    ""
                };
                println!("  - {module_name}{enabled_label}");

                if self.verbose {
                    print_local_metadata(module);
                }
            }
        }

        println!();
        println!(
            "Modules enabled in config ({}):",
            context.config_path.display()
        );
        if config.modules.is_empty() {
            println!("  (none)");
        } else {
            let mut configured_entries: Vec<_> = config.modules.iter().collect();
            configured_entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
            for (module_name, module) in configured_entries {
                let location_label = if local_modules.contains_key(module_name.as_str()) {
                    " (local workspace)"
                } else {
                    " (not found in workspace)"
                };
                println!("  - {module_name}{location_label}");

                if self.verbose {
                    print_config_metadata(module);
                }
            }
        }

        Ok(())
    }
}

fn discover_workspace_modules(
    workspace_path: &Path,
) -> anyhow::Result<HashMap<String, ConfigModule>> {
    let workspace_buf = PathBuf::from(workspace_path);
    get_module_name_from_crate(&workspace_buf).with_context(|| {
        format!(
            "failed to discover workspace modules at {}",
            workspace_path.display()
        )
    })
}

fn print_local_metadata(module: &ConfigModule) {
    let metadata = &module.metadata;
    if let Some(package) = &metadata.package {
        println!("      package: {package}");
    }
    if let Some(version) = &metadata.version {
        println!("      version: {version}");
    }
    if let Some(path) = &metadata.path {
        println!("      path: {path}");
    }

    if metadata.deps.is_empty() {
        println!("      deps: (none)");
    } else {
        println!("      deps:");
        for dep in &metadata.deps {
            println!("        - {dep}");
        }
    }

    if metadata.capabilities.is_empty() {
        println!("      capabilities: (none)");
    } else {
        println!("      capabilities:");
        for capability in &metadata.capabilities {
            println!("        - {capability}");
        }
    }
}

fn print_config_metadata(module: &crate::config::app_config::ModuleConfig) {
    let Some(metadata) = &module.metadata else {
        println!("      metadata: (none)");
        return;
    };

    if let Some(package) = &metadata.package {
        println!("      package: {package}");
    }
    if let Some(version) = &metadata.version {
        println!("      version: {version}");
    }
    if let Some(path) = &metadata.path {
        println!("      path: {path}");
    }

    if metadata.features.is_empty() {
        println!("      features: (none)");
    } else {
        println!("      features:");
        for feature in &metadata.features {
            println!("        - {feature}");
        }
    }

    if metadata.deps.is_empty() {
        println!("      deps: (none)");
    } else {
        println!("      deps:");
        for dep in &metadata.deps {
            println!("        - {dep}");
        }
    }
}

#[derive(Default)]
struct RegistryMetadata {
    latest_version: String,
    features: Vec<String>,
    deps: Vec<String>,
    capabilities: Vec<Capability>,
}

#[derive(Deserialize)]
struct CrateResponse {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
    versions: Vec<CrateVersion>,
}

#[derive(Deserialize)]
struct CrateInfo {
    max_version: String,
}

#[derive(Deserialize)]
struct CrateVersion {
    num: String,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
}

async fn fetch_all_crates_io_metadata() -> anyhow::Result<HashMap<&'static str, RegistryMetadata>> {
    let client = Client::builder()
        .user_agent("cyberfabric-cli")
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to create registry HTTP client")?;

    let mut join_set = tokio::task::JoinSet::new();
    for module in SYSTEM_REGISTRY_MODULES.iter().copied() {
        let cloned_client = client.clone();
        join_set.spawn(async move {
            let metadata = fetch_crates_io_metadata(&cloned_client, module)
                .await
                .with_context(|| format!("failed to fetch metadata for '{}'", module.crate_name))?;
            Ok::<_, anyhow::Error>((module.crate_name, metadata))
        });
    }

    let mut metadata_by_crate = HashMap::new();
    while let Some(task_result) = join_set.join_next().await {
        let (crate_name, metadata) = task_result.context("registry task panicked")??;
        metadata_by_crate.insert(crate_name, metadata);
    }

    Ok(metadata_by_crate)
}

async fn fetch_crates_io_metadata(
    client: &Client,
    module: SystemRegistryModule,
) -> anyhow::Result<RegistryMetadata> {
    let crate_url = format!("https://crates.io/api/v1/crates/{}", module.crate_name);
    let crate_response = client
        .get(&crate_url)
        .send()
        .await
        .with_context(|| format!("request failed for {}", module.crate_name))?
        .error_for_status()
        .with_context(|| format!("registry returned an error for {}", module.crate_name))?
        .json::<CrateResponse>()
        .await
        .with_context(|| format!("invalid crate metadata for {}", module.crate_name))?;

    let latest_version = crate_response.crate_info.max_version;
    let features = crate_response
        .versions
        .into_iter()
        .find(|version| version.num == latest_version)
        .map_or_else(Vec::new, |version| version.features.into_keys().collect());

    let module_rs_content = fetch_module_rs_content(client, module, &latest_version).await?;
    let module_metadata = parse_module_rs_source(&module_rs_content)
        .with_context(|| format!("invalid src/module.rs for {}", module.crate_name))?;

    Ok(RegistryMetadata {
        latest_version,
        features,
        deps: module_metadata.deps,
        capabilities: module_metadata.capabilities,
    })
}

async fn fetch_module_rs_content(
    client: &Client,
    module: SystemRegistryModule,
    latest_version: &str,
) -> anyhow::Result<String> {
    let download_url = format!(
        "https://crates.io/api/v1/crates/{}/{}/download",
        module.crate_name, latest_version
    );
    let crate_archive = client
        .get(&download_url)
        .send()
        .await
        .with_context(|| format!("download request failed for {}", module.crate_name))?
        .error_for_status()
        .with_context(|| {
            format!(
                "download endpoint returned an error for {}",
                module.crate_name
            )
        })?
        .bytes()
        .await
        .with_context(|| format!("failed to read downloaded source for {}", module.crate_name))?;

    extract_module_rs(crate_archive.as_ref())
        .with_context(|| format!("failed to extract src/module.rs for {}", module.crate_name))
}

fn extract_module_rs(crate_archive: &[u8]) -> anyhow::Result<String> {
    let decoder = GzDecoder::new(Cursor::new(crate_archive));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .context("failed to list crate archive entries")?;

    for entry in entries {
        let mut entry = entry.context("failed to read crate archive entry")?;
        let path = entry
            .path()
            .context("failed to read crate archive entry path")?;
        if path.ends_with(Path::new("src/module.rs")) {
            let mut module_rs = String::new();
            entry
                .read_to_string(&mut module_rs)
                .context("failed to read src/module.rs from crate archive")?;
            return Ok(module_rs);
        }
    }

    bail!("crate archive does not contain src/module.rs")
}
