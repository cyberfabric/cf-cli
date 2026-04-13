use crate::common::{self, PathConfigArgs};
use anyhow::{Context, bail};
use cargo_generate::{GenerateArgs, TemplatePath, Vcs, generate};
use clap::{Args, ValueEnum};
use module_parser::CargoTomlDependencies;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const OUTPUT_SUBDIR: &str = "deploy";
/// Exact directory/file names excluded from deploy bundle copies.
/// Additional pattern-based exclusions (`.env*`, `*.swp`, `*~`) are
/// handled by [`should_skip_bundle_entry`].
const FILTERED_ENTRY_NAMES: &[&str] =
    &[".DS_Store", ".git", ".github", ".idea", ".vscode", "target"];

#[derive(Args)]
pub struct DeployArgs {
    /// Deployment template kind to render
    #[arg(long, value_enum)]
    template: DeployTemplateKind,
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Override the generated server and binary name
    #[arg(long)]
    name: Option<String>,
    /// Output directory for the generated deploy bundle
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Allow replacing an existing custom output directory
    #[arg(long)]
    force: bool,
    /// Path to a local deploy template repository
    #[arg(long)]
    local_path: Option<String>,
    /// URL to the template git repo
    #[arg(
        long,
        default_value = "https://github.com/cyberfabric/cf-template-rust"
    )]
    git: Option<String>,
    /// Subfolder relative to the template repo root
    #[arg(long, default_value = "Deploy")]
    subfolder: String,
    /// Branch of the template git repo
    #[arg(long, default_value = "main")]
    branch: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DeployTemplateKind {
    #[value(name = "docker")]
    Docker,
}

impl DeployTemplateKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
        }
    }
}

impl DeployArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let (workspace_root, config_path, project_name) =
            common::resolve_workspace_config_and_name(&self.path_config, self.name.as_deref())?;
        let config = common::get_config(&config_path)?;
        let dependencies = config.create_dependencies()?;
        let output_dir = self.resolve_output_dir(&workspace_root, &project_name);
        prepare_output_dir(&output_dir, &workspace_root, self.force)?;

        copy_file(
            &workspace_root.join("Cargo.toml"),
            &output_dir.join("Cargo.toml"),
        )?;
        let has_cargo_lock = copy_optional_file(
            &workspace_root.join("Cargo.lock"),
            &output_dir.join("Cargo.lock"),
        )?;
        copy_file(&config_path, &output_dir.join("config.yml"))?;

        let local_paths = collect_required_local_paths(&workspace_root, &dependencies)?;
        for relative_path in &local_paths {
            copy_relative_workspace_path(&workspace_root, &output_dir, relative_path)?;
        }

        let rewritten_dependencies =
            rewrite_dependency_paths_for_bundle(&workspace_root, &dependencies)?;
        common::generate_server_structure_at(&output_dir, &project_name, &rewritten_dependencies)?;

        render_deploy_template(
            &output_dir,
            &project_name,
            &local_paths,
            has_cargo_lock,
            &TemplateSource {
                local_path: self.local_path.as_deref(),
                git: self.git.as_deref(),
                subfolder: &self.subfolder,
                kind: self.template,
                branch: self.branch.as_deref(),
            },
        )?;

        println!("Deploy bundle generated at {}", output_dir.display());
        Ok(())
    }

    fn resolve_output_dir(&self, workspace_root: &Path, project_name: &str) -> PathBuf {
        self.output_dir.as_ref().map_or_else(
            || {
                workspace_root
                    .join(common::BASE_PATH)
                    .join(OUTPUT_SUBDIR)
                    .join(project_name)
            },
            |path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    workspace_root.join(path)
                }
            },
        )
    }
}

