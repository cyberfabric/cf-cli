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

pub(super) fn docker_build(
    bundle_dir: &Path,
    image_ref: &str,
    build_args: &[String],
) -> anyhow::Result<()> {
    println!("Building Docker image {image_ref}…");
    let bundle = bundle_dir.display().to_string();
    let mut args = vec!["build", "-t", image_ref];
    for arg in build_args {
        args.push("--build-arg");
        args.push(arg);
    }
    args.push(&bundle);
    run_docker(&args)
}

pub(super) fn docker_push(image_ref: &str) -> anyhow::Result<()> {
    println!("Pushing Docker image {image_ref}…");
    run_docker(&["push", image_ref])
}
