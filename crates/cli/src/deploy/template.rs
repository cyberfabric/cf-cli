use anyhow::Context;
use cargo_generate::{GenerateArgs, TemplatePath, Vcs, generate};
use std::path::Path;

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
    source: &TemplateSource<'_>,
) -> anyhow::Result<()> {
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
        ..GenerateArgs::default()
    })
    .context("can't render deploy template")?;

    Ok(())
}
