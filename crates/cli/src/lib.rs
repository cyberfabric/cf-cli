mod build;
mod common;
mod config;
mod docs;
mod init;
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
    Init(init::InitArgs),
    Mod(r#mod::ModArgs),
    Config(Box<config::ConfigArgs>),
    Docs(docs::DocsArgs),
    Lint(lint::LintArgs),
    Test(test::TestArgs),
    Tools(tools::ToolsArgs),
    Run(run::RunArgs),
    Build(build::BuildArgs),
}

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        match self.command {
            Commands::Init(init) => init.run(),
            Commands::Mod(r#mod) => r#mod.run(),
            Commands::Config(config) => config.run(),
            Commands::Docs(docs) => docs.run(),
            Commands::Lint(lint) => lint.run(),
            Commands::Test(test) => test.run(),
            Commands::Tools(tools) => tools.run(),
            Commands::Run(run) => run.run(),
            Commands::Build(build) => build.run(),
        }
    }
}
