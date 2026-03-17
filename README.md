# CyberFabric CLI

Command-line interface for development and management of CyberFabric modules.

## Quickstart

### Prerequisites

- Rust toolchain with `cargo`
- A local clone of this repository

### Install the CLI

This workspace exposes two binaries:

- `cyberfabric`
- `cargo-cyberfabric` for the `cargo cyberfabric ...` invocation form

Install both from the repository root:

```bash
cargo install --path crates/cli --bin cyberfabric --bin cargo-cyberfabric
```

After installation, you can use either form:

```bash
cyberfabric --help
```

```bash
cargo cyberfabric --help
```

For local development without installing:

```bash
cargo run -p cli -- --help
```

## What the CLI can do

The current CLI surface is centered on CyberFabric workspace setup, configuration, code generation, and execution.

### Workspace scaffolding

- `mod init` initializes a new CyberFabric workspace from a template
- `mod add` adds module templates such as `background-worker`, `api-db-handler`, and `rest-gateway`

### Configuration management

- `config mod list` inspects available and configured modules
- `config mod add` and `config mod rm` manage module entries in the YAML config
- `config mod db add|edit|rm` manages module-level database settings
- `config db add|edit|rm` manages shared database server definitions

You need to provide the path to the configuration file with the `-c` flag. `-c config/quickstart.yml`

### Build and run generated servers

- `build` generates a runnable Cargo project under `.cyberfabric/` and builds it based on the `-c` configuration
  provided.
- `run` generates the same project and runs it. You can provide `-w` to enable watch mode and/or `--otel` to enable
  OpenTelemetry.

### Source inspection

- `docs` resolves Rust source for crates, modules, and items from the workspace, local cache, or `crates.io`

### Tool bootstrap

- `tools` installs or upgrades `rustup`, `rustfmt`, and `clippy`

### Current placeholders

- `lint` is declared but not implemented yet
- `test` is declared but not implemented yet

## Typical usage flow

Create a workspace, add a module, configure it, and run it:

```bash
cyberfabric mod init /tmp/cf-demo
cyberfabric mod add background-worker -p /tmp/cf-demo
cyberfabric config mod add background-worker -p /tmp/cf-demo -c /tmp/cf-demo/config/quickstart.yml
cyberfabric run -p /tmp/cf-demo -c /tmp/cf-demo/config/quickstart.yml
```

The `-p` is to specify the path. If you don't provide it, the default will be the current directory.

## Command overview

For the full command surface, arguments, and examples, check [SKILLS.md](SKILLS.md).

## License

This project is licensed under the Apache License, Version 2.0.

- Full license text: `LICENSE`
- License URL: <http://www.apache.org/licenses/LICENSE-2.0>

Unless required by applicable law or agreed to in writing, the software is distributed on an `AS IS` basis, without
warranties or conditions of any kind.