fn prepare_output_dir(output_dir: &Path, workspace_root: &Path, force: bool) -> anyhow::Result<()> {
    if output_dir.exists()
        && fs::symlink_metadata(output_dir)
            .with_context(|| format!("can't inspect {}", output_dir.display()))?
            .file_type()
            .is_symlink()
    {
        bail!(
            "output directory '{}' cannot be a symlink",
            output_dir.display()
        );
    }

    let workspace_root = workspace_root
        .canonicalize()
        .with_context(|| format!("can't canonicalize {}", workspace_root.display()))?;
    let output_dir = canonicalize_path_for_safety(output_dir)?;
    let base_path_root = workspace_root.join(common::BASE_PATH);
    let deploy_root = base_path_root.join(OUTPUT_SUBDIR);

    for reserved_path in [&workspace_root, &base_path_root, &deploy_root] {
        if output_dir == *reserved_path {
            bail!(
                "output directory cannot be the reserved path {}",
                reserved_path.display()
            );
        }
    }

    if output_dir.exists() {
        if !output_dir.is_dir() {
            bail!(
                "output directory '{}' exists but is not a directory",
                output_dir.display()
            );
        }
        if !output_dir.starts_with(&deploy_root) && !force {
            bail!(
                "refusing to replace existing custom output directory {}; pass --force to overwrite it",
                output_dir.display()
            );
        }
        fs::remove_dir_all(&output_dir)
            .with_context(|| format!("can't remove {}", output_dir.display()))?;
    }
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("can't create {}", output_dir.display()))
}

fn copy_optional_file(source: &Path, destination: &Path) -> anyhow::Result<bool> {
    if !source.is_file() {
        return Ok(false);
    }
    copy_file(source, destination)?;
    Ok(true)
}

fn copy_file(source: &Path, destination: &Path) -> anyhow::Result<()> {
    reject_symlink(source)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("can't create {}", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "can't copy {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn reject_symlink(path: &Path) -> anyhow::Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("can't inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "symlinked paths are not supported in deploy bundles: {}",
            path.display()
        );
    }
    Ok(())
}

fn copy_relative_workspace_path(
    workspace_root: &Path,
    output_dir: &Path,
    relative_path: &Path,
) -> anyhow::Result<()> {
    let source = workspace_root.join(relative_path);
    let destination = output_dir.join(relative_path);
    copy_path_recursively(&source, &destination)
}

fn copy_path_recursively(source: &Path, destination: &Path) -> anyhow::Result<()> {
    reject_symlink(source)?;
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("can't inspect {}", source.display()))?;

    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .with_context(|| format!("can't create {}", destination.display()))?;
        for entry in
            fs::read_dir(source).with_context(|| format!("can't read {}", source.display()))?
        {
            let entry = entry.with_context(|| format!("can't read {}", source.display()))?;
            if should_skip_bundle_entry(&entry.file_name()) {
                continue;
            }
            let child_source = entry.path();
            let child_destination = destination.join(entry.file_name());
            copy_path_recursively(&child_source, &child_destination)?;
        }
        return Ok(());
    }

    copy_file(source, destination)
}

fn should_skip_bundle_entry(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    FILTERED_ENTRY_NAMES.contains(&name.as_ref())
        || name.starts_with(".env")
        || name.ends_with(".swp")
        || name.ends_with('~')
}

fn collect_required_local_paths(
    workspace_root: &Path,
    dependencies: &CargoTomlDependencies,
) -> anyhow::Result<BTreeSet<PathBuf>> {
    let mut paths = read_workspace_members(workspace_root)?;
    for dependency in dependencies.values() {
        if let Some(path) = &dependency.path {
            paths.insert(resolve_workspace_relative_path(
                workspace_root,
                Path::new(path),
            )?);
        }
    }
    Ok(paths)
}

fn read_workspace_members(workspace_root: &Path) -> anyhow::Result<BTreeSet<PathBuf>> {
    let cargo_toml_path = workspace_root.join("Cargo.toml");
    let raw = fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("can't read {}", cargo_toml_path.display()))?;
    let doc = raw
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("can't parse {}", cargo_toml_path.display()))?;

    let members = doc["workspace"]["members"]
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(toml_edit::Value::as_str)
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut resolved = BTreeSet::new();
    for member in members {
        let relative = resolve_workspace_relative_path(workspace_root, &member)?;
        if !relative.as_os_str().is_empty() {
            resolved.insert(relative);
        }
    }
    Ok(resolved)
}

fn resolve_workspace_relative_path(
    workspace_root: &Path,
    raw_path: &Path,
) -> anyhow::Result<PathBuf> {
    let absolute_path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        workspace_root.join(raw_path)
    };
    let absolute_path = absolute_path
        .canonicalize()
        .with_context(|| format!("can't canonicalize {}", absolute_path.display()))?;
    let workspace_root = workspace_root
        .canonicalize()
        .with_context(|| format!("can't canonicalize {}", workspace_root.display()))?;

    if !absolute_path.starts_with(&workspace_root) {
        bail!(
            "local dependency path '{}' is outside the workspace root {}",
            absolute_path.display(),
            workspace_root.display()
        );
    }

    absolute_path
        .strip_prefix(workspace_root)
        .map(Path::to_path_buf)
        .context("can't derive workspace-relative path")
}

