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

Rust commands use the repository's latest Stable toolchain through
`rust-toolchain.toml`. Refresh it before verification with:

```bash
rustup update stable
rustup show active-toolchain
```

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

`make test` runs the layered offline gate:

- `test-contract` checks the local prompt semantics, validates the declared
  compatibility matrix, and rejects legacy tool names or missing
  recursive/overview obligations. If the reference checkout is present it also
  checks reference prompt anchors; the offline gate does not require the
  optional submodule;
- the replay drives the real packaged CLI through analysis, prompt rendering,
  recursive tree saving, leaf-first ordering, overview-context generation,
  document writes, session close, and ZIP extraction, then compares the full
  normalized result with `tests/golden/mini-repo.json`;
- Rust integration contracts cover prompt variables and rendering, exact
  recursive tree coverage and quality diagnostics, update routing/stale scans,
  and CLI behavior from a non-repository working directory;
- formatting, tests, Clippy, and runtime-package validation complete the gate.

The replay does not call an LLM. Prompt hashes and fixed Markdown make changes
to the host-agent contract visible without making generated prose part of the
byte-for-byte compatibility requirement.

Run the prompt/static layer alone with:

```bash
make test-contract
```

`make test-install` repeats the package validation through a temporary install
directory and checks that a stale file is removed during replacement.

`make test-reference` runs the same offline gate and then invokes the real
pinned Python reference implementation. It compares a normalized analyzer
contract (component IDs, locations, languages, dependencies, and leaves) plus
deterministic workflow semantics: leaf-first processing order and the
component-free, target-marked overview context with child `docs_path` fields.
It also runs the static prompt contract against both prompt trees with
`--require-reference`. Missing reference sources or dependencies are a hard
failure for this opt-in gate, never a skip.
Reference setup and its isolated environment are documented in
[`reference/README.md`](../reference/README.md).

`.github/workflows/verification.yml` runs the offline/package gate on every
push and pull request, and runs the pinned reference differential in a separate
job with the reference submodule and virtual environment initialized explicitly.

Use `make clean` to remove generated build, preview, archive, and local
packaging artifacts. It does not remove the pinned reference checkout.
