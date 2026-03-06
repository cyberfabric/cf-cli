use anyhow::{Context, bail};
use cargo_generate::{GenerateArgs, TemplatePath, generate};
use clap::{Args, ValueEnum};
use module_parser::{CargoTomlDependencies, CargoTomlDependency};
use semver::{Comparator, Op, Version, VersionReq};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
        ensure_modules_directory(&self.path)?;

        let generated_modules = self.generate_module()?;
        println!("Modules {generated_modules:?} created");

        let dependencies = prepare_generated_modules(&self.path, &generated_modules)?;
        update_workspace_cargo_toml(&self.path, &generated_modules, dependencies)
    }

    fn generate_module(&self) -> anyhow::Result<Vec<String>> {
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

        let mut generated = vec![format!("modules/{module_name}")];

        let sdk_template = module_path.join("sdk");
        if sdk_template.exists() {
            generated.push(format!("modules/{module_name}/sdk"));
        }

        Ok(generated)
    }
}

fn ensure_modules_directory(workspace_root: &Path) -> anyhow::Result<()> {
    let modules_dir = workspace_root.join("modules");
    if modules_dir.exists() {
        return Ok(());
    }

    bail!(
        "modules directory does not exist at {}. Make sure you are in a workspace initialized with 'init'.",
        modules_dir.display()
    );
}

fn prepare_generated_modules(
    workspace_root: &Path,
    generated_modules: &[String],
) -> anyhow::Result<CargoTomlDependencies> {
    let mut dependencies = CargoTomlDependencies::new();

    for module in generated_modules {
        let module_path = workspace_root.join(module);
        let mut doc = get_cargo_toml(&module_path)?;
        dependencies.extend(get_dependencies(&doc, &module_path, workspace_root));

        let mut changed = rewrite_dependencies_to_workspace_inheritance(&mut doc);
        changed |= ensure_workspace_lints_inheritance(&mut doc);
        if changed {
            let cargo_toml_path = module_path.join("Cargo.toml");
            save_toml_document(&cargo_toml_path, &doc)?;
        }
    }

    Ok(dependencies)
}

fn update_workspace_cargo_toml(
    workspace_root: &Path,
    generated_modules: &[String],
    dependencies: CargoTomlDependencies,
) -> anyhow::Result<()> {
    let mut workspace_doc = get_cargo_toml(workspace_root)?;
    add_modules_to_workspace(&mut workspace_doc, generated_modules)?;
    add_dependencies_to_workspace(&mut workspace_doc, dependencies)?;
    let cargo_toml_path = workspace_root.join("Cargo.toml");
    save_toml_document(&cargo_toml_path, &workspace_doc)
}

fn get_cargo_toml(path: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let cargo_toml_path = path.join("Cargo.toml");
    fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("can't read {}", path.display()))?
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("can't parse {}", path.display()))
}

fn get_dependencies(
    doc: &toml_edit::DocumentMut,
    source_crate_dir: &Path,
    workspace_root: &Path,
) -> CargoTomlDependencies {
    let mut result = CargoTomlDependencies::new();
    let Some(dependencies) = doc["dependencies"].as_table() else {
        return result;
    };

    for (name, value) in dependencies {
        let metadata = if let Some(dep) = value.as_str() {
            // Simple string version: `package = "1.0"`
            CargoTomlDependency {
                package: None,
                version: Some(dep.to_string()),
                ..Default::default()
            }
        } else {
            // Table or inline table: `package = { version = "1.0", ... }`
            let (package, version, pkg, default_features) = if let Some(table) = value.as_table() {
                (
                    table.get("package").and_then(|p| p.as_str()),
                    table.get("version").and_then(|v| v.as_str()),
                    table.get("path").and_then(|p| p.as_str()),
                    table
                        .get("default-features")
                        .and_then(toml_edit::Item::as_bool),
                )
            } else if let Some(inline) = value.as_inline_table() {
                (
                    inline.get("package").and_then(|p| p.as_str()),
                    inline.get("version").and_then(|v| v.as_str()),
                    inline.get("path").and_then(|p| p.as_str()),
                    inline
                        .get("default-features")
                        .and_then(toml_edit::Value::as_bool),
                )
            } else {
                continue;
            };

            CargoTomlDependency {
                package: normalize_workspace_package_name(name, package),
                version: version.map(String::from),
                path: pkg.map(|path| {
                    workspace_relative_dependency_path(path, source_crate_dir, workspace_root)
                        .unwrap_or_else(|| path.to_owned())
                }),
                default_features,
                ..Default::default()
            }
        };
        result.insert(name.to_string(), metadata);
    }

    result
}

