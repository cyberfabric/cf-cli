use crate::common::{cargo_cmd, parse_and_chdir};
use anyhow::{Context, Result};
use clap::Args;

#[cfg(feature = "dylint-rules")]
use std::collections::BTreeSet;
#[cfg(feature = "dylint-rules")]
use std::io::Write;
use std::path::PathBuf;

#[cfg(feature = "dylint-rules")]
mod ensure_toolchain_installed_shared {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/shared/ensure_toolchain_installed.rs"
    ));
}

#[cfg(feature = "dylint-rules")]
use ensure_toolchain_installed_shared::ensure_toolchain_installed;

#[derive(Args)]
pub struct LintArgs {
    /// Run all available lint rules
    #[arg(long)]
    all: bool,
    /// Path to the module workspace root
    #[arg(short = 'p', long, value_parser = parse_and_chdir)]
    pub path: Option<PathBuf>,
    /// Check whether the workspace is formatted with `cargo fmt`.
    #[arg(long)]
    fmt: bool,
    /// Run recommended clippy rules. Follows Cargo.toml exceptions if present.
    #[arg(long)]
    clippy: bool,
    /// Strict mode. Throws an error if any lint rule is triggered.
    #[arg(long)]
    strict: bool,
    /// Run extra lint rules made for cyberfabric modules.
    #[arg(long)]
    dylint: bool,
}

