mod build;
mod common;
mod config;
mod lint;
mod r#mod;
mod run;
mod test;
mod tools;

#[derive(clap::Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(name = "cyberfabric")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    Mod(r#mod::ModArgs),
    Config(Box<config::ConfigArgs>),
    Lint(lint::LintArgs),
    Test(test::TestArgs),
    Tools(tools::ToolsArgs),
    Run(run::RunArgs),
    Build(build::BuildArgs),
}

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        match self.command {
            Commands::Mod(r#mod) => r#mod.run(),
            Commands::Config(config) => config.run(),
            Commands::Lint(lint) => lint.run(),
            Commands::Test(test) => test.run(),
            Commands::Tools(tools) => tools.run(),
            Commands::Run(run) => run.run(),
            Commands::Build(build) => build.run(),
        }
    }
}
