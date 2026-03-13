use anyhow::{Context, bail};
use clap::Args;
use flate2::read::GzDecoder;
use module_parser::{ResolvedMetadataPath, extract_reexport_target, resolve_source_from_metadata};
use reqwest::{Client, StatusCode};
use semver::Version;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Args)]
#[command(disable_version_flag = true)]
/// Resolve Rust source code from a crate
pub struct DocsArgs {
    /// Path to the Cargo workspace or crate to inspect
    #[arg(short = 'p', long, default_value = ".")]
    path: PathBuf,
    /// Registry to query when the crate is not present in local metadata
    #[arg(long, default_value = "crates.io")]
    registry: String,
    /// Print query/package/version/source metadata before the resolved Rust source
    #[arg(short = 'v', long)]
    verbose: bool,
    /// Resolve a specific crate version after metadata/cache lookup misses
    #[arg(long)]
    version: Option<Version>,
    /// Remove the docs cache for the selected registry before resolving
    #[arg(long)]
    clean: bool,
    /// Rust path to resolve(start always by package name), for example `cf-modkit` it will resolve the lib.rs
    /// You can resolve modules `tokio::sync` to resolve the source code from the sync module from tokio crate
    /// You can also resolve by function name, for example `cf-modkit::gts::plugin::BaseModkitPluginV1`
    /// Also resolve by function name, for instance `cf-modkit::gts::schemas::get_core_gts_schemas`
    query: Option<String>,
}

impl DocsArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        if self.clean {
            clean_registry_cache(&self.registry)?;
        }

        let Some(query) = self.query.as_deref() else {
            return if self.clean {
                Ok(())
            } else {
                bail!("docs query is required unless --clean is used by itself")
            };
        };

        let workspace_path = self
            .path
            .canonicalize()
            .with_context(|| format!("can't canonicalize path {}", self.path.display()))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for docs queries")?;
        let client = build_registry_client()?;
        let resolution_ctx = Resolver {
            workspace_path: &workspace_path,
            client: &client,
            runtime: &runtime,
            registry: &self.registry,
        };
        let mut visited = HashSet::new();
        let final_resolution = resolve_query_recursive(
            &resolution_ctx,
            &workspace_path,
            query,
            self.version.as_ref(),
            &mut visited,
        )?;

        print_resolved_path(query, &final_resolution, self.verbose);

        Ok(())
    }
}

fn print_resolved_path(query: &str, resolved: &ResolvedMetadataPath, verbose: bool) {
    if verbose {
        println!("query: {query}");
        println!("package: {}", resolved.package_name);
        println!("library: {}", resolved.library_name);
        println!("version: {}", resolved.version);
        println!("manifest: {}", resolved.manifest_path.display());
        println!("source: {}", resolved.source_path.display());
        println!();
    }

    println!("{}", resolved.source);
}

fn build_registry_client() -> anyhow::Result<Client> {
    Client::builder()
        .user_agent("cyberfabric-cli")
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to create registry HTTP client")
}

struct Resolver<'a> {
    workspace_path: &'a Path,
    client: &'a Client,
    runtime: &'a tokio::runtime::Runtime,
    registry: &'a str,
}

fn resolve_query_recursive(
    context: &Resolver<'_>,
    preferred_path: &Path,
    query: &str,
    requested_version: Option<&Version>,
    visited: &mut HashSet<String>,
) -> anyhow::Result<ResolvedMetadataPath> {
    let visit_key = format!(
        "{}|{}|{}",
        preferred_path.display(),
        query,
        requested_version.map_or_else(|| "*".to_owned(), ToString::to_string)
    );
    if !visited.insert(visit_key) {
        bail!("detected recursive re-export loop while resolving '{query}'");
    }

    let Some(resolution) = resolve_from_paths(
        context.workspace_path,
        preferred_path,
        context.client,
        context.runtime,
        context.registry,
        query,
        requested_version,
    )?
    else {
        bail!("could not resolve '{query}'");
    };

    if let Some(next_step) = next_reexport_step(preferred_path, &resolution, query)? {
        return resolve_query_recursive(
            context,
            &next_step.preferred_path,
            &next_step.query,
            next_step.requested_version.as_ref(),
            visited,
        );
    }

    Ok(resolution)
}

