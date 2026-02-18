mod run_loop;
mod templates;

use crate::common::CommonArgs;
use anyhow::Context;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct RunArgs {
    /// Path to the module to run
    #[arg(short = 'p', long, default_value = ".")]
    path: PathBuf,
    /// Watch for changes
    #[arg(short = 'w', long)]
    watch: bool,
    /// Use OpenTelemetry tracing while running the project
    #[arg(long)]
    otel: bool,
    /// Not supported yet
    #[arg(short = 'r', long, hide = true)]
    release: bool,
    #[command(flatten)]
    common_args: CommonArgs,
}

impl RunArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let config_path = self
            .common_args
            .config
            .canonicalize()
            .context("can't canonicalize path")?;

        let path = self
            .path
            .canonicalize()
            .context("can't canonicalize path")?;

        let rl = run_loop::RunLoop::new(path, config_path);
        run_loop::OTEL.store(self.otel, std::sync::atomic::Ordering::Relaxed);

        rl.run(self.watch)
    }
}
