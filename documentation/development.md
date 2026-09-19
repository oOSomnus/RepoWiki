# Development notes

[Back to README](../README.md) · [中文说明](README.zh-CN.md)

This document keeps repository-maintenance details out of the project landing
page. The root README is for the project overview and common commands; this
page is for contributors working on the build, tests, and package layout.

## Repository structure

- `engine/` contains the Rust analyzer, CLI, embedded prompt sources, and Rust
  tests.
- `skill/` contains the runtime Skill source: `SKILL.md`, agent metadata, and
  references.
- `tools/` contains development-time replay, differential, and package
  validation tools.
- `reference/CodeWiki/` is the pinned read-only reference implementation used
  by the optional differential check.
- `preview/`, `dist/`, and `.build/` are generated locally and are not runtime
  source directories.

## Build pipeline

`make preview` builds the release `codewiki` executable and copies the runtime
Skill source into `preview/`. The binary is platform-specific, so build the
package on the platform where it will be installed.

`make build` packages `preview/` as `dist/RepoWiki-<version>.zip` and validates
both the preview directory and the archive. The runtime package contains:

```text
SKILL.md
agents/openai.yaml
references/cli-contract.md
references/prompt-map.md
scripts/codewiki                  # or scripts/codewiki.exe
```

The Rust prompt sources are embedded into the executable during the build.

`make install` builds the package, validates it, and installs it below
`INSTALL_DIR` (default: `~/.agents/skills`). Installation is staged and
validated before the existing `<INSTALL_DIR>/RepoWiki` directory is replaced.

To install an archive manually:

```bash
make build
mkdir -p ~/.agents/skills/RepoWiki
unzip dist/RepoWiki-*.zip -d ~/.agents/skills/RepoWiki
```

## Verification

`make test` runs the offline replay against the fixed 13-language fixture,
compares the result with the checked-in golden data, tests the Rust engine,
checks formatting and Clippy, and validates the runtime package.

`make test-install` repeats the package validation through a temporary install
directory and checks that a stale file is removed during replacement.

`make test-reference` runs the offline replay and then compares the analyzer
contract with the pinned Python reference implementation. Reference setup and
its isolated environment are documented in
[`reference/README.md`](../reference/README.md).

Use `make clean` to remove generated build, preview, archive, and local
packaging artifacts. It does not remove the pinned reference checkout.