fn resolve_from_paths(
    workspace_path: &Path,
    preferred_path: &Path,
    client: &Client,
    runtime: &tokio::runtime::Runtime,
    registry: &str,
    query: &str,
    requested_version: Option<&Version>,
) -> anyhow::Result<Option<ResolvedMetadataPath>> {
    if let Some(resolved) = resolve_source_from_metadata(preferred_path, query)? {
        return Ok(Some(resolved));
    }

    if preferred_path != workspace_path
        && let Some(resolved) = resolve_source_from_metadata(workspace_path, query)?
    {
        return Ok(Some(resolved));
    }

    runtime
        .block_on(resolve_from_registry(
            client,
            registry,
            query,
            requested_version,
        ))
        .map(Some)
}

async fn resolve_from_registry(
    client: &Client,
    registry: &str,
    query: &str,
    requested_version: Option<&Version>,
) -> anyhow::Result<ResolvedMetadataPath> {
    if registry != "crates.io" {
        bail!("unsupported registry '{registry}'. Only 'crates.io' is currently supported");
    }

    let crate_name = query
        .split("::")
        .next()
        .filter(|segment| !segment.is_empty())
        .context("query must not be empty")?;

    if let Some(resolved) = resolve_from_cache(registry, crate_name, query, requested_version)? {
        return Ok(resolved);
    }

    let resolved_version = if let Some(requested_version) = requested_version {
        requested_version.to_string()
    } else {
        fetch_exact_crates_io_candidate(client, registry, crate_name)
            .await?
            .with_context(|| {
                format!("could not resolve package '{crate_name}' from the crates.io registry")
            })?
            .max_version
    };
    let crate_root = cache_crate_source(client, registry, crate_name, &resolved_version).await?;

    resolve_source_from_metadata(&crate_root, query)?
        .with_context(|| format!("could not resolve '{query}' inside package '{crate_name}'"))
}

struct NextStep {
    preferred_path: PathBuf,
    query: String,
    requested_version: Option<Version>,
}

fn next_reexport_step(
    preferred_path: &Path,
    resolved: &ResolvedMetadataPath,
    query: &str,
) -> anyhow::Result<Option<NextStep>> {
    let Some(target_segments) = extract_reexport_target(
        &resolved.source,
        query
            .split("::")
            .last()
            .context("query must not be empty")?,
    )?
    else {
        return Ok(None);
    };

    let crate_root = resolved
        .manifest_path
        .parent()
        .context("resolved manifest path has no parent")?;
    let package_name = &resolved.package_name;
    let query_segments = split_query_segments(query)?;
    let package_index = query_segments
        .iter()
        .position(|segment| segment == package_name)
        .context("resolved query does not include package name")?;
    let containing_module_segments = query_segments[package_index + 1..]
        .split_last()
        .map_or_else(Vec::new, |(_, module_segments)| module_segments.to_vec());

    if let Some(relative_segments) =
        resolve_relative_reexport(&target_segments, &containing_module_segments)?
    {
        let next_query = build_query(package_name, &relative_segments);
        return Ok(Some(NextStep {
            preferred_path: preferred_path.to_path_buf(),
            query: next_query,
            requested_version: None,
        }));
    }

    let dependencies = parse_dependencies(crate_root)?;
    let Some(dep) = dependencies.get(&target_segments[0]) else {
        if let Some(next_query) = resolve_bare_relative_reexport(
            crate_root,
            package_name,
            &target_segments,
            &containing_module_segments,
        )? {
            return Ok(Some(NextStep {
                preferred_path: preferred_path.to_path_buf(),
                query: next_query,
                requested_version: None,
            }));
        }

        let next_query = build_query(package_name, &target_segments);
        return Ok(Some(NextStep {
            preferred_path: preferred_path.to_path_buf(),
            query: next_query,
            requested_version: None,
        }));
    };

    let remaining_segments = &target_segments[1..];
    let next_query = build_query(&dep.package_name, remaining_segments);
    let next_preferred_path = dep.path.as_ref().map_or_else(
        || preferred_path.to_path_buf(),
        |path| crate_root.join(path),
    );

    Ok(Some(NextStep {
        preferred_path: next_preferred_path,
        query: next_query,
        requested_version: dep.version.clone(),
    }))
}

