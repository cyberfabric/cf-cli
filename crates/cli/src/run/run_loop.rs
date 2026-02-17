use super::templates::{CARGO_CONFIG_TOML, CARGO_SERVER_MAIN, prepare_cargo_server_main};
use anyhow::Context;
use module_parser::{CargoToml, Config, ConfigModuleMetadata, get_module_name_from_crate};
use notify::{Event, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub(super) struct RunLoop {
    path: PathBuf,
    config_path: PathBuf,
}

impl RunLoop {
    pub(super) fn new(path: PathBuf, config_path: PathBuf) -> Self {
        Self { path, config_path }
    }

    pub(super) fn run(&self, watch: bool) -> anyhow::Result<()> {
        let dependencies = get_config(&self.path, &self.config_path)?.create_dependencies()?;
        generate_server_structure(&self.path, &self.config_path, &dependencies)?;

        let dependencies = Arc::new(RwLock::new(dependencies));
        let mut wait_for = vec![];
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        if watch {
            let mut watcher =
                notify::recommended_watcher(tx.clone()).context("can't create watcher")?;
            watcher
                .watch(&self.config_path, RecursiveMode::NonRecursive)
                .context("can't watch path")?;
            wait_for.push(std::thread::spawn(
                move || {
                    while let Ok(Ok(event)) = rx.recv() {}
                },
            ))
        }

        for w in wait_for {
            w.join().expect("can't join thread");
        }

        Ok(())
    }
}

fn get_config(path: &PathBuf, config_path: &PathBuf) -> anyhow::Result<Config> {
    let mut config = get_config_from_path(config_path)?;
    let mut members = get_module_name_from_crate(path)?;

    config.modules.iter_mut().for_each(|module| {
        if let Some(module_metadata) = members.remove(module.0.as_str()) {
            module.1.metadata = module_metadata.metadata;
        }
    });

    Ok(config)
}

fn get_config_from_path(path: &PathBuf) -> anyhow::Result<Config> {
    let config = fs::File::open(path).context("config not available")?;
    serde_saphyr::from_reader(config).context("config not valid")
}

fn create_features() -> HashMap<String, Vec<String>> {
    let mut res = HashMap::with_capacity(2);
    res.insert("default".to_owned(), vec![]);
    res.insert("otel".to_owned(), vec!["modkit/otel".to_owned()]);
    res
}

fn insert_required_deps(
    mut dependencies: HashMap<String, ConfigModuleMetadata>,
) -> HashMap<String, ConfigModuleMetadata> {
    dependencies.insert(
        "modkit".to_owned(),
        ConfigModuleMetadata {
            package: Some("cf-modkit".to_owned()),
            features: vec!["bootstrap".to_owned()],
            ..Default::default()
        },
    );
    dependencies.insert(
        "anyhow".to_owned(),
        ConfigModuleMetadata {
            package: Some("anyhow".to_owned()),
            version: Some("1".to_owned()),
            ..Default::default()
        },
    );
    dependencies.insert(
        "tokio".to_owned(),
        ConfigModuleMetadata {
            package: Some("tokio".to_owned()),
            features: vec!["full".to_owned()],
            version: Some("1".to_owned()),
            ..Default::default()
        },
    );
    dependencies.insert(
        "tracing".to_owned(),
        ConfigModuleMetadata {
            package: Some("tracing".to_owned()),
            version: Some("0.1".to_owned()),
            ..Default::default()
        },
    );
    dependencies.insert(
        "serde_json".to_owned(),
        ConfigModuleMetadata {
            package: Some("serde_json".to_owned()),
            version: Some("1".to_owned()),
            ..Default::default()
        },
    );
    dependencies
}

fn generate_server_structure(
    path: &PathBuf,
    config_path: &PathBuf,
    dependencies: &HashMap<String, ConfigModuleMetadata>,
) -> anyhow::Result<()> {
    // let dependencies = get_config(path, config_path)?.create_dependencies()?;
    let features = create_features();

    let cargo_toml = toml::to_string(&CargoToml {
        dependencies: insert_required_deps(dependencies.clone()),
        features,
        ..Default::default()
    })
    .context("something went wrong when transforming to toml")?;
    let main_template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(CARGO_SERVER_MAIN)?;

    create_file_structure(path, "Cargo.toml", &cargo_toml)?;
    create_file_structure(path, ".cargo/config.toml", CARGO_CONFIG_TOML)?;
    create_file_structure(
        path,
        "src/main.rs",
        &main_template.render(&prepare_cargo_server_main(&config_path, dependencies))?,
    )?;

    Ok(())
}

fn create_file_structure(
    path: &PathBuf,
    relative_path: &str,
    contents: &str,
) -> anyhow::Result<()> {
    const BASE_PATH: &str = ".cyberfabric";
    let path = PathBuf::from(path).join(BASE_PATH).join(relative_path);
    fs::create_dir_all(path.parent().unwrap()).context("can't create directory")?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .context("can't create file")?;
    file.write_all(contents.as_bytes())
        .context("can't write to file")
}
