use super::templates::{CARGO_CONFIG_TOML, CARGO_SERVER_MAIN, prepare_cargo_server_main};
use anyhow::{Context, bail};
use module_parser::{CargoToml, Config, ConfigModuleMetadata, get_module_name_from_crate};
use notify::{RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Duration;

enum RunSignal {
    Rerun,
    Stop,
}

pub(super) struct RunLoop {
    path: PathBuf,
    config_path: PathBuf,
}

const BASE_PATH: &str = ".cyberfabric";

pub(super) static OTEL: AtomicBool = AtomicBool::new(false);

impl RunLoop {
    pub(super) fn new(path: PathBuf, config_path: PathBuf) -> Self {
        Self { path, config_path }
    }

    pub(super) fn run(&self, watch: bool) -> anyhow::Result<()> {
        let dependencies = get_config(&self.path, &self.config_path)?.create_dependencies()?;
        generate_server_structure(&self.path, &self.config_path, &dependencies)?;

        let cargo_dir = self.path.join(BASE_PATH);

        if !watch {
            let status = cargo_run(&cargo_dir)
                .status()
                .context("failed to run cargo")?;
            if !status.success() {
                bail!("cargo run exited with {status}");
            }
            return Ok(());
        }

        // -- watch mode --

        let (signal_tx, signal_rx) = mpsc::channel::<RunSignal>();

        // Spawn cargo-run loop in a dedicated thread
        let cargo_dir_clone = cargo_dir.clone();
        let runner_handle = std::thread::spawn(move || {
            cargo_run_loop(&cargo_dir_clone, &signal_rx);
        });

        // File-system watcher
        let (fs_tx, fs_rx) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(fs_tx).context("failed to create file watcher")?;

        // Watch the config file itself
        watcher
            .watch(&self.config_path, RecursiveMode::NonRecursive)
            .context("failed to watch config file")?;

        // Watch dependency paths that have `path` set
        let mut watched_paths = watch_dependency_paths(&dependencies, &mut watcher, &self.path);
        let mut current_deps = dependencies;

        // Event loop - runs until the watcher channel closes
        while let Ok(Ok(event)) = fs_rx.recv() {
            let is_config_change = event.paths.contains(&self.config_path);

            if is_config_change {
                match get_config(&self.path, &self.config_path)
                    .and_then(|c| c.create_dependencies())
                {
                    Ok(new_deps) => {
                        if new_deps != current_deps {
                            if let Err(e) =
                                generate_server_structure(&self.path, &self.config_path, &new_deps)
                            {
                                eprintln!("failed to regenerate server structure: {e}");
                            }

                            // Reconcile watched dependency paths
                            let new_watched = collect_dep_paths(&new_deps, &self.path);
                            for old in watched_paths.difference(&new_watched) {
                                let _ = watcher.unwatch(old);
                            }
                            for new_p in new_watched.difference(&watched_paths) {
                                let _ = watcher.watch(new_p, RecursiveMode::Recursive);
                            }
                            watched_paths = new_watched;
                            current_deps = new_deps;
                        }
                        let _ = signal_tx.send(RunSignal::Rerun);
                    }
                    Err(e) => eprintln!("failed to reload config: {e}"),
                }
            } else {
                // A watched dependency path changed
                let _ = signal_tx.send(RunSignal::Rerun);
            }
        }

        // Watcher channel closed - shut down the runner
        let _ = signal_tx.send(RunSignal::Stop);
        runner_handle.join().expect("runner thread panicked");

        Ok(())
    }
}

fn cargo_run(path: &Path) -> Command {
    let otel = OTEL.load(std::sync::atomic::Ordering::Relaxed);
    let cargo = std::env::var("CARGO").unwrap_or("cargo".to_owned());
    let mut cmd = Command::new(cargo);
    cmd.arg("run");
    if otel {
        cmd.arg("-F").arg("otel");
    }
    cmd.current_dir(path);
    cmd
}

fn cargo_run_loop(cargo_dir: &PathBuf, signal_rx: &mpsc::Receiver<RunSignal>) {
    'outer: loop {
        let mut child = match cargo_run(cargo_dir).spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("failed to spawn cargo run: {e}");
                match signal_rx.recv() {
                    Ok(RunSignal::Rerun) => continue 'outer,
                    _ => return,
                }
            }
        };

        let rerun = loop {
            match child.try_wait() {
                Ok(Some(_)) => break false,
                Ok(None) => {}
                Err(e) => {
                    eprintln!("error checking child status: {e}");
                    break false;
                }
            }

            match signal_rx.try_recv() {
                Ok(RunSignal::Rerun) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break true;
                }
                Ok(RunSignal::Stop) | Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }

            std::thread::sleep(Duration::from_millis(100));
        };

        if rerun {
            continue 'outer;
        }

        // Child exited on its own, wait for a signal before restarting
        match signal_rx.recv() {
            Ok(RunSignal::Rerun) => continue 'outer,
            _ => return,
        }
    }
}

fn collect_dep_paths(
    deps: &HashMap<String, ConfigModuleMetadata>,
    base_path: &Path,
) -> HashSet<PathBuf> {
    deps.values()
        .filter_map(|d| d.path.as_ref())
        .map(|p| base_path.join(p))
        .collect()
}

fn watch_dependency_paths(
    deps: &HashMap<String, ConfigModuleMetadata>,
    watcher: &mut impl Watcher,
    base_path: &Path,
) -> HashSet<PathBuf> {
    let paths = collect_dep_paths(deps, base_path);
    for p in &paths {
        if let Err(e) = watcher.watch(p, RecursiveMode::Recursive) {
            eprintln!("failed to watch {}: {e}", p.display());
        }
    }
    paths
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
