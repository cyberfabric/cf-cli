use anyhow::{Context, bail};
use std::path::Path;
use std::process::Command;

fn run_docker(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("docker")
        .args(args)
        .status()
        .context("failed to run docker — is it installed and on PATH?")?;
    if !status.success() {
        bail!(
            "docker {} exited with {}",
            args.first().unwrap_or(&""),
            status
        );
    }
    Ok(())
}

pub(super) fn docker_build(bundle_dir: &Path, image_ref: &str) -> anyhow::Result<()> {
    println!("Building Docker image {image_ref}…");
    let bundle = bundle_dir.display().to_string();
    run_docker(&["build", "-t", image_ref, &bundle])
}

pub(super) fn docker_push(image_ref: &str) -> anyhow::Result<()> {
    println!("Pushing Docker image {image_ref}…");
    run_docker(&["push", image_ref])
}
