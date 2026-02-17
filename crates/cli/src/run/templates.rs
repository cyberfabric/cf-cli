use module_parser::ConfigModuleMetadata;
use std::collections::HashMap;
use std::path::Path;

pub(super) const CARGO_CONFIG_TOML: &str = r#"[build]
target-dir = "../target"
build-dir = "../target"
"#;

pub(super) const CARGO_SERVER_MAIN: &str = r#"
use anyhow::Result;
use modkit::bootstrap::{
    AppConfig, host::init_logging_unified, /* run_migrate, */ run_server,
};
{{dependencies}}

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::load_or_default(&Some(std::path::PathBuf::from("{{config_path}}")))?;

    // Build OpenTelemetry layer before logging
    // Convert TracingConfig from modkit::bootstrap to modkit's type (they have identical structure)
    #[cfg(feature = "otel")]
    let modkit_tracing_config: Option<modkit::telemetry::TracingConfig> = config
        .tracing
        .as_ref()
        .and_then(|tc| serde_json::to_value(tc).ok())
        .and_then(|v| serde_json::from_value(v).ok());
    #[cfg(feature = "otel")]
    let otel_layer = if let Some(tc) = modkit_tracing_config.as_ref()
        && tc.enabled
    {
        Some(modkit::telemetry::init::init_tracing(tc)?)
    } else {
        None
    };
    #[cfg(not(feature = "otel"))]
    let otel_layer = None;

    // Initialize logging + otel in one Registry
    let logging_config = config.logging.clone().unwrap_or_default();
    init_logging_unified(&logging_config, &config.server.home_dir, otel_layer);

    // One-time connectivity probe
    #[cfg(feature = "otel")]
    if let Some(tc) = modkit_tracing_config.as_ref()
        && let Err(e) = modkit::telemetry::init::otel_connectivity_probe(tc)
    {
        tracing::error!(error = %e, "OTLP connectivity probe failed");
    }

    tracing::info!("CyberFabric Server starting");

    run_server(config).await
}"#;

pub(super) fn prepare_cargo_server_main(
    config_path: &Path,
    dependencies: &HashMap<String, ConfigModuleMetadata>,
) -> liquid::Object {
    let dependencies = dependencies
        .keys()
        .map(|name| format!("use {name} as _;\n"))
        .collect::<String>();

    liquid::object!({
        "dependencies": dependencies,
        "config_path": config_path.display().to_string(),
    })
}
