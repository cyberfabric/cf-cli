use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct ModuleArgs {
    path: PathBuf,
}

impl ModuleArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        todo!("gimme time")
    }
}