fn resolve_bare_relative_reexport(
    crate_root: &Path,
    package_name: &str,
    target_segments: &[String],
    containing_module_segments: &[String],
) -> anyhow::Result<Option<String>> {
    if containing_module_segments.is_empty() {
        return Ok(None);
    }

    let relative_segments = containing_module_segments
        .iter()
        .cloned()
        .chain(target_segments.iter().cloned())
        .collect::<Vec<_>>();
    let relative_query = build_query(package_name, &relative_segments);

    Ok(resolve_source_from_metadata(crate_root, &relative_query)?.map(|_| relative_query))
}

fn split_query_segments(query: &str) -> anyhow::Result<Vec<String>> {
    let segments = query
        .split("::")
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        bail!("query must not be empty");
    }
    Ok(segments)
}

fn build_query(package_name: &str, segments: &[String]) -> String {
    if segments.is_empty() {
        package_name.to_owned()
    } else {
        format!("{package_name}::{}", segments.join("::"))
    }
}

fn resolve_relative_reexport(
    target_segments: &[String],
    containing_module_segments: &[String],
) -> anyhow::Result<Option<Vec<String>>> {
    let Some(first) = target_segments.first() else {
        return Ok(None);
    };

    match first.as_str() {
        "crate" => Ok(Some(target_segments[1..].to_vec())),
        "self" => Ok(Some(
            containing_module_segments
                .iter()
                .cloned()
                .chain(target_segments[1..].iter().cloned())
                .collect(),
        )),
        "super" => {
            let mut module_segments = containing_module_segments.to_vec();
            let mut index = 0;
            while target_segments
                .get(index)
                .is_some_and(|segment| segment == "super")
            {
                if module_segments.pop().is_none() {
                    bail!("re-export path moves above crate root");
                }
                index += 1;
            }
            Ok(Some(
                module_segments
                    .into_iter()
                    .chain(target_segments[index..].iter().cloned())
                    .collect(),
            ))
        }
        _ => Ok(None),
    }
}

#[derive(Clone)]
struct DependencySpec {
    package_name: String,
    version: Option<Version>,
    path: Option<PathBuf>,
}

fn parse_dependencies(crate_root: &Path) -> anyhow::Result<HashMap<String, DependencySpec>> {
    let manifest_path = crate_root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read manifest {}", manifest_path.display()))?;
    let manifest = manifest
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("failed to parse manifest {}", manifest_path.display()))?;

    let mut deps = HashMap::new();
    if let Some(table) = manifest
        .get("dependencies")
        .and_then(toml_edit::Item::as_table_like)
    {
        for (alias, value) in table.iter() {
            let spec = parse_dependency_spec(alias, value);
            deps.insert(alias.to_owned(), spec);
        }
    }

    Ok(deps)
}

