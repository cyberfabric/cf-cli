mod bundle;
mod docker;
mod template;

use crate::common::{self, PathConfigArgs};
use clap::{Args, ValueEnum};
use std::fs;
use std::path::{Path, PathBuf};

use bundle::{
    collect_required_local_paths, copy_file, copy_optional_file, copy_relative_workspace_path,
    prepare_output_dir, rewrite_dependency_paths_for_bundle,
};
use docker::{docker_build, docker_push};
use template::{TemplateSource, render_deploy_template};

const OUTPUT_SUBDIR: &str = "deploy";

#[derive(Args)]
pub struct DeployArgs {
    /// Deployment template kind to render
    #[arg(long, value_enum)]
    template: DeployTemplateKind,
    #[command(flatten)]
    path_config: PathConfigArgs,
    /// Override the generated server and binary name
    #[arg(long)]
    name: Option<String>,
    /// Output directory for the generated deploy bundle
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Allow replacing an existing custom output directory
    #[arg(long)]
    force: bool,
    /// Path to a local deploy template repository
    #[arg(long)]
    local_path: Option<String>,
    /// URL to the template git repo
    #[arg(
        long,
        default_value = "https://github.com/cyberfabric/cf-template-rust"
    )]
    git: Option<String>,
    /// Subfolder relative to the template repo root
    #[arg(long, default_value = "Deploy")]
    subfolder: String,
    /// Branch of the template git repo
    #[arg(long, default_value = "main")]
    branch: Option<String>,
    /// Build the Docker image from the generated bundle
    #[arg(long)]
    build: bool,
    /// Tag the built image with a given reference (implies --build)
    #[arg(long)]
    tag: Option<String>,
    /// Push the tagged image to a registry (requires --tag)
    #[arg(long, requires = "tag")]
    push: bool,
    /// Cargo feature flags passed to the Docker build
    #[arg(long)]
    features: Option<String>,
    /// Additional Docker build arguments (KEY=VALUE)
    #[arg(long = "docker-arg", value_name = "KEY=VALUE")]
    docker_args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DeployTemplateKind {
    #[value(name = "docker")]
    Docker,
}

impl DeployTemplateKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
        }
    }
}

