use clap::Args;

#[derive(Args)]
pub struct ToolsArgs {
    #[arg(short = 'a', long)]
    all: bool,
    #[arg(short = 'u', long)]
    upgrade: bool,
    #[arg(long, value_delimiter = ',')]
    install: Option<Vec<String>>,
    /// Do not ask for confirmation
    #[arg(short = 'y', long)]
    yolo: bool,
    /// Verbose output
    #[arg(short = 'v', long)]
    verbose: bool,
}

impl ToolsArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        unimplemented!("Not implemented yet")
    }
}
