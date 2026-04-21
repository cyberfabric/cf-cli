use crate::common;
use anyhow::{Context, bail};
use module_parser::CargoTomlDependencies;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// Exact directory/file names excluded from deploy bundle copies.
/// Additional pattern-based exclusions (`.env*`, `*.swp`, `*~`) are
/// handled by [`should_skip_bundle_entry`].
const FILTERED_ENTRY_NAMES: &[&str] =
    &[".DS_Store", ".git", ".github", ".idea", ".vscode", "target"];

pub(super) fn prepare_output_dir(
    output_dir: &Path,
    workspace_root: &Path,
    force: bool,
) -> anyhow::Result<()> {
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

    for reserved_path in [&workspace_root, &base_path_root] {
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
        if !output_dir.starts_with(&base_path_root) && !force {
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

pub(super) fn copy_optional_file(source: &Path, destination: &Path) -> anyhow::Result<bool> {
    if !source.is_file() {
        return Ok(false);
    }
    copy_file(source, destination)?;
    Ok(true)
}

pub(super) fn copy_file(source: &Path, destination: &Path) -> anyhow::Result<()> {
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

pub(super) fn copy_relative_workspace_path(
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

pub(super) fn collect_required_local_paths(
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

pub(super) fn rewrite_dependency_paths_for_bundle(
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
        collect_required_local_paths, copy_relative_workspace_path, prepare_output_dir,
        rewrite_dependency_paths_for_bundle,
    };
    use module_parser::{CargoTomlDependencies, CargoTomlDependency};
    use std::fs;
    use std::path::Path;

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
        let output_dir = workspace_root.join(".cyberfabric/demo/deploy");

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
}
