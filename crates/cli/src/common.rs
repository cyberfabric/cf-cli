use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct CommonArgs {
    #[arg(short = 'c', long, default_value = "./cyberfabric.yaml")]
    pub config: PathBuf,
}
