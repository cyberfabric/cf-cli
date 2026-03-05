use clap::{Args, Subcommand};

mod app_config;
mod db;
mod modules;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

impl ConfigArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        self.command.run()
    }
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    Mod(modules::ModulesArgs),
    Db(db::DbArgs),
}

impl ConfigCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            Self::Mod(args) => args.run(),
            Self::Db(args) => args.run(),
        }
    }
}