fn parse_dependency_spec(alias: &str, value: &toml_edit::Item) -> DependencySpec {
    if let Some(version) = value.as_str() {
        return DependencySpec {
            package_name: alias.to_owned(),
            version: Version::parse(version).ok(),
            path: None,
        };
    }

    if let Some(table) = value.as_inline_table() {
        return DependencySpec {
            package_name: table
                .get("package")
                .and_then(toml_edit::Value::as_str)
                .unwrap_or(alias)
                .to_owned(),
            version: table
                .get("version")
                .and_then(toml_edit::Value::as_str)
                .and_then(|version| Version::parse(version).ok()),
            path: table
                .get("path")
                .and_then(toml_edit::Value::as_str)
                .map(PathBuf::from),
        };
    }

    if let Some(table) = value.as_table_like() {
        return DependencySpec {
            package_name: table
                .get("package")
                .and_then(toml_edit::Item::as_str)
                .unwrap_or(alias)
                .to_owned(),
            version: table
                .get("version")
                .and_then(toml_edit::Item::as_str)
                .and_then(|version| Version::parse(version).ok()),
            path: table
                .get("path")
                .and_then(toml_edit::Item::as_str)
                .map(PathBuf::from),
        };
    }

    DependencySpec {
        package_name: alias.to_owned(),
        version: None,
        path: None,
    }
}

async fn fetch_exact_crates_io_candidate(
    client: &Client,
    registry: &str,
    crate_name: &str,
) -> anyhow::Result<Option<ExactCrate>> {
    let crate_url = format!("https://{registry}/api/v1/crates/{crate_name}");
    let response = client
        .get(&crate_url)
        .send()
        .await
        .with_context(|| format!("request failed for '{crate_name}'"))?;

    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response
        .error_for_status()
        .with_context(|| format!("registry returned an error for '{crate_name}'"))?
        .json::<ExactCrateResponse>()
        .await
        .with_context(|| format!("invalid crate metadata for '{crate_name}'"))?;

    Ok(Some(ExactCrate {
        max_version: response.crate_info.max_version,
    }))
}

#[derive(Deserialize)]
struct ExactCrateResponse {
    #[serde(rename = "crate")]
    crate_info: ExactCrateInfo,
}

#[derive(Deserialize)]
struct ExactCrateInfo {
    max_version: String,
}

struct ExactCrate {
    max_version: String,
}

async fn download_crate_archive(
    client: &Client,
    registry: &str,
    crate_name: &str,
    version: &str,
) -> anyhow::Result<Vec<u8>> {
    let download_url = format!("https://{registry}/api/v1/crates/{crate_name}/{version}/download");
    let archive = client
        .get(&download_url)
        .send()
        .await
        .with_context(|| format!("download request failed for {crate_name}"))?
        .error_for_status()
        .with_context(|| format!("download endpoint returned an error for {crate_name}"))?
        .bytes()
        .await
        .with_context(|| format!("failed to read downloaded source for {crate_name}"))?;

    Ok(archive.to_vec())
}

async fn cache_crate_source(
    client: &Client,
    registry: &str,
    crate_name: &str,
    version: &str,
) -> anyhow::Result<PathBuf> {
    let package_root = package_cache_root(registry, crate_name)?;
    let crate_root = package_root.join(version);

    if crate_root.join("Cargo.toml").is_file() {
        return Ok(crate_root);
    }

    let archive_bytes = download_crate_archive(client, registry, crate_name, version).await?;
    extract_crate_archive(&archive_bytes, &package_root, crate_name, version)?;
    update_latest_symlink(&package_root, version)?;

    if crate_root.join("Cargo.toml").is_file() {
        Ok(crate_root)
    } else {
        bail!("cached crate source is missing Cargo.toml for {crate_name} {version}");
    }
}

fn registry_cache_root(registry: &str) -> anyhow::Result<PathBuf> {
    let cache_root = std::env::temp_dir()
        .join("cyberfabric-docs-cache")
        .join(sanitize_registry_name(registry));
    fs::create_dir_all(&cache_root)
        .with_context(|| format!("failed to create cache dir {}", cache_root.display()))?;
    Ok(cache_root)
}

fn package_cache_root(registry: &str, crate_name: &str) -> anyhow::Result<PathBuf> {
    let package_root = registry_cache_root(registry)?.join(crate_name);
    fs::create_dir_all(&package_root).with_context(|| {
        format!(
            "failed to create package cache dir {}",
            package_root.display()
        )
    })?;
    Ok(package_root)
}

