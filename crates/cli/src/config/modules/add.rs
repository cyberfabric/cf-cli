use super::{
    ModulesContext, load_config, resolve_modules_context, save_config, validate_module_name,
};
use crate::common::PathConfigArgs;
use crate::config::app_config::ModuleConfig;
use anyhow::{Context, bail};
use clap::Args;
use module_parser::{ConfigModule, ConfigModuleMetadata, get_module_name_from_crate};
use std::collections::HashMap;

#[derive(Args)]
pub struct AddArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Module name
    module: String,
    /// Module package name for metadata
    #[arg(long)]
    package: Option<String>,
    /// Module package version for metadata
    #[arg(long = "module-version")]
    module_version: Option<String>,
    /// Whether Cargo default features should be enabled
    #[arg(long)]
    default_features: Option<bool>,
    /// Feature to include in metadata (repeatable)
    #[arg(long = "feature")]
    features: Vec<String>,
    /// Dependency name to include in metadata.deps (repeatable)
    #[arg(long = "dep")]
    deps: Vec<String>,
}

impl AddArgs {
    pub(super) fn run(&self) -> anyhow::Result<()> {
        validate_module_name(&self.module)?;
        let context = resolve_modules_context(&self.path_config)?;

        let mut config = load_config(&context.config_path)?;
        if config.modules.contains_key(&self.module) {
            let module = &self.module;
            bail!("module '{module}' already exists in modules section");
        }

        let local_modules = discover_local_modules(&context, self)?;
        let metadata = build_required_metadata(self, local_modules.get(&self.module))?;

        config.modules.insert(
            self.module.clone(),
            ModuleConfig {
                metadata: Some(metadata),
                ..ModuleConfig::default()
            },
        );

        save_config(&context.config_path, &config)
    }
}

fn discover_local_modules(
    context: &ModulesContext,
    args: &AddArgs,
) -> anyhow::Result<HashMap<String, ConfigModule>> {
    match get_module_name_from_crate(&context.workspace_path) {
        Ok(modules) => Ok(modules),
        Err(_) if args.package.is_some() && args.module_version.is_some() => {
            // Allow remote module additions even if the provided -p path is not a Cargo workspace.
            Ok(HashMap::new())
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to discover local modules at {}. \
                 if this is a remote module, provide both --package and --module-version",
                context.workspace_path.display()
            )
        }),
    }
}

fn build_required_metadata(
    args: &AddArgs,
    local_module: Option<&ConfigModule>,
) -> anyhow::Result<ConfigModuleMetadata> {
    let mut metadata = local_module.map_or_else(ConfigModuleMetadata::default, |module| {
        module.metadata.clone()
    });

    if let Some(package) = &args.package {
        metadata.package = Some(package.clone());
    }
    if let Some(version) = &args.module_version {
        metadata.version = Some(version.clone());
    }
    if let Some(default_features) = args.default_features {
        metadata.default_features = Some(default_features);
    }
    // Keep config portable: do not persist local filesystem paths in metadata.
    metadata.path = None;
    if !args.features.is_empty() {
        metadata.features.clone_from(&args.features);
    }
    if !args.deps.is_empty() {
        metadata.deps.clone_from(&args.deps);
    }

    validate_required_metadata(args, local_module.is_some(), &metadata)?;
    Ok(metadata)
}

fn validate_required_metadata(
    args: &AddArgs,
    is_local: bool,
    metadata: &ConfigModuleMetadata,
) -> anyhow::Result<()> {
    let package_missing = metadata
        .package
        .as_deref()
        .is_none_or(|package| package.trim().is_empty());
    let version_missing = metadata
        .version
        .as_deref()
        .is_none_or(|version| version.trim().is_empty());

    if !package_missing && !version_missing {
        return Ok(());
    }

    let module = &args.module;
    if is_local {
        bail!("module '{module}' is local, but metadata.package and metadata.version are required");
    }
    bail!("module '{module}' is remote, provide both --package and --module-version");
}

#[cfg(test)]
mod tests {
    use super::{AddArgs, build_required_metadata};
    use crate::common::PathConfigArgs;
    use module_parser::{ConfigModule, ConfigModuleMetadata};
    use std::path::PathBuf;

    #[test]
    fn build_required_metadata_uses_local_package_and_version() {
        let args = AddArgs {
            path_config: PathConfigArgs {
                path: PathBuf::from("."),
                config: None,
            },
            module: "demo".to_owned(),
            package: None,
            module_version: None,
            default_features: Some(false),
            features: vec!["foo".to_owned(), "bar".to_owned()],
            deps: vec!["authz".to_owned()],
        };
        let local_module = ConfigModule {
            metadata: ConfigModuleMetadata {
                package: Some("cf-demo-local".to_owned()),
                version: Some("0.3.0".to_owned()),
                deps: vec!["tenant-resolver".to_owned()],
                ..ConfigModuleMetadata::default()
            },
        };

        let metadata = build_required_metadata(&args, Some(&local_module)).expect("metadata");
        assert_eq!(metadata.package.as_deref(), Some("cf-demo-local"));
        assert_eq!(metadata.version.as_deref(), Some("0.3.0"));
        assert_eq!(metadata.default_features, Some(false));
        assert_eq!(metadata.path, None);
        assert_eq!(metadata.features, vec!["foo", "bar"]);
        assert_eq!(metadata.deps, vec!["authz"]);
    }

    #[test]
    fn build_required_metadata_requires_remote_package() {
        let args = AddArgs {
            path_config: PathConfigArgs {
                path: PathBuf::from("."),
                config: None,
            },
            module: "demo".to_owned(),
            package: None,
            module_version: Some("1.2.3".to_owned()),
            default_features: None,
            features: vec![],
            deps: vec![],
        };

        let err = match build_required_metadata(&args, None) {
            Ok(_) => panic!("should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("remote, provide both --package and --module-version")
        );
    }

    #[test]
    fn build_required_metadata_requires_remote_version() {
        let args = AddArgs {
            path_config: PathConfigArgs {
                path: PathBuf::from("."),
                config: None,
            },
            module: "demo".to_owned(),
            package: Some("cf-demo".to_owned()),
            module_version: None,
            default_features: None,
            features: vec![],
            deps: vec![],
        };

        let err = match build_required_metadata(&args, None) {
            Ok(_) => panic!("should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("remote, provide both --package and --module-version")
        );
    }

    #[test]
    fn build_required_metadata_accepts_remote_with_package_and_version() {
        let args = AddArgs {
            path_config: PathConfigArgs {
                path: PathBuf::from("."),
                config: None,
            },
            module: "demo".to_owned(),
            package: Some("cf-demo".to_owned()),
            module_version: Some("1.2.3".to_owned()),
            default_features: None,
            features: vec![],
            deps: vec![],
        };

        let metadata = build_required_metadata(&args, None).expect("metadata");
        assert_eq!(metadata.package.as_deref(), Some("cf-demo"));
        assert_eq!(metadata.version.as_deref(), Some("1.2.3"));
    }
}