fn normalize_workspace_package_name(dep_name: &str, package: Option<&str>) -> Option<String> {
    package
        .map(str::trim)
        .filter(|package| !package.is_empty() && *package != dep_name)
        .map(ToOwned::to_owned)
}

fn workspace_relative_dependency_path(
    raw_path: &str,
    source_crate_dir: &Path,
    workspace_root: &Path,
) -> Option<String> {
    let raw = Path::new(raw_path);
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        source_crate_dir.join(raw)
    };

    let normalized = normalize_path(&absolute);
    let relative = normalized.strip_prefix(workspace_root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        }
    }

    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn add_modules_to_workspace(
    doc: &mut toml_edit::DocumentMut,
    modules: &[String],
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
            members.push(m.clone());
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
        if let Some(existing_dep) = workspace_deps.get_mut(&name) {
            maybe_upgrade_workspace_dep_version(existing_dep, metadata.version.as_deref());
            maybe_apply_workspace_dep_default_features(existing_dep, metadata.default_features);
            continue;
        }
        workspace_deps.insert(
            &name,
            toml_edit::Item::Value(build_workspace_dep_inline_table(metadata).into()),
        );
    }

    Ok(())
}

fn build_workspace_dep_inline_table(metadata: CargoTomlDependency) -> toml_edit::InlineTable {
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

    if let Some(path) = metadata.path {
        dep_table.insert("path", path.into());
    }

    dep_table
}

fn maybe_upgrade_workspace_dep_version(existing_dep: &mut toml_edit::Item, incoming: Option<&str>) {
    let Some(incoming_version) = incoming.map(str::trim).filter(|v| !v.is_empty()) else {
        return;
    };

    let existing_version = get_workspace_dependency_version(existing_dep);
    if should_replace_with_newer_semver(existing_version.as_deref(), incoming_version) {
        set_workspace_dependency_version(existing_dep, incoming_version);
    }
}

fn maybe_apply_workspace_dep_default_features(
    existing_dep: &mut toml_edit::Item,
    incoming: Option<bool>,
) {
    let Some(default_features) = incoming else {
        return;
    };

    if let Some(table) = existing_dep.as_table_mut() {
        table.insert("default-features", toml_edit::value(default_features));
        return;
    }

    if let Some(inline) = existing_dep.as_inline_table_mut() {
        inline.insert("default-features", default_features.into());
        return;
    }

    let mut dep_table = toml_edit::InlineTable::new();
    dep_table.insert("version", existing_dep.as_str().unwrap_or("*").into());
    dep_table.insert("default-features", default_features.into());
    *existing_dep = toml_edit::Item::Value(dep_table.into());
}

fn get_workspace_dependency_version(dep: &toml_edit::Item) -> Option<String> {
    dep.as_str().map(ToOwned::to_owned).or_else(|| {
        dep.as_table()
            .and_then(|table| table.get("version"))
            .and_then(toml_edit::Item::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                dep.as_inline_table()
                    .and_then(|inline| inline.get("version"))
                    .and_then(toml_edit::Value::as_str)
                    .map(ToOwned::to_owned)
            })
    })
}

fn set_workspace_dependency_version(dep: &mut toml_edit::Item, version: &str) {
    if let Some(table) = dep.as_table_mut() {
        table.insert("version", toml_edit::value(version));
        return;
    }

    if let Some(inline) = dep.as_inline_table_mut() {
        inline.insert("version", version.into());
        return;
    }

    *dep = toml_edit::value(version);
}

fn should_replace_with_newer_semver(existing: Option<&str>, incoming: &str) -> bool {
    let incoming = incoming.trim();
    if incoming.is_empty() || incoming == "*" {
        return false;
    }

    let Some(existing) = existing.map(str::trim).filter(|v| !v.is_empty()) else {
        return true;
    };

    if existing == "*" {
        return true;
    }

    match (version_req_floor(existing), version_req_floor(incoming)) {
        (Some(existing_floor), Some(incoming_floor)) => incoming_floor > existing_floor,
        _ => false,
    }
}

