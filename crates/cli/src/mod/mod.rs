use clap::{Args, Subcommand};

mod add;
mod init;

#[derive(Args)]
pub struct ModArgs {
    #[command(subcommand)]
    command: ModCommand,
}

impl ModArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        self.command.run()
    }
}

#[derive(Subcommand)]
pub enum ModCommand {
    Init(init::InitArgs),
    Add(add::AddArgs),
}

impl ModCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            Self::Init(args) => args.run(),
            Self::Add(args) => args.run(),
        }
    }
}
