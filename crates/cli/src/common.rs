use clap::Args;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Args)]
pub struct CommonArgs {
    #[arg(short = 'c', long, default_value = "./cyberfabric.yaml")]
    pub config: PathBuf,
}

#[derive(Deserialize)]
pub struct Config {
    pub modules: HashMap<String, ConfigModule>,
}

#[derive(Deserialize)]
pub struct ConfigModule {
    pub metadata: ConfigModuleMetadata,
}

#[derive(Deserialize)]
pub struct ConfigModuleMetadata {
    pub package: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    pub path: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
}
