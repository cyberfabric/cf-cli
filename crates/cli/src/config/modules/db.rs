use super::{load_config, resolve_modules_context, save_config, validate_module_name};
use crate::common::PathConfigArgs;
use crate::config::app_config::DbConnConfig;
use anyhow::{Context, bail};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ModuleDbArgs {
    #[command(subcommand)]
    command: ModuleDbCommand,
}

#[derive(Subcommand)]
enum ModuleDbCommand {
    /// Add or update (upsert) a module-level database configuration
    Add(AddArgs),
    /// Edit a module-level database configuration
    Edit(EditArgs),
    /// Remove a module-level database configuration
    Rm(RemoveArgs),
}

impl ModuleDbArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            ModuleDbCommand::Add(args) => args.run(),
            ModuleDbCommand::Edit(args) => args.run(),
            ModuleDbCommand::Rm(args) => args.run(),
        }
    }
}

#[derive(Args)]
struct AddArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Module name under `modules.<module>`
    module: String,
    #[command(flatten)]
    conn: DbConnConfig,
}

impl AddArgs {
    fn run(&self) -> anyhow::Result<()> {
        validate_module_name(&self.module)?;
        ensure_conn_payload(&self.conn)?;

        let context = resolve_modules_context(&self.path_config)?;
        let mut config = load_config(&context.config_path)?;
        let module_cfg = config.modules.entry(self.module.clone()).or_default();
        if let Some(existing) = module_cfg.database.as_mut() {
            existing.apply_patch(self.conn.clone());
        } else {
            module_cfg.database = Some(self.conn.clone());
        }
        save_config(&context.config_path, &config)
    }
}

#[derive(Args)]
struct EditArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Module name under `modules.<module>`
    module: String,
    #[command(flatten)]
    conn: DbConnConfig,
}

impl EditArgs {
    fn run(&self) -> anyhow::Result<()> {
        validate_module_name(&self.module)?;
        ensure_conn_payload(&self.conn)?;

        let context = resolve_modules_context(&self.path_config)?;
        let mut config = load_config(&context.config_path)?;
        let module_cfg = config.modules.get_mut(&self.module).with_context(|| {
            format!(
                "module '{}' not found in {}",
                self.module,
                context.config_path.display()
            )
        })?;
        let db_cfg = module_cfg.database.as_mut().with_context(|| {
            format!(
                "module '{}' has no database config; use `config mod db add` first",
                self.module
            )
        })?;
        db_cfg.apply_patch(self.conn.clone());

        save_config(&context.config_path, &config)
    }
}

#[derive(Args)]
struct RemoveArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Module name under `modules.<module>`
    module: String,
}

impl RemoveArgs {
    fn run(&self) -> anyhow::Result<()> {
        validate_module_name(&self.module)?;

        let context = resolve_modules_context(&self.path_config)?;
        let mut config = load_config(&context.config_path)?;
        let module_cfg = config.modules.get_mut(&self.module).with_context(|| {
            format!(
                "module '{}' not found in {}",
                self.module,
                context.config_path.display()
            )
        })?;
        if module_cfg.database.take().is_none() {
            let module = &self.module;
            bail!("module '{module}' has no database config");
        }

        save_config(&context.config_path, &config)
    }
}

fn ensure_conn_payload(conn: &DbConnConfig) -> anyhow::Result<()> {
    if conn.has_any_value() {
        return Ok(());
    }
    bail!("no database fields provided");
}
