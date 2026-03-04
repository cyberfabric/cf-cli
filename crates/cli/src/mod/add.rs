use anyhow::{Context, bail};
use cargo_generate::{GenerateArgs, TemplatePath, generate};
use clap::{Args, ValueEnum};
use module_parser::{CargoTomlDependencies, CargoTomlDependency};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct AddArgs {
    /// Module template and module name to generate
    #[arg(value_enum)]
    name: ModuleTemplateName,
    /// Path to the workspace root (defaults to current directory)
    #[arg(short = 'p', long, default_value = ".")]
    path: PathBuf,
    /// Verbose output
    #[arg(short = 'v', long)]
    verbose: bool,
    /// Path to a local template (instead of git)
    #[arg(long, conflicts_with_all = ["git", "branch"])]
    local_path: Option<String>,
    /// URL to the git repo
    #[arg(
        long,
        default_value = "https://github.com/cyberfabric/cf-template-rust"
    )]
    git: Option<String>,
    /// Subfolder relative to the git repo
    #[arg(long, default_value = "Modules")]
    subfolder: String,
    /// Branch of the git repo
    #[arg(long, default_value = "main")]
    branch: Option<String>,
}

#[derive(Clone, Debug, ValueEnum)]
enum ModuleTemplateName {
    #[value(name = "background-worker")]
    BackgroundWorker,
    #[value(name = "api-db-handler")]
    ApiDbHandler,
    #[value(name = "rest-gateway")]
    RestGateway,
}

impl ModuleTemplateName {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::BackgroundWorker => "background-worker",
            Self::ApiDbHandler => "api-db-handler",
            Self::RestGateway => "rest-gateway",
        }
    }
}

impl AddArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let modules_dir = self.path.join("modules");

        if !modules_dir.exists() {
            bail!(
                "modules directory does not exist at {}. Make sure you are in a workspace initialized with 'init'.",
                modules_dir.display()
            );
        }

        let mut doc = get_cargo_toml(&self.path)?;

        // Generate the main module
        let (modules, dependencies) = self.generate_module()?;
        println!("Modules {modules:?} created");

        add_modules_to_workspace(&mut doc, modules)?;
        add_dependencies_to_workspace(&mut doc, dependencies)?;

        let cargo_toml_path = self.path.join("Cargo.toml");
        fs::write(&cargo_toml_path, doc.to_string()).context("can't write Cargo.toml")?;

        Ok(())
    }

    fn generate_module(&self) -> anyhow::Result<(Vec<String>, CargoTomlDependencies)> {
        let module_name = self.name.as_str();
        let modules_path = self.path.join("modules");
        let module_path = modules_path.join(module_name);
        if module_path.exists() {
            bail!("module {module_name} already exists");
        }

        let (git, branch) = if self.local_path.is_some() {
            (None, None)
        } else {
            (self.git.clone(), self.branch.clone())
        };

        let auto_path = format!("{}/{}", self.subfolder, module_name);

        generate(GenerateArgs {
            template_path: TemplatePath {
                auto_path: Some(auto_path),
                git,
                path: self.local_path.clone(),
                branch,
                ..TemplatePath::default()
            },
            destination: Some(modules_path),
            name: Some(module_name.to_string()),
            quiet: !self.verbose,
            verbose: self.verbose,
            no_workspace: true,
            ..GenerateArgs::default()
        })
        .with_context(|| format!("can't generate module '{module_name}'"))?;

        let mut dependencies = get_cargo_toml(&module_path).map(|x| get_dependencies(&x))?;

        let mut generated = vec![format!("modules/{}", module_name)];

        let sdk_template = module_path.join("sdk");
        if sdk_template.exists() {
            generated.push(format!("modules/{module_name}/sdk"));
            dependencies.extend(get_cargo_toml(&sdk_template).map(|x| get_dependencies(&x))?);
        }

        Ok((generated, dependencies))
    }
}

