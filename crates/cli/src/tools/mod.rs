use anyhow::{Context, bail};
use clap::Args;
use std::io::{self, Write};
use std::process::Command;

#[derive(Args)]
pub struct ToolsArgs {
    /// Install all tools
    #[arg(short = 'a', long, conflicts_with = "install")]
    all: bool,
    /// Upgrade tools to the recommended version
    #[arg(short = 'u', long)]
    upgrade: bool,
    /// Install specific tools
    #[arg(long, value_delimiter = ',', conflicts_with = "all")]
    install: Option<Vec<String>>,
    /// Do not ask for confirmation
    #[arg(short = 'y', long)]
    yolo: bool,
    /// Verbose output
    #[arg(short = 'v', long)]
    verbose: bool,
}

struct Tool {
    name: &'static str,
    check_binary: &'static str,
    install: InstallMethod,
}

enum InstallMethod {
    RustupComponent(&'static str),
    Prerequisite,
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "rustup",
        check_binary: "rustup",
        install: InstallMethod::Prerequisite,
    },
    Tool {
        name: "cargofmt",
        check_binary: "rustfmt",
        install: InstallMethod::RustupComponent("rustfmt"),
    },
    Tool {
        name: "clippy",
        check_binary: "cargo-clippy",
        install: InstallMethod::RustupComponent("clippy"),
    },
];

impl ToolsArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let tools = self.resolve_tools()?;

        if self.upgrade {
            return self.upgrade_tools(&tools);
        }

        self.install_tools(&tools)
    }

    fn resolve_tools(&self) -> anyhow::Result<Vec<&'static Tool>> {
        if let Some(names) = &self.install {
            let mut tools = Vec::with_capacity(names.len());
            for name in names {
                let tool = TOOLS
                    .iter()
                    .find(|t| t.name == name.as_str())
                    .with_context(|| {
                        format!(
                            "unknown tool '{}'. known tools: {}",
                            name,
                            TOOLS.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
                        )
                    })?;
                tools.push(tool);
            }
            return Ok(tools);
        }

        if self.all {
            return Ok(TOOLS.iter().collect());
        }

        bail!(
            "no tools specified. Use --all to install all tools, or --install <tool,...> to install specific tools. \
             Known tools: {}",
            TOOLS.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
        )
    }

    fn install_tools(&self, tools: &[&Tool]) -> anyhow::Result<()> {
        ensure_rustup(self.yolo)?;

        for tool in tools {
            let installed = is_installed(tool.check_binary);
            if installed {
                println!("✓ {} is already installed", tool.name);
                continue;
            }

            match tool.install {
                InstallMethod::Prerequisite => {
                    bail!(
                        "'{}' is required but not found. Please install it manually: https://rustup.rs",
                        tool.name
                    );
                }
                InstallMethod::RustupComponent(component) => {
                    if !self.yolo && !confirm(&format!("Install {} via rustup?", tool.name))? {
                        println!("Skipping {}", tool.name);
                        continue;
                    }
                    rustup_component_add(component, self.verbose)?;
                    println!("✓ {} installed", tool.name);
                }
            }
        }

        Ok(())
    }

    fn upgrade_tools(&self, tools: &[&Tool]) -> anyhow::Result<()> {
        ensure_rustup(self.yolo)?;

        let has_rustup = tools.iter().any(|t| t.name == "rustup");
        if has_rustup {
            if !self.yolo && !confirm("Upgrade rustup via 'rustup self update'?")? {
                println!("Skipping rustup upgrade");
            } else {
                run_verbose(
                    Command::new("rustup").arg("self").arg("update"),
                    self.verbose,
                )
                .context("failed to upgrade rustup")?;
                println!("✓ rustup upgraded");
            }
        }

        let components: Vec<_> = tools
            .iter()
            .filter(|t| matches!(t.install, InstallMethod::RustupComponent(_)))
            .collect();

        if !components.is_empty() {
            if !self.yolo && !confirm("Upgrade rustup components via 'rustup update'?")? {
                println!("Skipping component upgrades");
                return Ok(());
            }
            run_verbose(Command::new("rustup").arg("update"), self.verbose)
                .context("failed to run rustup update")?;
            for tool in &components {
                println!("✓ {} upgraded", tool.name);
            }
        }

        Ok(())
    }
}

fn ensure_rustup(yolo: bool) -> anyhow::Result<()> {
    if is_installed("rustup") {
        return Ok(());
    }

    if !yolo && !confirm("rustup is not installed. Install it now?")? {
        bail!(
            "rustup is required but not installed. \
             Please install it manually (https://rustup.rs) or re-run with --yolo to auto-install."
        );
    }

    println!("Installing rustup...");
    install_rustup().context("failed to install rustup")?;

    if !is_installed("rustup") && !is_installed(&cargo_bin_path("rustup")) {
        bail!(
            "rustup was installed but is not available on PATH. \
             Please restart your shell or add ~/.cargo/bin to your PATH."
        );
    }

    println!("✓ rustup installed");
    Ok(())
}

fn cargo_bin_path(binary: &str) -> String {
    let home = std::env::home_dir().unwrap_or_default();
    let bin = if cfg!(target_family = "windows") {
        format!("{binary}.exe")
    } else {
        binary.to_string()
    };
    home.join(".cargo")
        .join("bin")
        .join(bin)
        .to_string_lossy()
        .into_owned()
}

fn install_rustup() -> anyhow::Result<()> {
    if cfg!(target_family = "unix") {
        let status = Command::new("sh")
            .arg("-c")
            .arg("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y")
            .status()
            .context("failed to run rustup installer (is curl installed?)")?;
        if !status.success() {
            bail!("rustup installer exited with {status}");
        }
    } else if cfg!(target_family = "windows") {
        let tmp = std::env::temp_dir().join("rustup-init.exe");
        let status = Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile '{}'",
                    tmp.display()
                ),
            ])
            .status()
            .context("failed to download rustup-init.exe")?;
        if !status.success() {
            bail!("failed to download rustup-init.exe (exit {status})");
        }
        let status = Command::new(&tmp)
            .arg("-y")
            .status()
            .context("failed to run rustup-init.exe")?;
        if !status.success() {
            bail!("rustup-init.exe exited with {status}");
        }
    } else {
        bail!("unsupported platform. Please install rustup manually: https://rustup.rs");
    }

    Ok(())
}

fn is_installed(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn rustup_component_add(component: &str, verbose: bool) -> anyhow::Result<()> {
    run_verbose(
        Command::new("rustup")
            .arg("component")
            .arg("add")
            .arg(component),
        verbose,
    )
    .with_context(|| format!("failed to install rustup component '{component}'"))
}

fn run_verbose(cmd: &mut Command, verbose: bool) -> anyhow::Result<()> {
    if !verbose {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    let status = cmd.status().context("failed to execute command")?;
    if !status.success() {
        bail!("command exited with {status}");
    }
    Ok(())
}

fn confirm(prompt: &str) -> anyhow::Result<bool> {
    print!("{prompt} [Y/n] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();
    Ok(trimmed.is_empty() || trimmed == "y" || trimmed == "yes")
}
