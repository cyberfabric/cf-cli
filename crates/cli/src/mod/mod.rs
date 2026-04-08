use clap::{Args, Subcommand};

mod add;

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
    Add(add::AddArgs),
}

impl ModCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            Self::Add(args) => args.run(),
        }
    }
}
