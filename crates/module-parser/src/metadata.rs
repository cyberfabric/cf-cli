use super::config::ConfigModule;
use crate::{CargoTomlDependencies, CargoTomlDependency};
use anyhow::Context;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
