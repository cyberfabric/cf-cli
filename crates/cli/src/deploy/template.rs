use anyhow::Context;
use cargo_generate::{GenerateArgs, TemplatePath, Vcs, generate};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::DeployTemplateKind;

pub(super) struct TemplateSource<'a> {
    pub local_path: Option<&'a str>,
    pub git: Option<&'a str>,
    pub subfolder: &'a str,
    pub kind: DeployTemplateKind,
    pub branch: Option<&'a str>,
}

pub(super) fn render_deploy_template(
    output_dir: &Path,
    project_name: &str,
    local_paths: &BTreeSet<PathBuf>,
    has_cargo_lock: bool,
    source: &TemplateSource<'_>,
) -> anyhow::Result<()> {
    let generated_project_dir = format!(".cyberfabric/{project_name}");

    let copy_cargo_lock = if has_cargo_lock {
        "COPY Cargo.lock Cargo.lock\n".to_owned()
    } else {
        String::new()
    };

    let copy_local_paths = local_paths
        .iter()
        .map(|p| {
            let p = p.display().to_string().replace('\\', "/");
            format!("COPY {p} {p}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let values_path = output_dir.join(".cargo-generate-values.toml");
    fs::write(
        &values_path,
        format!(
            "[values]\ngenerated_project_dir = {gen}\nexecutable_name = {exe}\ncopy_cargo_lock = {lock}\ncopy_local_paths = {paths}\n",
            gen = toml::Value::from(&*generated_project_dir),
            exe = toml::Value::from(project_name),
            lock = toml::Value::from(&*copy_cargo_lock),
            paths = toml::Value::from(&*copy_local_paths),
        ),
    )
    .context("can't write template values file")?;

    let auto_path = format!("{}/{}", source.subfolder, source.kind.as_str());

    let (git, branch) = if source.local_path.is_some() {
        (None, None)
    } else {
        (
            source.git.map(ToOwned::to_owned),
            source.branch.map(ToOwned::to_owned),
        )
    };

    generate(GenerateArgs {
        template_path: TemplatePath {
            auto_path: Some(auto_path),
            git,
            path: source.local_path.map(ToOwned::to_owned),
            branch,
            ..TemplatePath::default()
        },
        destination: Some(output_dir.to_path_buf()),
        name: Some(project_name.to_owned()),
        force: true,
        silent: true,
        vcs: Some(Vcs::None),
        init: true,
        overwrite: true,
        no_workspace: true,
        template_values_file: Some(values_path.display().to_string()),
        ..GenerateArgs::default()
    })
    .context("can't render deploy template")?;

    let _ = fs::remove_file(&values_path);
    Ok(())
}
