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
cyberfabric mod init /tmp/cf-demo
```

You can run it straight away and you will see in the console a hello world message:

```bash
cd /tmp/cf-demo
# When running or building we recommend using cargo-cyberfabric binary instead of the standalone binary.
cargo cyberfabric run -c ./config/quickstart.yml
```

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
