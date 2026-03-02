mod run_loop;

use crate::common::BuildRunArgs;
use crate::run::run_loop::RunSignal;
use anyhow::Context;
use clap::Args;

#[derive(Args)]
pub struct RunArgs {
    /// Watch for changes
    #[arg(short = 'w', long)]
    watch: bool,
    #[command(flatten)]
    br_args: BuildRunArgs,
}

impl RunArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let path = self
            .br_args
            .path
            .canonicalize()
            .context("can't canonicalize workspace")?;

        let config_path = self
            .br_args
            .config
            .canonicalize()
            .context("can't canonicalize config")?;

        let rl = run_loop::RunLoop::new(path, config_path);
        run_loop::OTEL.store(self.br_args.otel, std::sync::atomic::Ordering::Relaxed);
        run_loop::RELEASE.store(self.br_args.release, std::sync::atomic::Ordering::Relaxed);

        loop {
            match rl.run(self.watch)? {
                RunSignal::Rerun => {}
                RunSignal::Stop => break Ok(()),
            }
        }
    }
}