impl DeployArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        let (workspace_root, config_path, project_name) =
            common::resolve_workspace_config_and_name(&self.path_config, self.name.as_deref())?;
        let config = common::get_config(&config_path)?;
        let dependencies = config.create_dependencies()?;
        let output_dir = self.resolve_output_dir(&workspace_root, &project_name);
        prepare_output_dir(&output_dir, &workspace_root, self.force)?;

        copy_file(
            &workspace_root.join("Cargo.toml"),
            &output_dir.join("Cargo.toml"),
        )?;
        copy_optional_file(
            &workspace_root.join("Cargo.lock"),
            &output_dir.join("Cargo.lock"),
        )?;
        let config_bundle_path = Path::new("config").join(&project_name);
        copy_file(&config_path, &output_dir.join(&config_bundle_path))?;

        let local_paths = collect_required_local_paths(&workspace_root, &dependencies)?;
        for relative_path in &local_paths {
            copy_relative_workspace_path(&workspace_root, &output_dir, relative_path)?;
        }
        fs::create_dir_all(output_dir.join("modules")).ok();

        let rewritten_dependencies =
            rewrite_dependency_paths_for_bundle(&workspace_root, &dependencies)?;
        common::generate_server_structure_at(&output_dir, &project_name, &rewritten_dependencies)?;

        render_deploy_template(
            &output_dir,
            &project_name,
            &TemplateSource {
                local_path: self.local_path.as_deref(),
                git: self.git.as_deref(),
                subfolder: &self.subfolder,
                kind: self.template,
                branch: self.branch.as_deref(),
            },
        )?;

        println!("Deploy bundle generated at {}", output_dir.display());

        let should_build = self.build || self.tag.is_some();
        if should_build {
            let default_ref = format!("{project_name}:latest");
            let image_ref = self.tag.as_deref().unwrap_or(&default_ref);
            let mut build_args = vec![format!("ARTIFACT_NAME={project_name}")];
            if let Some(features) = &self.features {
                build_args.push(format!("CYBERFABRIC_FEATURES={features}"));
            }
            build_args.extend(self.docker_args.iter().cloned());
            docker_build(&output_dir, image_ref, &build_args)?;
            if self.push {
                docker_push(image_ref)?;
            }
        }

        Ok(())
    }

    fn resolve_output_dir(&self, workspace_root: &Path, project_name: &str) -> PathBuf {
        self.output_dir.as_ref().map_or_else(
            || {
                workspace_root
                    .join(common::BASE_PATH)
                    .join(project_name)
                    .join(OUTPUT_SUBDIR)
            },
            |path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    workspace_root.join(path)
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{DeployArgs, DeployTemplateKind};
    use crate::common::PathConfigArgs;
    use module_parser::test_utils::TempDirExt;
    use std::fs;
    use std::path::PathBuf;

    struct CurrentDirGuard(PathBuf);

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn deploy_generates_docker_bundle_from_local_templates() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp_dir.path();
        temp_dir.write(
            "Cargo.toml",
            r#"[workspace]
members = ["modules/demo-module"]
resolver = "2"
"#,
        );
        temp_dir.write("Cargo.lock", "version = 4\n");
        temp_dir.write(
            "config/quickstart.yml",
            "modules:\n  demo:\n    metadata: {}\n",
        );
        temp_dir.write(
            "modules/demo-module/Cargo.toml",
            r#"[package]
name = "demo-module"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
        );
        temp_dir.write("modules/demo-module/src/lib.rs", "pub mod module;\n");
        temp_dir.write(
            "modules/demo-module/src/module.rs",
            "#[module(name = \"demo\")]\npub struct DemoModule;\n",
        );
        temp_dir.write(
            "templates/Deploy/docker/cargo-generate.toml",
            "[template]\nexclude = [\"**/.DS_Store\"]\n",
        );
        temp_dir.write(
            "templates/Deploy/docker/Dockerfile",
            "ARG ARTIFACT_NAME\nCOPY Cargo.toml Cargo.toml\nCOPY .cyberfabric/${ARTIFACT_NAME}/ .cyberfabric/${ARTIFACT_NAME}/\nCOPY modules/ modules/\nCOPY config/${ARTIFACT_NAME} config/${ARTIFACT_NAME}\nCMD [\"/srv/server\"]\n",
        );

        // chdir so workspace_root() resolves to the temp directory;
        // the guard restores the original CWD on drop to avoid leaking
        // process-global state to other parallel tests.
        let _cwd_guard = CurrentDirGuard(std::env::current_dir().expect("current dir"));
        std::env::set_current_dir(workspace_root).expect("chdir into temp workspace");

        let output_dir = workspace_root.join("bundle");
        let args = DeployArgs {
            template: DeployTemplateKind::Docker,
            path_config: PathConfigArgs {
                path: Some(workspace_root.to_path_buf()),
                config: workspace_root.join("config/quickstart.yml"),
            },
            name: Some("demo".to_owned()),
            output_dir: Some(output_dir.clone()),
            force: false,
            local_path: Some(workspace_root.join("templates").display().to_string()),
            git: None,
            subfolder: "Deploy".to_owned(),
            branch: None,
            build: false,
            tag: None,
            push: false,
            features: None,
            docker_args: vec![],
        };

        args.run().expect("deploy run");

        let generated_cargo_toml = output_dir.join(".cyberfabric/demo/Cargo.toml");
        let generated_main = output_dir.join(".cyberfabric/demo/src/main.rs");
        let dockerfile = output_dir.join("Dockerfile");

        assert!(generated_cargo_toml.is_file());
        assert!(generated_main.is_file());
        assert!(dockerfile.is_file());
        assert!(output_dir.join("config/demo").is_file());
        assert!(output_dir.join("Cargo.toml").is_file());
        assert!(output_dir.join("Cargo.lock").is_file());
        assert!(output_dir.join("modules/demo-module/Cargo.toml").is_file());

        let generated_cargo_toml =
            fs::read_to_string(&generated_cargo_toml).expect("generated cargo toml");
        assert!(generated_cargo_toml.contains("../../modules/demo-module"));

        let generated_main = fs::read_to_string(&generated_main).expect("generated main");
        assert!(generated_main.contains("CF_CLI_CONFIG"));
        assert!(generated_main.contains("run_server(config)"));

        let dockerfile = fs::read_to_string(&dockerfile).expect("dockerfile");
        assert!(dockerfile.contains("ARG ARTIFACT_NAME"));
        assert!(dockerfile.contains("COPY Cargo.toml Cargo.toml"));
        assert!(dockerfile.contains("COPY modules/ modules/"));
        assert!(dockerfile.contains("COPY config/${ARTIFACT_NAME} config/${ARTIFACT_NAME}"));
        assert!(dockerfile.contains("CMD [\"/srv/server\"]"));
    }
}
