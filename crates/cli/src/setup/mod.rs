use clap::{Args, Subcommand};

mod config;
mod init;
mod module;
mod tools;

#[derive(Args)]
pub struct SetupArgs {
    #[command(subcommand)]
    command: SetupCommand,
}

impl SetupArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        self.command.run()
    }
}

#[derive(Subcommand)]
pub enum SetupCommand {
    Tools(tools::ToolsArgs),
    Init(init::InitArgs),
    Module(module::ModuleArgs),
    Config(config::ConfigArgs),
}

impl SetupCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            SetupCommand::Tools(args) => args.run(),
            SetupCommand::Init(args) => args.run(),
            SetupCommand::Module(args) => args.run(),
            SetupCommand::Config(args) => args.run(),
        }
    }
}
