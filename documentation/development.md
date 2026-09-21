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
- `tools/` contains development-time replay and package validation tools.
- `reference/CodeWiki/` is read-only reference material used to curate the
  architecture few-shot examples under `skill/references/few-shots/`.
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

- `test-contract` checks the local architecture prompt semantics, the curated
  few-shot catalog, and rejects legacy tool names or missing module/overview
  obligations;
- the replay drives the real packaged CLI through analysis, prompt rendering,
  recursive tree saving, leaf-first ordering, overview-context generation,
  document writes, session close, and ZIP extraction, then compares the full
  normalized result with `tests/golden/mini-repo.json`;
- Rust integration contracts cover prompt variables and rendering, semantic
  architecture-anchor selection and quality diagnostics, update routing/stale
  scans, and CLI behavior from a non-repository working directory. The CLI
  smoke layer also exercises cross-process session serialization, strict
  input-artifact roles, and same-content document retries;
- formatting, tests, Clippy, and runtime-package validation complete the gate.

The replay does not call an LLM. Prompt hashes, the generated workflow host
contract, fixed Markdown, and processing-order assertions make changes to the
host-agent architecture contract visible while the tree intentionally leaves
low-level analysis candidates outside the published page hierarchy.

Run the prompt/static layer alone with:

```bash
make test-contract
```

`make test-install` repeats the package validation through a temporary install
directory and checks that a stale file is removed during replacement.

The reference checkout is not a compatibility target. When changing prompts,
update the architecture few-shots only when a new reference page adds a
useful reading pattern or diagram discipline.

Use `make clean` to remove generated build, preview, archive, and local
packaging artifacts.