fn resolve_from_cache(
    registry: &str,
    crate_name: &str,
    query: &str,
    requested_version: Option<&Version>,
) -> anyhow::Result<Option<ResolvedMetadataPath>> {
    let package_root = package_cache_root(registry, crate_name)?;

    if let Some(requested_version) = requested_version {
        let crate_root = package_root.join(requested_version.to_string());
        return resolve_from_cached_root(&crate_root, query);
    }

    let latest_link = package_root.join("latest");
    if let Some(resolved) = resolve_from_cached_root(&latest_link, query)? {
        return Ok(Some(resolved));
    }

    let mut cached_versions = cached_package_versions(&package_root)?;
    cached_versions
        .sort_by(|(left_version, _), (right_version, _)| right_version.cmp(left_version));

    if let Some((latest_version, latest_root)) = cached_versions.first()
        && latest_link != *latest_root
    {
        update_latest_symlink(&package_root, &latest_version.to_string())?;
    }

    for (_, crate_root) in cached_versions {
        if let Some(resolved) = resolve_from_cached_root(&crate_root, query)? {
            return Ok(Some(resolved));
        }
    }

    Ok(None)
}

fn resolve_from_cached_root(
    crate_root: &Path,
    query: &str,
) -> anyhow::Result<Option<ResolvedMetadataPath>> {
    if !crate_root.join("Cargo.toml").is_file() {
        return Ok(None);
    }

    resolve_source_from_metadata(crate_root, query)
}

fn cached_package_versions(package_root: &Path) -> anyhow::Result<Vec<(Version, PathBuf)>> {
    Ok(fs::read_dir(package_root)
        .with_context(|| format!("failed to read cache dir {}", package_root.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name();
            let file_name = file_name.to_str()?;
            if file_name == "latest" {
                return None;
            }

            let crate_root = entry.path();
            if !crate_root.join("Cargo.toml").is_file() {
                return None;
            }

            Some((Version::parse(file_name).ok()?, crate_root))
        })
        .collect::<Vec<_>>())
}

fn clean_registry_cache(registry: &str) -> anyhow::Result<()> {
    let cache_root = std::env::temp_dir()
        .join("cyberfabric-docs-cache")
        .join(sanitize_registry_name(registry));
    if cache_root.exists() {
        fs::remove_dir_all(&cache_root)
            .with_context(|| format!("failed to remove cache dir {}", cache_root.display()))?;
    }
    Ok(())
}

fn sanitize_registry_name(registry: &str) -> String {
    registry
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn extract_crate_archive(
    archive_bytes: &[u8],
    package_root: &Path,
    crate_name: &str,
    version: &str,
) -> anyhow::Result<()> {
    let decoder = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(package_root).with_context(|| {
        format!(
            "failed to unpack crate archive into {}",
            package_root.display()
        )
    })?;

    let extracted_root = package_root.join(format!("{crate_name}-{version}"));
    let crate_root = package_root.join(version);
    if extracted_root != crate_root && extracted_root.exists() && !crate_root.exists() {
        fs::rename(&extracted_root, &crate_root).with_context(|| {
            format!(
                "failed to move extracted crate from {} to {}",
                extracted_root.display(),
                crate_root.display()
            )
        })?;
    }

    if crate_root.join("Cargo.toml").is_file() {
        Ok(())
    } else {
        bail!("crate archive did not extract expected root for {crate_name} {version}")
    }
}

fn update_latest_symlink(package_root: &Path, version: &str) -> anyhow::Result<()> {
    let latest_link = package_root.join("latest");
    let target = Path::new(version);

    if let Ok(metadata) = fs::symlink_metadata(&latest_link) {
        if metadata.file_type().is_symlink() {
            remove_symlink(&latest_link)?;
        } else if metadata.is_dir() {
            fs::remove_dir_all(&latest_link).with_context(|| {
                format!(
                    "failed to remove existing latest entry {}",
                    latest_link.display()
                )
            })?;
        } else {
            fs::remove_file(&latest_link).with_context(|| {
                format!(
                    "failed to remove existing latest entry {}",
                    latest_link.display()
                )
            })?;
        }
    }

    create_dir_symlink(target, &latest_link)
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> anyhow::Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| {
        format!(
            "failed to create symlink from {} to {}",
            link.display(),
            target.display()
        )
    })
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) -> anyhow::Result<()> {
    std::os::windows::fs::symlink_dir(target, link).with_context(|| {
        format!(
            "failed to create symlink from {} to {}",
            link.display(),
            target.display()
        )
    })
}