fn rewrite_dependency_paths_for_bundle(
    workspace_root: &Path,
    dependencies: &CargoTomlDependencies,
) -> anyhow::Result<CargoTomlDependencies> {
    let mut rewritten = dependencies.clone();
    for dependency in rewritten.values_mut() {
        if let Some(path) = &dependency.path {
            let relative = resolve_workspace_relative_path(workspace_root, Path::new(path))?;
            dependency.path = Some(bundle_relative_dependency_path(&relative));
        }
    }
    Ok(rewritten)
}

fn bundle_relative_dependency_path(relative_path: &Path) -> String {
    PathBuf::from("..")
        .join("..")
        .join(relative_path)
        .display()
        .to_string()
        .replace('\\', "/")
}

struct TemplateSource<'a> {
    local_path: Option<&'a str>,
    git: Option<&'a str>,
    subfolder: &'a str,
    kind: DeployTemplateKind,
    branch: Option<&'a str>,
}

fn render_deploy_template(
    output_dir: &Path,
    project_name: &str,
    local_paths: &BTreeSet<PathBuf>,
    has_cargo_lock: bool,
    source: &TemplateSource<'_>,
) -> anyhow::Result<()> {
    let generated_project_dir = format!(".cyberfabric/{project_name}");

    let copy_cargo_lock = if has_cargo_lock {
        "COPY Cargo.lock Cargo.lock\n".to_owned()
    } else {
        String::new()
    };

    let copy_local_paths = local_paths
        .iter()
        .map(|p| {
            let p = p.display().to_string().replace('\\', "/");
            format!("COPY {p} {p}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let values_path = output_dir.join(".cargo-generate-values.toml");
    fs::write(
        &values_path,
        format!(
            "[values]\ngenerated_project_dir = {gen}\nexecutable_name = {exe}\ncopy_cargo_lock = {lock}\ncopy_local_paths = {paths}\n",
            gen = toml::Value::from(&*generated_project_dir),
            exe = toml::Value::from(project_name),
            lock = toml::Value::from(&*copy_cargo_lock),
            paths = toml::Value::from(&*copy_local_paths),
        ),
    )
    .context("can't write template values file")?;

    let auto_path = format!("{}/{}", source.subfolder, source.kind.as_str());

    let (git, branch) = if source.local_path.is_some() {
        (None, None)
    } else {
        (
            source.git.map(ToOwned::to_owned),
            source.branch.map(ToOwned::to_owned),
        )
    };

    generate(GenerateArgs {
        template_path: TemplatePath {
            auto_path: Some(auto_path),
            git,
            path: source.local_path.map(ToOwned::to_owned),
            branch,
            ..TemplatePath::default()
        },
        destination: Some(output_dir.to_path_buf()),
        name: Some(project_name.to_owned()),
        force: true,
        silent: true,
        vcs: Some(Vcs::None),
        init: true,
        overwrite: true,
        no_workspace: true,
        template_values_file: Some(values_path.display().to_string()),
        ..GenerateArgs::default()
    })
    .context("can't render deploy template")?;

    let _ = fs::remove_file(&values_path);
    Ok(())
}

fn canonicalize_path_for_safety(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("can't canonicalize {}", path.display()));
    }

    let parent = path
        .parent()
        .context("output directory must have a parent directory")?;
    let file_name = path
        .file_name()
        .context("output directory must have a final path component")?;

    Ok(canonicalize_path_for_safety(parent)?.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::{
        DeployArgs, DeployTemplateKind, collect_required_local_paths, copy_relative_workspace_path,
        prepare_output_dir, rewrite_dependency_paths_for_bundle,
    };
    use crate::common::PathConfigArgs;
    use module_parser::test_utils::TempDirExt;
    use module_parser::{CargoTomlDependencies, CargoTomlDependency};
    use std::fs;
    use std::path::{Path, PathBuf};

    struct CurrentDirGuard(PathBuf);

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn collects_workspace_members_and_dependency_paths() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"modules/foo\", \"modules/foo/sdk\"]\n",
        )
        .expect("workspace cargo toml");
        fs::create_dir_all(workspace_root.join("modules/foo/sdk/src")).expect("workspace dirs");
        fs::create_dir_all(workspace_root.join("extras/bar/src")).expect("extra dirs");

        let mut dependencies = CargoTomlDependencies::new();
        dependencies.insert(
            "bar".to_owned(),
            CargoTomlDependency {
                path: Some(workspace_root.join("extras/bar").display().to_string()),
                ..CargoTomlDependency::default()
            },
        );

        let paths = collect_required_local_paths(workspace_root, &dependencies).expect("paths");
        assert!(paths.contains(Path::new("modules/foo")));
        assert!(paths.contains(Path::new("modules/foo/sdk")));
        assert!(paths.contains(Path::new("extras/bar")));
    }

    #[test]
    fn rewrites_absolute_dependency_paths_for_bundle() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"modules/foo\"]\n",
        )
        .expect("workspace cargo toml");
        fs::create_dir_all(workspace_root.join("modules/foo/src")).expect("module dir");

        let mut dependencies = CargoTomlDependencies::new();
        dependencies.insert(
            "foo".to_owned(),
            CargoTomlDependency {
                path: Some(workspace_root.join("modules/foo").display().to_string()),
                ..CargoTomlDependency::default()
            },
        );

        let rewritten =
            rewrite_dependency_paths_for_bundle(workspace_root, &dependencies).expect("rewrite");
        assert_eq!(rewritten["foo"].path.as_deref(), Some("../../modules/foo"));
    }

    #[test]
    fn prepare_output_dir_replaces_default_bundle_dir_without_force() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        let output_dir = workspace_root.join(".cyberfabric/deploy/demo");

        fs::create_dir_all(&output_dir).expect("default deploy dir");
        fs::write(output_dir.join("stale.txt"), "stale").expect("stale file");

        prepare_output_dir(&output_dir, workspace_root, false).expect("default deploy dir");

        assert!(output_dir.is_dir());
        assert!(!output_dir.join("stale.txt").exists());
    }

    #[test]
    fn prepare_output_dir_requires_force_for_existing_custom_dir() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        let output_dir = workspace_root.join("bundle");

        fs::create_dir_all(&output_dir).expect("custom output dir");
        fs::write(output_dir.join("stale.txt"), "stale").expect("stale file");

        let err = prepare_output_dir(&output_dir, workspace_root, false)
            .expect_err("custom output dir should require force");
        assert!(err.to_string().contains("--force"));

        prepare_output_dir(&output_dir, workspace_root, true).expect("forced cleanup");
        assert!(output_dir.is_dir());
        assert!(!output_dir.join("stale.txt").exists());
    }

    #[test]
    fn copy_relative_workspace_path_filters_known_junk_entries() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        let source_dir = workspace_root.join("modules/demo-module");
        let output_dir = workspace_root.join("bundle");

        fs::create_dir_all(source_dir.join("src")).expect("source dir");
        fs::create_dir_all(source_dir.join("target/debug")).expect("target dir");
        fs::create_dir_all(source_dir.join(".git")).expect("git dir");
        fs::write(source_dir.join("src/lib.rs"), "pub fn demo() {}\n").expect("lib source");
        fs::write(source_dir.join(".env"), "SECRET=value\n").expect("env file");
        fs::write(source_dir.join(".DS_Store"), "junk").expect("finder junk");
        fs::write(source_dir.join("target/debug/demo"), "junk").expect("target artifact");

        copy_relative_workspace_path(
            workspace_root,
            &output_dir,
            Path::new("modules/demo-module"),
        )
        .expect("copy filtered bundle path");

        assert!(output_dir.join("modules/demo-module/src/lib.rs").is_file());
        assert!(!output_dir.join("modules/demo-module/.env").exists());
        assert!(!output_dir.join("modules/demo-module/.DS_Store").exists());
        assert!(!output_dir.join("modules/demo-module/.git").exists());
        assert!(!output_dir.join("modules/demo-module/target").exists());
    }

    #[cfg(unix)]
    #[test]
    fn copy_relative_workspace_path_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        let source_dir = workspace_root.join("modules/demo-module");
        let output_dir = workspace_root.join("bundle");

        fs::create_dir_all(&source_dir).expect("source dir");
        fs::write(workspace_root.join("outside.txt"), "hello\n").expect("outside file");
        symlink(
            workspace_root.join("outside.txt"),
            source_dir.join("linked.txt"),
        )
        .expect("symlink");

        let err = copy_relative_workspace_path(
            workspace_root,
            &output_dir,
            Path::new("modules/demo-module"),
        )
        .expect_err("symlinked bundle entries should be rejected");
        assert!(
            err.to_string()
                .contains("symlinked paths are not supported")
        );
    }

    #[test]
    fn deploy_generates_docker_bundle_from_local_templates() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        temp_dir.write(
            "Cargo.toml",
            r#"[workspace]
members = ["modules/demo-module"]
resolver = "2"
"#,
        );
        temp_dir.write("Cargo.lock", "version = 4\n");
        temp_dir.write(
            "config/quickstart.yml",
            "modules:\n  demo:\n    metadata: {}\n",
        );
        temp_dir.write(
            "modules/demo-module/Cargo.toml",
            r#"[package]
name = "demo-module"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
        );
        temp_dir.write("modules/demo-module/src/lib.rs", "pub mod module;\n");
        temp_dir.write(
            "modules/demo-module/src/module.rs",
            "#[module(name = \"demo\")]\npub struct DemoModule;\n",
        );
        temp_dir.write(
            "templates/Deploy/docker/cargo-generate.toml",
            "[template]\nexclude = [\"**/.DS_Store\"]\n",
        );
        temp_dir.write(
            "templates/Deploy/docker/Dockerfile.liquid",
            "COPY {{ generated_project_dir }}/Cargo.toml {{ generated_project_dir }}/Cargo.toml\n{{ copy_local_paths }}\nCOPY config.yml /srv/config.yml\nCOPY --from=builder /workspace/target/release/{{ executable_name }} /srv/{{ executable_name }}\n",
        );

        // chdir so workspace_root() resolves to the temp directory;
        // the guard restores the original CWD on drop to avoid leaking
        // process-global state to other parallel tests.
        let _cwd_guard = CurrentDirGuard(std::env::current_dir().expect("current dir"));
        std::env::set_current_dir(workspace_root).expect("chdir into temp workspace");

        let output_dir = workspace_root.join("bundle");
        let args = DeployArgs {
            template: DeployTemplateKind::Docker,
            path_config: PathConfigArgs {
                path: Some(workspace_root.to_path_buf()),
                config: workspace_root.join("config/quickstart.yml"),
            },
            name: Some("demo".to_owned()),
            output_dir: Some(output_dir.clone()),
            force: false,
            local_path: Some(workspace_root.join("templates").display().to_string()),
            git: None,
            subfolder: "Deploy".to_owned(),
            branch: None,
        };

        args.run().expect("deploy run");

        let generated_cargo_toml = output_dir.join(".cyberfabric/demo/Cargo.toml");
        let generated_main = output_dir.join(".cyberfabric/demo/src/main.rs");
        let dockerfile = output_dir.join("Dockerfile");

        assert!(generated_cargo_toml.is_file());
        assert!(generated_main.is_file());
        assert!(dockerfile.is_file());
        assert!(output_dir.join("config.yml").is_file());
        assert!(output_dir.join("Cargo.toml").is_file());
        assert!(output_dir.join("Cargo.lock").is_file());
        assert!(output_dir.join("modules/demo-module/Cargo.toml").is_file());

        let generated_cargo_toml =
            fs::read_to_string(&generated_cargo_toml).expect("generated cargo toml");
        assert!(generated_cargo_toml.contains("../../modules/demo-module"));

        let generated_main = fs::read_to_string(&generated_main).expect("generated main");
        assert!(generated_main.contains("CF_CLI_CONFIG"));
        assert!(generated_main.contains("run_server(config)"));

        let dockerfile = fs::read_to_string(&dockerfile).expect("dockerfile");
        assert!(
            dockerfile.contains("COPY .cyberfabric/demo/Cargo.toml .cyberfabric/demo/Cargo.toml")
        );
        assert!(dockerfile.contains("COPY modules/demo-module modules/demo-module"));
        assert!(dockerfile.contains("COPY config.yml /srv/config.yml"));
        assert!(dockerfile.contains("/srv/demo"));
    }
}
