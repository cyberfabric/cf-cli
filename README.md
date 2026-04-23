# CyberFabric CLI

Command-line interface for development and management of CyberFabric modules.

## Quickstart

### Prerequisites

- Rust toolchain with `cargo` (https://rust-lang.org/tools/install/)

### Install the CLI

This workspace exposes two binaries:

- `cyberfabric`
- `cargo-cyberfabric` for the `cargo cyberfabric ...` invocation form

Install both from the repository root:

```bash
cargo install --git https://github.com/cyberfabric/cf-cli
```

After installation, you can use either form:

```bash
cyberfabric --help
```

```bash
cargo cyberfabric --help
```

## Typical usage flow

First you can create a new workspace with a basic hello-world module with:

```bash
cyberfabric init /tmp/cf-demo
```

You can run it straight away and you will see in the console a hello world message:

```bash
cd /tmp/cf-demo
# When running or building we recommend using cargo-cyberfabric binary instead of the standalone binary.
cargo cyberfabric run -c ./config/quickstart.yml
```

The generated server reads its config path from the `CF_CLI_CONFIG` environment variable. `cyberfabric build` and
`cyberfabric run` set this automatically for the generated project.

Second, add a module to the workspace. You can choose among a set of templates: `background-worker`, `api-db-handler`,
and `rest-gateway`. For this example we'll use background-worker:

```bash
# bring the module to the workspace
cyberfabric mod add background-worker
# add the module to the config
cyberfabric config mod add background-worker -c ./config/quickstart.yml
```

Now, we run it again. We'll see every couple of seconds, the background worker printing a random Pokémon:

```bash
cargo cyberfabric run -c ./config/quickstart.yml
```

You can run the tool from any directory by specifying the path to the workspace with the `-p` flag. The default will be
the current directory. `cargo cyberfabric run -p /tmp/cf-demo -c /tmp/cf-demo/config/quickstart.yml`

## What the CLI can do

The current CLI surface is centered on CyberFabric workspace setup, configuration, code generation, and execution.

### Workspace scaffolding

- `init` initializes a new CyberFabric workspace from a template
- `mod add` adds module templates such as `background-worker`, `api-db-handler`, and `rest-gateway`

### Configuration management

- `config mod list` inspects available and configured modules
- `config mod add` and `config mod rm` manage module entries in the YAML config
- `config mod db add|edit|rm` manages module-level database settings
- `config db add|edit|rm` manages shared database server definitions

You need to provide the path to the configuration file with the `-c` flag. `-c config/quickstart.yml`

### Build and run generated servers

- `build` generates a runnable Cargo project under `.cyberfabric/<CONFIG_NAME>` and builds it based on the `-c`
  configuration
  provided.
- `run` generates the same project and runs it. You can provide `-w` to enable watch mode, `--otel` to enable
  OpenTelemetry, and `--fips` to enable the generated manifest's `fips` feature.
- `build` and `run` both pass `--otel` and `--fips` through as Cargo features on the generated project manifest.

The generated `src/main.rs` does not embed the config path. Instead, the generated server reads it from
`CF_CLI_CONFIG` at runtime. The CLI sets that variable for `build` and `run`, but if you execute `.cyberfabric/<name>/`
or the compiled binary yourself, you need to set `CF_CLI_CONFIG` manually.

Example manual run of the generated project:

```bash
CF_CLI_CONFIG=/tmp/cf-demo/config/quickstart.yml cargo run --manifest-path /tmp/cf-demo/.cyberfabric/quickstart/Cargo.toml
```

### Source inspection

- `docs` resolves Rust source for crates, modules, and items from the workspace, local cache, or `crates.io`

### Linting

- `lint` defaults to running all available lint suites for the current or selected workspace
- `lint --clippy` runs `cargo clippy --workspace --all-targets`
- `lint --strict` is valid when Clippy is active and upgrades Clippy warnings to errors
- `lint -p <PATH> --dylint` runs the embedded CyberFabric Dylint rules against the workspace rooted at the current or
  selected directory
- Passing `--clippy` and/or `--dylint` disables the implicit default `--all` selection unless `--all` is also passed
- `lint` uses `-p/--path` the same way as other workspace-aware commands: it changes the current working directory
  before resolving the target workspace

If the CLI is built without the `dylint-rules` feature, `lint --dylint` returns an error.

### Tool bootstrap

- `tools` installs or upgrades `rustup`, `rustfmt`, and `clippy`

### Current placeholders

- `test` is declared but not implemented yet

## Command overview

For the full command surface, arguments, and examples, check [SKILLS.md](SKILLS.md).

## Local development

To run the CLI from source:

```bash
cargo run -p cli -- --help
```

## License

This project is licensed under the Apache License, Version 2.0.

- Full license text: `LICENSE`
- License URL: <http://www.apache.org/licenses/LICENSE-2.0>

Unless required by applicable law or agreed to in writing, the software is distributed on an `AS IS` basis, without
warranties or conditions of any kind.
