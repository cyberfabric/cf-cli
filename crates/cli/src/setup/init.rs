use anyhow::{Context, bail};
use cargo_generate::{GenerateArgs, TemplatePath, generate};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct InitArgs {
    /// Path to initialize the project
    path: PathBuf,
    /// Verbose output
    #[arg(short = 'v', long)]
    verbose: bool,
}

impl InitArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        if self.path.exists() && !self.path.is_dir() {
            bail!("path is not a directory");
        }
        if !self.path.exists() {
            std::fs::create_dir_all(&self.path).context("path can't be created")?;
        }
        let name = self
            .path
            .file_name()
            .context("path is strange")?
            .to_str()
            .context("name is strange")?;
        generate(GenerateArgs {
            template_path: TemplatePath {
                auto_path: None,
                git: Some("https://github.com/Bechma/cf-template-rust".to_owned()),
                path: None,
                subfolder: Some("Init".to_owned()),
                branch: Some("setup".to_owned()),
                tag: None,
                test: false,
                revision: None,
                favorite: None,
            },
            destination: Some(self.path.clone()),
            overwrite: false,
            init: true,
            name: Some(name.to_owned()),
            quiet: !self.verbose,
            verbose: self.verbose,
            force_git_init: true,
            lib: false,
            no_workspace: true,
            ..Default::default()
        })
        .context("can't generate project")?;
        println!("Project initialized at {}", self.path.display());
        Ok(())
    }
}