#[cfg(feature = "dylint-rules")]
include!(concat!(env!("OUT_DIR"), "/generated_libs.rs"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EffectiveLintSelection {
    all: bool,
    fmt: bool,
    clippy: bool,
    dylint: bool,
}

impl LintArgs {
    const fn selection(&self) -> EffectiveLintSelection {
        let all = self.all || (!self.fmt && !self.clippy && !self.dylint);
        EffectiveLintSelection {
            all,
            fmt: self.fmt,
            clippy: self.clippy || all,
            dylint: self.dylint || (all && cfg!(feature = "dylint-rules")),
        }
    }

    fn validate(&self) -> Result<EffectiveLintSelection> {
        let selection = self.selection();
        if self.strict && !selection.clippy {
            anyhow::bail!("`--strict` requires `--clippy` or `--all`");
        }
        Ok(selection)
    }

    pub fn run(&self) -> Result<()> {
        let selection = self.validate()?;

        if selection.fmt {
            run_fmt()?;
        }

        if selection.clippy {
            run_clippy(self.strict)?;
        }

        if selection.dylint {
            run_dylint()?;
        }

        Ok(())
    }
}

fn run_fmt() -> Result<()> {
    let mut cmd = cargo_cmd()?;
    cmd.args(["fmt", "--check", "--all"]);

    let status = cmd.status().context("failed to run `cargo fmt --check`")?;
    if !status.success() {
        anyhow::bail!("`cargo fmt --check` failed with exit status {status}");
    }

    Ok(())
}

fn run_clippy(strict: bool) -> Result<()> {
    let mut cmd = cargo_cmd()?;
    cmd.args(["clippy", "--workspace", "--all-targets", "--all-features"]);

    // TODO Analyse the features that each crate has and try to test them against the feature set.

    if strict {
        cmd.arg("--").arg("-D").arg("warnings");
    }

    let status = cmd.status().context("failed to run `cargo clippy`")?;
    if !status.success() {
        anyhow::bail!("`cargo clippy` failed with exit status {status}");
    }

    Ok(())
}

#[cfg(feature = "dylint-rules")]
fn embedded_toolchains() -> Result<BTreeSet<String>> {
    LIBS.iter()
        .map(|(filename, _)| {
            let (_, toolchain_and_ext) = filename
                .rsplit_once('@')
                .with_context(|| format!("missing toolchain marker in `{filename}`"))?;
            let (toolchain, _) = toolchain_and_ext
                .rsplit_once('.')
                .with_context(|| format!("missing library extension in `{filename}`"))?;
            Ok(toolchain.to_owned())
        })
        .collect()
}

#[cfg(feature = "dylint-rules")]
fn run_dylint() -> Result<()> {
    for toolchain in embedded_toolchains()? {
        ensure_toolchain_installed(&toolchain)?;
    }

    // Write every embedded dylib to a per-run temp directory so dylint can
    // dlopen them. The temp dir (and its contents) is removed when `tmp_dir`
    // drops at the end of this function, which is safe because `dylint::run`
    // is synchronous and has already finished using the files by then.
    let tmp_dir = tempfile::tempdir().context("could not create temp dir for dylibs")?;

    let lib_paths: Vec<String> = LIBS
        .iter()
        .map(|(filename, bytes)| {
            let dest = tmp_dir.path().join(filename);
            let mut f = std::fs::File::create(&dest)
                .with_context(|| format!("could not create {filename} in temp dir"))?;
            f.write_all(bytes)
                .with_context(|| format!("could not write {filename} to temp dir"))?;
            Ok(dest.to_string_lossy().into_owned())
        })
        .collect::<Result<_>>()?;

    let opts = dylint::opts::Dylint {
        // Check all packages in the workspace found in the current working
        // directory.  No manifest_path → dylint resolves the workspace from
        // the CWD, which is exactly what we want when the tool is invoked
        // inside a project.
        operation: dylint::opts::Operation::Check(dylint::opts::Check {
            lib_sel: dylint::opts::LibrarySelection {
                // Point directly at the extracted, versioned dylib files.
                // dylint parses the toolchain from each filename so no further
                // discovery or building is necessary.
                lib_paths,
                ..Default::default()
            },
            // Lint the whole workspace, not just the root crate.
            workspace: true,
            ..Default::default()
        }),
        ..Default::default()
    };

    dylint::run(&opts)
}

#[cfg(not(feature = "dylint-rules"))]
fn run_dylint() -> Result<()> {
    anyhow::bail!("dylint-rules feature not enabled")
}

#[cfg(test)]
mod tests {
    use super::LintArgs;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        lint: LintArgs,
    }

    #[test]
    fn defaults_to_all_lints() {
        let cli = TestCli::try_parse_from(["cyberfabric"]).expect("lint args should parse");

        let selection = cli.lint.selection();

        assert!(selection.all);
        assert!(!selection.fmt);
        assert!(selection.clippy);
        #[cfg(feature = "dylint-rules")]
        assert!(selection.dylint);
        #[cfg(not(feature = "dylint-rules"))]
        assert!(!selection.dylint);
    }

    #[test]
    fn explicit_lint_selection_disables_default_all() {
        let cli =
            TestCli::try_parse_from(["cyberfabric", "--dylint"]).expect("lint args should parse");

        let selection = cli.lint.selection();

        assert!(!selection.all);
        assert!(!selection.fmt);
        assert!(!selection.clippy);
        assert!(selection.dylint);
    }

    #[test]
    fn fmt_selection_is_explicit() {
        let cli =
            TestCli::try_parse_from(["cyberfabric", "--fmt"]).expect("lint args should parse");

        let selection = cli.lint.selection();

        assert!(!selection.all);
        assert!(selection.fmt);
        assert!(!selection.clippy);
        assert!(!selection.dylint);
    }

    #[test]
    fn strict_with_clippy_is_accepted() {
        let cli = TestCli::try_parse_from(["cyberfabric", "--clippy", "--strict"])
            .expect("lint args should parse");

        cli.lint
            .validate()
            .expect("strict with clippy should be accepted");
    }

    #[test]
    fn strict_with_all_is_accepted() {
        let cli = TestCli::try_parse_from(["cyberfabric", "--all", "--strict"])
            .expect("lint args should parse");

        cli.lint
            .validate()
            .expect("strict with all should be accepted");
    }

    #[test]
    fn strict_requires_clippy_or_all() {
        let cli = TestCli::try_parse_from(["cyberfabric", "--dylint", "--strict"])
            .expect("lint args should parse");

        let error = cli.lint.validate().expect_err("strict should be rejected");

        assert_eq!(
            error.to_string(),
            "`--strict` requires `--clippy` or `--all`"
        );
    }
}
