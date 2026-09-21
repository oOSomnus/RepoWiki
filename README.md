# RepoWiki

[简体中文](documentation/README.zh-CN.md) · [Development notes](documentation/development.md) · [Architecture examples](skill/references/few-shots/README.md)

RepoWiki is an Agent Skill for generating repository wikis. It combines a
portable Rust analyzer with runtime instructions that let an agent inspect a
repository, organize its modules, and write the resulting documentation.

## Build and install

Requirements: GNU Make, Rust/Cargo, Python 3, `zip`, and `unzip`.

```bash
# Build the platform-specific executable and preview package.
make preview

# Build and validate the distributable ZIP archive.
make build

# Build, validate, and install to ~/.agents/skills/RepoWiki.
make install

# Install below a different Skill collection directory.
make install INSTALL_DIR=/custom/agent/skills

# Remove generated build artifacts.
make clean
```

Build the package on the operating system where it will run. An installed
Skill uses the bundled executable and does not require Cargo on the target
machine.

## Verify changes

```bash
# Offline replay, Rust checks, and package validation.
make test

# Local architecture prompt contract only.
make test-contract

# Temporary-directory installation smoke test.
make test-install

```

## Documentation

- [中文说明](documentation/README.zh-CN.md)
- [Development notes](documentation/development.md)
- [Runtime Skill instructions](skill/SKILL.md)
- [Reference-derived architecture examples](skill/references/few-shots/README.md)

## Standalone reader

Build and run the local read-only WebUI for an existing `.repowiki` directory:

```bash
make reader
.build/cargo-target/release/repowiki-reader /path/to/project/.repowiki
```

The reader listens on loopback, opens the default browser, and does not modify
the wiki. Use `--no-open` to print the URL without launching a browser or
`--port <port>` to choose a port.
