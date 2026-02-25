use anyhow::{Context, bail};
use cargo_generate::{GenerateArgs, TemplatePath, generate};
use clap::Args;
use module_parser::CargoTomlDependencies;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct ModuleArgs {
    /// Name of the new module to create (e.g., "goodbye")
    name: String,
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

impl ModuleArgs {
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
        let modules_path = self.path.join("modules");
        let module_path = modules_path.join(&self.name);
        if module_path.exists() {
            bail!("module {} already exists", self.name);
        }

        let (git, auto_path, branch) = if self.local_path.is_some() {
            (None, None, None)
        } else {
            (
                self.git.clone(),
                Some(self.subfolder.clone()),
                self.branch.clone(),
            )
        };

        let local_path = self.local_path.as_ref().map(|p| {
            PathBuf::from(p)
                .join(&self.subfolder)
                .to_string_lossy()
                .to_string()
        });

        generate(GenerateArgs {
            template_path: TemplatePath {
                auto_path,
                git,
                path: local_path,
                branch,
                ..TemplatePath::default()
            },
            destination: Some(modules_path.clone()),
            name: Some(self.name.clone()),
            quiet: !self.verbose,
            verbose: self.verbose,
            no_workspace: true,
            ..GenerateArgs::default()
        })
        .with_context(|| format!("can't generate module '{}'", self.name))?;

        let mut dependencies = get_cargo_toml(&module_path).and_then(|x| get_dependencies(&x))?;

        let mut generated = vec![format!("modules/{}", self.name)];

        let sdk_template = module_path.join("sdk");
        if sdk_template.exists() {
            let name = format!("{}-sdk", self.name);
            generated.push(format!("modules/{name}"));
            generate(GenerateArgs {
                template_path: TemplatePath {
                    path: Some(sdk_template.to_string_lossy().to_string()),
                    ..TemplatePath::default()
                },
                destination: Some(modules_path),
                name: Some(name),
                quiet: !self.verbose,
                verbose: self.verbose,
                no_workspace: true,
                ..GenerateArgs::default()
            })
            .with_context(|| format!("can't generate sdk module '{}-sdk'", self.name))?;
            dependencies.extend(get_cargo_toml(&sdk_template).and_then(|x| get_dependencies(&x))?);
            fs::remove_dir_all(sdk_template)
                .with_context(|| format!("can't remove sdk template for module '{}'", self.name))?;
        }

        Ok((generated, dependencies))
    }
}

fn get_cargo_toml(path: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let cargo_toml_path = path.join("Cargo.toml");
    fs::read_to_string(&cargo_toml_path)
        .context("can't read workspace Cargo.toml")?
        .parse::<toml_edit::DocumentMut>()
        .context("can't parse workspace Cargo.toml")
}

fn get_dependencies(doc: &toml_edit::DocumentMut) -> anyhow::Result<CargoTomlDependencies> {
    let dependencies = doc
        .get("dependencies")
        .and_then(|d| d.as_table())
        .context("dependencies section not found or not a table")?;

    let mut result = CargoTomlDependencies::new();

    for (name, value) in dependencies {
        let metadata = if let Some(dep) = value.as_str() {
            // Simple string version: `package = "1.0"`
            module_parser::ConfigModuleMetadata {
                package: Some(name.to_string()),
                version: Some(dep.to_string()),
                ..Default::default()
            }
        } else {
            // Table or inline table: `package = { version = "1.0", ... }`
            let (package, version) = if let Some(table) = value.as_table() {
                (
                    table.get("package").and_then(|p| p.as_str()),
                    table.get("version").and_then(|v| v.as_str()),
                )
            } else if let Some(inline) = value.as_inline_table() {
                (
                    inline.get("package").and_then(|p| p.as_str()),
                    inline.get("version").and_then(|v| v.as_str()),
                )
            } else {
                continue;
            };

            module_parser::ConfigModuleMetadata {
                package: package.map(String::from),
                version: version.map(String::from),
                ..Default::default()
            }
        };
        result.insert(name.to_string(), metadata);
    }

    Ok(result)
}

fn add_modules_to_workspace(
    doc: &mut toml_edit::DocumentMut,
    modules: Vec<String>,
) -> anyhow::Result<()> {
    let members = doc["workspace"]["members"]
        .as_array_mut()
        .context("workspace.members is not an array")?;
    members.extend(modules);
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

        if !metadata.features.is_empty() {
            let features_array: toml_edit::Array = metadata
                .features
                .into_iter()
                .map(toml_edit::Value::from)
                .collect();
            dep_table.insert("features", toml_edit::Value::Array(features_array));
        }

        workspace_deps.insert(&name, toml_edit::Item::Value(dep_table.into()));
    }

    Ok(())
}