fn version_req_floor(req: &str) -> Option<Version> {
    let parsed = VersionReq::parse(req).ok()?;

    parsed.comparators.iter().filter_map(comparator_floor).max()
}

fn comparator_floor(comparator: &Comparator) -> Option<Version> {
    match comparator.op {
        Op::Exact | Op::Greater | Op::GreaterEq | Op::Tilde | Op::Caret => {
            let mut version = Version::new(
                comparator.major,
                comparator.minor.unwrap_or(0),
                comparator.patch.unwrap_or(0),
            );
            version.pre = comparator.pre.clone();
            Some(version)
        }
        _ => None,
    }
}

fn save_toml_document(path: &Path, doc: &toml_edit::DocumentMut) -> anyhow::Result<()> {
    let mut serialized = doc.to_string();
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }

    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, serialized)
        .with_context(|| format!("can't write temp Cargo.toml file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| format!("can't replace {}", path.display()))
}

fn rewrite_dependencies_to_workspace_inheritance(doc: &mut toml_edit::DocumentMut) -> bool {
    let Some(dependencies) = doc["dependencies"].as_table_mut() else {
        return false;
    };

    let mut changed = false;
    for (_, dependency) in dependencies.iter_mut() {
        changed |= rewrite_dependency_to_workspace_inheritance(dependency);
    }
    changed
}

fn ensure_workspace_lints_inheritance(doc: &mut toml_edit::DocumentMut) -> bool {
    let root = doc.as_table_mut();

    let Some(lints_item) = root.get_mut("lints") else {
        let mut lints_table = toml_edit::Table::new();
        lints_table.insert("workspace", toml_edit::value(true));
        root.insert("lints", toml_edit::Item::Table(lints_table));
        return true;
    };

    if let Some(table) = lints_item.as_table_mut() {
        if table
            .get("workspace")
            .and_then(toml_edit::Item::as_bool)
            .is_some_and(|workspace| workspace)
        {
            return false;
        }
        table.insert("workspace", toml_edit::value(true));
        return true;
    }

    if let Some(inline) = lints_item.as_inline_table_mut() {
        if inline
            .get("workspace")
            .and_then(toml_edit::Value::as_bool)
            .is_some_and(|workspace| workspace)
        {
            return false;
        }
        inline.insert("workspace", true.into());
        return true;
    }

    let mut lints_table = toml_edit::Table::new();
    lints_table.insert("workspace", toml_edit::value(true));
    *lints_item = toml_edit::Item::Table(lints_table);
    true
}

fn rewrite_dependency_to_workspace_inheritance(dep: &mut toml_edit::Item) -> bool {
    if dep.as_str().is_some() {
        let mut dep_table = toml_edit::InlineTable::new();
        dep_table.insert("workspace", true.into());
        *dep = toml_edit::Item::Value(dep_table.into());
        return true;
    }

    if let Some(table) = dep.as_table_mut() {
        let mut changed = false;
        changed |= table.remove("version").is_some();
        changed |= table.remove("path").is_some();
        changed |= table.remove("default-features").is_some();
        changed |= table.remove("default_features").is_some();
        changed |= table.remove("git").is_some();
        changed |= table.remove("branch").is_some();
        changed |= table.remove("tag").is_some();
        changed |= table.remove("rev").is_some();
        if table
            .get("workspace")
            .and_then(toml_edit::Item::as_bool)
            .is_some_and(|workspace| workspace)
        {
            return changed;
        }
        table.insert("workspace", toml_edit::value(true));
        return true;
    }

    if let Some(inline) = dep.as_inline_table_mut() {
        let mut changed = false;
        changed |= inline.remove("version").is_some();
        changed |= inline.remove("path").is_some();
        changed |= inline.remove("default-features").is_some();
        changed |= inline.remove("default_features").is_some();
        changed |= inline.remove("git").is_some();
        changed |= inline.remove("branch").is_some();
        changed |= inline.remove("tag").is_some();
        changed |= inline.remove("rev").is_some();
        if inline
            .get("workspace")
            .and_then(toml_edit::Value::as_bool)
            .is_some_and(|workspace| workspace)
        {
            return changed;
        }
        inline.insert("workspace", true.into());
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::{
        add_dependencies_to_workspace, ensure_workspace_lints_inheritance, get_dependencies,
        normalize_workspace_package_name, rewrite_dependencies_to_workspace_inheritance,
        should_replace_with_newer_semver,
    };
    use module_parser::{CargoTomlDependencies, CargoTomlDependency};
    use std::path::Path;

    #[test]
    fn replaces_workspace_dep_version_with_newer_semver() {
        let mut doc = r#"
            [workspace]
            [workspace.dependencies]
            reqwest = { version = "0.12", features = ["json"] }
        "#
        .parse::<toml_edit::DocumentMut>()
        .expect("workspace cargo toml");

        let mut dependencies = CargoTomlDependencies::new();
        dependencies.insert(
            "reqwest".to_owned(),
            CargoTomlDependency {
                version: Some("0.13".to_owned()),
                features: vec!["stream".to_owned()],
                ..CargoTomlDependency::default()
            },
        );

        add_dependencies_to_workspace(&mut doc, dependencies).expect("add dependencies");

        let version = doc["workspace"]["dependencies"]["reqwest"]["version"]
            .as_str()
            .expect("reqwest version");
        assert_eq!(version, "0.13");
        let features = doc["workspace"]["dependencies"]["reqwest"]["features"]
            .as_array()
            .expect("reqwest features");
        assert_eq!(features.len(), 1);
        assert_eq!(
            features.get(0).and_then(toml_edit::Value::as_str),
            Some("json")
        );
    }

    #[test]
    fn forwards_default_features_but_not_features_for_new_workspace_dep() {
        let mut doc = r"
            [workspace]
            [workspace.dependencies]
        "
        .parse::<toml_edit::DocumentMut>()
        .expect("workspace cargo toml");

        let mut dependencies = CargoTomlDependencies::new();
        dependencies.insert(
            "reqwest".to_owned(),
            CargoTomlDependency {
                version: Some("0.13".to_owned()),
                default_features: Some(false),
                features: vec!["json".to_owned()],
                ..CargoTomlDependency::default()
            },
        );

        add_dependencies_to_workspace(&mut doc, dependencies).expect("add dependencies");

        let reqwest_dep = doc["workspace"]["dependencies"]["reqwest"]
            .as_inline_table()
            .expect("reqwest should be inline table");
        assert_eq!(
            reqwest_dep
                .get("version")
                .and_then(toml_edit::Value::as_str),
            Some("0.13")
        );
        assert_eq!(
            reqwest_dep
                .get("default-features")
                .and_then(toml_edit::Value::as_bool),
            Some(false)
        );
        assert!(reqwest_dep.get("features").is_none());
    }

    #[test]
    fn keeps_workspace_dep_version_when_incoming_is_older() {
        let mut doc = r#"
            [workspace]
            [workspace.dependencies]
            reqwest = { version = "0.13" }
        "#
        .parse::<toml_edit::DocumentMut>()
        .expect("workspace cargo toml");

        let mut dependencies = CargoTomlDependencies::new();
        dependencies.insert(
            "reqwest".to_owned(),
            CargoTomlDependency {
                version: Some("0.12".to_owned()),
                ..CargoTomlDependency::default()
            },
        );

        add_dependencies_to_workspace(&mut doc, dependencies).expect("add dependencies");

        let version = doc["workspace"]["dependencies"]["reqwest"]["version"]
            .as_str()
            .expect("reqwest version");
        assert_eq!(version, "0.13");
    }

    #[test]
    fn rewrites_module_deps_to_workspace_and_keeps_feature_flags() {
        let mut doc = r#"
            [dependencies]
            serde = { version = "1.0", path = "../deps/serde", features = ["derive"], default-features = false }
            anyhow = "1.0"
        "#
            .parse::<toml_edit::DocumentMut>()
            .expect("module cargo toml");

        let changed = rewrite_dependencies_to_workspace_inheritance(&mut doc);
        assert!(changed);

        let serde_dep = &doc["dependencies"]["serde"];
        let serde_inline = serde_dep
            .as_inline_table()
            .expect("serde should be an inline table");
        assert_eq!(
            serde_inline
                .get("workspace")
                .and_then(toml_edit::Value::as_bool),
            Some(true)
        );
        assert!(serde_inline.get("version").is_none());
        assert!(serde_inline.get("path").is_none());
        assert_eq!(
            serde_inline
                .get("default-features")
                .and_then(toml_edit::Value::as_bool),
            None
        );
        let features = serde_inline
            .get("features")
            .and_then(toml_edit::Value::as_array)
            .expect("serde features");
        assert_eq!(features.len(), 1);
        assert_eq!(
            features.get(0).and_then(toml_edit::Value::as_str),
            Some("derive")
        );

        let anyhow_dep = &doc["dependencies"]["anyhow"];
        let anyhow_inline = anyhow_dep
            .as_inline_table()
            .expect("anyhow should be an inline table");
        assert_eq!(
            anyhow_inline
                .get("workspace")
                .and_then(toml_edit::Value::as_bool),
            Some(true)
        );
        assert!(anyhow_inline.get("version").is_none());
    }

    #[test]
    fn newer_semver_detection_handles_missing_and_wildcard_versions() {
        assert!(should_replace_with_newer_semver(None, "1.2.3"));
        assert!(should_replace_with_newer_semver(Some("*"), "1.2.3"));
        assert!(!should_replace_with_newer_semver(Some("1.2.3"), "*"));
        assert!(!should_replace_with_newer_semver(Some("1.2.3"), "1.2.2"));
    }

    #[test]
    fn collects_dependencies_with_workspace_relative_paths() {
        let doc = r#"
            [dependencies]
            module_sdk = { path = "./sdk", version = "0.1.0" }
        "#
        .parse::<toml_edit::DocumentMut>()
        .expect("module cargo toml");

        let dependencies = get_dependencies(
            &doc,
            Path::new("/repo/modules/rest-gateway"),
            Path::new("/repo"),
        );

        let sdk_dep = dependencies
            .get("module_sdk")
            .expect("module_sdk dependency");
        assert_eq!(sdk_dep.path.as_deref(), Some("modules/rest-gateway/sdk"));
    }

    #[test]
    fn forwards_package_only_for_renamed_dependencies() {
        let doc = r#"
            [dependencies]
            serde = { package = "serde", version = "1.0", default-features = false }
            serde_json_alias = { package = "serde_json", version = "1.0" }
            anyhow = "1.0"
        "#
        .parse::<toml_edit::DocumentMut>()
        .expect("module cargo toml");

        let dependencies = get_dependencies(
            &doc,
            Path::new("/repo/modules/rest-gateway"),
            Path::new("/repo"),
        );

        assert_eq!(
            dependencies
                .get("serde")
                .and_then(|dep| dep.package.as_deref()),
            None
        );
        assert_eq!(
            dependencies
                .get("serde")
                .and_then(|dep| dep.default_features),
            Some(false)
        );
        assert_eq!(
            dependencies
                .get("serde_json_alias")
                .and_then(|dep| dep.package.as_deref()),
            Some("serde_json")
        );
        assert_eq!(
            dependencies
                .get("anyhow")
                .and_then(|dep| dep.package.as_deref()),
            None
        );
    }

    #[test]
    fn normalize_workspace_package_name_omits_equal_names() {
        assert_eq!(
            normalize_workspace_package_name("serde", Some("serde")),
            None
        );
        assert_eq!(
            normalize_workspace_package_name("serde_alias", Some("serde")),
            Some("serde".to_owned())
        );
    }

    #[test]
    fn adds_lints_workspace_section_when_missing() {
        let mut doc = r#"
            [package]
            name = "demo"
            version = "0.1.0"
        "#
        .parse::<toml_edit::DocumentMut>()
        .expect("module cargo toml");

        let changed = ensure_workspace_lints_inheritance(&mut doc);
        assert!(changed);
        assert_eq!(doc["lints"]["workspace"].as_bool(), Some(true));
    }

    #[test]
    fn keeps_existing_workspace_lints_unchanged() {
        let mut doc = r"
            [lints]
            workspace = true
        "
        .parse::<toml_edit::DocumentMut>()
        .expect("module cargo toml");

        let changed = ensure_workspace_lints_inheritance(&mut doc);
        assert!(!changed);
        assert_eq!(doc["lints"]["workspace"].as_bool(), Some(true));
    }
}
