use clap::{Parser, Subcommand};

mod build;
mod common;
mod lint;
mod r#mod;
mod run;
mod test;
mod tools;

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(name = "cyberfabric")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Mod(r#mod::ModArgs),
    Lint(lint::LintArgs),
    Test(test::TestArgs),
    Tools(tools::ToolsArgs),
    Run(run::RunArgs),
    Build(build::BuildArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Mod(r#mod) => r#mod.run(),
        Commands::Lint(lint) => lint.run(),
        Commands::Test(test) => test.run(),
        Commands::Tools(tools) => tools.run(),
        Commands::Run(run) => run.run(),
        Commands::Build(build) => build.run(),
    }
}
