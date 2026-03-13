use super::config::ConfigModule;
use super::source::{ResolvedRustPath, resolve_rust_path};
use crate::{CargoTomlDependencies, CargoTomlDependency};
use anyhow::Context;
use cargo_metadata::{Package, Target};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMetadataPath {
    pub package_name: String,
    pub library_name: String,
    pub version: String,
    pub manifest_path: PathBuf,
    pub source_path: PathBuf,
    pub source: String,
}

pub fn get_module_name_from_crate(path: &PathBuf) -> anyhow::Result<HashMap<String, ConfigModule>> {
    let res = cargo_metadata::MetadataCommand::new()
        .current_dir(path)
        .no_deps()
        .exec()
        .context("failed to run cargo metadata")?;
    let mut members = HashMap::new();
    for pkg in res.packages {
        for t in &pkg.targets {
            if t.is_lib() && !t.name.ends_with("sdk") {
                match super::module_rs::retrieve_module_rs(&pkg, t) {
                    Ok(module) => {
                        members.insert(module.0, module.1);
                    }
                    Err(e) => {
                        eprintln!("{e}");
                    }
                }
            }
        }
    }
    Ok(members)
}

pub fn resolve_source_from_metadata(
    path: &Path,
    query: &str,
) -> anyhow::Result<Option<ResolvedMetadataPath>> {
    let query = RustPathQuery::parse(query)?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(path)
        .exec()
        .context("failed to run cargo metadata")?;

    let Some(library_target) = select_library_target(&metadata.packages, &query.package_name)
    else {
        return Ok(None);
    };

    let segments = query
        .segments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let ResolvedRustPath {
        source_path,
        source,
    } = resolve_rust_path(&library_target.root_source_path, &segments)?;

    Ok(Some(ResolvedMetadataPath {
        package_name: library_target.package_name,
        library_name: library_target.library_name,
        version: library_target.version,
        manifest_path: library_target.manifest_path,
        source_path,
        source,
    }))
}

pub fn get_dependencies<S: std::hash::BuildHasher>(
    path: &Path,
    deps: &HashMap<String, String, S>,
) -> anyhow::Result<CargoTomlDependencies> {
    let meta = cargo_metadata::MetadataCommand::new()
        .current_dir(path)
        .exec()
        .context("failed to run cargo metadata")?;
    let mut res = CargoTomlDependencies::with_capacity(deps.len());
    for pkg in meta.packages {
        if let Some(name) = deps.get(pkg.name.as_str()) {
            res.insert(
                name.clone(),
                CargoTomlDependency {
                    package: if pkg.name == name {
                        None
                    } else {
                        Some(pkg.name.to_string())
                    },
                    version: Some(pkg.version.to_string()),
                    ..Default::default()
                },
            );
        }
    }
    Ok(res)
}

struct RustPathQuery {
    package_name: String,
    segments: Vec<String>,
}

impl RustPathQuery {
    fn parse(value: &str) -> anyhow::Result<Self> {
        let segments: Vec<_> = value
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect();

        let Some((package_name, segments)) = segments.split_first() else {
            anyhow::bail!("query must not be empty");
        };

        Ok(Self {
            package_name: package_name.clone(),
            segments: segments.to_vec(),
        })
    }
}

#[derive(Clone)]
struct LibraryTarget {
    package_name: String,
    library_name: String,
    version: String,
    semver_version: cargo_metadata::semver::Version,
    manifest_path: PathBuf,
    root_source_path: PathBuf,
    is_local: bool,
}

fn select_library_target(packages: &[Package], name: &str) -> Option<LibraryTarget> {
    let mut candidates = packages
        .iter()
        .flat_map(|package| {
            if package.name != name {
                return Vec::new().into_iter();
            }

            package
                .targets
                .iter()
                .filter(|target| target.is_lib())
                .map(move |target| to_library_target(package, target))
                .collect::<Vec<_>>()
                .into_iter()
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .is_local
            .cmp(&left.is_local)
            .then_with(|| right.semver_version.cmp(&left.semver_version))
    });

    candidates.into_iter().next()
}

fn to_library_target(package: &Package, target: &Target) -> LibraryTarget {
    LibraryTarget {
        package_name: package.name.to_string(),
        library_name: target.name.clone(),
        version: package.version.to_string(),
        semver_version: package.version.clone(),
        manifest_path: PathBuf::from(&package.manifest_path),
        root_source_path: PathBuf::from(&target.src_path),
        is_local: package.source.is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_source_from_metadata;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn does_not_resolve_using_library_name_from_metadata() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        temp_dir.write(
            "Cargo.toml",
            r#"
            [package]
            name = "cf-demo"
            version = "0.2.0"
            edition = "2024"

            [lib]
            name = "demo"
            path = "src/lib.rs"
            "#,
        );
        temp_dir.write(
            "src/lib.rs",
            r"
            pub mod sync;
            ",
        );
        temp_dir.write(
            "src/sync.rs",
            r"
            pub struct Mutex;

            #[cfg(test)]
            mod tests {
                #[test]
                fn hidden() {}
            }
            ",
        );

        let resolved =
            resolve_source_from_metadata(temp_dir.path(), "demo::sync").expect("query should run");

        assert!(resolved.is_none());
    }

    #[test]
    fn resolves_using_package_name_from_metadata() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        temp_dir.write(
            "Cargo.toml",
            r#"
            [package]
            name = "cf-demo"
            version = "0.3.0"
            edition = "2024"

            [lib]
            name = "demo"
            path = "src/lib.rs"
            "#,
        );
        temp_dir.write(
            "src/lib.rs",
            r"
            pub mod sync;
            pub struct Root;
            ",
        );
        temp_dir.write(
            "src/sync.rs",
            r"
            pub struct SyncRoot;
            ",
        );

        let resolved = resolve_source_from_metadata(temp_dir.path(), "cf-demo::sync")
            .expect("query should run");
        let resolved = resolved.expect("metadata should resolve query");

        assert_eq!(resolved.library_name, "demo");
        assert!(resolved.source.contains("pub struct SyncRoot;"));
    }

    trait TempDirExt {
        fn write(&self, relative_path: &str, content: &str);
    }

    impl TempDirExt for TempDir {
        fn write(&self, relative_path: &str, content: &str) {
            let path = self.path().join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("failed to create parent dir");
            }
            fs::write(path, content).expect("failed to write test file");
        }
    }
}
