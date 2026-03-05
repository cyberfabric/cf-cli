use super::app_config::{AppConfig, DbConnConfig};
use crate::common::PathConfigArgs;
use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use std::fs;
use std::path::Path;

#[derive(Args)]
pub struct DbArgs {
    #[command(subcommand)]
    command: DbCommand,
}

#[derive(Subcommand)]
enum DbCommand {
    /// Add or update (upsert) a global database server config under `database.servers`
    Add(AddArgs),
    /// Edit a global database server config under `database.servers`
    Edit(EditArgs),
    /// Remove a global database server config from `database.servers`
    Rm(RemoveArgs),
}

impl DbArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            DbCommand::Add(args) => args.run(),
            DbCommand::Edit(args) => args.run(),
            DbCommand::Rm(args) => args.run(),
        }
    }
}

#[derive(Args)]
struct AddArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Server name under `database.servers.<name>`
    name: String,
    #[command(flatten)]
    conn: DbConnConfig,
}

impl AddArgs {
    fn run(&self) -> anyhow::Result<()> {
        let config_path = self.path_config.resolve_config_required()?;
        validate_identifier(&self.name, "server")?;
        ensure_conn_payload(&self.conn)?;

        let mut config = load_config(&config_path)?;
        let database = config.database.get_or_insert_default();
        if let Some(existing) = database.servers.get_mut(&self.name) {
            existing.apply_patch(self.conn.clone());
        } else {
            database
                .servers
                .insert(self.name.clone(), self.conn.clone());
        }
        save_config(&config_path, &config)
    }
}

#[derive(Args)]
struct EditArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Server name under `database.servers.<name>`
    name: String,
    #[command(flatten)]
    conn: DbConnConfig,
}

impl EditArgs {
    fn run(&self) -> anyhow::Result<()> {
        let config_path = self.path_config.resolve_config_required()?;
        validate_identifier(&self.name, "server")?;
        ensure_conn_payload(&self.conn)?;

        let mut config = load_config(&config_path)?;
        let database = config
            .database
            .as_mut()
            .context("global database config is missing; use `config db add` first")?;
        let existing = database.servers.get_mut(&self.name).with_context(|| {
            format!(
                "database server '{}' not found in {}",
                self.name,
                config_path.display()
            )
        })?;
        existing.apply_patch(self.conn.clone());

        save_config(&config_path, &config)
    }
}

#[derive(Args)]
struct RemoveArgs {
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Server name under `database.servers.<name>`
    name: String,
}

impl RemoveArgs {
    fn run(&self) -> anyhow::Result<()> {
        let config_path = self.path_config.resolve_config_required()?;
        validate_identifier(&self.name, "server")?;

        let mut config = load_config(&config_path)?;
        let Some(database) = config.database.as_mut() else {
            bail!("global database config is missing");
        };

        if database.servers.remove(&self.name).is_none() {
            let name = &self.name;
            bail!("database server '{name}' not found");
        }

        if database.servers.is_empty() && database.auto_provision.is_none() {
            config.database = None;
        }

        save_config(&config_path, &config)
    }
}

fn ensure_conn_payload(conn: &DbConnConfig) -> anyhow::Result<()> {
    if conn.has_any_value() {
        return Ok(());
    }
    bail!("no database fields provided");
}

fn load_config(path: &Path) -> anyhow::Result<AppConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("can't read config file {}", path.display()))?;
    serde_saphyr::from_str(&raw).with_context(|| format!("config not valid at {}", path.display()))
}

fn save_config(path: &Path, config: &AppConfig) -> anyhow::Result<()> {
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

fn validate_identifier(value: &str, kind: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("invalid {kind} name '{value}'. Use only letters, numbers, '-' and '_'");
    }
    Ok(())
}
