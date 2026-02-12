use crate::common::CommonArgs;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct RunArgs {
    /// Path to the module to run
    #[arg(short = 'p', long, default_value = ".")]
    path: PathBuf,
    /// Not supported yet
    #[arg(short = 'r', long, hide = true)]
    release: bool,
    #[command(flatten)]
    common_args: CommonArgs,
}

impl RunArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        unimplemented!("Not implemented yet")
    }
}
