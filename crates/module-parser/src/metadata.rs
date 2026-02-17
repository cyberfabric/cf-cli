use super::config::ConfigModule;
use anyhow::Context;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn get_module_name_from_crate(path: &PathBuf) -> anyhow::Result<HashMap<String, ConfigModule>> {
    let res = cargo_metadata::MetadataCommand::new()
        .current_dir(path)
        .no_deps()
        .exec()
        .context("failed to run cargo metadata")?;
    let mut members = HashMap::new();
    for pkg in res.packages {
        for t in &pkg.targets {
            if t.is_lib() {
                match super::module_rs::retrieve_module_rs(&pkg, t.clone()) {
                    Ok(module) => {
                        members.insert(module.0, module.1);
                    }
                    Err(e) => {
                        eprintln!("{e}");
                    }
                };
            }
        }
    }
    Ok(members)
}
