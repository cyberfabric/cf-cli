use module_parser::ConfigModuleMetadata;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Main application configuration with strongly-typed global sections
/// and a flexible per-module configuration bag.
#[derive(Clone, Deserialize, Serialize)]
pub struct AppConfig {
    /// Core server configuration.
    pub server: ServerConfig,
    /// Typed database configuration (optional).
    #[serde(default)]
    pub database: Option<Value>,
    /// Logging configuration.
    #[serde(default = "default_logging_config")]
    pub logging: LoggingConfig,
    /// Tracing configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracing: Option<Value>,
    /// Directory containing per-module YAML files (optional).
    #[serde(default)]
    pub modules_dir: Option<String>,
    /// Per-module configuration bag: `module_name` -> module config.
    #[serde(default)]
    pub modules: BTreeMap<String, ModuleConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: None,
            logging: default_logging_config(),
            tracing: None,
            modules_dir: None,
            modules: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_home_dir")]
    pub home_dir: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            home_dir: default_home_dir(),
        }
    }
}

/// Logging configuration - maps subsystem names to their logging settings.
pub type LoggingConfig = BTreeMap<String, Value>;

/// Create a default logging configuration.
#[must_use]
pub fn default_logging_config() -> LoggingConfig {
    let mut logging = BTreeMap::new();
    logging.insert(
        "default".to_owned(),
        json!({
            "console_level": "info",
            "file": "logs/cyberfabric.log",
            "file_level": "debug",
            "max_age_days": 7,
            "max_backups": 3,
            "max_size_mb": 100
        }),
    );
    logging
}

/// Small typed view to parse each module entry.
#[derive(Clone, Deserialize, Serialize)]
pub struct ModuleConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<Value>,
    #[serde(default = "default_module_config")]
    pub config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<ModuleRuntime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ConfigModuleMetadata>,
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self {
            database: None,
            config: default_module_config(),
            runtime: None,
            metadata: None,
        }
    }
}

/// Runtime configuration for a module (local vs out-of-process).
#[derive(Clone, Deserialize, Serialize, Default)]
pub struct ModuleRuntime {
    #[serde(default, rename = "type")]
    pub mod_type: RuntimeKind,
    /// Execution configuration for `OoP` modules.
    #[serde(default)]
    pub execution: Option<ExecutionConfig>,
}

/// Execution configuration for out-of-process modules.
#[derive(Clone, Deserialize, Serialize, Default)]
pub struct ExecutionConfig {
    /// Path to the executable. Supports absolute paths or `~` expansion.
    pub executable_path: String,
    /// Command-line arguments to pass to the executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory for the process (optional, defaults to current dir).
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Environment variables to set for the process.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

/// Module runtime kind.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    #[default]
    Local,
    Oop,
}

fn default_home_dir() -> PathBuf {
    PathBuf::from(".cyberfabric")
}

fn default_module_config() -> Value {
    Value::Object(Map::new())
}
