use crate::common::{self, BuildRunArgs};
use anyhow::{Context, bail};
use clap::Args;

#[derive(Args)]
pub struct BuildArgs {
    #[command(flatten)]
    build_run_args: BuildRunArgs,
}

impl BuildArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let path = self
            .build_run_args
            .path
            .canonicalize()
            .context("can't canonicalize workspace")?;

        let config_path = self
            .build_run_args
            .config
            .canonicalize()
            .context("can't canonicalize config")?;

        let dependencies = common::get_config(&path, &config_path)?.create_dependencies()?;
        common::generate_server_structure(&path, &config_path, &dependencies)?;

        let cargo_dir = path.join(common::BASE_PATH);
        let status = common::cargo_command(
            "build",
            &cargo_dir,
            self.build_run_args.otel,
            self.build_run_args.release,
        )
        .status()
        .context("failed to run cargo build")?;

        if !status.success() {
            bail!("cargo build exited with {status}");
        }

        Ok(())
    }
}