#[cfg(unix)]
fn remove_symlink(path: &Path) -> anyhow::Result<()> {
    fs::remove_file(path).with_context(|| format!("failed to remove symlink {}", path.display()))
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> anyhow::Result<()> {
    fs::remove_dir(path).with_context(|| format!("failed to remove symlink {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{Resolver, build_registry_client, next_reexport_step, resolve_query_recursive};
    use module_parser::resolve_source_from_metadata;
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn next_reexport_step_prefers_current_module_for_bare_reexports() {
        let project = TestProject::new("docs-reexport-step");
        project.write(
            "Cargo.toml",
            r#"
            [package]
            name = "cf-modkit"
            version = "0.5.4"
            edition = "2024"
            "#,
        );
        project.write(
            "src/lib.rs",
            r"
            pub mod gts;
            ",
        );
        project.write(
            "src/gts/mod.rs",
            r"
            pub mod plugin;
            pub use plugin::BaseModkitPluginV1;
            ",
        );
        project.write(
            "src/gts/plugin.rs",
            r"
            pub struct BaseModkitPluginV1;
            ",
        );

        let query = "cf-modkit::gts::BaseModkitPluginV1";
        let resolved = resolve_source_from_metadata(project.path(), query)
            .expect("metadata query should run")
            .expect("query should resolve");

        let next_step = next_reexport_step(project.path(), &resolved, query)
            .expect("re-export step should resolve")
            .expect("re-export step should exist");

        assert_eq!(
            next_step.query,
            "cf-modkit::gts::plugin::BaseModkitPluginV1"
        );
        assert!(next_step.requested_version.is_none());
        assert_eq!(next_step.preferred_path, project.path());
    }

    #[test]
    fn resolve_query_recursive_follows_bare_relative_reexports() {
        let project = TestProject::new("docs-reexport-recursive");
        project.write(
            "Cargo.toml",
            r#"
            [package]
            name = "cf-modkit"
            version = "0.5.4"
            edition = "2024"
            "#,
        );
        project.write(
            "src/lib.rs",
            r"
            pub mod gts;
            ",
        );
        project.write(
            "src/gts/mod.rs",
            r"
            pub mod plugin;
            pub use plugin::BaseModkitPluginV1;
            ",
        );
        project.write(
            "src/gts/plugin.rs",
            r"
            pub struct BaseModkitPluginV1;
            ",
        );

        let client = build_registry_client().expect("client should build");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let resolver = Resolver {
            workspace_path: project.path(),
            client: &client,
            runtime: &runtime,
            registry: "crates.io",
        };
        let mut visited = HashSet::new();

        let resolved_query = resolve_query_recursive(
            &resolver,
            project.path(),
            "cf-modkit::gts::BaseModkitPluginV1",
            None,
            &mut visited,
        )
        .expect("recursive resolution should succeed");

        assert!(
            resolved_query
                .source
                .contains("pub struct BaseModkitPluginV1;")
        );
        assert!(
            resolved_query
                .source_path
                .ends_with(Path::new("src/gts/plugin.rs"))
        );
    }

    struct TestProject {
        path: PathBuf,
    }

    impl TestProject {
        fn new(prefix: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("cf-cli-{prefix}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("temp project dir should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, relative_path: &str, content: &str) {
            let path = self.path.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent dir should exist");
            }
            fs::write(path, content).expect("file should be written");
        }
    }

    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