fn get_cargo_toml(path: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let cargo_toml_path = path.join("Cargo.toml");
    fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("can't read {}", path.display()))?
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("can't parse {}", path.display()))
}

fn get_dependencies(doc: &toml_edit::DocumentMut) -> CargoTomlDependencies {
    let mut result = CargoTomlDependencies::new();
    let Some(dependencies) = doc["dependencies"].as_table() else {
        return result;
    };

    for (name, value) in dependencies {
        let metadata = if let Some(dep) = value.as_str() {
            // Simple string version: `package = "1.0"`
            CargoTomlDependency {
                package: Some(name.to_string()),
                version: Some(dep.to_string()),
                ..Default::default()
            }
        } else {
            // Table or inline table: `package = { version = "1.0", ... }`
            let (package, version, pkg, features, default_features) =
                if let Some(table) = value.as_table() {
                    let features = table
                        .get("features")
                        .and_then(|f| f.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let default_features = table
                        .get("default-features")
                        .or_else(|| table.get("default_features"))
                        .and_then(toml_edit::Item::as_bool);

                    (
                        table.get("package").and_then(|p| p.as_str()),
                        table.get("version").and_then(|v| v.as_str()),
                        table.get("path").and_then(|p| p.as_str()),
                        features,
                        default_features,
                    )
                } else if let Some(inline) = value.as_inline_table() {
                    let features = inline
                        .get("features")
                        .and_then(|f| f.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let default_features = inline
                        .get("default-features")
                        .or_else(|| inline.get("default_features"))
                        .and_then(toml_edit::Value::as_bool);

                    (
                        inline.get("package").and_then(|p| p.as_str()),
                        inline.get("version").and_then(|v| v.as_str()),
                        inline.get("path").and_then(|p| p.as_str()),
                        features,
                        default_features,
                    )
                } else {
                    continue;
                };

            CargoTomlDependency {
                package: package.map(String::from),
                version: version.map(String::from),
                path: pkg.map(String::from),
                features,
                default_features,
            }
        };
        result.insert(name.to_string(), metadata);
    }

    result
}

fn add_modules_to_workspace(
    doc: &mut toml_edit::DocumentMut,
    modules: Vec<String>,
) -> anyhow::Result<()> {
    let members = doc["workspace"]["members"]
        .as_array_mut()
        .context("workspace.members is not an array")?;
    for m in modules {
        let s = m.as_str();
        if !members
            .iter()
            .any(|x| matches!(x.as_str(), Some(inner) if inner == s))
        {
            members.push(m);
        }
    }
    Ok(())
}

fn add_dependencies_to_workspace(
    doc: &mut toml_edit::DocumentMut,
    dependencies: CargoTomlDependencies,
) -> anyhow::Result<()> {
    let workspace_deps = doc["workspace"]["dependencies"]
        .or_insert(toml_edit::table())
        .as_table_mut()
        .context("workspace.dependencies is not a table")?;

    for (name, metadata) in dependencies {
        if workspace_deps.contains_key(&name) {
            continue;
        }
        let mut dep_table = toml_edit::InlineTable::new();

        if let Some(package) = metadata.package {
            dep_table.insert("package", package.into());
        }

        if let Some(version) = metadata.version {
            dep_table.insert("version", version.into());
        } else {
            dep_table.insert("version", "*".into());
        }

        if let Some(default_features) = metadata.default_features {
            dep_table.insert("default-features", default_features.into());
        }

        if !metadata.features.is_empty() {
            let features_array: toml_edit::Array = metadata
                .features
                .into_iter()
                .map(toml_edit::Value::from)
                .collect();
            dep_table.insert("features", toml_edit::Value::Array(features_array));
        }

        if let Some(path) = metadata.path {
            dep_table.insert("path", path.into());
        }

        workspace_deps.insert(&name, toml_edit::Item::Value(dep_table.into()));
    }

    Ok(())
}
