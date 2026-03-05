use super::app_config::AppConfig;
use crate::common::PathConfigArgs;
use anyhow::Context;
use clap::{Args, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

mod add;
mod list;
mod remove;

#[derive(Clone, Copy)]
pub(super) struct SystemRegistryModule {
    pub module_name: &'static str,
    pub crate_name: &'static str,
}

pub(super) const SYSTEM_REGISTRY_MODULES: &[SystemRegistryModule] = &[
    SystemRegistryModule {
        module_name: "credstore",
        crate_name: "cf-credstore",
    },
    SystemRegistryModule {
        module_name: "file-parser",
        crate_name: "cf-file-parser",
    },
    SystemRegistryModule {
        module_name: "api-gateway",
        crate_name: "cf-api-gateway",
    },
    SystemRegistryModule {
        module_name: "authn-resolver",
        crate_name: "cf-authn-resolver",
    },
    SystemRegistryModule {
        module_name: "static-authn-plugin",
        crate_name: "cf-static-authn-plugin",
    },
    SystemRegistryModule {
        module_name: "authz-resolver",
        crate_name: "cf-authz-resolver",
    },
    SystemRegistryModule {
        module_name: "static-authz-plugin",
        crate_name: "cf-static-authz-plugin",
    },
    SystemRegistryModule {
        module_name: "grpc-hub",
        crate_name: "cf-grpc-hub",
    },
    SystemRegistryModule {
        module_name: "module-orchestrator",
        crate_name: "cf-module-orchestrator",
    },
    SystemRegistryModule {
        module_name: "nodes-registry",
        crate_name: "cf-nodes-registry",
    },
    SystemRegistryModule {
        module_name: "oagw",
        crate_name: "cf-oagw",
    },
    SystemRegistryModule {
        module_name: "single-tenant-tr-plugin",
        crate_name: "cf-single-tenant-tr-plugin",
    },
    SystemRegistryModule {
        module_name: "static-tr-plugin",
        crate_name: "cf-static-tr-plugin",
    },
    SystemRegistryModule {
        module_name: "tenant-resolver",
        crate_name: "cf-tenant-resolver",
    },
    SystemRegistryModule {
        module_name: "types-registry",
        crate_name: "cf-types-registry",
    },
];

#[derive(Args)]
pub struct ModulesArgs {
    #[command(subcommand)]
    command: ModulesCommand,
}

#[derive(Subcommand)]
pub enum ModulesCommand {
    /// List available system crates
    List(list::ListArgs),
    /// Add a module to the modules section
    Add(add::AddArgs),
    /// Remove a module from the modules section
    Rm(remove::RemoveArgs),
}

pub(super) struct ModulesContext {
    workspace_path: PathBuf,
    config_path: PathBuf,
}

impl ModulesArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            ModulesCommand::List(args) => args.run(),
            ModulesCommand::Add(args) => args.run(),
            ModulesCommand::Rm(args) => args.run(),
        }
    }
}

pub(super) fn resolve_modules_context(
    path_config: &PathConfigArgs,
) -> anyhow::Result<ModulesContext> {
    Ok(ModulesContext {
        workspace_path: path_config.path.clone(),
        config_path: path_config.resolve_config_required()?,
    })
}

pub(super) fn load_config(path: &Path) -> anyhow::Result<AppConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("can't read config file {}", path.display()))?;
    serde_saphyr::from_str(&raw).with_context(|| format!("config not valid at {}", path.display()))
}

pub(super) fn validate_module_name(module: &str) -> anyhow::Result<()> {
    if module.is_empty()
        || !module
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        anyhow::bail!("invalid module name '{module}'. Use only letters, numbers, '-' and '_'");
    }
    Ok(())
}

pub(super) fn save_config(path: &Path, config: &AppConfig) -> anyhow::Result<()> {
    let mut serialized = serde_saphyr::to_string(config).context("failed to serialize config")?;
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, serialized)
        .with_context(|| format!("can't write temp config file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("can't replace config file {}", path.display()))
}
